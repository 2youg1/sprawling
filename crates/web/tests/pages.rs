// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a person actually sees, asserted against a rendered tree.
//!
//! These render the real client through a `VirtualDom` and read the
//! mutations it produces - tags, static classes, attribute values and
//! every piece of text. The four defects that produced this suite were
//! invisible to every test that called a function instead of rendering
//! the tree that is supposed to call it.
//!
//! It lives beside the client rather than inside it because it exercises
//! nothing private: `Root`, `Snapshot`, `View` and the answer types are
//! the crate's public face, and a suite that only needs that face is an
//! acceptance test rather than a unit test (AGENTS.md: tests use the
//! same doors as production code).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]

use channels::{Address, ApprovalItem, B3Hash, EventKind, EventRecord, Payload};
use channels::{RunId, Seq, UsdMicros};
use dioxus::prelude::*;
use web::Phase;
use web::{Lens, Root, Snapshot, View};
use web::{destinations, latest_run, spend_line, started_here, watchable};

/// A snapshot holding the named sessions, folded from real records.
///
/// The production door and nothing beside it: every row here arrives
/// through [`Snapshot::apply`], so a test that seats a session is also
/// exercising the fold that seats one in a browser. A setter that wrote
/// into `runs` directly would let these tests pass while the fold was
/// broken, which is the failure they exist to catch.
fn seated(rows: &[(Option<&str>, Phase, u64)]) -> Snapshot {
    let mut snapshot = Snapshot::new();
    for (index, (addr, phase, seq)) in rows.iter().enumerate() {
        let mut run = [0u8; 16];
        // The index is what makes two rows two runs. Truncation cannot
        // happen below 256 rows and would only collide two fixtures.
        run[0] = u8::try_from(index).unwrap_or(u8::MAX);
        let id = RunId::from_bytes(run);
        snapshot.apply(&started(id, *addr, *seq));
        for record in ending(id, *phase, seq.saturating_add(1)) {
            snapshot.apply(&record);
        }
    }
    snapshot
}

fn started(run: RunId, addr: Option<&str>, seq: u64) -> EventRecord {
    EventRecord::from_draft(
        channels::EventDraft {
            run,
            t: channels::TimeMs::new(1_000),
            who: "test".to_owned(),
            addr: addr.and_then(|raw| Address::parse(raw).ok()),
            kind: EventKind::RunStarted,
            data: channels::Payload::empty(),
            ig: false,
        },
        Seq::new(seq),
        channels::B3Hash::digest(b"prev"),
    )
}

/// The records that put a run into the phase named, through the same
/// arms a live stream would take.
fn ending(run: RunId, phase: web::Phase, seq: u64) -> Vec<EventRecord> {
    let froze = |completion: &str| {
        let mut data = serde_json::Map::new();
        data.insert(
            "completion".to_owned(),
            serde_json::Value::String(completion.to_owned()),
        );
        vec![EventRecord::from_draft(
            channels::EventDraft {
                run,
                t: channels::TimeMs::new(1_000),
                who: "test".to_owned(),
                addr: None,
                kind: EventKind::RunFrozen,
                data: channels::Payload::new(data).unwrap_or_else(|_| channels::Payload::empty()),
                ig: false,
            },
            Seq::new(seq),
            channels::B3Hash::digest(b"prev"),
        )]
    };
    match phase {
        Phase::Running => Vec::new(),
        Phase::Frozen => froze("done"),
        Phase::Cancelled => froze("cancelled"),
        Phase::Waiting | Phase::Halted => {
            let kind = if phase == Phase::Waiting {
                EventKind::ApprovalRequested
            } else {
                EventKind::CityHalted
            };
            vec![EventRecord::from_draft(
                channels::EventDraft {
                    run,
                    t: channels::TimeMs::new(1_000),
                    who: "test".to_owned(),
                    addr: None,
                    kind,
                    data: channels::Payload::empty(),
                    ig: false,
                },
                Seq::new(seq),
                channels::B3Hash::digest(b"prev"),
            )]
        }
    }
}

fn record(seq: u64, kind: EventKind, run: [u8; 16]) -> EventRecord {
    EventRecord::from_draft(
        kernel_draft(kind, run),
        Seq::new(seq),
        B3Hash::digest(b"prev"),
    )
}

fn kernel_draft(kind: EventKind, run: [u8; 16]) -> channels::EventDraft {
    channels::EventDraft {
        run: RunId::from_bytes(run),
        t: channels::TimeMs::new(1_000),
        who: "test".to_owned(),
        addr: None,
        kind,
        data: Payload::empty(),
        ig: false,
    }
}

/// What a rendered tree actually contains.
///
/// The four failures this whole card repairs were invisible to every
/// existing test because those tests called functions instead of
/// rendering the tree that is supposed to call them. So this walks
/// the mutations a real `VirtualDom` produces: element tags, static
/// classes, and every piece of text a reader would see.
#[derive(Default)]
struct Painted {
    tags: Vec<String>,
    classes: Vec<String>,
    /// Every other static attribute value, so a placeholder - which is
    /// what a form says to a person before they type - is readable
    /// evidence like any other word on the page.
    attrs: Vec<String>,
    text: Vec<String>,
}

impl Painted {
    fn absorb(&mut self, node: &dioxus::dioxus_core::TemplateNode) {
        use dioxus::dioxus_core::{TemplateAttribute, TemplateNode};
        match *node {
            TemplateNode::Element {
                tag,
                attrs,
                children,
                ..
            } => {
                self.tags.push(tag.to_string());
                for attr in attrs {
                    if let TemplateAttribute::Static { name, value, .. } = *attr {
                        if name == "class" {
                            self.classes.push(value.to_string());
                        } else {
                            self.attrs.push(value.to_string());
                        }
                    }
                }
                for child in children {
                    self.absorb(child);
                }
            }
            TemplateNode::Text { text } => self.text.push(text.to_string()),
            TemplateNode::Dynamic { .. } => {}
        }
    }

    fn says(&self, needle: &str) -> bool {
        self.text.iter().any(|line| line.contains(needle))
            || self.attrs.iter().any(|value| value.contains(needle))
    }

    fn has_class(&self, needle: &str) -> bool {
        self.classes.iter().any(|class| class.contains(needle))
    }

    /// Where a piece of text sits in reading order, ignoring what is
    /// only in an attribute. Order is what two of this card's
    /// defects were about, and a placeholder is not a label.
    fn wrote(&self, needle: &str) -> Option<usize> {
        self.text.iter().position(|line| line.contains(needle))
    }
}

impl dioxus::dioxus_core::WriteMutations for Painted {
    fn load_template(
        &mut self,
        template: dioxus::dioxus_core::Template,
        index: usize,
        _id: dioxus::dioxus_core::ElementId,
    ) {
        if let Some(root) = template.roots.get(index) {
            self.absorb(root);
        }
    }

    fn create_text_node(&mut self, value: &str, _id: dioxus::dioxus_core::ElementId) {
        self.text.push(value.to_owned());
    }

    fn set_node_text(&mut self, value: &str, _id: dioxus::dioxus_core::ElementId) {
        self.text.push(value.to_owned());
    }

    fn set_attribute(
        &mut self,
        name: &'static str,
        _ns: Option<&'static str>,
        value: &dioxus::dioxus_core::AttributeValue,
        _id: dioxus::dioxus_core::ElementId,
    ) {
        if let dioxus::dioxus_core::AttributeValue::Text(text) = value {
            if name == "class" {
                self.classes.push(text.clone());
            } else {
                self.attrs.push(text.clone());
            }
        }
    }

    fn append_children(&mut self, _id: dioxus::dioxus_core::ElementId, _m: usize) {}
    fn assign_node_id(&mut self, _path: &'static [u8], _id: dioxus::dioxus_core::ElementId) {}
    fn create_placeholder(&mut self, _id: dioxus::dioxus_core::ElementId) {}
    fn replace_node_with(&mut self, _id: dioxus::dioxus_core::ElementId, _m: usize) {}
    fn replace_placeholder_with_nodes(&mut self, _path: &'static [u8], _m: usize) {}
    fn insert_nodes_after(&mut self, _id: dioxus::dioxus_core::ElementId, _m: usize) {}
    fn insert_nodes_before(&mut self, _id: dioxus::dioxus_core::ElementId, _m: usize) {}
    fn create_event_listener(&mut self, _name: &'static str, _id: dioxus::dioxus_core::ElementId) {}
    fn remove_event_listener(&mut self, _name: &'static str, _id: dioxus::dioxus_core::ElementId) {}
    fn remove_node(&mut self, _id: dioxus::dioxus_core::ElementId) {}
    fn push_root(&mut self, _id: dioxus::dioxus_core::ElementId) {}
}

/// The handlers have to be minted inside a running scope, so the tree
/// is entered through a component rather than by building props by
/// hand.
#[component]
fn Harness(
    view: View,
    snapshot: Snapshot,
    records: Vec<EventRecord>,
    refused: Option<web::Refused>,
) -> Element {
    // Live, because a test that rendered the disconnected client
    // would be asserting about the waiting room rather than the city.
    let live = use_signal(|| true);
    // The language, as `App` provides it in a browser. Without it
    // every component that says a word would be reading a context
    // nobody put there.
    use_context_provider(|| Signal::new(web::Lang::En));
    rsx! {
        Root {
            live,
            snapshot,
            view,
            endpoints: Some(endpoints_answer()),
            city: Some(city_answer()),
            cost: Some(cost_answer()),
            building: Some(building_answer()),
            discards: Some(discard_answer()),
            inbox: None,
            hits: None,
            filed: Some(registry_answer()),
            vitals: Some(metrics_answer()),
            steered: None,
            refused,
            records,
            selected: None,
            dropped: None,
            on_frame: move |_| {},
            on_select: move |_| {},
            on_drop: move |_| {},
            on_view: move |_| {},
            on_dismiss: move |()| {},
        }
    }
}

fn paint(view: View, snapshot: Snapshot, records: Vec<EventRecord>) -> Painted {
    painted_with(view, snapshot, records, None)
}

fn painted_with(
    view: View,
    snapshot: Snapshot,
    records: Vec<EventRecord>,
    refused: Option<web::Refused>,
) -> Painted {
    let mut dom = VirtualDom::new_with_props(
        Harness,
        HarnessProps {
            view,
            snapshot,
            records,
            refused,
        },
    );
    let mut painted = Painted::default();
    dom.rebuild(&mut painted);
    painted
}

fn endpoints_answer() -> channels::EndpointsAnswer {
    channels::EndpointsAnswer {
        endpoints: Vec::new(),
        chosen: Vec::new(),
    }
}

fn city_answer() -> channels::CityAnswer {
    channels::CityAnswer {
        pursuits: Vec::new(),
        runs: Vec::new(),
        active: 0,
        frozen: 0,
        // A city with a building in it, because an empty city
        // exercises the empty state and nothing else.
        buildings: vec![channels::BuildingProgress {
            blocked: Vec::new(),
            ready: 0,
            addr: Address::parse("lab").unwrap(),
            progress: channels::Progress::Planned(channels::PlannedProgress {
                done: 1,
                blocked: 0,
                total: 4,
                done_ppb: 0,
                blocked_ppb: 0,
            }),
            problems: Vec::new(),
        }],
    }
}

fn building_answer() -> channels::BuildingAnswer {
    channels::BuildingAnswer {
        plan: Vec::new(),
        blocked: Vec::new(),
        sandbox: None,
        mcp: Vec::new(),
        addr: Address::parse("lab").unwrap(),
        progress: channels::Progress::Planned(channels::PlannedProgress {
            done: 1,
            blocked: 0,
            total: 4,
            done_ppb: 0,
            blocked_ppb: 0,
        }),
        problems: Vec::new(),
        rooms: vec!["room1".to_owned()],
        docs: vec![channels::BuildingDoc {
            name: "Roadmap.md".to_owned(),
            text: "| # | item |".to_owned(),
            bytes: 12,
            truncated: false,
        }],
        archive: Vec::new(),
    }
}

fn discard_answer() -> channels::DiscardAnswer {
    channels::DiscardAnswer {
        rows: vec![channels::DiscardLine {
            path: "file:lab/room1/notes.md".to_owned(),
            restoration: Some(channels::Restoration::Tracked(
                channels::Locator::parse(&format!("file:lab/room1/notes.md@{}", "5a".repeat(20)))
                    .unwrap(),
            )),
            at: channels::TimeMs::new(900),
            restored: false,
        }],
    }
}

fn metrics_answer() -> channels::MetricsAnswer {
    channels::MetricsAnswer {
        events: 12_400,
        runs_active: 1,
        runs_frozen: 0,
        buildings: 1,
        approvals_waiting: 1,
        signals_waiting: 0,
        discards_outstanding: 1,
    }
}

fn registry_answer() -> channels::RegistryAnswer {
    channels::RegistryAnswer {
        assets: vec![channels::RegistryLine {
            addr: Address::parse("lab/room1").unwrap(),
            kind: "decision".to_owned(),
            subject: "we build without dx".to_owned(),
            at: channels::TimeMs::new(500),
        }],
    }
}

fn cost_answer() -> channels::CostAnswer {
    channels::CostAnswer {
        total: UsdMicros::new(420_000),
        by_run: Vec::new(),
        by_actor: Vec::new(),
        by_segment: Vec::new(),
        by_tool: vec![("exec".to_owned(), UsdMicros::new(420_000))],
        by_skill: Vec::new(),
    }
}

fn waiting_item() -> ApprovalItem {
    ApprovalItem {
        id: channels::ApprovalId::new("item-7".to_owned()).unwrap(),
        source: channels::ApprovalSource::Gate,
        actor: "urbanite-2".to_owned(),
        action_desc: "push to the remote".to_owned(),
        artifact: channels::Locator::parse(
            "file:lab/room1@0000000000000000000000000000000000000000",
        )
        .unwrap(),
        cluster_key: channels::ClusterKey {
            class: channels::ApprovalClass::AgentQuestion,
            detail: "lab".to_owned(),
        },
        created: channels::TimeMs::new(1_000),
        tainted: false,
    }
}

/// What each view must put on the page, as an exhaustive match.
///
/// A hand-written list of views would be a second authority for
/// "which views exist", and the variant somebody forgets to add is
/// exactly the one that ships as an empty div. This match does not
/// compile until a new variant states what it draws.
fn evidence_of(view: &View) -> Vec<(&'static str, &'static str)> {
    match *view {
        // The box work starts in, and the table its rows land in.
        // If either renders nothing, the first screen is a blank one.
        View::Sessions => vec![
            ("composer", "what needs doing?"),
            ("composer-plan", "send it to"),
        ],
        // The head's four facts, and the tabs under them. The third
        // fact is the em rule, which is asserted in `web::session`
        // against a value rather than against markup.
        View::Session(_) => vec![("session-head", "all sessions"), ("session-tabs", "turns")],
        View::Waiting => vec![("approvals", "push to the remote")],
        // The lens switch, because the record is one page with three
        // readings and a page that cannot change lens is one reading.
        View::Record(lens) => match lens {
            Lens::Ledger => vec![
                ("session-tabs", "the archive"),
                ("ledger", "sprawling replay"),
            ],
            Lens::Archive => vec![
                ("session-tabs", "the ledger"),
                ("archive-search", "filed lately"),
            ],
            Lens::Bin => vec![
                ("session-tabs", "the ledger"),
                ("recycle-bin", "the way back to each of it"),
            ],
        },
        View::Cost => vec![("dashboard", "exec")],
        View::Setup => vec![
            ("settings", "put the key in the vault"),
            ("subscription", "start the login"),
        ],
        // The room tab, because a room is a face of a building and
        // the page lists them in exactly one place.
        View::Building(_) => vec![("building", "Roadmap.md"), ("room", "room1/")],
        // An old link, before the fold that names its room arrives.
        View::Run(_) => vec![("panel", "asking the city what it holds")],
    }
}

#[test]
fn every_destination_in_the_nav_reaches_a_page_that_shows_something() {
    // The defect this pins down: four of the six views rendered an
    // empty `div` and the nav had no links at all, while every
    // module test stayed green because it called the module's pure
    // functions directly. A page is not mounted until the tree says
    // so, and this is the only test in the crate that asks the tree.
    let mut snapshot = seated(&[(Some("lab/room1"), Phase::Running, 1)]);
    snapshot.adopt_approvals(vec![waiting_item()]);
    let records = vec![record(2, EventKind::ToolCalled, [0u8; 16])];

    let every_view = [
        View::Sessions,
        View::Session(Address::parse("lab/room1").unwrap()),
        View::Waiting,
        View::Record(Lens::Ledger),
        View::Record(Lens::Archive),
        View::Record(Lens::Bin),
        View::Cost,
        View::Setup,
        View::Building(Address::parse("lab").unwrap()),
    ];

    for (view, (marker, sentence)) in every_view
        .iter()
        .flat_map(|view| evidence_of(view).into_iter().map(move |ev| (view, ev)))
    {
        let painted = paint(view.clone(), snapshot.clone(), records.clone());

        assert!(
            painted.has_class(marker),
            "{view:?} rendered no element of its own"
        );
        assert!(
            painted.says(sentence),
            "{view:?} rendered nothing a reader could read: wanted {sentence:?}"
        );
    }
}

#[test]
fn work_with_no_price_is_never_rendered_as_a_column_of_zeroes() {
    // A real provider on a subscription reports what it used and not
    // what it cost, so the authoritative total is zero while four runs
    // sit in the attribution. Rendering that as $0.00 five times over
    // is the interface answering a question nobody can answer.
    let painted = paint(View::Cost, Snapshot::new(), Vec::new());
    if painted.says("no provider reported a price") {
        assert!(
            painted.says("unpriced"),
            "the rows say so too, rather than each showing a zero"
        );
    } else {
        assert!(
            painted.says("where the money went") || painted.says("nothing has been spent"),
            "a cost page states one of the three cases and no fourth"
        );
    }
}

#[test]
fn every_verb_this_client_offers_is_one_the_city_executes() {
    // The rule this asserts is the one the audit produced: a control
    // whose command reaches `assembly`'s catch-all can only ever
    // produce a refusal, and a button that cannot succeed is worse
    // than a missing one. Takeover, Rollback and CreatePolicy were
    // all offered and all unexecuted.
    let snapshot = seated(&[(Some("lab/room1"), Phase::Running, 1)]);
    let session = paint(
        View::Session(Address::parse("lab/room1").unwrap()),
        snapshot.clone(),
        vec![record(2, EventKind::ToolCalled, [0u8; 16])],
    );
    assert!(session.says("branch a new run from step"), "Fork");
    assert!(session.says("send at the next safe point"), "Steer");
    assert!(session.says("stop this session"), "Cancel");
    assert!(
        !session.says("answer for this run from here"),
        "Takeover has no executor, so it may not be offered"
    );
    let waiting = paint(View::Waiting, snapshot, Vec::new());
    assert!(
        !waiting.says("and stop asking"),
        "CreatePolicy has no executor, so it may not be offered"
    );
}

#[test]
fn the_city_is_drawn_as_shapes_a_person_can_reach() {
    // What a canvas could not be asked. Before F2.02 the drawing
    // existed only on wasm, so no host test could see whether the
    // picture had been drawn at all - which is how it once shipped
    // painting the ground and no buildings.
    let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
    assert!(painted.tags.iter().any(|tag| tag == "svg"));
    assert!(painted.tags.iter().any(|tag| tag == "polygon"));
    assert!(
        painted.tags.iter().any(|tag| tag == "text"),
        "a tower says its own name; there is no legend to look away to"
    );
    assert!(
        painted.has_class("prism"),
        "each building is one group, which is what hover, focus and a keyboard reach"
    );
    assert!(
        painted.attrs.iter().any(|value| value == "button"),
        "and the group says it is a button, so a screen reader can say so too"
    );
}

#[test]
fn every_page_says_where_its_numbers_came_from() {
    // The rule this holds is the product's own claim turned into a
    // property of the interface: a city whose whole promise is an
    // auditable Ledger may not put a figure on screen without saying
    // what produced it. It is asserted here, over every view at once,
    // rather than in `panel` - what matters is not that the markup
    // renders but that no page escapes it.
    let mut snapshot = seated(&[(Some("lab/room1"), Phase::Running, 1)]);
    snapshot.adopt_approvals(vec![waiting_item()]);
    for view in [
        View::Sessions,
        View::Session(Address::parse("lab/room1").unwrap()),
        View::Waiting,
        View::Record(Lens::Ledger),
        View::Record(Lens::Archive),
        View::Record(Lens::Bin),
        View::Cost,
        View::Setup,
        View::Building(Address::parse("lab").unwrap()),
    ] {
        let painted = paint(view.clone(), snapshot.clone(), Vec::new());
        assert!(
            painted.has_class("panel-source"),
            "{view:?} states something without saying where it came from"
        );
        assert!(
            painted.has_class("panel-title"),
            "{view:?} has no heading, so a reader cannot tell which page they are on"
        );
    }
}

#[test]
fn a_begun_login_puts_the_url_on_the_page_and_a_finished_one_takes_it_away() {
    let mut snapshot = Snapshot::new();
    assert!(
        paint(View::Setup, snapshot.clone(), Vec::new()).says("no login is waiting"),
        "a page with no login pending says so rather than showing an empty box"
    );

    let mut data = serde_json::Map::new();
    data.insert(
        "provider".to_owned(),
        serde_json::Value::String("anthropic".to_owned()),
    );
    data.insert(
        "auth_url".to_owned(),
        serde_json::Value::String("https://example.invalid/authorize?state=x".to_owned()),
    );
    let mut draft = kernel_draft(EventKind::LoginStarted, [2u8; 16]);
    draft.data = Payload::new(data).unwrap();
    let begun = EventRecord::from_draft(draft, Seq::new(3), B3Hash::digest(b"prev"));
    snapshot.apply(&begun);
    let painted = paint(View::Setup, snapshot.clone(), Vec::new());
    assert!(
        painted.says("https://example.invalid/authorize?state=x"),
        "the url a person must open is the one the server recorded"
    );
    assert!(painted.says("finish the login"));

    snapshot.apply(&record(4, EventKind::SecretCaptured, [2u8; 16]));
    assert!(
        paint(View::Setup, snapshot, Vec::new()).says("no login is waiting"),
        "a credential in the vault ends the step that was asking for it"
    );
}

#[test]
fn the_left_nav_carries_every_destination_and_says_how_many_wait() {
    let mut snapshot = Snapshot::new();
    snapshot.adopt_approvals(vec![waiting_item()]);
    let painted = paint(View::Sessions, snapshot.clone(), Vec::new());
    // In the language the harness renders in, which is this
    // client's own: what matters here is that every destination
    // reaches the page, and `lang` holds that both languages exist.
    let word = |msg| web::say(web::Lang::En, msg);
    for spot in destinations(&snapshot) {
        assert!(
            painted.says(word(spot.label)),
            "the nav does not offer {:?}",
            spot.label
        );
    }
    assert!(
        painted.has_class("badge"),
        "one item waits and none is shown"
    );
}

/// The bar a person writes work into is a drop target.
///
/// Before this it was not, and the browser's own default was
/// answering the gesture instead: a bare text input accepts a
/// `text/plain` drop without anybody electing it, so a dragged
/// selection went in raw and `drop::read` never ran. Cancelling
/// `dragover` is what takes the gesture back.
#[test]
fn work_can_be_aimed_by_dropping_onto_the_box_it_is_written_in() {
    let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
    assert!(
        painted.has_class("composer-task"),
        "the box work is written in is not on the page: {:?}",
        painted.classes
    );
    assert!(
        painted
            .attrs
            .iter()
            .any(|value| value.contains("measure every read path")),
        "the placeholder is not one whole real task, so it teaches nothing about size"
    );
}

/// Every drop zone must be able to say "a drag is over me" without a
/// hover rule, because device input events are suppressed for the
/// whole of a drag and a hover rule therefore never lights.
#[test]
fn a_drop_zone_reports_a_drag_through_events_and_not_through_hover() {
    let source = include_str!("../assets/app.css");
    assert!(
        source.contains(".drop-zone.over"),
        "a drop zone has no drag state to show"
    );
    // Read at compile time, so this does not depend on which directory
    // the runner happened to start in.
    //
    // The two modules that draw one. `app.rs` used to be on this list
    // and passed on its own text: the needle was the literal in this
    // assertion, and the assertion lived in that file. Moving this suite
    // out of the crate is what made that visible.
    for (name, wired) in [
        ("live.rs", include_str!("../src/live.rs")),
        ("building_view.rs", include_str!("../src/building_view.rs")),
    ] {
        assert!(
            wired.contains("ondragenter") && wired.contains("ondragleave"),
            "{name} carries a drop zone it never lights"
        );
    }
}

#[test]
fn the_box_that_starts_work_asks_for_work_and_not_for_a_budget() {
    // A person cannot say what a task is worth before it runs, and a
    // subscription has no unit price to say it in (user verdict,
    // 2026-08-22). Whatever the box shows, it never shows a price.
    let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
    assert!(painted.has_class("panel composer"));
    assert!(
        !painted.says("budget") && !painted.says("how much"),
        "the box that starts work asks for money: {:?} / {:?}",
        painted.text,
        painted.attrs
    );
}

#[test]
fn a_call_with_no_reported_price_is_never_rendered_as_zero_dollars() {
    let mut snapshot = Snapshot::new();
    let mut data = serde_json::Map::new();
    data.insert(
        "usage".to_owned(),
        serde_json::json!({ "input_tokens": 40_000, "output_tokens": 8_207 }),
    );
    snapshot.apply(&EventRecord::from_draft(
        channels::EventDraft {
            run: RunId::from_bytes([9u8; 16]),
            t: channels::TimeMs::new(1),
            who: "urbanite-1".to_owned(),
            addr: None,
            kind: EventKind::ModelReturned,
            data: channels::Payload::new(data).unwrap(),
            ig: false,
        },
        Seq::new(1),
        channels::B3Hash::digest(b"prev"),
    ));
    let line = spend_line(web::Lang::En, &snapshot);
    assert!(!line.contains('$'), "{line}");
    assert!(line.contains("48.2k tokens"), "{line}");
    assert!(line.contains("no price reported"), "{line}");
    assert_eq!(snapshot.usage().unpriced_calls, 1);
}

/// The defect this card exists for, seen from the end that matters:
/// somebody presses attach, the city refuses, and the page has to
/// say so. Until this test existed the client received the refusal
/// frame and dropped it, and no page anywhere in this crate could
/// have shown one.
#[test]
fn a_refusal_is_on_the_page_with_the_way_out_beside_it() {
    let told = web::refused(
        web::Lang::En,
        &channels::AxError::failure(
            channels::AxCode::ConfigInvalid,
            "attach an endpoint",
            "modelscope",
        )
        .with_recovery("the base url needs its /v1"),
    );
    let painted = painted_with(View::Sessions, Snapshot::new(), Vec::new(), Some(told));
    assert!(
        painted.classes.iter().any(|c| c == "refusal"),
        "the refusal has nowhere to appear: {:?}",
        painted.classes
    );
    // A refusal a person can read is one they can act on, so the
    // way out is on screen beside what was refused.
    for part in ["refusal-what", "refusal-way"] {
        assert!(
            painted.classes.iter().any(|c| c == part),
            "{part} is missing from the page"
        );
    }
}

/// A city that has refused nothing draws no strip at all: a banner
/// that is always there is a banner nobody reads.
#[test]
fn a_page_with_nothing_refused_carries_no_strip() {
    let painted = painted_with(View::Sessions, Snapshot::new(), Vec::new(), None);
    assert!(!painted.classes.iter().any(|c| c == "refusal"));
}

/// The order of the settings page, which is the order a person can
/// perform its steps in.
///
/// The city this card was cut from had a key in its vault, two
/// buildings, and no endpoint: the one section that makes the other
/// three non-empty sat last, below the fold, with its own submit
/// button off-screen.
/// The run a person just started is the one they are taken to.
///
/// Not a guess between several runs - the client sent this dispatch
/// and knows which room it asked for, so recognising the start of
/// that run is knowledge rather than a coin toss (web-SPEC 8-31).
#[test]
fn the_session_a_person_just_started_is_the_one_they_are_shown() {
    let started = |addr: &str, run: [u8; 16]| {
        let mut draft = kernel_draft(EventKind::RunStarted, run);
        draft.addr = Some(Address::parse(addr).unwrap());
        EventRecord::from_draft(draft, Seq::new(1), B3Hash::digest(b"prev"))
    };
    let mine = started("lab/refactor", [4u8; 16]);
    assert_eq!(
        started_here(&mine, "lab/refactor"),
        Some(RunId::from_bytes([4u8; 16]))
    );
    // The city suffixes a name that is taken, so the room it opened
    // is not always the room that was asked for.
    let suffixed = started("lab/refactor-2", [5u8; 16]);
    assert_eq!(
        started_here(&suffixed, "lab/refactor"),
        Some(RunId::from_bytes([5u8; 16]))
    );
    // Somebody else's work, and a name that merely begins the same
    // way, are not this person's session.
    assert_eq!(
        started_here(&started("lab/other", [6u8; 16]), "lab/refactor"),
        None
    );
    assert_eq!(
        started_here(&started("lab/refactoring", [7u8; 16]), "lab/refactor"),
        None
    );
    // Only the start of a run: a later event in that room is not a
    // second reason to move the page somebody may have navigated
    // away from.
    let mut later = kernel_draft(EventKind::ToolCalled, [4u8; 16]);
    later.addr = Some(Address::parse("lab/refactor").unwrap());
    let later = EventRecord::from_draft(later, Seq::new(2), B3Hash::digest(b"prev"));
    assert_eq!(started_here(&later, "lab/refactor"), None);
}

/// A person standing on a building's page can start work there.
#[test]
fn a_building_page_offers_to_start_a_session_in_that_building() {
    let painted = paint(
        View::Building(Address::parse("lab").unwrap()),
        Snapshot::new(),
        Vec::new(),
    );
    assert!(
        painted.says("start a session here"),
        "the only way to work in this building is a bar on another page"
    );
}

/// A session is picked by the name its person gave it. The picker
/// offered `d41d8cd9 · running`, which identifies a run to a machine
/// and nothing at all to the person who started it.
#[test]
fn a_session_is_offered_by_its_name_and_not_by_its_hash() {
    let mut snapshot = Snapshot::new();
    let mut draft = kernel_draft(EventKind::RunStarted, [3u8; 16]);
    draft.addr = Some(Address::parse("lab/refactor-the-ledger").unwrap());
    snapshot.apply(&EventRecord::from_draft(
        draft,
        Seq::new(1),
        B3Hash::digest(b"prev"),
    ));
    let _run = latest_run(&snapshot).expect("one run started");

    let offered = watchable(&snapshot);
    let (_, label) = offered.first().expect("the run is offered");
    assert!(
        label.contains("refactor-the-ledger"),
        "the picker does not name the session: {label}"
    );

    let painted = paint(
        View::Session(Address::parse("lab/refactor-the-ledger").unwrap()),
        snapshot,
        Vec::new(),
    );
    assert!(
        painted.says("refactor-the-ledger"),
        "the page being read does not say which session it is"
    );
}

/// The session list was flat, so a person watching a city where a
/// run had handed work down saw two peers and no way to tell which
/// answered for which.
#[test]
fn work_handed_down_is_listed_under_the_run_that_handed_it_down() {
    let mut snapshot = Snapshot::new();
    let parent = RunId::from_bytes([3u8; 16]);
    let mut opened = kernel_draft(EventKind::RunStarted, [3u8; 16]);
    opened.addr = Some(Address::parse("lab/room1").unwrap());
    snapshot.apply(&EventRecord::from_draft(
        opened,
        Seq::new(1),
        B3Hash::digest(b"prev"),
    ));

    let mut handed = kernel_draft(EventKind::RunStarted, [4u8; 16]);
    handed.addr = Some(Address::parse("lab/helper").unwrap());
    let mut data = serde_json::Map::new();
    data.insert(
        "parent".to_owned(),
        serde_json::Value::String(parent.to_string()),
    );
    handed.data = Payload::new(data).unwrap();
    snapshot.apply(&EventRecord::from_draft(
        handed,
        Seq::new(2),
        B3Hash::digest(b"prev2"),
    ));

    let offered = watchable(&snapshot);
    let labels: Vec<&str> = offered.iter().map(|(_, label)| label.as_str()).collect();
    assert_eq!(labels.len(), 2);
    assert!(
        labels[0].starts_with("room1"),
        "the run that asked comes first: {labels:?}"
    );
    assert!(
        labels[1].starts_with("\u{21b3} helper (room1)"),
        "the delegate is listed under it and says whose it is: {labels:?}"
    );
}

#[test]
fn the_settings_page_leads_with_the_step_a_new_city_cannot_skip() {
    let painted = paint(View::Setup, Snapshot::new(), Vec::new());
    let attach = painted
        .wrote("Attach a provider")
        .expect("the page never offers to attach a provider");
    let choose = painted
        .wrote("choose a model for a job")
        .expect("the page never offers to choose a model");
    let tags = painted
        .wrote("what each model is for")
        .expect("the page never says what each tag is for");
    assert!(
        attach < choose && attach < tags,
        "the first step is not first: attach {attach}, choose {choose}, tags {tags}"
    );
}

/// At rest the box asks one question, and everything else it needs
/// is written out as a sentence a person can disagree with.
///
/// The bar this replaced stood seven controls open at once, which
/// asked a person to read the whole grammar of a dispatch before
/// writing one word of it.
#[test]
fn at_rest_the_box_is_one_field_and_one_sentence() {
    let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
    let fields = painted
        .classes
        .iter()
        .filter(|held| *held == "composer-task" || *held == "composer-field")
        .count();
    assert_eq!(fields, 1, "more than one control stands open at rest");
    for word in ["send it to ", "as ", "think "] {
        assert!(
            painted.text.iter().any(|line| line == word),
            "the inferred sentence does not say {word:?}: {:?}",
            painted.text
        );
    }
}

/// Nothing is hidden and nothing is asked.
///
/// The replaced bar folded four controls behind a "more" disclosure,
/// which is the same defect wearing a control: a page that hides what
/// it decided is a page answering on the reader's behalf. Every
/// decision is on screen, as a word that can be clicked.
#[test]
fn every_decision_the_city_made_is_on_screen_and_can_be_changed() {
    let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
    assert!(
        !painted.text.iter().any(|line| line == "more"),
        "something is folded away: {:?}",
        painted.text
    );
    let words = painted
        .classes
        .iter()
        .filter(|held| *held == "guess" || *held == "chosen")
        .count();
    assert_eq!(words, 3, "the sentence does not offer all three decisions");
}

/// Stopping is not the same kind of act as starting, and it used to
/// sit against the button a person's hand is already moving towards.
///
/// Only the dress is asserted here. Where the control sits is a fact
/// about the document, and this harness reads one template at a time
/// in the order the differ loads them, so an index taken from the top
/// bar cannot be compared with one taken from the control surface -
/// the placement was checked by looking at the running client.
#[test]
fn stopping_the_city_is_never_dressed_as_the_thing_that_starts_work() {
    let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
    assert!(painted.says("stop the city"), "the city cannot be stopped");
    assert!(
        painted.has_class("quiet"),
        "the halt control is dressed as a primary action: {:?}",
        painted.classes
    );
    assert!(
        painted.has_class("city-state"),
        "stopping the city stands away from the box that starts work"
    );
}
