// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Two lists that share one shape: things waiting for a person.
//!
//! **The Approval Inbox** groups items by cluster key, so
//! forty identical questions arrive as one decision with a count rather than
//! forty rows. Grouping is a view concern - the Ledger keeps every item -
//! but which items may be grouped is not: a tainted item stands alone,
//! because grouping it would let one answer cover a question the person
//! never actually read.
//!
//! **The Recycle Bin** shows what was discarded and how
//! it comes back. Every entry names its restoration, because a delete with
//! no way back is refused upstream and never reaches this list; a row here
//! that could not say how to undo itself would mean that refusal leaked.
//!
//! Neither list decides anything. Answering is a Command, and it is
//! authorised on the far side of the wire.

use std::collections::BTreeMap;

use channels::{ApprovalClass, ApprovalItem, ClientFrame, Locator, PolicyVerdict, TimeMs};
use dioxus::prelude::*;

/// A group of identical questions, presented as one.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// The key every member shares, rendered for a person.
    pub summary: String,
    /// Oldest first: the question that has waited longest leads.
    pub members: Vec<ApprovalItem>,
    /// Whether this group must be answered one at a time.
    pub answer_individually: bool,
}

impl Cluster {
    #[must_use]
    pub fn count(&self) -> usize {
        self.members.len()
    }

    /// When the oldest member arrived. Sorting on this puts the thing that
    /// has been blocking longest at the top, which is the only ordering a
    /// person waiting on their own work would accept.
    #[must_use]
    pub fn waiting_since(&self) -> Option<TimeMs> {
        self.members.iter().map(|item| item.created).min()
    }
}

/// Groups pending items into the list the Inbox shows.
///
/// Order is `(waiting_since, summary)`. Time leads because the oldest block
/// is the most expensive one; the summary breaks ties so the same set of
/// items always renders in the same order - two people looking at one city
/// must see one list.
#[must_use]
pub fn inbox(items: Vec<ApprovalItem>) -> Vec<Cluster> {
    let mut grouped: BTreeMap<(bool, String, String), Vec<ApprovalItem>> = BTreeMap::new();
    for item in items {
        // A tainted item is keyed by its own id, which makes its group a
        // group of one. This is C15 held by construction rather than by a
        // check somebody could forget: there is no key it can share.
        let key = if item.tainted {
            (true, item.id.as_str().to_owned(), String::new())
        } else {
            (
                false,
                format!("{:?}", item.cluster_key.class),
                item.cluster_key.detail.clone(),
            )
        };
        grouped.entry(key).or_default().push(item);
    }

    let mut clusters: Vec<Cluster> = grouped
        .into_iter()
        .map(|((tainted, class, detail), mut members)| {
            members.sort_by_key(|item| (item.created, item.id.as_str().to_owned()));
            let summary = if tainted {
                members
                    .first()
                    .map_or_else(|| class.clone(), |item| item.action_desc.clone())
            } else if detail.is_empty() {
                class
            } else {
                format!("{class}: {detail}")
            };
            Cluster {
                summary,
                members,
                answer_individually: tainted,
            }
        })
        .collect();
    clusters.sort_by(|left, right| {
        (left.waiting_since(), &left.summary).cmp(&(right.waiting_since(), &right.summary))
    });
    clusters
}

/// The inbox: what is waiting, grouped the way [`inbox`] groups it.
///
/// The page answers one item at a time and one group at a time, and the
/// difference is [`Cluster::answer_individually`] rather than a judgement
/// made here - a tainted item has no group to be answered with.
#[component]
pub fn ApprovalsView(
    items: Vec<ApprovalItem>,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<ClientFrame>,
) -> Element {
    let asked = use_signal(|| false);
    // What was already waiting before this page connected. The stream
    // carries what happens next and nothing earlier.
    use_effect(move || {
        let mut asked = asked;
        if live() && !asked() {
            asked.set(true);
            on_frame.call(ClientFrame::Query(channels::Query::ApprovalQueue));
        }
    });
    let clusters = inbox(items);
    let waiting: usize = clusters.iter().map(Cluster::count).sum();
    rsx! {
        section { class: "approvals",
            crate::panel::Panel {
                title: if clusters.is_empty() { "nothing is waiting for you".to_owned() }
                    else { "what the city stopped to ask you".to_owned() },
                figure: "{waiting}",
                scope: "one row per action a gate escalated rather than decided; grouped where one answer can settle several"
                    .to_owned(),
                source: "the approval queue as the city holds it, plus every approval_requested that has arrived since this page connected"
                    .to_owned(),
            if clusters.is_empty() {
                crate::panel::Empty {
                    status: "no gate has escalated anything".to_owned(),
                    what: "a run reaches a person only when a door refuses to decide by itself - a write outside its domain, a discard with no way back, an action a policy has not yet settled. Until then work runs without asking."
                        .to_owned(),
                }
            }
            for cluster in clusters {
                article {
                    key: "{cluster.summary}",
                    class: if cluster.answer_individually { "cluster tainted" } else { "cluster" },
                    header {
                        span { class: "what", "{cluster.summary}" }
                        span { class: "count", "{cluster.count()} waiting" }
                        if cluster.answer_individually {
                            span { class: "note",
                                "this one began with someone else's text: answered alone, and no policy can waive it"
                            }
                        }
                    }
                    for item in cluster.members.clone() {
                        div { key: "{item.id.as_str()}", class: "item",
                            span { class: "desc", "{item.action_desc}" }
                            span { class: "actor", "{item.actor}" }
                            button {
                                class: "allow",
                                onclick: {
                                    let id = item.id.clone();
                                    move |_| on_frame.call(answer_command(&id, PolicyVerdict::Allow))
                                },
                                "allow"
                            }
                            button {
                                class: "deny",
                                onclick: {
                                    let id = item.id.clone();
                                    move |_| on_frame.call(answer_command(&id, PolicyVerdict::Deny))
                                },
                                "refuse"
                            }
                            if policy_admits(&item) {
                                button {
                                    class: "policy",
                                    onclick: {
                                        let id = item.id.clone();
                                        move |_| on_frame.call(policy_command(&id))
                                    },
                                    "and stop asking me this"
                                }
                            }
                        }
                    }
                }
            }
            }
        }
    }
}

/// Whether a standing policy may be built from this item.
///
/// One class admits policies; the rest are never
/// waivable, and the button is absent rather than offered and refused -
/// an interface that offers what the far side will reject teaches people
/// to ignore refusals.
#[must_use]
pub fn policy_admits(item: &ApprovalItem) -> bool {
    !item.tainted && matches!(item.cluster_key.class, ApprovalClass::AgentQuestion)
}

fn answer_command(id: &channels::ApprovalId, verdict: PolicyVerdict) -> ClientFrame {
    ClientFrame::Command(Box::new(channels::WireCommand::Approve {
        idem: channels::IdemKey::derive(
            &channels::RunId::CITY,
            channels::Seq::FIRST,
            id.as_str().as_bytes(),
        ),
        item: id.clone(),
        verdict,
    }))
}

fn policy_command(id: &channels::ApprovalId) -> ClientFrame {
    ClientFrame::Command(Box::new(channels::WireCommand::CreatePolicy {
        idem: channels::IdemKey::derive(
            &channels::RunId::CITY,
            channels::Seq::FIRST,
            format!("policy-{}", id.as_str()).as_bytes(),
        ),
        from_item: id.clone(),
    }))
}

/// How a discarded thing comes back. Rendered as a sentence rather than a
/// type name, because the person reading it is deciding whether to bother.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnPath {
    /// Restorable from a checkpoint that already exists.
    FromCheckpoint(String),
    /// The bytes are in content-addressed storage.
    FromStore(String),
    /// Not stored, but reproducible: the reason says how.
    Rebuild(String),
    /// A restoration scheme this client is too old to describe.
    ///
    /// Fail-closed for a view means refusing to *invent an action*: the row
    /// still appears, because hiding a discarded thing would be worse, but
    /// it says "look at the Ledger" rather than offering a button whose
    /// behaviour this build cannot predict.
    Undescribed,
}

impl ReturnPath {
    /// Reads a `Restoration` as an instruction. Every arm has an answer,
    /// because a discard with no way back never became an event.
    #[must_use]
    pub fn of(restoration: &channels::Restoration) -> Self {
        match *restoration {
            channels::Restoration::Tracked(ref locator) => {
                Self::FromCheckpoint(render_locator(locator))
            }
            channels::Restoration::Interred(ref locator) => {
                Self::FromStore(render_locator(locator))
            }
            channels::Restoration::Rebuildable { ref reason } => Self::Rebuild(reason.clone()),
            _ => Self::Undescribed,
        }
    }

    #[must_use]
    pub fn sentence(&self) -> String {
        match *self {
            Self::FromCheckpoint(ref at) => format!("restore from the checkpoint at {at}"),
            Self::FromStore(ref at) => format!("restore the stored copy at {at}"),
            Self::Rebuild(ref how) => format!("rebuild it: {how}"),
            Self::Undescribed => {
                "this build cannot describe how to restore it; the Ledger records the plan"
                    .to_owned()
            }
        }
    }
}

fn render_locator(locator: &Locator) -> String {
    format!("{locator:?}")
}

/// One row of the Recycle Bin.
///
/// No byte count: `file_discarded` does not record one, and a column that
/// is zero on every row is not missing data but a lie repeated per row.
/// `restored` is here instead, because that is a thing the record does
/// know - `discard_restored` turns it true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinRow {
    pub what: String,
    pub discarded_at: TimeMs,
    pub return_path: ReturnPath,
    pub restored: bool,
}

/// Reads the city's answer as rows. The one place the wire's shape
/// becomes the view's shape, so "how a discard is described" is decided
/// once and read everywhere.
#[must_use]
pub fn bin_rows(answer: &channels::DiscardAnswer) -> Vec<BinRow> {
    recycle_bin(
        answer
            .rows
            .iter()
            .map(|line| BinRow {
                what: line.path.clone(),
                discarded_at: line.at,
                return_path: line
                    .restoration
                    .as_ref()
                    .map_or(ReturnPath::Undescribed, ReturnPath::of),
                restored: line.restored,
            })
            .collect(),
    )
}

/// The Recycle Bin: what was discarded, newest first, each row stating
/// how it comes back.
///
/// There is no restore button. The wire carries no such Command, and a
/// button that does nothing when pressed is worse than an instruction a
/// person can act on - which is what [`ReturnPath::sentence`] gives.
#[component]
pub fn RecycleBinView(
    answer: Option<channels::DiscardAnswer>,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<ClientFrame>,
) -> Element {
    let asked = use_signal(|| false);
    use_effect(move || {
        let mut asked = asked;
        if live() && !asked() {
            asked.set(true);
            on_frame.call(ClientFrame::Query(channels::Query::DiscardView));
        }
    });
    let Some(answer) = answer else {
        return rsx! {
            section { class: "recycle-bin",
                crate::panel::Empty {
                    status: "asking the city what it discarded".to_owned(),
                    what: "the list appears when the answer arrives".to_owned(),
                }
            }
        };
    };
    let rows = bin_rows(&answer);
    let count = rows.len();
    let outstanding = rows.iter().filter(|row| !row.restored).count();
    rsx! {
        section { class: "recycle-bin",
            crate::panel::Panel {
                title: if rows.is_empty() { "nothing has been discarded".to_owned() }
                    else { "what was deleted, and the way back to each of it".to_owned() },
                figure: "{outstanding}",
                scope: "the newest first; the figure counts what has not been taken back yet, and rows that already came back stay listed as evidence that a return path works"
                    .to_owned(),
                source: "folded from the Ledger's file_discarded and discard_restored records; the way back is the Restoration the discard was constructed with"
                    .to_owned(),
            if rows.is_empty() {
                crate::panel::Empty {
                    status: "no run has discarded anything".to_owned(),
                    what: "a deletion in this city cannot be constructed without a way back, so anything that disappears from a worktree lands here carrying the checkpoint or the content address it can be fetched from."
                        .to_owned(),
                }
            }
            for row in rows {
                article {
                    key: "{row.what}",
                    class: if row.restored { "binned back" } else { "binned" },
                    span { class: "what", "{row.what}" }
                    span { class: "way-back", "{row.return_path.sentence()}" }
                    if row.restored {
                        span { class: "note", "already restored" }
                    }
                }
            }
            if count > 0 {
                p { class: "note",
                    "There is no restore button here because the wire has no command that puts one file back. What each row gives is the sentence a person can act on - which checkpoint, or which content address."
                }
            }
            }
        }
    }
}

/// Orders the Recycle Bin newest first: the mistake a person is looking for
/// is almost always the last one they made.
#[must_use]
pub fn recycle_bin(mut rows: Vec<BinRow>) -> Vec<BinRow> {
    rows.sort_by(|left, right| {
        right
            .discarded_at
            .cmp(&left.discarded_at)
            .then_with(|| left.what.cmp(&right.what))
    });
    rows
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
    use channels::{ApprovalId, ApprovalSource, ClusterKey, Restoration};

    fn item(id: &str, detail: &str, created: u64, tainted: bool) -> ApprovalItem {
        ApprovalItem {
            id: ApprovalId::new(id).unwrap(),
            source: ApprovalSource::Gate,
            actor: "resident".to_owned(),
            action_desc: format!("discard files under notes ({id})"),
            artifact: Locator::parse(&format!("file:notes/a.md@{}", "5a".repeat(20))).unwrap(),
            cluster_key: ClusterKey {
                class: channels::ApprovalClass::DiscardEscalate,
                detail: detail.to_owned(),
            },
            created: TimeMs::new(created),
            tainted,
        }
    }

    #[test]
    fn identical_questions_arrive_as_one_decision() {
        let clusters = inbox(vec![
            item("a", "notes/", 30, false),
            item("b", "notes/", 10, false),
            item("c", "notes/", 20, false),
        ]);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].count(), 3);
        assert_eq!(clusters[0].waiting_since(), Some(TimeMs::new(10)));
        let order: Vec<&str> = clusters[0]
            .members
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(order, ["b", "c", "a"], "oldest first inside a group");
    }

    #[test]
    fn a_tainted_item_cannot_be_grouped_with_anything() {
        // C15: one answer must not cover a question the person never read.
        // The tainted item keys on its own id, so there is no key to share.
        let clusters = inbox(vec![
            item("a", "notes/", 10, false),
            item("b", "notes/", 11, true),
            item("c", "notes/", 12, false),
        ]);
        assert_eq!(clusters.len(), 2);
        let lone = clusters
            .iter()
            .find(|c| c.answer_individually)
            .expect("the tainted item stands alone");
        assert_eq!(lone.count(), 1);
        assert_eq!(lone.members[0].id.as_str(), "b");
    }

    #[test]
    fn the_longest_wait_leads_the_list() {
        let clusters = inbox(vec![
            item("a", "later/", 90, false),
            item("b", "earlier/", 5, false),
        ]);
        let leading = clusters.first().expect("two groups");
        assert_eq!(leading.waiting_since(), Some(TimeMs::new(5)));
    }

    #[test]
    fn grouping_is_stable_so_two_people_see_one_list() {
        let make = || {
            vec![
                item("a", "x/", 10, false),
                item("b", "y/", 10, false),
                item("c", "z/", 10, false),
            ]
        };
        let mut reversed = make();
        reversed.reverse();
        assert_eq!(inbox(make()), inbox(reversed));
    }

    #[test]
    fn every_discarded_row_can_say_how_it_comes_back() {
        let paths = [
            Restoration::Tracked(
                Locator::parse(&format!("file:notes/a.md@{}", "5a".repeat(20))).unwrap(),
            ),
            Restoration::Interred(
                Locator::parse(
                    "cas:b3-0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
            ),
            Restoration::Rebuildable {
                reason: "regenerated by the build".to_owned(),
            },
        ];
        for restoration in &paths {
            let path = ReturnPath::of(restoration);
            assert_ne!(path, ReturnPath::Undescribed);
            let sentence = path.sentence();
            assert!(
                sentence.contains("restore") || sentence.contains("rebuild"),
                "a row must name an action: {sentence}"
            );
        }
    }

    #[test]
    fn an_unknown_restoration_scheme_offers_no_button_it_cannot_honour() {
        // Fail-closed for a view: still show the row, never invent the
        // action. A client one version behind must not promise a restore
        // whose behaviour it cannot predict.
        let sentence = ReturnPath::Undescribed.sentence();
        assert!(sentence.contains("Ledger"));
        assert!(!sentence.contains("restore from"));
        assert!(!sentence.starts_with("rebuild"));
    }

    fn discarded(path: &str, at: u64, way: Option<Restoration>) -> channels::DiscardLine {
        channels::DiscardLine {
            path: path.to_owned(),
            restoration: way,
            at: TimeMs::new(at),
            restored: false,
        }
    }

    #[test]
    fn a_wire_row_becomes_a_sentence_through_the_one_place_that_writes_it() {
        // The defect this pins: the server used to compose the sentence
        // itself, so the way back had two authorities and the page had
        // none. The plan now travels as a plan.
        let tracked = Restoration::Tracked(
            Locator::parse(&format!("file:notes/a.md@{}", "5a".repeat(20))).unwrap(),
        );
        let rows = bin_rows(&channels::DiscardAnswer {
            rows: vec![discarded("file:notes/a.md", 10, Some(tracked))],
        });
        assert_eq!(rows.len(), 1);
        assert!(rows[0].return_path.sentence().contains("checkpoint"));
        assert!(!rows[0].restored);
    }

    #[test]
    fn a_plan_this_build_cannot_read_still_gets_a_row_and_no_invented_action() {
        let rows = bin_rows(&channels::DiscardAnswer {
            rows: vec![discarded("file:notes/b.md", 11, None)],
        });
        assert_eq!(rows.len(), 1, "the row is never dropped");
        assert_eq!(rows[0].return_path, ReturnPath::Undescribed);
        assert!(rows[0].return_path.sentence().contains("Ledger"));
    }

    #[test]
    fn a_row_somebody_already_took_back_says_so_instead_of_disappearing() {
        let mut line = discarded("file:notes/c.md", 12, None);
        line.restored = true;
        let rows = bin_rows(&channels::DiscardAnswer { rows: vec![line] });
        assert!(rows[0].restored, "evidence that the way back worked");
    }

    #[test]
    fn the_bin_shows_the_most_recent_mistake_first() {
        let row = |what: &str, at: u64| BinRow {
            what: what.to_owned(),
            discarded_at: TimeMs::new(at),
            restored: false,
            return_path: ReturnPath::Rebuild("make".to_owned()),
        };
        let ordered = recycle_bin(vec![row("old", 1), row("new", 9), row("mid", 5)]);
        let names: Vec<&str> = ordered.iter().map(|r| r.what.as_str()).collect();
        assert_eq!(names, ["new", "mid", "old"]);
    }
}
