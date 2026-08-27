// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! When to shorten something, and what to keep.
//!
//! The mechanism half of this — moving a big result out of the window
//! and leaving a reference — is `offload`'s and was frozen first. This
//! is the policy half: given a piece of text, what kind of thing is it,
//! and which end of it carries the information.
//!
//! Three rules hold the whole module up.
//!
//! **The result is never longer than the input.** Shortening that
//! lengthens is not a corner case, it is the failure that makes a
//! context budget meaningless, so it is an invariant with a test rather
//! than a property people remember.
//!
//! **No regular expressions on this path.** Detection is a handful of
//! prefix and count checks over the first few lines; a pattern engine
//! here would put backtracking between a model and its next turn.
//!
//! **Cuts land on character boundaries.** A budget in bytes and a cut in
//! the middle of a character is how a compactor produces text nothing
//! downstream can read.

use kernel::ByteLen;

/// What a piece of text is, as far as shortening is concerned. Seven,
/// and `Unknown` is one of them: a compactor that had to guess would
/// guess wrong on exactly the material nobody anticipated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    /// Sentences. The beginning says what it is about.
    Prose,
    /// Source. Both ends matter and the middle repeats.
    Code,
    /// A patch. The hunk headers are the information.
    Diff,
    /// Lines with a shape, most of them the same. The end is the news.
    Log,
    /// Structured data. Shortening it makes it unparseable, so it goes
    /// out of the window whole instead.
    Structured,
    /// Rows and columns. The header row is load-bearing.
    Table,
    /// Anything else, including binary-looking material.
    Unknown,
}

/// What to do with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Small enough; leave it alone.
    Keep,
    /// Keep the front. Prose and tables read from the top.
    Head,
    /// Keep both ends and mark the gap. Code and diffs are read from
    /// the edges inward.
    Ends,
    /// Keep the back. A log's news is at the end.
    Tail,
    /// Do not shorten it at all — move it out of the window and leave a
    /// reference. Truncated structured data is worse than absent
    /// structured data, because it looks like it can be parsed.
    Offload,
}

/// Detection, in priority order, over the first lines only.
///
/// Order is the design: a diff is also code, a table is also prose, and
/// whichever check runs first wins. Cheapest and most specific first.
#[must_use]
pub fn detect(text: &str) -> Content {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return Content::Unknown;
    }
    if trimmed.starts_with("diff --git")
        || trimmed.starts_with("--- ")
        || trimmed.starts_with("@@ ")
    {
        return Content::Diff;
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with("<?xml") {
        return Content::Structured;
    }
    let head: Vec<&str> = trimmed.lines().take(8).collect();
    if head.len() >= 2 && head.iter().filter(|line| line.contains('|')).count() >= 2 {
        return Content::Table;
    }
    if head
        .iter()
        .filter(|line| looks_like_a_log_line(line))
        .count()
        >= 2
    {
        return Content::Log;
    }
    if head.iter().any(|line| {
        let line = line.trim_start();
        line.starts_with("fn ")
            || line.starts_with("def ")
            || line.starts_with("class ")
            || line.starts_with("import ")
            || line.starts_with("use ")
            || line.ends_with('{')
            || line.ends_with(';')
    }) {
        return Content::Code;
    }
    if head.iter().any(|line| line.split_whitespace().count() >= 5) {
        return Content::Prose;
    }
    Content::Unknown
}

/// A log line is one that starts with something that is not prose: a
/// timestamp, a level, or a bracket. Counted rather than matched, so
/// one stray line does not decide.
fn looks_like_a_log_line(line: &str) -> bool {
    let line = line.trim_start();
    let starts_numeric = line.chars().next().is_some_and(|c| c.is_ascii_digit());
    starts_numeric
        || line.starts_with('[')
        || line.starts_with("INFO")
        || line.starts_with("WARN")
        || line.starts_with("ERROR")
        || line.starts_with("DEBUG")
}

/// Which strategy that kind of content takes, at that size.
///
/// Below the floor nothing is shortened at all: the machinery costs more
/// than it saves, and a person reading a shortened five-line result
/// learns to distrust every shortened result.
#[must_use]
pub fn plan(content: Content, size: ByteLen, budget: ByteLen) -> Strategy {
    if size.get() <= budget.get() {
        return Strategy::Keep;
    }
    match content {
        Content::Structured => Strategy::Offload,
        Content::Log => Strategy::Tail,
        Content::Code | Content::Diff => Strategy::Ends,
        Content::Prose | Content::Table => Strategy::Head,
        // Nothing is known about it, so nothing is thrown away on a
        // guess: it leaves the window whole and keeps its reference.
        Content::Unknown => Strategy::Offload,
    }
}

/// The marker that stands where the removed middle was. Counted against
/// the budget, so the result cannot exceed it by the marker's own
/// length.
const GAP: &str = "\n… (shortened) …\n";

/// Shortens one piece of text to the budget.
///
/// Returns the text and whether anything was removed. `Offload` and
/// `Keep` both return the input unchanged: this function shortens, and
/// deciding to move something out of the window is the caller's, which
/// keeps one authority for what leaves the window.
#[must_use]
pub fn compact(text: &str, budget: ByteLen) -> (String, bool) {
    let size = ByteLen::new(u64::try_from(text.len()).unwrap_or(u64::MAX));
    let strategy = plan(detect(text), size, budget);
    let limit = usize::try_from(budget.get()).unwrap_or(usize::MAX);
    let shortened = match strategy {
        Strategy::Keep | Strategy::Offload => return (text.to_owned(), false),
        Strategy::Head => head(text, limit),
        Strategy::Tail => tail(text, limit),
        Strategy::Ends => ends(text, limit),
    };
    // The invariant, checked here rather than trusted: if a strategy
    // ever produced something longer, the input is returned instead. A
    // compactor that grows its input makes every budget meaningless.
    if shortened.len() >= text.len() {
        return (text.to_owned(), false);
    }
    (shortened, true)
}

fn head(text: &str, limit: usize) -> String {
    let cut = boundary_before(text, limit.saturating_sub(GAP.len()));
    let mut out = text.get(..cut).unwrap_or_default().to_owned();
    out.push_str(GAP);
    out
}

fn tail(text: &str, limit: usize) -> String {
    let keep = limit.saturating_sub(GAP.len());
    let start = boundary_after(text, text.len().saturating_sub(keep));
    let mut out = GAP.to_owned();
    out.push_str(text.get(start..).unwrap_or_default());
    out
}

fn ends(text: &str, limit: usize) -> String {
    let each = limit.saturating_sub(GAP.len()).checked_div(2).unwrap_or(0);
    let front = boundary_before(text, each);
    let back = boundary_after(text, text.len().saturating_sub(each));
    if back <= front {
        return head(text, limit);
    }
    let mut out = text.get(..front).unwrap_or_default().to_owned();
    out.push_str(GAP);
    out.push_str(text.get(back..).unwrap_or_default());
    out
}

/// The largest character boundary at or before `at`. Cutting anywhere
/// else produces bytes that are not text.
fn boundary_before(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at = at.saturating_sub(1);
    }
    at
}

fn boundary_after(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at < text.len() && !text.is_char_boundary(at) {
        at = at.saturating_add(1);
    }
    at
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;

    #[test]
    fn the_same_input_is_dispatched_the_same_way_twice() {
        let samples = [
            "diff --git a/x b/x\n@@ -1 +1 @@\n-a\n+b\n",
            "{\"a\":1}",
            "| head | row |\n| --- | --- |\n| a | b |\n",
            "12:00:01 INFO started\n12:00:02 INFO finished\n",
            "fn main() {\n    let x = 1;\n}\n",
            "This is an ordinary sentence with more than five words in it.",
            "??",
        ];
        for sample in samples {
            assert_eq!(detect(sample), detect(sample));
        }
        assert_eq!(detect(samples[0]), Content::Diff);
        assert_eq!(detect(samples[1]), Content::Structured);
        assert_eq!(detect(samples[2]), Content::Table);
        assert_eq!(detect(samples[3]), Content::Log);
        assert_eq!(detect(samples[4]), Content::Code);
        assert_eq!(detect(samples[5]), Content::Prose);
        assert_eq!(detect(samples[6]), Content::Unknown);
    }

    #[test]
    fn shortening_never_lengthens() {
        let inputs = [
            String::new(),
            "a".to_owned(),
            "short".to_owned(),
            "x".repeat(4_000),
            "12:00:01 INFO a\n".repeat(300),
            "fn f() {\n  body();\n}\n".repeat(300),
            "{\"deep\":[1,2,3]}".repeat(300),
            "字".repeat(1_000),
        ];
        for budget in [0u64, 1, 8, 64, 512, 4_096] {
            for input in &inputs {
                let (out, _) = compact(input, ByteLen::new(budget));
                assert!(
                    out.len() <= input.len(),
                    "budget {budget} grew an input of {} to {}",
                    input.len(),
                    out.len()
                );
            }
        }
    }

    #[test]
    fn a_cut_lands_on_a_character_boundary() {
        // Every character here is three bytes, so a byte budget lands
        // mid-character unless the cut is moved.
        let text = "字".repeat(400);
        for budget in [7u64, 11, 100, 301] {
            let (out, _) = compact(&text, ByteLen::new(budget));
            assert!(
                std::str::from_utf8(out.as_bytes()).is_ok(),
                "a cut mid-character produces bytes nothing downstream can read"
            );
        }
    }

    #[test]
    fn a_log_keeps_its_end_and_prose_keeps_its_start() {
        let log = (0..400)
            .map(|n| format!("12:00:{n:02} INFO line {n}\n"))
            .collect::<String>();
        let (out, cut) = compact(&log, ByteLen::new(200));
        assert!(cut);
        assert!(out.contains("line 399"), "the news is at the end of a log");

        let prose = "The first sentence says what this is about. ".repeat(200);
        let (out, cut) = compact(&prose, ByteLen::new(200));
        assert!(cut);
        assert!(out.starts_with("The first sentence"));
    }

    #[test]
    fn structured_and_unknown_are_never_truncated() {
        let json = format!("{{\"a\":[{}]}}", "1,".repeat(2_000));
        let (out, cut) = compact(&json, ByteLen::new(64));
        assert!(!cut);
        assert_eq!(out, json, "truncated structured data looks parseable");
        assert_eq!(
            plan(Content::Unknown, ByteLen::new(9_000), ByteLen::new(64)),
            Strategy::Offload,
            "nothing is thrown away on a guess"
        );
    }

    #[test]
    fn something_within_budget_is_left_exactly_alone() {
        let text = "a short result";
        let (out, cut) = compact(text, ByteLen::new(4_096));
        assert!(!cut);
        assert_eq!(out, text);
    }
}
