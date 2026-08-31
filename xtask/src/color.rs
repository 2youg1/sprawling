// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

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

    // 7. Text reaches the contrast its own size demands.
    violations.extend(judge_readability(source, &greys));
    violations
}

/// The seventh assertion: every text token reaches the tier it claims, and
/// every type step claims the tier its size and weight actually demand.
///
/// This one is here rather than in a browser because it never needed one.
/// The surfaces are a closed ladder and the tokens are a closed table, so
/// the pairs are enumerable and the judgement is a pure function - what the
/// architecture calls a missing end-to-end gate is, for contrast, a missing
/// table. It is checked against `TEXT_SURFACE_CEILING`, the brightest
/// surface text may sit on, because a token that passed on the page and
/// failed on a card would be one rule with two answers.
fn judge_readability(source: &str, greys: &[(String, u16)]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let tokens = parse_text_tokens(source);
    let steps = parse_type_scale(source);
    if tokens.is_empty() || steps.is_empty() {
        violations.push(token_violation(
            "the text token and type tables are readable",
            "TEXT_TOKENS or TYPE_SCALE parsed to nothing".to_owned(),
        ));
        return violations;
    }
    let Some(surface) = text_surface_ceiling(source).and_then(|name| {
        greys
            .iter()
            .find(|(rung, _)| *rung == name)
            .map(|(_, l)| *l)
    }) else {
        violations.push(token_violation(
            "the brightest surface that carries text is a rung of the ramp",
            "TEXT_SURFACE_CEILING names no rung of GRAY_RAMP".to_owned(),
        ));
        return violations;
    };

    for (name, lightness, claimed) in &tokens {
        let reached = apca_lc(*lightness, surface);
        if reached + 0.05 < f64::from(*claimed) {
            violations.push(token_violation(
                "a text token reaches the tier it claims",
                format!("{name} claims Lc {claimed} and reaches {reached:.1}"),
            ));
        }
    }

    for (name, px, weight, claimed) in &steps {
        // A step called `body` is prose and takes the body column; every
        // other step is something that qualifies prose and takes the
        // content column. Bronze states different minimum sizes for the
        // two, and merging them would let a 15px note claim a tier only a
        // 15px sentence may claim.
        let derived = bronze_tier(*px, *weight, name == "body");
        match derived {
            None => violations.push(token_violation(
                "every type step is large enough for some tier to admit it",
                format!(
                    "{name} is {px}px at weight {weight}, below every Bronze minimum: \
                     no colour makes it legible"
                ),
            )),
            Some(tier) if tier != *claimed => violations.push(token_violation(
                "a type step claims the tier its size and weight demand",
                format!("{name} claims Lc {claimed} and demands Lc {tier}"),
            )),
            Some(_) => {}
        }
        if let Some(tier) = derived
            && !tokens.iter().any(|(_, _, claim)| *claim >= tier)
        {
            violations.push(token_violation(
                "some text token can serve every type step",
                format!("{name} demands Lc {tier} and no text token reaches it"),
            ));
        }
    }
    violations
}

/// `("TEXT", 928, 90),`
fn parse_text_tokens(source: &str) -> Vec<(String, u16, u16)> {
    table_body(source, "pub const TEXT_TOKENS")
        .lines()
        .filter_map(|line| {
            let mut fields = row_fields(line)?;
            let name = fields.next()?.trim_matches('"').to_owned();
            let lightness = resolve(fields.next()?)?;
            let tier = resolve(fields.next()?)?;
            Some((name, lightness, tier))
        })
        .collect()
}

/// `("body", 14, 400, 90),`
fn parse_type_scale(source: &str) -> Vec<(String, u16, u16, u16)> {
    table_body(source, "pub const TYPE_SCALE")
        .lines()
        .filter_map(|line| {
            let mut fields = row_fields(line)?;
            let name = fields.next()?.trim_matches('"').to_owned();
            let px = resolve(fields.next()?)?;
            let weight = resolve(fields.next()?)?;
            let tier = resolve(fields.next()?)?;
            Some((name, px, weight, tier))
        })
        .collect()
}

fn text_surface_ceiling(source: &str) -> Option<String> {
    let after = source
        .split_once("pub const TEXT_SURFACE_CEILING")
        .map(|(_, rest)| rest)?;
    let quoted = after.split_once('"').map(|(_, rest)| rest)?;
    quoted.split_once('"').map(|(name, _)| name.to_owned())
}

/// The APCA-RC Bronze Simple Mode minimum sizes, transcribed from the
/// published criterion: for each tier, the smallest size each weight may
/// use. Body text and everything else have different tables, which is the
/// whole reason the two are kept apart.
///
/// Lc 30 is deliberately absent. Its published scope is placeholder text,
/// disabled controls and non-text elements - it is not a floor content may
/// fall back to, and treating it as one makes every size pass.
const BRONZE_BODY: [(u16, &[(u16, u16)]); 2] = [
    (90, &[(300, 18), (400, 14)]),
    (75, &[(300, 24), (400, 18), (500, 16), (700, 14)]),
];
const BRONZE_CONTENT: [(u16, &[(u16, u16)]); 4] = [
    (90, &[(400, 12)]),
    (75, &[(400, 15)]),
    (
        60,
        &[
            (200, 48),
            (300, 36),
            (400, 24),
            (500, 21),
            (600, 18),
            (700, 16),
        ],
    ),
    (45, &[(400, 36), (700, 24)]),
];

/// The lowest tier that admits this size at this weight, or `None` when no
/// tier does - which means the step is too small to be content at any
/// contrast, and is a defect in the type scale rather than in the palette.
fn bronze_tier(px: u16, weight: u16, body: bool) -> Option<u16> {
    let rows: &[(u16, &[(u16, u16)])] = if body { &BRONZE_BODY } else { &BRONZE_CONTENT };
    rows.iter()
        .filter(|(_, mins)| {
            // A heavier face is legible smaller, so a weight the table does
            // not list takes the heaviest listed weight at or below it, and
            // failing that the lightest listed - which is the strictest
            // reading available.
            let floor = mins
                .iter()
                .filter(|(w, _)| *w <= weight)
                .max_by_key(|(w, _)| *w)
                .or_else(|| mins.iter().min_by_key(|(w, _)| *w));
            floor.is_some_and(|(_, min_px)| px >= *min_px)
        })
        .map(|(tier, _)| *tier)
        .min()
}

/// APCA lightness contrast between two on-axis tokens, both given as
/// lightness in per mille. Reverse polarity (light text on a dark surface)
/// reports negative, and the absolute value is what the tiers compare
/// against.
///
/// Constants are apca-w3 0.1.9 / 0.98G-4g. The 8-bit quantisation is
/// reproduced because a browser displays quantised channels, and the
/// difference decides a boundary case; it is done in floating point so that
/// no lossy integer conversion appears anywhere in this file.
fn apca_lc(text: u16, surface: u16) -> f64 {
    let text_y = soft_clamp(axis_luminance(text));
    let surface_y = soft_clamp(axis_luminance(surface));
    if (surface_y - text_y).abs() < 0.0005 {
        return 0.0;
    }
    let raw = if surface_y > text_y {
        let sapc = (surface_y.powf(0.56) - text_y.powf(0.57)) * 1.14;
        if sapc < 0.001 { 0.0 } else { sapc - 0.027 }
    } else {
        let sapc = (surface_y.powf(0.65) - text_y.powf(0.62)) * 1.14;
        if sapc > -0.001 { 0.0 } else { sapc + 0.027 }
    };
    (raw * 100.0).abs()
}

/// Screen luminance of an on-axis token: OKLCH through OKLab to linear
/// sRGB, quantised to eight bits, then APCA's own simple transfer curve.
fn axis_luminance(lightness: u16) -> f64 {
    let l = f64::from(lightness) / 1000.0;
    let chroma = f64::from(GRAY_CHROMA) / 1000.0;
    let hue = f64::from(HUE_AXIS).to_radians();
    let (a, b) = (chroma * hue.cos(), chroma * hue.sin());
    let long = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let medium = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let short = (l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    let linear = [
        4.076_741_662_1 * long - 3.307_711_591_3 * medium + 0.230_969_929_2 * short,
        -1.268_438_004_6 * long + 2.609_757_401_1 * medium - 0.341_319_396_5 * short,
        -0.004_196_086_3 * long - 0.703_418_614_7 * medium + 1.707_614_701_0 * short,
    ];
    let coefficients = [0.212_672_9, 0.715_152_2, 0.072_175_0];
    let mut y = 0.0;
    for (channel, weight) in linear.into_iter().zip(coefficients) {
        let clipped = channel.clamp(0.0, 1.0);
        let encoded = if clipped <= 0.003_130_8 {
            12.92 * clipped
        } else {
            1.055 * clipped.powf(1.0 / 2.4) - 0.055
        };
        let quantised = (encoded * 255.0).round() / 255.0;
        y += weight * quantised.powf(2.4);
    }
    y
}

/// APCA lifts the darkest values so that near-black pairs do not report
/// more contrast than an eye finds there.
fn soft_clamp(y: f64) -> f64 {
    if y > 0.022 {
        y
    } else {
        y + (0.022 - y).powf(1.414)
    }
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
pub const COLOUR_TOKENS: [(&str, u16, u16, u16); 5] = [
    ("ACCENT", 680, HUE_AXIS, ACCENT_CHROMA_PERCENT),
    ("ALERT", 919, HUE_ALERT, ALERT_CHROMA_PERCENT),
    ("ACCENT_HOVER", 760, HUE_AXIS, ACCENT_CHROMA_PERCENT),
    ("ALERT_HOVER", 945, HUE_ALERT, ALERT_CHROMA_PERCENT),
    ("ACCENT_SOLID", 919, HUE_AXIS, ACCENT_CHROMA_PERCENT),
];
pub const TEXT_SURFACE_CEILING: &str = "G2";
pub const TEXT_TOKENS: [(&str, u16, u16); 4] = [
    ("TEXT", 928, 90),
    ("TEXT_QUIET", 852, 75),
    ("TEXT_FAINT", 771, 60),
    ("TEXT_DISABLED", 582, 30),
];
pub const TYPE_SCALE: [(&str, u16, u16, u16); 6] = [
    ("figure", 28, 600, 60),
    ("title", 20, 600, 60),
    ("heading", 18, 600, 60),
    ("label", 14, 600, 90),
    ("body", 14, 400, 90),
    ("note", 15, 400, 75),
];
"#;

    #[test]
    fn the_real_shape_passes() {
        let found = judge_tokens(GOOD);
        assert!(found.is_empty(), "{}", rules(&found));
        assert_eq!(grey_ramp(GOOD).len(), 11);
        assert_eq!(parse_colour_tokens(GOOD).len(), 5);
        assert_eq!(parse_text_tokens(GOOD).len(), 4);
        assert_eq!(parse_type_scale(GOOD).len(), 6);
    }

    /// The reading this gate exists to make mechanical: these are the
    /// measured values the design was solved against, so a change to the
    /// transfer curve, the constants or the quantisation shows up here
    /// rather than as a page that is quietly harder to read.
    #[test]
    fn the_measurement_reproduces_the_readings_the_design_was_solved_against() {
        for (text, surface, expected) in [
            (930, 245, 90.6),
            (930, 195, 91.9),
            (930, 300, 88.1),
            (928, 245, 90.0),
            (852, 245, 75.0),
            (771, 245, 60.0),
            (830, 245, 70.4),
            (630, 195, 38.2),
        ] {
            let got = apca_lc(text, surface);
            assert!(
                (got - expected).abs() < 0.15,
                "L {text} on L {surface}: expected Lc {expected}, measured {got:.1}"
            );
        }
    }

    #[test]
    fn a_text_token_that_does_not_reach_its_tier_is_caught() {
        // G9 is the rung a designer would reach for when "a bit quieter"
        // is wanted. It reaches Lc 73.8 on a card, and body needs 90.
        let broken = GOOD.replace("(\"TEXT\", 928, 90)", "(\"TEXT\", 830, 90)");
        let found = judge_tokens(&broken);
        assert!(
            found
                .iter()
                .any(|v| v.violation.contains("TEXT claims Lc 90 and reaches 70.4")),
            "{}",
            rules(&found)
        );
    }

    #[test]
    fn a_type_step_claiming_the_wrong_tier_is_caught() {
        let broken = GOOD.replace("(\"note\", 15, 400, 75)", "(\"note\", 15, 400, 60)");
        let found = judge_tokens(&broken);
        assert!(
            found
                .iter()
                .any(|v| v.violation.contains("note claims Lc 60 and demands Lc 75")),
            "{}",
            rules(&found)
        );
    }

    /// One of the two steps this library used to ship is below every
    /// Bronze minimum, and no colour repairs that - which is why the fix
    /// was to the type scale and not to the greys.
    #[test]
    fn a_step_too_small_for_any_tier_is_caught() {
        let broken = GOOD.replace("(\"label\", 14, 600, 90)", "(\"label\", 11, 600, 90)");
        let found = judge_tokens(&broken);
        assert!(
            found
                .iter()
                .any(|v| v.violation.contains("below every Bronze minimum")),
            "{}",
            rules(&found)
        );
    }

    /// The other step was legal and still had to go, which is a different
    /// finding and worth its own assertion: 12px at weight 400 is admitted,
    /// but only at the top tier, so the only token that may paint it is the
    /// one body text uses. **A 12px line cannot be quieter than the prose
    /// beside it** - and "quieter" was its entire job. Quiet has to be
    /// bought with size here, not with grey.
    #[test]
    fn a_twelve_pixel_step_is_legal_and_still_cannot_be_quiet() {
        assert_eq!(bronze_tier(12, 400, false), Some(90));
        assert_eq!(bronze_tier(15, 400, false), Some(75));
        let quiet_enough: Vec<&str> = parse_text_tokens(GOOD)
            .into_iter()
            .filter(|(_, _, tier)| *tier < 90)
            .map(|(name, _, _)| match name.as_str() {
                "TEXT_QUIET" => "TEXT_QUIET",
                "TEXT_FAINT" => "TEXT_FAINT",
                _ => "other",
            })
            .collect();
        assert!(
            !quiet_enough.is_empty(),
            "there is a quieter token; it is 12px that cannot use it"
        );
    }

    #[test]
    fn text_on_a_surface_the_ladder_may_not_reach_is_caught() {
        // G3 is where the ladder stops carrying text. Pointing the ceiling
        // at it must make the body token illegal, because it is.
        let broken = GOOD.replace(
            "pub const TEXT_SURFACE_CEILING: &str = \"G2\"",
            "pub const TEXT_SURFACE_CEILING: &str = \"G3\"",
        );
        let found = judge_tokens(&broken);
        assert!(
            found.iter().any(|v| v.violation.starts_with("TEXT claims")),
            "{}",
            rules(&found)
        );
    }

    #[test]
    fn a_third_hue_is_caught() {
        let broken = GOOD.replace("(\"ALERT\", 919, HUE_ALERT", "(\"ALERT\", 919, 12");
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
