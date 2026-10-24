//! The gates (role: `tool`). `cargo xtask <gate>`; see `cargo xtask help`.
//!
//! Every gate runs a **positive control** before it trusts a pass: it first
//! checks a fixture it knows is wrong and fails if it does not find the fault.
//! A scanner that silently matches nothing looks exactly like a clean tree,
//! and the control is what tells the two apart.

use std::process::ExitCode;

mod arch;
mod budgets;
mod controls;
mod coverage;
mod docs;
mod init;
mod layers;
mod measure;
mod report;
mod scan;
mod sources;
mod tests_gate;
mod workspace;

use report::Report;

const HELP: &str = "\
cargo xtask <gate>

  arch      pillars 2, 5, 6: roles, no_std, sans-IO, workspace lints
  layers    pillar 3: every dependency edge against layers.toml
  budgets   pillars 1, 4, 6: counted constructs against budgets.toml
            (--print shows the current counts)
  docs      links, named paths, compile_fail twins, the CLAUDE.md budget
  sources   the research sources manifest and every source citation
  tests     tests that can fail: no skips, exact floors, listed ignores
  coverage  every public fn of domain and core is called by some test
  all       every gate above, in that order
  controls  plant one violation per gate in a scratch copy; each gate must
            fail, and for the planted reason (slow: it rebuilds per case)
  bench     --arm name=<binary> ... [--aa] [--rounds N] [--floor-ms F]
            [--stdin FILE] [--cold DIR] [-- args]: timed launches, A/A, verdicts
  arms      <rev> <rev> [--bin NAME]: build both at one path, compare code
  init      [--ratchet]: adopt the gates in an existing workspace; budgets
            start at 0 (every violation fails) or, with --ratchet, at today's counts
  crates    --role domain,core: the `-p <name>` arguments for crates of
            those roles, so tasks never hard-code a crate list
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let gate = args.first().map_or("help", String::as_str);
    let print = args.iter().any(|a| a == "--print");
    let result = match gate {
        "arch" => run(&[arch::run]),
        "layers" => run(&[layers::run]),
        "budgets" if print => budgets::print().map(|()| ExitCode::SUCCESS),
        "budgets" => run(&[budgets::run]),
        "docs" => run(&[docs::run]),
        "sources" => run(&[sources::run]),
        "tests" => run(&[tests_gate::run]),
        "coverage" => run(&[coverage::run]),
        "all" => run(&[
            arch::run,
            layers::run,
            budgets::run,
            docs::run,
            sources::run,
            tests_gate::run,
            coverage::run,
        ]),
        "crates" => crates(&args),
        "controls" => controls::run(),
        "init" => init::run(&args),
        "bench" => project(|root, name| {
            measure::bench(
                &args,
                root,
                &format!("{}_STARTUP", name.to_uppercase().replace('-', "_")),
            )
        }),
        "arms" => project(|root, name| measure::arms(&args, root, name)),
        _ => {
            eprint!("{HELP}");
            return if gate == "help" {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("xtask {gate}: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Runs `f` with the workspace root and the project name from
/// `[workspace.metadata.project]`.
fn project(
    f: impl FnOnce(&std::path::Path, &str) -> anyhow::Result<ExitCode>,
) -> anyhow::Result<ExitCode> {
    let ws = workspace::Workspace::load()?;
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(ws.root.join("Cargo.toml"))?)?;
    let name = manifest
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("project"))
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Cargo.toml needs [workspace.metadata.project] name"))?
        .to_string();
    f(&ws.root, &name)
}

/// Prints `-p <name>` for every crate whose role is in `--role a,b`.
fn crates(args: &[String]) -> anyhow::Result<ExitCode> {
    let roles: Vec<&str> = args
        .iter()
        .skip_while(|a| *a != "--role")
        .nth(1)
        .map(|r| r.split(',').collect())
        .unwrap_or_default();
    anyhow::ensure!(
        !roles.is_empty(),
        "usage: cargo xtask crates --role domain,core"
    );
    let ws = workspace::Workspace::load()?;
    let picked: Vec<String> = ws
        .members
        .iter()
        .filter(|m| ws.role(&m.name).is_some_and(|r| roles.contains(&r.name())))
        .map(|m| format!("-p {}", m.name))
        .collect();
    anyhow::ensure!(
        !picked.is_empty(),
        "no crate has role {roles:?}: a list that names nothing would build nothing"
    );
    println!("{}", picked.join(" "));
    Ok(ExitCode::SUCCESS)
}

type Gate = fn(&workspace::Workspace, &mut Report) -> anyhow::Result<()>;

fn run(gates: &[Gate]) -> anyhow::Result<ExitCode> {
    let ws = workspace::Workspace::load()?;
    let mut report = Report::default();
    for gate in gates {
        gate(&ws, &mut report)?;
    }
    Ok(report.finish())
}
