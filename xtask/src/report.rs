// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

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

pub(crate) fn internal_failure(err: &XtaskError) -> ExitCode {
    eprintln!("xtask internal failure: {err}");
    ExitCode::from(2)
}
