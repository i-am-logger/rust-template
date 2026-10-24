//! `cargo xtask docs`: documents are claims about the tree, so they are
//! checked against it.
//!
//! * every relative Markdown link resolves;
//! * every repository path named in backticks exists;
//! * every `compile_fail` case under `tests/ui/fail` has its passing twin
//!   under `tests/ui/pass` and its pinned `.stderr`, and every twin its case;
//! * CLAUDE.md is exactly at its byte budget: it is loaded into every agent
//!   session whole, so it may only shrink, and a raise is its own commit.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::report::Report;
use crate::workspace::{Workspace, files_under, read_toml};

const GATE: &str = "docs";

/// Top-level names a backticked path must start with to be checked as one.
const PATH_ROOTS: &[&str] = &[
    "crates/", "xtask/", "docs/", "nix/", ".github/", ".cargo/", ".claude/", "tests/",
];
const ROOT_FILES: &[&str] = &[
    "Cargo.toml",
    "layers.toml",
    "budgets.toml",
    "clippy.toml",
    "devenv.nix",
    "CLAUDE.md",
    "README.md",
    "CONTRIBUTING.md",
    "MEASUREMENTS.md",
    "release-plz.toml",
];

#[derive(Debug, Deserialize)]
struct DocBudgets {
    docs: Docs,
}

#[derive(Debug, Deserialize)]
struct Docs {
    claude_md_bytes: usize,
}

/// Relative link targets in Markdown text: `](target)` that is not a URL or
/// an in-page anchor. Returns each target with its anchor stripped.
fn link_targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        rest = &rest[i + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = rest[..end].split_whitespace().next().unwrap_or("");
        let is_external =
            target.contains("://") || target.starts_with("mailto:") || target.starts_with('#');
        let path = target.split('#').next().unwrap_or("");
        if !is_external && !path.is_empty() {
            out.push(path.to_string());
        }
    }
    out
}

/// Repository paths named in backticks.
fn named_paths(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.split(':').next().unwrap_or(s).trim_end_matches('/'))
        .filter(|s| !s.contains(['<', '*', '{', ' ', '$']))
        .filter(|s| {
            PATH_ROOTS
                .iter()
                .any(|r| s.starts_with(r.trim_end_matches('/')) && s.contains('/'))
                || ROOT_FILES.contains(s)
        })
        .map(str::to_string)
        .collect()
}

pub(crate) fn run(ws: &Workspace, r: &mut Report) -> Result<()> {
    control()?;
    let docs = ws.files(&["md"])?;
    // Pinned third-party sources are quoted, not ours: their links point
    // into their own repositories.
    let pinned = ws.root.join("docs/research/sources");
    for doc in docs.iter().filter(|d| !d.starts_with(&pinned)) {
        let text = fs::read_to_string(doc)?;
        let dir = doc.parent().unwrap_or(&ws.root);
        for t in link_targets(&text) {
            if !dir.join(&t).exists() {
                r.fail(
                    GATE,
                    ws.rel(doc).to_string(),
                    format!("links to `{t}`, which does not exist"),
                );
            }
        }
        for p in named_paths(&text) {
            if !ws.root.join(&p).exists() {
                r.fail(
                    GATE,
                    ws.rel(doc).to_string(),
                    format!("names `{p}`, which does not exist"),
                );
            }
        }
    }
    let cases = twins(ws, r)?;
    claude_md(ws, r)?;
    r.checked(
        GATE,
        format!("{} documents, {cases} compile_fail cases", docs.len()),
    );
    Ok(())
}

fn twins(ws: &Workspace, r: &mut Report) -> Result<usize> {
    let mut cases = 0;
    for m in &ws.members {
        let ui = m.dir().join("tests/ui");
        let fail: Vec<PathBuf> = files_under(&ui.join("fail"), &["rs"]).unwrap_or_default();
        let pass: Vec<PathBuf> = files_under(&ui.join("pass"), &["rs"]).unwrap_or_default();
        for f in &fail {
            cases += 1;
            let twin = ui.join("pass").join(f.file_name().unwrap_or_default());
            if !twin.exists() {
                r.fail(GATE, ws.rel(f).to_string(), "has no passing twin in tests/ui/pass: it could be failing for an unrelated reason");
            }
            if !f.with_extension("stderr").exists() {
                r.fail(
                    GATE,
                    ws.rel(f).to_string(),
                    "has no pinned .stderr: the error it must fail with is not stated",
                );
            }
        }
        for p in &pass {
            if !ui
                .join("fail")
                .join(p.file_name().unwrap_or_default())
                .exists()
            {
                r.fail(
                    GATE,
                    ws.rel(p).to_string(),
                    "is a twin with no compile_fail case",
                );
            }
        }
    }
    Ok(cases)
}

fn claude_md(ws: &Workspace, r: &mut Report) -> Result<()> {
    let path = ws.root.join("CLAUDE.md");
    let Ok(text) = fs::read(&path) else {
        return Ok(());
    };
    let budget = read_toml::<DocBudgets>(&ws.root.join("budgets.toml"))
        .context("budgets.toml needs [docs] claude_md_bytes")?
        .docs
        .claude_md_bytes;
    let got = text.len();
    if got > budget {
        r.fail(GATE, "CLAUDE.md", format!(
            "is {got} bytes, over its budget of {budget}: it is loaded into every session whole -- move records to docs/, or raise the budget in a commit of its own that says why"
        ));
    } else if got < budget {
        r.fail(
            GATE,
            "CLAUDE.md",
            format!("shrank to {got} bytes: lower [docs] claude_md_bytes from {budget} to {got}"),
        );
    }
    Ok(())
}

/// The positive control: a broken link and a missing named path must be seen.
fn control() -> Result<()> {
    let text = "see [x](no/such/file.md) and `crates/no-such-crate/src/lib.rs`.";
    anyhow::ensure!(
        link_targets(text) == ["no/such/file.md"],
        "positive control failed: link scanner"
    );
    anyhow::ensure!(
        named_paths(text) == ["crates/no-such-crate/src/lib.rs"],
        "positive control failed: path scanner"
    );
    anyhow::ensure!(
        !Path::new("crates/no-such-crate/src/lib.rs").exists(),
        "positive control fixture exists"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_skip_urls_and_anchors() {
        assert_eq!(
            link_targets("[a](https://x.y) [b](#top) [c](d.md#sec)"),
            ["d.md"]
        );
    }

    #[test]
    fn named_paths_skip_placeholders_and_code() {
        assert_eq!(
            named_paths("`crates/<name>/src` `cargo xtask` `layers.toml` `docs/a.md:12`"),
            ["layers.toml", "docs/a.md"]
        );
    }
}
