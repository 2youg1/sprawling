// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Run every gate in a fixed order and aggregate the findings. Order is
//! cheap-and-local first, git last; violations from all gates are rendered
//! together so a builder sees the full list, not the first stumble.

use std::path::Path;
use std::process::ExitCode;

use crate::report::{self, Violation, XtaskError};
use crate::zerojs;
use crate::{
    apisync, budget, color, depmap, guard, header, length, lexicon, modmap, release, secret,
    specalign,
};

/// How many gates run. The array below is typed by it, so the number and
/// the list are one token apart and cannot disagree; `vocabulary` reads
/// it so no document has to hold a copy.
pub(crate) const COUNT: usize = 13;

pub(crate) fn run(root: &Path, range: Option<&str>) -> ExitCode {
    let results: [(&str, Result<Vec<Violation>, XtaskError>); COUNT] = [
        ("header", header::check(root)),
        ("lexicon", lexicon::check(root)),
        ("modmap", modmap::check(root)),
        ("length", length::check(root)),
        ("depmap", depmap::check(root)),
        ("zerojs", zerojs::check(root)),
        ("secret", secret::check(root)),
        ("color", color::check(root)),
        ("budget", budget::check(root)),
        ("specalign", specalign::check(root)),
        ("apisync", apisync::check(root, range)),
        ("release", release::check(root)),
        ("guard", guard::check(root, range)),
    ];

    let mut all = Vec::new();
    for (name, result) in results {
        match result {
            Ok(violations) => {
                println!("gate {name}: {}", summary(&violations));
                all.extend(violations);
            }
            Err(err) => return report::internal_failure(&err),
        }
    }
    if all.is_empty() {
        println!("all gates green");
        ExitCode::SUCCESS
    } else {
        report::render(&all);
        println!("{} violation(s) across all gates", all.len());
        ExitCode::FAILURE
    }
}

fn summary(violations: &[Violation]) -> String {
    if violations.is_empty() {
        "ok".to_owned()
    } else {
        format!("{} violation(s)", violations.len())
    }
}
