//! Evidence and closing with fakes (`T-020-09`): the grading through `CMP-EVIDENCE` over simulated public networks
//! and the private nodes' records, for the routes the simulated providers can't run on the platform: a segment inside
//! a private node, a route across two private nodes, and one movement shared among orders. `CMP-EVIDENCE` itself is
//! the real one: `grade_outcome`, `public_record_check` and `replay_route` over `SimulatedNetwork`.

mod snapshot_support;

use std::collections::BTreeMap;

use rutent_contracts::core_internal::{
    Actor, ActorKind, Destination, Entry, FeeTo, MismatchKind, Receipt, ReplayFound, Role,
    RouteEnd, What,
};
use rutent_contracts::provider_role::{
    Advertisement, Denomination, EvidenceGrade, OuterNode, RESULT_SHAPE, Statement,
};
use rutent_contracts::router_api::{Amount, FeeFormula, Id, Operation, Timestamp};
use rutent_core_orch::closing::{
    Books, Closing, Evidenced, Moved, PublicRecords, Unadmitted, admits,
};
use rutent_core_orch::segments::{Graded, Grading, Short, ToGrade};
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

/// One order's part, graded segment by segment as the segments would grade it.
struct Part<'a> {
    order: Id,
    segments: Vec<Id>,
    operations: &'a [Operation],
    end: RouteEnd,
}

impl Part<'_> {
    fn grade(
        &self,
        grading: &mut dyn Grading,
        place: usize,
        provider: &str,
        transaction: &str,
        bytes: &[u8],
        amount_out: &str,
    ) -> Graded {
        let fee = no_fee();
        self.with(
            place,
            provider,
            transaction,
            bytes,
            amount_out,
            &fee,
            |settled| grading.grade(settled),
        )
    }

    /// The fee records `closing::fees` writes for the segment at `place`, under the operator's `fee`.
    fn fees(
        &self,
        place: usize,
        provider: &str,
        transaction: &str,
        bytes: &[u8],
        amount_out: &str,
        fee: &FeeFormula,
    ) -> Vec<What> {
        self.with(
            place,
            provider,
            transaction,
            bytes,
            amount_out,
            fee,
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
        then: impl FnOnce(&ToGrade<'_>) -> R,
    ) -> R {
        let (core, operator) = (
            actor("rutent-core", Role::Core),
            actor("ops-lead", Role::Operator),
        );
        let evidence = id(&format!("ev-{}-{place}", self.order.as_str()));
        let at = Timestamp::new("2026-11-10T09:00:00Z").unwrap();
        let result = receipt(10 + place as u64);
        let (provider, transaction) = (id(provider), id(transaction));
        then(&ToGrade {
            evidence: &evidence,
            order: &self.order,
            part: &id("p1"),
            segment: &self.segments[place],
            provider: &provider,
            transaction: &transaction,
            result: &result,
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

/// The evidence entries among `whats`: (id, segment, grade, whose).
fn evidence(whats: &[What]) -> Vec<(String, String, EvidenceGrade, String)> {
    whats
        .iter()
        .filter_map(|w| match w {
            What::Evidence {
                evidence,
                segment,
                grade,
                whose,
                ..
            } => Some((
                evidence.as_str().to_owned(),
                segment.as_str().to_owned(),
                *grade,
                whose.as_str().to_owned(),
            )),
            _ => None,
        })
        .collect()
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

/// The exchange inside `ledger-in` settles on its provider's signed word, and once the value leaves the node on a
/// final public record, it closes on the public movements into the node (the withdrawal's, on `co-ach`) and out of it
/// (the deposit's, on `polygon`), each recorded as that network's. Without the movement out, the deposit doesn't
/// settle and nothing closes.
// verifies: REQ-ORD-33
#[test]
fn segment_within_a_private_node_closes_on_the_public_movements_in_and_out() {
    let route = route(None);
    let (catalogue, mut nets, books) = fixture_world();
    let part = Part {
        order: id("ord-1"),
        segments: vec![id("p1-s1"), id("p1-s2"), id("p1-s3")],
        operations: &route,
        end: end("polygon", "acct-9", ("USDC", 6), D.1),
    };
    let mut closing = Closing::new();
    let withdrawal = result(
        "withdrawal",
        "reserve-bank-co",
        "tx-w",
        W,
        ("COP", "100000"),
        json!([piece(3, "book-w", "reserve-bank-co")]),
    );
    let exchange = result(
        "conversion",
        "exchange-a",
        "tx-x",
        X,
        ("COP", "199800"),
        json!([piece(3, "book-x", "exchange-a")]),
    );
    let deposit = result(
        "deposit",
        "reserve-wallet",
        "tx-d",
        D,
        ("USDC", "1000000"),
        json!([
            piece(1, "poly-55", "polygon"),
            piece(3, "book-d", "reserve-wallet")
        ]),
    );
    let graded =
        |nets: &Networks, closing: &mut Closing, place, provider, tx, bytes: &[u8], out| {
            let mut grading = Evidenced {
                catalogue: &catalogue,
                records: nets,
                books: &books,
                closing,
            };
            part.grade(&mut grading, place, provider, tx, bytes, out)
        };
    let (_, w) = settles(graded(
        &nets,
        &mut closing,
        0,
        "reserve-bank-co",
        "tx-w",
        &withdrawal,
        W.1,
    ));
    assert_eq!(
        evidence(&w),
        [(
            "ev-ord-1-0".into(),
            "p1-s1".into(),
            EvidenceGrade::SignedByProvider,
            "reserve-bank-co".into()
        )]
    );
    let (_, x) = settles(graded(
        &nets,
        &mut closing,
        1,
        "exchange-a",
        "tx-x",
        &exchange,
        X.1,
    ));
    assert_eq!(
        evidence(&x),
        [(
            "ev-ord-1-1".into(),
            "p1-s2".into(),
            EvidenceGrade::SignedByProvider,
            "exchange-a".into()
        )],
        "inside the node, its provider's signed word"
    );
    // No public movement out yet: the deposit's statement alone closes nothing.
    assert!(matches!(
        graded(
            &nets,
            &mut closing,
            2,
            "reserve-wallet",
            "tx-d",
            &deposit,
            D.1
        ),
        Graded::Waits(Short::NoPublicRecord(_))
    ));
    nets.moved("polygon", "tx-d", "poly-55", "acct-9", "USDC", D.1);
    let (cites, d) = settles(graded(
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
        evidence(&d),
        [
            (
                "ev-ord-1-2".into(),
                "p1-s3".into(),
                EvidenceGrade::PublicRecord,
                "polygon".into()
            ),
            (
                "ev-ord-1-2.reserve".into(),
                "p1-s3".into(),
                EvidenceGrade::SignedByProvider,
                "reserve-wallet".into()
            ),
            (
                "ev-ord-1-2.in.1".into(),
                "p1-s2".into(),
                EvidenceGrade::PublicRecord,
                "co-ach".into()
            ),
            (
                "ev-ord-1-2.out.1".into(),
                "p1-s2".into(),
                EvidenceGrade::PublicRecord,
                "polygon".into()
            ),
        ],
        "the deposit on the public record and the reserve's statement; the exchange on the movements in and out"
    );
    assert_eq!(
        replays(&d),
        [(
            vec!["ledger-in".to_owned()],
            ReplayFound::ConsistentWithTheExit {}
        )]
    );
    // Its fee records: the reserve's, and the operator's, which it keeps back, by the operator's formula over what
    // arrived, attributed to the operator.
    let operators: FeeFormula = serde_json::from_value(
        json!({ "flat": "10000", "proportional": { "numerator": "0", "denominator": "1" } }),
    )
    .unwrap();
    let fees: Vec<(FeeTo, String, String)> = part
        .fees(2, "reserve-wallet", "tx-d", &deposit, D.1, &operators)
        .into_iter()
        .filter_map(|w| match w {
            What::Fee {
                to, party, amount, ..
            } => Some((to, party.as_str().to_owned(), amount.as_str().to_owned())),
            _ => None,
        })
        .collect();
    assert_eq!(
        fees,
        [
            (
                FeeTo::Provider,
                "reserve-wallet".to_owned(),
                "1000000".to_owned()
            ),
            (FeeTo::Operator, "ops-lead".to_owned(), "10000".to_owned()),
        ]
    );
}

/// Five segments: into `ledger-in`, out of it onto the public `polygon`, from there into `ledger-b`, an exchange
/// inside it, and out of it to the destination. Each node is replayed as the value leaves it, against its own public movement out: two replays,
/// one node each. A record at the second node that doesn't match its movement out is a mismatch recorded against
/// that node, and the first node's replay stands.
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
              (amount_in, amount_out): (&str, &str),
              figures: &[&str]| {
        json!({
            "kind": kind, "provider": provider, "from": at(from, a), "to": at(to, b),
            "amount_in": amount_in, "fee": { "asset": a["id"], "amount": "0" }, "amount_out": amount_out,
            "figures": figures,
        })
    };
    let outer = |network: &str| json!({ "network": network });
    let route = route(Some(json!([
        op(
            "withdrawal",
            "reserve-bank-co",
            (outer("co-ach"), &cop),
            (json!("ledger-in"), &cop),
            ("100000000", "100000000"),
            &["f-w", "f-w-price", "f-confirm-w-100000000"]
        ),
        op(
            "route",
            "ramp-x",
            (json!("ledger-in"), &cop),
            (json!("polygon"), &cop),
            ("100000000", "100000000"),
            &["f-ramp-out", "f-ramp-out-price"]
        ),
        op(
            "route",
            "ramp-x",
            (json!("polygon"), &cop),
            (json!("ledger-b"), &cop),
            ("100000000", "100000000"),
            &["f-ramp-in", "f-ramp-in-price"]
        ),
        op(
            "exchange",
            "exchange-b",
            (json!("ledger-b"), &cop),
            (json!("ledger-b"), &usdc),
            ("100000000", "250000000"),
            &["f-ex-b", "f-ex-b-price", "f-ex-b-eligibility"]
        ),
        op(
            "deposit",
            "reserve-wallet",
            (json!("ledger-b"), &usdc),
            (outer("polygon"), &usdc),
            ("250000000", "250000000"),
            &["f-dep-b", "f-dep-b-price"]
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
        let part = Part {
            order: id("ord-1"),
            segments: (1..=5).map(|n| id(&format!("p1-s{n}"))).collect(),
            operations: &route,
            end: end("polygon", "acct-9", ("USDC", 6), "250000000"),
        };
        let mut closing = Closing::new();
        let mut found = Vec::new();
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
            let mut grading = Evidenced {
                catalogue: &catalogue,
                records: &nets,
                books: &books,
                closing: &mut closing,
            };
            let (_, whats) =
                settles(part.grade(&mut grading, place, provider, &tx, &bytes, amounts.1));
            found.push(replays(&whats));
        }
        found
    };
    let matching = run("250000000");
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
    let mismatching = run("249999999");
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
}

/// Two orders' withdrawals, each its own segment, that their reserve answered with one movement on `co-ach`: the
/// first order's segment is graded on its public record; the second's can't be, since one movement evidences one
/// segment, so its record is the reserve's word. A deposit on a movement another order's deposit closed on doesn't
/// close.
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
    let mut closing = Closing::new();
    let parts: Vec<Part> = ["ord-1", "ord-2"]
        .into_iter()
        .map(|order| Part {
            order: id(order),
            segments: vec![id("p1-s1"), id("p1-s2"), id("p1-s3")],
            operations: &route,
            end: end("polygon", "acct-9", ("USDC", 6), D.1),
        })
        .collect();
    let mut graded = Vec::new();
    for (part, tx) in parts.iter().zip(["tx-w1", "tx-w2"]) {
        let bytes = result(
            "withdrawal",
            "reserve-bank-co",
            tx,
            W,
            ("COP", "100000"),
            json!([piece(1, "ach-shared", "co-ach")]),
        );
        let mut grading = Evidenced {
            catalogue: &catalogue,
            records: &nets,
            books: &books,
            closing: &mut closing,
        };
        graded.push(evidence(
            &settles(part.grade(&mut grading, 0, "reserve-bank-co", tx, &bytes, W.1)).1,
        ));
    }
    assert_eq!(
        graded,
        [
            vec![(
                "ev-ord-1-0".to_owned(),
                "p1-s1".to_owned(),
                EvidenceGrade::PublicRecord,
                "co-ach".to_owned()
            )],
            vec![(
                "ev-ord-2-0".to_owned(),
                "p1-s1".to_owned(),
                EvidenceGrade::Statement,
                "reserve-bank-co".to_owned()
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
            let bytes = result(
                "deposit",
                "reserve-wallet",
                tx,
                D,
                ("USDC", "1000000"),
                json!([piece(1, "poly-shared", "polygon")]),
            );
            let mut grading = Evidenced {
                catalogue: &catalogue,
                records: &nets,
                books: &books,
                closing: &mut closing,
            };
            part.grade(&mut grading, 2, "reserve-wallet", tx, &bytes, D.1)
        })
        .collect();
    assert!(matches!(deposits[0], Graded::Settles { .. }));
    assert_eq!(deposits[1], Graded::Waits(Short::MovementShared));
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
}
