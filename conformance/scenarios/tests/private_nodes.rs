//! Private-node scenarios over the assembled Core (`T-024-05`, `DEC-24`, `REQ-CAT-20`, `REQ-CAT-22`, `REQ-ORD-41`):
//! one publisher's providers and private node on a route, a final deposit at a rail declared private, the replay of the
//! publishers' records against the public movement out of a private node, a shared holding account checked against
//! its orders, and routes across two private nodes.
//!
//! The platform's routes are the simulated providers that join one another's nodes (`rutent_conformance::sim::shape_of`):
//! `sim-join-*` for one publisher's reserve, exchange and ramp, and `sim-pair-*` for two publishers' private nodes with
//! a public node between them, in four segments (the router takes at most four hops).

mod execution_support;

use execution_support::*;
use rutent_conformance::sim::Behaviour;
use rutent_contracts::core_internal::{Difference, Direction, ReplayFound, What, Word};
use rutent_contracts::provider_role::{EvidenceGrade, NodeRef, Operation};
use rutent_contracts::router_api::{Id, OperationKind};
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::intake::{OrderRequest, Status, Told};
use rutent_core_orch::segments::{Accounted, Ran, SegmentOutcome};
use rutent_core_orch::snapshot::RouteRefusal;
use rutent_evidence::replay::{NodeRecord, RecordedSegment, Replay, RouteEnd, replay_route};
use rutent_prov::catalogue::LeftOutReason;
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{self, AMOUNT};
use serde_json::json;

const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const RAMP: &str = "sim-join-ramp";
const OUT: &str = "sim-join-out";
const OUT_PRIVATE: &str = "sim-join-out-private";
const OUT_SILENT: &str = "sim-join-out-silent";
const NODE: &str = "sim-join-ledger";
const CHAIN: &str = "sim-join-chain";
const PUBLISHER: &str = "sim-join-publisher";
/// The bank rail of the reserve that declares it private.
const RAIL: &str = "sim-join-out-private-bank";

const PAIR_IN: &str = "sim-pair-in";
const PAIR_RAMP_A: &str = "sim-pair-ramp-a";
const PAIR_RAMP_B: &str = "sim-pair-ramp-b";
const PAIR_OUT: &str = "sim-pair-out";
const NODE_A: &str = "sim-pair-a-ledger";
const NODE_B: &str = "sim-pair-b-ledger";
const BETWEEN: &str = "sim-pair-x";

/// The order that carries `AMOUNT` of COP from `sim-join-in`'s bank to USD at `rail`, delivered to the reserve `at`.
fn across(name: &str, session: &Id, at: &str, rail: &str) -> OrderRequest {
    request(
        name,
        session,
        AMOUNT,
        IN,
        outer("sim-join-in-bank", "COP"),
        outer(rail, "USD"),
        at,
        OTHER,
        "lowest_total_cost",
    )
}

/// The order across the two private nodes: COP from `sim-pair-in`'s bank to `sim-pair-out`'s.
fn across_two(name: &str, session: &Id) -> OrderRequest {
    request(
        name,
        session,
        AMOUNT,
        PAIR_IN,
        outer("sim-pair-in-bank", "COP"),
        outer("sim-pair-out-bank", "COP"),
        PAIR_OUT,
        OTHER,
        "lowest_total_cost",
    )
}

fn accounted(ran: &[Ran]) -> &[Accounted] {
    let [Ran::Segments(segments)] = ran else {
        panic!("one part ran its segments: {ran:?}");
    };
    segments
}

fn transaction(segment: &Accounted) -> Id {
    segment
        .transaction
        .clone()
        .expect("a transaction was assigned")
}

fn gave(segment: &Accounted) -> &str {
    let SegmentOutcome::Settled { amount_out, .. } = &segment.outcome else {
        panic!("the segment settled: {segment:?}");
    };
    amount_out.as_str()
}

/// The order across the joined route to the reserve `at`, run: its core, its decision record's route and its segments.
fn run_across(
    names: &[&str],
    at: &str,
    rail: &str,
    script: impl FnOnce(&mut Core),
) -> (Core, Id, Vec<Accounted>) {
    let (mut core, session) = core_with("AC-35", names);
    script(&mut core);
    let (order, ran) = run(&mut core, across("req-join", &session, at, rail));
    let segments = accounted(&ran).to_vec();
    assert_eq!(segments.len(), 4, "{segments:?}");
    assert!(
        segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{segments:?}"
    );
    (core, order, segments)
}

/// What the order's replays found, with the segment each was for and the nodes it replayed.
fn replays(core: &Core, order: &Id) -> Vec<(Id, Vec<String>, ReplayFound)> {
    entries(core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Replay {
                order: o,
                segment,
                nodes,
                found,
                ..
            } if &o == order => Some((
                segment,
                nodes.iter().map(|n| n.as_str().to_owned()).collect(),
                found,
            )),
            _ => None,
        })
        .collect()
}

fn recorded(
    asset_in: &str,
    amount_in: &str,
    asset_out: &str,
    amount_out: &str,
    fee: &str,
) -> RecordedSegment {
    RecordedSegment {
        asset_in: asset_in.to_owned(),
        amount_in: amount_in.to_owned(),
        asset_out: asset_out.to_owned(),
        amount_out: amount_out.to_owned(),
        fee: fee.to_owned(),
    }
}

// verifies: AC-29
#[test]
fn route_through_one_publishers_providers_and_private_node_is_expressed_and_run() {
    let (mut core, session) = core_with("AC-29", &[IN, EXCHANGE, RAMP, OUT]);
    let (order, told) = quoted(
        &mut core,
        across("req-29", &session, OUT, "sim-join-out-bank"),
    );
    let record = record_of(&core, &told);
    let route = &record.chosen().expect("a route").parts[0].operations;

    // The route withdraws from a reserve, converts inside the node, ramps from the private node onto the public one
    // and deposits into a reserve: each its own operation.
    let operations: Vec<_> = route.iter().collect();
    assert_eq!(
        operations.iter().map(|o| o.kind).collect::<Vec<_>>(),
        [
            OperationKind::Withdrawal,
            OperationKind::Exchange,
            OperationKind::Route,
            OperationKind::Deposit
        ]
    );
    assert_eq!(
        operations
            .iter()
            .map(|o| (node_name(&o.from.node), node_name(&o.to.node)))
            .collect::<Vec<_>>(),
        [
            ("sim-join-in-bank".to_owned(), NODE.to_owned()),
            (NODE.to_owned(), NODE.to_owned()),
            (NODE.to_owned(), CHAIN.to_owned()),
            (CHAIN.to_owned(), "sim-join-out-bank".to_owned()),
        ]
    );

    // The record shows the publisher behind each provider: the origin of the figures each operation was priced from.
    // One publisher stands behind the reserve, the exchange and the ramp; the reserve at the far end is another's.
    let publisher_of = |provider: &str| -> String {
        record
            .snapshot
            .figures
            .iter()
            .filter(|f| f.origin.provider().is_some_and(|p| p.as_str() == provider))
            .find_map(|f| f.origin.publisher().map(|p| p.as_str().to_owned()))
            .unwrap_or_else(|| panic!("a figure of {provider} names its publisher"))
    };
    for operation in &operations {
        let named: Vec<_> = operation
            .figures
            .iter()
            .filter_map(|id| record.snapshot.figures.iter().find(|f| &f.figure_id == id))
            .filter_map(|f| f.origin.publisher())
            .collect();
        assert!(
            !named.is_empty()
                && named
                    .iter()
                    .all(|p| p.as_str() == publisher_of(operation.provider.as_str())),
            "the figures it cites are its provider's publisher's: {operation:?}"
        );
    }
    assert_eq!(
        [IN, EXCHANGE, RAMP, OUT].map(publisher_of),
        [PUBLISHER, PUBLISHER, PUBLISHER, "sim-join-out-publisher"]
    );

    // And the publisher behind each node: the private node is the publisher's, the public node nobody's.
    let publishers = core.catalogue().publishers();
    assert_eq!(publishers.publisher_of_node(NODE), Some(PUBLISHER));
    for provider in [IN, EXCHANGE, RAMP] {
        assert_eq!(publishers.publisher_of_provider(provider), Some(PUBLISHER));
    }
    assert_eq!(publishers.publisher_of_node(CHAIN), None);
    assert!(core.catalogue().nodes().is_public(CHAIN));
    assert!(!core.catalogue().nodes().is_public(NODE));

    // Run: each conversion is its own segment, with its own transaction; the exchange's changes the asset and no value.
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted")
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    let ran = core.run(&order).expect("the order ran");
    let segments = accounted(&ran);
    assert_eq!(segments.len(), 4);
    let transactions: std::collections::BTreeSet<String> = segments
        .iter()
        .map(|s| transaction(s).as_str().to_owned())
        .collect();
    assert_eq!(transactions.len(), 4, "a transaction for each segment");
    let exchange = operations[1];
    assert_ne!(exchange.from.asset.id, exchange.to.asset.id);
    assert_eq!(
        String::from(exchange.amount_in.clone()),
        exchange.amount_out.as_str()
    );
    assert_eq!(
        gave(&segments[1]),
        AMOUNT,
        "the conversion changes no value"
    );
    assert_eq!(gave(&segments[0]), AMOUNT);
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // The log carries the publisher behind the segments inside its node: the exchange's evidence is that publisher's
    // word, naming it, and its provider's statement.
    let inside = evidence_of(&core, &order)
        .into_iter()
        .find(|e| e.segment == segments[1].segment && e.publisher.is_some())
        .expect("the segment inside the node is labelled with its publisher");
    assert_eq!(inside.publisher.as_ref().map(Id::as_str), Some(PUBLISHER));
    assert_eq!(inside.whose.as_str(), EXCHANGE);
    assert_eq!(inside.grade, EvidenceGrade::Statement);
}

/// The order to the reserve `at`, whose bank rail is `rail`, run: its core, the order and its four segments.
fn into_rail(at: &str, rail: &str) -> (Core, Id, Vec<Accounted>) {
    run_across(&[IN, EXCHANGE, RAMP, at], at, rail, |_| {})
}

// verifies: AC-34
#[test]
fn deposit_at_a_private_node_closes_on_the_movement_in_and_the_reserves_statement() {
    let (mut core, order, segments) = into_rail(OUT_PRIVATE, RAIL);
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // The rail has no public record of its own, and its publisher declares it a private node.
    assert!(
        core.world().network(RAIL).is_none(),
        "no public record of the rail"
    );
    assert_eq!(
        core.catalogue().publishers().publisher_of_node(RAIL),
        Some("sim-join-out-private-publisher")
    );

    // The deposit closes on the public movement into the node, naming it, which is the network's, and on the reserve's
    // statement, which is the reserve's word and no network's.
    let deposit = segments[3].segment.clone();
    let closing: Vec<_> = evidence_of(&core, &order)
        .into_iter()
        .filter(|e| e.segment == deposit)
        .collect();
    assert_eq!(closing.len(), 2, "{closing:?}");
    assert_eq!(
        (closing[0].whose.as_str(), closing[0].grade),
        (CHAIN, EvidenceGrade::PublicRecord),
        "the movement into the rail is the public node's"
    );
    let movement = closing[0].movement.as_ref().expect("the movement is named");
    assert_eq!(movement.network.as_str(), CHAIN);
    assert_eq!(
        movement.node.as_ref().map(Id::as_str),
        Some(RAIL),
        "naming the node it went into"
    );
    assert_eq!(movement.id.as_str(), transaction(&segments[3]).as_str());
    assert_eq!(
        closing[1].whose.as_str(),
        OUT_PRIVATE,
        "the step out is the reserve's word"
    );
    assert_ne!(closing[1].grade, EvidenceGrade::PublicRecord);
    assert!(closing[1].movement.is_none());
    assert!(
        closing[1].publisher.is_none(),
        "no publisher is bound to the step in from a public node"
    );

    // A reserve whose rail has no public record and that no publisher declares private can't end a route: it isn't
    // routed to, and nothing moves.
    let (mut silent, session) = core_with("AC-34", &[IN, EXCHANGE, RAMP, OUT_SILENT]);
    assert_eq!(
        silent
            .catalogue()
            .publishers()
            .publisher_of_node("sim-join-out-silent-bank"),
        None
    );
    assert!(silent.world().network("sim-join-out-silent-bank").is_none());
    let order = accepted(
        &mut silent,
        across(
            "req-silent",
            &session,
            OUT_SILENT,
            "sim-join-out-silent-bank",
        ),
    );
    let ran = silent.run(&order).expect("the order is taken up");
    let [Ran::Refused { refused, .. }] = ran.as_slice() else {
        panic!("the route is refused before anything is handed over: {ran:?}");
    };
    assert_eq!(refused.reason, RouteRefusal::NoPublicRecord(3));
    assert!(silent.world().journal(IN).is_empty(), "nothing moved");
    assert_ne!(state(&mut silent, &order), OrderState::Complete);
}

// verifies: AC-35
#[test]
fn mismatch_out_of_a_private_node_is_recorded_and_a_match_is_consistency_only() {
    // A match: the replay of the publisher's records against the movement out finds them consistent with it, which
    // is all it says; the segment inside the node stays its publisher's word.
    let (core, order, segments) =
        run_across(&[IN, EXCHANGE, RAMP, OUT], OUT, "sim-join-out-bank", |_| {});
    let found = replays(&core, &order);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0].0, segments[2].segment,
        "at the segment that leaves the node"
    );
    assert_eq!(found[0].1, [NODE]);
    assert_eq!(found[0].2, ReplayFound::ConsistentWithTheExit {});
    let inside: Vec<_> = evidence_of(&core, &order)
        .into_iter()
        .filter(|e| e.segment == segments[1].segment)
        .collect();
    assert!(
        inside
            .iter()
            .any(|e| e.grade == EvidenceGrade::Statement && e.publisher.is_some()),
        "the conversion inside the node is still its publisher's word: {inside:?}"
    );
    assert_eq!(
        out_of_the_node(&core, &order, &segments[1].segment),
        1,
        "and the public movement out is recorded against it, as the network's"
    );

    // A mismatch: its publisher records one less than the ramp gave out. The replay records it against the node, and
    // nothing stops: the order still completes (replay detects, it doesn't prove, `THR-131`).
    let (mut core, order, segments) = run_across(
        &[IN, EXCHANGE, RAMP, OUT],
        OUT,
        "sim-join-out-bank",
        |core| {
            core.world().script_for(
                RAMP,
                "transfer",
                Behaviour::misrecording(&(1_000_000 - 1).to_string()),
            );
        },
    );
    let found = replays(&core, &order);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, segments[2].segment);
    let ReplayFound::Mismatch { mismatches } = &found[0].2 else {
        panic!("the mismatch is recorded: {:?}", found[0].2);
    };
    assert!(!mismatches.is_empty());
    assert!(
        mismatches.iter().all(|m| m.node.as_str() == NODE),
        "against the node: {mismatches:?}"
    );
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    // A node the replay caught disagreeing gets no closing on the public movement out; one it found consistent did.
    assert_eq!(
        out_of_the_node(&core, &order, &segments[1].segment),
        0,
        "no public record backs what the replay caught"
    );
}

/// How many pieces of evidence for `segment` are the public node's record of the movement out of the joined node.
fn out_of_the_node(core: &Core, order: &Id, segment: &Id) -> usize {
    evidence_of(core, order)
        .into_iter()
        .filter(|e| &e.segment == segment && e.grade == EvidenceGrade::PublicRecord)
        .filter(|e| {
            e.movement.as_ref().is_some_and(|m| {
                m.network.as_str() == CHAIN && m.node.as_ref().is_some_and(|n| n.as_str() == NODE)
            })
        })
        .count()
}

// verifies: AC-35
#[test]
fn route_ending_inside_a_private_node_records_no_replay_check() {
    let (mut core, order, segments) = into_rail(OUT_PRIVATE, RAIL);
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // The one replay is of the node the value left for the public node: none names the rail the route ends in.
    let found = replays(&core, &order);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, segments[2].segment);
    assert_eq!(found[0].1, [NODE]);
    assert!(
        found
            .iter()
            .all(|(_, nodes, _)| !nodes.iter().any(|n| n == RAIL)),
        "no replay of the node the route ends inside: {found:?}"
    );

    // What there is of it is its publisher's word: the reserve's record of the deposit, labelled so, which is replayed
    // against no exit.
    let records = core.publishers_records(RAIL);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].maker, OUT_PRIVATE);
    assert_eq!(records[0].statement.node, RAIL);
    assert_eq!(
        records[0].statement.standing,
        rutent_contracts::provider_role::Standing::PublishersWord
    );
    let deposit = transaction(&segments[3]);
    let held = core
        .books_for(RAIL, std::slice::from_ref(&deposit))
        .expect("the record holds the deposit");
    assert_eq!(held, [recorded("USD", AMOUNT, "USD", AMOUNT, "0")]);
    let net = core
        .world()
        .network(CHAIN)
        .expect("the public node")
        .clone();
    assert_eq!(
        replay_route(
            &[NodeRecord {
                node: RAIL.to_owned(),
                segments: held
            }],
            &RouteEnd::InsideNode,
            &net
        ),
        Replay::PublishersWord
    );
}

const WALLET_NODE: &str = "sim-wallet-node";

/// The order whose value rests at the wallet's node: `AMOUNT`-sized holds for three orders.
fn three_orders_hold(core: &mut Core) {
    for (order, amount) in [("ord-1", "1000"), ("ord-2", "2500"), ("ord-3", "500")] {
        core.land(WALLET_NODE, "USDC", order, amount)
            .expect("the wallet takes the order's value");
    }
}

/// The checks and raises the log holds, in order.
fn checks_and_raises(core: &Core) -> Vec<(String, Option<Difference>)> {
    entries(core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::HoldingCheck { account, .. } => Some((account.as_str().to_owned(), None)),
            What::Raise { raise } => Some((String::new(), Some(raise.difference))),
            _ => None,
        })
        .collect()
}

/// The wallet is asked directly, as an operator's statement the host's attribution doesn't know of.
fn ask_directly(core: &mut Core, wallet: &str, kind: &str, order: &str, amount: &str) {
    let statement = if kind == "withdrawal" {
        json!({
            "kind": "withdrawal", "transaction": format!("direct-{kind}-{order}"), "provider": wallet,
            "origin": { "network": wallet }, "destination": WALLET_NODE, "asset": "USDC",
            "amount": amount, "dry_run": false, "inputs": { "order": order },
        })
    } else {
        json!({
            "kind": "deposit", "transaction": format!("direct-{kind}-{order}"), "provider": wallet,
            "origin": WALLET_NODE, "destination": { "network": wallet }, "asset": "USDC",
            "amount": amount, "dry_run": false, "inputs": { "order": order },
        })
    };
    let operation: Operation = serde_json::from_value(statement).expect("an operation");
    core.ask_wallet(wallet, operation)
        .expect("the wallet answers the operator");
}

// verifies: AC-39
#[test]
fn shared_holding_account_raises_on_a_difference_and_only_then() {
    let (mut core, _) = core_with("AC-39", &[IN]);
    let wallet = core
        .install_wallet(WALLET_NODE, "USDC")
        .expect("the wallet is installed");
    three_orders_hold(&mut core);

    // The balance matches the sum its three orders hold: the check is recorded and nothing is raised.
    let reading = core
        .holding_reading(WALLET_NODE, 2)
        .expect("the account is read");
    assert_eq!(reading.balance.as_str(), "4000");
    assert_eq!(reading.held_for.len(), 3);
    assert_eq!(reading.word, Word::Checked);
    core.check_holding(&reading).expect("the check is recorded");
    assert!(core.raised().is_empty(), "a match raises nothing");
    assert_eq!(
        checks_and_raises(&core),
        [(format!("holding-{WALLET_NODE}"), None)],
        "the log holds the check, and not the balance"
    );

    // Short: the wallet gave 500 out that the host attributes to an order. The operator is raised, by how much and
    // whose word; the attribution and the log's earlier entries are as they were.
    ask_directly(&mut core, &wallet.provider, "withdrawal", "ord-2", "500");
    let attributed = core.holdings().attributions();
    let before = entries(&core);
    let short = core
        .holding_reading(WALLET_NODE, 2)
        .expect("the account is read");
    assert_eq!(short.balance.as_str(), "3500");
    core.check_holding(&short).expect("the check is recorded");
    let raised = core.raised();
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert!(
        matches!(
            &raised[0].difference,
            Difference::Holding { direction: Direction::Short, by, word: Word::Checked, .. } if by.amount().as_str() == "500"
        ),
        "{:?}",
        raised[0].difference
    );
    assert_eq!(
        core.holdings().attributions(),
        attributed,
        "nothing else changes"
    );
    assert_eq!(&entries(&core)[..before.len()], &before[..]);
    // The log's record of the check is one per account and reading's content: the same check recorded again is the same
    // entry, so what the short added is the raise.
    let added: Vec<_> = checks_and_raises(&core).split_off(1);
    assert_eq!(added.len(), 1, "the raise: {added:?}");
    assert!(matches!(added[0], (_, Some(Difference::Holding { .. }))));

    // Over: the wallet took 700 more than the host attributes.
    ask_directly(&mut core, &wallet.provider, "deposit", "ord-1", "1200");
    let over = core
        .holding_reading(WALLET_NODE, 2)
        .expect("the account is read");
    assert_eq!(over.balance.as_str(), "4700");
    core.check_holding(&over).expect("the check is recorded");
    let raised = core.raised();
    assert_eq!(raised.len(), 2, "{raised:?}");
    assert!(
        matches!(
            &raised[1].difference,
            Difference::Holding { direction: Direction::Over, by, .. } if by.amount().as_str() == "700"
        ),
        "{:?}",
        raised[1].difference
    );

    // At a private node the balance is its publisher's word, labelled: a match raises nothing, a short raises with
    // the label. (A private node's account is its publisher's module's, which no simulated provider stands in for, so
    // its reading is stated here, as its publisher would.)
    let private = |balance: &str, label: &str| {
        let mut reading = serde_json::to_value(&reading).expect("a reading");
        reading["node"] = json!(NODE);
        reading["account"] = json!("holding");
        reading["balance"] = json!(balance);
        reading["word"] = json!(label);
        serde_json::from_value(reading).expect("a reading")
    };
    core.check_holding(&private("4000", "publisher"))
        .expect("recorded");
    assert_eq!(
        core.raised().len(),
        2,
        "a match at a private node raises nothing either"
    );
    core.check_holding(&private("3990", "publisher"))
        .expect("recorded");
    let raised = core.raised();
    assert_eq!(raised.len(), 3);
    assert!(
        matches!(
            &raised[2].difference,
            Difference::Holding {
                direction: Direction::Short,
                word: Word::Publisher,
                ..
            }
        ),
        "{:?}",
        raised[2].difference
    );
    let words: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::HoldingCheck { word, .. } => Some(word),
            _ => None,
        })
        .collect();
    assert_eq!(
        words,
        [Word::Checked, Word::Publisher],
        "each check's record says whose word the balance was"
    );
}

/// The order across the two private nodes, run: its core, the order and its four segments.
fn across_two_run(script: impl FnOnce(&mut Core)) -> (Core, Id, Vec<Accounted>) {
    let (mut core, session) = core_with("AC-35", &[PAIR_IN, PAIR_RAMP_A, PAIR_RAMP_B, PAIR_OUT]);
    script(&mut core);
    let (order, ran) = run(&mut core, across_two("req-pair", &session));
    let segments = accounted(&ran).to_vec();
    assert_eq!(segments.len(), 4, "{segments:?}");
    assert!(
        segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{segments:?}"
    );
    (core, order, segments)
}

// verifies: DEC-24, REQ-ORD-41
#[test]
fn route_across_two_private_nodes_closes_and_replays_each() {
    let (mut core, order, segments) = across_two_run(|_| {});

    // Two publishers' private nodes with a public node between them; each is the node of its own publisher.
    let publishers = core.catalogue().publishers();
    assert_eq!(
        publishers.publisher_of_node(NODE_A),
        Some("sim-pair-a-publisher")
    );
    assert_eq!(
        publishers.publisher_of_node(NODE_B),
        Some("sim-pair-b-publisher")
    );
    assert!(core.catalogue().nodes().is_public(BETWEEN));
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // Each node was replayed as the value left it, once, against its own movement out: the first onto the public node
    // between them, the second out through the final deposit onto the destination's rail.
    let found = replays(&core, &order);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(
        (found[0].0.clone(), found[0].1.clone(), found[0].2.clone()),
        (
            segments[1].segment.clone(),
            vec![NODE_A.to_owned()],
            ReplayFound::ConsistentWithTheExit {}
        )
    );
    assert_eq!(
        (found[1].0.clone(), found[1].1.clone(), found[1].2.clone()),
        (
            segments[3].segment.clone(),
            vec![NODE_B.to_owned()],
            ReplayFound::ConsistentWithTheExit {}
        )
    );
    let movements: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Replay {
                order: o, movement, ..
            } if o == order => movement,
            _ => None,
        })
        .collect();
    assert_eq!(
        movements
            .iter()
            .map(|m| (m.network.as_str(), m.id.as_str()))
            .collect::<Vec<_>>(),
        [
            (BETWEEN, transaction(&segments[1]).as_str()),
            ("sim-pair-out-bank", transaction(&segments[3]).as_str()),
        ]
    );

    // What each replay read is its own publisher's record of the segments inside its node.
    let a = core.publishers_records(NODE_A);
    let b = core.publishers_records(NODE_B);
    assert_eq!(
        a.iter().map(|r| r.maker.as_str()).collect::<Vec<_>>(),
        [PAIR_IN, PAIR_RAMP_A]
    );
    assert_eq!(
        b.iter().map(|r| r.maker.as_str()).collect::<Vec<_>>(),
        [PAIR_RAMP_B, PAIR_OUT]
    );
    assert!(
        a.iter()
            .all(|r| r.statement.publisher == "sim-pair-a-publisher")
    );
    assert!(
        b.iter()
            .all(|r| r.statement.publisher == "sim-pair-b-publisher")
    );
    let txs: Vec<Id> = segments.iter().map(transaction).collect();
    assert_eq!(
        core.books_for(NODE_A, &txs[..2]),
        Some(vec![
            recorded("COP", AMOUNT, "COP", AMOUNT, "0"),
            recorded("COP", AMOUNT, "COP", AMOUNT, "0"),
        ])
    );
    assert_eq!(
        core.books_for(NODE_B, &txs[2..]),
        Some(vec![
            recorded("COP", AMOUNT, "COP", AMOUNT, "0"),
            recorded("COP", AMOUNT, "COP", AMOUNT, "0"),
        ])
    );
    assert_eq!(
        core.books_for(NODE_A, &txs[2..]),
        None,
        "the other node's segments are in no book of this one"
    );

    // The order closed on the final deposit's public record and the reserve's statement.
    let closing: Vec<_> = evidence_of(&core, &order)
        .into_iter()
        .filter(|e| e.segment == segments[3].segment)
        .collect();
    assert!(
        closing
            .iter()
            .any(|e| e.grade == EvidenceGrade::PublicRecord)
    );
    assert!(closing.iter().any(|e| e.whose.as_str() == PAIR_OUT));
}

// verifies: DEC-24, REQ-ORD-41
#[test]
fn mismatch_at_the_second_of_two_private_nodes_is_recorded_against_it() {
    // The second node's publisher records the final deposit as giving out one less than it did: its record doesn't add
    // up against the exit. The first node, replayed as the value left it, agrees.
    let (mut core, order, segments) = across_two_run(|core| {
        core.world().script_for(
            PAIR_OUT,
            "deposit",
            Behaviour::misrecording(&(1_000_000 - 1).to_string()),
        );
    });
    let found = replays(&core, &order);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].0, segments[1].segment);
    assert_eq!(found[0].1, [NODE_A]);
    assert_eq!(
        found[0].2,
        ReplayFound::ConsistentWithTheExit {},
        "the first node agrees"
    );
    assert_eq!(found[1].0, segments[3].segment);
    assert_eq!(found[1].1, [NODE_B]);
    let ReplayFound::Mismatch { mismatches } = &found[1].2 else {
        panic!("the second node's mismatch is recorded: {:?}", found[1].2);
    };
    assert!(!mismatches.is_empty());
    assert!(
        mismatches.iter().all(|m| m.node.as_str() == NODE_B),
        "recorded against the second node, and no other: {mismatches:?}"
    );
    // Recorded and nothing more: the order still completes.
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // A mismatch at the first node is recorded against the first.
    let (core, order, segments) = across_two_run(|core| {
        core.world().script_for(
            PAIR_RAMP_A,
            "transfer",
            Behaviour::misrecording(&(1_000_000 - 1).to_string()),
        );
    });
    let found = replays(&core, &order);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].0, segments[1].segment);
    assert!(
        matches!(&found[0].2, ReplayFound::Mismatch { mismatches } if mismatches.iter().all(|m| m.node.as_str() == NODE_A))
    );
    assert_eq!(
        found[1].2,
        ReplayFound::ConsistentWithTheExit {},
        "the second node agrees"
    );
}

// verifies: REQ-CAT-22
#[test]
fn node_neither_public_nor_declared_private_is_on_no_scenarios_route() {
    // Every inner node on a route the scenarios run is on one list or the other.
    let listed = |core: &Core, node: &str| {
        core.catalogue().nodes().is_public(node)
            || core
                .catalogue()
                .publishers()
                .publisher_of_node(node)
                .is_some()
    };
    let (mut core, session) = core_with("AC-29", &[IN, EXCHANGE, RAMP, OUT]);
    let (_, told) = quoted(
        &mut core,
        across("req-listed", &session, OUT, "sim-join-out-bank"),
    );
    let (mut pair, pair_session) =
        core_with("AC-35", &[PAIR_IN, PAIR_RAMP_A, PAIR_RAMP_B, PAIR_OUT]);
    let (_, pair_told) = quoted(&mut pair, across_two("req-listed-two", &pair_session));
    for (core, told) in [(&core, &told), (&pair, &pair_told)] {
        let record = record_of(core, told);
        let nodes: Vec<_> = record.chosen().expect("a route").parts[0]
            .operations
            .iter()
            .flat_map(|o| [&o.from.node, &o.to.node])
            .filter_map(|n| match n {
                NodeRef::Inner(inner) => Some(inner.clone()),
                NodeRef::Outer(_) => None,
            })
            .collect();
        assert!(nodes.len() >= 4, "{nodes:?}");
        for node in nodes {
            assert!(listed(core, &node), "{node} is on neither list");
        }
    }

    // A reserve whose node is on neither is left out of the catalogue's snapshot, each tuple naming the node, and no
    // route runs through it.
    let (mut ghost, session) = core_with("AC-29", &[IN, EXCHANGE, RAMP, OUT, "sim-ghost"]);
    assert!(!listed(&ghost, "sim-ghost-ledger"));
    let build = ghost
        .catalogue()
        .snapshot("snap-ghost", ghost.clock.instant())
        .expect("a snapshot");
    let left: Vec<_> = build
        .left_out
        .iter()
        .filter(|l| l.provider == "sim-ghost")
        .collect();
    assert_eq!(left.len(), 2, "its two tuples: {left:?}");
    assert!(
        left.iter()
            .all(|l| l.reason == LeftOutReason::NodeNotListed("sim-ghost-ledger".to_owned()))
    );
    assert!(
        build.left_out.iter().all(|l| l.provider == "sim-ghost"),
        "no other provider is left out"
    );
    for figure in &build.snapshot.figures {
        assert!(
            figure
                .origin
                .provider()
                .is_none_or(|p| p.as_str() != "sim-ghost"),
            "none of its figures is a segment: {figure:?}"
        );
    }
    let ghost_order = request(
        "req-ghost",
        &session,
        AMOUNT,
        "sim-ghost",
        outer("sim-ghost-bank", "COP"),
        outer("sim-ghost-bank", "COP"),
        "sim-ghost",
        OTHER,
        "lowest_total_cost",
    );
    let told = ghost
        .request(ghost_order)
        .expect("the request opens an order");
    let order = Core::order_of(&told).expect("an order");
    let told = ghost.quote(&kit::customer(), &order).expect("quoted");
    assert!(
        matches!(
            told,
            Told::Exists {
                status: Status::CantProceed { .. },
                ..
            }
        ),
        "no route: {told:?}"
    );
    let _ = (RAMP, EXCHANGE, OUT_PRIVATE);
}
