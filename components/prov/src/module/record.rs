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

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rutent_contracts::provider_role::{Operation, ResultBody, Statement};
use serde::{Deserialize, Serialize};

use crate::catalogue::Instant;
use crate::module::lifecycle::{
    Counts, Decision, DecisionKind, Mark, Move, MoveReason, ReleaseState, SegmentRecord, Staged,
    Staging,
};
use rutent_contracts::module_interface::Identifier;

/// Where the platform keeps its carried-out record: a sequence of lines, each one entry, which the platform writes
/// and reads and nothing else interprets.
///
/// **After a write fails, every later write fails until the record is opened again.** Once an `append` or a
/// `replace` returns an error, an implementation refuses every later `append` and `replace` on the same value: it
/// never recovers in place. The platform relies on this: a decision it takes in memory where the record couldn't
/// hold it (`LifecycleRefusal::TakenUnrecorded`) is written by no later entry, since each entry holds only what
/// changed since what the platform holds, so a record that took writes again would hold a later state without the
/// mark the platform acted on. The platform also holds itself to it, whatever the implementation does: after one
/// failed write it sends the record nothing more until a platform opens on it again (`T-013-10`).
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

/// A record on a file: one entry per line, each appended and synced to disk before the platform goes on.
///
/// - **Created once, then opened.** [`FileRecord::create`] makes a new, empty record and refuses a path where
///   anything is already; [`FileRecord::open`] opens one that exists and refuses a missing one, so a mistyped or
///   lost path never opens as a fresh platform with nothing carried out.
/// - **One name, one holder.** The record is opened by its canonical path, every directory's symbolic link
///   resolved, and refused where the path is itself a symbolic link or, on Unix, the file has another name (a hard
///   link): a lock file beside the canonical path (its name and `.lock`) is held for as long as the record is open,
///   so two platforms never share one record, whatever name they reach it by.
/// - **A write that fails leaves the record whole.** An append cut short (a full disk, an I/O error) is cut back to
///   the last whole line, and every later write is refused until the record is opened again, so no entry is ever
///   glued to a fragment and a later opening reads every entry written before it. The platform refuses each
///   operation it then can't record (`Unavailable`).
/// - **A replacement is durable or refused.** [`Record::replace`] writes the new lines to `<name>.replacing`,
///   syncs it, renames it over the record and syncs the directory; a directory sync that fails refuses it (only a
///   system that can't sync a directory at all is let through).
#[derive(Debug)]
pub struct FileRecord {
    path: PathBuf,
    file: File,
    /// Held, never read: the lock is released when it drops.
    _lock: File,
    /// A write failed: every later one is refused until the record is opened again.
    broken: bool,
    /// The tests' short write: the next append writes this many bytes of its line, and fails.
    #[cfg(test)]
    cut_next: Option<usize>,
}

impl FileRecord {
    /// Creates a new, empty record at `path`, durably, and opens it. Refused, as `AlreadyExists`, where anything
    /// is at `path` already, a symbolic link included.
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .sync_all()?;
        sync_directory_of(path)?;
        Self::open(path)
    }

    /// Opens the record at `path`, which [`FileRecord::create`] made, and takes its lock. Refused as `NotFound`
    /// where there is none, as `InvalidInput` where `path` is a symbolic link, isn't a file, or names a file with
    /// another name, and as `WouldBlock` while another record holds it.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let given = path.as_ref();
        let meta = fs::symlink_metadata(given)?;
        if meta.file_type().is_symlink() {
            return Err(refused(
                given,
                "is a symbolic link: open the record by its own path",
            ));
        }
        if !meta.is_file() {
            return Err(refused(given, "is not a file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            if meta.nlink() != 1 {
                return Err(refused(given, "has another name (a hard link)"));
            }
        }
        let path = fs::canonicalize(given)?;
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
        let file = OpenOptions::new().append(true).open(&path)?;
        Ok(Self {
            path,
            file,
            _lock: lock,
            broken: false,
            #[cfg(test)]
            cut_next: None,
        })
    }

    /// The file it is kept in, by its canonical path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Refused once a write failed.
    fn usable(&self) -> io::Result<()> {
        if self.broken {
            return Err(io::Error::other(format!(
                "{}: a write to this record failed; it takes no more until it is opened again",
                self.path.display()
            )));
        }
        Ok(())
    }

    /// Writes `bytes` at the end of the file.
    fn write_end(&mut self, bytes: &[u8]) -> io::Result<()> {
        #[cfg(test)]
        if let Some(cut) = self.cut_next.take() {
            self.file.write_all(&bytes[..cut.min(bytes.len())])?;
            return Err(io::Error::other("the write was cut short"));
        }
        self.file.write_all(bytes)
    }
}

/// `path` refused for `why`.
fn refused(path: &Path, why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{}: {why}", path.display()),
    )
}

/// Syncs the directory `path` is in, so a name just made or renamed in it survives a stop. A system that can't
/// sync a directory at all (`Unsupported`, `EINVAL`) is let through; any other failure is an error.
fn sync_directory_of(path: &Path) -> io::Result<()> {
    /// `EINVAL`, the same on Linux and macOS: the descriptor can't be synced.
    const EINVAL: i32 = 22;
    let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return Ok(());
    };
    #[cfg(test)]
    let synced = match tests::DIR_SYNC_FAILS.take() {
        Some(fails) => Err(fails()),
        None => File::open(dir).and_then(|d| d.sync_all()),
    };
    #[cfg(not(test))]
    let synced = File::open(dir).and_then(|d| d.sync_all());
    match synced {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::Unsupported || e.raw_os_error() == Some(EINVAL) => {
            Ok(())
        }
        Err(e) => Err(e),
    }
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
        self.usable()?;
        let bytes = text_of([line])?;
        let before = self.file.metadata()?.len();
        let written = self.write_end(&bytes).and_then(|()| self.file.sync_data());
        if let Err(e) = written {
            // No later entry is glued to what this one left: the record takes nothing more until it is opened
            // again, and is cut back to its last whole line. A cut that fails too leaves a fragment with no line
            // break at the end, which an opening doesn't read.
            self.broken = true;
            let _ = self
                .file
                .set_len(before)
                .and_then(|()| self.file.sync_data());
            return Err(e);
        }
        Ok(())
    }

    fn replace(&mut self, lines: &[String]) -> io::Result<()> {
        self.usable()?;
        let replaced = (|| {
            let text = text_of(lines.iter().map(String::as_str))?;
            let next = beside(&self.path, ".replacing");
            {
                let mut file = File::create(&next)?;
                file.write_all(&text)?;
                file.sync_all()?;
            }
            fs::rename(&next, &self.path)?;
            // The rename is durable once the directory is.
            sync_directory_of(&self.path)?;
            // Appends go to the new file.
            OpenOptions::new().append(true).open(&self.path)
        })();
        match replaced {
            Ok(file) => {
                self.file = file;
                Ok(())
            }
            Err(e) => {
                // Which file the record's lines are in is no longer certain: nothing more is written to it.
                self.broken = true;
                Err(e)
            }
        }
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
        /// Whether it counted towards a staging as an answer (`true`) or as a refusal (`false`), where a release was
        /// staged to count it: replayed on opening, so the staging's counts survive a restart (`T-013-11`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answered: Option<bool>,
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
    /// A segment as it began, and the provider's releases as that left them where it changed the staging's counts
    /// (`T-013-11`).
    Began {
        segment: SegmentEntry,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        release: Option<Box<ReleasesEntry>>,
    },
    /// A provider's releases as they stood after a change, written before it took effect: which were accepted and
    /// by whom, the version in force, the staged one and its counts (`T-013-11`). The last one written for a
    /// provider holds.
    Releases(Box<ReleasesEntry>),
    /// The platform was dropped with its record whole: every decision it took is in the record. A record whose last
    /// entry isn't this one ended some other way, and what the platform held in memory beyond it, a restrictive
    /// decision it couldn't write, isn't known (`T-013-11`).
    Closed,
    /// The transaction ended: its segment ended, or its record was freed after its provider's evidence window. It
    /// never begins or carries out anything again.
    Ended { transaction: String },
    /// An operator's decision, written before the platform takes it: the marks it set on versions (stale,
    /// revoked, declared exploited), the segments it moved, and the decisions recorded with their actors. A mark
    /// written later replaces one written before for the same version.
    Decided {
        marks: Vec<MarkEntry>,
        segments: Vec<SegmentEntry>,
        decisions: Vec<DecisionEntry>,
        /// The releases the decision changed (promoting, rolling back, revoking the version in force).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        releases: Vec<ReleasesEntry>,
    },
}

/// One accepted release: its version, who accepted it (none for an automatic acceptance) and when, by the caller's
/// clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AcceptedEntry {
    pub(crate) version: String,
    /// The digest of the assertion that was accepted: only a release with this assertion is accepted again after a
    /// restart, never another under the same version (`REQ-MDL-1`, `REQ-MDL-3`).
    #[serde(default)]
    pub(crate) assertion_digest: String,
    pub(crate) by: Option<String>,
    pub(crate) at: u64,
}

/// What a version did while it was staged, or the one in force beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CountsEntry {
    pub(crate) segments: u64,
    pub(crate) answered: u64,
    pub(crate) refused: u64,
}

/// A staged release: when, under what policy, and what was counted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StagedEntry {
    pub(crate) version: String,
    pub(crate) since: u64,
    pub(crate) share: u8,
    pub(crate) window: u64,
    pub(crate) minimum: u64,
    pub(crate) began: u64,
    pub(crate) taken: u64,
    pub(crate) staged_counts: CountsEntry,
    pub(crate) in_force_counts: CountsEntry,
}

/// A provider's releases (`T-013-11`): the accepted ones in the order they were accepted, which is the order a
/// restart admits them again in, the version in force, those put in force in order, and the staged release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReleasesEntry {
    pub(crate) provider: String,
    pub(crate) accepted: Vec<AcceptedEntry>,
    pub(crate) in_force: Option<String>,
    pub(crate) history: Vec<String>,
    pub(crate) staged: Option<StagedEntry>,
}

impl From<Counts> for CountsEntry {
    fn from(counts: Counts) -> Self {
        Self {
            segments: counts.segments,
            answered: counts.answered,
            refused: counts.refused,
        }
    }
}

impl From<CountsEntry> for Counts {
    fn from(counts: CountsEntry) -> Self {
        Self {
            segments: counts.segments,
            answered: counts.answered,
            refused: counts.refused,
        }
    }
}

impl ReleasesEntry {
    /// `provider`'s `state`, its accepted releases being `accepted`, in the order they were accepted.
    pub(crate) fn of(provider: &str, state: &ReleaseState, accepted: &[AcceptedEntry]) -> Self {
        Self {
            provider: provider.to_owned(),
            accepted: accepted.to_vec(),
            in_force: state.in_force.clone(),
            history: state.history.clone(),
            staged: state.staged.as_ref().map(|staged| StagedEntry {
                version: staged.version.clone(),
                since: staged.since.0,
                share: staged.policy.share(),
                window: staged.policy.window(),
                minimum: staged.policy.minimum(),
                began: staged.began,
                taken: staged.taken,
                staged_counts: staged.staged_counts.into(),
                in_force_counts: staged.in_force_counts.into(),
            }),
        }
    }

    /// The releases the entry holds, as the lifecycle holds them.
    pub(crate) fn state(&self) -> io::Result<ReleaseState> {
        let staged =
            self.staged
                .as_ref()
                .map(|staged| -> io::Result<Staged> {
                    Ok(Staged {
                        version: staged.version.clone(),
                        since: Instant(staged.since),
                        policy: Staging::new(staged.share, staged.window, staged.minimum)
                            .ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "a staged release's share is outside 1 to 100",
                                )
                            })?,
                        began: staged.began,
                        taken: staged.taken,
                        staged_counts: staged.staged_counts.into(),
                        in_force_counts: staged.in_force_counts.into(),
                    })
                })
                .transpose()?;
        Ok(ReleaseState {
            accepted: self
                .accepted
                .iter()
                .map(|a| a.version.clone())
                .collect::<BTreeSet<_>>(),
            in_force: self.in_force.clone(),
            history: self.history.clone(),
            staged,
        })
    }
}

/// The operator's mark on a provider's version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MarkEntry {
    pub(crate) provider: String,
    pub(crate) version: String,
    pub(crate) mark: MarkKind,
}

/// [`Mark`], as the record writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MarkKind {
    Revoked,
    Exploited,
    Stale,
}

impl From<Mark> for MarkKind {
    fn from(mark: Mark) -> Self {
        match mark {
            Mark::Revoked => Self::Revoked,
            Mark::Exploited => Self::Exploited,
            Mark::Stale => Self::Stale,
        }
    }
}

impl From<MarkKind> for Mark {
    fn from(mark: MarkKind) -> Self {
        match mark {
            MarkKind::Revoked => Self::Revoked,
            MarkKind::Exploited => Self::Exploited,
            MarkKind::Stale => Self::Stale,
        }
    }
}

/// One recorded lifecycle decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionEntry {
    pub(crate) provider: String,
    pub(crate) version: String,
    pub(crate) kind: DecisionKindEntry,
    pub(crate) by: String,
    pub(crate) at: u64,
}

/// [`DecisionKind`], as the record writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DecisionKindEntry {
    Withdrawn,
    Promoted,
    RolledBack,
    Revoked,
    MarkedStale,
    DeclaredExploited,
    Reinstated,
    Confirmed,
}

impl From<&Decision> for DecisionEntry {
    fn from(decision: &Decision) -> Self {
        Self {
            provider: decision.provider.clone(),
            version: decision.version.clone(),
            kind: match decision.kind {
                DecisionKind::Withdrawn => DecisionKindEntry::Withdrawn,
                DecisionKind::Promoted => DecisionKindEntry::Promoted,
                DecisionKind::RolledBack => DecisionKindEntry::RolledBack,
                DecisionKind::Revoked => DecisionKindEntry::Revoked,
                DecisionKind::MarkedStale => DecisionKindEntry::MarkedStale,
                DecisionKind::DeclaredExploited => DecisionKindEntry::DeclaredExploited,
                DecisionKind::Reinstated => DecisionKindEntry::Reinstated,
                DecisionKind::Confirmed => DecisionKindEntry::Confirmed,
            },
            by: decision.by.as_str().to_owned(),
            at: decision.at.0,
        }
    }
}

impl TryFrom<DecisionEntry> for Decision {
    type Error = io::Error;

    fn try_from(decision: DecisionEntry) -> io::Result<Self> {
        Ok(Self {
            provider: decision.provider,
            version: decision.version,
            kind: match decision.kind {
                DecisionKindEntry::Withdrawn => DecisionKind::Withdrawn,
                DecisionKindEntry::Promoted => DecisionKind::Promoted,
                DecisionKindEntry::RolledBack => DecisionKind::RolledBack,
                DecisionKindEntry::Revoked => DecisionKind::Revoked,
                DecisionKindEntry::MarkedStale => DecisionKind::MarkedStale,
                DecisionKindEntry::DeclaredExploited => DecisionKind::DeclaredExploited,
                DecisionKindEntry::Reinstated => DecisionKind::Reinstated,
                DecisionKindEntry::Confirmed => DecisionKind::Confirmed,
            },
            by: Identifier::new(decision.by).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "a decision's actor is no identifier",
                )
            })?,
            at: Instant(decision.at),
        })
    }
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
    /// The version of the provider that carried it out, so a question about it later runs on that one, which knows it
    /// (`T-013-11`). `None` where the record was written without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
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

    thread_local! {
        /// The tests' failing directory sync: the next one on this thread fails with the error this makes.
        pub(super) static DIR_SYNC_FAILS: std::cell::Cell<Option<fn() -> io::Error>> =
            const { std::cell::Cell::new(None) };
    }

    /// The lines of the file at `path`, as a record reads them.
    fn lines_at(path: &Path) -> Vec<String> {
        whole_lines(&fs::read_to_string(path).expect("reads"))
    }

    /// A record opened through a directory link holds on to the directory the link named when it was opened: its
    /// replacement, and every append after it, land in that record and not wherever the link points by then, so a
    /// platform's entries never split across two files.
    // verifies: INV-8, NFR-6
    #[test]
    fn a_record_opened_through_a_directory_link_stays_in_the_directory_it_named() {
        let a = scratch("link-a");
        let b = scratch("link-b");
        let (a_dir, b_dir) = (
            a.parent().expect("a directory").to_path_buf(),
            b.parent().expect("a directory").to_path_buf(),
        );
        let link = a_dir.with_file_name(format!(
            "{}-link",
            a_dir.file_name().expect("a name").to_string_lossy()
        ));
        let _ = fs::remove_file(&link);
        drop(FileRecord::create(&a).expect("is created"));
        std::os::unix::fs::symlink(&a_dir, &link).expect("a link to the directory");
        let mut record = FileRecord::open(link.join("record")).expect("opens through the link");
        // The link now names another directory.
        fs::remove_file(&link).expect("unlinked");
        std::os::unix::fs::symlink(&b_dir, &link).expect("a link to another directory");
        record.replace(&["one".to_owned()]).expect("replaces");
        record.append("two").expect("appends");
        assert_eq!(lines_at(&a), ["one", "two"]);
        assert!(!b.exists(), "the record split into {}", b.display());
        let _ = fs::remove_file(&link);
        let _ = fs::remove_dir_all(&a_dir);
        let _ = fs::remove_dir_all(&b_dir);
    }

    /// A directory sync that fails is an error: a replacement is refused, and the record takes nothing more until
    /// it is opened again, and a creation is refused. Only a system that can't sync a directory at all
    /// (`Unsupported`, `EINVAL`) is let through.
    // verifies: INV-8, NFR-6
    #[test]
    fn a_failed_directory_sync_is_an_error_unless_the_system_cannot_sync_one() {
        let path = scratch("dir-sync");
        let mut record = FileRecord::create(&path).expect("is created");
        record.append("one").expect("appends");
        DIR_SYNC_FAILS.set(Some(|| {
            io::Error::other("the directory couldn't be synced")
        }));
        record
            .replace(&["two".to_owned()])
            .expect_err("a failed sync refuses the replacement");
        record.append("three").expect_err("broken until reopened");
        drop(record);
        let other = path.with_file_name("other");
        DIR_SYNC_FAILS.set(Some(|| {
            io::Error::other("the directory couldn't be synced")
        }));
        FileRecord::create(&other).expect_err("a failed sync refuses the creation");
        // A system that can't sync a directory at all isn't failing.
        let mut record = FileRecord::open(&path).expect("reopens");
        let cannot: [fn() -> io::Error; 2] = [
            || io::Error::from(io::ErrorKind::Unsupported),
            || io::Error::from_raw_os_error(22),
        ];
        for (n, fails) in cannot.into_iter().enumerate() {
            DIR_SYNC_FAILS.set(Some(fails));
            record
                .replace(&[format!("let through {n}")])
                .expect("let through");
        }
        record.append("four").expect("appends");
        assert_eq!(record.load().expect("loads"), ["let through 1", "four"]);
        let _ = fs::remove_dir_all(path.parent().expect("a directory"));
    }

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

    /// A `carrying` entry for one of provider-role's operation fixtures, as one line.
    fn carrying(transaction: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/provider-role/fixtures/valid/operation-withdrawal-from-a-bank-reserve.json");
        let text = fs::read_to_string(path).expect("the fixture reads");
        Entry::Carrying {
            transaction: transaction.to_owned(),
            scope: String::new(),
            provider: "reserve".to_owned(),
            operation: Box::new(serde_json::from_str(&text).expect("an operation")),
        }
        .line()
        .expect("a line")
    }

    // verifies: INV-8, NFR-6
    #[test]
    fn a_file_record_reads_back_what_it_appended_and_drops_a_line_cut_off() {
        let path = scratch("cut");
        {
            let mut record = FileRecord::create(&path).expect("is created");
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

    /// An append cut short (a full disk, an I/O error) leaves no fragment for a later entry to be glued to: the
    /// record is cut back to its last whole line and takes nothing more until it is opened again, and an opening
    /// reads every whole entry back, the `carrying` one before the failure included.
    // verifies: INV-8, NFR-6
    #[test]
    fn a_write_cut_short_leaves_every_whole_entry_readable_and_the_record_takes_no_more() {
        let path = scratch("short");
        let mut record = FileRecord::create(&path).expect("is created");
        let first = carrying("tx-1");
        record.append(&first).expect("appends");
        record.cut_next = Some(10);
        record
            .append(&carrying("tx-2"))
            .expect_err("the write was cut short");
        assert_eq!(
            fs::read(&path).expect("reads").len(),
            first.len() + 1,
            "cut back to the last whole line"
        );
        // Nothing more is written to it, glued or not, until it is opened again.
        record.append(&carrying("tx-3")).expect_err("refused");
        record.replace(&[]).expect_err("refused");
        drop(record);
        let mut record = FileRecord::open(&path).expect("reopens");
        let lines = record.load().expect("loads");
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0], first);
        assert!(matches!(
            Entry::read(&lines[0]).expect("reads back"),
            Entry::Carrying { transaction, .. } if transaction == "tx-1"
        ));
        // Reopened, it takes entries again.
        record.append(&carrying("tx-4")).expect("appends");
        assert_eq!(record.load().expect("loads").len(), 2);
        let _ = fs::remove_dir_all(path.parent().expect("a directory"));
    }

    // verifies: INV-8
    #[test]
    fn a_file_record_is_held_by_one_platform_at_a_time() {
        let path = scratch("lock");
        let mut first = FileRecord::create(&path).expect("is created");
        let second = FileRecord::open(&path).expect_err("held");
        assert_eq!(second.kind(), io::ErrorKind::WouldBlock);
        // Still held once replaced, which swaps the file the entries are kept in.
        first.replace(&[]).expect("replaces");
        assert_eq!(
            FileRecord::open(&path).expect_err("held").kind(),
            io::ErrorKind::WouldBlock
        );
        // Through another name of its directory too: the lock is the canonical path's.
        let dir = path.parent().expect("a directory");
        let alias = dir.with_file_name(format!(
            "{}-alias",
            dir.file_name().expect("a name").to_string_lossy()
        ));
        let _ = fs::remove_file(&alias);
        std::os::unix::fs::symlink(dir, &alias).expect("a link to the directory");
        assert_eq!(
            FileRecord::open(alias.join("record"))
                .expect_err("held")
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(first);
        FileRecord::open(&path).expect("free once the first is dropped");
        let _ = fs::remove_file(&alias);
        let _ = fs::remove_dir_all(dir);
    }

    /// A record is opened by its own name only: a symbolic link to it, or a second name of its file, would take a
    /// lock of its own. And a missing record is refused, never made: creating one is a step of its own.
    // verifies: INV-8
    #[test]
    fn a_record_is_opened_by_its_own_name_and_never_made_by_opening() {
        let path = scratch("names");
        assert_eq!(
            FileRecord::open(&path).expect_err("none yet").kind(),
            io::ErrorKind::NotFound
        );
        assert!(!path.exists(), "opening made nothing");
        drop(FileRecord::create(&path).expect("is created"));
        assert_eq!(
            FileRecord::create(&path).expect_err("there already").kind(),
            io::ErrorKind::AlreadyExists
        );
        let link = path.with_file_name("link");
        std::os::unix::fs::symlink(&path, &link).expect("a symbolic link");
        assert_eq!(
            FileRecord::open(&link).expect_err("a link").kind(),
            io::ErrorKind::InvalidInput
        );
        let hard = path.with_file_name("hard");
        fs::hard_link(&path, &hard).expect("a second name");
        for name in [&path, &hard] {
            assert_eq!(
                FileRecord::open(name).expect_err("two names").kind(),
                io::ErrorKind::InvalidInput,
                "{}",
                name.display()
            );
        }
        fs::remove_file(&hard).expect("one name again");
        FileRecord::open(&path).expect("opens by its own name");
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
