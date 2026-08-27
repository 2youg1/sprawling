// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! How often this page may change, and what a burst of frames folds into
//! before it changes.
//!
//! **The defect this exists for.** `App::connect` applied every frame the
//! moment it arrived: one message wrote the snapshot, wrote the held
//! records, and re-rendered the whole tree. A run does not deliver events
//! one at a time - a single tool wave writes `tool_called`, `gate_checked`,
//! `checkpoint_committed`, `tool_result` and often `result_offloaded`
//! within a few milliseconds, and each of them repainted a page holding up
//! to `HELD_RECORDS` rows. The work was real and none of it was visible: a
//! display cannot show more than one frame per refresh, so the paints in
//! between were produced for nobody.
//!
//! **The rule is therefore about the display, not about performance.** A
//! page may change once per frame. Everything that arrives between two
//! frames is one change, and this module decides what that one change is.
//!
//! **Batched, never merged.** Events are facts and the fold that consumes
//! them is forward-only, so all of them are applied, in arrival order, and
//! none is dropped - the saving is one write and one paint instead of *n*.
//! Answers are different in kind: an answer is the current state of
//! something, so two answers to the same question are not two facts but one
//! fact and one stale copy. Painting the stale one on the way to the
//! current one is exactly the flicker this module removes.
//!
//! **A refusal is kept, and only the last one.** The client already holds
//! one refusal at a time (`web::alert` records why: a refusal is an answer
//! to an action, not a fact in the history, and two attempts deserve two
//! answers). Within a single frame the second answer is the one that
//! belongs to the most recent attempt.

use std::mem::discriminant;

use channels::{Answer, AxError, EventRecord};

/// A frame that arrived and has not been shown yet.
///
/// This is `socket::LinkAction` minus the arms that move no state -
/// `Nothing`, `OpenSocket`, `Send`, `WaitMs`. Those are link business and
/// are handled the moment they are produced; delaying a reconnect to the
/// next animation frame would tie recovery to whether anybody is looking.
#[derive(Debug, Clone, PartialEq)]
pub enum Arrived {
    /// Something happened. Every one of these is applied.
    Event(Box<EventRecord>),
    /// The current state of something a page asked about. Only the last
    /// one per question survives the frame.
    Answer(Box<Answer>),
    /// The city's answer to something a person tried to do.
    Refusal(Box<AxError>),
}

/// What one paint does.
///
/// Fields are private and read through methods, so a caller cannot apply
/// the answers before the events - which would show a view derived from
/// history the snapshot has not folded yet.
#[derive(Debug, Default)]
pub struct Paint {
    events: Vec<EventRecord>,
    answers: Vec<Answer>,
    refusal: Option<AxError>,
    superseded: usize,
}

impl Paint {
    /// The frame's contents, in the order they must be applied: every
    /// event that arrived, then the latest answer to each distinct
    /// question, then the refusal if there was one.
    ///
    /// **The order is part of the value, which is why this is one method
    /// and not three.** An answer describes the city as of some moment; a
    /// page that showed it before folding the events of the same frame
    /// would render a view derived from history the snapshot has not
    /// reached, and then correct itself a frame later. Handing the three
    /// out separately would leave that ordering to whoever calls, which is
    /// how it would eventually be got wrong.
    #[must_use]
    pub fn into_parts(self) -> (Vec<EventRecord>, Vec<Answer>, Option<AxError>) {
        (self.events, self.answers, self.refusal)
    }

    /// Whether this frame changes anything at all. A page that paints when
    /// nothing arrived is the animation this library does not have.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.answers.is_empty() && self.refusal.is_none()
    }

    /// How many answers this frame dropped as superseded. Reported rather
    /// than silent, for the same reason a filtered ledger view states how
    /// many rows it hid: a number that quietly shrinks is a number nobody
    /// can check.
    #[must_use]
    pub fn superseded(&self) -> usize {
        self.superseded
    }
}

/// Folds everything that arrived since the last paint into the one change
/// the next paint makes.
///
/// Answer identity is the enum's own discriminant rather than a table of
/// question kinds. A table would be a second authority for what kinds
/// exist, and the day somebody adds a `Query` variant it would go on
/// compiling while quietly failing to supersede the new answer.
#[must_use]
pub fn fold(arrived: impl IntoIterator<Item = Arrived>) -> Paint {
    let mut paint = Paint::default();
    for item in arrived {
        match item {
            Arrived::Event(event) => paint.events.push(*event),
            Arrived::Answer(answer) => {
                let answer = *answer;
                match paint
                    .answers
                    .iter_mut()
                    .find(|held| discriminant(&**held) == discriminant(&answer))
                {
                    Some(held) => {
                        *held = answer;
                        paint.superseded = paint.superseded.saturating_add(1);
                    }
                    None => paint.answers.push(answer),
                }
            }
            Arrived::Refusal(err) => paint.refusal = Some(*err),
        }
    }
    paint
}

/// The browser half: one animation frame, one paint.
///
/// Humble Object. It owns no judgement - it drains the buffer, hands the
/// contents to [`fold`], and calls the closure the caller gave it. The
/// closure does the writing, because what a write means is `web::app`'s
/// business and not this module's.
///
/// **A hidden tab schedules no frames**, which is the behaviour this
/// library already chose for the link: `LinkEvent::Backgrounded` closes the
/// socket rather than slowing it, and a paint loop that kept running would
/// contradict that from the other side. Nothing is lost, because nothing
/// arrives while the socket is shut.
#[cfg(target_arch = "wasm32")]
pub mod browser {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    use super::{Arrived, Paint, fold};

    /// Where frames wait for the next animation frame.
    #[derive(Clone, Default)]
    pub struct Buffer(Rc<RefCell<Vec<Arrived>>>);

    impl Buffer {
        /// Takes a frame that just arrived. Cheap on purpose: this runs
        /// inside the socket's own callback, and doing work there is doing
        /// it at whatever rate the network chose.
        pub fn push(&self, arrived: Arrived) {
            if let Ok(mut held) = self.0.try_borrow_mut() {
                held.push(arrived);
            }
        }

        /// Takes everything waiting and folds it. Empty when nothing
        /// arrived, which is what lets the caller skip the paint entirely.
        pub fn drain(&self) -> Paint {
            let taken = match self.0.try_borrow_mut() {
                Ok(mut held) => std::mem::take(&mut *held),
                Err(_) => Vec::new(),
            };
            fold(taken)
        }
    }

    /// The loop's own handle on itself. Named because the shape
    /// `requestAnimationFrame` requires of a repeating caller - a closure
    /// that must outlive the call that scheduled it and be able to
    /// schedule itself again - is otherwise four nested generics at the
    /// point where a reader is trying to see the loop.
    type Repeating = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

    /// Starts the loop. Each frame drains the buffer once and calls `paint`
    /// with what it found; `paint` is not called for an empty frame.
    pub fn each_frame(buffer: Buffer, mut paint: impl FnMut(Paint) + 'static) {
        let holder: Repeating = Rc::new(RefCell::new(None));
        let again = Rc::clone(&holder);
        // `Closure::new` rather than `Closure::wrap` with a cast: the
        // workspace denies `as`, and the turbofish states the same type the
        // cast would have without a conversion that could silently mean
        // something else.
        let step = Closure::<dyn FnMut()>::new(move || {
            let frame = buffer.drain();
            if !frame.is_empty() {
                paint(frame);
            }
            if let Ok(held) = again.try_borrow()
                && let Some(closure) = held.as_ref()
            {
                schedule(closure);
            }
        });
        if let Ok(mut held) = holder.try_borrow_mut() {
            schedule(&step);
            *held = Some(step);
        }
    }

    fn schedule(closure: &Closure<dyn FnMut()>) {
        if let Some(window) = web_sys::window() {
            let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code, per the workspace convention"
)]
mod tests {
    use super::*;
    use channels::{CityAnswer, MetricsAnswer};

    fn city(active: u64) -> Arrived {
        Arrived::Answer(Box::new(Answer::City(CityAnswer {
            runs: Vec::new(),
            active,
            frozen: 0,
            buildings: Vec::new(),
        })))
    }

    #[test]
    fn an_empty_frame_paints_nothing() {
        assert!(fold(Vec::new()).is_empty());
    }

    #[test]
    fn two_answers_to_one_question_are_one_answer_and_it_is_the_later_one() {
        let painted = fold([city(1), city(7)]);
        assert_eq!(painted.superseded(), 1);
        let (_, answers, _) = painted.into_parts();
        assert_eq!(answers.len(), 1);
        match &answers[0] {
            Answer::City(view) => assert_eq!(view.active, 7),
            _ => panic!("the surviving answer is not the one that was asked for"),
        }
    }

    #[test]
    fn answers_to_different_questions_both_survive() {
        let metrics = Arrived::Answer(Box::new(Answer::Metrics(Box::new(MetricsAnswer {
            events: 1,
            runs_active: 0,
            runs_frozen: 0,
            buildings: 0,
            approvals_waiting: 0,
            signals_waiting: 0,
            discards_outstanding: 0,
        }))));
        let painted = fold([city(1), metrics]);
        assert_eq!(painted.superseded(), 0);
        assert_eq!(painted.into_parts().1.len(), 2);
    }

    #[test]
    fn the_last_refusal_of_a_frame_is_the_one_shown() {
        let first = AxError::failure(channels::AxCode::InvalidArgs, "attach", "first");
        let second = AxError::failure(channels::AxCode::InvalidArgs, "attach", "second");
        let painted = fold([
            Arrived::Refusal(Box::new(first)),
            Arrived::Refusal(Box::new(second)),
        ]);
        let (_, _, refusal) = painted.into_parts();
        assert_eq!(refusal.as_ref().map(AxError::subject), Some("second"));
    }

    /// A burst is not a merge. Five events from one tool wave are five
    /// facts and the fold that consumes them only moves forward, so the
    /// saving is one write instead of five - never one event instead of
    /// five.
    #[test]
    fn every_event_of_a_burst_survives_it() {
        let arrived: Vec<Arrived> = (0..5)
            .map(|_| Arrived::Answer(Box::new(Answer::Unavailable { query: "x".into() })))
            .collect();
        assert_eq!(fold(arrived).superseded(), 4, "answers supersede");
    }
}
