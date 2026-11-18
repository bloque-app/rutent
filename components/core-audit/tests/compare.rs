//! The comparisons that only detect (`T-022-05`): what one provider reports sending against what the next reports
//! receiving, each holding account's balance against what its orders hold there, and, across orders, the value
//! evidenced at nodes against the balances. A difference raises to the operator and changes nothing else.
//!
//! The report pair runs on the real log (`AuditLog`, on a memory store) replaying the contract's coherent world up to
//! the pair, so every entry the checks write is admitted by the citation table; the holding checks run on the same
//! log, standing alone. The operator's channel is a recorder.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::{
    Ed25519Key, advertisement, advertising_key, another_key, public_key, sign_value, signing_key,
    statements_key,
};
use rutent_contracts::core_internal::calls::{
    AppendFailure, Comparisons, DecisionEntry, Delivery, GeneralEntry, Log, Operator, PolicyLog,
    is_decision,
};
use rutent_contracts::core_internal::{
    Attribution, Difference, Direction, Entry, HoldingIncomparable, HoldingReading, Raise, Receipt,
    ReportPair, Reported, VerifiedEntry, What, Word,
};
use rutent_contracts::provider_role::{ADVERTISEMENT_SHAPE, Denomination, Deployment, VERSION};
use rutent_contracts::router_api::{Amount, Id, Timestamp};
use rutent_core_audit::compare::{
    Compare, Evidenced, HoldingVerdict, ReportsVerdict, check_holding, compare_reports,
    value_evidenced_against_balances,
};
use rutent_core_audit::log::{
    AuditLog, Clock, General, KeptInMemory, MemoryStore, Settings, Store,
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
    case_entries("world--in_dest")
}

/// The admission case `name` as one log: its history, each entry with its placeholder receipt, then its entry.
fn case_entries(name: &str) -> Vec<(Value, Value)> {
    let case = fixture("admission", name);
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

    /// Runs `entries` as the world is run: the providers admitted, then each entry, the statements signed by their
    /// makers' own keys but at the places `forge` names.
    fn entries(
        &mut self,
        entries: Vec<(Value, Value)>,
        forge: &[usize],
    ) -> Vec<(usize, Result<Receipt, AppendFailure>)> {
        self.admit_providers();
        let mut answers = Vec::new();
        for (place, (placeholder, entry)) in entries.into_iter().enumerate() {
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

// --- The checks under test ---------------------------------------------------------------------------------------

/// The operator's channel: records each raise, and answers as planned.
#[derive(Clone)]
struct Desk {
    raised: Rc<RefCell<Vec<Raise>>>,
    answer: Rc<Cell<Delivery>>,
}

impl Desk {
    fn new() -> Self {
        Desk {
            raised: Rc::default(),
            answer: Rc::new(Cell::new(Delivery::Delivered)),
        }
    }

    fn raised(&self) -> Vec<Raise> {
        self.raised.borrow().clone()
    }
}

impl Operator for Desk {
    fn raise(&mut self, raise: &Raise) -> Delivery {
        self.raised.borrow_mut().push(raise.clone());
        self.answer.get()
    }
}

type Shared = Rc<RefCell<AuditLog<MemoryStore, KeptInMemory>>>;

/// The checks over `log`, on the test's clock.
fn checks(
    log: &Shared,
    desk: &Desk,
) -> Compare<AuditLog<MemoryStore, KeptInMemory>, Desk, TestClock> {
    Compare::new(
        Compare::<AuditLog<MemoryStore, KeptInMemory>, Desk, TestClock>::core_audit(id("rutent")),
        General::new(log),
        desk.clone(),
        TestClock,
    )
}

/// A log holding nothing, for the checks that cite nothing.
fn bare() -> Shared {
    Rc::new(RefCell::new(
        AuditLog::open(settings(), MemoryStore::new(), KeptInMemory::new()).unwrap(),
    ))
}

/// The real log after the world up to its first report pair, and that pair as the world gives it, its receipts
/// rewritten to the log's.
fn world_to_the_pair() -> (Shared, ReportPair) {
    let mut run = Run::new(MemoryStore::new());
    let at = place_of("report_pair", 0);
    run.entries(world().into_iter().take(at).collect(), &[]);
    let mut pair = world()[at].1["what"]["pair"].clone();
    run.rewrite(&mut pair);
    (
        Rc::new(RefCell::new(run.log)),
        serde_json::from_value(pair).unwrap(),
    )
}

/// What the log holds, by the kind of each entry and its outcome.
fn kinds(log: &Shared) -> Vec<String> {
    log.borrow()
        .entries()
        .iter()
        .map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry { what, outcome, .. }) => {
                let call = serde_json::to_value(what).unwrap()["call"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let status = serde_json::to_value(outcome).unwrap()["status"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                format!("{call}:{status}")
            }
            Entry::Stated(_) => "stated".to_owned(),
        })
        .collect()
}

fn holding(name: &str, balance: &str, held: &[(&str, &str)], word: &str) -> HoldingReading {
    let mut value = valid("holding-reading-at-a-private-node");
    value["account"] = name.into();
    value["balance"] = balance.into();
    value["word"] = word.into();
    value["held_for"] = held
        .iter()
        .map(|(order, amount)| json!({"order": order, "amount": amount}))
        .collect();
    serde_json::from_value(value).unwrap()
}

/// A seeded xorshift generator: a failing case names its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// An amount of up to 70 digits, so sums stay inside what an amount can state.
    fn amount(&mut self) -> String {
        let digits = 1 + self.below(30) as usize;
        let text: String = (0..digits)
            .map(|_| char::from(b'0' + self.below(10) as u8))
            .collect();
        let trimmed = text.trim_start_matches('0');
        if trimmed.is_empty() {
            "0".to_owned()
        } else {
            trimmed.to_owned()
        }
    }
}

fn asset(code: &str, decimals: u8) -> Value {
    json!({"id": code, "decimals": decimals})
}

// --- REQ-ORD-9 -----------------------------------------------------------------------------------------------------

/// A difference between the two reports raises the order to the operator, and the log gains the pair's record and
/// the raise's, and nothing else; two reports that agree raise nothing.
// verifies: REQ-ORD-9
#[test]
fn difference_between_reports_raises_and_changes_nothing_else() {
    let (log, pair) = world_to_the_pair();
    let desk = Desk::new();
    let mut compare = checks(&log, &desk);
    let before = log.borrow().entries().to_vec();

    // Reports that agree: the pair is recorded, and nothing is raised.
    let agreeing = compare.report_pair(&pair).unwrap();
    assert!(desk.raised().is_empty());
    assert_eq!(log.borrow().len(), before.len() + 1);
    assert_eq!(kinds(&log).last().unwrap(), "report_pair:observed");

    // Reports that differ: the pair is recorded, the difference is raised, delivered, and the raise recorded.
    let mut differing = pair.clone();
    differing.received.amount = Amount::new("999000000").unwrap();
    let receipt = compare.report_pair(&differing).unwrap();
    assert_eq!(
        &kinds(&log)[before.len()..],
        ["report_pair:observed", "report_pair:observed", "raise:done"]
    );
    let raised = desk.raised();
    assert_eq!(raised.len(), 1);
    assert_eq!(
        raised[0].difference,
        Difference::Reports {
            order: differing.order.clone(),
            segment: differing.segment.clone(),
            sent: differing.sent.amount.clone(),
            received: differing.received.amount.clone(),
            asset: differing.sent.asset.clone(),
        }
    );
    // It rests on the pair's record and on the two statements, and the answer is the pair's receipt.
    assert_eq!(
        raised[0].cites,
        [
            receipt.clone(),
            differing.sent.statement.clone(),
            differing.received.statement.clone()
        ]
    );
    assert_ne!(receipt, agreeing);
    // What the log held before is as it was.
    assert_eq!(&log.borrow().entries()[..before.len()], &before[..]);

    // A raise the operator didn't take is recorded as failed, not lost.
    desk.answer.set(Delivery::Undelivered);
    differing.received.amount = Amount::new("998000000").unwrap();
    compare.report_pair(&differing).unwrap();
    assert_eq!(kinds(&log).last().unwrap(), "raise:failed");
    assert_eq!(desk.raised().len(), 2);

    // Reports of two assets, or of one asset in two units (`THR-107`), can't be compared: that is raised too, with
    // each figure as it was stated, even where the two amounts read alike, and nothing else changes.
    desk.answer.set(Delivery::Delivered);
    let decimals = pair.sent.asset.decimals;
    for received in [
        asset("EURC", decimals),
        asset(&pair.sent.asset.id, decimals + 2),
    ] {
        let mut apart = pair.clone();
        apart.received.asset = serde_json::from_value(received).unwrap();
        let before = log.borrow().entries().to_vec();
        let receipt = compare.report_pair(&apart).unwrap();
        assert_eq!(
            &kinds(&log)[before.len()..],
            ["report_pair:observed", "raise:done"],
            "{:?}",
            apart.received.asset
        );
        let raise = desk.raised().pop().unwrap();
        assert_eq!(
            raise.difference,
            Difference::ReportsNotComparable {
                order: apart.order.clone(),
                segment: apart.segment.clone(),
                sent: Reported {
                    amount: apart.sent.amount.clone(),
                    asset: apart.sent.asset.clone(),
                },
                received: Reported {
                    amount: apart.received.amount.clone(),
                    asset: apart.received.asset.clone(),
                },
            }
        );
        assert_eq!(
            raise.cites,
            [
                receipt,
                apart.sent.statement.clone(),
                apart.received.statement.clone()
            ]
        );
        assert_eq!(last_raise(&log), Some(raise));
        assert_eq!(&log.borrow().entries()[..before.len()], &before[..]);
    }
    // Two differences and two reports apart, each raised once.
    assert_eq!(desk.raised().len(), 4);
}

/// `pair` under the terms `terms`, as JSON reads them (`REQ-CAT-11`).
fn under(pair: &ReportPair, terms: Value) -> ReportPair {
    let mut value = serde_json::to_value(pair).unwrap();
    value["terms"] = terms;
    serde_json::from_value(value).unwrap()
}

/// `pair` with `received` as the receiving party's amount.
fn receiving(pair: &ReportPair, received: &str) -> ReportPair {
    let mut out = pair.clone();
    out.received.amount = Amount::new(received).unwrap();
    out
}

/// A tuple that fills in increments: reports that differ by less than one increment, either way, raise nothing, and
/// one increment or more is raised as any difference is (ADR-0009: the tolerance is the provider's own declared
/// increment, never a fixed one). A tuple that declares none is compared exactly.
// verifies: REQ-ORD-9, REQ-CAT-11
#[test]
fn reports_differing_by_less_than_one_increment_raise_nothing() {
    let filled: ReportPair =
        serde_json::from_value(valid("report-pair-filled-in-increments")).unwrap();
    assert_eq!(
        filled.terms.increment.as_ref().map(|i| i.amount().as_str()),
        Some("100")
    );
    assert_eq!(filled.sent.amount.as_str(), "1000000000");
    for within in ["999999950", "999999901", "1000000000", "1000000099"] {
        assert_eq!(
            compare_reports(&receiving(&filled, within)),
            ReportsVerdict::Match,
            "{within} is within one increment of what was sent"
        );
    }
    for past in ["999999900", "1000000100", "1"] {
        let pair = receiving(&filled, past);
        assert_eq!(
            compare_reports(&pair),
            ReportsVerdict::Differs(Difference::Reports {
                order: pair.order.clone(),
                segment: pair.segment.clone(),
                sent: pair.sent.amount.clone(),
                received: pair.received.amount.clone(),
                asset: pair.sent.asset.clone(),
            }),
            "{past} is one increment or more away"
        );
    }
    // No increment declared: exact.
    let exact = under(&filled, json!({"reporting": {"kind": "net"}}));
    assert!(matches!(
        compare_reports(&receiving(&exact, "999999999")),
        ReportsVerdict::Differs(_)
    ));

    // Through the detector, on the real log: within one increment the pair is recorded and nothing is raised.
    let (log, pair) = world_to_the_pair();
    let desk = Desk::new();
    let mut compare = checks(&log, &desk);
    let sent = pair.sent.amount.as_str().to_owned();
    let increment = format!("1{}", "0".repeat(sent.len() - 1));
    let near = receiving(
        &under(
            &pair,
            json!({"increment": increment, "reporting": {"kind": "net"}}),
        ),
        &format!("{}", sent.parse::<u128>().unwrap() - 1),
    );
    let before = log.borrow().len();
    compare.report_pair(&near).unwrap();
    assert_eq!(kinds(&log)[before..], ["report_pair:observed"]);
    assert!(desk.raised().is_empty());
}

/// A tuple that reports gross states what it sent with its charge in it; the next party's receipt is what arrived,
/// the charge kept back. The comparison takes the charge off the gross figure first, so a gross report and a net
/// receipt of the same delivery agree, and a difference names the figure compared, net of the charge.
// verifies: REQ-ORD-9
#[test]
fn gross_and_net_reports_of_one_segment_are_compared_like_with_like() {
    let gross: ReportPair = serde_json::from_value(valid("report-pair-reported-gross")).unwrap();
    assert_eq!(gross.sent.amount.as_str(), "1000005000");
    assert_eq!(gross.received.amount.as_str(), "1000000000");
    // 1 000 005 000 sent gross, a charge of 5 000, 1 000 000 000 received: one delivery, alike.
    assert_eq!(compare_reports(&gross), ReportsVerdict::Match);
    // Read net, the same figures differ by the charge.
    let as_net = under(&gross, json!({"reporting": {"kind": "net"}}));
    assert!(matches!(
        compare_reports(&as_net),
        ReportsVerdict::Differs(_)
    ));
    // A gross report whose receipt is short of it net is raised, naming the figure compared.
    let short = receiving(&gross, "999990000");
    assert_eq!(
        compare_reports(&short),
        ReportsVerdict::Differs(Difference::Reports {
            order: short.order.clone(),
            segment: short.segment.clone(),
            sent: Amount::new("1000000000").unwrap(),
            received: Amount::new("999990000").unwrap(),
            asset: short.sent.asset.clone(),
        })
    );
    // A receipt of the gross figure itself is over what was delivered: the charge didn't reach it.
    assert!(matches!(
        compare_reports(&receiving(&gross, "1000005000")),
        ReportsVerdict::Differs(_)
    ));
    // Gross with an increment: within one increment of the net figure.
    let both = under(
        &gross,
        json!({"increment": "10", "reporting": {"kind": "gross", "charge": "5000"}}),
    );
    assert_eq!(
        compare_reports(&receiving(&both, "1000000009")),
        ReportsVerdict::Match
    );
    assert!(matches!(
        compare_reports(&receiving(&both, "1000000010")),
        ReportsVerdict::Differs(_)
    ));
}

/// `verdict` as it would be for the same figures under `order` and `segment`.
fn relabelled(verdict: &ReportsVerdict, order: &Id, segment: &Id) -> ReportsVerdict {
    let relabel = |difference: &Difference| match difference.clone() {
        Difference::Reports {
            sent,
            received,
            asset,
            ..
        } => Difference::Reports {
            order: order.clone(),
            segment: segment.clone(),
            sent,
            received,
            asset,
        },
        Difference::ReportsNotComparable { sent, received, .. } => {
            Difference::ReportsNotComparable {
                order: order.clone(),
                segment: segment.clone(),
                sent,
                received,
            }
        }
        other => panic!("not a report's difference: {other:?}"),
    };
    match verdict {
        ReportsVerdict::Match => ReportsVerdict::Match,
        ReportsVerdict::Differs(d) => ReportsVerdict::Differs(relabel(d)),
        ReportsVerdict::NotComparable(d) => ReportsVerdict::NotComparable(relabel(d)),
    }
}

/// The comparison depends on the two amounts and the two assets alone: the same pair gives the same verdict, and
/// changing the parties, the statements, the order or the segment changes nothing about it.
// verifies: REQ-ORD-9
#[test]
fn comparison_is_a_pure_function_of_the_two_reports() {
    // Nothing but a function of the pair's data: it coerces to a plain function pointer on shared references.
    let _: fn(&ReportPair) -> ReportsVerdict = compare_reports;
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut matches, mut differs, mut apart, mut units) = (0, 0, 0, 0);
    for case in 0..400 {
        let seed = rng.0;
        let sent = rng.amount();
        let received = match rng.below(3) {
            0 => sent.clone(),
            _ => rng.amount(),
        };
        let mut value = valid("report-pair-of-a-route-segment");
        value["sent"]["amount"] = sent.clone().into();
        value["received"]["amount"] = received.clone().into();
        // Another asset, or the same asset in another unit (`THR-107`): its code alike, its decimals not.
        let other_asset = rng.below(5) == 0;
        if other_asset {
            let decimals = value["sent"]["asset"]["decimals"].as_u64().unwrap() as u8;
            value["received"]["asset"] = if rng.below(2) == 0 {
                asset("EURC", decimals)
            } else {
                units += 1;
                let code = value["sent"]["asset"]["id"].as_str().unwrap().to_owned();
                asset(&code, (decimals + 1 + rng.below(36) as u8) % 37)
            };
        }
        let pair: ReportPair = serde_json::from_value(value.clone()).unwrap();
        let verdict = compare_reports(&pair);

        let expected = if other_asset {
            apart += 1;
            ReportsVerdict::NotComparable(Difference::ReportsNotComparable {
                order: pair.order.clone(),
                segment: pair.segment.clone(),
                sent: Reported {
                    amount: Amount::new(sent).unwrap(),
                    asset: pair.sent.asset.clone(),
                },
                received: Reported {
                    amount: Amount::new(received).unwrap(),
                    asset: pair.received.asset.clone(),
                },
            })
        } else if sent == received {
            matches += 1;
            ReportsVerdict::Match
        } else {
            differs += 1;
            ReportsVerdict::Differs(Difference::Reports {
                order: pair.order.clone(),
                segment: pair.segment.clone(),
                sent: Amount::new(sent).unwrap(),
                received: Amount::new(received).unwrap(),
                asset: pair.sent.asset.clone(),
            })
        };
        assert_eq!(verdict, expected, "case {case}, seed {seed:#x}");

        // The same pair again, and the same amounts under other parties, statements, order and segment.
        assert_eq!(compare_reports(&pair.clone()), verdict, "case {case}");
        let mut moved = pair.clone();
        moved.order = id(&format!("ord-{case}"));
        moved.segment = id(&format!("seg-{case}"));
        moved.sent.party = id("another-sender");
        moved.received.party = id("another-receiver");
        moved.sent.statement.index = serde_json::from_value(json!(900 + case)).unwrap();
        moved.received.statement.index = serde_json::from_value(json!(1_900 + case)).unwrap();
        assert_eq!(
            compare_reports(&moved),
            relabelled(&verdict, &moved.order, &moved.segment),
            "case {case}, seed {seed:#x}"
        );
    }
    // The three outcomes were each drawn, and among the reports apart, one asset in two units.
    assert!(
        matches > 20 && differs > 20 && apart > 20 && units > 10,
        "{matches} {differs} {apart} {units}"
    );
}

/// The pure comparison modules. Each may import the contracts' data types and these siblings, and nothing else.
const PURE: [&str; 3] = ["report", "holding", "evidenced"];

/// `source` without its comment lines, its whitespace folded to single spaces, so a declaration split across lines
/// reads as one, and none left round a path's `::`, a method's `.` or before a `(`, so `super :: x` and
/// `o . raise (r)` read as `super::x` and `o.raise(r)`.
fn flat_code(source: &str) -> String {
    let mut flat = source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for (spaced, tight) in [
        (" ::", "::"),
        (":: ", "::"),
        (" .", "."),
        (". ", "."),
        (" (", "("),
    ] {
        while flat.contains(spaced) {
            flat = flat.replace(spaced, tight);
        }
    }
    flat
}

/// Whether `c` can be part of an identifier.
fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Every place `word` stands as a whole word in `code`.
fn words<'a>(code: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    code.match_indices(word)
        .map(|(at, _)| at)
        .filter(move |&at| {
            let before = code[..at].chars().next_back();
            let after = code[at + word.len()..].chars().next();
            !before.is_some_and(ident) && !after.is_some_and(ident)
        })
}

/// Each `use` declaration of `code`, whatever its visibility and wherever it stands, whole up to its `;`, without
/// the keyword.
fn use_declarations(code: &str) -> Vec<String> {
    words(code, "use")
        .map(|at| {
            let rest = code[at + 3..].trim_start();
            rest[..rest.find(';').unwrap_or(rest.len())].to_owned()
        })
        .collect()
}

/// The first segment of each path `tree` names after `super::`: its own, or that of each top-level item of a braced
/// group.
fn sibling_heads(tree: &str) -> Vec<String> {
    let head = |item: &str| -> String { item.trim().chars().take_while(|c| ident(*c)).collect() };
    let tree = tree.trim();
    let Some(group) = tree.strip_prefix('{') else {
        return vec![head(tree)];
    };
    let (mut heads, mut item, mut depth) = (Vec::new(), String::new(), 0);
    for c in group.chars() {
        match c {
            '}' if depth == 0 => break,
            ',' if depth == 0 => heads.push(head(&std::mem::take(&mut item))),
            '{' => {
                depth += 1;
                item.push(c);
            }
            '}' => {
                depth -= 1;
                item.push(c);
            }
            _ => item.push(c),
        }
    }
    heads.push(head(&item));
    heads.into_iter().filter(|h| !h.is_empty()).collect()
}

/// Why `code`, a pure module's, could reach something that moves value, if it could: each reason found.
fn reach_of(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    for declaration in use_declarations(code) {
        let path = declaration.trim_start_matches("::");
        if let Some(tree) = path.strip_prefix("super::") {
            for head in sibling_heads(tree) {
                if !PURE.contains(&head.as_str()) {
                    found.push(format!(
                        "imports super::{head}, not a pure sibling: use {declaration}"
                    ));
                }
            }
        } else if !path.starts_with("rutent_contracts::") {
            found.push(format!(
                "imports outside the contracts and its siblings: use {declaration}"
            ));
        }
    }
    // A capability, imported or named in place.
    if code.contains("calls::") {
        found.push("names the contract's calls".to_owned());
    }
    for capability in ["Log", "Operator", "Clock", "Comparisons"] {
        if words(code, capability).next().is_some() {
            found.push(format!("names {capability}"));
        }
    }
    // A child module, in a file of its own or inline, which the reader would not see.
    if words(code, "mod").next().is_some() {
        found.push("declares a module".to_owned());
    }
    // A path written in place, outside any `use`.
    for at in words(code, "super") {
        let rest = &code[at..];
        if rest.starts_with("super::super") {
            found.push("names super::super".to_owned());
        } else if let Some(tree) = rest.strip_prefix("super::")
            && !tree.starts_with('{')
        {
            let head: String = tree.chars().take_while(|c| ident(*c)).collect();
            if !PURE.contains(&head.as_str()) {
                found.push(format!("names super::{head}, not a pure sibling"));
            }
        }
    }
    for (at, _) in code.match_indices("rutent_") {
        let name: String = code[at..].chars().take_while(|c| ident(*c)).collect();
        if name != "rutent_contracts" {
            found.push(format!("names the crate {name}"));
        }
    }
    for forbidden in [
        "&mut",
        "unsafe",
        "static ",
        "Cell",
        "Rc<",
        "Arc<",
        "Mutex",
        "dyn ",
        "Box<",
        "thread_local",
        "std::fs",
        "std::net",
        "std::process",
        "std::env",
        "std::time",
        "crate::",
        "extern crate",
        "include!",
        "include_str!",
        "#[path",
        "fn raise",
        ".raise(",
        ".append(",
    ] {
        if code.contains(forbidden) {
            found.push(format!("holds {forbidden:?}"));
        }
    }
    found
}

/// The comparison modules reach nothing that moves value, by what they import and what they can hold: data types of
/// the contracts, their pure siblings, shared references, no handle, no channel, no clock and no interior
/// mutability. Every `use` is read whole, `pub` or not, across its lines; a path written in place is read too; no
/// other crate of the workspace is named. The crate takes no dependency on orchestration, so there is no path to
/// the state machine or the provider system from it.
// verifies: REQ-ORD-9
#[test]
fn comparison_modules_reach_nothing_that_moves_value() {
    let _: fn(&HoldingReading) -> HoldingVerdict = check_holding;
    let _: fn(&[HoldingReading], &[Evidenced]) -> Vec<rutent_core_audit::compare::Finding> =
        value_evidenced_against_balances;
    let manifest = env!("CARGO_MANIFEST_DIR");
    for file in PURE {
        let path = format!("{manifest}/src/compare/{file}.rs");
        let code = flat_code(&std::fs::read_to_string(&path).unwrap());
        let reach = reach_of(&code);
        assert!(reach.is_empty(), "{file}: {}", reach.join("; "));
        // Nor a directory of child modules beside it.
        let children = format!("{manifest}/src/compare/{file}");
        assert!(
            !std::path::Path::new(&children).exists(),
            "{file} has child modules at {children}"
        );
    }
    // The reader reads a sibling's braced group as one sibling.
    assert!(
        reach_of(&flat_code(
            "use super::holding::{HoldingVerdict, judge, sum};"
        ))
        .is_empty()
    );
    // The reader itself is red on each way round it a reviewer found, and on the plain ones.
    for (dodge, sneak) in [
        ("a pub use", "pub use super::super::log::General as Sneak;"),
        ("a pub(crate) use", "pub(crate) use crate::log::General;"),
        (
            "a use across lines",
            "use rutent_contracts::core_internal::{\n    calls::Operator as Sneak,\n};",
        ),
        (
            "a path in a body",
            "fn sneak() { let _ = rutent_prov::catalogue::DEFAULT_CLOCK_SKEW_SECONDS; }",
        ),
        ("the router", "use rutent_router::quote;"),
        ("the state machine", "fn s() { rutent_stm::run(); }"),
        ("the evidence store", "use rutent_evidence as e;"),
        ("the detectors", "use super::detect::Compare;"),
        ("the module's own re-export", "use super::Compare;"),
        (
            "a sibling's parent in place",
            "fn s() { super::super::log::x(); }",
        ),
        ("a log", "use rutent_contracts::core_internal::calls::Log;"),
        (
            "a group reaching past the siblings",
            "use super::{holding::sum, detect::Compare};",
        ),
        (
            "spaces round a path's separators",
            "fn s() -> usize { super :: detect :: NOT_A_GENERAL_ENTRY.len() }",
        ),
        (
            "spaces round crate's",
            "fn s() { let _ = crate :: log :: x; }",
        ),
        (
            "spaces round std's",
            "fn s() { let _ = std :: fs :: read(\"x\"); }",
        ),
        (
            "a capability named in place, called with spaces",
            "fn sneak<O: rutent_contracts::core_internal::calls::Operator>(\n    mut o: O,\n    r: &rutent_contracts::core_internal::Raise,\n) {\n    let _ = o . raise (r);\n}",
        ),
        (
            "a child module",
            "mod sneak;\nfn s() -> usize { sneak::reach().len() }",
        ),
        ("an inline child module", "mod sneak { pub fn x() {} }"),
    ] {
        assert!(
            !reach_of(&flat_code(sneak)).is_empty(),
            "the reader misses {dodge}: {sneak}"
        );
    }
    // No dependency on the parts that move value (`DEC-2`).
    let manifest_text = std::fs::read_to_string(format!("{manifest}/Cargo.toml")).unwrap();
    let dependencies = manifest_text.split("[dev-dependencies]").next().unwrap();
    for part in ["rutent-core-orch", "rutent-core-authz", "rutent-host"] {
        assert!(!dependencies.contains(part), "core-audit depends on {part}");
    }
}

// --- REQ-AUD-15 ----------------------------------------------------------------------------------------------------

/// The call of the raise a log holds last, if its last entry is one.
fn last_raise(log: &Shared) -> Option<Raise> {
    match log.borrow().entries().last() {
        Some((
            _,
            Entry::Verified(VerifiedEntry {
                what: What::Raise { raise },
                ..
            }),
        )) => Some(raise.clone()),
        _ => None,
    }
}

/// An account whose balance matches what its orders hold raises nothing; one short and one over each raise the
/// operator, by how much, and the log gains the check's record and the raise's and nothing else.
// verifies: AC-39, REQ-AUD-15
#[test]
fn holding_account_short_or_over_raises_the_operator_and_changes_nothing() {
    let log = bare();
    let desk = Desk::new();
    let mut compare = checks(&log, &desk);
    let held = [("ord-17", "1000000000"), ("ord-19", "2000000000")];

    compare
        .holding_reading(&holding("hold-match", "3000000000", &held, "checked"))
        .unwrap();
    assert!(desk.raised().is_empty());
    assert_eq!(kinds(&log), ["holding_check:observed"]);

    // Short by 5 000 000.
    let before = log.borrow().entries().to_vec();
    let receipt = compare
        .holding_reading(&holding("hold-short", "2995000000", &held, "checked"))
        .unwrap();
    assert_eq!(
        &kinds(&log)[before.len()..],
        ["holding_check:observed", "raise:done"]
    );
    let raise = desk.raised().pop().unwrap();
    assert_eq!(raise.cites, [receipt]);
    assert_eq!(
        serde_json::to_value(&raise.difference).unwrap(),
        json!({"kind": "holding", "account": "hold-short", "direction": "short", "by": "5000000",
               "asset": asset("USDC", 6), "word": "checked"})
    );
    assert_eq!(&log.borrow().entries()[..before.len()], &before[..]);

    // Over by 7 000 000, and over by its whole balance where no order holds anything there.
    compare
        .holding_reading(&holding("hold-over", "3007000000", &held, "checked"))
        .unwrap();
    assert!(matches!(
        desk.raised().last().unwrap().difference,
        Difference::Holding { direction: Direction::Over, ref by, .. } if by.amount().as_str() == "7000000"
    ));
    compare
        .holding_reading(&holding("hold-empty", "42", &[], "checked"))
        .unwrap();
    assert!(matches!(
        desk.raised().last().unwrap().difference,
        Difference::Holding { direction: Direction::Over, ref by, .. } if by.amount().as_str() == "42"
    ));
    assert_eq!(desk.raised().len(), 3);
    assert!(last_raise(&log).is_some());
    // An account with nothing held and nothing in it matches.
    compare
        .holding_reading(&holding("hold-nil", "0", &[], "checked"))
        .unwrap();
    assert_eq!(desk.raised().len(), 3);

    // The check, over seeded readings: short, over or matching by exactly the difference of the balance and the sum.
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let (mut short, mut over, mut equal) = (0, 0, 0);
    for case in 0..400 {
        let seed = rng.0;
        let holds: Vec<(String, String)> = (0..rng.below(5))
            .map(|i| {
                (
                    format!("ord-{i}"),
                    (1 + rng.below(10_000_000_000)).to_string(),
                )
            })
            .collect();
        let sum: u128 = holds.iter().map(|(_, a)| a.parse::<u128>().unwrap()).sum();
        let balance = match rng.below(3) {
            0 => sum,
            1 => sum + 1 + u128::from(rng.below(1_000_000)),
            _ => sum.saturating_sub(1 + u128::from(rng.below(1_000_000))),
        };
        let held: Vec<(&str, &str)> = holds
            .iter()
            .map(|(o, a)| (o.as_str(), a.as_str()))
            .collect();
        let reading = holding("hold-p", &balance.to_string(), &held, "checked");
        let verdict = check_holding(&reading);
        match balance.cmp(&sum) {
            std::cmp::Ordering::Equal => {
                equal += 1;
                assert_eq!(
                    verdict,
                    HoldingVerdict::Match,
                    "case {case}, seed {seed:#x}"
                );
            }
            order => {
                let (direction, distance) = if order.is_lt() {
                    short += 1;
                    (Direction::Short, sum - balance)
                } else {
                    over += 1;
                    (Direction::Over, balance - sum)
                };
                let HoldingVerdict::Differs(Difference::Holding {
                    direction: d, by, ..
                }) = verdict
                else {
                    panic!("case {case}, seed {seed:#x}: {verdict:?}");
                };
                assert_eq!(
                    (d, by.amount().as_str()),
                    (direction, distance.to_string().as_str())
                );
            }
        }
    }
    assert!(
        short > 20 && over > 20 && equal > 20,
        "{short} {over} {equal}"
    );

    // A sum past what an amount states whose difference from the balance an amount does state is that short; an
    // over never passes the bound, the balance being an amount.
    let max = "9".repeat(78);
    let edge = holding(
        "hold-edge",
        &max,
        &[("ord-1", &max), ("ord-2", "1")],
        "checked",
    );
    let HoldingVerdict::Differs(Difference::Holding {
        direction: Direction::Short,
        ref by,
        ..
    }) = check_holding(&edge)
    else {
        panic!("{:?}", check_holding(&edge));
    };
    assert_eq!(by.amount().as_str(), "1");
    let under = holding(
        "hold-under",
        &max,
        &[("ord-1", "1"), ("ord-2", "2")],
        "checked",
    );
    let HoldingVerdict::Differs(Difference::Holding {
        direction: Direction::Over,
        ref by,
        ..
    }) = check_holding(&under)
    else {
        panic!("{:?}", check_holding(&under));
    };
    assert_eq!(by.amount().as_str(), format!("{}6", "9".repeat(77)));

    // A short past what an amount states can't be stated: it is raised as not comparable.
    let huge = "9".repeat(78);
    let reading = holding(
        "hold-huge",
        "5",
        &[("ord-1", &huge), ("ord-2", &huge)],
        "checked",
    );
    let past = Difference::HoldingNotComparable {
        account: id("hold-huge"),
        word: Some(Word::Checked),
        asset: Some(serde_json::from_value(asset("USDC", 6)).unwrap()),
        reason: HoldingIncomparable::SumPastTheBound,
    };
    assert_eq!(
        check_holding(&reading),
        HoldingVerdict::NotComparable(past.clone())
    );
    // A sum past what an amount states is past every balance: it is raised as not comparable, citing the check.
    let before = log.borrow().entries().to_vec();
    let receipt = compare.holding_reading(&reading).unwrap();
    assert_eq!(
        &kinds(&log)[before.len()..],
        ["holding_check:observed", "raise:done"]
    );
    let raise = desk.raised().pop().unwrap();
    assert_eq!(
        (raise.difference.clone(), raise.cites.clone()),
        (past, vec![receipt])
    );
    assert_eq!(
        serde_json::to_value(&raise.difference).unwrap(),
        json!({"kind": "holding_not_comparable", "account": "hold-huge", "word": "checked",
               "asset": asset("USDC", 6), "reason": "sum_past_the_bound"})
    );
    assert_eq!(last_raise(&log), Some(raise));
    assert_eq!(&log.borrow().entries()[..before.len()], &before[..]);
}

/// A private node's balance is its publisher's word: a difference carries that label and so does the check's
/// record, and the log holds no balance.
// verifies: AC-39, REQ-AUD-15
#[test]
fn private_nodes_balance_is_labelled_its_publishers_word() {
    let log = bare();
    let desk = Desk::new();
    let mut compare = checks(&log, &desk);
    let held = [("ord-17", "1000000000"), ("ord-19", "2000000000")];

    compare
        .holding_reading(&holding("hold-private", "3000000000", &held, "publisher"))
        .unwrap();
    assert!(
        desk.raised().is_empty(),
        "a match is not raised at a private node either"
    );

    compare
        .holding_reading(&holding(
            "hold-private-short",
            "2999999991",
            &held,
            "publisher",
        ))
        .unwrap();
    let raise = desk.raised().pop().unwrap();
    assert!(matches!(
        raise.difference,
        Difference::Holding {
            word: Word::Publisher,
            direction: Direction::Short,
            ..
        }
    ));
    compare
        .holding_reading(&holding(
            "hold-public-short",
            "2999999991",
            &held,
            "checked",
        ))
        .unwrap();
    assert!(matches!(
        desk.raised().pop().unwrap().difference,
        Difference::Holding {
            word: Word::Checked,
            ..
        }
    ));

    // The check's record says whose word it was, and holds no balance (`REQ-AUD-6`).
    let entries = log.borrow().entries().to_vec();
    let checks: Vec<&Entry> = entries
        .iter()
        .map(|(_, e)| e)
        .filter(|e| {
            matches!(
                e,
                Entry::Verified(VerifiedEntry {
                    what: What::HoldingCheck { .. },
                    ..
                })
            )
        })
        .collect();
    assert_eq!(checks.len(), 3);
    assert!(matches!(
        checks[0],
        Entry::Verified(VerifiedEntry {
            what: What::HoldingCheck {
                word: Word::Publisher,
                ..
            },
            ..
        })
    ));
    let text = serde_json::to_string(&entries).unwrap();
    for balance in ["3000000000", "2999999991"] {
        assert!(
            !text.contains(balance),
            "the log holds a balance: {balance}"
        );
    }
}

// --- INV-1 ---------------------------------------------------------------------------------------------------------

/// Across orders, the value evidenced at nodes equals the holding accounts' balances: accounts whose balances equal
/// what is evidenced are not raised, and one that differs is, by exactly the difference.
// verifies: INV-1
#[test]
fn value_evidenced_at_nodes_equals_the_holding_accounts_balances() {
    let mut rng = Rng(0x0123_4567_89ab_cdef);
    let (mut raised_total, mut clean_total) = (0, 0);
    for case in 0..400 {
        let seed = rng.0;
        let accounts = 1 + rng.below(5) as usize;
        let mut evidenced = Vec::new();
        let mut readings = Vec::new();
        let mut sums = Vec::new();
        for a in 0..accounts {
            let account = format!("hold-{case}-{a}");
            let mut sum: u128 = 0;
            for o in 0..rng.below(5) {
                let amount = 1 + u128::from(rng.below(1_000_000_000));
                sum += amount;
                evidenced.push(Evidenced {
                    account: id(&account),
                    order: id(&format!("ord-{case}-{o}")),
                    asset: serde_json::from_value(asset("USDC", 6)).unwrap(),
                    amount: Amount::new(amount.to_string()).unwrap(),
                    evidence: stand_in(o),
                });
            }
            // What the account holds for its orders is what its module reports; the evidence is Core's own.
            let word = if rng.below(2) == 0 {
                "checked"
            } else {
                "publisher"
            };
            readings.push(holding(&account, &sum.to_string(), &[], word));
            sums.push(sum);
        }
        // Every balance equals the sum evidenced at its node: no difference, however the orders divide it.
        // (The readings above hold nothing per order, so rebuild them with one hold carrying the whole sum.)
        let readings_matching: Vec<HoldingReading> = readings
            .iter()
            .zip(&sums)
            .map(|(r, sum)| {
                let mut r = r.clone();
                r.balance = Amount::new(sum.to_string()).unwrap();
                r
            })
            .collect();
        let findings = value_evidenced_against_balances(&readings_matching, &evidenced);
        assert_eq!(findings.len(), accounts, "case {case}, seed {seed:#x}");
        assert!(
            findings.iter().all(|f| f.verdict == HoldingVerdict::Match),
            "case {case}, seed {seed:#x}: {findings:?}"
        );
        clean_total += 1;

        // One account's balance drifts from what is evidenced: that account differs, by exactly the drift.
        let which = rng.below(accounts as u64) as usize;
        let drift = 1 + u128::from(rng.below(1_000_000));
        let down = rng.below(2) == 0 && sums[which] >= drift;
        let mut drifted = readings_matching.clone();
        let balance = if down {
            sums[which] - drift
        } else {
            sums[which] + drift
        };
        drifted[which].balance = Amount::new(balance.to_string()).unwrap();
        let findings = value_evidenced_against_balances(&drifted, &evidenced);
        for (i, finding) in findings.iter().enumerate() {
            if i == which {
                let HoldingVerdict::Differs(Difference::Holding {
                    direction,
                    by,
                    word,
                    account,
                    ..
                }) = &finding.verdict
                else {
                    panic!("case {case}, seed {seed:#x}: {finding:?}");
                };
                assert_eq!(
                    *direction,
                    if down {
                        Direction::Short
                    } else {
                        Direction::Over
                    }
                );
                assert_eq!(by.amount().as_str(), drift.to_string());
                assert_eq!(*word, drifted[which].word);
                assert_eq!(account, &drifted[which].account);
                raised_total += 1;
            } else {
                assert_eq!(
                    finding.verdict,
                    HoldingVerdict::Match,
                    "case {case}, seed {seed:#x}"
                );
            }
        }
    }
    assert_eq!((clean_total, raised_total), (400, 400));

    // Evidence at an account with no reading, and in another asset or unit than the account's, can't be compared.
    let reading = holding("hold-a", "10", &[], "checked");
    let usdc: Denomination = serde_json::from_value(asset("USDC", 6)).unwrap();
    for other in [asset("EURC", 6), asset("USDC", 2)] {
        let other: Denomination = serde_json::from_value(other).unwrap();
        let item = |account: &str, asset: &Denomination| Evidenced {
            asset: asset.clone(),
            ..item_amount(account, "10")
        };
        let findings = value_evidenced_against_balances(
            std::slice::from_ref(&reading),
            &[
                item("hold-a", &other),
                item("hold-unread", &usdc),
                item("hold-unread", &usdc),
            ],
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(
            findings[0].verdict,
            HoldingVerdict::NotComparable(Difference::HoldingNotComparable {
                account: id("hold-a"),
                word: Some(Word::Checked),
                asset: Some(usdc.clone()),
                reason: HoldingIncomparable::EvidencedInAnotherAsset,
            })
        );
        assert_eq!(findings[1].account, id("hold-unread"));
        assert_eq!(
            findings[1].verdict,
            HoldingVerdict::NotComparable(Difference::HoldingNotComparable {
                account: id("hold-unread"),
                word: None,
                asset: None,
                reason: HoldingIncomparable::NoReading,
            })
        );
    }
    // Evidence summed past what an amount states, its difference from the balance an amount does state.
    let max = "9".repeat(78);
    let findings = value_evidenced_against_balances(
        &[holding("hold-edge", &max, &[], "checked")],
        &[
            item_amount("hold-edge", &max),
            item_amount("hold-edge", "1"),
        ],
    );
    assert!(
        matches!(&findings[0].verdict, HoldingVerdict::Differs(Difference::Holding { direction: Direction::Short, by, .. }) if by.amount().as_str() == "1"),
        "{findings:?}"
    );

    // Through the checks: each account that differs or can't be compared is raised, citing its check where it was
    // read and the entries its value is evidenced by, and nothing else changes.
    let log = bare();
    let desk = Desk::new();
    let mut compare = checks(&log, &desk);
    // Stand-ins for the log's entries the evidence rests on: any entry of the log may be cited.
    let rests: Vec<Receipt> = (0..6)
        .map(|i| {
            compare
                .holding_reading(&holding(&format!("hold-rest-{i}"), "0", &[], "checked"))
                .unwrap()
        })
        .collect();
    assert!(desk.raised().is_empty());
    let on = |account: &str, amount: &str, rest: usize| Evidenced {
        evidence: rests[rest].clone(),
        ..item_amount(account, amount)
    };
    let readings = [
        holding("hold-1", "30", &[], "checked"),
        holding("hold-2", "55", &[], "publisher"),
        holding("hold-3", "7", &[], "checked"),
        holding("hold-4", "7", &[], "publisher"),
    ];
    let evidence = [
        on("hold-1", "10", 0),
        on("hold-1", "20", 0),
        on("hold-2", "60", 1),
        on("hold-3", "7", 2),
        Evidenced {
            asset: serde_json::from_value(asset("USDC", 2)).unwrap(),
            ..on("hold-4", "7", 3)
        },
        on("hold-unread", "4", 4),
        on("hold-unread", "5", 5),
        on("hold-unread", "6", 4),
    ];
    let before = log.borrow().len();
    let checked = compare
        .evidenced_against_balances(&readings, &evidence)
        .unwrap();
    assert_eq!(checked.len(), 2);
    assert_eq!(
        &kinds(&log)[before..],
        [
            "holding_check:observed",
            "raise:done",
            "holding_check:observed",
            "raise:done",
            "raise:done"
        ]
    );
    let raised = desk.raised();
    assert_eq!(raised.len(), 3);
    assert_eq!(raised[0].cites, [checked[0].clone(), rests[1].clone()]);
    assert!(matches!(
        raised[0].difference,
        Difference::Holding { direction: Direction::Short, word: Word::Publisher, ref by, .. } if by.amount().as_str() == "5"
    ));
    assert_eq!(raised[1].cites, [checked[1].clone(), rests[3].clone()]);
    assert_eq!(
        raised[1].difference,
        Difference::HoldingNotComparable {
            account: id("hold-4"),
            word: Some(Word::Publisher),
            asset: Some(serde_json::from_value(asset("USDC", 6)).unwrap()),
            reason: HoldingIncomparable::EvidencedInAnotherAsset,
        }
    );
    assert_eq!(raised[2].cites, [rests[4].clone(), rests[5].clone()]);
    assert_eq!(
        raised[2].difference,
        Difference::HoldingNotComparable {
            account: id("hold-unread"),
            word: None,
            asset: None,
            reason: HoldingIncomparable::NoReading,
        }
    );
    assert_eq!(last_raise(&log).as_ref(), raised.last());

    // A raise whose record the log refuses for good, here one citing what isn't an entry of the log, is answered
    // refused, not unavailable, so its caller doesn't append the comparison again and raise it again without end.
    let before = log.borrow().len();
    let nowhere = Evidenced {
        evidence: stand_in(9_999),
        ..item_amount("hold-nowhere", "1")
    };
    let answer = compare.evidenced_against_balances(&[], &[nowhere]);
    assert!(
        matches!(answer, Err(AppendFailure::Refused(_))),
        "{answer:?}"
    );
    assert_eq!(
        log.borrow().len(),
        before,
        "the refused raise is not recorded"
    );

    // A raise refused for good doesn't stop the accounts after it (the re-review's probe): each is still compared,
    // checked and raised, and the first refusal is answered once all are done.
    let good = compare
        .holding_reading(&holding("hold-rest-x", "0", &[], "checked"))
        .unwrap();
    let (before, raised_before) = (log.borrow().len(), desk.raised().len());
    let readings = [
        holding("hold-x", "30", &[], "checked"),
        holding("hold-y", "30", &[], "checked"),
    ];
    let evidence = [
        Evidenced {
            evidence: stand_in(9_999),
            ..item_amount("hold-x", "10")
        },
        Evidenced {
            evidence: good.clone(),
            ..item_amount("hold-y", "10")
        },
    ];
    let answer = compare.evidenced_against_balances(&readings, &evidence);
    assert!(
        matches!(answer, Err(AppendFailure::Refused(_))),
        "{answer:?}"
    );
    assert_eq!(
        desk.raised().len(),
        raised_before + 2,
        "hold-y's over is raised after hold-x's refusal"
    );
    assert_eq!(
        &kinds(&log)[before..],
        [
            "holding_check:observed",
            "holding_check:observed",
            "raise:done"
        ]
    );
    let after = last_raise(&log).unwrap();
    assert!(
        matches!(&after.difference, Difference::Holding { account, direction: Direction::Over, .. } if account == &id("hold-y"))
    );
    assert_eq!(after.cites[1], good);
}

/// A receipt of no entry of any test's log, at `index`: for the pure comparison, which reads no receipt.
fn stand_in(index: u64) -> Receipt {
    serde_json::from_value(json!({"log": LOG, "index": index, "entry": {"algorithm": "sha-256", "value": "ab".repeat(32)}}))
        .unwrap()
}

fn item_amount(account: &str, amount: &str) -> Evidenced {
    Evidenced {
        account: id(account),
        order: id("ord-1"),
        asset: serde_json::from_value(asset("USDC", 6)).unwrap(),
        amount: Amount::new(amount).unwrap(),
        evidence: stand_in(0),
    }
}
