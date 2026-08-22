// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! MPL-2.0 header gate: every `.rs` in the repo starts with the exact
//! three-line notice.

use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

const EXPECTED: [&str; 3] = [
    "// This Source Code Form is subject to the terms of the Mozilla Public",
    "// License, v. 2.0. If a copy of the MPL was not distributed with this",
    "// file, You can obtain one at https://mozilla.org/MPL/2.0/.",
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
                rule: "every .rs file carries the 3-line MPL-2.0 notice".to_owned(),
                violation: "first three lines differ from the notice".to_owned(),
                alternative: "prepend the exact 3-line header; see any existing module".to_owned(),
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
