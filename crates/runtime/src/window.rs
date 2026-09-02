// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The run's conversation history: the volatile half of a request.
//!
//! Frozen-prefix bytes never live here. What does is every message the
//! run has exchanged since it started, folded forward turn by turn by
//! the executor that owns it, and handed to the model beneath the
//! prefix on each call.
//!
//! **One invariant, enforced at every entrance**: consecutive user
//! content joins the message already open rather than opening a second
//! one. A steer, a tool result and the opening task are three doors into
//! the same rule, so none of them can produce the alternating shape a
//! provider refuses.

use kernel::{ChatMessage, ContentBlock, Role};

/// How the first user message opens.
///
/// Exhaustive, and the choice is made once by the city that wrote (or
/// did not write) the job file. It is not a formatting preference: a
/// session working from an assignment and a session talking with the
/// person want different first words, and inferring which from an empty
/// string would make the emptiness of a goal mean two things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// Somebody wrote the task down; the job file's text is in the prefix.
    FromJob,
    /// Nobody did; the person is on the other side of this message.
    WithPerson,
}

/// The run's conversation history, owned by the executor and folded
/// forward turn by turn. Frozen-prefix bytes never live here — the
/// window is the volatile half of the request.
#[derive(Debug, Clone, Default)]
pub struct Window {
    messages: Vec<ChatMessage>,
}

impl Window {
    pub fn new() -> Window {
        Window::default()
    }

    /// The dispatch lines: deterministic from `run_started`'s recorded
    /// inputs, hence rebuildable.
    ///
    /// No pointer to the job file. Its text is the run segment of the
    /// frozen prefix, so a line sending the agent to fetch what it has
    /// already been handed costs a turn and buys nothing; the content
    /// hash that line used to carry is recorded twice in the ledger,
    /// which is where provenance belongs.
    pub fn push_task_lines(&mut self, task: &str, goal: &str, opening: Opening) {
        self.push_user_text(match opening {
            Opening::FromJob => format!("Task: {task}\nGoal: {goal}"),
            // The person's own line, unwrapped. A conversational turn
            // dressed in field labels reads as a form, and a form is
            // answered with a form.
            Opening::WithPerson => task.to_owned(),
        });
    }

    /// Steer joins the tail of the last user message, or opens one if none is open.
    pub fn push_steer(&mut self, source: &str, text: &str) {
        self.push_user_text(format!("{source}: {text}"));
    }

    pub fn push_assistant(&mut self, content: Vec<ContentBlock>) {
        if !content.is_empty() {
            self.messages.push(ChatMessage {
                role: Role::Assistant,
                content,
            });
        }
    }

    /// Tool results open the next user message.
    pub fn push_tool_results(&mut self, results: Vec<ContentBlock>) {
        if results.is_empty() {
            return;
        }
        match self.messages.last_mut() {
            Some(last) if last.role == Role::User => last.content.extend(results),
            _ => self.messages.push(ChatMessage {
                role: Role::User,
                content: results,
            }),
        }
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    fn push_user_text(&mut self, text: String) {
        let block = ContentBlock::Text { text };
        match self.messages.last_mut() {
            Some(last) if last.role == Role::User => last.content.push(block),
            _ => self.messages.push(ChatMessage {
                role: Role::User,
                content: vec![block],
            }),
        }
    }
}
