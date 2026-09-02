// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Everything a client may ask the city to do, and the two invariants
//! that live in the type system rather than in a check.
//!
//! - `Command` is generic over the carrier of a secret. `WireCommand`
//!   fixes that carrier to an uninhabited type, so a frame arriving from
//!   a socket cannot be a `PutSecret` — not "is rejected", but has no
//!   representation. Credentials are enrolled on the host machine, and
//!   that constraint is held by construction.
//! - Every state-changing Command owns an `IdemKey` field. There is no
//!   constructor that omits it, so "double-clicking twice opens two
//!   runs" is not reachable from this type.
//!
//! The enum is not `#[non_exhaustive]`: the wire version is the
//! versioning mechanism, so the assembly layer must handle every variant
//! and a new one fails to compile until somebody decides what it does.
//! That is what keeps a button off the client until the city can answer
//! the frame behind it.

use kernel::{
    Address, ApprovalId, Autonomy, BudgetCap, DialectKind, Effort, GitOid, IdemKey, McpServer,
    ModelTag, PolicyVerdict, RunId, SandboxLimits, Sealed, Seq, SessionName,
};
use serde::{Deserialize, Serialize};

use crate::carried_name::{ModeTag, ProviderName, TemplateName, UploadId};

/// The Command surface, in declaration order.
/// This table feeds [`schema_hash`]; a connection whose peer computes a
/// different hash is refused rather than served a half-understood protocol.
pub const COMMAND_NAMES: [&str; 23] = [
    "Dispatch",
    "Wake",
    "Login",
    "ProbeEndpoint",
    "AttachEndpoint",
    "SelectModel",
    "Fork",
    "Attach",
    "CreateBuilding",
    "ConfigureBuilding",
    "PutSecret",
    "Steer",
    "Cancel",
    "Takeover",
    "Rollback",
    "Halt",
    "Release",
    "BatchByBuilding",
    "Approve",
    "CreatePolicy",
    "SetAutonomy",
    "Pursue",
    "Auth",
];

/// Uninhabited on purpose. A value of this type cannot be produced, so
/// `Command<NoSecret>` has no reachable `PutSecret` variant. This is the
/// compile-time half of "a remote connection cannot spell that frame";
/// `Deserialize` supplies the runtime half for bytes that try anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoSecret {}

impl Serialize for NoSecret {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        match *self {}
    }
}

impl<'de> Deserialize<'de> for NoSecret {
    fn deserialize<D: serde::Deserializer<'de>>(_deserializer: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom(
            "a credential cannot be enrolled over a connection; enrol it on the host",
        ))
    }
}

/// Which step of a subscription login a `Login` frame carries.
///
/// The authorization code arrives by hand: the provider shows it to the
/// person after they approve, and the person brings it back. That is
/// the flow the profile table describes, and it needs no listening port
/// of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginStep {
    /// Mint the authorization URL for a person to open.
    Begin,
    /// Redeem the code that person brought back.
    Code { code: String },
}

/// What a Halt, Release or Autonomy change applies to. Unlike modes and
/// providers, this set is the protocol's own and has no upstream owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaltScope {
    City,
    Building(Address),
    Workshop(Address),
}

/// What a `Pursue` command does to a pursuit.
///
/// `Clear` and `Pause` are different actions and both exist: pausing
/// keeps the goal so it can be taken up again, and clearing throws it
/// away. Cancelling a *run* is a third thing again, and it has its own
/// command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PursuitStep {
    /// Declare one, replacing any goal this building already had.
    Set {
        goal: String,
    },
    Pause,
    Resume,
    Clear,
}

/// Commands change state, require authorization, and are idempotent.
///
/// Deliberately *not* `#[non_exhaustive]`: the schema hash is this type's
/// version mechanism, so the assembly layer must handle every one of them
/// and a new one fails to compile until somebody decides what it does.
/// That is the rule that keeps a button off the client until the city can
/// answer the frame behind it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command<Secret = Sealed<String>> {
    Dispatch {
        addr: Address,
        task: String,
        goal: String,
        mode: ModeTag,
        budget: BudgetCap,
        idem: IdemKey,
        /// What this session is called, when it is a new one.
        ///
        /// `Some` means `addr` names a building and the city opens a
        /// room of this name under it; `None` means `addr` already
        /// names the room, which is how an earlier session is
        /// continued. Two dispatches to one room are one session with a
        /// history, and that is a different thing from two sessions
        /// sharing a folder.
        session: Option<SessionName>,
        /// How hard the model is asked to think in this session.
        ///
        /// `None` leaves the layer above to answer, which is the city's
        /// own configuration and, failing that, the provider's default.
        /// A value is written into the session's own configuration when
        /// its room is opened, so it is chosen once and holds for every
        /// run in that room (city-SPEC.md section 8-14).
        effort: Option<Effort>,
    },
    /// One step of a subscription login. Which step is named rather
    /// than inferred: beginning and redeeming are different actions
    /// with different failure modes, and a page that means one must not
    /// be readable as the other.
    Login {
        provider: ProviderName,
        step: LoginStep,
        idem: IdemKey,
    },
    /// Register a provider the person just entered. The credential is
    /// already in the vault by the time this frame exists: what travels
    /// here is the reference to it, which is why this command has a byte
    /// form and `PutSecret` does not.
    /// Asks a base URL what it serves, and attaches nothing.
    ///
    /// A person cannot choose from a list they have not seen, and the
    /// list used to arrive only as a side effect of attaching - so
    /// looking at what a key buys meant registering it first. The answer
    /// lands as `endpoint_probed`, which the page folds like any other
    /// fact about this city.
    ProbeEndpoint {
        name: ProviderName,
        base_url: String,
        dialect: DialectKind,
        secret: Option<String>,
        auth_header: Option<String>,
        idem: IdemKey,
    },
    /// What a building's runs may reach: the sandbox's limits, and the
    /// external servers its tools come from.
    ///
    /// Both resolve city -> building -> room and neither had a surface,
    /// so a person could read what they were governed by and not change
    /// it. Each field is optional and an absent one leaves that section
    /// alone; an empty `mcp` list is a building that reaches no server,
    /// which is a different statement from not saying.
    ConfigureBuilding {
        addr: Address,
        sandbox: Option<SandboxLimits>,
        mcp: Option<Vec<McpServer>>,
        idem: IdemKey,
    },
    AttachEndpoint {
        name: ProviderName,
        /// The base URL as a provider's documentation prints it; the
        /// dialect owns the path that hangs off it.
        base_url: String,
        dialect: DialectKind,
        /// `secret:<realm>/<name>`, or absent for a local server that
        /// asks for no credential.
        secret: Option<String>,
        /// Header name for providers that do not take a bearer token.
        auth_header: Option<String>,
        /// Which of the models this endpoint serves the city admits. An
        /// empty list admits everything it serves, which is what a
        /// person who did not look at the list meant.
        admit: Vec<String>,
        idem: IdemKey,
    },
    /// Point one tag at one model of an attached endpoint, with the two
    /// facts no model list returns.
    SelectModel {
        endpoint: ProviderName,
        model: String,
        tag: ModelTag,
        context_tokens: u64,
        max_output_tokens: u64,
        idem: IdemKey,
    },
    Fork {
        run: RunId,
        at_seq: Seq,
        addr: Option<Address>,
        idem: IdemKey,
    },
    Attach {
        upload: UploadId,
        notify: Vec<RunId>,
        idem: IdemKey,
    },
    CreateBuilding {
        addr: Address,
        template: TemplateName,
        idem: IdemKey,
    },
    /// The one Command with no byte form. `Secret` is `Sealed<String>` in
    /// process and uninhabited on the wire.
    PutSecret {
        realm: String,
        name: String,
        value: Secret,
    },
    Steer {
        run: RunId,
        text: String,
        idem: IdemKey,
    },
    Cancel {
        run: RunId,
        idem: IdemKey,
    },
    Takeover {
        run: RunId,
        idem: IdemKey,
    },
    Rollback {
        checkpoint: GitOid,
        idem: IdemKey,
    },
    Halt {
        scope: HaltScope,
        idem: IdemKey,
    },
    Release {
        scope: HaltScope,
        idem: IdemKey,
    },
    BatchByBuilding {
        addr: Address,
        idem: IdemKey,
    },
    Approve {
        item: ApprovalId,
        verdict: PolicyVerdict,
        idem: IdemKey,
    },
    CreatePolicy {
        from_item: ApprovalId,
        idem: IdemKey,
    },
    SetAutonomy {
        scope: HaltScope,
        autonomy: Autonomy,
        idem: IdemKey,
    },
    /// A goal the city keeps working towards until the work runs out.
    ///
    /// One frame with four steps rather than four frames: a person who
    /// can set a goal can pause it, and splitting that into separate
    /// commands would let a client offer one without the other.
    Pursue {
        addr: Address,
        step: PursuitStep,
        idem: IdemKey,
    },
    /// Something happened outside. The city never asks whether anything
    /// did; the service holding the connection pushes, and this is the
    /// shape a push takes once it is inside.
    ///
    /// It carries no address. Where an arrival lands is the watch
    /// table's answer and then triage's, so a caller that could name a
    /// room would be a caller that could reach past the routing a person
    /// wrote.
    Wake {
        source: String,
        subject: String,
        body: String,
        idem: IdemKey,
    },
    /// Presenting a pairing token. Read-only, hence no `IdemKey`; the token
    /// is plain here because a token that must cross a wire has, by
    /// definition, no secrecy left to protect in transit - it is sealed the
    /// moment it lands (see `server::decide_handshake`).
    Auth {
        token: String,
    },
}

/// The Command set a socket can carry. `PutSecret` is unreachable because
/// `NoSecret` has no values.
pub type WireCommand = Command<NoSecret>;

impl<Secret> Command<Secret> {
    /// Exhaustive by construction: a new variant cannot compile without
    /// choosing its name here, which is what keeps [`COMMAND_NAMES`] honest.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match *self {
            Self::Dispatch { .. } => "Dispatch",
            Self::Wake { .. } => "Wake",
            Self::Login { .. } => "Login",
            Self::ConfigureBuilding { .. } => "ConfigureBuilding",
            Self::ProbeEndpoint { .. } => "ProbeEndpoint",
            Self::AttachEndpoint { .. } => "AttachEndpoint",
            Self::SelectModel { .. } => "SelectModel",
            Self::Fork { .. } => "Fork",
            Self::Attach { .. } => "Attach",
            Self::CreateBuilding { .. } => "CreateBuilding",
            Self::PutSecret { .. } => "PutSecret",
            Self::Steer { .. } => "Steer",
            Self::Cancel { .. } => "Cancel",
            Self::Takeover { .. } => "Takeover",
            Self::Rollback { .. } => "Rollback",
            Self::Halt { .. } => "Halt",
            Self::Release { .. } => "Release",
            Self::BatchByBuilding { .. } => "BatchByBuilding",
            Self::Approve { .. } => "Approve",
            Self::CreatePolicy { .. } => "CreatePolicy",
            Self::SetAutonomy { .. } => "SetAutonomy",
            Self::Pursue { .. } => "Pursue",
            Self::Auth { .. } => "Auth",
        }
    }

    /// The deduplication key. `None` only for the two Commands that change
    /// nothing: `Auth`, and `PutSecret` whose effect is confined to the host
    /// process and whose replay writes the same Vault entry.
    pub fn idem(&self) -> Option<&IdemKey> {
        match *self {
            Self::Dispatch { ref idem, .. }
            | Self::Wake { ref idem, .. }
            | Self::Login { ref idem, .. }
            | Self::Fork { ref idem, .. }
            | Self::ProbeEndpoint { ref idem, .. }
            | Self::ConfigureBuilding { ref idem, .. }
            | Self::Attach { ref idem, .. }
            | Self::CreateBuilding { ref idem, .. }
            | Self::Steer { ref idem, .. }
            | Self::Cancel { ref idem, .. }
            | Self::Takeover { ref idem, .. }
            | Self::Rollback { ref idem, .. }
            | Self::Halt { ref idem, .. }
            | Self::Release { ref idem, .. }
            | Self::BatchByBuilding { ref idem, .. }
            | Self::Approve { ref idem, .. }
            | Self::CreatePolicy { ref idem, .. }
            | Self::SetAutonomy { ref idem, .. }
            | Self::Pursue { ref idem, .. }
            | Self::AttachEndpoint { ref idem, .. }
            | Self::SelectModel { ref idem, .. } => Some(idem),
            Self::PutSecret { .. } | Self::Auth { .. } => None,
        }
    }
}

impl From<WireCommand> for Command {
    /// Widening a wire frame into the in-process Command set. Total: the
    /// `PutSecret` arm is unreachable because its payload cannot exist.
    fn from(wire: WireCommand) -> Self {
        match wire {
            Command::Wake {
                source,
                subject,
                body,
                idem,
            } => Self::Wake {
                source,
                subject,
                body,
                idem,
            },
            Command::Dispatch {
                addr,
                task,
                goal,
                mode,
                budget,
                idem,
                session,
                effort,
            } => Self::Dispatch {
                addr,
                task,
                goal,
                mode,
                budget,
                idem,
                session,
                effort,
            },
            Command::Login {
                provider,
                step,
                idem,
            } => Self::Login {
                provider,
                step,
                idem,
            },
            Command::ConfigureBuilding {
                addr,
                sandbox,
                mcp,
                idem,
            } => Self::ConfigureBuilding {
                addr,
                sandbox,
                mcp,
                idem,
            },
            Command::ProbeEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                idem,
            } => Self::ProbeEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                idem,
            },
            Command::AttachEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                admit,
                idem,
            } => Self::AttachEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                admit,
                idem,
            },
            Command::SelectModel {
                endpoint,
                model,
                tag,
                context_tokens,
                max_output_tokens,
                idem,
            } => Self::SelectModel {
                endpoint,
                model,
                tag,
                context_tokens,
                max_output_tokens,
                idem,
            },
            Command::Fork {
                run,
                at_seq,
                addr,
                idem,
            } => Self::Fork {
                run,
                at_seq,
                addr,
                idem,
            },
            Command::Attach {
                upload,
                notify,
                idem,
            } => Self::Attach {
                upload,
                notify,
                idem,
            },
            Command::CreateBuilding {
                addr,
                template,
                idem,
            } => Self::CreateBuilding {
                addr,
                template,
                idem,
            },
            Command::PutSecret { value, .. } => match value {},
            Command::Steer { run, text, idem } => Self::Steer { run, text, idem },
            Command::Cancel { run, idem } => Self::Cancel { run, idem },
            Command::Takeover { run, idem } => Self::Takeover { run, idem },
            Command::Rollback { checkpoint, idem } => Self::Rollback { checkpoint, idem },
            Command::Halt { scope, idem } => Self::Halt { scope, idem },
            Command::Release { scope, idem } => Self::Release { scope, idem },
            Command::BatchByBuilding { addr, idem } => Self::BatchByBuilding { addr, idem },
            Command::Approve {
                item,
                verdict,
                idem,
            } => Self::Approve {
                item,
                verdict,
                idem,
            },
            Command::CreatePolicy { from_item, idem } => Self::CreatePolicy { from_item, idem },
            Command::Pursue { addr, step, idem } => Self::Pursue { addr, step, idem },
            Command::SetAutonomy {
                scope,
                autonomy,
                idem,
            } => Self::SetAutonomy {
                scope,
                autonomy,
                idem,
            },
            Command::Auth { token } => Self::Auth { token },
        }
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    /// The runtime half of "a credential cannot be enrolled over a
    /// connection": the compile-time half is `NoSecret` having no values,
    /// and this is what happens to bytes that try anyway.
    #[test]
    fn a_wire_frame_carrying_put_secret_fails_to_decode() {
        let json = r#"{"put_secret":{"realm":"anthropic","name":"api","value":"sk-nope"}}"#;
        let decoded: Result<WireCommand, _> = serde_json::from_str(json);
        let err = decoded.expect_err("PutSecret has no wire form");
        assert!(
            !err.to_string().contains("sk-nope"),
            "the refusal must not echo the bytes it protects"
        );
    }
}
