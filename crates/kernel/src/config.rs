// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Three-layer config resolution and the frozen/live split. `FrozenConfig` and `LiveConfig` share no field: the freeze
//! line is a machine-checkable property, not a review note.

use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::consts_policy::CLOCK_STAMP_DEFAULT;
use crate::model::Effort;
use crate::tool::ServerLabel;

/// Clock-stamp cadence for result envelopes. The
/// granularity rides the enum; `Off` costs zero bytes in the window.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockStampGranularity {
    Off,
    Minute,
    FiveMinute,
    Hour,
}

/// One value across the City -> Building -> Resident override ladder.
/// Lower layers win; absence falls through upward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayeredValue<T> {
    pub city: Option<T>,
    pub building: Option<T>,
    pub resident: Option<T>,
}

// Manual impl: the derive would demand `T: Default`, an undesired bound —
// an all-None ladder is a valid default for any T.
impl<T> Default for LayeredValue<T> {
    fn default() -> Self {
        LayeredValue {
            city: None,
            building: None,
            resident: None,
        }
    }
}

impl<T> LayeredValue<T> {
    pub fn resolve(&self) -> Option<&T> {
        self.resident
            .as_ref()
            .or(self.building.as_ref())
            .or(self.city.as_ref())
    }
}

/// One concern timezone: an already-resolved offset, never a tz name —
/// re-resolving names against a moving tz database would fork replayed
/// history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockZone {
    pub id: String,
    pub offset_min: i32,
}

/// What a run's execution boundary allows. Resolved as one value rather
/// than field by field: a layer that speaks about the sandbox speaks
/// about all of it, so an under-specified layer can only ever reduce
/// what a run may do, never silently grant something the layer above it
/// never mentioned.
///
/// Host facts are deliberately absent — where the Python artifact lives
/// and which shell binary exists belong to the machine, not to the city,
/// and a city carried to another machine must not carry its paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxLimits {
    /// Whether the shell arm may be offered at all. Off by default: a
    /// shell line is the one arm whose reach cannot be read off its
    /// arguments.
    pub shell: bool,
    /// Instruction budget for one sandboxed call.
    pub fuel: u64,
    /// Extra readable paths, relative to the city root. The write domain
    /// is decided elsewhere; this only widens what may be read.
    pub mounts: Vec<Address>,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        SandboxLimits {
            shell: false,
            fuel: crate::consts_policy::SANDBOX_FUEL_DEFAULT,
            mounts: Vec::new(),
        }
    }
}

/// One external tool server this run may reach, as its configuration
/// states it: a label, a program on this machine, and that program's
/// arguments.
///
/// The command and its arguments are host facts, for the same reason
/// the sandbox keeps them out: a city carried to another machine must
/// not carry this machine's paths inside its history. They live in
/// `CONFIG.toml` and never in a ledger payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServer {
    pub label: ServerLabel,
    pub transport: McpTransport,
}

/// How this city reaches one server. Exhaustive rather than a URL that
/// might also be a command: the two are different machines to start
/// talking to, they fail differently, and a configuration that leaves it
/// to be guessed is one that guesses wrong on the day it matters.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    /// A program on this machine, spoken to over its own pipes.
    Stdio { command: String, args: Vec<String> },
    /// A server reached over HTTP, which is how a hosted catalogue is
    /// published. The city holds no account with it: whatever it needs
    /// travels in the header the configuration names.
    Http {
        url: String,
        /// A header to send, as `name: value`. Absent for a server that
        /// asks for none.
        header: Option<String>,
    },
}

/// Frozen at Run start, never re-read within the Run.
/// Fields only grow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenConfig {
    pub clock_stamp: ClockStampGranularity,
    pub clock_zones: Vec<ClockZone>,
    /// Frozen for the same reason the tool table is: what a run may
    /// reach decides what it was told it could do, and a capability that
    /// widens mid-run is one nobody reviewed.
    pub sandbox: SandboxLimits,
    /// External tool servers, frozen for the same reason as the tool
    /// table itself: their tools enter the catalog at run start, a
    /// provider hashes that array ahead of the system prompt, and a
    /// table that widened mid-run would both discard the cache and
    /// hand the model a capability nobody reviewed.
    pub mcp: Vec<McpServer>,
    /// How hard the model may think. Frozen rather than live because the
    /// provider renders it into the cached prompt prefix: changing the
    /// effort value mid-run invalidates the message cache breakpoints,
    /// so a run that could retune itself would keep paying to rebuild
    /// the cache it just discarded. A change applies to the next run.
    pub effort: Option<Effort>,
}

/// Hot-reloadable surface. Empty in S2 by design: the type exists so the
/// no-field-overlap assertion guards every future addition (S4 fills it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveConfig {}

/// Resolves the ladder into the Run-start snapshot. Absence everywhere
/// falls back to the policy default; the zones ladder overrides as a
/// whole list (a building that writes zones replaces the city list).
pub fn freeze(
    clock_stamp: &LayeredValue<ClockStampGranularity>,
    clock_zones: &LayeredValue<Vec<ClockZone>>,
    effort: &LayeredValue<Effort>,
    sandbox: &LayeredValue<SandboxLimits>,
    mcp: &LayeredValue<Vec<McpServer>>,
) -> FrozenConfig {
    FrozenConfig {
        clock_stamp: *clock_stamp.resolve().unwrap_or(&CLOCK_STAMP_DEFAULT),
        clock_zones: clock_zones.resolve().cloned().unwrap_or_default(),
        sandbox: sandbox.resolve().cloned().unwrap_or_default(),
        effort: effort.resolve().copied(),
        // A layer that speaks about servers speaks about all of them:
        // an unstated layer reaches nothing rather than inheriting a
        // reach nobody at that layer wrote down.
        mcp: mcp.resolve().cloned().unwrap_or_default(),
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
    use std::collections::BTreeSet;

    #[test]
    fn lower_layer_overrides_upper() {
        let ladder = LayeredValue {
            city: Some(ClockStampGranularity::Hour),
            building: Some(ClockStampGranularity::Minute),
            resident: None,
        };
        assert_eq!(
            freeze(
                &ladder,
                &LayeredValue::default(),
                &LayeredValue::default(),
                &LayeredValue::default(),
                &LayeredValue::default()
            )
            .clock_stamp,
            ClockStampGranularity::Minute
        );
        let ladder = LayeredValue {
            city: Some(ClockStampGranularity::Hour),
            building: Some(ClockStampGranularity::Minute),
            resident: Some(ClockStampGranularity::FiveMinute),
        };
        assert_eq!(
            freeze(
                &ladder,
                &LayeredValue::default(),
                &LayeredValue::default(),
                &LayeredValue::default(),
                &LayeredValue::default()
            )
            .clock_stamp,
            ClockStampGranularity::FiveMinute
        );
    }

    #[test]
    fn absence_everywhere_takes_the_policy_default() {
        let ladder: LayeredValue<ClockStampGranularity> = LayeredValue::default();
        let frozen = freeze(
            &ladder,
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
        );
        assert_eq!(frozen.clock_stamp, CLOCK_STAMP_DEFAULT);
        assert_eq!(frozen.clock_stamp, ClockStampGranularity::Off);
        assert!(frozen.clock_zones.is_empty());
        assert_eq!(
            frozen.effort, None,
            "an unstated effort stays unstated: the provider's default is not ours to name"
        );
    }

    #[test]
    fn effort_resolves_down_the_same_ladder() {
        let effort = LayeredValue {
            city: Some(Effort::Low),
            building: Some(Effort::Max),
            resident: None,
        };
        let frozen = freeze(
            &LayeredValue::default(),
            &LayeredValue::default(),
            &effort,
            &LayeredValue::default(),
            &LayeredValue::default(),
        );
        assert_eq!(frozen.effort, Some(Effort::Max));
    }

    #[test]
    fn the_sandbox_resolves_as_one_value_so_a_thin_layer_only_narrows() {
        let permissive = SandboxLimits {
            shell: true,
            fuel: 10,
            mounts: vec![Address::parse("lab/docs").unwrap()],
        };
        let terse = SandboxLimits {
            fuel: 20,
            ..SandboxLimits::default()
        };
        let ladder = LayeredValue {
            city: Some(permissive),
            building: Some(terse),
            resident: None,
        };
        let frozen = freeze(
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
            &ladder,
            &LayeredValue::default(),
        );
        assert_eq!(frozen.sandbox.fuel, 20);
        assert!(
            !frozen.sandbox.shell,
            "a layer that speaks about the sandbox speaks about all of it, and silence is the              closed answer"
        );
        assert!(frozen.sandbox.mounts.is_empty());
    }

    #[test]
    fn an_unstated_sandbox_is_closed_with_the_default_fuel() {
        let frozen = freeze(
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
        );
        assert!(!frozen.sandbox.shell);
        assert_eq!(
            frozen.sandbox.fuel,
            crate::consts_policy::SANDBOX_FUEL_DEFAULT
        );
        assert!(frozen.sandbox.mounts.is_empty());
    }

    #[test]
    fn zones_override_as_a_whole_list() {
        let zones = LayeredValue {
            city: Some(vec![
                ClockZone {
                    id: "tokyo".to_owned(),
                    offset_min: 540,
                },
                ClockZone {
                    id: "berlin".to_owned(),
                    offset_min: 120,
                },
            ]),
            building: Some(vec![ClockZone {
                id: "nyc".to_owned(),
                offset_min: -240,
            }]),
            resident: None,
        };
        let frozen = freeze(
            &LayeredValue::default(),
            &zones,
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
        );
        assert_eq!(frozen.clock_zones.len(), 1);
        assert_eq!(frozen.clock_zones[0].id, "nyc");
    }

    #[test]
    fn servers_override_as_a_whole_table_and_silence_reaches_nothing() {
        let server = |raw: &str| McpServer {
            label: ServerLabel::parse(raw).unwrap(),
            transport: McpTransport::Stdio {
                command: "mcp-server".to_owned(),
                args: Vec::new(),
            },
        };
        let ladder = LayeredValue {
            city: Some(vec![server("apps"), server("mail")]),
            building: Some(vec![server("apps")]),
            resident: None,
        };
        let frozen = freeze(
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
            &ladder,
        );
        assert_eq!(frozen.mcp.len(), 1);
        assert_eq!(frozen.mcp[0].label.as_str(), "apps");

        let silent = freeze(
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
            &LayeredValue::default(),
        );
        assert!(
            silent.mcp.is_empty(),
            "a building nobody granted a server reaches none of them"
        );
    }

    #[test]
    fn frozen_and_live_share_no_field() {
        let frozen = serde_json::to_value(FrozenConfig {
            clock_stamp: ClockStampGranularity::Off,
            clock_zones: Vec::new(),
            sandbox: SandboxLimits::default(),
            effort: None,
            mcp: Vec::new(),
        })
        .unwrap();
        let live = serde_json::to_value(LiveConfig {}).unwrap();
        let frozen_keys: BTreeSet<String> = frozen
            .as_object()
            .expect("frozen serializes to an object")
            .keys()
            .cloned()
            .collect();
        let live_keys: BTreeSet<String> = live
            .as_object()
            .expect("live serializes to an object")
            .keys()
            .cloned()
            .collect();
        assert!(
            frozen_keys.is_disjoint(&live_keys),
            "freeze line breached: {frozen_keys:?} vs {live_keys:?}"
        );
    }

    #[test]
    fn granularity_serde_is_snake_case() {
        let json = serde_json::to_string(&ClockStampGranularity::FiveMinute).unwrap();
        assert_eq!(json, "\"five_minute\"");
    }
}
