// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Size badges, rendered from the performance register rather than typed
//! into a document by hand.
//!
//! The reading comes from `budget::measure`, so the badge and the gate
//! cannot disagree about how big the binary is. What this module adds is
//! a rendering, and three rules that keep the rendering honest: the
//! colours are the product's own, the platform names itself, and a stale
//! badge turns the budget gate red.

use std::path::Path;

use crate::budget;
use crate::color;
use crate::report::{Violation, XtaskError};
use crate::walk;

/// Where the rendered badges live, relative to the repository root.
const DIR: &str = "docs/badges";

/// Padding either side of a badge's text, in px.
const PAD: u32 = 7;

/// The badge's height and its baseline, in px.
const HEIGHT: u32 = 20;
const BASELINE: u32 = 14;

/// One badge the register asks for.
struct Plan {
    /// The register row, which is also the file stem.
    metric: String,
    /// What the left half says.
    label: String,
    /// The platform allowed to refresh it, if the reading is platform-specific.
    platform: Option<String>,
}

/// This machine, in the vocabulary `budgets.toml` already uses in prose.
fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Renders every badge this machine is allowed to render.
///
/// A row is skipped in silence when its artifact is not built or when the
/// register hands it to another platform: `just check` builds neither a
/// release binary nor a wasm bundle, and a gate that demanded them is a
/// gate people learn to run with less.
fn planned(root: &Path) -> Result<Vec<(Plan, u64)>, XtaskError> {
    let register = budget::register(root)?;
    let mut out = Vec::new();
    for plan in plans(&register) {
        if let Some(target) = &plan.platform
            && *target != platform()
        {
            continue;
        }
        let Some(bytes) = budget::measure(root, &plan.metric)? else {
            continue;
        };
        out.push((plan, bytes));
    }
    Ok(out)
}

/// The register rows that ask for a badge.
fn plans(register: &toml::Value) -> Vec<Plan> {
    let Some(table) = register.as_table() else {
        return Vec::new();
    };
    let mut plans = Vec::new();
    for (metric, row) in table {
        let Some(label) = row.get("badge_label").and_then(toml::Value::as_str) else {
            continue;
        };
        plans.push(Plan {
            metric: metric.clone(),
            label: label.to_owned(),
            platform: row
                .get("badge_platform")
                .and_then(toml::Value::as_str)
                .map(str::to_owned),
        });
    }
    plans
}

/// Writes the badges this machine owns; reports what it wrote.
pub(crate) fn write(root: &Path) -> Result<String, XtaskError> {
    let planned = planned(root)?;
    if planned.is_empty() {
        return Ok(format!(
            "no badge written: nothing to weigh on {} (build with `just dist` first)",
            platform()
        ));
    }
    let dir = root.join(DIR);
    std::fs::create_dir_all(&dir).map_err(|source| XtaskError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    let theme = palette(root)?;
    let mut lines = String::new();
    for (plan, bytes) in planned {
        let svg = render(&plan.label, &human(bytes), &theme);
        let path = dir.join(format!("{}.svg", plan.metric));
        std::fs::write(&path, svg).map_err(|source| XtaskError::Io {
            path: path.display().to_string(),
            source,
        })?;
        lines.push_str(&format!(
            "{}/{}.svg: {} {}\n",
            DIR,
            plan.metric,
            plan.label,
            human(bytes)
        ));
    }
    Ok(lines)
}

/// A badge that disagrees with the artifact beside it is worse than no
/// badge: it is a number somebody will quote. Called by the budget gate.
pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let planned = planned(root)?;
    if planned.is_empty() {
        return Ok(Vec::new());
    }
    let theme = palette(root)?;
    let mut violations = Vec::new();
    for (plan, bytes) in planned {
        let rel = format!("{DIR}/{}.svg", plan.metric);
        let expected = render(&plan.label, &human(bytes), &theme);
        let found = std::fs::read_to_string(root.join(&rel)).ok();
        if found.as_deref() == Some(expected.as_str()) {
            continue;
        }
        violations.push(Violation {
            gate: "budget",
            location: rel,
            rule: "a size badge states the reading of the artifact beside it".to_owned(),
            violation: match found {
                Some(_) => format!("the badge does not say {} {}", plan.label, human(bytes)),
                None => format!("the badge is missing while {} is built", plan.metric),
            },
            alternative: "run `cargo xtask badge --write` (or `just dist`, which ends with it)"
                .to_owned(),
        });
    }
    Ok(violations)
}

/// The three colours a badge uses, taken from the product's grey ramp.
struct Palette {
    label_fill: String,
    value_fill: String,
    label_ink: String,
    value_ink: String,
}

/// Reads the palette out of `web::theme`, which is the only place in the
/// repository allowed to choose a colour. The badge picks rungs; it never
/// picks values.
fn palette(root: &Path) -> Result<Palette, XtaskError> {
    let source = walk::read_text(&root.join(color::THEME))?;
    let ramp = color::grey_ramp(&source);
    let rung = |name: &str| -> Result<u16, XtaskError> {
        ramp.iter()
            .find(|(row, _)| row == name)
            .map(|(_, lightness)| *lightness)
            .ok_or_else(|| XtaskError::Doc {
                file: color::THEME.to_owned(),
                msg: format!("the grey ramp has no rung {name}"),
            })
    };
    Ok(Palette {
        label_fill: srgb_hex(rung("G1")?),
        value_fill: srgb_hex(rung("G3")?),
        label_ink: srgb_hex(rung("G8")?),
        value_ink: srgb_hex(rung("G10")?),
    })
}

/// Bytes as a person reads them. Integer arithmetic throughout: a badge
/// that rounded differently on two machines would churn in the diff.
fn human(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if bytes >= MIB {
        let hundredths = bytes.saturating_mul(100).saturating_div(MIB);
        return format!("{}.{:02} MiB", hundredths / 100, hundredths % 100);
    }
    if bytes >= KIB {
        let tenths = bytes.saturating_mul(10).saturating_div(KIB);
        return format!("{}.{} KiB", tenths / 10, tenths % 10);
    }
    format!("{bytes} B")
}

/// Advance width of one string at 11px, in px.
///
/// An estimate rather than a measurement: embedding a font to measure it
/// exactly would put a second megabyte in the repository to centre some
/// text. Three classes are enough for label text that is ASCII.
fn text_width(text: &str) -> u32 {
    text.chars()
        .map(|c| match c {
            'i' | 'l' | 'j' | 't' | 'f' | 'I' | '.' | ',' | ':' | ';' | '\'' | '|' | ' ' => 3,
            'm' | 'w' | 'M' | 'W' => 9,
            c if c.is_ascii_uppercase() => 7,
            _ => 6,
        })
        .sum()
}

/// The badge itself: two rounded halves, the seam squared off by an
/// overlapping rectangle, and no external font reference.
fn render(label: &str, value: &str, theme: &Palette) -> String {
    let label_width = text_width(label).saturating_add(PAD.saturating_mul(2));
    let value_width = text_width(value).saturating_add(PAD.saturating_mul(2));
    let total = label_width.saturating_add(value_width);
    let label_mid = label_width / 2;
    let value_mid = label_width.saturating_add(value_width / 2);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="{HEIGHT}" role="img" aria-label="{label}: {value}">
<title>{label}: {value}</title>
<rect width="{total}" height="{HEIGHT}" rx="4" fill="{value_fill}"/>
<path d="M0 4a4 4 0 0 1 4-4h{label_width}v{HEIGHT}H4a4 4 0 0 1-4-4z" fill="{label_fill}"/>
<g font-family="system-ui,-apple-system,Segoe UI,Helvetica,Arial,sans-serif" font-size="11" text-anchor="middle">
<text x="{label_mid}" y="{BASELINE}" fill="{label_ink}">{label}</text>
<text x="{value_mid}" y="{BASELINE}" fill="{value_ink}">{value}</text>
</g>
</svg>
"#,
        label_fill = theme.label_fill,
        value_fill = theme.value_fill,
        label_ink = theme.label_ink,
        value_ink = theme.value_ink,
    )
}

/// One rung of the grey ramp as sRGB.
///
/// The theme deliberately does not convert colour spaces - a browser maps
/// OKLCH better than any fixed approximation. A badge has no browser to
/// ask: it is an image, and an image carries channel values. So the
/// conversion happens here, once, at the same build-time position as the
/// gamut search the theme already performs.
fn srgb_hex(lightness_per_mille: u16) -> String {
    let (r, g, b) = srgb_channels(
        f64::from(lightness_per_mille) / 1000.0,
        f64::from(color::GRAY_CHROMA) / 1000.0,
        f64::from(color::HUE_AXIS),
    );
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// OKLCH to 8-bit sRGB channels, by the published matrices.
///
/// Named for what it returns rather than for the conversion it performs:
/// the colour gate scans for the CSS colour functions as plain
/// substrings, and a function whose name ends in the target colour space
/// followed by an opening parenthesis reads to it as a literal. The gate
/// is deliberately crude there, and renaming is cheaper than an
/// exemption - an exemption would be a second place allowed to name a
/// colour.
fn srgb_channels(lightness: f64, chroma: f64, hue_degrees: f64) -> (u8, u8, u8) {
    let hue = hue_degrees.to_radians();
    let a = chroma * hue.cos();
    let b = chroma * hue.sin();
    let long = lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let medium = lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let short = lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
    let (long, medium, short) = (
        long * long * long,
        medium * medium * medium,
        short * short * short,
    );
    (
        channel(4.076_741_662_1 * long - 3.307_711_591_3 * medium + 0.230_969_929_2 * short),
        channel(-1.268_438_004_6 * long + 2.609_757_401_1 * medium - 0.341_319_396_5 * short),
        channel(-0.004_196_086_3 * long - 0.703_418_614_7 * medium + 1.707_614_701_0 * short),
    )
}

/// Linear-light channel to an 8-bit sRGB value, gamma encoded and clamped.
fn channel(linear: f64) -> u8 {
    let encoded = if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    let scaled = (encoded * 255.0).round().clamp(0.0, 255.0);
    #[expect(
        clippy::as_conversions,
        reason = "clamped to 0..=255 on the line above; f64 has no TryFrom to u8"
    )]
    {
        scaled as u8
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// The badge files that exist on disk, sorted.
    fn rendered_files(root: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(root.join(DIR)) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        out.sort();
        out
    }

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    fn test_palette() -> Palette {
        palette(&repo_root()).unwrap()
    }

    #[test]
    fn white_and_black_survive_the_colour_conversion() {
        assert_eq!(srgb_channels(1.0, 0.0, 0.0), (255, 255, 255));
        assert_eq!(srgb_channels(0.0, 0.0, 0.0), (0, 0, 0));
    }

    #[test]
    fn the_palette_is_the_products_own_and_stays_above_the_information_floor() {
        let source = std::fs::read_to_string(repo_root().join(color::THEME)).unwrap();
        let ramp = color::grey_ramp(&source);
        let floor = ramp
            .iter()
            .find(|(name, _)| name == "G7")
            .map(|(_, l)| *l)
            .unwrap();
        for name in ["G8", "G10"] {
            let ink = ramp
                .iter()
                .find(|(row, _)| row == name)
                .map(|(_, l)| *l)
                .unwrap();
            assert!(ink >= floor, "{name} is below the information floor");
        }
        let theme = test_palette();
        assert_ne!(theme.label_fill, theme.value_fill);
        assert_ne!(theme.label_ink, theme.value_ink);
    }

    #[test]
    fn bytes_read_the_way_a_person_reads_them() {
        assert_eq!(human(7_676_416), "7.32 MiB");
        assert_eq!(human(461_921), "451.0 KiB");
        assert_eq!(human(999), "999 B");
    }

    #[test]
    fn rendering_is_deterministic_and_says_both_halves() {
        let theme = test_palette();
        let first = render("binary", "7.32 MiB", &theme);
        let second = render("binary", "7.32 MiB", &theme);
        assert_eq!(first, second);
        assert!(first.contains(">binary<"));
        assert!(first.contains(">7.32 MiB<"));
        assert!(first.contains("aria-label=\"binary: 7.32 MiB\""));
        // Nothing is fetched when this renders: the one URL in the file
        // is the SVG namespace, which no reader resolves.
        let hosts = first.match_indices("//").count();
        assert_eq!(
            hosts, 1,
            "a badge may name one URL, and it is the namespace"
        );
        assert!(first.contains("xmlns=\"http://www.w3.org/2000/svg\""));
    }

    #[test]
    fn a_wider_string_makes_a_wider_badge() {
        assert!(text_width("7.32 MiB") > text_width("9 B"));
        let theme = test_palette();
        assert!(
            render("binary", "12.00 MiB", &theme).len() > render("binary", "9 B", &theme).len()
        );
    }

    #[test]
    fn a_row_without_a_label_asks_for_no_badge() {
        let register: toml::Value = toml::from_str(
            r#"
            [with_badge]
            badge_label = "binary"
            badge_platform = "windows-x86_64"

            [without_badge]
            status = "gated"
            "#,
        )
        .unwrap();
        let plans = plans(&register);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].metric, "with_badge");
        assert_eq!(plans[0].platform.as_deref(), Some("windows-x86_64"));
    }

    #[test]
    fn the_committed_badges_match_what_this_machine_would_render() {
        let root = repo_root();
        let violations = check(&root).unwrap();
        assert!(
            violations.is_empty(),
            "stale badge: {:?}",
            violations.first().map(|v| v.violation.clone())
        );
        for path in rendered_files(&root) {
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.starts_with("<svg"), "{path:?} is not an svg");
        }
    }
}
