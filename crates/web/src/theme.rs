// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The single-hue language, and the only place in the library that produces
//! a colour.
//!
//! Three mechanical rules generate every colour, so "add a new colour"
//! stops being a design discussion and becomes a decidable question:
//!
//! 1. **One hue axis**, `H = 264`. Past 270 the greys turn purple - that is
//!    a property of the colour space, not a preference.
//! 2. **One exception hue, and it is the opposite of the axis**:
//!    `84 = (264 + 180) mod 360`. Because the exception is derived, "there
//!    is exactly one exception" is machine-checkable down to where it sits.
//! 3. **Colour is a redundant layer.** Every coloured token is also encoded
//!    in lightness or shape, so setting the chroma coefficient to zero
//!    leaves the interface fully legible - and the desaturated snapshot is
//!    the same stylesheet evaluated twice, not a second set of rules.
//!
//! Values are stored in **per-mille integers**, not floats. `oklch()` takes
//! fractions, and formatting `145` as `0.145` is exact where a float would
//! invite a rounding difference between two builds; it also keeps the gate
//! comparing integers.
//!
//! This module does **not** convert colour spaces. The browser maps OKLCH to
//! whatever the screen can show, and it does that better than any fixed
//! approximation we could embed. The one exception is
//! the gamut search below, which answers "how much chroma fits here" once
//! at build time so the two coloured tokens can be a *ratio* rather than a
//! number somebody chose.

/// The city-wide hue. Everything except the single exception sits here.
pub const HUE_AXIS: u16 = 264;

/// The exception hue, derived rather than chosen: the complement of the
/// axis. It means exactly one thing, everywhere: *a person is needed here*.
pub const HUE_ALERT: u16 = (HUE_AXIS + 180) % 360;

/// Chroma of the grey ramp, per mille. Not zero: a completely neutral grey
/// reads as dead next to a tinted one, and this keeps the ramp on the axis.
pub const GRAY_CHROMA: u16 = 18;

/// The grey ramp's ends, per mille. These bound the *information surface*,
/// not every token: the interaction variants sit above the ceiling on
/// purpose, and a hover variant ships at 945 while this bound states 930.
/// The rule with no exception is the one below it - never pure black, never
/// pure white (one glares, the other smears on OLED).
pub const L_FLOOR: u16 = 145;
pub const L_CEILING: u16 = 930;

/// Share of the displayable chroma each coloured token takes, in percent.
/// Two ratios rather than one: the tokens do different jobs, and a shared
/// ratio pushes the alert colour back into saturated amber.
///
/// A hover variant keeps its base token's ratio - hovering is the same role
/// at a higher lightness, not a different amount of colour. There are
/// therefore exactly two ratios in the whole library, which is what makes
/// "two ratios, never merged" checkable rather than asserted.
pub const ACCENT_CHROMA_PERCENT: u16 = 90;
pub const ALERT_CHROMA_PERCENT: u16 = 55;

/// The grey ramp: name, lightness per mille. Chroma and hue are not columns
/// because they are the same for every row by rule 1 - a column that can
/// only hold one value is a place to store a lie.
///
/// `xtask color` parses this table. Keep one row per line, literal integers
/// only.
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

/// The coloured tokens: name, lightness per mille, hue, chroma ratio in
/// percent. **No chroma column** - that is the point. Chroma is resolved
/// from the ratio against what the screen can actually show.
///
/// `xtask color` parses this table too, and fails if a chroma literal ever
/// appears in it.
pub const COLOUR_TOKENS: [(&str, u16, u16, u16); 4] = [
    ("ACCENT", 680, HUE_AXIS, ACCENT_CHROMA_PERCENT),
    ("ALERT", 900, HUE_ALERT, ALERT_CHROMA_PERCENT),
    ("ACCENT_HOVER", 760, HUE_AXIS, ACCENT_CHROMA_PERCENT),
    ("ALERT_HOVER", 945, HUE_ALERT, ALERT_CHROMA_PERCENT),
];

/// Lowest rung that may carry information. G3 to G6 are decoration and are
/// exempt from the contrast floor; anything a reader must read starts here.
pub const INFORMATION_FLOOR: &str = "G7";

/// The library's only gradient: the finished part of a progress bar runs
/// from settled white to the blue head. Both ends sit on the axis, which
/// `xtask color` checks end by end.
pub const PROGRESS_DONE: (&str, &str) = ("G10", "ACCENT");

/// Name of the custom property that scales every coloured token's chroma.
/// Desaturation sets it to zero; nothing else changes.
pub const CHROMA_COEFFICIENT: &str = "--chroma";

/// The corner scales: name, nominal radius in px, superellipse exponent.
///
/// **The name says what a scale is used on, not how large it is.** A table
/// rather than radii scattered through the stylesheet, for the same reason
/// the greys are a table: twenty loose numbers are each defensible alone
/// and together cannot answer "why is a panel's corner not a button's".
///
/// A plain rounded corner is an ellipse, whose curvature jumps from zero to
/// `1/r` where the arc meets the straight edge. The eye sees that as a
/// pinch. A superellipse `|x/a|^n + |y/b|^n = 1` lets the curvature grow
/// instead of jumping, and how smoothly is decided by `n`: curvature along
/// the arc behaves as `s^(n-2)`, so the continuity order is `n - 1`.
/// `n = 2` is G1, `n = 3` is G2, `n = 4` is G3.
///
/// Each scale takes the order its job needs rather than the highest
/// available: a surface whose corner sits beside a straight edge shows the
/// jump most, and a badge must read as a circle, so it stays an ellipse.
///
/// The idea and the argument are transplanted from RefRain's `corners.zig`
/// (the author's other project); the code is not, because that is a Zig
/// canvas path builder and this is a stylesheet.
pub const CORNER_SCALES: [(&str, u16, u16); 4] = [
    ("panel", 10, 4),
    ("card", 7, 3),
    ("control", 5, 3),
    // A pill's radius is "half the short edge", which CSS reaches by
    // clamping a value larger than the box; and it must read as a circle,
    // so it keeps the ordinary ellipse.
    ("pill", 999, 2),
];

/// The CSS parameter for a superellipse exponent, in tenths.
///
/// `corner-shape: superellipse(K)` raises the ellipse equation to `2K`
/// (MDN, `superellipse()`), so `K` is half the exponent: `round` is
/// `superellipse(1)` and `squircle` is `superellipse(2)`. Tenths keep the
/// arithmetic integral - `n = 3` is `K = 1.5`, and formatting that from
/// `15` is exact where a float would invite a rounding difference between
/// two builds.
#[must_use]
pub fn superellipse_tenths(exponent: u16) -> u16 {
    exponent.saturating_mul(5)
}

/// The continuity order a scale reaches: curvature along the arc behaves
/// as `s^(n-2)`, so the first derivative that jumps is the `(n-1)`th.
#[must_use]
pub fn continuity_order(exponent: u16) -> u16 {
    exponent.saturating_sub(1)
}

/// Renders the whole token set as CSS custom properties.
///
/// This is the single production point for colour in the library. The
/// coefficient multiplies only the coloured tokens, because the grey ramp's
/// chroma is what keeps it on the axis rather than what makes it colourful.
#[must_use]
pub fn custom_properties() -> String {
    let mut css = String::from(":root{");
    css.push_str(CHROMA_COEFFICIENT);
    css.push_str(":1;");
    for (name, _) in GRAY_RAMP {
        // Routed through `gray_colour` so one function decides what a
        // grey token looks like. A name out of this very table cannot
        // fail to resolve, and if it somehow did the property would be
        // missing rather than wrong — which `xtask color` reads the same
        // tables to catch.
        if let Some(colour) = gray_colour(name) {
            css.push_str(&format!("--{name}:{colour};"));
        }
    }
    for (name, lightness, hue, percent) in COLOUR_TOKENS {
        css.push_str(&format!(
            "--{name}:oklch({} calc({} * var({CHROMA_COEFFICIENT})) {hue});",
            per_mille(lightness),
            per_mille(resolved_chroma(lightness, hue, percent)),
        ));
    }
    let (from, to) = PROGRESS_DONE;
    css.push_str(&format!(
        "--PROGRESS_DONE:linear-gradient(90deg,var(--{from}),var(--{to}));"
    ));
    // Shape travels with colour because both are presentation constants
    // with one production point. A stylesheet that had
    // to name its own radii would be the second place shape is decided.
    for (name, radius, exponent) in CORNER_SCALES {
        let tenths = superellipse_tenths(exponent);
        let whole = tenths.checked_div(10).unwrap_or_default();
        let fraction = tenths.checked_rem(10).unwrap_or_default();
        css.push_str(&format!("--corner-{name}:{radius}px;"));
        css.push_str(&format!(
            "--corner-{name}-shape:superellipse({whole}.{fraction});"
        ));
    }
    css.push('}');
    css
}

/// One grey token's colour, as a value rather than as a reference to a
/// custom property.
///
/// The canvas needs a value: `fillStyle` takes a colour, not `var(--G7)`.
/// Only the grey ramp is served, and that is the rule rather than an
/// omission — a canvas face carries its meaning in lightness (rule 3), so
/// a coloured face would be a meaning that disappears when colour does.
/// [`custom_properties`] produces the CSS side by calling this, so the
/// page and the canvas cannot disagree about what `G7` is.
#[must_use]
pub fn gray_colour(token: &str) -> Option<String> {
    GRAY_RAMP
        .into_iter()
        .find(|(name, _)| *name == token)
        .map(|(_, lightness)| {
            format!(
                "oklch({} {} {HUE_AXIS})",
                per_mille(lightness),
                per_mille(GRAY_CHROMA)
            )
        })
}

/// Formats a per-mille integer as the fraction `oklch()` expects. Exact:
/// `145` becomes `0.145`, with no float in the path.
#[must_use]
pub fn per_mille(value: u16) -> String {
    let whole = value.checked_div(1000).unwrap_or_default();
    let fraction = value.checked_rem(1000).unwrap_or_default();
    format!("{whole}.{fraction:03}")
}

/// Chroma actually used by a coloured token, per mille: the requested share
/// of the largest chroma that still lands inside sRGB at this lightness and
/// hue.
#[must_use]
pub fn resolved_chroma(lightness: u16, hue: u16, percent: u16) -> u16 {
    let ceiling = gamut_chroma_ceiling(lightness, hue);
    u16::try_from(u32::from(ceiling).saturating_mul(u32::from(percent)) / 100).unwrap_or(ceiling)
}

/// Largest chroma, per mille, that still lands inside sRGB at this
/// lightness and hue.
///
/// Binary search over integers; the in-gamut test itself is the standard
/// Oklab to linear-sRGB transform and needs real arithmetic. Floats are
/// confined to [`in_gamut`] and cannot reach any decision the rest of the
/// library makes - this answers a question about *screens*, once.
#[must_use]
pub fn gamut_chroma_ceiling(lightness: u16, hue: u16) -> u16 {
    let mut low: u16 = 0;
    let mut high: u16 = 400;
    while low < high {
        let mid = low
            .checked_add(high)
            .and_then(|sum| sum.checked_add(1))
            .map_or(low, |sum| sum / 2);
        if in_gamut(lightness, mid, hue) {
            low = mid;
        } else {
            high = mid.saturating_sub(1);
        }
    }
    low
}

/// Whether `oklch(l c h)` is displayable in sRGB.
///
/// The only floating point in the library outside tests. It is confined to
/// this function, it answers a question about screens rather than about the
/// city, and no value it produces reaches a decision - `gamut_chroma_ceiling`
/// takes only the yes-or-no. (No lint suppression needed:
/// `arithmetic_side_effects` covers integer overflow, which floats cannot
/// have. An `#[expect]` here reported itself unfulfilled and was removed.)
#[must_use]
fn in_gamut(lightness: u16, chroma: u16, hue: u16) -> bool {
    let lightness = f64::from(lightness) / 1000.0;
    let chroma = f64::from(chroma) / 1000.0;
    let radians = f64::from(hue) * std::f64::consts::PI / 180.0;
    let (a, b) = (chroma * radians.cos(), chroma * radians.sin());

    let long = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let medium = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let short = (lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);

    let red = 4.076_741_662_1 * long - 3.307_711_591_3 * medium + 0.230_969_929_2 * short;
    let green = -1.268_438_004_6 * long + 2.609_757_401_1 * medium - 0.341_319_396_5 * short;
    let blue = -0.004_196_086_3 * long - 0.703_418_614_7 * medium + 1.707_614_701_0 * short;

    [red, green, blue]
        .iter()
        .all(|channel| *channel >= -0.000_1 && *channel <= 1.000_1)
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
    fn the_exception_hue_is_the_complement_of_the_axis() {
        // Rule 2 is mechanical: the exception is not chosen, it is derived,
        // so "exactly one exception" is checkable down to its position.
        assert_eq!(HUE_ALERT, 84);
        assert_eq!((HUE_AXIS + 180) % 360, HUE_ALERT);
    }

    #[test]
    fn every_token_sits_on_one_of_the_two_hues() {
        for (name, _, hue, _) in COLOUR_TOKENS {
            assert!(
                hue == HUE_AXIS || hue == HUE_ALERT,
                "{name} is off both hues"
            );
        }
    }

    #[test]
    fn the_grey_ramp_climbs_and_stays_inside_the_floor_and_ceiling() {
        assert_eq!(GRAY_RAMP.len(), 11);
        let mut previous = 0;
        for (name, lightness) in GRAY_RAMP {
            assert!(lightness > previous, "{name} does not climb");
            assert!(
                (L_FLOOR..=L_CEILING).contains(&lightness),
                "{name} escapes the floor or ceiling"
            );
            previous = lightness;
        }
        assert_eq!(GRAY_RAMP.first().unwrap().1, L_FLOOR);
        assert_eq!(GRAY_RAMP.last().unwrap().1, L_CEILING);
    }

    #[test]
    fn the_gradient_has_both_ends_on_the_axis() {
        let (from, to) = PROGRESS_DONE;
        assert!(GRAY_RAMP.iter().any(|(name, _)| *name == from));
        assert!(
            COLOUR_TOKENS
                .iter()
                .any(|(name, _, hue, _)| *name == to && *hue == HUE_AXIS)
        );
    }

    #[test]
    fn desaturating_is_one_coefficient_and_touches_only_colour() {
        let css = custom_properties();
        assert!(css.contains("--chroma:1;"));
        // Every coloured token multiplies by the coefficient...
        for (name, ..) in COLOUR_TOKENS {
            assert!(
                css.contains(&format!("--{name}:oklch(")),
                "{name} missing from the stylesheet"
            );
        }
        let coefficient_uses = css.matches("var(--chroma)").count();
        assert_eq!(
            coefficient_uses,
            COLOUR_TOKENS.len(),
            "the coefficient reaches every coloured token and nothing else"
        );
        // ...and the grey ramp does not, because its chroma is what keeps it
        // on the axis, not what makes it colourful.
        assert!(css.contains(&format!("--G0:oklch(0.145 0.018 {HUE_AXIS});")));
    }

    #[test]
    fn chroma_is_resolved_from_a_ratio_and_lands_where_the_table_says() {
        // Constitution 17.1 records ACCENT near 0.151 and ALERT near 0.058.
        // Those are consequences of the two ratios, not inputs; if the
        // search drifted far from them, one of the two is wrong.
        let accent = resolved_chroma(680, HUE_AXIS, ACCENT_CHROMA_PERCENT);
        let alert = resolved_chroma(900, HUE_ALERT, ALERT_CHROMA_PERCENT);
        assert!(
            (140..=165).contains(&accent),
            "ACCENT chroma resolved to {accent} per mille"
        );
        assert!(
            (48..=70).contains(&alert),
            "ALERT chroma resolved to {alert} per mille"
        );
    }

    #[test]
    fn the_gamut_ceiling_is_a_real_boundary() {
        let ceiling = gamut_chroma_ceiling(680, HUE_AXIS);
        assert!(in_gamut(680, ceiling, HUE_AXIS), "the ceiling itself fits");
        assert!(
            !in_gamut(680, ceiling + 2, HUE_AXIS),
            "and just past it does not"
        );
    }

    #[test]
    fn the_whole_library_uses_exactly_two_chroma_ratios() {
        let mut ratios: Vec<u16> = COLOUR_TOKENS.iter().map(|(_, _, _, r)| *r).collect();
        ratios.sort_unstable();
        ratios.dedup();
        assert_eq!(ratios, [ALERT_CHROMA_PERCENT, ACCENT_CHROMA_PERCENT]);
    }

    #[test]
    fn a_hover_variant_is_brighter_at_the_same_ratio() {
        for (base, hover) in [("ACCENT", "ACCENT_HOVER"), ("ALERT", "ALERT_HOVER")] {
            let find = |wanted: &str| {
                COLOUR_TOKENS
                    .iter()
                    .find(|(name, ..)| *name == wanted)
                    .copied()
                    .unwrap()
            };
            let (_, base_l, base_h, base_ratio) = find(base);
            let (_, hover_l, hover_h, hover_ratio) = find(hover);
            assert!(hover_l > base_l, "{hover} must be the brighter one");
            assert_eq!(hover_h, base_h);
            assert_eq!(hover_ratio, base_ratio);
        }
    }

    #[test]
    fn every_corner_scale_states_the_order_it_reaches() {
        // The names are jobs, not sizes, and each one takes the order its
        // job needs. A scale that could not say why it is where it is is
        // how twenty loose radii start.
        for (name, radius, exponent) in CORNER_SCALES {
            assert!(!name.is_empty());
            assert!(radius > 0);
            assert!(
                (2..=5).contains(&exponent),
                "{name} takes an exponent outside the range the argument covers"
            );
            assert_eq!(
                continuity_order(exponent),
                exponent - 1,
                "curvature behaves as s^(n-2), so the order is n-1"
            );
        }
    }

    #[test]
    fn the_css_parameter_is_half_the_exponent() {
        // MDN: `superellipse(K)` raises the equation to 2K, so round is
        // K=1 and squircle is K=2. Getting this backwards would draw a
        // panel with corners nearly square.
        assert_eq!(superellipse_tenths(2), 10, "round");
        assert_eq!(superellipse_tenths(4), 20, "squircle");
        assert_eq!(superellipse_tenths(3), 15);
    }

    #[test]
    fn the_stylesheet_never_has_to_name_a_radius() {
        let css = custom_properties();
        for (name, radius, _) in CORNER_SCALES {
            assert!(
                css.contains(&format!("--corner-{name}:{radius}px;")),
                "{name}"
            );
            assert!(
                css.contains(&format!("--corner-{name}-shape:superellipse(")),
                "{name}"
            );
        }
        assert!(
            css.contains("--corner-pill-shape:superellipse(1.0)"),
            "a badge must still read as a circle"
        );
    }

    #[test]
    fn per_mille_formats_exactly() {
        assert_eq!(per_mille(145), "0.145");
        assert_eq!(per_mille(18), "0.018");
        assert_eq!(per_mille(930), "0.930");
        assert_eq!(per_mille(1000), "1.000");
    }
}
