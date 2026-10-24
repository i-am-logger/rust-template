//! `cargo xtask init [--ratchet]`: adopt the gates in an existing workspace.
//!
//! Writes `layers.toml` (every member as `adapter`, the role that is held
//! to the least, until you assign the real ones), `budgets.toml`,
//! `ignores.toml` and an empty sources manifest -- and never overwrites one
//! that exists.
//!
//! By default every budget is **0**, so each existing violation fails a gate
//! and is fixed, which is what adopting the pillars means. `--ratchet`
//! instead writes today's counts, freezing what exists so that it can only
//! shrink: a way to start, not a place to stay.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, ensure};

use crate::scan::{self, Kind};
use crate::workspace::{files_under, members};

const ROLES: &str = r#"
[roles.domain]
may_depend_on = []
external = ["zerocopy"]

[roles.core]
may_depend_on = ["domain"]
external = []

[roles.adapter]
may_depend_on = ["domain", "core", "adapter", "quarantine"]
external = ["*"]

[roles.bin]
may_depend_on = ["domain", "core", "adapter", "quarantine"]
external = ["*"]

[roles.quarantine]
may_depend_on = ["domain"]
external = ["*"]

[roles.tool]
may_depend_on = []
external = ["*"]
"#;

const SEAMS: &[&str] = &["get", "index", "of"];

fn write_new(path: &Path, text: &str) -> Result<()> {
    ensure!(
        !path.exists(),
        "{} exists: init never overwrites",
        path.display()
    );
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

pub(crate) fn run(args: &[String]) -> Result<ExitCode> {
    let ratchet = args.iter().any(|a| a == "--ratchet");
    let (root, members) = members()?;
    let seams: Vec<String> = SEAMS.iter().map(ToString::to_string).collect();

    let mut layers = String::from(
        "# Every crate's role. Assign each: domain, core, adapter, bin, tool or\n# quarantine (see the rust-architecture skill). Written by `cargo xtask init`.\n\n[crates]\n",
    );
    let mut budgets = format!(
        "# Written by `cargo xtask init{}`.\n\n[seams]\nnames = {:?}\n\n",
        if ratchet { " --ratchet" } else { "" },
        SEAMS
    );
    if let Ok(md) = fs::metadata(root.join("CLAUDE.md")) {
        writeln!(budgets, "[docs]\nclaude_md_bytes = {}\n", md.len())?;
    }
    for m in &members {
        writeln!(layers, "{:?} = \"adapter\"", m.name)?;
        writeln!(budgets, "[crates.{:?}]", m.name)?;
        if ratchet {
            let mut hits = Vec::new();
            for f in files_under(&m.dir().join("src"), &["rs"])? {
                hits.extend(
                    scan::scan(&fs::read_to_string(&f)?, &seams)
                        .with_context(|| format!("parsing {}", f.display()))?,
                );
            }
            let counts = scan::count(&hits);
            for k in Kind::ALL {
                writeln!(
                    budgets,
                    "{} = {}",
                    k.key(),
                    counts.get(&k).copied().unwrap_or(0)
                )?;
            }
        }
        budgets.push('\n');
    }
    layers.push_str(ROLES);
    write_new(&root.join("layers.toml"), &layers)?;
    write_new(&root.join("budgets.toml"), &budgets)?;
    write_new(
        &root.join("ignores.toml"),
        "# Every #[ignore]d test, with why ([[ignore]] file, test, reason).\n",
    )?;
    let sources = root.join("docs/research/sources");
    fs::create_dir_all(&sources)?;
    if !sources.join("MANIFEST.toml").exists() {
        fs::write(
            sources.join("MANIFEST.toml"),
            "# Pinned research sources (see the rust-research skill).\n",
        )?;
    }
    eprintln!(
        "wrote layers.toml, budgets.toml, ignores.toml for {} crates{}",
        members.len(),
        if ratchet {
            " (budgets at today's counts)"
        } else {
            " (every budget 0)"
        }
    );
    eprintln!(
        "next: assign roles in layers.toml, adopt [workspace.lints] and clippy.toml from the template, then `cargo xtask all`"
    );
    Ok(ExitCode::SUCCESS)
}
