//! The log (`T-022-01`): every record Core writes, appended through `core-internal`'s two handles onto a Merkle log in
//! `CMP-CORE-AUDIT`'s own store, attributed by the log, held to the citation table, and kept through failures.
//!
//! The runs replay the contract's coherent world (`fixtures/admission/world--*`), whose receipts are placeholders:
//! each entry is appended in turn, and every later citation of its placeholder receipt is rewritten to the receipt
//! the log answered. Its statements arrive as Core hands them over, unverified and naming their claimed maker, with
//! bytes signed for real and kept by the evidence store's stand-in, and the log attributes each. Its providers are
//! granted first, by a decision of policy and its effect, and their advertisements recorded.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::{
    ADVERTISED_AT, Ed25519Key, advertisement, advertising_key, another_key, public_key, sign_value,
    signing_key, statements_key,
};
use rutent_contracts::core_internal::calls::{
    AppendFailure, DecisionEntry, GeneralEntry, KeptStatements, Log, PolicyLog, is_decision,
};
use rutent_contracts::core_internal::{
    Attribution, Entry, HandedOver, Receipt, StatedEntry, VerifiedEntry, Violation, What,
};
use rutent_contracts::provider_role::{ADVERTISEMENT_SHAPE, Deployment, VERSION};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_audit::log::merkle::{leaf_hash, sha256, verify_inclusion};
use rutent_core_audit::log::{
    ADVERTISEMENT_NOT_KEPT, AuditLog, Clock, FileStore, General, KEPT_BYTES_DIFFER, KeptInMemory,
    MemoryStore, Policy, Settings, Store, StoreError, digest,
    keys::{
        ADVERTISEMENT_OFF_THE_LOGS_CLOCK, CHANGE_BEFORE_ITS_TUPLES_LAST, CHANGE_OFF_THE_LOGS_CLOCK,
    },
    record,
};
use serde_json::{Value, json};

// --- Fixtures ----------------------------------------------------------------------------------------------------

fn fixture(dir: &str, name: &str) -> Value {
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/{dir}/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

fn valid(name: &str) -> Value {
    fixture("valid", name)
}

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

/// The RFC 3339 UTC time `nanos` after the epoch, to the second, for times in November 2026.
fn rfc3339(nanos: i128) -> String {
    let month = Timestamp::new("2026-11-01T00:00:00Z").unwrap().unix_nanos();
    let seconds = (nanos - month) / 1_000_000_000;
    assert!(
        (0..30 * 86_400).contains(&seconds),
        "a time in November 2026"
    );
    format!(
        "2026-11-{:02}T{:02}:{:02}:{:02}Z",
        seconds / 86_400 + 1,
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// The coherent world: every entry of `world--in_dest`'s history, then its own entry.
fn world() -> Vec<(Value, Value)> {
    let case = fixture("admission", "world--in_dest");
    let mut entries: Vec<(Value, Value)> = case["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| (h["receipt"].clone(), h["entry"].clone()))
        .collect();
    entries.push((Value::Null, case["entry"].clone()));
    entries
}

/// The world's place of its `nth` entry (from 0) whose call is `call`, so a test names an entry by what it is
/// rather than by a place the contract's fixture may move.
fn place_of(call: &str, nth: usize) -> usize {
    world()
        .iter()
        .enumerate()
        .filter(|(_, (_, e))| e["what"]["call"] == call)
        .nth(nth)
        .map(|(place, _)| place)
        .unwrap_or_else(|| panic!("the world has no {call} {nth}"))
}

/// The world's place of the reserve's yes the acceptance rests on: its first stated result.
fn first_yes() -> usize {
    world()
        .iter()
        .position(|(_, e)| e["what"]["statement"] == "urn:rutent:provider-role:result")
        .unwrap()
}

// --- The deployment, its operator and its providers --------------------------------------------------------------

const LOG: &str = "log-main";
const OPERATOR: &str = "operator";
/// The world's providers, each of which states for itself.
const PROVIDERS: [&str; 6] = [
    "us-bank",
    "eu-bank",
    "bridge-pay",
    "fx-desk",
    "bank-co",
    "kyc-check",
];
/// When each provider is granted, before its advertisement.
const GRANTED_AT: &str = "2026-11-05T00:00:00Z";

fn operator_key() -> Ed25519Key {
    signing_key("the operator of rutent-test", 0)
}

fn deployment() -> Deployment {
    Deployment {
        name: "rutent-test".to_owned(),
        operator_key: public_key(&operator_key()),
    }
}

/// The deployment's settings: its operator, on the test's clock. First trust in a provider is its grant's.
fn settings() -> Settings {
    trusted().on(Rc::new(TestClock))
}

thread_local! {
    /// Now, by the log's clock in this test's thread: each test runs on a thread of its own.
    static NOW: std::cell::Cell<i128> =
        std::cell::Cell::new(Timestamp::new("2026-11-10T09:00:00Z").unwrap().unix_nanos());
}

/// Sets the log's clock to `at`.
fn set_now(at: &str) {
    NOW.with(|now| now.set(Timestamp::new(at).unwrap().unix_nanos()));
}

/// The log's clock in a test: what [`set_now`] last set in the test's thread.
#[derive(Debug)]
struct TestClock;

impl Clock for TestClock {
    fn unix_nanos(&self) -> i128 {
        NOW.with(|now| now.get())
    }
}

fn trusted() -> Settings {
    Settings::new(id(LOG), deployment(), id(OPERATOR), id(OPERATOR))
}

fn actor(party: &str, acts: &str, role: &str, kind: &str) -> Value {
    json!({"party": party, "acts": acts, "role": role, "kind": kind})
}

fn provider(name: &str) -> Value {
    actor(name, name, "provider", "component")
}

// --- Stores that fail ----------------------------------------------------------------------------------------------

/// What one append of a [`Faulty`] store does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// Fails before writing anything.
    Before,
    /// Writes the record durably, then fails, as a crash between the write and its answer does.
    After,
}

/// A store in memory whose appends fail as planned, one fault per append, in order; an append with no fault left
/// planned succeeds. Its clones share the store and the plan.
#[derive(Clone, Default)]
struct Faulty {
    inner: Rc<RefCell<MemoryStore>>,
    plan: Rc<RefCell<VecDeque<Option<Fault>>>>,
    /// While set, reading the store back fails as a store whose records can't be read whole does.
    unreadable: Rc<std::cell::Cell<bool>>,
}

impl Faulty {
    fn plan(&self, faults: &[Option<Fault>]) {
        self.plan.borrow_mut().extend(faults.iter().copied());
    }

    fn records(&self) -> Vec<Vec<u8>> {
        self.inner.borrow_mut().records().unwrap()
    }
}

impl Store for Faulty {
    fn append(&mut self, record: &[u8]) -> Result<(), StoreError> {
        match self.plan.borrow_mut().pop_front().flatten() {
            Some(Fault::Before) => Err(StoreError::Unavailable("injected before".into())),
            Some(Fault::After) => {
                self.inner.borrow_mut().append(record)?;
                Err(StoreError::Unavailable("injected after".into()))
            }
            None => self.inner.borrow_mut().append(record),
        }
    }

    fn records(&mut self) -> Result<Vec<Vec<u8>>, StoreError> {
        if self.unreadable.get() {
            return Err(StoreError::Corrupt(0));
        }
        self.inner.borrow_mut().records()
    }
}

/// A file in the platform's temp directory, removed when dropped.
struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        TempFile(std::env::temp_dir().join(format!(
            "rutent-core-audit-{name}-{}-{nanos}.log",
            std::process::id()
        )))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// --- A run --------------------------------------------------------------------------------------------------------

/// A log and the evidence store's stand-in beside it, with the placeholder receipts and commitments of the world
/// rewritten to the ones the log answered and the store kept.
struct Run<S: Store> {
    log: AuditLog<S, KeptInMemory>,
    kept: KeptInMemory,
    receipts: Vec<(Value, Value)>,
    salts: u8,
}

impl<S: Store> Run<S> {
    fn new(store: S) -> Self {
        Self::with(store, KeptInMemory::new())
    }

    /// A run on the log kept in `store`, with `kept` the evidence store's stand-in it ran with.
    fn with(store: S, kept: KeptInMemory) -> Self {
        Run {
            log: AuditLog::open(settings(), store, kept.clone()).unwrap(),
            kept,
            receipts: Vec::new(),
            salts: 0,
        }
    }

    /// `value` with every placeholder receipt the run has answered rewritten to the log's.
    fn rewrite(&self, value: &mut Value) {
        if let Some((_, real)) = self.receipts.iter().find(|(fake, _)| fake == value) {
            *value = real.clone();
            return;
        }
        match value {
            Value::Array(items) => items.iter_mut().for_each(|v| self.rewrite(v)),
            Value::Object(map) => map.values_mut().for_each(|v| self.rewrite(v)),
            _ => {}
        }
    }

    /// Keeps `bytes` in the evidence store's stand-in, under a fresh salt.
    fn keep(&mut self, bytes: &[u8]) -> Value {
        self.salts += 1;
        serde_json::to_value(self.kept.keep(bytes, [self.salts; 32])).unwrap()
    }

    /// Appends `entry`, a verified entry or a stated one already a claim, through the handle its kind goes by.
    fn append(&mut self, mut entry: Value) -> Result<Receipt, AppendFailure> {
        self.rewrite(&mut entry);
        let entry: Entry = serde_json::from_value(entry).unwrap();
        match entry {
            Entry::Verified(v) if is_decision(&v.what) => self
                .log
                .append_decision(&DecisionEntry::try_from(v).unwrap()),
            entry => self.log.append(&GeneralEntry::try_from(entry).unwrap()),
        }
    }

    /// Appends `entry` and records that `placeholder` stands for the receipt the log answered.
    fn append_for(&mut self, placeholder: &Value, entry: Value) -> Result<Receipt, AppendFailure> {
        let receipt = self.append(entry)?;
        if !placeholder.is_null() {
            self.receipts
                .push((placeholder.clone(), serde_json::to_value(&receipt).unwrap()));
        }
        Ok(receipt)
    }

    /// The entry the log holds at `receipt`.
    fn at(&self, receipt: &Receipt) -> &Entry {
        let (r, e) = &self.log.entries()[receipt.index.get() as usize];
        assert_eq!(r, receipt);
        e
    }

    /// Grants `name` a tuple, naming its advertising key: policy's permitting decision, through policy's handle, its
    /// effect and its `done` result, on the log's clock at `at`.
    fn grant(&mut self, name: &str, at: &str) -> Receipt {
        self.grant_keys(name, &[advertising_key(name)], at)
    }

    /// Grants `name` a tuple naming `keys` its advertising keys.
    fn grant_keys(&mut self, name: &str, keys: &[Ed25519Key], at: &str) -> Receipt {
        let keys: Vec<String> = keys
            .iter()
            .map(|k| public_key(k).as_str().to_owned())
            .collect();
        self.carry_out(json!({"kind": "change_catalogue", "change": "grant", "provider": name, "tuple": format!("tuple-{name}"), "advertising_keys": keys}), at)
    }

    /// Revokes the tuple [`Run::grant`] granted `name`.
    fn revoke(&mut self, name: &str, at: &str) -> Receipt {
        self.carry_out(json!({"kind": "change_catalogue", "change": "revoke_grant", "provider": name, "tuple": format!("tuple-{name}")}), at)
    }

    /// Carries `action` out at `at`, on the log's clock then: its decision, its effect and its `done` result.
    fn carry_out(&mut self, action: Value, at: &str) -> Receipt {
        set_now(at);
        let effect = self.try_change(action, at).unwrap();
        self.end(&effect, "done", at);
        effect
    }

    /// Appends the result of `effect`, with its outcome `status`.
    fn end(&mut self, effect: &Receipt, status: &str, at: &str) -> Receipt {
        self.append(json!({
            "kind": "verified",
            "who": actor("rutent", "core-orch", "core", "component"),
            "what": {"call": "result", "effect": effect},
            "when": {"at": at, "source": "core_clock"},
            "outcome": {"status": status},
        }))
        .unwrap()
    }

    /// Changes the catalogue for `name`'s tuple, its grant naming `name`'s advertising key: policy's permitting
    /// decision, and its effect dated `at`, whose append is answered.
    fn try_change_catalogue(
        &mut self,
        name: &str,
        change: &str,
        at: &str,
    ) -> Result<Receipt, AppendFailure> {
        let mut action = json!({"kind": "change_catalogue", "change": change, "provider": name, "tuple": format!("tuple-{name}")});
        if change == "grant" {
            action["advertising_keys"] = json!([public_key(&advertising_key(name)).as_str()]);
        }
        self.try_change(action, at)
    }

    /// Policy's permitting decision on `action`, and its effect dated `at`, whose append is answered.
    fn try_change(&mut self, action: Value, at: &str) -> Result<Receipt, AppendFailure> {
        let change = action["change"].as_str().unwrap().to_owned();
        let name = action["provider"].as_str().unwrap().to_owned();
        let (request, decision) = (
            format!("req-{change}-{name}-{at}-{}", self.log.len()),
            format!("dec-{change}-{name}-{at}-{}", self.log.len()),
        );
        let operator = actor(OPERATOR, "op-login-2", "operator", "person");
        let decided = self
            .append(json!({
                "kind": "verified",
                "who": operator,
                "what": {"call": "decide", "request": {"request": request, "caller": operator, "action": action, "at": at}, "decision": decision},
                "when": {"at": at, "source": "core_clock"},
                "outcome": {"status": "permitted"},
            }))
            .unwrap();
        self.append(json!({
            "kind": "verified",
            "who": actor("rutent", "core-orch", "core", "component"),
            "what": {"call": "effect", "action": action, "decision": {"request": request, "decision": decision, "answer": "permit", "receipt": decided}},
            "when": {"at": at, "source": "core_clock"},
            "outcome": {"status": "to_take_effect"},
        }))
    }

    /// Appends `bytes`, an advertisement as received a second after it was made, as a stated entry of its maker's,
    /// after keeping it.
    fn advertise(&mut self, maker: &str, bytes: &[u8]) -> Result<Receipt, AppendFailure> {
        let envelope: Value = serde_json::from_slice(bytes).unwrap();
        let made_at = envelope["made_at"].as_str().unwrap();
        let received_at = Timestamp::new(made_at).unwrap().unix_nanos() + 1_000_000_000;
        self.advertise_at(maker, bytes, &rfc3339(received_at))
    }

    /// Appends `bytes`, an advertisement received at `received_at`, as a stated entry of its maker's, after keeping
    /// it.
    fn advertise_at(
        &mut self,
        maker: &str,
        bytes: &[u8],
        received_at: &str,
    ) -> Result<Receipt, AppendFailure> {
        set_now(received_at);
        self.advertise_raw(maker, bytes, received_at)
    }

    /// Appends `bytes`, an advertisement received at `received_at`, whatever the log's clock says.
    fn advertise_raw(
        &mut self,
        maker: &str,
        bytes: &[u8],
        received_at: &str,
    ) -> Result<Receipt, AppendFailure> {
        let envelope: Value = serde_json::from_slice(bytes).unwrap();
        let commitment = self.keep(bytes);
        self.append(json!({
            "kind": "stated",
            "attribution": {"signature": "unverified", "claimed_by": provider(maker)},
            "what": {"statement": ADVERTISEMENT_SHAPE},
            "when": {"at": envelope["made_at"], "source": "party_stated"},
            "received_at": received_at,
            "outcome": {"status": "stated"},
            "signed_with": {"scheme": "ed25519", "key": envelope["signature"]["key"]},
            "commitment": commitment,
        }))
    }

    /// Grants every provider of the world and records its advertisement.
    fn admit_providers(&mut self) {
        for p in PROVIDERS {
            self.grant(p, GRANTED_AT);
            let ad = advertised(p, &advertising_key(p), p);
            let receipt = self.advertise(p, &ad).unwrap();
            // An advertisement is signed with an advertising key, which signs no other statement: it stays unverified.
            assert!(
                matches!(self.at(&receipt), Entry::Stated(s) if matches!(s.attribution, Attribution::Unverified { .. }))
            );
        }
    }

    /// `entry`, a stated entry of the world, as Core hands it to the log: signed for real by `key`, its bytes kept,
    /// and unverified, naming the maker it claims. `body` is the statement's body; it names the entry's transaction.
    fn claim(&mut self, entry: &Value, key: &Ed25519Key, body: Value) -> Value {
        let who = entry["attribution"]["who"].clone();
        let mut envelope = json!({
            "deployment": deployment(),
            "protocol_version": VERSION,
            "shape": entry["what"]["statement"],
            "maker": who["acts"],
            "made_at": entry["when"]["at"],
            "signature": {"scheme": "ed25519", "key": "", "value": ""},
            "statement": body,
        });
        sign_value(&mut envelope, key);
        let bytes = serde_json::to_vec(&envelope).unwrap();
        let mut claim = entry.clone();
        claim["attribution"] = json!({"signature": "unverified", "claimed_by": who});
        claim["signed_with"]["key"] = envelope["signature"]["key"].clone();
        claim["commitment"] = self.keep(&bytes);
        // A record citing the world's placeholder commitment, an erasure, cites the one kept.
        if !self
            .receipts
            .iter()
            .any(|(fake, _)| fake == &entry["commitment"])
        {
            self.receipts
                .push((entry["commitment"].clone(), claim["commitment"].clone()));
        }
        claim
    }

    /// Runs the world: its providers admitted, then each of its entries, the statements signed by their makers' own
    /// keys. `forge` names the world's places whose statement is signed with a key no one publishes instead. Each
    /// receipt is answered or the failure is, by the world's place.
    fn world(&mut self, forge: &[usize]) -> Vec<(usize, Result<Receipt, AppendFailure>)> {
        self.admit_providers();
        let mut answers = Vec::new();
        for (place, (placeholder, entry)) in world().into_iter().enumerate() {
            let entry = if entry["kind"] == "stated" {
                let maker = entry["attribution"]["who"]["acts"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let key = match (forge.contains(&place), maker == OPERATOR) {
                    (true, _) => another_key(),
                    (false, true) => operator_key(),
                    (false, false) => statements_key(&maker),
                };
                let body = match entry["about"]["transaction"].as_str() {
                    Some(t) => json!({"kind": "balance_verification", "transaction": t}),
                    None => json!({"kind": "judgement"}),
                };
                self.claim(&entry, &key, body)
            } else {
                entry
            };
            let answer = self.append_for(&placeholder, entry);
            answers.push((place, answer));
        }
        answers
    }
}

/// `provider`'s advertisement for the test deployment, stating for `publisher`, declaring its own ledger, signed with
/// `key`.
fn advertised(provider: &str, key: &Ed25519Key, publisher: &str) -> Vec<u8> {
    let context = SuiteContext {
        deployment: deployment(),
        ..SuiteContext::default()
    };
    // Each provider's ledger is its own private node, so no two publishers declare one node.
    let own = String::from_utf8(advertisement(provider, &context))
        .unwrap()
        .replace("stand-in-ledger", &format!("{provider}-ledger"));
    let mut value: Value = serde_json::from_str(&own).unwrap();
    value["statement"]["publisher"] = publisher.into();
    sign_value(&mut value, key);
    serde_json::to_vec(&value).unwrap()
}

/// The stand-alone records Core writes that cite nothing earlier, beside the world's.
const STANDALONE: [&str; 9] = [
    "verified-entry-of-a-charge-over-its-bound",
    "verified-entry-of-a-hand-over",
    "verified-entry-of-a-holding-check",
    "verified-entry-of-a-refusal-with-nothing-existing",
    "verified-entry-of-a-destination-whose-owner-changed",
    "verified-entry-of-a-settlement-time",
    "verified-entry-of-the-figures-behind-a-stuck-call",
    "verified-entry-of-an-operator-command",
    "verified-entry-of-a-registered-authorisation",
];

/// The contract's replay fixture, resting on a stated result of `run` about a segment: a replay cites its exit's
/// result (`replay-rests-on-its-exit`), so it follows the world rather than standing alone.
fn replay_on<S: Store>(run: &Run<S>) -> Value {
    let (receipt, about) = run
        .log
        .entries()
        .iter()
        .find_map(|(r, e)| match e {
            Entry::Stated(s) => s
                .about
                .as_ref()
                .filter(|a| a.order.is_some() && a.segment.is_some())
                .map(|a| (r.clone(), a.clone())),
            Entry::Verified(_) => None,
        })
        .expect("a stated result about a segment");
    let mut entry = valid("verified-entry-of-a-replay-consistent-with-its-exit");
    entry["what"]["order"] = serde_json::to_value(&about.order).unwrap();
    entry["what"]["segment"] = serde_json::to_value(&about.segment).unwrap();
    entry["what"]["rests_on"] = serde_json::to_value(&receipt).unwrap();
    entry
}

/// Every kind of verified entry `core-internal` defines, by its `call`, and the stated kind: an exhaustive match, so
/// a kind the contract adds fails this build until a test names it.
fn kind(entry: &Entry) -> &'static str {
    let Entry::Verified(VerifiedEntry { what, .. }) = entry else {
        return "stated";
    };
    match what {
        What::Decide { .. } => "decide",
        What::Authorise { .. } => "authorise",
        What::Effect { .. } => "effect",
        What::Result { .. } => "result",
        What::StuckDecide { .. } => "stuck_decide",
        What::StuckMove { .. } => "stuck_move",
        What::OpenSession { .. } => "open_session",
        What::Charge { .. } => "charge",
        What::AskAboutPerson { .. } => "ask_about_person",
        What::CheckRead { .. } => "check_read",
        What::GrantPermission { .. } => "grant_permission",
        What::RevokePermission { .. } => "revoke_permission",
        What::Refiling { .. } => "refiling",
        What::HandOver { .. } => "hand_over",
        What::ReportPair { .. } => "report_pair",
        What::HoldingCheck { .. } => "holding_check",
        What::Measure { .. } => "measure",
        What::Raise { .. } => "raise",
        What::Input { .. } => "input",
        What::Acceptance { .. } => "acceptance",
        What::OrderRefusal { .. } => "order_refusal",
        What::DestinationCheck { .. } => "destination_check",
        What::Arrival { .. } => "arrival",
        What::DecisionRecord { .. } => "decision_record",
        What::Shown { .. } => "shown",
        What::Settlement { .. } => "settlement",
        What::Evidence { .. } => "evidence",
        What::Fee { .. } => "fee",
        What::Replay { .. } => "replay",
        What::StuckCall { .. } => "stuck_call",
        What::Afterwards { .. } => "afterwards",
        What::Reliance { .. } => "reliance",
        What::Command { .. } => "command",
        What::Erasure { .. } => "erasure",
        What::ConfirmOutcome { .. } => "confirm_outcome",
        What::InputNotTaken { .. } => "input_not_taken",
    }
}

/// Every kind [`kind`] names.
const KINDS: [&str; 37] = [
    "decide",
    "authorise",
    "effect",
    "result",
    "stuck_decide",
    "stuck_move",
    "open_session",
    "charge",
    "ask_about_person",
    "check_read",
    "grant_permission",
    "revoke_permission",
    "refiling",
    "hand_over",
    "report_pair",
    "holding_check",
    "measure",
    "raise",
    "input",
    "acceptance",
    "order_refusal",
    "destination_check",
    "arrival",
    "decision_record",
    "shown",
    "settlement",
    "evidence",
    "fee",
    "replay",
    "stuck_call",
    "afterwards",
    "reliance",
    "command",
    "erasure",
    "confirm_outcome",
    "input_not_taken",
    "stated",
];

/// Whether the log recorded the stated entry at `receipt` verified.
fn verified_at<S: Store>(run: &Run<S>, receipt: &Receipt) -> bool {
    matches!(run.at(receipt), Entry::Stated(s) if matches!(s.attribution, Attribution::Verified { .. }))
}

// --- REQ-AUD-1 ------------------------------------------------------------------------------------------------------

/// Each entry of the world is appended and answered with a receipt of the entry recorded: at its place, SHA-256 of
/// its canonical JSON, under the log's root. A verified entry is recorded as handed in, a statement as the log
/// attributed it, and every entry says who, what, when, whose clock and the outcome.
// verifies: REQ-AUD-1, DEC-12, ADR-0005
#[test]
fn every_action_leaves_its_record() {
    let mut run = Run::new(MemoryStore::new());
    let answers = run.world(&[]);
    let world = world();
    for (place, answer) in &answers {
        let receipt = answer
            .as_ref()
            .unwrap_or_else(|e| panic!("world {place}: {e:?}"));
        let entry = run.at(receipt);
        let bytes = record(entry).unwrap();
        assert_eq!(receipt.entry, digest(&sha256(&bytes)));
        let index = receipt.index.get() as usize;
        let proof = run.log.inclusion(index, run.log.len()).unwrap();
        assert!(verify_inclusion(
            &leaf_hash(&bytes),
            index,
            run.log.len(),
            &proof,
            &run.log.root()
        ));
        let mut handed = world[*place].1.clone();
        run.rewrite(&mut handed);
        let recorded = serde_json::to_value(entry).unwrap();
        match entry {
            Entry::Verified(_) => assert_eq!(recorded, handed, "world {place}"),
            Entry::Stated(s) => {
                assert!(
                    matches!(s.attribution, Attribution::Verified { .. }),
                    "world {place}"
                );
                assert_eq!(
                    recorded["attribution"], handed["attribution"],
                    "world {place}"
                );
                for field in ["what", "when", "received_at", "outcome", "about"] {
                    assert_eq!(recorded[field], handed[field], "world {place}: {field}");
                }
            }
        }
        for field in ["what", "when", "outcome"] {
            assert!(!recorded[field].is_null(), "world {place}: {field}");
        }
        assert!(!recorded["when"]["source"].is_null());
        assert!(!recorded["who"].is_null() || !recorded["attribution"].is_null());
    }
    // Every grant (its decision, effect and result), advertisement and world entry is in the log, once.
    assert_eq!(run.log.len(), 4 * PROVIDERS.len() + world.len());
}

/// A refused decision, a refused read, a refused order and a request left unanswered are each recorded, with their
/// outcomes, as every permitted one is.
// verifies: REQ-AUD-1
#[test]
fn refusal_and_unanswered_request_each_leave_a_record() {
    let mut run = Run::new(MemoryStore::new());
    let answers = run.world(&[]);
    // The world's second effect has no result yet: its request goes unanswered.
    let effect = answers[place_of("effect", 1)].1.clone().unwrap();
    let mut unanswered = valid("verified-entry-of-a-result-unanswered");
    unanswered["what"]["effect"] = serde_json::to_value(&effect).unwrap();
    let mut refused_decide = valid("verified-entry-of-a-decision");
    refused_decide["what"]["decision"] = "dec-refused".into();
    refused_decide["outcome"] = json!({"status": "refused", "class": "not_permitted"});
    let mut not_known = valid("verified-entry-of-a-decision");
    not_known["what"]["decision"] = "dec-not-known".into();
    not_known["outcome"] = json!({"status": "not_known", "cause": "policy_unreachable"});
    for (entry, outcome) in [
        (unanswered, json!({"status": "unanswered"})),
        (
            refused_decide,
            json!({"status": "refused", "class": "not_permitted"}),
        ),
        (
            not_known,
            json!({"status": "not_known", "cause": "policy_unreachable"}),
        ),
        (
            valid("verified-entry-of-a-refused-read"),
            valid("verified-entry-of-a-refused-read")["outcome"].clone(),
        ),
        (
            valid("verified-entry-of-a-charge-over-its-bound"),
            valid("verified-entry-of-a-charge-over-its-bound")["outcome"].clone(),
        ),
        (
            valid("verified-entry-of-a-refusal-with-nothing-existing"),
            valid("verified-entry-of-a-refusal-with-nothing-existing")["outcome"].clone(),
        ),
    ] {
        let receipt = run
            .append(entry.clone())
            .unwrap_or_else(|e| panic!("{entry}: {e:?}"));
        let recorded = serde_json::to_value(run.at(&receipt)).unwrap();
        assert_eq!(recorded["outcome"], outcome);
        assert_eq!(recorded["what"], entry["what"]);
    }
}

/// Every kind of record Core writes reaches the log, each kind the contract defines among them, with no opt-in:
/// the world's, and the records that cite nothing earlier.
// verifies: REQ-AUD-1
#[test]
fn every_record_kind_core_writes_is_in_the_log() {
    let mut run = Run::new(MemoryStore::new());
    for (place, answer) in run.world(&[]) {
        answer.unwrap_or_else(|e| panic!("world {place}: {e:?}"));
    }
    for name in STANDALONE {
        run.append(valid(name))
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
    let replay = replay_on(&run);
    run.append(replay)
        .expect("the replay, on its exit's result");
    let found: BTreeSet<&str> = run.log.entries().iter().map(|(_, e)| kind(e)).collect();
    let all: BTreeSet<&str> = KINDS.into_iter().collect();
    assert_eq!(found, all);
}

/// No erasure reaches a statement a stuck order's decision relies on: the decision names its evidence by commitment,
/// and the log refuses an erasure's decision while it rests on it. The order is the world's, built in full: its
/// session, its shown quote, its acceptance on the reserve's verified yes, its permitted authorisation, its part
/// declared stuck, and the decision to deliver its value (core-internal's
/// `erasure-waits-while-anything-rests-on-the-statement`).
// verifies: REQ-AUD-13, REQ-AUD-2, REQ-AUZ-4
#[test]
fn no_erasure_reaches_a_statement_a_stuck_decision_relies_on() {
    let mut run = Run::new(MemoryStore::new());
    for (place, answer) in run.world(&[]) {
        answer.unwrap_or_else(|e| panic!("world {place}: {e:?}"));
    }
    let decided = world()
        .into_iter()
        .find(|(_, e)| e["what"]["call"] == "stuck_decide")
        .expect("the world's stuck decision")
        .1;
    let mut relied = decided["what"]["request"]["evidence"][0].clone();
    run.rewrite(&mut relied);
    assert!(
        run.log
            .entries()
            .iter()
            .any(|(_, e)| matches!(e, Entry::Stated(s)
            if serde_json::to_value(&s.commitment).unwrap() == relied)),
        "the statement the decision relies on is the log's"
    );
    let operator = actor(OPERATOR, "op-login-2", "operator", "person");
    let action =
        json!({"kind": "erase_statement", "commitment": relied, "reason": "retention_ended"});
    assert_eq!(
        run.append(json!({
            "kind": "verified",
            "who": operator,
            "what": {"call": "decide", "decision": "dec-erase-relied",
                     "request": {"request": "req-erase-relied", "caller": operator, "action": action,
                                 "at": "2026-11-10T09:00:00Z"}},
            "when": {"at": "2026-11-10T09:00:00Z", "source": "core_clock"},
            "outcome": {"status": "permitted"},
        })),
        Err(AppendFailure::Refused(Violation(
            "no statement is erased while an entry of this log rests on it"
        )))
    );
}

// --- INV-7, NFR-5 -------------------------------------------------------------------------------------------------

/// Orchestration acts on an effect only once the log answered its receipt. With a failure injected before the
/// write, and one after a durable write that still answered unavailable (a crash between the record and the
/// effect), no effect is taken without its record; the retry records the effect once and answers its receipt.
// verifies: INV-7, NFR-5
#[test]
fn failure_between_record_and_effect_leaves_no_effect() {
    for fault in [Fault::Before, Fault::After] {
        let store = Faulty::default();
        let mut run = Run::new(store.clone());
        // The decision, through policy's handle.
        let action = json!({"kind": "change_catalogue", "change": "grant", "provider": "fx-desk", "tuple": "tuple-fx-desk", "advertising_keys": [public_key(&advertising_key("fx-desk")).as_str()]});
        let mut decide = valid("verified-entry-of-a-decision");
        decide["what"]["request"]["action"] = action.clone();
        let decided = run.append(decide).unwrap();
        let mut effect = valid("verified-entry-of-an-effect");
        effect["what"]["action"] = action;
        effect["what"]["decision"]["receipt"] = serde_json::to_value(&decided).unwrap();

        // The effect is taken only on the receipt of its record.
        let mut taken: Vec<Receipt> = Vec::new();
        let act = |run: &mut Run<Faulty>, taken: &mut Vec<Receipt>| {
            if let Ok(receipt) = run.append(effect.clone()) {
                taken.push(receipt);
            }
        };
        store.plan(&[Some(fault)]);
        act(&mut run, &mut taken);
        assert!(
            taken.is_empty(),
            "{fault:?}: an effect was taken with no receipt"
        );
        let written = store.records().len();
        assert_eq!(
            written,
            if fault == Fault::After { 2 } else { 1 },
            "{fault:?}"
        );
        // Whatever the store holds, the log holds after it reads the store back; every effect taken has its record.
        act(&mut run, &mut taken);
        assert_eq!(taken.len(), 1, "{fault:?}");
        assert_eq!(
            store.records().len(),
            2,
            "{fault:?}: the retry wrote the effect once"
        );
        assert_eq!(run.log.len(), 2);
        for receipt in &taken {
            assert!(matches!(
                run.at(receipt),
                Entry::Verified(VerifiedEntry {
                    what: What::Effect { .. },
                    ..
                })
            ));
        }
        // Reopened, the log holds the effect once, at the receipt it answered.
        let reopened = AuditLog::open(settings(), store.clone(), run.kept.clone()).unwrap();
        assert_eq!(reopened.entries(), run.log.entries());
        assert_eq!(reopened.root(), run.log.root());
    }
}

// --- REQ-AUD-6 ----------------------------------------------------------------------------------------------------

/// After a full run, in a file store, the log's bytes hold no secret's value, no balance, and none of the kept
/// statements' bytes or salts. The run's secrets and a holding account's balance reach Core inside the statements
/// it keeps (the proxy's secrets as a provider's statement carries them, the balance as a `holding_balance` result
/// states it), and the evidence store holds each; the log holds only the salted commitment to them, and the proxy's
/// lookups and uses of the secrets without their values.
// verifies: REQ-AUD-6
#[test]
fn log_holds_no_secret_plaintext_and_no_balance_after_a_full_run() {
    const BALANCE: &str = "987654321.77";
    const SECRETS: [&str; 2] = ["sk-live-4f1c9a7e2b", "hunter2-of-the-proxy"];
    let file = TempFile::new("aud-6");
    let mut run = Run::new(FileStore::open(&file.0).unwrap());
    for (place, answer) in run.world(&[]) {
        answer.unwrap_or_else(|e| panic!("world {place}: {e:?}"));
    }
    for name in STANDALONE {
        run.append(valid(name)).unwrap();
    }
    let replay = replay_on(&run);
    run.append(replay).unwrap();
    // The proxy's lookups and uses of the run's secrets, handed over without their values.
    for (position, name) in [
        "handover-secret-lookup-resolved",
        "handover-secret-use-of-a-secret",
        "handover-secret-use-of-the-statement-key-refused",
        "handover-secret-lookup-of-a-sandboxed-part",
    ]
    .into_iter()
    .enumerate()
    {
        run.append(hand_over(valid(name), "drain-secrets", position))
            .unwrap();
    }
    // A holding account's balance, as its module states it: the statement is kept, the log names its commitment.
    let question = json!({
        "kind": "stated",
        "attribution": {"signature": "verified", "who": actor(OPERATOR, OPERATOR, "operator", "component")},
        "what": {"statement": "urn:rutent:provider-role:operation"},
        "when": {"at": "2026-11-06T15:04:05Z", "source": "party_stated"},
        "received_at": "2026-11-06T15:04:06Z",
        "outcome": {"status": "stated"},
        "signed_with": {"scheme": "ed25519", "key": public_key(&operator_key()).as_str()},
        "commitment": valid("stated-entry-of-a-quote")["commitment"],
        "about": {"order": "ord-1", "transaction": "tx-balance-1"},
    });
    let question = run.claim(
        &question,
        &operator_key(),
        json!({"kind": "holding_balance", "transaction": "tx-balance-1", "credential": SECRETS[0]}),
    );
    run.append(question).unwrap();
    let mut answer = question_answer("us-bank", "tx-balance-1");
    answer = run.claim(
        &answer,
        &statements_key("us-bank"),
        json!({"kind": "holding_balance", "transaction": "tx-balance-1", "balance": BALANCE, "token": SECRETS[1]}),
    );
    let receipt = run.append(answer).unwrap();
    assert!(verified_at(&run, &receipt));
    run.append(valid("verified-entry-of-a-holding-check"))
        .unwrap();

    let log = std::fs::read(&file.0).unwrap();
    let holds = |needle: &[u8]| log.windows(needle.len()).any(|w| w == needle);
    assert!(!holds(BALANCE.as_bytes()), "a balance in the log");
    for secret in SECRETS {
        assert!(!holds(secret.as_bytes()), "a secret in the log");
    }
    for kept in run.kept.statements() {
        assert!(
            !holds(kept.bytes.as_str().as_bytes()),
            "a kept statement's bytes in the log"
        );
        assert!(!holds(kept.salt.as_str().as_bytes()), "a salt in the log");
    }
    // Each of them did reach the evidence store, so the log's lacking them says something.
    let kept_holds = |needle: &str| {
        run.kept.statements().iter().any(|k| {
            let bytes = rutent_core_audit::log::kept::from_base64url(k.bytes.as_str()).unwrap();
            bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
        })
    };
    assert!(kept_holds(BALANCE));
    for secret in SECRETS {
        assert!(kept_holds(secret));
    }
}

/// A provider's answer to the operator's question under `transaction`, about the world's first order.
fn question_answer(maker: &str, transaction: &str) -> Value {
    json!({
        "kind": "stated",
        "attribution": {"signature": "verified", "who": provider(maker)},
        "what": {"statement": "urn:rutent:provider-role:result"},
        "when": {"at": "2026-11-06T15:04:05Z", "source": "party_stated"},
        "received_at": "2026-11-06T15:04:07Z",
        "outcome": {"status": "stated"},
        "signed_with": {"scheme": "ed25519", "key": public_key(&statements_key(maker)).as_str()},
        "commitment": valid("stated-entry-of-a-quote")["commitment"],
        "about": {"order": "ord-1", "transaction": transaction},
    })
}

// --- REQ-MDL-8, REQ-MDL-5 -----------------------------------------------------------------------------------------

/// `item` handed over in `drain` at `position`.
fn hand_over(item: Value, drain: &str, position: usize) -> Value {
    json!({
        "kind": "verified",
        "who": actor("rutent", "core-orch", "core", "component"),
        "what": {"call": "hand_over", "item": item, "drain": drain, "position": position},
        "when": {"at": "2026-11-10T09:00:00Z", "source": "platform_clock"},
        "outcome": {"status": "observed"},
    })
}

/// Every item the platform and the proxy hand over, in two drains, the same state read twice in one of them.
fn drains() -> Vec<Value> {
    let platform = [
        "handover-outbound-call",
        "handover-cross-module-call",
        "handover-state-read",
        "handover-state-read",
        "handover-limit",
        "handover-segment-record-moved-once",
        "handover-segment-record-never-moved",
        "handover-secret-lookup-resolved",
        "handover-secret-lookup-of-a-sandboxed-part",
    ];
    let proxy = [
        "handover-secret-use-of-a-secret",
        "handover-secret-use-of-the-statement-key-refused",
    ];
    let mut items = Vec::new();
    for (drain, names) in [("drain-1", &platform[..]), ("drain-2", &proxy[..])] {
        for (position, name) in names.iter().enumerate() {
            items.push(hand_over(valid(name), drain, position));
        }
    }
    items
}

/// The items the log holds, as values.
fn handed_in_log<S: Store>(run: &Run<S>) -> Vec<Value> {
    run.log
        .entries()
        .iter()
        .filter(|(_, e)| {
            matches!(
                e,
                Entry::Verified(VerifiedEntry {
                    what: What::HandOver { .. },
                    ..
                })
            )
        })
        .map(|(_, e)| serde_json::to_value(e).unwrap())
        .collect()
}

/// Every item the platform and the proxy hand over is in the log, whole, once: two identical state reads of one
/// drain are two entries, told apart by their place in it.
// verifies: REQ-MDL-8
#[test]
fn no_platform_event_handed_to_the_log_is_missing_from_it() {
    let mut run = Run::new(MemoryStore::new());
    let items = drains();
    for item in &items {
        run.append(item.clone()).unwrap();
    }
    assert_eq!(handed_in_log(&run), items);
    let reads = items
        .iter()
        .filter(|i| i["what"]["item"] == valid("handover-state-read"))
        .count();
    assert_eq!(reads, 2);
    // Each kind of item the contract hands over is among them.
    let kinds: BTreeSet<String> = run
        .log
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry {
                what: What::HandOver { item, .. },
                ..
            }) => Some(match item {
                HandedOver::OutboundCall { .. } => "outbound_call",
                HandedOver::CrossModuleCall { .. } => "cross_module_call",
                HandedOver::SecretLookup { .. } => "secret_lookup",
                HandedOver::StateRead { .. } => "state_read",
                HandedOver::Limit { .. } => "limit",
                HandedOver::SecretUse { .. } => "secret_use",
                HandedOver::SegmentRecord { .. } => "segment_record",
            }),
            _ => None,
        })
        .map(str::to_owned)
        .collect();
    assert_eq!(kinds.len(), 7);
}

/// A segment record handed over is in the log's file with the version it began on, the one it runs on and each
/// move between them, and is still there, whole and at the same place under the same root, when the log is opened
/// again from that file; a last line cut short by a crash is dropped, and the log goes on. While one store holds the
/// file, no second one opens it.
// verifies: REQ-MDL-5, DEC-12
#[test]
fn segment_record_is_kept_durably_with_its_versions_and_moves() {
    let file = TempFile::new("mdl-5");
    let records = [
        hand_over(valid("handover-segment-record-moved-once"), "drain-7", 0),
        hand_over(valid("handover-segment-record-never-moved"), "drain-7", 1),
    ];
    let (entries, root) = {
        let mut run = Run::new(FileStore::open(&file.0).unwrap());
        for r in &records {
            run.append(r.clone()).unwrap();
        }
        (run.log.entries().to_vec(), run.log.root())
    };
    // A crash mid-write leaves a torn last line.
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file.0)
            .unwrap();
        f.write_all(b"{\"kind\":\"verif").unwrap();
    }
    let mut reopened = Run::new(FileStore::open(&file.0).unwrap());
    assert_eq!(reopened.log.entries(), &entries[..]);
    assert_eq!(reopened.log.root(), root);
    for (record, (_, entry)) in records.iter().zip(reopened.log.entries()) {
        let entry = serde_json::to_value(entry).unwrap();
        let (handed, kept) = (&record["what"]["item"], &entry["what"]["item"]);
        assert_eq!(kept["kind"], "segment_record");
        for field in ["transaction", "provider", "began_on", "runs_on", "moves"] {
            assert_eq!(kept[field], handed[field], "{field}");
        }
    }
    assert_eq!(
        records[0]["what"]["item"]["moves"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    reopened
        .append(hand_over(valid("handover-state-read"), "drain-8", 0))
        .unwrap();
    // While one store holds the file, no other can write it.
    assert!(matches!(
        FileStore::open(&file.0),
        Err(StoreError::Unavailable(_))
    ));
    drop(reopened);
    let again = AuditLog::open(
        settings(),
        FileStore::open(&file.0).unwrap(),
        KeptInMemory::new(),
    )
    .unwrap();
    assert_eq!(again.len(), 3);
}

/// Handed-over items whose appends fail, before the write and after it, are appended again until the log answers:
/// then every item is in the log once, and the store holds each once.
// verifies: REQ-MDL-8, NFR-5
#[test]
fn handed_over_events_survive_a_failed_append_that_is_retried() {
    let store = Faulty::default();
    let mut run = Run::new(store.clone());
    let items = drains();
    store.plan(&[
        Some(Fault::Before),
        None,
        Some(Fault::After),
        Some(Fault::After),
        None,
        Some(Fault::Before),
        Some(Fault::Before),
        None,
        Some(Fault::After),
    ]);
    let mut failures = 0;
    for item in &items {
        // Orchestration keeps an item until the log answers its receipt.
        let mut tries = 0;
        while run.append(item.clone()).is_err() {
            failures += 1;
            tries += 1;
            assert!(tries < 5);
        }
    }
    assert!(failures >= 6);
    assert_eq!(handed_in_log(&run), items);
    assert_eq!(store.records().len(), items.len());
    // An item appended again after its receipt is the same entry, not a second.
    let receipt = run.append(items[0].clone()).unwrap();
    assert_eq!(receipt.index.get(), 0);
    assert_eq!(run.log.len(), items.len());
}

// --- REQ-AUD-2 ----------------------------------------------------------------------------------------------------

/// A statement whose writer claims a provider's signature it doesn't have is recorded unverified, attributed to no
/// one: bytes signed with a key no advertisement lists, and bytes that name the provider's key but were signed by
/// another. The provider's genuine statement is recorded verified. An acceptance resting on the reserve's yes the
/// log recorded unverified is refused, and the world goes on only on the genuine one.
// verifies: REQ-AUD-2, REQ-ORD-32
#[test]
fn a_stated_entry_claiming_a_verified_signature_it_doesnt_have_is_recorded_unverified() {
    // The reserve's yes signed with a key no one publishes: unverified, and the acceptance that rests on it is
    // refused.
    let (yes_at, acceptance_at) = (first_yes(), place_of("acceptance", 0));
    let mut forged = Run::new(MemoryStore::new());
    let answers = forged.world(&[yes_at]);
    let yes = answers[yes_at].1.clone().unwrap();
    assert!(!verified_at(&forged, &yes));
    assert!(matches!(
        forged.at(&yes),
        Entry::Stated(StatedEntry {
            attribution: Attribution::Unverified { .. },
            ..
        })
    ));
    let Err(AppendFailure::Refused(Violation(why))) = &answers[acceptance_at].1 else {
        panic!(
            "the acceptance on a forged yes: {:?}",
            answers[acceptance_at].1
        );
    };
    assert!(why.contains("verified"), "{why}");

    // The same world signed by its makers: every statement verified, the acceptance admitted.
    let mut genuine = Run::new(MemoryStore::new());
    let answers = genuine.world(&[]);
    assert!(verified_at(&genuine, answers[yes_at].1.as_ref().unwrap()));
    assert!(answers[acceptance_at].1.is_ok());

    // Bytes that name the provider's own statements key, signed by another: the signature fails, so unverified.
    let mut run = Run::new(MemoryStore::new());
    run.admit_providers();
    let question = question_of("tx-claim-1");
    let question = run.claim(
        &question,
        &operator_key(),
        json!({"kind": "balance_verification", "transaction": "tx-claim-1"}),
    );
    let receipt = run.append(question).unwrap();
    assert!(verified_at(&run, &receipt));
    let entry = question_answer("us-bank", "tx-claim-1");
    let mut claim = run.claim(
        &entry,
        &another_key(),
        json!({"kind": "balance_verification", "transaction": "tx-claim-1"}),
    );
    claim["signed_with"]["key"] = public_key(&statements_key("us-bank")).as_str().into();
    let bytes = {
        let mut envelope = json!({
            "deployment": deployment(), "protocol_version": VERSION, "shape": "urn:rutent:provider-role:result",
            "maker": "us-bank", "made_at": "2026-11-06T15:04:05Z",
            "signature": {"scheme": "ed25519", "key": "", "value": ""},
            "statement": {"kind": "balance_verification", "transaction": "tx-claim-1"},
        });
        sign_value(&mut envelope, &another_key());
        envelope["signature"]["key"] = public_key(&statements_key("us-bank")).as_str().into();
        serde_json::to_vec(&envelope).unwrap()
    };
    claim["commitment"] = run.keep(&bytes);
    let receipt = run.append(claim).unwrap();
    assert!(!verified_at(&run, &receipt));
    // The genuine answer is verified.
    let claim = run.claim(
        &entry,
        &statements_key("us-bank"),
        json!({"kind": "balance_verification", "transaction": "tx-claim-1"}),
    );
    let receipt = run.append(claim).unwrap();
    assert!(verified_at(&run, &receipt));
    // A writer can't hand the log a statement it attributed itself, built or read.
    let mut attributed = question_answer("us-bank", "tx-claim-1");
    attributed["commitment"] = run.keep(b"{}");
    let attributed: Entry = serde_json::from_value(attributed).unwrap();
    assert!(GeneralEntry::try_from(attributed).is_err());
}

/// The operator's question under `transaction`, about the world's first order.
fn question_of(transaction: &str) -> Value {
    json!({
        "kind": "stated",
        "attribution": {"signature": "verified", "who": actor(OPERATOR, OPERATOR, "operator", "component")},
        "what": {"statement": "urn:rutent:provider-role:operation"},
        "when": {"at": "2026-11-06T15:04:05Z", "source": "party_stated"},
        "received_at": "2026-11-06T15:04:06Z",
        "outcome": {"status": "stated"},
        "signed_with": {"scheme": "ed25519", "key": public_key(&operator_key()).as_str()},
        "commitment": valid("stated-entry-of-a-quote")["commitment"],
        "about": {"order": "ord-1", "transaction": transaction},
    })
}

/// `maker`'s answer under a fresh question with transaction `transaction`, signed by `key`, made at `made_at`:
/// whether the log recorded it verified.
fn answered<S: Store>(
    run: &mut Run<S>,
    maker: &str,
    key: &Ed25519Key,
    transaction: &str,
    made_at: &str,
) -> bool {
    let mut question = question_of(transaction);
    question["when"]["at"] = made_at.into();
    let question = run.claim(
        &question,
        &operator_key(),
        json!({"kind": "balance_verification", "transaction": transaction}),
    );
    run.append(question).unwrap();
    let mut answer = question_answer(maker, transaction);
    answer["when"]["at"] = made_at.into();
    let answer = run.claim(
        &answer,
        key,
        json!({"kind": "balance_verification", "transaction": transaction}),
    );
    let receipt = run.append(answer).unwrap();
    verified_at(run, &receipt)
}

// --- Whose keys: the register the log keeps from its own records ---------------------------------------------------

/// Orchestration can't make its own key a provider's: an advertisement for a granted provider signed with a key the
/// deployment doesn't trust for it, listing orchestration's key for statements, is recorded and changes no key, so a
/// statement signed with orchestration's key is recorded unverified. The provider's own first advertisement, signed
/// with the key the deployment trusts, is then accepted, and its statements key signs.
// verifies: REQ-AUD-2, THR-3, THR-4
#[test]
fn an_advertisement_orchestration_signed_names_no_key_of_the_provider() {
    let mut run = Run::new(MemoryStore::new());
    let orchestration = signing_key("orchestration's own", 9);
    run.grant("us-bank", GRANTED_AT);
    // Orchestration's advertisement for us-bank: its own key for advertising and statements.
    let mut forged: Value =
        serde_json::from_slice(&advertised("us-bank", &orchestration, "us-bank")).unwrap();
    forged["statement"]["keys"] = json!([
        {"scheme": "ed25519", "public_key": public_key(&orchestration).as_str(), "purpose": "advertising"},
        {"scheme": "ed25519", "public_key": public_key(&signing_key("orchestration's own", 10)).as_str(), "purpose": "statements"},
    ]);
    sign_value(&mut forged, &orchestration);
    run.advertise("us-bank", &serde_json::to_vec(&forged).unwrap())
        .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &signing_key("orchestration's own", 10),
        "tx-o-1",
        "2026-11-06T15:04:05Z"
    ));
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-o-2",
        "2026-11-06T15:04:05Z"
    ));
    // Bytes naming the key the deployment trusts for us-bank, but signed by orchestration's: the signature fails.
    let mut tampered: Value =
        serde_json::from_slice(&advertised("us-bank", &orchestration, "us-bank")).unwrap();
    tampered["signature"]["key"] = public_key(&advertising_key("us-bank")).as_str().into();
    run.advertise("us-bank", &serde_json::to_vec(&tampered).unwrap())
        .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-o-5",
        "2026-11-06T15:04:05Z"
    ));
    // The provider's own, signed with the key the deployment trusts for it.
    run.advertise(
        "us-bank",
        &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
    )
    .unwrap();
    assert!(answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-o-3",
        "2026-11-06T15:04:05Z"
    ));
    assert!(!answered(
        &mut run,
        "us-bank",
        &signing_key("orchestration's own", 10),
        "tx-o-4",
        "2026-11-06T15:04:05Z"
    ));
}

/// A later advertisement is accepted only as a successor: signed with an advertising key the one accepted last
/// lists, and newer. One signed with the provider's statements key, which a compromised outside part holds, adds no
/// key; one signed with its advertising key, adding a fresh statements key and dropping the old, is accepted, and
/// then only the fresh key signs.
// verifies: REQ-AUD-2, THR-4
#[test]
fn a_later_advertisement_signs_in_keys_only_as_a_successor() {
    let mut run = Run::new(MemoryStore::new());
    run.grant("us-bank", GRANTED_AT);
    run.advertise(
        "us-bank",
        &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
    )
    .unwrap();
    let fresh = signing_key("us-bank's fresh statements key", 3);
    let successor = |key: &Ed25519Key, made_at: &str| {
        let mut ad: Value = serde_json::from_slice(&advertised("us-bank", key, "us-bank")).unwrap();
        ad["made_at"] = made_at.into();
        ad["statement"]["keys"] = json!([
            {"scheme": "ed25519", "public_key": public_key(&advertising_key("us-bank")).as_str(), "purpose": "advertising"},
            {"scheme": "ed25519", "public_key": public_key(&fresh).as_str(), "purpose": "statements"},
        ]);
        sign_value(&mut ad, key);
        serde_json::to_vec(&ad).unwrap()
    };
    // Signed with the statements key: refused.
    run.advertise(
        "us-bank",
        &successor(&statements_key("us-bank"), "2026-11-06T00:00:00.5Z"),
    )
    .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &fresh,
        "tx-s-1",
        "2026-11-06T15:04:05Z"
    ));
    // Signed with the advertising key, but no newer: refused.
    run.advertise(
        "us-bank",
        &successor(&advertising_key("us-bank"), ADVERTISED_AT),
    )
    .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &fresh,
        "tx-s-2",
        "2026-11-06T15:04:05Z"
    ));
    // Signed with the advertising key, and newer: accepted.
    run.advertise(
        "us-bank",
        &successor(&advertising_key("us-bank"), "2026-11-06T00:00:00.5Z"),
    )
    .unwrap();
    assert!(answered(
        &mut run,
        "us-bank",
        &fresh,
        "tx-s-3",
        "2026-11-06T15:04:05Z"
    ));
    // The dropped key no longer signs, even for a statement dated while it was listed.
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-s-4",
        "2026-11-06T00:00:00.2Z"
    ));
    // A statement dated before the fresh key was listed isn't signed by it.
    assert!(!answered(
        &mut run,
        "us-bank",
        &fresh,
        "tx-s-5",
        "2026-11-06T00:00:00.2Z"
    ));
}

/// A provider's key signs only while a grant of it is in force: an advertisement recorded before any grant is
/// accepted for nothing, and once the grant is revoked a statement made after the revocation is recorded
/// unverified, while one made before it still verifies.
// verifies: REQ-AUD-2, REQ-CAT-2
#[test]
fn a_provider_signs_only_under_a_grant_in_force() {
    let mut run = Run::new(MemoryStore::new());
    run.advertise(
        "us-bank",
        &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
    )
    .unwrap();
    run.grant("us-bank", GRANTED_AT);
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-g-1",
        "2026-11-06T15:04:05Z"
    ));
    run.advertise(
        "us-bank",
        &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
    )
    .unwrap();
    assert!(answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-g-2",
        "2026-11-06T15:04:05Z"
    ));
    run.revoke("us-bank", "2026-11-07T00:00:00Z");
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-g-3",
        "2026-11-07T00:00:01Z"
    ));
    assert!(answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-g-4",
        "2026-11-06T23:59:59Z"
    ));
}

/// The operator's statements verify only under the deployment's `operator_key`, the deployment's own setting: a
/// question signed with any other key, a provider's included, is recorded unverified, so no answer follows it.
// verifies: REQ-AUD-2, REQ-ORD-32
#[test]
fn the_operators_statements_verify_only_under_the_deployments_key() {
    let mut run = Run::new(MemoryStore::new());
    run.admit_providers();
    let question = question_of("tx-q-1");
    for key in [another_key(), statements_key("us-bank")] {
        let mut claim = run.claim(
            &question,
            &key,
            json!({"kind": "balance_verification", "transaction": "tx-q-1"}),
        );
        claim["received_at"] = "2026-11-06T15:04:09Z".into();
        let receipt = run.append(claim).unwrap();
        assert!(!verified_at(&run, &receipt));
    }
    let answer = question_answer("us-bank", "tx-q-1");
    let answer = run.claim(
        &answer,
        &statements_key("us-bank"),
        json!({"kind": "balance_verification", "transaction": "tx-q-1"}),
    );
    assert!(matches!(run.append(answer), Err(AppendFailure::Refused(_))));
    let claim = run.claim(
        &question,
        &operator_key(),
        json!({"kind": "balance_verification", "transaction": "tx-q-1"}),
    );
    let receipt = run.append(claim).unwrap();
    assert!(verified_at(&run, &receipt));
}

// --- The append's own guards --------------------------------------------------------------------------------------

/// Every append is held to the whole citation table on its path: an effect citing a permit no permitting decision of
/// this log backs is refused, as is an effect on a permit already used, and nothing is recorded.
// verifies: INV-7, INV-8, REQ-AUZ-1
#[test]
fn an_effect_on_a_permit_this_log_holds_no_decision_for_is_refused() {
    let mut run = Run::new(MemoryStore::new());
    let action = json!({"kind": "change_catalogue", "change": "grant", "provider": "fx-desk", "tuple": "tuple-fx-desk", "advertising_keys": [public_key(&advertising_key("fx-desk")).as_str()]});
    let mut effect = valid("verified-entry-of-an-effect");
    effect["what"]["action"] = action.clone();
    assert!(matches!(
        run.append(effect.clone()),
        Err(AppendFailure::Refused(_))
    ));
    assert!(run.log.is_empty());
    let mut decide = valid("verified-entry-of-a-decision");
    decide["what"]["request"]["action"] = action;
    let decided = run.append(decide).unwrap();
    effect["what"]["decision"]["receipt"] = serde_json::to_value(&decided).unwrap();
    run.append(effect.clone()).unwrap();
    let mut other = effect;
    other["when"]["at"] = "2026-11-10T09:00:01Z".into();
    assert!(matches!(run.append(other), Err(AppendFailure::Refused(_))));
    assert_eq!(run.log.len(), 2);
}

/// An entry built in memory is held to the contract's rules as one read from the wire: a command whose `who` isn't
/// the operator, which the general handle's type lets through, is refused.
// verifies: REQ-AUD-1, REQ-AUD-6
#[test]
fn an_entry_breaking_the_contracts_rules_is_refused_however_it_was_built() {
    let mut run = Run::new(MemoryStore::new());
    let command = valid("verified-entry-of-an-operator-command");
    let mut entry: VerifiedEntry = serde_json::from_value(command).unwrap();
    entry.who = serde_json::from_value(actor("rutent", "core-orch", "core", "component")).unwrap();
    assert!(entry.check().is_err());
    let general = GeneralEntry::try_from(Entry::Verified(entry)).unwrap();
    assert!(matches!(
        run.log.append(&general),
        Err(AppendFailure::Refused(_))
    ));
    assert!(run.log.is_empty());
}

/// A stated entry whose bytes the evidence store doesn't keep is refused: it would rest on nothing anyone can check.
// verifies: REQ-AUD-2
#[test]
fn a_stated_entry_whose_bytes_are_not_kept_is_refused() {
    let mut run = Run::new(MemoryStore::new());
    let mut claim = question_of("tx-k-1");
    claim["attribution"] = json!({"signature": "unverified", "claimed_by": actor(OPERATOR, OPERATOR, "operator", "component")});
    assert!(matches!(run.append(claim), Err(AppendFailure::Refused(_))));
    assert!(run.log.is_empty());
}

/// Names an item only for a type that doesn't implement [`PolicyLog`]: for one that does, the two impls make the
/// name ambiguous and the test doesn't build.
trait NotPolicyLog<A> {
    fn holds() {}
}
impl<T: ?Sized> NotPolicyLog<()> for T {}
impl<T: ?Sized + PolicyLog> NotPolicyLog<u8> for T {}

/// `core-audit`'s checks hold a general handle on the log, which is no policy handle: they append through the
/// general path, and a decision entry can't reach the log through it, however it is built or read.
// verifies: REQ-AUZ-1, DEC-2
#[test]
fn the_checks_general_handle_writes_no_decision() {
    type Shared = AuditLog<MemoryStore, KeptInMemory>;
    <General<Shared> as NotPolicyLog<_>>::holds();
    let log = Rc::new(RefCell::new(
        AuditLog::open(settings(), MemoryStore::new(), KeptInMemory::new()).unwrap(),
    ));
    let mut checks = General::new(&log);
    let pair: GeneralEntry =
        serde_json::from_value(valid("verified-entry-of-a-holding-check")).unwrap();
    let receipt = checks.append(&pair).unwrap();
    assert_eq!(receipt.index.get(), 0);
    let decision =
        Entry::Verified(serde_json::from_value(valid("verified-entry-of-a-decision")).unwrap());
    assert!(GeneralEntry::try_from(decision).is_err());
    assert_eq!(log.borrow().len(), 1);
}

/// The log only grows: the root over its first entries never changes as later ones are appended, and each entry's
/// proof under an earlier root still verifies.
// verifies: DEC-12, ADR-0005, INV-6
#[test]
fn earlier_roots_stay_as_the_log_grows() {
    let mut run = Run::new(MemoryStore::new());
    let mut roots = BTreeMap::new();
    for item in drains() {
        run.append(item).unwrap();
        roots.insert(run.log.len(), run.log.root());
    }
    for (size, root) in &roots {
        assert_eq!(run.log.root_at(*size), Some(*root));
        for index in 0..*size {
            let bytes = record(&run.log.entries()[index].1).unwrap();
            let proof = run.log.inclusion(index, *size).unwrap();
            assert!(verify_inclusion(
                &leaf_hash(&bytes),
                index,
                *size,
                &proof,
                root
            ));
        }
    }
    assert_eq!(run.log.root_at(run.log.len() + 1), None);
}

/// The key history holds across advertisements and providers: a statements key a provider dropped never signs for
/// it again, even listed anew by a successor; a key another provider listed for advertising never signs anyone's
/// statements; and an advertisement dated past the skew ahead of its receipt, or made for another deployment, or
/// stating for a party its maker's statements then claim it doesn't, changes no key.
// verifies: REQ-AUD-2, THR-4, REQ-PRT-1
#[test]
fn a_dropped_or_crossed_key_never_signs_again() {
    let mut run = Run::new(MemoryStore::new());
    run.grant("us-bank", GRANTED_AT);
    run.grant("fx-desk", GRANTED_AT);
    run.advertise(
        "us-bank",
        &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
    )
    .unwrap();
    run.advertise(
        "fx-desk",
        &advertised("fx-desk", &advertising_key("fx-desk"), "fx-desk"),
    )
    .unwrap();
    let advertise_keys = |run: &mut Run<MemoryStore>, made_at: &str, keys: Value| {
        let mut ad: Value = serde_json::from_slice(&advertised(
            "us-bank",
            &advertising_key("us-bank"),
            "us-bank",
        ))
        .unwrap();
        ad["made_at"] = made_at.into();
        ad["statement"]["keys"] = keys;
        sign_value(&mut ad, &advertising_key("us-bank"));
        run.advertise("us-bank", &serde_json::to_vec(&ad).unwrap())
            .unwrap();
    };
    let listed = |key: &Ed25519Key, purpose: &str| json!({"scheme": "ed25519", "public_key": public_key(key).as_str(), "purpose": purpose});
    let fresh = signing_key("us-bank's next statements key", 4);
    // Drop the statements key for a fresh one, then list the dropped one again beside it.
    advertise_keys(
        &mut run,
        "2026-11-06T01:00:00Z",
        json!([
            listed(&advertising_key("us-bank"), "advertising"),
            listed(&fresh, "statements")
        ]),
    );
    advertise_keys(
        &mut run,
        "2026-11-06T02:00:00Z",
        json!([
            listed(&advertising_key("us-bank"), "advertising"),
            listed(&fresh, "statements"),
            listed(&statements_key("us-bank"), "statements")
        ]),
    );
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-d-1",
        "2026-11-06T15:04:05Z"
    ));
    assert!(answered(
        &mut run,
        "us-bank",
        &fresh,
        "tx-d-2",
        "2026-11-06T15:04:05Z"
    ));
    // List fx-desk's advertising key for statements.
    advertise_keys(
        &mut run,
        "2026-11-06T03:00:00Z",
        json!([
            listed(&advertising_key("us-bank"), "advertising"),
            listed(&fresh, "statements"),
            listed(&advertising_key("fx-desk"), "statements")
        ]),
    );
    assert!(!answered(
        &mut run,
        "us-bank",
        &advertising_key("fx-desk"),
        "tx-d-3",
        "2026-11-06T15:04:05Z"
    ));
    // Dated past the skew ahead of its receipt.
    let ahead = signing_key("us-bank's key from the future", 5);
    advertise_keys(
        &mut run,
        "2026-11-06T04:00:00Z",
        json!([
            listed(&advertising_key("us-bank"), "advertising"),
            listed(&ahead, "statements")
        ]),
    );
    assert!(answered(
        &mut run,
        "us-bank",
        &ahead,
        "tx-d-4",
        "2026-11-06T15:04:05Z"
    ));
    let later = signing_key("us-bank's key from the future", 6);
    let mut ad: Value = serde_json::from_slice(&advertised(
        "us-bank",
        &advertising_key("us-bank"),
        "us-bank",
    ))
    .unwrap();
    ad["made_at"] = "2026-11-06T04:10:00Z".into();
    ad["statement"]["keys"] = json!([
        listed(&advertising_key("us-bank"), "advertising"),
        listed(&later, "statements")
    ]);
    sign_value(&mut ad, &advertising_key("us-bank"));
    run.advertise_at(
        "us-bank",
        &serde_json::to_vec(&ad).unwrap(),
        "2026-11-06T04:04:59Z",
    )
    .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &later,
        "tx-d-5",
        "2026-11-06T15:04:05Z"
    ));
    // Made for another deployment.
    let elsewhere = signing_key("us-bank's key elsewhere", 7);
    let mut ad: Value = serde_json::from_slice(&advertised(
        "us-bank",
        &advertising_key("us-bank"),
        "us-bank",
    ))
    .unwrap();
    ad["made_at"] = "2026-11-06T05:00:00Z".into();
    ad["deployment"]["name"] = "another-deployment".into();
    ad["statement"]["keys"] = json!([
        listed(&advertising_key("us-bank"), "advertising"),
        listed(&elsewhere, "statements")
    ]);
    sign_value(&mut ad, &advertising_key("us-bank"));
    run.advertise("us-bank", &serde_json::to_vec(&ad).unwrap())
        .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &elsewhere,
        "tx-d-6",
        "2026-11-06T15:04:05Z"
    ));
    // A first advertisement made for another deployment, signed with the key this one trusts.
    run.grant("bank-co", GRANTED_AT);
    let mut ad: Value = serde_json::from_slice(&advertised(
        "bank-co",
        &advertising_key("bank-co"),
        "bank-co",
    ))
    .unwrap();
    ad["deployment"]["name"] = "another-deployment".into();
    sign_value(&mut ad, &advertising_key("bank-co"));
    run.advertise("bank-co", &serde_json::to_vec(&ad).unwrap())
        .unwrap();
    assert!(!answered(
        &mut run,
        "bank-co",
        &statements_key("bank-co"),
        "tx-d-8",
        "2026-11-06T15:04:05Z"
    ));
    run.advertise(
        "bank-co",
        &advertised("bank-co", &advertising_key("bank-co"), "bank-co"),
    )
    .unwrap();
    assert!(answered(
        &mut run,
        "bank-co",
        &statements_key("bank-co"),
        "tx-d-9",
        "2026-11-06T15:04:05Z"
    ));
    // A statement claiming us-bank states for another party than its advertisement's publisher.
    let mut question = question_of("tx-d-7");
    question = run.claim(
        &question,
        &operator_key(),
        json!({"kind": "balance_verification", "transaction": "tx-d-7"}),
    );
    run.append(question).unwrap();
    let mut answer = question_answer("us-bank", "tx-d-7");
    answer["attribution"]["who"]["party"] = "fx-desk".into();
    let answer = run.claim(
        &answer,
        &ahead,
        json!({"kind": "balance_verification", "transaction": "tx-d-7"}),
    );
    let receipt = run.append(answer).unwrap();
    assert!(!verified_at(&run, &receipt));
}

/// The log opens only on a store whose every record is an entry in its canonical form: a record rewritten in any
/// other form, or one that is no entry, is refused rather than read as history.
// verifies: DEC-12, INV-6
#[test]
fn a_store_holding_a_record_that_is_no_entry_of_the_log_is_refused() {
    let file = TempFile::new("corrupt");
    {
        let mut run = Run::new(FileStore::open(&file.0).unwrap());
        for item in drains() {
            run.append(item).unwrap();
        }
    }
    let good = std::fs::read(&file.0).unwrap();
    let first = good.iter().position(|b| *b == b'\n').unwrap();
    let mut entry: Value = serde_json::from_slice(&good[..first]).unwrap();
    entry["when"]["at"] = "2026-11-10T09:00:01Z".into();
    for (case, line) in [
        (
            "not canonical",
            serde_json::to_vec_pretty(&entry)
                .unwrap()
                .into_iter()
                .filter(|b| *b != b'\n')
                .collect::<Vec<u8>>(),
        ),
        ("no entry", b"{\"kind\":\"verified\"}".to_vec()),
    ] {
        let mut bytes = line;
        bytes.extend_from_slice(&good[first..]);
        std::fs::write(&file.0, &bytes).unwrap();
        let opened = AuditLog::open(
            settings(),
            FileStore::open(&file.0).unwrap(),
            KeptInMemory::new(),
        );
        assert!(matches!(opened, Err(AppendFailure::Refused(_))), "{case}");
    }
}

/// An advertisement that breaks provider-role's own rules changes no key, even signed with the key the deployment
/// trusts: one signed with an advertising key it doesn't list, and one listing a key that is no key to accept
/// signatures from (a point of small order) beside a good one.
// verifies: REQ-AUD-2, THR-4, THR-8
#[test]
fn an_advertisement_breaking_provider_roles_rules_changes_no_key() {
    let mut run = Run::new(MemoryStore::new());
    let listed = |key: &str, purpose: &str| json!({"scheme": "ed25519", "public_key": key, "purpose": purpose});
    for p in ["kyc-check", "fx-desk"] {
        run.grant(p, GRANTED_AT);
    }
    // kyc-check's, signed with its trusted advertising key, which it lists for nothing.
    let mut ad: Value = serde_json::from_slice(&advertised(
        "kyc-check",
        &advertising_key("kyc-check"),
        "kyc-check",
    ))
    .unwrap();
    ad["statement"]["keys"] = json!([listed(
        public_key(&statements_key("kyc-check")).as_str(),
        "statements"
    )]);
    sign_value(&mut ad, &advertising_key("kyc-check"));
    run.advertise("kyc-check", &serde_json::to_vec(&ad).unwrap())
        .unwrap();
    assert!(!answered(
        &mut run,
        "kyc-check",
        &statements_key("kyc-check"),
        "tx-r-1",
        "2026-11-06T15:04:05Z"
    ));
    // fx-desk's, listing the identity point, of small order, beside its statements key.
    let small_order = base64url_of(&{
        let mut b = [0u8; 32];
        b[0] = 1;
        b
    });
    let mut ad: Value = serde_json::from_slice(&advertised(
        "fx-desk",
        &advertising_key("fx-desk"),
        "fx-desk",
    ))
    .unwrap();
    ad["statement"]["keys"] = json!([
        listed(
            public_key(&advertising_key("fx-desk")).as_str(),
            "advertising"
        ),
        listed(
            public_key(&statements_key("fx-desk")).as_str(),
            "statements"
        ),
        listed(&small_order, "statements"),
    ]);
    sign_value(&mut ad, &advertising_key("fx-desk"));
    run.advertise("fx-desk", &serde_json::to_vec(&ad).unwrap())
        .unwrap();
    assert!(!answered(
        &mut run,
        "fx-desk",
        &statements_key("fx-desk"),
        "tx-r-2",
        "2026-11-06T15:04:05Z"
    ));
    // Each provider's own advertisement, as provider-role has it, is accepted.
    for p in ["kyc-check", "fx-desk"] {
        run.advertise(p, &advertised(p, &advertising_key(p), p))
            .unwrap();
    }
    assert!(answered(
        &mut run,
        "kyc-check",
        &statements_key("kyc-check"),
        "tx-r-3",
        "2026-11-06T15:04:05Z"
    ));
    assert!(answered(
        &mut run,
        "fx-desk",
        &statements_key("fx-desk"),
        "tx-r-4",
        "2026-11-06T15:04:05Z"
    ));
}

fn base64url_of(bytes: &[u8]) -> String {
    rutent_core_audit::log::kept::base64url(bytes)
}

/// A store that can't be read back after a failed write fails the next append as unavailable, not refused: the
/// entry broke no rule, so orchestration keeps it and appends it again, and once the store reads back the retry
/// is recorded once. A read-back that fails, or that comes back without what the log answered before, leaves the
/// log's history as it was.
// verifies: REQ-MDL-8, NFR-5, INV-7
#[test]
fn a_store_that_cant_be_read_back_leaves_the_entry_with_its_writer() {
    let store = Faulty::default();
    let mut run = Run::new(store.clone());
    let items = drains();
    store.plan(&[Some(Fault::After)]);
    assert_eq!(
        run.append(items[0].clone()),
        Err(AppendFailure::Unavailable)
    );
    store.unreadable.set(true);
    assert_eq!(
        run.append(items[0].clone()),
        Err(AppendFailure::Unavailable)
    );
    store.unreadable.set(false);
    let receipt = run.append(items[0].clone()).unwrap();
    assert_eq!(receipt.index.get(), 0);
    assert_eq!(store.records().len(), 1);
    assert_eq!(run.log.len(), 1);

    // After another failed write, a store read back without a record the log answered is no history of it: the
    // append is unavailable, and the log's own history stays as it was, whole.
    run.append(items[1].clone()).unwrap();
    store.plan(&[Some(Fault::After)]);
    assert_eq!(
        run.append(items[2].clone()),
        Err(AppendFailure::Unavailable)
    );
    let whole = store.records();
    *store.inner.borrow_mut() = MemoryStore::new();
    for record in &whole[1..] {
        store.inner.borrow_mut().append(record).unwrap();
    }
    let before = run.log.entries().to_vec();
    assert_eq!(
        run.append(items[3].clone()),
        Err(AppendFailure::Unavailable)
    );
    assert_eq!(run.log.entries(), &before[..]);
    store.unreadable.set(true);
    assert_eq!(
        run.append(items[3].clone()),
        Err(AppendFailure::Unavailable)
    );
    assert_eq!(run.log.entries(), &before[..]);
    // The store whole again, the log takes the record the failed write left and goes on.
    store.unreadable.set(false);
    *store.inner.borrow_mut() = MemoryStore::new();
    for record in &whole {
        store.inner.borrow_mut().append(record).unwrap();
    }
    assert_eq!(run.append(items[2].clone()).unwrap().index.get(), 2);
    assert_eq!(run.append(items[3].clone()).unwrap().index.get(), 3);
    assert_eq!(store.records().len(), 4);
}

/// The evidence store's stand-in as a reopened log reads it, with some statements gone and some altered under their
/// commitments.
struct Altered {
    inner: KeptInMemory,
    gone: Vec<Value>,
    altered: Vec<Value>,
}

impl KeptStatements for Altered {
    fn kept(
        &mut self,
        commitment: &rutent_contracts::module_interface::Digest,
    ) -> Result<Option<rutent_contracts::core_internal::KeptStatement>, AppendFailure> {
        let named = serde_json::to_value(commitment).unwrap();
        if self.gone.contains(&named) {
            return Ok(None);
        }
        let mut kept = self.inner.kept(commitment)?;
        if self.altered.contains(&named)
            && let Some(k) = kept.as_mut()
        {
            let mut value = serde_json::to_value(&*k).unwrap();
            value["bytes"] = base64url_of(b"{\"other\":\"bytes\"}").into();
            *k = serde_json::from_value(value).unwrap();
        }
        Ok(kept)
    }
}

/// The log's register comes back from its records only with every advertisement they hold: a log reopened with an
/// evidence store that lost a recorded advertisement, or keeps other bytes under its commitment, is refused, rather
/// than coming back with a rotation lost (a key retired by a later advertisement signing again) or with first trust
/// let in anew. Opened with the store it ran with, the rotation holds. A stated entry whose kept bytes don't bear out
/// its commitment is refused on append too.
// verifies: REQ-AUD-2, THR-4, REQ-AUD-13
#[test]
fn reopening_refuses_an_advertisement_the_evidence_store_no_longer_keeps_whole() {
    let store = Faulty::default();
    let mut run = Run::new(store.clone());
    run.grant("us-bank", GRANTED_AT);
    let first = run
        .advertise(
            "us-bank",
            &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
        )
        .unwrap();
    // The rotation: a fresh statements key, the old one dropped.
    let fresh = signing_key("us-bank's rotated statements key", 8);
    let mut ad: Value = serde_json::from_slice(&advertised(
        "us-bank",
        &advertising_key("us-bank"),
        "us-bank",
    ))
    .unwrap();
    ad["made_at"] = "2026-11-06T01:00:00Z".into();
    ad["statement"]["keys"] = json!([
        {"scheme": "ed25519", "public_key": public_key(&advertising_key("us-bank")).as_str(), "purpose": "advertising"},
        {"scheme": "ed25519", "public_key": public_key(&fresh).as_str(), "purpose": "statements"},
    ]);
    sign_value(&mut ad, &advertising_key("us-bank"));
    let rotation = run
        .advertise("us-bank", &serde_json::to_vec(&ad).unwrap())
        .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-re-1",
        "2026-11-06T15:04:05Z"
    ));
    let commitment = |r: &Receipt| match run.at(r) {
        Entry::Stated(s) => serde_json::to_value(&s.commitment).unwrap(),
        Entry::Verified(_) => panic!("an advertisement is a stated entry"),
    };
    let (first, rotation) = (commitment(&first), commitment(&rotation));
    let refused = Err(AppendFailure::Refused(Violation(ADVERTISEMENT_NOT_KEPT)));
    let opened = |gone: Vec<Value>, altered: Vec<Value>| {
        let kept = Altered {
            inner: run.kept.clone(),
            gone,
            altered,
        };
        AuditLog::open(settings(), store.clone(), kept).map(|_| ())
    };
    assert_eq!(
        AuditLog::open(settings(), store.clone(), KeptInMemory::new()).map(|_| ()),
        refused
    );
    assert_eq!(opened(vec![rotation.clone()], vec![]), refused);
    assert_eq!(opened(vec![first.clone()], vec![]), refused);
    assert_eq!(opened(vec![], vec![rotation.clone()]), refused);
    assert_eq!(opened(vec![], vec![]), Ok(()));

    // Reopened with the store it ran with, the retired key still signs nothing, and a replay of the first
    // advertisement, public and validly signed, brings it back no more than it did live.
    let mut reopened = Run::with(store.clone(), run.kept.clone());
    reopened.salts = run.salts;
    assert!(!answered(
        &mut reopened,
        "us-bank",
        &statements_key("us-bank"),
        "tx-re-2",
        "2026-11-06T15:04:05Z"
    ));
    reopened
        .advertise_at(
            "us-bank",
            &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
            "2026-11-06T02:00:00Z",
        )
        .unwrap();
    assert!(!answered(
        &mut reopened,
        "us-bank",
        &statements_key("us-bank"),
        "tx-re-3",
        "2026-11-06T15:04:05Z"
    ));
    assert!(answered(
        &mut reopened,
        "us-bank",
        &fresh,
        "tx-re-4",
        "2026-11-06T15:04:05Z"
    ));

    // Live, a statement whose kept bytes don't bear out its commitment is refused.
    let kept = KeptInMemory::new();
    let mut claim = question_of("tx-re-5");
    claim["attribution"] = json!({"signature": "unverified", "claimed_by": actor(OPERATOR, OPERATOR, "operator", "component")});
    claim["commitment"] =
        serde_json::to_value(kept.keep(b"{\"some\":\"statement\"}", [9; 32])).unwrap();
    let altered = Altered {
        inner: kept,
        gone: vec![],
        altered: vec![claim["commitment"].clone()],
    };
    let mut log = AuditLog::open(settings(), MemoryStore::new(), altered).unwrap();
    let claim: Entry = serde_json::from_value(claim).unwrap();
    assert_eq!(
        log.append(&GeneralEntry::try_from(claim).unwrap()),
        Err(AppendFailure::Refused(Violation(KEPT_BYTES_DIFFER)))
    );
    assert!(log.is_empty());
}

/// A grant's window runs on the log's own clock, not on the time orchestration writes in the effect: a revocation
/// dated far from the log's clock is refused, so it can't be put off to 2100 nor brought forward, and the grant
/// stays in force; and a tuple's changes run in the log's order, so a re-grant dated back behind a revocation is
/// refused and reopens nothing.
// verifies: REQ-AUD-2, REQ-CAT-2, THR-3
#[test]
fn a_catalogue_change_dated_off_the_logs_clock_or_behind_its_tuple_is_refused() {
    let mut run = Run::new(MemoryStore::new());
    run.grant("us-bank", GRANTED_AT);
    run.advertise(
        "us-bank",
        &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
    )
    .unwrap();
    // A revocation dated 2100, appended when the log's clock says 2026-11-07.
    set_now("2026-11-07T00:00:00Z");
    let before = run.log.len();
    assert_eq!(
        run.try_change_catalogue("us-bank", "revoke_grant", "2100-01-01T00:00:00Z"),
        Err(AppendFailure::Refused(Violation(CHANGE_OFF_THE_LOGS_CLOCK)))
    );
    // Only policy's decision is recorded; its effect isn't.
    assert_eq!(run.log.len(), before + 1);
    assert!(answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-c-1",
        "2026-11-06T15:04:05Z"
    ));
    // A revocation on the log's clock holds.
    run.revoke("us-bank", "2026-11-07T00:00:00Z");
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-c-2",
        "2026-11-07T00:00:01Z"
    ));
    // A re-grant dated back behind the revocation, on a clock that agrees with it, is still refused.
    set_now("2026-11-06T12:00:00Z");
    assert_eq!(
        run.try_change_catalogue("us-bank", "grant", "2026-11-06T12:00:00Z"),
        Err(AppendFailure::Refused(Violation(
            CHANGE_BEFORE_ITS_TUPLES_LAST
        )))
    );
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-c-3",
        "2026-11-07T00:00:01Z"
    ));
    // A re-grant dated after it reopens the window from its own time.
    run.grant("us-bank", "2026-11-08T00:00:00Z");
    assert!(answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-c-4",
        "2026-11-08T00:00:01Z"
    ));
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-c-5",
        "2026-11-07T12:00:00Z"
    ));
}

/// The catalogue's rules on what an advertisement declares hold in the log's register too: a successor naming
/// another publisher, one offering no way to follow an outcome, and a first one declaring a private node another
/// publisher declared each change no key; the provider's own, as the catalogue has it, signs in its key.
// verifies: REQ-AUD-2, REQ-CAT-20, REQ-CAT-16
#[test]
fn an_advertisement_the_catalogue_would_refuse_changes_no_key() {
    let mut run = Run::new(MemoryStore::new());
    for p in ["us-bank", "fx-desk"] {
        run.grant(p, GRANTED_AT);
    }
    run.advertise(
        "us-bank",
        &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
    )
    .unwrap();
    let listed = |key: &Ed25519Key, purpose: &str| json!({"scheme": "ed25519", "public_key": public_key(key).as_str(), "purpose": purpose});
    let fresh = signing_key("us-bank's key under another publisher", 11);
    let successor = |made_at: &str, edit: &dyn Fn(&mut Value)| {
        let mut ad: Value = serde_json::from_slice(&advertised(
            "us-bank",
            &advertising_key("us-bank"),
            "us-bank",
        ))
        .unwrap();
        ad["made_at"] = made_at.into();
        ad["statement"]["keys"] = json!([
            listed(&advertising_key("us-bank"), "advertising"),
            listed(&fresh, "statements")
        ]);
        edit(&mut ad);
        sign_value(&mut ad, &advertising_key("us-bank"));
        serde_json::to_vec(&ad).unwrap()
    };
    // Another publisher.
    run.advertise(
        "us-bank",
        &successor("2026-11-06T01:00:00Z", &|ad| {
            // Its ledger renamed too, so no node of another publisher's stands in the way.
            let renamed = ad
                .to_string()
                .replace("us-bank-ledger", "us-bank-new-ledger");
            *ad = serde_json::from_str(&renamed).unwrap();
            ad["statement"]["publisher"] = "fx-desk".into();
        }),
    )
    .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &fresh,
        "tx-p-1",
        "2026-11-06T15:04:05Z"
    ));
    // Nor does a statement of us-bank's stating for the publisher the refused successor named verify.
    let question = run.claim(
        &question_of("tx-p-1b"),
        &operator_key(),
        json!({"kind": "balance_verification", "transaction": "tx-p-1b"}),
    );
    run.append(question).unwrap();
    let mut answer = question_answer("us-bank", "tx-p-1b");
    answer["attribution"]["who"]["party"] = "fx-desk".into();
    let answer = run.claim(
        &answer,
        &fresh,
        json!({"kind": "balance_verification", "transaction": "tx-p-1b"}),
    );
    let receipt = run.append(answer).unwrap();
    assert!(!verified_at(&run, &receipt));
    // No way to follow an outcome.
    run.advertise(
        "us-bank",
        &successor("2026-11-06T02:00:00Z", &|ad| {
            ad["statement"]["declarations"]["tracking"] = json!([])
        }),
    )
    .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &fresh,
        "tx-p-2",
        "2026-11-06T15:04:05Z"
    ));
    // fx-desk's first advertisement, declaring us-bank's ledger.
    let mut ad: Value = serde_json::from_slice(&advertised(
        "fx-desk",
        &advertising_key("fx-desk"),
        "fx-desk",
    ))
    .unwrap();
    let own = serde_json::to_string(&ad)
        .unwrap()
        .replace("fx-desk-ledger", "us-bank-ledger");
    ad = serde_json::from_str(&own).unwrap();
    sign_value(&mut ad, &advertising_key("fx-desk"));
    run.advertise("fx-desk", &serde_json::to_vec(&ad).unwrap())
        .unwrap();
    assert!(!answered(
        &mut run,
        "fx-desk",
        &statements_key("fx-desk"),
        "tx-p-3",
        "2026-11-06T15:04:05Z"
    ));
    // The provider's own successor and first advertisement, as the catalogue has them.
    run.advertise("us-bank", &successor("2026-11-06T03:00:00Z", &|_| {}))
        .unwrap();
    assert!(answered(
        &mut run,
        "us-bank",
        &fresh,
        "tx-p-5",
        "2026-11-06T15:04:05Z"
    ));
    run.advertise(
        "fx-desk",
        &advertised("fx-desk", &advertising_key("fx-desk"), "fx-desk"),
    )
    .unwrap();
    assert!(answered(
        &mut run,
        "fx-desk",
        &statements_key("fx-desk"),
        "tx-p-6",
        "2026-11-06T15:04:05Z"
    ));
}

/// Names an item only for a type that doesn't implement [`Log`]: for one that does, the two impls make the name
/// ambiguous and the test doesn't build.
trait NotLog<A> {
    fn holds() {}
}
impl<T: ?Sized> NotLog<()> for T {}
impl<T: ?Sized + Log> NotLog<u8> for T {}

/// The same for `Clone`.
trait NotClone<A> {
    fn holds() {}
}
impl<T: ?Sized> NotClone<()> for T {}
impl<T: Clone> NotClone<u8> for T {}

/// Policy's handle on a shared log takes policy's decisions and nothing else: it is no general handle and can't be
/// copied, so the one handed to the policy engine is the only one.
// verifies: REQ-AUZ-1, DEC-2
#[test]
fn policys_handle_writes_only_decisions() {
    type Shared = AuditLog<MemoryStore, KeptInMemory>;
    <Policy<Shared> as NotLog<_>>::holds();
    <Policy<Shared> as NotClone<_>>::holds();
    let log = Rc::new(RefCell::new(
        AuditLog::open(settings(), MemoryStore::new(), KeptInMemory::new()).unwrap(),
    ));
    let mut policy = Policy::new(&log);
    let decision: DecisionEntry =
        serde_json::from_value(valid("verified-entry-of-a-decision")).unwrap();
    assert_eq!(policy.append_decision(&decision).unwrap().index.get(), 0);
    let record: VerifiedEntry =
        serde_json::from_value(valid("verified-entry-of-a-holding-check")).unwrap();
    assert!(DecisionEntry::try_from(record).is_err());
    assert_eq!(log.borrow().len(), 1);
}

/// `maker`'s answer under a fresh question with transaction `transaction`, stating for `party`, signed by `key`:
/// whether the log recorded it verified.
fn answered_for<S: Store>(
    run: &mut Run<S>,
    maker: &str,
    party: &str,
    key: &Ed25519Key,
    transaction: &str,
) -> bool {
    let question = run.claim(
        &question_of(transaction),
        &operator_key(),
        json!({"kind": "balance_verification", "transaction": transaction}),
    );
    run.append(question).unwrap();
    let mut answer = question_answer(maker, transaction);
    answer["attribution"]["who"]["party"] = party.into();
    let answer = run.claim(
        &answer,
        key,
        json!({"kind": "holding_balance", "transaction": transaction}),
    );
    let receipt = run.append(answer).unwrap();
    verified_at(run, &receipt)
}

/// First trust in a provider is its grant's, never a setting: an advertisement is trusted on a grant only where
/// it is signed with the grant's advertising keys and made after the grant's effect; a grant counts only once its
/// effect is done, so one whose effect failed, or has no result yet, trusts nothing; and a later grant naming a new
/// key lets a provider that lost its own back in, with an advertisement signed by the new key and made after it.
// verifies: REQ-AUD-2, REQ-AUD-9, REQ-CAT-2, THR-4
#[test]
fn first_trust_is_the_grants_for_advertisements_made_after_it() {
    let mut run = Run::new(MemoryStore::new());
    let us_bank = |key: &Ed25519Key, made_at: &str, statements: &Ed25519Key| {
        let mut ad: Value = serde_json::from_slice(&advertised("us-bank", key, "us-bank")).unwrap();
        ad["made_at"] = made_at.into();
        ad["statement"]["keys"] = json!([
            {"scheme": "ed25519", "public_key": public_key(key).as_str(), "purpose": "advertising"},
            {"scheme": "ed25519", "public_key": public_key(statements).as_str(), "purpose": "statements"},
        ]);
        sign_value(&mut ad, key);
        serde_json::to_vec(&ad).unwrap()
    };
    // Granted at 2026-11-06T01:00:00Z: an advertisement made before it rides on nothing.
    run.grant("us-bank", "2026-11-06T01:00:00Z");
    let early = us_bank(
        &advertising_key("us-bank"),
        "2026-11-06T00:59:59Z",
        &statements_key("us-bank"),
    );
    run.advertise_at("us-bank", &early, "2026-11-06T01:00:01Z")
        .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-f-1",
        "2026-11-06T15:04:05Z"
    ));
    let made = us_bank(
        &advertising_key("us-bank"),
        "2026-11-06T01:00:02Z",
        &statements_key("us-bank"),
    );
    run.advertise("us-bank", &made).unwrap();
    assert!(answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-f-2",
        "2026-11-06T15:04:05Z"
    ));

    // us-bank loses its advertising key: a grant naming a new one lets it back in, for an advertisement made
    // after that grant and signed by the new key, though no successor of the old.
    let new_key = signing_key("us-bank's new advertising key", 12);
    let new_statements = signing_key("us-bank's new statements key", 13);
    run.grant_keys(
        "us-bank",
        std::slice::from_ref(&new_key),
        "2026-11-06T03:00:00Z",
    );
    let before = us_bank(&new_key, "2026-11-06T02:59:59Z", &new_statements);
    run.advertise_at("us-bank", &before, "2026-11-06T03:00:01Z")
        .unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &new_statements,
        "tx-f-3",
        "2026-11-06T15:04:05Z"
    ));
    let after = us_bank(&new_key, "2026-11-06T03:00:02Z", &new_statements);
    run.advertise("us-bank", &after).unwrap();
    assert!(answered(
        &mut run,
        "us-bank",
        &new_statements,
        "tx-f-4",
        "2026-11-06T15:04:05Z"
    ));

    // fx-desk's grant counts only once its effect is done.
    set_now(GRANTED_AT);
    let pending = run
        .try_change_catalogue("fx-desk", "grant", GRANTED_AT)
        .unwrap();
    let fx = |made_at: &str| {
        let mut ad: Value = serde_json::from_slice(&advertised(
            "fx-desk",
            &advertising_key("fx-desk"),
            "fx-desk",
        ))
        .unwrap();
        ad["made_at"] = made_at.into();
        sign_value(&mut ad, &advertising_key("fx-desk"));
        serde_json::to_vec(&ad).unwrap()
    };
    run.advertise("fx-desk", &fx("2026-11-06T00:00:00Z"))
        .unwrap();
    assert!(!answered(
        &mut run,
        "fx-desk",
        &statements_key("fx-desk"),
        "tx-f-5",
        "2026-11-06T15:04:05Z"
    ));
    run.end(&pending, "failed", GRANTED_AT);
    run.advertise("fx-desk", &fx("2026-11-06T00:01:00Z"))
        .unwrap();
    assert!(!answered(
        &mut run,
        "fx-desk",
        &statements_key("fx-desk"),
        "tx-f-6",
        "2026-11-06T15:04:05Z"
    ));
    run.grant("fx-desk", "2026-11-05T01:00:00Z");
    run.advertise("fx-desk", &fx("2026-11-06T00:02:00Z"))
        .unwrap();
    assert!(answered(
        &mut run,
        "fx-desk",
        &statements_key("fx-desk"),
        "tx-f-7",
        "2026-11-06T15:04:05Z"
    ));
}

/// A module the operator publishes states for the operator's party and signs with the statement key of a release of
/// it the log accepted, once the acceptance is done: its statements verify under that key alone, stating for the
/// operator's party alone. No advertisement can make it a provider's: one for a module the log holds an accepted
/// release of, and no advertisement, changes nothing.
// verifies: REQ-AUD-2, REQ-CAT-23, REQ-MDL-1, REQ-AUD-15
#[test]
fn a_module_the_operator_publishes_signs_with_its_accepted_releases_key() {
    const MODULE: &str = "public-node-holding";
    let release = signing_key("the public node's holding module, 1.0.0", 14);
    let mut run = Run::new(MemoryStore::new());
    // Before any release is accepted, nothing signs for it.
    assert!(!answered_for(
        &mut run, MODULE, OPERATOR, &release, "tx-m-1"
    ));
    let accept = json!({"kind": "change_catalogue", "change": "accept_module", "provider": MODULE, "version": "1.0.0", "statement_key": public_key(&release).as_str()});
    set_now("2026-11-06T00:00:00Z");
    let effect = run.try_change(accept, "2026-11-06T00:00:00Z").unwrap();
    // Accepted but not yet done: still nothing.
    assert!(!answered_for(
        &mut run, MODULE, OPERATOR, &release, "tx-m-2"
    ));
    run.end(&effect, "done", "2026-11-06T00:00:00Z");
    assert!(answered_for(&mut run, MODULE, OPERATOR, &release, "tx-m-3"));
    assert!(!answered_for(
        &mut run,
        MODULE,
        OPERATOR,
        &another_key(),
        "tx-m-4"
    ));
    assert!(!answered_for(&mut run, MODULE, MODULE, &release, "tx-m-5"));
    // A grant and an advertisement for it, as if a provider's: changes nothing.
    run.grant(MODULE, GRANTED_AT);
    run.advertise(
        MODULE,
        &advertised(MODULE, &advertising_key(MODULE), MODULE),
    )
    .unwrap();
    assert!(!answered_for(
        &mut run,
        MODULE,
        MODULE,
        &statements_key(MODULE),
        "tx-m-6"
    ));
    assert!(answered_for(&mut run, MODULE, OPERATOR, &release, "tx-m-7"));
}

/// The log takes an advertisement only as received within the skew of its own clock, and a catalogue change's
/// effect only as dated within it: dated ahead of the clock, each is refused; behind it, as an append retried
/// after the skew is, each is unavailable, so its writer keeps it rather than drop it as broken.
// verifies: REQ-AUD-2, REQ-MDL-8, INV-7
#[test]
fn an_advertisement_or_a_catalogue_change_off_the_logs_clock_is_not_taken() {
    let mut run = Run::new(MemoryStore::new());
    run.grant("us-bank", GRANTED_AT);
    let ad = advertised("us-bank", &advertising_key("us-bank"), "us-bank");
    set_now("2026-11-06T00:00:01Z");
    assert_eq!(
        run.advertise_raw("us-bank", &ad, "2026-11-06T01:00:01Z"),
        Err(AppendFailure::Refused(Violation(
            ADVERTISEMENT_OFF_THE_LOGS_CLOCK
        )))
    );
    set_now("2026-11-06T02:00:00Z");
    assert_eq!(
        run.advertise_raw("us-bank", &ad, "2026-11-06T00:00:01Z"),
        Err(AppendFailure::Unavailable)
    );
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-k-1",
        "2026-11-06T15:04:05Z"
    ));
    // A revocation retried an hour after its time.
    set_now("2026-11-07T01:00:00Z");
    assert_eq!(
        run.try_change_catalogue("us-bank", "revoke_grant", "2026-11-07T00:00:00Z"),
        Err(AppendFailure::Unavailable)
    );
    run.advertise("us-bank", &ad).unwrap();
    assert!(answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-k-2",
        "2026-11-06T15:04:05Z"
    ));
}

/// `provider`'s advertisement made at `made_at`, listing `keys`, signed with `signer`.
fn ad_of(provider: &str, signer: &Ed25519Key, made_at: &str, keys: Value) -> Vec<u8> {
    let mut ad: Value = serde_json::from_slice(&advertised(provider, signer, provider)).unwrap();
    ad["made_at"] = made_at.into();
    ad["statement"]["keys"] = keys;
    sign_value(&mut ad, signer);
    serde_json::to_vec(&ad).unwrap()
}

fn listed(key: &Ed25519Key, purpose: &str) -> Value {
    json!({"scheme": "ed25519", "public_key": public_key(key).as_str(), "purpose": purpose})
}

/// A grant trusts only the keys it names, only for what follows it, and only once done: a first advertisement
/// listing an advertising key the grant doesn't name is refused, so that key signs in no successor; a granted key
/// the accepted advertisement no longer lists signs in nothing on the old grant; an advertisement on a new grant
/// is still no older than the one accepted last; a revocation whose effect failed takes nothing back; and an
/// advertisement received while the grant is revoked signs nothing in.
// verifies: REQ-AUD-2, REQ-CAT-2, THR-4
#[test]
fn a_grant_trusts_only_the_keys_it_names_once_done() {
    let mut run = Run::new(MemoryStore::new());
    // us-bank's first, signed with its granted key, but listing another advertising key besides.
    run.grant("us-bank", GRANTED_AT);
    let extra = signing_key("an advertising key no grant names", 15);
    let extra_statements = signing_key("statements under the ungranted key", 16);
    let first = ad_of(
        "us-bank",
        &advertising_key("us-bank"),
        ADVERTISED_AT,
        json!([
            listed(&advertising_key("us-bank"), "advertising"),
            listed(&extra, "advertising"),
            listed(&statements_key("us-bank"), "statements")
        ]),
    );
    run.advertise("us-bank", &first).unwrap();
    let next = ad_of(
        "us-bank",
        &extra,
        "2026-11-06T01:00:00Z",
        json!([
            listed(&extra, "advertising"),
            listed(&extra_statements, "statements")
        ]),
    );
    run.advertise("us-bank", &next).unwrap();
    assert!(!answered(
        &mut run,
        "us-bank",
        &statements_key("us-bank"),
        "tx-g2-1",
        "2026-11-06T15:04:05Z"
    ));
    assert!(!answered(
        &mut run,
        "us-bank",
        &extra_statements,
        "tx-g2-2",
        "2026-11-06T15:04:05Z"
    ));

    // fx-desk's grant names two keys; its accepted advertisement lists one, so the other signs in nothing.
    let second = signing_key("fx-desk's second granted key", 17);
    let second_statements = signing_key("statements under fx-desk's second key", 18);
    run.grant_keys(
        "fx-desk",
        &[advertising_key("fx-desk"), second.clone()],
        GRANTED_AT,
    );
    run.advertise(
        "fx-desk",
        &ad_of(
            "fx-desk",
            &advertising_key("fx-desk"),
            ADVERTISED_AT,
            json!([
                listed(&advertising_key("fx-desk"), "advertising"),
                listed(&statements_key("fx-desk"), "statements")
            ]),
        ),
    )
    .unwrap();
    run.advertise(
        "fx-desk",
        &ad_of(
            "fx-desk",
            &second,
            "2026-11-06T01:00:00Z",
            json!([
                listed(&second, "advertising"),
                listed(&second_statements, "statements")
            ]),
        ),
    )
    .unwrap();
    assert!(!answered(
        &mut run,
        "fx-desk",
        &second_statements,
        "tx-g2-3",
        "2026-11-06T15:04:05Z"
    ));
    assert!(answered(
        &mut run,
        "fx-desk",
        &statements_key("fx-desk"),
        "tx-g2-4",
        "2026-11-06T15:04:05Z"
    ));

    // bank-co's first is dated four minutes ahead of its receipt; a new grant comes in between, and an
    // advertisement on it made before the accepted one is no newer.
    run.grant("bank-co", "2026-11-06T00:00:00Z");
    run.advertise_at(
        "bank-co",
        &ad_of(
            "bank-co",
            &advertising_key("bank-co"),
            "2026-11-06T01:04:00Z",
            json!([
                listed(&advertising_key("bank-co"), "advertising"),
                listed(&statements_key("bank-co"), "statements")
            ]),
        ),
        "2026-11-06T01:00:00Z",
    )
    .unwrap();
    let renewed = signing_key("bank-co's renewed key", 19);
    let renewed_statements = signing_key("bank-co's renewed statements key", 20);
    run.grant_keys(
        "bank-co",
        std::slice::from_ref(&renewed),
        "2026-11-06T01:01:00Z",
    );
    let renewed_ad = |made_at: &str| {
        ad_of(
            "bank-co",
            &renewed,
            made_at,
            json!([
                listed(&renewed, "advertising"),
                listed(&renewed_statements, "statements")
            ]),
        )
    };
    run.advertise("bank-co", &renewed_ad("2026-11-06T01:02:00Z"))
        .unwrap();
    assert!(!answered(
        &mut run,
        "bank-co",
        &renewed_statements,
        "tx-g2-5",
        "2026-11-06T15:04:05Z"
    ));
    run.advertise("bank-co", &renewed_ad("2026-11-06T01:05:00Z"))
        .unwrap();
    assert!(answered(
        &mut run,
        "bank-co",
        &renewed_statements,
        "tx-g2-6",
        "2026-11-06T15:04:05Z"
    ));

    // kyc-check's revocation fails: it takes nothing back.
    run.grant("kyc-check", GRANTED_AT);
    run.advertise(
        "kyc-check",
        &advertised("kyc-check", &advertising_key("kyc-check"), "kyc-check"),
    )
    .unwrap();
    set_now("2026-11-07T00:00:00Z");
    let revoked = run
        .try_change_catalogue("kyc-check", "revoke_grant", "2026-11-07T00:00:00Z")
        .unwrap();
    run.end(&revoked, "failed", "2026-11-07T00:00:00Z");
    assert!(answered(
        &mut run,
        "kyc-check",
        &statements_key("kyc-check"),
        "tx-g2-7",
        "2026-11-07T00:00:01Z"
    ));

    // A successor received while kyc-check's grant is revoked signs nothing in, even once it is granted again.
    run.revoke("kyc-check", "2026-11-07T00:00:02Z");
    let later = signing_key("kyc-check's key while revoked", 21);
    run.advertise(
        "kyc-check",
        &ad_of(
            "kyc-check",
            &advertising_key("kyc-check"),
            "2026-11-07T01:00:00Z",
            json!([
                listed(&advertising_key("kyc-check"), "advertising"),
                listed(&later, "statements")
            ]),
        ),
    )
    .unwrap();
    run.grant("kyc-check", "2026-11-08T00:00:00Z");
    assert!(!answered(
        &mut run,
        "kyc-check",
        &later,
        "tx-g2-8",
        "2026-11-08T00:00:01Z"
    ));
}

/// A granted provider whose release is done before its advertisement is recorded is no module of the operator's:
/// its party is unknown, and its statements unverified, until its own advertisement is accepted, which no release
/// stands in the way of; then its statements key signs, for its publisher, and its release's key signs nothing as
/// the operator's.
// verifies: REQ-AUD-2, REQ-CAT-23, REQ-MDL-1
#[test]
fn a_granted_providers_early_release_doesnt_make_it_the_operators() {
    let release = signing_key("us-bank's module, 1.0.0", 22);
    let mut run = Run::new(MemoryStore::new());
    run.grant("us-bank", GRANTED_AT);
    run.carry_out(
        json!({"kind": "change_catalogue", "change": "accept_module", "provider": "us-bank", "version": "1.0.0", "statement_key": public_key(&release).as_str()}),
        "2026-11-05T12:00:00Z",
    );
    assert!(!answered_for(
        &mut run, "us-bank", OPERATOR, &release, "tx-e-1"
    ));
    assert!(!answered_for(
        &mut run,
        "us-bank",
        "us-bank",
        &statements_key("us-bank"),
        "tx-e-2"
    ));
    run.advertise(
        "us-bank",
        &advertised("us-bank", &advertising_key("us-bank"), "us-bank"),
    )
    .unwrap();
    assert!(answered_for(
        &mut run,
        "us-bank",
        "us-bank",
        &statements_key("us-bank"),
        "tx-e-3"
    ));
    assert!(!answered_for(
        &mut run, "us-bank", OPERATOR, &release, "tx-e-4"
    ));
}
