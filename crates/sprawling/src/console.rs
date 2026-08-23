// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What a line typed into a serving city means (sprawling-SPEC.md
//! section 8-11).
//!
//! `sprawling up` used to print four lines and block until Ctrl-C. That
//! terminal is a surface the product threw away, and on a machine with
//! no browser it is the only surface there is.
//!
//! Everything here is a pure judgement over one line of text. What the
//! judgement produces is either a control action the terminal carries
//! out or a `ClientFrame` that goes to the same desk a browser's frames
//! go to, so the console decides nothing the server does not.

use kernel::Address;
use std::io::{BufRead, Write};
use std::sync::Arc;

/// What a console needs from the process that started it.
///
/// The pairing token arrives as a copy of the one `serve` already read,
/// so `/web` can carry it and nobody has to transcribe a secret. It is
/// not re-read from the environment here: one read, one authority.
pub(crate) struct Terminal {
    pub(crate) url: String,
    pub(crate) token: Option<String>,
}

/// The console's own verbs, which are not on the wire.
///
/// Exhaustive, and checked against the wire's vocabulary so a name can
/// never mean two things.
const CONTROL: [&str; 4] = ["help", "web", "at", "quit"];

/// What one line asked for.
#[derive(Debug, PartialEq)]
pub(crate) enum Line {
    /// An empty line, which is not a question.
    Nothing,
    Help,
    OpenWeb,
    Select(Address),
    Quit,
    /// A wire verb, already built into the frame it names.
    Frame(Box<channels::ClientFrame>),
    /// Anything that does not begin with `/`: work for the selected
    /// room, in the words the person used.
    Work(String),
    /// A verb this console does not have, with the nearest ones it does.
    Unknown {
        verb: String,
        nearest: Vec<String>,
    },
}

/// `AttachEndpoint` becomes `attach_endpoint`.
///
/// The wire names itself in the shape Rust variants take; a terminal is
/// typed in lower case. One conversion, so the two spellings cannot
/// become two lists.
pub(crate) fn snake(camel: &str) -> String {
    let mut out = String::with_capacity(camel.len().saturating_add(4));
    for (index, ch) in camel.char_indices() {
        if ch.is_ascii_uppercase() && index > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

/// Every verb this console answers to.
///
/// **A projection, never a second list.** The wire half is derived from
/// `channels::COMMAND_NAMES` and `channels::QUERY_NAMES`, so a command
/// renamed there is renamed here in the same commit or not at all. A
/// hand-written table would be a second vocabulary, and the moment it
/// drifted nothing would say so.
pub(crate) fn verbs() -> Vec<String> {
    let mut out: Vec<String> = CONTROL.iter().map(|name| (*name).to_owned()).collect();
    out.extend(channels::COMMAND_NAMES.iter().map(|name| snake(name)));
    out.extend(channels::QUERY_NAMES.iter().map(|name| snake(name)));
    out
}

/// The verbs closest to something a person typed: those sharing the
/// longest prefix with it that any verb shares at all.
///
/// Cheap on purpose. An edit distance would be a better guess and a
/// worse answer, because somebody who mistyped a verb wants the short
/// list they can read, not the one winner they then have to doubt.
fn nearest(verb: &str) -> Vec<String> {
    let known = verbs();
    let typed: Vec<char> = verb.chars().collect();
    for length in (1..=typed.len().min(4)).rev() {
        let head: String = typed.iter().take(length).collect();
        let mut close: Vec<String> = known
            .iter()
            .filter(|name| name.starts_with(&head))
            .cloned()
            .collect();
        if !close.is_empty() {
            close.truncate(6);
            return close;
        }
    }
    Vec::new()
}

/// Reads one line.
///
/// # Errors
/// None: an unreadable line is an answer (`Unknown`), because a console
/// that refused to classify a line would have nothing to say about it.
pub(crate) fn parse(line: &str, selected: Option<&Address>) -> Line {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Line::Nothing;
    }
    let Some(rest) = trimmed.strip_prefix('/') else {
        // Plain text is work. Naming a room by hand for every task would
        // make the common case the expensive one.
        return match selected {
            Some(_) => Line::Work(trimmed.to_owned()),
            None => Line::Unknown {
                verb: "(no room selected)".to_owned(),
                nearest: vec!["at".to_owned()],
            },
        };
    };
    let (verb, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let tail = tail.trim();
    match verb {
        "help" | "?" => Line::Help,
        "web" => Line::OpenWeb,
        "quit" | "exit" => Line::Quit,
        "at" => match Address::parse(tail) {
            Ok(addr) => Line::Select(addr),
            Err(_) => Line::Unknown {
                verb: format!("at {tail}"),
                nearest: vec!["at <building>/<room>".to_owned()],
            },
        },
        other => wire_frame(other, tail),
    }
}

/// A wire verb and the rest of the line, turned into the frame it names.
///
/// The body is JSON because the wire is JSON: inventing a second
/// argument grammar here would be a second description of every type on
/// the wire, and the two would disagree the first time either moved.
fn wire_frame(verb: &str, tail: &str) -> Line {
    let body = if tail.is_empty() { "null" } else { tail };
    let known_command = channels::COMMAND_NAMES
        .iter()
        .find(|name| snake(name) == verb);
    let known_query = channels::QUERY_NAMES
        .iter()
        .find(|name| snake(name) == verb);
    let framed = match (known_command, known_query) {
        (Some(name), _) => format!("{{\"command\":{{{}:{body}}}}}", quoted(name)),
        (None, Some(name)) => {
            if tail.is_empty() {
                format!("{{\"query\":{}}}", quoted(name))
            } else {
                format!("{{\"query\":{{{}:{body}}}}}", quoted(name))
            }
        }
        (None, None) => {
            return Line::Unknown {
                verb: verb.to_owned(),
                nearest: nearest(verb),
            };
        }
    };
    match serde_json::from_str::<channels::ClientFrame>(&framed) {
        Ok(frame) => Line::Frame(Box::new(frame)),
        Err(_) => Line::Unknown {
            verb: format!("{verb} {tail}"),
            nearest: vec![format!("{verb} takes a JSON body; see `sprawling call`")],
        },
    }
}

/// A wire name in its snake_case spelling, as a JSON key.
fn quoted(camel: &str) -> String {
    serde_json::Value::String(snake(camel)).to_string()
}

/// What the console prints when asked what it knows.
///
/// Grouped the way the wire groups itself, and generated from the same
/// two constants the parser reads.
pub(crate) fn help(selected: Option<&Address>) -> String {
    let commands: Vec<String> = channels::COMMAND_NAMES.iter().map(|n| snake(n)).collect();
    let queries: Vec<String> = channels::QUERY_NAMES.iter().map(|n| snake(n)).collect();
    let room = selected.map_or_else(
        || "no room selected - `/at <building>/<room>` first".to_owned(),
        |addr| format!("work goes to {}", addr.as_str()),
    );
    format!(
        "\n  {room}\n\n  \
         /help                     this\n  \
         /at <building>/<room>     choose where plain lines go\n  \
         /web                      open the WebUI, token included\n  \
         /quit                     close this console; the city stops\n\n  \
         anything else             work, dispatched to the chosen room\n\n  \
         /<query>                  {}\n  \
         /<command> <json>         {}\n",
        queries.join(", "),
        commands.join(", ")
    )
}

/// The URL `/web` opens, with the pairing token on it when there is one.
///
/// A token in a query string is a token in the browser's history, and
/// that is the trade this makes deliberately: the alternative is a
/// person copying a secret by hand between two windows, which they will
/// do wrongly and then paste somewhere worse.
pub(crate) fn web_url(terminal: &Terminal) -> String {
    match &terminal.token {
        None => terminal.url.clone(),
        Some(token) => format!("{}/?token={token}", terminal.url.trim_end_matches('/')),
    }
}

/// Runs the console on threads of its own until the input ends.
///
/// Spawned rather than awaited because reading a keyboard blocks and the
/// reactor is serving a city. Ending the input ends the console and
/// **not** the city: one that stopped answering because nobody was
/// typing would have made interaction a condition of service.
pub(crate) fn start(
    terminal: Terminal,
    desk: Arc<crate::assembly::CommandDesk>,
    mut watching: tokio::sync::broadcast::Receiver<kernel::EventRecord>,
) {
    // What happened, printed as it happens, one JSON object per line -
    // the same shape `sprawling call` prints, because a second rendering
    // would be a second description of every event kind.
    std::thread::spawn(move || {
        while let Ok(record) = watching.blocking_recv() {
            if let Ok(text) = serde_json::to_string(&record) {
                println!("{text}");
            }
        }
    });
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        drive(&terminal, &desk, &mut stdin.lock(), &mut std::io::stdout());
    });
}

/// The loop, over any reader and writer so a test can drive it.
fn drive<R: BufRead, W: Write>(
    terminal: &Terminal,
    desk: &crate::assembly::CommandDesk,
    input: &mut R,
    out: &mut W,
) {
    let mut selected: Option<Address> = None;
    let mut typed = String::new();
    loop {
        typed.clear();
        if write!(out, "> ").is_err() || out.flush().is_err() {
            return;
        }
        match input.read_line(&mut typed) {
            // End of input: a pipe, a service, a machine with nobody at
            // it. The console stops; the city does not.
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        match parse(&typed, selected.as_ref()) {
            Line::Nothing => {}
            Line::Help => {
                let _ = writeln!(out, "{}", help(selected.as_ref()));
            }
            Line::OpenWeb => {
                let url = web_url(terminal);
                let _ = writeln!(out, "  {url}");
                // Never fatal: the URL is on the screen either way.
                let _ = crate::firstrun::open_in_browser(&url);
            }
            Line::Select(addr) => {
                let _ = writeln!(out, "  work goes to {}", addr.as_str());
                selected = Some(addr);
            }
            Line::Quit => {
                let _ = writeln!(out, "  the console is closing; the city keeps serving");
                return;
            }
            Line::Unknown { verb, nearest } => {
                let _ = writeln!(out, "  no verb `{verb}`");
                if !nearest.is_empty() {
                    let _ = writeln!(out, "  did you mean: {}", nearest.join(", "));
                }
            }
            Line::Frame(frame) => post(desk, *frame, out),
            Line::Work(task) => {
                let Some(addr) = selected.clone() else {
                    continue;
                };
                match dispatch(&addr, &task) {
                    Ok(frame) => post(desk, frame, out),
                    Err(err) => {
                        let _ = writeln!(out, "  {err}");
                    }
                }
            }
        }
    }
}

/// One frame, onto the same desk a browser's frames land on.
fn post<W: Write>(desk: &crate::assembly::CommandDesk, frame: channels::ClientFrame, out: &mut W) {
    match frame {
        channels::ClientFrame::Command(command) => {
            // A refusal comes back here rather than into a log file, over
            // the reply address the socket path already uses.
            desk.post(
                (*command).into(),
                channels::Reply::to(move |error: kernel::AxError| {
                    eprintln!("  {error}");
                    eprintln!("  {}", error.recovery());
                    channels::Delivered::ToThePeer
                }),
            );
        }
        // A question has an answer and a desk has no answer to give, so
        // it is redirected rather than posted and forgotten. The wire
        // client is the surface that answers questions.
        channels::ClientFrame::Query(query) => {
            let asked =
                serde_json::to_string(&channels::ClientFrame::Query(query)).unwrap_or_default();
            let _ = writeln!(out, "  a question is answered over the wire:");
            let _ = writeln!(out, "      sprawling call '{asked}'");
        }
        channels::ClientFrame::Hello(_) => {
            let _ = writeln!(out, "  this console is already inside the city");
        }
    }
}

/// A line of work, as the Command a browser would have sent for it.
///
/// # Errors
/// Refuses only when the mode tag this console names stops being a mode
/// tag, which would be a change in `channels::wire` this file has not
/// followed - so it is reported rather than assumed away.
fn dispatch(addr: &Address, task: &str) -> Result<channels::ClientFrame, kernel::AxError> {
    Ok(channels::ClientFrame::Command(Box::new(
        channels::WireCommand::Dispatch {
            addr: addr.clone(),
            task: task.to_owned(),
            goal: String::new(),
            mode: channels::ModeTag::parse("plan")?,
            budget: kernel::BudgetCap::default(),
            idem: kernel::IdemKey::derive(
                &kernel::RunId::CITY,
                kernel::Seq::FIRST,
                format!("console:{}:{task}", addr.as_str()).as_bytes(),
            ),
        },
    )))
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
    use super::{CONTROL, Line, help, parse, snake, verbs};
    use kernel::Address;

    fn room() -> Address {
        Address::parse("lab/room1").unwrap()
    }

    /// The load-bearing property of this whole module: the verb table is
    /// a projection of the wire's vocabulary. A hand-written list would
    /// drift, and nothing would say so.
    #[test]
    fn every_wire_verb_is_a_verb_this_console_answers_to() {
        let known = verbs();
        for name in channels::COMMAND_NAMES.iter().chain(&channels::QUERY_NAMES) {
            assert!(
                known.contains(&snake(name)),
                "{name} is on the wire and not in the console"
            );
        }
    }

    /// A name that meant one thing on the wire and another in the
    /// console would make `/cancel` ambiguous to a person and to this
    /// parser at once.
    #[test]
    fn no_control_verb_shares_a_name_with_a_wire_verb() {
        for control in CONTROL {
            let clash = channels::COMMAND_NAMES
                .iter()
                .chain(&channels::QUERY_NAMES)
                .any(|name| snake(name) == control);
            assert!(!clash, "{control} means two things");
        }
    }

    #[test]
    fn the_wire_spelling_becomes_the_typed_spelling() {
        assert_eq!(snake("AttachEndpoint"), "attach_endpoint");
        assert_eq!(snake("RunView"), "run_view");
        assert_eq!(snake("Fork"), "fork");
    }

    #[test]
    fn an_empty_line_is_not_a_question() {
        assert_eq!(parse("   ", Some(&room())), Line::Nothing);
    }

    #[test]
    fn plain_text_is_work_for_the_chosen_room() {
        assert_eq!(
            parse("  measure the beam  ", Some(&room())),
            Line::Work("measure the beam".to_owned())
        );
    }

    /// With nowhere for the work to go, the console says what to type
    /// rather than guessing a room on somebody's behalf.
    #[test]
    fn plain_text_with_no_room_chosen_says_what_to_type() {
        let Line::Unknown { nearest, .. } = parse("measure the beam", None) else {
            panic!("work with no room is refused");
        };
        assert_eq!(nearest, vec!["at".to_owned()]);
    }

    #[test]
    fn the_control_verbs_are_the_four_it_owns() {
        assert_eq!(parse("/help", None), Line::Help);
        assert_eq!(parse("/web", None), Line::OpenWeb);
        assert_eq!(parse("/quit", None), Line::Quit);
        assert_eq!(parse("/at lab/room1", None), Line::Select(room()));
    }

    #[test]
    fn a_query_with_no_arguments_is_the_bare_name() {
        let Line::Frame(frame) = parse("/city_view", None) else {
            panic!("city_view is a query");
        };
        assert!(matches!(
            *frame,
            channels::ClientFrame::Query(channels::Query::CityView)
        ));
    }

    #[test]
    fn a_query_that_needs_an_argument_takes_it_as_json() {
        let Line::Frame(frame) = parse("/archive_search {\"needle\":\"beam\"}", None) else {
            panic!("archive_search takes a needle");
        };
        match *frame {
            channels::ClientFrame::Query(channels::Query::ArchiveSearch { needle }) => {
                assert_eq!(needle, "beam");
            }
            _ => panic!("the frame is the query that was named"),
        }
    }

    #[test]
    fn an_unknown_verb_comes_back_with_the_ones_that_start_like_it() {
        let Line::Unknown { verb, nearest } = parse("/carn", None) else {
            panic!("carn is nobody's verb");
        };
        assert_eq!(verb, "carn");
        assert!(nearest.contains(&"cancel".to_owned()), "{nearest:?}");
    }

    /// A wire verb with a body it cannot read is a body problem, not a
    /// verb problem, and the answer says so.
    #[test]
    fn a_known_verb_with_an_unreadable_body_is_told_apart_from_an_unknown_one() {
        let Line::Unknown { nearest, .. } = parse("/dispatch not json", None) else {
            panic!("dispatch needs a body it can read");
        };
        assert!(nearest[0].contains("JSON body"), "{nearest:?}");
    }

    #[test]
    fn help_names_every_verb_the_parser_answers_to() {
        let text = help(Some(&room()));
        for name in channels::COMMAND_NAMES.iter().chain(&channels::QUERY_NAMES) {
            assert!(text.contains(&snake(name)), "{name} is missing from help");
        }
        assert!(text.contains("lab/room1"), "help says where work goes");
    }
}
