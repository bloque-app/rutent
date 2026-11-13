//! Checkpoints and the outside checker (`T-022-04`): fixed-tick, fixed-size, salted commitments to the log's root,
//! issued on a schedule whatever the log did, and a checker holding only the history, the checkpoints and their
//! openings, which sees a rewrite or a removal even when whoever made it had the operator's full access.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ed25519_dalek::{Signer as _, SigningKey};
use rutent_conformance::signing::{
    Ed25519Key, public_key, sign_value, signing_key, statements_key,
};
use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log};
use rutent_contracts::core_internal::{Entry, Violation};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    Deployment, JUDGEMENT_SHAPE, PublicKey, SafeInteger, VERSION, signing_input,
};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_audit::checkpoint::{
    CHECKPOINT_SHAPE, Checker, Checkpoint, Checkpointer, Finding, HISTORY_FAILS_ITS_CHECKPOINTS,
    KEY_OF_THE_OPERATOR, Kept, NOT_ITS_CHECKPOINTS, OPENING_SHAPE, Opening, Schedule, commitment,
    read_lines, split_kept, tick_text,
};
use rutent_core_audit::log::kept::base64url;
use rutent_core_audit::log::merkle::{self, Hash};
use rutent_core_audit::log::{
    AuditLog, Clock, FileStore, KeptInMemory, MemoryStore, Settings, Store, StoreError, record,
};
use rutent_core_audit::receipts::ReceiptSignature;
use serde_json::{Value, json};

// --- The deployment ----------------------------------------------------------------------------------------------

const LOG: &str = "log-main";
const OPERATOR: &str = "operator";
/// Tick 0 of every schedule here, and an hour between ticks.
const FIRST: &str = "2026-11-06T00:00:00Z";
const EVERY: u64 = 3600;
const MADE_AT: &str = "2026-11-06T00:00:00Z";

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn operator_key() -> Ed25519Key {
    signing_key("the operator of rutent-test", 0)
}

fn deployment() -> Deployment {
    Deployment {
        name: "rutent-test".to_owned(),
        operator_key: public_key(&operator_key()),
    }
}

/// The log's own key, which signs checkpoints here as it signs parties' receipts.
fn log_key() -> Ed25519Key {
    signing_key("the log of rutent-test", 0)
}

fn the_logs_key() -> PublicKey {
    public_key(&log_key())
}

fn schedule() -> Schedule {
    Schedule::new(&Timestamp::new(FIRST).unwrap(), EVERY).unwrap()
}

fn first_nanos() -> i128 {
    Timestamp::new(FIRST).unwrap().unix_nanos()
}

/// The log's clock, which a test sets.
#[derive(Debug, Clone)]
struct Dial(Rc<Cell<i128>>);

impl Clock for Dial {
    fn unix_nanos(&self) -> i128 {
        self.0.get()
    }
}

fn settings(dial: &Dial) -> Settings {
    Settings::new(id(LOG), deployment(), id(OPERATOR), id(OPERATOR)).on(Rc::new(dial.clone()))
}

fn checker() -> Checker {
    Checker::new(the_logs_key(), id(LOG), schedule())
}

// --- Entries ---------------------------------------------------------------------------------------------------

fn provider(name: &str) -> Value {
    json!({"party": name, "acts": name, "role": "provider", "kind": "component"})
}

/// `maker`'s judgement with `body`, signed, as received.
fn judgement(maker: &str, body: Value) -> Vec<u8> {
    let mut envelope = json!({
        "deployment": deployment(),
        "protocol_version": VERSION,
        "shape": JUDGEMENT_SHAPE,
        "maker": maker,
        "made_at": MADE_AT,
        "signature": {"scheme": "ed25519", "key": "", "value": ""},
        "statement": body,
    });
    sign_value(&mut envelope, &statements_key(maker));
    serde_json::to_vec(&envelope).unwrap()
}

/// The stated entry for `bytes`, kept under `commitment`.
fn stated(bytes: &[u8], commitment: &Digest) -> GeneralEntry {
    let envelope: Value = serde_json::from_slice(bytes).unwrap();
    serde_json::from_value(json!({
        "kind": "stated",
        "attribution": {"signature": "unverified", "claimed_by": provider(envelope["maker"].as_str().unwrap())},
        "what": {"statement": JUDGEMENT_SHAPE},
        "when": {"at": envelope["made_at"], "source": "party_stated"},
        "received_at": MADE_AT,
        "outcome": {"status": "stated"},
        "signed_with": {"scheme": "ed25519", "key": envelope["signature"]["key"]},
        "commitment": commitment,
    }))
    .unwrap()
}

// --- Where things live -------------------------------------------------------------------------------------------

/// A directory in the platform's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rutent-core-audit-checkpoints-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn history(&self) -> PathBuf {
        self.0.join("history")
    }

    fn checkpoints(&self) -> PathBuf {
        self.0.join("checkpoints")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Shared = Rc<RefCell<AuditLog<FileStore, KeptInMemory>>>;

/// A log in its file, its evidence stand-in beside it, and the checkpointer over both, with its own checkpoint store
/// in its own file: the operator's side, with a copy of every checkpoint it published as an outside checker, which
/// keeps its own, holds them.
struct Rig {
    dir: TempDir,
    dial: Dial,
    log: Shared,
    evidence: KeptInMemory,
    checkpointer: Checkpointer<FileStore, KeptInMemory, FileStore>,
    /// Every checkpoint published, as the checker received it.
    published: Vec<Vec<u8>>,
    entries: u8,
}

impl Rig {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(name);
        let dial = Dial(Rc::new(Cell::new(first_nanos())));
        let evidence = KeptInMemory::new();
        let log = AuditLog::open(
            settings(&dial),
            FileStore::open(dir.history()).unwrap(),
            evidence.clone(),
        )
        .unwrap();
        let log = Rc::new(RefCell::new(log));
        let checkpointer = Checkpointer::new(
            Rc::clone(&log),
            FileStore::open(dir.checkpoints()).unwrap(),
            log_key(),
            schedule(),
        )
        .unwrap();
        Rig {
            dir,
            dial,
            log,
            evidence,
            checkpointer,
            published: Vec::new(),
            entries: 0,
        }
    }

    /// Appends `n` entries, each its own statement, so none is a retry of another.
    fn add(&mut self, n: usize) {
        for _ in 0..n {
            self.entries += 1;
            let bytes = judgement(
                "us-bank",
                json!({"kind": "judgement", "says": format!("case {}", self.entries)}),
            );
            let commitment = self.evidence.keep(&bytes, [self.entries; 32]);
            self.log
                .borrow_mut()
                .append(&stated(&bytes, &commitment))
                .unwrap();
        }
    }

    /// The clock `seconds` after tick `tick`.
    fn set(&self, tick: u64, seconds: i128) {
        self.dial.0.set(
            first_nanos()
                + i128::from(tick) * i128::from(EVERY) * 1_000_000_000
                + seconds * 1_000_000_000,
        );
    }

    /// Runs the checkpointer as of tick `tick`, five seconds in, and keeps what it published.
    fn tick(&mut self, tick: u64) -> Vec<Vec<u8>> {
        self.set(tick, 5);
        let ran = self.checkpointer.run_due();
        assert_eq!(ran.stopped, None);
        self.published.extend(ran.published.clone());
        ran.published
    }

    /// The root of the log now.
    fn root(&self) -> Hash {
        self.log.borrow().root()
    }

    /// The history's records, as a checker holding the log's file reads them.
    fn records(&self) -> Vec<Vec<u8>> {
        self.log
            .borrow()
            .entries()
            .iter()
            .map(|(_, e)| record(e).unwrap())
            .collect()
    }

    /// The openings the operator hands the checker with the history.
    fn openings(&self) -> Vec<Vec<u8>> {
        self.checkpointer
            .kept()
            .iter()
            .map(|k| serde_json::to_vec(&k.opening).unwrap())
            .collect()
    }

    /// Drops the log and the checkpointer, freeing their files, and keeps what the checker held.
    fn close(self) -> (TempDir, Vec<Vec<u8>>) {
        let Rig {
            dir,
            log,
            checkpointer,
            published,
            ..
        } = self;
        drop(checkpointer);
        drop(log);
        (dir, published)
    }
}

/// Rewrites the record at `index` of the log's file with `change` made to its entry, in its canonical form, as
/// someone with the operator's access can.
fn rewrite(file: &Path, index: usize, change: impl FnOnce(&mut Value)) {
    let text = std::fs::read(file).unwrap();
    let mut lines: Vec<Vec<u8>> = text
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    let mut value: Value = serde_json::from_slice(&lines[index]).unwrap();
    change(&mut value);
    let entry: Entry = serde_json::from_value(value).unwrap();
    lines[index] = record(&entry).unwrap();
    let mut out = lines.join(&b'\n');
    out.push(b'\n');
    std::fs::write(file, out).unwrap();
}

/// Removes the line at `index` of the file.
fn remove_line(file: &Path, index: usize) {
    let text = std::fs::read(file).unwrap();
    let mut lines: Vec<&[u8]> = text
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .collect();
    lines.remove(index);
    let mut out = lines.join(&b'\n');
    out.push(b'\n');
    std::fs::write(file, out).unwrap();
}

fn lines(file: &Path) -> Vec<Vec<u8>> {
    read_lines(&std::fs::read(file).unwrap())
}

fn later_time(v: &mut Value) {
    v["when"]["at"] = "2026-11-06T00:00:09Z".into();
}

/// The checkpoint a signer with `key` publishes for `tick`, committing to `size` entries whose root is `root`, under
/// `salt`, and its opening: what an operator holding the log's key can make.
fn forged(
    key: &SigningKey,
    log: &str,
    tick: u64,
    salt: u8,
    size: u64,
    root: &Hash,
) -> (Vec<u8>, Vec<u8>) {
    let public = public_key(key);
    let mut checkpoint = Checkpoint {
        shape: CHECKPOINT_SHAPE.to_owned(),
        log: id(log),
        tick: tick_text(tick),
        at: schedule().at(tick).unwrap(),
        commitment: commitment(&id(log), tick, &[salt; 32], size, root),
        signature: ReceiptSignature {
            scheme: "ed25519".to_owned(),
            key: public,
            value: String::new(),
        },
    };
    let unsigned = serde_json::to_vec(&checkpoint).unwrap();
    checkpoint.signature.value =
        base64url(&key.sign(&signing_input(&unsigned).unwrap()).to_bytes());
    let opening = Opening {
        shape: OPENING_SHAPE.to_owned(),
        log: id(log),
        tick: tick_text(tick),
        salt: base64url(&[salt; 32]),
        size: SafeInteger::try_from(size).unwrap(),
    };
    (
        serde_json::to_vec(&checkpoint).unwrap(),
        serde_json::to_vec(&opening).unwrap(),
    )
}

/// A store that fails on demand: before the write, so nothing reached it, or after, so it did.
#[derive(Clone, Default)]
struct Flaky {
    inner: Rc<RefCell<MemoryStore>>,
    fail_before: Rc<Cell<bool>>,
    fail_after: Rc<Cell<bool>>,
}

impl Store for Flaky {
    fn append(&mut self, record: &[u8]) -> Result<(), StoreError> {
        if self.fail_before.get() {
            return Err(StoreError::Unavailable("before".into()));
        }
        self.inner.borrow_mut().append(record)?;
        if self.fail_after.get() {
            return Err(StoreError::Unavailable("after".into()));
        }
        Ok(())
    }

    fn records(&mut self) -> Result<Vec<Vec<u8>>, StoreError> {
        self.inner.borrow_mut().records()
    }
}

fn kinds(findings: &[Finding]) -> Vec<&'static str> {
    findings
        .iter()
        .map(|f| match f {
            Finding::RecordNotAnEntry { .. } => "record",
            Finding::NotACheckpoint { .. } => "not-a-checkpoint",
            Finding::Equivocation { .. } => "equivocation",
            Finding::Missing { .. } => "missing",
            Finding::NoOpening { .. } => "no-opening",
            Finding::DoesNotHold { .. } => "does-not-hold",
            Finding::HistoryShorter { .. } => "shorter",
            Finding::RolledBack { .. } => "rolled-back",
        })
        .collect()
}

// --- The row's tests ------------------------------------------------------------------------------------------------

/// Two runs on one schedule, one busy and one with nothing at all, the busy one catching up late through four
/// ticks at once: the checkpoints of the two give an observer nothing to tell them by. Every one is the same size, on
/// the schedule's own times, holding the same fields; no two commitments are alike, an unchanged root included;
/// and neither a size, a root nor a salt is in them. The idle run ticks as often as the busy one.
// verifies: AC-37, REQ-AUD-14, GUA-7
#[test]
fn checkpoints_of_busy_and_idle_periods_cant_be_told_apart() {
    let mut busy = Rig::new("busy");
    let mut idle = Rig::new("idle");
    let mut roots = Vec::new();
    for (tick, volume) in [(0, 0), (1, 3), (2, 40), (3, 0), (4, 7)] {
        busy.add(volume);
        roots.push(busy.root());
        busy.tick(tick);
        idle.tick(tick);
    }
    // The busy run stops for four ticks, takes volume, and runs once, late.
    busy.add(11);
    roots.extend([busy.root(); 4]);
    busy.set(8, 5);
    let late = busy.checkpointer.run_due();
    assert_eq!(late.published.len(), 4);
    busy.published.extend(late.published);
    idle.tick(8);

    assert_eq!(busy.published.len(), 9);
    assert_eq!(idle.published.len(), 9);
    let fields = |bytes: &[u8]| -> Value { serde_json::from_slice(bytes).unwrap() };
    for (tick, (b, i)) in busy.published.iter().zip(&idle.published).enumerate() {
        assert_eq!(b.len(), i.len(), "tick {tick}");
        let (b, i) = (fields(b), fields(i));
        let (b, i) = (b.as_object().unwrap(), i.as_object().unwrap());
        assert_eq!(
            b.keys().collect::<Vec<_>>(),
            i.keys().collect::<Vec<_>>(),
            "tick {tick}"
        );
        for key in b.keys() {
            if key != "commitment" && key != "signature" {
                assert_eq!(b[key], i[key], "{key} of tick {tick}");
            }
        }
        assert_eq!(b["tick"], tick_text(tick as u64));
        assert_eq!(
            b["at"].as_str().unwrap(),
            schedule().at(tick as u64).unwrap().as_str()
        );
    }
    // Nothing but a commitment: the fields are these and no others.
    let first = fields(&idle.published[0]);
    let mut names: Vec<&str> = first
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["at", "commitment", "log", "shape", "signature", "tick"]
    );
    // An idle log's root never moves, and its commitments never repeat.
    let commitments = |rig: &Rig| -> Vec<Value> {
        rig.published
            .iter()
            .map(|p| fields(p)["commitment"].clone())
            .collect()
    };
    for rig in [&idle, &busy] {
        let c = commitments(rig);
        for (a, early) in c.iter().enumerate() {
            for late in &c[a + 1..] {
                assert_ne!(early, late);
            }
        }
    }
    // No root, and no salt, is in what is published: a holder of either can't test a checkpoint against it.
    let hex = |h: &Hash| h.iter().map(|b| format!("{b:02x}")).collect::<String>();
    for (p, root) in busy.published.iter().zip(&roots) {
        let text = String::from_utf8_lossy(p);
        assert!(!text.contains(&hex(root)));
    }
    let empty = hex(&merkle::root(&[]));
    for rig in [&idle, &busy] {
        for (p, k) in rig.published.iter().zip(rig.checkpointer.kept()) {
            let text = String::from_utf8_lossy(p);
            assert!(!text.contains(&empty));
            assert!(!text.contains(&k.opening.salt));
        }
    }
    // Each opens, for the one who holds the history and the openings.
    for rig in [&idle, &busy] {
        let report = checker().check(&rig.records(), &rig.published, &rig.openings(), Some(8));
        assert!(report.is_clean(), "{:?}", report.findings);
    }
}

/// A rewritten record fails every checkpoint that committed to a history holding it, and no earlier one; a history
/// cut short fails the checkpoints that reached past its end; a checkpoint changed in a byte, or signed by another
/// key, or of another log, is no checkpoint.
// verifies: AC-37, REQ-AUD-14
#[test]
fn altered_history_fails_against_a_checkpoint() {
    let mut rig = Rig::new("altered");
    rig.add(2);
    rig.tick(0);
    rig.add(2);
    rig.tick(1);
    rig.add(2);
    rig.tick(2);
    let (held, openings) = (rig.published.clone(), rig.openings());
    let records = rig.records();
    assert!(
        checker()
            .check(&records, &held, &openings, Some(2))
            .is_clean()
    );
    let (dir, _) = rig.close();

    // The third record, which ticks 1 and 2 hold and tick 0 doesn't, is altered.
    rewrite(&dir.history(), 2, later_time);
    let altered = lines(&dir.history());
    assert_ne!(altered, records);
    let report = checker().check(&altered, &held, &openings, Some(2));
    assert_eq!(
        report.findings,
        [
            Finding::DoesNotHold { tick: 1 },
            Finding::DoesNotHold { tick: 2 }
        ]
    );
    // A record that is no entry at all.
    let mut garbage = records.clone();
    garbage[0] = b"{}".to_vec();
    let report = checker().check(&garbage, &held, &openings, Some(2));
    assert!(
        report
            .findings
            .contains(&Finding::RecordNotAnEntry { index: 0 })
    );
    assert!(report.findings.contains(&Finding::DoesNotHold { tick: 0 }));

    // The history cut short: only the first checkpoint's two records are left, and the others reach past them.
    let report = checker().check(&records[..3], &held, &openings, Some(2));
    assert_eq!(
        report.findings,
        [
            Finding::HistoryShorter { tick: 1 },
            Finding::HistoryShorter { tick: 2 }
        ]
    );

    // A checkpoint altered in a byte, one signed by another key, one of another log: none is a checkpoint.
    let mut flipped = held.clone();
    let at = flipped[1].len() - 12;
    flipped[1][at] = if flipped[1][at] == b'A' { b'B' } else { b'A' };
    let report = checker().check(&records, &flipped, &openings, Some(2));
    assert!(
        report
            .findings
            .contains(&Finding::NotACheckpoint { position: 1 })
    );
    let root = merkle::root(
        &records
            .iter()
            .map(|r| merkle::leaf_hash(r))
            .collect::<Vec<_>>(),
    );
    let other_key = signing_key("another key", 3);
    let (c, o) = forged(&other_key, LOG, 0, 1, 6, &root);
    let report = checker().check(&records, &[c], &[o], Some(0));
    assert!(
        report
            .findings
            .contains(&Finding::NotACheckpoint { position: 0 })
    );
    let (c, o) = forged(&log_key(), "log-other", 0, 1, 6, &root);
    let report = checker().check(&records, &[c], &[o], Some(0));
    assert!(
        report
            .findings
            .contains(&Finding::NotACheckpoint { position: 0 })
    );
}

/// Someone with every access to the operator rewrites the history, replays the schedule over it with the log's own
/// key, and hands the checker the new checkpoints and the openings of those: shown alone, they check out. The checker
/// that kept the copies it received as each was published sees the rewrite all the same, since the copies commit to
/// a history the rewrite is not, whatever the operator now shows.
// verifies: AC-22, REQ-AUD-9, INV-6
#[test]
fn rewrite_with_full_operator_access_is_detected_by_the_checker() {
    let mut rig = Rig::new("rewrite");
    for tick in 0..4 {
        rig.add(3);
        rig.tick(tick);
    }
    let original = rig.published.clone();
    let records = rig.records();
    let openings = rig.openings();
    assert!(
        checker()
            .check(&records, &original, &openings, Some(3))
            .is_clean()
    );
    let (dir, _) = rig.close();

    // The rewrite: an early record changed, then the whole schedule replayed over it, signed by the log's key.
    rewrite(&dir.history(), 1, later_time);
    let replay = TempDir::new("replay");
    std::fs::copy(dir.history(), replay.history()).unwrap();
    let dial = Dial(Rc::new(Cell::new(first_nanos())));
    let log = AuditLog::open(
        settings(&dial),
        FileStore::open(replay.history()).unwrap(),
        KeptInMemory::new(),
    )
    .unwrap();
    let log = Rc::new(RefCell::new(log));
    let mut replayed = Checkpointer::new(
        Rc::clone(&log),
        FileStore::open(replay.checkpoints()).unwrap(),
        log_key(),
        schedule(),
    )
    .unwrap();
    dial.0
        .set(first_nanos() + 3 * i128::from(EVERY) * 1_000_000_000 + 5_000_000_000);
    let forged_set = replayed.run_due();
    assert_eq!(forged_set.published.len(), 4);
    let forged_openings: Vec<Vec<u8>> = replayed
        .kept()
        .iter()
        .map(|k| serde_json::to_vec(&k.opening).unwrap())
        .collect();
    let rewritten = lines(&dir.history());
    assert_ne!(rewritten, records);

    // A checker with no copy of its own and only what the operator shows now sees nothing wrong: the limit.
    let blind = checker().check(&rewritten, &forged_set.published, &forged_openings, Some(3));
    assert!(blind.is_clean(), "{:?}", blind.findings);

    // The checker's own copies, against the history shown now and the operator's openings, the old and the new.
    let mut shown = openings.clone();
    shown.extend(forged_openings.clone());
    let report = checker().check(&rewritten, &original, &shown, Some(3));
    assert_eq!(
        kinds(&report.findings),
        [
            "does-not-hold",
            "does-not-hold",
            "does-not-hold",
            "does-not-hold"
        ]
    );
    // Holding the copies and the new ones both, it also sees the operator said two things for every tick.
    let mut both = original.clone();
    both.extend(forged_set.published.clone());
    let report = checker().check(&rewritten, &both, &shown, Some(3));
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|f| matches!(f, Finding::Equivocation { .. }))
            .count(),
        4
    );
    assert!(report.findings.contains(&Finding::DoesNotHold { tick: 0 }));
    // The operator withholding the openings of the old ones changes nothing: it is a finding too.
    let report = checker().check(&rewritten, &original, &forged_openings, Some(3));
    assert!(!report.is_clean());
    let report = checker().check(&rewritten, &original, &[], Some(3));
    assert_eq!(
        kinds(&report.findings),
        ["no-opening", "no-opening", "no-opening", "no-opening"]
    );
}

/// With a record removed from the history, but not the checkpoints, the operator's own review notices: it runs the
/// checker over its own stores. It notices a checkpoint removed from its store too, and a checkpointer opened on
/// either refuses: it would vouch for what the review found. A record removed after the last checkpoint is not seen
/// by checkpoints, which a party's receipt shows (`REQ-AUD-5`).
// verifies: REQ-AUD-3
#[test]
fn removed_record_is_noticed_by_the_operators_own_review() {
    let mut rig = Rig::new("review");
    for tick in 0..3 {
        rig.add(2);
        rig.tick(tick);
    }
    rig.add(1);
    let (dir, _) = rig.close();
    let review = |dir: &TempDir, through: u64| {
        let mut history = FileStore::open(dir.history()).unwrap();
        let mut kept = FileStore::open(dir.checkpoints()).unwrap();
        checker()
            .review(&mut history, &mut kept, Some(through))
            .unwrap()
    };
    assert!(review(&dir, 2).is_clean());

    // A record removed after the last checkpoint isn't reached by it: the limit.
    remove_line(&dir.history(), 6);
    assert!(review(&dir, 2).is_clean());

    // One before it is.
    remove_line(&dir.history(), 1);
    let report = review(&dir, 2);
    assert_eq!(
        report.findings,
        [
            Finding::DoesNotHold { tick: 0 },
            Finding::DoesNotHold { tick: 1 },
            Finding::HistoryShorter { tick: 2 }
        ]
    );
    // The checkpointer refuses a log that fails its own checkpoints.
    let log = AuditLog::open(
        settings(&Dial(Rc::new(Cell::new(first_nanos())))),
        FileStore::open(dir.history()).unwrap(),
        KeptInMemory::new(),
    )
    .unwrap();
    let opened = Checkpointer::new(
        Rc::new(RefCell::new(log)),
        FileStore::open(dir.checkpoints()).unwrap(),
        log_key(),
        schedule(),
    );
    assert_eq!(
        opened.err(),
        Some(AppendFailure::Refused(Violation(
            HISTORY_FAILS_ITS_CHECKPOINTS
        )))
    );

    // A checkpoint removed from the operator's own store is a tick missing against the schedule.
    let (dir, _) = {
        let mut rig = Rig::new("review-missing");
        for tick in 0..3 {
            rig.add(1);
            rig.tick(tick);
        }
        rig.close()
    };
    remove_line(&dir.checkpoints(), 1);
    let report = review(&dir, 2);
    assert_eq!(report.findings, [Finding::Missing { from: 1, to: 1 }]);
    let log = AuditLog::open(
        settings(&Dial(Rc::new(Cell::new(first_nanos())))),
        FileStore::open(dir.history()).unwrap(),
        KeptInMemory::new(),
    )
    .unwrap();
    let opened = Checkpointer::new(
        Rc::new(RefCell::new(log)),
        FileStore::open(dir.checkpoints()).unwrap(),
        log_key(),
        schedule(),
    );
    assert_eq!(
        opened.err(),
        Some(AppendFailure::Refused(Violation(NOT_ITS_CHECKPOINTS)))
    );
}

/// The appointed checker's tool, the `audit-check` binary, run with nothing but three files of the history, the
/// checkpoints and their openings, the log's public key and the schedule: it answers clean for the honest history and
/// names what it found for a rewritten one, and it answers neither for what it can't read.
// verifies: REQ-AUD-10
#[test]
fn checkers_tool_runs_from_the_history_and_checkpoints_alone() {
    let mut rig = Rig::new("tool");
    for tick in 0..3 {
        rig.add(2);
        rig.tick(tick);
    }
    let kept_lines: Vec<Vec<u8>> = lines(&rig.dir.checkpoints());
    let (published, openings) = split_kept(&kept_lines);
    assert_eq!(published, rig.published);
    let records = rig.records();
    let (dir, _) = rig.close();

    let outside = TempDir::new("outside");
    let write = |name: &str, lines: &[Vec<u8>]| -> PathBuf {
        let mut out = lines.join(&b'\n');
        out.push(b'\n');
        let path = outside.0.join(name);
        std::fs::write(&path, out).unwrap();
        path
    };
    let history = write("history", &records);
    let checkpoints = write("checkpoints", &published);
    let openings = write("openings", &openings);
    let run = |history: &Path, through: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_audit-check"))
            .env_clear()
            .current_dir(&outside.0)
            .args(["--history"])
            .arg(history)
            .arg("--checkpoints")
            .arg(&checkpoints)
            .arg("--openings")
            .arg(&openings)
            .args(["--key", the_logs_key().as_str(), "--log", LOG])
            .args(["--first", FIRST, "--every", &EVERY.to_string()])
            .args(["--through", through])
            .output()
            .unwrap()
    };
    let honest = run(&history, "2");
    assert_eq!(honest.status.code(), Some(0), "{honest:?}");
    assert!(String::from_utf8_lossy(&honest.stdout).contains("holds"));

    // Rewritten, and a tick expected that the operator never published.
    rewrite(&dir.history(), 3, later_time);
    let rewritten = write("rewritten", &lines(&dir.history()));
    let found = run(&rewritten, "2");
    assert_eq!(found.status.code(), Some(1), "{found:?}");
    let out = String::from_utf8_lossy(&found.stdout);
    assert!(out.contains("DoesNotHold"), "{out}");
    let missing = run(&history, "4");
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stdout).contains("Missing"));
    // What it can't read is neither clean nor a finding.
    let unreadable = run(&outside.0.join("none"), "2");
    assert_eq!(unreadable.status.code(), Some(2));
}

// --- The checkpointer's own rules -------------------------------------------------------------------------------

/// A run issues a checkpoint for every tick due and not yet issued, in order, each at the schedule's time; a run
/// before the first tick issues none, and one within a tick already issued issues none again.
// verifies: REQ-AUD-14
#[test]
fn a_checkpoint_is_issued_for_every_due_tick_in_order_at_the_schedules_time() {
    let mut rig = Rig::new("due");
    rig.dial.0.set(first_nanos() - 1_000_000_000);
    assert!(rig.checkpointer.run_due().published.is_empty());
    assert_eq!(rig.tick(0).len(), 1);
    rig.set(0, 1800);
    assert!(rig.checkpointer.run_due().published.is_empty());
    rig.set(3, 9);
    let ran = rig.checkpointer.run_due();
    assert_eq!(ran.published.len(), 3);
    for (i, bytes) in ran.published.iter().enumerate() {
        let v: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(v["tick"], tick_text(i as u64 + 1));
        assert_eq!(
            v["at"].as_str().unwrap(),
            schedule().at(i as u64 + 1).unwrap().as_str()
        );
    }
    let kept: Vec<&Kept> = rig.checkpointer.kept().iter().collect();
    assert_eq!(kept.len(), 4);
}

/// A write that failed before it reached the store issues nothing and is tried again; one that reached the store
/// and then said it failed is read back, so a tick is never issued twice; the store holds one record for each.
// verifies: REQ-AUD-14
#[test]
fn a_failed_write_issues_nothing_and_never_a_tick_twice() {
    let dir = TempDir::new("flaky");
    let dial = Dial(Rc::new(Cell::new(first_nanos() + 5_000_000_000)));
    let log = AuditLog::open(
        settings(&dial),
        FileStore::open(dir.history()).unwrap(),
        KeptInMemory::new(),
    )
    .unwrap();
    let flaky = Flaky::default();
    let mut cp = Checkpointer::new(
        Rc::new(RefCell::new(log)),
        flaky.clone(),
        log_key(),
        schedule(),
    )
    .unwrap();
    flaky.fail_before.set(true);
    let ran = cp.run_due();
    assert!(ran.published.is_empty());
    assert_eq!(ran.stopped, Some(AppendFailure::Unavailable));
    assert!(flaky.inner.borrow_mut().records().unwrap().is_empty());
    flaky.fail_before.set(false);
    flaky.fail_after.set(true);
    let ran = cp.run_due();
    assert!(ran.published.is_empty());
    assert_eq!(ran.stopped, Some(AppendFailure::Unavailable));
    flaky.fail_after.set(false);
    dial.0
        .set(first_nanos() + EVERY as i128 * 1_000_000_000 + 5_000_000_000);
    let ran = cp.run_due();
    assert_eq!(ran.stopped, None);
    // Tick 0 reached the store and was adopted when read back; only tick 1 is published now.
    assert_eq!(ran.published.len(), 1);
    assert_eq!(flaky.inner.borrow_mut().records().unwrap().len(), 2);
    assert_eq!(cp.kept().len(), 2);
}

/// The checkpointer takes the log's key and never the deployment's `operator_key`.
// verifies: REQ-AUD-14
#[test]
fn the_checkpointers_key_is_never_the_operators() {
    let dir = TempDir::new("key");
    let dial = Dial(Rc::new(Cell::new(first_nanos())));
    let log = Rc::new(RefCell::new(
        AuditLog::open(
            settings(&dial),
            FileStore::open(dir.history()).unwrap(),
            KeptInMemory::new(),
        )
        .unwrap(),
    ));
    let refused = Checkpointer::new(log, MemoryStore::new(), operator_key(), schedule());
    assert_eq!(
        refused.err(),
        Some(AppendFailure::Refused(Violation(KEY_OF_THE_OPERATOR)))
    );
}

// --- The checker's own rules --------------------------------------------------------------------------------------

/// A tick of the schedule with no checkpoint held shows against it, a range at a time; the tick still falling is not
/// yet missing; a second signed checkpoint for one tick is the operator saying two things; a later tick committing to
/// fewer entries than an earlier one is a rollback; a checkpoint dated off its tick is no checkpoint.
// verifies: REQ-AUD-14, REQ-AUD-9
#[test]
fn a_skipped_tick_a_second_word_and_a_rollback_show_against_the_schedule() {
    let mut rig = Rig::new("schedule");
    rig.add(4);
    let records = rig.records();
    let leaves: Vec<Hash> = records.iter().map(|r| merkle::leaf_hash(r)).collect();
    let at = |n: usize| merkle::root(&leaves[..n]);
    let key = log_key();
    let cp = |tick: u64, salt: u8, n: usize| forged(&key, LOG, tick, salt, n as u64, &at(n));
    let (c0, o0) = cp(0, 1, 2);
    let (c1, o1) = cp(1, 2, 3);
    let (c4, o4) = cp(4, 3, 4);

    // Ticks 2 and 3 were never published: a range, and not tick 5, which is still falling.
    let c = checker();
    let report = c.check(
        &records,
        &[c0.clone(), c1.clone(), c4.clone()],
        &[o0.clone(), o1.clone(), o4.clone()],
        Some(4),
    );
    assert_eq!(report.findings, [Finding::Missing { from: 2, to: 3 }]);
    let now = first_nanos() + 5 * i128::from(EVERY) * 1_000_000_000 + 60_000_000_000;
    assert_eq!(schedule().closed_through(now), Some(4));
    assert_eq!(schedule().tick_at(now), Some(5));
    assert_eq!(schedule().closed_through(first_nanos()), None);
    // A checker whose watch began at tick 4 isn't owed the earlier ones.
    let late = checker().from(4);
    assert!(
        late.check(
            &records,
            std::slice::from_ref(&c4),
            std::slice::from_ref(&o4),
            Some(4)
        )
        .is_clean()
    );

    // The same tick, committed twice differently.
    let (c1b, o1b) = cp(1, 9, 2);
    let report = c.check(
        &records,
        &[c0.clone(), c1.clone(), c1b],
        &[o0.clone(), o1.clone(), o1b],
        Some(1),
    );
    assert_eq!(report.findings, [Finding::Equivocation { tick: 1 }]);

    // Tick 1 commits to 3 entries, tick 2 to 2: the log went back.
    let (c2, o2) = cp(2, 5, 2);
    let report = c.check(
        &records,
        &[c0.clone(), c1.clone(), c2],
        &[o0.clone(), o1.clone(), o2],
        Some(2),
    );
    assert_eq!(report.findings, [Finding::RolledBack { tick: 2 }]);

    // A checkpoint whose time isn't its tick's.
    let mut off: Value = serde_json::from_slice(&c0).unwrap();
    off["at"] = "2026-11-06T00:00:01Z".into();
    let report = c.check(
        &records,
        &[serde_json::to_vec(&off).unwrap()],
        &[o0],
        Some(0),
    );
    assert_eq!(kinds(&report.findings), ["not-a-checkpoint", "missing"]);
    // And one whose tick isn't sixteen hexadecimal digits, or a line that is no JSON at all.
    let report = c.check(&records, &[b"not json".to_vec()], &[], None);
    assert_eq!(report.findings, [Finding::NotACheckpoint { position: 0 }]);
}

/// A checkpoint signed by the log's key but dated off its tick, an opening of another log's, and one whose salt isn't
/// 32 bytes, each with a good signature or commitment otherwise: none opens.
// verifies: REQ-AUD-14
#[test]
fn a_checkpoint_off_its_tick_and_an_opening_off_its_log_or_salt_open_nothing() {
    let mut rig = Rig::new("off");
    rig.add(3);
    let records = rig.records();
    let root = merkle::root(
        &records
            .iter()
            .map(|r| merkle::leaf_hash(r))
            .collect::<Vec<_>>(),
    );
    let key = log_key();
    let (c, o) = forged(&key, LOG, 0, 1, 3, &root);
    assert!(
        checker()
            .check(
                &records,
                std::slice::from_ref(&c),
                std::slice::from_ref(&o),
                Some(0)
            )
            .is_clean()
    );

    // Re-signed with the log's key, dated an hour late.
    let mut late: Checkpoint = serde_json::from_slice(&c).unwrap();
    late.at = schedule().at(1).unwrap();
    late.signature.value = String::new();
    let unsigned = serde_json::to_vec(&late).unwrap();
    late.signature.value = base64url(&key.sign(&signing_input(&unsigned).unwrap()).to_bytes());
    let report = checker().check(
        &records,
        &[serde_json::to_vec(&late).unwrap()],
        std::slice::from_ref(&o),
        Some(0),
    );
    assert!(
        report
            .findings
            .contains(&Finding::NotACheckpoint { position: 0 })
    );

    // The opening of another log's.
    let mut elsewhere: Opening = serde_json::from_slice(&o).unwrap();
    elsewhere.log = id("log-other");
    let report = checker().check(
        &records,
        std::slice::from_ref(&c),
        &[serde_json::to_vec(&elsewhere).unwrap()],
        Some(0),
    );
    assert_eq!(report.findings, [Finding::NoOpening { tick: 0 }]);

    // A 16-byte salt, whose commitment is made over it faithfully.
    let salt = [7u8; 16];
    let mut short: Checkpoint = serde_json::from_slice(&c).unwrap();
    short.commitment = commitment(&id(LOG), 0, &salt, 3, &root);
    short.signature.value = String::new();
    let unsigned = serde_json::to_vec(&short).unwrap();
    short.signature.value = base64url(&key.sign(&signing_input(&unsigned).unwrap()).to_bytes());
    let mut opening: Opening = serde_json::from_slice(&o).unwrap();
    opening.salt = base64url(&salt);
    let report = checker().check(
        &records,
        &[serde_json::to_vec(&short).unwrap()],
        &[serde_json::to_vec(&opening).unwrap()],
        Some(0),
    );
    assert_eq!(report.findings, [Finding::DoesNotHold { tick: 0 }]);
}

/// A commitment binds the log, the tick, the salt, the size and the root, each: changing any one gives another. An
/// observer holding an idle log's root, its size and the tick can't reproduce a checkpoint with a salt it guesses
/// (none, or zeros), since the salt is fresh and never published.
// verifies: AC-37, REQ-AUD-14, GUA-7
#[test]
fn a_commitment_binds_every_part_and_the_salt_is_not_guessable() {
    let root = merkle::root(&[]);
    let base = commitment(&id(LOG), 3, &[1; 32], 5, &root);
    assert_ne!(base, commitment(&id("log-other"), 3, &[1; 32], 5, &root));
    assert_ne!(base, commitment(&id(LOG), 4, &[1; 32], 5, &root));
    assert_ne!(base, commitment(&id(LOG), 3, &[2; 32], 5, &root));
    assert_ne!(base, commitment(&id(LOG), 3, &[1; 32], 6, &root));
    assert_ne!(
        base,
        commitment(&id(LOG), 3, &[1; 32], 5, &merkle::sha256(b"x"))
    );

    let mut idle = Rig::new("guess");
    let published = idle.tick(2);
    assert_eq!(published.len(), 3);
    for (tick, bytes) in published.iter().enumerate() {
        let seen: Checkpoint = serde_json::from_slice(bytes).unwrap();
        for guess in [&[][..], &[0u8; 32][..]] {
            assert_ne!(
                seen.commitment,
                commitment(&id(LOG), tick as u64, guess, 0, &root)
            );
        }
    }
}

/// The history is read from nothing but its records: the merkle root of every prefix, computed for the check, is
/// the log's own.
// verifies: DEC-12
#[test]
fn every_prefix_root_the_checker_computes_is_the_logs() {
    let mut rig = Rig::new("prefix");
    let mut roots = vec![rig.root()];
    for _ in 0..19 {
        rig.add(1);
        roots.push(rig.root());
    }
    // A checkpoint at each size opens against the history: the checker's roots are the log's.
    let records = rig.records();
    let key = log_key();
    let leaves: Vec<Hash> = records.iter().map(|r| merkle::leaf_hash(r)).collect();
    for (size, root) in roots.iter().enumerate() {
        assert_eq!(*root, merkle::root(&leaves[..size]));
        let (c, o) = forged(&key, LOG, 0, 4, size as u64, root);
        assert!(
            checker().check(&records, &[c], &[o], Some(0)).is_clean(),
            "{size}"
        );
    }
}
