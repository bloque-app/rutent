//! The segments' rules the simulated providers can't show (`T-020-08`): a route across three providers whose
//! withdrawal names a cross-module call, which no simulated provider's nodes admit and no simulated release declares,
//! and a segment record's move mapped to the log's item.

mod snapshot_support;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::core_internal::calls::{
    AppendFailure, Decisions, Evidence, Fetched, GeneralEntry, Kept, Log,
};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Answer, Decision, DecisionRequest, Destination, Effect, Entry, Outcome,
    ReadCheck, Receipt, Role, RouteEnd, What,
};
use rutent_contracts::module_interface::{
    CallRefusalReason, Declaration, Identifier, OperationKind, PlatformRefusal,
};
use rutent_contracts::provider_role::{Denomination, Operation, OuterNode, PublicKey, Statement};
use rutent_contracts::router_api::{DecisionRecord, Id, Timestamp};
use rutent_core_orch::rounds::{Fresh, Issued, OperatorKey};
use rutent_core_orch::segments::{
    Answered, Live, Modules, NoProxy, Parts, ProvidersWord, Ran, SegmentOutcome, Segments,
};
use rutent_core_orch::snapshot::{Clock, Exposures, Reading};
use rutent_prov::catalogue::{Catalogue, Instant};
use rutent_prov::module::lifecycle::{LifecycleRefusal, Move, MoveReason, SegmentRecord};
use rutent_prov::module::types::Events;
use rutent_prov::proxy::{CallAsked, RouteCalls, RouteNotNamed, judge_call};
use serde_json::{Value, json};

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn read(path: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts")
        .join(path);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

struct TestClock(Cell<Reading>);

impl Clock for TestClock {
    fn now(&self) -> Reading {
        self.0.get()
    }
}

struct TestKey(SigningKey);

impl OperatorKey for TestKey {
    fn key(&self) -> PublicKey {
        PublicKey::new(base64url(self.0.verifying_key().as_bytes())).unwrap()
    }

    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(input).to_bytes())
    }
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

#[derive(Default)]
struct Ids(u64);

impl Fresh for Ids {
    fn id(&mut self) -> Id {
        self.0 += 1;
        id(&format!("id-{:04}", self.0))
    }
}

fn receipt(index: usize) -> Receipt {
    serde_json::from_value(json!({
        "log": "log-1",
        "index": index,
        "entry": { "algorithm": "sha-256", "value": format!("{index:064x}") },
    }))
    .unwrap()
}

#[derive(Default)]
struct KeptLog(Vec<Entry>);

impl Log for KeptLog {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        self.0.push(entry.entry().clone());
        Ok(receipt(self.0.len() - 1))
    }
}

#[derive(Default)]
struct Store(usize);

impl Evidence for Store {
    fn keep(&mut self, _: &[u8]) -> Result<Kept, AppendFailure> {
        self.0 += 1;
        Ok(Kept {
            commitment: serde_json::from_value(
                json!({ "algorithm": "sha-256", "value": format!("{:064x}", self.0) }),
            )
            .unwrap(),
            restates: None,
        })
    }

    fn fetch(&mut self, _: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Ok(Fetched::Erased)
    }
}

/// Policy that permits every move, its entry at a receipt of its own.
struct Permitting(usize);

impl Decisions for Permitting {
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure> {
        self.0 += 1;
        Ok(Decision {
            request: request.request.clone(),
            decision: id(&format!("dec-{}", self.0)),
            outcome: Answer::Permit {},
            receipt: receipt(10_000 + self.0),
        })
    }
}

/// The platform's routes and lifecycle, as Core uses them; each module refuses before it passes anything out.
struct Fake {
    routes: RouteCalls,
    declaration: Declaration,
    /// For each operation handed over: its transaction, and the route check of three calls its provider might make.
    judged: Vec<(String, [Option<CallRefusalReason>; 3])>,
    staged: Option<String>,
    promoted: Vec<(String, String)>,
    /// Each segment ends on 1.1.0, moved there from 1.0.0 when 1.0.0 was marked stale.
    moved: bool,
}

impl Modules for Fake {
    fn begin_segment(&mut self, _: &Id, _: &Identifier) -> Result<String, PlatformRefusal> {
        Ok("1.0.0".into())
    }

    fn name_calls(
        &mut self,
        transaction: &Identifier,
        record: &DecisionRecord,
        at: (usize, usize),
        segment: &Identifier,
    ) -> Result<usize, RouteNotNamed> {
        self.routes.name_segment(transaction, record, at, segment)
    }

    fn carry_out(
        &mut self,
        _: &Effect,
        provider: &Id,
        operation: &Statement<Operation>,
    ) -> Answered {
        let transaction = serde_json::to_value(&operation.statement).unwrap()["transaction"]
            .as_str()
            .unwrap()
            .to_owned();
        let judged = |callee: &str, kind: OperationKind| {
            judge_call(
                &self.declaration,
                &self.routes,
                &CallAsked {
                    caller: provider.as_str(),
                    transaction: &transaction,
                    callee,
                    kind,
                    claimed: Some(kind),
                    in_flight: true,
                },
            )
            .refusal
        };
        let judged = [
            judged("exchange-a", OperationKind::Conversion),
            judged("exchange-a", OperationKind::Withdrawal),
            judged("reserve-wallet", OperationKind::Deposit),
        ];
        self.judged.push((transaction, judged));
        Answered::Refused(PlatformRefusal::Unavailable)
    }

    fn take_events(&mut self) -> Events {
        Events::default()
    }

    fn end_segment(&mut self, transaction: &Identifier) -> Option<SegmentRecord> {
        self.moved.then(|| SegmentRecord {
            transaction: transaction.as_str().to_owned(),
            provider: "reserve-bank-co".into(),
            began_on: "1.0.0".into(),
            runs_on: "1.1.0".into(),
            moves: vec![Move {
                from: "1.0.0".into(),
                to: "1.1.0".into(),
                reason: MoveReason::Stale,
                at: Instant(1_794_200_000),
            }],
        })
    }

    fn end_calls(&mut self, transaction: &Identifier) {
        self.routes.end(transaction);
    }

    fn staged(&self, _: &Id) -> Option<String> {
        self.staged.clone()
    }

    fn promote(
        &mut self,
        _: &Effect,
        provider: &Id,
        by: &Id,
        _: Instant,
    ) -> Result<String, LifecycleRefusal> {
        let staged = self.staged.take().ok_or(LifecycleRefusal::NotStaged)?;
        self.promoted
            .push((provider.as_str().to_owned(), by.as_str().to_owned()));
        Ok(staged)
    }
}

fn fake() -> Fake {
    let mut artefact = read(
        "module-interface/fixtures/valid/admission-artefact-of-a-first-release-freshly-accepted.json",
    );
    let declaration = &mut artefact["assertion"]["statement"]["declaration"];
    declaration["provider"] = "reserve-bank-co".into();
    declaration["outside"]["calls"] = json!([
        { "callee": "exchange-a", "kinds": ["conversion", "withdrawal"] },
        { "callee": "reserve-wallet", "kinds": ["deposit"] },
    ]);
    Fake {
        routes: RouteCalls::default(),
        declaration: serde_json::from_value(declaration.clone()).unwrap(),
        judged: Vec::new(),
        staged: None,
        promoted: Vec::new(),
        moved: false,
    }
}

/// A catalogue in which the route's withdrawal may run (`T-020-09`): its reserve joined, declaring the grade of its
/// evidence, its tuple's floor set, and its private node `ledger-in` with a holding account. The rest of the route's
/// providers aren't in it, since no segment after the withdrawal is handed over here.
fn admitting() -> Catalogue {
    use rutent_contracts::provider_role::EvidenceGrade;
    use rutent_prov::catalogue::tuples_of;
    let mut ad = snapshot_support::reserve("reserve-bank-co", "bank-co");
    let mut value = serde_json::to_value(&ad).unwrap();
    value["offers"]["reserve"]["tuples"] = json!([{
        "origin": { "network": "co-ach" }, "destination": "ledger-in", "asset": "COP",
        "decimals": { "origin": 2, "destination": 2 },
        "terms": { "bands": [{ "min": "1", "max": "1000000000000" }], "reporting": "gross", "required_inputs": [] },
    }]);
    value["private_nodes"] = json!(["ledger-in"]);
    ad = serde_json::from_value(value).unwrap();
    let mut catalogue = Catalogue::new();
    catalogue.set_context(
        snapshot_support::deployment(),
        rutent_contracts::provider_role::VERSION,
    );
    catalogue
        .grant(snapshot_support::grant("g-0", &ad, 5_000_000_000))
        .unwrap();
    catalogue
        .trust_advertising_key(
            "g-0",
            snapshot_support::public_key(&snapshot_support::advertising_key("reserve-bank-co")),
        )
        .unwrap();
    let keys = snapshot_support::keys_of(
        "reserve-bank-co",
        &[&snapshot_support::statements_key("reserve-bank-co", 1)],
    );
    catalogue
        .join(
            &snapshot_support::advertisement_bytes(&ad, keys, snapshot_support::JOINED_AT),
            Instant(snapshot_support::JOINED_AT),
        )
        .unwrap();
    for key in tuples_of(&ad) {
        catalogue.set_evidence_floor(key, EvidenceGrade::Statement);
    }
    catalogue
        .register_private_holding("ledger-in", "bank-co", "holding")
        .unwrap();
    catalogue
}

fn core() -> Actor {
    Actor {
        party: id("rutent"),
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

/// The router-api fixture's route, withdrawal, exchange and deposit, its withdrawal naming a call to the exchange
/// for the conversion, from a calls figure of its release.
fn record_naming_a_call() -> DecisionRecord {
    let mut envelope = read("router-api/fixtures/valid/decision-record-split-of-one-part.json");
    let record = &mut envelope["statement"];
    let origin = record["snapshot"]["figures"][0]["origin"].clone();
    record["snapshot"]["figures"].as_array_mut().unwrap().push(json!({
        "figure_id": "f-w-calls",
        "origin": origin,
        "age_seconds": 0,
        "value": { "kind": "calls", "calls": [{ "callee": "exchange-a", "kinds": ["conversion"] }] },
    }));
    let chosen = record["chosen"].as_str().unwrap().to_owned();
    let candidate = record["candidates"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["candidate_id"] == chosen.as_str())
        .unwrap();
    let withdrawal = &mut candidate["parts"][0]["operations"][0];
    assert_eq!(withdrawal["kind"], "withdrawal");
    withdrawal["figures"]
        .as_array_mut()
        .unwrap()
        .push("f-w-calls".into());
    withdrawal["calls"] = json!([{ "callee": "exchange-a", "kind": "conversion" }]);
    serde_json::from_value(record.clone()).expect("the record names its calls")
}

/// Runs the record's first part over `modules`, with a log, a store and policy permitting every move.
fn run(modules: &mut Fake) -> (Ran, KeptLog) {
    let record = record_naming_a_call();
    let clock = TestClock(Cell::new(Reading::at(1_794_200_000)));
    let catalogue = admitting();
    let mut log = KeptLog::default();
    let mut store = Store::default();
    let mut policy = Permitting(0);
    let mut key = TestKey(SigningKey::from_bytes(&[7; 32]));
    let mut ids = Ids::default();
    let issued = Issued::new();
    let mut exposures = Exposures::new();
    let mut segments = Segments::new();
    let (order, part, auth, origin) = (id("ord-1"), id("p1"), id("auth-1"), id("reserve-bank-co"));
    let plan = [id("p1-s1"), id("p1-s2"), id("p1-s3")];
    let end = RouteEnd {
        destination: Destination {
            reserve: id("reserve-wallet"),
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
    let (caller, core_actor, operator_actor) = (core(), core(), operator());
    let session = id("session-1");
    let inputs = BTreeMap::new();
    let live = Live {
        order: &order,
        part: &part,
        segments: &plan,
        record: &record,
        at: 0,
        authorisation: &auth,
        origin_reserve: &origin,
        route: &end,
        caller: &caller,
        core: &core_actor,
        operator: &operator_actor,
        session: Some(&session),
        inputs: &inputs,
        floor: None,
    };
    let mut parts = Parts {
        clock: &clock,
        catalogue: &catalogue,
        log: &mut log,
        evidence: &mut store,
        decisions: &mut policy,
        modules,
        uses: &mut NoProxy,
        grading: &mut ProvidersWord,
        operator_key: &mut key,
        fresh: &mut ids,
        issued: &issued,
        exposures: &mut exposures,
    };
    let ran = segments.run_part(&mut parts, &live).unwrap();
    (ran, log)
}

/// The live withdrawal names its call to the exchange's conversion and no other: the route check lets that one
/// through and refuses another kind to the same callee, and a call to another provider, while the segment is live;
/// once it has ended, it names none. The exchange's segment, whose operation names no call, names none.
// verifies: REQ-MDL-21, REQ-MDL-9
#[test]
fn call_the_live_segment_doesnt_name_is_refused() {
    let mut modules = fake();
    let (ran, log) = run(&mut modules);
    let Ran::Segments(done) = &ran else {
        panic!("{ran:?}");
    };
    assert!(matches!(done[0].outcome, SegmentOutcome::NotHandedOver(_)));
    let (transaction, judged) = &modules.judged[0];
    assert_eq!(
        judged[0], None,
        "the call the live segment names passes the route check"
    );
    assert_eq!(
        judged[1],
        Some(CallRefusalReason::NotNamedByRoute),
        "another kind is refused"
    );
    assert_eq!(
        judged[2],
        Some(CallRefusalReason::NotNamedByRoute),
        "another callee is refused"
    );
    assert!(
        modules
            .routes
            .naming(
                transaction,
                "reserve-bank-co",
                "exchange-a",
                OperationKind::Conversion
            )
            .is_none(),
        "the segment ended, and its calls with it"
    );
    // The effect was recorded before the operation was handed over, and its one result says it failed: the platform
    // refused it before its module passed anything out.
    let kinds: Vec<_> = log
        .0
        .iter()
        .filter_map(|e| match e {
            Entry::Verified(v) => Some((&v.what, &v.outcome)),
            Entry::Stated(_) => None,
        })
        .filter(|(w, _)| matches!(w, What::Effect { .. } | What::Result { .. }))
        .map(|(w, o)| (matches!(w, What::Effect { .. }), *o))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (true, Outcome::ToTakeEffect {}),
            (false, Outcome::Failed {})
        ]
    );
}

/// The record of a segment that moved, from a version marked stale to the one in force, reaches the log when the
/// segment ends, with its move.
// verifies: REQ-MDL-5
#[test]
fn segment_record_reaches_the_log_with_each_move_when_it_ends() {
    let mut modules = fake();
    modules.moved = true;
    let (_, log) = run(&mut modules);
    let records: Vec<_> = log
        .0
        .iter()
        .filter_map(|e| match e {
            Entry::Verified(v) => match &v.what {
                What::HandOver {
                    item:
                        rutent_contracts::core_internal::HandedOver::SegmentRecord {
                            began_on,
                            runs_on,
                            moves,
                            ..
                        },
                    ..
                } => Some((began_on.clone(), runs_on.clone(), moves.clone())),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect();
    assert_eq!(records.len(), 1, "the withdrawal's segment ended");
    let (began_on, runs_on, moves) = &records[0];
    assert_eq!((began_on.as_str(), runs_on.as_str()), ("1.0.0", "1.1.0"));
    assert_eq!(
        moves,
        &vec![rutent_contracts::core_internal::Move {
            from: id("1.0.0"),
            to: id("1.1.0"),
            reason: rutent_contracts::core_internal::MoveReason::Stale,
            at: Timestamp::new(Instant(1_794_200_000).rfc3339().unwrap()).unwrap(),
        }]
    );
}
