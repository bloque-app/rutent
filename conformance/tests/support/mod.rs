//! A minimal in-memory test double of a host, for the suite's own tests only. It is not the reference host
//! (`T-005-04`): it keeps the store as a plain list, rebuilds every index by scanning, and exists to show each cell
//! passing on a correct host and failing on a broken one. Each [`Flaw`] breaks one guarantee and keeps the double
//! contract-valid otherwise.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use rutent_contracts::state_machine::host::testing::{
    CrashPoint, FaultSeam, Harness, HarnessError,
};
use rutent_contracts::state_machine::host::{
    Delivery, Host, HostError, RecordRef, RefusalReason, RefusalRecord, SnapshotEntry,
    SnapshotStream, StreamCheck, TransitionRecord, VERSION, Version,
};
use rutent_contracts::state_machine::{
    Id, Order, RecordedInput, StateMachine, Timestamp, VERSION as MACHINE_VERSION,
};

/// The one guarantee a broken double breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flaw {
    /// A correct double.
    None,
    /// Acknowledges a record while it is only in memory, writing it to the store at the next submit; a kill loses
    /// it. Breaks `no-effect-without-record`.
    AcknowledgesBeforeDurable,
    /// Records a repeat as a new record, with the next sequence number. Breaks `exactly-once`.
    RecordsARepeatTwice,
    /// Exports a valid stream without its last order. Breaks `swap`.
    LosesAnOrderOnSwap,
    /// Records each state as if its input were applied twice; the record is still valid. Breaks `replay`.
    ReplayDiverges,
    /// Writes an order's new state to a durable cache before its record, and reads state from the cache. Breaks
    /// `durable` and `no-effect-without-record`: a stop in between leaves a state with no record.
    CachesStateBeforeRecord,
    /// Keeps the refusal log in memory; a kill loses it. Breaks `exactly-once` (the refusal is recorded).
    KeepsRefusalsInMemory,
    /// Leaves `recorded_at` out of input equality, so an input stamped again counts as a repeat. Breaks
    /// `exactly-once`.
    IgnoresRecordedAt,
    /// Looks a record id up only within the input's order. Breaks `exactly-once`.
    DedupsPerOrder,
    /// Numbers records with one counter across all orders. Breaks `per-order-sequence`. Its gapped per-order
    /// sequences also fail every cell that compares journals by sequence number or carries them through a stream,
    /// `stop-during-import-is-refused-at-open` included.
    OneSequenceAcrossOrders,
    /// Imports any stream onto any store, without checking it. Breaks `swap`.
    AcceptsAnyImport,
    /// Answers a submit while halted with `Halted`, but keeps it and applies it on resume. Breaks `swap`.
    QueuesSubmitsWhileHalted,
    /// Answers a submit while halted with `Halted`, but keeps it and applies it before the next submit after
    /// resume. Breaks `swap`.
    AppliesRefusedSubmitLater,
    /// Restamps the in-flight input's `recorded_at` when it is stopped after durable, so the record holds another
    /// input under the same record id. Breaks `durable`.
    RewritesTheInputAtAStop,
    /// After an import stopped part-way, the next import loses the refusal log. Breaks `swap`.
    DropsRefusalsAfterAStoppedImport,
    /// Exports only records that reached a state, dropping an input that opened nothing. Breaks `swap`.
    DropsStatelessRecordsOnExport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Record(TransitionRecord),
    Refusal(RefusalRecord),
}

/// What survives the double: its entries, its state cache, whether it is halted, and whether an import is
/// unfinished.
#[derive(Debug, Clone)]
pub struct Store {
    flaw: Flaw,
    entries: Vec<Entry>,
    state_cache: BTreeMap<Id, Order>,
    halted: bool,
    importing: bool,
}

/// The double host.
pub struct Double<M> {
    store: Store,
    unflushed: Vec<TransitionRecord>,
    memory_refusals: Vec<RefusalRecord>,
    queued: Vec<RecordedInput>,
    crash: Option<CrashPoint>,
    stopped: bool,
    machine: PhantomData<M>,
}

impl<M> Double<M> {
    fn flaw(&self) -> Flaw {
        self.store.flaw
    }

    fn records(&self) -> impl Iterator<Item = &TransitionRecord> {
        self.store
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Record(r) => Some(r),
                Entry::Refusal(_) => None,
            })
            .chain(&self.unflushed)
    }

    fn refusal_log(&self) -> Vec<RefusalRecord> {
        self.store
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Refusal(r) => Some(r.clone()),
                Entry::Record(_) => None,
            })
            .chain(self.memory_refusals.iter().cloned())
            .collect()
    }

    fn flush(&mut self) {
        let pending = std::mem::take(&mut self.unflushed);
        self.store
            .entries
            .extend(pending.into_iter().map(Entry::Record));
    }

    fn stop_if(&mut self, point: CrashPoint) -> Result<(), HostError> {
        if self.crash == Some(point) {
            self.stopped = true;
            return Err(HostError::Unavailable {
                reason: format!("stopped at {point:?}"),
            });
        }
        Ok(())
    }

    fn journal_of(&self, order: &Id) -> Vec<TransitionRecord> {
        self.records()
            .filter(|r| &r.order_id == order)
            .cloned()
            .collect()
    }

    fn same(&self, a: &RecordedInput, b: &RecordedInput) -> bool {
        if self.flaw() == Flaw::IgnoresRecordedAt {
            let mut b = b.clone();
            b.recorded_at = a.recorded_at.clone();
            return a == &b;
        }
        a == b
    }
}

fn unavailable(reason: &str) -> HostError {
    HostError::Unavailable {
        reason: reason.into(),
    }
}

fn refused(reason: impl Into<String>) -> HostError {
    HostError::SnapshotRefused {
        reason: reason.into(),
    }
}

impl<M: StateMachine> Host<M> for Double<M> {
    type Store = Store;

    fn open(store: Store) -> Result<Self, HostError> {
        if store.importing {
            return Err(HostError::ImportIncomplete);
        }
        Ok(Double {
            store,
            unflushed: Vec::new(),
            memory_refusals: Vec::new(),
            queued: Vec::new(),
            crash: None,
            stopped: false,
            machine: PhantomData,
        })
    }

    fn submit(&mut self, input: RecordedInput) -> Result<Delivery, HostError> {
        if self.stopped {
            return Err(unavailable("stopped"));
        }
        if self.store.halted {
            if matches!(
                self.flaw(),
                Flaw::QueuesSubmitsWhileHalted | Flaw::AppliesRefusedSubmitLater
            ) {
                self.queued.push(input);
            }
            return Err(HostError::Halted);
        }
        if self.flaw() == Flaw::AppliesRefusedSubmitLater {
            for kept in std::mem::take(&mut self.queued) {
                Host::<M>::submit(self, kept)?;
            }
        }
        self.stop_if(CrashPoint::AfterReceived)?;
        let existing = self
            .records()
            .filter(|r| self.flaw() != Flaw::DedupsPerOrder || r.order_id == input.subject.order)
            .find(|r| r.input.record_id == input.record_id)
            .cloned();
        if let Some(existing) = existing {
            if !self.same(&existing.input, &input) {
                let refusal = RefusalRecord {
                    seq: self.refusal_log().len() as u64 + 1,
                    input,
                    reason: RefusalReason::RecordIdReused,
                    existing: RecordRef {
                        order_id: existing.order_id,
                        seq: existing.seq,
                    },
                };
                self.flush();
                if self.flaw() == Flaw::KeepsRefusalsInMemory {
                    self.memory_refusals.push(refusal.clone());
                } else {
                    self.store.entries.push(Entry::Refusal(refusal.clone()));
                }
                return Err(HostError::RecordIdReused {
                    refusal: Box::new(refusal),
                });
            }
            if self.flaw() != Flaw::RecordsARepeatTwice {
                return Ok(Delivery::AlreadyRecorded(existing));
            }
        }
        let order = input.subject.order.clone();
        let journal = self.journal_of(&order);
        let previous: Option<Order> = journal.iter().rev().find_map(|r| r.state_after.clone());
        let mut state_after = match &previous {
            Some(state) => Some(M::transition(state, &input)),
            None => M::open(&input),
        };
        if self.flaw() == Flaw::ReplayDiverges {
            state_after = state_after.map(|s| M::transition(&s, &input));
        }
        let seq = match self.flaw() {
            Flaw::OneSequenceAcrossOrders => self.records().count(),
            _ => journal.len(),
        } as u64
            + 1;
        let record = TransitionRecord {
            order_id: order,
            seq,
            input,
            state_after,
        };
        if let (Flaw::CachesStateBeforeRecord, Some(state)) = (self.flaw(), &record.state_after) {
            self.store
                .state_cache
                .insert(record.order_id.clone(), state.clone());
        }
        self.stop_if(CrashPoint::AfterComputed)?;
        let mut record = record;
        if self.flaw() == Flaw::RewritesTheInputAtAStop
            && self.crash == Some(CrashPoint::AfterDurable)
        {
            record.input.recorded_at =
                Timestamp::new("2026-11-08T00:00:00Z").expect("an RFC 3339 time");
        }
        if self.flaw() == Flaw::AcknowledgesBeforeDurable {
            self.flush();
            self.unflushed.push(record.clone());
        } else {
            self.store.entries.push(Entry::Record(record.clone()));
        }
        self.stop_if(CrashPoint::AfterDurable)?;
        Ok(Delivery::Recorded(record))
    }

    fn orders(&self) -> Result<Vec<Id>, HostError> {
        let mut orders: Vec<Id> = Vec::new();
        for r in self.records() {
            if !orders.contains(&r.order_id) {
                orders.push(r.order_id.clone());
            }
        }
        Ok(orders)
    }

    fn state(&self, order: &Id) -> Result<Option<Order>, HostError> {
        if self.flaw() == Flaw::CachesStateBeforeRecord {
            return Ok(self.store.state_cache.get(order).cloned());
        }
        Ok(self
            .journal_of(order)
            .into_iter()
            .rev()
            .find_map(|r| r.state_after))
    }

    fn journal(&self, order: &Id) -> Result<Vec<TransitionRecord>, HostError> {
        Ok(self.journal_of(order))
    }

    fn refusals(&self) -> Result<Vec<RefusalRecord>, HostError> {
        Ok(self.refusal_log())
    }

    fn halt(&mut self) -> Result<(), HostError> {
        self.flush();
        self.store.halted = true;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), HostError> {
        self.store.halted = false;
        if self.flaw() == Flaw::QueuesSubmitsWhileHalted {
            for input in std::mem::take(&mut self.queued) {
                Host::<M>::submit(self, input)?;
            }
        }
        Ok(())
    }

    fn export(&self) -> Result<SnapshotStream<'_>, HostError> {
        if !self.store.halted {
            return Err(HostError::NotHalted);
        }
        let dropped = match self.flaw() {
            Flaw::LosesAnOrderOnSwap => Host::<M>::orders(self)?
                .into_iter()
                .rev()
                .find(|o| self.journal_of(o).iter().any(|r| r.state_after.is_some())),
            _ => None,
        };
        let mut kept: Vec<Entry> = self
            .store
            .entries
            .iter()
            .filter(|e| match (e, &dropped) {
                (Entry::Record(r), Some(d)) => &r.order_id != d,
                (Entry::Record(r), None) => {
                    self.flaw() != Flaw::DropsStatelessRecordsOnExport || r.state_after.is_some()
                }
                _ => true,
            })
            .cloned()
            .collect();
        kept.extend(self.memory_refusals.iter().cloned().map(Entry::Refusal));
        let records = kept
            .iter()
            .filter(|e| matches!(e, Entry::Record(_)))
            .count() as u64;
        let refusals = kept.len() as u64 - records;
        let header = SnapshotEntry::Header {
            host_version: Version::new(VERSION).expect("a version"),
            state_machine_version: Version::new(MACHINE_VERSION).expect("a version"),
        };
        let body = kept.into_iter().map(|e| match e {
            Entry::Record(r) => SnapshotEntry::Record(r),
            Entry::Refusal(r) => SnapshotEntry::Refusal(r),
        });
        let end = SnapshotEntry::End { records, refusals };
        Ok(Box::new(
            std::iter::once(header)
                .chain(body)
                .chain(std::iter::once(end))
                .map(Ok),
        ))
    }

    fn import(store: Store, stream: SnapshotStream<'_>) -> Result<Self, HostError> {
        let store = import_entries(store, stream, None)?;
        Self::open(store)
    }
}

/// Reads a stream onto an empty store, checking it as it goes; with a limit, stops abruptly after that many entries.
fn import_entries(
    mut store: Store,
    stream: SnapshotStream<'_>,
    limit: Option<u64>,
) -> Result<Store, HostError> {
    let checks = store.flaw != Flaw::AcceptsAnyImport;
    if checks && !store.entries.is_empty() {
        return Err(refused("the store isn't empty"));
    }
    store.importing = true;
    let mut check = StreamCheck::new();
    for (n, entry) in stream.enumerate() {
        if limit == Some(n as u64) {
            return Ok(store);
        }
        let entry = entry?;
        if checks {
            check.push(&entry).map_err(|e| refused(e.to_string()))?;
        }
        match entry {
            SnapshotEntry::Header {
                host_version,
                state_machine_version,
            } if checks
                && (host_version.as_str() != VERSION
                    || state_machine_version.as_str() != MACHINE_VERSION) =>
            {
                return Err(refused(format!(
                    "can't read host {} / state machine {}",
                    host_version.as_str(),
                    state_machine_version.as_str()
                )));
            }
            SnapshotEntry::Record(r) => {
                if let Some(state) = &r.state_after {
                    store.state_cache.insert(r.order_id.clone(), state.clone());
                }
                store.entries.push(Entry::Record(r));
            }
            SnapshotEntry::Refusal(r) => store.entries.push(Entry::Refusal(r)),
            SnapshotEntry::Header { .. } | SnapshotEntry::End { .. } => {}
        }
    }
    if checks {
        check.finish().map_err(|e| refused(e.to_string()))?;
    }
    store.importing = false;
    store.halted = true;
    Ok(store)
}

/// The double's harness. In-process, so it implements [`FaultSeam`] too.
pub struct DoubleHarness<M> {
    flaw: Flaw,
    stopped_import: bool,
    machine: PhantomData<M>,
}

impl<M> DoubleHarness<M> {
    /// A harness whose stores carry the flaw.
    pub fn new(flaw: Flaw) -> Self {
        DoubleHarness {
            flaw,
            stopped_import: false,
            machine: PhantomData,
        }
    }
}

impl<M: StateMachine> Harness<M> for DoubleHarness<M> {
    type Host = Double<M>;

    fn fresh_store(&mut self) -> Result<Store, HarnessError> {
        Ok(Store {
            flaw: self.flaw,
            entries: Vec::new(),
            state_cache: BTreeMap::new(),
            halted: false,
            importing: false,
        })
    }

    fn start(&mut self, store: Store) -> Result<Double<M>, HarnessError> {
        Ok(Double::open(store)?)
    }

    fn kill(&mut self, host: Double<M>) -> Result<Store, HarnessError> {
        // Whatever is only in memory is lost: nothing is flushed.
        Ok(host.store)
    }

    fn reopen(&mut self, store: Store) -> Result<Double<M>, HarnessError> {
        Ok(Double::open(store)?)
    }

    fn import(
        &mut self,
        store: Store,
        stream: SnapshotStream<'_>,
    ) -> Result<Double<M>, HarnessError> {
        let mut host = Double::import(store, stream)?;
        if self.flaw == Flaw::DropsRefusalsAfterAStoppedImport && self.stopped_import {
            host.store.entries.retain(|e| matches!(e, Entry::Record(_)));
        }
        Ok(host)
    }

    fn import_crashing_after(
        &mut self,
        store: Store,
        stream: SnapshotStream<'_>,
        entries: u64,
    ) -> Result<Store, HarnessError> {
        self.stopped_import = true;
        Ok(import_entries(store, stream, Some(entries))?)
    }
}

impl<M: StateMachine> FaultSeam<M> for DoubleHarness<M> {
    fn crash_at(&mut self, host: &mut Double<M>, point: CrashPoint) {
        host.crash = Some(point);
    }
}
