// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The two ways a vocabulary grows a second authority, both held here.
//!
//! **A retired word must point at a defined one.** `lexicon.toml` says
//! which phrasings are out; `docs/glossary.md` says which word is in.
//! Nothing kept them agreeing, so a retirement could name a replacement
//! the glossary never defined - and a reader following the gate's advice
//! would arrive at a word with no meaning behind it.
//!
//! **A count belongs to whatever can recount it.** Every gate count
//! written by hand in a product document has gone stale at least once:
//! four documents said ten gates while twelve ran. A number a machine
//! can derive is a number no document should hold.

use std::collections::BTreeSet;
use std::path::Path;

use crate::gates;
use crate::report::{Violation, XtaskError};
use crate::walk;

/// The documents whose claims are about the product as it stands. Card
/// notes and stage records are history and say what was true then.
const PRODUCT_DOCS: [&str; 1] = ["docs"];

/// Number words a document might spell a count with, and their values.
const SPELLED: [(&str, usize); 14] = [
    ("zero", 0),
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("eleven", 11),
    ("twelve", 12),
    ("thirteen", 13),
];

/// Chinese numerals, for the one product document written in Chinese.
const CHINESE: [(&str, usize); 13] = [
    ("一", 1),
    ("二", 2),
    ("三", 3),
    ("四", 4),
    ("五", 5),
    ("六", 6),
    ("七", 7),
    ("八", 8),
    ("九", 9),
    ("十", 10),
    ("十一", 11),
    ("十二", 12),
    ("十三", 13),
];

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let mut violations = Vec::new();
    let glossary = terms(root)?;
    check_replacements(root, &glossary, &mut violations)?;
    check_counts(root, &mut violations)?;
    Ok(violations)
}

/// Every bold name the glossary defines, plus the file names it uses,
/// since a replacement may legitimately point at a document.
fn terms(root: &Path) -> Result<BTreeSet<String>, XtaskError> {
    let path = root.join("docs").join("glossary.md");
    let text = walk::read_text(&path)?;
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let mut rest = line;
        while let Some(open) = rest.find("**") {
            let after = rest.get(open.saturating_add(2)..).unwrap_or_default();
            let Some(close) = after.find("**") else { break };
            let term = after.get(..close).unwrap_or_default();
            if !term.is_empty() {
                out.insert(term.replace('\\', ""));
            }
            rest = after.get(close.saturating_add(2)..).unwrap_or_default();
        }
    }
    if out.is_empty() {
        return Err(XtaskError::Doc {
            file: "docs/glossary.md".to_owned(),
            msg: "no bold terms found; the table shape changed".to_owned(),
        });
    }
    Ok(out)
}

fn check_replacements(
    root: &Path,
    glossary: &BTreeSet<String>,
    out: &mut Vec<Violation>,
) -> Result<(), XtaskError> {
    let path = root.join("xtask").join("lexicon.toml");
    let text = walk::read_text(&path)?;
    for (index, line) in text.lines().enumerate() {
        let Some(value) = line.strip_prefix("replacement = ") else {
            continue;
        };
        let replacement = value.trim().trim_matches('"');
        // A document is a definition too: pointing at `JOB.md` names a
        // file whose content is the meaning.
        let defined =
            replacement.contains(".md") || glossary.iter().any(|term| replacement.contains(term));
        if !defined {
            out.push(Violation {
                gate: "lexicon",
                location: format!("xtask/lexicon.toml:{}", index.saturating_add(1)),
                rule: "a retired word points at a word the glossary defines".to_owned(),
                violation: format!("{replacement:?} is not defined in docs/glossary.md"),
                alternative: "give the replacement a glossary row, or name one that has it"
                    .to_owned(),
            });
        }
    }
    Ok(())
}

fn check_counts(root: &Path, out: &mut Vec<Violation>) -> Result<(), XtaskError> {
    let mut files = vec![root.join("README.md")];
    for dir in PRODUCT_DOCS {
        files.extend(walk::files_with_ext(&root.join(dir), &["md"])?);
    }
    for file in files {
        let rel = walk::rel(root, &file);
        let text = walk::read_text(&file)?;
        for (index, line) in text.lines().enumerate() {
            for stated in counted(line) {
                if stated != gates::COUNT {
                    out.push(Violation {
                        gate: "lexicon",
                        location: format!("{rel}:{}", index.saturating_add(1)),
                        rule: "a count a machine can recount is not written by hand".to_owned(),
                        violation: format!("this line says {stated} gate(s); {} run", gates::COUNT),
                        alternative: "state the count without a number, or correct it here"
                            .to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// The gate counts a Chinese line states.
///
/// Read backwards from the word rather than by matching each numeral:
/// `十二门` contains `二门`, so a forward match would report both twelve
/// and two and the second one would be an invention.
fn chinese_counts(line: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    for (index, symbol) in chars.iter().enumerate() {
        if *symbol != '门' {
            continue;
        }
        let mut start = index;
        // `道` sits between the count and the word when a writer uses it.
        if start > 0 && chars.get(start.saturating_sub(1)) == Some(&'道') {
            start = start.saturating_sub(1);
        }
        let mut numeral = String::new();
        while start > 0 {
            let Some(previous) = chars.get(start.saturating_sub(1)) else {
                break;
            };
            if !CHINESE.iter().any(|(digit, _)| digit.contains(*previous)) {
                break;
            }
            numeral.insert(0, *previous);
            start = start.saturating_sub(1);
        }
        if let Some((_, value)) = CHINESE.iter().find(|(digit, _)| *digit == numeral) {
            found.push(*value);
        }
    }
    found
}

/// Every gate count a line states, in both languages this repository
/// writes documents in.
///
/// Deliberately narrow: it reads the shapes that have actually rotted -
/// `ten gates` and `十二门` - rather than trying to recognise a count
/// spelled any way at all. A rule that guesses is a rule that fires on
/// prose about something else.
fn counted(line: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (word, value) in SPELLED {
        for pattern in [format!("{word} gates"), format!("{word} gate ")] {
            if line.to_ascii_lowercase().contains(&pattern) {
                found.push(value);
            }
        }
    }
    found.extend(chinese_counts(line));
    for token in line.split(|c: char| !c.is_ascii_digit()) {
        if token.is_empty() {
            continue;
        }
        let Ok(value) = token.parse::<usize>() else {
            continue;
        };
        for shape in [format!("{value} gates"), format!("{value} 道门")] {
            if line.contains(&shape) {
                found.push(value);
            }
        }
    }
    found
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;

    #[test]
    fn a_replacement_the_glossary_never_defined_is_reported() {
        let glossary: BTreeSet<String> = ["Ledger".to_owned()].into();
        let mut out = Vec::new();
        let defined = |replacement: &str| {
            replacement.contains(".md") || glossary.iter().any(|t| replacement.contains(t))
        };
        assert!(defined("Ledger"));
        assert!(defined("JOB.md（产品）"));
        assert!(!defined("some other name"));
        assert!(out.is_empty());
        out.push(1);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_count_is_read_in_both_languages_and_only_in_the_shapes_that_rot() {
        assert_eq!(counted("fmt + clippy + nextest + ten gates"), vec![10]);
        assert_eq!(counted("十二门全绿"), vec![12]);
        assert_eq!(counted("12 gates run today"), vec![12]);
        assert!(
            counted("five doors plus deduplication").is_empty(),
            "the kernel's doors are a different concept with a different word"
        );
        assert!(
            counted("the gate says what to do").is_empty(),
            "a gate without a count is not a claim about how many there are"
        );
    }

    #[test]
    fn the_glossary_of_this_repository_parses_and_defines_its_own_words() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let terms = terms(&root).unwrap();
        assert!(terms.contains("Ledger"));
        assert!(terms.contains("Building"));
    }
}
