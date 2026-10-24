//! `cargo xtask arch`: what each role must be.
//!
//! * every member has a role, and every role-holder is a member;
//! * every crate but a quarantine crate inherits the workspace lints, which
//!   `forbid` unsafe code (pillar 6);
//! * domain and core are `#![no_std]`, keep `alloc`/`std` out of `default`,
//!   and carry a crate-local `clippy.toml` with every workspace ban plus the
//!   sans-IO ones (pillars 2, 4, 5);
//! * core takes no callback in its public API, and has a `chunk_invariance_*`
//!   test (pillar 2).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::report::Report;
use crate::scan::{self, Kind};
use crate::workspace::{Role, Workspace, files_under};

const GATE: &str = "arch";

/// Paths every domain and core `clippy.toml` must ban, beyond the workspace's.
const SANS_IO_BANS: &[&str] = &["std::time::Instant", "std::time::SystemTime"];

pub(crate) fn run(ws: &Workspace, r: &mut Report) -> Result<()> {
    control()?;
    roles(ws, r);
    for m in &ws.members {
        let Some(role) = ws.role(&m.name) else {
            continue;
        };
        lints(ws, r, &m.name, &m.manifest_path, role)?;
        if role.is_pure() {
            pure(ws, r, &m.name, m.dir(), &m.features)?;
        }
        if role == Role::Core {
            core(ws, r, &m.name, m.dir())?;
        }
    }
    r.checked(
        GATE,
        format!("{} crates against their roles", ws.members.len()),
    );
    Ok(())
}

/// The positive control: the detectors this gate relies on must fire on
/// inputs known to be wrong.
fn control() -> Result<()> {
    scan::control()?;
    anyhow::ensure!(
        !has_no_std("fn main() {}"),
        "positive control failed: no_std detector fires on a file without it"
    );
    anyhow::ensure!(
        has_no_std("#![no_std]\n"),
        "positive control failed: no_std detector misses #![no_std]"
    );
    Ok(())
}

fn roles(ws: &Workspace, r: &mut Report) {
    for m in &ws.members {
        if ws.role(&m.name).is_none() {
            r.fail(
                GATE,
                &m.name,
                "has no role: add it to [crates] in layers.toml",
            );
        }
    }
    for name in ws.layers.crates.keys() {
        if !ws.is_member(name) {
            r.fail(
                GATE,
                "layers.toml",
                format!("names `{name}`, which is not a workspace member"),
            );
        }
    }
}

fn lints(ws: &Workspace, r: &mut Report, name: &str, manifest: &Path, role: Role) -> Result<()> {
    let text =
        fs::read_to_string(manifest).with_context(|| format!("reading {}", manifest.display()))?;
    let doc: toml::Table = toml::from_str(&text)?;
    let inherits = doc
        .get("lints")
        .and_then(|l| l.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    if !inherits && role != Role::Quarantine {
        r.fail(GATE, ws.rel(manifest).to_string(), format!(
            "{name} must set `[lints] workspace = true`: only a quarantine crate may differ from the workspace lints (pillar 6)"
        ));
    }
    Ok(())
}

fn has_no_std(src: &str) -> bool {
    syn::parse_file(src).is_ok_and(|f| f.attrs.iter().any(|a| a.path().is_ident("no_std")))
}

fn pure(
    ws: &Workspace,
    r: &mut Report,
    name: &str,
    dir: &Path,
    features: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<()> {
    let lib = dir.join("src/lib.rs");
    let src = fs::read_to_string(&lib).with_context(|| format!("reading {}", lib.display()))?;
    if !has_no_std(&src) {
        r.fail(
            GATE,
            ws.rel(&lib).to_string(),
            "must be `#![no_std]` (pillar 5)",
        );
    }
    for f in features.get("default").into_iter().flatten() {
        if f.contains("alloc") || f.contains("std") {
            r.fail(
                GATE,
                name,
                format!("`default` turns on `{f}`: alloc/std must be opt-in (pillar 5)"),
            );
        }
    }
    clippy_bans(ws, r, dir)
}

/// The crate-local clippy.toml replaces the workspace one, so it must keep
/// every workspace ban and add the sans-IO ones.
fn clippy_bans(ws: &Workspace, r: &mut Report, dir: &Path) -> Result<()> {
    let local = dir.join("clippy.toml");
    let Ok(text) = fs::read_to_string(&local) else {
        r.fail(
            GATE,
            ws.rel(&local).to_string(),
            "missing: domain and core need their own clippy.toml (pillar 2)",
        );
        return Ok(());
    };
    let root = fs::read_to_string(ws.root.join("clippy.toml")).context("reading clippy.toml")?;
    let want = banned_paths(&root)?
        .into_iter()
        .chain(SANS_IO_BANS.iter().map(|s| (*s).to_string()));
    let have = banned_paths(&text)?;
    for path in want {
        if !have.contains(&path) {
            r.fail(
                GATE,
                ws.rel(&local).to_string(),
                format!("does not ban `{path}`"),
            );
        }
    }
    Ok(())
}

fn banned_paths(clippy_toml: &str) -> Result<Vec<String>> {
    let doc: toml::Table = toml::from_str(clippy_toml).context("parsing clippy.toml")?;
    let mut out = Vec::new();
    for key in ["disallowed-methods", "disallowed-types"] {
        for entry in doc
            .get(key)
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(p) = entry.get("path").and_then(toml::Value::as_str) {
                out.push(p.to_string());
            }
        }
    }
    Ok(out)
}

fn core(ws: &Workspace, r: &mut Report, name: &str, dir: &Path) -> Result<()> {
    for file in files_under(&dir.join("src"), &["rs"])? {
        let src = fs::read_to_string(&file)?;
        let hits = scan::scan(&src, &[]).with_context(|| format!("parsing {}", file.display()))?;
        for h in hits.iter().filter(|h| h.kind == Kind::CallbackParam) {
            r.fail(
                GATE,
                format!("{}:{}", ws.rel(&file), h.line),
                format!(
                    "{}: a core's effects are queued and polled, never called back (pillar 2)",
                    h.what
                ),
            );
        }
    }
    let has_property = files_under(&dir.join("tests"), &["rs"])
        .unwrap_or_default()
        .iter()
        .any(|f| fs::read_to_string(f).is_ok_and(|s| s.contains("fn chunk_invariance_")));
    if !has_property {
        r.fail(GATE, name, "needs a `chunk_invariance_*` test in tests/: effects must not depend on how input was split (pillar 2)");
    }
    Ok(())
}
