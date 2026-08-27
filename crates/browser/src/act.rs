// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Turning an intention into frames.
//!
//! Every action names a reference from the snapshot the caller is
//! looking at, so acting on a page nobody looked at is not expressible.
//! The generation travels with the action for the same reason: a click
//! decided against one view of a page is refused against another rather
//! than landing on whatever moved into that position.

use crate::port::Frame;
use crate::session::{ContextId, Session};
use crate::snapshot::PageSnapshot;
use kernel::{AxCode, AxError};

/// What a run wants done. Exhaustive: an action this crate cannot spell
/// should be a compile error at the caller, not a string that reaches a
/// page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Click { reference: String },
    Type { reference: String, text: String },
    Read { reference: String },
}

impl Action {
    #[must_use]
    pub fn reference(&self) -> &str {
        match self {
            Action::Click { reference }
            | Action::Type { reference, .. }
            | Action::Read { reference } => reference,
        }
    }
}

/// Escapes a string for a JavaScript single-quoted literal. Page text is
/// somebody else's, and the one place it becomes code is here.
fn quote(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().saturating_add(2));
    out.push('\'');
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

/// Builds the frame that performs `action` against the page `snapshot`
/// describes.
///
/// # Errors
/// Refuses a reference the snapshot did not mint, and one minted against
/// a different generation — the second is the stale-page case, and it is
/// refused rather than retried because the caller has to look again to
/// know what it is now clicking.
pub fn frame_for(
    session: &mut Session,
    context: &ContextId,
    snapshot: &PageSnapshot,
    generation: u64,
    action: &Action,
) -> Result<Frame, AxError> {
    if generation != snapshot.generation() {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "act on a page",
            format!(
                "the action was decided against snapshot {generation}, the page is at {}",
                snapshot.generation()
            ),
        )
        .with_recovery("take a fresh snapshot and decide again"));
    }
    let node = snapshot.resolve(action.reference())?;
    // The reference is an index into what the snapshot showed, so the
    // expression re-derives it the same way the snapshot did. Role and
    // name go into the expression as data, never as code.
    let selector = format!(
        "[...document.querySelectorAll('*')].filter(e => (e.getAttribute('role') || \
         e.tagName.toLowerCase()) === {})[{}]",
        quote(&node.role),
        index_of(action.reference())?
    );
    let expression = match action {
        Action::Click { .. } => format!("{selector}.click()"),
        Action::Type { text, .. } => {
            format!(
                "(el => {{ el.value = {}; el.dispatchEvent(new Event('input', {{ bubbles: true }})); }})({selector})",
                quote(text)
            )
        }
        Action::Read { .. } => format!("({selector}).textContent"),
    };
    session.evaluate(context, &expression)
}

/// The zero-based position a reference names. References are minted as
/// `e1`, `e2`, … by the snapshot, so this is the inverse of that.
fn index_of(reference: &str) -> Result<usize, AxError> {
    let digits = reference.strip_prefix('e').ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "read a page reference",
            reference.to_owned(),
        )
        .with_recovery("references are minted by the snapshot and look like `e1`")
    })?;
    let ordinal: usize = digits.parse().map_err(|_| {
        AxError::failure(
            AxCode::InvalidArgs,
            "read a page reference",
            reference.to_owned(),
        )
        .with_recovery("references are minted by the snapshot and look like `e1`")
    })?;
    ordinal.checked_sub(1).ok_or_else(|| {
        AxError::failure(AxCode::InvalidArgs, "read a page reference", "e0")
            .with_recovery("references start at e1")
    })
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
    use serde_json::{Value, json};

    fn page(generation: u64) -> PageSnapshot {
        PageSnapshot::read(
            generation,
            &json!([
                { "role": "button", "name": "Place order" },
                { "role": "textbox", "name": "Quantity" },
            ]),
        )
        .unwrap()
    }

    fn expression(frame: &Frame) -> String {
        frame
            .params()
            .get("expression")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    }

    #[test]
    fn a_click_names_the_node_the_snapshot_showed() {
        let mut session = Session::new();
        let context = ContextId::parse("c1").unwrap();
        let frame = frame_for(
            &mut session,
            &context,
            &page(1),
            1,
            &Action::Click {
                reference: "e1".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(frame.method(), "script.evaluate");
        assert!(expression(&frame).contains("'button'"));
        assert!(expression(&frame).ends_with(".click()"));
    }

    #[test]
    fn an_action_decided_against_an_older_page_is_refused_not_retried() {
        let mut session = Session::new();
        let context = ContextId::parse("c1").unwrap();
        let err = frame_for(
            &mut session,
            &context,
            &page(4),
            3,
            &Action::Click {
                reference: "e1".to_owned(),
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(err.recovery().contains("fresh snapshot"));
    }

    #[test]
    fn text_a_page_could_choose_never_becomes_code() {
        let mut session = Session::new();
        let context = ContextId::parse("c1").unwrap();
        let frame = frame_for(
            &mut session,
            &context,
            &page(1),
            1,
            &Action::Type {
                reference: "e2".to_owned(),
                text: "'); fetch('https://elsewhere.test'); ('".to_owned(),
            },
        )
        .unwrap();
        let script = expression(&frame);
        assert!(
            script.contains("\\'"),
            "the quote that would close the literal carries a backslash: {script}"
        );
        // The page's text sits inside exactly one literal: two
        // delimiters, and every quote between them escaped. Counting is
        // the assertion because "looks escaped" is not a property.
        let after = script.split("el.value = ").nth(1).unwrap();
        let literal = after.split("; el.dispatchEvent").next().unwrap();
        let mut bare = 0usize;
        let mut escaped = false;
        for ch in literal.chars() {
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '\'') => bare += 1,
                (false, _) => {}
            }
        }
        assert_eq!(
            bare, 2,
            "only the delimiters may be unescaped quotes: {literal}"
        );
    }

    #[test]
    fn a_reference_that_was_never_minted_is_refused_by_name() {
        let mut session = Session::new();
        let context = ContextId::parse("c1").unwrap();
        for reference in ["e9", "button", "e0", "e"] {
            assert!(
                frame_for(
                    &mut session,
                    &context,
                    &page(1),
                    1,
                    &Action::Read {
                        reference: reference.to_owned()
                    },
                )
                .is_err(),
                "{reference}"
            );
        }
    }
}
