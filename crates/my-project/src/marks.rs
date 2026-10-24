//! Startup and stage marks: how long the process took to reach each stage.
//!
//! Printed to stderr as `startup <ms> <stage>` when `MY_PROJECT_STARTUP=1`,
//! and free otherwise: one branch on a value read once. `cargo xtask bench`
//! reads these lines, so a startup row can say which stage moved instead of
//! only that the wall moved. They live here, in the driver: the core has no
//! clock (pillar 2), and measuring it is the host's business.

use std::time::Instant;

/// The environment variable that turns the marks on.
pub(crate) const ENV: &str = "MY_PROJECT_STARTUP";

/// Whether marks are printed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Print {
    On,
    Off,
}

/// The process's clock, started as early as `main` can start it.
#[derive(Debug)]
pub(crate) struct Marks {
    start: Instant,
    print: Print,
}

impl Marks {
    pub(crate) fn start() -> Self {
        let print = if std::env::var_os(ENV).is_some_and(|v| v == "1") {
            Print::On
        } else {
            Print::Off
        };
        Self {
            start: Instant::now(),
            print,
        }
    }

    /// Records that `stage` was reached now.
    pub(crate) fn mark(&self, stage: &str) {
        if self.print == Print::On {
            eprintln!(
                "startup {:.3} {stage}",
                self.start.elapsed().as_secs_f64() * 1e3
            );
        }
    }
}
