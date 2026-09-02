// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The isometric city, drawn as shapes in the document.
//!
//! **It was a canvas until F2.02, and the reason it was one did not reach
//! this picture.** The recorded argument was that a thousand Residents must
//! not become a thousand elements. This view has never drawn a Resident: it
//! draws Buildings, of which a city holds tens, and the canvas was charging
//! four certain costs for that hypothetical saving - a fixed bitmap resampled
//! by CSS on every display that is not exactly its size, no way to read a
//! custom property (which is why the selection outline settled for a grey
//! where the code plainly wanted `--ACCENT`), no hover, focus, or keyboard
//! reach without reimplementing all three, and a drawing path that existed
//! only on wasm and so was reachable by no host test or gate.
//!
//! **Hit testing is no longer a second derivation.** The browser tests hits
//! against the very polygons it painted, so "what is drawn is what can be
//! picked" stopped being an assertion and became the construction. The
//! inverse projection, the point-in-quadrilateral test and the pointer
//! coordinate clamp went with it.
//!
//! **The picture fills what it is given.** The viewBox is the bounding box
//! of what was drawn, so a city of three buildings is a picture of three
//! buildings rather than three specks in a fixed 1000x560 field. The old
//! fit reserved `2n+1` tile widths for a diamond `n` tiles wide, which is
//! where most of that empty field came from.
//!
//! **The silhouette is the data.** A tower's height is the work its plan
//! took on and the lit band up its walls is the part that is done, so
//! progress is read off the skyline rather than from a number beside it.

use std::collections::BTreeSet;

use crate::command::{create_command, dispatch_to_building};
use crate::isometry::Camera;
use crate::isometry::{PAN_STEP, ZOOM_STOPS, points_attr, view_box};
use crate::lang::{Msg, fill, say};
use crate::skyline::{done_band_of, draw, faces_of, painter_order};
use crate::skyline::{prisms_of, unreadable_rows, windows_of};
use channels::{Address, CityAnswer, ClientFrame, Query};
use dioxus::prelude::*;

/// The city page.
///
/// It asks for the city once when it mounts, because buildings appear
/// when someone creates one and not on every event; what moves with the
/// event stream is which of them are lit, and that arrives through
/// `busy` without another question.
/// The link into one building's own pages, said.
fn read_what(lang: crate::lang::Lang, id: &str) -> String {
    fill(say(lang, Msg::CityReadWhat), &[("id", id)])
}

#[component]
pub fn CityView(
    city: Option<CityAnswer>,
    busy: BTreeSet<Address>,
    selected: Option<String>,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<ClientFrame>,
    on_select: EventHandler<Option<String>>,
    /// The way into a building's own pages. The nav cannot carry them -
    /// a city may hold fifty buildings - so the city is the way in.
    on_open: EventHandler<String>,
) -> Element {
    let asked = use_signal(|| false);
    let mut raising = use_signal(String::new);
    let mut template = use_signal(|| "minimal".to_owned());
    let mut stop = use_signal(|| 0usize);
    let mut pan = use_signal(|| (0i32, 0i32));
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let mut task = use_signal(String::new);
    let mut goal = use_signal(String::new);
    use_effect(move || {
        let mut asked = asked;
        if live() && !asked() {
            asked.set(true);
            on_frame.call(ClientFrame::Query(Query::CityView));
        }
    });
    let Some(city) = city else {
        return rsx! {
            section { class: "city-view",
                crate::panel::Empty {
                    status: word(Msg::AskingWhatItHolds).to_owned(),
                    what: word(Msg::CityScope).to_owned(),
                }
            }
        };
    };
    let prisms = prisms_of(&city.buildings, &busy, lang());
    let listing: Vec<(String, String)> = prisms
        .iter()
        .map(|prism| (prism.id.clone(), prism.note.clone()))
        .collect();
    let (dx, dy) = *pan.read();
    let camera = Camera::tiles();
    let list = draw(&camera, prisms.clone(), selected.as_deref());
    let frame = view_box(&list, *stop.read(), (dx, dy));
    let problems = unreadable_rows(&city.buildings);
    // The selected building's name, held twice outside the markup: the
    // submit closure keeps one for the length of the page, and the
    // disabled check reads another on every render. Empty when nothing
    // is selected, which is the case where the panel is not drawn.
    let submitting = selected.clone().unwrap_or_default();
    let checking = submitting.clone();
    let raised = city.buildings.len();
    let busy_now = city.active;
    rsx! {
        section { class: "city-view",
            crate::panel::Panel {
                title: if raised == 0 { word(Msg::CityNoBuildings).to_owned() }
                    else {
                        crate::lang::fill(
                            word(Msg::CityStanding),
                            &[("raised", &raised.to_string()), ("busy", &busy_now.to_string())],
                        )
                    },
                scope: word(Msg::CityTowerNote).to_owned(),
                source: word(Msg::CitySource).to_owned(),
            // An empty city still draws its ground, because a reader who
            // sees where buildings will stand knows what the page is for.
            // It draws less of it: at the full height the picture is a
            // 520px void above the one form that can end it.
            svg {
                class: if raised == 0 { "stage bare" } else { "stage" },
                view_box: "{frame.attr()}",
                preserve_aspect_ratio: "xMidYMid meet",
                role: "group",
                "aria-label": "{word(Msg::CityStageLabel)}",
                // A click that lands on no building clears the selection.
                // The groups below stop their own clicks here, so this is
                // the ground and only the ground.
                onclick: move |_| on_select.call(None),
                for face in list.ground.clone() {
                    polygon {
                        key: "g{face.points[0].0}-{face.points[0].1}",
                        points: "{points_attr(&face.points)}",
                        style: "fill:var(--{face.token})",
                    }
                }
                for prism in painter_order(prisms.clone()) {
                    g {
                        key: "{prism.id}",
                        class: if selected.as_deref() == Some(prism.id.as_str()) { "prism here" } else { "prism" },
                        tabindex: "0",
                        role: "button",
                        "aria-pressed": if selected.as_deref() == Some(prism.id.as_str()) { "true" } else { "false" },
                        "aria-label": "{prism.id}, {prism.note}",
                        onclick: {
                            let name = prism.id.clone();
                            move |event: MouseEvent| {
                                event.stop_propagation();
                                on_select.call(Some(name.clone()));
                            }
                        },
                        onkeydown: {
                            let name = prism.id.clone();
                            move |event: KeyboardEvent| {
                                // Enter and Space, the two keys a role of
                                // button owes a keyboard.
                                let pressed = match event.key() {
                                    Key::Enter => true,
                                    Key::Character(ref typed) => typed == " ",
                                    _ => false,
                                };
                                if pressed {
                                    event.prevent_default();
                                    on_select.call(Some(name.clone()));
                                }
                            }
                        },
                        title { "{prism.id} - {prism.note}" }
                        for (index , face) in faces_of(&camera, &prism, selected.as_deref() == Some(prism.id.as_str())).into_iter().enumerate() {
                            polygon {
                                key: "f{index}",
                                class: "body",
                                points: "{points_attr(&face.points)}",
                                style: "fill:var(--{face.token})",
                            }
                        }
                        for (index , face) in done_band_of(&camera, &prism).into_iter().enumerate() {
                            polygon {
                                key: "d{index}",
                                class: "done",
                                points: "{points_attr(&face.points)}",
                                style: "fill:var(--{face.token})",
                            }
                        }
                        for (index , face) in windows_of(&camera, &prism).into_iter().enumerate() {
                            polygon {
                                key: "w{index}",
                                points: "{points_attr(&face.points)}",
                                style: "fill:var(--{face.token})",
                            }
                        }
                    }
                }
                // Every label after every tower, because a label belongs to
                // the picture rather than to the building it names: drawn
                // inside its own group, a far building's name was painted
                // over by the near building in front of it. The group keeps
                // the name in its aria-label, so nothing is lost to a
                // reader who is not looking at pixels.
                for (index , label) in list.labels.clone().into_iter().enumerate() {
                    text {
                        key: "t{index}-{label.id}",
                        x: "{label.at.0}",
                        y: "{label.at.1}",
                        class: if label.leading { "name" } else { "note" },
                        style: "fill:var(--{label.token})",
                        "{label.text}"
                    }
                }
                if let Some(points) = list.outline {
                    // The one stroke in the picture, and the one place the
                    // accent appears here. On a canvas this had to settle
                    // for a grey, because `fillStyle` takes a value and a
                    // custom property is not one.
                    polygon {
                        class: "chosen",
                        points: "{points_attr(&points)}",
                    }
                }
            }
            form {
                class: "new-building",
                onsubmit: move |event| {
                    event.prevent_default();
                    let (named, kind) = (raising.read().clone(), template.read().clone());
                    if let Some(frame) = create_command(&named, &kind) {
                        on_frame.call(frame);
                        raising.set(String::new());
                        // The city does not announce a new building on
                        // the event stream this page folds, so it is
                        // asked again rather than assumed.
                        on_frame.call(ClientFrame::Query(Query::CityView));
                    }
                },
                input {
                    name: "addr",
                    placeholder: "{word(Msg::CityBuildingNamePlaceholder)}",
                    value: "{raising}",
                    oninput: move |event| raising.set(event.value()),
                }
                select {
                    name: "template",
                    onchange: move |event| template.set(event.value()),
                    option { value: "minimal", "minimal" }
                    option { value: "confidential", "confidential" }
                }
                button {
                    r#type: "submit",
                    disabled: create_command(&raising.read(), &template.read()).is_none(),
                    "{word(Msg::CityRaiseBuilding)}"
                }
            }
            if city.buildings.is_empty() {
                crate::panel::Empty {
                    status: word(Msg::CityNoBuildings).to_owned(),
                    what: word(Msg::CityNoBuildingsWhat).to_owned(),
                }
            }
            // The index beside the picture. The canvas answers "where",
            // and a pixel hunt is no way to answer "which": this list is
            // how a building is selected without a mouse, and the only
            // route to its own pages that a keyboard can take.
            div { class: "index",
                for row in listing.clone() {
                    div { key: "{row.0}", class: "index-row",
                        button {
                            class: "pick",
                            "aria-current": if selected.as_deref() == Some(row.0.as_str()) { "true" } else { "false" },
                            onclick: {
                                let name = row.0.clone();
                                move |_| on_select.call(Some(name.clone()))
                            },
                            "{row.0}"
                        }
                        span { class: "note", "{row.1}" }
                        button {
                            class: "read",
                            onclick: {
                                let name = row.0.clone();
                                move |_| on_open.call(name.clone())
                            },
                            "{word(Msg::ReadIt)}"
                        }
                    }
                }
            }
            div { class: "camera",
                for (index , factor) in ZOOM_STOPS.iter().enumerate() {
                    button {
                        key: "{factor}",
                        r#type: "button",
                        // The current stop is said, not only shown: a
                        // control whose state is a shade of grey is a
                        // control a screen reader cannot report.
                        "aria-pressed": if *stop.read() == index { "true" } else { "false" },
                        onclick: move |_| stop.set(index),
                        "{factor}x"
                    }
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx.saturating_add(PAN_STEP), dy)),
                    "{word(Msg::CityMoveLeft)}"
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx.saturating_sub(PAN_STEP), dy)),
                    "{word(Msg::CityMoveRight)}"
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx, dy.saturating_add(PAN_STEP))),
                    "{word(Msg::CityMoveUp)}"
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx, dy.saturating_sub(PAN_STEP))),
                    "{word(Msg::CityMoveDown)}"
                }
                button {
                    r#type: "button",
                    disabled: *stop.read() == 0 && (dx, dy) == (0, 0),
                    onclick: move |_| {
                        stop.set(0);
                        pan.set((0, 0));
                    },
                    "{word(Msg::CityFit)}"
                }
            }
            if let Some(id) = selected.clone() {
                div { class: "selected",
                    p { "{id}" }
                    button {
                        class: "open-building",
                        onclick: {
                            let name = id.clone();
                            move |_| on_open.call(name.clone())
                        },
                        "{read_what(lang(), &id)}"
                    }
                    form {
                        class: "send-work",
                        onsubmit: move |event| {
                            event.prevent_default();
                            let frame = dispatch_to_building(&submitting, &task.read(), &goal.read());
                            if let Some(frame) = frame {
                                on_frame.call(frame);
                                task.set(String::new());
                                goal.set(String::new());
                            }
                        },
                        input {
                            name: "task",
                            placeholder: "{word(Msg::CityWhatShouldHappen)}",
                            value: "{task}",
                            oninput: move |event| task.set(event.value()),
                        }
                        input {
                            name: "goal",
                            placeholder: "{word(Msg::CityWhatCountsAsDone)}",
                            value: "{goal}",
                            oninput: move |event| goal.set(event.value()),
                        }
                        button {
                            r#type: "submit",
                            disabled: dispatch_to_building(&checking, &task.read(), &goal.read()).is_none(),
                            "{word(Msg::CitySendWorkHere)}"
                        }
                    }
                    button {
                        r#type: "button",
                        onclick: move |_| on_select.call(None),
                        "{word(Msg::CityClearSelection)}"
                    }
                }
            }
            if !problems.is_empty() {
                ul { class: "problems",
                    for row in problems {
                        li { key: "{row}", "{row}" }
                    }
                }
            }
            }
        }
    }
}
