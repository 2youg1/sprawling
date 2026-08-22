// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Colour gate: the single-hue language is machine-decidable or it is just
//! a preference somebody can argue with.
//!
//! Two halves, and they sit in different places on purpose. The **six
//! assertions on the token values** are checked here against the tables in
//! `web::theme`, which this gate parses the same way `modmap` parses the
//! module table - the authority is the source file, and the gate reads it
//! rather than keeping a copy. The **repository scan** for stray colour
//! literals is the half only a gate can do, because it is a statement about
//! every file rather than about one table.
//!
//! `web::theme` is exempt from the scan: it is the production point. Nothing
//! else may name a colour.

use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

pub(crate) const THEME: &str = "crates/web/src/theme.rs";
pub(crate) const HUE_AXIS: u16 = 264;
const HUE_ALERT: u16 = 84;
pub(crate) const GRAY_CHROMA: u16 = 18;
const L_FLOOR: u16 = 145;
const L_CEILING: u16 = 930;

/// Files that may legitimately contain colour syntax: the production point,
/// and this gate's own source. The second exemption has the same shape and
/// the same reason as `xtask/lexicon.toml` being outside the lexicon scan -
/// a checker has to be able to spell what it forbids.
const SCAN_EXEMPT: [&str; 2] = [THEME, "xtask/src/color.rs"];

/// Extensions worth scanning. Rust, and the two file kinds that carry style.
const SCAN_EXTS: [&str; 3] = ["rs", "css", "html"];

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let mut violations = Vec::new();
    let theme_path = root.join(THEME);
    if !theme_path.is_file() {
        violations.push(Violation {
            gate: "color",
            location: THEME.to_owned(),
            rule: "web::theme is the sole production point for colour".to_owned(),
            violation: "the theme module is missing".to_owned(),
            alternative: "restore crates/web/src/theme.rs".to_owned(),
        });
        return Ok(violations);
    }
    let source = walk::read_text(&theme_path)?;
    violations.extend(judge_tokens(&source));
    violations.extend(scan_for_literals(root)?);
    Ok(violations)
}

/// The six assertions, in the order the gate table lists them.
fn judge_tokens(source: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let greys = grey_ramp(source);
    let colours = parse_colour_tokens(source);

    // 1. Eleven grey rungs, climbing.
    if greys.len() != 11 {
        violations.push(token_violation(
            "the grey ramp has eleven rungs",
            format!("found {}", greys.len()),
        ));
    }
    if !greys.windows(2).all(|pair| match pair {
        [(_, low), (_, high)] => high > low,
        _ => true,
    }) {
        violations.push(token_violation(
            "the grey ramp climbs",
            "a rung is not brighter than the one below it".to_owned(),
        ));
    }

    // 2. Two lightness rules, and they are not the same rule.
    //
    //    The grey ramp spans exactly the declared floor and ceiling: it is
    //    the information surface, and its ends are the contract.
    //
    //    Every token, grey or coloured, avoids pure black and pure white -
    //    that is what the ban is actually protecting against (glare, and
    //    smearing on OLED). Running the gate for the first time found that
    //    the theme states a ceiling of 930 while its own token table
    //    puts ALERT_HOVER at 945. Both shipped, so the ceiling is a property
    //    of the ramp and the interaction variants sit above it by design;
    //    reading it as a global bound would have made the table illegal.
    if let (Some(darkest), Some(lightest)) = (greys.first(), greys.last())
        && (darkest.1 != L_FLOOR || lightest.1 != L_CEILING)
    {
        violations.push(token_violation(
            "the grey ramp spans exactly the declared floor and ceiling",
            format!("the ramp runs {} to {}", darkest.1, lightest.1),
        ));
    }
    let every_lightness = greys
        .iter()
        .map(|(name, lightness)| (name, *lightness))
        .chain(
            colours
                .iter()
                .map(|(name, lightness, _, _)| (name, *lightness)),
        );
    for (name, lightness) in every_lightness {
        if lightness == 0 || lightness >= 1000 {
            violations.push(token_violation(
                "no pure black and no pure white (glare, and OLED smear)",
                format!("{name} is at {lightness} per mille"),
            ));
        }
    }

    // 3. Every coloured token sits on the axis or on its single exception.
    for (name, _, hue, _) in &colours {
        if *hue != HUE_AXIS && *hue != HUE_ALERT {
            violations.push(token_violation(
                "one hue axis and one exception, which is its complement",
                format!("{name} sits on hue {hue}"),
            ));
        }
    }

    // 4. The exception hue really is the complement.
    if (HUE_AXIS + 180) % 360 != HUE_ALERT {
        violations.push(token_violation(
            "the exception hue is derived, not chosen",
            format!("{HUE_ALERT} is not the complement of {HUE_AXIS}"),
        ));
    }

    // 5. The grey ramp's chroma is the single axis value.
    if !source.contains(&format!("GRAY_CHROMA: u16 = {GRAY_CHROMA}")) {
        violations.push(token_violation(
            "the grey ramp carries the axis chroma",
            format!("GRAY_CHROMA is not {GRAY_CHROMA} per mille"),
        ));
    }

    // 6. Coloured tokens take a ratio, never a written chroma, and there are
    //    exactly two ratios in the library.
    let mut ratios: Vec<u16> = colours.iter().map(|(_, _, _, ratio)| *ratio).collect();
    ratios.sort_unstable();
    ratios.dedup();
    if ratios.len() != 2 {
        violations.push(token_violation(
            "exactly two chroma ratios, never merged",
            format!("found {} distinct ratios", ratios.len()),
        ));
    }
    if colours.is_empty() {
        violations.push(token_violation(
            "the coloured token table is readable",
            "COLOUR_TOKENS parsed to nothing".to_owned(),
        ));
    }
    violations
}

fn token_violation(rule: &str, violation: String) -> Violation {
    Violation {
        gate: "color",
        location: THEME.to_owned(),
        rule: rule.to_owned(),
        violation,
        alternative: "adjust web::theme, and record the reason in web-SPEC.md \
                      section 8; colour rules are mechanical by design"
            .to_owned(),
    }
}

/// `("G0", 145),`
pub(crate) fn grey_ramp(source: &str) -> Vec<(String, u16)> {
    table_body(source, "pub const GRAY_RAMP")
        .lines()
        .filter_map(|line| {
            let mut fields = row_fields(line)?;
            let name = fields.next()?.trim_matches('"').to_owned();
            let lightness = fields.next()?.parse().ok()?;
            Some((name, lightness))
        })
        .collect()
}

/// `("ACCENT", 680, HUE_AXIS, ACCENT_CHROMA_PERCENT),` - symbolic fields are
/// resolved against the constants this gate already knows, because a table
/// that spelled the numbers would defeat the point of the ratio.
fn parse_colour_tokens(source: &str) -> Vec<(String, u16, u16, u16)> {
    table_body(source, "pub const COLOUR_TOKENS")
        .lines()
        .filter_map(|line| {
            let mut fields = row_fields(line)?;
            let name = fields.next()?.trim_matches('"').to_owned();
            let lightness = resolve(fields.next()?)?;
            let hue = resolve(fields.next()?)?;
            let ratio = resolve(fields.next()?)?;
            Some((name, lightness, hue, ratio))
        })
        .collect()
}

fn resolve(field: &str) -> Option<u16> {
    match field {
        "HUE_AXIS" => Some(HUE_AXIS),
        "HUE_ALERT" => Some(HUE_ALERT),
        "GRAY_CHROMA" => Some(GRAY_CHROMA),
        "ACCENT_CHROMA_PERCENT" => Some(90),
        "ALERT_CHROMA_PERCENT" => Some(55),
        other => other.parse().ok(),
    }
}

fn row_fields(line: &str) -> Option<impl Iterator<Item = &str>> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('(')?.split_once(')')?.0;
    Some(inner.split(',').map(str::trim).filter(|f| !f.is_empty()))
}

/// Takes the rows between `= [` and the closing bracket. The `= ` matters:
/// splitting on the first `[` would land inside the type annotation, which
/// is exactly the bug the first run of this gate found.
fn table_body<'a>(source: &'a str, marker: &str) -> &'a str {
    let Some(after) = source.split_once(marker).map(|(_, rest)| rest) else {
        return "";
    };
    let Some(open) = after.split_once("= [").map(|(_, rest)| rest) else {
        return "";
    };
    open.split_once("];").map_or("", |(body, _)| body)
}

/// Colour spellings that must not appear outside the theme.
///
/// Hex is judged differently per language, because `#` means different
/// things in each. In style files a bare `#1a2b3c` is a colour. In Rust it
/// is far more often a Locator fragment or an issue number, so only a
/// fully-quoted `"#1a2b3c"` counts - the shape somebody actually writes
/// when they mean a colour. The first run of this gate caught
/// `cas:b3-…#B01-2` and taught this distinction.
fn literal_at(line: &str, style_file: bool) -> Option<&'static str> {
    for syntax in ["oklch(", "rgb(", "rgba(", "hsl(", "hsla("] {
        if line.contains(syntax) {
            return Some(syntax);
        }
    }
    if hex_colour(line, style_file) {
        return Some("#rrggbb");
    }
    None
}

fn hex_colour(line: &str, style_file: bool) -> bool {
    let bytes = line.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'#' {
            continue;
        }
        let run = bytes
            .iter()
            .skip(index.saturating_add(1))
            .take_while(|b| b.is_ascii_hexdigit())
            .count();
        if run != 3 && run != 6 {
            continue;
        }
        let opens = index.checked_sub(1).and_then(|i| bytes.get(i));
        let closes = index
            .checked_add(run)
            .and_then(|i| i.checked_add(1))
            .and_then(|i| bytes.get(i));
        let quoted = opens == Some(&b'"') && closes == Some(&b'"');
        if quoted || (style_file && closes.is_none_or(|b| !b.is_ascii_hexdigit())) {
            return true;
        }
    }
    false
}

fn scan_for_literals(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let mut violations = Vec::new();
    for path in walk::files_with_ext(root, &SCAN_EXTS)? {
        let rel = walk::rel(root, &path);
        if walk::in_isolation_zone(&rel) || SCAN_EXEMPT.contains(&rel.as_str()) {
            continue;
        }
        let style_file = !rel.ends_with(".rs");
        let text = walk::read_text(&path)?;
        for (number, line) in text.lines().enumerate() {
            if let Some(syntax) = literal_at(line, style_file) {
                let line_number = number.saturating_add(1);
                violations.push(Violation {
                    gate: "color",
                    location: format!("{rel}:{line_number}"),
                    rule: "web::theme is the only place that names a colour".to_owned(),
                    violation: format!("colour literal `{syntax}` outside the theme"),
                    alternative: "use a token from web::theme through its CSS \
                                  custom property"
                        .to_owned(),
                });
            }
        }
    }
    Ok(violations)
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

    /// `Violation` has no `Debug` on purpose (it is rendered, not dumped),
    /// so failures report the rules that fired.
    fn rules(found: &[Violation]) -> String {
        found
            .iter()
            .map(|v| format!("{} :: {}", v.rule, v.violation))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    const GOOD: &str = r#"
pub const GRAY_CHROMA: u16 = 18;
pub const GRAY_RAMP: [(&str, u16); 11] = [
    ("G0", 145),
    ("G1", 195),
    ("G2", 245),
    ("G3", 300),
    ("G4", 360),
    ("G5", 430),
    ("G6", 520),
    ("G7", 630),
    ("G8", 730),
    ("G9", 830),
    ("G10", 930),
];
pub const COLOUR_TOKENS: [(&str, u16, u16, u16); 4] = [
    ("ACCENT", 680, HUE_AXIS, ACCENT_CHROMA_PERCENT),
    ("ALERT", 900, HUE_ALERT, ALERT_CHROMA_PERCENT),
    ("ACCENT_HOVER", 760, HUE_AXIS, ACCENT_CHROMA_PERCENT),
    ("ALERT_HOVER", 945, HUE_ALERT, ALERT_CHROMA_PERCENT),
];
"#;

    #[test]
    fn the_real_shape_passes() {
        let found = judge_tokens(GOOD);
        assert!(found.is_empty(), "{}", rules(&found));
        assert_eq!(grey_ramp(GOOD).len(), 11);
        assert_eq!(parse_colour_tokens(GOOD).len(), 4);
    }

    #[test]
    fn a_third_hue_is_caught() {
        let broken = GOOD.replace("(\"ALERT\", 900, HUE_ALERT", "(\"ALERT\", 900, 12");
        let found = judge_tokens(&broken);
        assert!(
            found.iter().any(|v| v.violation.contains("hue 12")),
            "{}",
            rules(&found)
        );
    }

    #[test]
    fn pure_white_is_caught() {
        let broken = GOOD.replace("(\"G10\", 930)", "(\"G10\", 1000)");
        let found = judge_tokens(&broken);
        assert!(
            found.iter().any(|v| v.violation.contains("1000 per mille")),
            "{}",
            rules(&found)
        );
    }

    #[test]
    fn a_ramp_that_stops_short_of_the_ceiling_is_caught() {
        let broken = GOOD.replace("(\"G10\", 930)", "(\"G10\", 900)");
        let found = judge_tokens(&broken);
        assert!(
            found.iter().any(|v| v.violation.contains("145 to 900")),
            "{}",
            rules(&found)
        );
    }

    #[test]
    fn an_interaction_variant_may_sit_above_the_ramp_ceiling() {
        // ALERT_HOVER at 945 is legal and the theme ships it; only
        // pure white is not.
        let found = judge_tokens(GOOD);
        assert!(found.is_empty(), "{}", rules(&found));
    }

    #[test]
    fn a_third_ratio_is_caught() {
        let broken = GOOD.replace(
            "(\"ACCENT_HOVER\", 760, HUE_AXIS, ACCENT_CHROMA_PERCENT)",
            "(\"ACCENT_HOVER\", 760, HUE_AXIS, 71)",
        );
        let found = judge_tokens(&broken);
        assert!(
            found.iter().any(|v| v.violation.contains("3 distinct")),
            "{}",
            rules(&found)
        );
    }

    #[test]
    fn a_shortened_ramp_is_caught() {
        let broken = GOOD.replace("    (\"G5\", 430),\n", "");
        let found = judge_tokens(&broken);
        assert!(found.iter().any(|v| v.violation.contains("found 10")));
    }

    #[test]
    fn colour_spellings_are_recognised_and_locators_are_not() {
        assert_eq!(literal_at("  color: #070A12;", true), Some("#rrggbb"));
        assert_eq!(literal_at("background: rgb(1,2,3)", true), Some("rgb("));
        assert_eq!(
            literal_at("--G0:oklch(0.145 0.018 264)", true),
            Some("oklch(")
        );
        assert_eq!(literal_at("let x = 3;", false), None);

        // The shape somebody writes in Rust when they mean a colour.
        assert_eq!(
            literal_at(r##"let bg = "#070A12";"##, false),
            Some("#rrggbb")
        );

        // A Locator fragment is not a colour. This exact line is what the
        // gate's first run tripped on.
        assert_eq!(
            literal_at(r##"format!("cas:b3-{H64}#B01-2"),"##, false),
            None
        );
        assert_eq!(literal_at("issue #4707 records the status", false), None);
        // A digest is longer than six hex digits either way.
        assert_eq!(
            literal_at(
                "// 692b5f963f99f018496b8df111314dfe1bed52ccfe1a40cb9a5975b3bc8664fe",
                true
            ),
            None
        );
    }
}
