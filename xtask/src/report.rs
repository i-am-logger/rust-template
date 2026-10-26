//! Findings, collected across gates and printed once.

use std::fmt;
use std::process::ExitCode;

/// One violation: which gate, where, and what to do about it.
#[derive(Debug)]
pub(crate) struct Finding {
    gate: &'static str,
    place: String,
    message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.gate, self.place, self.message)
    }
}

/// Every finding so far, plus what each gate checked.
#[derive(Debug, Default)]
pub(crate) struct Report {
    findings: Vec<Finding>,
    checked: Vec<(&'static str, String)>,
}

impl Report {
    pub(crate) fn fail(
        &mut self,
        gate: &'static str,
        place: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.findings.push(Finding {
            gate,
            place: place.into(),
            message: message.into(),
        });
    }

    /// Records what a gate covered, so a pass says how much it looked at.
    pub(crate) fn checked(&mut self, gate: &'static str, what: impl Into<String>) {
        self.checked.push((gate, what.into()));
    }

    pub(crate) fn finish(self) -> ExitCode {
        for (gate, what) in &self.checked {
            eprintln!("[{gate}] checked {what}");
        }
        if self.findings.is_empty() {
            eprintln!("ok");
            return ExitCode::SUCCESS;
        }
        for f in &self.findings {
            eprintln!("{f}");
        }
        eprintln!("{} finding(s)", self.findings.len());
        ExitCode::FAILURE
    }
}
