// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Which region of the page shows what, and the client that mounts it.
//!
//! [`Root`] is the whole of the region rule: given a view and the
//! answers a page needs, it renders one page into the middle and the
//! same nav, status strip and alert strip around every one of them. It
//! decides nothing about the city — every judgement it applies belongs
//! to a module that can be tested without a renderer.
//!
//! [`App`] is the live client: it holds the snapshot the stream folds
//! into and renders `Root` against it. Every judgement about the
//! connection belongs to `socket::Link`, and every judgement about what
//! an event means belongs to `Snapshot::apply`; this component only
//! holds the two together.
//!
//! What a browser contributes — the address bar, the keyboard, the
//! socket, the animation frame — is `web::mount`'s.

use channels::{Address, EventRecord};
use dioxus::prelude::*;

use crate::app::{ProviderHealth, Snapshot};
use crate::asking::{room_asked_for, watchable};
use crate::command::DEFAULT_EFFORT;
use crate::lang::Msg;
#[cfg(not(target_arch = "wasm32"))]
use crate::mount::Outbound;
use crate::mount::{Keyboard, follow_the_address_bar, listen_for_keys};
#[cfg(target_arch = "wasm32")]
use crate::mount::{Wiring, connect};
use crate::phase::Phase;
use crate::readout::{page_named, standing_of};
use crate::route::View;
use crate::route::{destinations, showing};

/// The root: three regions, and nothing that decides anything.
///
/// Business state is the server's; this reads a snapshot handed to it.
///
/// **Five regions became three.** The right-hand column carried a
/// provider status that read "normal" almost always, and a steady
/// "everything is fine" is the absence of a problem rather than a fact:
/// it never changed anybody's next action, so it does not stay on
/// screen. The three counts it also held do change the next action, so
/// they moved into the top bar. The footer held a dispatch bar, which
/// now stands at the top of the table its rows land in.
#[component]
pub fn Root(
    snapshot: Snapshot,
    view: View,
    endpoints: Option<channels::EndpointsAnswer>,
    city: Option<channels::CityAnswer>,
    cost: Option<channels::CostAnswer>,
    building: Option<channels::BuildingAnswer>,
    discards: Option<channels::DiscardAnswer>,
    inbox: Option<channels::InboxAnswer>,
    hits: Option<channels::ArchiveAnswer>,
    filed: Option<channels::RegistryAnswer>,
    /// What the city last refused this person, if anything. Cleared by
    /// the person, never by the passage of time: an answer that fades
    /// before it is read is an answer nobody gave.
    refused: Option<crate::alert::Refused>,
    records: Vec<EventRecord>,
    selected: Option<String>,
    /// A task line a drop wrote, on its way to the composer.
    dropped: Option<String>,
    /// A line a drop wrote into an open session's box. Separate from
    /// `dropped` because the two boxes take different gestures: aiming
    /// new work, and saying something into work already running.
    steered: Option<String>,
    /// What this city has written down, counted. Read by the record
    /// page, which is the page those counts are about.
    vitals: Option<channels::MetricsAnswer>,
    /// What the open session changed on disk, once the server has said.
    changes: Option<channels::ChangesAnswer>,
    /// Whether frames are flowing yet.
    ///
    /// A page asks its question when it mounts, and the first mount
    /// happens before the socket has finished its handshake - a frame
    /// sent then is dropped, by design, because a queue would be a second
    /// place where "what did the person ask for" lives. So the pages
    /// watch this instead and ask again the moment there is somebody to
    /// ask. Without it the first page a person sees says "asking the city
    /// what it holds" forever.
    live: Signal<bool>,
    on_frame: EventHandler<channels::ClientFrame>,
    on_select: EventHandler<Option<String>>,
    /// Where a gesture goes. One handler for every drop zone, so what a
    /// drag means is answered once.
    on_drop: EventHandler<(crate::drop::Target, crate::drop::Dropped)>,
    on_view: EventHandler<View>,
    on_dismiss: EventHandler<()>,
) -> Element {
    // The language every word on this page is said in. One signal for
    // the whole tree rather than a prop through twenty components: what
    // a person reads in is a fact about the page, not about a panel.
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: crate::lang::Msg| crate::lang::say(lang(), msg);
    let spots = destinations(&snapshot);
    let counts = crate::sessions::counts_said(lang(), &snapshot);
    let unwell = !matches!(snapshot.provider(), ProviderHealth::Healthy);

    // An old link naming a run is resolved here, where the room is
    // known, and the address bar is rewritten to the name a person can
    // read. The router stays pure; the redirect happens once, in the one
    // place that holds the fact it needs.
    let resolving = view.clone();
    let resolved = crate::session::room_for_link(&snapshot, &resolving);
    use_effect(use_reactive!(|(resolved,)| {
        if let Some(landed) = resolved.clone() {
            on_view.call(landed);
        }
    }));

    rsx! {
        main { class: "layout",
            header { class: "top-bar",
                span { class: "address", "{page_named(lang(), &view)}" }
                // Only when it is not normal. A steady "provider: fine"
                // is a problem's absence, and an absence that occupies a
                // permanent line teaches a reader to stop reading it.
                if unwell {
                    span { class: "unwell",
                        if matches!(snapshot.provider(), ProviderHealth::Unknown) {
                            "{word(crate::lang::Msg::CityUnwell)}"
                        } else {
                            "{word(snapshot.provider().word())}"
                        }
                    }
                }
                if let Some(told) = refused.clone() {
                    div { class: "refusal", role: "alert",
                        span { class: "refusal-code", "{told.code}" }
                        span { class: "refusal-what", "{told.what}" }
                        span { class: "refusal-way", "{told.recovery}" }
                        button {
                            class: "refusal-close",
                            "aria-label": "{word(crate::lang::Msg::AlertDismiss)}",
                            onclick: move |_| on_dismiss.call(()),
                            "\u{00d7}"
                        }
                    }
                }
                span { class: "counts",
                    for said in counts {
                        span { key: "{said}", "{said}" }
                    }
                }
            }
            nav { class: "left-nav",
                // Anchors, not buttons. Writing the fragment is the only
                // way a view changes, so an `<a href>` is already a whole
                // navigation - and it arrives with the keyboard, the
                // middle click, "copy link address" and the link role a
                // screen reader announces, none of which a button with an
                // onclick would have had.
                div { class: "nav-group",
                    for spot in spots {
                        a {
                            key: "{spot.label:?}",
                            class: "nav-item",
                            href: "{crate::route::to_fragment(&spot.view)}",
                            "aria-current": if showing(&spot.view, &view) { "page" } else { "false" },
                            "{word(spot.label)}"
                            if let Some(waiting) = spot.waiting {
                                span { class: "badge", "{waiting}" }
                            }
                        }
                    }
                }
                // What this whole city is doing, at the foot of the
                // column that names its parts. Stopping it left the top
                // bar because it stood beside the send button, which is
                // the one place a person's hand is already moving fast.
                div { class: "city-state",
                    p { class: "standing", "{word(standing_of(&snapshot))}" }
                    button {
                        class: "quiet",
                        r#type: "button",
                        onclick: move |_| on_frame.call(crate::command::halt(!snapshot.is_halted())),
                        if snapshot.is_halted() {
                            "{word(crate::lang::Msg::ReleaseCity)}"
                        } else {
                            "{word(crate::lang::Msg::HaltCity)}"
                        }
                    }
                }
            }
            section { class: "centre",
                match view {
                    // A run named by an old link, while the fold that
                    // says which room it is in has not arrived. Said
                    // rather than left blank: the link is not broken, the
                    // answer is not here yet.
                    View::Run(_) => rsx! {
                        crate::panel::Panel {
                            title: word(crate::lang::Msg::AskingWhatItHolds).to_owned(),
                            scope: None,
                            figure: None,
                            source: word(crate::lang::Msg::SessionSource).to_owned(),
                        }
                    },
                    View::Sessions => rsx! {
                        crate::sessions::SessionsView {
                            snapshot: snapshot.clone(),
                            city: city.clone(),
                            endpoints: endpoints.clone(),
                            effort: DEFAULT_EFFORT.to_owned(),
                            dropped: dropped.clone(),
                            live,
                            on_frame,
                            on_view,
                            on_drop,
                        }
                    },
                    View::Session(ref addr) => rsx! {
                        crate::session::SessionView {
                            addr: addr.clone(),
                            snapshot: snapshot.clone(),
                            records: records.clone(),
                            changes: changes.clone(),
                            cost: cost.clone(),
                            building: building.clone(),
                            steered: steered.clone(),
                            live,
                            on_frame,
                            on_drop,
                        }
                    },
                    View::Waiting => rsx! {
                        crate::waiting::WaitingView {
                            snapshot: snapshot.clone(),
                            live,
                            on_frame,
                        }
                    },
                    View::Record(lens) => rsx! {
                        crate::record::RecordView {
                            lens,
                            records: records.clone(),
                            hits: hits.clone(),
                            filed: filed.clone(),
                            discards: discards.clone(),
                            vitals: vitals.clone(),
                            live,
                            on_frame,
                        }
                    },
                    View::Cost => rsx! {
                        crate::dashboard::CostsView {
                            answer: cost.clone(),
                            usage: snapshot.usage(),
                            spent: snapshot.spent(),
                            live,
                            on_frame,
                        }
                    },
                    View::Setup => rsx! {
                        crate::settings::Settings {
                            answer: endpoints.clone(),
                            login_url: snapshot.login_url().map(str::to_owned),
                            served: snapshot.served().clone(),
                            live,
                            on_frame,
                        }
                    },
                    View::Building(ref addr) => rsx! {
                        crate::building_view::BuildingView {
                            addr: addr.clone(),
                            answer: building.clone(),
                            pursuits: crate::command::pursuits_of(city.as_ref()),
                            inbox: inbox.clone(),
                            signals: snapshot.signals_seen(),
                            live,
                            on_frame,
                            on_select,
                            on_drop,
                        }
                    },
                }
            }
        }
    }
}

/// The live client: it holds the snapshot the stream folds into, and
/// renders [`Root`] against it. Every judgement about the connection
/// belongs to `socket::Link`; every judgement about what an event means
/// belongs to `Snapshot::apply`. This component only holds the two
/// together and decides nothing itself.
#[component]
pub fn App() -> Element {
    // Provided before anything renders, because every component below
    // reads it. The first value is the browser's own setting: a person
    // whose machine is in Chinese should not have to find a switch to
    // be spoken to in Chinese.
    use_context_provider(|| Signal::new(crate::lang::preferred()));
    // Read back rather than kept from the line above: the signal is the
    // one authority for what language this page reads in, and the
    // listeners below say their words when they fire, not when they are
    // registered - so a person who switches language mid-session is
    // answered in the new one.
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let snapshot = use_signal(Snapshot::new);
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            unused_mut,
            reason = "in a browser the address bar moves the signal, not this handle"
        )
    )]
    let mut view = use_signal(View::default);
    // The address bar is the authority for which page is showing, and
    // the listener below is the only thing that moves the signal. A
    // click writes the fragment and hears its own change back, so a
    // click and the browser's back button travel the same path and
    // cannot disagree about where the person is.
    let endpoints = use_signal(|| None::<channels::EndpointsAnswer>);
    let city = use_signal(|| None::<channels::CityAnswer>);
    let cost = use_signal(|| None::<channels::CostAnswer>);
    let building = use_signal(|| None::<channels::BuildingAnswer>);
    let discards = use_signal(|| None::<channels::DiscardAnswer>);
    let inbox = use_signal(|| None::<channels::InboxAnswer>);
    let hits = use_signal(|| None::<channels::ArchiveAnswer>);
    let filed = use_signal(|| None::<channels::RegistryAnswer>);
    let records = use_signal(Vec::<EventRecord>::new);
    let mut refused = use_signal(|| None::<crate::alert::Refused>);
    // The address bar is the authority for which page is showing, and the
    // listener below is the only thing that moves the signal, so a click
    // and the browser's back button travel one path and cannot disagree
    // about where the person is. A fragment this build cannot resolve
    // becomes a refusal rather than a silent landing on the first page.
    follow_the_address_bar(view, refused, lang);
    let mut selected = use_signal(|| None::<String>);
    let mut dropped = use_signal(|| None::<String>);
    // A line a drop wrote into the session's box, held here for the same
    // reason `dropped` is: the box belongs to a view that a drop can
    // reach from outside it.
    // What the open session changed on disk. An answer, so it is held
    // beside the others and a reload asks again rather than trusting it.
    let changes = use_signal(|| None::<channels::ChangesAnswer>);
    let live = use_signal(|| false);
    // What the keyboard opened. Held here rather than inside `Root`
    // because the listener that sets them is registered once for the
    // window, and a page redraw must not take a reader's palette away.
    let mut palette = use_signal(|| false);
    let mut keymap = use_signal(|| false);
    listen_for_keys(Keyboard {
        chord: use_signal(crate::keys::Chord::default),
        palette,
        keymap,
        view,
        refused,
    });
    // The room the last dispatch asked for, so its run can be opened
    // when it starts rather than left for the person to find among the
    // others.
    // What the record page's ledger lens states about the whole history.
    let vitals = use_signal(|| None::<channels::MetricsAnswer>);
    // A line a drop wrote into an open session's box, on its way there.
    let mut steered = use_signal(|| None::<String>);
    let mut expecting = use_signal(|| None::<String>);
    #[cfg(target_arch = "wasm32")]
    let outbound = connect(Wiring {
        snapshot,
        endpoints,
        city,
        cost,
        building,
        discards,
        inbox,
        hits,
        filed,
        vitals,
        changes,
        records,
        live,
        view,
        expecting,
        refused,
        lang,
    });
    #[cfg(not(target_arch = "wasm32"))]
    let outbound = Outbound;
    rsx! {
        Root {
            snapshot: snapshot(),
            view: view(),
            endpoints: endpoints(),
            city: city(),
            cost: cost(),
            building: building(),
            discards: discards(),
            inbox: inbox(),
            hits: hits(),
            filed: filed(),
            refused: refused(),
            records: records(),
            selected: selected(),
            dropped: dropped(),
            steered: steered(),
            vitals: vitals(),
            changes: changes(),
            live,
            on_frame: move |frame: channels::ClientFrame| {
                if let Some(room) = room_asked_for(&frame) {
                    expecting.set(Some(room));
                }
                outbound.call(frame);
            },
            on_select: move |id| selected.set(id),
            // One place answers what a drag meant, whichever zone it
            // landed on. A refusal takes the same route every other
            // refusal takes, so a gesture with no meaning reads like
            // everything else the city would not do.
            on_drop: move |(target, what): (crate::drop::Target, crate::drop::Dropped)| {
                match crate::drop::read(&target, &what) {
                    crate::drop::Meaning::Aim { addr, task } => {
                        selected.set(Some(addr.as_str().to_owned()));
                        dropped.set(Some(task));
                    }
                    // The bar already knows where the work goes, because
                    // somebody put it there. Only the task is written.
                    crate::drop::Meaning::Task { task } => {
                        dropped.set(Some(task));
                    }
                    // Into the session's own box, unsent. The button is
                    // still the person's to press.
                    crate::drop::Meaning::Say { said, .. } => {
                        steered.set(Some(said));
                    }
                    crate::drop::Meaning::Refused { because } => {
                        refused.set(Some(crate::alert::refused(
                            lang(),
                            &crate::drop::refusal(lang(), because),
                        )));
                    }
                }
            },
            on_view: move |next: View| {
                #[cfg(target_arch = "wasm32")]
                crate::route::go(&next);
                #[cfg(not(target_arch = "wasm32"))]
                view.set(next);
            },

            on_dismiss: move |()| refused.set(None),
        }
        if palette() {
            crate::palette::Palette {
                offers: reachable(&snapshot(), city().as_ref(), lang()),
                on_go: move |going: View| {
                    palette.set(false);
                    #[cfg(target_arch = "wasm32")]
                    crate::route::go(&going);
                    #[cfg(not(target_arch = "wasm32"))]
                    view.set(going);
                },
                on_close: move |()| palette.set(false),
            }
        }
        if keymap() {
            KeyMap { on_close: move |()| keymap.set(false) }
        }
    }
}

/// Everything the palette can reach, in the order a reader expects it.
///
/// Pages first because they are the answer most of the time, then the
/// buildings this city holds, then the sessions it knows of. Assembled
/// here because this is where the nav, the city answer and the run list
/// already meet; the palette holding its own list would be a second
/// answer to "where can a person go".
#[must_use]
fn reachable(
    snapshot: &Snapshot,
    city: Option<&channels::CityAnswer>,
    lang: crate::lang::Lang,
) -> Vec<crate::palette::Offer> {
    let mut offers: Vec<crate::palette::Offer> = destinations(snapshot)
        .into_iter()
        .map(|spot| crate::palette::Offer {
            label: crate::lang::say(lang, spot.label).to_owned(),
            kind: crate::palette::Kind::Page,
            going: spot.view,
        })
        .collect();
    if let Some(answer) = city {
        offers.extend(answer.buildings.iter().filter_map(|building| {
            let addr = Address::parse(building.addr.as_str()).ok()?;
            Some(crate::palette::Offer {
                label: building.addr.as_str().to_owned(),
                kind: crate::palette::Kind::Building,
                going: View::Building(addr),
            })
        }));
    }
    offers.extend(
        watchable(snapshot)
            .into_iter()
            .map(|(id, said)| crate::palette::Offer {
                label: said,
                kind: crate::palette::Kind::Session,
                going: View::Run(id),
            }),
    );
    offers
}

/// The key map, shown by the key that is hardest to guess.
///
/// A product whose shortcuts are undocumented has no shortcuts: nobody
/// tries a chord they have not been told about. This is the one page in
/// the client that exists to be read once.
#[component]
fn KeyMap(on_close: EventHandler<()>) -> Element {
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| crate::lang::say(lang(), msg);
    let rows = [
        ("Ctrl / \u{2318} + K", Msg::KeysPalette),
        ("Ctrl / \u{2318} + \u{21b5}", Msg::KeysCompose),
        ("Esc", Msg::KeysDismiss),
        ("g", Msg::KeysGo),
        ("?", Msg::KeysShow),
    ];
    rsx! {
        div { class: "palette-scrim", onclick: move |_| on_close.call(()),
            div {
                class: "keymap",
                onclick: move |event| event.stop_propagation(),
                h2 { "{word(Msg::KeysTitle)}" }
                p { class: "note", "{word(Msg::KeysScope)}" }
                dl {
                    for (chord, said) in rows {
                        div { key: "{chord}", class: "keymap-row",
                            dt { class: "chord", "{chord}" }
                            dd { "{word(said)}" }
                        }
                    }
                }
            }
        }
    }
}

/// Which buildings have a run in flight, folded from the snapshot rather
/// than asked of the server: the event stream already says it, and a
/// second question would be a second answer.
#[must_use]
pub(crate) fn busy_buildings(snapshot: &Snapshot) -> std::collections::BTreeSet<Address> {
    snapshot
        .runs()
        .filter(|(_, row)| matches!(row.phase, Phase::Running | Phase::Waiting))
        .filter_map(|(_, row)| row.addr.clone())
        .filter_map(|addr| building_of(&addr))
        .collect()
}

/// The building an address belongs to: its first segment. The city keeps
/// the authority on that (a building is a top-level address); this is the
/// same rule read on the page, so a run in `lab/room1` lights `lab`.
fn building_of(addr: &Address) -> Option<Address> {
    let head = addr.as_str().split('/').next()?;
    Address::parse(head).ok()
}
