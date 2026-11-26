//! The publishers' signed record of the segments inside their private nodes, read by the assembled Core's books
//! (`T-024-14`, `REQ-ORD-41`, `REQ-ORD-3`, `AC-35`, `DEC-24`): each simulated provider that serves inside a private
//! node states, as its publisher's word, the segments it ran there (`Simulated`'s answer to `inner_segments_query`), and
//! Core asks it through the platform and its `Books` read that statement, held to its maker's advertisement and its signature, where the kit's
//! books used to be rebuilt by hand. Over `T-024-04`'s providers that join one another's node.
//!
//! A route across two private nodes can't be run on the platform: the router joins two segments only where one ends at
//! the endpoint the next starts from, and a reserve enters a private node from an outer node while a ramp, the only
//! segment that leaves one for a public node, ends at an inner one. Where two nodes are replayed together here, they
//! are two reserves' nodes, each run by an order of its own, and the replay is `CMP-EVIDENCE`'s over what the
//! assembly's books read of both.

mod execution_support;

use execution_support::*;
use rutent_conformance::sim::{Behaviour, Spec};
use rutent_contracts::core_internal::{ReplayFound, What};
use rutent_contracts::provider_role::Standing;
use rutent_contracts::router_api::Id;
use rutent_core_orch::intake::OrderRequest;
use rutent_core_orch::segments::{Accounted, Ran, SegmentOutcome};
use rutent_evidence::replay::{
    Mismatch, NodeRecord, PublicExit, RecordedSegment, Replay, RouteEnd, Stated, replay_route,
};
use rutent_evidence::{MovementId, PublicNetwork, Status};
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::AMOUNT;

const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const QUICK: &str = "sim-join-ramp-quick";
const OUT: &str = "sim-join-out";
const NODE: &str = "sim-join-ledger";
const CHAIN: &str = "sim-join-chain";
const RESERVE: &str = "sim-reserve-b";
const OTHER_RESERVE: &str = "sim-reserve-a";

/// What the ramp keeps of the amount it moves.
const RAMP_FEE: u128 = 5_000;

fn across(name: &str, session: &Id) -> OrderRequest {
    request(
        name,
        session,
        AMOUNT,
        IN,
        outer("sim-join-in-bank", "COP"),
        outer("sim-join-out-bank", "USD"),
        OUT,
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

/// The transaction a segment ran under.
fn transaction(segment: &Accounted) -> Id {
    segment
        .transaction
        .clone()
        .expect("a transaction was assigned")
}

/// What a settled segment gave out.
fn gave(segment: &Accounted) -> &str {
    let SegmentOutcome::Settled { amount_out, .. } = &segment.outcome else {
        panic!("the segment settled: {segment:?}");
    };
    amount_out.as_str()
}

fn core_over(providers: &[&str]) -> (Core, Id) {
    core_with("AC-35", providers)
}

/// The order across the joined providers, run: its core, the order, and its four segments.
fn joined(core: &mut Core, session: &Id) -> (Id, Vec<Accounted>) {
    let (order, ran) = run(core, across("req-join", session));
    let segments = accounted(&ran).to_vec();
    assert_eq!(segments.len(), 4, "{segments:?}");
    assert!(
        segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{segments:?}"
    );
    (order, segments)
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

/// What the order's replays found, with the segment each was for.
fn replays(core: &Core, order: &Id) -> Vec<(Id, Vec<Id>, ReplayFound)> {
    entries(core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Replay {
                order: o,
                segment,
                nodes,
                found,
                ..
            } if &o == order => Some((segment, nodes, found)),
            _ => None,
        })
        .collect()
}

// verifies: REQ-ORD-41
#[test]
fn publishers_emitted_record_is_read_by_the_assemblys_books() {
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
    let reserve = within_run(&mut core, &session, "req-reserve", RESERVE);
    let (order, segments) = joined(&mut core, &session);
    let (tx_in, tx_exchange, tx_ramp) = (
        transaction(&segments[0]),
        transaction(&segments[1]),
        transaction(&segments[2]),
    );
    let moved = gave(&segments[2]);
    assert_eq!(moved, (1_000_000 - RAMP_FEE).to_string());

    // Each provider that served in the node states its own record of it, for its publisher, as it signed it.
    let mut records = published(&mut core, NODE);
    // Core asks the providers in name order, so the records come in that order: the route's order here.
    records.sort_by_key(|r| [IN, EXCHANGE, QUICK].iter().position(|p| *p == r.maker));
    assert_eq!(
        records.iter().map(|r| r.maker.as_str()).collect::<Vec<_>>(),
        [IN, EXCHANGE, QUICK],
        "one record for each provider that served in the node, and none of another node's"
    );
    for record in &records {
        assert_eq!(record.statement.node, NODE);
        assert_eq!(record.statement.publisher, "sim-join-publisher");
        assert_eq!(record.statement.standing, Standing::PublishersWord);
        assert_eq!(record.statement.segments.len(), 1);
    }
    let seen = |i: usize| {
        let s = &records[i].statement.segments[0];
        (
            s.transaction.as_str(),
            s.asset_in.as_str(),
            s.amount_in.as_str(),
            s.asset_out.as_str(),
            s.amount_out.as_str(),
            s.fee.as_str(),
        )
    };
    assert_eq!(
        seen(0),
        (tx_in.as_str(), "COP", AMOUNT, "COP", AMOUNT, "0"),
        "the withdrawal into the node"
    );
    assert_eq!(
        seen(1),
        (tx_exchange.as_str(), "COP", AMOUNT, "USD", AMOUNT, "0"),
        "the conversion inside it"
    );
    assert_eq!(
        seen(2),
        (tx_ramp.as_str(), "USD", AMOUNT, "USD", moved, "5000"),
        "the transfer out of it: what it took in, what it gave out, and what it kept"
    );
    // The other reserve's node holds its own record, of its own publisher, and the joined node's books don't read it.
    let other = published(&mut core, "sim-reserve-b-ledger");
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].maker, RESERVE);
    assert_eq!(other[0].statement.publisher, "sim-reserve-b-publisher");
    assert_eq!(
        other[0].statement.segments.len(),
        2,
        "its withdrawal and its deposit"
    );
    assert!(
        other[0]
            .statement
            .segments
            .iter()
            .zip(&reserve)
            .all(|(s, t)| s.transaction == transaction(t).as_str())
    );

    // The books read those statements, for the transactions asked, in the order asked.
    let all = [tx_in.clone(), tx_exchange.clone(), tx_ramp.clone()];
    assert_eq!(
        core.books_for(NODE, &all),
        Some(vec![
            recorded("COP", AMOUNT, "COP", AMOUNT, "0"),
            recorded("COP", AMOUNT, "USD", AMOUNT, "0"),
            recorded("USD", AMOUNT, "USD", moved, "5000"),
        ])
    );
    assert_eq!(
        core.books_for(NODE, std::slice::from_ref(&tx_exchange)),
        Some(vec![recorded("COP", AMOUNT, "USD", AMOUNT, "0")]),
        "a transaction that was asked for, and no other"
    );
    assert_eq!(
        core.books_for(NODE, &[transaction(&segments[3])]),
        None,
        "the deposit ran at a public node: no node's book holds it"
    );
    assert_eq!(
        core.books_for(NODE, &reserve.iter().map(transaction).collect::<Vec<_>>()),
        None,
        "the other reserve's segments are in no book of this node"
    );
    assert_eq!(core.books_for("sim-reserve-b-ledger", &all), None);

    // And the replay of what they read agrees with the public movement out, and with nothing more.
    assert_eq!(
        state(&mut core, &order),
        rutent_contracts::state_machine::OrderState::Complete
    );
    let found = replays(&core, &order);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, segments[2].segment);
    assert_eq!(found[0].2, ReplayFound::ConsistentWithTheExit {});
    let movement = core
        .world()
        .network(CHAIN)
        .expect("the public node's network")
        .status(&MovementId::new(tx_ramp.as_str()).expect("an id"));
    let Status::Final(movement) = movement else {
        panic!("the movement out is final: {movement:?}");
    };
    assert_eq!(
        movement.amount, moved,
        "the record and the exit are one figure"
    );
}

/// The order `within` the reserve `reserve`, bank to bank, run: its two segments.
fn within_run(core: &mut Core, session: &Id, name: &str, reserve: &str) -> Vec<Accounted> {
    let (_, ran) = run(core, within(name, session, AMOUNT, reserve, OTHER));
    let segments = accounted(&ran).to_vec();
    assert_eq!(
        segments.len(),
        2,
        "a withdrawal and a deposit: {segments:?}"
    );
    segments
}

/// The nodes' records as the books give them, and the exit the second one's last segment made: what `CMP-EVIDENCE`
/// replays, with the network.
fn replayed(
    core: &mut Core,
    first: (&str, &[Accounted]),
    second: (&str, &[Accounted]),
    network: &str,
) -> Replay {
    let node = |core: &Core, (reserve, segments): (&str, &[Accounted])| NodeRecord {
        node: format!("{reserve}-ledger"),
        segments: core
            .books_for(
                &format!("{reserve}-ledger"),
                &segments.iter().map(transaction).collect::<Vec<_>>(),
            )
            .expect("the books hold the node's segments"),
    };
    let nodes = [node(core, first), node(core, second)];
    let exit = transaction(&second.1[1]);
    let net = core.world().network(network).expect("a network").clone();
    let Status::Final(movement) = net.status(&MovementId::new(exit.as_str()).expect("an id"))
    else {
        panic!("the exit is on the network, final");
    };
    replay_route(
        &nodes,
        &RouteEnd::Public(PublicExit {
            movement: MovementId::new(exit.as_str()).expect("an id"),
            network: network.to_owned(),
            destination: movement.destination,
            stated: Stated {
                asset: "COP".to_owned(),
                amount: gave(&second.1[1]).to_owned(),
            },
        }),
        &net,
    )
}

//
// The platform's part runs one node: a route across two can't run on the platform (the module's doc). The second node's
// part is the replay of two reserves' nodes over what the assembly's books read of both.
// verifies: REQ-ORD-41, DEC-24
#[test]
fn mismatching_record_is_replayed_and_recorded_at_the_second_of_two_private_nodes() {
    // On the platform, a record that says what the segment didn't do still reaches Core's replay, which records it
    // against the node.
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
    core.world().script_for(
        QUICK,
        "transfer",
        Behaviour::misrecording(&(1_000_000 - RAMP_FEE - 1).to_string()),
    );
    let (order, segments) = joined(&mut core, &session);
    let moved = gave(&segments[2]).to_owned();
    assert_eq!(moved, (1_000_000 - RAMP_FEE).to_string());
    let record = published(&mut core, NODE)
        .into_iter()
        .find(|r| r.maker == QUICK)
        .expect("the ramp's record");
    assert_eq!(
        record.statement.segments[0].amount_out,
        (1_000_000 - RAMP_FEE - 1).to_string(),
        "its publisher recorded one less than the ramp gave out"
    );
    let found = replays(&core, &order);
    assert_eq!(found.len(), 1, "{found:?}");
    let ReplayFound::Mismatch { mismatches } = &found[0].2 else {
        panic!("the mismatch is recorded: {:?}", found[0].2);
    };
    assert!(!mismatches.is_empty());
    assert!(
        mismatches.iter().all(|m| m.node.as_str() == NODE),
        "recorded against the node: {mismatches:?}"
    );

    // Across two nodes of two publishers, a record that agrees at the first and not at the second is recorded
    // against the second.
    let (mut core, session) = core_over(&[RESERVE, OTHER_RESERVE]);
    let a = within_run(&mut core, &session, "req-a", OTHER_RESERVE);
    let b = within_run(&mut core, &session, "req-b", RESERVE);
    let network = Spec::of(rutent_conformance::sim::Role::Reserve, RESERVE).bank();
    let (first, second) = ((OTHER_RESERVE, a.as_slice()), (RESERVE, b.as_slice()));
    let consistent = replayed(&mut core, first, second, &network);
    let Replay::Consistent(consistent) = consistent else {
        panic!("both nodes' records agree with the exit: {consistent:?}");
    };
    assert_eq!((consistent.nodes, consistent.amount.as_str()), (2, AMOUNT));

    core.world().script_for(
        RESERVE,
        "deposit",
        Behaviour::misrecording(&(1_000_000 - 1).to_string()),
    );
    let b = within_run(&mut core, &session, "req-c", RESERVE);
    let second = (RESERVE, b.as_slice());
    let Replay::Mismatch(found) = replayed(&mut core, first, second, &network) else {
        panic!("the second node's record doesn't agree with its exit");
    };
    let first_node = format!("{OTHER_RESERVE}-ledger");
    let second_node = format!("{RESERVE}-ledger");
    assert!(
        found.contains(&Mismatch::FeeMismatch {
            node: second_node.clone(),
            at: 1
        }),
        "its deposit doesn't add up: {found:?}"
    );
    assert!(
        found
            .iter()
            .any(|m| matches!(m, Mismatch::StatedOutcome { .. })),
        "{found:?}"
    );
    assert!(
        found.iter().any(|m| matches!(m, Mismatch::PublicExit(_))),
        "{found:?}"
    );
    assert!(
        !found.iter().any(|m| matches!(
            m,
            Mismatch::FeeMismatch { node, .. } | Mismatch::BrokenChain { node, .. } if *node == first_node
        )),
        "nothing is recorded against the first: {found:?}"
    );
}

// verifies: REQ-ORD-3, AC-35
#[test]
fn record_of_a_route_ending_inside_the_node_is_labelled_the_publishers_word() {
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
    let (order, segments) = joined(&mut core, &session);
    let (tx_in, tx_exchange) = (transaction(&segments[0]), transaction(&segments[1]));

    // The withdrawal and the conversion end inside the node: its publisher's record of them is labelled as its word,
    // and no replay was recorded for either. Only the transfer out of the node was replayed.
    let records = published(&mut core, NODE);
    assert_eq!(records.len(), 3);
    assert!(
        records
            .iter()
            .all(|r| r.statement.standing == Standing::PublishersWord)
    );
    let inside = [tx_in.clone(), tx_exchange.clone()];
    let held: Vec<RecordedSegment> = core.books_for(NODE, &inside).expect("recorded");
    assert_eq!(
        held,
        [
            recorded("COP", AMOUNT, "COP", AMOUNT, "0"),
            recorded("COP", AMOUNT, "USD", AMOUNT, "0"),
        ]
    );
    let found = replays(&core, &order);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, segments[2].segment, "the transfer's alone");
    assert!(
        found
            .iter()
            .all(|(s, ..)| { *s != segments[0].segment && *s != segments[1].segment })
    );

    // The same record, where the route ends inside the node, is replayed against no exit: the publisher's word.
    let net = core.world().network(CHAIN).expect("a network").clone();
    let replay = replay_route(
        &[NodeRecord {
            node: NODE.to_owned(),
            segments: held,
        }],
        &RouteEnd::InsideNode,
        &net,
    );
    assert_eq!(replay, Replay::PublishersWord);
}

/// Closing asks the node's publisher for its record, in the operator's name through the platform, and reads what the
/// provider answered: no test asks, no tick is needed, and the assembly names no node and no provider (`REQ-ORD-41`,
/// `T-020-58`). Core chooses them from the route it runs: the node the segments pass and the providers its publisher
/// declares it for, and no other node, such as the other reserve's.
// verifies: REQ-ORD-41
#[test]
fn private_node_record_is_asked_of_its_publisher_and_read_by_closing() {
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
    assert!(
        core.publishers_asked().is_empty(),
        "nothing asked before an order runs"
    );
    let (order, segments) = joined(&mut core, &session);
    // Asked while the order ran, from closing: no tick ran and no test asked.
    let asked = core.publishers_asked();
    assert!(
        !asked.is_empty(),
        "closing asked the publisher for its record"
    );
    // The nodes and providers are Core's: the one node the route passes, and the providers that serve in it.
    for (provider, node, _) in &asked {
        assert_eq!(node, NODE, "{provider} asked for another node's record");
        assert!(
            [IN, EXCHANGE, QUICK].contains(&provider.as_str()),
            "{provider}"
        );
    }
    for provider in [IN, EXCHANGE, QUICK] {
        assert!(
            asked.iter().any(|(p, ..)| p == provider),
            "{provider} serves in the node and was asked"
        );
    }
    // Each question under a transaction of its own, none a segment's.
    let mut own: Vec<&str> = asked.iter().map(|(.., t)| t.as_str()).collect();
    let questions = own.len();
    own.sort_unstable();
    own.dedup();
    assert_eq!(own.len(), questions, "a transaction for each question");
    for segment in &segments {
        let tx = transaction(segment);
        assert!(
            !own.contains(&tx.as_str()),
            "a question under a segment's transaction"
        );
    }
    // What closing read is what the providers answered: the books hold their records, and the replay found them
    // consistent with the exit.
    let asked_for: Vec<Id> = segments[..3].iter().map(transaction).collect();
    assert_eq!(core.books_for(NODE, &asked_for).map(|b| b.len()), Some(3));
    let found = replays(&core, &order);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].2, ReplayFound::ConsistentWithTheExit {});
    // The tick's job asks as well, over the nodes of the routes Core runs, and takes the same records.
    let before = core.publishers_asked().len();
    let round = core.tick();
    assert!(core.publishers_asked().len() > before, "the tick asked too");
    let mut makers: Vec<&str> = round
        .published_in(NODE)
        .iter()
        .map(|r| r.maker.as_str())
        .collect();
    makers.sort_unstable();
    assert_eq!(makers, [EXCHANGE, IN, QUICK]);
    assert!(
        core.publishers_asked()
            .iter()
            .all(|(_, node, _)| node == NODE),
        "the tick asks no node a route of Core's doesn't pass"
    );
}

/// A provider of the node's publisher that declares no private node is never asked for the node's record: Core asks the
/// providers that declare the node alone (`REQ-ORD-41`, `T-020-58`).
// verifies: REQ-ORD-41
#[test]
fn a_provider_of_the_publisher_that_declares_no_node_is_never_asked_for_its_record() {
    const CLERK: &str = "sim-join-cashier";
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, CLERK]);
    let (_, segments) = joined(&mut core, &session);
    core.tick();
    let asked = core.publishers_asked();
    assert!(!asked.is_empty());
    assert!(
        asked.iter().all(|(provider, ..)| provider != CLERK),
        "{CLERK} declares no node and was asked: {asked:?}"
    );
    for provider in [IN, EXCHANGE, QUICK] {
        assert!(asked.iter().any(|(p, ..)| p == provider), "{provider}");
    }
    let books: Vec<Id> = segments[..3].iter().map(transaction).collect();
    assert!(core.books_for(NODE, &books).is_some());
}

/// The read is the operator's, decided by policy (`read_node_record`) and recorded as an effect before the question is
/// put, for each provider of the node and under a fresh transaction each (`REQ-AUZ-1`, `REQ-ORD-41`, `T-020-58`).
/// Where policy denies it, nothing is asked, no effect is recorded, and the node's books say nothing: the replay records
/// that no record was had, and the order is not stopped by it.
// verifies: REQ-AUZ-1, REQ-ORD-41
#[test]
fn private_node_record_is_not_read_where_policy_denies_it() {
    use rutent_contracts::core_internal::Action;
    let read_effects = |core: &Core| {
        entries(core)
            .into_iter()
            .filter(|v| {
                matches!(
                    &v.what,
                    What::Effect {
                        action: Action::ReadNodeRecord { .. },
                        ..
                    }
                )
            })
            .count()
    };
    // The control: with the permission, each read is an effect, one for each question put.
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
    let (_, segments) = joined(&mut core, &session);
    assert!(read_effects(&core) > 0);
    assert!(
        core.publishers_asked().len() >= read_effects(&core),
        "every effect is followed by a question"
    );
    let asked: Vec<Id> = segments[..3].iter().map(transaction).collect();
    assert!(core.books_for(NODE, &asked).is_some());

    // Denied: the operator doesn't hold the permission.
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
    core.registers()
        .permissions
        .retain(|(_, p)| *p != rutent_core_authz::roles::Permission::ReadNodeRecord);
    let (order, segments) = joined(&mut core, &session);
    assert!(core.publishers_asked().is_empty(), "nothing was asked");
    assert_eq!(read_effects(&core), 0, "no read was taken as an effect");
    let asked: Vec<Id> = segments[..3].iter().map(transaction).collect();
    assert_eq!(core.books_for(NODE, &asked), None);
    let found = replays(&core, &order);
    assert!(
        found.len() == 1 && matches!(found[0].2, ReplayFound::NothingRecorded { .. }),
        "{found:?}"
    );
}

/// The production platform answers the question of a private node's record: `impl Modules for Platform` carries it
/// through `Platform::handle` as the permitted effect lets, and never answers `NotHandled` (`REQ-ORD-41`, `T-020-58`).
// verifies: REQ-ORD-41
#[test]
fn production_platform_answers_the_question_of_a_private_nodes_record() {
    use rutent_contracts::core_internal::{Action, Effect, Entry};
    use rutent_contracts::provider_role::{InnerSegmentsOutcome, ResultBody, Statement};
    use rutent_core_orch::segments::Answered;
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
    joined(&mut core, &session);
    // An effect Core recorded on a permit of policy's, for a read of the node's record.
    let effect = core
        .log()
        .entries()
        .iter()
        .find_map(|(receipt, entry)| match entry {
            Entry::Verified(v) => match &v.what {
                What::Effect {
                    action: action @ Action::ReadNodeRecord { .. },
                    decision,
                } => Some(Effect {
                    action: action.clone(),
                    decision: decision.clone(),
                    receipt: receipt.clone(),
                }),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .expect("closing recorded a read");
    let Action::ReadNodeRecord { provider, node, .. } = &effect.action else {
        unreachable!()
    };
    let (provider, node) = (provider.as_str().to_owned(), node.as_str().to_owned());
    let Answered::Bytes(bytes) =
        core.ask_record_of_the_platform(&effect, &provider, &node, "tx-production-question")
    else {
        panic!("the production platform didn't answer");
    };
    let answer: Statement<ResultBody> = serde_json::from_slice(&bytes).expect("a result");
    let ResultBody::InnerSegmentsQuery(result) = answer.statement else {
        panic!("not an answer to the question of a record");
    };
    assert_eq!(result.transaction, "tx-production-question");
    assert_eq!(result.node, node);
    assert!(matches!(result.outcome, InnerSegmentsOutcome::Answered(_)));
}

/// A provider that answers the question with a record it can't stand behind is not taken as the node's record, however
/// it errs: a record of another publisher's, of a node it doesn't declare, signed with a key it doesn't list, or altered
/// after it was signed (`REQ-ORD-41`, `T-020-58`).
// verifies: REQ-ORD-41
#[test]
fn a_record_its_maker_cannot_stand_behind_is_not_the_nodes_record() {
    use rutent_scenarios::assembly::Tamper;
    for how in [
        Tamper::Figure,
        Tamper::Publisher,
        Tamper::Node,
        Tamper::Key,
        Tamper::Answer,
    ] {
        let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
        core.tamper_with_record_of(IN, how);
        let (order, segments) = joined(&mut core, &session);
        let asked: Vec<Id> = segments[..3].iter().map(transaction).collect();
        assert_eq!(core.books_for(NODE, &asked), None, "{how:?}");
        let found = replays(&core, &order);
        assert!(
            found.len() == 1 && matches!(found[0].2, ReplayFound::NothingRecorded { .. }),
            "{how:?}: {found:?}"
        );
    }
}

// verifies: REQ-ORD-41
#[test]
fn record_refused_for_one_provider_of_the_node_leaves_the_node_with_nothing_recorded() {
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
    core.forge_record_of(IN);
    let (order, segments) = joined(&mut core, &session);
    let asked: Vec<Id> = segments[..3].iter().map(transaction).collect();
    // The first provider's record is refused, so the two others' segments are not the node's whole record.
    assert_eq!(
        published(&mut core, NODE)
            .iter()
            .map(|r| r.maker.as_str())
            .collect::<Vec<_>>(),
        [EXCHANGE, QUICK]
    );
    assert_eq!(core.books_for(NODE, &asked), None);
    let found = replays(&core, &order);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        matches!(&found[0].2, ReplayFound::NothingRecorded { nodes } if nodes.iter().map(|n| n.as_str()).eq([NODE])),
        "recorded as nothing recorded for the node: {:?}",
        found[0].2
    );
}

/// Core's schedule reads each publisher's signed record of its private node through the platform, held to its maker's
/// advertisement and signature, and the replay's books are the records it read (`REQ-ORD-41`, `T-020-48`).
// verifies: REQ-ORD-41
#[test]
fn publishers_records_are_read_through_the_platform_for_a_replay() {
    let (mut core, session) = core_over(&[IN, EXCHANGE, QUICK, OUT, RESERVE]);
    let (_, segments) = joined(&mut core, &session);
    let asked: Vec<Id> = segments[..3].iter().map(transaction).collect();
    let round = core.tick();
    assert!(round.did(rutent_core_orch::schedule::Job::PublishersRecords));
    let records = round.published_in(NODE);
    assert_eq!(
        records.iter().map(|r| r.maker.as_str()).collect::<Vec<_>>(),
        [EXCHANGE, IN, QUICK],
        "one record for each provider that served in the node, in name order, as Core asks them"
    );
    let books = core.books_for(NODE, &asked).expect("the node's books");
    let stated: usize = records.iter().map(|r| r.statement.segments.len()).sum();
    assert_eq!(books.len(), stated, "the replay reads what the round read");
    // A record altered after it was signed is refused: it is not among those the round read.
    core.forge_record_of(IN);
    let round = core.tick();
    assert_eq!(
        round
            .published_in(NODE)
            .iter()
            .map(|r| r.maker.as_str())
            .collect::<Vec<_>>(),
        [EXCHANGE, QUICK]
    );
}
