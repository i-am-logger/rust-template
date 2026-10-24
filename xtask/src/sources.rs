//! `cargo xtask sources`: the research sources manifest, and every citation.
//!
//! `docs/research/sources/MANIFEST.toml` lists each source a note or a
//! comment cites, by key, with its sha256:
//!
//! * `retrieved`: committed under `docs/research/sources/`, re-hashed here;
//! * `external`: read and hashed, but not committed (licence or size);
//! * `gated`: never obtained, so it may not be cited at all.
//!
//! Every file in the sources directory must be a retrieved entry's, and every
//! `[[source: <key>]]` in a `.md` or `.rs` file must name a citable entry.
//! A key is lowercase letters, digits, `.`, `_` and `-`; anything else after
//! the marker -- an example in angle brackets, this scanner's own source -- is
//! not a citation.

use std::collections::BTreeMap;
use std::fs;

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::report::Report;
use crate::workspace::{Workspace, read_toml};

const GATE: &str = "sources";
const DIR: &str = "docs/research/sources";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Retrieved,
    External,
    Gated,
}

#[derive(Debug, Deserialize)]
struct Source {
    status: Status,
    sha256: Option<String>,
    file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    sources: BTreeMap<String, Source>,
}

/// The citation marker, built so this file does not contain it literally.
const MARK: &str = concat!("[", "[source:");

fn is_key(k: &str) -> bool {
    !k.is_empty()
        && k.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// The keys cited in a text.
fn citations(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(MARK) {
        rest = &rest[i + MARK.len()..];
        let Some(end) = rest.find("]]") else { break };
        let key = rest[..end].trim();
        if is_key(key) {
            out.push(key.to_string());
        }
    }
    out
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn run(ws: &Workspace, r: &mut Report) -> Result<()> {
    control()?;
    let dir = ws.root.join(DIR);
    let manifest: Manifest = read_toml(&dir.join("MANIFEST.toml"))?;
    entries(ws, r, &manifest)?;
    accounted(ws, r, &manifest)?;
    let mut cited = 0;
    for file in ws.files(&["md", "rs"])? {
        let text = fs::read_to_string(&file)?;
        for key in citations(&text) {
            cited += 1;
            match manifest.sources.get(&key) {
                None => r.fail(
                    GATE,
                    ws.rel(&file).to_string(),
                    format!("cites `{key}`, which MANIFEST.toml does not list"),
                ),
                Some(s) if s.status == Status::Gated => r.fail(
                    GATE,
                    ws.rel(&file).to_string(),
                    format!("cites `{key}`, which is gated: never obtained, so not citable"),
                ),
                Some(_) => {}
            }
        }
    }
    r.checked(
        GATE,
        format!("{} sources, {cited} citations", manifest.sources.len()),
    );
    Ok(())
}

fn entries(ws: &Workspace, r: &mut Report, m: &Manifest) -> Result<()> {
    for (key, s) in &m.sources {
        let place = format!("MANIFEST.toml [sources.{key}]");
        if s.status != Status::Gated && !s.sha256.as_deref().is_some_and(is_sha256) {
            r.fail(GATE, &place, "needs a sha256 of 64 lowercase hex digits");
        }
        if s.status != Status::Retrieved {
            continue;
        }
        let Some(file) = &s.file else {
            r.fail(GATE, &place, "is retrieved but names no file");
            continue;
        };
        let path = ws.root.join(DIR).join(file);
        let Ok(bytes) = fs::read(&path) else {
            r.fail(
                GATE,
                &place,
                format!("names `{file}`, which is not in {DIR}"),
            );
            continue;
        };
        let got = format!("{:x}", Sha256::digest(&bytes));
        if s.sha256.as_deref() != Some(got.as_str()) {
            r.fail(
                GATE,
                &place,
                format!("`{file}` hashes to {got}, not the pinned sha256"),
            );
        }
    }
    Ok(())
}

fn accounted(ws: &Workspace, r: &mut Report, m: &Manifest) -> Result<()> {
    let dir = ws.root.join(DIR);
    let listed: Vec<&str> = m
        .sources
        .values()
        .filter_map(|s| s.file.as_deref())
        .collect();
    for f in any_file(&dir).context("listing sources")? {
        let name = f
            .strip_prefix(&dir)
            .unwrap_or(&f)
            .to_string_lossy()
            .to_string();
        if name != "MANIFEST.toml" && name != "README.md" && !listed.contains(&name.as_str()) {
            r.fail(
                GATE,
                ws.rel(&f).to_string(),
                "is in the sources directory but in no MANIFEST.toml entry",
            );
        }
    }
    Ok(())
}

/// Every file under `dir`, whatever its extension.
fn any_file(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// The positive control: an unknown citation and a bad hash must be seen.
fn control() -> Result<()> {
    let fixture = format!("x {MARK} nope]] y {MARK} <key>]]");
    anyhow::ensure!(
        citations(&fixture) == ["nope"],
        "positive control failed: citation scanner"
    );
    anyhow::ensure!(
        !is_sha256("abc"),
        "positive control failed: sha256 check accepts a short digest"
    );
    anyhow::ensure!(
        is_sha256(&format!("{:x}", Sha256::digest(b""))),
        "positive control failed: sha256 check rejects a real digest"
    );
    Ok(())
}
