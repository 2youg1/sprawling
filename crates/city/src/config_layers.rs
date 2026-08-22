// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The three configuration files a run is governed by, and where they
//! live.
//!
//! One file name, three locations: the layer is the position, so a file
//! placed at the wrong layer is a mistake you can see in the directory
//! tree rather than one you have to read character by character. The
//! city's own layer lives inside the reserved subtree, which is outside
//! every write domain — that is what makes "an agent cannot edit its own
//! configuration" a judgment instead of an expectation.
//!
//! Which layer wins and what an unstated value falls back to are
//! `kernel::config`'s answers, not this module's. This module answers
//! only which three files to read.

use std::path::{Path, PathBuf};

use kernel::{
    Address, AxCode, AxError, Effort, FrozenConfig, LayeredValue, McpServer, McpTransport,
    RESERVED_PREFIX, SandboxLimits, ServerLabel,
};
use serde::Deserialize;

use crate::building::Building;

/// The configuration file at every layer.
pub const CONFIG_FILE: &str = "CONFIG.toml";

/// One rung of the City -> Building -> Resident ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    City,
    Building,
    Resident,
}

/// Where a layer's file lives for a run at `addr`.
///
/// # Errors
/// Propagates the reserved-subtree refusal from [`Building::of`]: an
/// address with no building has no building layer to read.
pub fn path(city_root: &Path, addr: &Address, layer: Layer) -> Result<PathBuf, AxError> {
    let dir = match layer {
        Layer::City => city_root.join(RESERVED_PREFIX),
        Layer::Building => Building::of(addr)?.root(city_root),
        Layer::Resident => {
            let mut path = city_root.to_path_buf();
            for segment in addr.as_str().split('/') {
                path.push(segment);
            }
            path
        }
    };
    Ok(dir.join(CONFIG_FILE))
}

/// What one layer declares. An absent file declares nothing, which is
/// how most layers stay: a value is stated where somebody meant to
/// depart from the default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigLayer {
    effort: Option<Effort>,
    sandbox: Option<SandboxLimits>,
    mcp: Option<Vec<McpServer>>,
}

impl ConfigLayer {
    /// Reads one layer's text.
    ///
    /// # Errors
    /// Refuses a file that does not parse, and a key this version does
    /// not read. Ignoring an unrecognised key produces the one state
    /// nobody can diagnose: the setting is written, and nothing happens.
    pub fn parse(text: &str) -> Result<ConfigLayer, AxError> {
        let file: ConfigFile = toml::from_str(text).map_err(|err| refuse(err.to_string()))?;
        let sandbox = match file.sandbox {
            None => None,
            Some(section) => {
                let mut mounts = Vec::new();
                for raw in &section.mounts {
                    let mount = Address::parse(raw)?;
                    if mount.is_reserved() {
                        return Err(refuse(format!(
                            "{raw}: the city's own subtree is not mountable"
                        )));
                    }
                    mounts.push(mount);
                }
                Some(SandboxLimits {
                    shell: section.shell,
                    fuel: section
                        .fuel
                        .unwrap_or(kernel::consts_policy::SANDBOX_FUEL_DEFAULT),
                    mounts,
                })
            }
        };
        let mcp = match file.mcp {
            None => None,
            Some(entries) => {
                let mut servers: Vec<McpServer> = Vec::new();
                for entry in entries {
                    let label = ServerLabel::parse(&entry.label)
                        .map_err(|err| refuse(format!("{}: {}", entry.label, err.recovery())))?;
                    // One transport or the other, never both and never
                    // neither: a row that names a command and a url is a
                    // row whose reader has to guess which one was meant.
                    let transport = match (entry.command.as_deref(), entry.url.as_deref()) {
                        (Some(command), None) if !command.trim().is_empty() => {
                            McpTransport::Stdio {
                                command: command.to_owned(),
                                args: entry.args,
                            }
                        }
                        (None, Some(url)) if !url.trim().is_empty() => McpTransport::Http {
                            url: url.to_owned(),
                            header: entry.header,
                        },
                        (Some(_), Some(_)) => {
                            return Err(refuse(format!(
                                "{}: a server is reached by a command or by a url, not both",
                                label.as_str()
                            )));
                        }
                        _ => {
                            return Err(refuse(format!(
                                "{}: an mcp server needs a command to start or a url to reach",
                                label.as_str()
                            )));
                        }
                    };
                    // Two servers under one label would put one tool name
                    // in front of two processes, and that is a routing
                    // mistake rather than a preference.
                    if servers.iter().any(|held| held.label == label) {
                        return Err(refuse(format!(
                            "{}: two servers are named the same in one layer",
                            label.as_str()
                        )));
                    }
                    servers.push(McpServer { label, transport });
                }
                Some(servers)
            }
        };
        Ok(ConfigLayer {
            effort: file.model.effort,
            sandbox,
            mcp,
        })
    }

    #[must_use]
    pub fn effort(&self) -> Option<Effort> {
        self.effort
    }

    #[must_use]
    pub fn sandbox(&self) -> Option<&SandboxLimits> {
        self.sandbox.as_ref()
    }

    #[must_use]
    pub fn mcp(&self) -> Option<&[McpServer]> {
        self.mcp.as_deref()
    }
}

/// Resolves the three layers into the snapshot a run is frozen with.
///
/// # Errors
/// Refuses an address with no building, an unreadable file, and a file
/// that does not parse. A missing file is not an error: it is the
/// ordinary case.
pub fn load(city_root: &Path, addr: &Address) -> Result<FrozenConfig, AxError> {
    let city = read_layer(&path(city_root, addr, Layer::City)?)?;
    let building_path = path(city_root, addr, Layer::Building)?;
    let building = read_layer(&building_path)?;
    // An address that *is* its building has two rungs, not three: the
    // same file counted twice would suggest it can override itself.
    let resident_path = path(city_root, addr, Layer::Resident)?;
    let resident = if resident_path == building_path {
        ConfigLayer::default()
    } else {
        read_layer(&resident_path)?
    };

    Ok(kernel::freeze(
        &LayeredValue::default(),
        &LayeredValue::default(),
        &LayeredValue {
            city: city.effort(),
            building: building.effort(),
            resident: resident.effort(),
        },
        &LayeredValue {
            city: city.sandbox().cloned(),
            building: building.sandbox().cloned(),
            resident: resident.sandbox().cloned(),
        },
        &LayeredValue {
            city: city.mcp().map(<[McpServer]>::to_vec),
            building: building.mcp().map(<[McpServer]>::to_vec),
            resident: resident.mcp().map(<[McpServer]>::to_vec),
        },
    ))
}

fn read_layer(path: &Path) -> Result<ConfigLayer, AxError> {
    match std::fs::read_to_string(path) {
        // Three files can fail; the refusal says which one did.
        Ok(text) => ConfigLayer::parse(&text)
            .map_err(|err| refuse(format!("{}: {}", path.display(), err.subject()))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ConfigLayer::default()),
        Err(err) => Err(AxError::failure(
            AxCode::StorageFatal,
            "read a configuration layer",
            format!("{}: {err}", path.display()),
        )
        .with_recovery("fix the file's permissions; a configuration that exists is read")),
    }
}

/// One refusal shape for every way a layer can fail to be read, so the
/// recovery line is written once and cannot drift between callers.
fn refuse(subject: String) -> AxError {
    AxError::failure(AxCode::ConfigInvalid, "read a configuration layer", subject).with_recovery(
        "this version reads three sections: `[model] effort = \"low|medium|high|xhigh|max\"`, \
         `[sandbox] shell = <bool>, fuel = <integer>, mounts = [<path>]`, and \
         `[[mcp]] label = <lowercase>, and either command = <program> with args = [<argument>]          or url = <https url> with an optional header = \"Name: value\"`",
    )
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    #[serde(default)]
    model: ModelSection,
    #[serde(default)]
    sandbox: Option<SandboxSection>,
    #[serde(default)]
    mcp: Option<Vec<McpSection>>,
}

/// One `[[mcp]]` entry, read as written rather than as parsed types:
/// the label's grammar is `kernel`'s answer, and a deserializer that
/// enforced it here would be the second place that rule lives.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct McpSection {
    label: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    header: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxSection {
    #[serde(default)]
    shell: bool,
    #[serde(default)]
    fuel: Option<u64>,
    #[serde(default)]
    mounts: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelSection {
    #[serde(default)]
    effort: Option<Effort>,
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

    fn addr(raw: &str) -> Address {
        Address::parse(raw).unwrap()
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_city_with_no_configuration_at_all_runs_on_the_policy_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let frozen = load(dir.path(), &addr("lab/room1")).unwrap();
        assert_eq!(
            frozen.effort, None,
            "an unstated effort stays unstated: the provider's default is not ours to name"
        );
    }

    #[test]
    fn the_lower_layer_wins_and_the_upper_one_is_what_it_falls_back_to() {
        let dir = tempfile::tempdir().unwrap();
        let room = addr("lab/room1");
        write(
            &path(dir.path(), &room, Layer::City).unwrap(),
            "[model]\neffort = \"low\"\n",
        );
        assert_eq!(load(dir.path(), &room).unwrap().effort, Some(Effort::Low));

        write(
            &path(dir.path(), &room, Layer::Building).unwrap(),
            "[model]\neffort = \"high\"\n",
        );
        assert_eq!(load(dir.path(), &room).unwrap().effort, Some(Effort::High));

        write(
            &path(dir.path(), &room, Layer::Resident).unwrap(),
            "[model]\neffort = \"max\"\n",
        );
        assert_eq!(load(dir.path(), &room).unwrap().effort, Some(Effort::Max));

        // A room next door reads the building's value, not its neighbour's.
        assert_eq!(
            load(dir.path(), &addr("lab/room2")).unwrap().effort,
            Some(Effort::High)
        );
    }

    #[test]
    fn the_citys_own_layer_lives_where_no_write_domain_reaches() {
        let dir = tempfile::tempdir().unwrap();
        let city = path(dir.path(), &addr("lab"), Layer::City).unwrap();
        assert!(city.starts_with(dir.path().join(RESERVED_PREFIX)));
        assert!(city.ends_with(CONFIG_FILE));
        assert!(Address::parse(RESERVED_PREFIX).unwrap().is_reserved());
    }

    #[test]
    fn an_address_that_is_its_own_building_has_two_layers_not_three() {
        let dir = tempfile::tempdir().unwrap();
        let lab = addr("lab");
        assert_eq!(
            path(dir.path(), &lab, Layer::Building).unwrap(),
            path(dir.path(), &lab, Layer::Resident).unwrap()
        );
        write(
            &path(dir.path(), &lab, Layer::Building).unwrap(),
            "[model]\neffort = \"medium\"\n",
        );
        assert_eq!(load(dir.path(), &lab).unwrap().effort, Some(Effort::Medium));
    }

    #[test]
    fn a_key_this_version_does_not_read_is_refused_rather_than_ignored() {
        let err = ConfigLayer::parse("[model]\nefort = \"high\"\n").unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(err.recovery().contains("[model]"));

        let err = ConfigLayer::parse("[clock]\nstamp = \"minute\"\n").unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
    }

    #[test]
    fn an_effort_level_no_provider_offers_is_refused_at_the_file() {
        let err = ConfigLayer::parse("[model]\neffort = \"maximum\"\n").unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert_eq!(
            ConfigLayer::parse("[model]\neffort = \"xhigh\"\n")
                .unwrap()
                .effort(),
            Some(Effort::XHigh)
        );
    }

    #[test]
    fn a_broken_file_names_itself_in_the_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let room = addr("lab/room1");
        write(
            &path(dir.path(), &room, Layer::Building).unwrap(),
            "[model\neffort = \"high\"\n",
        );
        let err = load(dir.path(), &room).unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(
            err.subject().contains("lab"),
            "three files can fail; the refusal says which one did: {}",
            err.subject()
        );
    }

    #[test]
    fn a_building_states_which_servers_it_reaches_and_replaces_the_citys_table() {
        let dir = tempfile::tempdir().unwrap();
        let room = addr("lab/room1");
        write(
            &path(dir.path(), &room, Layer::City).unwrap(),
            "[[mcp]]\nlabel = \"apps\"\ncommand = \"mcp-apps\"\nargs = [\"--stdio\"]\n\n\
             [[mcp]]\nlabel = \"mail\"\ncommand = \"mcp-mail\"\n",
        );
        let frozen = load(dir.path(), &room).unwrap();
        assert_eq!(frozen.mcp.len(), 2);
        assert_eq!(frozen.mcp[0].label.as_str(), "apps");
        assert_eq!(
            frozen.mcp[0].transport,
            McpTransport::Stdio {
                command: "mcp-apps".to_owned(),
                args: vec!["--stdio".to_owned()],
            }
        );

        write(
            &path(dir.path(), &room, Layer::Building).unwrap(),
            "[[mcp]]\nlabel = \"mail\"\ncommand = \"mcp-mail\"\n",
        );
        let frozen = load(dir.path(), &room).unwrap();
        assert_eq!(
            frozen.mcp.len(),
            1,
            "a layer that speaks about servers speaks about all of them"
        );
        assert_eq!(frozen.mcp[0].label.as_str(), "mail");
    }

    #[test]
    fn a_server_table_that_could_not_route_a_call_is_refused_at_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let room = addr("lab/room1");
        write(
            &path(dir.path(), &room, Layer::Building).unwrap(),
            "[[mcp]]\nlabel = \"Apps\"\ncommand = \"mcp-apps\"\n",
        );
        let err = load(dir.path(), &room).unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(
            err.subject().contains("lab"),
            "the refusal says which file: {}",
            err.subject()
        );

        let err = ConfigLayer::parse(
            "[[mcp]]\nlabel = \"apps\"\ncommand = \"one\"\n\n\
             [[mcp]]\nlabel = \"apps\"\ncommand = \"two\"\n",
        )
        .unwrap_err();
        assert!(
            err.subject().contains("named the same"),
            "{}",
            err.subject()
        );

        let err = ConfigLayer::parse("[[mcp]]\nlabel = \"apps\"\ncommand = \"\"\n").unwrap_err();
        assert!(err.subject().contains("command"), "{}", err.subject());

        let err =
            ConfigLayer::parse("[[mcp]]\nlabel = \"apps\"\nprogram = \"mcp-apps\"\n").unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(err.recovery().contains("[[mcp]]"));
    }

    #[test]
    fn the_reserved_subtree_has_no_configuration_layers() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(dir.path(), &addr(".sprawling/ledger")).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
    }
}
