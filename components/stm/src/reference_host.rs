//! The in-memory reference host (`T-005-04`): a host of the host contract (`contracts/state-machine/host/`) kept
//! as simple as it can be, the baseline both `EXP-1` candidates are compared with and the floor if neither passes
//! the host-contract suite (`DEC-6`, `REQ-OPS-3`).
//!
//! # How it keeps each guarantee
//!
//! The store, [`MemoryStore`], is a log of entries, each a transition record or a refusal, plus two flags:
//! halted, and an import not yet finished. Every write is one push onto the log, so it is atomic: an entry is in
//! the log whole or not at all. The host keeps indexes over the log (each order's records, each record id's
//! place, the refusal log) and rebuilds them from the log whenever it opens a store, so nothing it answers can
//! differ from what the log holds.
//!
//! - `durable`: a record is pushed onto the log before `submit` returns it, and `halt` sets the store's flag.
//! - `exactly-once`: a record id is looked up across the whole log; an equal input (equal `RecordedInput` values,
//!   which for these types is the same canonical JSON) returns the recorded record; any other input under the id
//!   is refused, and the refusal is pushed onto the log before it is returned.
//! - `per-order-sequence`: each order's next sequence number is its journal's length plus one, and each input is
//!   applied to the state its order's previous record reached, in submit order. Nothing is sorted.
//! - `no-effect-without-record`: nothing is returned or readable before its entry is in the log.
//! - `swap`: export streams the log as it is, between a header and an end; import checks the stream with
//!   [`StreamCheck`] as it reads, accepts only its own contract versions and an empty store, and marks the store
//!   complete only after the end.
//! - `replay`: the log is never compacted, and each record holds its input whole and the state it reached.
//!
//! # What memory can't give
//!
//! The store outlives the host value, not the process. In the suite's terms, a kill is [`Harness::kill`], which
//! drops the host's indexes and keeps its store; against that, every guarantee holds. Against a real process kill,
//! a power loss or a torn write, nothing survives, because nothing is on disk: the reference host is the in-process
//! baseline, and `EXP-1` scores durability across a real kill on the candidates only.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use rutent_contracts::state_machine::host::testing::{
    CrashPoint, FaultSeam, Harness, HarnessError,
};
use rutent_contracts::state_machine::host::{
    self as contract, Delivery, Host, HostError, RecordRef, RefusalReason, RefusalRecord,
    SnapshotEntry, SnapshotStream, StreamCheck, TransitionRecord, Version,
};
use rutent_contracts::state_machine::{
    self as machine, Id, Order, RecordId, RecordedInput, StateMachine,
};

/// One entry of the log.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Record(TransitionRecord),
    Refusal(RefusalRecord),
}

/// The reference host's store: everything that survives the host value.
///
/// Not `Clone`, on purpose: a store is moved from host to harness to host, never copied, so two hosts can't open
/// copies of one store that then drift apart. The suite moves stores only; it never needs a copy.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MemoryStore {
    log: Vec<Entry>,
    halted: bool,
    importing: bool,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        MemoryStore::default()
    }
}

/// The reference host, running the state machine `M`.
#[derive(Debug)]
pub struct ReferenceHost<M> {
    store: MemoryStore,
    /// Each order's records, in sequence order.
    journals: BTreeMap<Id, Vec<TransitionRecord>>,
    /// Orders in the order their first record was written.
    orders: Vec<Id>,
    /// Where each record id's record is.
    by_id: BTreeMap<RecordId, RecordRef>,
    /// The refusal log.
    refusals: Vec<RefusalRecord>,
    /// Fault-seam machinery: the crash point the suite set, if any. Settable only through
    /// [`FaultSeam::crash_at`]; a production caller can't reach it.
    crash: Option<CrashPoint>,
    /// Fault-seam machinery: whether the host stopped at a crash point, after which every call that could change
    /// it answers `Unavailable`. Set only by [`ReferenceHost::stop_at`].
    stopped: bool,
    machine: PhantomData<M>,
}

impl<M> ReferenceHost<M> {
    fn on_store(store: MemoryStore) -> Self {
        let mut host = ReferenceHost {
            store: MemoryStore::default(),
            journals: BTreeMap::new(),
            orders: Vec::new(),
            by_id: BTreeMap::new(),
            refusals: Vec::new(),
            crash: None,
            stopped: false,
            machine: PhantomData,
        };
        for entry in &store.log {
            host.index(entry);
        }
        host.store = store;
        host
    }

    fn index(&mut self, entry: &Entry) {
        match entry {
            Entry::Record(r) => {
                if !self.journals.contains_key(&r.order_id) {
                    self.orders.push(r.order_id.clone());
                }
                self.by_id.insert(
                    r.input.record_id.clone(),
                    RecordRef {
                        order_id: r.order_id.clone(),
                        seq: r.seq,
                    },
                );
                self.journals
                    .entry(r.order_id.clone())
                    .or_default()
                    .push(r.clone());
            }
            Entry::Refusal(r) => self.refusals.push(r.clone()),
        }
    }

    /// Writes one entry: a single push onto the log, then the indexes.
    fn write(&mut self, entry: Entry) {
        self.index(&entry);
        self.store.log.push(entry);
    }

    /// A host the harness stopped answers `Unavailable` to anything that could change it.
    fn refuse_if_stopped(&self) -> Result<(), HostError> {
        if self.stopped {
            return Err(HostError::Unavailable {
                reason: "stopped by the harness".into(),
            });
        }
        Ok(())
    }

    fn record_at(&self, at: &RecordRef) -> Option<&TransitionRecord> {
        self.journals
            .get(&at.order_id)?
            .get(usize::try_from(at.seq).ok()?.checked_sub(1)?)
    }

    /// Fault-seam machinery: stops the host if the suite set this crash point through [`FaultSeam::crash_at`].
    /// Without one set, it does nothing.
    fn stop_at(&mut self, point: CrashPoint) -> Result<(), HostError> {
        if self.crash == Some(point) {
            self.stopped = true;
            return Err(HostError::Unavailable {
                reason: format!("stopped by the harness at {point:?}"),
            });
        }
        Ok(())
    }
}

/// A count as `u64`; a `usize` count always fits on the platforms rutent builds for.
fn count(n: usize) -> u64 {
    u64::try_from(n).expect("a count fits in u64")
}

fn refused(reason: impl Into<String>) -> HostError {
    HostError::SnapshotRefused {
        reason: reason.into(),
    }
}

impl<M: StateMachine> Host<M> for ReferenceHost<M> {
    type Store = MemoryStore;

    fn open(store: MemoryStore) -> Result<Self, HostError> {
        if store.importing {
            return Err(HostError::ImportIncomplete);
        }
        Ok(ReferenceHost::on_store(store))
    }

    fn submit(&mut self, input: RecordedInput) -> Result<Delivery, HostError> {
        self.refuse_if_stopped()?;
        if self.store.halted {
            return Err(HostError::Halted);
        }
        self.stop_at(CrashPoint::AfterReceived)?;

        if let Some(at) = self.by_id.get(&input.record_id).cloned() {
            let existing = self
                .record_at(&at)
                .cloned()
                .ok_or_else(|| HostError::Unavailable {
                    reason: "a record id points to no record".into(),
                })?;
            if existing.input == input {
                return Ok(Delivery::AlreadyRecorded(existing));
            }
            let refusal = RefusalRecord {
                seq: count(self.refusals.len()) + 1,
                input,
                reason: RefusalReason::RecordIdReused,
                existing: at,
            };
            self.write(Entry::Refusal(refusal.clone()));
            return Err(HostError::RecordIdReused {
                refusal: Box::new(refusal),
            });
        }

        let order = input.subject.order.clone();
        let journal = self
            .journals
            .get(&order)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let previous = journal.iter().rev().find_map(|r| r.state_after.as_ref());
        let state_after = match previous {
            Some(state) => Some(M::transition(state, &input)),
            None => M::open(&input),
        };
        let record = TransitionRecord {
            order_id: order,
            seq: count(journal.len()) + 1,
            input,
            state_after,
        };
        self.stop_at(CrashPoint::AfterComputed)?;
        self.write(Entry::Record(record.clone()));
        self.stop_at(CrashPoint::AfterDurable)?;
        Ok(Delivery::Recorded(record))
    }

    fn orders(&self) -> Result<Vec<Id>, HostError> {
        Ok(self.orders.clone())
    }

    fn state(&self, order: &Id) -> Result<Option<Order>, HostError> {
        Ok(self
            .journals
            .get(order)
            .and_then(|j| j.iter().rev().find_map(|r| r.state_after.clone())))
    }

    fn journal(&self, order: &Id) -> Result<Vec<TransitionRecord>, HostError> {
        Ok(self.journals.get(order).cloned().unwrap_or_default())
    }

    fn refusals(&self) -> Result<Vec<RefusalRecord>, HostError> {
        Ok(self.refusals.clone())
    }

    fn halt(&mut self) -> Result<(), HostError> {
        self.refuse_if_stopped()?;
        self.store.halted = true;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), HostError> {
        self.refuse_if_stopped()?;
        self.store.halted = false;
        Ok(())
    }

    fn export(&self) -> Result<SnapshotStream<'_>, HostError> {
        if !self.store.halted {
            return Err(HostError::NotHalted);
        }
        let header = SnapshotEntry::Header {
            host_version: Version::new(contract::VERSION)
                .expect("the host contract's version is MAJOR.MINOR.PATCH"),
            state_machine_version: Version::new(machine::VERSION)
                .expect("the state-machine contract's version is MAJOR.MINOR.PATCH"),
        };
        let records = count(
            self.store
                .log
                .iter()
                .filter(|e| matches!(e, Entry::Record(_)))
                .count(),
        );
        let end = SnapshotEntry::End {
            records,
            refusals: count(self.store.log.len()) - records,
        };
        let body = self.store.log.iter().map(|e| match e {
            Entry::Record(r) => SnapshotEntry::Record(r.clone()),
            Entry::Refusal(r) => SnapshotEntry::Refusal(r.clone()),
        });
        Ok(Box::new(
            std::iter::once(header)
                .chain(body)
                .chain(std::iter::once(end))
                .map(Ok),
        ))
    }

    fn import(store: MemoryStore, stream: SnapshotStream<'_>) -> Result<Self, HostError> {
        Self::open(import_onto(store, stream, None)?)
    }
}

/// Reads a stream onto an empty store, checking it as it goes, and marks the store complete and halted after its
/// end. With a limit, stops abruptly after that many entries and leaves the store marked as an unfinished import.
///
/// It trusts each imported record's `state_after` and doesn't replay the inputs through `M`. [`StreamCheck`] and
/// the records' own checks hold the stream's shape, ids and sequences, but not that each state follows from its
/// input. That is acceptable for the baseline: the stream comes from another host the operator is swapping away
/// from, under the same contract, and replaying every journal is the `replay` guarantee, which the host-contract
/// suite checks on the host that wrote the records. Replaying on import would also bind the import to `M`'s
/// version at the time of the swap.
fn import_onto(
    mut store: MemoryStore,
    stream: SnapshotStream<'_>,
    limit: Option<u64>,
) -> Result<MemoryStore, HostError> {
    if !store.log.is_empty() || store.importing {
        return Err(refused("an import needs an empty store"));
    }
    store.importing = true;
    let mut check = StreamCheck::new();
    for (n, entry) in stream.enumerate() {
        if limit == Some(count(n)) {
            return Ok(store);
        }
        let entry = entry?;
        check.push(&entry).map_err(|e| refused(e.to_string()))?;
        match entry {
            SnapshotEntry::Header {
                host_version,
                state_machine_version,
            } => {
                if host_version.as_str() != contract::VERSION
                    || state_machine_version.as_str() != machine::VERSION
                {
                    return Err(refused(format!(
                        "this host reads host contract {} and state machine {}, not {} and {}",
                        contract::VERSION,
                        machine::VERSION,
                        host_version.as_str(),
                        state_machine_version.as_str()
                    )));
                }
            }
            SnapshotEntry::Record(r) => store.log.push(Entry::Record(r)),
            SnapshotEntry::Refusal(r) => store.log.push(Entry::Refusal(r)),
            SnapshotEntry::End { .. } => {}
        }
    }
    check.finish().map_err(|e| refused(e.to_string()))?;
    store.importing = false;
    store.halted = true;
    Ok(store)
}

/// The reference host's harness for the host-contract suite: in-process, so it implements [`FaultSeam`] too. A kill
/// drops the host's indexes and keeps its [`MemoryStore`].
#[derive(Debug, Default)]
pub struct ReferenceHarness<M> {
    machine: PhantomData<M>,
}

impl<M> ReferenceHarness<M> {
    /// A harness.
    pub fn new() -> Self {
        ReferenceHarness {
            machine: PhantomData,
        }
    }
}

impl<M: StateMachine> Harness<M> for ReferenceHarness<M> {
    type Host = ReferenceHost<M>;

    fn fresh_store(&mut self) -> Result<MemoryStore, HarnessError> {
        Ok(MemoryStore::new())
    }

    fn start(&mut self, store: MemoryStore) -> Result<ReferenceHost<M>, HarnessError> {
        Ok(ReferenceHost::open(store)?)
    }

    fn kill(&mut self, host: ReferenceHost<M>) -> Result<MemoryStore, HarnessError> {
        Ok(host.store)
    }

    fn reopen(&mut self, store: MemoryStore) -> Result<ReferenceHost<M>, HarnessError> {
        Ok(ReferenceHost::open(store)?)
    }

    fn import(
        &mut self,
        store: MemoryStore,
        stream: SnapshotStream<'_>,
    ) -> Result<ReferenceHost<M>, HarnessError> {
        Ok(ReferenceHost::import(store, stream)?)
    }

    fn import_crashing_after(
        &mut self,
        store: MemoryStore,
        stream: SnapshotStream<'_>,
        entries: u64,
    ) -> Result<MemoryStore, HarnessError> {
        Ok(import_onto(store, stream, Some(entries))?)
    }
}

impl<M: StateMachine> FaultSeam<M> for ReferenceHarness<M> {
    fn crash_at(&mut self, host: &mut ReferenceHost<M>, point: CrashPoint) {
        host.crash = Some(point);
    }
}
