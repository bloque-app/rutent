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
    Deployment, JUDGEMENT_SHAPE, PublicKey, SafeInteger, SignatureScheme, VERSION, signing_input,
};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_audit::checkpoint::{
    Checker, Checkpoint, CheckpointOpeningShape, CheckpointShape, CheckpointSignature,
    Checkpointer, Finding, HISTORY_FAILS_ITS_CHECKPOINTS, KEY_OF_THE_OPERATOR, Kept, MOST_PER_RUN,
    NOT_ITS_CHECKPOINTS, Opening, Ran, Salt, Schedule, SignatureValue, Tick, commitment,
    read_lines, split_kept, tick_text,
};
use rutent_core_audit::log::kept::base64url;
use rutent_core_audit::log::merkle::{self, Hash};
use rutent_core_audit::log::{
    AuditLog, Clock, FileStore, KeptInMemory, MemoryStore, Settings, Store, StoreError, record,
};
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
        self.checkpointer.acknowledge(&ran).unwrap();
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

/// A signature value of the contract's shape, which the signing input leaves out.
fn placeholder() -> SignatureValue {
    SignatureValue::new("A".repeat(86)).unwrap()
}

/// `checkpoint` signed by `key`, over the signing input of its bytes.
fn sign(checkpoint: &mut Checkpoint, key: &SigningKey) {
    let unsigned = serde_json::to_vec(&*checkpoint).unwrap();
    checkpoint.signature.value = SignatureValue::new(base64url(
        &key.sign(&signing_input(&unsigned).unwrap()).to_bytes(),
    ))
    .unwrap();
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
        shape: CheckpointShape::Checkpoint,
        log: id(log),
        tick: Tick::new(tick_text(tick)).unwrap(),
        at: schedule().at(tick).unwrap(),
        commitment: commitment(&id(log), tick, &[salt; 32], size, root),
        signature: CheckpointSignature {
            scheme: SignatureScheme::Ed25519,
            key: public,
            value: placeholder(),
        },
    };
    sign(&mut checkpoint, key);
    let opening = Opening {
        shape: CheckpointOpeningShape::CheckpointOpening,
        log: id(log),
        tick: Tick::new(tick_text(tick)).unwrap(),
        salt: Salt::new(base64url(&[salt; 32])).unwrap(),
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
    busy.checkpointer.acknowledge(&late).unwrap();
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
            assert!(!text.contains(k.opening.salt.as_str()));
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
    replayed.acknowledge(&forged_set).unwrap();
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
    // The store's lines are each checkpoint followed by the acknowledgement of it: line 2 is tick 1's.
    remove_line(&dir.checkpoints(), 2);
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
    rig.checkpointer.acknowledge(&ran).unwrap();
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
    // Tick 0 reached the store and was adopted when read back, but no caller was ever handed it: it is published
    // now, with tick 1, so a checker is never left holding a tick the store has and nobody published.
    assert_eq!(ran.published.len(), 2);
    for (tick, bytes) in ran.published.iter().enumerate() {
        let v: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(v["tick"], tick_text(tick as u64));
    }
    cp.acknowledge(&ran).unwrap();
    assert!(cp.run_due().published.is_empty());
    // Two checkpoints and the acknowledgement of them.
    assert_eq!(flaky.inner.borrow_mut().records().unwrap().len(), 3);
    assert_eq!(cp.kept().len(), 2);
}

/// A checkpoint stored and not yet acknowledged as published is handed out again by the next run, and by a
/// checkpointer opened afresh on the same store, until the caller acknowledges it; an acknowledgement is durable.
/// So a process that dies between the store and the answer loses no tick.
// verifies: REQ-AUD-14
#[test]
fn a_stored_checkpoint_is_published_at_least_once_across_a_restart() {
    let dir = TempDir::new("restart");
    let dial = Dial(Rc::new(Cell::new(
        first_nanos() + EVERY as i128 * 1_000_000_000 + 5_000_000_000,
    )));
    let log = Rc::new(RefCell::new(
        AuditLog::open(
            settings(&dial),
            FileStore::open(dir.history()).unwrap(),
            KeptInMemory::new(),
        )
        .unwrap(),
    ));
    let shared = Flaky::default();
    let open =
        || Checkpointer::new(Rc::clone(&log), shared.clone(), log_key(), schedule()).unwrap();
    let mut cp = open();
    let ran = cp.run_due();
    assert_eq!(ran.published.len(), 2);
    // The process dies before the caller published them: nothing was acknowledged.
    drop(cp);
    let mut cp = open();
    let again = cp.run_due();
    assert_eq!(again.published, ran.published);
    assert_eq!(shared.inner.borrow_mut().records().unwrap().len(), 2);
    // The first is acknowledged and the process dies: only the second is handed out again.
    let first = Ran {
        published: vec![again.published[0].clone()],
        stopped: None,
        through: Some(0),
    };
    cp.acknowledge(&first).unwrap();
    drop(cp);
    let mut cp = open();
    let rest = cp.run_due();
    assert_eq!(rest.published, again.published[1..]);
    cp.acknowledge(&rest).unwrap();
    drop(cp);
    assert!(open().run_due().published.is_empty());
    // An acknowledgement of a tick the store doesn't hold is refused.
    let mut cp = open();
    let bogus = Ran {
        published: Vec::new(),
        stopped: None,
        through: Some(9),
    };
    assert!(cp.acknowledge(&bogus).is_err());
}

/// A long outage is issued a batch at a time: no run issues more than the most per run, the next continues in
/// order, and nothing is skipped.
// verifies: REQ-AUD-14
#[test]
fn a_long_outage_is_caught_up_a_batch_at_a_time() {
    let mut rig = Rig::new("outage");
    let ticks = 2 * MOST_PER_RUN + 7;
    rig.set(ticks, 5);
    let mut seen = Vec::new();
    for round in 0..4 {
        let ran = rig.checkpointer.run_due();
        assert!(ran.published.len() as u64 <= MOST_PER_RUN, "round {round}");
        assert_eq!(ran.stopped, None);
        // Nor does a run store more than the most.
        assert!(rig.checkpointer.kept().len() as u64 <= (round + 1) * MOST_PER_RUN);
        rig.checkpointer.acknowledge(&ran).unwrap();
        seen.extend(ran.published);
    }
    assert_eq!(seen.len() as u64, ticks + 1);
    for (tick, bytes) in seen.iter().enumerate() {
        let v: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(v["tick"], tick_text(tick as u64));
    }
    assert!(rig.checkpointer.run_due().published.is_empty());

    // A caller that never acknowledges is handed the same first batch again, never a backlog of all.
    let mut rig = Rig::new("outage-unacknowledged");
    rig.set(ticks, 5);
    let first = rig.checkpointer.run_due();
    let second = rig.checkpointer.run_due();
    assert_eq!(first.published.len() as u64, MOST_PER_RUN);
    assert_eq!(second.published, first.published);
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
    sign(&mut late, &key);
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

    // A 16-byte salt, whose commitment is made over it faithfully. The contract's opening holds a salt of 32 bytes,
    // so what names a short one is no opening: nothing opens the checkpoint (`T-022-07`, which read it as one that
    // opens nothing).
    let salt = [7u8; 16];
    let mut short: Checkpoint = serde_json::from_slice(&c).unwrap();
    short.commitment = commitment(&id(LOG), 0, &salt, 3, &root);
    sign(&mut short, &key);
    let mut opening: Value = serde_json::from_slice(&o).unwrap();
    opening["salt"] = base64url(&salt).into();
    assert!(serde_json::from_value::<Opening>(opening.clone()).is_err());
    let report = checker().check(
        &records,
        &[serde_json::to_vec(&short).unwrap()],
        &[serde_json::to_vec(&opening).unwrap()],
        Some(0),
    );
    assert_eq!(report.findings, [Finding::NoOpening { tick: 0 }]);
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

// --- The contract's shapes (`T-022-07`) --------------------------------------------------------------------------

/// The core-internal contract's directory.
fn contract_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/core-internal")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .unwrap()
}

/// A validator for the contract's shape `shape`, with the schemas it refers to registered by `$id`, so every `$ref`
/// resolves offline.
fn validator(shape: &str) -> jsonschema::Validator {
    let contracts = contract_dir().join("..");
    let mut schemas: Vec<(String, Value)> = Vec::new();
    for contract in [
        "core-internal",
        "provider-role",
        "module-interface",
        "router-api",
        "state-machine",
        // core-internal's listing of a public network names the reader contract's shapes (`T-020-59`).
        "reader",
    ] {
        let dir = contracts.join(contract).join("schemas");
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.to_string_lossy().ends_with(".schema.json") {
                let schema = read_json(&path);
                schemas.push((schema["$id"].as_str().unwrap().to_owned(), schema));
            }
        }
    }
    let mut registry = jsonschema::Registry::new();
    for (id, schema) in schemas {
        registry = registry.add(id, schema).unwrap();
    }
    let registry = registry.prepare().unwrap();
    let own = read_json(
        &contract_dir()
            .join("schemas")
            .join(format!("{shape}.schema.json")),
    );
    jsonschema::options()
        .offline()
        .with_registry(&registry)
        .build(&own)
        .unwrap()
}

/// What `validator` finds wrong with `bytes`.
fn schema_errors(shape: &str, bytes: &[u8]) -> Vec<String> {
    let value: Value = serde_json::from_slice(bytes).unwrap();
    validator(shape)
        .iter_errors(&value)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect()
}

/// Every checkpoint a checkpointer publishes, a busy period's and an idle one's, is the contract's checkpoint: valid
/// against its schema, and bytes of the contract's own type, which writes them as they are.
// verifies: REQ-AUD-14
#[test]
fn checkpoint_issued_is_valid_against_the_contracts_schema() {
    let mut rig = Rig::new("shape-checkpoint");
    for (tick, volume) in [(0, 0), (1, 4), (2, 0)] {
        rig.add(volume);
        rig.tick(tick);
    }
    assert_eq!(rig.published.len(), 3);
    for (tick, bytes) in rig.published.iter().enumerate() {
        assert_eq!(schema_errors("checkpoint", bytes), Vec::<String>::new());
        let value: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(value["shape"], "urn:rutent:core-internal:checkpoint");
        let typed: rutent_contracts::core_internal::Checkpoint =
            serde_json::from_slice(bytes).unwrap();
        assert_eq!(&serde_json::to_vec(&typed).unwrap(), bytes, "tick {tick}");
    }
    // What the checkpoint store keeps for the checker's side is the same bytes.
    let (published, _) = split_kept(&lines(&rig.dir.checkpoints()));
    assert_eq!(published, rig.published);
}

/// Every opening a checkpointer keeps, and hands the checker, is the contract's opening.
// verifies: REQ-AUD-14
#[test]
fn opening_issued_is_valid_against_the_contracts_schema() {
    let mut rig = Rig::new("shape-opening");
    for (tick, volume) in [(0, 2), (1, 0)] {
        rig.add(volume);
        rig.tick(tick);
    }
    let handed = rig.openings();
    let (_, from_the_store) = split_kept(&lines(&rig.dir.checkpoints()));
    assert_eq!(handed.len(), 2);
    assert_eq!(handed, from_the_store);
    for bytes in &handed {
        assert_eq!(
            schema_errors("checkpoint-opening", bytes),
            Vec::<String>::new()
        );
        let value: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(
            value["shape"],
            "urn:rutent:core-internal:checkpoint-opening"
        );
        let typed: rutent_contracts::core_internal::CheckpointOpening =
            serde_json::from_slice(bytes).unwrap();
        assert_eq!(&serde_json::to_vec(&typed).unwrap(), bytes);
    }
}

fn fixture(kind: &str, name: &str) -> Value {
    read_json(
        &contract_dir()
            .join("fixtures")
            .join(kind)
            .join(format!("{name}.json")),
    )
}

/// The contract's checkpoint fixture with the three members no fixture can hold right: its commitment to `root`'s
/// history, and the log's key and signature. Every other member, and the whole opening, are the fixture's.
fn fixture_checkpoint(root: &Hash) -> Vec<u8> {
    let opening = fixture("valid", "checkpoint-opening-of-a-tick");
    let salt =
        rutent_core_audit::log::kept::from_base64url(opening["salt"].as_str().unwrap()).unwrap();
    let mut value = fixture("valid", "checkpoint-of-a-tick");
    let size = opening["size"].as_u64().unwrap();
    let tick = u64::from_str_radix(value["tick"].as_str().unwrap(), 16).unwrap();
    value["commitment"] =
        serde_json::to_value(commitment(&id(LOG), tick, &salt, size, root)).unwrap();
    value["signature"]["key"] = the_logs_key().as_str().into();
    let placeholder = serde_json::to_vec(&value).unwrap();
    value["signature"]["value"] = base64url(
        &log_key()
            .sign(&signing_input(&placeholder).unwrap())
            .to_bytes(),
    )
    .into();
    serde_json::to_vec(&value).unwrap()
}

/// The checker reads the contract's own fixtures: the opening as it is, and the checkpoint with only the commitment
/// and signature a static file can't hold; a checkpoint of the contract's invalid fixtures, one carrying a size, one
/// a salt, one whose tick tells its size, is none, and an opening with no salt opens nothing. `audit-check` accepts
/// the same bytes.
// verifies: REQ-AUD-10
#[test]
fn checker_reads_the_contracts_checkpoint_and_opening_fixtures() {
    let opening = fixture("valid", "checkpoint-opening-of-a-tick");
    let at = fixture("valid", "checkpoint-of-a-tick")["at"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(opening["size"], 110);
    // The schedule the fixture is on: tick 42 falls at its time, an hour to the tick.
    let first = Timestamp::new("2026-11-08T15:00:00Z").unwrap();
    let schedule = Schedule::new(&first, EVERY).unwrap();
    assert_eq!(schedule.at(42).unwrap().as_str(), at);
    let checker = Checker::new(the_logs_key(), id(LOG), schedule).from(42);

    let mut rig = Rig::new("shape-fixtures");
    rig.add(110);
    let records = rig.records();
    let root = rig.root();
    let checkpoint = fixture_checkpoint(&root);
    let opening_bytes = serde_json::to_vec(&opening).unwrap();
    let report = checker.check(
        &records,
        std::slice::from_ref(&checkpoint),
        std::slice::from_ref(&opening_bytes),
        Some(42),
    );
    assert!(report.is_clean(), "{:?}", report.findings);

    // The contract's invalid fixtures are no checkpoint, and the one without its salt is no opening.
    for name in [
        "checkpoint-carrying-its-size",
        "checkpoint-carrying-its-salt",
        "checkpoint-whose-tick-tells-its-size",
    ] {
        // Signed by the log's key all the same, so only its shape refuses it.
        let mut value = fixture("invalid", name);
        value["signature"]["key"] = the_logs_key().as_str().into();
        let unsigned = serde_json::to_vec(&value).unwrap();
        value["signature"]["value"] = base64url(
            &log_key()
                .sign(&signing_input(&unsigned).unwrap())
                .to_bytes(),
        )
        .into();
        let bytes = serde_json::to_vec(&value).unwrap();
        let report = checker.check(
            &records,
            &[bytes],
            std::slice::from_ref(&opening_bytes),
            None,
        );
        assert_eq!(
            report.findings,
            [Finding::NotACheckpoint { position: 0 }],
            "{name}"
        );
    }
    let saltless =
        serde_json::to_vec(&fixture("invalid", "checkpoint-opening-without-its-salt")).unwrap();
    let report = checker.check(
        &records,
        std::slice::from_ref(&checkpoint),
        &[saltless],
        Some(42),
    );
    assert_eq!(report.findings, [Finding::NoOpening { tick: 42 }]);

    // The tool reads the same bytes, from files.
    let outside = TempDir::new("shape-fixtures-outside");
    let write = |name: &str, lines: &[Vec<u8>]| -> PathBuf {
        let mut out = lines.join(&b'\n');
        out.push(b'\n');
        let path = outside.0.join(name);
        std::fs::write(&path, out).unwrap();
        path
    };
    let (history, checkpoints, openings) = (
        write("history", &records),
        write("checkpoints", &[checkpoint]),
        write("openings", &[opening_bytes]),
    );
    let run = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_audit-check"))
            .env_clear()
            .current_dir(&outside.0)
            .args(["--history"])
            .arg(&history)
            .arg("--checkpoints")
            .arg(&checkpoints)
            .arg("--openings")
            .arg(&openings)
            .args(["--key", the_logs_key().as_str(), "--log", LOG])
            .args(["--first", first.as_str(), "--every", &EVERY.to_string()])
            .args(["--from", "42", "--through", "42"])
            .args(extra)
            .output()
            .unwrap()
    };
    let honest = run(&[]);
    assert_eq!(honest.status.code(), Some(0), "{honest:?}");
    // Asked about a history that isn't the one committed to, it finds the rewrite.
    rewrite(&rig.dir.history(), 3, later_time);
    let rewritten = write("rewritten", &lines(&rig.dir.history()));
    let found = std::process::Command::new(env!("CARGO_BIN_EXE_audit-check"))
        .env_clear()
        .current_dir(&outside.0)
        .arg("--history")
        .arg(&rewritten)
        .arg("--checkpoints")
        .arg(&checkpoints)
        .arg("--openings")
        .arg(&openings)
        .args(["--key", the_logs_key().as_str(), "--log", LOG])
        .args(["--first", first.as_str(), "--every", &EVERY.to_string()])
        .args(["--from", "42", "--through", "42"])
        .output()
        .unwrap();
    assert_eq!(found.status.code(), Some(1), "{found:?}");
    assert!(String::from_utf8_lossy(&found.stdout).contains("DoesNotHold"));
}

/// On the contract's shape the checkpoints of a busy period and an idle one are still alike to an observer: each is
/// valid against the one schema, the same size and the same six members, and holds neither a size, a root, a salt
/// nor a count.
// verifies: AC-37, REQ-AUD-14
#[test]
fn checkpoints_of_busy_and_idle_periods_still_cant_be_told_apart() {
    let mut busy = Rig::new("shape-busy");
    let mut idle = Rig::new("shape-idle");
    for (tick, volume) in [(0, 0), (1, 30), (2, 1), (3, 0)] {
        busy.add(volume);
        busy.tick(tick);
        idle.tick(tick);
    }
    assert_eq!(busy.published.len(), 4);
    for (tick, (b, i)) in busy.published.iter().zip(&idle.published).enumerate() {
        assert_eq!(b.len(), i.len(), "tick {tick}");
        for bytes in [b, i] {
            assert_eq!(schema_errors("checkpoint", bytes), Vec::<String>::new());
            let value: Value = serde_json::from_slice(bytes).unwrap();
            let mut names: Vec<&str> = value
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
        }
    }
}

/// A checkpoint store written in core-audit's own earlier shapes, which no deployment holds yet, is not read: the
/// checkpointer refuses to open on it, and the checker takes an old-shaped checkpoint for none.
// verifies: REQ-AUD-14
#[test]
fn a_checkpoint_in_the_old_shape_is_no_checkpoint_and_its_store_is_refused() {
    let mut rig = Rig::new("shape-old");
    rig.add(2);
    rig.tick(0);
    let current: Value = serde_json::from_slice(&rig.published[0]).unwrap();
    let mut old = current.clone();
    old["shape"] = "urn:rutent:core-audit:checkpoint".into();
    let unsigned = serde_json::to_vec(&old).unwrap();
    old["signature"]["value"] = base64url(
        &log_key()
            .sign(&signing_input(&unsigned).unwrap())
            .to_bytes(),
    )
    .into();
    let old = serde_json::to_vec(&old).unwrap();
    let report = checker().check(
        &rig.records(),
        std::slice::from_ref(&old),
        &rig.openings(),
        Some(0),
    );
    assert!(
        report
            .findings
            .contains(&Finding::NotACheckpoint { position: 0 })
    );

    let mut opening: Value = serde_json::from_slice(&rig.openings()[0]).unwrap();
    opening["shape"] = "urn:rutent:core-audit:checkpoint-opening".into();
    let record = serde_json::to_vec(&json!({
        "checkpoint": serde_json::from_slice::<Value>(&old).unwrap(),
        "opening": opening,
    }))
    .unwrap();
    let mut store = MemoryStore::default();
    store.append(&record).unwrap();
    let refused = Checkpointer::new(Rc::clone(&rig.log), store, log_key(), schedule());
    assert_eq!(
        refused.err(),
        Some(AppendFailure::Refused(Violation(NOT_ITS_CHECKPOINTS)))
    );
}
