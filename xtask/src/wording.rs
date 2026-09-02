// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Wording gate: a word a reader is given comes from the phrase table
//! (web-SPEC.md section 8-61).
//!
//! **The two assertions in `web::lang` both read what a view *asks* the
//! table for.** One walks every phrase and demands some view name it;
//! the other forbids a view from spelling out a sentence the table
//! already holds. A sentence that never calls `say` is in neither
//! field of view: the table does not know it exists, so there is
//! nothing to compare it against. Three English sentences lived on the
//! cost page for a whole stage that way, and what found them was a
//! photograph of the running client.
//!
//! **This gate reads the other direction: what a view *says* by
//! itself.** It is the half only a gate can do, for the same reason
//! `color`'s literal scan is - it is a statement about every file
//! rather than about one table.
//!
//! **The predicate is a position, not a vocabulary.** A first cut that
//! scanned every string literal would judge class names, wire values,
//! event kinds and format keys, and the same shape of mistake once
//! produced 79 findings that were all addresses. So this walks the RSX
//! brace structure and keeps two positions, both of which are a reader
//! being handed a word:
//!
//! 1. a **text node** - a literal standing on its own in an element
//!    body, which is what the browser paints;
//! 2. the value of a **spoken attribute** - `placeholder`, `title`,
//!    `alt` and the `aria-*` names whose value is author-supplied text,
//!    which is what a screen reader reads out.
//!
//! Everything else is excluded by where it sits rather than by a list
//! of exceptions: an attribute value is not a text node, a call
//! argument is not in an element body, a match arm is not content.
//!
//! **What remains after the city's own values are removed is the
//! question.** `"{percent}%"`, `"+{added}"` and `"{room}/"` hand the
//! reader nothing the view wrote; `"{count} waiting"` hands them an
//! English word. So the slots come out and the gate asks whether two
//! adjacent letters are left.
//!
//! **A proper noun cannot go in the table**, because `web::lang`'s own
//! test refuses a phrase whose two languages are equal, and `openai` is
//! `openai` in both. Those sites carry `wording-ok: <reason>` on the
//! line or the line above - the same mark, the same two-line rule and
//! the same trade as `lexicon-ok:`.

use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

/// Where the client a reader reads lives. Nothing else in the tree
/// draws, and English in a wire value or an error code is correct.
const CLIENT: &str = "crates/web/src";

/// The waiver, spelled as `lexicon`'s is.
const EXEMPT_MARK: &str = "wording-ok:";

/// Attributes whose value is author-supplied text that a person is
/// shown or read out. A closed vocabulary from HTML and ARIA, not a
/// list of the ones this tree happens to use today: the day someone
/// writes `alt`, the rule already covers it.
const SPOKEN: [&str; 8] = [
    "placeholder",
    "title",
    "alt",
    "aria-label",
    "aria-description",
    "aria-placeholder",
    "aria-roledescription",
    "aria-valuetext",
];

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let dir = root.join(CLIENT);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut violations = Vec::new();
    for path in walk::files_with_ext(&dir, &["rs"])? {
        let location = walk::rel(root, &path);
        let text = walk::read_text(&path)?;
        let lines: Vec<&str> = text.lines().collect();
        for said in handed_to_a_reader(drawn(&text)) {
            if waived(&lines, said.line) {
                continue;
            }
            violations.push(Violation {
                gate: "wording",
                location: format!("{location}:{}", said.line),
                rule: "a word a reader is given comes from web::lang, not from the view".to_owned(),
                violation: format!(
                    "{} carries {:?}, which no phrase produced",
                    said.seat,
                    clipped(&said.left)
                ),
                alternative: format!(
                    "add a Msg with both languages and fill its named slots; or, for a name that \
                     is the same word in both, justify inline with `{EXEMPT_MARK} <reason>`"
                ),
            });
        }
    }
    Ok(violations)
}

/// The part of a module that draws, which is everything above its own
/// test module. The same cut `web::lang` makes, for the same reason: a
/// sentence quoted by a test to assert that a page says it is evidence,
/// not a second authority for the wording.
fn drawn(body: &str) -> &str {
    match body.find("#[cfg(test)]") {
        Some(at) => body.get(..at).unwrap_or(body),
        None => body,
    }
}

/// A waiver on the line itself or the line above it.
fn waived(lines: &[&str], line: usize) -> bool {
    let here = line.checked_sub(1).and_then(|at| lines.get(at));
    let above = line.checked_sub(2).and_then(|at| lines.get(at));
    [here, above]
        .into_iter()
        .flatten()
        .any(|text| text.contains(EXEMPT_MARK))
}

/// The first 60 characters, so a report of forty findings stays
/// readable and a wall of markup never becomes the message.
fn clipped(text: &str) -> String {
    let mut out: String = text.chars().take(60).collect();
    if out.chars().count() < text.chars().count() {
        out.push('…');
    }
    out
}

/// One literal a reader is handed, and the words left in it.
struct Said {
    line: usize,
    seat: &'static str,
    left: String,
}

// ---------------------------------------------------------------- lexing

/// A token, reduced to the seven distinctions the brace walk needs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A string literal.
    Text,
    /// An identifier, including `if`, `for` and `match`.
    Name,
    /// `{`, the only delimiter that can open an element body.
    Brace,
    /// `(` or `[`, which never can.
    Bracket,
    /// `}`, `)` or `]`.
    Close,
    /// `,` or `;`: one item ends and the next begins.
    Break,
    /// A single `:`, which makes the item before it an attribute name.
    Colon,
    /// Everything else, `::` and `=>` included.
    Other,
}

struct Lexeme {
    kind: Kind,
    /// The identifier, or a string literal without its quotes.
    body: String,
    line: usize,
}

fn lex(src: &str) -> Vec<Lexeme> {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<Lexeme> = Vec::new();
    let mut at = 0_usize;
    let mut line = 1_usize;
    while let Some(&here) = chars.get(at) {
        let next = chars.get(at.saturating_add(1)).copied();
        if here == '\n' {
            line = line.saturating_add(1);
            at = at.saturating_add(1);
        } else if here.is_whitespace() {
            at = at.saturating_add(1);
        } else if here == '/' && next == Some('/') {
            at = skip_to(&chars, at, '\n');
        } else if here == '/' && next == Some('*') {
            let (to, crossed) = skip_block_comment(&chars, at);
            line = line.saturating_add(crossed);
            at = to;
        } else if here == '\'' {
            at = skip_char_literal(&chars, at);
        } else if let Some((body, to, crossed)) = read_string(&chars, at) {
            out.push(Lexeme {
                kind: Kind::Text,
                body,
                line,
            });
            line = line.saturating_add(crossed);
            at = to;
        } else if here.is_alphabetic() || here == '_' {
            let (body, to) = read_name(&chars, at);
            out.push(Lexeme {
                kind: Kind::Name,
                body,
                line,
            });
            at = to;
        } else {
            let (kind, width) = punctuation(here, next);
            out.push(Lexeme {
                kind,
                body: String::new(),
                line,
            });
            at = at.saturating_add(width);
        }
    }
    out
}

fn punctuation(here: char, next: Option<char>) -> (Kind, usize) {
    match (here, next) {
        (':', Some(':')) => (Kind::Other, 2),
        ('=', Some('>')) => (Kind::Other, 2),
        (':', _) => (Kind::Colon, 1),
        ('{', _) => (Kind::Brace, 1),
        ('(' | '[', _) => (Kind::Bracket, 1),
        ('}' | ')' | ']', _) => (Kind::Close, 1),
        (',' | ';', _) => (Kind::Break, 1),
        _ => (Kind::Other, 1),
    }
}

fn skip_to(chars: &[char], from: usize, stop: char) -> usize {
    let mut at = from;
    while let Some(&here) = chars.get(at) {
        if here == stop {
            return at;
        }
        at = at.saturating_add(1);
    }
    at
}

fn skip_block_comment(chars: &[char], from: usize) -> (usize, usize) {
    let mut at = from.saturating_add(2);
    let mut crossed = 0_usize;
    while let Some(&here) = chars.get(at) {
        if here == '\n' {
            crossed = crossed.saturating_add(1);
        }
        if here == '*' && chars.get(at.saturating_add(1)) == Some(&'/') {
            return (at.saturating_add(2), crossed);
        }
        at = at.saturating_add(1);
    }
    (at, crossed)
}

/// `'a'` and `'\n'` are consumed whole; `'static` is left for the name
/// reader, which spells it as an ordinary identifier and is right to.
fn skip_char_literal(chars: &[char], from: usize) -> usize {
    let escaped = chars.get(from.saturating_add(1)) == Some(&'\\');
    let closes_at_two = chars.get(from.saturating_add(2)) == Some(&'\'');
    if !escaped && !closes_at_two {
        return from.saturating_add(1);
    }
    let mut at = from.saturating_add(1);
    while let Some(&here) = chars.get(at) {
        if here == '\\' {
            at = at.saturating_add(2);
            continue;
        }
        if here == '\'' {
            return at.saturating_add(1);
        }
        at = at.saturating_add(1);
    }
    at
}

/// A string literal starting at `from`, in any of its three spellings.
///
/// Returns its body, the index after it, and how many lines it crossed.
/// `None` when nothing starts here - which is what tells `r#type` from
/// `r#"..."#`: a raw identifier has no quote after its hashes.
fn read_string(chars: &[char], from: usize) -> Option<(String, usize, usize)> {
    let mut at = from;
    let mut hashes = 0_usize;
    if chars.get(at) == Some(&'b') || chars.get(at) == Some(&'r') {
        let raw = chars.get(at) == Some(&'r');
        at = at.saturating_add(1);
        if raw {
            while chars.get(at) == Some(&'#') {
                hashes = hashes.saturating_add(1);
                at = at.saturating_add(1);
            }
        }
        if chars.get(at) != Some(&'"') {
            return None;
        }
        if hashes > 0 {
            return read_raw(chars, at, hashes);
        }
    }
    if chars.get(at) != Some(&'"') {
        return None;
    }
    at = at.saturating_add(1);
    let mut body = String::new();
    let mut crossed = 0_usize;
    while let Some(&here) = chars.get(at) {
        if here == '\\' {
            body.push(here);
            if let Some(&escaped) = chars.get(at.saturating_add(1)) {
                body.push(escaped);
            }
            at = at.saturating_add(2);
            continue;
        }
        if here == '"' {
            return Some((body, at.saturating_add(1), crossed));
        }
        if here == '\n' {
            crossed = crossed.saturating_add(1);
        }
        body.push(here);
        at = at.saturating_add(1);
    }
    Some((body, at, crossed))
}

fn read_raw(chars: &[char], quote: usize, hashes: usize) -> Option<(String, usize, usize)> {
    let mut at = quote.saturating_add(1);
    let mut body = String::new();
    let mut crossed = 0_usize;
    while let Some(&here) = chars.get(at) {
        if here == '"' {
            let closed = (1..=hashes).all(|step| chars.get(at.saturating_add(step)) == Some(&'#'));
            if closed {
                return Some((body, at.saturating_add(hashes).saturating_add(1), crossed));
            }
        }
        if here == '\n' {
            crossed = crossed.saturating_add(1);
        }
        body.push(here);
        at = at.saturating_add(1);
    }
    Some((body, at, crossed))
}

fn read_name(chars: &[char], from: usize) -> (String, usize) {
    let mut at = from;
    let mut body = String::new();
    while let Some(&here) = chars.get(at) {
        if !(here.is_alphanumeric() || here == '_') {
            break;
        }
        body.push(here);
        at = at.saturating_add(1);
    }
    (body, at)
}

// ------------------------------------------------------------ the walk

/// One brace, and what its items mean.
struct Frame {
    /// Its items are RSX: attributes and content, not statements.
    element: bool,
    /// The item being read is `name: value`.
    attribute: bool,
    /// The next token opens a fresh item.
    fresh: bool,
    /// The token that opened the item: an identifier, or an attribute
    /// name in quotes. Empty when the item began with anything else.
    head: String,
    /// That token was an identifier rather than a string.
    head_is_name: bool,
}

impl Frame {
    fn plain() -> Frame {
        Frame {
            element: false,
            attribute: false,
            fresh: true,
            head: String::new(),
            head_is_name: false,
        }
    }

    fn element() -> Frame {
        Frame {
            element: true,
            ..Frame::plain()
        }
    }
}

/// Every literal that reaches a reader, with the words the view wrote.
fn handed_to_a_reader(src: &str) -> Vec<Said> {
    let toks = lex(src);
    let mut stack: Vec<Frame> = vec![Frame::plain()];
    let mut out = Vec::new();
    for (index, tok) in toks.iter().enumerate() {
        let follows = toks.get(index.saturating_add(1)).map(|next| next.kind);
        let Some(top) = stack.last_mut() else {
            continue;
        };
        let fresh = top.fresh;
        if fresh && tok.kind != Kind::Close {
            top.head = tok.body.clone();
            top.head_is_name = tok.kind == Kind::Name;
            top.fresh = false;
            top.attribute = top.element
                && follows == Some(Kind::Colon)
                && matches!(tok.kind, Kind::Name | Kind::Text);
        }
        if tok.kind == Kind::Text
            && let Some(seat) = seat_of(top, fresh)
            && let Some(left) = words_the_view_wrote(&tok.body)
        {
            out.push(Said {
                line: tok.line,
                seat,
                left,
            });
        }
        step(&mut stack, &toks, index);
    }
    out
}

/// Where this literal sits, when it sits somewhere a reader can see.
fn seat_of(top: &Frame, fresh: bool) -> Option<&'static str> {
    if top.element && !top.attribute && fresh {
        return Some("a text node");
    }
    if top.attribute && !fresh && SPOKEN.contains(&top.head.as_str()) {
        return Some("a spoken attribute");
    }
    None
}

/// Push and pop the brace stack for one token.
fn step(stack: &mut Vec<Frame>, toks: &[Lexeme], index: usize) {
    let Some(tok) = toks.get(index) else {
        return;
    };
    match tok.kind {
        Kind::Brace => {
            let element = opens_an_element_body(stack, toks, index);
            stack.push(if element {
                Frame::element()
            } else {
                Frame::plain()
            });
        }
        Kind::Bracket => stack.push(Frame::plain()),
        Kind::Close => {
            if stack.len() > 1 {
                stack.pop();
            }
            if let Some(top) = stack.last_mut() {
                top.fresh = false;
            }
        }
        Kind::Break => {
            if let Some(top) = stack.last_mut() {
                top.fresh = true;
                top.attribute = false;
                top.head.clear();
                top.head_is_name = false;
            }
        }
        _ => {}
    }
}

/// Whether the brace at `index` opens an element body.
///
/// Two ways in. `rsx!` opens one wherever it appears, which is how a
/// match arm gets back into RSX. Otherwise the item has to begin with
/// an identifier inside an element body that is not reading an
/// attribute: `div {`, `Panel {`, `if x {` and `for a in b {` all do.
/// `match m {` is refused by name - its items are patterns, and a
/// pattern that matches a string is not a reader being handed one.
fn opens_an_element_body(stack: &[Frame], toks: &[Lexeme], index: usize) -> bool {
    let bang = index
        .checked_sub(1)
        .and_then(|at| toks.get(at))
        .is_some_and(|tok| tok.kind == Kind::Other);
    let macro_name = index
        .checked_sub(2)
        .and_then(|at| toks.get(at))
        .is_some_and(|tok| tok.kind == Kind::Name && tok.body == "rsx");
    if bang && macro_name {
        return true;
    }
    stack
        .last()
        .is_some_and(|top| top.element && !top.attribute && top.head_is_name && top.head != "match")
}

// -------------------------------------------------------- the predicate

/// What a literal says once the city's own values are taken out of it,
/// or `None` when nothing of the view's own is left.
///
/// A slot carries a name, a number or an address the city produced; an
/// escape carries a character the view spelled in hexadecimal. Neither
/// is the view choosing a word. What survives both is judged by the
/// only test that matters here: two letters side by side.
fn words_the_view_wrote(literal: &str) -> Option<String> {
    let mut left = String::new();
    let mut chars = literal.chars().peekable();
    let mut depth = 0_usize;
    while let Some(here) = chars.next() {
        match here {
            '\\' => {
                if chars.next() == Some('u') && chars.peek() == Some(&'{') {
                    for inner in chars.by_ref() {
                        if inner == '}' {
                            break;
                        }
                    }
                }
            }
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => depth = depth.saturating_add(1),
            '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => left.push(here),
            _ => {}
        }
    }
    let mut run = 0_usize;
    for here in left.chars() {
        run = if here.is_ascii_alphabetic() {
            run.saturating_add(1)
        } else {
            0
        };
        if run >= 2 {
            return Some(left.trim().to_owned());
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn found(src: &str) -> Vec<String> {
        handed_to_a_reader(src)
            .into_iter()
            .map(|said| said.left)
            .collect()
    }

    /// The ablation this gate exists for: the three sentences V3.50
    /// took off the cost page, put back the way they were written.
    #[test]
    fn the_three_sentences_that_escaped_both_of_langs_assertions_are_caught() {
        let src = r##"
            fn view() -> Element {
                rsx! {
                    p { class: "consumed",
                        "{render_tokens(usage.input)} in, {render_tokens(usage.output)} out"
                    }
                    p { class: "unpriced",
                        "{usage.unpriced_calls} call(s) came back with no price."
                    }
                    p { class: "spent-line",
                        "{render_usd(spent)} of that arrived through this page's own stream"
                    }
                }
            }
        "##;
        let hits = found(src);
        assert_eq!(hits.len(), 3, "{hits:?}");
        assert!(hits[0].contains("in,"), "{hits:?}");
        assert!(hits[1].contains("came back with no price"), "{hits:?}");
        assert!(hits[2].contains("arrived through"), "{hits:?}");
    }

    /// The first cut hit 79 addresses. Each of these is the shape that
    /// made it wrong, and the position rule refuses all of them without
    /// naming a single one.
    #[test]
    fn class_names_wire_values_and_arguments_are_not_words_a_reader_was_given() {
        let src = r##"
            fn view() -> Element {
                let mode = pick("build the parser");
                rsx! {
                    div { class: "panel composer", id: "compose",
                        span { class: if hot { "phase alert" } else { "phase" } }
                        button { onclick: move |_| send(Command::Dispatch { goal: "ship it" }) }
                        input { r#type: "text", value: "{addr}", name: "room" }
                        "{word(Msg::DispatchSend)}"
                        "{percent(row.share)}%"
                        "+{added}"
                        "\u{2212}{removed}"
                        "{room}/"
                    }
                }
            }
        "##;
        assert!(found(src).is_empty(), "{:?}", found(src));
    }

    #[test]
    fn a_word_in_a_text_node_or_spoken_attribute_is_caught_either_way() {
        let text = r##"fn v() { rsx! { span { class: "count", "{n} waiting" } } }"##;
        assert_eq!(found(text), vec!["waiting".to_owned()]);
        let spoken = r##"fn v() { rsx! { button { "aria-label": "dismiss" } } }"##;
        assert_eq!(found(spoken), vec!["dismiss".to_owned()]);
        let obeyed = r##"fn v() { rsx! { button { "aria-current": "true" } } }"##;
        assert!(found(obeyed).is_empty());
    }

    /// A match on strings sits in the middle of RSX all over this
    /// client. Its arms are patterns; `rsx!` is how one gets back to
    /// being content, and the walk has to tell those apart.
    #[test]
    fn match_arms_are_patterns_until_rsx_says_otherwise() {
        let src = r##"
            fn v() -> Element {
                rsx! {
                    div {
                        match dialect {
                            "anthropic messages" => rsx! { span { "the wire spoke" } },
                            _ => rsx! { span { "{word(Msg::Unknown)}" } },
                        }
                    }
                }
            }
        "##;
        assert_eq!(found(src), vec!["the wire spoke".to_owned()]);
    }

    #[test]
    fn a_waiver_on_the_line_or_the_line_above_is_honoured() {
        let lines = [
            "a",
            "option { value: \"openai\", \"openai\" } // wording-ok: a name",
        ];
        assert!(waived(&lines, 2));
        let above = [
            "// wording-ok: the two dialects name themselves",
            "option {}",
        ];
        assert!(waived(&above, 2));
        assert!(!waived(&["plain", "plain"], 2));
    }

    /// Everything below a module's own `#[cfg(test)]` is evidence a
    /// test wrote down, not a page.
    #[test]
    fn a_sentence_a_test_quotes_is_not_a_sentence_a_page_says() {
        let src = "fn v() { rsx! { p { \"live text\" } } }\n#[cfg(test)]\nmod t { const S: &str = \"quoted evidence\"; }";
        assert_eq!(found(drawn(src)), vec!["live text".to_owned()]);
    }

    #[test]
    fn a_raw_identifier_is_not_a_raw_string() {
        let src = r##"fn v() { rsx! { input { r#type: "text", "a word here" } } }"##;
        assert_eq!(found(src), vec!["a word here".to_owned()]);
    }
}
