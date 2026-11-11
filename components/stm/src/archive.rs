//! The archive of an ended order (`G-33`, `REQ-OPS-3`): what a host that keeps its live journal bounded moves out of
//! it, and how an archived order is read back.
//!
//! An order that has ended ([`is_ended`]) takes few inputs, but not none: a repeat, a late input, and the undo of a
//! complete part are recorded after its ending (`REQ-STM-6`). A host archives such an order by packing its whole
//! journal into one row ([`pack`]) and keeping that row apart from the live journal; it appends to the row when
//! another record comes. The journal stays complete and uncompacted (the host contract's `replay` guarantee,
//! `REQ-STM-3`): every entry is kept byte for byte, so [`unpack`] returns what [`pack`] was given, and [`replay`]
//! folds the archived inputs to the states the journal recorded (`NFR-4`).
//!
//! The format is the library's, not the contract's: the host contract and the state-machine contract never see an
//! archive, since a host serves archived records through the same reads as live ones. Nothing here parses JSON, so
//! the library keeps its dependencies; the host holds each entry as the canonical JSON it already stores.
//!
//! # Format
//!
//! `RSA1`, the entry count as a `u32`, then each entry: the input's length as a `u32` and its bytes, a `0` or `1`
//! for whether a state follows, and when `1` the state's length as a `u32` and its bytes. All lengths are
//! little-endian. [`unpack`] refuses anything else, bytes after the last entry included.

use alloc::vec::Vec;

use rutent_contracts::state_machine::{Order, RecordedInput, StateMachine};

use crate::Stm;
use crate::parts::is_ended;

const MAGIC: &[u8; 4] = b"RSA1";

/// One record of an archived order, as the host stores it: its input and its state as canonical JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The input, as canonical JSON.
    pub input: Vec<u8>,
    /// The state it reached, as canonical JSON, if it reached one.
    pub state: Option<Vec<u8>>,
}

/// Why bytes are not an archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveError(pub &'static str);

/// The most a stored archive row aims to hold, in packed bytes: half of VOS's 64 KiB cap on a stored value
/// (`cap::VALUE`), so a row never nears it. A journal longer than this is split across rows ([`split`]).
pub const CHUNK_BYTES: usize = 32 << 10;

/// The most one record may weigh in an archive row, packed alone: under VOS's 64 KiB cap on a stored value with room
/// for the row's framing. A host refuses a record above it when the record arrives.
pub const MAX_ENTRY: usize = 60 << 10;

/// Whether `entry` can be archived: alone in a row, it stays under [`MAX_ENTRY`].
pub fn entry_fits(entry: &Entry) -> bool {
    8 + entry_len(entry) <= MAX_ENTRY
}

/// Whether a host seals an order's live records into archive rows now: they have filled a row, or the record just
/// written ends the order. Sealing as records arrive keeps the step that ends an order from writing its whole
/// journal, which VOS caps at 256 KiB a step.
pub fn seal_due(live_bytes: usize, ends: bool) -> bool {
    live_bytes >= CHUNK_BYTES || (ends && live_bytes > 0)
}

/// The most one step of an import aims to write, in bytes: under VOS's 256 KiB cap on a step's writes, with room
/// for the entry that passes it and for the index rows.
pub const STEP_BUDGET: usize = 200 << 10;

/// The write budget of one import step: entries are admitted while their charges (`import_cost`) keep the step
/// under [`STEP_BUDGET`], and the first is always admitted. The program's `import_chunk` and the tests drive the
/// same rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StepBudget {
    spent: usize,
    taken: usize,
}

impl StepBudget {
    /// A step that has taken nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes an entry that will write at most `cost` bytes, if the step has room for it; false means stop here, and
    /// the entry goes in the next step.
    pub fn admit(&mut self, cost: usize) -> bool {
        if self.taken > 0 && self.spent + cost > STEP_BUDGET {
            return false;
        }
        self.spent += cost;
        self.taken += 1;
        true
    }

    /// What the entries admitted so far charge in all.
    pub fn spent(&self) -> usize {
        self.spent
    }
}

/// What an index row put or a delete is charged beyond its value: its key and the row's framing.
pub const INDEX_PUT: usize = 48;

/// What an order holds when an import takes a record for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// The order is open: `count` records weighing `bytes` are live, and the record may seal them.
    Live {
        /// Their packed weight.
        bytes: usize,
        /// How many there are.
        count: usize,
    },
    /// The order has ended: a record is appended to its last archive row, of `last_row` stored bytes, which it
    /// rewrites whole (or starts a new row, if that one is full).
    Ended {
        /// The stored length of the last row.
        last_row: usize,
    },
}

/// What taking one record of `weight` bytes into an import step writes, at most:
///
/// - its own row and the index rows it puts (`ids`, `orders`, `written`, the counters), each charged [`INDEX_PUT`];
/// - when it seals (`seal_due`), the order's live records and itself packed again into archive rows, a delete for
///   each record sealed, and the archive head;
/// - when the order has ended, the order's last archive row rewritten, and its head.
///
/// A host stops taking entries before the step's total would pass [`STEP_BUDGET`], so a run of entries that each
/// seal a different order, or each rewrite a different ended order's last row, cannot pass the cap.
pub fn import_cost(held: Held, weight: usize, ends: bool) -> usize {
    let puts = 5 * INDEX_PUT;
    match held {
        Held::Live { bytes, count } => {
            let own = weight + puts;
            if seal_due(bytes + weight, ends) {
                own + bytes + weight + (count + 1) * INDEX_PUT + 3 * INDEX_PUT
            } else {
                own
            }
        }
        Held::Ended { last_row } => weight + puts + last_row + 2 * INDEX_PUT,
    }
}

/// What `entry` adds to a packed row.
pub fn entry_len(entry: &Entry) -> usize {
    9 + entry.input.len() + entry.state.as_ref().map_or(0, Vec::len)
}

/// `entries` in order, as rows that each pack to at most `limit` bytes, or hold one entry alone (an entry larger
/// than the limit is never cut). No row is empty, and joining the rows gives `entries` back.
pub fn split(entries: &[Entry], limit: usize) -> Vec<Vec<Entry>> {
    let mut rows: Vec<Vec<Entry>> = Vec::new();
    let mut size = 0;
    for entry in entries {
        let add = entry_len(entry);
        match rows.last_mut() {
            Some(row) if size + add + 8 <= limit => {
                row.push(entry.clone());
                size += add;
            }
            _ => {
                rows.push(alloc::vec![entry.clone()]);
                size = add;
            }
        }
    }
    rows
}

/// The number of the row, among rows that begin at `starts` (the first record each holds), that holds record `seq`.
/// An error if `starts` does not begin at or before `seq`: a head that is damaged, not a record that is absent.
pub fn chunk_of(starts: &[u64], seq: u64) -> Result<usize, ArchiveError> {
    starts
        .iter()
        .rposition(|start| *start <= seq)
        .ok_or(ArchiveError("an archive does not begin at record 1"))
}

/// Record `seq` of a row whose first record is `first`. An error if the row is shorter than that.
pub fn entry_in(entries: &[Entry], first: u64, seq: u64) -> Result<&Entry, ArchiveError> {
    seq.checked_sub(first)
        .and_then(|at| usize::try_from(at).ok())
        .and_then(|at| entries.get(at))
        .ok_or(ArchiveError("an archive row is shorter than its head says"))
}

/// Whether an order is one a host archives: it has ended (`REQ-STM-6`). Stuck and waiting orders are not
/// archived: someone can still resolve them, and they take inputs.
pub fn is_archivable(order: &Order) -> bool {
    is_ended(order.state)
}

/// `entries` as one archive row.
pub fn pack(entries: &[Entry]) -> Vec<u8> {
    let size = 8 + entries
        .iter()
        .map(|e| 9 + e.input.len() + e.state.as_ref().map_or(0, Vec::len))
        .sum::<usize>();
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        out.extend_from_slice(&(entry.input.len() as u32).to_le_bytes());
        out.extend_from_slice(&entry.input);
        match &entry.state {
            Some(state) => {
                out.push(1);
                out.extend_from_slice(&(state.len() as u32).to_le_bytes());
                out.extend_from_slice(state);
            }
            None => out.push(0),
        }
    }
    out
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ArchiveError> {
        if self.0.len() < n {
            return Err(ArchiveError("an archive ends inside an entry"));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn length(&mut self) -> Result<usize, ArchiveError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
    }
}

/// The entries of an archive row, in the order they were packed.
pub fn unpack(bytes: &[u8]) -> Result<Vec<Entry>, ArchiveError> {
    let mut reader = Reader(bytes);
    if reader.take(4)? != MAGIC {
        return Err(ArchiveError("not an archive"));
    }
    let count = reader.length()?;
    // Each entry takes at least five bytes, so a count the bytes cannot hold is refused before anything is reserved.
    if count > reader.0.len() / 5 {
        return Err(ArchiveError("an archive's count is more than it holds"));
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let len = reader.length()?;
        let input = reader.take(len)?.to_vec();
        let state = match reader.take(1)?[0] {
            0 => None,
            1 => {
                let len = reader.length()?;
                Some(reader.take(len)?.to_vec())
            }
            _ => return Err(ArchiveError("an entry's state flag is 0 or 1")),
        };
        entries.push(Entry { input, state });
    }
    if !reader.0.is_empty() {
        return Err(ArchiveError("bytes after an archive's last entry"));
    }
    Ok(entries)
}

/// The states an order's archived inputs reach, one for each, the way a host folds them: `open` until one opens the
/// order, `transition` after (`REQ-STM-3`, `NFR-4`). `Stm` is deterministic, so this equals the states the journal
/// recorded.
pub fn replay(inputs: &[RecordedInput]) -> Vec<Option<Order>> {
    let mut state: Option<Order> = None;
    inputs
        .iter()
        .map(|input| {
            state = match &state {
                Some(order) => Some(Stm::transition(order, input)),
                None => Stm::open(input),
            };
            state.clone()
        })
        .collect()
}
