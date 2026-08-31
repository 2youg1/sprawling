// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! MPL-2.0 header gate: every `.rs` in the repo opens with the exact
//! Exhibit A notice, then the copyright line.
//!
//! The notice is what the licence asks for. The copyright line is not:
//! Mozilla's own FAQ answers "what do I have to do" with the notice alone
//! and says a name "is not necessary" (MPL 2.0 FAQ, Q4). It ships here
//! because the person who owns this tree chose to be named in it, and it
//! is gated for the reason every other convention in this repo is gated -
//! a line nothing checks is a line the next new file will not carry.
//!
//! The notice takes the first three rows because MPL 2.0 section 3.4
//! forbids altering the substance of a licence notice, so it is the part
//! that must stay quotable and verbatim, and nothing may split it. The
//! copyright line follows immediately, with no blank comment row between
//! them: the four rows are one head, and a reader who has finished the
//! third row is already at the name.
//!
//! The year is the year of first publication and does not advance with the
//! calendar - a gate that demanded the current year would turn every
//! January red across every file at once, which is a chore rather than a
//! fact about the work.
//!
//! "and the sprawling contributors" names people who do not exist yet on
//! purpose. Contributors hold copyright in what they write and grant it
//! downstream themselves under section 2.1, so the clause transfers
//! nothing; what it buys is that the first outside contribution does not
//! oblige anyone to rewrite every header in the tree. Exhibit A allows
//! "additional accurate notices of copyright ownership", and a standing
//! class is accurate in a way an enumerated list is not - Mozilla dropped
//! the per-file contributor list in the 1.1 to 2.0 upgrade because it was
//! "neither a complete nor accurate list" and a source of merge conflicts.
//!
//! RefRain and kusanagi carry the same four rows with their own project
//! name. Anyone changing the shape here changes it in all three.

use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

const EXPECTED: [&str; 4] = [
    "// This Source Code Form is subject to the terms of the Mozilla Public",
    "// License, v. 2.0. If a copy of the MPL was not distributed with this",
    "// file, You can obtain one at https://mozilla.org/MPL/2.0/.",
    "// Copyright (c) 2026 2youg1 and the sprawling contributors",
];

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let mut violations = Vec::new();
    for file in walk::files_with_ext(root, &["rs"])? {
        let rel = walk::rel(root, &file);
        if walk::in_isolation_zone(&rel) {
            continue;
        }
        let text = walk::read_text(&file)?;
        let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
        let ok = EXPECTED.iter().all(|want| lines.next() == Some(want));
        if !ok {
            violations.push(Violation {
                gate: "header",
                location: rel,
                rule: "every .rs file carries the MPL-2.0 notice and the copyright line".to_owned(),
                violation: "the first four lines differ from the header".to_owned(),
                alternative: "prepend the exact 4-line header; see any existing module".to_owned(),
            });
        }
    }
    Ok(violations)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::EXPECTED;

    #[test]
    fn this_file_carries_the_header() {
        let text = include_str!("header.rs");
        let mut lines = text.lines();
        for want in EXPECTED {
            assert_eq!(lines.next(), Some(want));
        }
    }
}
