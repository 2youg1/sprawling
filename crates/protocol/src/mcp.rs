// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reaching a Model Context Protocol server.
//!
//! The city does not know any particular server. Which one a building
//! talks to is that building's configuration; what this module knows is
//! the protocol, and the protocol is the same whether the far end is a
//! hosted catalogue of a thousand applications or a script somebody
//! wrote this morning.
//!
//! Two things about the current revision shape the code. The list of
//! tools may not vary per connection, which is the same rule as freezing
//! a run's tool table — the two arrived from opposite directions and
//! agree, so the tool table is read once and frozen with the run. And
//! there is no protocol-level session, so a server that needs state
//! across calls mints its own handle and passes it as an ordinary
//! argument, which means this module has no session to keep.
//!
//! Everything a server returns is other people's text. It lands on the
//! same tool seam as the local tools, so it enters the taint ring the
//! same way, and there is no unwrapping face here.

use kernel::{
    AxCode, AxError, CostTier, Effect, Payload, RenderIntent, ServerLabel, Temporal, TimeoutMs,
    Tool, ToolCall, ToolMeta, ToolName, ToolOutcome,
};
use serde_json::{Map, Value, json};

/// How long an external call may take before the transport gives up on
/// it. One number, written where the tool is registered, so what the
/// model is promised and what the transport enforces cannot drift.
///
/// The handshake that precedes any tool uses it too: waiting for a
/// server to say what it offers is the same kind of wait as waiting for
/// it to do something.
pub const EXTERNAL_CALL_PATIENCE: TimeoutMs = TimeoutMs(60_000);

/// The wire this module speaks over. Adapters: a stdio subprocess in the
/// binary, and [`ScriptedOutbound`] for replay.
///
/// One method, and it is synchronous, because a tool call is a question
/// with an answer. Where the bytes go and how long they take belongs to
/// the adapter.
pub trait Outbound {
    /// Sends one JSON-RPC line and returns the line that answered it,
    /// giving up after `patience`.
    ///
    /// The deadline is an argument rather than a property of the
    /// transport because it is the tool's declared timeout: a
    /// `TimeoutMs` in a registration promises the call can be given up
    /// on, and a promise nobody executes is decoration.
    ///
    /// # Errors
    /// Transport failures and the deadline. A server's own refusal comes
    /// back as a JSON-RPC error inside a successful exchange.
    fn call(&mut self, line: &str, patience: TimeoutMs) -> Result<String, AxError>;
}

/// Builds request lines and reads answer lines. Holds an id counter and
/// nothing else.
#[derive(Debug, Default)]
pub struct Rpc {
    next: u64,
}

impl Rpc {
    #[must_use]
    pub fn new() -> Rpc {
        Rpc { next: 0 }
    }

    fn mint(&mut self) -> u64 {
        self.next = self.next.saturating_add(1);
        self.next
    }

    /// One JSON-RPC request, on one line. The transport is line
    /// delimited and a message may not contain a newline, so the line is
    /// assembled here rather than left to a pretty printer.
    fn request(&mut self, method: &str, params: Value) -> String {
        let id = self.mint();
        format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":{},\"params\":{}}}",
            Value::String(method.to_owned()),
            params
        )
    }

    /// `server/discover`: which protocol versions and capabilities the
    /// far end has. Asked first, so a version this city cannot speak is
    /// a refusal before any tool is offered to a model.
    pub fn discover(&mut self) -> String {
        self.request("server/discover", json!({}))
    }

    /// `tools/list`.
    pub fn list_tools(&mut self) -> String {
        self.request("tools/list", json!({}))
    }

    /// `tools/call`.
    ///
    /// # Errors
    /// Refuses arguments carrying a floating point number. Every call is
    /// written to the ledger, ledger payloads carry no floats, and a
    /// call that cannot be recorded is a call the city cannot replay.
    /// The refusal is scoped to the call rather than the tool: one
    /// badly-shaped argument is not evidence that the tool is unusable.
    pub fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<String, AxError> {
        if let Some(path) = float_at(arguments, String::new()) {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "call an external tool",
                format!("a fractional number at `{path}` cannot be recorded"),
            )
            .with_recovery(
                "send the value as a string or an integer; every call is written to the ledger, \
                 and the ledger holds no floats",
            ));
        }
        Ok(self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        ))
    }

    /// Reads one answer line.
    ///
    /// # Errors
    /// Refuses a line that is not a JSON-RPC answer, and turns a
    /// server's error object into a refusal that keeps the server's own
    /// words.
    pub fn read(line: &str) -> Result<Value, AxError> {
        let value: Value = serde_json::from_str(line).map_err(|err| {
            AxError::failure(AxCode::WireMismatch, "read an mcp answer", err.to_string())
                .with_recovery("check the server's protocol revision")
        })?;
        let object = value.as_object().ok_or_else(|| {
            AxError::failure(AxCode::WireMismatch, "read an mcp answer", "not an object")
                .with_recovery("check the server's protocol revision")
        })?;
        if let Some(error) = object.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("no message");
            return Err(AxError::failure(
                AxCode::ToolUnavailable,
                "call an external tool",
                format!("{code}: {message}"),
            )
            .with_recovery("the server refused; read its message before trying another shape"));
        }
        object.get("result").cloned().ok_or_else(|| {
            AxError::failure(
                AxCode::WireMismatch,
                "read an mcp answer",
                "neither result nor error",
            )
            .with_recovery("check the server's protocol revision")
        })
    }
}

/// Where a fractional number sits, if one does. Returns the path so the
/// refusal can name it rather than saying "somewhere in your arguments".
fn float_at(value: &Value, path: String) -> Option<String> {
    match value {
        Value::Number(number) => {
            if number.is_f64() && number.as_i64().is_none() && number.as_u64().is_none() {
                Some(if path.is_empty() {
                    "(the argument itself)".to_owned()
                } else {
                    path
                })
            } else {
                None
            }
        }
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(index, item)| float_at(item, format!("{path}[{index}]"))),
        Value::Object(map) => map.iter().find_map(|(key, item)| {
            let next = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            float_at(item, next)
        }),
        Value::Null | Value::Bool(_) | Value::String(_) => None,
    }
}

/// One tool a server offers, under both of its names.
///
/// The two travel together because the function that derives one from
/// the other is the only place that knows both: a caller that had to
/// pair a list of registrations with a list of remote names by position
/// would be one reordering away from calling the wrong tool.
pub struct Listed {
    /// The name the server knows it by, which is what goes back out.
    pub remote: String,
    /// The name this city knows it by, which is what the model sees.
    pub meta: ToolMeta,
}

/// Turns a `tools/list` result into the registrations a catalog admits.
///
/// # Errors
/// Refuses a result whose shape this version does not read, and a tool
/// whose name cannot be spelled after prefixing.
pub fn tools_from(server: &ServerLabel, result: &Value) -> Result<Vec<Listed>, AxError> {
    let listed = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AxError::failure(
                AxCode::WireMismatch,
                "read an mcp tool list",
                "no `tools` array",
            )
            .with_recovery("check the server's protocol revision")
        })?;
    let mut out = Vec::new();
    for entry in listed {
        let raw = entry.get("name").and_then(Value::as_str).ok_or_else(|| {
            AxError::failure(
                AxCode::WireMismatch,
                "read an mcp tool list",
                "a tool without a name",
            )
            .with_recovery("check the server's protocol revision")
        })?;
        let name = ToolName::parse(&format!("{}_{}", server.as_str(), sanitise(raw)))?;
        let disclosure = entry
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("an external tool this server offers")
            .to_owned();
        let params = entry
            .get("inputSchema")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        out.push(Listed {
            remote: raw.to_owned(),
            meta: ToolMeta {
                name,
                disclosure,
                params: Payload::new(params)?,
                // Every external call leaves this process. Naming the
                // connector is what routes it to the gate that can say
                // no, and what tells that gate where it goes: the
                // destination is this server, on every call, so there is
                // nothing here for a model to fill in.
                effect: Effect::Connector {
                    label: server.clone(),
                },
                cost_tier: CostTier::Heavy,
                timeout: Some(EXTERNAL_CALL_PATIENCE),
                render: RenderIntent::Generic,
                // The far end is a live service, so the answer is about
                // now.
                temporal: Temporal::Timestamped,
            },
        });
    }
    Ok(out)
}

fn sanitise(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// A tool a server offers, ready to be registered on the tool seam.
pub struct McpTool {
    meta: ToolMeta,
    remote: String,
    patience: TimeoutMs,
    rpc: Rpc,
    outbound: Box<dyn Outbound>,
}

impl McpTool {
    /// # Errors
    /// Refuses to exist inside a confidential building. That mark means
    /// what happens here leaves no trace outside the run, and an
    /// outbound call is the largest trace there is — so the refusal is
    /// at construction, where there is nothing yet to leak.
    ///
    /// Refuses a registration that declares no timeout: an outbound tool
    /// with no deadline is a tool that can hang a run, and the run has
    /// no second way to notice.
    pub fn new(
        meta: ToolMeta,
        remote: String,
        outbound: Box<dyn Outbound>,
        confidential: bool,
    ) -> Result<McpTool, AxError> {
        if confidential {
            return Err(AxError::failure(
                AxCode::GateDenied,
                "offer an external tool",
                format!("{remote}: this building is confidential"),
            )
            .with_recovery("do this work in a building that is allowed to reach the network"));
        }
        let patience = meta.timeout.ok_or_else(|| {
            AxError::failure(
                AxCode::InvalidArgs,
                "offer an external tool",
                format!("{remote}: no timeout declared"),
            )
            .with_recovery("register the tool with a timeout; an outbound call needs a deadline")
        })?;
        Ok(McpTool {
            meta,
            remote,
            patience,
            rpc: Rpc::new(),
            outbound,
        })
    }

    /// The server-side name, which is what goes back out on the wire.
    #[must_use]
    pub fn remote(&self) -> &str {
        &self.remote
    }
}

impl Tool for McpTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "call an external tool",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let arguments = Value::Object(call.args.as_map().clone());
        let line = self.rpc.call_tool(&self.remote, &arguments)?;
        let answer = self.outbound.call(&line, self.patience)?;
        let result = Rpc::read(&answer)?;
        let map = match result {
            Value::Object(map) => map,
            other => {
                let mut wrapped = Map::new();
                wrapped.insert("result".to_owned(), other);
                wrapped
            }
        };
        Ok(ToolOutcome {
            result: Payload::new(map)?,
        })
    }
}

/// The second adapter: answers somebody already collected.
///
/// Keyed by the request's method and params rather than by arrival
/// order, so a run that legitimately reorders two independent calls
/// still replays.
#[derive(Debug, Default)]
pub struct ScriptedOutbound {
    answers: std::collections::BTreeMap<String, String>,
    missed: Vec<String>,
}

impl ScriptedOutbound {
    #[must_use]
    pub fn new() -> ScriptedOutbound {
        ScriptedOutbound::default()
    }

    /// Records the answer for a request line, ignoring its id.
    ///
    /// # Errors
    /// Refuses a request line it cannot read, since a key derived from
    /// an unreadable line would never match anything.
    pub fn answer(&mut self, request: &str, answer: &str) -> Result<(), AxError> {
        self.answers.insert(key_of(request)?, answer.to_owned());
        Ok(())
    }

    #[must_use]
    pub fn missed(&self) -> &[String] {
        &self.missed
    }
}

fn key_of(request: &str) -> Result<String, AxError> {
    let value: Value = serde_json::from_str(request).map_err(|err| {
        AxError::failure(AxCode::WireMismatch, "key an mcp request", err.to_string())
            .with_recovery("record a well-formed JSON-RPC line")
    })?;
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    Ok(format!("{method} {params}"))
}

impl Outbound for ScriptedOutbound {
    // A recorded answer is instant, so the deadline has nothing to bound
    // here; it stays in the signature because the seam, not the adapter,
    // is what the tool's declared timeout travels through.
    fn call(&mut self, line: &str, _patience: TimeoutMs) -> Result<String, AxError> {
        let key = key_of(line)?;
        match self.answers.get(&key) {
            Some(answer) => Ok(answer.clone()),
            None => {
                self.missed.push(key.clone());
                Err(
                    AxError::failure(AxCode::ToolUnavailable, "replay an mcp call", key)
                        .with_recovery("record this call against the real server before replaying"),
                )
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

    fn label() -> ServerLabel {
        ServerLabel::parse("apps").unwrap()
    }

    fn listing() -> Value {
        json!({ "tools": [
            { "name": "GITHUB_CREATE_ISSUE", "description": "open an issue",
              "inputSchema": { "type": "object" } },
            { "name": "gmail.send", "description": "send mail" },
        ] })
    }

    #[test]
    fn a_request_is_one_line_with_no_newline_inside_it() {
        let mut rpc = Rpc::new();
        let line = rpc.list_tools();
        assert!(!line.contains('\n'));
        assert!(line.starts_with("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\""));
        assert!(rpc.discover().contains("\"id\":2"), "ids are never reused");
    }

    #[test]
    fn two_servers_offering_the_same_verb_stay_two_tools() {
        let one = tools_from(&label(), &listing()).unwrap();
        let other = ServerLabel::parse("desk").unwrap();
        let two = tools_from(&other, &listing()).unwrap();
        assert_eq!(one[0].meta.name.as_str(), "apps_github_create_issue");
        assert_eq!(two[0].meta.name.as_str(), "desk_github_create_issue");
        assert_ne!(one[0].meta.name, two[0].meta.name);
    }

    #[test]
    fn an_external_tool_declares_itself_as_leaving_the_machine() {
        let tools = tools_from(&label(), &listing()).unwrap();
        assert_eq!(tools[0].meta.effect, Effect::Connector { label: label() });
        assert_eq!(tools[0].meta.temporal, Temporal::Timestamped);
        assert_eq!(tools[1].meta.name.as_str(), "apps_gmail_send");
    }

    #[test]
    fn a_fractional_argument_is_refused_by_position_and_only_that_call() {
        let mut rpc = Rpc::new();
        let err = rpc
            .call_tool("x", &json!({ "amount": 1.5, "note": "ok" }))
            .unwrap_err();
        assert!(err.subject().contains("amount"), "{}", err.subject());
        assert!(err.recovery().contains("ledger"));
        // The tool is still usable with a shape that can be recorded.
        assert!(rpc.call_tool("x", &json!({ "amount": 2 })).is_ok());
        let nested = rpc
            .call_tool("x", &json!({ "a": { "b": [1, 2.25] } }))
            .unwrap_err();
        assert!(nested.subject().contains("a.b[1]"), "{}", nested.subject());
    }

    #[test]
    fn a_servers_refusal_keeps_the_servers_own_words() {
        let err = Rpc::read(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32602,\"message\":\"no such repo\"}}",
        )
        .unwrap_err();
        assert!(err.subject().contains("no such repo"));
        assert_eq!(err.code(), &AxCode::ToolUnavailable);
    }

    #[test]
    fn an_answer_this_version_cannot_read_is_refused_rather_than_guessed() {
        for line in ["not json", "[]", "{\"jsonrpc\":\"2.0\",\"id\":1}"] {
            assert!(Rpc::read(line).is_err(), "{line}");
        }
    }

    #[test]
    fn a_confidential_building_cannot_hold_an_outbound_tool_at_all() {
        let listed = tools_from(&label(), &listing()).unwrap().remove(0);
        let meta = listed.meta;
        let err = McpTool::new(
            meta,
            "GITHUB_CREATE_ISSUE".to_owned(),
            Box::new(ScriptedOutbound::new()),
            true,
        )
        .err()
        .expect("a confidential building refuses an outbound tool");
        assert_eq!(err.code(), &AxCode::GateDenied);
        assert!(err.recovery().contains("reach the network"));
    }

    #[test]
    fn a_recorded_call_replays_to_the_same_answer() {
        let mut rpc = Rpc::new();
        let request = rpc
            .call_tool("GITHUB_CREATE_ISSUE", &json!({ "title": "kiln" }))
            .unwrap();
        let mut scripted = ScriptedOutbound::new();
        scripted
            .answer(
                &request,
                "{\"jsonrpc\":\"2.0\",\"id\":9,\"result\":{\"number\":41}}",
            )
            .unwrap();

        let listed = tools_from(&label(), &listing()).unwrap().remove(0);
        let meta = listed.meta;
        let name = meta.name.clone();
        let mut tool = McpTool::new(
            meta,
            "GITHUB_CREATE_ISSUE".to_owned(),
            Box::new(scripted),
            false,
        )
        .map_err(|err| format!("{err}"))
        .unwrap();
        let outcome = tool
            .invoke(&ToolCall {
                id: "tu_1".to_owned(),
                name,
                args: Payload::new(json!({ "title": "kiln" }).as_object().unwrap().clone())
                    .unwrap(),
            })
            .unwrap();
        assert_eq!(
            outcome
                .result
                .as_map()
                .get("number")
                .and_then(Value::as_u64),
            Some(41)
        );
    }

    #[test]
    fn an_outbound_tool_without_a_deadline_is_refused_at_construction() {
        let mut meta = tools_from(&label(), &listing()).unwrap().remove(0).meta;
        assert_eq!(meta.timeout, Some(EXTERNAL_CALL_PATIENCE));
        meta.timeout = None;
        let err = McpTool::new(
            meta,
            "GITHUB_CREATE_ISSUE".to_owned(),
            Box::new(ScriptedOutbound::new()),
            false,
        )
        .err()
        .expect("a tool that cannot be given up on is not offered");
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(err.recovery().contains("deadline"));
    }
}
