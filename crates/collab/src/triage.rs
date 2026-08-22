// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Where something from outside lands.
//!
//! Everything that arrives — a person's sentence, a webhook, a signal
//! from another city — goes through here, and what comes out is always
//! an Address. That is the whole point: the dispatch surface has one
//! shape, and the only thing triage varies is how much machinery meets
//! the arrival.
//!
//! Rules are read before the reflex is chosen, because a rule is
//! somebody's decision and a reflex is a default. A rule that matches
//! settles both the address and the reflex; nothing matching is not an
//! error, it is the case where a person reads it.

use kernel::{Address, AxCode, AxError};

/// How much machinery meets an arrival. Four, and the fourth is a whole
/// Resident: the ladder exists so that most arrivals do not get one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reflex {
    /// Recognised and not worth a line. Recorded, never acted on.
    Discard,
    /// Worth knowing, not worth doing: it reaches the person's view and
    /// starts nothing.
    Notify,
    /// A small, bounded reaction — one tool, one answer, no session.
    Light,
    /// A full Resident with a Run.
    Full,
}

impl Reflex {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Reflex::Discard => "discard",
            Reflex::Notify => "notify",
            Reflex::Light => "light",
            Reflex::Full => "full",
        }
    }
}

/// One thing that arrived, in the only shape triage reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    pub source: String,
    pub subject: String,
    /// Whether it came from outside the city. A tainted arrival never
    /// gets a reflex above `Notify` from a rule alone: acting on
    /// somebody else's text without a person in the loop is the thing
    /// the taint ring exists to prevent.
    pub tainted: bool,
}

/// A decision somebody already made about a kind of arrival.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Matched against source and subject, case-insensitively, as a
    /// substring. Not a pattern language: a rule table people write by
    /// hand is read far more often than it is written, and a regular
    /// expression in it would be a second thing to debug at the moment
    /// something is already going wrong.
    pub matches: String,
    pub landing: Address,
    pub reflex: Reflex,
}

/// Where an arrival goes and how much meets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landing {
    pub addr: Address,
    pub reflex: Reflex,
    /// Which rule decided this, or why none did. Carried because a
    /// routing decision nobody can explain is one nobody can fix.
    pub because: String,
}

/// The table, plus the address that catches everything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Triage {
    rules: Vec<Rule>,
    fallback: Address,
}

impl Triage {
    /// # Errors
    /// Refuses a rule with an empty match string: it would match
    /// everything, which is what the fallback is for, and it would
    /// shadow every rule under it.
    pub fn new(rules: Vec<Rule>, fallback: Address) -> Result<Triage, AxError> {
        for rule in &rules {
            if rule.matches.trim().is_empty() {
                return Err(AxError::failure(
                    AxCode::ConfigInvalid,
                    "build a triage table",
                    "a rule with nothing to match".to_owned(),
                )
                .with_recovery(
                    "give the rule something to match, or change the fallback address, which is \
                     what catches everything else",
                ));
            }
        }
        Ok(Triage { rules, fallback })
    }

    /// Decides where one arrival lands.
    ///
    /// First match in table order wins, so the table reads top to bottom
    /// the way the person wrote it. Two runs of the same arrival against
    /// the same table give the same answer — there is no clock here and
    /// no state that survives the call.
    #[must_use]
    pub fn decide(&self, arrival: &Arrival) -> Landing {
        let haystack = format!("{} {}", arrival.source, arrival.subject).to_lowercase();
        for rule in &self.rules {
            if !haystack.contains(&rule.matches.trim().to_lowercase()) {
                continue;
            }
            // A rule may send tainted material to a person or to the
            // bin; it may not start work on it by itself.
            let reflex = if arrival.tainted && matches!(rule.reflex, Reflex::Light | Reflex::Full) {
                Reflex::Notify
            } else {
                rule.reflex
            };
            let because = if reflex == rule.reflex {
                format!("rule `{}`", rule.matches)
            } else {
                format!(
                    "rule `{}`, held at notify because it came from outside",
                    rule.matches
                )
            };
            return Landing {
                addr: rule.landing.clone(),
                reflex,
                because,
            };
        }
        Landing {
            addr: self.fallback.clone(),
            reflex: Reflex::Notify,
            because: "no rule matched, so a person reads it".to_owned(),
        }
    }
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

    fn table() -> Triage {
        Triage::new(
            vec![
                Rule {
                    matches: "build failed".to_owned(),
                    landing: Address::parse("lab/ci").unwrap(),
                    reflex: Reflex::Full,
                },
                Rule {
                    matches: "newsletter".to_owned(),
                    landing: Address::parse("lab").unwrap(),
                    reflex: Reflex::Discard,
                },
            ],
            Address::parse("hall").unwrap(),
        )
        .unwrap()
    }

    fn arrival(source: &str, subject: &str, tainted: bool) -> Arrival {
        Arrival {
            source: source.to_owned(),
            subject: subject.to_owned(),
            tainted,
        }
    }

    #[test]
    fn the_same_arrival_twice_lands_the_same_way() {
        let table = table();
        let one = arrival("ci@lab", "build failed on main", false);
        assert_eq!(table.decide(&one), table.decide(&one));
        let landing = table.decide(&one);
        assert_eq!(landing.addr.as_str(), "lab/ci");
        assert_eq!(landing.reflex, Reflex::Full);
    }

    #[test]
    fn nothing_matching_is_a_person_reading_it_rather_than_an_error() {
        let landing = table().decide(&arrival("someone", "a question", false));
        assert_eq!(landing.addr.as_str(), "hall");
        assert_eq!(landing.reflex, Reflex::Notify);
        assert!(landing.because.contains("no rule matched"));
    }

    #[test]
    fn outside_material_never_starts_work_by_itself() {
        let landing = table().decide(&arrival("ci@lab", "build failed on main", true));
        assert_eq!(
            landing.reflex,
            Reflex::Notify,
            "a rule may route tainted material; it may not act on it"
        );
        assert_eq!(landing.addr.as_str(), "lab/ci", "the address still holds");
        assert!(landing.because.contains("outside"));
    }

    #[test]
    fn a_rule_that_matches_everything_is_refused_at_the_table() {
        let refused = Triage::new(
            vec![Rule {
                matches: "  ".to_owned(),
                landing: Address::parse("lab").unwrap(),
                reflex: Reflex::Full,
            }],
            Address::parse("hall").unwrap(),
        );
        assert!(refused.is_err());
    }

    #[test]
    fn the_table_reads_top_to_bottom() {
        let table = Triage::new(
            vec![
                Rule {
                    matches: "build".to_owned(),
                    landing: Address::parse("lab/first").unwrap(),
                    reflex: Reflex::Light,
                },
                Rule {
                    matches: "build failed".to_owned(),
                    landing: Address::parse("lab/second").unwrap(),
                    reflex: Reflex::Full,
                },
            ],
            Address::parse("hall").unwrap(),
        )
        .unwrap();
        assert_eq!(
            table
                .decide(&arrival("ci", "build failed", false))
                .addr
                .as_str(),
            "lab/first",
            "the person wrote the table in an order, and that order is the answer"
        );
    }
}
