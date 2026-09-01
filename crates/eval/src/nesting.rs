// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Which nested format a model edits with fewest mistakes, and how it
//! fails when it fails (v0.0.3 card V3.16).
//!
//! **The product of this module is a number, not a preference.** The
//! plan tree has to live in a file a model edits every day, and three
//! formats were argued for on taste. Taste is not evidence: a format
//! that reads well and is edited wrongly one time in six is a worse
//! format than one nobody likes. So the question is settled by counting.
//!
//! **The failure distribution matters more than the rate.** A format
//! that fails by dropping a field is far worse here than one that fails
//! by mangling indentation, because a dropped field is a plan node that
//! silently stops existing while a broken indent refuses to parse. So a
//! run reports both: how often, and how.
//!
//! This grades a real edit against the text that came back. It does not
//! call a model — [`Attempt`] is what one produced — because a suite
//! that owned a provider could not run offline, could not be replayed,
//! and would be measuring the network as much as the model.

use std::collections::BTreeMap;

use kernel::{AxCode, AxError};

/// A format the plan tree could be written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Shape {
    /// Nested tables, which is what the repository's configuration
    /// already uses.
    Toml,
    /// Nested objects, which every model has seen most of.
    Json,
    /// A nested list, which is what a person reads fastest.
    Markdown,
}

impl Shape {
    /// Every shape, in the order a report lists them.
    pub const ALL: [Shape; 3] = [Shape::Toml, Shape::Json, Shape::Markdown];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Markdown => "markdown",
        }
    }
}

/// How one edit went wrong. Exhaustive, and ordered by how much damage
/// each one does to a plan tree that nobody is watching.
///
/// The order is the finding this suite exists to produce: `LostField` is
/// first because it is the only one that leaves a *readable* file that
/// is missing a node, and a plan node that silently stops existing is
/// worse than a file that will not parse at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fault {
    /// The result parses and a field the original had is gone.
    LostField,
    /// The result parses and a value the edit was not asked to touch has
    /// changed.
    ChangedBystander,
    /// The result does not parse.
    Unparseable,
    /// The result stops early, mid-structure.
    Truncated,
    /// It parses, keeps every field, and the edit was not applied.
    NotApplied,
}

impl Fault {
    /// Every fault, worst first.
    pub const ALL: [Fault; 5] = [
        Fault::LostField,
        Fault::ChangedBystander,
        Fault::Unparseable,
        Fault::Truncated,
        Fault::NotApplied,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LostField => "lost a field",
            Self::ChangedBystander => "changed something it was not asked to",
            Self::Unparseable => "does not parse",
            Self::Truncated => "stops early",
            Self::NotApplied => "the edit is not there",
        }
    }
}

/// One real edit, and what a model returned for it.
///
/// `before` and `after` are the whole document, because the failure this
/// is looking for is what happened to the parts of the document nobody
/// asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub shape: Shape,
    /// The document as it was.
    pub before: String,
    /// The document as the model returned it.
    pub returned: String,
    /// The leaf paths the edit was allowed to change, as `a/b/c`.
    pub touching: Vec<String>,
    /// What those paths were supposed to become.
    pub wanted: BTreeMap<String, String>,
}

/// What one attempt came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub shape: Shape,
    /// `None` is a clean edit.
    pub fault: Option<Fault>,
}

/// Grades one attempt.
///
/// **Worst fault wins.** A result can be several kinds of wrong at once,
/// and reporting the mildest would flatter the format: a document that
/// lost a field *and* failed to apply the edit is counted as having lost
/// a field, because that is the one that survives into a plan tree.
///
/// # Errors
/// Refuses an attempt whose `before` this build cannot read. That is a
/// broken fixture rather than a model failure, and counting it as a
/// model failure would make the corpus grade itself.
pub fn grade(attempt: &Attempt) -> Result<Verdict, AxError> {
    let before = read(attempt.shape, &attempt.before).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "grade a nested edit",
            attempt.shape.as_str().to_owned(),
        )
        .with_recovery("the fixture's own `before` does not parse; fix the corpus, not the model")
    })?;
    let Some(after) = read(attempt.shape, &attempt.returned) else {
        // Told apart by whether the text stops inside the structure: a
        // model that ran out of tokens and one that wrote something
        // malformed call for different answers, and only one of them is
        // fixed by asking for more tokens.
        let fault = if stops_early(attempt.shape, &attempt.returned) {
            Fault::Truncated
        } else {
            Fault::Unparseable
        };
        return Ok(Verdict {
            shape: attempt.shape,
            fault: Some(fault),
        });
    };
    for path in before.keys() {
        if !after.contains_key(path) {
            return Ok(Verdict {
                shape: attempt.shape,
                fault: Some(Fault::LostField),
            });
        }
    }
    for (path, was) in &before {
        if attempt.touching.contains(path) {
            continue;
        }
        if after.get(path) != Some(was) {
            return Ok(Verdict {
                shape: attempt.shape,
                fault: Some(Fault::ChangedBystander),
            });
        }
    }
    for (path, wanted) in &attempt.wanted {
        if after.get(path) != Some(wanted) {
            return Ok(Verdict {
                shape: attempt.shape,
                fault: Some(Fault::NotApplied),
            });
        }
    }
    Ok(Verdict {
        shape: attempt.shape,
        fault: None,
    })
}

/// How one format did across a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grades {
    pub shape: Shape,
    pub tried: u32,
    pub wrong: u32,
    /// How it failed when it failed, worst first. Absent faults are
    /// absent rather than zero: a table of zeroes reads as a
    /// measurement, and these were not measured to be zero, they simply
    /// did not happen.
    pub faults: BTreeMap<Fault, u32>,
}

impl Grades {
    /// The rate, per mille. Per mille rather than a float, for the
    /// reason the rest of this crate uses it: an exact integer compares
    /// the same way twice.
    #[must_use]
    pub fn wrong_per_mille(&self) -> u32 {
        if self.tried == 0 {
            return 0;
        }
        self.wrong
            .saturating_mul(1000)
            .checked_div(self.tried)
            .unwrap_or(0)
    }
}

/// Every format's grades from one run, in [`Shape::ALL`] order.
///
/// A shape nobody tried is reported with `tried: 0` rather than left
/// out: a comparison missing one of its three columns is a comparison a
/// reader will misread as a clean sweep.
#[must_use]
pub fn tally(verdicts: &[Verdict]) -> Vec<Grades> {
    Shape::ALL
        .into_iter()
        .map(|shape| {
            let mine: Vec<&Verdict> = verdicts.iter().filter(|held| held.shape == shape).collect();
            let mut faults: BTreeMap<Fault, u32> = BTreeMap::new();
            for held in &mine {
                if let Some(fault) = held.fault {
                    let counted = faults.entry(fault).or_default();
                    *counted = counted.saturating_add(1);
                }
            }
            Grades {
                shape,
                tried: u32::try_from(mine.len()).unwrap_or(u32::MAX),
                wrong: faults.values().copied().fold(0u32, u32::saturating_add),
                faults,
            }
        })
        .collect()
}

/// The format to write the plan tree in, from one run's grades.
///
/// **Fewest mistakes wins, and a tie is broken by the worst fault each
/// one makes.** Two formats that fail equally often are not equally
/// good: the one whose failures leave a readable file with a node
/// missing costs more than the one whose failures refuse to parse,
/// because only one of them is noticed.
///
/// `None` when nothing was tried, which is not a recommendation to use
/// anything.
#[must_use]
pub fn recommended(grades: &[Grades]) -> Option<Shape> {
    grades
        .iter()
        .filter(|held| held.tried > 0)
        .min_by_key(|held| {
            // `Fault::ALL` is ordered worst first, so a lower position is
            // a worse failure and the winner is the one whose worst
            // failure sits *latest* in that list. Reversed for exactly
            // that reason: without it this picks the format that fails
            // most damagingly.
            let worst = Fault::ALL
                .iter()
                .position(|fault| held.faults.contains_key(fault))
                .unwrap_or(Fault::ALL.len());
            (held.wrong_per_mille(), std::cmp::Reverse(worst), held.shape)
        })
        .map(|held| held.shape)
}

/// Every leaf of a document, as `path -> value`.
///
/// One reading for all three formats, because the question is about the
/// document's leaves and not about its syntax: a comparison whose three
/// arms read three different things is comparing readers rather than
/// formats.
fn read(shape: Shape, text: &str) -> Option<BTreeMap<String, String>> {
    match shape {
        Shape::Json => leaves(&serde_json::from_str::<serde_json::Value>(text).ok()?),
        Shape::Toml => leaves(&toml::from_str::<serde_json::Value>(text).ok()?),
        Shape::Markdown => markdown_leaves(text),
    }
}

/// Flattens a tree into `a/b/c -> value`.
fn leaves(value: &serde_json::Value) -> Option<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    walk(value, &mut String::new(), &mut found);
    Some(found)
}

fn walk(value: &serde_json::Value, at: &mut String, into: &mut BTreeMap<String, String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (name, held) in map {
                let was = at.len();
                if !at.is_empty() {
                    at.push('/');
                }
                at.push_str(name);
                walk(held, at, into);
                at.truncate(was);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, held) in items.iter().enumerate() {
                let was = at.len();
                if !at.is_empty() {
                    at.push('/');
                }
                at.push_str(&index.to_string());
                walk(held, at, into);
                at.truncate(was);
            }
        }
        other => {
            into.insert(at.clone(), scalar(other));
        }
    }
}

/// A leaf as text. Numbers keep their own spelling, because a format
/// that turned `2` into `2.0` changed something and this must see it.
fn scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// A nested list, read as `parent/child -> value`.
///
/// The shape this grades is the one a plan tree takes: `- name: value`,
/// nested by two spaces. Deliberately strict — a reader that repaired
/// sloppy indentation would hide the exact failure this suite is
/// counting.
fn markdown_leaves(text: &str) -> Option<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    let mut stack: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len().checked_sub(line.trim_start().len())?;
        // Two spaces per level, exactly. An odd indent is a malformed
        // document rather than a document to be forgiving about.
        if indent % 2 != 0 {
            return None;
        }
        let depth = indent.checked_div(2)?;
        let item = line.trim_start().strip_prefix("- ")?;
        let (name, value) = match item.split_once(": ") {
            Some((name, value)) => (name.trim(), Some(value.trim())),
            None => (item.trim(), None),
        };
        if name.is_empty() {
            return None;
        }
        stack.truncate(depth);
        if stack.len() != depth {
            return None;
        }
        stack.push(name.to_owned());
        if let Some(value) = value {
            found.insert(stack.join("/"), value.to_owned());
        }
    }
    Some(found)
}

/// Whether text stops inside the structure rather than being malformed.
///
/// Counted by what is still open at the end. Not a parser: a parser
/// already refused this text, and the only question left is whether it
/// refused because the model ran out of room or because it wrote
/// something wrong.
fn stops_early(shape: Shape, text: &str) -> bool {
    match shape {
        Shape::Json => {
            let mut depth: i64 = 0;
            let mut inside = false;
            let mut escaped = false;
            for glyph in text.chars() {
                match glyph {
                    _ if escaped => escaped = false,
                    '\\' if inside => escaped = true,
                    '"' => inside = !inside,
                    '{' | '[' if !inside => depth = depth.saturating_add(1),
                    '}' | ']' if !inside => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            depth > 0 || inside
        }
        // A table header with nothing under it, or a value that never
        // closed its quote.
        Shape::Toml => {
            let last = text.lines().last().unwrap_or_default().trim();
            last.starts_with('[') && !last.ends_with(']')
                || last.matches('"').count() % 2 == 1
                || last.ends_with('=')
        }
        // A bullet with a name and no value under it, which is what a
        // nested list looks like when it stops mid-branch.
        Shape::Markdown => {
            let last = text.lines().last().unwrap_or_default();
            last.trim_start().starts_with("- ") && !last.contains(": ")
        }
    }
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
    use super::{Attempt, Fault, Grades, Shape, Verdict, grade, recommended, tally};
    use std::collections::BTreeMap;

    fn wanting(path: &str, value: &str) -> BTreeMap<String, String> {
        let mut wanted = BTreeMap::new();
        wanted.insert(path.to_owned(), value.to_owned());
        wanted
    }

    fn attempt(shape: Shape, before: &str, returned: &str) -> Attempt {
        Attempt {
            shape,
            before: before.to_owned(),
            returned: returned.to_owned(),
            touching: vec!["plan/one/state".to_owned()],
            wanted: wanting("plan/one/state", "done"),
        }
    }

    const JSON_BEFORE: &str =
        r#"{"plan":{"one":{"state":"open","weight":"3"},"two":{"state":"open"}}}"#;

    #[test]
    fn a_clean_edit_has_no_fault() {
        let after = r#"{"plan":{"one":{"state":"done","weight":"3"},"two":{"state":"open"}}}"#;
        let verdict = grade(&attempt(Shape::Json, JSON_BEFORE, after)).unwrap();
        assert_eq!(verdict.fault, None);
    }

    /// The finding this whole suite exists to make visible: a result
    /// that parses and is missing a node is the worst outcome, because
    /// it is the only one nothing downstream notices.
    #[test]
    fn a_document_that_parses_with_a_node_missing_is_the_worst_outcome() {
        let after = r#"{"plan":{"one":{"state":"done","weight":"3"}}}"#;
        let verdict = grade(&attempt(Shape::Json, JSON_BEFORE, after)).unwrap();
        assert_eq!(verdict.fault, Some(Fault::LostField));
        assert_eq!(Fault::ALL[0], Fault::LostField, "and it is ordered first");
    }

    /// Losing a field outranks failing to apply the edit: a result can
    /// be several kinds of wrong, and reporting the mildest would
    /// flatter the format.
    #[test]
    fn the_worst_fault_is_the_one_reported() {
        let after = r#"{"plan":{"one":{"state":"open","weight":"3"}}}"#;
        let verdict = grade(&attempt(Shape::Json, JSON_BEFORE, after)).unwrap();
        assert_eq!(verdict.fault, Some(Fault::LostField));
    }

    #[test]
    fn a_value_nobody_asked_about_may_not_move() {
        let after = r#"{"plan":{"one":{"state":"done","weight":"5"},"two":{"state":"open"}}}"#;
        let verdict = grade(&attempt(Shape::Json, JSON_BEFORE, after)).unwrap();
        assert_eq!(verdict.fault, Some(Fault::ChangedBystander));
    }

    #[test]
    fn an_edit_that_never_landed_is_counted() {
        let after = r#"{"plan":{"one":{"state":"open","weight":"3"},"two":{"state":"open"}}}"#;
        let verdict = grade(&attempt(Shape::Json, JSON_BEFORE, after)).unwrap();
        assert_eq!(verdict.fault, Some(Fault::NotApplied));
    }

    /// Running out of room and writing nonsense call for different
    /// answers, and only one of them is fixed by asking for more tokens.
    #[test]
    fn stopping_early_is_told_apart_from_writing_nonsense() {
        let cut = r#"{"plan":{"one":{"state":"done","#;
        assert_eq!(
            grade(&attempt(Shape::Json, JSON_BEFORE, cut))
                .unwrap()
                .fault,
            Some(Fault::Truncated)
        );
        let wrong = r#"{"plan": <<<}"#;
        assert_eq!(
            grade(&attempt(Shape::Json, JSON_BEFORE, wrong))
                .unwrap()
                .fault,
            Some(Fault::Unparseable)
        );
    }

    #[test]
    fn all_three_shapes_are_read_into_the_same_leaves() {
        let toml = "[plan.one]\nstate = \"open\"\nweight = \"3\"\n[plan.two]\nstate = \"open\"\n";
        let after = "[plan.one]\nstate = \"done\"\nweight = \"3\"\n[plan.two]\nstate = \"open\"\n";
        assert_eq!(
            grade(&attempt(Shape::Toml, toml, after)).unwrap().fault,
            None
        );

        let md =
            "- plan\n  - one\n    - state: open\n    - weight: 3\n  - two\n    - state: open\n";
        let md_after =
            "- plan\n  - one\n    - state: done\n    - weight: 3\n  - two\n    - state: open\n";
        assert_eq!(
            grade(&attempt(Shape::Markdown, md, md_after))
                .unwrap()
                .fault,
            None
        );
    }

    /// A reader that repaired sloppy indentation would hide the exact
    /// failure this suite counts.
    #[test]
    fn a_nested_list_with_a_broken_indent_does_not_quietly_parse() {
        let md = "- plan\n  - one\n    - state: open\n";
        let bent = "- plan\n   - one\n    - state: done\n";
        assert_eq!(
            grade(&Attempt {
                shape: Shape::Markdown,
                before: md.to_owned(),
                returned: bent.to_owned(),
                touching: vec!["plan/one/state".to_owned()],
                wanted: wanting("plan/one/state", "done"),
            })
            .unwrap()
            .fault,
            Some(Fault::Unparseable)
        );
    }

    /// A broken fixture is not a model failure. Counting it as one would
    /// let the corpus grade itself.
    #[test]
    fn a_corpus_that_does_not_parse_refuses_rather_than_scoring() {
        let broken = grade(&attempt(Shape::Json, "{not json", "{}"));
        assert!(broken.is_err());
    }

    #[test]
    fn a_shape_nobody_tried_is_reported_rather_than_left_out() {
        let grades = tally(&[Verdict {
            shape: Shape::Json,
            fault: None,
        }]);
        assert_eq!(grades.len(), 3);
        assert_eq!(grades[1].shape, Shape::Json);
        assert_eq!(grades[1].tried, 1);
        let untried: Vec<Shape> = grades
            .iter()
            .filter(|held| held.tried == 0)
            .map(|held| held.shape)
            .collect();
        assert_eq!(untried, vec![Shape::Toml, Shape::Markdown]);
    }

    #[test]
    fn the_rate_is_an_exact_integer() {
        let grades = tally(&[
            Verdict {
                shape: Shape::Toml,
                fault: Some(Fault::LostField),
            },
            Verdict {
                shape: Shape::Toml,
                fault: None,
            },
            Verdict {
                shape: Shape::Toml,
                fault: None,
            },
        ]);
        assert_eq!(grades[0].tried, 3);
        assert_eq!(grades[0].wrong, 1);
        assert_eq!(grades[0].wrong_per_mille(), 333);
    }

    /// Two formats that fail equally often are not equally good: the one
    /// whose failures leave a readable file with a node missing costs
    /// more, because only one of them is noticed.
    #[test]
    fn a_tie_is_broken_by_how_badly_each_one_fails() {
        let mut lost = BTreeMap::new();
        lost.insert(Fault::LostField, 1);
        let mut unreadable = BTreeMap::new();
        unreadable.insert(Fault::Unparseable, 1);
        let grades = vec![
            Grades {
                shape: Shape::Toml,
                tried: 10,
                wrong: 1,
                faults: lost,
            },
            Grades {
                shape: Shape::Json,
                tried: 10,
                wrong: 1,
                faults: unreadable,
            },
        ];
        assert_eq!(recommended(&grades), Some(Shape::Json));
    }

    #[test]
    fn nothing_tried_recommends_nothing() {
        assert_eq!(recommended(&tally(&[])), None);
    }
}
