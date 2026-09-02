// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! How a city is stood up and served, as opposed to how one piece of
//! work is run.
//!
//! Four things happen here and nothing else: the key this listener will
//! present at its door is settled before a socket exists, the vault is
//! opened and asked what it really is, the one writer thread is started
//! with the ledger inside it, and the socket is handed the four sinks it
//! may reach the city through.
//!
//! **The writer thread is the city's one writer.** The ledger is opened
//! inside it and never leaves, so the type never has to cross a thread
//! boundary to prove that a city has one writer (ARCHITECTURE section
//! 10). Everything a socket does reaches it as a `Command` on a desk,
//! one at a time.
//!
//! Randomness is drawn here rather than in `bin::keying`, which is pure:
//! this crate draws entropy in one place, and a key a third party can
//! predict is a door a third party can open.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc;

use kernel::{AxCode, AxError, EventRecord, Payload, RunId};
use runtime::Interrupt;

use crate::assembly::{RunWorker, acp_dispatch, ledger_dir, now_ms, rebuild_views};
use crate::views::Views;

/// A URL-safe random string of `bytes` bytes of OS entropy.
///
/// Deliberately not the simulator's seeded randomness: a verifier a
/// third party can predict is a login a third party can finish. This is
/// the one place in the binary where reproducibility would be a defect.
pub(crate) fn random_token(bytes: usize) -> Result<String, AxError> {
    let mut raw = vec![0u8; bytes];
    getrandom::fill(&mut raw).map_err(|err| {
        AxError::failure(
            AxCode::ConfigInvalid,
            "draw randomness for a login",
            err.to_string(),
        )
        .with_recovery("this machine's entropy source refused; no login can be started safely")
    })?;
    // The alphabet belongs to the flow that consumes it, and a copy of
    // it here would be both a second authority and - being sixty-four
    // mixed characters at rest - exactly the shape the secret scanner
    // hunts for.
    Ok(gateway::oauth_random(&raw))
}

/// What this serve will present at its door, and whether a person has
/// to be shown it.
///
/// The three cases carry different obligations, so they are an enum
/// rather than an `Option<String>` plus a flag: only [`Self::Minted`] is
/// shown, and only because nobody else has ever seen it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keyed {
    /// A loopback listener. Nothing is presented and nothing is shown.
    NothingToPresent,
    /// What the operator configured, adopted as it stands.
    Adopted(String),
    /// Minted for this serve from this machine's entropy. Shown once,
    /// stored nowhere, and dead when the process ends.
    Minted(String),
}

impl Keyed {
    /// The code the listener and the console both carry, if there is one.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::NothingToPresent => None,
            Self::Adopted(code) | Self::Minted(code) => Some(code),
        }
    }
}

/// Settles this serve's pairing key before anything is bound.
///
/// [`crate::keying::Keying`] decides which of the three cases applies;
/// this performs the one that needs entropy. Minting happens here for
/// the reason `channels::auth` states from its own side — that module
/// samples nothing, and the assembly layer is where randomness in this
/// binary comes from.
///
/// The result is what `channels::decide_bind` will be asked about, so an
/// address reaching beyond this machine arrives at the socket already
/// carrying a key. The guard is satisfied, never relaxed: there is still
/// no moment in which the port is open and unauthenticated.
///
/// # Errors
/// When this machine's entropy source refuses. No key can be minted
/// safely then, and serving anyway would open the port without one.
pub fn key_for(bind: std::net::SocketAddr, configured: Option<String>) -> Result<Keyed, AxError> {
    match crate::keying::Keying::decide(bind, configured.is_some()) {
        crate::keying::Keying::NothingToPresent => Ok(Keyed::NothingToPresent),
        // Refused rather than unwrapped: `decide` answers `Adopt` only
        // when the value is there, and a mismatch between those two would
        // be a defect in this file worth naming out loud.
        crate::keying::Keying::Adopt => configured.map(Keyed::Adopted).ok_or_else(|| {
            AxError::failure(
                AxCode::ConfigInvalid,
                "adopt the configured pairing token",
                "a token was decided on and then was not there",
            )
            .with_recovery("report this: keying::Keying::decide and key_for disagree")
        }),
        crate::keying::Keying::Mint => {
            let mut entropy = [0u8; 32];
            getrandom::fill(&mut entropy).map_err(|err| {
                AxError::failure(
                    AxCode::ConfigInvalid,
                    "draw randomness for a pairing key",
                    err.to_string(),
                )
                .with_recovery(
                    "this machine's entropy source refused; set SPRAWLING_PAIRING_TOKEN \
                     or bind a loopback address",
                )
            })?;
            // The token half is dropped here on purpose. `serve` derives
            // the digest it compares against from this same code through
            // `from_configured`, so holding both would be two paths to
            // one digest and a second thing to keep in step.
            let (_token, code) = channels::PairingToken::mint(entropy);
            Ok(Keyed::Minted(code))
        }
    }
}

/// Opens the credential vault and says what it really is.
///
/// The probe writes, reads and deletes once; whatever backend survives
/// that is the one in use, and the notice it returns is the disclosure
/// that goes in the ledger. A vault that silently forgets across a
/// restart would turn one configuration act into a later egress failure,
/// far from its cause.
#[must_use]
pub fn open_vault() -> (gateway::Custodian, Option<Payload>) {
    gateway::Custodian::probe()
}

/// Where commands wait between the socket and the worker.
///
/// A desk rather than a channel, because a channel hands an item to
/// whoever is blocked on it, and during a run that is nobody: the
/// worker is inside a dispatch. A Cancel that waits for the run it
/// cancels is not a Cancel. The desk keeps arrival order, and the run
/// looks at it only at its own safe points.
pub(crate) struct CommandDesk {
    waiting: std::sync::Mutex<Waiting>,
    arrived: std::sync::Condvar,
    /// Set once, by whoever decided the city stops. Read at the same
    /// point the queue is read, so a close lands between commands and
    /// never inside one.
    closing: std::sync::atomic::AtomicBool,
}

/// What the desk holds, under one lock.
///
/// The queue and the keys are read together and must agree: a key that
/// left the queue between two locks would let the same ask through
/// twice, which is the whole of what this pair prevents.
struct Waiting {
    queue: std::collections::VecDeque<Posted>,
    /// Every key that is either still in `queue` or being carried out.
    /// `kernel::gate` says the seen set belongs to the caller and it
    /// only judges membership; this is that set for commands, as
    /// `ToolBench::seen` is that set for tool calls.
    keys: std::collections::BTreeSet<kernel::IdemKey>,
}

/// The key of the command somebody took off the desk, held until the
/// work is over.
///
/// A guard rather than a call the drainer has to remember: the key is in
/// flight for exactly as long as this value lives, including when the
/// loop that took it leaves early.
pub(crate) struct Underway<'desk> {
    desk: &'desk CommandDesk,
    key: Option<kernel::IdemKey>,
}

impl Drop for Underway<'_> {
    fn drop(&mut self) {
        let Some(key) = self.key else { return };
        if let Ok(mut waiting) = self.desk.waiting.lock() {
            waiting.keys.remove(&key);
        }
    }
}

impl Waiting {
    /// Takes the command at `at` out of the line and releases its key.
    ///
    /// A command consumed as an interrupt is over the moment it is
    /// taken, and every client mints one key per run for cancelling, so
    /// a key left behind here would make a second Cancel unspeakable
    /// for the life of the city.
    fn forget(&mut self, at: usize) -> Option<Posted> {
        let posted = self.queue.remove(at)?;
        if let Some(key) = posted.command.idem() {
            self.keys.remove(key);
        }
        Some(posted)
    }
}

/// A command and the address its refusal goes back to.
///
/// The two travel together because they are separated by a thread and
/// by minutes: by the time the worker refuses, the socket task that
/// accepted the command has long returned.
pub(crate) struct Posted {
    pub(crate) command: channels::Command,
    pub(crate) reply: channels::Reply,
}

/// What the worker found when it looked at the desk. Exhaustive, because
/// "nothing arrived" and "nobody will ever arrive again" are different
/// facts and the loop does different things about them.
pub(crate) enum DeskWait<'desk> {
    /// Boxed because this variant is the only one that carries
    /// anything: a `Posted` holds a whole `Command`, and an enum shaped
    /// like this one is returned by every idle tick as well.
    Command(Box<Posted>, Underway<'desk>),
    Idle,
    /// A person chose to stop. Distinct from `Gone`, which is the desk
    /// itself breaking: one of these deserves a handoff and the other is
    /// a city that can no longer write one.
    Close,
    Gone,
}

/// How long the worker waits before looking at the schedule. Short
/// enough that a job stated to the minute starts within the minute,
/// long enough that an idle city is idle.
const SCHEDULE_TICK: std::time::Duration = std::time::Duration::from_secs(20);

impl CommandDesk {
    /// Visible to the crate so the console loop can be driven in a test
    /// through the door production uses, rather than through a second
    /// one opened for testing.
    pub(crate) fn new() -> CommandDesk {
        CommandDesk {
            waiting: std::sync::Mutex::new(Waiting {
                queue: std::collections::VecDeque::new(),
                keys: std::collections::BTreeSet::new(),
            }),
            arrived: std::sync::Condvar::new(),
            closing: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Says the city is stopping, and wakes the worker so it hears.
    ///
    /// Not a Command: closing is not something a peer asks the city for,
    /// it is the process's own end, and a wire frame that could spell it
    /// would be a stranger's way to stop somebody's city. The worker
    /// reads it where it reads the queue, so whatever is running
    /// finishes first.
    pub(crate) fn close(&self) {
        self.closing
            .store(true, std::sync::atomic::Ordering::Release);
        self.arrived.notify_all();
    }

    /// Puts a command in line, unless the same one is already being
    /// dealt with.
    ///
    /// A key is in flight from here until the command it belongs to has
    /// been carried out, and a second frame carrying that key is
    /// dropped: the sender asked for one thing and one thing is
    /// happening. Every client mints the key from what the person
    /// entered, so a double-click, a transport that resent a frame and
    /// an editor retrying after a timeout all arrive as one ask - and
    /// each of them used to be a second run against a paid provider.
    /// Once the work is over the key is forgotten, so asking for the
    /// same work again is a second piece of work rather than silence.
    pub(crate) fn post(&self, command: channels::Command, reply: channels::Reply) {
        if let Ok(mut waiting) = self.waiting.lock() {
            if let Some(key) = command.idem() {
                if kernel::dedup(&waiting.keys, key) == kernel::DedupVerdict::Duplicate {
                    return;
                }
                waiting.keys.insert(*key);
            }
            waiting.queue.push_back(Posted { command, reply });
            self.arrived.notify_one();
        }
    }

    /// Waits for a command, for at most `patience`.
    ///
    /// The wait has an end so that the worker gets its own idle moment:
    /// a city whose schedule says something starts at nine cannot depend
    /// on somebody clicking at nine.
    pub(crate) fn wait(&self, patience: std::time::Duration) -> DeskWait<'_> {
        let Ok(mut waiting) = self.waiting.lock() else {
            return DeskWait::Gone;
        };
        // Work already accepted is finished first: a close that dropped
        // a queued command would make "stopped" and "lost" the same
        // thing in the record.
        if let Some(posted) = waiting.queue.pop_front() {
            return self.carrying(posted);
        }
        if self.closing.load(std::sync::atomic::Ordering::Acquire) {
            return DeskWait::Close;
        }
        match self.arrived.wait_timeout(waiting, patience) {
            Ok((mut waiting, _)) => match waiting.queue.pop_front() {
                Some(posted) => self.carrying(posted),
                None if self.closing.load(std::sync::atomic::Ordering::Acquire) => DeskWait::Close,
                None => DeskWait::Idle,
            },
            Err(_) => DeskWait::Gone,
        }
    }

    /// Hands out one command together with the guard that keeps its key
    /// in flight until whoever took it is done.
    fn carrying(&self, posted: Posted) -> DeskWait<'_> {
        let key = posted.command.idem().copied();
        DeskWait::Command(Box::new(posted), Underway { desk: self, key })
    }

    /// Takes one command if any is waiting, without waiting for one.
    /// Used where a test drives the desk directly; the worker loop waits.
    #[cfg(test)]
    pub(crate) fn take(&self) -> Option<channels::Command> {
        let mut waiting = self.waiting.lock().ok()?;
        let posted = waiting.queue.pop_front()?;
        if let Some(key) = posted.command.idem() {
            waiting.keys.remove(key);
        }
        Some(posted.command)
    }

    /// What the run at `run` should do at this safe point, if anything.
    ///
    /// Cancel outranks Steer on the same boundary: stopping and changing
    /// course are mutually exclusive, and stopping is the one that cannot
    /// be taken back. Commands for other runs keep their place in line.
    pub(crate) fn interrupt_for(&self, run: RunId) -> Interrupt {
        let Ok(mut waiting) = self.waiting.lock() else {
            return Interrupt::None;
        };
        let cancel = waiting.queue.iter().position(
            |posted| matches!(&posted.command, channels::Command::Cancel { run: r, .. } if *r == run),
        );
        if let Some(at) = cancel {
            waiting.forget(at);
            return Interrupt::Cancel;
        }
        let steer = waiting.queue.iter().position(
            |posted| matches!(&posted.command, channels::Command::Steer { run: r, .. } if *r == run),
        );
        let Some(at) = steer else {
            return Interrupt::None;
        };
        let Some(Posted {
            command: channels::Command::Steer { text, .. },
            ..
        }) = waiting.forget(at)
        else {
            return Interrupt::None;
        };
        // The person's entrance is the only one that renders as `user`,
        // and an empty steer is not an interruption.
        match collab::Steer::from_person(&text) {
            Ok(steer) => Interrupt::Steer {
                source: steer.source().to_owned(),
                text: steer.text().to_owned(),
            },
            Err(_) => Interrupt::None,
        }
    }
}

/// Everything one served city is made of, in one value.
///
/// Eight loose parameters is a signature nobody calls correctly from
/// memory, and two `Option`s of the same shape passed the wrong way
/// round is a mistake the compiler cannot see. Named fields make the
/// call site say which is which.
pub struct Serving {
    pub city_root: std::path::PathBuf,
    pub addr: SocketAddr,
    /// The pairing token in plaintext, read once by the caller. It gets
    /// no further than the digest this takes from it, except into the
    /// console's `/web`, which is the one place it has to travel.
    pub token: Option<String>,
    pub client: channels::ClientAssets,
    pub vault: gateway::Custodian,
    pub vault_notice: Option<Payload>,
    pub log: runtime::diagnostics::Diagnostics,
    pub console: Option<crate::console::Terminal>,
}

/// Starts the city's one writer, and returns once it is running.
///
/// The ledger is opened *inside* this thread and never leaves it: a city
/// has one writer, and the type never has to cross a thread boundary to
/// prove it. The handshake is part of the contract - a thread that comes
/// back from this function has already opened the history and said so,
/// so a caller never serves a socket over a city that failed to open.
///
/// # Errors
/// Propagates whatever opening the history reports, a thread the
/// platform will not start, and a worker that ended before reporting.
/// What a worker is opened with: where the city is, whose keys it may
/// redeem, what the vault turned out to be, and where its diagnostics
/// go.
///
/// Four values that always travel together and are never chosen
/// independently - `serve` settles all four before it has a thread to
/// hand them to - so they travel as one, as `Reporter` does.
struct Opening {
    city_root: std::path::PathBuf,
    vault: gateway::Custodian,
    /// What the vault probe found, on its way to the ledger as a
    /// disclosure. Consumed by the first `open_for_service`.
    notice: Option<Payload>,
    log: runtime::diagnostics::Diagnostics,
}

/// Where a worker's work goes, and where it comes from.
///
/// The desk is the mouth and the other three are the ears: a record
/// reaches the fold every query is answered from, the clients watching
/// the city, and - while a model is still speaking - the watchers of one
/// run's increments.
struct Outward {
    desk: Arc<CommandDesk>,
    views: Arc<std::sync::Mutex<Views>>,
    to_clients: tokio::sync::broadcast::Sender<EventRecord>,
    to_watchers: tokio::sync::broadcast::Sender<channels::Delta>,
}

fn spawn_worker(
    opening: Opening,
    outward: Outward,
) -> Result<std::thread::JoinHandle<()>, AxError> {
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), AxError>>(0);
    let Opening {
        city_root: worker_root,
        vault,
        notice: vault_notice,
        log,
    } = opening;
    let Outward {
        desk: worker_desk,
        views,
        to_clients,
        to_watchers,
    } = outward;
    // The one sanctioned thread besides the runtime's own. The ledger is
    // opened *inside* it and never leaves: a city has one writer, and the
    // type never has to cross a thread boundary to prove it.
    let worker_thread = std::thread::Builder::new()
        .name("sprawling-runs".to_owned())
        .spawn(move || {
            let mut worker = match RunWorker::new(&worker_root, vault, log) {
                Ok(mut worker) => {
                    worker.open_for_service(vault_notice);
                    let _ = ready_tx.send(Ok(()));
                    worker
                }
                Err(err) => {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };
            // Somebody is watching, so runs ask their provider to
            // stream. A worker without this call never installs a sink,
            // and its runs take the blocking path unchanged.
            worker.watch(std::sync::Arc::new(move |delta: channels::Delta| {
                // No subscribers is not a failure: a city with no browser
                // open is a city doing its work.
                let _ = to_watchers.send(delta);
            }));
            worker.observe(Box::new(move |record: &EventRecord| {
                if let Ok(mut views) = views.lock() {
                    // A record the views refuse to fold is reported and
                    // skipped: the ledger already has it, and a view that
                    // crashed the writer would make history hostage to a
                    // projection.
                    if let Err(err) = views.apply(record) {
                        eprintln!("view fold refused {}: {err}", record.seq().value());
                    }
                }
                // A send with no subscribers is not a failure: a city with
                // no browser open is a city doing its work.
                let _ = to_clients.send(record.clone());
            }));
            // A run in progress asks the same desk what arrived, so a
            // Cancel does not have to wait for the run it cancels.
            let interrupt_desk = Arc::clone(&worker_desk);
            worker.attach_interrupts(Box::new(move |run: RunId| {
                interrupt_desk.interrupt_for(run)
            }));
            loop {
                match worker_desk.wait(SCHEDULE_TICK) {
                    // `carrying` holds this command's key in flight for
                    // the length of the arm, so a frame that repeats it
                    // while the work is going adds no second run.
                    DeskWait::Command(posted, carrying) => {
                        worker.serve_one(*posted);
                        drop(carrying);
                    }
                    // The refusal is written inside `tick`; a schedule
                    // that cannot be read must not stop the city from
                    // answering the person.
                    DeskWait::Idle => {
                        if let Ok(now) = now_ms() {
                            let _ = worker.tick(now);
                        }
                    }
                    DeskWait::Close => {
                        if let Err(err) = worker.close_city() {
                            eprintln!("the city could not write its handoff: {err}");
                        }
                        break;
                    }
                    DeskWait::Gone => break,
                }
            }
        })
        .map_err(|source| {
            AxError::failure(
                AxCode::StorageFatal,
                "start the run worker",
                source.to_string(),
            )
            .with_recovery("check process thread limits")
        })?;
    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(err),
        Err(_) => {
            return Err(AxError::failure(
                AxCode::StorageFatal,
                "start the run worker",
                "the worker ended before reporting",
            )
            .with_recovery("check the city directory and rerun"));
        }
    }
    Ok(worker_thread)
}

/// Waits for the person to stop the city from the keyboard.
///
/// A Windows console delivers two of these - Ctrl-C and Ctrl-Break - and
/// a city that closed on one and died on the other would be two
/// behaviours for one gesture, decided by which key a person happened to
/// press. Elsewhere there is one.
#[cfg(windows)]
async fn closed_by_hand() -> std::io::Result<()> {
    let mut broken = tokio::signal::windows::ctrl_break()?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = broken.recv() => Ok(()),
    }
}

#[cfg(not(windows))]
async fn closed_by_hand() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}

/// Serves one city until the person stops it, and returns when the last
/// worker has finished what it was doing.
///
/// The worker runs on its own thread and the socket never touches the
/// Ledger: a refreshed page cannot kill work, and a command is accepted
/// in one place and executed in another.
///
/// # Errors
/// Refuses before serving when the city cannot be opened — an unreadable
/// chain, a store that will not open — and propagates whatever binding
/// the address reports.
pub async fn serve(serving: Serving) -> Result<(), AxError> {
    let Serving {
        city_root,
        addr,
        token,
        client,
        vault,
        vault_notice,
        log,
        console,
    } = serving;
    let city_root = city_root.as_path();
    let token = token.as_deref();
    let cas_root = city_root.join(".sprawling").join("cas");
    std::fs::create_dir_all(&cas_root).map_err(|source| {
        AxError::failure(
            AxCode::StorageFatal,
            "prepare the upload store",
            format!("{}: {source}", cas_root.display()),
        )
        .with_recovery("check the city directory is writable")
    })?;

    let token_digest = match token {
        Some(raw) => Some(channels::PairingToken::from_configured(raw)?.digest()),
        None => None,
    };

    // The event fan-out, and the one writer that feeds it. The worker
    // owns the ledger, so a city has a single writer no matter how many
    // tabs are open; the socket tasks only read from the broadcast.
    let (events, _first) = tokio::sync::broadcast::channel(1024);
    // Increments, on their own channel. Smaller and separate: an
    // increment nobody received is nothing, and sharing the event
    // channel would let a talkative model push records out of a slow
    // reader's window.
    let (deltas, _watching) = tokio::sync::broadcast::channel(256);
    // The views the control surface reads. Rebuilt from the ledger here,
    // folded forward by the write observer inside the worker: one fold
    // rule, two call sites, no second definition of what a view means.
    let views = Arc::new(std::sync::Mutex::new(rebuild_views(&ledger_dir(
        city_root,
    ))?));
    let query_views = Arc::clone(&views);
    // Built once and handed to both surfaces below. The socket and the
    // terminal are two ways into one city, and this is the read half of
    // what makes that literally true rather than a claim.
    let answering: crate::console::Answering = Arc::new(move |query: channels::Query| {
        let mut views = query_views.lock().map_err(|_| {
            AxError::failure(
                AxCode::StorageFatal,
                "read the city views",
                "the view lock is poisoned",
            )
            .with_recovery("restart the server; its views rebuild from the ledger")
        })?;
        Ok(views.answer(&query))
    });
    // Read once, at startup, from the views the ledger just rebuilt.
    let city_name = views.lock().ok().and_then(|views| views.city());
    // The in-process Command set, not the wire one: the enrolment
    // route delivers a sealed credential here, and no wire frame can.
    let desk = Arc::new(CommandDesk::new());
    let commands_desk = Arc::clone(&desk);
    let secrets_desk = Arc::clone(&desk);
    let acp_desk = Arc::clone(&desk);
    // The one sanctioned thread besides the runtime's own, running by
    // the time this returns.
    let worker_thread = spawn_worker(
        Opening {
            city_root: city_root.to_path_buf(),
            vault,
            notice: vault_notice,
            log,
        },
        Outward {
            desk: Arc::clone(&desk),
            views: Arc::clone(&views),
            to_clients: events.clone(),
            to_watchers: deltas.clone(),
        },
    )?;

    let sink_root = cas_root.clone();
    let config = channels::ServeConfig {
        addr,
        token_digest,
        client: Arc::new(client),
        commands: Arc::new(
            move |command: channels::WireCommand, reply: channels::Reply| {
                commands_desk.post(command.into(), reply);
                Ok(())
            },
        ),
        events,
        deltas,
        city: city_name,
        secrets: Arc::new(move |command: channels::Command, reply: channels::Reply| {
            // The route waits for whichever comes first, so the
            // reply address is the credential's own request rather
            // than nowhere: a vault that refuses is a fact the
            // person typing the key needs, and it used to reach
            // nobody at all.
            secrets_desk.post(command, reply);
            Ok(())
        }),
        queries: Arc::clone(&answering),
        // An outside editor's request becomes an ordinary Dispatch on
        // the same desk a person's does. It is not a second control
        // surface: the admission decides what a stranger may learn, and
        // everything after that is the city's usual path.
        acp: Arc::new(move |body, authentic| acp_dispatch(&acp_desk, body, authentic)),
        upload_sink: Arc::new(move |bytes: Vec<u8>| {
            // Attach bytes reach the content-addressed store, and the handle
            // a later Command names is the address they landed at. Nothing
            // enters a work tree here: staging is read-only and outside every
            // WriteDomain.
            let digest = kernel::B3Hash::digest(&bytes).to_string();
            let path = sink_root.join(&digest);
            std::fs::write(&path, &bytes).map_err(|source| {
                AxError::failure(
                    AxCode::StorageFatal,
                    "stage an attachment",
                    format!("{}: {source}", path.display()),
                )
                .with_recovery("check free space under the city directory")
            })?;
            channels::UploadId::parse(&digest)
        }),
    };
    // The terminal this city is running in, if it was asked for. It gets
    // the same desk the socket posts to and the same event stream the
    // browser reads, so nothing here is a second control surface - it is
    // the first one, reached from the keyboard that started the city.
    if let Some(terminal) = console {
        let console_desk = Arc::clone(&desk);
        let watching = config.events.subscribe();
        // The same answering function the socket was given, not a second
        // one built beside it: a count this terminal prints and a count
        // a browser draws are one call, so they cannot disagree.
        crate::console::start(terminal, console_desk, Arc::clone(&answering), watching);
    }
    // Ctrl-C used to be a process death: `sprawling resume` recovered
    // it, and a stop somebody chose and a stop that was a crash left the
    // same silence in the record. The listener stops accepting first,
    // then the worker is told - it reads that where it reads its queue,
    // so whatever command is running finishes and the handoff is the
    // last line rather than a line in the middle of one.
    let served = tokio::select! {
        result = channels::serve(config) => result,
        signal = closed_by_hand() => {
            // A signal handler that cannot be installed is worth saying
            // out loud: the city keeps serving, and the person now knows
            // that Ctrl-C will be the hard stop it always was.
            signal.map_err(|source| {
                AxError::failure(
                    AxCode::StorageFatal,
                    "listen for an orderly close",
                    source.to_string(),
                )
                .with_recovery("stop the city from the console instead; /quit closes it")
            })
        }
    };
    desk.close();
    // Joined rather than left to the process exit: the handoff is
    // written by that thread, and a main that returned first would end
    // the process before the line it exists to write.
    if let Err(panicked) = worker_thread.join() {
        eprintln!("the run worker ended abnormally: {panicked:?}");
    }
    served
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

    /// **The property the whole card exists for.** A minted code has to
    /// be the code that opens the door, and the two halves reach that
    /// digest by different routes: `PairingToken::mint` hashes the text
    /// it just produced, while `serve` hashes the text it is handed back
    /// through `from_configured`. If those ever stop agreeing, an
    /// exposed city refuses the person who started it, holding a key it
    /// minted for them.
    #[test]
    fn the_key_this_city_mints_is_the_key_its_own_door_accepts() {
        let exposed = "0.0.0.0:8787".parse().expect("a literal address");
        let Keyed::Minted(code) = key_for(exposed, None).expect("this machine has entropy") else {
            panic!("an address beyond this machine with nothing configured mints one");
        };
        let digest = channels::PairingToken::from_configured(&code)
            .expect("a minted code is long enough to adopt")
            .digest();
        assert!(
            channels::verify(Some(&code), &digest),
            "the code shown to a person opens the door it guards"
        );
        assert!(!channels::verify(None, &digest), "silence is not the key");
    }

    /// One-time means one time. Two serves of the same address must not
    /// produce the same code, or a key read off yesterday's terminal
    /// still works today.
    #[test]
    fn two_serves_of_one_address_mint_two_different_keys() {
        let exposed = "0.0.0.0:8787".parse().expect("a literal address");
        let first = key_for(exposed, None).expect("entropy");
        let second = key_for(exposed, None).expect("entropy");
        assert_ne!(first.code(), second.code());
    }

    /// What the operator configured is adopted, never replaced. Minting
    /// over it would break every browser already paired with this city.
    #[test]
    fn a_configured_token_is_adopted_rather_than_replaced() {
        let exposed = "0.0.0.0:8787".parse().expect("a literal address");
        let configured = "a-token-the-operator-chose".to_owned();
        assert_eq!(
            key_for(exposed, Some(configured.clone())).expect("no entropy is drawn"),
            Keyed::Adopted(configured)
        );
    }

    /// Loopback stays frictionless: nothing is minted and nothing is
    /// asked for, which is the property `decide_bind` is written to keep.
    #[test]
    fn a_loopback_listener_is_handed_nothing_to_present() {
        let local = "127.0.0.1:8787".parse().expect("a literal address");
        let keyed = key_for(local, None).expect("no entropy is drawn");
        assert_eq!(keyed, Keyed::NothingToPresent);
        assert_eq!(keyed.code(), None);
    }
}
