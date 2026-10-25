//! `cargo xtask controls`: proof that every gate can fail.
//!
//! A gate that has never been seen failing is indistinguishable from one that
//! cannot. For each gate this plants one violation in a scratch copy of the
//! workspace, runs the gate there, and requires it to fail **for the planted
//! reason**: the gate's output must contain the expected finding, error code
//! or lint. A gate that fails for another reason -- a link error, a compile
//! error elsewhere -- is reported as WRONG, not counted as red.
//!
//! Plants are appends, new files and edits of stable markers (`#![no_std]`,
//! `[lints] workspace = true`), so they survive the project's code changing.
//! Every edit is resolved inside the copy, and refused if it is not: the
//! workspace itself is never touched.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail, ensure};

use crate::workspace::{Role, Workspace};

/// One edit inside the copy.
#[derive(Debug)]
enum Edit {
    Append(PathBuf, String),
    Write(PathBuf, String),
    Replace(PathBuf, String, String),
    /// Increment one integer in budgets.toml: `[crates.<crate>] <key>`.
    BumpBudget(String, String),
    /// Remove a crate from layers.toml's `[crates]`.
    Unrole(String),
}

/// One planted violation: what to change, what to run, and what the output
/// must say for the red to count.
#[derive(Debug)]
struct Case {
    name: &'static str,
    edits: Vec<Edit>,
    cmd: Vec<String>,
    reason: String,
}

/// The crates the cases plant into, found by role rather than by name.
#[derive(Debug)]
struct Targets {
    domain: (String, PathBuf),
    core: (String, PathBuf),
    bin: (String, PathBuf),
    /// A workspace dependency the core may not use.
    foreign: String,
}

fn cmd(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn app(dir: &Path, file: &str, text: &str) -> Edit {
    Edit::Append(dir.join(file), text.to_string())
}

fn budget_cases(t: &Targets) -> Vec<Case> {
    let (d, dd) = (&t.domain.0, &t.domain.1);
    let lib = "src/lib.rs";
    vec![
        Case {
            name: "primitive in a domain API",
            edits: vec![app(dd, lib, "\npub fn planted_raw(x: u32) -> u32 { x }\n")],
            cmd: cmd("cargo xtask budgets"),
            reason: "primitive_api is".into(),
        },
        Case {
            name: "a public constant of a primitive type",
            edits: vec![app(dd, lib, "\npub const PLANTED_LIMIT: u32 = 3;\n")],
            cmd: cmd("cargo xtask budgets"),
            reason: "pub const PLANTED_LIMIT: primitive".into(),
        },
        Case {
            name: "public newtype field",
            edits: vec![app(dd, lib, "\npub struct PlantedLeaky(pub u8);\n")],
            cmd: cmd("cargo xtask budgets"),
            reason: "pub_newtype_field is".into(),
        },
        Case {
            name: "bool parameter",
            edits: vec![app(dd, lib, "\npub fn planted_flag(_on: bool) {}\n")],
            cmd: cmd("cargo xtask budgets"),
            reason: "bool_param_or_field is".into(),
        },
        Case {
            name: "primitive through a trait impl",
            edits: vec![app(
                dd,
                lib,
                "\npub struct PlantedQ;\nimpl From<u32> for PlantedQ { fn from(_: u32) -> Self { Self } }\n",
            )],
            cmd: cmd("cargo xtask budgets"),
            reason: "primitive crosses the API through a trait".into(),
        },
        Case {
            name: "an extra cast (over budget)",
            edits: vec![app(dd, lib, "\nconst PLANTED: u64 = 1_u32 as u64;\n")],
            cmd: cmd("cargo xtask budgets"),
            reason: "as_cast is".into(),
        },
        Case {
            name: "a budget above its count (under)",
            edits: vec![Edit::BumpBudget(d.clone(), "as_cast".into())],
            cmd: cmd("cargo xtask budgets"),
            reason: "as_cast fell to".into(),
        },
    ]
}

fn arch_cases(t: &Targets) -> Vec<Case> {
    let (dd, (c, cd), (_, bd)) = (&t.domain.1, &t.core, &t.bin);
    vec![
        Case {
            name: "callback in a core API",
            edits: vec![app(
                cd,
                "src/lib.rs",
                "\n#[derive(Debug)]\npub struct PlantedSink;\nimpl PlantedSink { pub fn on(&mut self, _cb: impl Fn()) {} }\n",
            )],
            cmd: cmd("cargo xtask arch"),
            reason: "never called back".into(),
        },
        Case {
            name: "no chunk-invariance property",
            edits: vec![Edit::Replace(
                cd.join("tests"),
                "fn chunk_invariance_".into(),
                "fn renamed_".into(),
            )],
            cmd: cmd("cargo xtask arch"),
            reason: "chunk_invariance_".into(),
        },
        Case {
            name: "no_std removed",
            edits: vec![Edit::Replace(
                dd.join("src/lib.rs"),
                "#![no_std]".into(),
                String::new(),
            )],
            cmd: cmd("cargo xtask arch"),
            reason: "must be `#![no_std]`".into(),
        },
        Case {
            name: "a crate opts out of the workspace lints",
            edits: vec![Edit::Replace(
                bd.join("Cargo.toml"),
                "[lints]\nworkspace = true".into(),
                "[lints.rust]\nunused = \"deny\"".into(),
            )],
            cmd: cmd("cargo xtask arch"),
            reason: "[lints] workspace = true".into(),
        },
        Case {
            name: "a crate-local clippy.toml drops a ban",
            edits: vec![Edit::Replace(
                cd.join("clippy.toml"),
                "\"alloc::boxed::Box::leak\"".into(),
                "\"alloc::boxed::Box::into_raw\"".into(),
            )],
            cmd: cmd("cargo xtask arch"),
            reason: "does not ban".into(),
        },
        Case {
            name: "a crate with no role",
            edits: vec![Edit::Unrole(c.clone())],
            cmd: cmd("cargo xtask arch"),
            reason: "has no role".into(),
        },
        Case {
            name: "core depends on a foreign crate",
            edits: vec![Edit::Replace(
                cd.join("Cargo.toml"),
                "[dependencies]\n".into(),
                format!("[dependencies]\n{}.workspace = true\n", t.foreign),
            )],
            cmd: cmd("cargo xtask layers"),
            reason: "may not depend on the external crate".into(),
        },
    ]
}

fn snippet_cases() -> Vec<Case> {
    vec![Case {
        name: "a Rust block in the docs that does not compile",
        edits: vec![Edit::Write(
            "docs/planted-snippet.md".into(),
            "```rust\nlet n: u32 = \"not a number\";\n```\n".into(),
        )],
        cmd: cmd("cargo xtask snippets"),
        reason: "a Rust block does not compile or run".into(),
    }]
}

fn docs_cases(t: &Targets) -> Vec<Case> {
    let dd = &t.domain.1;
    let mark = concat!("[", "[source:");
    vec![
        Case {
            name: "compile_fail case with no twin",
            edits: vec![
                app(dd, "tests/ui/fail/planted.rs", "fn main() {}\n"),
                app(dd, "tests/ui/fail/planted.stderr", ""),
            ],
            cmd: cmd("cargo xtask docs"),
            reason: "no passing twin".into(),
        },
        Case {
            name: "a broken link",
            edits: vec![Edit::Write(
                "docs/planted.md".into(),
                "See [gone](no-such-file.md).\n".into(),
            )],
            cmd: cmd("cargo xtask docs"),
            reason: "links to `no-such-file.md`".into(),
        },
        Case {
            name: "a named path that does not exist",
            edits: vec![Edit::Write(
                "docs/planted.md".into(),
                "See `crates/no-such-crate/src/lib.rs`.\n".into(),
            )],
            cmd: cmd("cargo xtask docs"),
            reason: "names `crates/no-such-crate/src/lib.rs`".into(),
        },
        Case {
            name: "CLAUDE.md over its budget",
            edits: vec![Edit::Append(
                "CLAUDE.md".into(),
                "\nOne more rule.\n".into(),
            )],
            cmd: cmd("cargo xtask docs"),
            reason: "over its budget".into(),
        },
        Case {
            name: "an unknown citation",
            edits: vec![Edit::Write(
                "docs/planted.md".into(),
                format!("Per {mark} planted-key]] section 1.\n"),
            )],
            cmd: cmd("cargo xtask sources"),
            reason: "does not list".into(),
        },
        Case {
            name: "an unaccounted source file",
            edits: vec![Edit::Write(
                "docs/research/sources/planted.txt".into(),
                "x".into(),
            )],
            cmd: cmd("cargo xtask sources"),
            reason: "in no MANIFEST.toml entry".into(),
        },
        Case {
            name: "a tampered pinned source",
            edits: vec![
                Edit::Write("docs/research/sources/planted.txt".into(), "real".into()),
                Edit::Append(
                    "docs/research/sources/MANIFEST.toml".into(),
                    format!(
                        "\n[sources.planted]\nstatus = \"retrieved\"\nfile = \"planted.txt\"\nsha256 = \"{}\"\n",
                        "0".repeat(64)
                    ),
                ),
            ],
            cmd: cmd("cargo xtask sources"),
            reason: "not the pinned sha256".into(),
        },
    ]
}

fn test_cases(t: &Targets) -> Vec<Case> {
    let cd = &t.core.1;
    let file = "tests/planted_tests.rs";
    vec![
        Case {
            name: "a test that skips itself",
            edits: vec![app(
                cd,
                file,
                "#[test]\nfn planted() { if !cfg!(target_os = \"none\") { return; } }\n",
            )],
            cmd: cmd("cargo xtask tests"),
            reason: "returns early after probing".into(),
        },
        Case {
            name: "a test that says it skipped",
            edits: vec![app(
                cd,
                file,
                "#[test]\nfn planted() { eprintln!(\"skipped: no device\"); }\n",
            )],
            cmd: cmd("cargo xtask tests"),
            reason: "prints that it skipped".into(),
        },
        Case {
            name: "an inexact floor",
            edits: vec![app(
                cd,
                file,
                "#[test]\nfn planted() { let n = 5; assert!(n > 3); }\n",
            )],
            cmd: cmd("cargo xtask tests"),
            reason: "lower bound against a literal".into(),
        },
        Case {
            name: "a public fn no test calls",
            edits: vec![app(
                &t.domain.1,
                "src/lib.rs",
                "\npub fn planted_uncalled() {}\n",
            )],
            cmd: cmd("cargo xtask coverage"),
            reason: "no test calls it".into(),
        },
        Case {
            name: "an unlisted #[ignore]",
            edits: vec![app(cd, file, "#[test]\n#[ignore]\nfn planted() {}\n")],
            cmd: cmd("cargo xtask tests"),
            reason: "not listed in ignores.toml".into(),
        },
    ]
}

fn compile_cases(t: &Targets) -> Vec<Case> {
    let ((d, dd), (c, cd), (b, bd)) = (&t.domain, &t.core, &t.bin);
    let main = "src/main.rs";
    vec![
        Case {
            name: "unsafe in the bin",
            edits: vec![app(
                bd,
                main,
                "\n#[expect(dead_code, reason = \"planted\")]\nfn planted_unsafe() { unsafe {} }\n",
            )],
            cmd: cmd(&format!("cargo check -p {b}")),
            reason: "usage of an `unsafe` block".into(),
        },
        Case {
            name: "Box::leak in the bin",
            edits: vec![app(
                bd,
                main,
                "\n#[expect(dead_code, reason = \"planted\")]\nfn planted_leak() { let _ = Box::leak(Box::new(1_u8)); }\n",
            )],
            cmd: cmd(&format!("cargo clippy -p {b} -- -D warnings")),
            reason: "disallowed method".into(),
        },
        Case {
            name: "ManuallyDrop in a no_std domain (core:: path)",
            edits: vec![app(
                dd,
                "src/lib.rs",
                "\npub fn planted_hold() { let _ = core::mem::ManuallyDrop::new(()); }\n",
            )],
            cmd: cmd(&format!("cargo clippy -p {d} -- -D warnings")),
            reason: "disallowed method `core::mem::ManuallyDrop::new`".into(),
        },
        Case {
            name: "std used in the core",
            edits: vec![app(cd, "src/lib.rs", "\nextern crate std;\n")],
            cmd: cmd(&format!(
                "cargo build -p {c} --no-default-features --target x86_64-unknown-none"
            )),
            reason: "can't find crate for `std`".into(),
        },
        Case {
            name: "a feature that does not build alone",
            edits: vec![
                Edit::Replace(
                    dd.join("Cargo.toml"),
                    "[features]\n".into(),
                    "[features]\nplanted = []\n".into(),
                ),
                app(
                    dd,
                    "src/lib.rs",
                    "\n#[cfg(feature = \"planted\")]\npub fn planted_missing() -> PlantedMissing { todo!() }\n",
                ),
            ],
            cmd: cmd(&format!(
                "cargo hack check -p {d} --each-feature --no-dev-deps"
            )),
            reason: "cannot find type `PlantedMissing`".into(),
        },
        Case {
            name: "an illegal program that compiles",
            edits: vec![
                app(dd, "tests/ui/fail/planted_compiles.rs", "fn main() {}\n"),
                app(dd, "tests/ui/fail/planted_compiles.stderr", ""),
                app(dd, "tests/ui/pass/planted_compiles.rs", "fn main() {}\n"),
            ],
            cmd: cmd(&format!("cargo test -p {d} --test typesystem")),
            reason: "Expected test case to fail to compile".into(),
        },
    ]
}

fn targets(ws: &Workspace) -> Result<Targets> {
    let pick = |role: Role| -> Result<(String, PathBuf)> {
        let m = ws
            .members
            .iter()
            .find(|m| ws.role(&m.name) == Some(role))
            .with_context(|| format!("controls need a crate with role {}", role.name()))?;
        Ok((
            m.name.clone(),
            m.dir()
                .strip_prefix(&ws.root)
                .unwrap_or(m.dir())
                .to_path_buf(),
        ))
    };
    let core = pick(Role::Core)?;
    let rule = ws
        .layers
        .roles
        .get("core")
        .context("layers.toml has no [roles.core]")?;
    let root: toml::Table = toml::from_str(&fs::read_to_string(ws.root.join("Cargo.toml"))?)?;
    let deps = root
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(toml::Value::as_table);
    let foreign = deps
        .into_iter()
        .flatten()
        .map(|(k, _)| k.clone())
        .find(|k| !ws.is_member(k) && !rule.external.iter().any(|e| e == "*" || e == k))
        .context("controls need a [workspace.dependencies] entry the core may not use")?;
    Ok(Targets {
        domain: pick(Role::Domain)?,
        core,
        bin: pick(Role::Bin)?,
        foreign,
    })
}

pub(crate) fn run() -> Result<ExitCode> {
    let ws = Workspace::load()?;
    let t = targets(&ws)?;
    let base = ws.root.join("target/controls");
    let work = base.join("work");
    let target_dir = base.join("target");
    let cases: Vec<Case> = [
        budget_cases(&t),
        arch_cases(&t),
        docs_cases(&t),
        snippet_cases(),
        test_cases(&t),
        compile_cases(&t),
    ]
    .into_iter()
    .flatten()
    .collect();

    copy_workspace(&ws.root, &work)?;
    let baseline = run_in(&work, &target_dir, &cmd("cargo xtask all"))?;
    ensure!(
        baseline.0,
        "the unplanted copy is not green, so no red below would mean anything:\n{}",
        tail(&baseline.1)
    );
    eprintln!("ok    baseline: `cargo xtask all` passes on the unplanted copy");

    let mut bad = 0;
    for case in &cases {
        copy_workspace(&ws.root, &work)?;
        for e in &case.edits {
            apply(&work, e).with_context(|| format!("planting `{}`", case.name))?;
        }
        let (passed, out) = run_in(&work, &target_dir, &case.cmd)?;
        let verdict = match (passed, out.contains(&case.reason)) {
            (false, true) => "RED  ",
            (true, _) => {
                bad += 1;
                "MISS "
            }
            (false, false) => {
                bad += 1;
                "WRONG"
            }
        };
        eprintln!("{verdict} {}  ({})", case.name, case.cmd.join(" "));
        if verdict != "RED  " {
            eprintln!(
                "      expected the output to say: {}\n{}",
                case.reason,
                tail(&out)
            );
        }
    }
    eprintln!(
        "\n{} of {} gates failed for the planted reason",
        cases.len() - bad,
        cases.len()
    );
    Ok(if bad == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn tail(s: &str) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(12)..]
        .iter()
        .map(|l| format!("      | {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_in(work: &Path, target_dir: &Path, argv: &[String]) -> Result<(bool, String)> {
    let (prog, args) = argv.split_first().context("empty command")?;
    let prog = if prog == "cargo" {
        std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
    } else {
        prog.clone()
    };
    let out = Command::new(prog)
        .args(args)
        .current_dir(work)
        .env("CARGO_TARGET_DIR", target_dir)
        .output()
        .with_context(|| format!("running {}", argv.join(" ")))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out.status.success(), text))
}

/// Resolves a path inside the copy, refusing anything outside it.
fn inside(work: &Path, rel: &Path) -> Result<PathBuf> {
    ensure!(
        rel.is_relative()
            && !rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
        "refusing to plant outside the copy: {}",
        rel.display()
    );
    Ok(work.join(rel))
}

fn apply(work: &Path, e: &Edit) -> Result<()> {
    match e {
        Edit::Append(p, text) => {
            let p = inside(work, p)?;
            if let Some(dir) = p.parent() {
                fs::create_dir_all(dir)?;
            }
            let mut s = fs::read_to_string(&p).unwrap_or_default();
            s.push_str(text);
            fs::write(p, s)?;
        }
        Edit::Write(p, text) => {
            let p = inside(work, p)?;
            if let Some(dir) = p.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(p, text)?;
        }
        Edit::Replace(p, old, new) => replace(&inside(work, p)?, old, new)?,
        Edit::BumpBudget(krate, key) => {
            let p = inside(work, Path::new("budgets.toml"))?;
            let mut doc: toml::Table = toml::from_str(&fs::read_to_string(&p)?)?;
            let table = doc
                .get_mut("crates")
                .and_then(|c| c.get_mut(krate.as_str()))
                .and_then(toml::Value::as_table_mut)
                .with_context(|| format!("budgets.toml has no [crates.{krate}]"))?;
            let n = table
                .get(key.as_str())
                .and_then(toml::Value::as_integer)
                .unwrap_or(0);
            table.insert(key.clone(), toml::Value::Integer(n + 1));
            fs::write(p, toml::to_string(&doc)?)?;
        }
        Edit::Unrole(krate) => {
            let p = inside(work, Path::new("layers.toml"))?;
            let mut doc: toml::Table = toml::from_str(&fs::read_to_string(&p)?)?;
            doc.get_mut("crates")
                .and_then(toml::Value::as_table_mut)
                .context("layers.toml has no [crates]")?
                .remove(krate);
            fs::write(p, toml::to_string(&doc)?)?;
        }
    }
    Ok(())
}

/// Replaces `old` in a file, or in every `.rs` file under a directory; the
/// marker must be found at least once, or the plant would be silent.
fn replace(p: &Path, old: &str, new: &str) -> Result<()> {
    let files = if p.is_dir() {
        crate::workspace::files_under(p, &["rs"])?
    } else {
        vec![p.to_path_buf()]
    };
    let mut hits = 0;
    for f in files {
        let s = fs::read_to_string(&f)?;
        if s.contains(old) {
            hits += 1;
            fs::write(&f, s.replace(old, new))?;
        }
    }
    if hits == 0 {
        bail!(
            "marker `{old}` not found under {}: the plant would plant nothing",
            p.display()
        );
    }
    Ok(())
}

/// A fresh copy of the workspace sources, without build output or VCS state.
fn copy_workspace(root: &Path, work: &Path) -> Result<()> {
    if work.exists() {
        fs::remove_dir_all(work)?;
    }
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let src = root.join(&rel);
        fs::create_dir_all(work.join(&rel))?;
        for entry in fs::read_dir(&src)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(name.as_ref(), "target" | ".git" | ".direnv") || name.starts_with(".devenv")
            {
                continue;
            }
            let child = rel.join(name.as_ref());
            if entry.file_type()?.is_dir() {
                stack.push(child);
            } else if entry.file_type()?.is_file() {
                fs::copy(root.join(&child), work.join(&child))?;
            }
        }
    }
    Ok(())
}
