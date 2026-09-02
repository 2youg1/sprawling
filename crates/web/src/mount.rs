// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The four things only a browser has, and nothing that decides.
//!
//! The address bar, the keyboard, the socket and the animation frame.
//! Every one of them is `#[cfg(target_arch = "wasm32")]`, which is what
//! makes this the humble end of the Humble Object the client is built as
//! (ARCHITECTURE section 9): it moves values between the browser and the
//! signals `web::shell` renders from, and it applies no rule of its own.
//!
//! Each borrows its judgement from a module that can be tested on the
//! host: `web::keys` decides what a keystroke means, `web::route`
//! translates the address bar both ways, `web::pace` decides how often a
//! page may change, and `Snapshot::apply` decides what an event means.

use dioxus::prelude::*;

use channels::EventRecord;

use crate::app::Snapshot;
#[cfg(target_arch = "wasm32")]
use crate::asking::{hold, invalidated_by, started_here};
#[cfg(target_arch = "wasm32")]
use crate::lang::Msg;
use crate::route::View;
#[cfg(target_arch = "wasm32")]
use crate::route::place_view;
#[cfg(target_arch = "wasm32")]
use crate::shell::App;

#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(dead_code, reason = "the only caller is the browser's socket")
)]
pub(crate) struct Wiring {
    pub(crate) snapshot: Signal<Snapshot>,
    pub(crate) endpoints: Signal<Option<channels::EndpointsAnswer>>,
    pub(crate) city: Signal<Option<channels::CityAnswer>>,
    pub(crate) cost: Signal<Option<channels::CostAnswer>>,
    pub(crate) building: Signal<Option<channels::BuildingAnswer>>,
    pub(crate) discards: Signal<Option<channels::DiscardAnswer>>,
    pub(crate) inbox: Signal<Option<channels::InboxAnswer>>,
    pub(crate) hits: Signal<Option<channels::ArchiveAnswer>>,
    pub(crate) filed: Signal<Option<channels::RegistryAnswer>>,
    pub(crate) vitals: Signal<Option<channels::MetricsAnswer>>,
    pub(crate) changes: Signal<Option<channels::ChangesAnswer>>,
    pub(crate) records: Signal<Vec<EventRecord>>,
    pub(crate) live: Signal<bool>,
    /// Which page is showing, so the run a person just asked for can be
    /// opened when it starts.
    pub(crate) view: Signal<View>,
    /// The room this client last dispatched to, as `building/name`.
    /// Cleared by the run it was waiting for; see [`started_here`].
    pub(crate) expecting: Signal<Option<String>>,
    /// The last thing the city refused. Beside the snapshot rather than
    /// inside it: a refusal is not something that happened to the city,
    /// it is the answer to something one person asked, and the snapshot
    /// holds only what the ledger says.
    pub(crate) refused: Signal<Option<crate::alert::Refused>>,
    /// What language the words this wiring produces are said in. A
    /// signal rather than a value: these closures speak long after the
    /// page mounted.
    pub(crate) lang: Signal<crate::lang::Lang>,
}

/// Mounts the one reader of the address bar.
///
/// Registered once for the life of the page: `use_hook` runs on the
/// first render only, so the listener is not rebuilt on every state
/// change - a second listener would apply the same change twice.
#[cfg(target_arch = "wasm32")]
pub(crate) fn follow_the_address_bar(
    mut view: Signal<View>,
    mut refused: Signal<Option<crate::alert::Refused>>,
    lang: Signal<crate::lang::Lang>,
) {
    use dioxus::prelude::use_hook;
    use wasm_bindgen::JsCast as _;
    use_hook(move || {
        // What the person arrived at, before any event has happened.
        match crate::route::current() {
            Some(arrived) => view.set(arrived),
            // A link that does not land is a fact the person may want to
            // act on. Leaving them on the first page without a word is the
            // quiet substitution this design refuses: it teaches somebody
            // their own bookmarks are unreliable while never admitting it.
            None => {
                if let Some(named) = crate::route::unresolved() {
                    let said = lang();
                    refused.set(Some(crate::alert::Refused {
                        code: "E_NO_SUCH_PAGE".to_owned(),
                        what: crate::lang::fill(
                            crate::lang::say(said, Msg::RouteNoSuchPage),
                            &[("named", &named)],
                        ),
                        recovery: crate::lang::say(said, Msg::RouteNoSuchPageRecovery).to_owned(),
                    }));
                }
            }
        }
        let Some(window) = web_sys::window() else {
            return;
        };
        let moved = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            // A fragment that names nothing leaves the page where it is
            // rather than landing somewhere the person did not ask for.
            if let Some(next) = crate::route::current() {
                view.set(next);
            }
        });
        if window
            .add_event_listener_with_callback("hashchange", moved.as_ref().unchecked_ref())
            .is_ok()
        {
            // The listener outlives this scope, and the page outlives
            // the listener: dropping the closure here would unregister
            // the only thing that reads the address bar.
            moved.forget();
        }
    });
}

/// Off the browser there is no address bar, so the signal is the only
/// authority and nothing has to follow anything.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn follow_the_address_bar(
    _view: Signal<View>,
    _refused: Signal<Option<crate::alert::Refused>>,
    _lang: Signal<crate::lang::Lang>,
) {
}

/// What a keystroke may move. Bundled for the reason [`Wiring`] is: a
/// listener that took six handles would grow a seventh without anybody
/// noticing which of them it actually writes.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        dead_code,
        reason = "the only reader is the browser's keydown listener"
    )
)]
#[derive(Clone, Copy)]
pub(crate) struct Keyboard {
    pub(crate) chord: Signal<crate::keys::Chord>,
    pub(crate) palette: Signal<bool>,
    pub(crate) keymap: Signal<bool>,
    pub(crate) view: Signal<View>,
    pub(crate) refused: Signal<Option<crate::alert::Refused>>,
}

/// The one place a keystroke reaches this client.
///
/// On the window rather than on an element: a person who has clicked
/// nothing still has a keyboard, and a handler hung on the layout would
/// never see a key pressed while the body itself holds focus.
///
/// The browser contributes three facts and no judgement - which key,
/// whether the accelerator was down, and whether focus sits in something
/// the reader types into - and `web::keys` decides the rest, which is what
/// keeps the key map testable on the host.
#[cfg(target_arch = "wasm32")]
pub(crate) fn listen_for_keys(keyboard: Keyboard) {
    use dioxus::prelude::use_hook;
    use wasm_bindgen::JsCast as _;
    use_hook(move || {
        let Some(window) = web_sys::window() else {
            return;
        };
        let mut held = keyboard;
        let pressed = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                let key = event.key();
                let stroke = crate::keys::Stroke {
                    key: &key,
                    command: event.ctrl_key() || event.meta_key(),
                    in_text: typing_now(),
                };
                // `peek` rather than a read: this closure lives outside
                // the render that created it, and subscribing here would
                // tie a DOM listener to a reactive scope it outlives.
                let (next, act) = crate::keys::press(*held.chord.peek(), &stroke);
                held.chord.set(next);
                match act {
                    crate::keys::Act::Ignore => return,
                    crate::keys::Act::OpenPalette => {
                        held.keymap.set(false);
                        held.palette.set(true);
                    }
                    // One key closes whatever is open, outermost first, so
                    // a reader never has to know how deep they are.
                    crate::keys::Act::Dismiss => {
                        held.palette.set(false);
                        held.keymap.set(false);
                        held.refused.set(None);
                    }
                    crate::keys::Act::Compose => {
                        held.palette.set(false);
                        focus_where_work_starts();
                    }
                    crate::keys::Act::ShowKeys => {
                        held.keymap.set(true);
                    }
                    crate::keys::Act::Go(place) => {
                        held.palette.set(false);
                        held.keymap.set(false);
                        let going = place_view(place);
                        crate::route::go(&going);
                        held.view.set(going);
                    }
                }
                // Only what this client claimed: an ignored key belongs to
                // the browser, and taking it would break the reader's own
                // find-in-page and text entry.
                event.prevent_default();
            },
        );
        if window
            .add_event_listener_with_callback("keydown", pressed.as_ref().unchecked_ref())
            .is_ok()
        {
            // The page outlives the listener; dropping the closure here
            // would unregister the only thing that reads the keyboard.
            pressed.forget();
        }
    });
}

/// Whether focus sits in something the reader is writing into.
///
/// Without this, writing the word "goal" into the task box would navigate
/// away on its `g`.
#[cfg(target_arch = "wasm32")]
fn typing_now() -> bool {
    let Some(active) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
    else {
        return false;
    };
    matches!(
        active.tag_name().to_ascii_uppercase().as_str(),
        "INPUT" | "TEXTAREA" | "SELECT"
    ) || active.has_attribute("contenteditable")
}

/// Puts the cursor in the box work is described in.
///
/// The discarded result follows `route::go`: a focus call that the
/// document refuses has no second thing to try, and the page is already
/// showing the field it failed to reach.
#[cfg(target_arch = "wasm32")]
fn focus_where_work_starts() {
    use wasm_bindgen::JsCast as _;
    if let Some(field) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("dispatch-task"))
        .and_then(|found| found.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = field.focus();
    }
}

/// Off the browser there is no keyboard to listen to.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn listen_for_keys(_keyboard: Keyboard) {}

#[cfg(target_arch = "wasm32")]
pub(crate) fn connect(wiring: Wiring) -> Outbound {
    use dioxus::prelude::use_hook;

    // Only two of these move here now. The rest are copied into the
    // frame's own wiring and moved once per animation frame instead of
    // once per arriving message, which is the whole of what this loop
    // changed (`web::pace`).
    let Wiring {
        mut snapshot,
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
        mut live,
        view,
        expecting,
        refused,
        lang,
    } = wiring;
    use_hook(move || {
        let outbound = std::rc::Rc::new(std::cell::RefCell::new(None));
        let Some(url) = crate::socket::socket_url() else {
            return send_through(outbound);
        };
        // The code the host put on this URL. Hard-coded `None` here made
        // every exposed city unreachable by its own WebUI: the server
        // asked for a token and the page had no way to have one.
        let link = std::rc::Rc::new(std::cell::RefCell::new(crate::socket::Link::new(
            crate::socket::pairing_token(),
        )));
        // What has already claimed somebody's attention. Held beside the
        // link because a reconnect re-delivers events, and one fact must
        // not interrupt twice for having been sent twice.
        let alerts = std::rc::Rc::new(std::cell::RefCell::new(crate::alert::Alerts::new()));
        let socket = std::rc::Rc::new(std::cell::RefCell::new(None));
        // Where frames wait for the next animation frame. A run does not
        // deliver one event at a time - a tool wave writes five in a few
        // milliseconds - and applying each on arrival repainted the page
        // once per event at whatever rate the network chose. A display
        // cannot show more than one frame per refresh, so those paints
        // were work produced for nobody (`web::pace`).
        let buffer = crate::pace::browser::Buffer::default();
        {
            let buffer_for_loop = buffer.clone();
            let alerts = std::rc::Rc::clone(&alerts);
            let socket = std::rc::Rc::clone(&socket);
            crate::pace::browser::each_frame(buffer_for_loop, move |paint| {
                apply_frame(
                    paint,
                    &socket,
                    &alerts,
                    FrameWiring {
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
                        view,
                        expecting,
                        refused,
                        lang,
                    },
                );
            });
        }
        let opened = {
            let link = std::rc::Rc::clone(&link);
            let socket = std::rc::Rc::clone(&socket);
            let buffer = buffer.clone();
            crate::socket::open(&url, move |event| {
                let action = match link.try_borrow_mut() {
                    Ok(mut link) => link.advance(event),
                    Err(_) => return,
                };
                // The pages watch this to know when asking is worth
                // anything. Read from the link rather than inferred from
                // the action, because the link owns what "live" means.
                let flowing = link.try_borrow().is_ok_and(|link| link.is_live());
                let opened = flowing && !*live.peek();
                if *live.peek() != flowing {
                    live.set(flowing);
                }
                if let Ok(link) = link.try_borrow()
                    && let Some(city) = link.city()
                    && snapshot.peek().city() != Some(city)
                {
                    snapshot.write().adopt_city(city.clone());
                }
                let held = socket.borrow();
                let Some(socket) = held.as_ref() else {
                    return;
                };
                // The stream carries what happens next, so a tab opened
                // over a city that has been running for a month saw an
                // empty one. Asked once, the moment frames start
                // flowing, and before any live record can have been
                // folded - which is the condition `backfill` refuses to
                // work without.
                if opened {
                    let _ = crate::socket::send(
                        socket,
                        &channels::ClientFrame::Query(channels::Query::History {
                            before: None,
                            limit: channels::HISTORY_MAX,
                        }),
                    );
                }
                match action {
                    crate::socket::LinkAction::Send(hello) => {
                        let _ = crate::socket::send(socket, &channels::ClientFrame::Hello(*hello));
                    }
                    // The three actions that change the page do not change
                    // it here. They go into the buffer and the animation
                    // frame applies them together, because the rate a
                    // network delivers at is not a rate a display can show
                    // (`web::pace`).
                    crate::socket::LinkAction::Deliver(event) => {
                        buffer.push(crate::pace::Arrived::Event(event));
                    }
                    crate::socket::LinkAction::Answered(answer) => {
                        buffer.push(crate::pace::Arrived::Answer(answer));
                    }
                    crate::socket::LinkAction::Report(error) => {
                        buffer.push(crate::pace::Arrived::Refusal(error));
                    }
                    crate::socket::LinkAction::Saying(delta) => {
                        buffer.push(crate::pace::Arrived::Saying(delta));
                    }
                    // The retry ladder is not history either, and
                    // closing on the way out of view is the transport
                    // layer's to carry out; here they are the same as
                    // any other instruction that moves no snapshot.
                    crate::socket::LinkAction::WaitMs(_)
                    | crate::socket::LinkAction::OpenSocket
                    | crate::socket::LinkAction::CloseSocket
                    | crate::socket::LinkAction::Nothing => {}
                }
            })
        };
        if let Ok(handle) = opened {
            *socket.borrow_mut() = Some(handle);
            let _ = link.borrow_mut().connect();
        }
        *outbound.borrow_mut() = Some(std::rc::Rc::clone(&socket));
        send_through(outbound)
    })
}

/// The one way a component reaches the server. A frame sent before the
/// socket exists is dropped rather than queued: the page that sent it
/// asks again, and a queue would be a second place where "what did the
/// person ask for" lives.
#[cfg(target_arch = "wasm32")]
pub(crate) fn send_through(outbound: OutboundCell) -> Outbound {
    Outbound(outbound)
}

/// The socket handle as the component tree may hold it: cloneable,
/// because a hook's value is cloned on every render.
#[cfg(target_arch = "wasm32")]
type OutboundCell = std::rc::Rc<
    std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<Option<web_sys::WebSocket>>>>>,
>;

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub struct Outbound(OutboundCell);

#[cfg(target_arch = "wasm32")]
impl Outbound {
    pub(crate) fn call(&self, frame: channels::ClientFrame) {
        let held = self.0.borrow();
        let Some(socket) = held.as_ref() else {
            return;
        };
        let socket = socket.borrow();
        if let Some(socket) = socket.as_ref() {
            let _ = crate::socket::send(socket, &frame);
        }
    }
}

/// Off the browser there is no socket, so a frame goes nowhere. The
/// type exists on both targets because the component tree names it.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct Outbound;

#[cfg(not(target_arch = "wasm32"))]
impl Outbound {
    pub(crate) fn call(&self, _frame: channels::ClientFrame) {}
}

/// Hands the client to the browser. The only wasm-specific entry in this
/// crate, and it decides nothing.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    install_theme();
    crate::alert::ask_to_interrupt();
    launch(App);
}

/// Writes the token set into the document before the first paint.
///
/// The shipped page names no colour; it reads custom properties that arrive
/// here. That is what makes "one production point for colour" true of what
/// the browser renders and not only of the Rust source.
#[cfg(target_arch = "wasm32")]
fn install_theme() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(head) = document.head() else {
        return;
    };
    let Ok(style) = document.create_element("style") else {
        return;
    };
    style.set_text_content(Some(&crate::theme::custom_properties()));
    let _ = head.append_child(&style);
}

/// The signals one painted frame may move.
///
/// A struct rather than fifteen parameters, and by value because every
/// field is a `Copy` handle: this is the same reasoning that gave `Wiring`
/// its shape, and splitting it would produce two halves neither of which
/// can paint a frame.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
struct FrameWiring {
    snapshot: Signal<Snapshot>,
    endpoints: Signal<Option<channels::EndpointsAnswer>>,
    city: Signal<Option<channels::CityAnswer>>,
    cost: Signal<Option<channels::CostAnswer>>,
    building: Signal<Option<channels::BuildingAnswer>>,
    discards: Signal<Option<channels::DiscardAnswer>>,
    inbox: Signal<Option<channels::InboxAnswer>>,
    hits: Signal<Option<channels::ArchiveAnswer>>,
    filed: Signal<Option<channels::RegistryAnswer>>,
    vitals: Signal<Option<channels::MetricsAnswer>>,
    changes: Signal<Option<channels::ChangesAnswer>>,
    records: Signal<Vec<channels::EventRecord>>,
    view: Signal<View>,
    expecting: Signal<Option<String>>,
    refused: Signal<Option<crate::alert::Refused>>,
    lang: Signal<crate::lang::Lang>,
}

/// Applies one animation frame's worth of arrivals.
///
/// The order is the one `Paint::into_parts` hands them out in and the
/// reason is recorded there: an answer describes the city as of some
/// moment, so folding the frame's events first is what stops a page
/// rendering a view its own snapshot has not caught up with.
///
/// Every event is still read one at a time - `alert::absorb` deduplicates
/// an interruption, `invalidated_by` asks again, `started_here` recognises
/// the room this client asked for. What the frame changed is *when the
/// signals move*, and they now move once for the whole burst.
#[cfg(target_arch = "wasm32")]
fn apply_frame(
    paint: crate::pace::Paint,
    socket: &std::rc::Rc<std::cell::RefCell<Option<web_sys::WebSocket>>>,
    alerts: &std::rc::Rc<std::cell::RefCell<crate::alert::Alerts>>,
    wiring: FrameWiring,
) {
    let FrameWiring {
        mut snapshot,
        mut endpoints,
        mut city,
        mut cost,
        mut building,
        mut discards,
        mut inbox,
        mut hits,
        mut filed,
        mut vitals,
        mut changes,
        mut records,
        mut view,
        mut expecting,
        mut refused,
        lang,
    } = wiring;
    let (events, answers, refusal, saying) = paint.into_parts();
    let said = lang();
    // Increments first, and into the snapshot's own discardable buffer.
    // Before the events on purpose: `model_returned` in this same burst
    // throws the buffer away, so a call that both streamed and settled
    // inside one frame ends with the record showing rather than the
    // increments that preceded it.
    if !saying.is_empty() {
        snapshot.with_mut(|held| {
            for delta in &saying {
                held.is_saying(delta);
            }
        });
    }
    // Everything the burst adds to history, folded and kept in one write
    // each rather than in one write each per event.
    let mut keep: Vec<channels::EventRecord> = Vec::new();
    for event in events {
        // Decided in the same pass as the snapshot: what happened and
        // whether it needs a person are two readings of one event, not two
        // readers of the stream.
        if let Ok(mut alerts) = alerts.try_borrow_mut()
            && crate::alert::absorb(said, &mut alerts, &event) == crate::alert::Raise::Interrupt
            && let Some(alert) = crate::alert::alert_for(said, &event)
        {
            crate::alert::interrupt(said, &alert);
        }
        if let Some(query) = invalidated_by(event.kind()) {
            let held = socket.borrow();
            if let Some(socket) = held.as_ref() {
                let _ = crate::socket::send(socket, &channels::ClientFrame::Query(query));
            }
        }
        // The session this person asked for, opening. Knowledge rather
        // than a guess: this client sent that dispatch and knows the room
        // it named. Read and released before the write below: a signal
        // held open across its own set is a panic in a browser and nothing
        // at all in a host test.
        let waiting = expecting.read().clone();
        if let Some(waiting) = waiting
            && started_here(&event, &waiting).is_some()
            && let Some(addr) = event.addr().cloned()
        {
            // The room, not the run: a session opened by this person is
            // named by the name they gave it, and that is the address
            // this build puts in the bar.
            expecting.set(None);
            view.set(View::Session(addr.clone()));
            crate::route::go(&View::Session(addr));
        }
        if snapshot.write().apply(&event) {
            keep.push(event);
        }
    }
    // Which session the person has open, so a store at its bound gives
    // way in what they are not reading rather than in what they are.
    let reading = match &*view.read() {
        View::Session(addr) => snapshot.read().session_at(addr).map(|(run, _)| run),
        View::Run(run) => Some(*run),
        _ => None,
    };
    if !keep.is_empty() {
        // Kept once, read by every page that reads history, and bounded by
        // the one function that answers "how much does a tab hold".
        hold(&mut records.write(), keep, reading);
    }
    // An answer reaches the view that asked for it. It is not history: it
    // moves no snapshot, and a reload asks again rather than trusting what
    // is held.
    for answer in answers {
        match answer {
            channels::Answer::Endpoints(held) => endpoints.set(Some(held)),
            channels::Answer::City(held) => city.set(Some(held)),
            channels::Answer::Cost(held) => cost.set(Some(*held)),
            channels::Answer::Building(held) => building.set(Some(*held)),
            channels::Answer::Discards(held) => discards.set(Some(held)),
            channels::Answer::Inbox(held) => inbox.set(Some(held)),
            channels::Answer::Archive(held) => hits.set(Some(held)),
            channels::Answer::Registry(held) => filed.set(Some(held)),
            channels::Answer::Metrics(held) => vitals.set(Some(*held)),
            channels::Answer::Changes(held) => changes.set(Some(held)),
            // What happened before this tab opened. Folded into the
            // snapshot and kept for the pages that read history, in the
            // same bounded store the live stream fills - one answer to
            // "how much does a tab hold".
            // What happened before this tab opened, from either of the
            // two questions that ask it: the city's own slice at connect,
            // and one session's when its page opens. The backfill is
            // forward-only and refuses the second, which is correct - a
            // snapshot already folded past these must not be walked
            // back - but the records themselves are still what the
            // session page reads, so they are kept either way.
            channels::Answer::History(held) => {
                snapshot.write().backfill(&held.records);
                hold(&mut records.write(), held.records, reading);
            }
            // What was already waiting when this page connected. The
            // stream carries what happens next; without this the inbox
            // would show only the items raised since the tab opened.
            channels::Answer::Approvals(held) => {
                snapshot.write().adopt_approvals(held.items);
            }
            // Named one by one rather than caught by a wildcard: each of
            // these has an answer the server can give and no page that
            // asks for it yet, and a wildcard here would hide the next one
            // that arrives as well.
            channels::Answer::Run(_) | channels::Answer::Unavailable { .. } => {}
        }
    }
    // A refusal is not history and must not move the snapshot - but it is
    // the answer to something a person just did, so it goes where they can
    // read it.
    if let Some(error) = refusal {
        refused.set(Some(crate::alert::refused(said, &error)));
    }
}
