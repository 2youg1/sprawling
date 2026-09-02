// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Who gets woken, and where the work lands.

use kernel::{Address, AxCode, AxError, EventKind};
use kernel::{Locator, RunId, TimeMs};

use crate::serving::CommandDesk;

use crate::effect;

use super::{
    CITY_VERIFIER, Driven, Driving, Ending, RunWorker, Sweep, artifact_of, new_inbox, now_ms,
};

/// A resident who was signalled and has no run open.
///
/// The second of the two ways a signal reaches somebody. The first
/// slips under the door of a run that is already working - a steer-kind
/// signal, landing at that run's next safe point with the sender's
/// address in front of it. This one knocks: it starts a run for a
/// resident who is not working, because a message nobody is there to
/// read is the same as no message.
/// What one dispatch is, before anything is built for it.
///
/// Four values that reach every phase below together and are never
/// chosen independently: standing the run up, laying out its bench,
/// freezing its plan, settling its desks and concluding it all name the
/// same address, mode and ceiling. Passed side by side they were four
/// parameters on six signatures, and the depth had to be derived twice.
pub(super) struct Assignment {
    pub(super) addr: Address,
    pub(super) mode: runtime::Mode,
    pub(super) budget: kernel::BudgetCap,
    /// The run that handed this work down, when somebody did.
    pub(super) parent: Option<RunId>,
}

impl Assignment {
    /// Derived from whether somebody handed this work down, rather than
    /// carried beside it. Two values that must agree are two chances to
    /// disagree, and the one that disagrees here is a grand-delegate.
    pub(super) fn depth(&self) -> kernel::Depth {
        match self.parent {
            None => kernel::Depth::Root,
            Some(_) => kernel::Depth::Delegated,
        }
    }
}

/// What the run was given: the brief on disk, the words it was written
/// from, and the pin of the bytes the run segment carried.
///
/// One value because the four are one act of writing: the brief is
/// rendered from the task and the goal, and the locator pins the brief's
/// own bytes. Two of the four reaching a phase without the others would
/// let the prompt and the history disagree about what was asked.
pub(super) struct Given {
    pub(super) brief: city::RunBrief,
    pub(super) task: String,
    pub(super) goal: String,
    pub(super) job: Locator,
}

pub(super) struct Knock {
    pub(super) addr: Address,
    /// Who spoke, as they will be named in the woken run's own brief.
    pub(super) from: String,
    /// The mode and the spending ceiling of the run that spoke. Carried
    /// rather than defaulted: an answer belongs to the same piece of
    /// work as the question, and a run with no ceiling is the one
    /// failure with no floor under it.
    pub(super) mode: runtime::Mode,
    pub(super) budget: kernel::BudgetCap,
}

/// What one dispatch left behind. Carried rather than re-derived,
/// because the run that asked for the work has to be told how it ended
/// and the ledger is not a thing this layer reads back mid-command.
pub(crate) struct Dispatched {
    pub(super) run: RunId,
    pub(super) addr: Address,
    pub(super) who: String,
    pub(super) completion: kernel::Completion,
}

/// What the digest model is told when it is asked to name a piece of
/// work.
///
/// Short on purpose, and it states the shape of a legal answer rather
/// than trusting one: `SessionName::parse` is the authority and refuses
/// anything with a separator in it, so a prompt that did not say "one
/// segment" would spend a call to be refused.
pub(super) const NAME_THE_WORK: &str = "Name this piece of work in two to four words, joined by hyphens, in \
     lowercase ASCII. Answer with the name alone: no path, no quotes, no \
     explanation. Example: refactor-ledger-reads";

/// How many tokens a name is worth. Four words do not need more, and a
/// ceiling is what stops a model that decided to explain itself from
/// costing a person real money for a filename.
pub(super) const NAME_TOKENS: u64 = 32;

/// How many turns one dispatch may take before it freezes at `Limit`.
/// A budget the caller cannot set yet is still a budget: an unbounded
/// loop against a paid provider is the one failure with no ceiling.
pub(super) const DISPATCH_TURN_BUDGET: u32 = 24;

/// A run's identity, derived rather than drawn: the same job dispatched
/// at the same millisecond to the same address is the same run, and no
/// randomness enters the ledger's identifiers.
pub(super) fn run_id_for(job: &Locator, addr: &Address, now: TimeMs) -> RunId {
    let seed = format!("{job}|{}|{}", addr.as_str(), now.value());
    let digest = kernel::B3Hash::digest(seed.as_bytes()).to_string();
    let mut bytes = [0u8; 16];
    for (slot, pair) in bytes.iter_mut().zip(digest.as_bytes().chunks_exact(2)) {
        let hex = std::str::from_utf8(pair).unwrap_or("00");
        *slot = u8::from_str_radix(hex, 16).unwrap_or(0);
    }
    RunId::from_bytes(bytes)
}

/// An outside editor's request, turned into the city's usual dispatch.
///
/// Not a second control surface: the admission decides what a stranger
/// may learn, and everything after it is the path a person's dispatch
/// takes. The run identifier is minted when the worker takes the work,
/// so what an editor is told now is the honest thing - accepted, and
/// nothing finished yet.
pub(crate) fn acp_dispatch(
    desk: &CommandDesk,
    body: channels::AcpBody,
    authentic: bool,
) -> Result<channels::AcpProgress, AxError> {
    let incoming = protocol::Incoming {
        token: body.token,
        addr: Address::parse(&body.addr)?,
        task: body.task,
        goal: body.goal,
    };
    let protocol::Admitted::Dispatch { addr, task, goal } = protocol::admit(&incoming, authentic)?;
    let idem = kernel::IdemKey::derive(
        &RunId::CITY,
        kernel::Seq::FIRST,
        format!("acp:{}:{task}", addr.as_str()).as_bytes(),
    );
    // An editor gets its answer from this function and then stops
    // listening, so there is no peer left for a later refusal to reach.
    // Saying that in the type beats a silent third meaning of `Reply`.
    desk.post(
        channels::Command::Dispatch {
            addr,
            task,
            goal,
            mode: channels::ModeTag::parse("plan")?,
            budget: kernel::BudgetCap::default(),
            idem,
            // An editor drives an address it already chose.
            session: None,
            // ...and says nothing about how hard to think, so the
            // layers above answer.
            effort: None,
        },
        channels::Reply::nowhere(),
    );
    Ok(channels::AcpProgress {
        run: idem.to_string(),
        turns: 0,
        finished: false,
    })
}

impl RunWorker {
    /// One dispatch under a stated mode. The mode decides nothing until
    /// the work is offered back to the building: what it changes is the
    /// evidence that offer has to carry.
    pub(super) fn dispatch_in(
        &mut self,
        at: Assignment,
        task: String,
        goal: String,
    ) -> Result<Dispatched, AxError> {
        // Nothing is written before the city agrees to take the work:
        // a halted city that laid a job file down would leave a task in
        // a room no run ever opened.
        if let Some(scope) = self.halted_by(&at.addr) {
            return Err(AxError::failure(
                AxCode::GateDenied,
                "dispatch work",
                at.addr.as_str().to_owned(),
            )
            .with_recovery(format!(
                "{scope} is halted; release it to let work in again. Runs already going are \
                 unaffected - stopping one is `cancel`"
            )));
        }
        // The task file exists first, then the run exists: the job on
        // disk is what the agent reads, and the copy in the store is
        // what the history keeps, so editing one cannot rewrite the
        // other.
        let brief = city::write_brief(
            &self.city_root,
            &at.addr,
            &city::JobBrief {
                task: &task,
                goal: &goal,
                budget: &format!("{DISPATCH_TURN_BUDGET} turns"),
            },
        )?;
        // What the run was given, pinned whichever arm it is: for a
        // session nobody assigned, the pin holds the words that said so,
        // so the ledger's `job` locator resolves to the bytes the run
        // segment actually carried rather than to a file that was never
        // written.
        let job_hash = self
            .cas
            .put(brief.segment_text().as_bytes())
            .map_err(memory::MemoryError::into_ax)?;
        let given = Given {
            job: Locator::parse(&format!("cas:b3-{job_hash}"))?,
            brief,
            task,
            goal,
        };
        // Kept for the post-drive sweep: an escalation names the work it
        // interrupted, and by then the plan has consumed the original.
        let job_locator = given.job.clone();

        // Where this run stands, and the desks it works at: two phases,
        // two values, both carried whole rather than taken apart into a
        // row of locals every phase below would then have to be handed
        // one at a time.
        let mut site = self.stand_up(&at, &given)?;
        let desks = self.open_desks(&site, &at.addr)?;

        // What the model may see, what routes the call it makes, and
        // who it may hand work down to: one phase, one value.
        let mut workbench = self.lay_out_workbench(&site, &desks, &at, &job_locator)?;
        // The plan this run is frozen with, and the handoff that says
        // what to read to pick it up again: one phase, because the
        // handoff quotes the plan and the plan is what the prefix was
        // assembled for.
        let (plan, handoff) = self.freeze_plan(&site, &workbench, &at, given)?;

        let fence_scope = site.fence_scope(&at.addr);
        let Driven {
            outcome: driven,
            fenced,
            ran,
            mut raised,
        } = self.drive_dispatch(
            plan,
            &handoff,
            Driving {
                adapter: site.adapter.as_mut(),
                bench: &mut workbench.bench,
                signals: &desks.signals,
                write_root: &site.write_root,
                fence_scope: fence_scope.clone(),
                who: &site.who,
                run_id: site.run_id,
            },
        )?;
        self.settle_desks(
            &site,
            &at,
            &desks,
            Sweep {
                fenced: &fenced,
                raised: &mut raised,
                job_locator: &job_locator,
            },
        )?;
        // What the run can show for itself. `None` is not `Some(false)`:
        // "nothing ran" and "something ran and failed" are different
        // facts, and the modes that care refuse them differently.
        let produced = {
            let (ok, failed) = ran;
            runtime::Produced {
                tests_passed: if ok == 0 && failed == 0 {
                    None
                } else {
                    Some(failed == 0)
                },
                // The city cannot see a contract move from here; a run
                // that renovates says so through its evidence, and until
                // it can, SC admits on the honest default.
                contract_moved: false,
                held_in: None,
                held_out: None,
            }
        };
        self.settle_requests(&site, &at, &desks.pr, &produced)?;
        self.conclude(
            site,
            &at,
            Ending {
                driven,
                raised,
                delegates: &workbench.delegates,
            },
        )
    }

    /// Where a building keeps the plan its residents claim rows from.
    /// One dispatch, run to its frozen end on this thread.
    pub(super) fn dispatch(
        &mut self,
        addr: Address,
        task: String,
        goal: String,
    ) -> Result<(), AxError> {
        self.dispatch_in(
            Assignment {
                addr,
                mode: runtime::Mode::PlanGoal,
                budget: kernel::BudgetCap::default(),
                parent: None,
            },
            task,
            goal,
        )
        .map(drop)
    }

    /// Where a dispatch works: the room a named session opens, or the
    /// address it was sent to.
    ///
    /// A named session opens a room of its own under the building, so
    /// two sessions started from the same screen do not write over each
    /// other's files. An unnamed one is a person continuing what is
    /// already at that address.
    /// What this piece of work should be called, when the person did not
    /// say.
    ///
    /// A person who writes one sentence has named the work in it, and
    /// making them name it twice is the ceremony this interface exists
    /// to remove. So an address that names a building with no session
    /// beside it is answered here, by asking the cheapest model this
    /// city has for a short name.
    ///
    /// **A refusal, never a guess.** When no model answers, when the
    /// answer is not a legal session name, or when the address already
    /// names a room, this returns what it was given. The failure a
    /// person then sees names the field they have to fill, and the
    /// composer opens that one control — which is the whole reason the
    /// fallback is a refusal rather than a name this city made up. A run
    /// living in a room somebody did not choose cannot be found again by
    /// the name they would look for.
    pub(super) fn session_for(
        &mut self,
        addr: &Address,
        session: Option<kernel::SessionName>,
        task: &str,
    ) -> Result<Option<kernel::SessionName>, AxError> {
        if session.is_some() {
            return Ok(session);
        }
        // An address with a room in it is already a session: this is the
        // shape a second dispatch into an open session takes, and naming
        // it again would open a room inside a room.
        if addr.as_str().contains('/') {
            return Ok(None);
        }
        let Some(named) = self.name_the_work(task) else {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "work out what to call this work",
                addr.as_str().to_owned(),
            )
            .with_recovery(
                "name the room yourself: send the work to `building/name` rather than to \
                 `building`",
            ));
        };
        Ok(Some(named))
    }

    /// One cheap model call, turning a task into a short room name.
    ///
    /// The digest tag, which is this city's word for the small model
    /// that reads so the main one does not have to. Naming a piece of
    /// work is exactly that shape of job, and putting it on the main
    /// model would charge a person reasoning tokens for a filename.
    ///
    /// `None` for every failure this can have — no model registered for
    /// the tag, a call that did not come back, an answer that is not a
    /// legal session name. The caller turns that into a refusal naming
    /// the field, so a failure here costs a person one field rather than
    /// putting their work somewhere they will not look for it.
    fn name_the_work(&mut self, task: &str) -> Option<kernel::SessionName> {
        let chosen = self
            .book
            .select(kernel::ModelTag::Digest, &kernel::BuildingPolicy::default())
            .ok()?;
        let model_id = chosen.entry.id.clone();
        let mut adapter = self.adapter_for(&chosen).ok()?;
        let answer = adapter
            .call(&kernel::ModelRequest {
                policy: kernel::BuildingPolicy::default(),
                segments: [kernel::B3Hash::digest(b""); 4],
                chat: kernel::ChatRequest {
                    model: model_id,
                    max_tokens: NAME_TOKENS,
                    system: vec![kernel::SystemBlock {
                        text: NAME_THE_WORK.to_owned(),
                        cache: false,
                    }],
                    messages: vec![kernel::ChatMessage {
                        role: kernel::Role::User,
                        content: vec![kernel::ContentBlock::Text {
                            text: task.to_owned(),
                        }],
                    }],
                    tools: Vec::new(),
                    effort: None,
                },
            })
            .ok()?;
        // The model is asked for one word and sometimes writes a
        // sentence around it. The first line, stripped of the
        // punctuation an answer tends to arrive wrapped in, is what is
        // offered to the parser — and the parser decides, not this.
        let said = kernel::content_from_message(&answer.message)
            .ok()?
            .into_iter()
            .find_map(|block| match block {
                kernel::ContentBlock::Text { text } => Some(text),
                _ => None,
            })?;
        let candidate = said
            .lines()
            .next()?
            .trim()
            .trim_matches(|glyph: char| glyph == '`' || glyph == '"' || glyph == '.');
        kernel::SessionName::parse(candidate).ok()
    }

    pub(super) fn room_for(
        &self,
        addr: Address,
        session: Option<&kernel::SessionName>,
    ) -> Result<Address, AxError> {
        match session {
            None => Ok(addr),
            Some(name) => {
                let building = city::Building::of(&addr)?;
                city::open_room(&self.city_root, building.addr(), name)
            }
        }
    }

    /// Tells the run that asked for the work how it came back.
    ///
    /// The child's account is pinned in the store before it is judged,
    /// so the locator the parent is handed resolves to bytes rather than
    /// to a sentence this process happened to build. The city verifies:
    /// `Completion::Done` is something the city observed, and a producer
    /// verifying itself is what `Claim::verified` refuses.
    pub(super) fn deliver_handback(
        &mut self,
        parent: &Address,
        child: &Dispatched,
    ) -> Result<(), AxError> {
        let account = format!(
            "room: {}\nby: {}\nending: {}\n",
            child.addr.as_str(),
            child.who,
            child.completion.name()
        );
        let digest = self
            .cas
            .put(account.as_bytes())
            .map_err(memory::MemoryError::into_ax)?;
        let claim = collab::Claim::new(
            collab::NodeId::parse(child.addr.as_str())?,
            Locator::parse(&format!("cas:b3-{digest}"))?,
            digest,
            child.who.clone(),
        );
        let back = collab::Handback::of(
            claim,
            matches!(child.completion, kernel::Completion::Done(_)),
            CITY_VERIFIER,
        );
        let signal = back.signal(
            collab::SignalId::parse(&format!("handback-{}", child.run))?,
            parent.clone(),
            now_ms()?,
        )?;
        // Recorded, then delivered - the same order every other signal
        // takes, so the queue only ever changes as a consequence of a
        // line the history already has.
        self.record_for(
            child.run,
            effect::Line {
                who: child.who.to_owned(),
                addr: parent.clone(),
                kind: EventKind::SignalEnqueued,
                data: signal.enqueued_payload()?,
            },
        )?;
        self.inboxes
            .entry(parent.clone())
            .or_insert_with(new_inbox)
            .deliver(&signal)?;
        // And into the room's join, by the same reading a restart would
        // do: one function decides what a handback signal means.
        if let Some(artifact) = artifact_of(&signal) {
            self.joins
                .entry(parent.clone())
                .or_default()
                .accept(artifact);
        }
        Ok(())
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
    use crate::assembly::fixture::*;
    use crate::assembly::*;

    /// The other half of the same rule: a steer-kind signal slips under
    /// the door of the run it reaches, landing at that run's next safe
    /// point with the sender's address in front of it. `collab::steer`
    /// has held both entrances since P2.04 and the resident's one had no
    /// caller until now.
    #[test]
    fn a_steer_from_a_resident_lands_in_the_window_as_that_resident() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        city::create_building(
            dir.path(),
            &Address::parse("market").unwrap(),
            city::BuildingTemplate::Minimal,
        )
        .unwrap();
        for who in ["ito", "hana"] {
            let room = dir.path().join("market").join(who);
            std::fs::create_dir_all(&room).unwrap();
            std::fs::write(
                room.join(city::URBANITE_FILE),
                format!("# URBANITE.md\n\nTrades in the market as {who}.\n"),
            )
            .unwrap();
        }

        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "cutting in",
                    "tu_1",
                    "signal",
                    serde_json::json!({
                        "action": "send",
                        "to": "market/hana",
                        "kind": "steer",
                        "text": "drop the glaze order, the kiln comes first",
                    }),
                ),
                completion("sent", None),
                tool_completion("where do I stand", "tu_2", "status", serde_json::json!({})),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("market/ito").unwrap(),
                task: "tell hana what matters first".to_owned(),
                goal: "hana knows".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();

        let asked = provider.bodies().join("\n");
        assert!(
            asked.contains("@market/ito: drop the glaze order"),
            "the steer lands at the end of a tool result, attributed to the resident who sent it"
        );
        assert!(
            !asked.contains("user: drop the glaze order"),
            "only the person's entrance can render as the person"
        );
    }

    /// A dispatch that never says when to stop is a conversation, and the
    /// prefix says so instead of handing over a form with its one
    /// irreplaceable field blank.
    #[test]
    fn a_dispatch_with_no_goal_leaves_no_job_file_and_says_the_person_is_here() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let room = Address::parse("lab/room1").unwrap();
        let (base_url, provider) = fake_openai(&["m-local"], vec![completion("done", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: room.clone(),
                task: "what do you make of this".to_owned(),
                goal: String::new(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"talk"),
                session: None,
                effort: None,
            })
            .unwrap();

        assert!(
            !city::job_path(dir.path(), &room).exists(),
            "a conversation writes no task file"
        );
        let asked = provider.bodies().join("\n");
        assert!(
            asked.contains("working with the person directly"),
            "the prefix states the situation: {asked}"
        );
        assert!(
            !asked.contains("Task: what do you make of this"),
            "the person's line goes out as they wrote it, not as a form"
        );
    }
}
