//! The platform's carried-out record (`T-013-10`; `INV-8`, `NFR-6`, `REQ-CAT-10`): what the platform carried out
//! under each transaction, the segments in flight and the transactions that ended, and how many operations each
//! provider's version ran, kept where a restart of the platform's process doesn't lose them.
//!
//! The record is a port, [`Record`]: an append-only sequence of entries, each one line of JSON the platform writes
//! and reads back, and never interprets anywhere else. The platform writes an entry before it acts on it: the
//! operation it is about to hand a module before the hand-over, so a platform stopped while the module runs reopens
//! with the operation carried out and no answer, which a repeat is refused on, or run again only where the provider
//! declares a repeat safe. When the platform opens on a record ([`Platform::open`](super::platform::Platform::open))
//! it reads every entry back, rebuilds what it held, and replaces the record with what it rebuilt, so the record
//! holds what is live and not every operation ever carried out.
//!
//! Two implementations: [`FileRecord`], on a file the platform holds an exclusive lock on, each entry synced to
//! disk before the platform goes on; and [`MemoryRecord`], for tests that need no disk, whose clones share one
//! sequence, so a platform opened on a clone reads what an earlier one wrote.
//!
//! This is the platform's admission lifecycle's state, a `DEC-20` path.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rutent_contracts::provider_role::{Operation, ResultBody, Statement};
use serde::{Deserialize, Serialize};

use crate::catalogue::Instant;
use crate::module::lifecycle::{Move, MoveReason, SegmentRecord};

/// Where the platform keeps its carried-out record: a sequence of lines, each one entry, which the platform writes
/// and reads and nothing else interprets.
pub trait Record {
    /// Every line written, in order. A line the record couldn't finish writing (a stop mid-write) is not one: the
    /// platform wrote nothing it relies on until the line was written whole.
    fn load(&mut self) -> io::Result<Vec<String>>;

    /// Appends `line`, which holds no line break, and returns once it would survive a stop of the process.
    fn append(&mut self, line: &str) -> io::Result<()>;

    /// Replaces every line with `lines`, at once: a stop part-way leaves either the old lines or the new.
    fn replace(&mut self, lines: &[String]) -> io::Result<()>;
}

/// A record in memory. Its clones share one sequence: a platform opened on a clone reads what another wrote, as a
/// platform reopened on a file would.
#[derive(Debug, Clone, Default)]
pub struct MemoryRecord {
    lines: Arc<Mutex<Vec<String>>>,
}

impl MemoryRecord {
    /// An empty record.
    pub fn new() -> Self {
        Self::default()
    }

    /// The lines it holds.
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().map(|l| l.clone()).unwrap_or_default()
    }

    fn held(&self) -> io::Result<std::sync::MutexGuard<'_, Vec<String>>> {
        self.lines
            .lock()
            .map_err(|_| io::Error::other("the record's lock was poisoned"))
    }
}

impl Record for MemoryRecord {
    fn load(&mut self) -> io::Result<Vec<String>> {
        Ok(self.held()?.clone())
    }

    fn append(&mut self, line: &str) -> io::Result<()> {
        self.held()?.push(line.to_owned());
        Ok(())
    }

    fn replace(&mut self, lines: &[String]) -> io::Result<()> {
        *self.held()? = lines.to_vec();
        Ok(())
    }
}

/// A record on a file: one entry per line, each appended and synced to disk before the platform goes on. A lock
/// file beside it (its name and `.lock`) is held for as long as the record is open, so two platforms never share
/// one record: a second opening is refused while the first holds it.
#[derive(Debug)]
pub struct FileRecord {
    path: PathBuf,
    file: File,
    /// Held, never read: the lock is released when it drops.
    _lock: File,
}

impl FileRecord {
    /// Opens the record at `path`, creating it if it doesn't exist, and takes its lock. Refused, as
    /// `WouldBlock`, while another record holds it.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(beside(&path, ".lock"))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!("{}: another platform holds this record", path.display()),
                ));
            }
            Err(TryLockError::Error(e)) => return Err(e),
        }
        let file = appending(&path)?;
        Ok(Self {
            path,
            file,
            _lock: lock,
        })
    }

    /// The file it is kept in.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The file `path` opened for appending, created if it doesn't exist.
fn appending(path: &Path) -> io::Result<File> {
    OpenOptions::new().append(true).create(true).open(path)
}

/// `path` with `suffix` added to its name.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Each of `lines` with its line break, refused if one holds a line break of its own.
fn text_of<'a>(lines: impl IntoIterator<Item = &'a str>) -> io::Result<Vec<u8>> {
    let mut text = Vec::new();
    for line in lines {
        if line.contains('\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "an entry holds a line break",
            ));
        }
        text.extend_from_slice(line.as_bytes());
        text.push(b'\n');
    }
    Ok(text)
}

impl Record for FileRecord {
    fn load(&mut self) -> io::Result<Vec<String>> {
        let mut text = String::new();
        File::open(&self.path)?.read_to_string(&mut text)?;
        Ok(whole_lines(&text))
    }

    fn append(&mut self, line: &str) -> io::Result<()> {
        self.file.write_all(&text_of([line])?)?;
        self.file.sync_data()
    }

    fn replace(&mut self, lines: &[String]) -> io::Result<()> {
        let text = text_of(lines.iter().map(String::as_str))?;
        let next = beside(&self.path, ".replacing");
        {
            let mut file = File::create(&next)?;
            file.write_all(&text)?;
            file.sync_all()?;
        }
        fs::rename(&next, &self.path)?;
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            // The rename is durable once the directory is: best effort where the system can't sync one.
            let _ = File::open(dir).and_then(|d| d.sync_all());
        }
        // Appends go to the new file.
        self.file = appending(&self.path)?;
        Ok(())
    }
}

/// The lines of `text` that end in a line break: a last line without one was cut off as it was written.
fn whole_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    // Whatever follows the last line break is unfinished, or nothing.
    lines.pop();
    lines
}

/// One entry of the record, as the platform writes it. See [`Platform::open`](super::platform::Platform::open)
/// for how it is read back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Entry {
    /// The platform is about to hand `operation`, of `transaction` in `scope`, to `provider`'s module. Written
    /// before the hand-over; until a [`Entry::Ran`] or [`Entry::Withdrawn`] of it follows, it is carried out with
    /// no answer.
    Carrying {
        transaction: String,
        scope: String,
        provider: String,
        operation: Box<Statement<Operation>>,
    },
    /// The provider's `version` ran an operation, and, where it was one the platform carries out once, what it
    /// carried out: the record of its transaction in its scope from then on.
    Ran {
        provider: String,
        version: String,
        carried: Option<Box<CarriedEntry>>,
    },
    /// The operation written as [`Entry::Carrying`] never reached the module: nothing of it ran.
    Withdrawn { transaction: String, scope: String },
    /// What was carried out under a transaction in a scope, as the platform rebuilt it on opening.
    Carried(Box<CarriedEntry>),
    /// How many operations the provider's `version` had run, as the platform rebuilt it on opening.
    Runs {
        provider: String,
        version: String,
        count: u64,
    },
    /// A segment in flight, as it began or after it moved: the last one written for a transaction holds.
    Segment(SegmentEntry),
    /// The transaction ended: its segment ended, or its record was freed after its provider's evidence window. It
    /// never begins or carries out anything again.
    Ended { transaction: String },
}

/// What was carried out under a transaction in one scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CarriedEntry {
    pub(crate) transaction: String,
    pub(crate) scope: String,
    pub(crate) provider: String,
    pub(crate) operation: Statement<Operation>,
    pub(crate) answer: Option<Statement<ResultBody>>,
    /// When, in seconds of the world's clock, the record may be freed: its provider's evidence window after it was
    /// carried out, for one no segment held. `None` is never: a segment's record is freed when it ends.
    pub(crate) free_at: Option<u64>,
}

/// A segment in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SegmentEntry {
    pub(crate) transaction: String,
    pub(crate) provider: String,
    pub(crate) began_on: String,
    pub(crate) runs_on: String,
    pub(crate) moves: Vec<MoveEntry>,
}

/// One move of a segment, for a stale mark (`REQ-MDL-19`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MoveEntry {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) at: u64,
}

impl From<&SegmentRecord> for SegmentEntry {
    fn from(segment: &SegmentRecord) -> Self {
        Self {
            transaction: segment.transaction.clone(),
            provider: segment.provider.clone(),
            began_on: segment.began_on.clone(),
            runs_on: segment.runs_on.clone(),
            moves: segment
                .moves
                .iter()
                .map(|m| {
                    // Each reason a segment moves for is written as itself: a new one is a new entry.
                    let MoveReason::Stale = m.reason;
                    MoveEntry {
                        from: m.from.clone(),
                        to: m.to.clone(),
                        at: m.at.0,
                    }
                })
                .collect(),
        }
    }
}

impl From<SegmentEntry> for SegmentRecord {
    fn from(segment: SegmentEntry) -> Self {
        Self {
            transaction: segment.transaction,
            provider: segment.provider,
            began_on: segment.began_on,
            runs_on: segment.runs_on,
            moves: segment
                .moves
                .into_iter()
                .map(|m| Move {
                    from: m.from,
                    to: m.to,
                    reason: MoveReason::Stale,
                    at: Instant(m.at),
                })
                .collect(),
        }
    }
}

impl Entry {
    /// The entry as one line.
    pub(crate) fn line(&self) -> io::Result<String> {
        serde_json::to_string(self).map_err(io::Error::other)
    }

    /// The entry a line holds.
    pub(crate) fn read(line: &str) -> io::Result<Self> {
        serde_json::from_str(line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rutent-record-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch directory");
        dir.join("record")
    }

    // verifies: INV-8, NFR-6
    #[test]
    fn a_file_record_reads_back_what_it_appended_and_drops_a_line_cut_off() {
        let path = scratch("cut");
        {
            let mut record = FileRecord::open(&path).expect("opens");
            record.append("one").expect("appends");
            record.append("two").expect("appends");
        }
        // A stop mid-write leaves a line with no line break.
        OpenOptions::new()
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(b"{\"entry\":\"car"))
            .expect("writes");
        let mut record = FileRecord::open(&path).expect("reopens");
        assert_eq!(record.load().expect("loads"), ["one", "two"]);
        record.replace(&["three".to_owned()]).expect("replaces");
        record.append("four").expect("appends");
        drop(record);
        let mut record = FileRecord::open(&path).expect("reopens");
        assert_eq!(record.load().expect("loads"), ["three", "four"]);
        let _ = fs::remove_dir_all(path.parent().expect("a directory"));
    }

    // verifies: INV-8
    #[test]
    fn a_file_record_is_held_by_one_platform_at_a_time() {
        let path = scratch("lock");
        let mut first = FileRecord::open(&path).expect("opens");
        let second = FileRecord::open(&path).expect_err("held");
        assert_eq!(second.kind(), io::ErrorKind::WouldBlock);
        // Still held once replaced, which swaps the file the entries are kept in.
        first.replace(&[]).expect("replaces");
        assert_eq!(
            FileRecord::open(&path).expect_err("held").kind(),
            io::ErrorKind::WouldBlock
        );
        drop(first);
        FileRecord::open(&path).expect("free once the first is dropped");
        let _ = fs::remove_dir_all(path.parent().expect("a directory"));
    }

    // verifies: NFR-6
    #[test]
    fn memory_records_share_their_lines_between_clones() {
        let mut first = MemoryRecord::new();
        first.append("one").expect("appends");
        let mut second = first.clone();
        assert_eq!(second.load().expect("loads"), ["one"]);
        second.replace(&[]).expect("replaces");
        assert!(first.lines().is_empty());
    }
}
