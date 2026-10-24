//! The gates (role: `tool`). `cargo xtask <gate>`; see `cargo xtask help`.
//!
//! Every gate runs a **positive control** before it trusts a pass: it first
//! checks a fixture it knows is wrong and fails if it does not find the fault.
//! A scanner that silently matches nothing looks exactly like a clean tree,
//! and the control is what tells the two apart.

use std::process::ExitCode;

mod arch;
mod budgets;
mod docs;
mod layers;
mod report;
mod scan;
mod sources;
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
  all       every gate above, in that order
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
        "all" => run(&[
            arch::run,
            layers::run,
            budgets::run,
            docs::run,
            sources::run,
        ]),
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

type Gate = fn(&workspace::Workspace, &mut Report) -> anyhow::Result<()>;

fn run(gates: &[Gate]) -> anyhow::Result<ExitCode> {
    let ws = workspace::Workspace::load()?;
    let mut report = Report::default();
    for gate in gates {
        gate(&ws, &mut report)?;
    }
    Ok(report.finish())
}
