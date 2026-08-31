// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Signals between residents, and the side that receives them.
//!
//! Delivery is at-least-once, so the receiving side deduplicates by the
//! signal's own id, and it does so before anything that cannot be
//! replayed: a second delivery must not bill twice or send a second
//! letter. That deduplication is the queue's `seen` set rather than a
//! table kept here, which holds only while one id always lands in one
//! lane — so a signal's lane is derived from the signal, never chosen at
//! the call site.
//!
//! Taking is the receiver's move. A sender cannot push into another
//! agent's context window; the receiver pulls at most its own bandwidth,
//! and what stands in the prefix is nothing at all — only `status`
//! reports that signals are waiting.

use kernel::{Address, Admission, AxCode, AxError, IdemKey, Payload, RunId, Seq, TimeMs, Version};
use memory::{EventQueue, QueueLane};
use serde_json::{Map, Value};

/// A signal's identity, and the thing duplicates are recognised by.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SignalId(String);

impl SignalId {
    /// # Errors
    /// Refuses an empty id and one carrying whitespace: an id is
    /// compared, logged and replayed, and all three go wrong quietly
    /// when it can contain a space.
    pub fn parse(raw: &str) -> Result<SignalId, AxError> {
        if raw.is_empty() || raw.chars().any(char::is_whitespace) {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "read a signal id",
                format!("{raw:?}"),
            )
            .with_recovery(
                "use a non-empty id with no whitespace, such as a run id and a counter",
            ));
        }
        Ok(SignalId(raw.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What kind of communication a signal is. `Steer` is a fourth kind
/// rather than a flag beside the other three: it is the only one that
/// overtakes, and urgency has to belong to the signal for one id to
/// always take one lane.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    Mention,
    Thread,
    Broadcast,
    Steer,
}

impl SignalKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SignalKind::Mention => "mention",
            SignalKind::Thread => "thread",
            SignalKind::Broadcast => "broadcast",
            SignalKind::Steer => "steer",
        }
    }

    /// # Errors
    /// Refuses a kind this version does not know.
    pub fn parse(raw: &str) -> Result<SignalKind, AxError> {
        match raw {
            "mention" => Ok(SignalKind::Mention),
            "thread" => Ok(SignalKind::Thread),
            "broadcast" => Ok(SignalKind::Broadcast),
            "steer" => Ok(SignalKind::Steer),
            other => {
                Err(
                    AxError::failure(AxCode::InvalidArgs, "read a signal kind", other.to_owned())
                        .with_recovery("mention, thread, broadcast or steer"),
                )
            }
        }
    }
}

/// Which line a signal waits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Urgent,
    Ordinary,
}

/// One communication between residents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    id: SignalId,
    kind: SignalKind,
    from: String,
    room: Address,
    room_version: Version,
    payload: Payload,
    at: TimeMs,
}

impl Signal {
    /// Sole constructor.
    ///
    /// # Errors
    /// Refuses a signal with no sender: `from` decides how the receiver
    /// renders it, and an unattributed signal renders as nobody.
    pub fn new(
        id: SignalId,
        kind: SignalKind,
        from: String,
        room: Address,
        room_version: Version,
        payload: Payload,
        at: TimeMs,
    ) -> Result<Signal, AxError> {
        if from.is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "build a signal",
                id.as_str().to_owned(),
            )
            .with_recovery("name the sender; a signal is read as coming from someone"));
        }
        Ok(Signal {
            id,
            kind,
            from,
            room,
            room_version,
            payload,
            at,
        })
    }

    #[must_use]
    pub fn id(&self) -> &SignalId {
        &self.id
    }

    #[must_use]
    pub fn kind(&self) -> SignalKind {
        self.kind
    }

    #[must_use]
    pub fn from(&self) -> &str {
        &self.from
    }

    #[must_use]
    pub fn room(&self) -> &Address {
        &self.room
    }

    /// The room version the sender saw when speaking. A held draft is
    /// judged against it (P2.05).
    #[must_use]
    pub fn room_version(&self) -> Version {
        self.room_version
    }

    #[must_use]
    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    #[must_use]
    pub fn at(&self) -> TimeMs {
        self.at
    }

    /// Urgent for a steer, ordinary for everything else. Derived rather
    /// than supplied: see the module note on deduplication.
    #[must_use]
    pub fn lane(&self) -> Lane {
        match self.kind {
            SignalKind::Steer => Lane::Urgent,
            SignalKind::Mention | SignalKind::Thread | SignalKind::Broadcast => Lane::Ordinary,
        }
    }

    /// The `signal_enqueued` record.
    ///
    /// # Errors
    /// Propagates the payload's refusal to hold what it was given.
    pub fn enqueued_payload(&self) -> Result<Payload, AxError> {
        let mut map = self.wire();
        map.insert(
            "lane".to_owned(),
            Value::String(
                match self.lane() {
                    Lane::Urgent => "urgent",
                    Lane::Ordinary => "ordinary",
                }
                .to_owned(),
            ),
        );
        Payload::new(map)
    }

    /// The `signal_consumed` record: the id and who took it, because the
    /// content is already in the enqueue record and history does not
    /// need it twice.
    ///
    /// # Errors
    /// Propagates the payload's refusal to hold what it was given.
    pub fn consumed_payload(&self, by: &str) -> Result<Payload, AxError> {
        let mut map = Map::new();
        map.insert("id".to_owned(), Value::String(self.id.as_str().to_owned()));
        map.insert("by".to_owned(), Value::String(by.to_owned()));
        Payload::new(map)
    }

    fn wire(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("id".to_owned(), Value::String(self.id.as_str().to_owned()));
        map.insert(
            "kind".to_owned(),
            Value::String(self.kind.as_str().to_owned()),
        );
        map.insert("from".to_owned(), Value::String(self.from.clone()));
        map.insert(
            "room".to_owned(),
            Value::String(self.room.as_str().to_owned()),
        );
        map.insert(
            "room_version".to_owned(),
            Value::Number(self.room_version.value().into()),
        );
        map.insert(
            "payload".to_owned(),
            Value::Object(self.payload.as_map().clone()),
        );
        map.insert("at".to_owned(), Value::Number(self.at.value().into()));
        map
    }

    /// Reads back what `enqueued_payload` wrote. The inverse is public
    /// because rebuilding the queues from the ledger is the only way the
    /// city knows what is waiting after a restart, and a second parser
    /// of this shape would be a second answer to that question.
    ///
    /// # Errors
    /// Refuses a payload missing a field or carrying a kind this version
    /// does not know.
    pub fn from_payload(payload: &Payload) -> Result<Signal, AxError> {
        Signal::from_wire(payload.as_map())
    }

    fn from_wire(map: &Map<String, Value>) -> Result<Signal, AxError> {
        let text = |key: &str| -> Result<String, AxError> {
            map.get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| broken(key))
        };
        let number = |key: &str| -> Result<u64, AxError> {
            map.get(key)
                .and_then(Value::as_u64)
                .ok_or_else(|| broken(key))
        };
        let payload = map
            .get("payload")
            .and_then(Value::as_object)
            .ok_or_else(|| broken("payload"))?;
        Signal::new(
            SignalId::parse(&text("id")?)?,
            SignalKind::parse(&text("kind")?)?,
            text("from")?,
            Address::parse(&text("room")?)?,
            Version::new(number("room_version")?),
            Payload::new(payload.clone())?,
            TimeMs::new(number("at")?),
        )
    }
}

fn broken(field: &str) -> AxError {
    AxError::failure(
        AxCode::InvalidArgs,
        "read a queued signal",
        field.to_owned(),
    )
    .with_recovery(
        "the queue writes this shape itself; a missing field means the two halves disagree",
    )
}

/// The receiving side: two lines and a bandwidth.
pub struct Inbox {
    urgent: EventQueue,
    ordinary: EventQueue,
    bandwidth: u32,
}

// Hand-written: what a reader of a failure needs is how much is waiting
// in each line, which is exactly what the queues will not print.
impl std::fmt::Debug for Inbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inbox")
            .field("urgent", &self.urgent.len())
            .field("ordinary", &self.ordinary.len())
            .field("bandwidth", &self.bandwidth)
            .finish()
    }
}

impl Inbox {
    /// `bandwidth` is how many signals one pull may take. Zero would
    /// make a receiver that never reads, so it is raised to one.
    #[must_use]
    pub fn new(capacity: u64, bandwidth: u32) -> Inbox {
        Inbox {
            urgent: EventQueue::new(QueueLane::Signal, capacity),
            ordinary: EventQueue::new(QueueLane::Signal, capacity),
            bandwidth: bandwidth.max(1),
        }
    }

    /// Delivers one signal. A repeat of an id already delivered is
    /// admitted and dropped: the sender did its job, and a refusal would
    /// only buy a retry that changes nothing.
    ///
    /// # Errors
    /// Propagates the queue's own refusal to hold the payload.
    pub fn deliver(&mut self, signal: &Signal) -> Result<Admission, AxError> {
        let key = IdemKey::derive(&RunId::CITY, Seq::FIRST, signal.id().as_str().as_bytes());
        let payload = Payload::new(signal.wire())?;
        let queue = match signal.lane() {
            Lane::Urgent => &mut self.urgent,
            Lane::Ordinary => &mut self.ordinary,
        };
        queue
            .enqueue(key, payload, signal.at())
            .map_err(memory::MemoryError::into_ax)
    }

    /// Takes up to the receiver's bandwidth, urgent first.
    ///
    /// # Errors
    /// Propagates a queued payload that does not read back as a signal.
    pub fn pull(&mut self) -> Result<Vec<Signal>, AxError> {
        let mut out = Vec::new();
        while u32::try_from(out.len()).unwrap_or(u32::MAX) < self.bandwidth {
            let item = match self.urgent.consume() {
                Some(item) => item,
                None => match self.ordinary.consume() {
                    Some(item) => item,
                    None => break,
                },
            };
            out.push(Signal::from_wire(item.payload.as_map())?);
        }
        Ok(out)
    }

    /// Takes one signal from the urgent line, or nothing when it is
    /// empty. The ordinary line is not touched.
    ///
    /// The urgent line and the steer kind are the same set — [`Signal::lane`]
    /// sends `Steer` there and nothing else — so this is how a run
    /// collects what is allowed to interrupt it without reading the mail
    /// it has not asked for yet.
    ///
    /// A queued payload that does not read back as a signal is dropped
    /// rather than returned as a failure, and the contract says so:
    /// this is called from a run's safe point, where the only
    /// alternative to moving on is stopping a run over somebody else's
    /// corrupt entry. The same payload still fails loudly through
    /// [`Inbox::pull`], which is the door the model itself uses.
    pub fn take_steer(&mut self) -> Option<Signal> {
        let item = self.urgent.consume()?;
        Signal::from_wire(item.payload.as_map()).ok()
    }

    /// What `status` reports as `signals_pending`.
    #[must_use]
    pub fn pending(&self) -> u32 {
        let total = self.urgent.len().saturating_add(self.ordinary.len());
        u32::try_from(total).unwrap_or(u32::MAX)
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

    fn signal(id: &str, kind: SignalKind) -> Signal {
        let mut body = Map::new();
        body.insert("text".to_owned(), Value::String(format!("body of {id}")));
        Signal::new(
            SignalId::parse(id).unwrap(),
            kind,
            "lab/room1".to_owned(),
            Address::parse("lab/room2").unwrap(),
            Version::new(3),
            Payload::new(body).unwrap(),
            TimeMs::new(1_000),
        )
        .unwrap()
    }

    #[test]
    fn the_same_signal_delivered_twice_is_taken_once() {
        let mut inbox = Inbox::new(16, 8);
        assert!(matches!(
            inbox.deliver(&signal("s-1", SignalKind::Mention)).unwrap(),
            Admission::Admit
        ));
        assert!(
            matches!(
                inbox.deliver(&signal("s-1", SignalKind::Mention)).unwrap(),
                Admission::Admit
            ),
            "a second delivery is the sender doing its job, not an error"
        );
        assert_eq!(inbox.pending(), 1);
        assert_eq!(inbox.pull().unwrap().len(), 1);

        // And a copy arriving after consumption is still recognised: the
        // side effect already ran once.
        inbox.deliver(&signal("s-1", SignalKind::Mention)).unwrap();
        assert!(inbox.pull().unwrap().is_empty());
    }

    #[test]
    fn a_steer_overtakes_what_was_already_waiting() {
        let mut inbox = Inbox::new(16, 8);
        inbox.deliver(&signal("s-1", SignalKind::Mention)).unwrap();
        inbox.deliver(&signal("s-2", SignalKind::Thread)).unwrap();
        inbox.deliver(&signal("s-3", SignalKind::Steer)).unwrap();

        let taken = inbox.pull().unwrap();
        let order: Vec<&str> = taken.iter().map(|s| s.id().as_str()).collect();
        assert_eq!(order, ["s-3", "s-1", "s-2"]);
    }

    #[test]
    fn a_receiver_takes_its_own_bandwidth_and_no_more() {
        let mut inbox = Inbox::new(16, 2);
        for n in 1..=5 {
            inbox
                .deliver(&signal(&format!("s-{n}"), SignalKind::Mention))
                .unwrap();
        }
        assert_eq!(inbox.pending(), 5);
        assert_eq!(inbox.pull().unwrap().len(), 2);
        assert_eq!(inbox.pending(), 3, "the rest wait; nobody pushed them out");
        assert_eq!(inbox.pull().unwrap().len(), 2);
        assert_eq!(inbox.pull().unwrap().len(), 1);
        assert!(inbox.pull().unwrap().is_empty());
    }

    #[test]
    fn a_flood_is_shed_at_the_door_and_what_is_queued_still_gets_out() {
        let mut inbox = Inbox::new(4, 8);
        let mut shed = 0;
        for n in 1..=64 {
            match inbox
                .deliver(&signal(&format!("s-{n}"), SignalKind::Mention))
                .unwrap()
            {
                Admission::Admit => {}
                Admission::Shed { .. } => shed += 1,
            }
        }
        assert!(shed > 0, "a flood is refused at the door");
        let taken = inbox.pull().unwrap();
        assert!(
            !taken.is_empty(),
            "shedding refuses new items and never starves the queued ones"
        );
    }

    #[test]
    fn a_signal_survives_the_queue_byte_for_byte() {
        let mut inbox = Inbox::new(16, 8);
        let sent = signal("s-1", SignalKind::Thread);
        inbox.deliver(&sent).unwrap();
        let taken = inbox.pull().unwrap();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0], sent);
    }

    #[test]
    fn the_records_say_which_line_it_took_and_who_took_it() {
        let sent = signal("s-1", SignalKind::Steer);
        let enqueued = sent.enqueued_payload().unwrap();
        assert_eq!(
            enqueued.as_map().get("lane").and_then(Value::as_str),
            Some("urgent")
        );
        assert_eq!(
            enqueued.as_map().get("kind").and_then(Value::as_str),
            Some("steer")
        );

        let consumed = sent.consumed_payload("lab/room2").unwrap();
        assert_eq!(consumed.as_map().len(), 2);
        assert_eq!(
            consumed.as_map().get("by").and_then(Value::as_str),
            Some("lab/room2")
        );
    }

    #[test]
    fn a_signal_from_nobody_is_not_a_signal() {
        let err = Signal::new(
            SignalId::parse("s-1").unwrap(),
            SignalKind::Mention,
            String::new(),
            Address::parse("lab").unwrap(),
            Version::new(1),
            Payload::empty(),
            TimeMs::new(1),
        )
        .unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(SignalId::parse("s 1").is_err());
        assert!(SignalId::parse("").is_err());
    }
}
