//! The journal actor: the VOS guest that holds a rutent host's durable state for `EXP-1` (`T-007-03`).
//!
//! The state machine does not run here. rutent's state-machine contract needs `std` and `serde`, and a VOS guest
//! is `no_std` on RV64EM with no atomics, so the adapter computes each transition and this actor decides, in one
//! handler step, whether it is recorded: VOS commits a step and its writes together or not at all.
//!
//! Every rule of the host contract that needs state is checked here, against this actor's rows, so nothing the
//! adapter keeps in memory is authoritative:
//!
//! - a halted journal refuses every append;
//! - a record id seen before returns its record when the input is byte-equal (the adapter sends canonical JSON),
//!   and is refused, the refusal written to the refusal log, when it is not;
//! - an order's next record must carry the next sequence number, a compare-and-set;
//! - an import marks the journal incomplete until its end.
//!
//! Inputs and states are stored as the adapter's JSON bytes, whole: the journal never compacts them.

#![no_std]

use vos_sdk::prelude::*;

/// What an append did.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub enum Appended {
    /// The record is written.
    #[codec(index = 0)]
    Recorded,
    /// An equal input is already recorded: its place and the state it reached. Nothing was written.
    #[codec(index = 1)]
    Already {
        order: Vec<u8>,
        seq: u64,
        state: Option<Vec<u8>>,
    },
    /// The record id is recorded with another input. The refusal is written at `refusal`.
    #[codec(index = 2)]
    Reused {
        refusal: u64,
        order: Vec<u8>,
        seq: u64,
    },
    /// The journal is halted. Nothing was written.
    #[codec(index = 3)]
    Halted,
    /// The order's next sequence number is `next`, not the one sent. Nothing was written.
    #[codec(index = 4)]
    OutOfSequence { next: u64 },
    /// An import has not finished. Nothing was written.
    #[codec(index = 5)]
    Importing,
}

/// Whether an import step was taken.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub enum Imported {
    /// Taken.
    #[codec(index = 0)]
    Taken,
    /// Refused: the journal is not empty, or no import is open, or the entry is out of place.
    #[codec(index = 1)]
    Refused(Vec<u8>),
}

/// One order's records, as the journal holds them: the order's place in the journal, its id, its last sequence
/// number, and the sequence number of its latest record that has a state (0 for none).
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct OrderRow {
    pub id: Vec<u8>,
    pub len: u64,
    pub latest_state: u64,
}

/// A record as stored: its input's JSON, and its state's JSON when it has one.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct RecordRow {
    pub input: Vec<u8>,
    pub state: Option<Vec<u8>>,
}

/// A refusal as stored: the refused input's JSON, and the order and sequence number of the record holding its
/// record id.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct RefusalRow {
    pub input: Vec<u8>,
    pub order: Vec<u8>,
    pub seq: u64,
}

/// The journal's flags and counts.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct Status {
    pub halted: bool,
    pub importing: bool,
    pub orders: u64,
    pub records: u64,
    pub refusals: u64,
}

/// The most a page of reads carries, in stored bytes, so its reply stays under VOS's 64 KiB reply cap.
const PAGE_BYTES: usize = 40 << 10;

#[actor]
pub struct Journal {
    halted: bool,
    importing: bool,
    order_count: u64,
    record_count: u64,
    refusal_count: u64,
    /// Each order by its place, from 1, in the order its first record was written.
    orders: Table<u64, OrderRow>,
    /// Each order's place, by its id.
    places: Table<Vec<u8>, u64>,
    /// Each record, by its order's place and its sequence number.
    records: Table<(u64, u64), RecordRow>,
    /// Where each record id is recorded: its order's place and sequence number.
    ids: Table<Vec<u8>, (u64, u64)>,
    /// The refusal log, from 1.
    refusals: Table<u64, RefusalRow>,
}

impl Journal {
    fn empty(&self) -> bool {
        self.order_count == 0 && self.refusal_count == 0
    }

    /// Writes one record at the order's next place; the caller has checked the record id and the sequence.
    fn write_record(
        &mut self,
        order: Vec<u8>,
        record_id: Vec<u8>,
        input: Vec<u8>,
        state: Option<Vec<u8>>,
    ) {
        let place = match self.places.get(&order) {
            Some(place) => place,
            None => {
                self.order_count += 1;
                let place = self.order_count;
                self.places.insert(&order, &place);
                place
            }
        };
        let mut row = self.orders.get(&place).unwrap_or(OrderRow {
            id: order,
            len: 0,
            latest_state: 0,
        });
        row.len += 1;
        if state.is_some() {
            row.latest_state = row.len;
        }
        self.records
            .insert(&(place, row.len), &RecordRow { input, state });
        self.ids.insert(&record_id, &(place, row.len));
        self.orders.insert(&place, &row);
        self.record_count += 1;
    }

    fn next_seq(&self, order: &Vec<u8>) -> u64 {
        self.places
            .get(order)
            .and_then(|place| self.orders.get(&place))
            .map_or(1, |row| row.len + 1)
    }
}

#[messages]
impl Journal {
    pub fn new() -> Self {
        Self {
            halted: false,
            importing: false,
            order_count: 0,
            record_count: 0,
            refusal_count: 0,
            orders: Table::default(),
            places: Table::default(),
            records: Table::default(),
            ids: Table::default(),
            refusals: Table::default(),
        }
    }

    /// Record one input as the order's record `seq`, reaching `state`, unless the journal is halted, the record id
    /// is recorded already, or `seq` is not the order's next.
    #[msg(gas = 50_000_000)]
    pub async fn append(
        &mut self,
        order: Vec<u8>,
        seq: u64,
        record_id: Vec<u8>,
        input: Vec<u8>,
        state: Option<Vec<u8>>,
    ) -> Appended {
        if self.importing {
            return Appended::Importing;
        }
        if self.halted {
            return Appended::Halted;
        }
        if let Some((place, at)) = self.ids.get(&record_id) {
            let row = self.records.get(&(place, at));
            let id = self.orders.get(&place).map(|o| o.id).unwrap_or_default();
            if let Some(row) = row
                && row.input == input
            {
                return Appended::Already {
                    order: id,
                    seq: at,
                    state: row.state,
                };
            }
            self.refusal_count += 1;
            let refusal = self.refusal_count;
            self.refusals.insert(
                &refusal,
                &RefusalRow {
                    input,
                    order: id.clone(),
                    seq: at,
                },
            );
            return Appended::Reused {
                refusal,
                order: id,
                seq: at,
            };
        }
        let next = self.next_seq(&order);
        if seq != next {
            return Appended::OutOfSequence { next };
        }
        self.write_record(order, record_id, input, state);
        Appended::Recorded
    }

    /// Stop taking appends, until `resume`. False while an import is open.
    #[msg]
    pub async fn halt(&mut self) -> bool {
        if self.importing {
            return false;
        }
        self.halted = true;
        true
    }

    /// Take appends again. False while an import is open.
    #[msg]
    pub async fn resume(&mut self) -> bool {
        if self.importing {
            return false;
        }
        self.halted = false;
        true
    }

    /// Open an import: only on an empty journal.
    #[msg]
    pub async fn begin_import(&mut self) -> Imported {
        if self.importing || !self.empty() {
            return Imported::Refused(b"an import needs an empty store".to_vec());
        }
        self.importing = true;
        Imported::Taken
    }

    /// One record of an open import, at its order's next place.
    #[msg(gas = 50_000_000)]
    pub async fn import_record(
        &mut self,
        order: Vec<u8>,
        seq: u64,
        record_id: Vec<u8>,
        input: Vec<u8>,
        state: Option<Vec<u8>>,
    ) -> Imported {
        if !self.importing {
            return Imported::Refused(b"no import is open".to_vec());
        }
        if self.ids.contains(&record_id) {
            return Imported::Refused(b"a record id is recorded once".to_vec());
        }
        if seq != self.next_seq(&order) {
            return Imported::Refused(b"each order's records run from 1 with no gap".to_vec());
        }
        self.write_record(order, record_id, input, state);
        Imported::Taken
    }

    /// One refusal of an open import, at the refusal log's next place.
    #[msg(gas = 50_000_000)]
    pub async fn import_refusal(
        &mut self,
        seq: u64,
        input: Vec<u8>,
        order: Vec<u8>,
        at: u64,
    ) -> Imported {
        if !self.importing {
            return Imported::Refused(b"no import is open".to_vec());
        }
        if seq != self.refusal_count + 1 {
            return Imported::Refused(b"the refusal log runs from 1 with no gap".to_vec());
        }
        self.refusal_count = seq;
        self.refusals.insert(
            &seq,
            &RefusalRow {
                input,
                order,
                seq: at,
            },
        );
        Imported::Taken
    }

    /// Close an open import: the journal is complete, and halted.
    #[msg]
    pub async fn finish_import(&mut self) -> Imported {
        if !self.importing {
            return Imported::Refused(b"no import is open".to_vec());
        }
        self.importing = false;
        self.halted = true;
        Imported::Taken
    }

    #[msg]
    pub async fn status(&self) -> Status {
        Status {
            halted: self.halted,
            importing: self.importing,
            orders: self.order_count,
            records: self.record_count,
            refusals: self.refusal_count,
        }
    }

    /// Orders from place `from`, as many as a page holds.
    #[msg(gas = 100_000_000)]
    pub async fn orders_from(&self, from: u64) -> Vec<(u64, OrderRow)> {
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
        out
    }

    /// An order's row, by its id.
    #[msg]
    pub async fn order(&self, order: Vec<u8>) -> Option<(u64, OrderRow)> {
        let place = self.places.get(&order)?;
        Some((place, self.orders.get(&place)?))
    }

    /// One record, by its order's place and sequence number.
    #[msg]
    pub async fn record(&self, place: u64, seq: u64) -> Option<RecordRow> {
        self.records.get(&(place, seq))
    }

    /// An order's records from `from`, as many as a page holds; at least one if any is there.
    #[msg(gas = 100_000_000)]
    pub async fn records_from(&self, place: u64, from: u64) -> Vec<(u64, RecordRow)> {
        let Some(row) = self.orders.get(&place) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut bytes = 0;
        let mut seq = from.max(1);
        while seq <= row.len && (out.is_empty() || bytes < PAGE_BYTES) && out.len() < 64 {
            let Some(record) = self.records.get(&(place, seq)) else {
                break;
            };
            bytes += record.input.len() + record.state.as_ref().map_or(0, Vec::len);
            if !out.is_empty() && bytes > PAGE_BYTES {
                break;
            }
            out.push((seq, record));
            seq += 1;
        }
        out
    }

    /// The refusal log from `from`, as many as a page holds; at least one if any is there.
    #[msg(gas = 100_000_000)]
    pub async fn refusals_from(&self, from: u64) -> Vec<(u64, RefusalRow)> {
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
        out
    }
}
