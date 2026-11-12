//! Receipts, and evidence apart from the log (`T-022-03`): the evidence store that keeps each statement's bytes,
//! durably and apart from the log, named from it by a salted commitment; the log reopened with it; a party's
//! signed receipt, which shows a changed record; and erasure, which reaches only what a reference names outside the
//! log, leaving the reference and a history that still checks.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::{
    Ed25519Key, advertisement, another_key, public_key, sign_value, signing_key, statements_key,
};
use rutent_contracts::core_internal::calls::{
    self, AppendFailure, DecisionEntry, Evidence, Fetched, GeneralEntry, KeptStatements, Log,
    PolicyLog,
};
use rutent_contracts::core_internal::{
    Decision, Entry, PermissionChange, ReadAnswer, ReadCheck, ReadOutcome, Receipt, RefusalClass,
    StatedEntry, VerifiedEntry, Violation,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Deployment, JUDGEMENT_SHAPE, RESULT_SHAPE, SafeInteger, VERSION,
    signing_input,
};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_audit::log::kept::{base64url, from_base64url};
use rutent_core_audit::log::merkle::sha256;
use rutent_core_audit::log::{
    ADVERTISEMENT_NOT_KEPT, AuditLog, Clock, FileStore, KeptInMemory, MemoryStore, Settings,
    digest, record,
};
use rutent_core_audit::receipts::receipt::{KEY_ACROSS_PURPOSES, NOT_A_RECEIPT, NOT_COVERED};
use rutent_core_audit::receipts::store::{
    ADVERTISEMENT_NEVER_ERASED, ANSWER_OF_ANOTHER_READ, ERASURE_BY_THE_OPERATOR,
    EVIDENCE_RESTED_ON, FETCH_READS_EVIDENCE, MARK_WITHOUT_ITS_ERASURE, NO_RECORDED_ERASURE,
    NOT_A_KEPT_STATEMENT, NOT_KEPT, NOTHING_TO_KEEP,
};
use rutent_core_audit::receipts::{
    BoundStore, EvidenceStore, Issued, Issuer, Keeper, LogsRead, MemoryShelf, PartyReceipt, Shelf,
    ShelfContents, ShelfError, Shelved, bearing_out, check_kept, check_receipt, holds_against,
};
use serde_json::{Value, json};

// --- The deployment ----------------------------------------------------------------------------------------------

const LOG: &str = "log-main";
const OPERATOR: &str = "operator";
/// When every statement here was made, and when the log's clock stands, a second later.
const MADE_AT: &str = "2026-11-06T00:00:00Z";
const NOW: &str = "2026-11-06T00:00:01Z";

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

/// The log's clock, standing at [`NOW`].
#[derive(Debug)]
struct Fixed;

impl Clock for Fixed {
    fn unix_nanos(&self) -> i128 {
        Timestamp::new(NOW).unwrap().unix_nanos()
    }
}

fn settings() -> Settings {
    Settings::new(id(LOG), deployment(), id(OPERATOR), id(OPERATOR)).on(Rc::new(Fixed))
}

fn actor(party: &str, acts: &str, role: &str, kind: &str) -> Value {
    json!({"party": party, "acts": acts, "role": role, "kind": kind})
}

fn provider(name: &str) -> Value {
    actor(name, name, "provider", "component")
}

// --- Statements and entries ----------------------------------------------------------------------------------------

/// A statement of `shape` by `maker`, its body `body`, signed by `key`, as received.
fn statement(maker: &str, key: &Ed25519Key, shape: &str, body: Value) -> Vec<u8> {
    let mut envelope = json!({
        "deployment": deployment(),
        "protocol_version": VERSION,
        "shape": shape,
        "maker": maker,
        "made_at": MADE_AT,
        "signature": {"scheme": "ed25519", "key": "", "value": ""},
        "statement": body,
    });
    sign_value(&mut envelope, key);
    serde_json::to_vec(&envelope).unwrap()
}

/// `maker`'s judgement, `body` its body.
fn judgement(maker: &str, body: Value) -> Vec<u8> {
    statement(maker, &statements_key(maker), JUDGEMENT_SHAPE, body)
}

/// `provider`'s advertisement for the test deployment.
fn advertised(provider: &str) -> Vec<u8> {
    let context = SuiteContext {
        deployment: deployment(),
        ..SuiteContext::default()
    };
    advertisement(provider, &context)
}

/// The stated entry Core appends for `bytes`, kept under `commitment`: unverified, claimed by its envelope's maker,
/// labelled `shape`, about `about` where one is given.
fn stated_as(bytes: &[u8], commitment: &Digest, shape: &str, about: Option<Value>) -> GeneralEntry {
    let envelope: Value = serde_json::from_slice(bytes).unwrap();
    let mut entry = json!({
        "kind": "stated",
        "attribution": {"signature": "unverified", "claimed_by": provider(envelope["maker"].as_str().unwrap())},
        "what": {"statement": shape},
        "when": {"at": envelope["made_at"], "source": "party_stated"},
        "received_at": NOW,
        "outcome": {"status": "stated"},
        "signed_with": {"scheme": "ed25519", "key": envelope["signature"]["key"]},
        "commitment": commitment,
    });
    if let Some(about) = about {
        entry["about"] = about;
    }
    serde_json::from_value(entry).unwrap()
}

/// The stated entry for `bytes`, labelled with its own shape.
fn stated(bytes: &[u8], commitment: &Digest, about: Option<Value>) -> GeneralEntry {
    let envelope: Value = serde_json::from_slice(bytes).unwrap();
    stated_as(
        bytes,
        commitment,
        envelope["shape"].as_str().unwrap(),
        about,
    )
}

/// The operator's erasure of what `commitment` names, with its outcome `status`.
fn erasure(commitment: &Digest, status: &str) -> GeneralEntry {
    erasure_by(
        actor(OPERATOR, "op-login-2", "operator", "person"),
        commitment,
        status,
    )
}

/// `who`'s erasure of what `commitment` names, with its outcome `status`.
fn erasure_by(who: Value, commitment: &Digest, status: &str) -> GeneralEntry {
    serde_json::from_value(json!({
        "kind": "verified",
        "who": who,
        "what": {"call": "erasure", "evidence": commitment},
        "when": {"at": NOW, "source": "core_clock"},
        "outcome": {"status": status},
    }))
    .unwrap()
}

/// The operator's permitted decision to erase what `commitment` names, its effect and its done result, in `log`:
/// the path the log admits an erasure on. `n` tells its request and decision apart from others'.
fn decide_erasure(
    log: &mut (impl Log + PolicyLog),
    n: usize,
    commitment: &Digest,
) -> Result<(), AppendFailure> {
    let (request, decision) = (format!("req-erase-{n}"), format!("dec-erase-{n}"));
    let action =
        json!({"kind": "erase_statement", "commitment": commitment, "reason": "retention_ended"});
    let operator = actor(OPERATOR, "op-login-2", "operator", "person");
    let decided: VerifiedEntry = serde_json::from_value(json!({
        "kind": "verified",
        "who": operator,
        "what": {"call": "decide", "decision": decision,
                 "request": {"request": request, "caller": operator, "action": action, "at": NOW}},
        "when": {"at": NOW, "source": "core_clock"},
        "outcome": {"status": "permitted"},
    }))
    .unwrap();
    let permit = log.append_decision(&DecisionEntry::try_from(decided).unwrap())?;
    let core = actor("rutent", "core-orch", "core", "component");
    let effect: GeneralEntry = serde_json::from_value(json!({
        "kind": "verified",
        "who": core,
        "what": {"call": "effect", "action": action,
                 "decision": {"request": request, "decision": decision, "answer": "permit", "receipt": permit}},
        "when": {"at": NOW, "source": "core_clock"},
        "outcome": {"status": "to_take_effect"},
    }))
    .unwrap();
    let effect = log.append(&effect)?;
    let result: GeneralEntry = serde_json::from_value(json!({
        "kind": "verified",
        "who": core,
        "what": {"call": "result", "effect": effect},
        "when": {"at": NOW, "source": "core_clock"},
        "outcome": {"status": "done"},
    }))
    .unwrap();
    log.append(&result)?;
    Ok(())
}

/// A valid fixture of the contract.
fn valid(name: &str) -> Value {
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/valid/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A record Core writes about order `ord-17`.
fn about_the_order() -> GeneralEntry {
    serde_json::from_value(valid("verified-entry-of-a-destination-whose-owner-changed")).unwrap()
}

/// `reader`'s read check of `reads`.
fn read(request: &str, reader: &str, reads: Value) -> ReadCheck {
    serde_json::from_value(
        json!({"request": request, "reader": provider(reader), "reads": reads, "at": NOW}),
    )
    .unwrap()
}

/// `reader`'s read of the statement kept under `commitment`.
fn read_of(request: &str, reader: &str, commitment: &Digest) -> ReadCheck {
    read(
        request,
        reader,
        json!({"kind": "evidence", "commitment": commitment}),
    )
}

// --- Policy's read check ------------------------------------------------------------------------------------------

/// Policy's read check in a test: it allows every read, or refuses every one, and counts what it was asked. Its
/// clones share their switches.
#[derive(Clone, Default)]
struct Policy {
    refuse: Rc<Cell<bool>>,
    /// While set, it answers another request than the one asked.
    astray: Rc<Cell<bool>>,
    asked: Rc<Cell<usize>>,
}

impl calls::Reads for Policy {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        self.asked.set(self.asked.get() + 1);
        let outcome = if self.refuse.get() {
            ReadOutcome::Refused {
                class: RefusalClass::NotPermitted,
            }
        } else {
            ReadOutcome::Allowed {}
        };
        let request = if self.astray.get() {
            id("another-read")
        } else {
            check.request.clone()
        };
        Ok(ReadAnswer {
            request,
            outcome,
            receipt: Receipt {
                log: id(LOG),
                index: SafeInteger::try_from(0u64).unwrap(),
                entry: digest(&[0; 32]),
            },
        })
    }

    fn grant_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn revoke_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}

// --- Where the log and the evidence store live --------------------------------------------------------------------

/// A directory in the platform's temp directory, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rutent-core-audit-receipts-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn log(&self) -> PathBuf {
        self.0.join("log")
    }

    fn evidence(&self) -> PathBuf {
        self.0.join("evidence")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Every file under `dir`, with its bytes.
fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for item in std::fs::read_dir(dir).unwrap() {
        let path = item.unwrap().path();
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push((path.clone(), std::fs::read(&path).unwrap()));
        }
    }
    out
}

/// Whether `haystack` holds `needle`.
fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

type Shared = Rc<RefCell<AuditLog<FileStore, LogsRead>>>;

/// A log in its file and an evidence store in its directory, apart, bound to each other, with orchestration's
/// handle on the store, the log's issuer of receipts, and policy's read check behind both.
struct Run {
    log: Shared,
    store: BoundStore<FileStore, LogsRead>,
    /// The log's read of the store, for the test to look at.
    kept: LogsRead,
    keeper: Keeper,
    issuer: Issuer<FileStore, LogsRead>,
    policy: Policy,
}

impl Run {
    /// Opens the log and the evidence store kept under `dir`, the log with the store's read, and binds the store
    /// to the log.
    fn open(dir: &TempDir) -> Result<Self, AppendFailure> {
        let store = EvidenceStore::open(dir.evidence())?;
        let log = AuditLog::open(
            settings(),
            FileStore::open(dir.log()).unwrap(),
            store.for_the_log(),
        )?;
        let log = Rc::new(RefCell::new(log));
        let policy = Policy::default();
        let keeper = store.keeper(Box::new(policy.clone()));
        let kept = store.for_the_log();
        let issuer = Issuer::new(Rc::clone(&log), Box::new(policy.clone()), log_key()).unwrap();
        Ok(Run {
            store: store.bind(Rc::clone(&log))?,
            log,
            kept,
            keeper,
            issuer,
            policy,
        })
    }

    /// Keeps `bytes` and appends their stated entry, about `about`: the commitment and the receipt.
    fn state(&mut self, bytes: &[u8], about: Option<Value>) -> (Digest, Receipt) {
        let kept = self.keeper.keep(bytes).unwrap();
        let receipt = self
            .log
            .borrow_mut()
            .append(&stated(bytes, &kept.commitment, about))
            .unwrap();
        (kept.commitment, receipt)
    }

    /// Appends `entry`; an erasure first on its decided path, as the log admits one only after the done result of
    /// a permitted erase effect (core-internal's `erasure-is-carried-out-on-its-decision`).
    fn append(&self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        if let Entry::Verified(VerifiedEntry {
            what: rutent_contracts::core_internal::What::Erasure { evidence },
            ..
        }) = entry.entry()
        {
            self.decide_erasure(evidence)?;
        }
        self.log.borrow_mut().append(entry)
    }

    /// The operator's permitted decision to erase what `commitment` names, its effect and its done result.
    fn decide_erasure(&self, commitment: &Digest) -> Result<(), AppendFailure> {
        let n = self.log.borrow().entries().len();
        decide_erasure(&mut *self.log.borrow_mut(), n, commitment)
    }

    fn entries(&self) -> Vec<(Receipt, Entry)> {
        self.log.borrow().entries().to_vec()
    }

    fn stated_at(&self, receipt: &Receipt) -> StatedEntry {
        match &self.log.borrow().entries()[receipt.index.get() as usize].1 {
            Entry::Stated(s) => s.clone(),
            Entry::Verified(_) => panic!("a stated entry"),
        }
    }

    /// `reader`'s receipt of the entry at `index`, on a read of `reads`.
    fn receipt(&mut self, reader: &str, reads: Value, index: u64) -> Result<Issued, AppendFailure> {
        self.issuer
            .issue(&read("req-receipt", reader, reads), index as usize)
    }

    /// Fetches what `commitment` names, as `reader`.
    fn fetch(&mut self, reader: &str, commitment: &Digest) -> Result<Fetched, AppendFailure> {
        self.keeper.fetch(&read_of("req-fetch", reader, commitment))
    }

    /// Erases what `commitment` names: the check, the erasure's entry, then the erasure.
    fn erase(&self, commitment: &Digest) -> Result<Receipt, AppendFailure> {
        self.store.erasable(commitment)?;
        let at = self.append(&erasure(commitment, "done"))?;
        self.store.erase(&at)?;
        Ok(at)
    }
}

/// The receipt's bytes, where one was issued.
fn issued(answer: Result<Issued, AppendFailure>) -> Vec<u8> {
    match answer {
        Ok(Issued::Receipt(bytes)) => bytes,
        other => panic!("a receipt, not {other:?}"),
    }
}

/// The log's own key, which signs parties' receipts.
fn log_key() -> Ed25519Key {
    signing_key("the log of rutent-test", 0)
}

fn the_logs_key() -> rutent_contracts::provider_role::PublicKey {
    public_key(&log_key())
}

/// Checks `bytes` as a receipt of the test log's.
fn check(
    bytes: &[u8],
    key: &rutent_contracts::provider_role::PublicKey,
    entry: &Entry,
) -> Result<PartyReceipt, Violation> {
    check_receipt(bytes, key, &id(LOG), entry)
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

/// Removes the record at `index` of the log's file.
fn remove_record(file: &Path, index: usize) {
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

// --- The row's tests ------------------------------------------------------------------------------------------------

/// A party keeps its signed receipt of what it stated. When someone with the operator's full access later rewrites
/// that record, or removes one before it, the receipt still verifies under the log's key, and no longer holds
/// against the history the log shows: the party's receipt shows the changed record.
// verifies: REQ-AUD-5
#[test]
fn partys_receipt_shows_a_changed_record() {
    let dir = TempDir::new("changed");
    let mut run = Run::open(&dir).unwrap();
    let bytes = judgement("us-bank", json!({"kind": "judgement", "says": "eligible"}));
    let (commitment, at) = run.state(&bytes, Some(json!({"order": "ord-17"})));
    run.append(&about_the_order()).unwrap();
    let mine = issued(run.receipt(
        "us-bank",
        json!({"kind": "evidence", "commitment": commitment}),
        at.index.get(),
    ));
    let entry = run.entries()[0].1.clone();
    let receipt = check(&mine, &the_logs_key(), &entry).unwrap();
    assert!(holds_against(&receipt, &run.entries()));
    // The log grows; the receipt still holds against it.
    run.state(&judgement("bank-co", json!({"kind": "judgement"})), None);
    assert!(holds_against(&receipt, &run.entries()));
    drop(run);

    // The party's own record rewritten: what it stated is now said to be about another order.
    rewrite(&dir.log(), 0, |v| v["about"]["order"] = "ord-18".into());
    let run = Run::open(&dir).unwrap();
    assert_ne!(run.entries()[0].1, entry);
    assert!(check(&mine, &the_logs_key(), &entry).is_ok());
    assert!(!holds_against(&receipt, &run.entries()));
    drop(run);

    // Restored, it holds again; another record before the receipt's size changed, or one removed, shows too.
    rewrite(&dir.log(), 0, |v| v["about"]["order"] = "ord-17".into());
    assert!(holds_against(&receipt, &Run::open(&dir).unwrap().entries()));
    rewrite(&dir.log(), 1, |v| {
        v["when"]["at"] = "2026-11-06T00:00:02Z".into()
    });
    assert!(!holds_against(
        &receipt,
        &Run::open(&dir).unwrap().entries()
    ));
    rewrite(&dir.log(), 1, |v| {
        v["when"]["at"] = "2026-11-10T09:00:00Z".into()
    });
    assert!(holds_against(&receipt, &Run::open(&dir).unwrap().entries()));
    remove_record(&dir.log(), 1);
    assert!(!holds_against(
        &receipt,
        &Run::open(&dir).unwrap().entries()
    ));
}

/// A statement's bytes, a holding account's balance among them, are kept in the evidence store's own directory,
/// apart from the log's file; the log names them only by their salted commitment. Fetched by that commitment on a
/// read policy allows, they check against the stated entry as the contract gives the steps, and the log's file
/// holds neither the bytes, nor the salt, nor the signature, nor the unsalted digest of the signed content.
// verifies: REQ-AUD-5
#[test]
fn evidence_is_named_from_the_log_by_digest_and_stored_apart() {
    let dir = TempDir::new("apart");
    let mut run = Run::open(&dir).unwrap();
    let bytes = statement(
        "us-bank",
        &statements_key("us-bank"),
        RESULT_SHAPE,
        json!({"kind": "holding_balance", "balance": "4217.35", "account": "acct-ref-9"}),
    );
    let (commitment, at) = run.state(&bytes, Some(json!({"order": "ord-17"})));
    let entry = run.stated_at(&at);
    assert_eq!(entry.commitment, commitment);

    let Fetched::Kept(kept) = run.fetch("us-bank", &commitment).unwrap() else {
        panic!("the statement, kept");
    };
    assert_eq!(bearing_out(&kept).unwrap(), bytes);
    check_kept(&kept, &entry).unwrap();
    assert_eq!(run.policy.asked.get(), 1);
    // Against an entry naming another key, or another commitment, it doesn't check.
    let mut elsewhere = entry.clone();
    elsewhere.signed_with.key = public_key(&another_key());
    assert!(check_kept(&kept, &elsewhere).is_err());
    let mut elsewhere = entry.clone();
    elsewhere.commitment = digest(&[3; 32]);
    assert!(check_kept(&kept, &elsewhere).is_err());
    // Nor does a statement whose signature isn't its key's, though its bytes bear out their commitment.
    let mut forged: Value = serde_json::from_slice(&bytes).unwrap();
    forged["statement"]["balance"] = "1.00".into();
    let forged = serde_json::to_vec(&forged).unwrap();
    let (c, at) = run.state(&forged, None);
    let Fetched::Kept(k) = run.fetch("us-bank", &c).unwrap() else {
        panic!("the forged statement, kept");
    };
    assert!(bearing_out(&k).is_some());
    assert!(check_kept(&k, &run.stated_at(&at)).is_err());

    let log = std::fs::read(dir.log()).unwrap();
    assert!(holds(&log, commitment.value.as_str().as_bytes()));
    let envelope: Value = serde_json::from_slice(&bytes).unwrap();
    let content: String = sha256(&signing_input(&bytes).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    for secret in [
        b"4217.35".as_slice(),
        kept.bytes.as_str().as_bytes(),
        kept.salt.as_str().as_bytes(),
        envelope["signature"]["value"].as_str().unwrap().as_bytes(),
        content.as_bytes(),
    ] {
        assert!(!holds(&log, secret));
    }
    let shelved = files(&dir.evidence());
    assert!(
        shelved
            .iter()
            .any(|(_, b)| holds(b, kept.bytes.as_str().as_bytes()))
    );
    // The index of signed content is never written anywhere.
    assert!(!shelved.iter().any(|(_, b)| holds(b, content.as_bytes())));
    assert!(!dir.log().starts_with(dir.evidence()));
}

/// An erasure reaches only what its reference names, outside the log: the bytes and salt are gone from the evidence
/// store and a read answers erased, while the stated entry naming them stays, every earlier root is unchanged, a
/// receipt issued before still holds, and the log reopens with the evidence store.
// verifies: AC-36, REQ-AUD-13
#[test]
fn erasure_leaves_the_reference_and_history_still_checks() {
    let dir = TempDir::new("erasure");
    let mut run = Run::open(&dir).unwrap();
    let ad = advertised("us-bank");
    run.state(&ad, None);
    let bytes = judgement(
        "us-bank",
        json!({"kind": "judgement", "person": "ref-person-3"}),
    );
    let (commitment, at) = run.state(&bytes, Some(json!({"order": "ord-17"})));
    let other = judgement("bank-co", json!({"kind": "judgement"}));
    let (kept_on, _) = run.state(&other, None);
    let mine = issued(run.receipt(
        "us-bank",
        json!({"kind": "evidence", "commitment": commitment}),
        at.index.get(),
    ));
    let receipt = check(&mine, &the_logs_key(), &run.entries()[1].1).unwrap();
    let Fetched::Kept(kept) = run.fetch("us-bank", &commitment).unwrap() else {
        panic!("kept before its erasure");
    };
    let before = run.entries();
    let roots: Vec<_> = (0..=before.len())
        .map(|n| run.log.borrow().root_at(n).unwrap())
        .collect();

    let erased = run.erase(&commitment).unwrap();

    // What the reference named is gone, outside the log.
    assert_eq!(run.fetch("us-bank", &commitment).unwrap(), Fetched::Erased);
    assert_eq!(run.kept.kept(&commitment).unwrap(), None);
    assert!(run.store.is_erased(&commitment));
    for (path, b) in files(&dir.evidence()) {
        for trace in [
            kept.bytes.as_str().as_bytes(),
            kept.salt.as_str().as_bytes(),
            bytes.as_slice(),
        ] {
            assert!(
                !holds(&b, trace),
                "{path:?} holds a trace of the erased statement"
            );
        }
    }
    // The reference stays, and history still checks.
    let after = run.entries();
    assert_eq!(after[..before.len()], before[..]);
    assert!(matches!(
        &after[erased.index.get() as usize].1,
        Entry::Verified(_)
    ));
    for (n, root) in roots.iter().enumerate() {
        assert_eq!(run.log.borrow().root_at(n).as_ref(), Some(root));
    }
    assert!(check(&mine, &the_logs_key(), &before[1].1).is_ok());
    assert!(holds_against(&receipt, &after));
    // The rest stays kept.
    assert!(matches!(
        run.fetch("bank-co", &kept_on).unwrap(),
        Fetched::Kept(_)
    ));
    // Erasing it again is erasing nothing more.
    run.store.erase(&erased).unwrap();
    // The restatement index forgot it: the same statement kept again restates nothing.
    assert_eq!(run.keeper.keep(&bytes).unwrap().restates, None);
    drop(run);

    // Reopened, the log reads back every advertisement it needs, and the erasure holds.
    let mut run = Run::open(&dir).unwrap();
    assert_eq!(run.entries()[..after.len()], after[..]);
    assert_eq!(run.fetch("us-bank", &commitment).unwrap(), Fetched::Erased);
    assert!(holds_against(&receipt, &run.entries()));
}

// --- The store's rules ---------------------------------------------------------------------------------------------

/// The log is reopened with the durable evidence store it ran with: every advertisement it recorded is read back
/// from the store's directory. With an empty store in its place, it refuses to open.
// verifies: REQ-AUD-2, REQ-AUD-13
#[test]
fn the_log_reopens_with_the_durable_evidence_store_it_ran_with() {
    let dir = TempDir::new("reopen");
    let mut run = Run::open(&dir).unwrap();
    let ad = advertised("us-bank");
    let (commitment, _) = run.state(&ad, None);
    run.state(&judgement("us-bank", json!({"kind": "judgement"})), None);
    let entries = run.entries();
    drop(run);

    let mut run = Run::open(&dir).unwrap();
    assert_eq!(run.entries(), entries);
    let kept = run.kept.kept(&commitment).unwrap().unwrap();
    assert_eq!(bearing_out(&kept).unwrap(), ad);
    drop(run);

    let empty = EvidenceStore::in_memory();
    assert_eq!(
        AuditLog::open(
            settings(),
            FileStore::open(dir.log()).unwrap(),
            empty.for_the_log()
        )
        .err(),
        Some(AppendFailure::Refused(Violation(ADVERTISEMENT_NOT_KEPT)))
    );
}

/// A statement kept again, as received or as a relay passed it on with its members reordered, is found as a
/// restatement of the one kept first, before and after the store is reopened, and the index that finds it is in no
/// file.
// verifies: REQ-AUD-2
#[test]
fn a_restatement_is_found_in_the_store_and_survives_a_reopen() {
    let dir = TempDir::new("restated");
    let store = EvidenceStore::open(dir.evidence()).unwrap();
    let mut keeper = store.keeper(Box::new(Policy::default()));
    let bytes = judgement("us-bank", json!({"kind": "judgement", "says": "eligible"}));
    let first = keeper.keep(&bytes).unwrap();
    assert_eq!(first.restates, None);
    assert_eq!(
        keeper.keep(&bytes).unwrap().restates,
        Some(first.commitment.clone())
    );
    // The same signed content, as a relay passed it on: its members in another order, with spaces.
    let relayed =
        serde_json::to_vec_pretty(&serde_json::from_slice::<Value>(&bytes).unwrap()).unwrap();
    assert_ne!(relayed, bytes);
    assert_eq!(
        keeper.keep(&relayed).unwrap().restates,
        Some(first.commitment.clone())
    );
    // Another statement restates nothing.
    let another = judgement(
        "us-bank",
        json!({"kind": "judgement", "says": "ineligible"}),
    );
    assert_eq!(keeper.keep(&another).unwrap().restates, None);
    drop((keeper, store));

    let store = EvidenceStore::open(dir.evidence()).unwrap();
    let mut keeper = store.keeper(Box::new(Policy::default()));
    assert_eq!(
        keeper.keep(&bytes).unwrap().restates,
        Some(first.commitment)
    );
}

/// The same bytes kept twice are kept under two fresh salts, so two commitments: nothing in the log lets anyone
/// tell they are one statement, or confirm a guess of what it says.
// verifies: REQ-AUD-6, REQ-AUD-2
#[test]
fn one_statement_kept_twice_gets_two_salts_and_two_commitments() {
    let store = EvidenceStore::in_memory();
    let mut keeper = store.keeper(Box::new(Policy::default()));
    let bytes = judgement("us-bank", json!({"kind": "judgement"}));
    let (a, b) = (keeper.keep(&bytes).unwrap(), keeper.keep(&bytes).unwrap());
    assert_ne!(a.commitment, b.commitment);
    let mut log = store.for_the_log();
    let (ka, kb) = (
        log.kept(&a.commitment).unwrap().unwrap(),
        log.kept(&b.commitment).unwrap().unwrap(),
    );
    assert_ne!(ka.salt, kb.salt);
    for k in [&ka, &kb] {
        assert_eq!(bearing_out(k).unwrap(), bytes);
    }
    assert_eq!(
        keeper.keep(b""),
        Err(AppendFailure::Refused(Violation(NOTHING_TO_KEEP)))
    );
}

/// A read policy refuses hands over nothing, and tells kept, erased and never kept apart for no one: policy is asked
/// first, every time. A fetch of anything but a kept statement, or one policy answers for another read, is refused.
// verifies: REQ-AUD-5, REQ-AUZ-7
#[test]
fn a_read_policy_refuses_hands_over_nothing() {
    let dir = TempDir::new("refused");
    let mut run = Run::open(&dir).unwrap();
    let (kept, _) = run.state(&judgement("us-bank", json!({"kind": "judgement"})), None);
    let (gone, _) = run.state(
        &judgement("us-bank", json!({"kind": "judgement", "n": 2})),
        None,
    );
    run.erase(&gone).unwrap();
    let never = digest(&[7; 32]);
    run.policy.refuse.set(true);
    for c in [&kept, &gone, &never] {
        let asked = run.policy.asked.get();
        assert!(
            matches!(run.fetch("bank-co", c).unwrap(), Fetched::Refused(a) if matches!(a.outcome, ReadOutcome::Refused { .. }))
        );
        assert_eq!(run.policy.asked.get(), asked + 1);
    }
    run.policy.refuse.set(false);
    assert!(matches!(
        run.fetch("us-bank", &kept).unwrap(),
        Fetched::Kept(_)
    ));
    assert_eq!(run.fetch("us-bank", &gone).unwrap(), Fetched::Erased);
    assert_eq!(
        run.fetch("us-bank", &never),
        Err(AppendFailure::Refused(Violation(NOT_KEPT)))
    );
    let asked = run.policy.asked.get();
    assert_eq!(
        run.keeper
            .fetch(&read("req-trail", "us-bank", json!({"kind": "trail"}))),
        Err(AppendFailure::Refused(Violation(FETCH_READS_EVIDENCE)))
    );
    assert_eq!(run.policy.asked.get(), asked);
    run.policy.astray.set(true);
    assert_eq!(
        run.fetch("us-bank", &kept),
        Err(AppendFailure::Refused(Violation(ANSWER_OF_ANOTHER_READ)))
    );
}

/// A shelf in memory whose writes fail while told to. Its clones share it.
#[derive(Clone, Default)]
struct Faulty {
    inner: Rc<RefCell<MemoryShelf>>,
    fail: Rc<Cell<bool>>,
    /// While set, a write reaches the shelf and then fails.
    after: Rc<Cell<bool>>,
}

impl Shelf for Faulty {
    fn put(&mut self, shelved: &Shelved) -> Result<(), ShelfError> {
        if self.fail.get() {
            return Err(ShelfError::Unavailable("injected".into()));
        }
        self.inner.borrow_mut().put(shelved)?;
        if self.after.get() {
            return Err(ShelfError::Unavailable("injected after".into()));
        }
        Ok(())
    }

    fn erase(&mut self, commitment: &Digest) -> Result<(), ShelfError> {
        if self.fail.get() {
            return Err(ShelfError::Unavailable("injected".into()));
        }
        self.inner.borrow_mut().erase(commitment)
    }

    fn load(&mut self) -> Result<ShelfContents, ShelfError> {
        self.inner.borrow_mut().load()
    }
}

/// A keep the shelf fails to write keeps nothing and answers unavailable, so no stated entry can name it; an erasure
/// the shelf fails to carry out erases nothing, and carried out again once it can, erases it.
// verifies: INV-7, REQ-AUD-13
#[test]
fn a_shelf_that_cant_write_keeps_and_erases_nothing() {
    let shelf = Faulty::default();
    let store = EvidenceStore::on(Box::new(shelf.clone())).unwrap();
    let mut keeper = store.keeper(Box::new(Policy::default()));
    let mut kept_now = store.for_the_log();
    let log = Rc::new(RefCell::new(
        AuditLog::open(settings(), MemoryStore::new(), store.for_the_log()).unwrap(),
    ));
    let store = store.bind(Rc::clone(&log)).unwrap();
    let bytes = judgement("us-bank", json!({"kind": "judgement"}));

    shelf.fail.set(true);
    assert_eq!(keeper.keep(&bytes), Err(AppendFailure::Unavailable));
    assert!(shelf.inner.borrow_mut().load().unwrap().kept.is_empty());
    shelf.fail.set(false);
    let kept = keeper.keep(&bytes).unwrap();
    // The failed keep left nothing in the index either.
    assert_eq!(kept.restates, None);

    log.borrow_mut()
        .append(&stated(&bytes, &kept.commitment, None))
        .unwrap();
    store.erasable(&kept.commitment).unwrap();
    let n = log.borrow().entries().len();
    decide_erasure(&mut *log.borrow_mut(), n, &kept.commitment).unwrap();
    let at = log
        .borrow_mut()
        .append(&erasure(&kept.commitment, "done"))
        .unwrap();
    shelf.fail.set(true);
    assert_eq!(store.erase(&at), Err(AppendFailure::Unavailable));
    assert!(kept_now.kept(&kept.commitment).unwrap().is_some());
    assert!(!store.is_erased(&kept.commitment));
    shelf.fail.set(false);
    store.erase(&at).unwrap();
    assert!(store.is_erased(&kept.commitment));
    // Reopened on the shelf and bound to the log, the store holds what it held.
    let again = EvidenceStore::on(Box::new(shelf.clone()))
        .unwrap()
        .bind(Rc::clone(&log))
        .unwrap();
    assert!(again.is_erased(&kept.commitment));
}

/// A write that reached the shelf before it failed still takes its place, so no later statement shares it, and
/// the order statements were kept in, which restatements are found by, survives a reopen.
// verifies: REQ-AUD-2, INV-7
#[test]
fn a_write_that_fails_after_reaching_the_shelf_keeps_its_place_to_itself() {
    let shelf = Faulty::default();
    let store = EvidenceStore::on(Box::new(shelf.clone())).unwrap();
    let mut keeper = store.keeper(Box::new(Policy::default()));
    shelf.after.set(true);
    let first = judgement("us-bank", json!({"kind": "judgement", "n": 1}));
    assert_eq!(keeper.keep(&first), Err(AppendFailure::Unavailable));
    shelf.after.set(false);
    keeper
        .keep(&judgement("us-bank", json!({"kind": "judgement", "n": 2})))
        .unwrap();
    let places: BTreeSet<u64> = shelf
        .inner
        .borrow_mut()
        .load()
        .unwrap()
        .kept
        .iter()
        .map(|s| s.place)
        .collect();
    assert_eq!(places.len(), 2);
}

/// A statement on the store's shelf whose bytes don't bear out its commitment, or filed under another's, refuses the
/// store: it opens on none of it.
// verifies: REQ-AUD-2, REQ-AUD-5
#[test]
fn a_kept_statement_that_doesnt_bear_out_its_commitment_refuses_the_store() {
    let dir = TempDir::new("tampered");
    let store = EvidenceStore::open(dir.evidence()).unwrap();
    let mut keeper = store.keeper(Box::new(Policy::default()));
    let kept = keeper
        .keep(&judgement("us-bank", json!({"kind": "judgement"})))
        .unwrap();
    let other = keeper
        .keep(&judgement("bank-co", json!({"kind": "judgement"})))
        .unwrap();
    drop((keeper, store));
    let file = dir
        .evidence()
        .join("kept")
        .join(format!("{}.json", kept.commitment.value.as_str()));
    let original = std::fs::read(&file).unwrap();

    let mut shelved: Value = serde_json::from_slice(&original).unwrap();
    let altered = base64url(&judgement(
        "us-bank",
        json!({"kind": "judgement", "says": "altered"}),
    ));
    shelved["kept"]["bytes"] = altered.into();
    std::fs::write(&file, serde_json::to_vec(&shelved).unwrap()).unwrap();
    assert_eq!(
        EvidenceStore::open(dir.evidence()).err(),
        Some(AppendFailure::Refused(Violation(NOT_A_KEPT_STATEMENT)))
    );

    std::fs::write(&file, &original).unwrap();
    assert!(EvidenceStore::open(dir.evidence()).is_ok());
    let misfiled = dir
        .evidence()
        .join("kept")
        .join(format!("{}.json", other.commitment.value.as_str()));
    std::fs::rename(&file, &misfiled).unwrap();
    assert_eq!(
        EvidenceStore::open(dir.evidence()).err(),
        Some(AppendFailure::Refused(Violation(NOT_A_KEPT_STATEMENT)))
    );
}

/// The store's directory is its own while it lives: a second store can't open it.
// verifies: REQ-AUD-2
#[test]
fn a_second_store_cant_open_the_directory_one_holds() {
    let dir = TempDir::new("locked");
    let store = EvidenceStore::open(dir.evidence()).unwrap();
    assert_eq!(
        EvidenceStore::open(dir.evidence()).err(),
        Some(AppendFailure::Unavailable)
    );
    drop(store);
    assert!(EvidenceStore::open(dir.evidence()).is_ok());
}

// --- Erasure's rules ------------------------------------------------------------------------------------------------

/// No erasure reaches an advertisement, whether its bytes are one or a stated entry labels them one: the log reads
/// them back on opening, and still opens.
// verifies: REQ-AUD-13, REQ-AUD-2
#[test]
fn no_erasure_reaches_an_advertisement() {
    let dir = TempDir::new("advertisement");
    let mut run = Run::open(&dir).unwrap();
    let refused = Err(AppendFailure::Refused(Violation(
        ADVERTISEMENT_NEVER_ERASED,
    )));

    let ad = advertised("us-bank");
    let (by_bytes, _) = run.state(&ad, None);
    assert_eq!(run.erase(&by_bytes).map(|_| ()), refused);
    // Recorded anyway, the erasure is still not carried out.
    let at = run.append(&erasure(&by_bytes, "done")).unwrap();
    assert_eq!(run.store.erase(&at), refused);

    // An advertisement's bytes, labelled as another statement by the only entry naming them.
    let mislabelled = run.keeper.keep(&ad).unwrap();
    run.append(&stated_as(
        &ad,
        &mislabelled.commitment,
        JUDGEMENT_SHAPE,
        None,
    ))
    .unwrap();
    assert_eq!(run.erase(&mislabelled.commitment).map(|_| ()), refused);

    // A judgement's bytes, labelled an advertisement by a stated entry.
    let bytes = judgement("us-bank", json!({"kind": "judgement"}));
    let kept = run.keeper.keep(&bytes).unwrap();
    run.append(&stated(&bytes, &kept.commitment, None)).unwrap();
    run.append(&stated_as(
        &bytes,
        &kept.commitment,
        ADVERTISEMENT_SHAPE,
        None,
    ))
    .unwrap();
    assert_eq!(run.erase(&kept.commitment).map(|_| ()), refused);
    let at = run.append(&erasure(&kept.commitment, "done")).unwrap();
    assert_eq!(run.store.erase(&at), refused);
    drop(run);

    let mut run = Run::open(&dir).unwrap();
    // The erasures recorded but refused are reported when the store is bound again.
    let refused: Vec<_> = run.store.refused_erasures().to_vec();
    assert_eq!(refused.len(), 2);
    assert!(
        refused
            .iter()
            .all(|(_, why)| *why == Violation(ADVERTISEMENT_NEVER_ERASED))
    );
    assert!(matches!(
        run.fetch("us-bank", &by_bytes).unwrap(),
        Fetched::Kept(_)
    ));
    assert!(matches!(
        run.fetch("us-bank", &kept.commitment).unwrap(),
        Fetched::Kept(_)
    ));
}

/// An erasure is carried out only on the log's own record of it, a `done` erasure entry naming the statement: on no
/// receipt of another entry, of a failed erasure, or of no entry of this log, so nothing is erased that the log
/// doesn't reference first. The log itself refuses an erasure naming what no stated entry holds.
// verifies: REQ-AUD-13, INV-7
#[test]
fn an_erasure_the_log_holds_no_record_of_erases_nothing() {
    let dir = TempDir::new("unrecorded");
    let mut run = Run::open(&dir).unwrap();
    let bytes = judgement("us-bank", json!({"kind": "judgement"}));
    let (commitment, stated_at) = run.state(&bytes, None);
    let refused = Err(AppendFailure::Refused(Violation(NO_RECORDED_ERASURE)));

    assert_eq!(run.store.erase(&stated_at), refused);
    let failed = run.append(&erasure(&commitment, "failed")).unwrap();
    assert_eq!(run.store.erase(&failed), refused);
    let done = run.append(&erasure(&commitment, "done")).unwrap();
    let mut forged = done.clone();
    forged.index = SafeInteger::try_from(done.index.get() + 1).unwrap();
    assert_eq!(run.store.erase(&forged), refused);
    let mut forged = done.clone();
    forged.entry = digest(&[9; 32]);
    assert_eq!(run.store.erase(&forged), refused);
    assert!(matches!(
        run.fetch("us-bank", &commitment).unwrap(),
        Fetched::Kept(_)
    ));

    // Kept but named by no stated entry: the log records no erasure of it.
    let unnamed = run
        .keeper
        .keep(&judgement("bank-co", json!({"kind": "judgement"})))
        .unwrap();
    assert!(matches!(
        run.append(&erasure(&unnamed.commitment, "done")),
        Err(AppendFailure::Refused(_))
    ));
    assert!(matches!(
        run.fetch("bank-co", &unnamed.commitment).unwrap(),
        Fetched::Kept(_)
    ));

    // On its record, it is carried out.
    run.store.erase(&done).unwrap();
    assert_eq!(run.fetch("us-bank", &commitment).unwrap(), Fetched::Erased);
}

/// Bytes a crash left behind their mark, between the shelf's mark and its removal of them, are removed when the
/// store is opened again, and the bound store reads them erased.
// verifies: REQ-AUD-13
#[test]
fn bytes_left_behind_their_erasure_mark_are_removed_on_opening() {
    let dir = TempDir::new("left-behind");
    let mut run = Run::open(&dir).unwrap();
    let bytes = judgement("us-bank", json!({"kind": "judgement"}));
    let (commitment, _) = run.state(&bytes, None);
    let kept = run.kept.kept(&commitment).unwrap().unwrap();
    run.append(&erasure(&commitment, "done")).unwrap();
    drop(run);
    // The crash: the mark is written, the bytes are not yet removed.
    let value = commitment.value.as_str();
    std::fs::write(dir.evidence().join("erased").join(value), b"").unwrap();
    let file = dir.evidence().join("kept").join(format!("{value}.json"));
    assert!(file.exists());

    let mut run = Run::open(&dir).unwrap();
    assert!(run.store.is_erased(&commitment));
    assert_eq!(run.kept.kept(&commitment).unwrap(), None);
    assert!(!file.exists());
    for (_, b) in files(&dir.evidence()) {
        assert!(!holds(&b, kept.bytes.as_str().as_bytes()));
    }
}

/// An erasure the log recorded done, cut short by a crash before the store marked it, is carried out when the
/// store is bound to the log again.
// verifies: REQ-AUD-13, AC-36
#[test]
fn binding_carries_out_an_erasure_the_log_recorded_before_a_crash() {
    let dir = TempDir::new("pending");
    let mut run = Run::open(&dir).unwrap();
    let bytes = judgement("us-bank", json!({"kind": "judgement"}));
    let (commitment, _) = run.state(&bytes, None);
    let kept = run.kept.kept(&commitment).unwrap().unwrap();
    run.store.erasable(&commitment).unwrap();
    run.append(&erasure(&commitment, "done")).unwrap();
    drop(run);

    let mut run = Run::open(&dir).unwrap();
    assert!(run.store.is_erased(&commitment));
    assert_eq!(run.fetch("us-bank", &commitment).unwrap(), Fetched::Erased);
    for (_, b) in files(&dir.evidence()) {
        assert!(!holds(&b, kept.bytes.as_str().as_bytes()));
    }
    assert!(run.store.missing().is_empty());
}

/// A statement marked erased that no erasure of the operator's in the log records refuses the binding: its bytes
/// went with no record.
// verifies: REQ-AUD-13, INV-7
#[test]
fn binding_refuses_a_statement_marked_erased_with_no_erasure_in_the_log() {
    let dir = TempDir::new("unrecorded-mark");
    let mut run = Run::open(&dir).unwrap();
    let (commitment, _) = run.state(&judgement("us-bank", json!({"kind": "judgement"})), None);
    drop(run);
    let value = commitment.value.as_str();
    std::fs::write(dir.evidence().join("erased").join(value), b"").unwrap();
    assert_eq!(
        Run::open(&dir).err(),
        Some(AppendFailure::Refused(Violation(MARK_WITHOUT_ITS_ERASURE)))
    );
}

/// A stated entry whose bytes the store neither keeps nor marks erased, gone with no erasure, is reported when the
/// store is bound, for the outside checker, and the rest is bound as before.
// verifies: REQ-AUD-13, REQ-AUD-9
#[test]
fn binding_reports_a_statement_whose_bytes_are_gone_with_no_erasure() {
    let dir = TempDir::new("gone");
    let mut run = Run::open(&dir).unwrap();
    let (gone, at) = run.state(&judgement("us-bank", json!({"kind": "judgement"})), None);
    let (stays, _) = run.state(&judgement("bank-co", json!({"kind": "judgement"})), None);
    let (erased, _) = run.state(&judgement("fx-desk", json!({"kind": "judgement"})), None);
    run.erase(&erased).unwrap();
    assert!(run.store.missing().is_empty());
    drop(run);
    std::fs::remove_file(
        dir.evidence()
            .join("kept")
            .join(format!("{}.json", gone.value.as_str())),
    )
    .unwrap();

    let mut run = Run::open(&dir).unwrap();
    assert_eq!(run.store.missing(), [at]);
    assert!(matches!(
        run.fetch("bank-co", &stays).unwrap(),
        Fetched::Kept(_)
    ));
}

/// An erasure is the operator's: one recorded by a provider, or in the operator's role for another party, is not
/// carried out, and the store, bound to its one log, carries out none another log records, however that log holds
/// the same commitment.
// verifies: REQ-AUD-13, REQ-AUZ-1
#[test]
fn an_erasure_by_anyone_but_the_operator_is_refused() {
    let dir = TempDir::new("whose-erasure");
    let mut run = Run::open(&dir).unwrap();
    let bytes = judgement("us-bank", json!({"kind": "judgement"}));
    let (commitment, _) = run.state(&bytes, None);
    let refused = Err(AppendFailure::Refused(Violation(ERASURE_BY_THE_OPERATOR)));

    let by_provider = run
        .append(&erasure_by(provider("us-bank"), &commitment, "done"))
        .unwrap();
    assert_eq!(run.store.erase(&by_provider), refused);
    let by_another = run
        .append(&erasure_by(
            actor("us-bank", "op-login-9", "operator", "person"),
            &commitment,
            "done",
        ))
        .unwrap();
    assert_eq!(run.store.erase(&by_another), refused);

    // Another log holding the same statement, under the same commitment, and the operator's erasure of it.
    let kept = run.kept.kept(&commitment).unwrap().unwrap();
    let scratch_kept = KeptInMemory::new();
    let salt: [u8; 32] = from_base64url(kept.salt.as_str())
        .unwrap()
        .try_into()
        .unwrap();
    assert_eq!(scratch_kept.keep(&bytes, salt), commitment);
    let mut scratch = AuditLog::open(settings(), MemoryStore::new(), scratch_kept).unwrap();
    scratch.append(&stated(&bytes, &commitment, None)).unwrap();
    let n = scratch.entries().len();
    decide_erasure(&mut scratch, n, &commitment).unwrap();
    let elsewhere = scratch.append(&erasure(&commitment, "done")).unwrap();
    assert_eq!(
        run.store.erase(&elsewhere),
        Err(AppendFailure::Refused(Violation(NO_RECORDED_ERASURE)))
    );
    assert!(matches!(
        run.fetch("us-bank", &commitment).unwrap(),
        Fetched::Kept(_)
    ));

    run.erase(&commitment).unwrap();
    assert_eq!(run.fetch("us-bank", &commitment).unwrap(), Fetched::Erased);
    drop(run);
    // Bound again, the two erasures recorded by others are reported as refused, and nothing else.
    let run = Run::open(&dir).unwrap();
    assert_eq!(
        run.store.refused_erasures(),
        [
            (by_provider, Violation(ERASURE_BY_THE_OPERATOR)),
            (by_another, Violation(ERASURE_BY_THE_OPERATOR)),
        ]
    );
}

/// No erasure reaches a statement an entry of the log rests on, here a measure citing it, while one nothing rests
/// on is erased; a read of a statement rests on nothing. The log refuses the erasure's decision too, so it records
/// no erasure the store must refuse (core-internal's `erasure-waits-while-anything-rests-on-the-statement`).
// verifies: REQ-AUD-13, REQ-AUD-2
#[test]
fn no_erasure_reaches_a_statement_an_entry_rests_on() {
    let dir = TempDir::new("rested-on");
    let mut run = Run::open(&dir).unwrap();
    let (cited, at) = run.state(&judgement("fx-desk", json!({"kind": "judgement"})), None);
    let (free, _) = run.state(&judgement("bank-co", json!({"kind": "judgement"})), None);
    let mut measure = valid("verified-entry-of-a-measure");
    measure["what"]["record"]["cites"] = json!([at]);
    run.append(&serde_json::from_value(measure).unwrap())
        .unwrap();
    let refused = Err(AppendFailure::Refused(Violation(EVIDENCE_RESTED_ON)));

    assert_eq!(run.store.erasable(&cited), refused);
    assert_eq!(
        run.append(&erasure(&cited, "done")),
        Err(AppendFailure::Refused(Violation(
            "no statement is erased while an entry of this log rests on it"
        )))
    );
    assert!(matches!(
        run.fetch("fx-desk", &cited).unwrap(),
        Fetched::Kept(_)
    ));
    // A stuck order's decision names its evidence by commitment: what it relies on is not erased either.
    let (relied, _) = run.state(
        &judgement("bank-co", json!({"kind": "judgement", "n": 3})),
        None,
    );
    run.append(
        &serde_json::from_value(json!({
            "kind": "verified",
            "who": actor("rutent", "core-orch", "core", "component"),
            "what": {"call": "input", "input": {
                "record_id": "rec-0003", "recorded_at": "2026-11-06T10:00:00Z", "time_source": "core-orch clock",
                "source": {"kind": "orchestrator", "id": "core-orch"}, "standing": "stated",
                "subject": {"order": "ord-21", "part": "p3"},
                "observed": {"kind": "stuck_declared", "reason": "over_time"},
            }},
            "when": {"at": NOW, "source": "core_clock"},
            "outcome": {"status": "observed"},
        }))
        .unwrap(),
    )
    .unwrap();
    let mut decided = valid("verified-entry-of-a-stuck-decision");
    decided["what"]["request"]["evidence"] = json!([relied]);
    let decided: VerifiedEntry = serde_json::from_value(decided).unwrap();
    run.log
        .borrow_mut()
        .append_decision(&DecisionEntry::try_from(decided).unwrap())
        .unwrap();
    assert_eq!(run.store.erasable(&relied), refused);
    // A read of the other, recorded by policy, rests on nothing.
    let mut read = valid("verified-entry-of-a-refused-read");
    read["what"]["check"]["reads"] = json!({"kind": "evidence", "commitment": free});
    let read: VerifiedEntry = serde_json::from_value(read).unwrap();
    run.log
        .borrow_mut()
        .append_decision(&DecisionEntry::try_from(read).unwrap())
        .unwrap();
    run.erase(&free).unwrap();
    assert_eq!(run.fetch("bank-co", &free).unwrap(), Fetched::Erased);
    drop(run);
    // Reopened, the statement rested on is still kept, and no erasure of it was recorded to refuse.
    let mut run = Run::open(&dir).unwrap();
    assert!(matches!(
        run.fetch("fx-desk", &cited).unwrap(),
        Fetched::Kept(_)
    ));
    assert!(run.store.refused_erasures().is_empty());
}

// --- Receipts' rules -----------------------------------------------------------------------------------------------

/// A receipt altered after it was signed, signed by another key than the log's, or checked against another entry,
/// is refused.
// verifies: REQ-AUD-5
#[test]
fn a_receipt_altered_or_signed_by_another_key_is_refused() {
    let dir = TempDir::new("tampered-receipt");
    let mut run = Run::open(&dir).unwrap();
    run.state(&judgement("bank-co", json!({"kind": "judgement"})), None);
    let (commitment, at) = run.state(&judgement("us-bank", json!({"kind": "judgement"})), None);
    run.state(&judgement("fx-desk", json!({"kind": "judgement"})), None);
    let reads = json!({"kind": "evidence", "commitment": commitment});
    let bytes = issued(run.receipt("us-bank", reads.clone(), at.index.get()));
    let entries = run.entries();
    let (entry, another) = (&entries[1].1, &entries[0].1);
    let refused = Err(Violation(NOT_A_RECEIPT));
    assert!(check(&bytes, &the_logs_key(), entry).is_ok());
    assert_eq!(
        check_receipt(&bytes, &the_logs_key(), &id("log-other"), entry).map(|_| ()),
        Err(Violation(NOT_A_RECEIPT))
    );
    assert_eq!(check(&bytes, &the_logs_key(), another).map(|_| ()), refused);
    assert_eq!(
        check(&bytes, &public_key(&operator_key()), entry).map(|_| ()),
        refused
    );

    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    for (field, altered) in [
        ("root", serde_json::to_value(digest(&[1; 32])).unwrap()),
        ("index", json!(0)),
        ("tree_size", json!(2)),
        ("inclusion", json!([])),
        ("shape", json!("urn:rutent:provider-role:result")),
        ("issued_at", json!("2026-11-07T00:00:01Z")),
        ("log", json!("log-other")),
    ] {
        let original = value[field].clone();
        value[field] = altered;
        let altered = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            check(&altered, &the_logs_key(), entry).map(|_| ()),
            refused,
            "{field}"
        );
        value[field] = original;
    }

    // Signed anew by another key over the same content, it is not the log's word.
    let mut resigned = value.clone();
    sign_value(&mut resigned, &another_key());
    let resigned = serde_json::to_vec(&resigned).unwrap();
    assert_eq!(
        check(&resigned, &the_logs_key(), entry).map(|_| ()),
        refused
    );
}

/// A receipt is issued only on a read policy allows, and only of an entry that read covers: a party's own kept
/// statement, an entry naming its order or segment, or any entry for a read of the trail.
// verifies: REQ-AUD-5, REQ-AUZ-7
#[test]
fn a_receipt_is_issued_only_for_an_entry_the_allowed_read_covers() {
    let dir = TempDir::new("covers");
    let mut run = Run::open(&dir).unwrap();
    let (mine, _) = run.state(&judgement("us-bank", json!({"kind": "judgement"})), None);
    let (theirs, _) = run.state(&judgement("bank-co", json!({"kind": "judgement"})), None);
    run.append(&about_the_order()).unwrap();
    let not_covered = Err(AppendFailure::Refused(Violation(NOT_COVERED)));
    let evidence = |c: &Digest| json!({"kind": "evidence", "commitment": c});

    issued(run.receipt("us-bank", evidence(&mine), 0));
    assert_eq!(run.receipt("us-bank", evidence(&mine), 1), not_covered);
    assert_eq!(run.receipt("us-bank", evidence(&theirs), 0), not_covered);
    assert_eq!(run.receipt("us-bank", evidence(&mine), 9), not_covered);
    issued(run.receipt("us-bank", json!({"kind": "order", "order": "ord-17"}), 2));
    assert_eq!(
        run.receipt("us-bank", json!({"kind": "order", "order": "ord-18"}), 2),
        not_covered
    );
    assert_eq!(
        run.receipt("us-bank", json!({"kind": "order", "order": "ord-17"}), 0),
        not_covered
    );
    assert_eq!(
        run.receipt(
            "us-bank",
            json!({"kind": "segment", "order": "ord-17", "segment": "seg-1", "provider": "us-bank"}),
            2
        ),
        not_covered
    );
    // An order's state alone is no entry of the log: not the order's own record, nor a statement about it.
    for index in 0..3 {
        assert_eq!(
            run.receipt(
                "us-bank",
                json!({"kind": "order_state", "order": "ord-17"}),
                index
            ),
            not_covered
        );
    }
    issued(run.receipt("us-bank", json!({"kind": "trail"}), 1));
    assert_eq!(
        run.receipt(
            "us-bank",
            json!({"kind": "resource", "resource": "checker"}),
            1
        ),
        not_covered
    );

    run.policy.astray.set(true);
    assert_eq!(
        run.receipt("us-bank", evidence(&mine), 0),
        Err(AppendFailure::Refused(Violation(ANSWER_OF_ANOTHER_READ)))
    );
    run.policy.astray.set(false);
    run.policy.refuse.set(true);
    assert!(matches!(
        run.receipt("us-bank", evidence(&mine), 0).unwrap(),
        Issued::Refused(a) if matches!(a.outcome, ReadOutcome::Refused { .. })
    ));
}

/// The log's receipts are signed with a key of the log's own, never the deployment's operator key, which signs the
/// operator's statements.
// verifies: REQ-AUD-5, REQ-AUD-2
#[test]
fn the_receipts_key_is_never_the_operators() {
    let log = Rc::new(RefCell::new(
        AuditLog::open(
            settings(),
            MemoryStore::new(),
            EvidenceStore::in_memory().for_the_log(),
        )
        .unwrap(),
    ));
    assert_eq!(
        Issuer::new(log, Box::new(Policy::default()), operator_key()).err(),
        Some(Violation(KEY_ACROSS_PURPOSES))
    );
    assert_ne!(the_logs_key(), public_key(&operator_key()));
}

/// Names an item only for a type that doesn't implement [`KeptStatements`]: for one that does, the two impls make
/// the name ambiguous and the test doesn't build.
trait NotKeptStatements<A> {
    fn holds() {}
}
impl<T: ?Sized> NotKeptStatements<()> for T {}
impl<T: ?Sized + KeptStatements> NotKeptStatements<u8> for T {}

/// The same for [`Evidence`].
trait NotEvidence<A> {
    fn holds() {}
}
impl<T: ?Sized> NotEvidence<()> for T {}
impl<T: ?Sized + Evidence> NotEvidence<u8> for T {}

/// The log's read hands out statements with no read check and keeps none, and orchestration's handle keeps them and
/// hands them out only through policy: neither is the other, and the store itself is neither.
// verifies: REQ-AUD-5, DEC-2
#[test]
fn the_logs_read_and_orchestrations_handle_each_answer_one_call() {
    <Keeper as NotKeptStatements<_>>::holds();
    <LogsRead as NotEvidence<_>>::holds();
    <EvidenceStore as NotKeptStatements<_>>::holds();
    <EvidenceStore as NotEvidence<_>>::holds();
    <BoundStore<MemoryStore, LogsRead> as NotKeptStatements<_>>::holds();
    <BoundStore<MemoryStore, LogsRead> as NotEvidence<_>>::holds();
    let _: &dyn Evidence = &EvidenceStore::in_memory().keeper(Box::new(Policy::default()));
    let _: &dyn KeptStatements = &EvidenceStore::in_memory().for_the_log();
}
