// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Accessibility gate: what a settled screen offers a screen reader, the
//! shipped client offers too (v0.0.3 card V3.15).
//!
//! **The drift this catches is the one that happened.** The four-step
//! method settles a screen in HTML, translates it, and then adds
//! bindings — and the rule for step three is that only text nodes and
//! control flow may change. A role, an `aria-label` or an `aria-current`
//! dropped during that step is invisible: the page still renders, the
//! pixels still match, and the only thing lost is what a person who
//! cannot see the pixels was going to be told. Nothing else in this
//! repository would notice.
//!
//! **This is not a computed accessibility tree, and it does not claim to
//! be.** A computed tree needs a browser, a browser needs a binary this
//! build does not ship, and a gate that cannot run offline is a gate
//! that stops running. What this compares is the *authored* affordances:
//! the roles, the accessible names, the current-page marks and the
//! landmark elements that a settled screen wrote down. Those are the
//! ones a translation loses. The computed tree is still worth checking
//! against a running client, and that is a person's job with a browser
//! open, not a gate's.
//!
//! Read from the source both ways, so neither side keeps a copy: the
//! screens are the design authority and `crates/web/src` is the build.

use std::collections::BTreeSet;
use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

/// Where the settled screens live.
const SCREENS: &str = "crates/web/screens";

/// Where the client that must match them lives.
const CLIENT: &str = "crates/web/src";

/// The attributes that carry an accessible affordance.
///
/// `role` and `aria-label` are the two a translation drops; `aria-current`
/// is the one this build spends on "you are here", and it is listed
/// because losing it leaves a nav with no way to say which entry is the
/// page. Every other `aria-*` is deliberately not here: a gate that
/// demanded all of them would fail on the ones a binding computes.
const CARRIED: [&str; 3] = ["role", "aria-label", "aria-current"];

/// Attributes whose value is content rather than vocabulary.
///
/// An accessible name is a sentence a person reads, so it is
/// translated: a settled screen writes it in one language and the client
/// takes it from `web::lang`. Comparing the two literally would demand
/// the client hard-code the screen's Chinese, which is the defect this
/// repository's phrase table exists to prevent. So for these the gate
/// asks whether the name is there at all, which is the thing a
/// translation loses.
///
/// `role` and `aria-current` are not here: their values come from a
/// closed vocabulary, are never translated, and `role="button"` where
/// the screen said `role="img"` is a real defect a presence check would
/// wave through.
const BY_PRESENCE: [&str; 1] = ["aria-label"];

/// Elements whose presence is itself the affordance: they are what a
/// screen reader lists when somebody asks for the shape of a page.
const LANDMARKS: [&str; 4] = ["main", "nav", "header", "h1"];

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let screens = root.join(SCREENS);
    if !screens.is_dir() {
        // No settled screens is not a failure. It is what a repository
        // looks like before the first one is written, and a gate that
        // refused that would have to be disabled to start work.
        return Ok(Vec::new());
    }
    let client = gather(root, CLIENT, "rs")?;
    let mut violations = Vec::new();
    for path in walk::files_with_ext(&screens, &["html"])? {
        let name = path
            .file_name()
            .and_then(|held| held.to_str())
            .unwrap_or("a screen")
            .to_owned();
        let body = walk::read_text(&path)?;
        for offered in affordances(&body) {
            if client.contains(&offered) {
                continue;
            }
            violations.push(Violation {
                gate: "ax",
                location: format!("{SCREENS}/{name}"),
                rule: "what a settled screen offers a screen reader, the client offers too"
                    .to_owned(),
                violation: format!("{offered} is on the screen and in no client module"),
                alternative: "put it back in the module that draws this screen; step three of the \
                     four-step method changes text nodes and control flow, never markup"
                    .to_owned(),
            });
        }
    }
    Ok(violations)
}

/// Everything the client says, as one haystack.
///
/// One string rather than a set per module, because the question is
/// whether the client offers an affordance at all — which module draws
/// it is the module map's business, not this gate's.
fn gather(root: &Path, under: &str, extension: &str) -> Result<BTreeSet<String>, XtaskError> {
    let mut found = BTreeSet::new();
    let at = root.join(under);
    if !at.is_dir() {
        return Ok(found);
    }
    for path in walk::files_with_ext(&at, &[extension])? {
        let body = walk::read_text(&path)?;
        found.extend(affordances(&body));
    }
    Ok(found)
}

/// The accessible affordances one file authors.
///
/// Read the same way from HTML and from RSX, which is what makes the two
/// sides comparable: `role: "img"` and `role="img"` are the same
/// affordance written in two syntaxes, and a gate that read them with two
/// parsers would be comparing the parsers.
fn affordances(body: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for name in CARRIED {
        // Both syntaxes. RSX spells a hyphenated attribute in quotes and
        // an unhyphenated one bare, so three spellings reach one meaning.
        for opener in [
            format!("{name}=\""),
            format!("{name}: \""),
            format!("\"{name}\": \""),
            format!("{}: \"", name.replace('-', "_")),
        ] {
            for (at, _) in body.match_indices(&opener) {
                let Some(rest) = body.get(at.saturating_add(opener.len())..) else {
                    continue;
                };
                let Some(end) = rest.find('"') else {
                    continue;
                };
                let Some(value) = rest.get(..end) else {
                    continue;
                };
                // A value a binding computes is not a literal anybody can
                // compare, and a name is content rather than vocabulary.
                // Both are recorded by attribute, so losing the attribute
                // entirely is still caught.
                let value = if value.contains('{') || BY_PRESENCE.contains(&name) {
                    "*"
                } else {
                    value
                };
                found.insert(format!("{name}={value}"));
            }
        }
        // The attribute whose value is chosen by a branch, which is how
        // "you are here" is written in every list this client draws.
        // Every literal on that line is one value the attribute takes,
        // and missing them would have this gate demand a page put back
        // the mark it already has.
        for opener in [format!("{name}\": if "), format!("{name}: if ")] {
            for (at, _) in body.match_indices(&opener) {
                // The whole line, from its own start: a slice beginning
                // mid-attribute starts inside a quote, and then every
                // other field of a split is the wrong half.
                let from = body
                    .get(..at)
                    .and_then(|before| before.rfind('\n'))
                    .map_or(0, |break_at| break_at.saturating_add(1));
                let Some(rest) = body.get(from..) else {
                    continue;
                };
                let line = rest.lines().next().unwrap_or_default();
                for chosen in line.split('"').skip(1).step_by(2) {
                    if !chosen.is_empty() && !chosen.contains('{') && !chosen.contains(name) {
                        found.insert(format!("{name}={chosen}"));
                    }
                }
            }
        }
    }
    for tag in LANDMARKS {
        if body.contains(&format!("<{tag}")) || body.contains(&format!("{tag} {{")) {
            found.insert(format!("<{tag}>"));
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
    use super::affordances;

    /// The same affordance in the two syntaxes is one affordance. Without
    /// this the gate would report every screen as broken and be turned
    /// off within a day.
    #[test]
    fn html_and_rsx_spellings_of_one_affordance_agree() {
        let html = affordances(r#"<span role="img" aria-label="running"></span>"#);
        let rsx = affordances(r#"span { role: "img", "aria-label": "running" }"#);
        assert!(html.contains("role=img"));
        assert_eq!(html, rsx);
    }

    /// A name is content and a role is vocabulary, so only one of them
    /// may be compared literally. A gate that compared both would demand
    /// the client hard-code the screen's Chinese, which is the defect the
    /// phrase table exists to prevent.
    #[test]
    fn a_name_is_compared_by_presence_and_a_role_by_value() {
        let settled = affordances(r#"<span role="img" aria-label="在跑"></span>"#);
        let built = affordances(r#"span { role: "img", "aria-label": "{word(phase)}" }"#);
        assert!(settled.contains("aria-label=*"));
        assert_eq!(settled, built, "one is Chinese and one is a binding");

        let wrong_role = affordances(r#"<span role="button" aria-label="在跑"></span>"#);
        assert!(!wrong_role.contains("role=img"), "a changed role is caught");
    }

    /// `dx translate` writes `aria_current`, and a gate that did not know
    /// that would demand the client put back something already there.
    #[test]
    fn the_translator_s_underscore_spelling_is_the_same_attribute() {
        let translated = affordances(r#"a { aria_current: "page" }"#);
        assert!(translated.contains("aria-current=page"));
    }

    /// A label a binding computes is still a label. The gate records that
    /// the attribute exists rather than what it says, so removing it is
    /// caught and filling it from a signal is not a violation.
    #[test]
    fn a_computed_name_is_recorded_by_its_attribute_and_not_by_its_text() {
        let bound = affordances(r#"span { "aria-label": "{say(lang(), mark.word())}" }"#);
        assert!(bound.contains("aria-label=*"));
        assert!(!bound.iter().any(|held| held.contains("say(")));
    }

    /// The landmarks are what a screen reader lists when somebody asks
    /// for the shape of a page, so their absence is the absence of that
    /// list.
    #[test]
    fn landmarks_are_found_in_both_syntaxes() {
        assert!(affordances("<nav class=\"left-nav\">").contains("<nav>"));
        assert!(affordances("nav { class: \"left-nav\",").contains("<nav>"));
        assert!(!affordances("<div></div>").contains("<nav>"));
    }
}
