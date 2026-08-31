// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a building's runs may reach, and the one form that sets it.
//!
//! Two configuration sections resolve city → building → room and had no
//! surface at all: a person could be governed by them and not change
//! them. This is that surface, and it is a separate module from the
//! building page on purpose — that page states it reads and does not
//! edit, because editing a document an agent writes would be a second
//! way to change a building that leaves no run and no ledger line.
//!
//! **Configuration is not that case.** `CONFIG.toml` lives in the
//! reserved subtree, which no write domain reaches, so no run can write
//! it and a person's form is not a second way to do anything — it is the
//! only way. What is written here governs the next run; what a run does
//! with it is what the ledger keeps.
//!
//! The two fields a person edits are text, one entry per line, because
//! the alternative is a widget per transport and a form that grows a
//! branch every time the upstream enum does. Parsing is a pure function
//! here, and an unparseable line is reported rather than dropped.

use channels::{
    Address, ClientFrame, IdemKey, McpServer, McpTransport, RunId, SandboxLimits, Seq, ServerLabel,
};
use dioxus::prelude::*;

use crate::lang::{Msg, say};

/// Reads the mount list: one address per line, blank lines ignored.
///
/// The unreadable lines come back beside the good ones rather than being
/// dropped: a form that silently discarded a line would write a
/// configuration the person did not mean and could not see they had not
/// meant.
#[must_use]
pub fn read_mounts(text: &str) -> (Vec<Address>, Vec<String>) {
    let mut mounts = Vec::new();
    let mut unreadable = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match Address::parse(trimmed) {
            Ok(addr) => mounts.push(addr),
            Err(_) => unreadable.push(trimmed.to_owned()),
        }
    }
    (mounts, unreadable)
}

/// Reads the server list.
///
/// One per line: `label url` for a hosted server, `label ! command args`
/// for a program on this machine. The separator is a bare `!` because a
/// URL cannot contain one at that position and a command can begin with
/// anything — so which transport a line names is decided by the line's
/// shape rather than guessed from its content, which is the same reason
/// `McpTransport` is an enum and not a string that might be either.
#[must_use]
pub fn read_servers(text: &str) -> (Vec<McpServer>, Vec<String>) {
    let mut servers = Vec::new();
    let mut unreadable = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((label, rest)) = trimmed.split_once(char::is_whitespace) else {
            unreadable.push(trimmed.to_owned());
            continue;
        };
        let Ok(label) = ServerLabel::parse(label.trim()) else {
            unreadable.push(trimmed.to_owned());
            continue;
        };
        let rest = rest.trim();
        let transport = match rest.strip_prefix('!') {
            Some(command) => {
                let mut words = command.split_whitespace();
                match words.next() {
                    Some(program) => McpTransport::Stdio {
                        command: program.to_owned(),
                        args: words.map(str::to_owned).collect(),
                    },
                    None => {
                        unreadable.push(trimmed.to_owned());
                        continue;
                    }
                }
            }
            None if rest.is_empty() => {
                unreadable.push(trimmed.to_owned());
                continue;
            }
            None => McpTransport::Http {
                url: rest.to_owned(),
                header: None,
            },
        };
        servers.push(McpServer { label, transport });
    }
    (servers, unreadable)
}

/// Writes the server list back as the form shows it. The inverse of
/// [`read_servers`], in the same file, so the two cannot drift.
#[must_use]
pub fn show_servers(servers: &[McpServer]) -> String {
    servers
        .iter()
        .map(|server| match &server.transport {
            McpTransport::Http { url, .. } => format!("{} {url}", server.label.as_str()),
            McpTransport::Stdio { command, args } => {
                let mut line = format!("{} ! {command}", server.label.as_str());
                for arg in args {
                    line.push(' ');
                    line.push_str(arg);
                }
                line
            }
            // A transport this build cannot spell is shown by its label
            // alone rather than as a line that would read back as
            // something else.
            _ => server.label.as_str().to_owned(),
        })
        .collect::<Vec<String>>()
        .join("\n")
}

#[must_use]
pub fn show_mounts(mounts: &[Address]) -> String {
    mounts
        .iter()
        .map(|addr| addr.as_str().to_owned())
        .collect::<Vec<String>>()
        .join("\n")
}

/// The command a filled form asks for.
///
/// The key is derived from the building and what is being set, so
/// pressing save twice writes once.
#[must_use]
pub fn configure_command(
    addr: &Address,
    sandbox: SandboxLimits,
    servers: Vec<McpServer>,
) -> channels::WireCommand {
    channels::WireCommand::ConfigureBuilding {
        idem: IdemKey::derive(
            &RunId::CITY,
            Seq::FIRST,
            format!(
                "reach:{}:{}:{}:{}",
                addr.as_str(),
                sandbox.shell,
                sandbox.fuel,
                servers.len()
            )
            .as_bytes(),
        ),
        addr: addr.clone(),
        sandbox: Some(sandbox),
        mcp: Some(servers),
    }
}

/// The form. It decides nothing that is not decided above: every
/// judgement it makes is one of the pure functions in this file.
#[component]
pub fn ReachForm(
    addr: Address,
    /// The building's own rung of the ladder, not the resolved value: a
    /// form filled from the resolved value would write the city's
    /// setting into this building the first time anybody saved.
    sandbox: Option<SandboxLimits>,
    servers: Vec<McpServer>,
    on_frame: EventHandler<ClientFrame>,
) -> Element {
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let held = sandbox.clone().unwrap_or_default();
    let mut shell = use_signal(|| held.shell);
    let mut fuel = use_signal(|| held.fuel.to_string());
    let mut mounts = use_signal(|| show_mounts(&held.mounts));
    let mut wired = use_signal(|| show_servers(&servers));
    let of = addr.clone();
    rsx! {
        form { class: "reach",
            onsubmit: move |event| {
                event.prevent_default();
                let (mounts, _) = read_mounts(&mounts.read());
                let (servers, _) = read_servers(&wired.read());
                let limits = SandboxLimits {
                    shell: shell(),
                    // An unreadable number leaves the budget where it
                    // was rather than dropping it to zero, which would
                    // be a sandbox that refuses every call.
                    fuel: fuel.read().trim().parse().unwrap_or(held.fuel),
                    mounts,
                };
                on_frame.call(ClientFrame::Command(Box::new(
                    configure_command(&of, limits, servers),
                )));
            },
            label { class: "field",
                input {
                    r#type: "checkbox",
                    name: "shell",
                    checked: shell(),
                    onchange: move |event| shell.set(event.checked()),
                }
                span { "{word(Msg::BuildingShell)}" }
            }
            div { class: "field",
                label { r#for: "reach-fuel", "{word(Msg::BuildingFuel)}" }
                input {
                    id: "reach-fuel",
                    name: "fuel",
                    value: "{fuel}",
                    oninput: move |event| fuel.set(event.value()),
                }
            }
            div { class: "field wide",
                label { r#for: "reach-mounts", "{word(Msg::BuildingMounts)}" }
                textarea {
                    id: "reach-mounts",
                    name: "mounts",
                    value: "{mounts}",
                    oninput: move |event| mounts.set(event.value()),
                }
                for line in read_mounts(&mounts.read()).1 {
                    span { key: "{line}", class: "hint blocking", "{line}" }
                }
            }
            div { class: "field wide",
                label { r#for: "reach-servers", "{word(Msg::BuildingServers)}" }
                textarea {
                    id: "reach-servers",
                    name: "servers",
                    value: "{wired}",
                    oninput: move |event| wired.set(event.value()),
                }
                span { class: "hint", "{word(Msg::BuildingServersHint)}" }
                for line in read_servers(&wired.read()).1 {
                    span { key: "{line}", class: "hint blocking", "{line}" }
                }
            }
            div { class: "field wide submit",
                button { r#type: "submit", "{word(Msg::BuildingSaveReach)}" }
            }
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

    #[test]
    fn a_line_says_which_transport_it_names_by_its_shape() {
        let (servers, bad) =
            read_servers("docs https://mcp.example/v1\nlocal ! node server.js x\n");
        assert!(bad.is_empty());
        assert_eq!(servers.len(), 2);
        assert_eq!(
            servers[0].transport,
            McpTransport::Http {
                url: "https://mcp.example/v1".to_owned(),
                header: None,
            }
        );
        assert_eq!(
            servers[1].transport,
            McpTransport::Stdio {
                command: "node".to_owned(),
                args: vec!["server.js".to_owned(), "x".to_owned()],
            }
        );
    }

    /// A dropped line would be a configuration the person did not write
    /// and could not see they had not written.
    #[test]
    fn a_line_this_build_cannot_read_is_reported_rather_than_dropped() {
        let (servers, bad) = read_servers("docs\nDOCS https://x.example\ngood https://y.example\n");
        assert_eq!(servers.len(), 1);
        assert_eq!(
            bad,
            vec!["docs".to_owned(), "DOCS https://x.example".to_owned()]
        );

        let (mounts, bad) = read_mounts("shared/notes\n../escape\n");
        assert_eq!(mounts.len(), 1);
        assert_eq!(bad, vec!["../escape".to_owned()]);
    }

    #[test]
    fn what_is_shown_reads_back_as_what_was_shown() {
        let (servers, _) = read_servers("docs https://mcp.example/v1\nlocal ! node server.js\n");
        let (again, bad) = read_servers(&show_servers(&servers));
        assert!(bad.is_empty());
        assert_eq!(again, servers);
    }

    #[test]
    fn saving_twice_configures_once() {
        let addr = Address::parse("lab").unwrap();
        let limits = SandboxLimits {
            shell: true,
            fuel: 4096,
            mounts: Vec::new(),
        };
        let first = configure_command(&addr, limits.clone(), Vec::new());
        let second = configure_command(&addr, limits, Vec::new());
        assert_eq!(first.idem(), second.idem());
    }
}
