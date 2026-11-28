//! Reversals and ending with fakes (`T-020-12`): the router-api fixture's route across three providers, a withdrawal
//! at `reserve-bank-co`, an exchange at `exchange-a` inside its private `ledger-in` and a deposit at `reserve-wallet`,
//! run through the segments over a fake platform whose providers answer as each test scripts. The simulated
//! providers can't run it: their routes stay at one reserve, whose withdrawal is cancelled when it fails and whose
//! deposit owes no reversal, so no segment of theirs can owe one. Everything the reversals write is replayed through
//! core-internal's `attribute` and `admit_entry` over the log before it, so the log is seen to admit it.

mod snapshot_support;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::core_internal::calls::{
    AppendFailure, Decisions, Delivery, Evidence, Fetched, GeneralEntry, Kept, KeptStatements, Log,
    Operator, Reads, Signatures, StatementKeys, is_decision,
};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Afterwards, Answer, Attribution, Decision, DecisionRequest,
    DeploymentOperator, Destination, Difference, Effect, Entry, KeptStatement, Outcome,
    PermissionChange, ReadAnswer, ReadCheck, ReadOutcome, Receipt, RefusalClass, Role, RouteEnd,
    ShowingNamed, SignedWith, Through, What, admit_entry, attribute,
};
use rutent_contracts::module_interface::{Digest, Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{
    Denomination, EvidenceGrade, Operation, OuterNode, PublicKey, RESULT_SHAPE,
    REVERSAL_CONFIRMATION_SHAPE, Statement, signing_input, to_canonical,
};
use rutent_contracts::router_api::{
    DecisionRecord, Id, PolicyBounds, Timestamp, TupleKey as RouterTupleKey,
};
use rutent_contracts::state_machine::{self as stm, OrderState, RecordedInput, SegmentState};
use rutent_core_orch::quote::{ChargeDesk, ChargeError, Charged, Charges, Checked, Terms};
use rutent_core_orch::recalc::evidenced;
use rutent_core_orch::reversal::{
    Accepting, Asked, Asking, At, Carrier, Confirmation, Moving, NotCounted,
    Parts as ReversalParts, ReversalError, Reversals, StatusAsked, Word,
};
use rutent_core_orch::rounds::{Fresh, Issued, OperatorKey, Providers};
use rutent_core_orch::segments::{
    Answered, Graded, Grading, Live, Modules, NoProxy, Parts, ProvidersWord, Ran, SegmentOutcome,
    Segments, SegmentsError, StatusError, ToGrade,
};
use rutent_core_orch::snapshot::{Clock, Exposures, Reading};
use rutent_core_orch::stuck::{Stuck, StuckError};
use rutent_prov::catalogue::{Catalogue, Endpoint, Instant, Observation, Publisher, TupleKey};
use rutent_prov::module::lifecycle::{LifecycleRefusal, SegmentRecord};
use rutent_prov::module::types::Events;
use rutent_prov::proxy::RouteNotNamed;
use serde_json::{Value, json};

/// No public records: none of these reversals leaves a held public node, so none is read (`T-020-60` (6)).
struct NoRecords;

impl rutent_core_orch::closing::PublicRecords for NoRecords {
    fn rules(
        &self,
        _: &str,
    ) -> Result<Option<rutent_evidence::NetworkRules>, rutent_core_orch::closing::Unreadable> {
        Ok(None)
    }

    fn status(
        &self,
        _: &str,
        _: &rutent_evidence::MovementId,
    ) -> Result<rutent_evidence::Status, rutent_core_orch::closing::Unreadable> {
        Ok(rutent_evidence::Status::Unknown)
    }

    fn movement(
        &self,
        _: &str,
        _: &Id,
    ) -> Result<Option<rutent_core_orch::closing::Moved>, rutent_core_orch::closing::Unreadable>
    {
        Ok(None)
    }
}

const ORDER: &str = "ord-1";
const PART: &str = "p1";
const SEGMENTS: [&str; 3] = ["p1-s1", "p1-s2", "p1-s3"];
const RESERVE: &str = "reserve-bank-co";
const EXCHANGE: &str = "exchange-a";
const WALLET: &str = "reserve-wallet";
/// The ramp that delivers the value to `ledger-b` in the route across inner nodes (`T-020-27`).
const RAMP: &str = "ramp-x";
/// The exchange whose conversion starts at `ledger-b`, a later inner node, and fails there.
const EXCHANGE_B: &str = "exchange-b";
/// What the ramp took in and gave out, in COP.
const RAMPED: &str = "99895000";
/// The route across inner nodes: the withdrawal into `ledger-in`, the ramp's transfer from it into `ledger-b`,
/// `exchange-b`'s conversion at `ledger-b` and the deposit out of it.
const LATER_SEGMENTS: [&str; 4] = ["p1-s1", "p1-s2", "p1-s3", "p1-s4"];
/// What the exchange took in, in COP: what the withdrawal handed on.
const EXCHANGED: &str = "99900000";
/// What the withdrawal took in, in COP.
const WITHDRAWN: &str = "100000000";
const NOW: u64 = 1_794_200_000;
/// The customer's open session.
const SESSION: &str = "sess-customer";

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

/// The register of actors: the deployment operator's login, as policy's register lists it, and no other.
struct Listed(Vec<Actor>);

impl rutent_core_orch::intake::Actors for Listed {
    fn acts_for(&mut self, acts: &Id, party: &Id, kind: ActorKind, role: Role) -> bool {
        self.0
            .iter()
            .any(|a| &a.acts == acts && &a.party == party && a.kind == kind && a.role == role)
    }
}

fn listed() -> Listed {
    Listed(vec![operator()])
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
/// The evidence store's bytes, shared with the log when it attributes (`World::attributing`).
#[derive(Default)]
struct Store(Rc<RefCell<Vec<Vec<u8>>>>);

fn commitment(place: usize) -> Digest {
    serde_json::from_value(
        json!({ "algorithm": "sha-256", "value": format!("{:064x}", place + 1) }),
    )
    .unwrap()
}

impl Evidence for Store {
    fn keep(&mut self, bytes: &[u8]) -> Result<Kept, AppendFailure> {
        self.0.borrow_mut().push(bytes.to_vec());
        Ok(Kept {
            commitment: commitment(self.0.borrow().len() - 1),
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
    /// While set, the log answers every append with the receipt of another entry.
    misreceipting: bool,
    /// Where set, the log records as the audit log does (`World::attributing`).
    attribution: Option<Attributing>,
}

/// What the log reads to attribute a statement, as `core-audit`'s log does (`AuditLog::append_through`): the evidence
/// store's bytes and the catalogue's keys and publishers.
struct Attributing {
    store: Rc<RefCell<Vec<Vec<u8>>>>,
    catalogue: Catalogue,
    operator_known: bool,
}

impl Attributing {
    fn sources(&self) -> Sources<'_> {
        Sources {
            store: &self.store,
            catalogue: &self.catalogue,
            operator_known: self.operator_known,
        }
    }
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
        if self.misreceipting {
            let at = self.entries.borrow().len();
            self.entries.borrow_mut().push(entry.entry().clone());
            return Ok(receipt_of(at, &input_entry()));
        }
        let Some(real) = &self.attribution else {
            return Ok(push(&self.entries, entry.entry().clone()));
        };
        // The audit log's steps that change what it answers: it attributes a stated entry from its kept bytes (the
        // entry it records is not the one handed in), and an equal entry already held answers with its own receipt.
        // Its `admit_entry` is not run here: the policy this world stands in for writes its decisions raw, so
        // `World::admitted_from` replays it over what is recorded, as every test of this file does.
        let history: Vec<(Receipt, Entry)> = self
            .entries
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, e)| (receipt_of(i, e), e.clone()))
            .collect();
        let recorded = match entry.entry() {
            Entry::Stated(claim) => Entry::Stated(attribute(
                claim,
                &history,
                &mut real.sources(),
                &mut real.sources(),
                &real.sources(),
            )?),
            other => other.clone(),
        };
        if let Some(i) = history.iter().position(|(_, e)| *e == recorded) {
            return Ok(history[i].0.clone());
        }
        Ok(push(&self.entries, recorded))
    }
}

/// Policy that permits every move, its decision recorded in the log on its own handle.
struct Permitting {
    decided: usize,
    log: Entries,
    /// Refuses every request while set.
    refusing: bool,
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
            "outcome": if self.refusing {
                json!({ "status": "refused", "class": "not_permitted" })
            } else {
                json!({ "status": "permitted" })
            },
        }))
        .expect("a decide entry");
        Ok(Decision {
            request: request.request.clone(),
            decision,
            outcome: if self.refusing {
                serde_json::from_value(json!({ "answer": "refuse", "class": "not_permitted" }))
                    .expect("a refusal")
            } else {
                Answer::Permit {}
            },
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

/// Policy's reads: every read refused, for the class it is set to, and each check kept.
struct Refusing(RefusalClass, Vec<ReadCheck>);

impl Reads for Refusing {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        self.1.push(check.clone());
        Ok(ReadAnswer {
            request: check.request.clone(),
            outcome: ReadOutcome::Refused { class: self.0 },
            receipt: receipt_of(300_000 + self.1.len(), &input_entry()),
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
    /// Its conversions fail and every other operation settles.
    FailConversion,
    /// The module passed something out and ended with no answer.
    Unknown,
}

/// The platform: each provider answers as scripted, with a result it signs.
struct Platform {
    script: BTreeMap<&'static str, Act>,
    /// What a provider asked for its late status says its segment's move came to: `None`, nothing comes back.
    late: Option<Value>,
    /// The late statuses asked: the fresh transaction each was asked under, and the provider asked.
    statuses: Vec<(Id, Id)>,
}

impl Platform {
    fn settling() -> Self {
        Platform {
            script: BTreeMap::from([
                (RESERVE, Act::Settle),
                (EXCHANGE, Act::Settle),
                (WALLET, Act::Settle),
                (RAMP, Act::Settle),
                (EXCHANGE_B, Act::Settle),
            ]),
            late: None,
            statuses: Vec::new(),
        }
    }

    /// A provider asked for its late status says the segment's move came to `outcome`.
    fn saying(mut self, outcome: Value) -> Self {
        self.late = Some(outcome);
        self
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
        let act = match (self.script[provider.as_str()], kind.as_str()) {
            (Act::FailConversion, "conversion") => Act::Fail,
            (Act::FailConversion, _) => Act::Settle,
            (act, _) => act,
        };
        let outcome = match (act, kind.as_str()) {
            (Act::FailConversion, _) => unreachable!("read above"),
            (Act::Unknown, _) => return Answered::Refused(PlatformRefusal::OutcomeUnknown),
            (Act::Fail, _) => json!({ "status": "failed", "evidence": piece(provider.as_str()) }),
            (Act::Settle, "withdrawal") => json!({
                "status": "succeeded", "amount_in": WITHDRAWN, "amount_out": EXCHANGED,
                "fee": { "asset": "COP", "amount": "100000" }, "evidence": piece(provider.as_str()),
            }),
            (Act::Settle, "transfer") => json!({
                "status": "succeeded", "amount_in": EXCHANGED, "amount_out": RAMPED,
                "fee": { "asset": "COP", "amount": "5000" }, "evidence": piece(provider.as_str()),
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

    fn status(
        &mut self,
        _: &Effect,
        provider: &Id,
        asking: &Id,
        about: &Statement<Operation>,
    ) -> Answered {
        self.statuses.push((asking.clone(), provider.clone()));
        let op = serde_json::to_value(&about.statement).unwrap();
        let (kind, transaction) = (
            op["kind"].as_str().unwrap().to_owned(),
            op["transaction"].as_str().unwrap().to_owned(),
        );
        match &self.late {
            Some(outcome) => Answered::Bytes(result(
                provider.as_str(),
                &kind,
                &transaction,
                outcome.clone(),
            )),
            None => Answered::Refused(PlatformRefusal::OutcomeUnknown),
        }
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

thread_local! {
    /// What the segments told the grading, on this test's thread: the number of receipts each `recorded` call
    /// handed it, and how many times it was asked to raise (`T-020-22`).
    static TOLD: RefCell<(Vec<usize>, usize)> = const { RefCell::new((Vec::new(), 0)) };
}

impl Grading for PublicDeposit {
    fn recorded(&mut self, _: &ToGrade<'_>, receipts: &[Receipt]) {
        TOLD.with(|t| t.borrow_mut().0.push(receipts.len()));
    }

    fn raise(
        &mut self,
        _: rutent_core_orch::segments::RaiseDesk<'_>,
    ) -> Result<(), rutent_contracts::core_internal::calls::AppendFailure> {
        TOLD.with(|t| t.borrow_mut().1 += 1);
        Ok(())
    }

    fn grade(&mut self, settled: &ToGrade<'_>) -> Graded {
        // The withdrawal into `ledger-in`, a private node: its public movement into the node, graded a public
        // record, naming the node and resting on the reserve's result, as `closing::Evidenced` records it, so the log
        // binds the node as the publisher's the reserve states for (b38d30f).
        if settled.place == 0 {
            let entry: Entry = serde_json::from_value(json!({
                "kind": "verified",
                "who": settled.core,
                "what": { "call": "evidence", "evidence": settled.evidence, "order": settled.order,
                    "segment": settled.segment, "grade": 1, "whose": "co-ach", "rests_on": [settled.result],
                    "movement": { "network": "co-ach", "id": format!("mv-{}", settled.transaction.as_str()),
                        "node": "ledger-in", "read_by": { "reader": "co-ach-reader", "version": "0.1.0" } } },
                "when": { "at": settled.at, "source": "core_clock" },
                "outcome": { "status": "observed" },
            }))
            .unwrap();
            return Graded::Settles {
                cites: settled.evidence.clone(),
                entries: vec![entry],
            };
        }
        if !rutent_core_orch::closing::is_final_deposit(settled) {
            return ProvidersWord.grade(settled);
        }
        let entry: Entry = serde_json::from_value(json!({
            "kind": "verified",
            "who": settled.core,
            "what": { "call": "evidence", "evidence": settled.evidence, "order": settled.order,
                "segment": settled.segment, "grade": 1, "whose": "polygon", "rests_on": [settled.result],
                "movement": { "network": "polygon", "id": format!("mv-{}", settled.transaction.as_str()),
                    "read_by": { "reader": "polygon-reader", "version": "0.1.0" } } },
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
    let providers = [RESERVE, EXCHANGE, WALLET];
    let nodes: Vec<(&str, &str, Vec<&str>)> = providers
        .into_iter()
        .enumerate()
        .map(|(n, provider)| {
            let private = if n == ledger_of {
                vec!["ledger-in"]
            } else {
                vec![]
            };
            (provider, publishers[n], private)
        })
        .collect();
    catalogue_of(
        record,
        &nodes,
        &[("ledger-in", publishers[ledger_of], "holding")],
    )
}

/// A catalogue of `providers` (each with its publisher and the private nodes it declares) and the holding accounts
/// of `holdings` (node, publisher, reference), each tuple of `record`'s route under the weakest floor.
fn catalogue_of(
    record: &DecisionRecord,
    providers: &[(&str, &str, Vec<&str>)],
    holdings: &[(&str, &str, &str)],
) -> Catalogue {
    let mut catalogue = Catalogue::new();
    catalogue.set_context(
        snapshot_support::deployment(),
        rutent_contracts::provider_role::VERSION,
    );
    catalogue.add_public_node("polygon").unwrap();
    for (n, (provider, publisher, private)) in providers.iter().enumerate() {
        join(&mut catalogue, n, provider, publisher, private);
    }
    for (node, publisher, reference) in holdings {
        catalogue
            .register_private_holding(node, publisher, reference)
            .unwrap();
    }
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

/// The router-api fixture's record with its second candidate chosen, the route across two inner nodes (`T-020-27`): the
/// withdrawal into `ledger-in`, a ramp's transfer from `ledger-in` into `ledger-b`, `exchange-b`'s conversion at
/// `ledger-b` and the deposit out of it. The conversion starts at an inner node after the first, where the
/// withdrawal's reserve holds nothing of this order.
fn later_record() -> DecisionRecord {
    let mut value = serde_json::to_value(record()).unwrap();
    value["chosen"] = json!("c2");
    for candidate in value["candidates"].as_array_mut().unwrap() {
        match candidate["candidate_id"].as_str().unwrap() {
            "c2" => {
                candidate.as_object_mut().unwrap().remove("dropped");
            }
            "c1" => candidate["dropped"] = json!({ "reason": "outranked", "by": "c2" }),
            _ => {}
        }
    }
    serde_json::from_value(value).unwrap()
}

/// [`later_record`] with `exchange-b` doing the ramp's transfer as well: one provider delivers the value to `ledger-b`
/// and fails the conversion that starts there.
fn same_provider_record() -> DecisionRecord {
    let value = serde_json::to_value(later_record()).unwrap();
    // The snapshot holds one publisher for a provider: the ramp's figures take the exchange's.
    let publisher = value["snapshot"]["figures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["origin"]["provider"] == EXCHANGE_B)
        .map(|f| f["origin"]["publisher"].as_str().unwrap().to_owned())
        .unwrap();
    let text = serde_json::to_string(&value).unwrap();
    let text = text
        .replace(
            &format!("\"publisher\":\"{RAMP}\""),
            &format!("\"publisher\":\"{publisher}\""),
        )
        .replace(RAMP, EXCHANGE_B);
    serde_json::from_str(&text).unwrap()
}

/// The catalogue of the route across inner nodes: the bank's `ledger-in` and the ramp's `ledger-b`, each declared
/// private by its provider's publisher and held by it, except `ledger-b`'s holding account is `holding_of_b`, where
/// there is one, which the ramp's publisher gives.
fn later_catalogue(
    record: &DecisionRecord,
    holding_of_b: Option<&str>,
    deliverer: &str,
) -> Catalogue {
    let mut holdings = vec![("ledger-in", "bank-co", "holding")];
    holdings.extend(holding_of_b.map(|reference| ("ledger-b", "ramp-co", reference)));
    let mut providers = vec![
        (RESERVE, "bank-co", vec!["ledger-in"]),
        (deliverer, "ramp-co", vec!["ledger-b"]),
        (WALLET, "wallet-co", vec![]),
    ];
    if deliverer != EXCHANGE_B {
        providers.push((EXCHANGE_B, "bridge-co", vec![]));
    }
    catalogue_of(record, &providers, &holdings)
}

/// The order's `route_chosen`, of the record's chosen route.
fn route_chosen(record: &DecisionRecord) -> RecordedInput {
    route_chosen_of(record, &SEGMENTS)
}

/// The order's `route_chosen`, of the record's chosen route, its segments named `segments`.
fn route_chosen_of(record: &DecisionRecord, segments: &[&str]) -> RecordedInput {
    let chosen = serde_json::to_value(record.chosen().unwrap()).unwrap();
    let plan: Vec<Value> = chosen["parts"][0]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .zip(segments)
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
    /// The part's segments, in order.
    plan: Vec<Id>,
    platform: Platform,
    reversals: Reversals,
    stuck: Stuck,
    core: Actor,
    operator: Actor,
}

impl World {
    /// The order across two inner nodes over `platform` (`T-020-27`): its transfer starts at `ledger-b`, which
    /// `holding_of_b`, if any, holds as its holding account.
    fn across(platform: Platform, holding_of_b: Option<&str>) -> Self {
        let record = later_record();
        let catalogue = later_catalogue(&record, holding_of_b, RAMP);
        Self::assembled(platform, record, catalogue, &LATER_SEGMENTS)
    }

    /// [`World::across`] with `exchange-b` delivering the value to `ledger-b` as well as converting there.
    fn across_by_one(platform: Platform) -> Self {
        let record = same_provider_record();
        let catalogue = later_catalogue(&record, Some("holding-b"), EXCHANGE_B);
        Self::assembled(platform, record, catalogue, &LATER_SEGMENTS)
    }

    /// The order over `platform`, the exchange published by `acme`.
    fn over(platform: Platform) -> Self {
        Self::published(platform, ["bank-co", "acme", "wallet-co"], 0)
    }

    /// The order over `platform`, the bank, the exchange and the wallet published by `publishers`, `ledger-in`
    /// declared private by the `ledger_of`th of them, which holds it.
    fn published(platform: Platform, publishers: [&str; 3], ledger_of: usize) -> Self {
        let record = record();
        let catalogue = catalogue(&record, publishers, ledger_of);
        Self::assembled(platform, record, catalogue, &SEGMENTS)
    }

    /// The order over `platform` of `record`, whose segments are `segments`, its providers joined to `catalogue`.
    fn assembled(
        platform: Platform,
        record: DecisionRecord,
        catalogue: Catalogue,
        segments: &[&str],
    ) -> Self {
        let clock = TestClock(Cell::new(Reading::at(NOW)));
        let chosen = route_chosen_of(&record, segments);
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
        let plan: Vec<Id> = segments.iter().map(|s| id(s)).collect();
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
        // The customer's session, open, as policy's register records it on its handle.
        log.entries.borrow_mut().push(
            serde_json::from_value(json!({
                "kind": "verified",
                "who": { "party": "operator", "acts": "policy", "role": "core", "kind": "component" },
                "what": { "call": "open_session", "request": {
                        "request": "req-session", "account": "acct-1", "holder": customer().party,
                        "requester": customer(), "until": "2027-01-01T00:00:00Z", "at": "2026-11-10T09:00:00Z" },
                    "opened": { "session": SESSION, "expires_at": "2027-01-01T00:00:00Z" } },
                "when": { "at": "2026-11-10T09:00:00Z", "source": "core_clock" },
                "outcome": { "status": "permitted" },
            }))
            .expect("an open session"),
        );
        let shared = Rc::clone(&log.entries);
        let mut reversals = Reversals::new();
        let history: Vec<(Receipt, Entry)> = log
            .entries()
            .iter()
            .enumerate()
            .map(|(i, e)| (receipt_of(i, e), e.clone()))
            .collect();
        assert!(reversals.follow(&id(ORDER), &history));
        let world = World {
            catalogue,
            clock,
            log,
            store: Store::default(),
            policy: Permitting {
                decided: 0,
                log: shared,
                refusing: false,
            },
            key: TestKey(snapshot_support::operator_key()),
            ids: Ids::default(),
            issued: Issued::new(),
            exposures: Exposures::new(),
            segments,
            record,
            plan,
            platform,
            reversals,
            stuck: Stuck::new(),
            core: core(),
            operator: operator(),
        };
        world.attributing()
    }

    /// The world over a log that records as the audit log does: it attributes a stated entry from its kept bytes, so
    /// the entry it records, and its receipt, are not the entry Core handed it.
    fn attributing(mut self) -> Self {
        self.log.attribution = Some(Attributing {
            store: Rc::clone(&self.store.0),
            catalogue: self.catalogue.clone(),
            operator_known: true,
        });
        self
    }

    /// Runs the order's part through the segments.
    fn run(&mut self) -> Result<Ran, SegmentsError> {
        let (order, part, auth, origin) = (id(ORDER), id(PART), id("auth-1"), id(RESERVE));
        let plan = self.plan.clone();
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
            record_entry: None,
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
            channel: &mut rutent_core_orch::notice::Unbuilt,
        };
        self.segments.run_part(&mut parts, &live)
    }

    /// The outcomes of the part's segments, run once.
    fn ran(&mut self) -> Vec<SegmentOutcome> {
        let done = match self.run().expect("the part runs") {
            Ran::Segments(done) => done.into_iter().map(|a| a.outcome).collect(),
            refused => panic!("{refused:?}"),
        };
        // The assembly follows the order again from the log as it grows: the nodes it binds.
        let history = self.history();
        assert!(self.reversals.follow(&id(ORDER), &history));
        done
    }

    /// The log's entries as the log holds them: each stated entry attributed from its kept bytes, each with the
    /// receipt it was handed.
    fn history(&self) -> Vec<(Receipt, Entry)> {
        let mut history: Vec<(Receipt, Entry)> = Vec::new();
        for (i, appended) in self.log.entries().iter().enumerate() {
            let entry = match appended {
                Entry::Stated(claim)
                    if matches!(claim.attribution, Attribution::Unverified { .. })
                        && self.log.attribution.is_none() =>
                {
                    let mut sources = Sources::of(self);
                    let mut keys = Sources::of(self);
                    Entry::Stated(
                        attribute(claim, &history, &mut sources, &mut keys, &Sources::of(self))
                            .expect("attributed"),
                    )
                }
                verified => verified.clone(),
            };
            history.push((receipt_of(i, appended), entry));
        }
        history
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
        // Core asks a confirmation again only on its retry cadence (the review of #355's round 2, H1): each question
        // here is put a cadence after the last, as the deployment's timer would put it.
        self.clock.0.set(Reading::at(
            self.clock.0.get().floor().0
                + rutent_core_orch::reversal::DEFAULT_REVERSAL_RETRY_SECONDS,
        ));
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
            records: &rutent_core_orch::closing::Public::stand_in(&NoRecords),
            closing: &rutent_core_orch::closing::Closing::new(),
        };
        self.reversals.confirm_reversal(&mut parts, &mut asking, at)
    }

    /// `by` accepting the last showing of the exchange's fault for `p1-s2`, as the customer: policy's decision of its
    /// `accept_fault`, asked a second after the showing arrived, in the customer's open session, and Core citing it.
    fn accept_as_customer(&mut self, by: &Actor) -> Result<Carrier, ReversalError> {
        self.accept_as_customer_naming(by, None)
    }

    /// [`World::accept_as_customer`], its decision naming `commitment` in place of the showing's, where given.
    fn accept_as_customer_naming(
        &mut self,
        by: &Actor,
        commitment: Option<Digest>,
    ) -> Result<Carrier, ReversalError> {
        let request = self.acceptance_request(by, commitment);
        self.policy.decide(&request).expect("policy decides");
        self.accept_decided(by)
    }

    /// `by` asking policy to accept the last showing of the exchange's fault for `p1-s2`, a second after the Core
    /// clock last read, in the customer's session: naming `commitment` in place of the showing's, where given.
    fn acceptance_request(&mut self, by: &Actor, commitment: Option<Digest>) -> DecisionRequest {
        let showing = self
            .log
            .entries()
            .iter()
            .rev()
            .find_map(|e| match e {
                Entry::Stated(s)
                    if s.what.statement.as_str() == RESULT_SHAPE
                        && s.about
                            .as_ref()
                            .and_then(|a| a.segment.as_ref())
                            .map(|x| x.as_str())
                            == Some("p1-s2") =>
                {
                    Some(s.clone())
                }
                _ => None,
            })
            .expect("a showing");
        let transaction = showing
            .about
            .as_ref()
            .and_then(|a| a.transaction.clone())
            .unwrap();
        self.clock
            .0
            .set(Reading::at(self.clock.0.get().floor().0 + 1));
        DecisionRequest {
            request: id(&format!("req-accept-{}", self.policy.decided + 1)),
            caller: by.clone(),
            action: Action::AcceptFault {
                order: id(ORDER),
                segment: id("p1-s2"),
                showing: ShowingNamed {
                    transaction,
                    commitment: commitment.unwrap_or(showing.commitment),
                },
                session: id(SESSION),
            },
            at: Timestamp::new(Instant(self.clock.0.get().floor().0).rfc3339().unwrap()).unwrap(),
        }
    }

    /// The exchange's failed result naming the customer at fault, shown to Core for `p1-s2`.
    fn show_customers_fault(&mut self) {
        let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
        let tx = self
            .segments
            .sent(&order, &s2)
            .unwrap()
            .0
            .as_str()
            .to_owned();
        let shown = result(
            EXCHANGE,
            "conversion",
            &tx,
            json!({ "status": "failed", "fault": "customer", "evidence": piece(EXCHANGE) }),
        );
        self.with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &shown))
            .unwrap();
    }

    /// Core accepting the fault for `by`, reading policy's decision from the log as it follows it now.
    fn accept_decided(&mut self, by: &Actor) -> Result<Carrier, ReversalError> {
        let history = self.history();
        assert!(self.reversals.follow(&id(ORDER), &history));
        let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
        self.with(|r, p, _| {
            r.accept_fault(
                p,
                &mut listed(),
                at(&order, &part, &s2),
                Accepting::Customer { by },
            )
        })
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
            records: &rutent_core_orch::closing::Public::stand_in(&NoRecords),
            closing: &rutent_core_orch::closing::Closing::new(),
        };
        let mut moving = Moving {
            decisions: &mut self.policy,
            modules: &mut self.platform,
        };
        let asked = self.reversals.ask(&mut parts, &mut asking, &mut moving, at);
        assert!(no_one.asked.is_empty(), "no reserve is asked to reverse");
        asked
    }

    /// Asks the provider of `segment` where it stands, late, policy deciding and the platform carrying it out.
    fn ask_status(&mut self, segment: &str) -> Result<StatusAsked, ReversalError> {
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
        let mut moving = Moving {
            decisions: &mut self.policy,
            modules: &mut self.platform,
        };
        self.reversals.ask_status(&mut parts, &mut moving, at)
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
                Entry::Stated(claim)
                    if matches!(claim.attribution, Attribution::Unverified { .. })
                        && self.log.attribution.is_none() =>
                {
                    let mut sources = Sources::of(self);
                    let mut keys = Sources::of(self);
                    Entry::Stated(
                        attribute(claim, &history, &mut sources, &mut keys, &Sources::of(self))
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
struct Sources<'a> {
    store: &'a Rc<RefCell<Vec<Vec<u8>>>>,
    catalogue: &'a Catalogue,
    /// Whether the log knows the operator's key and party; where not, it records Core's statements unattributed.
    operator_known: bool,
}

impl<'a> Sources<'a> {
    fn of(world: &'a World) -> Self {
        Sources {
            store: &world.store.0,
            catalogue: &world.catalogue,
            operator_known: true,
        }
    }
}

impl KeptStatements for Sources<'_> {
    fn kept(&mut self, wanted: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        let bytes = self.store.borrow();
        let place = (0..bytes.len()).find(|i| &commitment(*i) == wanted);
        Ok(place.map(|i| {
            serde_json::from_value(json!({
                "commitment": wanted,
                "salt": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "bytes": snapshot_support::base64url(&bytes[i]),
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
            return Ok(self.operator_known.then(|| operator().party));
        }
        Ok(
            match self.catalogue.publishers().publisher(maker.as_str()) {
                Some(Publisher::Operator) => Some(operator().party),
                Some(Publisher::Party(party)) => Id::new(party),
                None => None,
            },
        )
    }

    fn signs_for(
        &mut self,
        _: &Id,
        _: &SignedWith,
        _: &Timestamp,
        _: &Timestamp,
    ) -> Result<bool, AppendFailure> {
        Ok(true)
    }
}

impl Signatures for Sources<'_> {
    fn verifies(&self, received: &[u8]) -> bool {
        let Ok(value) = serde_json::from_slice::<Value>(received) else {
            return false;
        };
        // Ed25519 signs deterministically: the operator's statement verifies where signing it again gives it.
        let maker = value["maker"].as_str().unwrap_or_default().to_owned();
        let key = (maker == operator().acts.as_str()).then(snapshot_support::operator_key);
        if let Some(key) = key {
            let mut again = value.clone();
            snapshot_support::sign_value(&mut again, &key);
            return again == value && signing_input(received).is_ok();
        }
        self.catalogue
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
        .with(|r, p, _| r.report(p, at(&order, &part, &s2), &own, &presenter_of(&own)))
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
    // Policy refusing, nothing is asked or recorded but its decision.
    let mut world = exchange_failed();
    world.policy.refusing = true;
    let mark = world.log.len();
    assert!(matches!(
        world.ask("p1-s2"),
        Err(ReversalError::NotPermitted(_))
    ));
    assert!(
        !world
            .whats_from(mark)
            .iter()
            .any(|w| matches!(w, What::Effect { .. })),
        "no effect without a permit"
    );

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
        (
            EXCHANGE,
            exchange_tx.as_str(),
            "ledger-in",
            "COP",
            EXCHANGED
        )
    );
    assert_ne!(
        reversal.transaction,
        exchange_tx.as_str(),
        "under a fresh transaction"
    );
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
        (
            EXCHANGE,
            EvidenceGrade::Statement,
            &Word::Publishers(id("bank-co"))
        )
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
        world.with(|r, p, _| r.accept_fault(
            p,
            &mut listed(),
            at(&order, &part, &s2),
            Accepting::Operator { by: &operator() }
        )),
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
    // An operator of another party isn't the deployment's operator.
    let impostor = Actor {
        party: id("other-operator"),
        ..operator()
    };
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(
            p,
            &mut listed(),
            at(&order, &part, &s2),
            Accepting::Operator { by: &impostor }
        )),
        Err(ReversalError::NotTheParty)
    );
    // A login the register doesn't list for the operator's party, stating its party and role, is no operator
    // (`T-020-62`): it accepts nothing, and nothing is appended.
    let unlisted = Actor {
        acts: id("not-in-the-register"),
        ..operator()
    };
    let len = world.log.len();
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(
            p,
            &mut listed(),
            at(&order, &part, &s2),
            Accepting::Operator { by: &unlisted }
        )),
        Err(ReversalError::NotTheParty),
        "an operator login the register doesn't list cannot accept a fault"
    );
    assert_eq!(world.log.len(), len);
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(
            p,
            &mut listed(),
            at(&order, &part, &s2),
            Accepting::Operator { by: &operator() }
        )),
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
    assert_eq!(
        world.admitted_from(mark_operator),
        Ok(world.log.len() - mark_operator)
    );
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
        world.accept_as_customer(&customer()),
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
        world.accept_as_customer(&customer()).is_err(),
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
        world.accept_as_customer(&stranger),
        Err(ReversalError::NotTheParty)
    );
    // Before any decision of its own showing is in the log: the one naming another showing is no acceptance.
    let other: Digest =
        serde_json::from_value(json!({ "algorithm": "sha-256", "value": "ab".repeat(32) }))
            .unwrap();
    assert_eq!(
        world.accept_as_customer_naming(&customer(), Some(other)),
        Err(ReversalError::NotAnAcceptance),
        "an acceptance naming another showing than the provider's"
    );
    // An acceptance the log couldn't take changes no carrier.
    world.log.down = true;
    assert_eq!(
        world.accept_as_customer(&customer()),
        Err(ReversalError::Append(AppendFailure::Unavailable))
    );
    world.log.down = false;
    assert_eq!(
        world.reversals.carrier(&order, &s2),
        Some(&Carrier::Provider(id(EXCHANGE)))
    );
    let mark_customer = world.log.len();
    assert_eq!(world.accept_as_customer(&customer()), Ok(Carrier::Customer));
    assert!(world.whats_from(mark_customer).iter().any(|w| matches!(
        w,
        What::ReversalCarrier { carrier: rutent_contracts::core_internal::Carrier::Customer, accepted_by: Some(by), .. }
            if *by == customer()
    )));
    assert_eq!(
        world.admitted_from(mark_customer),
        Ok(world.log.len() - mark_customer)
    );
    assert_ne!(world.followed().parts[0].state, OrderState::Stuck);

    // Once the reversal completed, no carrier is recorded for it: nothing is owed.
    let mut world = exchange_failed();
    world
        .with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &showing(EXCHANGE, "operator")))
        .unwrap();
    assert!(matches!(
        world
            .confirm(&mut Answering::confirming(), "p1-s2")
            .unwrap(),
        Confirmation::Completed(_)
    ));
    assert_eq!(
        world.with(|r, p, _| r.accept_fault(
            p,
            &mut listed(),
            at(&order, &part, &s2),
            Accepting::Operator { by: &operator() }
        )),
        Err(ReversalError::NotOwed)
    );
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
        .with(|r, p, _| r.report(p, at(&order, &part, &s2), &own, &presenter_of(&own)))
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
    let (mark, kept_bytes) = (world.log.len(), world.store.0.borrow().len());
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
            .with(|r, p, _| r.report(p, at(&order, &part, &s1), piece, &presenter_of(piece)))
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
        world.store.0.borrow().len(),
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

    // Past its first segment: the withdrawal settled, the exchange failed. The part goes on: an end ends only parts
    // not past their first segment (`part-waits-for-value-in-motion` reads those alone).
    let mut world = exchange_failed();
    let appended = world.log.len();
    assert!(matches!(
        world.with(|r, p, _| r.end(p, &order)),
        Err(ReversalError::NotApplicable(_))
    ));
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
        world
            .confirm(&mut Answering::confirming(), "p1-s2")
            .unwrap(),
        Confirmation::Completed(_)
    ));
    assert!(matches!(
        world
            .confirm(&mut Answering::confirming(), "p1-s1")
            .unwrap(),
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

/// The segments tell the grading the receipts of the entries it answered for each segment that settles, one entry
/// each here, and ask it to raise on every run of the part, so a raise it couldn't record is recorded when it can
/// (`T-020-22`).
// verifies: REQ-ORD-9
#[test]
fn the_grading_is_told_what_it_answered_and_asked_to_raise_on_every_run() {
    TOLD.with(|t| *t.borrow_mut() = (Vec::new(), 0));
    let mut world = World::over(Platform::settling());
    let ran = world.ran();
    assert_eq!(ran.len(), 3, "{ran:?}");
    let (told, asked) = TOLD.with(|t| t.borrow().clone());
    assert_eq!(
        told,
        [1, 1, 1],
        "each settled segment's receipts, one entry each"
    );
    assert!(asked >= 1, "asked to raise on the run");
    let again = TOLD.with(|t| t.borrow().1);
    world.ran();
    assert!(
        TOLD.with(|t| t.borrow().1) > again,
        "asked on every run, also where nothing is handed over"
    );
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

/// What came after an order, read by a stranger: policy refuses the read, whatever the reason, and the stranger is told
/// one bare refusal, the same for an order that ended and for one that doesn't exist; the reason stays in the log.
// verifies: REQ-ORD-25, REQ-AUZ-7
#[test]
fn a_refused_reader_of_what_came_after_is_told_the_same_whether_the_order_exists() {
    let mut world = World::over(Platform::settling());
    world.with(|r, p, _| r.end(p, &id(ORDER))).unwrap();
    let stranger = Actor {
        party: id("stranger"),
        ..customer()
    };
    for class in [RefusalClass::NotPermitted, RefusalClass::MissingInput] {
        let mut told = Vec::new();
        for order in [id(ORDER), id("ord-nobody")] {
            let mut reads = Refusing(class, Vec::new());
            told.push(world.reversals.seen(
                &mut reads,
                &world.clock,
                &mut world.ids,
                &stranger,
                None,
                &order,
            ));
            assert_eq!(reads.1.len(), 1, "the read was checked and recorded");
        }
        assert_eq!(told[0], told[1], "{class:?}");
        assert!(matches!(told[0], Err(StatusError::NotReadable)), "{told:?}");
    }
}

/// A stranger accepting a fault is told it is not the party, before anything of the order, its showing or its state:
/// the same for a real segment with a showing, with none, and for an order and a segment that don't exist.
// verifies: REQ-ORD-11, REQ-AUZ-7
#[test]
fn a_stranger_accepting_fault_learns_nothing_of_the_order() {
    let mut world = exchange_failed();
    let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
    let tx = world
        .segments
        .sent(&order, &s2)
        .unwrap()
        .0
        .as_str()
        .to_owned();
    let showing = result(
        EXCHANGE,
        "conversion",
        &tx,
        json!({ "status": "failed", "fault": "operator", "evidence": piece(EXCHANGE) }),
    );
    let stranger = Actor {
        party: id("stranger"),
        ..operator()
    };
    let ask = |world: &mut World, order: &Id, part: &Id, segment: &Id| {
        world.with(|r, p, _| {
            r.accept_fault(
                p,
                &mut listed(),
                at(order, part, segment),
                Accepting::Operator { by: &stranger },
            )
        })
    };
    let answers = |world: &mut World| {
        vec![
            ask(world, &order, &part, &s2),
            ask(world, &order, &part, &id("p9-s9")),
            ask(world, &id("ord-nobody"), &part, &s2),
        ]
    };
    let before_a_showing = answers(&mut world);
    world
        .with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &showing))
        .unwrap();
    let with_a_showing = answers(&mut world);
    for told in before_a_showing.iter().chain(&with_a_showing) {
        assert_eq!(told, &Err(ReversalError::NotTheParty), "{told:?}");
    }
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
        .with(|r, p, _| r.later(p, at(&order, &part, &s2), &late, &presenter_of(&late)))
        .unwrap();
    let again = world
        .with(|r, p, _| r.later(p, at(&order, &part, &s2), &late, &presenter_of(&late)))
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
        world.with(|r, p, _| r.later(p, at(&order, &part, &s1), &early, &presenter_of(&early))),
        Err(ReversalError::NotAnswered)
    );
    assert_eq!(world.log.len(), appended);
}

/// A late statement whose body is one Core holds, made at another time and signed again, is that result held: no second
/// statement. One whose body differs is kept beside it, a further statement that moves nothing, so a provider that
/// contradicts itself is on the log (`REQ-AUD-2`, `REQ-AUD-5`, `INV-4`, `T-020-47`).
// verifies: REQ-AUD-2, INV-4
#[test]
fn late_statement_of_another_body_is_kept_beside_the_result_held_and_of_the_same_body_is_not() {
    let mut world = exchange_failed();
    let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
    let tx = world.segments.sent(&order, &s2).unwrap().0.clone();
    let delivered = |amount_out: &str| {
        result(
            EXCHANGE,
            "conversion",
            tx.as_str(),
            json!({ "status": "succeeded", "amount_in": EXCHANGED, "amount_out": amount_out, "rate": "2.5",
                "fee": { "asset": "COP", "amount": "0" }, "evidence": piece(EXCHANGE) }),
        )
    };
    let held = delivered("249250500");
    let keep = |world: &mut World, bytes: &[u8]| {
        world
            .with(|r, p, _| r.later(p, at(&order, &part, &s2), bytes, &presenter_of(bytes)))
            .unwrap()
    };
    let first = keep(&mut world, &held);
    let mark = world.log.len();
    // The same body, made at another time.
    let mut again: serde_json::Value = serde_json::from_slice(&held).unwrap();
    again["made_at"] = "2026-11-06T15:04:06Z".into();
    let again = serde_json::to_vec(&again).unwrap();
    assert_ne!(again, held);
    assert_eq!(keep(&mut world, &again), first, "the result held");
    assert_eq!(world.log.len(), mark, "no second statement");
    // Another body: the provider says another thing, kept beside the first.
    let other = delivered("249000000");
    let second = keep(&mut world, &other);
    assert_ne!(second, first);
    assert_eq!(world.log.len(), mark + 1, "one further statement");
    assert_eq!(keep(&mut world, &other), second, "kept once");
    assert!(
        world.whats_from(mark).is_empty(),
        "nothing moved, nothing recorded but statements"
    );
    assert_eq!(world.state_of("p1-s2"), SegmentState::FailedReversalOwed);
    assert!(world.held("p1-s2"));
    assert_eq!(
        world.admitted_from(mark - 1),
        Ok(world.log.len() - mark + 1)
    );
}

/// A later answer is the result held only where its maker is the segment's provider and its body is equal: another
/// provider's answer of the same body is a further statement, kept beside it, never the provider's result held
/// (`REQ-AUD-2`, `INV-4`).
// verifies: REQ-AUD-2, INV-4
#[test]
fn another_providers_answer_of_the_same_body_is_not_the_result_held() {
    let mut world = exchange_failed();
    let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
    let tx = world.segments.sent(&order, &s2).unwrap().0.clone();
    let delivered = |maker: &str| {
        result(
            maker,
            "conversion",
            tx.as_str(),
            json!({ "status": "succeeded", "amount_in": EXCHANGED, "amount_out": "249250500", "rate": "2.5",
                "fee": { "asset": "COP", "amount": "0" }, "evidence": piece(EXCHANGE) }),
        )
    };
    let keep = |world: &mut World, bytes: &[u8]| {
        world
            .with(|r, p, _| r.later(p, at(&order, &part, &s2), bytes, &presenter_of(bytes)))
            .unwrap()
    };
    let held = keep(&mut world, &delivered(EXCHANGE));
    let mark = world.log.len();
    // The same figures, said by another provider: its own statement, not the provider's result held.
    let other = keep(&mut world, &delivered(EXCHANGE_B));
    assert_ne!(other, held, "not the result held");
    assert_eq!(world.log.len(), mark + 1, "kept beside it");
    // And the provider's own again is still the one entry.
    assert_eq!(keep(&mut world, &delivered(EXCHANGE)), held);
    assert_eq!(world.log.len(), mark + 1);
}

/// The held side of the same rule (the review of `T-020-48`'s first build, its L1): another provider's answer kept
/// first, and the segment's own provider's answer of the same body after it, is the provider's own statement, kept
/// beside the other's, never folded into it: a statement is the result held only where the one held is the provider's
/// too (`REQ-AUD-2`, `INV-4`).
// verifies: REQ-AUD-2, INV-4
#[test]
fn providers_own_answer_after_another_providers_of_the_same_body_is_kept_as_its_own() {
    let mut world = exchange_failed();
    let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
    let tx = world.segments.sent(&order, &s2).unwrap().0.clone();
    let delivered = |maker: &str| {
        result(
            maker,
            "conversion",
            tx.as_str(),
            json!({ "status": "succeeded", "amount_in": EXCHANGED, "amount_out": "249250500", "rate": "2.5",
                "fee": { "asset": "COP", "amount": "0" }, "evidence": piece(EXCHANGE) }),
        )
    };
    let keep = |world: &mut World, bytes: &[u8]| {
        world
            .with(|r, p, _| r.later(p, at(&order, &part, &s2), bytes, &presenter_of(bytes)))
            .unwrap()
    };
    let other = keep(&mut world, &delivered(EXCHANGE_B));
    let mark = world.log.len();
    let own = keep(&mut world, &delivered(EXCHANGE));
    assert_ne!(
        own, other,
        "the provider's answer is its own, not another's held"
    );
    assert_eq!(world.log.len(), mark + 1, "kept beside it");
    // And each again is its own one entry.
    assert_eq!(keep(&mut world, &delivered(EXCHANGE)), own);
    assert_eq!(keep(&mut world, &delivered(EXCHANGE_B)), other);
    assert_eq!(world.log.len(), mark + 1);
}

/// A statement that says the outcome is unknown, kept, shadows no final delivery after it: the final one is kept as the
/// provider's result of the segment, beside the unknown one, and moves nothing twice (`REQ-AUD-5`, `INV-4`, `T-020-47`).
// verifies: REQ-AUD-5, INV-4
#[test]
fn kept_unknown_statement_does_not_shadow_a_later_final_delivery() {
    let mut world = exchange_failed();
    let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
    let tx = world.segments.sent(&order, &s2).unwrap().0.clone();
    let say = |outcome: serde_json::Value| result(EXCHANGE, "conversion", tx.as_str(), outcome);
    let unknown = say(json!({ "status": "unknown" }));
    let delivered = say(
        json!({ "status": "succeeded", "amount_in": EXCHANGED, "amount_out": "249250500", "rate": "2.5",
            "fee": { "asset": "COP", "amount": "0" }, "evidence": piece(EXCHANGE) }),
    );
    let keep = |world: &mut World, bytes: &[u8]| {
        world
            .with(|r, p, _| r.later(p, at(&order, &part, &s2), bytes, &presenter_of(bytes)))
            .unwrap()
    };
    let before = world.log.len();
    let kept_unknown = keep(&mut world, &unknown);
    let kept_final = keep(&mut world, &delivered);
    assert_ne!(
        kept_final, kept_unknown,
        "the final delivery is not the unknown statement"
    );
    assert_eq!(world.log.len(), before + 2, "both statements, once each");
    // The final one again, made at another time, is the final result held, not the unknown one.
    let mut again: serde_json::Value = serde_json::from_slice(&delivered).unwrap();
    again["made_at"] = "2026-11-06T15:04:06Z".into();
    let again = serde_json::to_vec(&again).unwrap();
    assert_eq!(keep(&mut world, &again), kept_final);
    assert_eq!(world.log.len(), before + 2);
    assert!(world.whats_from(before).is_empty(), "nothing moved");
    assert_eq!(world.state_of("p1-s2"), SegmentState::FailedReversalOwed);
    assert_eq!(world.admitted_from(before), Ok(2));
}

/// The customer's acceptance is policy's permitted decision of its own `accept_fault` for this showing, and the
/// carrier cites that decision: a refused decision, one of another caller's request, one answering another request,
/// another action, another segment's and another showing's are each no acceptance, and change no carrier.
// verifies: REQ-ORD-11
#[test]
fn customers_acceptance_is_policys_permitted_decision_and_the_carrier_cites_it() {
    let (order, s2) = (id(ORDER), id("p1-s2"));
    let mut world = exchange_failed();
    let part = id(PART);
    let tx = world
        .segments
        .sent(&order, &s2)
        .unwrap()
        .0
        .as_str()
        .to_owned();
    let shown = result(
        EXCHANGE,
        "conversion",
        &tx,
        json!({ "status": "failed", "fault": "customer", "evidence": piece(EXCHANGE) }),
    );
    world
        .with(|r, p, _| r.show_fault(p, at(&order, &part, &s2), &shown))
        .unwrap();
    let still_the_providers = |world: &World| {
        assert_eq!(
            world.reversals.carrier(&id(ORDER), &id("p1-s2")),
            Some(&Carrier::Provider(id(EXCHANGE)))
        );
    };

    // Policy refused it.
    world.policy.refusing = true;
    assert_eq!(
        world.accept_as_customer(&customer()),
        Err(ReversalError::NotAnAcceptance)
    );
    world.policy.refusing = false;
    still_the_providers(&world);

    // A decision of a request another asked, answered to the customer, or of another request.
    let stranger = Actor {
        party: id("customer-2"),
        acts: id("customer-2"),
        ..customer()
    };
    let theirs = world.acceptance_request(&stranger, None);
    world.policy.decide(&theirs).unwrap();
    assert_eq!(
        world.accept_decided(&customer()),
        Err(ReversalError::NotAnAcceptance),
        "a request of another caller's"
    );
    let ours = world.acceptance_request(&customer(), None);
    assert_eq!(
        world.accept_decided(&customer()),
        Err(ReversalError::NotAnAcceptance),
        "no decision of its own request is in the log: the one there is another's"
    );
    let _ = ours;
    // Another action, or another segment's acceptance.
    let mut elsewhere = world.acceptance_request(&customer(), None);
    if let Action::AcceptFault { segment, .. } = &mut elsewhere.action {
        *segment = id("p1-s3");
    }
    world.policy.decide(&elsewhere).unwrap();
    assert_eq!(
        world.accept_decided(&customer()),
        Err(ReversalError::NotAnAcceptance),
        "another segment's"
    );
    for (what, change) in [
        (
            "another order's",
            Box::new(|a: &mut Action| {
                if let Action::AcceptFault { order, .. } = a {
                    *order = id("ord-2");
                }
            }) as Box<dyn Fn(&mut Action)>,
        ),
        (
            "another transaction's",
            Box::new(|a: &mut Action| {
                if let Action::AcceptFault { showing, .. } = a {
                    showing.transaction = id("tx-elsewhere");
                }
            }),
        ),
    ] {
        let mut request = world.acceptance_request(&customer(), None);
        change(&mut request.action);
        world.policy.decide(&request).unwrap();
        assert_eq!(
            world.accept_decided(&customer()),
            Err(ReversalError::NotAnAcceptance),
            "{what}"
        );
    }
    let mut another = world.acceptance_request(&customer(), None);
    another.action = Action::AskReversal {
        order: id(ORDER),
        segment: id("p1-s2"),
        provider: id(EXCHANGE),
    };
    world.policy.decide(&another).unwrap();
    assert_eq!(
        world.accept_decided(&customer()),
        Err(ReversalError::NotAnAcceptance),
        "another action"
    );
    still_the_providers(&world);

    // The decision of the customer's own request: the carrier cites its receipt.
    let mark = world.log.len();
    let request = world.acceptance_request(&customer(), None);
    let decision = world.policy.decide(&request).unwrap();
    assert_eq!(world.accept_decided(&customer()), Ok(Carrier::Customer));
    assert!(world.whats_from(mark).iter().any(|w| matches!(
        w,
        What::ReversalCarrier { carrier: rutent_contracts::core_internal::Carrier::Customer,
            accepted_by: Some(by), accepted_with: Some(act), .. }
            if *by == customer() && *act == decision.receipt
    )));
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
    assert_eq!(
        world.reversals.carrier(&order, &s2),
        Some(&Carrier::Customer)
    );
}

/// The customer's `accept_fault` decision is read from Core's own log: the permitted decide of the request for the
/// order, segment and showing, and nothing the caller holds. A refused decision of an earlier request, still held
/// by the caller, carries nothing and blocks nothing: Core follows the log, so what it holds is what it decides on.
// verifies: REQ-ORD-11
#[test]
fn customers_acceptance_of_a_fault_is_read_from_the_log_and_not_from_the_callers_copy() {
    let (order, s2) = (id(ORDER), id("p1-s2"));
    let mut world = exchange_failed();
    world.show_customers_fault();
    // A refused decision of an earlier request, which the caller still holds.
    world.policy.refusing = true;
    let stale_request = world.acceptance_request(&customer(), None);
    world.policy.decide(&stale_request).unwrap();
    world.policy.refusing = false;
    // The customer's own request, which the log holds as permitted.
    let request = world.acceptance_request(&customer(), None);
    let decision = world.policy.decide(&request).unwrap();
    let mark = world.log.len();
    assert_eq!(
        world.accept_decided(&customer()),
        Ok(Carrier::Customer),
        "the log's permitted decision carries it, not the earlier refused one"
    );
    assert!(world.whats_from(mark).iter().any(|w| matches!(
        w,
        What::ReversalCarrier { accepted_with: Some(act), .. } if *act == decision.receipt
    )));
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
    assert_eq!(
        world.reversals.carrier(&order, &s2),
        Some(&Carrier::Customer)
    );
}

/// A decision the log doesn't hold as permitted doesn't carry the reversal, whatever a caller holds or says: here
/// the log holds a refusal of the customer's request.
// verifies: REQ-ORD-11
#[test]
fn callers_decision_that_the_log_does_not_hold_as_permitted_does_not_carry_the_reversal() {
    let (order, s2) = (id(ORDER), id("p1-s2"));
    let mut world = exchange_failed();
    world.show_customers_fault();
    world.policy.refusing = true;
    let request = world.acceptance_request(&customer(), None);
    world.policy.decide(&request).unwrap();
    world.policy.refusing = false;
    let mark = world.log.len();
    assert_eq!(
        world.accept_decided(&customer()),
        Err(ReversalError::NotAnAcceptance)
    );
    assert_eq!(world.log.len(), mark, "nothing is appended");
    assert_eq!(
        world.reversals.carrier(&order, &s2),
        Some(&Carrier::Provider(id(EXCHANGE)))
    );
}

// A segment's charge ------------------------------------------------------------------------------------------------
//
// Over a world that ran, since a raise rests on a log that holds the segment's move and its provider's verified result
// (`charge-raise-rests-on-its-result`) and the terms in force at its start (`charge-raise-rests-on-terms-in-force`).

/// The operator's channel: every raise it is handed, delivered or not as set.
struct Channel {
    raised: Vec<rutent_contracts::core_internal::Raise>,
    deliver: bool,
}

impl Operator for Channel {
    fn raise(&mut self, raise: &rutent_contracts::core_internal::Raise) -> Delivery {
        self.raised.push(raise.clone());
        if self.deliver {
            Delivery::Delivered
        } else {
            Delivery::Undelivered
        }
    }
}

impl World {
    /// The receipt of the stated entry of the last result about `segment`.
    fn result_of(&self, segment: &str) -> Receipt {
        let entries = self.log.entries();
        let at = entries
            .iter()
            .rposition(|e| {
                matches!(e, Entry::Stated(s)
                    if s.what.statement.as_str() == RESULT_SHAPE
                        && s.about.as_ref().and_then(|a| a.segment.as_ref()).map(|x| x.as_str())
                            == Some(segment))
            })
            .expect("a result about the segment");
        receipt_of(at, &entries[at])
    }

    /// When the first move of value on `segment` took effect: when it started.
    fn started_at(&self, segment: &str) -> Timestamp {
        self.log
            .entries()
            .iter()
            .find_map(|e| match e {
                Entry::Verified(v) => match &v.what {
                    What::Effect {
                        action: Action::MoveValue { segment: s, .. },
                        ..
                    } if s.as_str() == segment => Some(v.when.at.clone()),
                    _ => None,
                },
                Entry::Stated(_) => None,
            })
            .expect("a move on the segment")
    }

    /// `tuple`'s provider's terms, with a fee of at most `maximum`, made at `made_at` and kept in `terms` as they
    /// arrive now, by the clock: signed, kept, appended as a stated entry, attributed by the log, and the receipt of
    /// that entry.
    fn state_terms(
        &mut self,
        terms: &mut Terms,
        tuple: &RouterTupleKey,
        made_at: u64,
        maximum: &str,
    ) -> Receipt {
        let provider = tuple.provider.as_str();
        let mut statement = snapshot_support::envelope(
            &snapshot_support::deployment(),
            rutent_contracts::provider_role::TERMS_SHAPE,
            provider,
            made_at,
            json!({ "tuple": tuple, "fee": { "flat": "0",
                "proportional": { "numerator": "1", "denominator": "1000" }, "maximum": maximum } }),
        );
        snapshot_support::sign_value(
            &mut statement,
            &snapshot_support::statements_key(provider, 1),
        );
        let bytes = serde_json::to_vec(&statement).unwrap();
        let kept = self.store.keep(&bytes).expect("kept");
        let claim: rutent_contracts::core_internal::StatedEntry = serde_json::from_value(json!({
            "kind": "stated",
            "attribution": { "signature": "unverified", "claimed_by": {
                "party": "wallet-co", "acts": provider, "role": "provider", "kind": "component" } },
            "what": { "statement": rutent_contracts::provider_role::TERMS_SHAPE },
            "when": { "at": statement["made_at"], "source": "party_stated" },
            "received_at": Instant(self.clock.0.get().floor().0).rfc3339().unwrap(),
            "outcome": { "status": "stated" },
            "signed_with": { "scheme": "ed25519", "key": statement["signature"]["key"] },
            "commitment": kept.commitment,
        }))
        .expect("a stated entry");
        let entry = Entry::Stated(claim);
        let at = self.log.len();
        self.log
            .append(&GeneralEntry::try_from(entry.clone()).unwrap())
            .expect("the log takes the terms");
        let history = self.history();
        let Entry::Stated(attributed) = &history[at].1 else {
            panic!("a stated entry");
        };
        let receipt = receipt_of(at, &entry);
        terms
            .state(
                attributed,
                &receipt,
                &serde_json::from_slice(&bytes).expect("a terms statement"),
            )
            .expect("kept as terms");
        receipt
    }
}

/// A deposit segment's tuple at the wallet.
fn wallet_tuple() -> RouterTupleKey {
    snapshot_support::deposit_tuple(WALLET)
}

fn formula(maximum: &str) -> rutent_contracts::router_api::FeeFormula {
    serde_json::from_value(json!({
        "flat": "0", "proportional": { "numerator": "1", "denominator": "1000" }, "maximum": maximum,
    }))
    .unwrap()
}

/// `p1-s3`'s charge of `charge`, on its last result.
fn charged_on(world: &World, charge: &str, advertised: &str) -> Charged {
    Charged {
        order: id(ORDER),
        segment: id("p1-s3"),
        provider: id(WALLET),
        tuple: wallet_tuple(),
        started_at: world.started_at("p1-s3"),
        amount_in: rutent_contracts::router_api::Amount::new("1000000").unwrap(),
        charged: rutent_contracts::router_api::Amount::new(charge).unwrap(),
        charged_in: None,
        advertised: formula(advertised),
        result: world.result_of("p1-s3"),
    }
}

impl World {
    /// Checks `charged` against `terms`, `channel` the operator's.
    fn charge(
        &mut self,
        charges: &mut Charges,
        terms: &Terms,
        channel: &mut Channel,
        charged: &Charged,
    ) -> Result<Checked, ChargeError> {
        charges.check(
            ChargeDesk {
                terms,
                log: &mut self.log,
                clock: &self.clock,
                fresh: &mut self.ids,
                operator: channel,
                core: &self.core,
            },
            charged,
        )
    }
}

/// A charge above the formula that bound its segment is raised to the operator resting on its provider's verified
/// result and on the stated entry of the terms in force at the segment's start, as the log admits it. Terms made
/// before the start and received after it bind nothing; terms stated after it neither: the charge is checked against
/// the terms that bound it, and where none did, against the price its route was priced on, naming no terms.
// verifies: REQ-QTE-5, REQ-QTE-6, AC-14
#[test]
fn a_charge_is_raised_on_its_result_and_the_terms_that_bound_its_segment() {
    let mut world = World::over(Platform::settling());
    // This test cites the statements it kept by the receipts of the entries as handed in; a log that records them
    // attributed would answer others, and no reversal question is asked here.
    world.log.attribution = None;
    let mut terms = Terms::new();
    // Stated before the segment starts, one second before the clock the order runs at: binds from then.
    world.clock.0.set(Reading::at(NOW - 100));
    let first = world.state_terms(&mut terms, &wallet_tuple(), NOW - 101, "1000");
    // Made before the start, but received at the start's second: also binds from no later than it.
    world.clock.0.set(Reading::at(NOW));
    world.ran();
    let base = world.log.len();
    // Dated before the start and received after it: binds nothing of this segment, and neither does a looser one.
    world.clock.0.set(Reading::at(NOW + 50));
    let late = world.state_terms(&mut terms, &wallet_tuple(), NOW - 50, "9000");
    let looser = world.state_terms(&mut terms, &wallet_tuple(), NOW + 60, "90000");
    assert_ne!(late, first);
    assert_ne!(looser, first);

    let mut charges = Charges::new();
    let mut channel = Channel {
        raised: Vec::new(),
        deliver: true,
    };
    let over = charged_on(&world, "2001", "5000");
    let mark = world.log.len();
    let Ok(Checked::Flagged(flagged)) = world.charge(&mut charges, &terms, &mut channel, &over)
    else {
        panic!("a charge above the terms that bound the segment isn't flagged");
    };
    let Difference::Charge {
        result,
        terms: named,
        maximum,
        ..
    } = &flagged.raise.difference
    else {
        panic!("a charge difference");
    };
    assert_eq!(result, &over.result, "the result's stated entry");
    assert_eq!(
        named.as_ref(),
        Some(&first),
        "the terms in force at its start"
    );
    assert_eq!(
        maximum,
        &rutent_contracts::router_api::Amount::new("1000").unwrap()
    );
    assert_eq!(
        flagged.raise.cites,
        vec![over.result.clone(), first.clone()]
    );
    assert!(flagged.delivered);
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    // Checked again: the same answer, raised once.
    assert_eq!(
        world.charge(&mut charges, &terms, &mut channel, &over),
        Ok(Checked::Flagged(Box::new((*flagged).clone())))
    );
    assert_eq!(channel.raised.len(), 1);

    // Another segment of a provider that stated no terms by its start is held to the price its route was priced on,
    // and names no terms.
    let bare = Terms::new();
    let mut charged = charged_on(&world, "6000", "5000");
    charged.segment = id("p1-s1");
    charged.provider = id(RESERVE);
    charged.tuple = snapshot_support::deposit_tuple(RESERVE);
    charged.started_at = world.started_at("p1-s1");
    charged.result = world.result_of("p1-s1");
    let mark = world.log.len();
    let Ok(Checked::Flagged(on_the_price)) =
        world.charge(&mut charges, &bare, &mut channel, &charged)
    else {
        panic!("a charge above the price isn't flagged");
    };
    assert!(matches!(
        &on_the_price.raise.difference,
        Difference::Charge { terms: None, maximum, .. }
            if maximum == &rutent_contracts::router_api::Amount::new("5000").unwrap()
    ));
    assert_eq!(on_the_price.raise.cites, vec![charged.result.clone()]);
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    // Within the terms, between the fee and the maximum: nothing raised. Another provider than the tuple's: refused.
    let mut within = charged_on(&world, "1000", "5000");
    within.segment = id("p1-s2");
    within.result = world.result_of("p1-s2");
    within.started_at = world.started_at("p1-s2");
    assert_eq!(
        world.charge(&mut charges, &terms, &mut channel, &within),
        Ok(Checked::Within)
    );
    let mut foreign = charged_on(&world, "9000", "5000");
    foreign.provider = id("p-other");
    assert_eq!(
        world.charge(&mut charges, &terms, &mut channel, &foreign),
        Err(ChargeError::NotTheTuplesProvider)
    );
    // The segment checked on one result refuses another.
    let mut again = charged_on(&world, "9000", "5000");
    again.result = world.result_of("p1-s2");
    assert_eq!(
        world.charge(&mut charges, &terms, &mut channel, &again),
        Err(ChargeError::AnotherCharge)
    );
    assert_eq!(channel.raised.len(), 2, "no other was raised");

    // A raise that doesn't reach the operator is recorded failed.
    let mut lost = Channel {
        raised: Vec::new(),
        deliver: false,
    };
    let mut other = charged_on(&world, "9000", "5000");
    other.segment = id("p1-s1");
    other.provider = id(RESERVE);
    other.tuple = snapshot_support::deposit_tuple(RESERVE);
    other.started_at = world.started_at("p1-s1");
    other.result = world.result_of("p1-s1");
    let mut fresh = Charges::new();
    let Ok(Checked::Flagged(undelivered)) = world.charge(&mut fresh, &bare, &mut lost, &other)
    else {
        panic!("a charge above the price isn't flagged");
    };
    assert!(!undelivered.delivered);
    assert_eq!(world.admitted_from(base), Ok(world.log.len() - base));
}

/// A charge stated in another asset than the one its tuple's formula counts in is held to no formula, and is never
/// passed for it: any of it is raised, against a maximum of nothing and in the asset it is stated in, as the log
/// admits it. A charge of nothing in that asset charges nothing and raises nothing.
// verifies: REQ-QTE-5, AC-14
#[test]
fn a_charge_in_another_asset_is_raised_and_never_passed() {
    let mut world = World::over(Platform::settling());
    world.ran();
    let mut charges = Charges::new();
    let mut channel = Channel {
        raised: Vec::new(),
        deliver: true,
    };
    let terms = Terms::new();
    let other = rutent_contracts::provider_role::Denomination {
        id: "EURC".to_owned(),
        decimals: 6,
    };
    // Within what the price allows, were it in the tuple's own asset: 100 against a maximum of 5 000.
    let mut foreign = charged_on(&world, "100", "5000");
    foreign.charged_in = Some(other.clone());
    let mark = world.log.len();
    let Ok(Checked::Flagged(flagged)) = world.charge(&mut charges, &terms, &mut channel, &foreign)
    else {
        panic!("a charge in another asset is passed for want of a formula to hold it to");
    };
    let Difference::Charge {
        charged,
        maximum,
        asset,
        terms: named,
        ..
    } = &flagged.raise.difference
    else {
        panic!("a charge difference");
    };
    assert_eq!(charged.as_str(), "100");
    assert_eq!(maximum.as_str(), "0", "no formula bounds it");
    assert_eq!(asset, &other, "in the asset it is stated in");
    assert!(named.is_none());
    assert_eq!(flagged.raise.cites, vec![foreign.result.clone()]);
    assert!(flagged.delivered);
    assert_eq!(channel.raised.len(), 1);
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    // Nothing charged, in whatever asset: nothing to raise.
    let mut nothing = charged_on(&world, "0", "5000");
    nothing.segment = id("p1-s2");
    nothing.result = world.result_of("p1-s2");
    nothing.started_at = world.started_at("p1-s2");
    nothing.charged_in = Some(other);
    assert_eq!(
        world.charge(&mut charges, &terms, &mut channel, &nothing),
        Ok(Checked::Within)
    );
    assert_eq!(channel.raised.len(), 1);
}

/// A charge stated in another asset than the segment's is raised marked as having no formula (`no_formula`), beside
/// the maximum of nothing the contract holds it to: the raise says a formula no more allows nothing but that none
/// covers the asset. The raise goes through the real log, whose check admits the mark only for an asset other than the
/// planned segment's own.
// verifies: REQ-QTE-5
#[test]
fn charge_in_another_asset_than_the_segments_is_raised_marked_as_having_no_formula() {
    let mut world = World::over(Platform::settling());
    world.ran();
    let mut charges = Charges::new();
    let mut channel = Channel {
        raised: Vec::new(),
        deliver: true,
    };
    let terms = Terms::new();
    let other = rutent_contracts::provider_role::Denomination {
        id: "EURC".to_owned(),
        decimals: 6,
    };
    let mut foreign = charged_on(&world, "100", "5000");
    foreign.charged_in = Some(other.clone());
    let mark = world.log.len();
    let Ok(Checked::Flagged(flagged)) = world.charge(&mut charges, &terms, &mut channel, &foreign)
    else {
        panic!("a charge in another asset is raised");
    };
    let Difference::Charge {
        maximum,
        asset,
        no_formula,
        ..
    } = &flagged.raise.difference
    else {
        panic!("a charge difference");
    };
    assert_eq!(
        no_formula,
        &Some(rutent_contracts::core_internal::NoFormula::ForThisAsset),
        "it says no formula covers the asset"
    );
    assert_eq!(
        maximum.as_str(),
        "0",
        "beside the maximum the contract holds it to"
    );
    assert_eq!(asset, &other);
    assert_eq!(channel.raised.len(), 1);
    assert_eq!(
        &channel.raised[0].difference, &flagged.raise.difference,
        "the operator was told the same"
    );
    // The log admits the marked raise: the segment's plan counts in another `from` asset.
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// A charge in the segment's own asset is held to its formula and is never marked: not where its asset is not stated,
/// and not where it is stated as the segment's own, above the formula or within it.
// verifies: REQ-QTE-5, AC-14
#[test]
fn charge_in_the_segments_own_asset_is_not_marked() {
    let mut world = World::over(Platform::settling());
    world.ran();
    let mut charges = Charges::new();
    let mut channel = Channel {
        raised: Vec::new(),
        deliver: true,
    };
    let terms = Terms::new();
    let own = wallet_tuple().from.asset;
    let mark = world.log.len();

    // Above the formula, its asset not stated.
    let unstated = charged_on(&world, "6000", "5000");
    let Ok(Checked::Flagged(a)) = world.charge(&mut charges, &terms, &mut channel, &unstated)
    else {
        panic!("a charge above its formula is raised");
    };
    // Above the formula, stated as the segment's own asset.
    let mut stated = charged_on(&world, "6000", "5000");
    stated.segment = id("p1-s1");
    stated.provider = id(RESERVE);
    stated.tuple = snapshot_support::deposit_tuple(RESERVE);
    stated.result = world.result_of("p1-s1");
    stated.started_at = world.started_at("p1-s1");
    stated.charged_in = Some(own.clone());
    let Ok(Checked::Flagged(b)) = world.charge(&mut charges, &terms, &mut channel, &stated) else {
        panic!("a charge above its formula is raised");
    };
    for raise in [&a.raise, &b.raise] {
        let Difference::Charge {
            maximum,
            asset,
            no_formula,
            ..
        } = &raise.difference
        else {
            panic!("a charge difference");
        };
        assert_eq!(no_formula, &None, "a formula covers it: nothing to mark");
        assert_eq!(maximum.as_str(), "5000", "the formula's own maximum");
        assert_eq!(asset, &own);
    }
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

// Over a log that attributes, as the audit log does (`F-020` notes, "journal receipts from the log") ---------------

/// The statements of `log` from `from` on, each the entry the log recorded: its writer handed it in unverified, and
/// the log attributed it from its kept bytes, so its receipt is of the entry recorded, not of the one handed in.
fn recorded_verified(world: &World, from: usize) -> usize {
    let stated: Vec<_> = world.log.entries()[from..]
        .iter()
        .filter_map(|e| match e {
            Entry::Stated(s) => Some(s.clone()),
            Entry::Verified(_) => None,
        })
        .collect();
    assert!(!stated.is_empty(), "a statement is recorded");
    for s in &stated {
        assert!(
            matches!(s.attribution, Attribution::Verified { .. }),
            "the log attributes {}: {s:?}",
            s.what.statement.as_str()
        );
    }
    stated.len()
}

/// The failed exchange's reversal is asked over a log that attributes a statement: Core's question, signed by the
/// operator, is recorded verified, so its receipt is not the digest of the entry as built. The ask takes the log's
/// receipt, records the effect and its result, and the log admits every entry.
// verifies: REQ-ORD-3, REQ-ORD-11
#[test]
fn a_reversal_is_asked_over_a_log_that_attributes_its_statements() {
    let mut world = World::over(Platform::settling().with(EXCHANGE, Act::Fail));
    assert_eq!(world.ran()[1], SegmentOutcome::Failed);
    let mark = world.log.len();
    let asked = world.ask("p1-s2").expect("the log took the question");
    assert_eq!(asked.why, Some(NotCounted::FailedProvidersOwn));
    assert!(recorded_verified(&world, mark) >= 1);
    let whats = world.whats_from(mark);
    assert!(whats.iter().any(|w| matches!(
        w,
        What::Effect {
            action: rutent_contracts::core_internal::Action::AskReversal { .. },
            ..
        }
    )));
    assert!(
        whats
            .iter()
            .any(|w| matches!(w, What::Result { effect } if *effect == asked.effect))
    );
    assert_eq!(world.state_of("p1-s2"), SegmentState::FailedReversalOwed);
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// The origin reserve is asked whether the credit arrived, over the same log: Core's question and the reserve's
/// signed answer are recorded attributed, and the reversal completes as the reserve's word. Asking again records
/// nothing more, the log answering an equal entry with its own receipt.
// verifies: REQ-ORD-3
#[test]
fn a_reversal_is_confirmed_over_a_log_that_attributes_its_statements() {
    let mut world = World::over(Platform::settling().with(EXCHANGE, Act::Fail));
    assert_eq!(world.ran()[1], SegmentOutcome::Failed);
    let mark = world.log.len();
    let mut reserve = Answering::confirming();
    let done = match world
        .confirm(&mut reserve, "p1-s2")
        .expect("the log took it")
    {
        Confirmation::Completed(done) => done,
        other => panic!("{other:?}"),
    };
    assert_eq!(done.whose, id(RESERVE));
    assert!(
        recorded_verified(&world, mark) >= 2,
        "the question and the answer"
    );
    assert_eq!(world.state_of("p1-s2"), SegmentState::ReversalComplete);
    let appended = world.log.len();
    assert_eq!(
        world.confirm(&mut reserve, "p1-s2").unwrap(),
        Confirmation::Completed(done)
    );
    assert_eq!(world.log.len(), appended, "nothing is recorded twice");
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// The steps after a reversal is asked, over the same log: the provider's late word is kept as its statement, once;
/// and the complete order's return is its statement and the `afterwards` citing it.
// verifies: REQ-STM-6, REQ-ORD-31
#[test]
fn a_late_word_and_a_return_are_kept_over_a_log_that_attributes_its_statements() {
    let mut world = World::over(Platform::settling().with(EXCHANGE, Act::Fail));
    assert_eq!(world.ran()[1], SegmentOutcome::Failed);
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
        .with(|r, p, _| r.later(p, at(&order, &part, &s2), &late, &presenter_of(&late)))
        .expect("the log took the late word");
    let again = world
        .with(|r, p, _| r.later(p, at(&order, &part, &s2), &late, &presenter_of(&late)))
        .unwrap();
    assert_eq!(first, again, "kept once");
    assert_eq!(world.log.len(), mark + 1);
    assert_eq!(recorded_verified(&world, mark), 1);
    assert_eq!(world.admitted_from(mark), Ok(1));

    let mut world = World::over(Platform::settling());
    assert_eq!(world.ran().len(), 3);
    let mark = world.log.len();
    let returned = result(
        WALLET,
        "deposit",
        "tx-returned",
        json!({ "status": "failed", "evidence": piece("polygon") }),
    );
    world
        .with(|r, p, _| r.returned(p, &order, &returned))
        .expect("the log took the return");
    // The return names no transaction, so the log leaves it unverified, recorded as handed in: its receipt is the
    // digest of the entry as built, which the journal takes all the same.
    assert!(
        world
            .whats_from(mark)
            .iter()
            .any(|w| matches!(w, What::Afterwards { .. }))
    );
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
}

/// What the check protected stays: a log that answers a verified entry with another entry's receipt gives no answer,
/// so the ask, which can't rest on a record the log didn't take, ends unavailable. A statement's receipt is the log's
/// to give, so the question is taken however it is answered, and the effect that cites it is what is refused.
// verifies: INV-8
#[test]
fn a_verified_entrys_receipt_is_still_its_own_digest() {
    let mut world = exchange_failed();
    world.log.misreceipting = true;
    assert_eq!(
        world.ask("p1-s2").map(|_| ()),
        Err(ReversalError::Append(AppendFailure::Unavailable))
    );
}

/// A refusal of the log is never swallowed, a statement's included: the ask ends refused, and nothing is retried.
// verifies: INV-8
#[test]
fn a_refusal_of_the_log_is_never_swallowed() {
    let mut world = World::over(Platform::settling().with(EXCHANGE, Act::Fail));
    assert_eq!(world.ran()[1], SegmentOutcome::Failed);
    world.log.refusing = true;
    let mark = world.log.len();
    assert!(matches!(
        world.ask("p1-s2"),
        Err(ReversalError::Append(AppendFailure::Refused(_)))
    ));
    // Only policy's decision, which it records on its own handle, is new: no question, no effect.
    assert!(
        world.log.entries()[mark..].iter().all(|e| matches!(
            e,
            Entry::Verified(v) if is_decision(&v.what)
        )),
        "nothing of Core's is recorded"
    );
}

/// Core's own question is never backed by a record the log made unattributed: where the log can't tie it to the
/// operator, its receipt is the digest of the entry as built, and nothing is handed over. The ask records no effect;
/// the confirmation asks no reserve; each is refused again on a retry.
// verifies: INV-7
#[test]
fn a_question_the_log_leaves_unattributed_backs_no_move() {
    let mut world = World::over(Platform::settling().with(EXCHANGE, Act::Fail));
    assert_eq!(world.ran()[1], SegmentOutcome::Failed);
    world.log.attribution.as_mut().unwrap().operator_known = false;
    let mark = world.log.len();
    for _ in 0..2 {
        assert!(matches!(
            world.ask("p1-s2"),
            Err(ReversalError::Append(AppendFailure::Refused(_)))
        ));
    }
    assert!(
        !world
            .whats_from(mark)
            .iter()
            .any(|w| matches!(w, What::Effect { .. })),
        "no effect, so nothing is carried out"
    );
    let mut reserve = Answering::confirming();
    assert!(matches!(
        world.confirm(&mut reserve, "p1-s2"),
        Err(ReversalError::Append(AppendFailure::Refused(_)))
    ));
    assert!(reserve.asked.is_empty(), "no reserve is asked");
}

// T-020-27: a late status asked, and a reversal at a later inner node ---------------------------------------------

/// What the exchange says, asked late, if its conversion went through after all.
fn delivered_after_all() -> Value {
    json!({ "status": "succeeded", "amount_in": EXCHANGED, "amount_out": "249250500", "rate": "2.5",
        "fee": { "asset": "COP", "amount": "0" }, "evidence": piece(EXCHANGE) })
}

/// The exchange's failed segment, its provider answering a late status as `delivered_after_all` says.
fn exchange_failed_delivering() -> World {
    let mut world = World::over(
        Platform::settling()
            .with(EXCHANGE, Act::Fail)
            .saying(delivered_after_all()),
    );
    let ran = world.ran();
    assert_eq!(ran[1], SegmentOutcome::Failed, "{ran:?}");
    world
}

/// The `ask_status` effects, the provider statements kept and the results of effects among `entries`, in order.
fn status_trail(entries: &[Entry]) -> Vec<&'static str> {
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::Verified(v) => match &v.what {
                What::Effect {
                    action: Action::AskStatus { .. },
                    ..
                } => Some("effect"),
                What::Result { .. } => Some("result"),
                _ => None,
            },
            Entry::Stated(_) => Some("statement"),
        })
        .collect()
}

/// A provider that failed a segment, or fell silent, is asked where the segment stands: policy decides the ask, Core
/// records it as an effect under a fresh transaction before anything is sent, and what the provider answers is kept as
/// its own statement about the segment and the transaction it answers, once. A settled segment's provider, a segment
/// whose operation was never carried out and an ask policy refuses send nothing.
// verifies: INV-4
#[test]
fn late_status_of_a_segment_is_asked_under_a_fresh_transaction_and_kept_as_the_providers_statement()
{
    let mut world = exchange_failed_delivering();
    let (order, s2) = (id(ORDER), id("p1-s2"));
    let tx = world.segments.sent(&order, &s2).unwrap().0.clone();
    let mark = world.log.len();
    let asked = world.ask_status("p1-s2").expect("asked");
    assert_ne!(asked.transaction, tx, "not the segment's own transaction");
    assert!(!world.issued.contains(asked.transaction.as_str()));
    assert_eq!(
        world.platform.statuses,
        vec![(asked.transaction.clone(), id(EXCHANGE))],
        "the exchange was asked once, under the fresh transaction"
    );
    // The ask is recorded before it is sent, and the answer follows it, then the effect's result.
    let appended: Vec<Entry> = world.log.entries()[mark..].to_vec();
    assert_eq!(
        status_trail(&appended),
        vec!["effect", "statement", "result"],
        "{appended:?}"
    );
    let Some(Entry::Verified(effect)) = appended
        .iter()
        .find(|e| matches!(e, Entry::Verified(v) if matches!(v.what, What::Effect { .. })))
    else {
        panic!("the effect")
    };
    assert_eq!(
        effect.what,
        What::Effect {
            action: Action::AskStatus {
                order: order.clone(),
                segment: s2.clone(),
                provider: id(EXCHANGE),
                transaction: asked.transaction.clone(),
            },
            decision: match &effect.what {
                What::Effect { decision, .. } => decision.clone(),
                _ => unreachable!(),
            },
        }
    );
    let Some(Entry::Stated(kept)) = appended.iter().find(|e| matches!(e, Entry::Stated(_))) else {
        panic!("the provider's statement")
    };
    assert_eq!(kept.what.statement.as_str(), RESULT_SHAPE);
    assert_eq!(
        kept.about.as_ref().and_then(|a| a.transaction.as_ref()),
        Some(&tx),
        "about the transaction it answers, so the log verifies it"
    );
    assert!(asked.answer.is_some());
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    // Asked again, under another fresh transaction: the same statement is kept once.
    let again = world.ask_status("p1-s2").expect("asked again");
    assert_ne!(again.transaction, asked.transaction);
    assert_ne!(again.effect, asked.effect);
    assert_eq!(again.answer, asked.answer, "its statement is kept once");
    assert_eq!(
        status_trail(&world.log.entries()[mark..]),
        vec!["effect", "statement", "result", "effect", "result"]
    );
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    // A settled segment's provider isn't asked, nor one whose operation was never carried out, nor on a refusal.
    let before = (world.log.len(), world.platform.statuses.len());
    assert_eq!(
        world.ask_status("p1-s1"),
        Err(ReversalError::StatusNotNeeded)
    );
    world.policy.refusing = true;
    assert!(matches!(
        world.ask_status("p1-s2"),
        Err(ReversalError::NotPermitted(_))
    ));
    assert_eq!(world.platform.statuses.len(), before.1, "nothing was sent");
    assert_eq!(
        world
            .log
            .entries()
            .iter()
            .skip(before.0)
            .filter(|e| matches!(e, Entry::Verified(v) if matches!(v.what, What::Effect { .. })))
            .count(),
        0,
        "no effect is recorded where policy refuses"
    );
    let mut world = World::over(Platform::settling());
    world.log.down = true;
    assert!(world.run().is_err());
    world.log.down = false;
    let appended = world.log.len();
    assert_eq!(
        world.ask_status("p1-s1"),
        Err(ReversalError::NotAnswered),
        "a segment never carried out has nothing to ask about"
    );
    assert_eq!(world.log.len(), appended);
    assert!(world.platform.statuses.is_empty());
}

/// A late status, whatever the provider says (that its move went through, or nothing at all), moves nothing: the
/// segment stays failed and owed, its value held, the order as Core follows it unchanged, and the log gets no
/// state-machine input or evidence, only the ask, the statement and the effect's result.
// verifies: INV-4
#[test]
fn late_status_moves_nothing() {
    for says in [Some(delivered_after_all()), None] {
        let platform = Platform::settling().with(EXCHANGE, Act::Fail);
        let mut world = World::over(match says.clone() {
            Some(outcome) => platform.saying(outcome),
            None => platform,
        });
        let ran = world.ran();
        assert_eq!(ran[1], SegmentOutcome::Failed, "{ran:?}");
        let (order, part, s2) = (id(ORDER), id(PART), id("p1-s2"));
        world
            .with(|r, p, _| r.owe(p, at(&order, &part, &s2)))
            .unwrap();
        let (followed, mark) = (world.followed().clone(), world.log.len());
        let asked = world.ask_status("p1-s2").expect("asked");
        assert_eq!(asked.answer.is_some(), says.is_some(), "{says:?}");
        assert_eq!(world.followed(), &followed, "the order is as it was");
        assert_eq!(world.state_of("p1-s2"), SegmentState::FailedReversalOwed);
        assert!(world.held("p1-s2"), "its value is still held");
        assert!(world.catalogue.owed().is_owed(&owed_id()), "still owed");
        assert!(world.reversals.completed(&order, &s2).is_none());
        for what in world.whats_from(mark) {
            assert!(
                matches!(
                    &what,
                    What::Effect { .. } | What::Result { .. } | What::Decide { .. }
                ),
                "nothing is recorded but the ask and its result: {what:?}"
            );
        }
        // The result of the ask says how it came out: done where something came back, unanswered where not.
        let outcome = world.log.entries()[mark..]
            .iter()
            .find_map(|e| match e {
                Entry::Verified(v) if matches!(v.what, What::Result { .. }) => Some(v.outcome),
                _ => None,
            })
            .expect("the result");
        assert_eq!(
            outcome,
            if says.is_some() {
                Outcome::Done {}
            } else {
                Outcome::Unanswered {}
            }
        );
        assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
    }
}

/// The route across inner nodes, `exchange-b`'s conversion failing where it starts, `ledger-b`, a later inner node.
fn conversion_failed(holding_of_b: Option<&str>) -> World {
    let mut world = World::across(
        Platform::settling().with(EXCHANGE_B, Act::Fail),
        holding_of_b,
    );
    let ran = world.ran();
    assert_eq!(ran.len(), 3, "{ran:?}");
    assert!(
        ran[..2]
            .iter()
            .all(|o| matches!(o, SegmentOutcome::Settled { .. })),
        "{ran:?}"
    );
    assert_eq!(ran[2], SegmentOutcome::Failed, "{ran:?}");
    assert_eq!(world.state_of("p1-s3"), SegmentState::FailedReversalOwed);
    world
}

/// A segment that fails starting at an inner node after the first is confirmed by that node's holding account, the
/// provider that delivered the value there and gave the operator its interface: Core asks it, not the origin
/// reserve, whose word is about its own node. The origin reserve's word and the failed provider's own complete
/// nothing there. The holding account's signed confirmation completes the reversal as its word, and the log admits
/// it.
// verifies: REQ-ORD-3, DEC-24
#[test]
fn failed_segment_at_a_later_inner_node_is_confirmed_by_that_nodes_holding_account() {
    let mut world = conversion_failed(Some("holding-b"));
    let (order, part, s3) = (id(ORDER), id(PART), id("p1-s3"));
    let tx = world.segments.sent(&order, &s3).unwrap().0.clone();
    let mark = world.log.len();
    for (maker, refused) in [
        (RESERVE, NotCounted::NotTheOriginReserve),
        (EXCHANGE_B, NotCounted::FailedProvidersOwn),
    ] {
        let mut answering = Answering::replying(Reply::Confirms {
            maker,
            amount: None,
        });
        assert_eq!(why(world.confirm(&mut answering, "p1-s3")), refused);
        let Operation::ReversalConfirmation(question) = &answering.asked[0].statement else {
            panic!("a confirmation is asked")
        };
        assert_eq!(
            question.provider, RAMP,
            "the holding account of ledger-b is asked, not the origin reserve"
        );
        assert_eq!(question.segment, tx.as_str());
        assert_eq!(
            (question.asset.as_str(), question.amount.as_str()),
            ("COP", RAMPED)
        );
        assert_eq!(world.state_of("p1-s3"), SegmentState::FailedReversalOwed);
        assert!(world.held("p1-s3"));
    }
    let mut answering = Answering::replying(Reply::Confirms {
        maker: RAMP,
        amount: None,
    });
    let Confirmation::Completed(done) = world
        .confirm(&mut answering, "p1-s3")
        .expect("the holding account answers")
    else {
        panic!("the holding account's word counts")
    };
    assert_eq!(done.whose, id(RAMP));
    assert_eq!(done.word, Word::Reserves, "its word, not its publisher's");
    assert_eq!(done.grade, EvidenceGrade::SignedByProvider);
    assert_eq!(world.state_of("p1-s3"), SegmentState::ReversalComplete);
    assert!(!world.held("p1-s3"), "the hold is released");
    assert_eq!(
        world
            .reversals
            .completed(&order, &s3)
            .map(|c| c.whose.clone()),
        Some(id(RAMP))
    );
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));
    let _ = part;
}

/// A reversal at a later inner node whose only deliverer is the failed provider has no party to confirm it: the
/// provider that delivered the value to `ledger-b` fails the next segment from it, and its own word never confirms its
/// failure. Nothing is asked or recorded: the reversal stays owed on the catalogue's list and held, for the stuck
/// orders, failing closed. (A node with no deliverer is the first node, which the origin reserve answers, and a node
/// with no holding account is one no route runs through.)
// verifies: REQ-ORD-3, INV-2
#[test]
fn reversal_at_a_later_inner_node_with_no_confirming_party_stays_owed_and_held() {
    let mut world =
        World::across_by_one(Platform::settling().with(EXCHANGE_B, Act::FailConversion));
    let ran = world.ran();
    assert_eq!(ran.len(), 3, "{ran:?}");
    assert_eq!(ran[2], SegmentOutcome::Failed, "{ran:?}");
    let (order, part, s3) = (id(ORDER), id(PART), id("p1-s3"));
    world
        .with(|r, p, _| r.owe(p, at(&order, &part, &s3)))
        .unwrap();
    let mark = world.log.len();
    for who in [EXCHANGE_B, RESERVE] {
        let mut answering = Answering::replying(Reply::Confirms {
            maker: who,
            amount: None,
        });
        assert_eq!(
            world.confirm(&mut answering, "p1-s3"),
            Err(ReversalError::NoReserveAtNode),
            "{who}"
        );
        assert!(answering.asked.is_empty(), "no one is asked");
    }
    assert_eq!(world.log.len(), mark, "nothing is recorded");
    assert_eq!(world.state_of("p1-s3"), SegmentState::FailedReversalOwed);
    assert!(world.held("p1-s3"));
    assert!(world.catalogue.owed().is_owed(&format!("{ORDER}/p1-s3")));
    assert!(world.reversals.completed(&order, &s3).is_none());
}

/// Who hands `bytes` over where their maker does: the maker their envelope names, for itself.
fn presenter_of(bytes: &[u8]) -> Actor {
    let acts = id(value_maker(bytes).as_str());
    Actor {
        party: acts.clone(),
        acts,
        role: Role::Provider,
        kind: ActorKind::Component,
    }
}

fn value_maker(bytes: &[u8]) -> String {
    serde_json::from_slice::<Value>(bytes).unwrap()["maker"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The stated entries of `maker`'s statements the log holds from `from` on, with who each says relayed it.
fn stated_by(world: &World, maker: &str, from: usize) -> Vec<Option<Actor>> {
    world.log.entries()[from..]
        .iter()
        .filter_map(|entry| match entry {
            Entry::Stated(stated) if stated.attribution.maker().acts == id(maker) => {
                Some(stated.relayed_by.clone())
            }
            _ => None,
        })
        .collect()
}

/// The origin reserve is asked and answers with another provider's signed statement: the entry for it is the
/// statement's maker's, unverified, and names the reserve that handed it over as its relay. The reserve's own answer
/// names none.
// verifies: REQ-AUD-2
#[test]
fn reversal_statement_restated_by_a_relay_is_recorded_naming_the_relay() {
    let mut world = exchange_failed();
    let mark = world.log.len();
    let mut relaying = Answering::replying(Reply::Confirms {
        maker: WALLET,
        amount: None,
    });
    assert_eq!(
        why(world.confirm(&mut relaying, "p1-s2")),
        NotCounted::NotTheOriginReserve
    );
    let [relayed] = stated_by(&world, WALLET, mark).try_into().unwrap();
    let relay = relayed.expect("the relay is named");
    assert_eq!(relay.acts, id(RESERVE));
    assert_eq!(world.admitted_from(mark), Ok(world.log.len() - mark));

    let mut world = exchange_failed();
    let mark = world.log.len();
    assert!(world.confirm(&mut Answering::confirming(), "p1-s2").is_ok());
    assert_eq!(stated_by(&world, RESERVE, mark), vec![None]);
}
