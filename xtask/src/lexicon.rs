// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lexicon gate: one concept, one name. Scans markdown and Rust sources for
//! banned terms (the machine subset of the retired-term list; data face is
//! xtask/lexicon.toml). A line is exempt when it or the line above carries
//! `lexicon-ok: <reason>` (redline C6).

use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

const EXEMPT_MARK: &str = "lexicon-ok:";

#[derive(serde::Deserialize)]
struct Data {
    entry: Vec<Entry>,
}

#[derive(serde::Deserialize)]
struct Entry {
    banned: String,
    replacement: String,
}

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let data = load(root)?;
    let mut violations = Vec::new();
    // The same gate also holds the two ways a vocabulary grows a second
    // authority; see `vocabulary`.
    violations.extend(crate::vocabulary::check(root)?);
    for file in walk::files_with_ext(root, &["md", "rs"])? {
        let rel = walk::rel(root, &file);
        if walk::in_isolation_zone(&rel) {
            continue;
        }
        let text = walk::read_text(&file)?;
        scan(&rel, &text, &data.entry, &mut violations);
    }
    Ok(violations)
}

fn load(root: &Path) -> Result<Data, XtaskError> {
    let path = root.join("xtask").join("lexicon.toml");
    let text = walk::read_text(&path)?;
    toml::from_str(&text).map_err(|err| XtaskError::Doc {
        file: "xtask/lexicon.toml".to_owned(),
        msg: err.to_string(),
    })
}

fn scan(location: &str, text: &str, entries: &[Entry], out: &mut Vec<Violation>) {
    let mut previous_exempts = false;
    for (index, line) in text.lines().enumerate() {
        let marked = line.contains(EXEMPT_MARK);
        let exempt = marked || previous_exempts;
        if !exempt {
            for entry in entries {
                if line.contains(&entry.banned) {
                    out.push(Violation {
                        gate: "lexicon",
                        location: format!("{location}:{}", index.saturating_add(1)),
                        rule: "the vocabulary authority is docs/glossary.md".to_owned(),
                        violation: format!("banned term {:?}", entry.banned),
                        alternative: format!(
                            "use {}; or justify inline with `{EXEMPT_MARK} <reason>`",
                            entry.replacement
                        ),
                    });
                }
            }
        }
        previous_exempts = marked;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn entries() -> Vec<Entry> {
        vec![Entry {
            banned: "banned-term".to_owned(),
            replacement: "Ledger".to_owned(),
        }]
    }

    #[test]
    fn hit_is_reported_with_line_number() {
        let mut out = Vec::new();
        scan(
            "doc.md",
            "ok\nuses banned-term here\n",
            &entries(),
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].location, "doc.md:2");
    }

    #[test]
    fn same_line_and_previous_line_marks_exempt() {
        let mut out = Vec::new();
        scan(
            "doc.md",
            "banned-term lexicon-ok: quoting the old name\n",
            &entries(),
            &mut out,
        );
        scan(
            "doc.md",
            "lexicon-ok: next line quotes it\nbanned-term\n",
            &entries(),
            &mut out,
        );
        assert!(out.is_empty());
    }
}
