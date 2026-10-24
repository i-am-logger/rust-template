//! `cargo xtask budgets`: counted constructs against `budgets.toml`.
//!
//! A budget is the count, exactly. Over it fails, because something was
//! added; under it fails too, because a budget above the count measures
//! nothing -- lower it in the same commit, and raise one only in a commit of
//! its own that says why. Domain and core are held on every kind; every
//! other crate on casts, suppressions and unsafe code.

use std::collections::BTreeMap;
use std::fs;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::report::Report;
use crate::scan::{self, Counts, Hit, Kind};
use crate::workspace::{Member, Workspace, files_under, read_toml};

const GATE: &str = "budgets";

#[derive(Debug, Deserialize)]
struct Budgets {
    seams: Seams,
    crates: BTreeMap<String, BTreeMap<String, usize>>,
}

#[derive(Debug, Deserialize)]
struct Seams {
    names: Vec<String>,
}

/// The kinds a crate is held to, by role.
fn gated(ws: &Workspace, m: &Member) -> Vec<Kind> {
    if ws
        .role(&m.name)
        .is_some_and(crate::workspace::Role::is_pure)
    {
        Kind::ALL.to_vec()
    } else {
        vec![Kind::AsCast, Kind::ExpectAttr, Kind::UnsafeCode]
    }
}

fn load(ws: &Workspace) -> Result<Budgets> {
    read_toml(&ws.root.join("budgets.toml"))
}

fn scan_member(m: &Member, seams: &[String]) -> Result<Vec<(String, Hit)>> {
    let mut out = Vec::new();
    for file in files_under(&m.dir().join("src"), &["rs"])? {
        let src = fs::read_to_string(&file)?;
        let hits =
            scan::scan(&src, seams).with_context(|| format!("parsing {}", file.display()))?;
        out.extend(hits.into_iter().map(|h| (file.display().to_string(), h)));
    }
    Ok(out)
}

pub(crate) fn run(ws: &Workspace, r: &mut Report) -> Result<()> {
    scan::control()?;
    let budgets = load(ws)?;
    for m in &ws.members {
        let hits = scan_member(m, &budgets.seams.names)?;
        let counts: Counts = scan::count(&hits.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>());
        let limits = budgets.crates.get(&m.name);
        for kind in gated(ws, m) {
            let got = counts.get(&kind).copied().unwrap_or(0);
            let budget = limits.and_then(|l| l.get(kind.key())).copied().unwrap_or(0);
            if got > budget {
                r.fail(
                    GATE,
                    &m.name,
                    format!("{} is {got}, over its budget of {budget}:", kind.key()),
                );
                for (file, h) in hits.iter().filter(|(_, h)| h.kind == kind) {
                    r.fail(
                        GATE,
                        format!("  {}:{}", ws.rel(std::path::Path::new(file)), h.line),
                        h.what.clone(),
                    );
                }
            } else if got < budget {
                r.fail(GATE, &m.name, format!(
                    "{} fell to {got}: lower its budget from {budget} to {got} in budgets.toml (a budget above the count measures nothing)",
                    kind.key()
                ));
            }
        }
    }
    r.checked(GATE, format!("{} crates", ws.members.len()));
    Ok(())
}

/// `--print`: the current counts, ready to paste into budgets.toml.
pub(crate) fn print() -> Result<()> {
    let ws = Workspace::load()?;
    let budgets = load(&ws)?;
    for m in &ws.members {
        let hits = scan_member(m, &budgets.seams.names)?;
        let counts = scan::count(&hits.into_iter().map(|(_, h)| h).collect::<Vec<_>>());
        println!("[crates.{}]", m.name);
        for kind in gated(&ws, m) {
            println!(
                "{} = {}",
                kind.key(),
                counts.get(&kind).copied().unwrap_or(0)
            );
        }
        println!();
    }
    Ok(())
}
