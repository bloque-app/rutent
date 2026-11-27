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
use rutent_core_orch::schedule::Job;
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
    // Closing reads no record of a node the route ends inside: Core reads it, decided, for the deposit's own segment.
    let deposit_tx = transaction(&segments[3]);
    let records = core.read_records_about(
        &order,
        &segments[3].segment,
        RAIL,
        std::slice::from_ref(&deposit_tx),
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].maker, OUT_PRIVATE);
    assert_eq!(records[0].statement.node, RAIL);
    assert_eq!(
        records[0].statement.standing,
        rutent_contracts::provider_role::Standing::PublishersWord
    );
    let held: Vec<_> = records[0]
        .statement
        .segments
        .iter()
        .map(|s| {
            (
                s.transaction.as_str(),
                s.asset_in.as_str(),
                s.amount_in.as_str(),
                s.asset_out.as_str(),
                s.amount_out.as_str(),
                s.fee.as_str(),
            )
        })
        .collect();
    assert_eq!(
        held,
        [(deposit_tx.as_str(), "USD", AMOUNT, "USD", AMOUNT, "0")]
    );
    let net = core
        .world()
        .network(CHAIN)
        .expect("the public node")
        .clone();
    assert_eq!(
        replay_route(
            &[NodeRecord {
                node: RAIL.to_owned(),
                segments: vec![recorded("USD", AMOUNT, "USD", AMOUNT, "0")]
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

/// One tick of Core's schedule, its holding check run: the account of `node` as the round read it and checked it
/// (`REQ-AUD-15`, `T-020-48`). The reading is taken through the platform; no test states one.
fn checked(core: &mut Core, node: &str) -> rutent_core_orch::schedule::HoldingChecked {
    core.set_cadence(Job::HoldingCheck, 0);
    core.tick()
        .holding
        .into_iter()
        .find(|h| h.node == node)
        .expect("the round read the account")
}

// verifies: AC-39
#[test]
fn shared_holding_account_raises_on_a_difference_and_only_then() {
    let (mut core, _) = core_with("AC-39", &[IN]);
    let wallet = core
        .install_wallet(WALLET_NODE, "USDC")
        .expect("the wallet is installed");
    three_orders_hold(&mut core);
    let raised_by = |core: &Core, direction: Direction| -> Vec<String> {
        core.raised()
            .into_iter()
            .filter_map(|r| match r.difference {
                Difference::Holding {
                    direction: d, by, ..
                } if d == direction => Some(by.amount().as_str().to_owned()),
                _ => None,
            })
            .collect()
    };

    // The three orders were landed by the host alone, not by a segment of Core's: what Core's own segments rest at the node
    // is nothing, so the balance the wallet gives is more than Core attributes (`T-020-63`), by all of it, and that is the
    // one raise. The host's record, the second comparison, names the three orders and matches the balance.
    let round = checked(&mut core, WALLET_NODE);
    let reading = round.reading.expect("the account is read");
    assert_eq!(reading.balance.as_str(), "4000");
    assert!(
        reading.held_for.is_empty(),
        "Core's segments rest nothing there"
    );
    assert_eq!(reading.word, Word::Checked);
    round.check.expect("the check is recorded");
    let hosts = round
        .against_wallet
        .expect("the host's record differs from Core's");
    assert_eq!(hosts.reading.balance.as_str(), "4000");
    assert_eq!(hosts.reading.held_for.len(), 3);
    hosts.check.expect("the host's check is recorded too");
    assert_eq!(
        raised_by(&core, Direction::Over),
        ["4000"],
        "{:?}",
        core.raised()
    );
    assert!(raised_by(&core, Direction::Short).is_empty());
    assert_eq!(
        checks_and_raises(&core)
            .iter()
            .filter(|(account, _)| !account.is_empty())
            .count(),
        2,
        "the log holds the two checks, and not the balance"
    );

    // Short: the wallet gave 500 out that the host attributes to an order. The host's record says so: the operator is
    // raised, by how much and whose word; the attribution and the log's earlier entries are as they were.
    ask_directly(&mut core, &wallet.provider, "withdrawal", "ord-2", "500");
    let attributed = core.holdings().attributions();
    let before = entries(&core);
    let round = checked(&mut core, WALLET_NODE);
    assert_eq!(
        round.reading.expect("the account is read").balance.as_str(),
        "3500"
    );
    round.check.expect("the check is recorded");
    assert_eq!(
        raised_by(&core, Direction::Short),
        ["500"],
        "short of what the host attributes, whose word it is"
    );
    assert_eq!(raised_by(&core, Direction::Over), ["4000", "3500"]);
    assert_eq!(
        core.holdings().attributions(),
        attributed,
        "nothing else changes"
    );
    assert_eq!(&entries(&core)[..before.len()], &before[..]);

    // Over: the wallet took 700 more than the host attributes.
    ask_directly(&mut core, &wallet.provider, "deposit", "ord-1", "1200");
    let round = checked(&mut core, WALLET_NODE);
    assert_eq!(
        round.reading.expect("the account is read").balance.as_str(),
        "4700"
    );
    round.check.expect("the check is recorded");
    assert_eq!(
        raised_by(&core, Direction::Over),
        ["4000", "3500", "4700", "700"],
        "Core's, then the host's: 700 more than the host attributes"
    );
}

/// Core's schedule checks each holding account at its cadence, and only then: the reading is the account's module's
/// own answer, asked through the platform once each check for each account, never a reading handed in, and a
/// difference raises the operator (`REQ-AUD-15`, `AC-39`, `T-020-48`; the review's L3: each account is asked once a
/// check, not once for each account there is).
// verifies: AC-39, REQ-AUD-15
#[test]
fn holding_account_check_runs_on_cores_schedule_and_a_difference_raises_the_operator() {
    let (mut core, _) = core_with("AC-39", &[IN]);
    let wallet = core
        .install_wallet(WALLET_NODE, "USDC")
        .expect("the wallet is installed");
    let publishers = core
        .install_publisher_account(NODE, PUBLISHER, "USDC")
        .expect("the publisher's account is installed");
    three_orders_hold(&mut core);
    let asked = |core: &Core| {
        (
            core.operations_received(&wallet.provider),
            core.operations_received(&publishers.provider),
        )
    };

    // A Core's first tick runs every job: each account is read, once, and checked.
    let before = asked(&core);
    let round = core.tick();
    assert!(round.did(Job::HoldingCheck));
    assert_eq!(round.holding.len(), 2, "both accounts: {:?}", round.holding);
    assert_eq!(
        asked(&core),
        (before.0 + 1, before.1 + 1),
        "each account's module is asked once"
    );
    let public = round.holding_of(WALLET_NODE).expect("read");
    assert_eq!(
        public.reading.as_ref().expect("read").balance.as_str(),
        "4000"
    );
    public.check.as_ref().expect("checked");
    // The three orders were landed by the host alone, so what Core's segments rest at the node is nothing: the one raise
    // is that the wallet holds more than Core attributes (`T-020-63`). The private node's account, empty, matches.
    assert_eq!(core.raised().len(), 1, "{:?}", core.raised());

    // The check runs hourly by default: a tick before then reads no account and asks no module.
    core.clock.advance(3599);
    let round = core.tick();
    assert!(!round.did(Job::HoldingCheck));
    assert!(round.holding.is_empty());
    ask_directly(&mut core, &wallet.provider, "withdrawal", "ord-2", "500");
    let between = asked(&core);
    assert_eq!(core.raised().len(), 1, "nothing is checked between");

    // Due: the account is asked again, its own figure is what the check reads, and the short is raised.
    core.clock.advance(1);
    let round = core.tick();
    assert!(round.did(Job::HoldingCheck));
    assert_eq!(asked(&core), (between.0 + 1, between.1 + 1));
    assert_eq!(
        round
            .holding_of(WALLET_NODE)
            .and_then(|h| h.reading.as_ref().ok())
            .map(|r| r.balance.as_str().to_owned()),
        Some("3500".to_owned()),
        "the module's figure, not one the schedule was handed"
    );
    // Core's comparison says the wallet holds 3500 where Core attributes nothing; the host's record, the second, says it
    // holds 500 less than the host attributes.
    let raised = core.raised();
    assert_eq!(raised.len(), 3, "{raised:?}");
    assert!(matches!(
        &raised[1].difference,
        Difference::Holding { direction: Direction::Over, by, .. } if by.amount().as_str() == "3500"
    ));
    assert!(matches!(
        &raised[2].difference,
        Difference::Holding { direction: Direction::Short, by, .. } if by.amount().as_str() == "500"
    ));
    // The operator's cadence is the job's: set to a day, an hour's ticks check nothing.
    core.set_cadence(Job::HoldingCheck, 86_400);
    core.clock.advance(3600);
    assert!(!core.tick().did(Job::HoldingCheck));
    assert_eq!(core.raised().len(), 3);
}

/// A holding account whose module doesn't give its balance is raised to the operator, never left silent: once in a
/// spell of failure, the raise carrying no balance and citing the account's last check where it has one, and again
/// in a later spell once a check came between (`REQ-AUD-15`, `REQ-ORD-7`; the review of `T-020-48`'s first build, its
/// S2, `core-internal`'s `holding_not_read`). Nothing else moves.
// verifies: REQ-AUD-15
#[test]
fn holding_reading_a_module_does_not_give_is_raised_to_the_operator() {
    let (mut core, _) = core_with("AC-39", &[IN]);
    let wallet = core
        .install_publisher_account(NODE, PUBLISHER, "USDC")
        .expect("the publisher's account is installed");
    core.set_cadence(Job::HoldingCheck, 0);
    let unread = |core: &Core| -> Vec<(Difference, usize)> {
        core.raised()
            .into_iter()
            .filter(|r| matches!(r.difference, Difference::HoldingNotRead { .. }))
            .map(|r| (r.difference, r.cites.len()))
            .collect()
    };

    // The module's answer can't be signed and passed on: the platform passes no answer.
    core.world().make_key_store_unavailable(&wallet.provider);
    let round = core.tick();
    let checked = round.holding_of(NODE).expect("the account was asked");
    assert!(checked.reading.is_err(), "{checked:?}");
    assert!(checked.check.is_err(), "no check without a reading");
    assert!(
        matches!(checked.not_read, Some(Ok(_))),
        "raised: {checked:?}"
    );
    let raised = unread(&core);
    assert_eq!(raised.len(), 1, "{raised:?}");
    let account = rutent_contracts::router_api::Id::new(format!("holding-{NODE}")).expect("an id");
    assert!(
        matches!(
            &raised[0],
            (Difference::HoldingNotRead { account: a, node, provider, why: rutent_contracts::core_internal::HoldingNotRead::NotAnswered {} }, 0)
                if a == &account && node.as_str() == NODE && provider.as_str() == wallet.provider.as_str()
        ),
        "an account never read cites nothing: {raised:?}"
    );
    // Delivered to the operator on Core's own channel (`raised` reads the operator's inbox), and recorded in the log.
    let logged = |core: &Core| {
        core.raise_entries()
            .into_iter()
            .filter(|r| matches!(r.difference, Difference::HoldingNotRead { .. }))
            .count()
    };
    assert_eq!(logged(&core), 1);

    // Still failing: the same spell, not raised again, at the next tick or the one after.
    for _ in 0..2 {
        core.clock.advance(60);
        let round = core.tick();
        assert!(matches!(
            round.holding_of(NODE).and_then(|h| h.not_read.clone()),
            Some(Err(_))
        ));
    }
    assert_eq!(unread(&core).len(), 1, "once in a spell");
    assert_eq!(logged(&core), 1);

    // Read again: the check ends the spell, and nothing more is raised.
    core.world().make_key_store_available(&wallet.provider);
    core.clock.advance(60);
    let round = core.tick();
    round
        .holding_of(NODE)
        .and_then(|h| h.check.clone().ok())
        .expect("checked");
    assert_eq!(unread(&core).len(), 1);

    // A later spell is raised again, resting on the account's last check.
    core.world().make_key_store_unavailable(&wallet.provider);
    core.clock.advance(60);
    core.tick();
    let raised = unread(&core);
    assert_eq!(raised.len(), 2, "{raised:?}");
    assert_eq!(raised[1].1, 1, "it cites the account's last check");
    assert_eq!(logged(&core), 2);
    assert!(
        entries(&core)
            .iter()
            .filter(|v| matches!(v.what, What::HoldingCheck { .. }))
            .count()
            >= 1
    );
}

/// A holding account's balance is read as any action policy decides is (`REQ-AUZ-1`, `REQ-AUD-15`, `INV-7`, `T-020-63`):
/// each read is a permitted `read_holding_balance` whose effect is in the log, and its result recorded, before and after
/// the module is asked, and where policy no longer permits it the module is asked nothing and the account is raised as not
/// read, once. Nothing reaches a holding account's module through the platform undecided.
// verifies: REQ-AUZ-1, REQ-AUD-15
#[test]
fn holding_balance_is_read_on_a_decision_and_refused_where_policy_refuses() {
    use rutent_contracts::core_internal::{Action, HoldingNotRead, Outcome};
    let (mut core, _) = core_with("AC-39", &[IN]);
    let wallet = core
        .install_publisher_account(NODE, PUBLISHER, "USDC")
        .expect("the publisher's account is installed");
    core.set_cadence(Job::HoldingCheck, 0);
    let reads = |core: &Core| -> Vec<(String, String, String, Outcome)> {
        let all = entries(core);
        all.iter()
            .enumerate()
            .filter_map(|(at, v)| match &v.what {
                What::Effect {
                    action:
                        Action::ReadHoldingBalance {
                            node,
                            provider,
                            asset,
                            ..
                        },
                    ..
                } => {
                    // The effect's result is the next entry that names it: `done` once the module was asked.
                    let result = all[at + 1..].iter().find_map(|r| match &r.what {
                        What::Result { .. } => Some(r.outcome),
                        _ => None,
                    })?;
                    Some((
                        node.as_str().to_owned(),
                        provider.as_str().to_owned(),
                        asset.as_str().to_owned(),
                        result,
                    ))
                }
                _ => None,
            })
            .collect()
    };

    // Permitted: the read's effect and its result are in the log, the module was asked once, and the reading is the one
    // the module's answer gives.
    let asked = core.operations_received(&wallet.provider);
    let round = core.tick();
    let held = round.holding_of(NODE).expect("the account is read");
    assert!(held.reading.is_ok(), "{held:?}");
    assert_eq!(core.operations_received(&wallet.provider), asked + 1);
    assert_eq!(
        reads(&core),
        [(
            NODE.to_owned(),
            wallet.provider.clone(),
            "USDC".to_owned(),
            Outcome::Done {}
        )]
    );

    // Refused: Core's actor no longer holds the permission the operator's it carries onward. Nothing is asked of the
    // module, no effect is recorded, and the account is raised as not read, for want of a permit.
    core.registers().permissions.retain(|(acts, p)| {
        !(acts == kit::core().acts.as_str()
            && *p == rutent_core_authz::roles::Permission::ReadHoldingBalance)
    });
    core.clock.advance(60);
    let asked = core.operations_received(&wallet.provider);
    let round = core.tick();
    let held = round
        .holding_of(NODE)
        .expect("the account is still in the round");
    assert!(held.reading.is_err() && held.check.is_err(), "{held:?}");
    assert!(matches!(held.not_read, Some(Ok(_))), "{held:?}");
    assert_eq!(
        core.operations_received(&wallet.provider),
        asked,
        "asked nothing"
    );
    assert_eq!(reads(&core).len(), 1, "and recorded no effect");
    let unread: Vec<_> = core
        .raised()
        .into_iter()
        .filter_map(|r| match r.difference {
            Difference::HoldingNotRead { why, .. } => Some(why),
            _ => None,
        })
        .collect();
    assert_eq!(unread, [HoldingNotRead::NotPermitted {}]);
}

/// The private nodes' publisher accounts are the deployment's register's, which goes on across a restart: a reopened
/// Core reads each again on its first tick, its publisher's word as before, so no account is forgotten
/// (`NFR-6`, `REQ-AUD-15`; the review of `T-020-48`'s first build, its S1).
// verifies: NFR-6, REQ-AUD-15
#[test]
fn private_nodes_publisher_accounts_are_rebuilt_after_core_reopens() {
    let (mut core, _) = core_with("AC-39", &[IN]);
    let wallet = core
        .install_publisher_account(NODE, PUBLISHER, "USDC")
        .expect("the publisher's account is installed");
    let before = checked(&mut core, NODE);
    assert!(before.reading.is_ok(), "{before:?}");
    ask_wallet_to_hold(&mut core, &wallet.provider, "ord-x", "3990");

    let mut core = rutent_scenarios::harness::Scenario::new("execution", "AC-39", given(&[IN]))
        .reopen(core, 1, &mut rutent_scenarios::assembly::machine::Reference);
    // A reopened Core's first tick runs every job, the holding check among them.
    let round = core.tick();
    let after = round
        .holding_of(NODE)
        .expect("the private node's account is read after the reopen");
    let reading = after
        .reading
        .as_ref()
        .expect("its publisher's module answers");
    assert_eq!(reading.word, Word::Publisher);
    assert_eq!(reading.balance.as_str(), "3990", "its figure, read again");
    after.check.as_ref().expect("checked");
    let raised = core.raised();
    assert!(
        raised.iter().any(|r| matches!(
            &r.difference,
            Difference::Holding {
                direction: Direction::Over,
                word: Word::Publisher,
                ..
            }
        )),
        "the difference it states is raised: {raised:?}"
    );
}

// verifies: AC-39, REQ-AUD-15
#[test]
fn private_nodes_holding_reading_is_its_publishers_signed_word() {
    let (mut core, _) = core_with("AC-39", &[IN]);
    let wallet = core
        .install_publisher_account(NODE, PUBLISHER, "USDC")
        .expect("the publisher's account is installed");

    // Nothing rests at the node and its publisher states nothing: a match, and nothing is raised.
    let round = checked(&mut core, NODE);
    let reading = round.reading.expect("the publisher's module answers");
    assert_eq!(reading.balance.as_str(), "0");
    assert_eq!(reading.word, Word::Publisher, "labelled as its word");
    assert_eq!(reading.node.as_str(), NODE);
    round.check.expect("the check is recorded");
    assert!(core.raised().is_empty(), "a match raises nothing");

    // The publisher's module comes to state value no order holds there. The reading is its signed answer, taken through
    // the platform, and the raise carries the label: the balance is its publisher's word, which rutent can compare and
    // not check.
    let before = core.operations_received(&wallet.provider);
    ask_wallet_to_hold(&mut core, &wallet.provider, "ord-x", "3990");
    let round = checked(&mut core, NODE);
    assert!(
        core.operations_received(&wallet.provider) > before + 1,
        "the reading is the module's own answer to Core's question"
    );
    assert_eq!(
        round.reading.expect("read").balance.as_str(),
        "3990",
        "its figure, not one the schedule was handed"
    );
    round.check.expect("the check is recorded");
    let raised = core.raised();
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert!(
        matches!(
            &raised[0].difference,
            Difference::Holding {
                direction: Direction::Over,
                word: Word::Publisher,
                by,
                ..
            } if by.amount().as_str() == "3990"
        ),
        "{:?}",
        raised[0].difference
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
        [Word::Publisher],
        "each check's record says whose word the balance was, and holds no balance: the same check is the one entry"
    );
}

/// The publisher's account is asked directly to take `amount` for `order`, as its publisher's own deposit into it.
fn ask_wallet_to_hold(core: &mut Core, wallet: &str, order: &str, amount: &str) {
    let operation: Operation = serde_json::from_value(json!({
        "kind": "deposit", "transaction": format!("direct-deposit-{order}"), "provider": wallet,
        "origin": NODE, "destination": { "network": wallet }, "asset": "USDC",
        "amount": amount, "dry_run": false, "inputs": { "order": order },
    }))
    .expect("an operation");
    core.ask_wallet(wallet, operation)
        .expect("the publisher's account takes it");
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
    let a = published(&mut core, NODE_A);
    let b = published(&mut core, NODE_B);
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

/// How many of the publishers' records of a node's segments the log holds, each a stated entry about the order.
fn node_records(core: &Core, order: &Id) -> usize {
    core.log()
        .entries()
        .iter()
        .filter(|(_, e)| {
            matches!(e, rutent_contracts::core_internal::Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::INNER_SEGMENTS_SHAPE
                    && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order))
        })
        .count()
}

/// A segment's provider answered, and the publisher's record of the private node's segments its grading asks to keep
/// is queued, when the log can't take the record and Core stops. Opened again, Core goes on from the answer the log
/// holds, never asking the provider again: the record is written once, the order completes, the log holds the records
/// a Core that ran on holds, and each provider was asked what it asked them, nothing twice (`NFR-6`, `INV-8`).
// verifies: NFR-6, INV-8
#[test]
fn private_node_segment_record_queued_before_core_reopens_is_written_once() {
    let names = [IN, EXCHANGE, RAMP, OUT];
    let (mut ran_on, ran_order, _) = run_across(&names, OUT, "sim-join-out-bank", |_| {});
    let written = node_records(&ran_on, &ran_order);
    assert!(written > 0, "the replay keeps a publisher's record");
    let ran_on_journals: std::collections::BTreeMap<&str, usize> = names
        .iter()
        .map(|p| (*p, ran_on.world().journal(p).len()))
        .collect();

    let (mut core, session) = core_with("AC-35", &names);
    let order = accepted(
        &mut core,
        across("req-join", &session, OUT, "sim-join-out-bank"),
    );
    core.refuse_log_appends(rutent_contracts::provider_role::INNER_SEGMENTS_SHAPE, 3);
    assert!(
        core.run(&order).is_err(),
        "the run stops on the record the log can't take"
    );
    assert_eq!(
        node_records(&core, &order),
        0,
        "the record is queued, not written"
    );
    let mut core = rutent_scenarios::harness::Scenario::new("execution", "AC-35", given(&names))
        .reopen(core, 1, &mut rutent_scenarios::assembly::machine::Reference);
    core.run(&order).expect("the reopened Core goes on");
    assert_eq!(
        node_records(&core, &order),
        written,
        "written once, as the Core that ran on"
    );
    for provider in names {
        // The operations carried out, as the provider's books hold them: Core's questions of a private node's record
        // move nothing and are asked again at each grading (`T-020-58`), so they are no part of the count.
        assert_eq!(
            core.world().journal(provider).len(),
            ran_on_journals[provider],
            "{provider} is asked what the Core that ran on asked it, nothing twice"
        );
    }
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    core.run(&order).expect("runs again");
    assert_eq!(node_records(&core, &order), written);
}

/// An order into a private rail closes on the public movement into it, which Core holds as taken. Opened again, Core
/// reads the close back from the log: the order still reads complete, and a second order paid by the same movement (a
/// reserve tying one payment to two orders) doesn't close on it (`NFR-6`, `REQ-ORD-38`).
// verifies: NFR-6
#[test]
fn private_rail_closed_before_core_reopens_stays_closed() {
    let names = [IN, EXCHANGE, RAMP, OUT_PRIVATE];
    let (core, order, _) = into_rail(OUT_PRIVATE, RAIL);
    let mut core = rutent_scenarios::harness::Scenario::new("execution", "AC-35", given(&names))
        .reopen(core, 1, &mut rutent_scenarios::assembly::machine::Reference);
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    core.tie_payments_by_destination();
    // The session the first order ran in, opened before the restart and held after it (`T-021-17`): its id, the
    // deployment's one, opens no other session.
    let session = kit::id(&rutent_scenarios::assembly::context().session);
    let second = accepted(&mut core, across("req-join-2", &session, OUT_PRIVATE, RAIL));
    let _ = core.run(&second);
    assert_ne!(
        state(&mut core, &second),
        OrderState::Complete,
        "the movement into the rail closed the first order alone"
    );
}

/// A listing the catalogue refuses is recorded `failed` (`T-020-65`, the reviewer's Low on `list()`): the operator's
/// `public_node` setting listing the publishers' private node is permitted and its effect recorded, but the catalogue
/// won't list a node a provider declared private, so its result is `failed`, the node stays private, and the setting is
/// not the operator's (a reopened Core takes no `failed` setting).
// verifies: REQ-CAT-22
#[test]
fn listing_a_node_the_catalogue_refuses_is_recorded_failed() {
    use rutent_contracts::core_internal::{Entry, OperatorSetting, Outcome, What};
    use rutent_core_orch::settings::SettingRefused;
    let (mut core, _) = core_with("AC-29", &[IN, EXCHANGE, RAMP, OUT]);
    assert!(!core.catalogue().nodes().is_public(NODE));
    assert!(
        core.catalogue()
            .publishers()
            .publisher_of_node(NODE)
            .is_some()
    );
    let listed: OperatorSetting = serde_json::from_value(serde_json::json!({
        "setting": "public_node", "node": { "network": NODE }, "listed": true,
    }))
    .expect("a setting");
    let refused = core.set_setting(&listed);
    assert!(
        matches!(refused, Err(SettingRefused::NotTaken(_))),
        "{refused:?}"
    );
    let last = core.log().entries().last().cloned().expect("an entry");
    assert!(
        matches!(&last.1, Entry::Verified(v) if matches!(v.what, What::Result { .. }) && v.outcome == Outcome::Failed {}),
        "its result is failed: {last:?}"
    );
    assert!(!core.catalogue().nodes().is_public(NODE), "still private");
    assert!(
        core.settings().public_node(NODE).is_none(),
        "no setting of the operator's"
    );
}
