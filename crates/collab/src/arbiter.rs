// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Two goals want the same resource. Who decides, and how far up.
//!
//! Detection is `kernel::goal`'s and stays there; this module holds the
//! three levels and the way between them. The split is the point:
//! deciding *that* two goals clash is mechanical, deciding *what to do*
//! usually is not.
//!
//! Level one is serialisation: when the clash is mechanically decidable
//! (the same paths, one goal standing and one not), the later goal
//! waits, and no judgment is involved. Level two is an arbitration
//! agent: reading two goal statements and telling whether they really
//! conflict is reading, which is a model's work. Level three is the
//! person, and two things always reach it: anything touching intent, and
//! anything a gate refused. A machine does not overrule a gate, and a
//! machine that guessed at intent would be guessing at the thing the
//! person is here for.

use kernel::{AxError, GoalEntry, GoalId, GoalResource, GoalVerdict, Payload};
use serde_json::{Map, Value};

/// Who settles this clash.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    /// One after the other; nobody has to judge anything.
    Serialize { after: GoalId },
    /// A resident reads both statements and decides.
    Arbitrate { with: GoalId },
    /// The person decides.
    Owner { with: GoalId, because: Escalation },
}

/// Why a clash went to the person rather than to an agent.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escalation {
    /// A gate refused one of the goals. A machine does not overrule a
    /// gate; that is what makes the gate a gate.
    GateRefused,
    /// The clash is about what the person wants, not about resources.
    Intent,
    /// An arbitration was already tried and did not settle it.
    ArbitrationExhausted,
}

impl Escalation {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Escalation::GateRefused => "gate_refused",
            Escalation::Intent => "intent",
            Escalation::ArbitrationExhausted => "arbitration_exhausted",
        }
    }
}

/// What the caller already knows about the pair, and cannot be worked
/// out from the goal entries alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Circumstance {
    /// A gate refused one of these goals.
    pub gate_refused: bool,
    /// One of the statements is about intent rather than about work.
    pub touches_intent: bool,
    /// An arbitration agent already looked and did not settle it.
    pub arbitration_tried: bool,
}

/// Decides who settles a clash between `candidate` and the goals already
/// registered.
///
/// Returns `None` when there is nothing to settle. The order of the
/// checks is fixed - the person's two reasons first, then exhaustion,
/// then serialisation, then arbitration - so the same pair always lands
/// at the same level, which is what makes a replay comparable.
#[must_use]
pub fn arbitrate(
    registered: &[GoalEntry],
    candidate: &GoalEntry,
    circumstance: Circumstance,
) -> Option<Level> {
    let GoalVerdict::Conflict { with } = kernel::detect_conflict(registered, candidate) else {
        return None;
    };
    if circumstance.gate_refused {
        return Some(Level::Owner {
            with,
            because: Escalation::GateRefused,
        });
    }
    if circumstance.touches_intent {
        return Some(Level::Owner {
            with,
            because: Escalation::Intent,
        });
    }
    if circumstance.arbitration_tried {
        return Some(Level::Owner {
            with,
            because: Escalation::ArbitrationExhausted,
        });
    }
    let held = registered.iter().find(|entry| entry.id == with);
    // Mechanically decidable: both sides claim paths, and exactly one of
    // them is a standing goal, so "the standing one first" needs no
    // judgment. Anything else is a reading.
    let mechanical = held.is_some_and(|entry| {
        entry.standing != candidate.standing
            && entry
                .resources
                .iter()
                .chain(candidate.resources.iter())
                .all(|resource| matches!(resource, GoalResource::Path(_)))
    });
    if mechanical {
        return Some(Level::Serialize { after: with });
    }
    Some(Level::Arbitrate { with })
}

/// The `goal_conflict` record: the pair, the level, and the reason when
/// there is one.
///
/// # Errors
/// Propagates the payload's refusal to hold what it was given.
pub fn conflict_payload(candidate: &GoalEntry, level: &Level) -> Result<Payload, AxError> {
    let mut map = Map::new();
    map.insert(
        "goal".to_owned(),
        Value::String(candidate.id.as_str().to_owned()),
    );
    let (with, name, because) = match level {
        Level::Serialize { after } => (after, "serialize", None),
        Level::Arbitrate { with } => (with, "arbitrate", None),
        Level::Owner { with, because } => (with, "owner", Some(*because)),
    };
    map.insert("with".to_owned(), Value::String(with.as_str().to_owned()));
    map.insert("level".to_owned(), Value::String(name.to_owned()));
    if let Some(because) = because {
        map.insert(
            "because".to_owned(),
            Value::String(because.as_str().to_owned()),
        );
    }
    Payload::new(map)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;
    use kernel::Address;

    fn goal(id: &str, path: &str, standing: bool) -> GoalEntry {
        GoalEntry {
            id: GoalId::new(id).unwrap(),
            owner: format!("lab/{id}"),
            resources: vec![GoalResource::Path(Address::parse(path).unwrap())],
            statement: format!("finish {id}"),
            standing,
        }
    }

    fn external(id: &str, name: &str) -> GoalEntry {
        GoalEntry {
            id: GoalId::new(id).unwrap(),
            owner: format!("lab/{id}"),
            resources: vec![GoalResource::External(name.to_owned())],
            statement: format!("use {name}"),
            standing: false,
        }
    }

    #[test]
    fn goals_that_do_not_touch_have_nothing_to_settle() {
        let registered = vec![goal("a", "lab/one", true)];
        assert_eq!(
            arbitrate(
                &registered,
                &goal("b", "lab/two", false),
                Circumstance::default()
            ),
            None
        );
    }

    #[test]
    fn a_clash_a_machine_can_settle_is_settled_by_waiting() {
        let registered = vec![goal("a", "lab/one", true)];
        assert_eq!(
            arbitrate(
                &registered,
                &goal("b", "lab/one/deeper", false),
                Circumstance::default()
            ),
            Some(Level::Serialize {
                after: GoalId::new("a").unwrap()
            })
        );
    }

    #[test]
    fn a_clash_that_needs_reading_goes_to_a_resident() {
        // Two standing goals on the same path: which comes first is not
        // decidable from the entries, so somebody has to read them.
        let registered = vec![goal("a", "lab/one", true)];
        assert_eq!(
            arbitrate(
                &registered,
                &goal("b", "lab/one", true),
                Circumstance::default()
            ),
            Some(Level::Arbitrate {
                with: GoalId::new("a").unwrap()
            })
        );

        // An external resource is a name, not a tree: overlap says
        // nothing about order.
        let registered = vec![external("a", "the printer")];
        assert!(matches!(
            arbitrate(
                &registered,
                &external("b", "the printer"),
                Circumstance::default()
            ),
            Some(Level::Arbitrate { .. })
        ));
    }

    #[test]
    fn a_gate_refusal_is_never_overruled_by_a_machine() {
        let registered = vec![goal("a", "lab/one", true)];
        assert_eq!(
            arbitrate(
                &registered,
                &goal("b", "lab/one/deeper", false),
                Circumstance {
                    gate_refused: true,
                    ..Circumstance::default()
                }
            ),
            Some(Level::Owner {
                with: GoalId::new("a").unwrap(),
                because: Escalation::GateRefused
            }),
            "the mechanical path would have serialised this; a refused gate outranks it"
        );
    }

    #[test]
    fn intent_and_an_exhausted_arbitration_both_reach_the_person() {
        let registered = vec![goal("a", "lab/one", true)];
        for (circumstance, expected) in [
            (
                Circumstance {
                    touches_intent: true,
                    ..Circumstance::default()
                },
                Escalation::Intent,
            ),
            (
                Circumstance {
                    arbitration_tried: true,
                    ..Circumstance::default()
                },
                Escalation::ArbitrationExhausted,
            ),
        ] {
            let level = arbitrate(&registered, &goal("b", "lab/one", true), circumstance);
            assert_eq!(
                level,
                Some(Level::Owner {
                    with: GoalId::new("a").unwrap(),
                    because: expected
                })
            );
        }
    }

    #[test]
    fn the_record_says_who_clashed_and_how_far_up_it_went() {
        let level = Level::Owner {
            with: GoalId::new("a").unwrap(),
            because: Escalation::Intent,
        };
        let payload = conflict_payload(&goal("b", "lab/one", true), &level).unwrap();
        let map = payload.as_map();
        assert_eq!(map.get("goal").and_then(Value::as_str), Some("b"));
        assert_eq!(map.get("with").and_then(Value::as_str), Some("a"));
        assert_eq!(map.get("level").and_then(Value::as_str), Some("owner"));
        assert_eq!(map.get("because").and_then(Value::as_str), Some("intent"));

        let serialized = conflict_payload(
            &goal("b", "lab/one", false),
            &Level::Serialize {
                after: GoalId::new("a").unwrap(),
            },
        )
        .unwrap();
        assert!(
            !serialized.as_map().contains_key("because"),
            "a level with no reason to give does not invent one"
        );
    }
}
