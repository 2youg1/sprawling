// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The control surface: which frames intervene in work already running, and
//! what an intervention owes when it does.
//!
//! A human's Steer arrives here, not through an Inbox;
//! an Agent's Steer arrives as a high-priority Signal. Both land in the same
//! place - the result envelope - so the model only ever learns one shape.
//!
//! This module decides and returns; it holds no Ledger handle and writes
//! nothing. The obligation it reports is discharged by the assembly layer,
//! and the closing card asserts the obligation was met.

use kernel::{AxCode, AxError, RunId};

use crate::wire::Command;

/// The verbs the control surface shows. Five interventions plus `Release`,
/// the return path that `Halt` needs in order not to be a trap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intervention {
    Steer,
    Cancel,
    Takeover,
    Rollback,
    Halt,
    Release,
}

impl Intervention {
    /// The verb as the interface spells it. This is also the microcopy
    /// authority: one verb, one word, everywhere.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steer => "Steer",
            Self::Cancel => "Cancel",
            Self::Takeover => "Takeover",
            Self::Rollback => "Rollback",
            Self::Halt => "Halt",
            Self::Release => "Release",
        }
    }
}

/// What the control surface makes of one Command.
#[derive(Debug)]
pub enum ControlVerdict {
    Intervene {
        verb: Intervention,
        /// The Run being interrupted, when the verb names one. `Halt` and
        /// `Release` act on a scope and leave this empty.
        run: Option<RunId>,
        /// Whether the assembly layer owes a `handoff_written` before the
        /// intervention is complete.
        must_write_handoff: bool,
    },
    NotAnIntervention,
    Refuse(AxError),
}

/// Classifies one Command.
///
/// Pure and exhaustive: every one of the Commands is answered, so
/// a new Command cannot be added without deciding whether it interrupts
/// anything. That question is the reason this module is not part of the
/// listener - the listener could not answer it, and would not know it had
/// been asked.
#[must_use]
pub fn classify(command: &Command) -> ControlVerdict {
    match *command {
        Command::Steer { run, ref text, .. } => {
            if text.trim().is_empty() {
                return ControlVerdict::Refuse(
                    AxError::failure(
                        AxCode::WireMismatch,
                        "steer a Run",
                        "the steer carries no text",
                    )
                    .with_recovery("say what should change, or use Cancel to stop the Run"),
                );
            }
            intervene(Intervention::Steer, Some(run))
        }
        Command::Cancel { run, .. } => intervene(Intervention::Cancel, Some(run)),
        Command::Takeover { run, .. } => intervene(Intervention::Takeover, Some(run)),
        Command::Rollback { .. } => intervene(Intervention::Rollback, None),
        Command::Halt { .. } => scope_intervention(Intervention::Halt),
        Command::Release { .. } => scope_intervention(Intervention::Release),
        Command::Dispatch { .. }
        | Command::Wake { .. }
        | Command::ProbeEndpoint { .. }
        | Command::ConfigureBuilding { .. }
        | Command::AttachEndpoint { .. }
        | Command::SelectModel { .. }
        | Command::Login { .. }
        | Command::Fork { .. }
        | Command::Attach { .. }
        | Command::CreateBuilding { .. }
        | Command::PutSecret { .. }
        | Command::BatchByBuilding { .. }
        | Command::Approve { .. }
        | Command::CreatePolicy { .. }
        | Command::SetAutonomy { .. }
        | Command::Auth { .. } => ControlVerdict::NotAnIntervention,
    }
}

/// Interrupting a turn always owes a Handoff: the next holder - a person
/// taking over, or the Run itself after a resume - must find the complete
/// scene rather than a truncated one.
fn intervene(verb: Intervention, run: Option<RunId>) -> ControlVerdict {
    ControlVerdict::Intervene {
        verb,
        run,
        must_write_handoff: true,
    }
}

/// Halting a scope stops future work rather than interrupting a turn in
/// progress, so there is no scene to hand over.
fn scope_intervention(verb: Intervention) -> ControlVerdict {
    ControlVerdict::Intervene {
        verb,
        run: None,
        must_write_handoff: false,
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
    use kernel::{IdemKey, Seq};

    #[test]
    fn rollback_intervenes_without_naming_a_run_but_still_owes_a_handoff() {
        // Rollback names a checkpoint, not a Run: which Runs it disturbs is
        // the assembly layer's to work out from the checkpoint's scope. The
        // Handoff obligation stands, because whatever was running stops.
        let run = kernel::RunId::from_bytes([2u8; 16]);
        let command = Command::Rollback {
            checkpoint: kernel::GitOid::from_bytes([1u8; 20]),
            idem: IdemKey::derive(&run, Seq::new(1), b"rb"),
        };
        let ControlVerdict::Intervene {
            verb,
            run,
            must_write_handoff,
        } = classify(&command)
        else {
            panic!("Rollback is one of the verbs");
        };
        assert_eq!(verb, Intervention::Rollback);
        assert!(run.is_none());
        assert!(must_write_handoff);
    }
}
