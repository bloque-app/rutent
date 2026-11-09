//! The purpose-built store (`T-007-02`): a host of the host contract (`contracts/state-machine/host/`) on real
//! disk, the `pb` candidate of `EXP-1` and ADR-0002's ruled fallback if VOS is dropped (`DEC-6`, `REQ-OPS-3`,
//! `NFR-6`). It is built only with the `purpose-built-store` feature of `rutent-stm`, off by default.
//!
//! It is the reference host of `T-005-04` with its log moved onto disk, and nothing more: the same entries, the same
//! indexes rebuilt on open, the same answers. What it adds is what memory can't give: a store that survives a real
//! process kill, and a log that recovers from a write torn part-way.
//!
//! # The store
//!
//! A store, [`DiskStore`], is a directory holding one file, [`LOG_FILE`]: an append-only log of frames. A frame is
//!
//! ```text
//! [ length: u32, little-endian ][ checksum: u32, little-endian ][ payload: `length` bytes of JSON ]
//! ```
//!
//! The checksum is CRC-32 (IEEE) over the length's four bytes and the payload, so a torn or corrupted length is
//! caught as surely as a torn payload. A length of zero is never written, so a zero-filled tail never reads as a
//! frame. The payload is one log entry: the store's format (always first, and saying whether the log starts an
//! import), a transition record, a refusal, halted, resumed, or an import complete. Records and refusals are the contract's own types, serialised
//! as the contract's schemas say, so a record on disk is exactly a record of a snapshot stream.
//!
//! # Writing: one frame, one write, one sync
//!
//! Every append is exactly one frame, built in one buffer, written with a single `write_all`, and synced with
//! [`File::sync_data`] before anything is returned or indexed and before the next append. A fresh log's first
//! write appends its format entry that way first, then the change. So at any moment at most one frame, the last,
//! is unsynced. Nothing is buffered in user space, so dropping a host loses nothing that was returned. Every open
//! syncs the store's directory, so the log's name is durable however the open before it ended. The directory's
//! own entry in its parent is the caller's to make durable (see [`DiskStore::at`]).
//!
//! On macOS, `sync_data` is `fcntl(F_FULLFSYNC)`, which asks the drive to flush its own cache; on Linux it is
//! `fdatasync`, which also flushes the file's length, the one piece of metadata an append changes. This module
//! does not test power loss: a process kill keeps the operating system's page cache, and the suite can't cut the
//! power. That durability rests on the call above, which `EXP-1` scores by reading this code.
//!
//! If a write or a sync fails, the host undoes the append: it cuts the log back to its length before the write
//! (`set_len`) and syncs it (`sync_all`), so once the undo is done, a frame whose sync failed can't be read back
//! as if it were durable. If the process dies after the failed sync and before the cut, the frame is still in the
//! log: read back damaged or zeroed, it is refused as damage, as below; read back whole, it is taken as a record
//! present and unacknowledged, which is a window this module doesn't close. Then it stops: it answers [`HostError::Unavailable`] to every later submit, halt or resume, and must be opened
//! again. If the undo fails too, it also writes a marker, [`UNVERIFIED_FILE`], beside the log, and every open
//! refuses the store with [`HostError::Unavailable`] while the marker is there: the log may hold a frame that was
//! never durable, and an operator verifies it and removes the marker. If even the marker can't be written, nothing
//! on disk records the failure; the host stays stopped, and that one case is not covered.
//!
//! # Recovery on open
//!
//! [`Host::open`] takes an exclusive lock on the log ([`File::try_lock`], released by the operating system when the
//! process dies), refuses the store if [`UNVERIFIED_FILE`] is beside the log, reads the log frame by frame, and
//! then, at the first frame that is short or fails its checksum:
//!
//! - **A torn tail.** If no whole frame starts anywhere after it, and the bytes from it to the end fit in one
//!   frame (fewer than a frame's header, or no more than the header and the length that header declares), they are
//!   the one unsynced append a crash cut short. It was never acknowledged, and it is cut off (`set_len`, then a
//!   sync) before anything new is written.
//! - **More than one append.** If no whole frame starts after it but the bytes are more than one append could
//!   leave (a header whose length is unreadable, such as zeros, or bytes past the length it declares), they hold
//!   entries that were synced. The store is refused with [`HostError::Unavailable`] and nothing is cut. A
//!   zero-filled tail is refused this way even when it is only the append in flight, as a file system may leave
//!   one after a power loss: refusing is the side that loses nothing, and the operator checks the log.
//! - **Corruption.** If a whole frame starts after the bad one, the bad bytes are inside synced history. The store
//!   is refused with [`HostError::Unavailable`] and nothing is cut.
//! - **What it can't tell apart.** Damage confined to the last frame, its length intact, looks exactly like a torn
//!   write and is cut as one: an acknowledged last entry that the media damaged afterwards is lost without a
//!   refusal. The frame's checksum catches the damage but not its cause: a torn write and later damage fail it
//!   alike, so only a copy kept apart from the log could tell the two apart. Recovery never cuts more than that
//!   one frame.
//! - **An unfinished import.** An import's log starts with a format entry that says it starts an import, written
//!   and synced as one frame before anything else. Such a log with no import-complete entry refuses the store with
//!   [`HostError::ImportIncomplete`], torn or corrupt bytes after it included: a crash during an import, whose
//!   remedy is to redo the import on a fresh store. Damage inside an import that did complete is corruption, as
//!   above, never `ImportIncomplete`: the import's complete entry is looked for in the whole frames past the
//!   damage, and found there, it means the store was in use after the import, and redoing the import would drop
//!   every record made since.
//! - **A torn first frame.** A log whose very first frame is torn is refused with [`HostError::Unavailable`], not
//!   cut: it can't say whether it was an import's start, and an empty store that opened would look like a finished
//!   import of nothing. It holds nothing that was ever durable, so the remedy is a fresh store. A crash before the
//!   first byte reached the log leaves an empty log, which opens as a fresh store; nothing of an import is in it,
//!   and the import that crashed never returned.
//! - **The rest.** The entries are checked with the contract's own [`StreamCheck`] (gapless sequences, one record
//!   per record id, refusals pointing to their records), and the indexes are rebuilt from them, as the reference
//!   host rebuilds its own.
//!
//! # How it keeps each guarantee
//!
//! - `durable`: a record or refusal is written and synced before `submit` returns it; halt and resume are entries
//!   too, so a halted host reopens halted; a torn write is cut back to the last whole entry.
//! - `exactly-once`: a record id is looked up across every record; an equal input returns the recorded record;
//!   any other input under the id is refused, and the refusal is synced before it is returned.
//! - `per-order-sequence`: each order's next sequence number is its journal's length plus one, applied in submit
//!   order.
//! - `no-effect-without-record`: nothing is indexed, returned or readable before its frame is synced.
//! - `swap`: export streams every record and refusal between a header and an end, from a halted host only; import
//!   writes a format entry that starts an import and syncs it, checks the stream with [`StreamCheck`] as it reads, accepts only
//!   its own contract versions and an empty store, syncs the records, and only then writes halted and
//!   import-complete.
//! - `replay`: the log is never compacted, and each record holds its input whole and the state it reached.
//!
//! # Limits
//!
//! The host keeps every record in memory, as the reference host does, and reads the whole log on open. One store
//! is one directory on one machine; there is no replication. Neither is a claim about `NFR-1`: none is made before
//! `M4` (`PLAN.md` §6.2, risk 9).
//!
//! The lock is advisory (`flock` on Unix): it keeps out another host of this module, not a process that ignores
//! it, and on NFS it is emulated and weaker. Only Unix is supported: the directory sync opens the directory as a
//! file, which Windows does not allow, and nothing here was built or run on Windows.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
use serde::{Deserialize, Serialize};

use crate::open_retry::{OpenRetry, is_open_elsewhere, retry_open};

/// The name of the log file inside a store's directory.
pub const LOG_FILE: &str = "log";

/// The store format this module writes, and the only one it reads. A later format either reads this one or
/// documents its upgrade path.
pub const STORE_FORMAT: u32 = 1;

/// The name of the marker written beside the log when a failed append couldn't be undone. While it is there, the
/// store is refused at open; an operator verifies the log and removes it.
pub const UNVERIFIED_FILE: &str = "log.unverified";

/// The bytes before a frame's payload: its length and its checksum.
pub const FRAME_HEADER: usize = 8;

/// The largest payload a frame may hold. A larger length is read as a bad frame.
const MAX_PAYLOAD: usize = 64 << 20;

/// A purpose-built store: a directory holding the log. Opening it takes an exclusive lock, so two hosts never write
/// one store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskStore {
    dir: PathBuf,
}

impl DiskStore {
    /// The store in this directory. The directory must exist; the log is created in it on first open, and every
    /// open syncs the directory, so the log's name in it is durable. The directory's own entry in its parent is the
    /// caller's to make durable, by syncing the parent after creating it.
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        DiskStore { dir: dir.into() }
    }

    /// The store's directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The path of the store's log.
    pub fn log_path(&self) -> PathBuf {
        self.dir.join(LOG_FILE)
    }

    /// The path of the marker that refuses the store until its log is verified.
    pub fn unverified_path(&self) -> PathBuf {
        self.dir.join(UNVERIFIED_FILE)
    }
}

/// One entry of the log, one per frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "entry",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum LogEntry {
    /// The first entry of every log: the store format, whether the log starts an import, and the contract
    /// versions it was written under.
    Format {
        store_format: u32,
        importing: bool,
        host_version: String,
        state_machine_version: String,
    },
    Record(TransitionRecord),
    Refusal(RefusalRecord),
    Halted,
    Resumed,
    ImportComplete,
}

/// A log's first entry; `importing` when it starts an import.
fn format_entry(importing: bool) -> LogEntry {
    LogEntry::Format {
        store_format: STORE_FORMAT,
        importing,
        host_version: contract::VERSION.into(),
        state_machine_version: machine::VERSION.into(),
    }
}

/// CRC-32 (IEEE 802.3, reflected, polynomial `0xEDB88320`), the checksum of a frame.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// The CRC-32 of the parts, in order, as one byte string.
pub fn crc32(parts: &[&[u8]]) -> u32 {
    let mut c = !0u32;
    for part in parts {
        for &b in *part {
            c = CRC_TABLE[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
        }
    }
    !c
}

fn unavailable(reason: impl Into<String>) -> HostError {
    HostError::Unavailable {
        reason: reason.into(),
    }
}

fn refused(reason: impl Into<String>) -> HostError {
    HostError::SnapshotRefused {
        reason: reason.into(),
    }
}

fn io_failed(what: &str, e: std::io::Error) -> HostError {
    unavailable(format!("{what}: {e}"))
}

/// Appends one entry's frame to the buffer.
fn frame(entry: &LogEntry, out: &mut Vec<u8>) -> Result<(), HostError> {
    let payload = serde_json::to_vec(entry).map_err(|e| unavailable(format!("encoding: {e}")))?;
    if payload.is_empty() || payload.len() > MAX_PAYLOAD {
        return Err(unavailable("an entry is larger than a frame holds"));
    }
    let len = u32::try_from(payload.len())
        .expect("a payload no larger than MAX_PAYLOAD fits in u32")
        .to_le_bytes();
    out.extend_from_slice(&len);
    out.extend_from_slice(&crc32(&[&len, &payload]).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(())
}

/// The whole frame starting at `at`, as its end and its payload, or `None` if the bytes there are not one.
fn frame_at(bytes: &[u8], at: usize) -> Option<(usize, &[u8])> {
    let header = bytes.get(at..at.checked_add(FRAME_HEADER)?)?;
    let len_bytes: [u8; 4] = header[..4].try_into().ok()?;
    let sum = u32::from_le_bytes(header[4..].try_into().ok()?);
    let len = usize::try_from(u32::from_le_bytes(len_bytes)).ok()?;
    if len == 0 || len > MAX_PAYLOAD {
        return None;
    }
    let start = at + FRAME_HEADER;
    let payload = bytes.get(start..start.checked_add(len)?)?;
    (crc32(&[&len_bytes, payload]) == sum).then_some((start + len, payload))
}

/// Damage a scan found that is not a torn tail: bytes that held entries which were synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Damage {
    /// A bad frame with a whole frame somewhere after it.
    Corrupt,
    /// A bad frame with no whole frame after it, but more bytes than one append could leave.
    LongerThanOneAppend,
}

/// What reading a log found.
struct Scan {
    entries: Vec<LogEntry>,
    /// The end of the last whole frame; anything after it is a torn tail, or damage.
    end: usize,
    /// Damage at `end`, if the bytes from there are not a torn tail.
    damage: Option<Damage>,
    /// Whether an import-complete entry is in a whole frame past the damage, if there is damage.
    import_completed_past_it: bool,
}

/// Whether bytes that hold no whole frame could be the one unsynced append a crash cut short: fewer than a frame's
/// header, or a header with a readable length that covers them all.
fn one_torn_append(tail: &[u8]) -> bool {
    let Some(header) = tail.get(..FRAME_HEADER) else {
        return true;
    };
    let len_bytes: [u8; 4] = header[..4].try_into().expect("a header holds a length");
    let len = usize::try_from(u32::from_le_bytes(len_bytes)).unwrap_or(usize::MAX);
    (1..=MAX_PAYLOAD).contains(&len) && tail.len() <= FRAME_HEADER + len
}

/// Whether a whole frame holding an import-complete entry starts anywhere at or after `from`. Past bad bytes it
/// tries each next byte until a whole frame starts again, then walks frame by frame.
fn import_completed_after(bytes: &[u8], from: usize) -> bool {
    let mut at = from;
    while at < bytes.len() {
        match frame_at(bytes, at) {
            Some((end, payload)) => {
                if matches!(
                    serde_json::from_slice(payload),
                    Ok(LogEntry::ImportComplete)
                ) {
                    return true;
                }
                at = end;
            }
            None => at += 1,
        }
    }
    false
}

/// Reads a log's frames in order, stopping at the first bad one.
fn scan(bytes: &[u8]) -> Result<Scan, HostError> {
    let mut entries = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let Some((end, payload)) = frame_at(bytes, at) else {
            // A whole frame starting anywhere after the bad one means the bad bytes are inside synced history. A
            // payload's own bytes can't pass for one: a payload is compact JSON, which never holds a raw byte below
            // 0x20, and a length no larger than MAX_PAYLOAD (64 MiB) has a high byte of at most 0x04, so no frame
            // can start where its length's high byte would fall inside a payload. Only one starting in a payload's
            // last three bytes could, and then its checksum would have to match by chance.
            let corrupt = (at + 1..bytes.len()).any(|k| frame_at(bytes, k).is_some());
            let damage = if corrupt {
                Some(Damage::Corrupt)
            } else if one_torn_append(&bytes[at..]) {
                None
            } else {
                Some(Damage::LongerThanOneAppend)
            };
            return Ok(Scan {
                entries,
                end: at,
                damage,
                import_completed_past_it: corrupt && import_completed_after(bytes, at + 1),
            });
        };
        let entry = serde_json::from_slice(payload).map_err(|e| {
            unavailable(format!(
                "the log's entry at byte {at} has a good checksum but doesn't read: {e}"
            ))
        })?;
        entries.push(entry);
        at = end;
    }
    Ok(Scan {
        entries,
        end: at,
        damage: None,
        import_completed_past_it: false,
    })
}

/// Opens (creating if needed) a store's log for reading and appending, locks it, and syncs the store's directory,
/// so the log's name is durable whether or not this open created it.
fn open_log(store: &DiskStore) -> Result<File, HostError> {
    let path = store.log_path();
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(&path)
        .map_err(|e| io_failed("opening the log", e))?;
    file.try_lock().map_err(|e| match e {
        fs::TryLockError::WouldBlock => unavailable(crate::open_retry::STORE_OPEN_ELSEWHERE),
        fs::TryLockError::Error(e) => io_failed("locking the log", e),
    })?;
    // No test reaches this: a missing directory sync only shows after a power cut, which needs tooling this
    // module doesn't have. It rests on the code.
    sync_dir(store.dir())?;
    Ok(file)
}

/// Syncs a directory, so a file created in it is durable by name.
fn sync_dir(dir: &Path) -> Result<(), HostError> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| io_failed("syncing the store's directory", e))
}

/// The purpose-built store's host, running the state machine `M`.
#[derive(Debug)]
pub struct PurposeBuiltHost<M> {
    store: DiskStore,
    /// The log, open for appending and locked.
    file: File,
    /// The log's length: the end of its last synced frame.
    end: u64,
    /// Whether the log holds its format entry yet; a fresh log gets it with its first write.
    formatted: bool,
    halted: bool,
    /// Each order's records, in sequence order.
    journals: BTreeMap<Id, Vec<TransitionRecord>>,
    /// Orders in the order their first record was written.
    orders: Vec<Id>,
    /// Where each record id's record is.
    by_id: BTreeMap<RecordId, RecordRef>,
    /// The refusal log.
    refusals: Vec<RefusalRecord>,
    /// Fault-seam machinery: the crash point the suite set, if any. Settable only through
    /// [`FaultSeam::crash_at`].
    crash: Option<CrashPoint>,
    /// Why the host stopped: at a crash point the suite set, or after a write or sync failed. Once set, every call
    /// that could change the host answers `Unavailable`.
    stopped: Option<String>,
    /// Test-only seam: an I/O failure to inject into the next append.
    #[cfg(test)]
    inject: Option<Inject>,
    machine: PhantomData<M>,
}

/// An I/O failure a unit test injects into an append.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inject {
    /// The write stops half way through the frame, and fails.
    WriteFails,
    /// The write succeeds and the sync fails.
    SyncFails,
    /// The write succeeds, the sync fails, and so does the undo.
    SyncAndUndoFail,
}

impl<M> PurposeBuiltHost<M> {
    /// The store, giving up the host and its lock: what a kill leaves.
    pub fn into_store(self) -> DiskStore {
        self.store
    }

    fn index_record(&mut self, r: &TransitionRecord) {
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

    /// Appends one entry, first the format entry if the log has none yet, each as its own frame, written and synced
    /// before the next.
    fn append(&mut self, entry: &LogEntry) -> Result<(), HostError> {
        if !self.formatted {
            self.append_frame(&format_entry(false))?;
            self.formatted = true;
        }
        self.append_frame(entry)
    }

    /// Writes one frame with one write and syncs it. On failure, undoes the append and stops the host; if the undo
    /// fails too, marks the store unverified.
    fn append_frame(&mut self, entry: &LogEntry) -> Result<(), HostError> {
        let mut buf = Vec::new();
        frame(entry, &mut buf)?;
        let written = self.write_log(&buf).and_then(|()| self.sync_log());
        let Err(e) = written else {
            self.end += count(buf.len());
            return Ok(());
        };
        let mut reason = format!("appending to the log failed, so the host stopped: {e}");
        if let Err(undo) = self.undo_append() {
            reason.push_str(&format!(
                "; cutting the log back to {} bytes failed too ({undo}), so the store is refused until its log \
                 is verified",
                self.end
            ));
            // The marker's existence is what refuses the store, so its name must be durable (the directory sync);
            // its contents are synced too, so an operator reads why.
            let marked = File::create(self.store.unverified_path())
                .and_then(|mut m| {
                    m.write_all(format!("{reason}\n").as_bytes())?;
                    m.sync_all()
                })
                .map_err(|e| e.to_string())
                .and_then(|()| sync_dir(self.store.dir()).map_err(|e| format!("{e:?}")));
            if let Err(m) = marked {
                reason.push_str(&format!(", but the marker couldn't be written ({m})"));
            }
        }
        self.stopped = Some(reason.clone());
        Err(unavailable(reason))
    }

    /// Writes a frame to the log in one write.
    fn write_log(&mut self, buf: &[u8]) -> std::io::Result<()> {
        #[cfg(test)]
        if self.inject == Some(Inject::WriteFails) {
            self.file.write_all(&buf[..buf.len() / 2])?;
            return Err(std::io::Error::other("a write failure a test injected"));
        }
        self.file.write_all(buf)
    }

    /// Syncs the log's data.
    fn sync_log(&self) -> std::io::Result<()> {
        #[cfg(test)]
        if matches!(
            self.inject,
            Some(Inject::SyncFails | Inject::SyncAndUndoFail)
        ) {
            return Err(std::io::Error::other("a sync failure a test injected"));
        }
        self.file.sync_data()
    }

    /// Cuts the log back to its last synced frame and syncs it.
    fn undo_append(&self) -> std::io::Result<()> {
        #[cfg(test)]
        if self.inject == Some(Inject::SyncAndUndoFail) {
            return Err(std::io::Error::other("an undo failure a test injected"));
        }
        self.file
            .set_len(self.end)
            .and_then(|()| self.file.sync_all())
    }

    /// Recovers a host from a store whose log this process has open and locked.
    fn open_locked(store: DiskStore, mut file: File) -> Result<Self, HostError> {
        let marked = store
            .unverified_path()
            .try_exists()
            .map_err(|e| io_failed("checking for the unverified marker", e))?;
        if marked {
            return Err(unavailable(format!(
                "the store is marked unverified ({UNVERIFIED_FILE}): an append to its log failed and couldn't be \
                 undone, so the log may hold a frame that was never durable; verify the log, then remove the marker"
            )));
        }
        let mut bytes = Vec::new();
        file.seek(SeekFrom::Start(0))
            .and_then(|_| file.read_to_end(&mut bytes))
            .map_err(|e| io_failed("reading the log", e))?;
        let scan = scan(&bytes)?;

        let mut host = PurposeBuiltHost {
            store,
            file,
            end: 0,
            formatted: false,
            halted: false,
            journals: BTreeMap::new(),
            orders: Vec::new(),
            by_id: BTreeMap::new(),
            refusals: Vec::new(),
            crash: None,
            stopped: None,
            #[cfg(test)]
            inject: None,
            machine: PhantomData,
        };
        let mut check = StreamCheck::new();
        check
            .push(&stream_header())
            .expect("a header opens a stream");
        let mut importing = false;
        let mut records = 0usize;
        for (n, entry) in scan.entries.into_iter().enumerate() {
            match entry {
                LogEntry::Format {
                    store_format,
                    importing: starts_an_import,
                    host_version,
                    state_machine_version,
                } if n == 0 => {
                    if store_format != STORE_FORMAT {
                        return Err(unavailable(format!(
                            "the store is in format {store_format}; this host reads format {STORE_FORMAT}"
                        )));
                    }
                    if host_version != contract::VERSION
                        || state_machine_version != machine::VERSION
                    {
                        return Err(unavailable(format!(
                            "the store was written under host contract {host_version} and state machine \
                             {state_machine_version}; this host reads {} and {}",
                            contract::VERSION,
                            machine::VERSION
                        )));
                    }
                    host.formatted = true;
                    importing = starts_an_import;
                }
                _ if n == 0 => return Err(unavailable("the log doesn't start with its format")),
                LogEntry::Format { .. } => {
                    return Err(unavailable("the log names its format twice"));
                }
                LogEntry::ImportComplete if importing => importing = false,
                LogEntry::ImportComplete => {
                    return Err(unavailable("an import completes only once it has begun"));
                }
                LogEntry::Halted => host.halted = true,
                LogEntry::Resumed => host.halted = false,
                LogEntry::Record(r) => {
                    let entry = SnapshotEntry::Record(r);
                    check.push(&entry).map_err(|v| unavailable(v.to_string()))?;
                    if let SnapshotEntry::Record(r) = &entry {
                        host.index_record(r);
                        records += 1;
                    }
                }
                LogEntry::Refusal(r) => {
                    let entry = SnapshotEntry::Refusal(r);
                    check.push(&entry).map_err(|v| unavailable(v.to_string()))?;
                    if let SnapshotEntry::Refusal(r) = entry {
                        host.refusals.push(r);
                    }
                }
            }
        }
        // An unfinished import is refused, torn or corrupt bytes after it included, and nothing is cut. An import
        // whose complete entry lies past the damage did finish: that damage is corruption, below.
        if importing && !scan.import_completed_past_it {
            return Err(HostError::ImportIncomplete);
        }
        if scan.damage.is_none() && scan.end == 0 && !bytes.is_empty() {
            return Err(unavailable(
                "the log's first frame is torn: nothing in it was ever durable, and it may have been an import's \
                 start, so it isn't cut; open a fresh store",
            ));
        }
        match scan.damage {
            Some(Damage::Corrupt) => {
                return Err(unavailable(format!(
                    "the log is corrupt at byte {}, inside entries that were synced; nothing was cut",
                    scan.end
                )));
            }
            Some(Damage::LongerThanOneAppend) => {
                return Err(unavailable(format!(
                    "the log's last {} bytes, from byte {}, hold no whole frame and are more than one append \
                     could leave, so they held synced entries; nothing was cut",
                    bytes.len() - scan.end,
                    scan.end
                )));
            }
            None => {}
        }
        check
            .push(&SnapshotEntry::End {
                records: count(records),
                refusals: count(host.refusals.len()),
            })
            .and_then(|()| check.finish())
            .map_err(|v| unavailable(format!("the log breaks the contract's rules: {v}")))?;
        if scan.end < bytes.len() {
            host.file
                .set_len(count(scan.end))
                .and_then(|()| host.file.sync_all())
                .map_err(|e| io_failed("cutting the log's torn tail", e))?;
        }
        host.end = count(scan.end);
        Ok(host)
    }

    fn refuse_if_stopped(&self) -> Result<(), HostError> {
        match &self.stopped {
            Some(reason) => Err(unavailable(reason.clone())),
            None => Ok(()),
        }
    }

    fn record_at(&self, at: &RecordRef) -> Option<&TransitionRecord> {
        self.journals
            .get(&at.order_id)?
            .get(usize::try_from(at.seq).ok()?.checked_sub(1)?)
    }

    /// Fault-seam machinery: stops the host if the suite set this crash point. Without one set, it does nothing.
    fn stop_at(&mut self, point: CrashPoint) -> Result<(), HostError> {
        if self.crash == Some(point) {
            let reason = format!("stopped by the harness at {point:?}");
            self.stopped = Some(reason.clone());
            return Err(unavailable(reason));
        }
        Ok(())
    }
}

/// A count as `u64`; a `usize` count always fits on the platforms rutent builds for.
fn count(n: usize) -> u64 {
    u64::try_from(n).expect("a count fits in u64")
}

impl<M: StateMachine> Host<M> for PurposeBuiltHost<M> {
    type Store = DiskStore;

    fn open(store: DiskStore) -> Result<Self, HostError> {
        // A child being spawned beside this open can hold a just-dropped host's lock for a moment (`open_retry`).
        let file = retry_open(OpenRetry::SPAWN_WINDOW, is_open_elsewhere, || {
            open_log(&store)
        })?;
        Self::open_locked(store, file)
    }

    fn submit(&mut self, input: RecordedInput) -> Result<Delivery, HostError> {
        self.refuse_if_stopped()?;
        if self.halted {
            return Err(HostError::Halted);
        }
        self.stop_at(CrashPoint::AfterReceived)?;

        if let Some(at) = self.by_id.get(&input.record_id).cloned() {
            let existing = self
                .record_at(&at)
                .cloned()
                .ok_or_else(|| unavailable("a record id points to no record"))?;
            if existing.input == input {
                return Ok(Delivery::AlreadyRecorded(existing));
            }
            let refusal = RefusalRecord {
                seq: count(self.refusals.len()) + 1,
                input,
                reason: RefusalReason::RecordIdReused,
                existing: at,
            };
            self.append(&LogEntry::Refusal(refusal.clone()))?;
            self.refusals.push(refusal.clone());
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
        self.append(&LogEntry::Record(record.clone()))?;
        self.index_record(&record);
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
        if !self.halted {
            self.append(&LogEntry::Halted)?;
            self.halted = true;
        }
        Ok(())
    }

    fn resume(&mut self) -> Result<(), HostError> {
        self.refuse_if_stopped()?;
        if self.halted {
            self.append(&LogEntry::Resumed)?;
            self.halted = false;
        }
        Ok(())
    }

    fn export(&self) -> Result<SnapshotStream<'_>, HostError> {
        if !self.halted {
            return Err(HostError::NotHalted);
        }
        let records = self.journals.values().map(Vec::len).sum::<usize>();
        let end = SnapshotEntry::End {
            records: count(records),
            refusals: count(self.refusals.len()),
        };
        let journals = self
            .orders
            .iter()
            .filter_map(|order| self.journals.get(order))
            .flatten()
            .map(|r| SnapshotEntry::Record(r.clone()));
        let refusals = self
            .refusals
            .iter()
            .map(|r| SnapshotEntry::Refusal(r.clone()));
        Ok(Box::new(
            std::iter::once(stream_header())
                .chain(journals)
                .chain(refusals)
                .chain(std::iter::once(end))
                .map(Ok),
        ))
    }

    fn import(store: DiskStore, stream: SnapshotStream<'_>) -> Result<Self, HostError> {
        // The log stays locked from the import's first write to the open that recovers it.
        let (store, file) = import_onto(store, stream, None)?;
        Self::open_locked(store, file)
    }
}

/// The header of a stream written under this crate's contract versions.
fn stream_header() -> SnapshotEntry {
    SnapshotEntry::Header {
        host_version: Version::new(contract::VERSION)
            .expect("the host contract's version is MAJOR.MINOR.PATCH"),
        state_machine_version: Version::new(machine::VERSION)
            .expect("the state-machine contract's version is MAJOR.MINOR.PATCH"),
    }
}

/// Writes one frame to an import's log without syncing it.
fn write_frame(file: &mut File, entry: &LogEntry) -> Result<(), HostError> {
    let mut buf = Vec::new();
    frame(entry, &mut buf)?;
    file.write_all(&buf)
        .map_err(|e| io_failed("writing the import", e))
}

/// Writes one frame to an import's log and syncs it: one of the import's marks.
fn write_mark(file: &mut File, entry: &LogEntry) -> Result<(), HostError> {
    write_frame(file, entry)?;
    sync(file)
}

fn sync(file: &File) -> Result<(), HostError> {
    file.sync_data()
        .map_err(|e| io_failed("syncing the import", e))
}

/// Reads a stream onto an empty store, checking it as it goes, and returns the store with its log still open and
/// locked. The format entry that starts an import is written and synced first, as one frame, then the records,
/// synced together, then halted and import-complete, each written and synced: a store whose complete entry is on disk
/// holds the whole stream. With a limit, stops abruptly after that many entries, leaving the import unfinished.
///
/// Like the reference host, it trusts each imported record's `state_after` and doesn't replay the inputs through
/// `M`: [`StreamCheck`] and the records' own checks hold the stream's shape, ids and sequences, and the `replay`
/// guarantee is checked on the host that wrote the records.
fn import_onto(
    store: DiskStore,
    stream: SnapshotStream<'_>,
    limit: Option<u64>,
) -> Result<(DiskStore, File), HostError> {
    let mut file = open_log(&store)?;
    let len = file
        .metadata()
        .map_err(|e| io_failed("reading the log's length", e))?
        .len();
    if len != 0 {
        return Err(refused("an import needs an empty store"));
    }
    // One frame, one write, one sync: there is no moment at which the log holds a format entry that doesn't say
    // it starts an import.
    write_mark(&mut file, &format_entry(true))?;
    let mut check = StreamCheck::new();
    for (n, entry) in stream.enumerate() {
        if limit == Some(count(n)) {
            return Ok((store, file));
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
            SnapshotEntry::Record(r) => write_frame(&mut file, &LogEntry::Record(r))?,
            SnapshotEntry::Refusal(r) => write_frame(&mut file, &LogEntry::Refusal(r))?,
            SnapshotEntry::End { .. } => {}
        }
    }
    check.finish().map_err(|e| refused(e.to_string()))?;
    sync(&file)?;
    write_mark(&mut file, &LogEntry::Halted)?;
    write_mark(&mut file, &LogEntry::ImportComplete)?;
    Ok((store, file))
}

/// The purpose-built store's harness for the host-contract suite: in-process, so it implements [`FaultSeam`] too.
/// Each fresh store is a new directory under one the harness makes in the platform's temp directory and removes
/// when it is dropped. A kill drops the host, closing the log and releasing its lock, and keeps its directory: with
/// nothing buffered in user space, that leaves the log as a killed process would.
#[derive(Debug)]
pub struct PurposeBuiltHarness<M> {
    root: PathBuf,
    next: u64,
    machine: PhantomData<M>,
}

impl<M> PurposeBuiltHarness<M> {
    /// A harness with its own directory in the platform's temp directory.
    pub fn new() -> Self {
        static HARNESSES: AtomicU64 = AtomicU64::new(0);
        let n = HARNESSES.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rutent-purpose-built-store-{}-{n}",
            std::process::id()
        ));
        PurposeBuiltHarness {
            root,
            next: 0,
            machine: PhantomData,
        }
    }

    /// The directory the harness keeps its stores in.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl<M> Default for PurposeBuiltHarness<M> {
    fn default() -> Self {
        PurposeBuiltHarness::new()
    }
}

impl<M> Drop for PurposeBuiltHarness<M> {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn runtime(what: &str, e: std::io::Error) -> HarnessError {
    HarnessError::Runtime {
        reason: format!("{what}: {e}"),
    }
}

impl<M: StateMachine> Harness<M> for PurposeBuiltHarness<M> {
    type Host = PurposeBuiltHost<M>;

    fn fresh_store(&mut self) -> Result<DiskStore, HarnessError> {
        let dir = self.root.join(format!("store-{}", self.next));
        self.next += 1;
        fs::create_dir_all(&dir).map_err(|e| runtime("making a fresh store", e))?;
        Ok(DiskStore::at(dir))
    }

    fn start(&mut self, store: DiskStore) -> Result<PurposeBuiltHost<M>, HarnessError> {
        Ok(PurposeBuiltHost::open(store)?)
    }

    fn kill(&mut self, host: PurposeBuiltHost<M>) -> Result<DiskStore, HarnessError> {
        Ok(host.into_store())
    }

    fn reopen(&mut self, store: DiskStore) -> Result<PurposeBuiltHost<M>, HarnessError> {
        Ok(PurposeBuiltHost::open(store)?)
    }

    fn import(
        &mut self,
        store: DiskStore,
        stream: SnapshotStream<'_>,
    ) -> Result<PurposeBuiltHost<M>, HarnessError> {
        Ok(PurposeBuiltHost::import(store, stream)?)
    }

    fn import_crashing_after(
        &mut self,
        store: DiskStore,
        stream: SnapshotStream<'_>,
        entries: u64,
    ) -> Result<DiskStore, HarnessError> {
        Ok(import_onto(store, stream, Some(entries))?.0)
    }
}

impl<M: StateMachine> FaultSeam<M> for PurposeBuiltHarness<M> {
    fn crash_at(&mut self, host: &mut PurposeBuiltHost<M>, point: CrashPoint) {
        host.crash = Some(point);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: NFR-5, INV-7
    #[test]
    fn the_frame_checksum_is_crc_32_ieee() {
        assert_eq!(crc32(&[b"123456789"]), 0xCBF4_3926);
        assert_eq!(crc32(&[b"1234", b"56789"]), 0xCBF4_3926);
    }

    use rutent_conformance::host::probe::{self, Probe};

    type Pb = PurposeBuiltHost<Probe>;

    /// A host holding the first three inputs of one order, its harness, the store, the inputs and the log's length.
    fn three_records() -> (PurposeBuiltHarness<Probe>, Pb, Vec<RecordedInput>, u64) {
        let mut harness = PurposeBuiltHarness::<Probe>::new();
        let store = harness.fresh_store().unwrap();
        let inputs = probe::order_inputs("ord-1", 0);
        let mut host = Pb::open(store.clone()).unwrap();
        for input in &inputs[..3] {
            host.submit(input.clone()).unwrap();
        }
        let len = fs::metadata(store.log_path()).unwrap().len();
        (harness, host, inputs, len)
    }

    /// After a failed append the host answers `Unavailable` to everything that could change it, and nothing of the
    /// failed submit is readable.
    fn assert_stopped(host: &mut Pb, inputs: &[RecordedInput]) {
        for (what, result) in [
            ("a submit", host.submit(inputs[4].clone()).map(|_| ())),
            ("a halt", host.halt()),
            ("a resume", host.resume()),
        ] {
            assert!(
                matches!(result, Err(HostError::Unavailable { .. })),
                "{what} after the failed append gave {result:?}"
            );
        }
        assert_eq!(host.journal(&inputs[0].subject.order).unwrap().len(), 3);
    }

    /// The failed append was undone: the log is back to its length, and reopened, the store records the input at
    /// the next sequence number.
    fn assert_undone(
        harness: &mut PurposeBuiltHarness<Probe>,
        host: Pb,
        inputs: &[RecordedInput],
        len: u64,
    ) {
        let store = harness.kill(host).unwrap();
        assert_eq!(fs::metadata(store.log_path()).unwrap().len(), len);
        assert!(!store.unverified_path().exists());
        let mut host = Pb::open(store).unwrap();
        assert_eq!(host.journal(&inputs[0].subject.order).unwrap().len(), 3);
        match host.submit(inputs[3].clone()) {
            Ok(Delivery::Recorded(r)) => assert_eq!(r.seq, 4),
            other => panic!("the input whose append failed, submitted again, gave {other:?}"),
        }
    }

    // verifies: NFR-5, INV-7, REQ-STM-4
    #[test]
    fn a_failed_write_is_undone_and_stops_the_host_until_it_is_opened_again() {
        let (mut harness, mut host, inputs, len) = three_records();
        // The seam writes half the frame, then fails.
        host.inject = Some(Inject::WriteFails);
        assert!(matches!(
            host.submit(inputs[3].clone()),
            Err(HostError::Unavailable { .. })
        ));
        assert_stopped(&mut host, &inputs);
        assert_undone(&mut harness, host, &inputs, len);
    }

    // verifies: NFR-5, INV-7, REQ-STM-4
    #[test]
    fn a_failed_sync_is_undone_so_its_frame_is_never_read_back_as_durable() {
        let (mut harness, mut host, inputs, len) = three_records();
        // The seam lets the whole frame be written, then fails its sync.
        host.inject = Some(Inject::SyncFails);
        assert!(matches!(
            host.submit(inputs[3].clone()),
            Err(HostError::Unavailable { .. })
        ));
        assert_stopped(&mut host, &inputs);
        assert_undone(&mut harness, host, &inputs, len);
    }

    // verifies: NFR-5, INV-7, REQ-STM-4
    #[test]
    fn an_append_that_cannot_be_undone_refuses_the_store_until_its_log_is_verified() {
        let (mut harness, mut host, inputs, len) = three_records();
        host.inject = Some(Inject::SyncAndUndoFail);
        match host.submit(inputs[3].clone()) {
            Err(HostError::Unavailable { reason }) => {
                assert!(reason.contains("verified"), "{reason}")
            }
            other => panic!("a failed append that couldn't be undone gave {other:?}"),
        }
        assert_stopped(&mut host, &inputs);
        let store = harness.kill(host).unwrap();
        // The frame whose sync failed is still in the log, so the store is refused, and nothing is cut.
        let log = fs::read(store.log_path()).unwrap();
        assert!(log.len() as u64 > len);
        assert!(store.unverified_path().exists());
        match Pb::open(store.clone()) {
            Err(HostError::Unavailable { reason }) => {
                assert!(reason.contains("unverified"), "{reason}")
            }
            other => panic!("an unverified store opened: {:?}", other.map(|_| ())),
        }
        assert_eq!(fs::read(store.log_path()).unwrap(), log);
        // Once an operator has verified the log and removed the marker, it opens.
        fs::remove_file(store.unverified_path()).unwrap();
        Pb::open(store).unwrap();
    }

    // verifies: NFR-5, INV-7
    #[test]
    fn a_real_write_and_undo_failure_marks_the_store_unverified() {
        let (mut harness, mut host, inputs, _) = three_records();
        // No injection: the log's handle is swapped for one opened read-only, so the write and the undo both fail
        // as a failing disk's would. The locked handle is kept until the kill.
        let log_path = host.store.log_path();
        let locked = std::mem::replace(&mut host.file, File::open(&log_path).unwrap());
        assert!(matches!(
            host.submit(inputs[3].clone()),
            Err(HostError::Unavailable { .. })
        ));
        assert_stopped(&mut host, &inputs);
        let store = harness.kill(host).unwrap();
        drop(locked);
        assert!(store.unverified_path().exists());
        assert!(matches!(
            Pb::open(store),
            Err(HostError::Unavailable { .. })
        ));
    }

    // verifies: NFR-5, INV-7
    #[test]
    fn a_zero_filled_tail_never_reads_as_a_frame_nor_is_cut_as_a_torn_append() {
        let mut log = Vec::new();
        frame(&LogEntry::Halted, &mut log).unwrap();
        let whole = log.len();
        log.extend_from_slice(&[0; 64]);
        let found = scan(&log).unwrap();
        assert_eq!(found.entries, vec![LogEntry::Halted]);
        assert_eq!(found.end, whole);
        assert_eq!(found.damage, Some(Damage::LongerThanOneAppend));
    }

    /// A store holding one frame: a format entry, then a halt.
    fn store_with_format(harness: &mut PurposeBuiltHarness<Probe>, format: LogEntry) -> DiskStore {
        let store = harness.fresh_store().unwrap();
        let mut log = Vec::new();
        frame(&format, &mut log).unwrap();
        frame(&LogEntry::Halted, &mut log).unwrap();
        fs::write(store.log_path(), log).unwrap();
        store
    }

    // verifies: NFR-5, REQ-OPS-3, NFR-6
    #[test]
    fn a_store_in_another_format_or_under_other_contract_versions_is_refused() {
        let mut harness = PurposeBuiltHarness::<Probe>::new();
        let format = |store_format, host: &str, machine: &str| LogEntry::Format {
            store_format,
            importing: false,
            host_version: host.into(),
            state_machine_version: machine.into(),
        };
        let ours = store_with_format(&mut harness, format_entry(false));
        assert!(Pb::open(ours).is_ok(), "the store's own format is refused");
        for (what, entry) in [
            ("format 2", format(2, contract::VERSION, machine::VERSION)),
            (
                "another host contract",
                format(STORE_FORMAT, "9.9.9", machine::VERSION),
            ),
            (
                "another state machine",
                format(STORE_FORMAT, contract::VERSION, "9.9.9"),
            ),
        ] {
            let store = store_with_format(&mut harness, entry);
            let before = fs::read(store.log_path()).unwrap();
            assert!(
                matches!(Pb::open(store.clone()), Err(HostError::Unavailable { .. })),
                "a store in {what} opened"
            );
            assert_eq!(fs::read(store.log_path()).unwrap(), before, "{what}");
        }
    }

    // verifies: REQ-OPS-3, NFR-6
    #[test]
    fn the_lock_is_held_from_an_import_to_the_open_that_recovers_it() {
        let mut harness = PurposeBuiltHarness::<Probe>::new();
        let mut source = Pb::open(harness.fresh_store().unwrap()).unwrap();
        for input in probe::order_inputs("ord-1", 0) {
            source.submit(input).unwrap();
        }
        source.halt().unwrap();
        let target = harness.fresh_store().unwrap();
        let (store, file) = import_onto(target, source.export().unwrap(), None).unwrap();
        // Between the import and the open that follows it, no other host can take the store.
        match Pb::open(store.clone()) {
            Err(HostError::Unavailable { reason }) => {
                assert!(reason.contains("another host"), "{reason}")
            }
            other => panic!(
                "a second host opened an imported store: {:?}",
                other.map(|_| ())
            ),
        }
        let host = Pb::open_locked(store, file).unwrap();
        assert_eq!(host.orders().unwrap().len(), 1);
    }

    // verifies: NFR-5, INV-7
    #[test]
    fn a_torn_append_is_at_most_one_frame() {
        let mut log = Vec::new();
        frame(&LogEntry::Halted, &mut log).unwrap();
        let one = log.len();
        assert!(one_torn_append(&log[..FRAME_HEADER - 1]));
        assert!(one_torn_append(&log[..FRAME_HEADER]));
        assert!(one_torn_append(&log[..one - 1]));
        assert!(one_torn_append(&log));
        frame(&LogEntry::Resumed, &mut log).unwrap();
        assert!(!one_torn_append(&log));
        assert!(!one_torn_append(&[0; FRAME_HEADER]));
    }
}
