// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Run every gate in a fixed order and aggregate the findings. Order is
//! cheap-and-local first, git last; violations from all gates are rendered
//! together so a builder sees the full list, not the first stumble.
//!
//! The array below is the only authority for which gates run and in what
//! order; `COUNT` is its length parameter, one token away. `report` owns
//! what a run's outcome reads like and what it exits with.

use std::path::Path;
use std::process::ExitCode;

use crate::report::{self, Violation, XtaskError};
use crate::{
    apisync, ax, budget, color, depmap, guard, header, length, lexicon, modmap, release, secret,
    specalign,
};

/// How many gates run. The array below is typed by it, so the number and
/// the list are one token apart and cannot disagree; `vocabulary` reads
/// it so no document has to hold a copy.
pub(crate) const COUNT: usize = 13;

pub(crate) fn run(root: &Path, range: Option<&str>) -> ExitCode {
    let results: [(&'static str, Result<Vec<Violation>, XtaskError>); COUNT] = [
        ("header", header::check(root)),
        ("lexicon", lexicon::check(root)),
        ("modmap", modmap::check(root)),
        ("length", length::check(root)),
        ("depmap", depmap::check(root)),
        ("secret", secret::check(root)),
        ("color", color::check(root)),
        ("ax", ax::check(root)),
        ("budget", budget::check(root)),
        ("specalign", specalign::check(root)),
        ("apisync", apisync::check(root, range)),
        ("release", release::check(root)),
        ("guard", guard::check(root, range)),
    ];

    report::finish_all(results)
}
