// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Cost, savings, and the machine's own vital signs.
//!
//! **Series are told apart without hue.** Width, dash pattern and end marker
//! carry the distinction; at most four series to a chart; each line labelled
//! at its end rather than in a legend. A legend asks the reader to hold a
//! mapping in their head while looking somewhere else.
//!
//! **The layout is anti-attention.** Normal rows fold to a single line;
//! abnormal ones rise to the top and open. No red dots, no unread counts, no
//! infinite stream - all three manufacture returns to the screen without
//! adding information.
//!
//! **The cost page never advises.** It orders facts. Whether a SKILL earns
//! the bytes it occupies needs something the interface does not have -
//! whether it will still be used next quarter - so the ranking is the
//! deliverable and the judgement stays with the person.
//!
//! Machine metrics live here and go no further: not into `status`, not into
//! any prefix. A model's decisions do not use resident memory or link rate,
//! and putting them in context would charge every turn for a report nobody
//! reads.

use crate::lang::{Msg, fill, say};
use channels::UsdMicros;
use dioxus::prelude::*;

use crate::readout::render_usd;

/// The five cuts of one authoritative total. Exhaustive:
/// a sixth dimension would have to be added here, and to the reconciliation
/// that `memory::attribution` already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CostDimension {
    PrefixSegment,
    Skill,
    Tool,
    SubRun,
    Occupant,
}

impl CostDimension {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrefixSegment => "prefix segment",
            Self::Skill => "SKILL",
            Self::Tool => "tool",
            Self::SubRun => "sub-Run",
            Self::Occupant => "Building / Resident",
        }
    }

    /// All five, in the order the page shows them.
    #[must_use]
    pub fn all() -> [Self; 5] {
        [
            Self::PrefixSegment,
            Self::Skill,
            Self::Tool,
            Self::SubRun,
            Self::Occupant,
        ]
    }
}

/// One line of the cost page: what it cost, what share that is, which way it
/// is moving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostRow {
    pub label: String,
    pub spent: UsdMicros,
    /// Share of the dimension's total, per mille.
    pub share: u16,
    pub trend: Trend,
}

/// Which way a number is moving. An enum, because "up" and "down" are the
/// only readings the page draws and a raw delta would invite a fourth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    Rising,
    Steady,
    Falling,
}

impl Trend {
    /// The mark drawn beside the number. Shape, not colour: the page must
    /// survive desaturation like everything else.
    #[must_use]
    pub fn mark(self) -> &'static str {
        match self {
            Self::Rising => "^",
            Self::Steady => "-",
            Self::Falling => "v",
        }
    }

    /// Reads a trend from two totals. Equal is `Steady` - a page that called
    /// every unchanged row "rising by 0%" would be noise.
    #[must_use]
    pub fn between(previous: UsdMicros, current: UsdMicros) -> Self {
        match current.get().cmp(&previous.get()) {
            std::cmp::Ordering::Greater => Self::Rising,
            std::cmp::Ordering::Less => Self::Falling,
            std::cmp::Ordering::Equal => Self::Steady,
        }
    }
}

/// Builds one dimension's rows, largest first.
///
/// Shares are computed against the total handed in, not against the sum of
/// the rows: the two agree by A20's reconciliation, and computing it here
/// would create a second opinion about what the total is.
#[must_use]
pub fn cost_rows(entries: Vec<(String, UsdMicros, UsdMicros)>, total: UsdMicros) -> Vec<CostRow> {
    let mut rows: Vec<CostRow> = entries
        .into_iter()
        .map(|(label, previous, spent)| CostRow {
            label,
            spent,
            share: share_per_mille(spent, total),
            trend: Trend::between(previous, spent),
        })
        .collect();
    rows.sort_by(|left, right| {
        right
            .spent
            .get()
            .cmp(&left.spent.get())
            .then_with(|| left.label.cmp(&right.label))
    });
    rows
}

/// Share of a total, per mille. Zero total means zero share rather than a
/// division that cannot be performed.
#[must_use]
pub fn share_per_mille(part: UsdMicros, total: UsdMicros) -> u16 {
    if total.get() == 0 {
        return 0;
    }
    let scaled = u128::from(part.get()).saturating_mul(1000);
    let share = scaled.checked_div(u128::from(total.get())).unwrap_or(0);
    u16::try_from(share.min(1000)).unwrap_or(1000)
}

/// What the pipeline and offload saved, kept beside what was spent. One
/// side records what went out, the other what did not have to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavingsRow {
    pub label: String,
    pub saved: UsdMicros,
    pub tokens_saved: u64,
}

/// Renders a cost row as the single line the folded view shows.
#[must_use]
pub fn fold_line(row: &CostRow) -> String {
    let whole = row.share.checked_div(10).unwrap_or_default();
    format!(
        "{}  {}  {}%  {}",
        row.label,
        render_usd(row.spent),
        whole,
        row.trend.mark()
    )
}

/// What an empty cut says, said.
fn cut_empty(lang: crate::lang::Lang, dimension: &str) -> String {
    fill(say(lang, Msg::CostCutEmpty), &[("dimension", dimension)])
}

/// The cost page: one total, five cuts of it, and what the city cannot
/// price.
///
/// The trend mark is absent here on purpose. A direction needs two
/// samples, this client holds one, and `-` beside every row would be a
/// claim of steadiness nobody measured.
#[component]
pub fn CostsView(
    answer: Option<channels::CostAnswer>,
    usage: crate::app::Usage,
    spent: UsdMicros,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<channels::ClientFrame>,
) -> Element {
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let asked = use_signal(|| false);
    use_effect(move || {
        let mut asked = asked;
        if live() && !asked() {
            asked.set(true);
            on_frame.call(channels::ClientFrame::Query(channels::Query::CostView));
        }
    });
    let Some(answer) = answer else {
        return rsx! {
            section { class: "dashboard",
                crate::panel::Empty {
                    status: word(Msg::CostAskingSpent).to_owned(),
                    what: word(Msg::CostAskingSpentWhat).to_owned(),
                }
            }
        };
    };
    let cuts = [
        (CostDimension::PrefixSegment, answer.by_segment.clone()),
        (CostDimension::Skill, answer.by_skill.clone()),
        (CostDimension::Tool, answer.by_tool.clone()),
        (CostDimension::SubRun, answer.by_run.clone()),
        (CostDimension::Occupant, answer.by_actor.clone()),
    ];
    let total = answer.total;
    // Five cuts each saying "nothing attributed here yet" is one fact
    // repeated five times, and repetition of an absence reads as a broken
    // page rather than an empty one. Before any money exists there is one
    // thing to say, so the page says it once.
    let nothing_yet = cuts.iter().all(|(_, entries)| entries.is_empty());
    // Work was attributed and every figure is zero. That is not a free
    // city: it is a provider that reported no price, and rendering it as
    // $0.00 in every row is the interface answering a question nobody can
    // answer. Zero and unknown are different, and only one is ever true
    // here. Found on a real provider that bills a subscription.
    let unpriced = !nothing_yet && total == UsdMicros::default();
    rsx! {
        section { class: "dashboard",
            crate::panel::Panel {
                title: match (nothing_yet, unpriced) {
                    (true, _) => word(Msg::CostNothingSpent).to_owned(),
                    (false, true) => word(Msg::CostUnpricedTitle).to_owned(),
                    (false, false) => word(Msg::CostWhereMoneyWent).to_owned(),
                },
                figure: (!unpriced).then(|| render_usd(total)),
                scope: if unpriced {
                    word(Msg::CostUnpricedScope).to_owned()
                } else {
                    word(Msg::CostScope).to_owned()
                },
                source: word(Msg::CostSource).to_owned(),
                p { class: "consumed",
                    "{crate::readout::render_tokens(usage.input)} in, {crate::readout::render_tokens(usage.output)} out, {crate::readout::render_tokens(usage.cache_read)} from cache"
                }
                if usage.unpriced_calls > 0 {
                    p { class: "unpriced",
                        "{usage.unpriced_calls} call(s) came back with no price: a subscription or a local model reports what it used, not what it cost. Those calls are counted in tokens above and in no dollar figure anywhere."
                    }
                }
                if nothing_yet {
                    crate::panel::Empty {
                        status: word(Msg::CostNoneBilled).to_owned(),
                        what: word(Msg::CostNoneBilledWhat).to_owned(),
                    }
                } else {
                    for (dimension, entries) in cuts {
                        article { key: "{dimension.as_str()}", class: "dimension",
                            h2 { "{dimension.as_str()}" }
                            if entries.is_empty() {
                                p { class: "note",
                                    "{cut_empty(lang(), dimension.as_str())}"
                                }
                            }
                            for row in cost_rows(
                                entries.into_iter().map(|(label, spent)| (label, spent, spent)).collect(),
                                total,
                            ) {
                                div { key: "{row.label}", class: "row",
                                    span { class: "label", "{row.label}" }
                                    span {
                                        class: "track",
                                        span {
                                            class: "fill",
                                            style: "width: {row.share.checked_div(10).unwrap_or_default()}%",
                                        }
                                    }
                                    span { class: "amount",
                                        if unpriced {
                                            "unpriced"
                                        } else {
                                            "{render_usd(row.spent)}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                p { class: "spent-line",
                    "{render_usd(spent)} of that arrived through this page's own stream"
                }
            }
        }
    }
}

/// How many series one chart may carry. Beyond this the reader is decoding
/// rather than reading.
pub const SERIES_PER_CHART_MAX: usize = 4;

/// The three widths and three dash patterns that distinguish series without
/// hue. Paired by index, so four series get four distinguishable
/// combinations before the cap is reached.
pub const SERIES_WIDTHS: [&str; 4] = ["1", "1.75", "2.5", "1"];
pub const SERIES_DASHES: [&str; 4] = ["none", "4 2", "1 2", "4 2"];

/// Whether a series count is drawable. Not a warning: past the cap the
/// chart is split, because a fifth line has no unused width-and-dash pair.
#[must_use]
pub fn drawable(series: usize) -> bool {
    series <= SERIES_PER_CHART_MAX
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

    fn usd(v: u64) -> UsdMicros {
        UsdMicros::new(v)
    }

    #[test]
    fn the_page_carries_exactly_the_five_attribution_dimensions() {
        // Constitution 12.3, and the same five memory::attribution reconciles
        // against one authoritative total (A20).
        assert_eq!(CostDimension::all().len(), 5);
        let mut names: Vec<&str> = CostDimension::all().iter().map(|d| d.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 5);
    }

    #[test]
    fn rows_are_ordered_by_what_they_cost() {
        let rows = cost_rows(
            vec![
                ("skill-a".to_owned(), usd(0), usd(100_000)),
                ("skill-b".to_owned(), usd(0), usd(900_000)),
                ("skill-c".to_owned(), usd(0), usd(500_000)),
            ],
            usd(1_500_000),
        );
        let order: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(order, ["skill-b", "skill-c", "skill-a"]);
        assert_eq!(rows[0].share, 600);
    }

    #[test]
    fn shares_are_taken_against_the_authoritative_total_not_the_row_sum() {
        // Rows may not cover the whole spend - an unattributed remainder is
        // honest and A20 keeps it. Normalising to the row sum would hide it.
        let rows = cost_rows(
            vec![("tool".to_owned(), usd(0), usd(250_000))],
            usd(1_000_000),
        );
        assert_eq!(rows[0].share, 250, "a quarter of the real total");
    }

    #[test]
    fn an_unchanged_row_is_steady_rather_than_rising_by_nothing() {
        assert_eq!(Trend::between(usd(10), usd(10)), Trend::Steady);
        assert_eq!(Trend::between(usd(10), usd(11)), Trend::Rising);
        assert_eq!(Trend::between(usd(11), usd(10)), Trend::Falling);
    }

    #[test]
    fn trends_are_marked_by_shape_so_the_page_survives_desaturation() {
        let mut marks: Vec<&str> = [Trend::Rising, Trend::Steady, Trend::Falling]
            .iter()
            .map(|t| t.mark())
            .collect();
        marks.sort_unstable();
        marks.dedup();
        assert_eq!(marks.len(), 3);
    }

    #[test]
    fn a_chart_refuses_a_fifth_series_rather_than_reusing_a_pattern() {
        assert!(drawable(4));
        assert!(!drawable(5));
        assert_eq!(SERIES_WIDTHS.len(), SERIES_PER_CHART_MAX);
        assert_eq!(SERIES_DASHES.len(), SERIES_PER_CHART_MAX);
        let pairs: std::collections::BTreeSet<(&str, &str)> = SERIES_WIDTHS
            .iter()
            .zip(SERIES_DASHES.iter())
            .map(|(w, d)| (*w, *d))
            .collect();
        assert_eq!(pairs.len(), SERIES_PER_CHART_MAX, "no pair repeats");
    }

    #[test]
    fn a_zero_total_produces_no_share_rather_than_a_division() {
        assert_eq!(share_per_mille(usd(5), usd(0)), 0);
    }

    #[test]
    fn the_folded_line_holds_everything_the_row_says() {
        let row = CostRow {
            label: "prefix".to_owned(),
            spent: usd(1_500_000),
            share: 333,
            trend: Trend::Rising,
        };
        let line = fold_line(&row);
        assert!(line.contains("prefix"));
        assert!(line.contains("$1.50"));
        assert!(line.contains("33%"));
        assert!(line.contains('^'));
    }
}
