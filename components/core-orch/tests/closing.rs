//! Evidence and closing with fakes (`T-020-09`): the grading through `CMP-EVIDENCE` over simulated public networks
//! and the private nodes' records, for the routes the simulated providers can't run on the platform: a segment inside
//! a private node, a route across two private nodes, and one movement shared among orders. `CMP-EVIDENCE` itself is
//! the real one: `grade_outcome`, `public_record_check` and `replay_route` over `SimulatedNetwork`.

mod snapshot_support;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::{
    Actor, ActorKind, DeploymentOperator, Destination, Entry, FeeTo, MismatchKind, PublicMovement,
    Receipt, ReplayFound, Role, RouteEnd, Through, What, admit_entry,
};
use rutent_contracts::provider_role::{
    Advertisement, Denomination, EvidenceGrade, OuterNode, RESULT_SHAPE, Statement,
};
use rutent_contracts::router_api::{Amount, FeeFormula, Id, Operation, Timestamp};
use rutent_core_orch::closing::{
    Books, Closing, Evidenced, Moved, PublicRecords, Unadmitted, admits,
};
use rutent_core_orch::segments::{Graded, Grading, ProvidersWord, Short, ToGrade};
use rutent_evidence::replay::RecordedSegment;
use rutent_evidence::{
    Finality, Movement, MovementId, NetworkRules, PublicNetwork, ReorgRule, SimulatedNetwork,
    Status,
};
use rutent_prov::catalogue::{
    Bond, Catalogue, Category, Grant, GrantTerms, Instant, ReversalCapacity, tuples_of,
};
use serde_json::{Value, json};

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn read(path: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts")
        .join(path);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn actor(acts: &str, role: Role) -> Actor {
    Actor {
        party: id("operator"),
        acts: id(acts),
        role,
        kind: ActorKind::Component,
    }
}

fn receipt(index: u64) -> Receipt {
    serde_json::from_value(json!({
        "log": "log-1",
        "index": index,
        "entry": { "algorithm": "sha-256", "value": format!("{index:064x}") },
    }))
    .unwrap()
}

/// The router-api fixture's chosen route, or `operations` where given, as the router chose it.
fn route(operations: Option<Value>) -> Vec<Operation> {
    let operations = operations.unwrap_or_else(|| {
        let envelope = read("router-api/fixtures/valid/decision-record-split-of-one-part.json");
        let record = &envelope["statement"];
        record["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["candidate_id"] == record["chosen"])
            .unwrap()["parts"][0]["operations"]
            .clone()
    });
    serde_json::from_value(operations).expect("a route")
}

fn no_fee() -> FeeFormula {
    serde_json::from_value(
        json!({ "flat": "0", "proportional": { "numerator": "0", "denominator": "1" } }),
    )
    .unwrap()
}

/// The fixture's route: a withdrawal at `reserve-bank-co` from `co-ach` into its private `ledger-in`, an exchange at
/// `exchange-a` inside it, COP to USDC, and a deposit at `reserve-wallet` out to `polygon`.
const W: (&str, &str) = ("100000000", "99900000");
const X: (&str, &str) = ("99900000", "249250500");
const D: (&str, &str) = ("249250500", "248250500");

/// Joins `provider` of `publisher` to `catalogue`, declaring evidence of `grade` and the private `nodes`.
fn join(
    catalogue: &mut Catalogue,
    n: usize,
    provider: &str,
    publisher: &str,
    grade: u8,
    nodes: &[&str],
) {
    let ad = snapshot_support::reserve(provider, publisher);
    let mut value = serde_json::to_value(&ad).unwrap();
    value["declarations"]["evidence"]["grade"] = grade.into();
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

/// A catalogue with `polygon` public and each `(provider, publisher, grade, private nodes)` joined.
fn catalogue(providers: &[(&str, &str, u8, &[&str])]) -> Catalogue {
    let mut catalogue = Catalogue::new();
    catalogue.set_context(
        snapshot_support::deployment(),
        rutent_contracts::provider_role::VERSION,
    );
    catalogue.add_public_node("polygon").unwrap();
    for (n, (provider, publisher, grade, nodes)) in providers.iter().enumerate() {
        join(&mut catalogue, n, provider, publisher, *grade, nodes);
    }
    catalogue
}

fn instant() -> NetworkRules {
    NetworkRules {
        finality: Finality::Instant,
        reorg: ReorgRule::Never,
    }
}

/// Simulated public networks, and each segment's movement as the network ties it to its transaction.
#[derive(Default)]
struct Networks {
    nets: BTreeMap<String, SimulatedNetwork>,
    moved: BTreeMap<(String, String), Moved>,
}

impl Networks {
    /// Records `movement` on `network`, made by the segment run under `transaction`.
    fn moved(
        &mut self,
        network: &str,
        transaction: &str,
        movement: &str,
        to: &str,
        asset: &str,
        amount: &str,
    ) {
        let net = self
            .nets
            .entry(network.to_owned())
            .or_insert_with(|| SimulatedNetwork::new(network, instant()));
        let movement_id = MovementId::new(movement).unwrap();
        let _ = net.record(
            movement_id.clone(),
            Movement {
                destination: to.into(),
                asset: asset.into(),
                amount: amount.into(),
            },
        );
        self.moved.insert(
            (network.to_owned(), transaction.to_owned()),
            Moved {
                id: movement_id,
                destination: to.into(),
            },
        );
    }

    fn served(&mut self, network: &str) {
        self.nets
            .entry(network.to_owned())
            .or_insert_with(|| SimulatedNetwork::new(network, instant()));
    }

    /// Serves `network` under `rules`.
    fn served_under(&mut self, network: &str, rules: NetworkRules) {
        self.nets
            .insert(network.to_owned(), SimulatedNetwork::new(network, rules));
    }
}

impl PublicRecords for Networks {
    fn rules(&self, network: &str) -> Option<NetworkRules> {
        self.nets.get(network).map(PublicNetwork::rules)
    }

    fn status(&self, network: &str, id: &MovementId) -> Status {
        self.nets
            .get(network)
            .map_or(Status::Unknown, |net| net.status(id))
    }

    fn movement(&self, network: &str, transaction: &Id) -> Option<Moved> {
        self.moved
            .get(&(network.to_owned(), transaction.as_str().to_owned()))
            .cloned()
    }
}

/// Each private node's record, as its publisher kept it, by transaction.
#[derive(Default)]
struct Kept(BTreeMap<String, Vec<(String, RecordedSegment)>>);

impl Kept {
    fn record(
        &mut self,
        node: &str,
        transaction: &str,
        (asset_in, amount_in): (&str, &str),
        (asset_out, amount_out): (&str, &str),
        fee: &str,
    ) {
        self.0.entry(node.to_owned()).or_default().push((
            transaction.to_owned(),
            RecordedSegment {
                asset_in: asset_in.into(),
                amount_in: amount_in.into(),
                asset_out: asset_out.into(),
                amount_out: amount_out.into(),
                fee: fee.into(),
            },
        ));
    }
}

impl Books for Kept {
    fn recorded(&self, node: &str, transactions: &[Id]) -> Option<Vec<RecordedSegment>> {
        let kept: Vec<RecordedSegment> = self
            .0
            .get(node)?
            .iter()
            .filter(|(tx, _)| transactions.iter().any(|t| t.as_str() == tx))
            .map(|(_, s)| s.clone())
            .collect();
        (!kept.is_empty()).then_some(kept)
    }
}

/// A succeeded result of `kind` by `maker` for `transaction`, with its fee and `pieces` of evidence.
fn result(
    kind: &str,
    maker: &str,
    transaction: &str,
    (amount_in, amount_out): (&str, &str),
    fee: (&str, &str),
    pieces: Value,
) -> Vec<u8> {
    let mut outcome = json!({
        "status": "succeeded",
        "amount_in": amount_in,
        "amount_out": amount_out,
        "fee": { "asset": fee.0, "amount": fee.1 },
        "evidence": pieces,
    });
    if kind == "conversion" {
        outcome["rate"] = "2.5".into();
    }
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

fn piece(grade: u8, reference: &str, by: &str) -> Value {
    json!({ "grade": grade, "reference": reference, "by": by })
}

fn end(network: &str, account: &str, asset: (&str, u8), delivers: &str) -> RouteEnd {
    RouteEnd {
        destination: Destination {
            reserve: id("reserve-wallet"),
            node: OuterNode {
                network: network.into(),
            },
            account: id(account),
        },
        to_asset: Denomination {
            id: asset.0.into(),
            decimals: asset.1,
        },
        delivers: Amount::new(delivers).unwrap(),
    }
}

/// The log the gradings are recorded in, as the log itself admits them: the order's plan and its moves first, then
/// each segment's result as its stated entry, and each entry a grading answers held to the real `admit_entry` over
/// what came before it, the segment's `segment_settled` after.
#[derive(Default)]
struct Log(RefCell<Vec<(Receipt, Entry)>>);

impl Log {
    fn push(&self, entry: Entry) -> Receipt {
        let mut log = self.0.borrow_mut();
        let receipt = receipt(log.len() as u64);
        log.push((receipt.clone(), entry));
        receipt
    }

    /// Admits `entry` over the history, as the log would, and appends it.
    fn admit(&self, entry: Entry) -> Result<Receipt, String> {
        let history = self.0.borrow().clone();
        let operator = DeploymentOperator {
            maker: id("operator"),
            party: id("operator"),
        };
        admit_entry(&history, &entry, Through::General, &operator).map_err(|v| v.0.to_owned())?;
        Ok(self.push(entry))
    }

    fn verified(what: Value) -> Entry {
        serde_json::from_value(json!({
            "kind": "verified",
            "who": { "party": "operator", "acts": "rutent-core", "role": "core", "kind": "component" },
            "what": what,
            "when": { "at": "2026-11-10T09:00:00Z", "source": "core_clock" },
            "outcome": { "status": if what["call"] == "effect" { "to_take_effect" } else { "observed" } },
        }))
        .expect("a verified entry")
    }

    fn input(
        order: &str,
        part: Option<&str>,
        segment: Option<&str>,
        observed: Value,
        record: &str,
    ) -> Entry {
        let mut subject = json!({ "order": order });
        if let Some(part) = part {
            subject["part"] = part.into();
        }
        if let Some(segment) = segment {
            subject["segment"] = segment.into();
        }
        Self::verified(json!({ "call": "input", "input": {
            "record_id": record, "recorded_at": "2026-11-10T09:00:00Z",
            "time_source": "core_clock", "source": { "kind": "orchestrator", "id": "rutent-core" },
            "standing": "stated", "subject": subject, "observed": observed,
        }}))
    }

    fn stated(
        maker: &str,
        role: &str,
        shape: &str,
        order: &str,
        segment: &str,
        transaction: &str,
    ) -> Entry {
        serde_json::from_value(json!({
            "kind": "stated",
            "attribution": { "signature": "verified", "who": { "party": maker, "acts": maker, "role": role, "kind": "component" } },
            "what": { "statement": shape },
            "when": { "at": "2026-11-06T15:04:05Z", "source": "party_stated" },
            "received_at": "2026-11-06T15:04:06Z",
            "outcome": { "status": "stated" },
            "signed_with": { "scheme": "ed25519", "key": "PT4_QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1w" },
            "commitment": { "algorithm": "sha-256", "value": format!("{:064x}", transaction.len() * 7919 + segment.len()) },
            "about": { "order": order, "transaction": transaction, "segment": segment },
        }))
        .expect("a stated entry")
    }

    fn entries(&self) -> Vec<(Receipt, Entry)> {
        self.0.borrow().clone()
    }
}

/// One order's part, graded segment by segment as the segments would grade it, over `log`: each segment bound to its
/// provider before its result, as the segments bind it before they carry its operation out, and graded only once the
/// one before it settled, as the state machine takes them (`input-is-one-the-state-machine-takes`).
struct Part<'a> {
    order: Id,
    segments: Vec<Id>,
    operations: &'a [Operation],
    end: RouteEnd,
    log: &'a Log,
    /// The places bound to their provider, each once.
    bound: RefCell<BTreeSet<usize>>,
    /// The places settled.
    settled: RefCell<BTreeSet<usize>>,
}

impl<'a> Part<'a> {
    /// The part, its plan and each segment's move recorded in `log` first.
    fn new(order: &str, operations: &'a [Operation], end: RouteEnd, log: &'a Log) -> Self {
        let segments: Vec<Id> = (1..=operations.len())
            .map(|n| id(&format!("p1-s{n}")))
            .collect();
        let plan: Vec<Value> = operations
            .iter()
            .zip(&segments)
            .map(|(op, segment)| {
                let op = serde_json::to_value(op).unwrap();
                json!({ "segment_id": segment, "provider": op["provider"], "kind": op["kind"], "from": op["from"], "to": op["to"] })
            })
            .collect();
        log.push(Log::input(
            order,
            None,
            None,
            json!({ "kind": "route_chosen", "decision_record": "dr-1", "parts": [{ "part_id": "p1", "segments": plan }] }),
            &format!("rec-{order}-0"),
        ));
        for (op, segment) in operations.iter().zip(&segments) {
            log.push(Log::verified(json!({
                "call": "effect",
                "action": {
                    "kind": "move_value", "order": order, "segment": segment, "provider": op.provider,
                    "origin_reserve": operations[0].provider, "authorisation": "auth-1", "route": end,
                },
                "decision": { "request": "req-1", "decision": "dec-1", "answer": "permit", "receipt": receipt(0) },
            })));
        }
        Part {
            order: id(order),
            segments,
            operations,
            end,
            log,
            bound: RefCell::default(),
            settled: RefCell::default(),
        }
    }

    /// Settles every segment before `place` not settled yet, each on a result its provider signs for at grade 3 and
    /// moving what its operation moves, graded by `grading`: the part as the state machine would have it before the
    /// segment at `place` runs.
    fn settle_before(&self, grading: &mut dyn Grading, place: usize) {
        for before in 0..place {
            if self.settled.borrow().contains(&before) {
                continue;
            }
            let op = &self.operations[before];
            let kind = match op.kind {
                rutent_contracts::router_api::OperationKind::Withdrawal => "withdrawal",
                rutent_contracts::router_api::OperationKind::Deposit => "deposit",
                rutent_contracts::router_api::OperationKind::Route => "transfer",
                rutent_contracts::router_api::OperationKind::Exchange => "conversion",
            };
            let tx = format!("tx-{}-{before}", self.order.as_str());
            let provider = op.provider.as_str();
            let bytes = result(
                kind,
                provider,
                &tx,
                (op.amount_in.amount().as_str(), op.amount_out.as_str()),
                (&op.from.asset.id, "0"),
                json!([piece(3, &format!("book-{before}"), provider)]),
            );
            settles(self.grade(
                grading,
                before,
                provider,
                &tx,
                &bytes,
                op.amount_out.as_str(),
            ));
        }
    }

    /// Grades the segment at `place`, its result recorded first; what it settles on is admitted and appended, and its
    /// `segment_settled` after it. Every entry the grading answers is admitted, or the test fails.
    fn grade(
        &self,
        grading: &mut dyn Grading,
        place: usize,
        provider: &str,
        transaction: &str,
        bytes: &[u8],
        amount_out: &str,
    ) -> Graded {
        let (graded, admitted) =
            self.try_grade(grading, place, provider, transaction, bytes, amount_out);
        admitted.unwrap_or_else(|why| panic!("the log refuses what the grading answered: {why}"));
        graded
    }

    /// [`Part::grade`], with what the log refused, if it refused anything.
    fn try_grade(
        &self,
        grading: &mut dyn Grading,
        place: usize,
        provider: &str,
        transaction: &str,
        bytes: &[u8],
        amount_out: &str,
    ) -> (Graded, Result<(), String>) {
        let fee = no_fee();
        assert!(
            (0..place).all(|before| self.settled.borrow().contains(&before)),
            "a segment runs once the one before it settled"
        );
        // Bound to its provider before its operation is carried out, once (`segments`: `provider_bound`).
        if self.bound.borrow_mut().insert(place) {
            self.log.push(Log::input(
                self.order.as_str(),
                Some("p1"),
                Some(self.segments[place].as_str()),
                json!({ "kind": "provider_bound" }),
                &format!("rec-{}-bound-{place}", self.order.as_str()),
            ));
        }
        let result = self.log.push(Log::stated(
            provider,
            "provider",
            RESULT_SHAPE,
            self.order.as_str(),
            self.segments[place].as_str(),
            transaction,
        ));
        let graded = self.with(
            place,
            provider,
            transaction,
            bytes,
            amount_out,
            &fee,
            &result,
            |settled| grading.grade(settled),
        );
        let mut admitted = Ok(());
        if let Graded::Settles { cites, entries } = &graded {
            for entry in entries {
                if let Err(why) = self.log.admit(entry.clone()) {
                    admitted = Err(why);
                    break;
                }
            }
            if admitted.is_ok() {
                admitted = self
                    .log
                    .admit(Log::input(
                        self.order.as_str(),
                        Some("p1"),
                        Some(self.segments[place].as_str()),
                        json!({ "kind": "segment_settled", "evidence": cites }),
                        &format!("rec-{}-{}", self.order.as_str(), place + 1),
                    ))
                    .map(|_| ());
            }
            if admitted.is_ok() {
                self.settled.borrow_mut().insert(place);
            }
        }
        (graded, admitted)
    }

    /// The fee records `closing::fees` writes for the segment at `place`, under the operator's `fee`.
    #[allow(clippy::too_many_arguments)]
    fn fees(
        &self,
        place: usize,
        provider: &str,
        transaction: &str,
        bytes: &[u8],
        amount_out: &str,
        fee: &FeeFormula,
    ) -> Vec<What> {
        let result = receipt(0);
        self.with(
            place,
            provider,
            transaction,
            bytes,
            amount_out,
            fee,
            &result,
            |settled| {
                rutent_core_orch::closing::fees(settled)
                    .into_iter()
                    .map(|e| match e {
                        Entry::Verified(v) => v.what,
                        Entry::Stated(_) => panic!("a fee record is verified"),
                    })
                    .collect()
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with<R>(
        &self,
        place: usize,
        provider: &str,
        transaction: &str,
        bytes: &[u8],
        amount_out: &str,
        fee: &FeeFormula,
        result: &Receipt,
        then: impl FnOnce(&ToGrade<'_>) -> R,
    ) -> R {
        let (core, operator) = (
            actor("rutent-core", Role::Core),
            actor("operator", Role::Operator),
        );
        let evidence = id(&format!("ev-{}-{place}", self.order.as_str()));
        let at = Timestamp::new("2026-11-10T09:00:00Z").unwrap();
        let (provider, transaction) = (id(provider), id(transaction));
        then(&ToGrade {
            evidence: &evidence,
            order: &self.order,
            part: &id("p1"),
            segment: &self.segments[place],
            provider: &provider,
            transaction: &transaction,
            result,
            bytes,
            operations: self.operations,
            operator_fee: fee,
            place,
            segments: &self.segments,
            end: &self.end,
            amount_out,
            core: &core,
            operator: &operator,
            at: &at,
        })
    }
}

fn settles(graded: Graded) -> (Id, Vec<What>) {
    match graded {
        Graded::Settles { cites, entries } => (
            cites,
            entries
                .into_iter()
                .map(|e| match e {
                    Entry::Verified(v) => v.what,
                    Entry::Stated(_) => panic!("grading writes verified entries"),
                })
                .collect(),
        ),
        Graded::Waits(short) => panic!("it waits: {short:?}"),
    }
}

fn replays(whats: &[What]) -> Vec<(Vec<String>, ReplayFound)> {
    whats
        .iter()
        .filter_map(|w| match w {
            What::Replay { nodes, found, .. } => Some((
                nodes.iter().map(|n| n.as_str().to_owned()).collect(),
                found.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// The fixture's route over `ledger-in`, declared private by `reserve-bank-co`'s publisher, with each segment's
/// movement and the node's record.
fn fixture_world() -> (Catalogue, Networks, Kept) {
    let catalogue = catalogue(&[
        ("reserve-bank-co", "bank-co", 3, &["ledger-in"]),
        ("exchange-a", "exchange-co", 3, &[]),
        ("reserve-wallet", "wallet-co", 1, &[]),
    ]);
    let mut nets = Networks::default();
    nets.moved("co-ach", "tx-w", "ach-77", "ledger-in-holding", "COP", W.1);
    nets.served("polygon");
    let mut books = Kept::default();
    books.record("ledger-in", "tx-w", ("COP", W.0), ("COP", W.1), "100000");
    books.record("ledger-in", "tx-x", ("COP", X.0), ("USDC", X.1), "199800");
    books.record("ledger-in", "tx-d", ("USDC", D.0), ("USDC", D.1), "1000000");
    (catalogue, nets, books)
}

/// Grades with `CMP-EVIDENCE` over `catalogue`, `nets` and `books`, Core's record across orders in `closing`.
fn grading<'a>(
    catalogue: &'a Catalogue,
    nets: &'a Networks,
    books: &'a Kept,
    closing: &'a mut Closing,
) -> Evidenced<'a> {
    Evidenced {
        catalogue,
        records: nets,
        books,
        closing,
    }
}

/// The evidence entries among `whats`, with the movement each names: (id, segment, grade, whose, movement).
fn named(
    whats: &[What],
) -> Vec<(
    String,
    String,
    EvidenceGrade,
    String,
    Option<PublicMovement>,
)> {
    whats
        .iter()
        .filter_map(|w| match w {
            What::Evidence {
                evidence,
                segment,
                grade,
                whose,
                movement,
                ..
            } => Some((
                evidence.as_str().to_owned(),
                segment.as_str().to_owned(),
                *grade,
                whose.as_str().to_owned(),
                movement.clone(),
            )),
            _ => None,
        })
        .collect()
}

fn moved(network: &str, movement: &str, node: Option<&str>) -> Option<PublicMovement> {
    Some(PublicMovement {
        network: id(network),
        id: id(movement),
        node: node.map(id),
    })
}

fn withdrawal(tx: &str, pieces: Value) -> Vec<u8> {
    result(
        "withdrawal",
        "reserve-bank-co",
        tx,
        W,
        ("COP", "100000"),
        pieces,
    )
}

fn deposit(tx: &str, pieces: Value) -> Vec<u8> {
    result(
        "deposit",
        "reserve-wallet",
        tx,
        D,
        ("USDC", "1000000"),
        pieces,
    )
}

/// The exchange inside `ledger-in` settles on its provider's signed word, and once the value leaves the node on a
/// final public record, it closes on the public movements into the node (the withdrawal's, on `co-ach`) and out of it
/// (the deposit's, on `polygon`), each recorded as that network's and naming the movement and the node. The deposit
/// closes on its own public record, naming its movement, and the reserve's statement; the ledger's replay rests on the
/// deposit's result and names the movement out. Without the movement out, the deposit doesn't settle. Every entry is
/// admitted by the log. The deposit's fee records carry what the reserve stated of billing.
// verifies: REQ-ORD-33
#[test]
fn segment_within_a_private_node_closes_on_the_public_movements_in_and_out() {
    let route = route(None);
    let (catalogue, mut nets, books) = fixture_world();
    let log = Log::default();
    let part = Part::new(
        "ord-1",
        &route,
        end("polygon", "acct-9", ("USDC", 6), D.1),
        &log,
    );
    let mut closing = Closing::new();
    let exchange = result(
        "conversion",
        "exchange-a",
        "tx-x",
        X,
        ("COP", "199800"),
        json!([piece(3, "book-x", "exchange-a")]),
    );
    let pieces = json!([
        piece(1, "poly-55", "polygon"),
        piece(3, "book-d", "reserve-wallet")
    ]);
    let deposit = deposit("tx-d", pieces);
    let w = withdrawal("tx-w", json!([piece(3, "book-w", "reserve-bank-co")]));
    let g = |nets: &Networks, closing: &mut Closing, place, provider, tx, bytes: &[u8], out| {
        part.grade(
            &mut grading(&catalogue, nets, &books, closing),
            place,
            provider,
            tx,
            bytes,
            out,
        )
    };
    let (_, w) = settles(g(
        &nets,
        &mut closing,
        0,
        "reserve-bank-co",
        "tx-w",
        &w,
        W.1,
    ));
    assert_eq!(
        named(&w),
        [(
            "ev-ord-1-0".into(),
            "p1-s1".into(),
            EvidenceGrade::SignedByProvider,
            "reserve-bank-co".into(),
            moved("co-ach", "ach-77", None)
        )],
        "the reserve's signed word, naming the movement Core checked"
    );
    let (_, x) = settles(g(
        &nets,
        &mut closing,
        1,
        "exchange-a",
        "tx-x",
        &exchange,
        X.1,
    ));
    assert_eq!(
        named(&x),
        [(
            "ev-ord-1-1".into(),
            "p1-s2".into(),
            EvidenceGrade::SignedByProvider,
            "exchange-a".into(),
            None
        )],
        "inside the node, its provider's signed word"
    );
    // No public movement out yet: the deposit's statement alone closes nothing.
    let mut waiting = grading(&catalogue, &nets, &books, &mut closing);
    assert!(matches!(
        part.grade(&mut waiting, 2, "reserve-wallet", "tx-d", &deposit, D.1),
        Graded::Waits(Short::NoPublicRecord(_))
    ));
    nets.moved("polygon", "tx-d", "poly-55", "acct-9", "USDC", D.1);
    let (cites, d) = settles(g(
        &nets,
        &mut closing,
        2,
        "reserve-wallet",
        "tx-d",
        &deposit,
        D.1,
    ));
    assert_eq!(cites.as_str(), "ev-ord-1-2");
    assert_eq!(
        named(&d),
        [
            (
                "ev-ord-1-2".into(),
                "p1-s3".into(),
                EvidenceGrade::PublicRecord,
                "polygon".into(),
                moved("polygon", "poly-55", None)
            ),
            (
                "ev-ord-1-2.reserve".into(),
                "p1-s3".into(),
                EvidenceGrade::SignedByProvider,
                "reserve-wallet".into(),
                None
            ),
            (
                "ev-ord-1-2.in.1".into(),
                "p1-s2".into(),
                EvidenceGrade::PublicRecord,
                "co-ach".into(),
                moved("co-ach", "ach-77", Some("ledger-in"))
            ),
            (
                "ev-ord-1-2.out.1".into(),
                "p1-s2".into(),
                EvidenceGrade::PublicRecord,
                "polygon".into(),
                moved("polygon", "poly-55", Some("ledger-in"))
            ),
        ],
        "the deposit on its public record and the reserve's statement; the exchange on the movements in and out"
    );
    let replay = d
        .iter()
        .find_map(|w| match w {
            What::Replay {
                nodes,
                found,
                rests_on,
                movement,
                ..
            } => Some((
                nodes.clone(),
                found.clone(),
                rests_on.clone(),
                movement.clone(),
            )),
            _ => None,
        })
        .expect("the ledger replayed");
    assert_eq!(replay.0, [id("ledger-in")]);
    assert_eq!(replay.1, ReplayFound::ConsistentWithTheExit {});
    assert_eq!(
        replay.3,
        moved("polygon", "poly-55", None),
        "the movement out it was compared with"
    );
    let deposit_result = log
        .entries()
        .into_iter()
        .rev()
        .find(|(_, e)| matches!(e, Entry::Stated(s) if s.about.as_ref().and_then(|a| a.transaction.as_ref()).is_some_and(|t| t.as_str() == "tx-d")))
        .unwrap()
        .0;
    assert_eq!(replay.2, deposit_result, "resting on the deposit's result");
    // Its fee records: the reserve's, as it stated billing, and the operator's, which it keeps back, by the operator's
    // formula over what arrived, attributed to the operator.
    let operators: FeeFormula = serde_json::from_value(
        json!({ "flat": "10000", "proportional": { "numerator": "0", "denominator": "1" } }),
    )
    .unwrap();
    let fees = |bytes: &[u8]| -> Vec<(FeeTo, String, String, Option<bool>)> {
        part.fees(2, "reserve-wallet", "tx-d", bytes, D.1, &operators)
            .into_iter()
            .filter_map(|w| match w {
                What::Fee {
                    to,
                    party,
                    amount,
                    billed_separately,
                    ..
                } => Some((
                    to,
                    party.as_str().to_owned(),
                    amount.as_str().to_owned(),
                    billed_separately,
                )),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        fees(&deposit),
        [
            (
                FeeTo::Provider,
                "reserve-wallet".to_owned(),
                "1000000".to_owned(),
                None
            ),
            (
                FeeTo::Operator,
                "operator".to_owned(),
                "10000".to_owned(),
                None
            ),
        ],
        "a reserve that says nothing of billing: the record says nothing either"
    );
    let mut billed: Value = serde_json::from_slice(&deposit).unwrap();
    billed["statement"]["outcome"]["fee"]["billed_separately"] = true.into();
    let billed = serde_json::to_vec(&billed).unwrap();
    assert_eq!(
        fees(&billed)[0].3,
        Some(true),
        "billed apart, as the reserve stated"
    );
}

/// Five segments: into `ledger-in`, out of it onto the public `polygon`, from there into `ledger-b`, an exchange
/// inside it, and out of it to the destination. Each node is replayed as the value leaves it, against its own public
/// movement out: two replays, one node each. A record at the second node that doesn't match its movement out is a
/// mismatch recorded against that node, the first node's replay stands, and the exchange inside the node caught
/// misreporting gets no public-record entries for the movements in and out.
// verifies: REQ-ORD-41, DEC-24
#[test]
fn route_across_two_private_nodes_replays_each_on_its_way_out() {
    let (cop, usdc) = (
        json!({ "id": "COP", "decimals": 2 }),
        json!({ "id": "USDC", "decimals": 6 }),
    );
    let at = |node: Value, asset: &Value| json!({ "node": node, "asset": asset });
    let op = |kind: &str,
              provider: &str,
              (from, a): (Value, &Value),
              (to, b): (Value, &Value),
              (amount_in, amount_out): (&str, &str)| {
        json!({
            "kind": kind, "provider": provider, "from": at(from, a), "to": at(to, b),
            "amount_in": amount_in, "fee": { "asset": a["id"], "amount": "0" }, "amount_out": amount_out,
            "figures": ["f-1"],
        })
    };
    let outer = |network: &str| json!({ "network": network });
    let route = route(Some(json!([
        op(
            "withdrawal",
            "reserve-bank-co",
            (outer("co-ach"), &cop),
            (json!("ledger-in"), &cop),
            ("100000000", "100000000")
        ),
        op(
            "route",
            "ramp-x",
            (json!("ledger-in"), &cop),
            (json!("polygon"), &cop),
            ("100000000", "100000000")
        ),
        op(
            "route",
            "ramp-x",
            (json!("polygon"), &cop),
            (json!("ledger-b"), &cop),
            ("100000000", "100000000")
        ),
        op(
            "exchange",
            "exchange-b",
            (json!("ledger-b"), &cop),
            (json!("ledger-b"), &usdc),
            ("100000000", "250000000")
        ),
        op(
            "deposit",
            "reserve-wallet",
            (json!("ledger-b"), &usdc),
            (outer("polygon"), &usdc),
            ("250000000", "250000000")
        ),
    ])));
    let catalogue = catalogue(&[
        ("reserve-bank-co", "bank-co", 3, &["ledger-in"]),
        ("ramp-x", "ramp-co", 3, &[]),
        ("exchange-b", "b-co", 3, &["ledger-b"]),
        ("reserve-wallet", "wallet-co", 3, &[]),
    ]);
    let run = |deposit_out: &str| {
        let mut nets = Networks::default();
        nets.moved(
            "co-ach",
            "tx-0",
            "ach-1",
            "ledger-in-holding",
            "COP",
            "100000000",
        );
        nets.moved(
            "polygon",
            "tx-1",
            "poly-1",
            "ramp-x-polygon",
            "COP",
            "100000000",
        );
        nets.moved(
            "polygon",
            "tx-2",
            "poly-2",
            "ledger-b-holding",
            "COP",
            "100000000",
        );
        nets.moved("polygon", "tx-4", "poly-3", "acct-9", "USDC", "250000000");
        let mut books = Kept::default();
        books.record(
            "ledger-in",
            "tx-0",
            ("COP", "100000000"),
            ("COP", "100000000"),
            "0",
        );
        books.record(
            "ledger-in",
            "tx-1",
            ("COP", "100000000"),
            ("COP", "100000000"),
            "0",
        );
        books.record(
            "ledger-b",
            "tx-2",
            ("COP", "100000000"),
            ("COP", "100000000"),
            "0",
        );
        books.record(
            "ledger-b",
            "tx-3",
            ("COP", "100000000"),
            ("USDC", "250000000"),
            "0",
        );
        books.record(
            "ledger-b",
            "tx-4",
            ("USDC", "250000000"),
            ("USDC", deposit_out),
            "0",
        );
        let log = Log::default();
        let part = Part::new(
            "ord-1",
            &route,
            end("polygon", "acct-9", ("USDC", 6), "250000000"),
            &log,
        );
        let mut closing = Closing::new();
        let mut found = Vec::new();
        let mut inner = Vec::new();
        let ran = [
            (
                "withdrawal",
                "reserve-bank-co",
                ("100000000", "100000000"),
                "COP",
            ),
            ("transfer", "ramp-x", ("100000000", "100000000"), "COP"),
            ("transfer", "ramp-x", ("100000000", "100000000"), "COP"),
            (
                "conversion",
                "exchange-b",
                ("100000000", "250000000"),
                "COP",
            ),
            (
                "deposit",
                "reserve-wallet",
                ("250000000", "250000000"),
                "USDC",
            ),
        ];
        for (place, (kind, provider, amounts, asset)) in ran.into_iter().enumerate() {
            let tx = format!("tx-{place}");
            let bytes = result(
                kind,
                provider,
                &tx,
                amounts,
                (asset, "0"),
                json!([piece(3, &format!("book-{place}"), provider)]),
            );
            let (_, whats) = settles(part.grade(
                &mut grading(&catalogue, &nets, &books, &mut closing),
                place,
                provider,
                &tx,
                &bytes,
                amounts.1,
            ));
            found.push(replays(&whats));
            inner.extend(
                named(&whats)
                    .into_iter()
                    .filter(|e| e.1 == "p1-s4")
                    .map(|e| e.0),
            );
        }
        (found, inner)
    };
    let (matching, inner) = run("250000000");
    assert_eq!(
        matching[0],
        [],
        "entering the first node, nothing to replay"
    );
    assert_eq!(
        matching[1],
        [(
            vec!["ledger-in".to_owned()],
            ReplayFound::ConsistentWithTheExit {}
        )],
        "the first node, as the value leaves it onto polygon"
    );
    assert_eq!(matching[2], [], "entering the second");
    assert_eq!(matching[3], [], "inside it");
    assert_eq!(
        matching[4],
        [(
            vec!["ledger-b".to_owned()],
            ReplayFound::ConsistentWithTheExit {}
        )],
        "the second, in turn, as the value leaves it to the destination"
    );
    assert_eq!(
        inner,
        ["ev-ord-1-3", "ev-ord-1-4.in.3", "ev-ord-1-4.out.3"],
        "the exchange closes on the movements in and out"
    );
    let (mismatching, inner) = run("249999999");
    assert_eq!(
        mismatching[1], matching[1],
        "the first node's replay stands"
    );
    let [(nodes, ReplayFound::Mismatch { mismatches })] = mismatching[4].as_slice() else {
        panic!("{:?}", mismatching[4]);
    };
    assert_eq!(nodes, &["ledger-b".to_owned()]);
    assert!(!mismatches.is_empty());
    assert!(mismatches.iter().all(|m| m.node.as_str() == "ledger-b"));
    assert!(
        mismatches
            .iter()
            .any(|m| m.kind == MismatchKind::PublicExit)
    );
    assert_eq!(
        inner,
        ["ev-ord-1-3"],
        "a node caught misreporting: its exchange gets no public record of in and out"
    );
}

/// Two orders' withdrawals, each its own segment, that their reserve answered with one movement on `co-ach`: the
/// first order's segment is graded on its public record, naming the movement; the second's can't be, since one
/// movement evidences one segment, so its record is the reserve's word. A deposit on a movement another order's
/// deposit closed on doesn't close. The log admits each entry.
// verifies: REQ-ORD-38
#[test]
fn shared_movement_keeps_one_segment_per_order_as_the_providers_word() {
    let route = route(None);
    let catalogue = catalogue(&[
        ("reserve-bank-co", "bank-co", 1, &["ledger-in"]),
        ("exchange-a", "exchange-co", 3, &[]),
        ("reserve-wallet", "wallet-co", 1, &[]),
    ]);
    let mut nets = Networks::default();
    for tx in ["tx-w1", "tx-w2"] {
        nets.moved("co-ach", tx, "ach-shared", "ledger-in-holding", "COP", W.1);
    }
    for tx in ["tx-d1", "tx-d2"] {
        nets.moved("polygon", tx, "poly-shared", "acct-9", "USDC", D.1);
    }
    let books = Kept::default();
    let log = Log::default();
    let mut closing = Closing::new();
    let parts: Vec<Part> = ["ord-1", "ord-2"]
        .into_iter()
        .map(|order| {
            Part::new(
                order,
                &route,
                end("polygon", "acct-9", ("USDC", 6), D.1),
                &log,
            )
        })
        .collect();
    let mut graded = Vec::new();
    for (part, tx) in parts.iter().zip(["tx-w1", "tx-w2"]) {
        let bytes = withdrawal(tx, json!([piece(1, "ach-shared", "co-ach")]));
        let mut g = grading(&catalogue, &nets, &books, &mut closing);
        graded.push(named(
            &settles(part.grade(&mut g, 0, "reserve-bank-co", tx, &bytes, W.1)).1,
        ));
    }
    assert_eq!(
        graded,
        [
            vec![(
                "ev-ord-1-0".to_owned(),
                "p1-s1".to_owned(),
                EvidenceGrade::PublicRecord,
                "co-ach".to_owned(),
                moved("co-ach", "ach-shared", None)
            )],
            vec![(
                "ev-ord-2-0".to_owned(),
                "p1-s1".to_owned(),
                EvidenceGrade::Statement,
                "reserve-bank-co".to_owned(),
                None
            )],
        ],
        "each order its own segment; the second on the reserve's word"
    );
    assert_eq!(
        closing.evidences("co-ach", "ach-shared"),
        Some(("ord-1", "p1-s1"))
    );
    let deposits: Vec<Graded> = parts
        .iter()
        .zip(["tx-d1", "tx-d2"])
        .map(|(part, tx)| {
            // Each order's exchange inside the node settles, then its deposit runs.
            part.settle_before(&mut grading(&catalogue, &nets, &books, &mut closing), 2);
            let bytes = deposit(tx, json!([piece(1, "poly-shared", "polygon")]));
            part.grade(
                &mut grading(&catalogue, &nets, &books, &mut closing),
                2,
                "reserve-wallet",
                tx,
                &bytes,
                D.1,
            )
        })
        .collect();
    assert!(matches!(deposits[0], Graded::Settles { .. }));
    assert_eq!(deposits[1], Graded::Waits(Short::MovementShared));
}

/// Two orders to the same account for the same amount; the reserve pays once, one movement on `polygon`, and answers
/// both deposits with it. Core closes the first. It restarts: rebuilt from the log, it knows the movement closed the
/// first, and the second waits. A Core that forgot it can't close the second either: the log refuses a second segment
/// relying on the movement.
// verifies: REQ-ORD-38, REQ-ORD-33
#[test]
fn one_payment_closes_one_order_across_a_restart() {
    let route = route(None);
    let (catalogue, mut nets, books) = fixture_world();
    for tx in ["tx-d1", "tx-d2"] {
        nets.moved("polygon", tx, "poly-once", "acct-9", "USDC", D.1);
    }
    let log = Log::default();
    let parts: Vec<Part> = ["ord-1", "ord-2"]
        .into_iter()
        .map(|order| {
            Part::new(
                order,
                &route,
                end("polygon", "acct-9", ("USDC", 6), D.1),
                &log,
            )
        })
        .collect();
    let bytes = |tx: &str| deposit(tx, json!([piece(3, "book", "reserve-wallet")]));
    let mut before = Closing::new();
    // Each order's withdrawal and exchange settle before its deposit runs.
    for part in &parts {
        part.settle_before(&mut grading(&catalogue, &nets, &books, &mut before), 2);
    }
    let first = parts[0].grade(
        &mut grading(&catalogue, &nets, &books, &mut before),
        2,
        "reserve-wallet",
        "tx-d1",
        &bytes("tx-d1"),
        D.1,
    );
    assert!(matches!(first, Graded::Settles { .. }));
    // Core restarts, its record across orders rebuilt from the log.
    let mut rebuilt = Closing::recorded(&log.entries());
    assert_eq!(
        rebuilt.evidences("polygon", "poly-once"),
        Some(("ord-1", "p1-s3"))
    );
    let second = parts[1].grade(
        &mut grading(&catalogue, &nets, &books, &mut rebuilt),
        2,
        "reserve-wallet",
        "tx-d2",
        &bytes("tx-d2"),
        D.1,
    );
    assert_eq!(second, Graded::Waits(Short::MovementShared));
    // A Core that forgot it: the log refuses the second close.
    let mut forgot = Closing::new();
    let (graded, admitted) = parts[1].try_grade(
        &mut grading(&catalogue, &nets, &books, &mut forgot),
        2,
        "reserve-wallet",
        "tx-d2",
        &bytes("tx-d2"),
        D.1,
    );
    assert!(matches!(graded, Graded::Settles { .. }));
    assert_eq!(
        admitted,
        Err(
            "one public movement evidences one segment of one order: another relied on it already"
                .to_owned()
        )
    );
}

/// A deposit into a private rail from another private node: the only public movement is the customer's payment into
/// the first node, which is no movement into the rail. The part doesn't close on it, and a part ending so isn't
/// routed: nothing could close it (`REQ-ORD-33`, "a reserve that can't give them MUST NOT end a route").
// verifies: REQ-ORD-33
#[test]
fn deposit_into_a_private_rail_from_a_private_node_does_not_close() {
    let usdc = json!({ "id": "USDC", "decimals": 6 });
    let route = route(Some(json!([
        { "kind": "withdrawal", "provider": "reserve-bank-co", "from": { "node": { "network": "co-ach" }, "asset": usdc },
          "to": { "node": "ledger-in", "asset": usdc }, "amount_in": "1000", "fee": { "asset": "USDC", "amount": "0" },
          "amount_out": "1000", "figures": ["f-1"] },
        { "kind": "deposit", "provider": "reserve-wallet", "from": { "node": "ledger-in", "asset": usdc },
          "to": { "node": { "network": "bank-rail" }, "asset": usdc }, "amount_in": "1000",
          "fee": { "asset": "USDC", "amount": "0" }, "amount_out": "1000", "figures": ["f-2"] },
    ])));
    let catalogue = catalogue(&[
        ("reserve-bank-co", "bank-co", 3, &["ledger-in"]),
        ("reserve-wallet", "wallet-co", 3, &["bank-rail"]),
    ]);
    let mut nets = Networks::default();
    nets.moved(
        "co-ach",
        "tx-w",
        "ach-1",
        "ledger-in-holding",
        "USDC",
        "1000",
    );
    let books = Kept::default();
    let log = Log::default();
    let part = Part::new(
        "ord-1",
        &route,
        end("bank-rail", "acct-9", ("USDC", 6), "1000"),
        &log,
    );
    let mut closing = Closing::new();
    let bytes = |kind, provider, tx| {
        result(
            kind,
            provider,
            tx,
            ("1000", "1000"),
            ("USDC", "0"),
            json!([piece(3, "book", provider)]),
        )
    };
    settles(part.grade(
        &mut grading(&catalogue, &nets, &books, &mut closing),
        0,
        "reserve-bank-co",
        "tx-w",
        &bytes("withdrawal", "reserve-bank-co", "tx-w"),
        "1000",
    ));
    let deposit = part.grade(
        &mut grading(&catalogue, &nets, &books, &mut closing),
        1,
        "reserve-wallet",
        "tx-d",
        &bytes("deposit", "reserve-wallet", "tx-d"),
        "1000",
    );
    assert!(
        matches!(deposit, Graded::Waits(Short::NoPublicRecord(_))),
        "the customer's payment into the first node is no movement into the rail: {deposit:?}"
    );
    assert!(
        !grading(&catalogue, &nets, &books, &mut closing).can_close(&route[1]),
        "a part ending so isn't routed"
    );
    // From a public node, the deposit's own movement is the one into the rail: that part can close.
    let mut from_public = route[1].clone();
    from_public.from.node = rutent_contracts::provider_role::NodeRef::Inner("polygon".into());
    assert!(grading(&catalogue, &nets, &books, &mut closing).can_close(&from_public));
}

/// Every movement Core checked is taken, live and after a restart, as the log takes it: order A's withdrawal on
/// `ach-77` (its reserve's signed word, grade 3) and its deposit on `poly-55`, and its exchange inside the node on
/// both. Order B's withdrawal, tied by its reserve to `ach-77`, isn't graded on it, even claiming it as a public record,
/// and its deposit, tied to `poly-55`, waits: so the log admits everything, and nothing loops on a refusal. A Core
/// rebuilt from the log after a restart takes the same movements.
// verifies: REQ-ORD-38
#[test]
fn movements_core_checked_are_taken_live_and_after_a_restart() {
    let route = route(None);
    let (_, mut nets, books) = fixture_world();
    // The withdrawal's reserve declares public records: order B's claims one.
    let catalogue = catalogue(&[
        ("reserve-bank-co", "bank-co", 1, &["ledger-in"]),
        ("exchange-a", "exchange-co", 3, &[]),
        ("reserve-wallet", "wallet-co", 1, &[]),
    ]);
    nets.moved("polygon", "tx-d", "poly-55", "acct-9", "USDC", D.1);
    nets.moved("co-ach", "tx-wb", "ach-77", "ledger-in-holding", "COP", W.1);
    nets.moved("polygon", "tx-db", "poly-55", "acct-9", "USDC", D.1);
    let log = Log::default();
    let a = Part::new(
        "ord-a",
        &route,
        end("polygon", "acct-9", ("USDC", 6), D.1),
        &log,
    );
    let mut live = Closing::new();
    let exchange = result(
        "conversion",
        "exchange-a",
        "tx-x",
        X,
        ("COP", "199800"),
        json!([piece(3, "book-x", "exchange-a")]),
    );
    for (place, provider, tx, bytes, out) in [
        (
            0,
            "reserve-bank-co",
            "tx-w",
            withdrawal("tx-w", json!([piece(3, "book-w", "reserve-bank-co")])),
            W.1,
        ),
        (1, "exchange-a", "tx-x", exchange, X.1),
        (
            2,
            "reserve-wallet",
            "tx-d",
            deposit("tx-d", json!([piece(3, "book-d", "reserve-wallet")])),
            D.1,
        ),
    ] {
        settles(a.grade(
            &mut grading(&catalogue, &nets, &books, &mut live),
            place,
            provider,
            tx,
            &bytes,
            out,
        ));
    }
    let a_withdrawal = named(&settles_of(&log, "ord-a", "p1-s1"));
    assert_eq!(
        a_withdrawal[0].4,
        moved("co-ach", "ach-77", None),
        "the reserve's signed word names the movement Core checked, so the log holds it"
    );
    let b_withdrawal = withdrawal("tx-wb", json!([piece(1, "ach-77", "co-ach")]));
    let b_deposit = deposit("tx-db", json!([piece(1, "poly-55", "polygon")]));
    let after = |closing: &mut Closing, log: &Log| {
        let b = Part::new(
            "ord-b",
            &route,
            end("polygon", "acct-9", ("USDC", 6), D.1),
            log,
        );
        let (_, w) = settles(b.grade(
            &mut grading(&catalogue, &nets, &books, closing),
            0,
            "reserve-bank-co",
            "tx-wb",
            &b_withdrawal,
            W.1,
        ));
        // B's exchange inside the node, then its deposit.
        b.settle_before(&mut grading(&catalogue, &nets, &books, closing), 2);
        let d = b.grade(
            &mut grading(&catalogue, &nets, &books, closing),
            2,
            "reserve-wallet",
            "tx-db",
            &b_deposit,
            D.1,
        );
        (named(&w), d)
    };
    // Live.
    let (w, d) = after(&mut live, &log);
    assert_eq!(
        w,
        [(
            "ev-ord-b-0".into(),
            "p1-s1".into(),
            EvidenceGrade::Statement,
            "reserve-bank-co".into(),
            None
        )],
        "order A's movement backs nothing of order B's"
    );
    assert_eq!(d, Graded::Waits(Short::MovementShared));
    // After a restart, from a log that holds A's part alone.
    let restarted = Log::default();
    for entry in log
        .entries()
        .into_iter()
        .filter(|(_, e)| !about(e, "ord-b"))
    {
        restarted.push(entry.1);
    }
    let mut rebuilt = Closing::recorded(&restarted.entries());
    let (w, d) = after(&mut rebuilt, &restarted);
    assert_eq!(
        w,
        [(
            "ev-ord-b-0".into(),
            "p1-s1".into(),
            EvidenceGrade::Statement,
            "reserve-bank-co".into(),
            None
        )]
    );
    assert_eq!(d, Graded::Waits(Short::MovementShared));
    // A Core that forgot A's part entirely: B's withdrawal would claim `ach-77`, and the log refuses it, since A's
    // signed word, grade 3, named it already (the exchange's records of the node's movements left out, so it is the
    // signed word alone that the log holds to it).
    let forgot_log = Log::default();
    for entry in log.entries().into_iter().filter(|(_, e)| {
        !about(e, "ord-b") && !about(e, "ev-ord-a-2.in.1") && !about(e, "ev-ord-a-2.out.1")
    }) {
        forgot_log.push(entry.1);
    }
    let b = Part::new(
        "ord-b",
        &route,
        end("polygon", "acct-9", ("USDC", 6), D.1),
        &forgot_log,
    );
    let mut forgot = Closing::new();
    let (graded, admitted) = b.try_grade(
        &mut grading(&catalogue, &nets, &books, &mut forgot),
        0,
        "reserve-bank-co",
        "tx-wb",
        &b_withdrawal,
        W.1,
    );
    assert!(matches!(graded, Graded::Settles { .. }));
    assert_eq!(
        admitted,
        Err(
            "one public movement evidences one segment of one order: another relied on it already"
                .to_owned()
        )
    );
}

/// Core restarts between a grade-3 withdrawal into `ledger-in` and the deposit out of it: rebuilt from the log, it
/// knows the withdrawal's movement into the node from the evidence that names it, and the exchange inside the node
/// still closes on the movements in and out when the deposit leaves on its public record.
// verifies: REQ-ORD-33, REQ-ORD-41
#[test]
fn restart_between_a_withdrawal_and_its_deposit_keeps_the_movement_into_the_node() {
    let route = route(None);
    let (catalogue, mut nets, books) = fixture_world();
    nets.moved("polygon", "tx-d", "poly-55", "acct-9", "USDC", D.1);
    let log = Log::default();
    let part = Part::new(
        "ord-1",
        &route,
        end("polygon", "acct-9", ("USDC", 6), D.1),
        &log,
    );
    let mut before = Closing::new();
    let exchange = result(
        "conversion",
        "exchange-a",
        "tx-x",
        X,
        ("COP", "199800"),
        json!([piece(3, "book-x", "exchange-a")]),
    );
    for (place, provider, tx, bytes, out) in [
        (
            0,
            "reserve-bank-co",
            "tx-w",
            withdrawal("tx-w", json!([piece(3, "book-w", "reserve-bank-co")])),
            W.1,
        ),
        (1, "exchange-a", "tx-x", exchange, X.1),
    ] {
        settles(part.grade(
            &mut grading(&catalogue, &nets, &books, &mut before),
            place,
            provider,
            tx,
            &bytes,
            out,
        ));
    }
    // Core restarts.
    let mut rebuilt = Closing::recorded(&log.entries());
    let (_, d) = settles(part.grade(
        &mut grading(&catalogue, &nets, &books, &mut rebuilt),
        2,
        "reserve-wallet",
        "tx-d",
        &deposit("tx-d", json!([piece(1, "poly-55", "polygon")])),
        D.1,
    ));
    let inner: Vec<_> = named(&d).into_iter().filter(|e| e.1 == "p1-s2").collect();
    assert_eq!(
        inner,
        [
            (
                "ev-ord-1-2.in.1".into(),
                "p1-s2".into(),
                EvidenceGrade::PublicRecord,
                "co-ach".into(),
                moved("co-ach", "ach-77", Some("ledger-in"))
            ),
            (
                "ev-ord-1-2.out.1".into(),
                "p1-s2".into(),
                EvidenceGrade::PublicRecord,
                "polygon".into(),
                moved("polygon", "poly-55", Some("ledger-in"))
            ),
        ],
        "the movement into the node, rebuilt from the withdrawal's signed word naming it"
    );
    assert_eq!(
        replays(&d),
        [(
            vec!["ledger-in".to_owned()],
            ReplayFound::ConsistentWithTheExit {}
        )]
    );
}

/// The entries settling `segment` of `order`: its evidence and what came with it.
fn settles_of(log: &Log, order: &str, segment: &str) -> Vec<What> {
    log.entries()
        .into_iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Evidence {
                    order: o,
                    segment: s,
                    ..
                } if o.as_str() == order && s.as_str() == segment => Some(v.what.clone()),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect()
}

/// Whether `entry` is about `order`.
fn about(entry: &Entry, order: &str) -> bool {
    serde_json::to_string(entry)
        .unwrap()
        .contains(&format!("\"{order}\""))
}

/// A route to a rail declared private, from a public node: the deposit's movement on `polygon` is the movement into
/// the private rail, and the part closes on it and the reserve's statement, the record naming the rail it went
/// into. A route ending inside the node has no replay.
// verifies: REQ-ORD-33
#[test]
fn final_deposit_into_a_private_rail_closes_on_the_public_movement_into_it() {
    let usdc = json!({ "id": "USDC", "decimals": 6 });
    let route = route(Some(json!([
        { "kind": "withdrawal", "provider": "reserve-x", "from": { "node": { "network": "co-ach" }, "asset": usdc },
          "to": { "node": "polygon", "asset": usdc }, "amount_in": "1000", "fee": { "asset": "USDC", "amount": "0" },
          "amount_out": "1000", "figures": ["f-1"] },
        { "kind": "deposit", "provider": "reserve-wallet", "from": { "node": "polygon", "asset": usdc },
          "to": { "node": { "network": "bank-rail" }, "asset": usdc }, "amount_in": "1000",
          "fee": { "asset": "USDC", "amount": "0" }, "amount_out": "1000", "figures": ["f-2"] },
    ])));
    let catalogue = catalogue(&[
        ("reserve-x", "x-co", 3, &[]),
        ("reserve-wallet", "wallet-co", 3, &["bank-rail"]),
    ]);
    let mut nets = Networks::default();
    nets.moved("co-ach", "tx-w", "ach-9", "polygon-holding", "USDC", "1000");
    nets.moved("polygon", "tx-d", "poly-9", "bank-rail-in", "USDC", "1000");
    let books = Kept::default();
    let log = Log::default();
    let part = Part::new(
        "ord-1",
        &route,
        end("bank-rail", "acct-9", ("USDC", 6), "1000"),
        &log,
    );
    let mut closing = Closing::new();
    let bytes = |kind, provider, tx| {
        result(
            kind,
            provider,
            tx,
            ("1000", "1000"),
            ("USDC", "0"),
            json!([piece(3, "book", provider)]),
        )
    };
    settles(part.grade(
        &mut grading(&catalogue, &nets, &books, &mut closing),
        0,
        "reserve-x",
        "tx-w",
        &bytes("withdrawal", "reserve-x", "tx-w"),
        "1000",
    ));
    let (_, d) = settles(part.grade(
        &mut grading(&catalogue, &nets, &books, &mut closing),
        1,
        "reserve-wallet",
        "tx-d",
        &bytes("deposit", "reserve-wallet", "tx-d"),
        "1000",
    ));
    assert_eq!(
        named(&d),
        [
            (
                "ev-ord-1-1".into(),
                "p1-s2".into(),
                EvidenceGrade::PublicRecord,
                "polygon".into(),
                moved("polygon", "poly-9", Some("bank-rail"))
            ),
            (
                "ev-ord-1-1.reserve".into(),
                "p1-s2".into(),
                EvidenceGrade::SignedByProvider,
                "reserve-wallet".into(),
                None
            ),
        ]
    );
    assert_eq!(replays(&d), [], "ending inside the node: no replay");
}

/// A movement that isn't the deposit's to its destination doesn't close the order: to another account, of another
/// asset, or not yet final on its network. Each waits, saying why.
// verifies: REQ-ORD-33
#[test]
fn evidence_for_another_segment_does_not_close_an_order() {
    use rutent_evidence::Unmatched;
    let route = route(None);
    let (catalogue, _, books) = fixture_world();
    let cases: [(&str, &str, &str, Option<NetworkRules>, Unmatched); 3] = [
        ("acct-other", "USDC", D.1, None, Unmatched::WrongDestination),
        ("acct-9", "USDT", D.1, None, Unmatched::WrongAsset),
        (
            "acct-9",
            "USDC",
            D.1,
            Some(NetworkRules {
                finality: Finality::Confirmations(2),
                reorg: ReorgRule::DepthBound(2),
            }),
            Unmatched::NotFinal,
        ),
    ];
    for (to, asset, amount, rules, why) in cases {
        let mut nets = Networks::default();
        if let Some(rules) = rules {
            nets.served_under("polygon", rules);
        }
        nets.moved("polygon", "tx-d", "poly-1", to, asset, amount);
        let log = Log::default();
        let part = Part::new(
            "ord-1",
            &route,
            end("polygon", "acct-9", ("USDC", 6), D.1),
            &log,
        );
        let mut closing = Closing::new();
        part.settle_before(&mut grading(&catalogue, &nets, &books, &mut closing), 2);
        let graded = part.grade(
            &mut grading(&catalogue, &nets, &books, &mut closing),
            2,
            "reserve-wallet",
            "tx-d",
            &deposit("tx-d", json!([piece(1, "poly-1", "polygon")])),
            D.1,
        );
        assert_eq!(
            graded,
            Graded::Waits(Short::NoPublicRecord(Some(why))),
            "{to} {asset}"
        );
    }
}

/// A public record a provider's piece names, however it names who gave it, is the network's that Core checked: the
/// withdrawal's piece says an auditor of the reserve's choosing gave it, and the record says `co-ach`.
// verifies: REQ-ORD-39
#[test]
fn public_record_is_the_networks_whatever_the_piece_names() {
    let route = route(None);
    let catalogue = catalogue(&[("reserve-bank-co", "bank-co", 1, &["ledger-in"])]);
    let (_, nets, books) = fixture_world();
    let log = Log::default();
    let part = Part::new(
        "ord-1",
        &route,
        end("polygon", "acct-9", ("USDC", 6), D.1),
        &log,
    );
    let mut closing = Closing::new();
    let (_, w) = settles(part.grade(
        &mut grading(&catalogue, &nets, &books, &mut closing),
        0,
        "reserve-bank-co",
        "tx-w",
        &withdrawal("tx-w", json!([piece(1, "ach-77", "auditor-x")])),
        W.1,
    ));
    assert_eq!(
        named(&w),
        [(
            "ev-ord-1-0".into(),
            "p1-s1".into(),
            EvidenceGrade::PublicRecord,
            "co-ach".into(),
            moved("co-ach", "ach-77", None)
        )]
    );
}

/// The stand-in grading settles a segment on its provider's word, but never a part's final deposit: a statement
/// alone closes no order. The part reaches its deposit as the state machine takes it, each segment before it settled.
// verifies: REQ-ORD-33
#[test]
fn providers_word_never_closes_a_part() {
    let route = route(None);
    let log = Log::default();
    let part = Part::new(
        "ord-1",
        &route,
        end("polygon", "acct-9", ("USDC", 6), D.1),
        &log,
    );
    let w = withdrawal("tx-w", json!([piece(3, "book-w", "reserve-bank-co")]));
    assert!(matches!(
        part.grade(&mut ProvidersWord, 0, "reserve-bank-co", "tx-w", &w, W.1),
        Graded::Settles { .. }
    ));
    // The exchange inside the node settles on its provider's word too: then the part reaches its deposit.
    part.settle_before(&mut ProvidersWord, 2);
    let d = deposit("tx-d", json!([piece(3, "book-d", "reserve-wallet")]));
    assert_eq!(
        part.grade(&mut ProvidersWord, 2, "reserve-wallet", "tx-d", &d, D.1),
        Graded::Waits(Short::NoPublicRecord(None))
    );
    // A grading that settled it on the reserve's word anyway: the log refuses the close.
    struct Word;
    impl Grading for Word {
        fn grade(&mut self, settled: &ToGrade<'_>) -> Graded {
            let mut stand_in = ProvidersWord;
            let mut middle = *settled;
            middle.place = 0;
            match stand_in.grade(&middle) {
                Graded::Settles { cites, mut entries } => {
                    if let Some(Entry::Verified(v)) = entries.first_mut()
                        && let What::Evidence { segment, .. } = &mut v.what
                    {
                        *segment = settled.segment.clone();
                    }
                    Graded::Settles { cites, entries }
                }
                waits => waits,
            }
        }
    }
    let (graded, admitted) = part.try_grade(&mut Word, 2, "reserve-wallet", "tx-d", &d, D.1);
    assert!(matches!(graded, Graded::Settles { .. }));
    assert!(
        admitted
            .as_ref()
            .is_err_and(|why| why.contains("never a statement alone")),
        "{admitted:?}"
    );
}

/// Two adjacent private nodes whose publishers recorded nothing: the replay names both.
// verifies: REQ-ORD-41
#[test]
fn every_node_that_recorded_nothing_is_named() {
    let usdc = json!({ "id": "USDC", "decimals": 6 });
    let at = |node: Value| json!({ "node": node, "asset": usdc });
    let op = |kind: &str, provider: &str, from: Value, to: Value| {
        json!({ "kind": kind, "provider": provider, "from": at(from), "to": at(to), "amount_in": "1000",
                "fee": { "asset": "USDC", "amount": "0" }, "amount_out": "1000", "figures": ["f-1"] })
    };
    let route = route(Some(json!([
        op(
            "withdrawal",
            "reserve-bank-co",
            json!({ "network": "co-ach" }),
            json!("ledger-in")
        ),
        op("route", "ramp-x", json!("ledger-in"), json!("ledger-b")),
        op(
            "deposit",
            "reserve-wallet",
            json!("ledger-b"),
            json!({ "network": "polygon" })
        ),
    ])));
    let catalogue = catalogue(&[
        ("reserve-bank-co", "bank-co", 3, &["ledger-in"]),
        ("ramp-x", "ramp-co", 3, &[]),
        ("reserve-wallet", "wallet-co", 3, &["ledger-b"]),
    ]);
    let mut nets = Networks::default();
    nets.moved(
        "co-ach",
        "tx-0",
        "ach-1",
        "ledger-in-holding",
        "USDC",
        "1000",
    );
    nets.moved("polygon", "tx-2", "poly-1", "acct-9", "USDC", "1000");
    let books = Kept::default();
    let log = Log::default();
    let part = Part::new(
        "ord-1",
        &route,
        end("polygon", "acct-9", ("USDC", 6), "1000"),
        &log,
    );
    let mut closing = Closing::new();
    let mut last = Vec::new();
    for (place, (kind, provider)) in [
        ("withdrawal", "reserve-bank-co"),
        ("transfer", "ramp-x"),
        ("deposit", "reserve-wallet"),
    ]
    .into_iter()
    .enumerate()
    {
        let tx = format!("tx-{place}");
        let bytes = result(
            kind,
            provider,
            &tx,
            ("1000", "1000"),
            ("USDC", "0"),
            json!([piece(3, "book", provider)]),
        );
        last = settles(part.grade(
            &mut grading(&catalogue, &nets, &books, &mut closing),
            place,
            provider,
            &tx,
            &bytes,
            "1000",
        ))
        .1;
    }
    assert_eq!(
        replays(&last),
        [(
            vec!["ledger-in".to_owned(), "ledger-b".to_owned()],
            ReplayFound::NothingRecorded {
                nodes: vec![id("ledger-in"), id("ledger-b")]
            }
        )]
    );
}

/// Joins the provider-role fixture advertisement `fixture` to `catalogue`, granted for every tuple it offers in
/// `category`, with the floor `Statement` on each.
fn join_fixture(catalogue: &mut Catalogue, fixture: &str, category: Category) -> Advertisement {
    let ad: Statement<Advertisement> = serde_json::from_value(read(&format!(
        "provider-role/fixtures/valid/{fixture}.json"
    )))
    .unwrap();
    let ad = ad.statement;
    let grant = Grant::new(GrantTerms {
        id: format!("g-{}", ad.provider),
        provider: ad.provider.clone(),
        party: "Acme Payments Ltd".into(),
        granted_by: "operator-admin".into(),
        granted_at: Instant(1_000),
        expires_at: Instant(5_000_000_000),
        categories: vec![category],
        tuples: tuples_of(&ad),
        reversal_capacity: ReversalCapacity::check(
            Bond {
                asset: "USDC".into(),
                amount: 1_000,
                held_by: "escrow-co".into(),
            },
            500,
            "ops-lead",
            Instant(1_000),
        )
        .unwrap(),
    })
    .unwrap();
    let id = format!("g-{}", ad.provider);
    catalogue.grant(grant).unwrap();
    catalogue
        .trust_advertising_key(
            &id,
            snapshot_support::public_key(&snapshot_support::advertising_key(&ad.provider)),
        )
        .unwrap();
    let keys = snapshot_support::keys_of(
        &ad.provider,
        &[&snapshot_support::statements_key(&ad.provider, 1)],
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
    ad
}

/// The guard before a hand-over reads an exchange's and a ramp's tuples as the catalogue keys them: an operation at
/// each is admitted under the floor its tuple has, refused once the floor is above what its provider declares, and a
/// ramp into a private node with no holding account there is refused.
// verifies: REQ-ORD-39, INV-5
#[test]
fn exchange_and_ramp_operations_are_held_to_their_own_tuples_floors() {
    let mut catalogue = Catalogue::new();
    catalogue.set_context(
        snapshot_support::deployment(),
        rutent_contracts::provider_role::VERSION,
    );
    catalogue.add_public_node("polygon").unwrap();
    catalogue.publish_operator_module("wallet-module").unwrap();
    catalogue
        .register_public_holding("polygon", "wallet-module")
        .unwrap();
    join_fixture(
        &mut catalogue,
        "advertisement-exchange-within-one-node",
        Category::Exchange,
    );
    join_fixture(
        &mut catalogue,
        "advertisement-ramp-between-a-public-and-a-private-node",
        Category::Ramp,
    );
    let usdc = json!({ "id": "USDC", "decimals": 6 });
    let usdt = json!({ "id": "USDT", "decimals": 6 });
    let op = |kind: &str, provider: &str, from: (&str, &Value), to: (&str, &Value)| -> Operation {
        serde_json::from_value(json!({
            "kind": kind, "provider": provider,
            "from": { "node": from.0, "asset": from.1 }, "to": { "node": to.0, "asset": to.1 },
            "amount_in": "1000", "fee": { "asset": from.1["id"], "amount": "0" }, "amount_out": "1000",
            "figures": ["f-1"],
        }))
        .unwrap()
    };
    let exchange = op(
        "exchange",
        "dex-pool",
        ("polygon", &usdc),
        ("polygon", &usdt),
    );
    let ramp = op(
        "route",
        "bridge-pay",
        ("polygon", &usdc),
        ("acme-ledger", &usdc),
    );
    assert_eq!(admits(&catalogue, &exchange, false), Ok(()));
    assert_eq!(
        admits(&catalogue, &ramp, false),
        Err(Unadmitted::NoHoldingAccount),
        "no holding account at acme-ledger yet"
    );
    catalogue
        .register_private_holding("acme-ledger", "acme", "holding")
        .unwrap();
    assert_eq!(admits(&catalogue, &ramp, false), Ok(()));
    // The ramp declares grade 2: a floor of 1 on its tuple stops it, and the exchange's tuple is its own.
    let ramp_ad: Statement<Advertisement> = serde_json::from_value(read(
        "provider-role/fixtures/valid/advertisement-ramp-between-a-public-and-a-private-node.json",
    ))
    .unwrap();
    for key in tuples_of(&ramp_ad.statement) {
        catalogue.set_evidence_floor(key, EvidenceGrade::PublicRecord);
    }
    assert_eq!(
        admits(&catalogue, &ramp, false),
        Err(Unadmitted::BelowEvidenceFloor)
    );
    assert_eq!(admits(&catalogue, &exchange, false), Ok(()));
    // A provider the catalogue doesn't list is that, not one below a floor.
    let nobody = op("exchange", "nobody", ("polygon", &usdc), ("polygon", &usdt));
    assert_eq!(
        admits(&catalogue, &nobody, false),
        Err(Unadmitted::NotListed)
    );
}
