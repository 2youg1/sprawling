// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Speaking into a run that is already working.
//!
//! Two entrances, one landing. The person's steer arrives as a control
//! surface command and never travels through the Inbox; an agent's steer
//! is a signal that overtakes the queue. Both land in the same place —
//! appended to the end of the next tool result — because the model
//! should have to recognise one shape, not two.
//!
//! The entrances stay apart for a different reason than the landing
//! stays together: content that claims to come from the person must not
//! be able to render as the person. Only [`Steer::from_person`] can
//! write the `user` prefix; an agent's steer builds its prefix from its
//! own id.

use kernel::{Address, AxCode, AxError, TimeMs, Version};

use crate::inbox::{Signal, SignalId, SignalKind};

/// How the person's steer is attributed in the window.
const PERSON_SOURCE: &str = "user";

/// A steer at its landing: who is speaking, and what they said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Steer {
    source: String,
    text: String,
}

impl Steer {
    /// The control surface's entrance. The only constructor that can
    /// produce the `user` prefix.
    ///
    /// # Errors
    /// Refuses empty text: an interruption that says nothing costs a
    /// turn and gives the run nothing to act on.
    pub fn from_person(text: &str) -> Result<Steer, AxError> {
        Ok(Steer {
            source: PERSON_SOURCE.to_owned(),
            text: non_empty(text)?,
        })
    }

    /// The Inbox's entrance: a steer-kind signal, attributed to the
    /// resident that sent it.
    ///
    /// # Errors
    /// Refuses a signal of any other kind — an ordinary mention does not
    /// get to interrupt — and one whose payload carries no text.
    pub fn from_signal(signal: &Signal) -> Result<Steer, AxError> {
        if signal.kind() != SignalKind::Steer {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "read a signal as a steer",
                signal.kind().as_str().to_owned(),
            )
            .with_recovery("only a steer-kind signal overtakes; deliver the rest to the inbox"));
        }
        let text = signal
            .payload()
            .as_map()
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        Ok(Steer {
            source: agent_source(signal.from()),
            text: non_empty(text)?,
        })
    }

    /// `user`, or `@id` for a resident.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A steer on its way out of one resident and into another's inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSteer {
    id: String,
    text: String,
}

impl AgentSteer {
    /// # Errors
    /// Refuses an unnamed sender and empty text.
    pub fn new(id: &str, text: &str) -> Result<AgentSteer, AxError> {
        if id.is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "build an agent steer",
                "unnamed sender".to_owned(),
            )
            .with_recovery("a steer is read as coming from someone; give the sender's address"));
        }
        Ok(AgentSteer {
            id: id.to_owned(),
            text: non_empty(text)?,
        })
    }

    /// The signal this steer travels as. It carries its text in the
    /// payload under `text`, which is where [`Steer::from_signal`] reads
    /// it: one writer, one reader.
    ///
    /// # Errors
    /// Propagates the signal's own refusals.
    pub fn signal(
        &self,
        id: SignalId,
        room: Address,
        room_version: Version,
        at: TimeMs,
    ) -> Result<Signal, AxError> {
        let mut body = serde_json::Map::new();
        body.insert(
            "text".to_owned(),
            serde_json::Value::String(self.text.clone()),
        );
        Signal::new(
            id,
            SignalKind::Steer,
            self.id.clone(),
            room,
            room_version,
            kernel::Payload::new(body)?,
            at,
        )
    }

    /// What this steer looks like where it lands.
    #[must_use]
    pub fn landing(&self) -> Steer {
        Steer {
            source: agent_source(&self.id),
            text: self.text.clone(),
        }
    }
}

fn agent_source(id: &str) -> String {
    format!("@{id}")
}

fn non_empty(text: &str) -> Result<String, AxError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "build a steer",
            "empty text".to_owned(),
        )
        .with_recovery("say what should change; a steer with no words costs a turn"));
    }
    Ok(trimmed.to_owned())
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
    use kernel::Payload;

    fn room() -> Address {
        Address::parse("lab/room2").unwrap()
    }

    #[test]
    fn the_person_is_the_only_one_who_lands_as_user() {
        let person = Steer::from_person("wrap up").unwrap();
        assert_eq!(person.source(), "user");

        let agent = AgentSteer::new("lab/room1", "wrap up").unwrap();
        assert_eq!(agent.landing().source(), "@lab/room1");
        assert_ne!(agent.landing().source(), "user");
    }

    #[test]
    fn an_agent_steer_travels_as_a_signal_and_lands_as_itself() {
        let agent = AgentSteer::new("lab/room1", "check the units").unwrap();
        let signal = agent
            .signal(
                SignalId::parse("s-1").unwrap(),
                room(),
                Version::new(2),
                TimeMs::new(9),
            )
            .unwrap();
        assert_eq!(signal.kind(), SignalKind::Steer);

        let landed = Steer::from_signal(&signal).unwrap();
        assert_eq!(landed, agent.landing());
        assert_eq!(landed.text(), "check the units");
    }

    #[test]
    fn an_ordinary_mention_does_not_get_to_interrupt() {
        let mut body = serde_json::Map::new();
        body.insert(
            "text".to_owned(),
            serde_json::Value::String("have a look".to_owned()),
        );
        let mention = Signal::new(
            SignalId::parse("s-2").unwrap(),
            SignalKind::Mention,
            "lab/room1".to_owned(),
            room(),
            Version::new(1),
            Payload::new(body).unwrap(),
            TimeMs::new(9),
        )
        .unwrap();

        let err = Steer::from_signal(&mention).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(err.recovery().contains("inbox"));
    }

    #[test]
    fn a_steer_with_nothing_in_it_is_refused_at_every_entrance() {
        assert!(Steer::from_person("   ").is_err());
        assert!(AgentSteer::new("lab/room1", "\n").is_err());
        assert!(AgentSteer::new("", "text").is_err());
    }

    #[test]
    fn the_text_arrives_trimmed_because_the_window_shows_it_verbatim() {
        assert_eq!(
            Steer::from_person("  wrap up \n").unwrap().text(),
            "wrap up"
        );
    }
}
