// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

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
//! 4. **A text lightness is a required contrast, not a rung somebody
//!    picked.** The yardstick is **APCA** (apca-w3 0.1.9, constants
//!    0.98G-4g) at the **APCA-RC Bronze Simple Mode** thresholds, which is
//!    what a dark interface needs: WCAG 2.x measures a ratio of relative
//!    luminance and is known to misjudge light-on-dark, where APCA weights
//!    the two polarities separately. `TEXT_TOKENS` therefore stores, for
//!    each Bronze tier this library can reach, the lightness that reaches
//!    it - solved, the way a coloured token's chroma is solved from a
//!    ratio. `cargo xtask color` re-solves both and fails on drift.
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
pub const COLOUR_TOKENS: [(&str, u16, u16, u16); 5] = [
    ("ACCENT", 680, HUE_AXIS, ACCENT_CHROMA_PERCENT),
    ("ALERT", 919, HUE_ALERT, ALERT_CHROMA_PERCENT),
    ("ACCENT_HOVER", 760, HUE_AXIS, ACCENT_CHROMA_PERCENT),
    ("ALERT_HOVER", 945, HUE_ALERT, ALERT_CHROMA_PERCENT),
    ("ACCENT_SOLID", 919, HUE_AXIS, ACCENT_CHROMA_PERCENT),
];

/// The brightest surface that may have text on it.
///
/// Depth in a dark interface is built by stacking lightness, and every step
/// up spends the contrast the text still needs. Measured: the ceiling
/// itself reaches only Lc 88.1 on G3, and body text needs Lc 90 - so **the
/// raised rung cannot carry body text at any lightness this library is
/// allowed to use**. G3 keeps its job as a border, a rule and a fill that
/// carries nothing; text stops at G2.
///
/// This is also why `TEXT_TOKENS` is solved against G2 rather than against
/// the page: a token solved on the darkest surface would be legal on the
/// page and illegal on a card, which is the same defect as a rule with two
/// authorities.
pub const TEXT_SURFACE_CEILING: &str = "G2";

/// The lightness at which text reaches each Bronze tier, and the tier it
/// answers: name, lightness per mille, Lc.
///
/// **These are not rungs of the grey ramp.** The ramp is the surface
/// ladder; a rung of it is a place to put something, not a permission to
/// write. G9 (830) reaches Lc 70.4 on a card and body text needs 90, so
/// picking a rung that "looks quiet enough" is exactly the judgement this
/// table removes.
///
/// The consequence is worth stating because it decides how a page is
/// composed: on a dark surface APCA charges heavily for small text, so
/// **"quieter" cannot be bought with a darker grey - it has to be bought
/// with a larger step.** A 14px line has exactly one legal colour here.
/// Hierarchy at one size is therefore weight, and colour only moves when
/// the size does.
///
/// `cargo xtask color` parses this table. Keep one row per line, literal
/// integers only.
pub const TEXT_TOKENS: [(&str, u16, u16); 4] = [
    ("TEXT", 928, 90),
    ("TEXT_QUIET", 852, 75),
    ("TEXT_FAINT", 771, 60),
    ("TEXT_DISABLED", 582, 30),
];

/// Lowest rung that may carry information. G3 to G6 are decoration and are
/// exempt from the contrast floor; anything a reader must read starts here.
///
/// This governs one surface only: the release badges `cargo xtask badge`
/// renders, which are their own drawing with their own background. The
/// interface's own floor is `TEXT_TOKENS`, which is measured rather than
/// named.
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

/// The face the chrome is set in. A stack of the faces a desktop already
/// has, ending in the generic family.
///
/// **No font file is embedded and none is fetched.** That half of the old
/// decision stands, for its original reason: everything on screen except
/// the chrome is content a person wrote - a Building's name, `Memo.md`, a
/// Ledger payload - whose character set cannot be predicted, so no subset
/// could cover it, and a whole CJK face costs several megabytes against a
/// two-megabyte budget.
///
/// **What changed is naming nothing at all.** The library used to declare
/// the bare generic families, on the argument that `sans-serif` hands the
/// choice to the reader while any named face takes it back. Measured on
/// Windows, that argument buys the opposite of what it promises: the
/// browser resolves `sans-serif` to Arial and `monospace` to Courier New,
/// and this interface sets every address, identifier, amount and count in
/// the fixed-width face - so a typewriter face drew about half the
/// characters on the page. A reader who never opened *Customise fonts*
/// was not handed a choice; they were handed Courier New.
///
/// The generic family is therefore last rather than alone: a reader who
/// has set a preference still ends there, and a reader who has not gets
/// the face their own platform dresses its interface in.
///
/// **No family named here is CJK, and that is load-bearing.** A CJK family
/// listed first renders *Latin* text with that family's Latin glyphs, so
/// English chrome came out drawn by a Chinese face on every machine that
/// had one installed. Latin faces are named, Han is left to the browser's
/// own fallback, and the two never compete for one run of text.
///
/// The interface pane of the settings page says where the setting lives,
/// because a preference the product obeys and never mentions is a
/// preference the reader cannot find.
///
/// **Four OFL faces lead, and the platform stack is kept whole behind
/// them.** Every name before `system-ui` is SIL Open Font License 1.1 and
/// is drawn for an interface at 14-20px rather than for a page. A reader
/// who has one installed gets a face designed for this size; a reader who
/// has none gets, byte for byte, what this interface gave them before -
/// the face their own platform dresses its own interface in. **The stack
/// can therefore improve a machine and cannot regress one**, which is the
/// only shape in which a font preference is worth spending a stack on.
///
/// **Lato was measured and dropped, and that is the rule this entry
/// states.** It is OFL, it is the one OFL interface face already installed
/// on the machine this was settled on, and set beside the platform face at
/// both 15px and 20px/600 against the same Han fallback it is a lateral
/// move: narrower letterforms, a lighter bold, no gain the eye can name. A
/// stack entry that changes what a reader sees without improving it is
/// worse than no entry, because it makes the interface's appearance depend
/// on which machine somebody opened it on and buys nothing for that. **A
/// face earns a place here by beating the platform default, not by being
/// free.**
///
/// The licence is a constraint on what may be *named as a target*, not on
/// what is distributed: this product ships no font file at all, and the
/// test below is what holds that. Naming a proprietary platform face in
/// the tail incurs nothing, because naming is not distribution - but a
/// face the reader cannot legally go and install is not a design target,
/// and the four that lead are ones anybody may.
///
/// **No OFL interface face is installed by default on any of the three
/// desktops**, so on a machine nobody has furnished, this stack resolves
/// to the same platform face as before. That is a statement about the
/// world rather than about this table, and the way to change what one
/// reader sees is to install one of the four - no rebuild, because a
/// system stack is read at paint time.
pub const FONT_SANS: &str = "Inter, 'IBM Plex Sans', 'Source Sans 3', 'Public Sans', \
     system-ui, -apple-system, 'Segoe UI', Roboto, Ubuntu, Cantarell, Arial, sans-serif";

/// Numbers, identifiers, hashes and addresses take the fixed-width face.
/// A second family rather than `tabular-nums` alone: a column of digits
/// should line up *and* read as a different kind of thing from the prose
/// beside it, which is how a terminal separates a value from its label
/// without emphasising either.
///
/// `ui-monospace` first, for the face a platform already uses in its own
/// developer surfaces; then the four that ship with the three desktops;
/// then the generic family as the reader's last word. Courier New is
/// deliberately not named - it is what `monospace` alone resolved to on
/// Windows, and it is the reason this stack exists.
pub const FONT_MONO: &str =
    "ui-monospace, 'Cascadia Mono', 'SF Mono', Menlo, Consolas, 'DejaVu Sans Mono', monospace";

/// The type scale: name, size in px, weight, and the Bronze tier that size
/// and weight demand.
///
/// A table for the same reason the greys are a table. Before this existed
/// the stylesheet held eleven sizes between 11px and 28px, four of them
/// within half a pixel of each other (12, 12.5, 13), and no rule said
/// which one a new line should take. Six steps, each with a job in its
/// name, answer that question once.
///
/// **The name says what a step is for, not how large it is**, so a step
/// can be retuned without every reader of it becoming wrong. `figure` is
/// the one number a page exists to state; `title` names the page's
/// conclusion; `heading` divides a page; `body` is prose; `note` is
/// everything that qualifies something else - scope, provenance, an empty
/// state; `label` is a name shouted into uppercase that must not compete
/// with what it labels.
///
/// **The fourth column is why two steps changed size.** Bronze states a
/// minimum size for each tier, so a step's size decides how much contrast
/// it needs, and the two smallest steps this library used to have - 12px
/// at weight 400 and 11px at weight 600 - are below every tier's minimum:
/// no colour makes them legible, because the problem is not the colour.
/// `small` became `note` at 15px, which is the smallest size Bronze admits
/// at Lc 75; `micro` became `label` at 14px, which Bronze admits at Lc 90.
/// `heading` moved from 15px to 18px so that it can be quieter than the
/// body it divides, which at 15px it was not allowed to be.
///
/// `cargo xtask color` re-derives this column from the size and the weight
/// and fails on drift, so the tier is checked rather than asserted.
/// **`body` is 15px because `note` is 15px.** The scale shipped prose at
/// 14px and the line that qualifies prose at 15px, so on every panel the
/// sentence explaining a figure was set larger than the rows stating it:
/// the second tier of information was the loudest thing in the panel. The
/// two steps are now one size and the hierarchy is carried by colour -
/// `body` takes TEXT, `note` takes TEXT_QUIET - which is the distinction
/// that survives a reader who has zoomed the page.
///
/// 15px rather than dropping `note` to 13px: Bronze admits a 13px content
/// step only at Lc 90, which is the tier `body` itself claims, so a
/// smaller note would have had to be as loud as the prose it qualifies in
/// order to stay legible. `xtask color` re-derives both rows, so this
/// trade is checked rather than asserted.
pub const TYPE_SCALE: [(&str, u16, u16, u16); 6] = [
    ("figure", 28, 600, 60),
    ("title", 20, 600, 60),
    ("heading", 18, 600, 60),
    ("label", 14, 600, 90),
    ("body", 15, 400, 90),
    ("note", 15, 400, 75),
];

/// The spacing scale: name, size in px. Every step is a multiple of four,
/// which was already the rule and is now the only place it is written
/// down.
///
/// Six steps rather than a continuous choice, for the reason three zoom
/// stops beat a slider: a value picked freely is a value picked again
/// slightly differently on the next page, and the difference is visible
/// long before anybody can name it.
pub const SPACE_SCALE: [(&str, u16); 6] = [
    ("tight", 4),
    ("snug", 8),
    ("base", 12),
    ("pane", 16),
    ("wide", 24),
    ("section", 32),
];

/// The two widths a page is built from, in px.
///
/// **`measure` is a reading constraint, not a taste.** A line stops making
/// the eye search for its own beginning somewhere between 45 and 75
/// characters, and this interface is read in two scripts at once: at the
/// body step 520px is about 74 Latin characters and about 37 Chinese ones,
/// which is inside the comfortable band of both. Three different caps -
/// 88ch, 78ch and 72ch - used to answer this one question in three places.
///
/// **`page` bounds rows and tables**, which may use it all, where prose may
/// not. The top bar takes the same bound as the content under it, so the
/// page has one spine down each side. Its absence is what issue #1
/// photographed: a dispatch strip 2376px wide, with no line start to find.
///
/// `cargo xtask color` and `theme`'s own tests read this table. Keep one
/// row per line, literal integers only.
pub const WIDTH_SCALE: [(&str, u16); 2] = [("measure", 520), ("page", 1040)];

/// The one duration in the library, in milliseconds.
///
/// **A change a person did not ask for may not be animated at all.** The
/// judgement is stated as a question with one answer: if the movement would
/// happen while nobody is touching the interface, it is manufacturing
/// attention and is deleted. That rules out every ambient effect - a striped
/// progress bar, a pulsing badge, a row that fades in as it arrives - and it
/// leaves exactly one case, which is the acknowledgement a control owes the
/// hand that just moved to it.
///
/// 90ms because the window closes at about 100ms: past that the
/// acknowledgement is late enough to read as lag rather than as feedback,
/// and below about 60ms it is not seen at all. One value rather than a
/// scale, for the reason the spacing steps are a scale rather than a free
/// choice - two durations differ visibly long before anybody can say which
/// is which, and there is only one thing here worth timing.
pub const MOTION_QUICK_MS: u16 = 90;

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
    // The text tokens sit on the axis and take the ramp's chroma, because
    // what puts them on the axis is the same fact that puts the ramp on it.
    // They are written after the ramp so that a stylesheet reading
    // `var(--TEXT)` gets a value chosen by a required contrast rather than
    // by which rung looked quiet enough.
    for (name, lightness, _) in TEXT_TOKENS {
        css.push_str(&format!(
            "--{name}:oklch({} {} {HUE_AXIS});",
            per_mille(lightness),
            per_mille(GRAY_CHROMA),
        ));
    }
    let (from, to) = PROGRESS_DONE;
    css.push_str(&format!(
        "--PROGRESS_DONE:linear-gradient(90deg,var(--{from}),var(--{to}));"
    ));
    // Type and space travel with colour for the same reason shape does:
    // they are presentation constants, and a stylesheet that named its own
    // sizes would be the second place a size is decided.
    for (name, size, weight, _) in TYPE_SCALE {
        css.push_str(&format!("--text-{name}:{size}px;--weight-{name}:{weight};"));
    }
    for (name, size) in SPACE_SCALE {
        css.push_str(&format!("--space-{name}:{size}px;"));
    }
    for (name, size) in WIDTH_SCALE {
        css.push_str(&format!("--width-{name}:{size}px;"));
    }
    css.push_str(&format!("--motion-quick:{MOTION_QUICK_MS}ms;"));
    css.push_str(&format!("--font-sans:{FONT_SANS};--font-mono:{FONT_MONO};"));
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
        // Constitution 17.1 records ACCENT near 0.151. That is a
        // consequence of the ratio, not an input; if the search drifted far
        // from it, either the ratio or the gamut search is wrong.
        let accent = resolved_chroma(680, HUE_AXIS, ACCENT_CHROMA_PERCENT);
        assert!(
            (140..=165).contains(&accent),
            "ACCENT chroma resolved to {accent} per mille"
        );
        // Every row resolves to its own share of what the screen can show
        // at its own lightness. Reading the table rather than repeating a
        // lightness keeps this true when a token moves: ALERT rose from 900
        // to 919 so that dark numerals on a badge reach Lc 90, and its
        // chroma fell with the gamut, which is the ratio working rather
        // than a number going stale.
        for (name, lightness, hue, percent) in COLOUR_TOKENS {
            let ceiling = gamut_chroma_ceiling(lightness, hue);
            let resolved = resolved_chroma(lightness, hue, percent);
            assert!(
                resolved <= ceiling,
                "{name} resolved past the gamut ceiling"
            );
            assert!(resolved > 0, "{name} resolved to no colour at all");
            let expected = u32::from(ceiling)
                .saturating_mul(u32::from(percent))
                .checked_div(100)
                .unwrap_or_default();
            assert_eq!(
                u32::from(resolved),
                expected,
                "{name} is not its ratio of the ceiling"
            );
        }
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

    /// The stylesheet the browser actually receives. Read here rather than
    /// described, for the same reason `xtask color` parses this file: a
    /// rule about every line of a document has to be checked against that
    /// document.
    const SHIPPED: &str = include_str!("../assets/app.css");

    /// A screen designed in plain HTML reads the same tokens the product
    /// installs.
    ///
    /// The product writes them from `custom_properties` after the wasm
    /// loads, which a file opened in a browser never does - so without
    /// this a prototype renders with no tokens at all and the four-step
    /// method cannot take its first step.
    ///
    /// It lands in `target/` rather than beside the screen, because it is
    /// derived: deleting it and running the tests brings it back. That is
    /// also what keeps `xtask color` correct - a file full of `oklch()`
    /// inside the tree would be a second place a colour is written, and
    /// the gate is right to refuse one whether or not a person typed it.
    ///
    /// **Absent is not stale, and the difference is the whole test.** A
    /// path under `target/` is absent in every fresh clone, which is its
    /// first normal state rather than a defect - writing it and passing
    /// is what a fresh clone is owed. Drift is the red worth keeping:
    /// bytes that disagree with the tables mean the tables moved while a
    /// prototype was still linking the old file, so the screen somebody
    /// was looking at was rendered with tokens the product no longer
    /// installs. `xtask render` already reads the two states apart this
    /// way (xtask-SPEC.md, the fifteenth gate); this test did not, so it
    /// failed on every clean checkout while telling the reader to commit
    /// a path that cannot be committed.
    #[test]
    fn the_design_screens_read_the_same_tokens_the_product_installs() {
        let want = format!(
            "/* Generated by web::theme's own test. Do not edit: the \
             authority is the token\n   tables in src/theme.rs, and this \
             file is rewritten from them. */\n{}\n",
            custom_properties()
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("screens")
            .join("tokens.css");
        let found = std::fs::read_to_string(&path).ok();
        if found.as_deref() == Some(want.as_str()) {
            return;
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
        assert!(
            found.is_none(),
            "target/screens/tokens.css disagreed with the token tables and has been \
             rewritten: every screen opened since the tables last moved was rendered \
             with tokens the product no longer installs. Reopen the screens."
        );
    }

    #[test]
    fn the_stylesheet_never_has_to_name_a_size_or_a_family() {
        let css = custom_properties();
        for (name, size, weight, _) in TYPE_SCALE {
            assert!(css.contains(&format!("--text-{name}:{size}px;")), "{name}");
            assert!(
                css.contains(&format!("--weight-{name}:{weight};")),
                "{name}"
            );
        }
        for (name, size) in SPACE_SCALE {
            assert!(css.contains(&format!("--space-{name}:{size}px;")), "{name}");
        }
        for (name, size) in WIDTH_SCALE {
            assert!(css.contains(&format!("--width-{name}:{size}px;")), "{name}");
        }
        assert!(css.contains(&format!("--font-sans:{FONT_SANS};")));
        assert!(css.contains(&format!("--font-mono:{FONT_MONO};")));
        // And the shipped page reads them rather than repeating them. A
        // `font-family` in the stylesheet is a second production point for
        // presentation, which is the thing the token tables exist to stop.
        assert!(
            !SHIPPED.contains("font-family: \"") && !SHIPPED.contains("font-family: '"),
            "the stylesheet names a font family; it should read var(--font-sans) or var(--font-mono)"
        );
    }

    /// Renaming a step used to be silent: the table changed, the
    /// stylesheet went on reading the old name, and the browser resolved
    /// it to nothing - which paints at the initial value rather than at
    /// the value anybody chose. Nothing in the library noticed, because
    /// every existing assertion reads the table and none reads the page.
    #[test]
    fn the_stylesheet_never_reads_a_token_that_is_not_produced() {
        let produced = custom_properties();
        let mut missing = Vec::new();
        for (at, _) in SHIPPED.match_indices("var(--") {
            let Some(rest) = SHIPPED.get(at.saturating_add(4)..) else {
                continue;
            };
            let Some(end) = rest.find([')', ',']) else {
                continue;
            };
            let Some(name) = rest.get(..end) else {
                continue;
            };
            // `--chroma` is the coefficient itself, declared first and by
            // name rather than through a table.
            if name == CHROMA_COEFFICIENT {
                continue;
            }
            if !produced.contains(&format!("{name}:")) {
                missing.push(name);
            }
        }
        missing.sort_unstable();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "the stylesheet reads tokens no table produces: {missing:?}"
        );
    }

    #[test]
    fn no_font_file_ships_and_the_generic_family_is_the_last_word() {
        // Han is left to the browser's own fallback. A CJK family listed in
        // either stack renders *Latin* text with that family's Latin
        // glyphs, so English chrome comes out drawn by a Chinese face on
        // every machine that has one installed - which is what this
        // interface used to do, and the reason the rule is a rule.
        for cjk in [
            "Noto Sans SC",
            "Noto Sans CJK",
            "Zen Kaku Gothic New",
            "Source Han",
            "PingFang",
            "Microsoft YaHei",
            "SimSun",
            "Hiragino",
        ] {
            assert!(
                !FONT_SANS.contains(cjk) && !FONT_MONO.contains(cjk) && !SHIPPED.contains(cjk),
                "{cjk} is named in a stack whose Latin glyphs would then draw the chrome"
            );
        }
        // The reader's *Customise fonts* setting is what a generic family
        // resolves to, so it stays reachable - last, where it decides the
        // case nothing above it covered, rather than first, where it once
        // resolved every identifier on the page to Courier New.
        assert!(
            FONT_SANS.ends_with("sans-serif"),
            "the generic family is the last word in the sans stack: {FONT_SANS}"
        );
        assert!(
            FONT_MONO.ends_with("monospace"),
            "the generic family is the last word in the mono stack: {FONT_MONO}"
        );
        // A stack of one is the state this rule was written to leave: it is
        // the bare generic family under another name.
        assert!(
            FONT_SANS.contains(',') && FONT_MONO.contains(','),
            "a stack names at least one face before the generic family"
        );
        for embedded in [
            "@font-face",
            ".woff",
            ".ttf",
            "fonts.googleapis",
            "fonts.gstatic",
        ] {
            assert!(
                !SHIPPED.contains(embedded),
                "no font file ships and none is fetched: {embedded}"
            );
        }
    }

    /// Nothing moves unless a person just moved.
    ///
    /// The judgement, unchanged since the layout rules were written: if a
    /// movement would happen while nobody is touching the interface, it is
    /// manufacturing attention, and it is deleted. `@keyframes` and
    /// `animation` can only do that - they run on their own clock - so
    /// they are refused outright.
    ///
    /// A `transition` is allowed, and is checked rather than trusted: every
    /// property it names must be one that only `:hover`, `:focus-visible`
    /// or `:active` changes. A transition on `height` or `opacity` would
    /// fire when a page redraws with new content, which is an animation
    /// nobody asked for wearing the syntax of a permitted one.
    #[test]
    fn nothing_moves_unless_a_person_just_moved() {
        for banned in ["@keyframes", "animation:", "animation-name"] {
            assert!(
                !SHIPPED.contains(banned),
                "{banned} runs on its own clock; a movement nobody asked for is deleted"
            );
        }
        // Properties an interaction pseudo-class is allowed to change.
        // `transform` is here because a press displaces by one pixel, which
        // survives the desaturated snapshot where a colour-only press does
        // not.
        const ANSWERABLE: [&str; 5] = [
            "background-color",
            "border-color",
            "color",
            "outline-color",
            "transform",
        ];
        for (at, _) in SHIPPED.match_indices("transition:") {
            let Some(rest) = SHIPPED.get(at.saturating_add("transition:".len())..) else {
                continue;
            };
            let Some(end) = rest.find(';') else {
                panic!("a transition with no end: {rest:.60}");
            };
            let Some(declared) = rest.get(..end) else {
                continue;
            };
            for step in declared.split(',') {
                let mut words = step.split_whitespace();
                let Some(property) = words.next() else {
                    continue;
                };
                assert!(
                    ANSWERABLE.contains(&property),
                    "{property} is not a property an interaction changes, so a \
                     transition on it fires when the page redraws: {declared}"
                );
                // Everything after the property is the timing, and this
                // library has one duration to spend on it.
                for timing in words {
                    assert!(
                        timing == "var(--motion-quick)",
                        "{timing} is a second duration; the library has one, \
                         and MOTION_QUICK_MS produces it"
                    );
                }
            }
        }
    }

    #[test]
    fn per_mille_formats_exactly() {
        assert_eq!(per_mille(145), "0.145");
        assert_eq!(per_mille(18), "0.018");
        assert_eq!(per_mille(930), "0.930");
        assert_eq!(per_mille(1000), "1.000");
    }
}
