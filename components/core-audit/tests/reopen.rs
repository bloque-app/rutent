//! The log's opening re-attributes what it reads back, and `audit-check` can read the evidence (`T-022-08`,
//! `T-022-02`'s finding 1).
//!
//! On opening, every stated entry the store marks `verified` is held to the statement kept under its commitment by
//! the append's own attribution: a kept statement that contradicts it refuses the opening, naming the entry, and
//! nothing is repaired or dropped; a statement erased since is no loss, and the log opens (`REQ-AUD-13`, `AC-36`).
//! The checker's tool, given the evidence store's directory, runs `unproven` over the history, and says so when it
//! wasn't given it.
//!
//! The runs use the real log on its file (`FileStore`) and the real evidence store in its directory
//! (`EvidenceStore`), reopened from the disk as a restart would; the providers are granted and advertised through the
//! log first, so their statements verify. Every entry is admitted by the log.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::{
    Ed25519Key, advertisement, advertising_key, another_key, public_key, sign_value, signing_key,
    statements_key,
};
use rutent_contracts::core_internal::calls::{
    self, AppendFailure, DecisionEntry, Evidence, GeneralEntry, KeptStatements, Log, PolicyLog,
};
use rutent_contracts::core_internal::{
    Actor, Attribution, Decision, Encoded, Entry, KeptStatement, PermissionChange, ReadAnswer,
    ReadCheck, Receipt, Violation,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{Deployment, PublicKey, TERMS_SHAPE, VERSION};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_audit::checkpoint::{Checkpointer, Schedule};
use rutent_core_audit::log::kept::base64url;
use rutent_core_audit::log::{
    AuditLog, Clock, FileStore, KEPT_STATEMENT_CONTRADICTS, NotOpened,
    STATEMENT_GONE_WITHOUT_ITS_ERASURE, Settings, record,
};
use rutent_core_audit::receipts::{EvidenceStore, Keeper, LogsRead};
use serde_json::{Value, json};

// --- The deployment ----------------------------------------------------------------------------------------------

const LOG: &str = "log-main";
const OPERATOR: &str = "operator";
const GRANTED_AT: &str = "2026-11-05T00:00:00Z";
const MADE_AT: &str = "2026-11-06T15:04:05Z";
const RECEIVED_AT: &str = "2026-11-06T15:04:06Z";
/// A second after the conformance crate's advertisements are made.
const ADVERTISED_RECEIVED_AT: &str = "2026-11-06T00:00:01Z";
/// Tick 0 of the checkpoint schedule here, and an hour between ticks.
const FIRST: &str = "2026-11-06T15:00:00Z";
const EVERY: u64 = 3600;

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

/// The log's own key, which signs its checkpoints.
fn log_key() -> Ed25519Key {
    signing_key("the log of rutent-test", 0)
}

fn the_logs_key() -> PublicKey {
    public_key(&log_key())
}

/// The log's clock: whatever a test last set.
#[derive(Debug, Clone)]
struct Stepped(Rc<Cell<i128>>);

impl Clock for Stepped {
    fn unix_nanos(&self) -> i128 {
        self.0.get()
    }
}

fn nanos(at: &str) -> i128 {
    Timestamp::new(at).unwrap().unix_nanos()
}

fn settings(now: &Rc<Cell<i128>>) -> Settings {
    Settings::new(id(LOG), deployment(), id(OPERATOR), id(OPERATOR))
        .on(Rc::new(Stepped(Rc::clone(now))))
}

fn actor(party: &str, acts: &str, role: &str, kind: &str) -> Actor {
    serde_json::from_value(json!({"party": party, "acts": acts, "role": role, "kind": kind}))
        .unwrap()
}

/// A provider, which states for itself.
fn provider(name: &str) -> Actor {
    actor(name, name, "provider", "component")
}

fn core() -> Actor {
    actor("rutent", "core-orch", "core", "component")
}

// --- Statements ----------------------------------------------------------------------------------------------------

/// A statement envelope naming `maker`, of the terms shape, made at `MADE_AT`, with `body`, signed by `key`.
fn statement(maker: &str, key: &Ed25519Key, body: Value) -> Vec<u8> {
    let mut envelope = json!({
        "deployment": deployment(),
        "protocol_version": VERSION,
        "shape": TERMS_SHAPE,
        "maker": maker,
        "made_at": MADE_AT,
        "signature": {"scheme": "ed25519", "key": "", "value": ""},
        "statement": body,
    });
    sign_value(&mut envelope, key);
    serde_json::to_vec(&envelope).unwrap()
}

/// `maker`'s quote under `transaction` for `price`, signed by its own statements key.
fn quote(maker: &str, transaction: &str, price: &str) -> Vec<u8> {
    statement(
        maker,
        &statements_key(maker),
        json!({"kind": "quote", "transaction": transaction, "price": price}),
    )
}

/// `provider`'s advertisement, stating for itself, its ledger its own private node.
fn advertised(provider: &str) -> Vec<u8> {
    let context = SuiteContext {
        deployment: deployment(),
        ..SuiteContext::default()
    };
    let own = String::from_utf8(advertisement(provider, &context))
        .unwrap()
        .replace("stand-in-ledger", &format!("{provider}-ledger"));
    let mut value: Value = serde_json::from_str(&own).unwrap();
    value["statement"]["publisher"] = provider.into();
    sign_value(&mut value, &advertising_key(provider));
    serde_json::to_vec(&value).unwrap()
}

/// The stated entry of the terms shape about `transaction`, made at `MADE_AT`, signed with `key`, kept under
/// `commitment`, attributed as `attribution` says.
fn stated_entry(
    attribution: Value,
    key: &PublicKey,
    commitment: &Digest,
    transaction: &str,
) -> Value {
    json!({
        "kind": "stated",
        "attribution": attribution,
        "what": {"statement": TERMS_SHAPE},
        "when": {"at": MADE_AT, "source": "party_stated"},
        "received_at": RECEIVED_AT,
        "outcome": {"status": "stated"},
        "signed_with": {"scheme": "ed25519", "key": key.as_str()},
        "commitment": commitment,
        "about": {"transaction": transaction},
    })
}

/// A stated entry marked `verified` to `who`, as someone with the operator's access to the log's file could write
/// it there: the log never appends one its writer attributed.
fn marked_verified(who: &Actor, key: &PublicKey, commitment: &Digest, transaction: &str) -> Entry {
    serde_json::from_value(stated_entry(
        json!({"signature": "verified", "who": who}),
        key,
        commitment,
        transaction,
    ))
    .unwrap()
}

// --- Policy's read check, which the keeper holds ---------------------------------------------------------------------

/// Policy's read check: nothing here reads, so it answers none.
struct NoReads;

impl calls::Reads for NoReads {
    fn check_read(&mut self, _: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn grant_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn revoke_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
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
            "rutent-core-audit-reopen-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn history(&self) -> PathBuf {
        self.0.join("history")
    }

    fn evidence(&self) -> PathBuf {
        self.0.join("evidence")
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

/// Appends `entry`'s record to the log's file, as someone with the operator's access to it can.
fn write_into(file: &Path, entry: &Entry) {
    let mut text = std::fs::read(file).unwrap();
    text.extend_from_slice(&record(entry).unwrap());
    text.push(b'\n');
    std::fs::write(file, text).unwrap();
}

// --- The run -------------------------------------------------------------------------------------------------------

type Shared = Rc<RefCell<AuditLog<FileStore, LogsRead>>>;

/// A log on its file and the evidence store in its directory, both in `dir`.
struct Run {
    dir: TempDir,
    log: Shared,
    evidence: Option<EvidenceStore>,
    keeper: RefCell<Keeper>,
    now: Rc<Cell<i128>>,
}

impl Run {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(name);
        let now = Rc::new(Cell::new(nanos(RECEIVED_AT)));
        let evidence = EvidenceStore::open(dir.evidence()).unwrap();
        let log = AuditLog::open(
            settings(&now),
            FileStore::open(dir.history()).unwrap(),
            evidence.for_the_log(),
        )
        .unwrap();
        Run {
            keeper: RefCell::new(evidence.keeper(Box::new(NoReads))),
            log: Rc::new(RefCell::new(log)),
            evidence: Some(evidence),
            dir,
            now,
        }
    }

    fn set_now(&self, at: &str) {
        self.now.set(nanos(at));
    }

    fn append_json(&self, entry: Value) -> Result<Receipt, AppendFailure> {
        let entry: Entry = serde_json::from_value(entry).unwrap();
        self.log
            .borrow_mut()
            .append(&GeneralEntry::try_from(entry).unwrap())
    }

    /// Policy's permitting decision on `action`, its effect and its `done` result, by the operator's login, on the
    /// log's clock at `at`: the receipt of the result.
    fn carry_out(&self, action: Value, at: &str) -> Receipt {
        self.set_now(at);
        let n = self.log.borrow().len();
        let (request, decision) = (format!("req-{n}"), format!("dec-{n}"));
        let operator = actor(OPERATOR, "op-login-2", "operator", "person");
        let decided = DecisionEntry::try_from(
            serde_json::from_value::<rutent_contracts::core_internal::VerifiedEntry>(json!({
                "kind": "verified",
                "who": operator,
                "what": {"call": "decide", "request": {"request": request, "caller": operator, "action": action, "at": at}, "decision": decision},
                "when": {"at": at, "source": "core_clock"},
                "outcome": {"status": "permitted"},
            }))
            .unwrap(),
        )
        .unwrap();
        let permit = self.log.borrow_mut().append_decision(&decided).unwrap();
        let effect = self
            .append_json(json!({
                "kind": "verified",
                "who": core(),
                "what": {"call": "effect", "action": action, "decision": {"request": request, "decision": decision, "answer": "permit", "receipt": permit}},
                "when": {"at": at, "source": "core_clock"},
                "outcome": {"status": "to_take_effect"},
            }))
            .unwrap();
        let result = self
            .append_json(json!({
                "kind": "verified",
                "who": core(),
                "what": {"call": "result", "effect": effect},
                "when": {"at": at, "source": "core_clock"},
                "outcome": {"status": "done"},
            }))
            .unwrap();
        self.set_now(RECEIVED_AT);
        result
    }

    /// Keeps `bytes` and appends their stated entry, unverified, claimed to be `claimed`'s, for the log to attribute.
    fn present(&self, bytes: &[u8], claimed: &Actor, received_at: &str) -> Receipt {
        let kept = self.keeper.borrow_mut().keep(bytes).unwrap();
        let envelope: Value = serde_json::from_slice(bytes).unwrap();
        let mut entry = json!({
            "kind": "stated",
            "attribution": {"signature": "unverified", "claimed_by": claimed},
            "what": {"statement": envelope["shape"]},
            "when": {"at": envelope["made_at"], "source": "party_stated"},
            "received_at": received_at,
            "outcome": {"status": "stated"},
            "signed_with": {"scheme": "ed25519", "key": envelope["signature"]["key"]},
            "commitment": kept.commitment,
        });
        if let Some(t) = envelope["statement"]["transaction"].as_str() {
            entry["about"] = json!({"transaction": t});
        }
        self.append_json(entry).unwrap()
    }

    /// Grants `name` a tuple and records its advertisement, so its statements keys sign.
    fn admit(&self, name: &str) {
        let action = json!({"kind": "change_catalogue", "change": "grant", "provider": name, "tuple": format!("tuple-{name}"), "advertising_keys": [public_key(&advertising_key(name)).as_str()]});
        self.carry_out(action, GRANTED_AT);
        self.set_now(ADVERTISED_RECEIVED_AT);
        self.present(&advertised(name), &provider(name), ADVERTISED_RECEIVED_AT);
        self.set_now(RECEIVED_AT);
    }

    /// `bytes` kept in the evidence store with no entry of the log naming them, as the operator's own keeper can.
    fn keep(&self, bytes: &[u8]) -> Digest {
        self.keeper.borrow_mut().keep(bytes).unwrap().commitment
    }

    fn entries(&self) -> Vec<(Receipt, Entry)> {
        self.log.borrow().entries().to_vec()
    }

    /// Whether the entry at `receipt` is a stated entry attributed `verified`.
    fn verified(&self, receipt: &Receipt) -> bool {
        matches!(
            &self.log.borrow().entries()[receipt.index.get() as usize].1,
            Entry::Stated(s) if matches!(s.attribution, Attribution::Verified { .. })
        )
    }

    /// Erases, outside the log, the statement kept under `commitment`, on the log's own decided erasure, and records
    /// the erasure: the bound evidence store's path (`REQ-AUD-13`).
    fn erase(&mut self, commitment: &Digest) {
        let action = json!({"kind": "erase_statement", "commitment": commitment, "reason": "retention_ended"});
        let result = self.carry_out(action, RECEIVED_AT);
        let bound = self
            .evidence
            .take()
            .unwrap()
            .bind(Rc::clone(&self.log))
            .unwrap();
        bound.erase(&result).unwrap();
        self.append_json(json!({
            "kind": "verified",
            "who": core(),
            "what": {"call": "erasure", "evidence": commitment},
            "when": {"at": RECEIVED_AT, "source": "core_clock"},
            "outcome": {"status": "done"},
        }))
        .unwrap();
        assert!(bound.is_erased(commitment));
    }

    /// Drops the log and the evidence store, freeing their files.
    fn close(self) -> TempDir {
        let Run {
            dir,
            log,
            evidence,
            keeper,
            ..
        } = self;
        drop(keeper);
        drop(evidence);
        drop(log);
        dir
    }
}

/// The log reopened from the files in `dir`, as a restart opens it, or why it didn't open.
fn reopen(dir: &TempDir) -> Result<AuditLog<FileStore, LogsRead>, NotOpened> {
    let now = Rc::new(Cell::new(nanos(RECEIVED_AT)));
    let evidence = EvidenceStore::open(dir.evidence()).unwrap();
    AuditLog::open_naming(
        settings(&now),
        FileStore::open(dir.history()).unwrap(),
        evidence.for_the_log(),
    )
}

/// The same, through the plain opening, which answers as every other call of the log does.
fn reopen_plain(dir: &TempDir) -> Result<(), AppendFailure> {
    let now = Rc::new(Cell::new(nanos(RECEIVED_AT)));
    let evidence = EvidenceStore::open(dir.evidence()).unwrap();
    AuditLog::open(
        settings(&now),
        FileStore::open(dir.history()).unwrap(),
        evidence.for_the_log(),
    )
    .map(|_| ())
}

/// A run with two providers admitted and one genuine quote of each, verified: the honest history the tests start
/// from. Answers the run and the receipt of us-bank's quote.
fn honest(name: &str) -> (Run, Receipt) {
    let run = Run::new(name);
    run.admit("us-bank");
    run.admit("relay-net");
    let bank = run.present(
        &quote("us-bank", "tx-3", "10.00"),
        &provider("us-bank"),
        RECEIVED_AT,
    );
    let relay = run.present(
        &quote("relay-net", "tx-4", "11.00"),
        &provider("relay-net"),
        RECEIVED_AT,
    );
    assert!(run.verified(&bank) && run.verified(&relay));
    (run, bank)
}

/// The receipt the entry at `index` of the log named `LOG` has, its record being `entry`'s.
fn receipt_at(index: usize, entry: &Entry) -> Receipt {
    let bytes = record(entry).unwrap();
    serde_json::from_value(json!({
        "log": LOG,
        "index": index,
        "entry": rutent_core_audit::log::digest(&rutent_core_audit::log::merkle::sha256(&bytes)),
    }))
    .unwrap()
}

/// Opening `dir` is refused, naming the entry at `index` whose record is `entry`'s; the plain opening refuses it by
/// the rule; and the log's file is left exactly as it was: nothing repaired or dropped.
fn refused_naming(dir: &TempDir, index: usize, entry: &Entry, case: &str) {
    let before = std::fs::read(dir.history()).unwrap();
    assert_eq!(
        reopen(dir).err(),
        Some(NotOpened::Contradicted {
            entry: receipt_at(index, entry)
        }),
        "{case}"
    );
    assert_eq!(
        reopen_plain(dir),
        Err(AppendFailure::Refused(Violation(
            KEPT_STATEMENT_CONTRADICTS
        ))),
        "{case}"
    );
    assert_eq!(
        std::fs::read(dir.history()).unwrap(),
        before,
        "{case}: nothing repaired or dropped"
    );
}

/// Writes `forged` at the end of a copy of `honest`'s file, checks the opening refuses it, naming it, and puts the
/// honest file back.
fn refused_with(dir: &TempDir, honest: &[u8], forged: &Entry, case: &str) {
    std::fs::write(dir.history(), honest).unwrap();
    write_into(&dir.history(), forged);
    let index = honest.iter().filter(|b| **b == b'\n').count();
    refused_naming(dir, index, forged, case);
    std::fs::write(dir.history(), honest).unwrap();
}

// --- The opening -----------------------------------------------------------------------------------------------------

/// A store holding a stated entry marked `verified` that the statement kept under its commitment doesn't bear out is
/// refused on opening, naming the entry: bytes signed by another key under the provider's key's name, bytes altered
/// after signing, the genuine bytes under an entry naming another key, another time, or another transaction, and an
/// honest entry whose kept bytes were swapped for others that don't hash to its commitment. The log's file is left as
/// it was.
// verifies: REQ-AUD-2
#[test]
fn store_holding_a_verified_entry_no_kept_statement_bears_out_is_refused_on_opening() {
    let (run, bank_quote) = honest("unborne");
    let quote_entry = run.entries()[bank_quote.index.get() as usize].1.clone();
    let Entry::Stated(stated) = &quote_entry else {
        panic!("a stated entry");
    };
    let quote_commitment = stated.commitment.clone();
    let bank = provider("us-bank");
    let key = public_key(&statements_key("us-bank"));
    let body = json!({"kind": "quote", "transaction": "tx-9", "price": "1.00"});
    let mut by_another: Value =
        serde_json::from_slice(&statement("us-bank", &another_key(), body.clone())).unwrap();
    by_another["signature"]["key"] = key.as_str().into();
    let by_another = run.keep(&serde_json::to_vec(&by_another).unwrap());
    let genuine = quote("us-bank", "tx-9", "1.00");
    let altered = run.keep(
        &String::from_utf8(genuine.clone())
            .unwrap()
            .replace("1.00", "0.01")
            .into_bytes(),
    );
    let genuine = run.keep(&genuine);
    let dir = run.close();

    let honest_file = std::fs::read(dir.history()).unwrap();
    // The honest history opens.
    assert!(reopen(&dir).is_ok());
    let cases = [
        (
            "signed by another key",
            marked_verified(&bank, &key, &by_another, "tx-9"),
        ),
        (
            "altered after signing",
            marked_verified(&bank, &key, &altered, "tx-9"),
        ),
        (
            "naming another key",
            marked_verified(&bank, &public_key(&another_key()), &genuine, "tx-9"),
        ),
        (
            "about another transaction",
            marked_verified(&bank, &key, &genuine, "tx-1"),
        ),
        ("made at another time", {
            let mut v = stated_entry(
                json!({"signature": "verified", "who": bank}),
                &key,
                &genuine,
                "tx-9",
            );
            v["when"]["at"] = "2026-11-06T15:04:04Z".into();
            serde_json::from_value(v).unwrap()
        }),
    ];
    for (case, forged) in &cases {
        refused_with(&dir, &honest_file, forged, case);
    }

    // The honest history over an evidence store whose bytes under the genuine quote's commitment were swapped for
    // others that don't hash to it: refused, naming the quote's entry.
    let quote_at = bank_quote.index.get() as usize;
    struct Swapped(LogsRead, Digest);
    impl KeptStatements for Swapped {
        fn kept(&mut self, c: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
            let mut kept = self.0.kept(c)?;
            if *c == self.1
                && let Some(k) = kept.as_mut()
            {
                k.bytes = Encoded::new(base64url(&quote("us-bank", "tx-3", "99.00"))).unwrap();
            }
            Ok(kept)
        }
    }
    let now = Rc::new(Cell::new(nanos(RECEIVED_AT)));
    let evidence = EvidenceStore::open(dir.evidence()).unwrap();
    let opened = AuditLog::open_naming(
        settings(&now),
        FileStore::open(dir.history()).unwrap(),
        Swapped(evidence.for_the_log(), quote_commitment.clone()),
    );
    assert_eq!(
        opened.err(),
        Some(NotOpened::Contradicted {
            entry: receipt_at(quote_at, &quote_entry)
        }),
        "bytes that don't bear out the commitment"
    );
    drop(evidence);

    // Back to the honest history: it opens as before.
    assert!(reopen(&dir).is_ok());
}

/// A store holding a stated entry marked `verified` whose kept statement names another maker is refused on opening:
/// a provider's genuine quote marked as another provider's, and a statement whose envelope names us-bank but is signed
/// by relay-net's own statements key, which `unproven` can't tell from a genuine one (the key verifies the
/// signature), but the log's register can: the key isn't us-bank's.
// verifies: REQ-AUD-2
#[test]
fn store_holding_a_verified_entry_whose_kept_statement_names_another_maker_is_refused_on_opening() {
    let (run, _) = honest("maker");
    let (bank, relay) = (provider("us-bank"), provider("relay-net"));
    let bank_key = public_key(&statements_key("us-bank"));
    let relay_key = public_key(&statements_key("relay-net"));
    let genuine = run.keep(&quote("us-bank", "tx-9", "1.00"));
    let in_banks_name = run.keep(&statement(
        "us-bank",
        &statements_key("relay-net"),
        json!({"kind": "quote", "transaction": "tx-9", "price": "1.00"}),
    ));
    let dir = run.close();
    let honest_file = std::fs::read(dir.history()).unwrap();

    let marked_as_relays = marked_verified(&relay, &bank_key, &genuine, "tx-9");
    refused_with(
        &dir,
        &honest_file,
        &marked_as_relays,
        "us-bank's quote marked relay-net's",
    );

    let signed_by_another_maker = marked_verified(&bank, &relay_key, &in_banks_name, "tx-9");
    // What a reader without the register finds: nothing, for this one.
    {
        std::fs::write(dir.history(), &honest_file).unwrap();
        let records = rutent_core_audit::checkpoint::read_lines(&honest_file);
        let mut history = rutent_core_audit::log::read_history(&id(LOG), &records);
        history.push((
            receipt_at(records.len(), &signed_by_another_maker),
            signed_by_another_maker.clone(),
        ));
        let evidence = EvidenceStore::open(dir.evidence()).unwrap();
        let mut kept = evidence.for_the_log();
        let found =
            rutent_core_audit::statements::unproven(&history, &mut kept, &deployment()).unwrap();
        assert!(
            found.found.is_empty() && found.unread.is_empty(),
            "{found:?}"
        );
    }
    refused_with(
        &dir,
        &honest_file,
        &signed_by_another_maker,
        "signed by another maker's key",
    );
}

/// A store whose `verified` entries are all borne out opens as before: the same entries, the same root, and the log
/// appends on, attributing the next statement as it did. An entry marked unverified is not held to anything on
/// opening, whatever its kept statement says.
// verifies: REQ-AUD-2
#[test]
fn store_whose_verified_entries_are_all_borne_out_opens_as_before() {
    let (run, _) = honest("borne");
    // An unverified entry whose bytes are signed by another key: not held on opening.
    let mut by_another: Value = serde_json::from_slice(&statement(
        "us-bank",
        &another_key(),
        json!({"kind": "quote", "transaction": "tx-5", "price": "2.00"}),
    ))
    .unwrap();
    by_another["signature"]["key"] = public_key(&statements_key("us-bank")).as_str().into();
    let unverified = run.present(
        &serde_json::to_vec(&by_another).unwrap(),
        &provider("us-bank"),
        RECEIVED_AT,
    );
    assert!(!run.verified(&unverified));
    let (entries, root) = (run.entries(), run.log.borrow().root());
    let dir = run.close();

    let reopened = reopen(&dir).expect("every verified entry is borne out");
    assert_eq!(reopened.entries(), &entries[..]);
    assert_eq!(reopened.root(), root);
    drop(reopened);
    assert_eq!(reopen_plain(&dir), Ok(()));

    // It appends on, attributing as before.
    let now = Rc::new(Cell::new(nanos(RECEIVED_AT)));
    let evidence = EvidenceStore::open(dir.evidence()).unwrap();
    let mut log = AuditLog::open(
        settings(&now),
        FileStore::open(dir.history()).unwrap(),
        evidence.for_the_log(),
    )
    .unwrap();
    let mut keeper = evidence.keeper(Box::new(NoReads));
    let bytes = quote("us-bank", "tx-6", "3.00");
    let kept = keeper.keep(&bytes).unwrap();
    let entry: Entry = serde_json::from_value(stated_entry(
        json!({"signature": "unverified", "claimed_by": provider("us-bank")}),
        &public_key(&statements_key("us-bank")),
        &kept.commitment,
        "tx-6",
    ))
    .unwrap();
    let receipt = log.append(&GeneralEntry::try_from(entry).unwrap()).unwrap();
    assert!(matches!(
        &log.entries()[receipt.index.get() as usize].1,
        Entry::Stated(s) if matches!(s.attribution, Attribution::Verified { .. })
    ));
}

/// A store whose `verified` entry's statement was erased since, on the log's own decided erasure, still opens: the
/// reference stays, history still checks, and the entry is as it was, though nothing is left to hold it to
/// (`REQ-AUD-13`, `AC-36`).
// verifies: REQ-AUD-13
#[test]
fn store_whose_verified_entrys_statement_was_erased_still_opens() {
    let (mut run, bank) = honest("erased");
    let Entry::Stated(stated) = run.entries()[bank.index.get() as usize].1.clone() else {
        panic!("a stated entry");
    };
    run.erase(&stated.commitment);
    let (entries, root) = (run.entries(), run.log.borrow().root());
    let dir = run.close();

    let reopened = reopen(&dir).expect("an erased statement is no loss: the log opens");
    assert_eq!(reopened.entries(), &entries[..]);
    assert_eq!(reopened.root(), root);
    assert!(matches!(
        &reopened.entries()[bank.index.get() as usize].1,
        Entry::Stated(s) if matches!(s.attribution, Attribution::Verified { .. })
    ));
    drop(reopened);
    let evidence = EvidenceStore::open(dir.evidence()).unwrap();
    assert_eq!(evidence.for_the_log().kept(&stated.commitment), Ok(None));
}

/// An evidence store that can't be read at opening is no erasure: the opening fails as unavailable, and the log
/// doesn't open as sound. An erasure is the store answering none; a store that can't answer says nothing.
// verifies: REQ-AUD-2
#[test]
fn evidence_store_that_cant_be_read_fails_the_opening() {
    let (run, _) = honest("unreadable");
    let dir = run.close();

    /// The evidence store's read, which fails once it has answered `answers` times.
    struct Failing(LogsRead, Cell<usize>);
    impl KeptStatements for Failing {
        fn kept(&mut self, c: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
            if self.1.get() == 0 {
                return Err(AppendFailure::Unavailable);
            }
            self.1.set(self.1.get() - 1);
            self.0.kept(c)
        }
    }
    let now = Rc::new(Cell::new(nanos(RECEIVED_AT)));
    // The two advertisements' bytes are read back first, for the register (which today's opening already does); the
    // read after those can't be made.
    let evidence = EvidenceStore::open(dir.evidence()).unwrap();
    let opened = AuditLog::open_naming(
        settings(&now),
        FileStore::open(dir.history()).unwrap(),
        Failing(evidence.for_the_log(), Cell::new(2)),
    );
    assert_eq!(
        opened.err(),
        Some(NotOpened::Failed(AppendFailure::Unavailable))
    );
}

// --- The checker's tool, with and without the evidence ----------------------------------------------------------------

/// The tool's run over `dir`'s files: the history, its checkpoints and openings, and with `evidence` the evidence
/// store's directory, with the deployment named.
fn audit_check(
    dir: &TempDir,
    history: &Path,
    evidence: bool,
    through: &str,
    from: &str,
) -> std::process::Output {
    let (checkpoints, openings) = (dir.0.join("checkpoints-held"), dir.0.join("openings"));
    if !checkpoints.exists() {
        std::fs::write(&checkpoints, b"").unwrap();
        std::fs::write(&openings, b"").unwrap();
    }
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_audit-check"));
    command
        .env_clear()
        .current_dir(&dir.0)
        .arg("--history")
        .arg(history)
        .arg("--checkpoints")
        .arg(&checkpoints)
        .arg("--openings")
        .arg(&openings)
        .args(["--key", the_logs_key().as_str(), "--log", LOG])
        .args(["--first", FIRST, "--every", &EVERY.to_string()])
        .args(["--from", from, "--through", through]);
    if evidence {
        command
            .arg("--evidence")
            .arg(dir.evidence())
            .args(["--deployment", &deployment().name])
            .args(["--operator-key", deployment().operator_key.as_str()]);
    }
    command.output().unwrap()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// With the evidence store's directory, the tool holds each `verified` entry of the history to its kept statement and
/// prints each one no kept statement bears out as a finding, exit 1; the honest history beside the same evidence is
/// clean, exit 0, and says the evidence was read.
// verifies: REQ-AUD-9, REQ-AUD-10
#[test]
fn audit_check_with_the_evidence_reports_a_verified_entry_no_kept_statement_bears_out() {
    let (run, _) = honest("tool-evidence");
    let bank = provider("us-bank");
    let key = public_key(&statements_key("us-bank"));
    let genuine = run.keep(&quote("us-bank", "tx-9", "1.00"));
    let dir = run.close();

    let clean = audit_check(&dir, &dir.history(), true, "0", "1");
    assert_eq!(clean.status.code(), Some(0), "{clean:?}");
    let out = stdout(&clean);
    assert!(out.contains("borne out"), "{out}");
    assert!(
        !out.contains("not read") && !out.contains("NotBorneOut"),
        "{out}"
    );

    let index = std::fs::read(dir.history())
        .unwrap()
        .iter()
        .filter(|b| **b == b'\n')
        .count();
    write_into(
        &dir.history(),
        &marked_verified(&bank, &key, &genuine, "tx-1"),
    );
    let found = audit_check(&dir, &dir.history(), true, "0", "1");
    assert_eq!(found.status.code(), Some(1), "{found:?}");
    let out = stdout(&found);
    assert!(
        out.contains(&format!("NotBorneOut {{ index: {index} }}")),
        "{out}"
    );
    assert_eq!(out.matches("NotBorneOut").count(), 1, "{out}");

    // Evidence it can't read is neither clean nor a finding.
    std::fs::rename(dir.evidence(), dir.0.join("moved")).unwrap();
    let unreadable = audit_check(&dir, &dir.history(), true, "0", "1");
    assert_eq!(unreadable.status.code(), Some(2), "{unreadable:?}");
}

/// Without the evidence the tool finds what it did before, a rewrite against a checkpoint and nothing in a history
/// whose checkpoints hold, and its output says the evidence was not read, so a clean run isn't taken for more than it
/// checked; a `verified` entry no kept statement bears out isn't found, since nothing was read to find it.
// verifies: REQ-AUD-10
#[test]
fn audit_check_without_the_evidence_says_it_did_not_read_it_and_finds_what_it_did_before() {
    let (run, _) = honest("tool-none");
    let bank = provider("us-bank");
    let key = public_key(&statements_key("us-bank"));
    let genuine = run.keep(&quote("us-bank", "tx-9", "1.00"));
    // A checkpoint of tick 0 over the history so far.
    let schedule = Schedule::new(&Timestamp::new(FIRST).unwrap(), EVERY).unwrap();
    let mut checkpointer = Checkpointer::new(
        Rc::clone(&run.log),
        FileStore::open(run.dir.checkpoints()).unwrap(),
        log_key(),
        schedule,
    )
    .unwrap();
    let ran = checkpointer.run_due();
    assert_eq!((ran.published.len(), ran.stopped.clone()), (1, None));
    checkpointer.acknowledge(&ran).unwrap();
    let openings: Vec<Vec<u8>> = checkpointer
        .kept()
        .iter()
        .map(|k| serde_json::to_vec(&k.opening).unwrap())
        .collect();
    drop(checkpointer);
    let dir = run.close();
    let lines = |items: &[Vec<u8>]| {
        let mut out = items.join(&b'\n');
        out.push(b'\n');
        out
    };
    std::fs::write(dir.0.join("checkpoints-held"), lines(&ran.published)).unwrap();
    std::fs::write(dir.0.join("openings"), lines(&openings)).unwrap();

    let honest_run = audit_check(&dir, &dir.history(), false, "0", "0");
    assert_eq!(honest_run.status.code(), Some(0), "{honest_run:?}");
    let out = stdout(&honest_run);
    assert!(out.contains("holds"), "{out}");
    assert!(out.contains("the evidence was not read"), "{out}");

    // A forged verified entry after the checkpoint: nothing the tool reads without the evidence finds it.
    write_into(
        &dir.history(),
        &marked_verified(&bank, &key, &genuine, "tx-1"),
    );
    let unread = audit_check(&dir, &dir.history(), false, "0", "0");
    assert_eq!(unread.status.code(), Some(0), "{unread:?}");
    assert!(stdout(&unread).contains("the evidence was not read"));
    assert!(!stdout(&unread).contains("NotBorneOut"));

    // A rewrite before the checkpoint: found, as before, and the evidence still said not read.
    let file = std::fs::read(dir.history()).unwrap();
    let mut records = rutent_core_audit::checkpoint::read_lines(&file);
    let mut value: Value = serde_json::from_slice(&records[0]).unwrap();
    value["when"]["at"] = "2026-11-05T00:00:09Z".into();
    records[0] = record(&serde_json::from_value::<Entry>(value).unwrap()).unwrap();
    let rewritten = dir.0.join("rewritten");
    std::fs::write(&rewritten, lines(&records)).unwrap();
    let found = audit_check(&dir, &rewritten, false, "0", "0");
    assert_eq!(found.status.code(), Some(1), "{found:?}");
    let out = stdout(&found);
    assert!(out.contains("DoesNotHold"), "{out}");
    assert!(out.contains("the evidence was not read"), "{out}");
}

/// With the evidence, an entry whose statement was erased on the log's decision is reported apart, as unread, and not
/// as a finding: the run is clean, exit 0, and names the entry it could hold to nothing (`REQ-AUD-13`).
// verifies: REQ-AUD-13
#[test]
fn audit_check_with_the_evidence_reports_an_erased_statement_as_unread_and_not_as_a_finding() {
    let (mut run, bank) = honest("tool-erased");
    let Entry::Stated(stated) = run.entries()[bank.index.get() as usize].1.clone() else {
        panic!("a stated entry");
    };
    run.erase(&stated.commitment);
    let dir = run.close();

    let output = audit_check(&dir, &dir.history(), true, "0", "1");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let out = stdout(&output);
    assert!(
        !out.contains("NotBorneOut") && !out.contains("StatementLost") && !out.contains("Mark"),
        "{out}"
    );
    let erased: Vec<&str> = out.lines().filter(|l| l.starts_with("erased")).collect();
    assert_eq!(erased.len(), 1, "{out}");
    assert!(
        erased[0].contains(&format!("entry {}", bank.index.get())),
        "{out}"
    );

    // The same decided erasure with the store's mark gone: no erasure the store carried out, so the statement is
    // lost, a finding.
    std::fs::remove_file(
        dir.evidence()
            .join("erased")
            .join(stated.commitment.value.as_str()),
    )
    .unwrap();
    let output = audit_check(&dir, &dir.history(), true, "0", "1");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let out = stdout(&output);
    assert!(
        out.contains(&format!("StatementLost {{ index: {} }}", bank.index.get())),
        "{out}"
    );
    assert!(!out.lines().any(|l| l.starts_with("erased")), "{out}");
}

/// Where the statement kept behind an entry is gone with no decided erasure of it, `kept` is the file the evidence
/// store files it in.
fn kept_file(dir: &TempDir, commitment: &Digest) -> PathBuf {
    dir.evidence()
        .join("kept")
        .join(format!("{}.json", commitment.value.as_str()))
}

fn mark_file(dir: &TempDir, commitment: &Digest) -> PathBuf {
    dir.evidence()
        .join("erased")
        .join(commitment.value.as_str())
}

/// The stated entry at `receipt` in `run`'s log.
fn commitment_at(run: &Run, receipt: &Receipt) -> Digest {
    match &run.entries()[receipt.index.get() as usize].1 {
        Entry::Stated(s) => s.commitment.clone(),
        Entry::Verified(_) => panic!("a stated entry"),
    }
}

/// Tampering that passes for an erasure doesn't open as sound. A `verified` entry whose statement is gone with no
/// decided erasure of it in the history is refused on opening, naming it: an honest entry whose statement was removed
/// by hand, a forged entry whose statement was removed after it was written, and a forged entry beside a mark of
/// erasure made by hand with its contradicting statement still filed (the evidence store removes bytes behind a mark
/// when it opens, so the log then finds nothing kept, and no decided erasure). An erasure the log decided still opens
/// (`store whose verified entry's statement was erased still opens`).
// verifies: REQ-AUD-2, REQ-AUD-13
#[test]
fn store_whose_verified_entrys_statement_is_gone_with_no_erasure_is_refused_on_opening() {
    let (run, bank) = honest("gone");
    let (who, key) = (provider("us-bank"), public_key(&statements_key("us-bank")));
    let bank_commitment = commitment_at(&run, &bank);
    let genuine = run.keep(&quote("us-bank", "tx-9", "1.00"));
    let dir = run.close();
    let honest_file = std::fs::read(dir.history()).unwrap();
    let index = honest_file.iter().filter(|b| **b == b'\n').count();
    let gone = |entry: Receipt| Some(NotOpened::Gone { entry });

    // An honest entry's statement removed by hand.
    let bytes = std::fs::read(kept_file(&dir, &bank_commitment)).unwrap();
    std::fs::remove_file(kept_file(&dir, &bank_commitment)).unwrap();
    assert_eq!(
        reopen(&dir).err(),
        gone(bank.clone()),
        "an honest entry's statement removed"
    );
    assert_eq!(
        reopen_plain(&dir),
        Err(AppendFailure::Refused(Violation(
            STATEMENT_GONE_WITHOUT_ITS_ERASURE
        )))
    );
    assert_eq!(
        std::fs::read(dir.history()).unwrap(),
        honest_file,
        "nothing repaired or dropped"
    );
    std::fs::write(kept_file(&dir, &bank_commitment), &bytes).unwrap();
    assert!(reopen(&dir).is_ok());

    // Probe 1: a forged entry, its statement removed after it was written.
    let forged = marked_verified(&who, &key, &genuine, "tx-9");
    write_into(&dir.history(), &forged);
    assert!(
        reopen(&dir).is_ok(),
        "the genuine bytes bear the forged entry out while they are kept"
    );
    let bytes = std::fs::read(kept_file(&dir, &genuine)).unwrap();
    std::fs::remove_file(kept_file(&dir, &genuine)).unwrap();
    assert_eq!(
        reopen(&dir).err(),
        gone(receipt_at(index, &forged)),
        "probe 1"
    );
    std::fs::write(kept_file(&dir, &genuine), &bytes).unwrap();
    std::fs::write(dir.history(), &honest_file).unwrap();

    // Probe 2: a forged entry the filed statement contradicts, and a mark of erasure made by hand.
    let contradicted = marked_verified(&who, &key, &genuine, "tx-1");
    write_into(&dir.history(), &contradicted);
    std::fs::write(mark_file(&dir, &genuine), b"").unwrap();
    assert_eq!(
        reopen(&dir).err(),
        gone(receipt_at(index, &contradicted)),
        "probe 2"
    );
}

/// With the evidence, tampering that passes for an erasure is a finding, never an erasure: a forged entry whose
/// statement was removed (`StatementLost`), a forged entry beside a hand-made mark with its contradicting statement
/// still filed (`NotBorneOut`, `MarkWithoutItsErasure`, `MarkedYetFiled`), and an honest entry's statement removed
/// behind a hand-made mark (`MarkWithoutItsErasure`). None is printed as erased, and each run exits 1.
// verifies: REQ-AUD-9, REQ-AUD-13
#[test]
fn audit_check_with_the_evidence_reports_a_statement_gone_with_no_erasure_as_a_finding() {
    let (run, bank) = honest("tool-gone");
    let (who, key) = (provider("us-bank"), public_key(&statements_key("us-bank")));
    let bank_commitment = commitment_at(&run, &bank);
    let genuine = run.keep(&quote("us-bank", "tx-9", "1.00"));
    let dir = run.close();
    let honest_file = std::fs::read(dir.history()).unwrap();
    let index = honest_file.iter().filter(|b| **b == b'\n').count();
    let no_erasure = |out: &str| assert!(!out.lines().any(|l| l.starts_with("erased")), "{out}");

    // Probe 1.
    write_into(
        &dir.history(),
        &marked_verified(&who, &key, &genuine, "tx-9"),
    );
    let bytes = std::fs::read(kept_file(&dir, &genuine)).unwrap();
    std::fs::remove_file(kept_file(&dir, &genuine)).unwrap();
    let found = audit_check(&dir, &dir.history(), true, "0", "1");
    assert_eq!(found.status.code(), Some(1), "{found:?}");
    let out = stdout(&found);
    assert!(
        out.contains(&format!("StatementLost {{ index: {index} }}")),
        "{out}"
    );
    assert_eq!(out.matches("StatementLost").count(), 1, "{out}");
    no_erasure(&out);
    std::fs::write(kept_file(&dir, &genuine), &bytes).unwrap();
    std::fs::write(dir.history(), &honest_file).unwrap();

    // Probe 2.
    write_into(
        &dir.history(),
        &marked_verified(&who, &key, &genuine, "tx-1"),
    );
    std::fs::write(mark_file(&dir, &genuine), b"").unwrap();
    let found = audit_check(&dir, &dir.history(), true, "0", "1");
    assert_eq!(found.status.code(), Some(1), "{found:?}");
    let out = stdout(&found);
    let value = genuine.value.as_str();
    assert!(
        out.contains(&format!("NotBorneOut {{ index: {index} }}")),
        "{out}"
    );
    assert!(
        out.contains(&format!("MarkWithoutItsErasure {{ commitment: {value} }}")),
        "{out}"
    );
    assert!(
        out.contains(&format!("MarkedYetFiled {{ commitment: {value} }}")),
        "{out}"
    );
    no_erasure(&out);
    std::fs::remove_file(mark_file(&dir, &genuine)).unwrap();
    std::fs::write(dir.history(), &honest_file).unwrap();

    // An honest entry's statement removed behind a hand-made mark.
    std::fs::remove_file(kept_file(&dir, &bank_commitment)).unwrap();
    std::fs::write(mark_file(&dir, &bank_commitment), b"").unwrap();
    let found = audit_check(&dir, &dir.history(), true, "0", "1");
    assert_eq!(found.status.code(), Some(1), "{found:?}");
    let out = stdout(&found);
    assert!(
        out.contains(&format!(
            "MarkWithoutItsErasure {{ commitment: {} }}",
            bank_commitment.value.as_str()
        )),
        "{out}"
    );
    assert!(
        !out.contains("MarkedYetFiled") && !out.contains("StatementLost"),
        "{out}"
    );
    no_erasure(&out);
}

/// A store whose verified statements were attributed under a grant later revoked by a done catalogue change still
/// opens: each is held to the register as it stood before it, when the grant was in force.
// verifies: REQ-AUD-2
#[test]
fn store_still_opens_after_a_trusted_providers_grant_is_revoked_later() {
    let (run, bank) = honest("revoked");
    run.carry_out(
        json!({"kind": "change_catalogue", "change": "revoke_grant", "provider": "us-bank", "tuple": "tuple-us-bank"}),
        "2026-11-07T00:00:00Z",
    );
    let (entries, root) = (run.entries(), run.log.borrow().root());
    let dir = run.close();
    let reopened = reopen(&dir).expect("the grant was in force when the quote was attributed");
    assert_eq!(reopened.entries(), &entries[..]);
    assert_eq!(reopened.root(), root);
    assert!(matches!(
        &reopened.entries()[bank.index.get() as usize].1,
        Entry::Stated(s) if matches!(s.attribution, Attribution::Verified { .. })
    ));
}

/// A provider whose grants are all revoked has no accepted advertisement (`T-020-49`, the milestone reviewer's check of
/// `T-020-55`): the log's key register answers none for it from the revocation's effect, so nothing Core reads of the
/// advertisement (policy's `Declarations`, closing, the snapshot) trusts what it advertised. A provider still granted
/// keeps its own, and a reopened log answers the same.
// verifies: REQ-AUZ-9, REQ-MDL-20
#[test]
fn revoked_providers_advertisement_declares_nothing() {
    let (run, _) = honest("revoked-advertisement");
    assert!(run.log.borrow().accepted_advertisement("us-bank").is_some());
    run.carry_out(
        json!({"kind": "change_catalogue", "change": "revoke_grant", "provider": "us-bank", "tuple": "tuple-us-bank"}),
        "2026-11-07T00:00:00Z",
    );
    assert_eq!(
        run.log.borrow().accepted_advertisement("us-bank"),
        None,
        "every grant revoked: nothing it advertised is accepted"
    );
    assert!(
        run.log
            .borrow()
            .accepted_advertisement("relay-net")
            .is_some(),
        "another provider's stands"
    );
    let dir = run.close();
    let reopened = reopen(&dir).expect("reopened");
    assert_eq!(reopened.accepted_advertisement("us-bank"), None);
    assert!(reopened.accepted_advertisement("relay-net").is_some());
}
