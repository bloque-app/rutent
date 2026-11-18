//! The host contract: what any runtime that runs the state machine must guarantee (`DEC-3`, ADR-0002). Durable
//! transition storage, at most one record per record id, one order of application per order, no effect without
//! its record, replacing the execution environment without losing an order in flight, and replay.
//!
//! The contract is runtime-neutral: a durable store, VOS, or anything else meets it through [`Host`]. The
//! host-contract suite (`T-005-03`) scores each candidate (`DEC-5`, `DEC-6`) through [`Host`], and injects faults
//! in-process through [`testing::FaultSeam`] or, for a runtime in another process, by killing that process. Each
//! guarantee is listed in [`GUARANTEES`] with the fault that tests it; `contracts/state-machine/host/README.md`
//! holds the same table.
//!
//! Only what crosses a process boundary has a schema: a [`TransitionRecord`], a [`RefusalRecord`], and the
//! [`SnapshotEntry`] stream a swap carries. As in the state-machine contract, deserialising a record runs its
//! `check`, and a whole stream is checked by [`StreamCheck`], so Rust accepts nothing the schemas reject.
//!
//! # The life of one input
//!
//! A submitted input passes four points, in order:
//!
//! 1. **Received**: the host has the input. Nothing is recorded.
//! 2. **Computed**: the host has called [`StateMachine::open`] (the order has no state yet) or
//!    [`StateMachine::transition`] and holds the result in memory. Nothing is recorded.
//! 3. **Durable**: the [`TransitionRecord`] (the input whole, its sequence number and the state it reached) is in
//!    the store. A write is atomic: after a crash the record is whole or absent.
//! 4. **Acknowledged**: [`Host::submit`] returns the record. No record leaves the host, by `submit` or by any read,
//!    before it is durable; leaving the host is its *release*.
//!
//! An *effect* is anything done because of a transition, such as calling a provider. It is driven only from a
//! released record, so every effect has its record (`INV-7`). What happens to effects after that is not the
//! host's to guarantee, and not scored through [`Host`]: it is the effect driver's obligation, [`EFFECT_DRIVER`].

use alloc::collections::BTreeMap;
use alloc::{boxed::Box, string::String, vec::Vec};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use super::{Id, Order, RecordId, RecordedInput, StateMachine, Violation};

pub mod testing;

/// The contract's version: the first `## ` heading of `contracts/state-machine/host/CHANGELOG.md`.
pub const VERSION: &str = "3.0.0";

/// The largest sequence number, 2^53 - 1, so it survives RFC 8785 canonical JSON, which writes numbers as IEEE
/// doubles: a transition record's `seq`, a refusal record's `seq`, and the `seq` a refusal points to.
pub const MAX_SEQ: u64 = (1 << 53) - 1;

/// The effect driver's obligation: what whoever acts on released records owes, per record id. A record can be
/// released more than once (a resubmit after a crash, a read), so this, not the host, keeps a repeat from moving
/// value twice (`INV-8`). It is scored by `REQ-CAT-10`'s own test in a later milestone, not by the host-contract
/// suite.
pub const EFFECT_DRIVER: &str = "An effect driver drives effects only from released records. It makes at least \
                                 one attempt per record id, however often the record is released, and never \
                                 repeats an attempt that was confirmed. Without safe_retry, it makes at most \
                                 one attempt per record id; an unconfirmed attempt is never repeated \
                                 automatically: the segment becomes outcome unknown (REQ-ORD-1) and waits for \
                                 the operator.";

/// One guarantee of the contract, with the ids it serves and how the host-contract suite tests it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guarantee {
    /// A short, stable name.
    pub id: &'static str,
    /// What every host guarantees.
    pub statement: &'static str,
    /// The SPEC ids it serves.
    pub serves: &'static [&'static str],
    /// The fault or check `T-005-03` uses to test it.
    pub suite: &'static str,
}

/// Every guarantee of the contract.
pub const GUARANTEES: [Guarantee; 6] = [
    Guarantee {
        id: "durable",
        statement: "Once submit returns a record or a refusal, its entry survives the host stopped abruptly at any \
                    point and its store reopened. A write is atomic: after a crash an entry is whole or absent. A \
                    record that was durable but not yet acknowledged when the host stopped is present after \
                    reopening, unacknowledged; submitting the same input again returns it as already recorded, \
                    which releases it. Being halted is durable: a halted host reopens halted. Nothing that is not \
                    yet durable can be read.",
        serves: &["INV-7", "NFR-5"],
        suite: "Stop the host after each crash point, reopen its store, and compare its journals with the records \
                acknowledged before the stop; the record in flight at after-durable is present. Halt, stop and \
                reopen: the host is still halted.",
    },
    Guarantee {
        id: "exactly-once",
        statement: "The host holds at most one record per record id. Two inputs are the same when their \
                    RecordedInput values are equal, that is, the same canonical JSON under RFC 8785 (JCS). A \
                    retry resends the input unchanged, recorded_at included; an input stamped again with another \
                    time is a different input. Submitting an input equal to a recorded one returns that record \
                    and records nothing. A retry after a crash before \
                    durability may call the state machine again, which is harmless because it is pure, but never \
                    makes a second record. A different input under a used record id is refused by name, and the \
                    refusal is itself recorded. Halted and record-id-reused are definite refusals; every other \
                    error leaves the outcome unknown, and submitting the same input again is always safe.",
        serves: &["REQ-STM-4", "INV-8", "REQ-AUD-1"],
        suite: "Repeat a delivery before and after a crash and after a swap: the host holds one record for it and \
                no state moves twice. Reuse a record id with other content: it is refused, and the refusal is in \
                the refusal log.",
    },
    Guarantee {
        id: "per-order-sequence",
        statement: "Each order's records carry sequence numbers 1, 2, 3 and so on, with no gap, in the order the \
                    host applied them, each to the state the previous one reached. Submits for one order are \
                    applied one at a time. The host never reorders inputs, by recorded_at or otherwise; nothing \
                    is promised between orders.",
        serves: &["REQ-STM-3", "REQ-STM-4"],
        suite: "Deliver one order's inputs out of their recorded order: the journal is gapless, in submit order, \
                and each record's state follows from the one before.",
    },
    Guarantee {
        id: "no-effect-without-record",
        statement: "A record is released, to the submitter or to any read, only once it is durable. A crash \
                    before the record is durable leaves no record and nothing released to act on.",
        serves: &["INV-7", "NFR-5"],
        suite: "Stop the host after received and after computed: after reopening there is no record, no state \
                change and nothing released, and submitting again applies the input once. Stop it after durable: \
                after reopening the record is present and unacknowledged, and submitting again releases it once, \
                as already recorded.",
    },
    Guarantee {
        id: "swap",
        statement: "The execution environment is replaced by halting the host, exporting its snapshot stream, \
                    importing the stream into another implementation on an empty store, and resuming. A submit \
                    is either recorded before the halt or refused with halted, never half applied. After the \
                    import every order's state and journal, the refusal log and the set of record ids seen are \
                    the same, and the same further inputs reach the same states. The stream names both \
                    contracts' versions, and an import refuses one it can't read. A crash during an import leaves \
                    a store that open refuses as incomplete; the import is redone on a fresh store. This version \
                    allows a swap that halts the host; a later one may add a live swap.",
        serves: &["REQ-OPS-3", "NFR-6", "REQ-STM-4"],
        suite: "Swap mid-order between two implementations: no order is lost or duplicated, a repeat of an input \
                from before the swap is still recognised, and the order runs to its end on the new host. Stop \
                the new host during the import: open refuses its store.",
    },
    Guarantee {
        id: "replay",
        statement: "Each order's journal is complete and never compacted. It holds every input whole, its source \
                    and time included, and the state it reached, and orders lists every order with a record. \
                    Folding open and transition over a journal's inputs, in sequence order, reaches every \
                    recorded state.",
        serves: &["REQ-STM-3", "NFR-4", "REQ-AUD-1"],
        suite: "Replay each journal of each listed order through the state machine alone and compare every state \
                with the recorded one.",
    },
];

/// One input as the host recorded it: the input whole, its place in its order's sequence, and the state it
/// reached. Schema: `urn:rutent:host:transition-record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct TransitionRecord {
    /// The order the input is about: its subject's order.
    pub order_id: Id,
    /// Its place in the order's sequence, from 1, with no gap, at most [`MAX_SEQ`].
    pub seq: u64,
    /// The input, exactly as submitted.
    pub input: RecordedInput,
    /// The state the input reached. Absent only for an input that opened no order and came before any that did;
    /// such an input is still recorded (`REQ-STM-6`, `REQ-AUD-1`).
    #[serde(
        default,
        deserialize_with = "some_order",
        skip_serializing_if = "Option::is_none"
    )]
    pub state_after: Option<Order>,
}

fn some_order<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Order>, D::Error> {
    Order::deserialize(deserializer).map(Some)
}

impl TransitionRecord {
    /// Checks the rules a field type can't carry: the sequence starts at 1 and is at most [`MAX_SEQ`]; the
    /// record, its input's subject and its state name the same order; and the state names this input as its last.
    pub fn check(&self) -> Result<(), Violation> {
        if self.seq == 0 {
            return Err(Violation("sequence numbers start at 1"));
        }
        if self.seq > MAX_SEQ {
            return Err(Violation("a sequence number is at most 2^53 - 1"));
        }
        if self.input.subject.order != self.order_id {
            return Err(Violation("a record is about its input's order"));
        }
        if let Some(state) = &self.state_after {
            if state.order_id != self.order_id {
                return Err(Violation("a record's state is its own order's"));
            }
            if state.last_input.record_id != self.input.record_id {
                return Err(Violation(
                    "a record's state names the record's input as its last",
                ));
            }
        }
        Ok(())
    }
}

impl Serialize for TransitionRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TransitionRecord::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for TransitionRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let record = TransitionRecord::deserialize(deserializer)?;
        record.check().map_err(D::Error::custom)?;
        Ok(record)
    }
}

/// Where a record is: its order and its place in that order's sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordRef {
    /// The order.
    pub order_id: Id,
    /// The record's sequence number.
    pub seq: u64,
}

/// Why the host refused an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    /// The record id is already recorded with other content.
    RecordIdReused,
}

/// A refused input, recorded whole in the host's refusal log (`REQ-AUD-1`: refusals are recorded). Schema:
/// `urn:rutent:host:refusal-record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct RefusalRecord {
    /// Its place in the host's refusal log, from 1, with no gap, at most [`MAX_SEQ`].
    pub seq: u64,
    /// The refused input, exactly as submitted.
    pub input: RecordedInput,
    /// Why it was refused.
    pub reason: RefusalReason,
    /// The record that already holds the input's record id.
    pub existing: RecordRef,
}

impl RefusalRecord {
    /// Checks the rules a field type can't carry: both sequence numbers start at 1 and are at most [`MAX_SEQ`].
    /// That the record it points to
    /// holds its record id is checked over a whole stream, by [`StreamCheck::finish`].
    pub fn check(&self) -> Result<(), Violation> {
        if self.seq == 0 || self.existing.seq == 0 {
            return Err(Violation("sequence numbers start at 1"));
        }
        if self.seq > MAX_SEQ || self.existing.seq > MAX_SEQ {
            return Err(Violation("a sequence number is at most 2^53 - 1"));
        }
        Ok(())
    }
}

impl Serialize for RefusalRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RefusalRecord::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for RefusalRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let refusal = RefusalRecord::deserialize(deserializer)?;
        refusal.check().map_err(D::Error::custom)?;
        Ok(refusal)
    }
}

/// One entry of a snapshot stream, the runtime-neutral form a swap carries, one JSON value per line. Schema:
/// `urn:rutent:host:snapshot-entry`.
///
/// A stream is a [`SnapshotEntry::Header`], then every record and every refusal, each order's records in sequence
/// order and the refusals in refusal-log order, then a [`SnapshotEntry::End`] with the counts. [`StreamCheck`]
/// checks a whole stream as it passes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "entry",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SnapshotEntry {
    /// The first entry: the versions the stream was written under.
    Header {
        /// The host contract version.
        host_version: Version,
        /// The state-machine contract version.
        state_machine_version: Version,
    },
    /// One transition record.
    Record(TransitionRecord),
    /// One refusal record.
    Refusal(RefusalRecord),
    /// The last entry: how many records and refusals came before it, so a cut-short stream is told apart.
    End {
        /// The number of records.
        records: u64,
        /// The number of refusals.
        refusals: u64,
    },
}

/// A contract version, `MAJOR.MINOR.PATCH`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Version(String);

impl Version {
    /// The version, or `None` if the text isn't `MAJOR.MINOR.PATCH`.
    pub fn new(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        let parts: Vec<&str> = text.split('.').collect();
        let valid = parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        valid.then_some(Version(text))
    }

    /// The version as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Version {
    type Error = Violation;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Version::new(text).ok_or(Violation("a version is MAJOR.MINOR.PATCH"))
    }
}

impl From<Version> for String {
    fn from(value: Version) -> Self {
        value.0
    }
}

/// Checks a snapshot stream entry by entry, holding only each order's last sequence number and the record ids
/// seen. An import runs it over the stream it reads; [`StreamCheck::finish`] fails a stream with no end.
#[derive(Debug, Default)]
pub struct StreamCheck {
    header: bool,
    ended: bool,
    last_seq: BTreeMap<Id, (u64, bool)>,
    record_ids: BTreeMap<RecordId, RecordRef>,
    refusals: Vec<RecordRef>,
    refusal_ids: Vec<RecordId>,
}

impl StreamCheck {
    /// A check before the first entry.
    pub fn new() -> Self {
        StreamCheck::default()
    }

    /// Checks the next entry, running each record's and refusal's own `check`, so an entry built in Rust is
    /// held to the same rules as one deserialised.
    pub fn push(&mut self, entry: &SnapshotEntry) -> Result<(), Violation> {
        if self.ended {
            return Err(Violation("nothing follows a stream's end"));
        }
        match entry {
            SnapshotEntry::Header { .. } => {
                if self.header {
                    return Err(Violation("a stream has one header"));
                }
                self.header = true;
                return Ok(());
            }
            _ if !self.header => return Err(Violation("a stream starts with its header")),
            SnapshotEntry::Record(record) => {
                record.check()?;
                let (last, opened) = self
                    .last_seq
                    .entry(record.order_id.clone())
                    .or_insert((0, false));
                if record.seq != *last + 1 {
                    return Err(Violation(
                        "each order's records run from 1 with no gap, in sequence order",
                    ));
                }
                if *opened && record.state_after.is_none() {
                    return Err(Violation("once a record has a state, every later one does"));
                }
                *last = record.seq;
                *opened |= record.state_after.is_some();
                let at = RecordRef {
                    order_id: record.order_id.clone(),
                    seq: record.seq,
                };
                if self
                    .record_ids
                    .insert(record.input.record_id.clone(), at)
                    .is_some()
                {
                    return Err(Violation("a record id is recorded once in the whole host"));
                }
            }
            SnapshotEntry::Refusal(refusal) => {
                refusal.check()?;
                if refusal.seq != self.refusals.len() as u64 + 1 {
                    return Err(Violation(
                        "the refusal log runs from 1 with no gap, in order",
                    ));
                }
                self.refusals.push(refusal.existing.clone());
                self.refusal_ids.push(refusal.input.record_id.clone());
            }
            SnapshotEntry::End { records, refusals } => {
                if *records != self.record_ids.len() as u64
                    || *refusals != self.refusals.len() as u64
                {
                    return Err(Violation(
                        "a stream's end counts the records and refusals before it",
                    ));
                }
                self.ended = true;
            }
        }
        Ok(())
    }

    /// Checks that the stream ended, and that every refusal points to the record holding its record id.
    pub fn finish(self) -> Result<(), Violation> {
        if !self.ended {
            return Err(Violation("a stream ends with its end entry"));
        }
        for (existing, id) in self.refusals.iter().zip(&self.refusal_ids) {
            if self.record_ids.get(id) != Some(existing) {
                return Err(Violation(
                    "a refusal points to the record that holds its record id",
                ));
            }
        }
        Ok(())
    }
}

/// What a submit did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// The input was new: this is its record, now durable.
    Recorded(TransitionRecord),
    /// An equal input was already recorded: this is that record, and nothing was recorded.
    AlreadyRecorded(TransitionRecord),
}

/// Why a host refused or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// Definite: the record id is already recorded with other content. Nothing was applied; this refusal is
    /// recorded in the refusal log.
    RecordIdReused {
        /// The refusal's entry in the refusal log.
        refusal: Box<RefusalRecord>,
    },
    /// Definite: the host is halted. Nothing was recorded.
    Halted,
    /// An export was asked of a host that isn't halted.
    NotHalted,
    /// An import was refused: a version this host can't read, a stream that fails [`StreamCheck`], or a store
    /// that isn't empty.
    SnapshotRefused {
        /// Why.
        reason: String,
    },
    /// The store holds an import that didn't finish; it can't be opened, and the import is redone on a fresh
    /// store.
    ImportIncomplete,
    /// The store failed, or the host was stopped part-way (see [`testing`]). The outcome is unknown; submitting
    /// again is safe.
    Unavailable {
        /// What failed.
        reason: String,
    },
}

/// The entries of a snapshot stream, as a host exports them or an import reads them.
pub type SnapshotStream<'a> = Box<dyn Iterator<Item = Result<SnapshotEntry, HostError>> + 'a>;

/// A runtime that runs the state machine `M` against a durable store.
///
/// Every implementation meets every guarantee of [`GUARANTEES`]. Submits take `&mut self`, so they are applied
/// one at a time; an implementation shared between threads serialises one order's submits itself.
pub trait Host<M: StateMachine>: Sized {
    /// The durable store: what survives the host.
    type Store;

    /// Opens a host on a store, recovering everything durable in it, halted if it was halted. A store holding an
    /// unfinished import is refused with [`HostError::ImportIncomplete`].
    fn open(store: Self::Store) -> Result<Self, HostError>;

    /// Records and applies one input: the order's state is [`StateMachine::open`]ed if it has none yet, or
    /// moved by [`StateMachine::transition`], and the record is durable before it is returned. A different input
    /// under a used record id is refused, and the refusal is durable before it is returned.
    fn submit(&mut self, input: RecordedInput) -> Result<Delivery, HostError>;

    /// Every order with at least one record.
    fn orders(&self) -> Result<Vec<Id>, HostError>;

    /// The order's current state: the state of its latest record that has one.
    fn state(&self, order: &Id) -> Result<Option<Order>, HostError>;

    /// The order's journal, complete and in sequence order; empty for an order with no record.
    fn journal(&self, order: &Id) -> Result<Vec<TransitionRecord>, HostError>;

    /// The refusal log, complete and in order.
    fn refusals(&self) -> Result<Vec<RefusalRecord>, HostError>;

    /// Stops accepting submits, durably: from here every submit is refused with [`HostError::Halted`], across
    /// any reopening, until [`Host::resume`].
    fn halt(&mut self) -> Result<(), HostError>;

    /// Accepts submits again, durably.
    fn resume(&mut self) -> Result<(), HostError>;

    /// Everything the host holds, as a snapshot stream, for a swap. Only a halted host exports
    /// ([`HostError::NotHalted`]).
    fn export(&self) -> Result<SnapshotStream<'_>, HostError>;

    /// A halted host on an empty store, holding exactly what the stream holds. The stream is checked with
    /// [`StreamCheck`] as it is read, and the import is marked complete only after its end entry.
    fn import(store: Self::Store, stream: SnapshotStream<'_>) -> Result<Self, HostError>;
}
