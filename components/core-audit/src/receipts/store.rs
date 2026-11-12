//! The evidence store (ADR-0005, `EXP-1`'s `G-10`, `T-022-03`): each statement's bytes as received, with the
//! signature inside them, kept apart from the log beside a fresh 32-byte salt, and named from the log by the salted
//! commitment, SHA-256 of the salt followed by the bytes.
//!
//! [`EvidenceStore`] is the store itself, which the assembly holds. It hands out two handles, each implementing one
//! of `core-internal`'s calls and nothing else, so no part reaches the bytes but by its own path:
//!
//! - [`LogsRead`], the log's: [`KeptStatements`], the statement under a commitment, with no read check, since
//!   nothing leaves Core's audit part. The log is opened with it, and reopened with it after a restart.
//! - [`Keeper`], orchestration's and the parties': [`Evidence`], which keeps a statement and answers a party's or
//!   a checker's read only once policy allows it ([`calls::Reads`]).
//!
//! Erasure is the bound store's ([`BoundStore::erase`]): the store is bound once to the one log it serves
//! ([`EvidenceStore::bind`]), and erases only what that log's own `erasure` entry names, made by the operator
//! (`REQ-AUD-13`). The record comes first, so the bytes go only once the log holds the reference that stays. No
//! erasure reaches an advertisement, a provider's public statement whose bytes the log reads back on opening, nor
//! a statement any entry of the log rests on. Nothing is erased but by an erasure: how long anything is kept is the
//! operator's (`REQ-AUD-13`).
//!
//! **Restatements are found here, not in the log.** The store indexes each statement by SHA-256 of its signed
//! content (provider-role's `signing_input`; the bytes themselves where they have none) and answers each
//! [`Evidence::keep`] with the earliest commitment still kept whose content is the same (`REQ-AUD-2`). The index is
//! built from the kept bytes on opening and never written anywhere, so an erasure leaves none of it behind and
//! none of it enters the log (`REQ-AUD-6`).
//!
//! **Where the bytes live.** A [`Shelf`]: [`MemoryShelf`] for a run that keeps nothing, or [`DirectoryShelf`], one
//! file per statement in a directory of the store's own, apart from the log's file, each written whole and synced
//! before [`Evidence::keep`] answers.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{
    self, AppendFailure, Evidence, Fetched, Kept, KeptStatements,
};
use rutent_contracts::core_internal::{
    Action, Encoded, Entry, KeptStatement, Outcome, ReadCheck, ReadOutcome, Reads as Read, Receipt,
    Role, Salt, Violation, What,
};
use rutent_contracts::module_interface::{Digest, DigestValue};
use rutent_contracts::provider_role::{ADVERTISEMENT_SHAPE, signing_input};
use rutent_contracts::router_api::Id;
use serde::{Deserialize, Serialize};

use crate::log::kept::{base64url, from_base64url};
use crate::log::merkle::{Hash, sha256};
use crate::log::{AuditLog, Store, digest};

/// The refusal to keep no bytes: every statement holds at least one.
pub const NOTHING_TO_KEEP: &str = "a statement kept holds at least one byte";

/// The refusal of a fetch that reads anything but a kept statement.
pub const FETCH_READS_EVIDENCE: &str = "the evidence store answers only a read of a kept statement";

/// The refusal of a fetch whose policy answer is of another read.
pub const ANSWER_OF_ANOTHER_READ: &str = "policy's answer to a fetch is the answer to that read";

/// The refusal of a fetch, or an erasure, of a commitment no statement was ever kept under.
pub const NOT_KEPT: &str = "a statement is kept under that commitment, or was and was erased";

/// The refusal of an erasure the log holds no `done` erasure entry for, at the receipt given.
pub const NO_RECORDED_ERASURE: &str =
    "an erasure is carried out only on the log's done erasure entry naming the commitment";

/// The refusal of an erasure by anyone but the operator: an actor in the operator's role, for the operator's
/// party. Until core-internal makes erasure a decided action, the operator's own record is what authorises one.
pub const ERASURE_BY_THE_OPERATOR: &str = "an erasure is the operator's: its entry's actor is in the operator's role, for the operator's party";

/// The refusal to erase a statement an entry of the log rests on: a report, an arrival, graded evidence, a
/// reliance, a stuck order's decision, a measure, a raise, or any other entry citing it.
pub const EVIDENCE_RESTED_ON: &str = "no erasure reaches a statement an entry of the log rests on";

/// The refusal to bind a store holding a statement marked erased that no erasure in the log records.
pub const MARK_WITHOUT_ITS_ERASURE: &str = "every statement the evidence store marks erased has its done erasure, the operator's, in the log";

/// The refusal to erase an advertisement: a provider's public statement, which the log reads back on opening and
/// no erasure reaches (`REQ-AUD-13`, the coordinator's ruling).
pub const ADVERTISEMENT_NEVER_ERASED: &str = "no erasure reaches an advertisement";

/// The refusal to open a store holding a statement that doesn't bear out its commitment, or is filed under another.
pub const NOT_A_KEPT_STATEMENT: &str = "every statement the evidence store holds bears out its commitment: SHA-256 of the salt followed by the bytes";

/// Why a shelf took or gave nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShelfError {
    /// It can't be read or written now; the store may try again.
    Unavailable(String),
    /// What it holds can't be read as kept statements: the store refuses to open on it.
    Corrupt(String),
}

/// A statement as a shelf holds it: its place in the order statements were kept in, and the statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shelved {
    /// Its place: later statements have greater places.
    pub place: u64,
    /// The statement, its salt and its bytes.
    pub kept: KeptStatement,
}

/// What a shelf holds: the statements kept, in no particular order, and the commitments erased.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShelfContents {
    /// The statements still kept.
    pub kept: Vec<Shelved>,
    /// The commitments whose statements were erased, by their digest's value.
    pub erased: Vec<DigestValue>,
}

/// Where the evidence store's bytes live. A write is durable once it answers `Ok`; an `Err` says nothing either way.
pub trait Shelf {
    /// Puts `shelved` on the shelf durably.
    fn put(&mut self, shelved: &Shelved) -> Result<(), ShelfError>;

    /// Erases the statement kept under `commitment`: first marks it erased, durably, then removes its bytes and
    /// salt, so bytes a crash left behind a mark are removed by the next [`Shelf::load`]. Erasing one already
    /// erased is `Ok`.
    fn erase(&mut self, commitment: &Digest) -> Result<(), ShelfError>;

    /// Everything the shelf holds, the bytes of every statement marked erased removed first.
    fn load(&mut self) -> Result<ShelfContents, ShelfError>;
}

/// A shelf in memory: nothing survives the process.
#[derive(Debug, Clone, Default)]
pub struct MemoryShelf {
    contents: ShelfContents,
}

impl MemoryShelf {
    /// An empty shelf.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Shelf for MemoryShelf {
    fn put(&mut self, shelved: &Shelved) -> Result<(), ShelfError> {
        self.contents.kept.push(shelved.clone());
        Ok(())
    }

    fn erase(&mut self, commitment: &Digest) -> Result<(), ShelfError> {
        if !self.contents.erased.contains(&commitment.value) {
            self.contents.erased.push(commitment.value.clone());
        }
        self.contents
            .kept
            .retain(|s| s.kept.commitment != *commitment);
        Ok(())
    }

    fn load(&mut self) -> Result<ShelfContents, ShelfError> {
        Ok(self.contents.clone())
    }
}

/// A shelf in a directory of its own: `kept/<commitment>.json` for each statement kept, written to a temporary
/// file, synced and renamed into place, the directory synced after; `erased/<commitment>` for each erased, a mark
/// with nothing in it. The directory is locked for this shelf alone while it lives, so no second store writes it.
#[derive(Debug)]
pub struct DirectoryShelf {
    kept: PathBuf,
    erased: PathBuf,
    /// Held open, under its exclusive lock, for as long as the shelf lives.
    _lock: File,
}

fn unavailable(e: std::io::Error) -> ShelfError {
    ShelfError::Unavailable(e.to_string())
}

/// Syncs the directory `dir`, so the entries made or removed in it are durable.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> Result<(), ShelfError> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(unavailable)
}

/// A directory can't be opened to sync where the platform isn't Unix; each file's own sync stands.
#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> Result<(), ShelfError> {
    Ok(())
}

/// Removes `path`, where it is there.
fn remove(path: &Path) -> Result<(), ShelfError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(unavailable(e)),
        _ => Ok(()),
    }
}

impl DirectoryShelf {
    /// The shelf in the directory `dir`, made where there is none, and locked for this shelf alone. A directory
    /// another shelf holds is unavailable.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, ShelfError> {
        let dir = dir.as_ref();
        let (kept, erased) = (dir.join("kept"), dir.join("erased"));
        fs::create_dir_all(&kept).map_err(unavailable)?;
        fs::create_dir_all(&erased).map_err(unavailable)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("lock"))
            .map_err(unavailable)?;
        lock.try_lock()
            .map_err(|e| ShelfError::Unavailable(e.to_string()))?;
        sync_dir(dir)?;
        if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            sync_dir(parent)?;
        }
        Ok(DirectoryShelf {
            kept,
            erased,
            _lock: lock,
        })
    }

    fn kept_file(&self, commitment: &DigestValue) -> PathBuf {
        self.kept.join(format!("{}.json", commitment.as_str()))
    }
}

impl Shelf for DirectoryShelf {
    fn put(&mut self, shelved: &Shelved) -> Result<(), ShelfError> {
        let value = &shelved.kept.commitment.value;
        let text =
            serde_json::to_vec(shelved).map_err(|e| ShelfError::Unavailable(e.to_string()))?;
        let part = self.kept.join(format!("{}.part", value.as_str()));
        let mut file = File::create(&part).map_err(unavailable)?;
        file.write_all(&text).map_err(unavailable)?;
        file.sync_all().map_err(unavailable)?;
        fs::rename(&part, self.kept_file(value)).map_err(unavailable)?;
        sync_dir(&self.kept)
    }

    fn erase(&mut self, commitment: &Digest) -> Result<(), ShelfError> {
        let value = &commitment.value;
        File::create(self.erased.join(value.as_str()))
            .and_then(|f| f.sync_all())
            .map_err(unavailable)?;
        sync_dir(&self.erased)?;
        remove(&self.kept_file(value))?;
        sync_dir(&self.kept)
    }

    fn load(&mut self) -> Result<ShelfContents, ShelfError> {
        let names = |dir: &Path| -> Result<Vec<String>, ShelfError> {
            let mut names = Vec::new();
            for item in fs::read_dir(dir).map_err(unavailable)? {
                let name = item.map_err(unavailable)?.file_name();
                names.push(name.into_string().map_err(|n| {
                    ShelfError::Corrupt(format!("{n:?} is no name the shelf writes"))
                })?);
            }
            Ok(names)
        };
        let mut erased = Vec::new();
        for name in names(&self.erased)? {
            let value = DigestValue::new(name.clone())
                .ok_or_else(|| ShelfError::Corrupt(format!("erased/{name} names no commitment")))?;
            // Bytes a crash left behind their mark, between the mark and the removal, are removed here.
            remove(&self.kept_file(&value))?;
            erased.push(value);
        }
        sync_dir(&self.kept)?;
        let mut kept = Vec::new();
        for name in names(&self.kept)? {
            if name.ends_with(".part") {
                // A statement whose write was cut short was never answered as kept.
                remove(&self.kept.join(&name))?;
                continue;
            }
            let corrupt =
                || ShelfError::Corrupt(format!("kept/{name} is no statement the shelf wrote"));
            let text = fs::read(self.kept.join(&name)).map_err(unavailable)?;
            let shelved: Shelved = serde_json::from_slice(&text).map_err(|_| corrupt())?;
            if name != format!("{}.json", shelved.kept.commitment.value.as_str()) {
                return Err(corrupt());
            }
            kept.push(shelved);
        }
        Ok(ShelfContents { kept, erased })
    }
}

/// The bytes `kept` holds, where they bear its commitment out: SHA-256 of the decoded salt followed by them.
pub fn bearing_out(kept: &KeptStatement) -> Option<Vec<u8>> {
    let mut salted = from_base64url(kept.salt.as_str())?;
    let bytes = from_base64url(kept.bytes.as_str())?;
    (salted.len() == 32).then_some(())?;
    salted.extend_from_slice(&bytes);
    (commitment_of(&salted) == kept.commitment).then_some(bytes)
}

/// The commitment of `salted`, the salt followed by the bytes: its SHA-256 digest.
fn commitment_of(salted: &[u8]) -> Digest {
    digest(&sha256(salted))
}

/// What the restatement index files a statement by: SHA-256 of its signed content, provider-role's
/// `signing_input`, or of the bytes themselves where they have none.
fn content_of(bytes: &[u8]) -> Hash {
    sha256(&signing_input(bytes).unwrap_or_else(|_| bytes.to_vec()))
}

/// Whether `bytes` are an advertisement: a statement whose envelope names the advertisement's shape.
fn is_advertisement(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .is_ok_and(|v| v["shape"].as_str() == Some(ADVERTISEMENT_SHAPE))
}

/// 32 fresh bytes from the operating system's random source.
fn fresh_salt() -> Result<[u8; 32], AppendFailure> {
    let mut salt = [0u8; 32];
    getrandom::fill(&mut salt).map_err(|_| AppendFailure::Unavailable)?;
    Ok(salt)
}

/// What the store holds in memory, all read from its shelf.
struct Inner {
    shelf: Box<dyn Shelf>,
    /// Each statement kept, by its commitment's value: its place, the statement and its content's hash.
    kept: HashMap<String, (u64, KeptStatement, Hash)>,
    /// The restatement index: each content's commitments, by place.
    by_content: HashMap<Hash, BTreeMap<u64, Digest>>,
    /// The commitments erased.
    erased: BTreeSet<String>,
    /// The place the next statement kept takes.
    next: u64,
}

impl Inner {
    fn take(&mut self, place: u64, kept: KeptStatement, bytes: &[u8]) {
        let content = content_of(bytes);
        self.by_content
            .entry(content)
            .or_default()
            .insert(place, kept.commitment.clone());
        self.next = self.next.max(place + 1);
        self.kept.insert(
            kept.commitment.value.as_str().to_owned(),
            (place, kept, content),
        );
    }

    fn kept(&self, commitment: &Digest) -> Option<&KeptStatement> {
        self.kept
            .get(commitment.value.as_str())
            .map(|(_, k, _)| k)
            .filter(|k| k.commitment == *commitment)
    }
}

/// The evidence store before it is bound to its log: the statements kept, on their [`Shelf`], with the restatement
/// index read from them. It implements none of `core-internal`'s calls itself: it hands out [`LogsRead`] to the log
/// and [`Keeper`] to orchestration, and is then bound, once, to the log it serves ([`EvidenceStore::bind`]), which
/// alone carries out erasures.
///
/// **Only the assembly holds it** (`rutent-scenarios`, `T-024-02`; `F-030`'s in production). It calls
/// [`for_the_log`](EvidenceStore::for_the_log) once, to open the log with, and [`keeper`](EvidenceStore::keeper)
/// once, for orchestration, and then binds the store, which consumes it, so no other read without a read check can
/// be made, and no other log can ever authorise an erasure.
pub struct EvidenceStore {
    inner: Rc<RefCell<Inner>>,
}

impl EvidenceStore {
    /// An empty store in memory.
    pub fn in_memory() -> Self {
        Self::on(Box::new(MemoryShelf::new())).expect("an empty shelf in memory reads back")
    }

    /// The store kept in the directory `dir`, made empty where there is none, apart from the log's own store.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, AppendFailure> {
        let shelf = DirectoryShelf::open(dir).map_err(|_| AppendFailure::Unavailable)?;
        Self::on(Box::new(shelf))
    }

    /// The store kept on `shelf`, reading back everything it holds. It refuses a shelf holding a statement that
    /// doesn't bear out its commitment: the log would otherwise attribute, or a party check, bytes that are not
    /// the ones committed to.
    pub fn on(mut shelf: Box<dyn Shelf>) -> Result<Self, AppendFailure> {
        let contents = shelf.load().map_err(|e| match e {
            ShelfError::Unavailable(_) => AppendFailure::Unavailable,
            ShelfError::Corrupt(_) => AppendFailure::Refused(Violation(NOT_A_KEPT_STATEMENT)),
        })?;
        let mut inner = Inner {
            shelf,
            kept: HashMap::new(),
            by_content: HashMap::new(),
            erased: contents
                .erased
                .iter()
                .map(|v| v.as_str().to_owned())
                .collect(),
            next: 0,
        };
        let mut shelved = contents.kept;
        shelved.sort_by_key(|s| s.place);
        for s in shelved {
            let bytes = bearing_out(&s.kept)
                .ok_or(AppendFailure::Refused(Violation(NOT_A_KEPT_STATEMENT)))?;
            if inner.erased.contains(s.kept.commitment.value.as_str()) {
                return Err(AppendFailure::Refused(Violation(NOT_A_KEPT_STATEMENT)));
            }
            inner.take(s.place, s.kept, &bytes);
        }
        Ok(EvidenceStore {
            inner: Rc::new(RefCell::new(inner)),
        })
    }

    /// The log's read of this store, which the log is opened, and reopened, with. Only the assembly calls it, once,
    /// before binding: a [`LogsRead`] answers with no read check.
    pub fn for_the_log(&self) -> LogsRead {
        LogsRead(Rc::clone(&self.inner))
    }

    /// Orchestration's and the parties' handle on this store: it keeps statements, and answers a read of one only
    /// once `reads`, policy's read check, allows it. Only the assembly calls it, once, before binding.
    pub fn keeper(&self, reads: Box<dyn calls::Reads>) -> Keeper {
        Keeper {
            inner: Rc::clone(&self.inner),
            reads,
        }
    }

    /// Binds this store to `log`, the one log it serves, opened with this store's [`LogsRead`]: from now on only
    /// that log's own records authorise an erasure. It consumes the store, so it is bound once.
    ///
    /// Binding reconciles the store with the log, so a crash anywhere in an erasure leaves nothing half done:
    ///
    /// - every erasure the log records as done, by the operator, of a statement still kept, is carried out, where
    ///   [`BoundStore::erasable`] allows it (an erasure cut short after its entry and before its mark); one the
    ///   store refuses, by anyone, is reported ([`BoundStore::refused_erasures`]), a finding for the outside checker
    ///   (`T-022-04`);
    /// - a statement marked erased with no such erasure in the log refuses the binding
    ///   ([`MARK_WITHOUT_ITS_ERASURE`]): bytes went with no record of it;
    /// - every stated entry whose bytes the store neither keeps nor marks erased is reported
    ///   ([`BoundStore::missing`]), a finding for the outside checker (`T-022-04`): bytes gone with no erasure.
    pub fn bind<S: Store, K: KeptStatements>(
        self,
        log: Rc<RefCell<AuditLog<S, K>>>,
    ) -> Result<BoundStore<S, K>, AppendFailure> {
        let mut bound = BoundStore {
            inner: self.inner,
            log,
            missing: Vec::new(),
            refused: Vec::new(),
        };
        bound.reconcile()?;
        Ok(bound)
    }
}

/// The evidence store bound to the one log it serves: it carries out that log's erasures (`REQ-AUD-13`).
pub struct BoundStore<S: Store, K: KeptStatements> {
    inner: Rc<RefCell<Inner>>,
    log: Rc<RefCell<AuditLog<S, K>>>,
    missing: Vec<Receipt>,
    refused: Vec<(Receipt, Violation)>,
}

/// The commitment the entry recorded at `at` erases, where it is a verified `erasure` entry, done, by an actor in
/// the operator's role for the operator's party.
fn erasure_at<'a>(
    entries: &'a [(Receipt, Entry)],
    operator_party: &Id,
    at: &Receipt,
) -> Result<&'a Digest, AppendFailure> {
    let recorded = usize::try_from(at.index.get())
        .ok()
        .and_then(|i| entries.get(i))
        .filter(|(r, _)| r == at);
    match recorded {
        Some((_, Entry::Verified(v))) if matches!(v.outcome, Outcome::Done {}) => match &v.what {
            What::Erasure { evidence } => {
                if v.who.role != Role::Operator || v.who.party != *operator_party {
                    return Err(AppendFailure::Refused(Violation(ERASURE_BY_THE_OPERATOR)));
                }
                Ok(evidence)
            }
            _ => Err(AppendFailure::Refused(Violation(NO_RECORDED_ERASURE))),
        },
        _ => Err(AppendFailure::Refused(Violation(NO_RECORDED_ERASURE))),
    }
}

/// Whether `value` holds `needle` anywhere.
fn holds_value(value: &serde_json::Value, needle: &serde_json::Value) -> bool {
    value == needle
        || match value {
            serde_json::Value::Array(items) => items.iter().any(|v| holds_value(v, needle)),
            serde_json::Value::Object(map) => map.values().any(|v| holds_value(v, needle)),
            _ => false,
        }
}

/// Whether an entry of `entries` rests on the statement kept under `commitment`: cites the receipt of a stated
/// entry naming it, or names the commitment itself, as a stuck order's decision names its evidence. Stated entries,
/// erasures, the erasure's decided path, and reads (a read of a statement, and a permission over one) rest on nothing
/// they name.
fn cited(entries: &[(Receipt, Entry)], commitment: &Digest) -> bool {
    let mut needles: Vec<serde_json::Value> = entries
        .iter()
        .filter(|(_, e)| matches!(e, Entry::Stated(s) if s.commitment == *commitment))
        .filter_map(|(r, _)| serde_json::to_value(r).ok())
        .collect();
    needles.extend(serde_json::to_value(commitment).ok());
    entries.iter().any(|(_, e)| match e {
        Entry::Stated(_) => false,
        Entry::Verified(v) => {
            // The erasure's own path (its decision, its effect and their result) rests on nothing it names.
            let erasing = match &v.what {
                What::Decide { request, .. } => {
                    matches!(request.action, Action::EraseStatement { .. })
                }
                What::Effect { action, .. } => matches!(action, Action::EraseStatement { .. }),
                What::Result { .. } => true,
                _ => false,
            };
            !erasing
                && !matches!(
                    v.what,
                    What::Erasure { .. }
                        | What::CheckRead { .. }
                        | What::GrantPermission { .. }
                        | What::RevokePermission { .. }
                )
                && serde_json::to_value(&v.what)
                    .is_ok_and(|what| needles.iter().any(|n| holds_value(&what, n)))
        }
    })
}

impl<S: Store, K: KeptStatements> BoundStore<S, K> {
    fn operator_party(&self) -> Id {
        self.log.borrow().settings().operator_party.clone()
    }

    fn reconcile(&mut self) -> Result<(), AppendFailure> {
        let party = self.operator_party();
        let entries = self.log.borrow().entries().to_vec();
        let recorded: Vec<(Receipt, Digest)> = entries
            .iter()
            .filter_map(|(r, _)| {
                erasure_at(&entries, &party, r)
                    .ok()
                    .map(|c| (r.clone(), c.clone()))
            })
            .collect();
        let marks: Vec<String> = self.inner.borrow().erased.iter().cloned().collect();
        for mark in marks {
            if !recorded.iter().any(|(_, c)| c.value.as_str() == mark) {
                return Err(AppendFailure::Refused(Violation(MARK_WITHOUT_ITS_ERASURE)));
            }
        }
        // Every done erasure the log records is carried out, or, where the store refuses it, reported.
        self.refused = Vec::new();
        for (at, e) in &entries {
            let done_erasure = matches!(e, Entry::Verified(v)
                if matches!(v.what, What::Erasure { .. }) && matches!(v.outcome, Outcome::Done {}));
            if !done_erasure {
                continue;
            }
            match self.erase(at) {
                Ok(()) => {}
                Err(AppendFailure::Refused(why)) => self.refused.push((at.clone(), why)),
                Err(e) => return Err(e),
            }
        }
        let inner = self.inner.borrow();
        self.missing = entries
            .iter()
            .filter_map(|(r, e)| match e {
                Entry::Stated(s)
                    if inner.kept(&s.commitment).is_none()
                        && !inner.erased.contains(s.commitment.value.as_str()) =>
                {
                    Some(r.clone())
                }
                _ => None,
            })
            .collect();
        Ok(())
    }

    /// The stated entries of the log, by receipt, whose bytes the store neither kept nor erased when it was bound:
    /// evidence gone with no erasure, a finding for the outside checker (`T-022-04`).
    pub fn missing(&self) -> &[Receipt] {
        &self.missing
    }

    /// The erasures the log records as done that the store refused when it was bound, by receipt, with why: an
    /// erasure recorded but not carried out, by anyone but the operator, of an advertisement, of a statement an
    /// entry rests on. A finding for the outside checker (`T-022-04`), beside [`BoundStore::missing`].
    pub fn refused_erasures(&self) -> &[(Receipt, Violation)] {
        &self.refused
    }

    /// Whether the statement kept under `commitment` may be erased, over the log's history: it is kept, or was and
    /// is erased already; it is no advertisement, by its bytes or by any stated entry naming it; and no entry of
    /// the log rests on it. Run it before appending the erasure's entry, so the log records no erasure the store
    /// would refuse.
    pub fn erasable(&self, commitment: &Digest) -> Result<(), AppendFailure> {
        let inner = self.inner.borrow();
        if inner.erased.contains(commitment.value.as_str()) {
            return Ok(());
        }
        let kept = inner
            .kept(commitment)
            .ok_or(AppendFailure::Refused(Violation(NOT_KEPT)))?;
        let log = self.log.borrow();
        let labelled = log.entries().iter().any(|(_, e)| {
            matches!(e, Entry::Stated(s) if s.commitment == *commitment
                && s.what.statement.as_str() == ADVERTISEMENT_SHAPE)
        });
        if labelled || bearing_out(kept).is_some_and(|b| is_advertisement(&b)) {
            return Err(AppendFailure::Refused(Violation(
                ADVERTISEMENT_NEVER_ERASED,
            )));
        }
        if cited(log.entries(), commitment) {
            return Err(AppendFailure::Refused(Violation(EVIDENCE_RESTED_ON)));
        }
        Ok(())
    }

    /// Erases, outside the log, the statement the `erasure` entry at `at` names (`REQ-AUD-13`): its bytes and its
    /// salt go, and its place in the restatement index; its commitment stays, in the log's stated entry and in the
    /// store's mark, so a read of it answers [`Fetched::Erased`] and history still checks.
    ///
    /// It is carried out only on the bound log's own record: the log must hold, at `at`, a verified `erasure` entry
    /// whose outcome is `done`, by an actor in the operator's role for the operator's party
    /// ([`ERASURE_BY_THE_OPERATOR`]). So the reference is recorded before anything is erased. Should the erasure be
    /// cut short after its entry, by a failure answered `Unavailable`, calling this again carries it out, and so
    /// does binding the store again after a crash. It refuses what [`BoundStore::erasable`] refuses. A statement
    /// erased already answers `Ok`.
    pub fn erase(&self, at: &Receipt) -> Result<(), AppendFailure> {
        let commitment = {
            let log = self.log.borrow();
            erasure_at(log.entries(), &log.settings().operator_party, at)?.clone()
        };
        self.erasable(&commitment)?;
        let mut inner = self.inner.borrow_mut();
        let key = commitment.value.as_str().to_owned();
        if inner.erased.contains(&key) {
            return Ok(());
        }
        inner
            .shelf
            .erase(&commitment)
            .map_err(|_| AppendFailure::Unavailable)?;
        if let Some((place, _, content)) = inner.kept.remove(&key)
            && let Some(places) = inner.by_content.get_mut(&content)
        {
            places.remove(&place);
            if places.is_empty() {
                inner.by_content.remove(&content);
            }
        }
        inner.erased.insert(key);
        Ok(())
    }

    /// Whether the statement once kept under `commitment` was erased.
    pub fn is_erased(&self, commitment: &Digest) -> bool {
        self.inner
            .borrow()
            .erased
            .contains(commitment.value.as_str())
    }
}

/// The log's read of the evidence store ([`KeptStatements`]): the statement kept under a commitment, with no read
/// check, since nothing it answers leaves Core's audit part. It implements nothing else.
pub struct LogsRead(Rc<RefCell<Inner>>);

impl KeptStatements for LogsRead {
    fn kept(&mut self, commitment: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        Ok(self.0.borrow().kept(commitment).cloned())
    }
}

/// Orchestration's and the parties' handle on the evidence store ([`Evidence`]): it keeps statements, and hands one
/// out only on a read policy allows. It implements nothing else, so whoever holds it reads no statement but through
/// policy.
pub struct Keeper {
    inner: Rc<RefCell<Inner>>,
    reads: Box<dyn calls::Reads>,
}

impl Evidence for Keeper {
    fn keep(&mut self, received: &[u8]) -> Result<Kept, AppendFailure> {
        if received.is_empty() {
            return Err(AppendFailure::Refused(Violation(NOTHING_TO_KEEP)));
        }
        let salt = fresh_salt()?;
        let mut salted = salt.to_vec();
        salted.extend_from_slice(received);
        let commitment = commitment_of(&salted);
        let kept = KeptStatement {
            commitment: commitment.clone(),
            salt: Salt::new(base64url(&salt)).expect("32 bytes of base64url are a salt"),
            bytes: Encoded::new(base64url(received)).expect("base64url text is encoded bytes"),
        };
        let mut inner = self.inner.borrow_mut();
        let key = commitment.value.as_str();
        if inner.kept.contains_key(key) || inner.erased.contains(key) {
            // Two salts drawn alike: draw again by trying again.
            return Err(AppendFailure::Unavailable);
        }
        let restates = inner
            .by_content
            .get(&content_of(received))
            .and_then(|places| places.values().next())
            .cloned();
        // The place is taken whatever the write answers: a write that reached the shelf before failing must not
        // leave its place to the next statement.
        let place = inner.next;
        inner.next += 1;
        inner
            .shelf
            .put(&Shelved {
                place,
                kept: kept.clone(),
            })
            .map_err(|_| AppendFailure::Unavailable)?;
        inner.take(place, kept, received);
        Ok(Kept {
            commitment,
            restates,
        })
    }

    fn fetch(&mut self, check: &ReadCheck) -> Result<Fetched, AppendFailure> {
        let Read::Evidence { commitment } = &check.reads else {
            return Err(AppendFailure::Refused(Violation(FETCH_READS_EVIDENCE)));
        };
        // Policy is asked before anything is looked up, so a refused reader learns nothing of what is kept. No
        // borrow of the store is held while it answers: it records the read in the log, which may read the store.
        let answer = self.reads.check_read(check)?;
        if answer.request != check.request {
            return Err(AppendFailure::Refused(Violation(ANSWER_OF_ANOTHER_READ)));
        }
        if let ReadOutcome::Refused { .. } = answer.outcome {
            return Ok(Fetched::Refused(answer));
        }
        let inner = self.inner.borrow();
        if inner.erased.contains(commitment.value.as_str()) {
            return Ok(Fetched::Erased);
        }
        inner
            .kept(commitment)
            .cloned()
            .map(Fetched::Kept)
            .ok_or(AppendFailure::Refused(Violation(NOT_KEPT)))
    }
}
