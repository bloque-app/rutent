//! `CMP-STM` as a VOS program (`T-010-05`, `DEC-26`): one actor holding both the journal and the state machine,
//! which is `rutent-stm`'s [`Stm`], built `no_std` with `default-features = false`. The program is `EXP-1`'s
//! re-score program (`T-007-03`) with the probe replaced by the library.
//!
//! This package links `vos-sdk` and is AGPL-3.0-or-later, as are the binaries built from it
//! (`vos-licence-v0`). `rutent-stm` itself stays MIT.
//!
//! The feature `probe-machine` swaps the host-contract suite's probe in for `Stm`. The suite is typed on its probe and
//! compares states with it, so it scores the host's mechanics on a program built so; it is never the program that ships.
//!
//! # One handler step a submit
//!
//! [`Rutent::submit`] does, in one handler step, which VOS commits whole or not at all:
//!
//! 1. the format check (below), then the import and halted checks;
//! 2. record-id dedup on byte-equal canonical JSON: an equal input returns its record, a different one is refused
//!    and the refusal written to the refusal log;
//! 3. the order's next sequence number, read and written in the same step (a compare-and-set);
//! 4. `open` or `transition`, run here;
//! 5. the record written, or the refusal-log entry.
//!
//! **This rests on VOS v1's dispatch rule**: a leader dispatches one input at a time, in batch order, on a base
//! that includes its pending overlay (`docs/design/core-model.md:241-243`, `:388-400`), so two submits never run
//! on the same base and nothing here can race. The design doc lists running inputs of one batch in parallel as
//! later work (`:246-249`); if that lands, this program must be read again.
//!
//! # Reads
//!
//! Queries serve `orders`, `state`, `journal`, `refusals` and the paged `export`, each page under VOS's 64 KiB reply
//! cap. The effect driver reads released records through [`Rutent::released_from`], a cursor over the records in
//! the order they were written: a query reads committed state only, so a record it returns is released. Records go
//! back to the adapter as their stored canonical JSON, which it reads with the original `contracts/` types.
//!
//! # Archiving ended orders (`T-010-07`, `G-33`)
//!
//! A record is kept in the live `records` table until it is sealed into the order's archive rows. Records are sealed
//! as they arrive: once an order's live records weigh [`CHUNK_BYTES`], and when the record that ends it
//! ([`is_archivable`]: complete, failed, handed on or ended without completing) is written. Sealing packs them into
//! rows of `chunks` of at most [`CHUNK_BYTES`] (the library's [`split`] and [`pack`]; VOS caps a stored value at
//! 64 KiB and a step's writes at 256 KiB) and deletes their live rows. So no step writes more than a few rows,
//! however long the journal: a step that ends an order seals what is live, under [`CHUNK_BYTES`] plus one record.
//! A record written to an ended order afterwards (a repeat, a late input, the undo of a complete part) is appended
//! to its last row. The live journal holds only the records of orders still open that have not filled a row, and
//! stays bounded under a stream of completed orders. A record that alone would not fit a row
//! ([`rutent_stm::archive::MAX_ENTRY`]) is refused when it arrives, with a reason.
//!
//! Archiving is invisible to the host contract: every read goes through `row_of`, which falls back to the archive,
//! and the entries are the stored canonical JSON byte for byte, so `journal`, `state`, `export`, a refusal's
//! pointer to its record and the effect driver's cursor read what they read before, and the host-contract suite
//! passes unchanged (`REQ-STM-3`, the host contract's `replay`: a journal is never compacted). `contracts/` is
//! untouched: no `DEC-4` bump, no `DEC-20` path.
//!
//! What stays live, one small fixed-size row per record or order: `written` (the write order), `ids` (the record
//! id's place, which dedup needs across the whole host), `orders`, and `places`. They keep the effect driver's
//! cursor and the export in write order, and the host-wide record-id check. [`Status::live_records`] counts the
//! records still in `records`, the payload this bounds. Bounding the index rows too needs a cold store outside the
//! actor, which this does not add.
//!
//! # Formats
//!
//! The actor's struct state is the same in every version of this program, so `vos_sdk::state::load` always
//! decodes it. Its `format` field names the layout of the rows; a version whose layout changed refuses every handler and query on rows of another format before it reads one, so it
//! never misreads bytes. At the tip, `Admin::Install` over a running instance touches no row and checks nothing
//! (`docs/design/light-node.md:500-507`); this check is rutent's.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use rutent_contracts::state_machine::host::{MAX_SEQ, SnapshotEntry, VERSION as HOST_VERSION};
use rutent_contracts::state_machine::{
    Order, RecordedInput, StateMachine, VERSION as MACHINE_VERSION,
};
#[cfg(feature = "probe-machine")]
use rutent_probe::Probe as Machine;
#[cfg(not(feature = "probe-machine"))]
use rutent_stm::Stm as Machine;
use rutent_stm::archive::{
    CHUNK_BYTES, Entry, chunk_of, entry_fits, entry_in, entry_len, is_archivable, pack, seal_due,
    split, unpack,
};
use serde::Serialize;
use vos_sdk::prelude::*;

/// The layout of this version's rows. A change to [`RecordRow`] changes this number.
pub const FORMAT: u32 = 2;

/// This build's version of the program, apart from its format.
pub const PROGRAM: u32 = 2;

/// The most a page of reads carries, in stored bytes, so its reply stays under VOS's 64 KiB reply cap.
pub const PAGE_BYTES: usize = 40 << 10;

/// The most a reply carries, in bytes: VOS's 64 KiB reply cap less framing.
pub const REPLY_BYTES: usize = 60 << 10;

/// `value` as canonical JSON: serde_json's value model (an object's keys in a `BTreeMap`, so sorted by their UTF-8
/// bytes) written with no whitespace. The adapter's corpus checks it against the native build, byte for byte.
pub fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(value).map_err(|e| alloc::format!("{e}"))?;
    serde_json::to_vec(&value).map_err(|e| alloc::format!("{e}"))
}

/// What a submit did.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub enum Submitted {
    /// The record is written: its order, sequence number, input and state, as canonical JSON.
    #[codec(index = 0)]
    Recorded {
        order: Vec<u8>,
        seq: u64,
        row: RecordRow,
    },
    /// An equal input is recorded already: that record. Nothing was written.
    #[codec(index = 1)]
    Already {
        order: Vec<u8>,
        seq: u64,
        row: RecordRow,
    },
    /// The record id is recorded with another input: the refusal written at `refusal`.
    #[codec(index = 2)]
    Reused {
        refusal: u64,
        order: Vec<u8>,
        seq: u64,
    },
    /// Halted. Nothing was written.
    #[codec(index = 3)]
    Halted,
    /// An import has not finished. Nothing was written.
    #[codec(index = 4)]
    Importing,
    /// The input is not a valid recorded input, or not under the record id sent. Nothing was written.
    #[codec(index = 5)]
    Invalid(Vec<u8>),
    /// The rows are in another format than this version reads. Nothing was read or written.
    #[codec(index = 6)]
    Format { stored: u32, expected: u32 },
    /// The order's next sequence number would pass `MAX_SEQ`. Nothing was written.
    #[codec(index = 7)]
    Full,
}

/// Why a read or an operator's handler refused.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub enum Refused {
    /// The rows are in another format than this version reads.
    #[codec(index = 0)]
    Format { stored: u32, expected: u32 },
    /// Only a halted host exports.
    #[codec(index = 1)]
    NotHalted,
    /// An import is open, or the store is not empty, or an entry is out of place: the reason.
    #[codec(index = 2)]
    Import(Vec<u8>),
    /// An archive row is missing or damaged, or is too large for one reply: the reason.
    #[codec(index = 3)]
    Archive(Vec<u8>),
}

/// One order: its id, its last sequence number, the sequence number of its latest record that has a state (0 for
/// none), and the packed weight of its records still in the live journal.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct OrderRow {
    pub id: Vec<u8>,
    pub len: u64,
    pub latest_state: u64,
    pub live_bytes: u64,
}

/// Where an order's archive rows begin: the sequence number of the first record of each. Records are sealed into
/// rows as they arrive, so an order that is still open can have a head: it covers the records up to `covered`, and
/// the rest are live. Once `ended`, every record is covered and a new one is appended to the last row.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct ArchiveHead {
    pub starts: Vec<u64>,
    pub covered: u64,
    pub ended: bool,
}

/// The program's two archive counters.
#[derive(Clone, Debug, Default, PartialEq, Eq, Encode, Decode)]
pub struct Counters {
    /// The records still in the live journal.
    pub live_records: u64,
    /// The orders whose journal is archived.
    pub archived_orders: u64,
}

/// An archive row read and unpacked, kept while a page of reads goes through it.
pub struct Opened {
    place: u64,
    chunk: u64,
    first: u64,
    entries: Vec<Entry>,
}

/// A record as stored: its input and its state as canonical JSON.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct RecordRow {
    pub input: Vec<u8>,
    pub state: Option<Vec<u8>>,
}

/// A refusal as stored: the refused input as canonical JSON, and the record holding its record id.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct RefusalRow {
    pub input: Vec<u8>,
    pub order: Vec<u8>,
    pub seq: u64,
}

/// One released record, as the effect driver's cursor and the export read it: its place in write order, its
/// order, its sequence number and its row.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct Released {
    pub index: u64,
    pub order: Vec<u8>,
    pub seq: u64,
    pub row: RecordRow,
}

/// The host's flags and counts, and the program's version and format.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct Status {
    pub program: u32,
    pub format: u32,
    pub stored_format: u32,
    pub halted: bool,
    pub importing: bool,
    pub import_next: u64,
    pub orders: u64,
    pub records: u64,
    pub refusals: u64,
    /// The records still in the live journal: those of orders not archived.
    pub live_records: u64,
    /// The orders whose journal is packed in the archive.
    pub archived_orders: u64,
}

/// What an import step did.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub enum Imported {
    /// The entries were taken: the next entry's place in the stream, and whether the stream ended.
    #[codec(index = 0)]
    Taken { next: u64, ended: bool },
    /// Refused: the reason. Nothing of the chunk was written.
    #[codec(index = 1)]
    Refused(Refused),
}

#[actor]
pub struct Rutent {
    /// The format of the rows. Never moves: every version of the program has this struct.
    format: u32,
    halted: bool,
    importing: bool,
    /// While an import is open, the place in the stream of its next entry.
    import_next: u64,
    order_count: u64,
    record_count: u64,
    refusal_count: u64,
    /// Each order by its place, from 1, in the order its first record was written.
    orders: Table<u64, OrderRow, Ordered>,
    /// Each order's place, by its id.
    places: Table<Vec<u8>, u64>,
    /// Each record, by its order's place and its sequence number.
    records: Table<(u64, u64), RecordRow, Ordered>,
    /// An archived order's rows: the sequence number each begins at, by the order's place. An order is archived
    /// exactly when it has a row here.
    archive: Table<u64, ArchiveHead>,
    /// An archived order's journal, packed (`rutent_stm::archive`) in rows of at most `CHUNK_BYTES`, by its place
    /// and the row's number.
    chunks: Table<(u64, u64), Vec<u8>>,
    /// The two counters, under key 0. They live in a table, not in the struct, so the struct's encoding is the
    /// same as format 1's, and a format-1 instance is refused by the format check instead of failing to load.
    meta: Table<u8, Counters>,
    /// Each record's order place and sequence number, by its place in write order, from 1.
    written: Table<u64, (u64, u64), Ordered>,
    /// Where each record id is recorded.
    ids: Table<Vec<u8>, (u64, u64)>,
    /// The refusal log, from 1.
    refusals: Table<u64, RefusalRow, Ordered>,
}

impl Rutent {
    /// The format the rows of this instance are in, as stored.
    pub fn stored_format(&self) -> u32 {
        self.format
    }

    fn format_error(&self) -> Option<(u32, u32)> {
        (self.format != FORMAT).then_some((self.format, FORMAT))
    }

    fn empty(&self) -> bool {
        self.order_count == 0 && self.refusal_count == 0
    }

    fn place_of(&self, order: &Vec<u8>) -> Option<u64> {
        self.places.get(order)
    }

    fn counters(&self) -> Counters {
        self.meta.get(&0).unwrap_or_default()
    }

    /// A record, from the live journal or else from its order's archive: the one place a record is read.
    /// `open` keeps the archive row last read, so a page of reads unpacks each row once. A row that is missing or
    /// damaged is an error, never a record that is not there.
    fn row_in(
        &self,
        open: &mut Option<Opened>,
        place: u64,
        seq: u64,
    ) -> Result<Option<RecordRow>, Vec<u8>> {
        if let Some(row) = self.records.get(&(place, seq)) {
            return Ok(Some(row));
        }
        let Some(head) = self.archive.get(&place) else {
            return Ok(None);
        };
        let chunk = chunk_of(&head.starts, seq).map_err(|e| e.0.as_bytes().to_vec())? as u64;
        if !open
            .as_ref()
            .is_some_and(|o| o.place == place && o.chunk == chunk)
        {
            let packed = self
                .chunks
                .get(&(place, chunk))
                .ok_or_else(|| b"an archive row is missing".to_vec())?;
            let entries = unpack(&packed).map_err(|e| e.0.as_bytes().to_vec())?;
            *open = Some(Opened {
                place,
                chunk,
                first: head.starts[chunk as usize],
                entries,
            });
        }
        let opened = open.as_ref().expect("opened above");
        let entry =
            entry_in(&opened.entries, opened.first, seq).map_err(|e| e.0.as_bytes().to_vec())?;
        Ok(Some(RecordRow {
            input: entry.input.clone(),
            state: entry.state.clone(),
        }))
    }

    fn row_of(&self, place: u64, seq: u64) -> Result<Option<RecordRow>, Vec<u8>> {
        self.row_in(&mut None, place, seq)
    }

    /// Writes one record at the order's next place; the caller checked the record id and the sequence.
    /// With `ends`, the record's state is one an order has ended in, and the order's live records are sealed with it.
    fn write_record(
        &mut self,
        order: Vec<u8>,
        record_id: Vec<u8>,
        row: RecordRow,
        ends: bool,
    ) -> Result<u64, Vec<u8>> {
        let entry = Entry {
            input: row.input.clone(),
            state: row.state.clone(),
        };
        if !entry_fits(&entry) {
            return Err(
                b"the record is larger than an archive row holds, so it cannot be archived"
                    .to_vec(),
            );
        }
        let place = match self.place_of(&order) {
            Some(place) => place,
            None => {
                self.order_count += 1;
                let place = self.order_count;
                self.places.insert(&order, &place);
                place
            }
        };
        let mut order_row = self.orders.get(&place).unwrap_or(OrderRow {
            id: order,
            len: 0,
            latest_state: 0,
            live_bytes: 0,
        });
        order_row.len += 1;
        if row.state.is_some() {
            order_row.latest_state = order_row.len;
        }
        let seq = order_row.len;
        let head = self.archive.get(&place);
        match head {
            Some(head) if head.ended => self.append_archived(place, head, seq, &row)?,
            _ => {
                self.records.insert(&(place, seq), &row);
                order_row.live_bytes += entry_len(&entry) as u64;
                let mut counters = self.counters();
                counters.live_records += 1;
                self.meta.insert(&0, &counters);
                // Sealed as they arrive, so the step that ends an order writes one row or two, never the journal.
                if seal_due(order_row.live_bytes as usize, ends) {
                    self.seal(place, seq, ends)?;
                    order_row.live_bytes = 0;
                }
            }
        }
        self.ids.insert(&record_id, &(place, seq));
        self.orders.insert(&place, &order_row);
        self.record_count += 1;
        self.written.insert(&self.record_count, &(place, seq));
        Ok(seq)
    }

    /// Adds the record at `seq` to the last archive row of the order at `place`, or to a new row when that one
    /// would pass `CHUNK_BYTES`.
    fn append_archived(
        &mut self,
        place: u64,
        mut head: ArchiveHead,
        seq: u64,
        row: &RecordRow,
    ) -> Result<(), Vec<u8>> {
        let entry = Entry {
            input: row.input.clone(),
            state: row.state.clone(),
        };
        let last = head.starts.len() as u64 - 1;
        let packed = self
            .chunks
            .get(&(place, last))
            .ok_or_else(|| b"an archive row is missing".to_vec())?;
        let mut entries = unpack(&packed).map_err(|e| e.0.as_bytes().to_vec())?;
        if packed.len() + entry_len(&entry) > CHUNK_BYTES {
            head.starts.push(seq);
            head.covered = seq;
            self.chunks.insert(&(place, last + 1), &pack(&[entry]));
            self.archive.insert(&place, &head);
        } else {
            entries.push(entry);
            head.covered = seq;
            self.chunks.insert(&(place, last), &pack(&entries));
            self.archive.insert(&place, &head);
        }
        Ok(())
    }

    /// Moves the live records of the order at `place`, up to `len`, into archive rows of at most `CHUNK_BYTES`.
    /// With `ends`, the order has ended and its head says so.
    fn seal(&mut self, place: u64, len: u64, ends: bool) -> Result<(), Vec<u8>> {
        let mut head = self.archive.get(&place).unwrap_or(ArchiveHead {
            starts: Vec::new(),
            covered: 0,
            ended: false,
        });
        let from = head.covered + 1;
        let mut entries = Vec::new();
        for seq in from..=len {
            let row = self
                .records
                .get(&(place, seq))
                .ok_or_else(|| b"a live record is missing".to_vec())?;
            entries.push(Entry {
                input: row.input,
                state: row.state,
            });
        }
        let mut next = from;
        for rows in split(&entries, CHUNK_BYTES) {
            self.chunks
                .insert(&(place, head.starts.len() as u64), &pack(&rows));
            head.starts.push(next);
            next += rows.len() as u64;
        }
        head.covered = len;
        head.ended |= ends;
        self.archive.insert(&place, &head);
        for seq in from..=len {
            self.records.remove(&(place, seq));
        }
        let mut counters = self.counters();
        counters.live_records -= len + 1 - from;
        counters.archived_orders += u64::from(ends);
        self.meta.insert(&0, &counters);
        Ok(())
    }

    fn latest_state(&self, place: u64) -> Result<Option<Order>, Vec<u8>> {
        let Some(row) = self.orders.get(&place) else {
            return Ok(None);
        };
        if row.latest_state == 0 {
            return Ok(None);
        }
        let state = self
            .row_of(place, row.latest_state)?
            .and_then(|r| r.state)
            .ok_or_else(|| b"an order's latest state is missing".to_vec())?;
        serde_json::from_slice(&state)
            .map(Some)
            .map_err(|e| alloc::format!("a stored state: {e}").into_bytes())
    }

    fn new_row(input: Vec<u8>, state: Option<Vec<u8>>) -> RecordRow {
        RecordRow { input, state }
    }

    /// One snapshot entry of an open import, checked against what the import took before it.
    fn take(&mut self, entry: SnapshotEntry) -> Result<bool, Vec<u8>> {
        match entry {
            SnapshotEntry::Header {
                host_version,
                state_machine_version,
            } => {
                if self.import_next != 0 {
                    return Err(b"a stream has one header, first".to_vec());
                }
                if host_version.as_str() != HOST_VERSION
                    || state_machine_version.as_str() != MACHINE_VERSION
                {
                    return Err(alloc::format!(
                        "this host reads host contract {HOST_VERSION} and state machine {MACHINE_VERSION}, not {} and {}",
                        host_version.as_str(),
                        state_machine_version.as_str()
                    )
                    .into_bytes());
                }
                Ok(false)
            }
            _ if self.import_next == 0 => Err(b"a stream starts with its header".to_vec()),
            SnapshotEntry::Record(record) => {
                let order = record.order_id.as_str().as_bytes().to_vec();
                let record_id = record.input.record_id.as_str().as_bytes().to_vec();
                if self.ids.contains(&record_id) {
                    return Err(b"a record id is recorded once in the whole host".to_vec());
                }
                let current = self
                    .place_of(&order)
                    .and_then(|place| self.orders.get(&place));
                let next = current.as_ref().map_or(1, |row| row.len + 1);
                if record.seq != next {
                    return Err(
                        b"each order's records run from 1 with no gap, in sequence order".to_vec(),
                    );
                }
                if current.is_some_and(|row| row.latest_state != 0) && record.state_after.is_none()
                {
                    return Err(b"once a record has a state, every later one does".to_vec());
                }
                let input = canonical(&record.input).map_err(String::into_bytes)?;
                let state = record
                    .state_after
                    .as_ref()
                    .map(canonical)
                    .transpose()
                    .map_err(String::into_bytes)?;
                let ends = record.state_after.as_ref().is_some_and(is_archivable);
                self.write_record(order, record_id, Self::new_row(input, state), ends)?;
                Ok(false)
            }
            SnapshotEntry::Refusal(refusal) => {
                if refusal.seq != self.refusal_count + 1 {
                    return Err(b"the refusal log runs from 1 with no gap, in order".to_vec());
                }
                let order = refusal.existing.order_id.as_str().as_bytes().to_vec();
                let holder = self
                    .ids
                    .get(&refusal.input.record_id.as_str().as_bytes().to_vec());
                if holder.is_none()
                    || holder != self.place_of(&order).map(|p| (p, refusal.existing.seq))
                {
                    return Err(b"a refusal points to the record that holds its record id".to_vec());
                }
                let input = canonical(&refusal.input).map_err(String::into_bytes)?;
                self.refusal_count = refusal.seq;
                self.refusals.insert(
                    &refusal.seq,
                    &RefusalRow {
                        input,
                        order,
                        seq: refusal.existing.seq,
                    },
                );
                Ok(false)
            }
            SnapshotEntry::End { records, refusals } => {
                if records != self.record_count || refusals != self.refusal_count {
                    return Err(
                        b"a stream's end counts the records and refusals before it".to_vec()
                    );
                }
                Ok(true)
            }
        }
    }
}

#[messages]
impl Rutent {
    pub fn new() -> Self {
        Self {
            format: FORMAT,
            halted: false,
            importing: false,
            import_next: 0,
            order_count: 0,
            record_count: 0,
            refusal_count: 0,
            orders: Table::default(),
            places: Table::default(),
            records: Table::default(),
            archive: Table::default(),
            chunks: Table::default(),
            meta: Table::default(),
            written: Table::default(),
            ids: Table::default(),
            refusals: Table::default(),
        }
    }

    /// Record and apply one input, all in this one step (see the crate's documentation).
    #[msg(gas = 100_000_000)]
    pub async fn submit(&mut self, record_id: Vec<u8>, input: Vec<u8>) -> Submitted {
        if let Some((stored, expected)) = self.format_error() {
            return Submitted::Format { stored, expected };
        }
        if self.importing {
            return Submitted::Importing;
        }
        if self.halted {
            return Submitted::Halted;
        }
        let input: RecordedInput = match serde_json::from_slice(&input) {
            Ok(input) => input,
            Err(e) => return Submitted::Invalid(alloc::format!("{e}").into_bytes()),
        };
        if input.record_id.as_str().as_bytes() != record_id.as_slice() {
            return Submitted::Invalid(b"the input is not under the record id sent".to_vec());
        }
        let bytes = match canonical(&input) {
            Ok(bytes) => bytes,
            Err(e) => return Submitted::Invalid(e.into_bytes()),
        };
        if let Some((place, at)) = self.ids.get(&record_id) {
            let order = self.orders.get(&place).map(|o| o.id).unwrap_or_default();
            let held = match self.row_of(place, at) {
                Ok(held) => held,
                Err(e) => return Submitted::Invalid(e),
            };
            if let Some(row) = held
                && row.input == bytes
            {
                return Submitted::Already {
                    order,
                    seq: at,
                    row,
                };
            }
            self.refusal_count += 1;
            let refusal = self.refusal_count;
            self.refusals.insert(
                &refusal,
                &RefusalRow {
                    input: bytes,
                    order: order.clone(),
                    seq: at,
                },
            );
            return Submitted::Reused {
                refusal,
                order,
                seq: at,
            };
        }
        let order = input.subject.order.as_str().as_bytes().to_vec();
        let place = self.place_of(&order);
        let len = place
            .and_then(|p| self.orders.get(&p))
            .map_or(0, |row| row.len);
        if len >= MAX_SEQ {
            return Submitted::Full;
        }
        let latest = match place.map(|p| self.latest_state(p)).transpose() {
            Ok(latest) => latest.flatten(),
            Err(e) => return Submitted::Invalid(e),
        };
        let state_after = match latest {
            Some(state) => Some(Machine::transition(&state, &input)),
            None => Machine::open(&input),
        };
        let state = match state_after.as_ref().map(canonical).transpose() {
            Ok(state) => state,
            Err(e) => return Submitted::Invalid(e.into_bytes()),
        };
        let row = Self::new_row(bytes, state);
        let ends = state_after.as_ref().is_some_and(is_archivable);
        let seq = match self.write_record(order.clone(), record_id, row.clone(), ends) {
            Ok(seq) => seq,
            Err(e) => return Submitted::Invalid(e),
        };
        Submitted::Recorded { order, seq, row }
    }

    /// Stop taking submits, durably, until `resume`. Refused while an import is open.
    #[msg]
    pub async fn halt(&mut self) -> Result<(), Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        if self.importing {
            return Err(Refused::Import(b"an import has not finished".to_vec()));
        }
        self.halted = true;
        Ok(())
    }

    /// Take submits again, durably. Refused while an import is open.
    #[msg]
    pub async fn resume(&mut self) -> Result<(), Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        if self.importing {
            return Err(Refused::Import(b"an import has not finished".to_vec()));
        }
        self.halted = false;
        Ok(())
    }

    /// Open an import, on an empty store only.
    #[msg]
    pub async fn begin_import(&mut self) -> Imported {
        if let Some((stored, expected)) = self.format_error() {
            return Imported::Refused(Refused::Format { stored, expected });
        }
        if self.importing || !self.empty() {
            return Imported::Refused(Refused::Import(b"an import needs an empty store".to_vec()));
        }
        self.importing = true;
        self.import_next = 0;
        Imported::Taken {
            next: 0,
            ended: false,
        }
    }

    /// One chunk of an open import: the snapshot stream's entries from place `at`, each one JSON line, which must
    /// be the import's next. Each entry is checked as `StreamCheck` checks it, against what the import took
    /// before it. A chunk that fails is refused whole: the handler returns its error, which fails the step, so VOS
    /// keeps none of the chunk's writes, and the import can go on from `at`. The end entry closes the import,
    /// leaving the host halted.
    #[msg(gas = 100_000_000)]
    pub async fn import_chunk(
        &mut self,
        at: u64,
        entries: Vec<Vec<u8>>,
    ) -> Result<Imported, Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        if !self.importing {
            return Err(Refused::Import(b"no import is open".to_vec()));
        }
        if at != self.import_next {
            return Err(Refused::Import(
                alloc::format!("the import's next entry is {}, not {at}", self.import_next)
                    .into_bytes(),
            ));
        }
        let mut ended = false;
        for line in &entries {
            if ended {
                return Err(Refused::Import(b"nothing follows a stream's end".to_vec()));
            }
            let entry: SnapshotEntry = match serde_json::from_slice(line) {
                Ok(entry) => entry,
                Err(e) => {
                    return Err(Refused::Import(alloc::format!("{e}").into_bytes()));
                }
            };
            match self.take(entry) {
                Ok(end) => ended = end,
                Err(why) => return Err(Refused::Import(why)),
            }
            self.import_next += 1;
        }
        if ended {
            self.importing = false;
            self.halted = true;
        }
        Ok(Imported::Taken {
            next: self.import_next,
            ended,
        })
    }

    #[msg]
    pub async fn status(&self) -> Status {
        Status {
            program: PROGRAM,
            format: FORMAT,
            stored_format: self.format,
            halted: self.halted,
            importing: self.importing,
            import_next: self.import_next,
            orders: self.order_count,
            records: self.record_count,
            refusals: self.refusal_count,
            live_records: self.counters().live_records,
            archived_orders: self.counters().archived_orders,
        }
    }

    /// Orders from place `from`, as many as a page holds.
    #[msg(gas = 100_000_000)]
    pub async fn orders_from(&self, from: u64) -> Result<Vec<(u64, OrderRow)>, Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        let mut out = Vec::new();
        let mut bytes = 0;
        let mut place = from.max(1);
        while place <= self.order_count && bytes < PAGE_BYTES && out.len() < 256 {
            if let Some(row) = self.orders.get(&place) {
                bytes += row.id.len() + 24;
                out.push((place, row));
            }
            place += 1;
        }
        Ok(out)
    }

    /// An order's place and row, by its id.
    #[msg]
    pub async fn order(&self, order: Vec<u8>) -> Result<Option<(u64, OrderRow)>, Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        Ok(self
            .place_of(&order)
            .and_then(|place| Some((place, self.orders.get(&place)?))))
    }

    /// An order's current state, as canonical JSON: its latest record's that has one.
    #[msg]
    pub async fn state(&self, order: Vec<u8>) -> Result<Option<Vec<u8>>, Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        let Some(place) = self.place_of(&order) else {
            return Ok(None);
        };
        let Some(row) = self.orders.get(&place) else {
            return Ok(None);
        };
        if row.latest_state == 0 {
            return Ok(None);
        }
        Ok(self
            .row_of(place, row.latest_state)
            .map_err(Refused::Archive)?
            .and_then(|r| r.state))
    }

    /// Row number `chunk` (from 0) of an archived order's journal, as the library packs it: the journal to
    /// replay it from (`rutent_stm::archive::unpack` and `replay`). `None` past the last row, or for an order
    /// that is not archived. A row larger than a reply holds is refused.
    #[msg]
    pub async fn archived(&self, order: Vec<u8>, chunk: u64) -> Result<Option<Vec<u8>>, Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        let Some(place) = self.place_of(&order) else {
            return Ok(None);
        };
        let Some(packed) = self.chunks.get(&(place, chunk)) else {
            return Ok(None);
        };
        if packed.len() > REPLY_BYTES {
            return Err(Refused::Archive(
                b"an archive row is more than a reply holds".to_vec(),
            ));
        }
        Ok(Some(packed))
    }

    /// An order's records from `from`, as many as a page holds, and at least one if any is there.
    #[msg(gas = 100_000_000)]
    pub async fn records_from(
        &self,
        place: u64,
        from: u64,
    ) -> Result<Vec<(u64, RecordRow)>, Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        let Some(row) = self.orders.get(&place) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut bytes = 0;
        let mut seq = from.max(1);
        let mut open = None;
        while seq <= row.len && out.len() < 64 {
            let Some(record) = self
                .row_in(&mut open, place, seq)
                .map_err(Refused::Archive)?
            else {
                break;
            };
            bytes += record.input.len() + record.state.as_ref().map_or(0, Vec::len);
            if !out.is_empty() && bytes > PAGE_BYTES {
                break;
            }
            out.push((seq, record));
            seq += 1;
        }
        Ok(out)
    }

    /// The refusal log from `from`, as many as a page holds, and at least one if any is there.
    #[msg(gas = 100_000_000)]
    pub async fn refusals_from(&self, from: u64) -> Result<Vec<(u64, RefusalRow)>, Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        let mut out = Vec::new();
        let mut bytes = 0;
        let mut seq = from.max(1);
        while seq <= self.refusal_count && out.len() < 64 {
            let Some(refusal) = self.refusals.get(&seq) else {
                break;
            };
            bytes += refusal.input.len() + refusal.order.len();
            if !out.is_empty() && bytes > PAGE_BYTES {
                break;
            }
            out.push((seq, refusal));
            seq += 1;
        }
        Ok(out)
    }

    /// Released records from place `from` in write order, as many as a page holds: the effect driver's cursor.
    /// With `halted_only`, refused unless the host is halted: the export's pages, read from a host nothing writes.
    #[msg(gas = 100_000_000)]
    pub async fn released_from(
        &self,
        from: u64,
        halted_only: bool,
    ) -> Result<Vec<Released>, Refused> {
        if let Some((stored, expected)) = self.format_error() {
            return Err(Refused::Format { stored, expected });
        }
        if halted_only && !self.halted {
            return Err(Refused::NotHalted);
        }
        let mut out = Vec::new();
        let mut bytes = 0;
        let mut index = from.max(1);
        let mut open = None;
        while index <= self.record_count && out.len() < 64 {
            let Some((place, seq)) = self.written.get(&index) else {
                break;
            };
            let (Some(order), Some(row)) = (
                self.orders.get(&place),
                self.row_in(&mut open, place, seq)
                    .map_err(Refused::Archive)?,
            ) else {
                break;
            };
            bytes += row.input.len() + row.state.as_ref().map_or(0, Vec::len) + order.id.len();
            if !out.is_empty() && bytes > PAGE_BYTES {
                break;
            }
            out.push(Released {
                index,
                order: order.id,
                seq,
                row,
            });
            index += 1;
        }
        Ok(out)
    }

    /// The canonical JSON this program writes for `json`, read as a recorded input (`as_input`) or as any JSON
    /// value: the guest's side of the byte-equality corpus.
    #[msg(gas = 100_000_000)]
    pub async fn canonical_of(&self, json: Vec<u8>, as_input: bool) -> Result<Vec<u8>, Vec<u8>> {
        let written = if as_input {
            serde_json::from_slice::<RecordedInput>(&json)
                .map_err(|e| alloc::format!("{e}"))
                .and_then(|input| canonical(&input))
        } else {
            serde_json::from_slice::<serde_json::Value>(&json)
                .map_err(|e| alloc::format!("{e}"))
                .and_then(|value| canonical(&value))
        };
        written.map_err(String::into_bytes)
    }
}
