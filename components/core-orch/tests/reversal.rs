//! Reversals and ending with fakes (`T-020-12`): the router-api fixture's route across three providers, a withdrawal
//! at `reserve-bank-co`, an exchange at `exchange-a` inside its private `ledger-in` and a deposit at `reserve-wallet`,
//! run through the segments over a fake platform whose providers answer as each test scripts. The simulated
//! providers can't run it: their routes stay at one reserve, whose withdrawal is cancelled when it fails and whose
//! deposit owes no reversal, so no segment of theirs can owe one. Everything the reversals write is replayed through
//! core-internal's `attribute` and `admit_entry` over the log before it, so the log is seen to admit it.

mod snapshot_support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::collections::BTreeMap;

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::core_internal::calls::{
    AppendFailure, Decisions, Evidence, Fetched, GeneralEntry, Kept, KeptStatements, is_decision, Log, Reads,
    Signatures, StatementKeys,
};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Afterwards, Answer, Decision, DecisionRequest, DeploymentOperator,
    Destination, Effect, Entry, KeptStatement, PermissionChange, ReadAnswer, ReadCheck,
    ReadOutcome, Receipt, Role, RouteEnd, SignedWith, Through, What, admit_entry, attribute,
};
use rutent_contracts::module_interface::{Digest, Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{
    Denomination, EvidenceGrade, Operation, OuterNode, PublicKey, RESULT_SHAPE,
    REVERSAL_CONFIRMATION_SHAPE, Statement, signing_input, to_canonical,
};
use rutent_contracts::router_api::{DecisionRecord, Id, PolicyBounds, Timestamp};
use rutent_contracts::state_machine::{self as stm, OrderState, RecordedInput, SegmentState};
use rutent_core_orch::recalc::evidenced;
use rutent_core_orch::reversal::{
    Asked, Asking, At, Carrier, Confirmation, Moving, NotCounted, Parts as ReversalParts,
    ReversalError, Reversals, Word,
};
use rutent_core_orch::rounds::{Fresh, Issued, OperatorKey, Providers};
use rutent_core_orch::segments::{
    Answered, Graded, Grading, Live, Modules, NoProxy, Parts, ProvidersWord, Ran, SegmentOutcome,
    Segments, SegmentsError, ToGrade,
};
use rutent_core_orch::snapshot::{Clock, Exposures, Reading};
use rutent_core_orch::stuck::{Stuck, StuckError};
use rutent_prov::catalogue::{Catalogue, Endpoint, Instant, Observation, Publisher, TupleKey};
use rutent_prov::module::lifecycle::{LifecycleRefusal, SegmentRecord};
use rutent_prov::module::types::Events;
use rutent_prov::proxy::RouteNotNamed;
use serde_json::{Value, json};

const ORDER: &str = "ord-1";
const PART: &str = "p1";
const SEGMENTS: [&str; 3] = ["p1-s1", "p1-s2", "p1-s3"];
const RESERVE: &str = "reserve-bank-co";
const EXCHANGE: &str = "exchange-a";
const WALLET: &str = "reserve-wallet";
/// What the exchange took in, in COP: what the withdrawal handed on.
const EXCHANGED: &str = "99900000";
/// What the withdrawal took in, in COP.
const WITHDRAWN: &str = "100000000";
const NOW: u64 = 1_794_200_000;

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn read(path: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts")
        .join(path);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

// Who acts --------------------------------------------------------------------------------------------------------

fn core() -> Actor {
    Actor {
        party: id("operator"),
        acts: id("rutent-core"),
        role: Role::Core,
        kind: ActorKind::Component,
    }
}

fn operator() -> Actor {
    Actor {
        party: id("operator"),
        acts: id("ops-lead"),
        role: Role::Operator,
        kind: ActorKind::Person,
    }
}

fn customer() -> Actor {
    Actor {
        party: id("customer-1"),
        acts: id("customer-1"),
        role: Role::Customer,
        kind: ActorKind::Person,
    }
}

// Fakes -----------------------------------------------------------------------------------------------------------

struct TestClock(Cell<Reading>);

impl Clock for TestClock {
    fn now(&self) -> Reading {
        self.0.get()
    }
}

struct TestKey(SigningKey);

impl OperatorKey for TestKey {
    fn key(&self) -> PublicKey {
        snapshot_support::public_key(&self.0)
    }

    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(input).to_bytes())
    }
}

#[derive(Default)]
struct Ids(u64);

impl Fresh for Ids {
    fn id(&mut self) -> Id {
        self.0 += 1;
        id(&format!("id-{:04}", self.0))
    }
}

/// The evidence store: each statement's bytes, under a commitment of its place.
#[derive(Default)]
struct Store(Vec<Vec<u8>>);

fn commitment(place: usize) -> Digest {
    serde_json::from_value(
        json!({ "algorithm": "sha-256", "value": format!("{:064x}", place + 1) }),
    )
    .unwrap()
}

impl Evidence for Store {
    fn keep(&mut self, bytes: &[u8]) -> Result<Kept, AppendFailure> {
        self.0.push(bytes.to_vec());
        Ok(Kept {
            commitment: commitment(self.0.len() - 1),
            restates: None,
        })
    }

    fn fetch(&mut self, _: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Ok(Fetched::Erased)
    }
}

/// The receipt of `entry` at `index`: the receipt rule's digest of its canonical JSON.
fn receipt_of(index: usize, entry: &Entry) -> Receipt {
    let value = rutent_prov::module::wire::sha256(&to_canonical(entry).unwrap());
    serde_json::from_value(json!({
        "log": "log-1",
        "index": index,
        "entry": { "algorithm": "sha-256", "value": value },
    }))
    .unwrap()
}

/// The entries the log holds, shared with policy, whose decisions it records on its own handle.
type Entries = Rc<RefCell<Vec<Entry>>>;

/// A log that keeps every entry as handed in, answering the receipt rule's receipt; while it is `down`, it can't take
/// any, and while it is `refusing`, it refuses each.
#[derive(Default)]
struct KeptLog {
    entries: Entries,
    down: bool,
    refusing: bool,
}

impl KeptLog {
    /// The entries, as appended.
    fn entries(&self) -> std::cell::Ref<'_, Vec<Entry>> {
        self.entries.borrow()
    }

    fn len(&self) -> usize {
        self.entries.borrow().len()
    }
}

/// Appends `entry` to `entries`, answering its receipt.
fn push(entries: &Entries, entry: Entry) -> Receipt {
    let mut entries = entries.borrow_mut();
    let receipt = receipt_of(entries.len(), &entry);
    entries.push(entry);
    receipt
}

impl Log for KeptLog {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        if self.down {
            return Err(AppendFailure::Unavailable);
        }
        if self.refusing {
            return Err(AppendFailure::Refused(
                rutent_contracts::core_internal::Violation("the log refuses it"),
            ));
        }
        Ok(push(&self.entries, entry.entry().clone()))
    }
}

/// Policy that permits every move, its decision recorded in the log on its own handle.
struct Permitting {
    decided: usize,
    log: Entries,
}

impl Decisions for Permitting {
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure> {
        self.decided += 1;
        let decision = id(&format!("dec-{}", self.decided));
        let entry: Entry = serde_json::from_value(json!({
            "kind": "verified",
            "who": { "party": "operator", "acts": "policy", "role": "core", "kind": "component" },
            "what": { "call": "decide", "request": request, "decision": decision },
            "when": { "at": request.at, "source": "core_clock" },
            "outcome": { "status": "permitted" },
        }))
        .expect("a decide entry");
        Ok(Decision {
            request: request.request.clone(),
            decision,
            outcome: Answer::Permit {},
            receipt: push(&self.log, entry),
        })
    }
}

/// Policy's reads: every read allowed, and each check kept.
#[derive(Default)]
struct Allowing(Vec<ReadCheck>);

impl Reads for Allowing {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        self.0.push(check.clone());
        Ok(ReadAnswer {
            request: check.request.clone(),
            outcome: ReadOutcome::Allowed {},
            receipt: receipt_of(200_000 + self.0.len(), &input_entry()),
        })
    }

    fn grant_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn revoke_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}

/// How a provider answers its segment's operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Settle,
    Fail,
    /// The module passed something out and ended with no answer.
    Unknown,
}

/// The platform: each provider answers as scripted, with a result it signs.
struct Platform {
    script: BTreeMap<&'static str, Act>,
}

impl Platform {
    fn settling() -> Self {
        Platform {
            script: BTreeMap::from([
                (RESERVE, Act::Settle),
                (EXCHANGE, Act::Settle),
                (WALLET, Act::Settle),
            ]),
        }
    }

    fn with(mut self, provider: &'static str, act: Act) -> Self {
        self.script.insert(provider, act);
        self
    }
}

/// A result of `kind` by `maker` for `transaction`, its outcome `outcome`, signed by the maker's statements key.
fn result(maker: &str, kind: &str, transaction: &str, outcome: Value) -> Vec<u8> {
    let body = json!({ "kind": kind, "transaction": transaction, "outcome": outcome });
    let envelope = snapshot_support::envelope(
        &snapshot_support::deployment(),
        RESULT_SHAPE,
        maker,
        2_000,
        body,
    );
    snapshot_support::sign(envelope, &snapshot_support::statements_key(maker, 1))
}

fn piece(by: &str) -> Value {
    json!([{ "grade": 3, "reference": format!("{by}:book-1"), "by": by }])
}

impl Modules for Platform {
    fn begin_segment(&mut self, _: &Id, _: &Identifier) -> Result<String, PlatformRefusal> {
        Ok("1.0.0".into())
    }

    fn name_calls(
        &mut self,
        _: &Identifier,
        _: &DecisionRecord,
        _: (usize, usize),
        _: &Identifier,
    ) -> Result<usize, RouteNotNamed> {
        Ok(0)
    }

    fn carry_out(
        &mut self,
        _: &Effect,
        provider: &Id,
        operation: &Statement<Operation>,
    ) -> Answered {
        let op = serde_json::to_value(&operation.statement).unwrap();
        let (kind, transaction) = (
            op["kind"].as_str().unwrap().to_owned(),
            op["transaction"].as_str().unwrap().to_owned(),
        );
        if kind == "reversal" {
            let outcome = json!({
                "status": "succeeded", "node": op["node"], "asset": op["asset"], "amount": op["amount"],
                "evidence": piece(provider.as_str()),
            });
            return Answered::Bytes(result(provider.as_str(), "reversal", &transaction, outcome));
        }
        let act = self.script[provider.as_str()];
        let outcome = match (act, kind.as_str()) {
            (Act::Unknown, _) => return Answered::Refused(PlatformRefusal::OutcomeUnknown),
            (Act::Fail, _) => json!({ "status": "failed", "evidence": piece(provider.as_str()) }),
            (Act::Settle, "withdrawal") => json!({
                "status": "succeeded", "amount_in": WITHDRAWN, "amount_out": EXCHANGED,
                "fee": { "asset": "COP", "amount": "100000" }, "evidence": piece(provider.as_str()),
            }),
            (Act::Settle, "conversion") => json!({
                "status": "succeeded", "amount_in": EXCHANGED, "amount_out": "249250500", "rate": "2.5",
                "fee": { "asset": "COP", "amount": "0" }, "evidence": piece(provider.as_str()),
            }),
            (Act::Settle, _) => json!({
                "status": "succeeded", "amount_in": "249250500", "amount_out": "248250500",
                "fee": { "asset": "USDC", "amount": "1000000" }, "evidence": piece(provider.as_str()),
            }),
        };
        Answered::Bytes(result(provider.as_str(), &kind, &transaction, outcome))
    }

    fn take_events(&mut self) -> Events {
        Events::default()
    }

    fn end_segment(&mut self, _: &Identifier) -> Option<SegmentRecord> {
        None
    }

    fn end_calls(&mut self, _: &Identifier) {}

    fn staged(&self, _: &Id) -> Option<String> {
        None
    }

    fn promote(
        &mut self,
        _: &Effect,
        _: &Id,
        _: &Id,
        _: Instant,
    ) -> Result<String, LifecycleRefusal> {
        Err(LifecycleRefusal::NotStaged)
    }
}

/// The grading: the provider's word for every segment but the final deposit, which closes on its public movement
/// on `polygon`, graded a public record, as `closing::Evidenced` closes one; the grading is `T-020-09`'s, and its own
/// tests show it.
struct PublicDeposit;

impl Grading for PublicDeposit {
    fn grade(&mut self, settled: &ToGrade<'_>) -> Graded {
        if !rutent_core_orch::closing::is_final_deposit(settled) {
            return ProvidersWord.grade(settled);
        }
        let entry: Entry = serde_json::from_value(json!({
            "kind": "verified",
            "who": settled.core,
            "what": { "call": "evidence", "evidence": settled.evidence, "order": settled.order,
                "segment": settled.segment, "grade": 1, "whose": "polygon", "rests_on": [settled.result],
                "movement": { "network": "polygon", "id": format!("mv-{}", settled.transaction.as_str()) } },
            "when": { "at": settled.at, "source": "core_clock" },
            "outcome": { "status": "observed" },
        }))
        .unwrap();
        Graded::Settles {
            cites: settled.evidence.clone(),
            entries: vec![entry],
        }
    }
}

/// How the origin reserve answers Core's question about a credit.
#[derive(Debug, Clone)]
enum Reply {
    /// It confirms the credit the question names, made by `maker`, at `amount` where given.
    Confirms {
        maker: &'static str,
        amount: Option<&'static str>,
    },
    /// It confirms a credit as given where given, the inner confirmation made by `inner`, of `segment`, in `asset`.
    Credit {
        inner: &'static str,
        segment: Option<&'static str>,
        asset: Option<&'static str>,
        shape: Option<&'static str>,
    },
    /// It confirms the credit in full, as an answer to an earlier question, under `transaction`.
    Replays { transaction: String },
    /// Bytes no statement is.
    Garbage,
    /// No credit has arrived.
    NotConfirmed,
    /// Nothing comes back.
    Silent,
}

/// The providers' modules, as Core asks them: the origin reserve's answer to each question, and the questions asked.
struct Answering {
    reply: Reply,
    asked: Vec<Statement<Operation>>,
}

impl Answering {
    fn confirming() -> Self {
        Answering {
            reply: Reply::Confirms {
                maker: RESERVE,
                amount: None,
            },
            asked: Vec::new(),
        }
    }

    fn replying(reply: Reply) -> Self {
        Answering {
            reply,
            asked: Vec::new(),
        }
    }
}

/// The reserve's confirmation of a credit of `amount` of `asset` for `segment`, as `maker` signs it, answering
/// `transaction`.
fn confirmation(
    maker: &str,
    transaction: &str,
    segment: &str,
    asset: &str,
    amount: &str,
) -> Vec<u8> {
    confirmation_inside(maker, maker, transaction, segment, asset, amount)
}

/// [`confirmation`], its inner confirmation made by `inner`.
fn confirmation_inside(
    maker: &str,
    inner_maker: &str,
    transaction: &str,
    segment: &str,
    asset: &str,
    amount: &str,
) -> Vec<u8> {
    confirmation_shaped(
        (maker, inner_maker),
        REVERSAL_CONFIRMATION_SHAPE,
        transaction,
        segment,
        asset,
        amount,
    )
}

/// [`confirmation_inside`], its inner confirmation of `shape`.
fn confirmation_shaped(
    (maker, inner_maker): (&str, &str),
    shape: &str,
    transaction: &str,
    segment: &str,
    asset: &str,
    amount: &str,
) -> Vec<u8> {
    let deployment = snapshot_support::deployment();
    let key = snapshot_support::statements_key(maker, 1);
    let mut inner = snapshot_support::envelope(
        &deployment,
        shape,
        inner_maker,
        2_000,
        json!({ "segment": segment, "asset": asset, "amount": amount }),
    );
    snapshot_support::sign_value(
        &mut inner,
        &snapshot_support::statements_key(inner_maker, 1),
    );
    let body = json!({
        "kind": "reversal_confirmation",
        "transaction": transaction,
        "outcome": { "status": "confirmed", "confirmation": inner },
    });
    snapshot_support::sign(
        snapshot_support::envelope(&deployment, RESULT_SHAPE, maker, 2_000, body),
        &key,
    )
}

impl Providers for Answering {
    fn ask(&mut self, provider: &Id, question: &[u8]) -> Option<Vec<u8>> {
        let asked: Statement<Operation> = serde_json::from_slice(question).unwrap();
        let Operation::ReversalConfirmation(q) = &asked.statement else {
            panic!("only a reversal's confirmation is asked: {asked:?}");
        };
        assert_eq!(provider.as_str(), q.provider);
        let q = q.clone();
        self.asked.push(asked);
        match &self.reply {
            Reply::Confirms { maker, amount } => Some(confirmation(
                maker,
                &q.transaction,
                &q.segment,
                &q.asset,
                amount.unwrap_or(&q.amount),
            )),
            Reply::Credit {
                inner,
                segment,
                asset,
                shape,
            } => Some(confirmation_shaped(
                (RESERVE, inner),
                shape.unwrap_or(REVERSAL_CONFIRMATION_SHAPE),
                &q.transaction,
                segment.unwrap_or(&q.segment),
                asset.unwrap_or(&q.asset),
                &q.amount,
            )),
            Reply::Replays { transaction } => Some(confirmation(
                RESERVE,
                transaction,
                &q.segment,
                &q.asset,
                &q.amount,
            )),
            Reply::Garbage => Some(b"not a statement".to_vec()),
            Reply::NotConfirmed => Some(result(
                RESERVE,
                "reversal_confirmation",
                &q.transaction,
                json!({ "status": "not_confirmed" }),
            )),
            Reply::Silent => None,
        }
    }
}

// The catalogue and the route -------------------------------------------------------------------------------------

/// Joins `provider` of `publisher`, listing its first statements key, with `nodes` private.
fn join(catalogue: &mut Catalogue, n: usize, provider: &str, publisher: &str, nodes: &[&str]) {
    let ad = snapshot_support::reserve(provider, publisher);
    let mut value = serde_json::to_value(&ad).unwrap();
    value["private_nodes"] = json!(nodes);
    let ad = serde_json::from_value(value).unwrap();
    let grant = format!("g-{n}");
    catalogue
        .grant(snapshot_support::grant(&grant, &ad, 5_000_000_000))
        .unwrap();
    catalogue
        .trust_advertising_key(
            &grant,
            snapshot_support::public_key(&snapshot_support::advertising_key(provider)),
        )
        .unwrap();
    let keys =
        snapshot_support::keys_of(provider, &[&snapshot_support::statements_key(provider, 1)]);
    catalogue
        .join(
            &snapshot_support::advertisement_bytes(&ad, keys, snapshot_support::JOINED_AT),
            Instant(snapshot_support::JOINED_AT),
        )
        .unwrap();
}

/// The route's three providers, the exchange published by `exchange_publisher`, each tuple of the route under the
/// weakest floor, and `ledger-in` held by the bank's publisher.
fn catalogue(record: &DecisionRecord, publishers: [&str; 3], ledger_of: usize) -> Catalogue {
    let mut catalogue = Catalogue::new();
    catalogue.set_context(
        snapshot_support::deployment(),
        rutent_contracts::provider_role::VERSION,
    );
    catalogue.add_public_node("polygon").unwrap();
    for (n, provider) in [RESERVE, EXCHANGE, WALLET].into_iter().enumerate() {
        let private: &[&str] = if n == ledger_of { &["ledger-in"] } else { &[] };
        join(&mut catalogue, n, provider, publishers[n], private);
    }
    catalogue
        .register_private_holding("ledger-in", publishers[ledger_of], "holding")
        .unwrap();
    for operation in record.chosen().unwrap().parts[0].operations.as_slice() {
        let category = match operation.kind {
            rutent_contracts::router_api::OperationKind::Exchange => {
                rutent_prov::catalogue::Category::Exchange
            }
            rutent_contracts::router_api::OperationKind::Route => {
                rutent_prov::catalogue::Category::Ramp
            }
            _ => rutent_prov::catalogue::Category::Reserve,
        };
        let exchange = category == rutent_prov::catalogue::Category::Exchange;
        catalogue.set_evidence_floor(
            TupleKey {
                category,
                origin: Endpoint::from(&operation.from.node),
                destination: Endpoint::from(&operation.to.node),
                asset: operation.from.asset.id.clone(),
                to_asset: exchange.then(|| operation.to.asset.id.clone()),
            },
            EvidenceGrade::Statement,
        );
    }
    catalogue
}

/// The router-api fixture's record, made for the tests' deployment.
fn record() -> DecisionRecord {
    let mut envelope = read("router-api/fixtures/valid/decision-record-split-of-one-part.json");
    let fixtures = envelope["statement"]["bounds"]["deployment"].clone();
    let ours = serde_json::to_value(snapshot_support::deployment()).unwrap();
    /// Every statement of the fixture's deployment, the record's bounds among them, made ours.
    fn replace(value: &mut Value, from: &Value, to: &Value) {
        if value == from {
            *value = to.clone();
        } else if let Some(items) = value.as_array_mut() {
            items.iter_mut().for_each(|v| replace(v, from, to));
        } else if let Some(fields) = value.as_object_mut() {
            fields.values_mut().for_each(|v| replace(v, from, to));
        }
    }
    replace(&mut envelope, &fixtures, &ours);
    serde_json::from_value(envelope["statement"].clone()).unwrap()
}

/// The order's `route_chosen`, of the record's chosen route.
fn route_chosen(record: &DecisionRecord) -> RecordedInput {
    let chosen = serde_json::to_value(record.chosen().unwrap()).unwrap();
    let plan: Vec<Value> = chosen["parts"][0]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .zip(SEGMENTS)
        .map(|(op, segment)| {
            json!({ "segment_id": segment, "provider": op["provider"], "kind": op["kind"], "from": op["from"], "to": op["to"] })
        })
        .collect();
    serde_json::from_value(json!({
        "record_id": "rec-chosen", "recorded_at": "2026-11-10T09:00:00Z",
        "time_source": "core_clock", "source": { "kind": "orchestrator", "id": "rutent-core" },
        "standing": "verified", "subject": { "order": ORDER },
        "observed": { "kind": "route_chosen", "decision_record": record.record_id(),
            "parts": [{ "part_id": PART, "segments": plan }] },
    }))
    .unwrap()
}

/// A verified input entry, to stand for one where a fake needs an entry for its receipt.
fn input_entry() -> Entry {
    Entry::Verified(rutent_contracts::core_internal::VerifiedEntry {
        kind: rutent_contracts::core_internal::VerifiedKind::Verified,
        who: core(),
        what: What::Input {
            input: route_chosen(&record()),
        },
        when: rutent_contracts::core_internal::When {
            at: Timestamp::new("2026-11-10T09:00:00Z").unwrap(),
            source: rutent_contracts::core_internal::TimeSource::CoreClock,
        },
        outcome: rutent_contracts::core_internal::Outcome::Observed {},
    })
}

// The world --------------------------------------------------------------------------------------------------------

/// Core around the fake platform: the order's route chosen and followed, its `route_chosen` in the log.
struct World {
    clock: TestClock,
    catalogue: Catalogue,
    log: KeptLog,
    store: Store,
    policy: Permitting,
    key: TestKey,
    ids: Ids,
    issued: Issued,
    exposures: Exposures,
    segments: Segments,
    record: DecisionRecord,
    platform: Platform,
    reversals: Reversals,
    stuck: Stuck,
    core: Actor,
    operator: Actor,
}

impl World {
    /// The order over `platform`, the exchange published by `acme`.
    fn over(platform: Platform) -> Self {
        Self::published(platform, ["bank-co", "acme", "wallet-co"], 0)
    }

    /// The order over `platform`, the bank, the exchange and the wallet published by `publishers`, `ledger-in`
    /// declared private by the `ledger_of`th of them, which holds it.
    fn published(platform: Platform, publishers: [&str; 3], ledger_of: usize) -> Self {
        let record = record();
        let clock = TestClock(Cell::new(Reading::at(NOW)));
        let chosen = route_chosen(&record);
        let log = KeptLog::default();
        log.entries.borrow_mut().push(Entry::Verified(
            rutent_contracts::core_internal::VerifiedEntry {
                kind: rutent_contracts::core_internal::VerifiedKind::Verified,
                who: core(),
                what: What::Input {
                    input: chosen.clone(),
                },
                when: rutent_contracts::core_internal::When {
                    at: Timestamp::new("2026-11-10T09:00:00Z").unwrap(),
                    source: rutent_contracts::core_internal::TimeSource::CoreClock,
                },
                outcome: rutent_contracts::core_internal::Outcome::Observed {},
            },
        ));
        let mut segments = Segments::new();
        assert!(segments.follow(&clock, &chosen));
        // The order's acceptance, by its customer's party, as intake records it: whose acceptance of a fault counts.
        log.entries.borrow_mut().push(
            serde_json::from_value(json!({
                "kind": "verified",
                "who": { "party": "operator", "acts": "rutent-core", "role": "core", "kind": "component" },
                "what": { "call": "acceptance", "acceptance": "acc-1", "order": ORDER, "request": "req-0",
                    "quote": { "algorithm": "sha-256", "value": "ab".repeat(32) }, "authorisation": "auth-1",
                    "basis": "automatic_deposit",
                    "parties": { "requester": customer().party, "beneficiary": customer().party,
                        "payer": customer().party },
                    "origin_reserve": RESERVE },
                "when": { "at": "2026-11-10T09:00:00Z", "source": "core_clock" },
                "outcome": { "status": "observed" },
            }))
            .expect("an acceptance"),
        );
        let mut reversals = Reversals::new();
        assert!(reversals.follow(&id(ORDER), &log.entries()));
        World {
            catalogue: catalogue(&record, publishers, ledger_of),
            clock,
            log,
            store: Store::default(),
            policy: Permitting {
                decided: 0,
                log: Rc::clone(&log.entries),
            },
            key: TestKey(snapshot_support::operator_key()),
            ids: Ids::default(),
            issued: Issued::new(),
            exposures: Exposures::new(),
            segments,
            record,
            platform,
            reversals,
            stuck: Stuck::new(),
            core: core(),
            operator: operator(),
        }
    }

    /// Runs the order's part through the segments.
    fn run(&mut self) -> Result<Ran, SegmentsError> {
        let (order, part, auth, origin) = (id(ORDER), id(PART), id("auth-1"), id(RESERVE));
        let plan = SEGMENTS.map(id);
        let end = RouteEnd {
            destination: Destination {
                reserve: id(WALLET),
                node: OuterNode {
                    network: "polygon".into(),
                },
                account: id("acct-2"),
            },
            to_asset: Denomination {
                id: "USDC".into(),
                decimals: 6,
            },
            delivers: rutent_contracts::router_api::Amount::new("1").unwrap(),
        };
        let session = id("session-1");
        let inputs = BTreeMap::new();
        let live = Live {
            order: &order,
            part: &part,
            segments: &plan,
            record: &self.record,
            at: 0,
            authorisation: &auth,
            origin_reserve: &origin,
            route: &end,
            caller: &self.core,
            core: &self.core,
            operator: &self.operator,
            session: Some(&session),
            inputs: &inputs,
            floor: None,
        };
        let mut parts = Parts {
            clock: &self.clock,
            catalogue: &self.catalogue,
            log: &mut self.log,
            evidence: &mut self.store,
            decisions: &mut self.policy,
            modules: &mut self.platform,
            uses: &mut NoProxy,
            grading: &mut PublicDeposit,
            operator_key: &mut self.key,
            fresh: &mut self.ids,
            issued: &self.issued,
            exposures: &mut self.exposures,
        };
        self.segments.run_part(&mut parts, &live)
    }

    /// The outcomes of the part's segments, run once.
    fn ran(&mut self) -> Vec<SegmentOutcome> {
        match self.run().expect("the part runs") {
            Ran::Segments(done) => done.into_iter().map(|a| a.outcome).collect(),
            refused => panic!("{refused:?}"),
        }
    }

    /// Calls `f` with Core's reversals, their parts and the stuck orders.
    fn with<R>(
        &mut self,
        f: impl FnOnce(&mut Reversals, &mut ReversalParts<'_>, &mut Stuck) -> R,
    ) -> R {
        let mut parts = ReversalParts {
            clock: &self.clock,
            catalogue: &mut self.catalogue,
            log: &mut self.log,
            evidence: &mut self.store,
            fresh: &mut self.ids,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
            core: &self.core,
            operator: &self.operator,
        };
        f(&mut self.reversals, &mut parts, &mut self.stuck)
    }

    /// Asks the origin reserve about `segment`'s reversal, `answering` answering.
    fn confirm(
        &mut self,
        answering: &mut Answering,
        segment: &str,
    ) -> Result<Confirmation, ReversalError> {
        let bounds: PolicyBounds = self.record.bounds.clone();
        let (order, part, segment) = (id(ORDER), id(PART), id(segment));
        let at = At {
            order: &order,
            part: &part,
            segment: &segment,
        };
        let mut parts = ReversalParts {
            clock: &self.clock,
            catalogue: &mut self.catalogue,
            log: &mut self.log,
            evidence: &mut self.store,
            fresh: &mut self.ids,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
            core: &self.core,
            operator: &self.operator,
        };
        let mut asking = Asking {
            bounds: &bounds,
            key: &mut self.key,
            providers: answering,
        };
        self.reversals.confirm(&mut parts, &mut asking, at)
    }

    /// Asks the failed provider of `segment` to reverse it, policy deciding and the platform carrying it out.
    fn ask(&mut self, segment: &str) -> Result<Asked, ReversalError> {
        let bounds: PolicyBounds = self.record.bounds.clone();
        let (order, part, segment) = (id(ORDER), id(PART), id(segment));
        let at = At {
            order: &order,
            part: &part,
            segment: &segment,
        };
        let mut parts = ReversalParts {
            clock: &self.clock,
            catalogue: &mut self.catalogue,
            log: &mut self.log,
            evidence: &mut self.store,
            fresh: &mut self.ids,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
            core: &self.core,
            operator: &self.operator,
        };
        let mut no_one = Answering::replying(Reply::Silent);
        let mut asking = Asking {
            bounds: &bounds,
            key: &mut self.key,
            providers: &mut no_one,
        };
        let mut moving = Moving {
            decisions: &mut self.policy,
            modules: &mut self.platform,
        };
        let asked = self.reversals.ask(&mut parts, &mut asking, &mut moving, at);
        assert!(no_one.asked.is_empty(), "no reserve is asked to reverse");
        asked
    }

    /// The order as Core follows it.
    fn followed(&self) -> &stm::Order {
        self.segments.order(&id(ORDER)).unwrap()
    }

    /// The state of `segment` in the order Core follows.
    fn state_of(&self, segment: &str) -> SegmentState {
        self.followed().parts[0]
            .segments
            .iter()
            .find(|s| s.segment_id.as_str() == segment)
            .unwrap()
            .state
    }

    /// Whether `segment`'s value is held in flight.
    fn held(&self, segment: &str) -> bool {
        self.exposures.held().any(|h| h.segment.as_str() == segment)
    }

    /// The verified records appended from `from` on.
    fn whats_from(&self, from: usize) -> Vec<What> {
        self.log.entries()[from..]
            .iter()
            .filter_map(|e| match e {
                Entry::Verified(v) => Some(v.what.clone()),
                Entry::Stated(_) => None,
            })
            .collect()
    }

    /// The log as the log would keep it: each stated entry attributed from its kept bytes over the entries before
    /// it, and each entry from `from` on admitted over the entries before it. Refused at the first it wouldn't
    /// admit.
    fn admitted_from(&self, from: usize) -> Result<usize, (usize, String)> {
        let mut history: Vec<(Receipt, Entry)> = Vec::new();
        // Policy's decisions are policy's to admit, on its own handle: its engine's tests hold them.
        for (i, appended) in self.log.entries().iter().enumerate() {
            let entry = match appended {
                Entry::Stated(claim) => {
                    let mut sources = Sources(self);
                    let mut keys = Sources(self);
                    Entry::Stated(
                        attribute(claim, &history, &mut sources, &mut keys, &Sources(self))
                            .map_err(|e| (i, format!("{e:?}")))?,
                    )
                }
                verified => verified.clone(),
            };
            let policys = matches!(&entry, Entry::Verified(v) if is_decision(&v.what));
            if i >= from && !policys {
                admit_entry(
                    &history,
                    &entry,
                    Through::General,
                    &DeploymentOperator {
                        maker: operator().acts,
                        party: operator().party,
                    },
                )
                .map_err(|v| (i, v.0.to_owned()))?;
            }
            history.push((receipt_of(i, appended), entry));
        }
        Ok(self.log.len() - from)
    }
}

/// What the log reads to attribute a statement: the store's bytes, the catalogue's keys and publishers, and the
/// operator's key for Core's questions.
struct Sources<'a>(&'a World);

impl KeptStatements for Sources<'_> {
    fn kept(&mut self, wanted: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        let place = (0..self.0.store.0.len()).find(|i| &commitment(*i) == wanted);
        Ok(place.map(|i| {
            serde_json::from_value(json!({
                "commitment": wanted,
                "salt": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "bytes": snapshot_support::base64url(&self.0.store.0[i]),
            }))
            .unwrap()
        }))
    }
}

impl StatementKeys for Sources<'_> {
    fn deployment(&mut self) -> Result<rutent_contracts::provider_role::Deployment, AppendFailure> {
        Ok(snapshot_support::deployment())
    }

    fn operator(&mut self) -> Result<Id, AppendFailure> {
        Ok(operator().acts)
    }

    fn publisher(&mut self, maker: &Id) -> Result<Option<Id>, AppendFailure> {
        if *maker == operator().acts {
            return Ok(Some(operator().party));
        }
        Ok(
            match self.0.catalogue.publishers().publisher(maker.as_str()) {
                Some(Publisher::Operator) => Some(operator().party),
                Some(Publisher::Party(party)) => Id::new(party),
                None => None,
            },
        )
    }

    fn signs_for(&mut self, _: &Id, _: &SignedWith, _: &Timestamp) -> Result<bool, AppendFailure> {
        Ok(true)
    }
}

impl Signatures for Sources<'_> {
    fn verifies(&self, received: &[u8]) -> bool {
        let Ok(value) = serde_json::from_slice::<Value>(received) else {
            return false;
        };
        if value["maker"].as_str() == Some(operator().acts.as_str()) {
            // Ed25519 signs deterministically: the operator's statement verifies where signing it again gives it.
            let mut again = value.clone();
            snapshot_support::sign_value(&mut again, &snapshot_support::operator_key());
            return again == value && signing_input(received).is_ok();
        }
        self.0
            .catalogue
            .verify_statement(received, Instant(NOW))
            .is_ok()
    }
}

fn at<'a>(order: &'a Id, part: &'a Id, segment: &'a Id) -> At<'a> {
    At {
        order,
        part,
        segment,
    }
}

/// The exchange's failed segment, as `World::over(platform)` leaves it after one run.
fn exchange_failed() -> World {
    let mut world = World::over(Platform::settling().with(EXCHANGE, Act::Fail));
    let ran = world.ran();
    assert!(matches!(ran[0], SegmentOutcome::Settled { .. }), "{ran:?}");
    assert_eq!(ran[1], SegmentOutcome::Failed, "{ran:?}");
    world
}

/// The order complete: every segment settled.
fn completed() -> World {
    let mut world = World::over(Platform::settling());
    let ran = world.ran();
    assert_eq!(ran.len(), 3, "{ran:?}");
    assert_eq!(world.followed().state, OrderState::Complete);
    world
}

/// Why a confirmation completed nothing.
fn why(confirmed: Result<Confirmation, ReversalError>) -> NotCounted {
    match confirmed.expect("the question is asked") {
        Confirmation::NotCompleted { why, .. } => why,
        done => panic!("{done:?}"),
    }
}

fn owed_id() -> String {
    format!("{ORDER}/p1-s2")
}

// The tests --------------------------------------------------------------------------------------------------------

/// The exchange failed. Its own statement that it reversed in full is kept and completes nothing, asked or not: it is
/// the failed provider's own word. A confirmation by a reserve sharing the exchange's publisher, and the origin
/// reserve's confirmation of a short credit, complete nothing either. Through it all the reversal stays owed, the
/// segment's value held and its part unrecalculable, and the log admits every entry.
// verifies: REQ-ORD-3
#[test]
fn failed_providers_own_statement_does_not_complete_its_reversal() {
    let mut world = exchange_failed();
    let mark = world.log.len();
    let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
    let own = result(
        EXCHANGE,
        "reversal",
        "tx-undo",
        json!({ "status": "succeeded", "node": "ledger-in", "asset": "COP", "amount": EXCHANGED, "evidence": piece(EXCHANGE) }),
    );
    let reported = world
        .with(|r, p, _| r.report(p, at(&order, &part, &s2), &own))
        .unwrap();
    assert_eq!(reported.why, NotCounted::FailedProvidersOwn);
    // Asked in the reserve's place, the exchange's answer is still its own.
    let mut by_the_exchange = Answering::replying(Reply::Confirms {
        maker: EXCHANGE,
        amount: None,
    });
    assert!(matches!(
        world.confirm(&mut by_the_exchange, "p1-s2").unwrap(),
        Confirmation::NotCompleted {
            why: NotCounted::FailedProvidersOwn,
            ..
        }
    ));
    // The origin reserve, confirming less than the exchange took in: not in full.
    let mut short = Answering::replying(Reply::Confirms {
        maker: RESERVE,
        amount: Some("99800000"),
    });
    assert_eq!(
        why(world.confirm(&mut short, "p1-s2")),
        NotCounted::NotInFull {
            stated: "99800000".into()
        }
    );
    // In another asset, of another segment, or with a confirmation inside that isn't the reserve's own.
    for (reply, expected) in [
        (
            Reply::Credit {
                inner: RESERVE,
                segment: None,
                asset: Some("USDC"),
                shape: None,
            },
            NotCounted::AnotherAsset,
        ),
        (
            Reply::Credit {
                inner: RESERVE,
                segment: Some("tx-another"),
                asset: None,
                shape: None,
            },
            NotCounted::AnotherSegment,
        ),
        (
            Reply::Credit {
                inner: WALLET,
                segment: None,
                asset: None,
                shape: None,
            },
            NotCounted::NotItsOwn,
        ),
        (
            Reply::Credit {
                inner: RESERVE,
                segment: None,
                asset: None,
                shape: Some(RESULT_SHAPE),
            },
            NotCounted::NotItsOwn,
        ),
    ] {
        assert_eq!(
            why(world.confirm(&mut Answering::replying(reply), "p1-s2")),
            expected
        );
    }
    // An answer to an earlier question, the one nothing came back to, replayed under a fresh one.
    let mut silent = Answering::replying(Reply::Silent);
    assert_eq!(
        why(world.confirm(&mut silent, "p1-s2")),
        NotCounted::Unanswered
    );
    let Operation::ReversalConfirmation(earlier) = &silent.asked[0].statement else {
        unreachable!()
    };
    let mut replayed = Answering::replying(Reply::Replays {
        transaction: earlier.transaction.clone(),
    });
    assert_eq!(
        why(world.confirm(&mut replayed, "p1-s2")),
        NotCounted::AnotherQuestion
    );
    let Operation::ReversalConfirmation(fresh) = &replayed.asked[0].statement else {
        unreachable!()
    };
    assert_ne!(
        fresh.transaction, earlier.transaction,
        "asked under a fresh transaction"
    );
    assert_eq!(world.state_of("p1-s2"), SegmentState::FailedReversalOwed);
    assert!(world.catalogue.owed().is_owed(&owed_id()));
    assert!(
        world.held("p1-s2"),
        "the failed exchange's value is still held"
    );
    assert!(evidenced(&world.followed().parts[0]).is_err());
    assert!(
        !world
            .whats_from(mark)
            .iter()
            .any(|w| matches!(w, What::Evidence { .. } | What::Input { .. })),
        "nothing is recorded as evidence or input"
    );
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    // The exchange and the bank of one publisher, `acme`, at `ledger-in`, a private node another publisher holds:
    // the origin reserve's word is its publisher's own about its own provider's work.
    let mut world = World::published(
        Platform::settling().with(EXCHANGE, Act::Fail),
        ["acme", "acme", "wallet-co"],
        2,
    );
    assert_eq!(world.ran()[1], SegmentOutcome::Failed);
    assert_eq!(
        why(world.confirm(&mut Answering::confirming(), "p1-s2")),
        NotCounted::SamePublisher
    );
    assert_eq!(world.state_of("p1-s2"), SegmentState::FailedReversalOwed);
    assert!(world.catalogue.owed().is_owed(&owed_id()));

    // Inside the publisher's own private node, `ledger-in`, its word counts labelled as the publisher's
    // (`REQ-ORD-3`'s third sentence) only as the log takes the label: the failed provider's own statement, graded
    // a statement. The origin reserve's signed confirmation can't carry it, so it completes nothing; the publisher's
    // word is the failed provider's answer when it is asked to reverse.
    let mut world = World::published(
        Platform::settling().with(EXCHANGE, Act::Fail),
        ["bank-co", "bank-co", "wallet-co"],
        0,
    );
    assert_eq!(world.ran()[1], SegmentOutcome::Failed);
    assert_eq!(
        why(world.confirm(&mut Answering::confirming(), "p1-s2")),
        NotCounted::SamePublisher
    );
    assert_eq!(world.state_of("p1-s2"), SegmentState::FailedReversalOwed);
}

/// The exchange failed. Core asks it to reverse: policy decides `ask_reversal` of the exchange, the effect is recorded
/// on its permit, and the reversal's operation, under a fresh transaction, names the exchange's transaction, the
/// node it took the value from and what it took in, in full. The exchange's answer, that it reversed, is kept, and
/// the effect's result recorded; its own word completes nothing. Inside its publisher's own private node, the same
/// answer is that publisher's word and completes the reversal, labelled the publisher's. The log admits every entry.
// verifies: REQ-ORD-3, REQ-ORD-11
#[test]
fn failed_provider_is_asked_to_reverse_and_its_word_counts_only_as_its_nodes_publishers() {
    let mut world = exchange_failed();
    let mark = world.log.len();
    let asked = world.ask("p1-s2").unwrap();
    assert_eq!(asked.completed, None);
    assert_eq!(asked.why, Some(NotCounted::FailedProvidersOwn));
    let whats = world.whats_from(mark);
    assert!(whats.iter().any(|w| matches!(
        w,
        What::Effect { action: rutent_contracts::core_internal::Action::AskReversal { segment, provider, .. }, .. }
            if segment.as_str() == "p1-s2" && provider.as_str() == EXCHANGE
    )));
    assert!(whats.iter().any(|w| matches!(
        w,
        What::Result { effect } if *effect == asked.effect
    )));
    let (exchange_tx, _) = world.segments.sent(&id(ORDER), &id("p1-s2")).unwrap();
    let Operation::Reversal(reversal) = &asked.operation.statement else {
        panic!("a reversal is asked: {:?}", asked.operation)
    };
    assert_eq!(
        (
            reversal.provider.as_str(),
            reversal.reverses.as_str(),
            reversal.node.as_str(),
            reversal.asset.as_str(),
            reversal.amount.as_str()
        ),
        (EXCHANGE, exchange_tx.as_str(), "ledger-in", "COP", EXCHANGED)
    );
    assert_ne!(reversal.transaction, exchange_tx.as_str(), "under a fresh transaction");
    assert_eq!(world.state_of("p1-s2"), SegmentState::FailedReversalOwed);
    assert!(world.catalogue.owed().is_owed(&owed_id()));
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    // The exchange published by `bank-co`, which holds `ledger-in`: the exchange's word there is its publisher's.
    let mut world = World::published(
        Platform::settling().with(EXCHANGE, Act::Fail),
        ["bank-co", "bank-co", "wallet-co"],
        0,
    );
    assert_eq!(world.ran()[1], SegmentOutcome::Failed);
    let mark = world.log.len();
    let done = world.ask("p1-s2").unwrap().completed.expect("completed");
    assert_eq!(
        (done.whose.as_str(), done.grade, &done.word),
        (EXCHANGE, EvidenceGrade::Statement, &Word::Publishers(id("bank-co")))
    );
    assert!(world.whats_from(mark).iter().any(|w| matches!(
        w,
        What::Evidence { publisher: Some(p), whose, .. } if p.as_str() == "bank-co" && whose.as_str() == EXCHANGE
    )));
    assert_eq!(world.state_of("p1-s2"), SegmentState::ReversalComplete);
    assert!(!world.catalogue.owed().is_owed(&owed_id()));
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// The exchange failed, its reversal owed. Core asks the origin reserve, the bank whose withdrawal took the value to
/// `ledger-in`, whether the credit arrived: its question names the exchange's transaction, the COP the exchange took
/// in, in full. The reserve's signed confirmation completes the reversal as its word: an `evidence` whose it is,
/// signed by it, and the `reversal_completed` citing it. The order Core follows takes it, the exchange's hold goes,
/// the owed reversal is discharged, and the part's value is evidenced at `ledger-in` again, so it can be recalculated.
/// Asking again records nothing more. The log admits every entry.
// verifies: REQ-ORD-3
#[test]
fn origin_reserves_statement_completes_a_reversal_as_its_word() {
    let mut world = exchange_failed();
    let mark = world.log.len();
    assert!(evidenced(&world.followed().parts[0]).is_err());
    // Another party than the origin reserve completes nothing (ADR-0008).
    assert_eq!(
        why(world.confirm(
            &mut Answering::replying(Reply::Confirms {
                maker: WALLET,
                amount: None
            }),
            "p1-s2"
        )),
        NotCounted::NotTheOriginReserve
    );
    let mut reserve = Answering::confirming();
    let done = match world.confirm(&mut reserve, "p1-s2").unwrap() {
        Confirmation::Completed(done) => done,
        other => panic!("{other:?}"),
    };
    assert_eq!(done.whose, id(RESERVE));
    assert_eq!(done.grade, EvidenceGrade::SignedByProvider);
    let Operation::ReversalConfirmation(asked) = &reserve.asked[0].statement else {
        unreachable!()
    };
    let (exchange_tx, _) = world.segments.sent(&id(ORDER), &id("p1-s2")).unwrap();
    assert_eq!(
        (
            asked.provider.as_str(),
            asked.segment.as_str(),
            asked.asset.as_str(),
            asked.amount.as_str()
        ),
        (RESERVE, exchange_tx.as_str(), "COP", EXCHANGED)
    );
    let whats = world.whats_from(mark);
    assert!(whats.iter().any(|w| matches!(
        w,
        What::Evidence { evidence, whose, grade: EvidenceGrade::SignedByProvider, .. }
            if *evidence == done.evidence && whose.as_str() == RESERVE
    )));
    assert!(whats.iter().any(|w| matches!(
        w,
        What::Input { input } if matches!(input.observed, stm::Observed::ReversalCompleted { .. })
    )));
    assert_eq!(world.state_of("p1-s2"), SegmentState::ReversalComplete);
    assert!(!world.held("p1-s2"), "the reversed exchange holds nothing");
    assert!(!world.catalogue.owed().is_owed(&owed_id()));
    let (from, _) = evidenced(&world.followed().parts[0]).expect("the value is evidenced again");
    assert_eq!(
        serde_json::to_value(from).unwrap()["node"],
        "ledger-in",
        "where the exchange took it from"
    );
    let appended = world.log.len();
    assert_eq!(
        world.confirm(&mut reserve, "p1-s2").unwrap(),
        Confirmation::Completed(done)
    );
    assert_eq!(world.log.len(), appended, "nothing is recorded twice");
    assert_eq!(reserve.asked.len(), 1, "no one is asked twice");
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// The exchange shows the failure was the customer's: its signed result failing the conversion, under the
/// conversion's own transaction, its evidence naming the customer's fault. The kind of fault is read from those signed
/// bytes. Until the customer accepts, the exchange carries the reversal; the operator can't accept for the customer.
/// The customer disputes it: the part is declared stuck in a dispute, in the order Core follows and in the stuck
/// orders', and the order is marked stuck. Its reversal still completes on the stuck part. Another provider's
/// statement, or one naming no fault, is no showing. A dispute the stuck orders couldn't record leaves the showing
/// open. A showing accepted can't be replaced.
// verifies: REQ-ORD-11
#[test]
fn dispute_over_a_fault_makes_the_order_stuck() {
    let mut world = exchange_failed();
    let mark = world.log.len();
    let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
    let tx = world
        .segments
        .sent(&order, &s2)
        .unwrap()
        .0
        .as_str()
        .to_owned();
    let showing = |maker: &str, fault: &str| {
        result(
            maker,
            "conversion",
            &tx,
            json!({ "status": "failed", "fault": fault, "evidence": piece(maker) }),
        )
    };
    assert_eq!(
        world.with(|r, p, _| r.show_fault(
            p,
            at(&order, &part, &s2),
            &showing(RESERVE, "customer")
        )),
        Err(ReversalError::NotTheProvider)
    );
    let no_fault = result(
        EXCHANGE,
        "conversion",
        &tx,
        json!({ "status": "failed", "evidence": piece(EXCHANGE) }),
    );
    assert_eq!(
        world.with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &no_fault)),
        Err(ReversalError::NoFaultShown)
    );
    // Only a failed result shows a fault: one that says it succeeded shows none.
    let succeeded = result(
        EXCHANGE,
        "conversion",
        &tx,
        json!({ "status": "succeeded", "amount_in": EXCHANGED, "amount_out": "249250500", "rate": "2.5",
            "fee": { "asset": "COP", "amount": "0" }, "evidence": piece(EXCHANGE) }),
    );
    assert_eq!(
        world.with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &succeeded)),
        Err(ReversalError::NoFaultShown)
    );
    // Nor does a statement of another shape than a result, though it reads as a failure naming one.
    let mut shaped = snapshot_support::envelope(
        &snapshot_support::deployment(),
        REVERSAL_CONFIRMATION_SHAPE,
        EXCHANGE,
        2_000,
        json!({ "kind": "conversion", "transaction": tx,
            "outcome": { "status": "failed", "fault": "customer", "evidence": piece(EXCHANGE) } }),
    );
    snapshot_support::sign_value(&mut shaped, &snapshot_support::statements_key(EXCHANGE, 1));
    let shaped = serde_json::to_vec(&shaped).unwrap();
    assert_eq!(
        world.with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &shaped)),
        Err(ReversalError::NoFaultShown)
    );
    world
        .with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &showing(EXCHANGE, "customer")))
        .unwrap();
    let Some(Entry::Stated(kept)) = world.log.entries().last().cloned() else {
        panic!("the showing is kept")
    };
    assert_eq!(
        kept.about
            .as_ref()
            .and_then(|a| a.transaction.as_ref())
            .map(|t| t.as_str()),
        Some(tx.as_str()),
        "about the segment's transaction, so the log verifies it"
    );
    assert_eq!(
        world.reversals.carrier(&order, &s2),
        Some(&Carrier::Provider(id(EXCHANGE)))
    );
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(p, at(&order, &part, &s2), &operator())),
        Err(ReversalError::NotTheParty),
        "the signed bytes name the customer's fault, not the operator's"
    );
    world
        .with(|r, p, s| r.dispute(p, s, at(&order, &part, &s2), &customer()))
        .unwrap();
    assert_eq!(world.followed().parts[0].state, OrderState::Stuck);
    assert_eq!(world.followed().state, OrderState::Stuck);
    assert!(world.stuck.is_stuck(&order, &part));
    assert!(
        world.exposures.stuck().iter().any(|h| h.order == order),
        "the stuck order's value is held as stuck"
    );
    assert_eq!(
        world.reversals.carrier(&order, &s2),
        Some(&Carrier::Provider(id(EXCHANGE))),
        "the provider carries it while it is disputed"
    );
    assert!(world.whats_from(mark).iter().any(|w| matches!(
        w,
        What::Input { input } if input.observed == stm::Observed::StuckDeclared { reason: stm::StuckReason::Dispute }
    )));
    // Its reversal still completes on the stuck part.
    assert!(matches!(
        world
            .confirm(&mut Answering::confirming(), "p1-s2")
            .unwrap(),
        Confirmation::Completed(_)
    ));
    assert_eq!(world.state_of("p1-s2"), SegmentState::ReversalComplete);
    assert_eq!(world.followed().parts[0].state, OrderState::Stuck);
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    // Accepted, the operator's fault is the operator's to carry, and the carrier can't be changed again.
    let mut world = exchange_failed();
    let mark_operator = world.log.len();
    world
        .with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &showing(EXCHANGE, "operator")))
        .unwrap();
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(p, at(&order, &part, &s2), &operator())),
        Ok(Carrier::Operator)
    );
    let accepted = world.whats_from(mark_operator);
    assert!(
        accepted.iter().any(|w| matches!(
            w,
            What::ReversalCarrier { carrier: rutent_contracts::core_internal::Carrier::Operator, showing: Some(_), accepted_by: Some(by), .. }
                if *by == operator()
        )),
        "the carrier recorded, with the showing and who accepted it"
    );
    assert_eq!(world.admitted_from(mark_operator), Ok(world.log.len() - mark_operator));
    assert_eq!(
        world.with(|r, p, _| r.show_fault(
            p,
            at(&order, &part, &s2),
            &showing(EXCHANGE, "customer")
        )),
        Err(ReversalError::FaultSettled)
    );
    assert_eq!(
        world.reversals.carrier(&order, &s2),
        Some(&Carrier::Operator)
    );
    assert_eq!(
        world.followed().parts[0].state,
        OrderState::FirstSegmentSettled
    );

    // The reviewer's sequence: the customer disputes while the log is down, so the stuck orders keep the built
    // declaration, to append when the part is next watched or disputed. Accepting the fault now would leave that
    // declaration to make the part stuck over an accepted fault: it is refused until the dispute is retried to
    // completion, and then the dispute stands.
    let mut world = exchange_failed();
    world
        .with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &showing(EXCHANGE, "customer")))
        .unwrap();
    world.log.down = true;
    assert_eq!(
        world.with(|r, p, s| r.dispute(p, s, at(&order, &part, &s2), &customer())),
        Err(ReversalError::Stuck(StuckError::Append(
            AppendFailure::Unavailable
        )))
    );
    world.log.down = false;
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(p, at(&order, &part, &s2), &customer())),
        Err(ReversalError::DisputePending)
    );
    assert_eq!(
        world.reversals.carrier(&order, &s2),
        Some(&Carrier::Provider(id(EXCHANGE)))
    );
    world
        .with(|r, p, s| r.dispute(p, s, at(&order, &part, &s2), &customer()))
        .unwrap();
    assert_eq!(world.followed().parts[0].state, OrderState::Stuck);
    assert!(
        world.with(|r, p, _| r.accept_fault(p, at(&order, &part, &s2), &customer()))
            .is_err(),
        "a disputed fault isn't accepted"
    );

    // A dispute the log refused keeps nothing: the showing stays open, and the customer may still accept it.
    let mut world = exchange_failed();
    world
        .with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &showing(EXCHANGE, "customer")))
        .unwrap();
    world.log.refusing = true;
    assert!(matches!(
        world.with(|r, p, s| r.dispute(p, s, at(&order, &part, &s2), &customer())),
        Err(ReversalError::Stuck(StuckError::Append(
            AppendFailure::Refused(_)
        )))
    ));
    world.log.refusing = false;
    // Only the order's own customer accepts for the customer: another party in the customer's role doesn't.
    let stranger = Actor {
        party: id("customer-2"),
        acts: id("customer-2"),
        ..customer()
    };
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(p, at(&order, &part, &s2), &stranger)),
        Err(ReversalError::NotTheParty)
    );
    // An acceptance the log couldn't take changes no carrier.
    world.log.down = true;
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(p, at(&order, &part, &s2), &customer())),
        Err(ReversalError::Append(AppendFailure::Unavailable))
    );
    world.log.down = false;
    assert_eq!(
        world.reversals.carrier(&order, &s2),
        Some(&Carrier::Provider(id(EXCHANGE)))
    );
    let mark_customer = world.log.len();
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(p, at(&order, &part, &s2), &customer())),
        Ok(Carrier::Customer)
    );
    assert!(world.whats_from(mark_customer).iter().any(|w| matches!(
        w,
        What::ReversalCarrier { carrier: rutent_contracts::core_internal::Carrier::Customer, accepted_by: Some(by), .. }
            if *by == customer()
    )));
    assert_eq!(world.admitted_from(mark_customer), Ok(world.log.len() - mark_customer));
    assert_ne!(world.followed().parts[0].state, OrderState::Stuck);
}

/// The exchange's failure puts its reversal on the catalogue's list, owed by the exchange, in COP, for what it took
/// in. Neither its own statement, nor the reserve saying no credit has arrived, nor the reserve's silence, nor the
/// catalogue seeing the exchange silent or out of standing, discharges it; only the reversal completed does.
// verifies: REQ-CAT-7
#[test]
fn owed_reversal_stays_owed_until_it_is_confirmed() {
    let mut world = exchange_failed();
    let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
    let owing = world
        .with(|r, p, _| r.owe(p, at(&order, &part, &s2)))
        .unwrap();
    assert_eq!(
        (
            owing.provider.as_str(),
            owing.asset.as_str(),
            owing.amount.as_str()
        ),
        (EXCHANGE, "COP", EXCHANGED)
    );
    let owed = world.catalogue.owed().owed_by(EXCHANGE);
    assert_eq!(owed.len(), 1);
    assert_eq!(
        (owed[0].asset.as_str(), owed[0].amount),
        ("COP", 99_900_000)
    );
    let own = result(
        EXCHANGE,
        "reversal",
        "tx-undo",
        json!({ "status": "succeeded", "node": "ledger-in", "asset": "COP", "amount": EXCHANGED, "evidence": piece(EXCHANGE) }),
    );
    world
        .with(|r, p, _| r.report(p, at(&order, &part, &s2), &own))
        .unwrap();
    assert!(world.catalogue.owed().is_owed(&owed_id()));
    assert_eq!(
        why(world.confirm(&mut Answering::replying(Reply::NotConfirmed), "p1-s2")),
        NotCounted::NotConfirmed
    );
    assert!(world.catalogue.owed().is_owed(&owed_id()));
    assert_eq!(
        why(world.confirm(&mut Answering::replying(Reply::Silent), "p1-s2")),
        NotCounted::Unanswered
    );
    assert!(world.catalogue.owed().is_owed(&owed_id()));
    for seen in [
        Observation::Silence {
            since: Instant(NOW - 86_400),
        },
        Observation::StandingLost,
    ] {
        world
            .catalogue
            .owed_mut()
            .observe(&owed_id(), seen)
            .unwrap();
        assert!(world.catalogue.owed().is_owed(&owed_id()));
    }
    assert!(matches!(
        world
            .confirm(&mut Answering::confirming(), "p1-s2")
            .unwrap(),
        Confirmation::Completed(_)
    ));
    assert!(!world.catalogue.owed().is_owed(&owed_id()));
    assert!(world.catalogue.owed().owed_by(EXCHANGE).is_empty());

    // A failed deposit owes no reversal: its value stays where it was.
    let mut world = World::over(Platform::settling().with(WALLET, Act::Fail));
    assert_eq!(world.ran()[2], SegmentOutcome::Failed);
    let s3 = id("p1-s3");
    assert_eq!(
        world.with(|r, p, _| r.owe(p, at(&order, &part, &s3))),
        Err(ReversalError::NotOwed)
    );
    assert!(world.catalogue.owed().owed_by(WALLET).is_empty());
}

/// The origin reserve answers Core's question with bytes no statement is: nothing can be kept of it, and the question
/// is dropped, so the next call asks again under a fresh question and completes on the reserve's confirmation.
// verifies: REQ-ORD-3, INV-8
#[test]
fn malformed_answer_drops_its_question_and_is_asked_again() {
    let mut world = exchange_failed();
    let mut garbled = Answering::replying(Reply::Garbage);
    assert_eq!(
        world.confirm(&mut garbled, "p1-s2"),
        Ok(Confirmation::NotCompleted {
            statement: None,
            why: NotCounted::Unreadable,
        })
    );
    let mut reserve = Answering::confirming();
    assert!(matches!(
        world.confirm(&mut reserve, "p1-s2").unwrap(),
        Confirmation::Completed(_)
    ));
    assert_eq!(reserve.asked.len(), 1, "asked again");
    let (Operation::ReversalConfirmation(first), Operation::ReversalConfirmation(again)) =
        (&garbled.asked[0].statement, &reserve.asked[0].statement)
    else {
        unreachable!()
    };
    assert_ne!(first.transaction, again.transaction);
}

/// The order complete, its origin reserve reports the withdrawal undone in pieces, a chargeback of part and then of
/// the rest. Each piece is kept as it arrives, once, as the reserve's statement about the order and its withdrawal,
/// and none alone proves the start undone: the order stays complete. The loss is the origin reserve's to carry.
// verifies: REQ-ORD-30
#[test]
fn partial_evidence_of_an_undone_start_is_kept() {
    let mut world = completed();
    let (mark, kept_bytes) = (world.log.len(), world.store.0.len());
    let (order, part, s1) = (id(ORDER), id(PART), id("p1-s1"));
    let (withdrawal_tx, _) = world.segments.sent(&order, &s1).unwrap();
    let withdrawal_tx = withdrawal_tx.as_str().to_owned();
    let pieces = [
        confirmation(
            RESERVE,
            "tx-chargeback-1",
            &withdrawal_tx,
            "COP",
            "40000000",
        ),
        confirmation(
            RESERVE,
            "tx-chargeback-2",
            &withdrawal_tx,
            "COP",
            "60000000",
        ),
    ];
    let mut kept = Vec::new();
    for piece in pieces.iter().chain(pieces.iter()) {
        let reported = world
            .with(|r, p, _| r.report(p, at(&order, &part, &s1), piece))
            .unwrap();
        assert!(
            matches!(reported.why, NotCounted::NotInFull { .. }),
            "{reported:?}"
        );
        kept.push(reported.statement);
    }
    assert_eq!(kept[0], kept[2], "a piece reported again is kept once");
    assert_ne!(kept[0], kept[1]);
    let stated: Vec<_> = world.log.entries()[mark..]
        .iter()
        .filter_map(|e| match e {
            Entry::Stated(s) => s.about.clone(),
            Entry::Verified(_) => None,
        })
        .collect();
    assert_eq!(stated.len(), 2, "each piece is kept once");
    assert_eq!(
        world.store.0.len(),
        kept_bytes + 2,
        "each piece's bytes are kept once"
    );
    assert!(
        stated
            .iter()
            .all(|a| a.order.as_ref() == Some(&order) && a.segment.as_ref() == Some(&s1))
    );
    assert_eq!(world.followed().state, OrderState::Complete);
    assert_eq!(
        world.reversals.carrier(&order, &s1),
        Some(&Carrier::OriginReserve(id(RESERVE)))
    );
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// An order whose first segment hasn't settled ends without completing on request, and nothing of it is handed over
/// after. While its withdrawal is in motion, it doesn't end, even where the request was kept from before the
/// withdrawal was handed over; once its withdrawal settled, it doesn't either.
// verifies: REQ-ORD-12
#[test]
fn order_ends_without_completing_only_before_its_first_segment_settles() {
    let order = id(ORDER);
    let mut world = World::over(Platform::settling());
    let mark = world.log.len();
    world.with(|r, p, _| r.end(p, &order)).unwrap();
    assert_eq!(world.followed().state, OrderState::EndedWithoutCompleting);
    assert_eq!(world.run(), Err(SegmentsError::Ended(id(PART))));
    assert!(
        !world
            .whats_from(mark)
            .iter()
            .any(|w| matches!(w, What::Effect { .. })),
        "nothing was handed over"
    );
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    // In motion: the withdrawal carried out with no answer.
    let mut world = World::over(Platform::settling().with(RESERVE, Act::Unknown));
    assert!(matches!(world.ran()[0], SegmentOutcome::Unknown(_)));
    let appended = world.log.len();
    assert_eq!(
        world.with(|r, p, _| r.end(p, &order)),
        Err(ReversalError::InMotion(id("p1-s1")))
    );
    assert_eq!(world.log.len(), appended);

    // The reviewer's sequence: the log down for the first request, which is kept; the withdrawal handed over
    // meanwhile, its outcome unknown; the request again is refused, in motion.
    let mut world = World::over(Platform::settling().with(RESERVE, Act::Unknown));
    world.log.down = true;
    assert_eq!(
        world.with(|r, p, _| r.end(p, &order)),
        Err(ReversalError::Append(AppendFailure::Unavailable))
    );
    world.log.down = false;
    assert!(matches!(world.ran()[0], SegmentOutcome::Unknown(_)));
    let appended = world.log.len();
    assert_eq!(
        world.with(|r, p, _| r.end(p, &order)),
        Err(ReversalError::InMotion(id("p1-s1")))
    );
    assert_eq!(world.log.len(), appended, "no end is recorded");
    assert_eq!(world.followed().state, OrderState::NotPastFirstSegment);

    // Past its first segment: the withdrawal settled, the exchange failed, its value disputed with it until its
    // reversal completes, so in motion (`part-waits-for-value-in-motion`).
    let mut world = exchange_failed();
    let appended = world.log.len();
    assert_eq!(
        world.with(|r, p, _| r.end(p, &order)),
        Err(ReversalError::InMotion(id("p1-s2")))
    );
    assert_eq!(world.log.len(), appended, "nothing is recorded");
    assert_eq!(world.followed().state, OrderState::FirstSegmentSettled);
}

/// A complete order's withdrawal proven undone by its origin reserve, its own segment's reserve, labelled its word:
/// the part, and so the order, is marked failed. An open part's start is undone the same way once nothing after its
/// withdrawal holds value: while its failed exchange owes its reversal, that value is in motion and nothing is asked;
/// once the exchange is reversed, the start proven undone fails the part.
// verifies: REQ-ORD-12, REQ-STM-6
#[test]
fn complete_order_whose_start_is_proven_undone_is_marked_failed() {
    let mut world = exchange_failed();
    let mut reserve = Answering::confirming();
    assert_eq!(
        world.confirm(&mut reserve, "p1-s1"),
        Err(ReversalError::InMotion(id("p1-s2"))),
        "the failed exchange's value is in motion"
    );
    assert!(reserve.asked.is_empty(), "nothing is asked");
    let mark = world.log.len();
    assert!(matches!(
        world.confirm(&mut Answering::confirming(), "p1-s2").unwrap(),
        Confirmation::Completed(_)
    ));
    assert!(matches!(
        world.confirm(&mut Answering::confirming(), "p1-s1").unwrap(),
        Confirmation::Completed(_)
    ));
    assert_eq!(world.followed().parts[0].state, OrderState::Failed);
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    let mut world = completed();
    let mark = world.log.len();
    let mut reserve = Answering::confirming();
    let done = match world.confirm(&mut reserve, "p1-s1").unwrap() {
        Confirmation::Completed(done) => done,
        other => panic!("{other:?}"),
    };
    let Operation::ReversalConfirmation(asked) = &reserve.asked[0].statement else {
        unreachable!()
    };
    assert_eq!(
        (asked.asset.as_str(), asked.amount.as_str()),
        ("COP", WITHDRAWN)
    );
    assert_eq!(
        (done.whose.as_str(), done.grade, &done.word),
        (RESERVE, EvidenceGrade::Statement, &Word::Reserves)
    );
    assert!(
        world.whats_from(mark).iter().any(|w| matches!(
            w,
            What::Evidence { whose, grade: EvidenceGrade::Statement, segment, .. }
                if whose.as_str() == RESERVE && segment.as_str() == "p1-s1"
        )),
        "recorded as the reserve's own word, a statement"
    );
    assert_eq!(world.state_of("p1-s1"), SegmentState::ReversalComplete);
    assert_eq!(world.followed().parts[0].state, OrderState::Failed);
    assert_eq!(world.followed().state, OrderState::Failed);
    assert_eq!(
        world.reversals.carrier(&id(ORDER), &id("p1-s1")),
        Some(&Carrier::OriginReserve(id(RESERVE)))
    );
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// The complete order's destination reports the payment returned: kept as the reserve's statement about the order
/// and recorded as `afterwards`, `returned`, and the order stays complete. Another provider's report isn't the
/// destination's, and an order not complete has no return to record.
// verifies: REQ-ORD-31
#[test]
fn return_after_complete_is_recorded_and_the_order_stays_complete() {
    let order = id(ORDER);
    let returned = result(
        WALLET,
        "deposit",
        "tx-returned",
        json!({ "status": "failed", "evidence": piece("polygon") }),
    );
    let mut world = exchange_failed();
    assert_eq!(
        world.with(|r, p, _| r.returned(p, &order, &returned)),
        Err(ReversalError::NotComplete)
    );
    let mut world = completed();
    let mark = world.log.len();
    let by_the_exchange = result(
        EXCHANGE,
        "deposit",
        "tx-returned",
        json!({ "status": "failed", "evidence": piece("polygon") }),
    );
    assert_eq!(
        world.with(|r, p, _| r.returned(p, &order, &by_the_exchange)),
        Err(ReversalError::NotTheDestination)
    );
    assert_eq!(world.log.len(), mark, "nothing is recorded");
    let after = world
        .with(|r, p, _| r.returned(p, &order, &returned))
        .unwrap();
    assert_eq!(
        (after.event, after.maker.as_str()),
        (Afterwards::Returned, WALLET)
    );
    assert!(world.whats_from(mark).iter().any(|w| matches!(
        w,
        What::Afterwards { event: Afterwards::Returned, statement, .. } if *statement == after.statement
    )));
    assert_eq!(world.followed().state, OrderState::Complete);
    let again = world
        .with(|r, p, _| r.returned(p, &order, &returned))
        .unwrap();
    assert_eq!(again, after, "recorded once");
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// The order ended before its first segment settled; the origin reserve's approval arrives after. It is kept as the
/// reserve's statement and recorded as `afterwards`, `late_approval`; nothing is handed over, the order stays ended,
/// and the customer sees it, its read checked by policy. An approval for an order still open isn't late.
// verifies: REQ-ORD-25
#[test]
fn late_origin_approval_starts_nothing_and_is_visible() {
    let order = id(ORDER);
    let approval = result(
        RESERVE,
        "withdrawal",
        "tx-approval",
        json!({ "status": "failed", "evidence": piece(RESERVE) }),
    );
    let mut world = World::over(Platform::settling());
    assert_eq!(
        world.with(|r, p, _| r.late_approval(p, &order, &approval)),
        Err(ReversalError::NotEnded)
    );
    world.with(|r, p, _| r.end(p, &order)).unwrap();
    let mark = world.log.len();
    let after = world
        .with(|r, p, _| r.late_approval(p, &order, &approval))
        .unwrap();
    assert_eq!(
        (after.event, after.maker.as_str()),
        (Afterwards::LateApproval, RESERVE)
    );
    assert_eq!(world.run(), Err(SegmentsError::Ended(id(PART))));
    assert!(
        !world
            .whats_from(mark)
            .iter()
            .any(|w| matches!(w, What::Effect { .. } | What::Input { .. })),
        "nothing starts"
    );
    assert_eq!(world.followed().state, OrderState::EndedWithoutCompleting);
    let mut reads = Allowing::default();
    let seen = world
        .reversals
        .seen(
            &mut reads,
            &world.clock,
            &mut world.ids,
            &customer(),
            None,
            &order,
        )
        .unwrap();
    assert_eq!(seen, vec![after]);
    assert_eq!(reads.0.len(), 1, "the read was checked");
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// The exchange's provider reports late that its failed conversion delivered after all: kept as its statement about
/// the segment and its transaction, the question it answers, and nothing moves: the segment stays failed and owed.
// verifies: REQ-STM-6, INV-4
#[test]
fn late_delivery_is_recorded_and_moves_nothing() {
    let mut world = exchange_failed();
    let mark = world.log.len();
    let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
    let tx = world.segments.sent(&order, &s2).unwrap().0.clone();
    let late = result(
        EXCHANGE,
        "conversion",
        tx.as_str(),
        json!({ "status": "succeeded", "amount_in": EXCHANGED, "amount_out": "249250500", "rate": "2.5",
            "fee": { "asset": "COP", "amount": "0" }, "evidence": piece(EXCHANGE) }),
    );
    let first = world
        .with(|r, p, _| r.later(p, at(&order, &part, &s2), &late))
        .unwrap();
    let again = world
        .with(|r, p, _| r.later(p, at(&order, &part, &s2), &late))
        .unwrap();
    assert_eq!(first, again, "kept once");
    assert_eq!(world.log.len(), mark + 1);
    let Entry::Stated(kept) = world.log.entries()[mark].clone() else {
        panic!("a statement")
    };
    assert_eq!(
        kept.about.as_ref().and_then(|a| a.transaction.as_ref()),
        Some(&tx),
        "about the transaction it answers, so the log verifies it"
    );
    assert!(
        world.whats_from(mark).is_empty(),
        "nothing is recorded but the statement"
    );
    assert_eq!(world.state_of("p1-s2"), SegmentState::FailedReversalOwed);
    assert!(world.held("p1-s2"));
    assert_eq!(world.admitted_from(mark), Ok(1));

    // A segment whose operation was never carried out has no answer taken yet: nothing comes late.
    let mut world = World::over(Platform::settling());
    world.log.down = true;
    assert!(world.run().is_err());
    world.log.down = false;
    let s1 = id("p1-s1");
    let tx = world.segments.sent(&order, &s1).unwrap().0.clone();
    let early = result(
        RESERVE,
        "withdrawal",
        tx.as_str(),
        json!({ "status": "failed", "evidence": piece(RESERVE) }),
    );
    let appended = world.log.len();
    assert_eq!(
        world.with(|r, p, _| r.later(p, at(&order, &part, &s1), &early)),
        Err(ReversalError::NotAnswered)
    );
    assert_eq!(world.log.len(), appended);
}
