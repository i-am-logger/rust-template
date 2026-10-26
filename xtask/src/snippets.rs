//! `cargo xtask snippets`: every Rust code block in the project's Markdown
//! compiles and runs.
//!
//! A document is a claim about the code, and a code block is the most direct
//! claim of all. Each Markdown file outside the skipped directories (and the
//! pinned third-party sources) that holds a ```` ```rust ```` block is run with
//! `rustdoc --test`, linked against the domain and core crates: a block runs
//! as a doctest, so `ignore`, `no_run`, `compile_fail` and `should_panic` mean
//! what they mean in rustdoc. A block that names a crate the project does not
//! have must say `ignore` and why. Blocks are compiled with `--cfg test`, so a
//! `#[cfg(test)]` module in a document is compiled rather than silently empty.
//!
//! `snippets.toml` maps the crate names the documents use to this project's
//! crates (docs written against the template's `my_project_domain` read the
//! same in every project made from it).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::report::Report;
use crate::workspace::{Workspace, read_toml};

const GATE: &str = "snippets";

#[derive(Debug, Default, Deserialize)]
struct Config {
    /// Name used in documents -> workspace package name.
    #[serde(default)]
    r#extern: BTreeMap<String, String>,
}

/// Every library the domain and core crates' tests build -- the crates
/// themselves, their dependencies and dev-dependencies (zerocopy, proptest,
/// dhat, ...) -- by crate name, and the directory their own dependencies
/// resolve from. A snippet may use what a test there may use.
fn build_libs(ws: &Workspace, packages: &[String]) -> Result<(BTreeMap<String, PathBuf>, PathBuf)> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.args(["test", "--no-run", "--quiet", "--message-format=json"])
        .current_dir(&ws.root);
    for p in packages {
        cmd.args(["-p", p]);
    }
    let out = cmd
        .output()
        .context("building the crates the snippets link against")?;
    ensure!(
        out.status.success(),
        "cargo test --no-run failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut libs = BTreeMap::new();
    let mut deps = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let is_lib = msg["target"]["kind"]
            .as_array()
            .is_some_and(|k| k.iter().any(|v| v == "lib" || v == "rlib"));
        if msg["reason"] != "compiler-artifact" || !is_lib {
            continue;
        }
        let crate_name = msg["target"]["name"]
            .as_str()
            .unwrap_or("")
            .replace('-', "_");
        for f in msg["filenames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f.as_str())
            .filter(|f| f.ends_with(".rlib"))
        {
            let path = PathBuf::from(f);
            deps = path.parent().map(Path::to_path_buf);
            libs.insert(crate_name.clone(), path);
        }
    }
    let deps = deps.context("the build produced no rlib for the snippets to link against")?;
    Ok((libs, deps))
}

/// Markdown files that hold a Rust block.
fn documents(ws: &Workspace) -> Result<Vec<PathBuf>> {
    let pinned = ws.root.join("docs/research/sources");
    Ok(ws
        .files(&["md"])?
        .into_iter()
        .filter(|d| !d.starts_with(&pinned))
        .filter(|d| fs::read_to_string(d).is_ok_and(|s| s.contains("```rust")))
        .collect())
}

/// Runs `rustdoc --test` on one document; `None` if every block passed.
fn test(
    doc: &Path,
    externs: &[(String, PathBuf)],
    deps: &Path,
    scratch: &Path,
) -> Result<Option<String>> {
    let mut cmd = Command::new("rustdoc");
    cmd.args(["--test", "--edition", "2024", "--cfg", "test", "-L"])
        .arg(format!("dependency={}", deps.display()))
        .arg(doc)
        .env("RUSTDOC_TEMP", scratch);
    for (name, rlib) in externs {
        cmd.arg("--extern")
            .arg(format!("{name}={}", rlib.display()));
    }
    let out = cmd.output().context("running rustdoc --test")?;
    if out.status.success() {
        return Ok(None);
    }
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let log = scratch.join(format!(
        "{}.log",
        doc.to_string_lossy().replace(['/', '\\'], "_")
    ));
    fs::write(&log, &text)?;
    // rustdoc names a failing block `<file> - <heading> (line N)`.
    let lines: Vec<String> = text
        .lines()
        .filter(|l| l.ends_with("FAILED") && l.contains("(line "))
        .filter_map(|l| {
            l.rsplit_once("(line ")
                .map(|(_, n)| format!("line {}", n.trim_end_matches(") ... FAILED")))
        })
        .collect();
    Ok(Some(format!(
        "failing block(s) at {} -- rustdoc's output: {}",
        if lines.is_empty() {
            "?".into()
        } else {
            lines.join(", ")
        },
        log.display()
    )))
}

pub(crate) fn run(ws: &Workspace, r: &mut Report) -> Result<()> {
    let config: Config = if ws.root.join("snippets.toml").exists() {
        read_toml(&ws.root.join("snippets.toml"))?
    } else {
        Config::default()
    };
    let pure: Vec<String> = ws
        .members
        .iter()
        .filter(|m| {
            ws.role(&m.name)
                .is_some_and(crate::workspace::Role::is_pure)
        })
        .map(|m| m.name.clone())
        .collect();
    let (libs, deps) = build_libs(ws, &pure)?;
    let mut externs: Vec<(String, PathBuf)> =
        libs.iter().map(|(n, l)| (n.clone(), l.clone())).collect();
    for (alias, pkg) in &config.r#extern {
        let rlib = libs.get(&pkg.replace('-', "_")).with_context(|| {
            format!("snippets.toml maps `{alias}` to `{pkg}`, which the build did not produce")
        })?;
        externs.push((alias.clone(), rlib.clone()));
    }
    let scratch = ws.root.join("target/snippets");
    fs::create_dir_all(&scratch)?;
    control(&externs, &deps, &scratch)?;
    let docs = documents(ws)?;
    for doc in &docs {
        if let Some(failure) = test(doc, &externs, &deps, &scratch)? {
            r.fail(GATE, ws.rel(doc).to_string(), format!("a Rust block does not compile or run (mark it `ignore` and say why if it shows code that cannot exist here): {failure}"));
        }
    }
    r.checked(GATE, format!("{} documents with Rust blocks", docs.len()));
    Ok(())
}

/// The positive control: a broken block must fail, and a sound one pass.
fn control(externs: &[(String, PathBuf)], deps: &Path, scratch: &Path) -> Result<()> {
    let good = scratch.join("control-good.md");
    let bad = scratch.join("control-bad.md");
    fs::write(
        &good,
        "```rust\nlet n: u32 = 2;\nassert_eq!(n + 1, 3);\n```\n",
    )?;
    fs::write(&bad, "```rust\nlet n: u32 = \"not a number\";\n```\n")?;
    ensure!(
        test(&good, externs, deps, scratch)?.is_none(),
        "positive control failed: a sound block did not pass rustdoc --test"
    );
    ensure!(
        test(&bad, externs, deps, scratch)?.is_some(),
        "positive control failed: a broken block passed rustdoc --test"
    );
    Ok(())
}
