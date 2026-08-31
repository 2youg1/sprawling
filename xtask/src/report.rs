// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Violation shape and rendering. Mirrors the product's three-part refusal:
//! the builder who trips a gate gets rule | violation | alternative,
//! never a bare "failed" (xtask-SPEC.md section 10-5).

use std::process::ExitCode;

/// One gate finding. `location` is a repo-relative path, `path:line`, or a commit id.
pub(crate) struct Violation {
    pub(crate) gate: &'static str,
    pub(crate) location: String,
    pub(crate) rule: String,
    pub(crate) violation: String,
    pub(crate) alternative: String,
}

/// Gate-internal failures. A broken gate exits 2 and says why; it never
/// pretends to pass (exit 0) or to have judged (exit 1).
#[derive(Debug, thiserror::Error)]
pub(crate) enum XtaskError {
    #[error("io on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{file}: {msg}")]
    Doc { file: String, msg: String },
    #[error("command `{cmd}` failed: {msg}")]
    Cmd { cmd: String, msg: String },
}

pub(crate) fn render(violations: &[Violation]) {
    for v in violations {
        println!("[{}] {}", v.gate, v.location);
        println!("  rule:        {}", v.rule);
        println!("  violation:   {}", v.violation);
        println!("  alternative: {}", v.alternative);
    }
}

/// Render a single gate's outcome and map it to an exit code.
pub(crate) fn finish(gate: &'static str, result: Result<Vec<Violation>, XtaskError>) -> ExitCode {
    match result {
        Ok(violations) if violations.is_empty() => {
            println!("gate {gate}: ok");
            ExitCode::SUCCESS
        }
        Ok(violations) => {
            render(&violations);
            println!("gate {gate}: {} violation(s)", violations.len());
            ExitCode::FAILURE
        }
        Err(err) => internal_failure(&err),
    }
}

/// Render a whole run: one line per gate, then every finding, then one
/// exit code.
///
/// A gate that could not judge does **not** stop the walk. The gates are
/// all evaluated before the first line is printed, so the ones after the
/// failure are already holding their findings, and returning early threw
/// those away — which made "not judged" and "clean" the same output on
/// any machine without `cargo-public-api` (xtask-SPEC.md section 12).
pub(crate) fn finish_all(
    results: impl IntoIterator<Item = (&'static str, Result<Vec<Violation>, XtaskError>)>,
) -> ExitCode {
    let mut findings = Vec::new();
    let mut broken = 0_usize;
    for (gate, result) in results {
        match result {
            Ok(violations) if violations.is_empty() => println!("gate {gate}: ok"),
            Ok(violations) => {
                println!("gate {gate}: {} violation(s)", violations.len());
                findings.extend(violations);
            }
            Err(err) => {
                println!("gate {gate}: could not judge");
                say_internal_failure(&err);
                broken = broken.saturating_add(1);
            }
        }
    }
    render(&findings);
    if !findings.is_empty() {
        println!("{} violation(s) across all gates", findings.len());
    }
    let outcome = RunOutcome::of(broken, findings.len());
    match outcome {
        RunOutcome::Green => println!("all gates green"),
        RunOutcome::Refused => {}
        RunOutcome::Broken => println!("{broken} gate(s) could not judge"),
    }
    outcome.code()
}

/// What one whole run concluded.
///
/// Ordered by severity, and the order is the point: exit 2 says the gate
/// is broken, exit 1 says the tree is, and a run that hit both must say
/// the first. Collapsing them lets a missing tool read as a clean tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunOutcome {
    Green,
    Refused,
    Broken,
}

impl RunOutcome {
    fn of(broken: usize, findings: usize) -> Self {
        if broken > 0 {
            Self::Broken
        } else if findings > 0 {
            Self::Refused
        } else {
            Self::Green
        }
    }

    fn code(self) -> ExitCode {
        match self {
            Self::Green => ExitCode::SUCCESS,
            Self::Refused => ExitCode::FAILURE,
            Self::Broken => ExitCode::from(2),
        }
    }
}

pub(crate) fn internal_failure(err: &XtaskError) -> ExitCode {
    say_internal_failure(err);
    ExitCode::from(2)
}

/// The one wording for "a gate could not judge", so the single-gate path
/// and the whole-run path cannot describe the same event differently.
fn say_internal_failure(err: &XtaskError) {
    eprintln!("xtask internal failure: {err}");
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::RunOutcome;

    #[test]
    fn a_broken_gate_outranks_a_violation() {
        // The regression this pins: `apisync` failing to run used to
        // discard `guard`'s findings, so a commit that had loosened a
        // gate printed the same thing as a commit that had not.
        assert_eq!(RunOutcome::of(1, 7), RunOutcome::Broken);
        assert_eq!(RunOutcome::of(1, 0), RunOutcome::Broken);
        assert_eq!(RunOutcome::of(0, 7), RunOutcome::Refused);
        assert_eq!(RunOutcome::of(0, 0), RunOutcome::Green);
    }
}
