// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Reading a document as spans, so the interface can show its shape.
//!
//! The one place this city puts a file on screen is a building's own
//! pages, and every one of them is Markdown: `BUILDING.md` and whatever
//! `*.md` sits beside it. Those are what an agent writes for the next
//! agent and what a person reads to find out what happened, and they
//! arrived as one flat wall of `<pre>`.
//!
//! **Spans, not markup.** This returns where things are and what they
//! are; it never rewrites the text. A highlighter that emitted markup
//! would be deciding presentation in the crate that is not allowed to
//! know about presentation, and the interface could no longer choose to
//! show the same document as plain bytes.
//!
//! **No grammar engine here.** Inside a fence the whole block is one
//! `Code` span, because reading `**x**` in a line of Rust as bold text
//! would be this module inventing a fact. When a real grammar is worth
//! its dependency it goes to the server and the wire grows then; the
//! seam is this function's signature.

use serde::{Deserialize, Serialize};

/// What a stretch of a document is.
///
/// Nine, and closed: every arm is something a reader can act on in a
/// document an agent wrote. A tenth needs a reason on screen, not just a
/// pattern in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Token {
    /// A `#` line, hashes included.
    Heading,
    /// `**strong**`.
    Strong,
    /// `*emphasis*` or `_emphasis_`.
    Emphasis,
    /// A `` `span` ``, or everything inside a fence.
    Code,
    /// The ``` line that opens or closes a fence.
    Fence,
    /// The language written after an opening fence.
    Meta,
    /// The `[text](target)` of a link, whole.
    Link,
    /// A list bullet or a numbered marker, at the start of its line.
    Marker,
    /// A `>` quote marker.
    Quote,
}

/// Where one token sits, in bytes from the start of the document.
///
/// Bytes rather than characters because the caller slices the same
/// `&str`, and both ends are always on a character boundary — a document
/// in Chinese would otherwise take the interface down on its first
/// heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub len: u32,
    pub token: Token,
}

/// The fence delimiter, in both spellings a document may use.
const FENCES: [&str; 2] = ["```", "~~~"];

/// Reads a Markdown document as spans, in order and never overlapping.
///
/// Ordered by `start` and disjoint, so the caller walks them once beside
/// the text and never has to decide which of two claims on the same byte
/// wins — that decision is a lexical rule and it belongs here.
#[must_use]
pub fn markdown(text: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut fenced: Option<usize> = None;
    let mut at: usize = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let indent = trimmed.len().saturating_sub(trimmed.trim_start().len());
        let body = trimmed.trim_start();
        match (fenced, opens_fence(body)) {
            // Closing a fence: the block between the two delimiters is
            // one Code span, and the delimiter line is its own.
            (Some(opened), true) => {
                push(&mut spans, opened, at.saturating_sub(opened), Token::Code);
                push(
                    &mut spans,
                    at.saturating_add(indent),
                    body.len(),
                    Token::Fence,
                );
                fenced = None;
            }
            // Opening one: the delimiter, then the language if it says.
            (None, true) => {
                let start = at.saturating_add(indent);
                let mark = fence_len(body);
                push(&mut spans, start, mark, Token::Fence);
                let info = body.get(mark..).unwrap_or_default();
                let lead = info.len().saturating_sub(info.trim_start().len());
                let named = info.trim();
                if !named.is_empty() {
                    push(
                        &mut spans,
                        start.saturating_add(mark).saturating_add(lead),
                        named.len(),
                        Token::Meta,
                    );
                }
                fenced = Some(at.saturating_add(line.len()));
            }
            // Inside a fence nothing is read: the block is code, and the
            // span for it is pushed when the fence closes.
            (Some(_), false) => {}
            (None, false) => read_line(&mut spans, at, line, trimmed),
        }
        at = at.saturating_add(line.len());
    }
    // A fence nobody closed still ends somewhere, and the text after it
    // is still code. Dropping it would leave the tail of a truncated
    // document rendered as prose.
    if let Some(opened) = fenced {
        push(&mut spans, opened, at.saturating_sub(opened), Token::Code);
    }
    spans.sort_by_key(|span| span.start);
    spans
}

/// One line outside any fence.
fn read_line(spans: &mut Vec<Span>, at: usize, line: &str, trimmed: &str) {
    let indent = trimmed.len().saturating_sub(trimmed.trim_start().len());
    let body = trimmed.trim_start();
    let start = at.saturating_add(indent);
    if body.starts_with('#') {
        push(spans, start, body.len(), Token::Heading);
        return;
    }
    if let Some(rest) = body.strip_prefix('>') {
        push(
            spans,
            start,
            body.len().saturating_sub(rest.len()),
            Token::Quote,
        );
        inline(spans, start.saturating_add(1), rest);
        return;
    }
    if let Some(mark) = marker_len(body) {
        push(spans, start, mark, Token::Marker);
        inline(
            spans,
            start.saturating_add(mark),
            body.get(mark..).unwrap_or_default(),
        );
        return;
    }
    let _ = line;
    inline(spans, start, body);
}

/// How long the list marker at the front of `body` is, when there is one.
///
/// A bullet needs the space after it: `*text*` opens emphasis and `* text`
/// opens a list, and reading the first as a bullet would eat the star a
/// reader meant as punctuation.
fn marker_len(body: &str) -> Option<usize> {
    for bullet in ['-', '*', '+'] {
        if body.starts_with(bullet) && body.get(1..2) == Some(" ") {
            return Some(2);
        }
    }
    let digits = body
        .char_indices()
        .take_while(|&(_, ch)| ch.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    let after = body.get(digits..)?;
    if after.starts_with(". ") || after.starts_with(") ") {
        return Some(digits.saturating_add(2));
    }
    None
}

/// Whether this line is a fence delimiter.
fn opens_fence(body: &str) -> bool {
    FENCES.iter().any(|mark| body.starts_with(mark))
}

/// How many bytes of the delimiter this line spells.
fn fence_len(body: &str) -> usize {
    FENCES
        .iter()
        .find(|mark| body.starts_with(**mark))
        .map_or(0, |mark| mark.len())
}

/// The spans inside one line of prose.
///
/// Code first, because a backtick outranks the rest: `` `**x**` `` is a
/// code span containing stars, not bold text inside code. That order is
/// the whole of the precedence this module has, and it lives here rather
/// than in whatever draws the result.
fn inline(spans: &mut Vec<Span>, at: usize, body: &str) {
    let mut taken: Vec<(usize, usize)> = Vec::new();
    scan(spans, &mut taken, at, body, "`", Token::Code);
    scan(spans, &mut taken, at, body, "**", Token::Strong);
    scan(spans, &mut taken, at, body, "*", Token::Emphasis);
    scan(spans, &mut taken, at, body, "_", Token::Emphasis);
    links(spans, &mut taken, at, body);
}

/// Finds every `mark … mark` pair not already claimed.
fn scan(
    spans: &mut Vec<Span>,
    taken: &mut Vec<(usize, usize)>,
    at: usize,
    body: &str,
    mark: &str,
    token: Token,
) {
    let mut from = 0usize;
    while let Some(open) = body.get(from..).and_then(|rest| rest.find(mark)) {
        let start = from.saturating_add(open);
        let after = start.saturating_add(mark.len());
        let Some(close) = body.get(after..).and_then(|rest| rest.find(mark)) else {
            return;
        };
        let end = after.saturating_add(close).saturating_add(mark.len());
        // An empty pair is punctuation somebody typed, not a span.
        if close > 0 && claim(taken, start, end) {
            push(
                spans,
                at.saturating_add(start),
                end.saturating_sub(start),
                token,
            );
        }
        from = end;
    }
}

/// Finds `[text](target)` pairs not already claimed.
fn links(spans: &mut Vec<Span>, taken: &mut Vec<(usize, usize)>, at: usize, body: &str) {
    let mut from = 0usize;
    while let Some(open) = body.get(from..).and_then(|rest| rest.find('[')) {
        let start = from.saturating_add(open);
        let Some(shut) = body.get(start..).and_then(|rest| rest.find("](")) else {
            return;
        };
        let target = start.saturating_add(shut).saturating_add(2);
        let Some(end) = body.get(target..).and_then(|rest| rest.find(')')) else {
            return;
        };
        let stop = target.saturating_add(end).saturating_add(1);
        if claim(taken, start, stop) {
            push(
                spans,
                at.saturating_add(start),
                stop.saturating_sub(start),
                Token::Link,
            );
        }
        from = stop;
    }
}

/// Takes a stretch, or reports that something already has it.
fn claim(taken: &mut Vec<(usize, usize)>, start: usize, end: usize) -> bool {
    if taken.iter().any(|&(from, to)| start < to && from < end) {
        return false;
    }
    taken.push((start, end));
    true
}

/// Records one span, dropping a length that will not fit the wire's
/// width rather than truncating it into a slice that ends mid-character.
fn push(spans: &mut Vec<Span>, start: usize, len: usize, token: Token) {
    if len == 0 {
        return;
    }
    let (Ok(start), Ok(len)) = (u32::try_from(start), u32::try_from(len)) else {
        return;
    };
    spans.push(Span { start, len, token });
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

    fn cut<'a>(text: &'a str, span: &Span) -> &'a str {
        let start = usize::try_from(span.start).unwrap();
        let len = usize::try_from(span.len).unwrap();
        text.get(start..start + len)
            .expect("a span slices its text")
    }

    fn tokens(text: &str) -> Vec<(Token, &str)> {
        markdown(text)
            .iter()
            .map(|span| (span.token, cut(text, span)))
            .collect()
    }

    #[test]
    fn a_heading_is_read_whole_including_its_hashes() {
        assert_eq!(
            tokens("# What happened\n"),
            vec![(Token::Heading, "# What happened")]
        );
    }

    #[test]
    fn a_fence_names_its_language_and_holds_its_block_as_code() {
        let doc = "before\n```rust\nlet x = 1;\nlet y = 2;\n```\nafter\n";
        let read = tokens(doc);
        assert_eq!(read[0], (Token::Fence, "```"));
        assert_eq!(read[1], (Token::Meta, "rust"));
        assert_eq!(read[2], (Token::Code, "let x = 1;\nlet y = 2;\n"));
        assert_eq!(read[3], (Token::Fence, "```"));
    }

    /// This build has no grammar engine, so reading `**x**` inside a
    /// block of Rust as bold text would be inventing a fact.
    #[test]
    fn nothing_inside_a_fence_is_read_as_prose() {
        let doc = "```rust\nlet a = b ** c; // *not* emphasis\n```\n";
        let read = tokens(doc);
        assert!(
            read.iter()
                .all(|(token, _)| matches!(token, Token::Fence | Token::Meta | Token::Code)),
            "{read:?}"
        );
    }

    /// A truncated document still ends somewhere, and its tail is still
    /// code. `BuildingDoc` carries a `truncated` flag, so this arrives.
    #[test]
    fn a_fence_nobody_closed_still_holds_the_rest_of_the_document() {
        let doc = "```\nhalf a program\n";
        let read = tokens(doc);
        assert_eq!(read[0].0, Token::Fence);
        assert_eq!(read[1], (Token::Code, "half a program\n"));
    }

    #[test]
    fn a_bullet_is_a_marker_and_a_star_around_a_word_is_not() {
        assert_eq!(
            tokens("- *ready*\n"),
            vec![(Token::Marker, "- "), (Token::Emphasis, "*ready*")]
        );
        assert_eq!(tokens("*ready*\n"), vec![(Token::Emphasis, "*ready*")]);
    }

    /// A backtick outranks a star: a code span holding stars is code, not
    /// bold text inside code.
    #[test]
    fn a_code_span_wins_the_bytes_it_covers() {
        assert_eq!(
            tokens("call `a ** b` twice\n"),
            vec![(Token::Code, "`a ** b`")]
        );
    }

    #[test]
    fn strong_is_not_read_as_two_emphases() {
        assert_eq!(tokens("**loud**\n"), vec![(Token::Strong, "**loud**")]);
    }

    #[test]
    fn a_link_is_read_whole_so_a_reader_can_see_where_it_goes() {
        assert_eq!(
            tokens("see [the plan](lab/Roadmap.md) first\n"),
            vec![(Token::Link, "[the plan](lab/Roadmap.md)")]
        );
    }

    #[test]
    fn a_quote_marker_is_its_own_span_and_its_line_is_still_read() {
        assert_eq!(
            tokens("> **note**\n"),
            vec![(Token::Quote, ">"), (Token::Strong, "**note**")]
        );
    }

    /// The property the interface depends on: every span slices, and no
    /// two claim the same byte. A page walking them beside the text would
    /// otherwise have to decide which claim wins, which is a lexical rule
    /// leaking into a view.
    #[test]
    fn every_span_slices_its_text_and_none_of_them_overlap() {
        let doc = "# 标题\n\n段落里有 `代码` 与 **重点**，还有 [链接](到/别处.md)。\n\n\
                   - 第一条 *强调*\n- 第二条\n\n```rust\nlet 值 = 1;\n```\n\n> 引用一句\n";
        let spans = markdown(doc);
        assert!(!spans.is_empty());
        let mut end = 0u32;
        for span in &spans {
            assert!(
                span.start >= end,
                "spans overlap at {}: {spans:?}",
                span.start
            );
            // Panics on a boundary that is not a character boundary,
            // which is the failure a Chinese document would produce.
            let _ = cut(doc, span);
            end = span.start.saturating_add(span.len);
        }
    }

    #[test]
    fn an_empty_document_reads_as_nothing_rather_than_as_one_empty_span() {
        assert!(markdown("").is_empty());
        assert!(markdown("\n\n").is_empty());
    }

    /// Punctuation somebody typed is not a span with no content.
    #[test]
    fn an_empty_pair_of_marks_is_not_a_span() {
        assert!(markdown("nothing `` here\n").is_empty());
    }
}
