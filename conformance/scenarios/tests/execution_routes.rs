//! Routes across more than one provider, run on the assembled Core against the simulated providers that join one
//! another's nodes (`T-024-04`): a reserve, an exchange and a ramp (`AC-1`, `AC-2`, `AC-30`), and an on-chain route
//! of two ramps with no exchange and no local money (`AC-26`, `AC-27`). The providers are `rutent_conformance::sim`'s
//! name-chosen shapes (`shape_of`): a group of one publisher shares a private node, so a ramp and an exchange reach
//! a reserve's.

mod execution_support;

use execution_support::*;
use rutent_conformance::sim::Simulated;
use rutent_contracts::core_internal::{
    AcceptanceBasis, Action, ReplayFound, Role, ShownAs, TimeSource, What,
};
use rutent_contracts::provider_role::{EvidenceGrade, NodeRef};
use rutent_contracts::router_api::{Candidate, DropReason, Id, Objective, OperationKind, Router};
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::arrival::{Arrived, Filed};
use rutent_core_orch::intake::{OrderRequest, Status, Told};
use rutent_core_orch::segments::{Accounted, Ran, SegmentOutcome};
use rutent_router::RouteFinder;
use rutent_scenarios::assembly::{Core, component};
use rutent_scenarios::kit::{self, AMOUNT};

const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const RAMP: &str = "sim-join-ramp";
const QUICK: &str = "sim-join-ramp-quick";
const STEPS: &str = "sim-join-ramp-steps";
const OUT: &str = "sim-join-out";

const CHAIN_IN: &str = "sim-chain-in";
const RAMP_B: &str = "sim-chain-ramp-b";
const RAMP_C: &str = "sim-chain-ramp-c";
const OUT_C: &str = "sim-chain-out-c";
const OUT_B: &str = "sim-chain-out-b";

/// The address the requester provides for a deposit on chain, which the destination reserve's register holds as the
/// customer's.
const ADDRESS: &str = "0x5e11a9d0c0ffee";

/// The order that carries `AMOUNT` of COP from the reserve `sim-join-in`'s bank to USD at the bank of `sim-join-out`.
fn across(name: &str, session: &Id, objective: &str) -> OrderRequest {
    request(
        name,
        session,
        AMOUNT,
        IN,
        outer("sim-join-in-bank", "COP"),
        outer("sim-join-out-bank", "USD"),
        OUT,
        OTHER,
        objective,
    )
}

/// The segments of the one part `ran` ran.
fn accounted(ran: &[Ran]) -> &[Accounted] {
    let [Ran::Segments(segments)] = ran else {
        panic!("one part ran its segments: {ran:?}");
    };
    segments
}

/// The providers a candidate's operations run through, in order.
fn route_of(candidate: &Candidate) -> Vec<String> {
    candidate
        .parts
        .iter()
        .flat_map(|p| p.operations.iter())
        .map(|o| o.provider.as_str().to_owned())
        .collect()
}

/// Whether every segment `segments` ran settled.
fn all_settled(segments: &[Accounted]) -> bool {
    segments
        .iter()
        .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. }))
}

/// The customer's account at the destination reserve, given as an address the requester provides.
fn register_address(core: &Core) {
    core.registers().accounts.insert(
        ADDRESS.to_owned(),
        kit::customer().party.as_str().to_owned(),
    );
}

// verifies: AC-1
#[test]
fn order_across_three_providers_completes_with_who_did_what_when_and_the_executed_price() {
    let (mut core, session) = core_with("AC-1", &[IN, EXCHANGE, RAMP, OUT]);
    let (order, told) = quoted(
        &mut core,
        across("req-three", &session, "lowest_total_cost"),
    );
    // A reserve, an exchange and a ramp, and the reserve the value is deposited in.
    let record = record_of(&core, &told);
    assert_eq!(providers(&record), [IN, EXCHANGE, RAMP, OUT]);
    let kinds: Vec<_> = record.chosen().unwrap().parts[0]
        .operations
        .iter()
        .map(|o| o.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            OperationKind::Withdrawal,
            OperationKind::Exchange,
            OperationKind::Route,
            OperationKind::Deposit
        ]
    );
    let Told::Exists {
        status: Status::Quoted { quote, digest },
        ..
    } = told
    else {
        panic!("quoted")
    };
    assert_eq!(quote.shown_as, ShownAs::Quote, "before it runs, a quote");
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    let ran = core.run(&order).expect("the order ran");

    // Every segment settles, and the order is complete.
    let segments = accounted(&ran);
    assert_eq!(segments.len(), 4);
    assert!(all_settled(segments), "{segments:?}");
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // The record shows who did what, and when: for each segment, an effect of Core's own, carrying the user's
    // authorisation onward, naming the provider it handed the segment to, and when, by Core's clock, with its
    // source said; then how long it took.
    let log = entries(&core);
    for (accounted, provider) in segments.iter().zip([IN, EXCHANGE, RAMP, OUT]) {
        let effect = log
            .iter()
            .find(|v| {
                matches!(&v.what, What::Effect { action: Action::MoveValue { order: o, segment, .. }, .. }
                    if o == &order && segment == &accounted.segment)
            })
            .expect("the segment's effect is recorded");
        assert_eq!(
            effect.who,
            kit::core(),
            "who: Core, acting for the customer"
        );
        let What::Effect {
            action: Action::MoveValue { provider: to, .. },
            ..
        } = &effect.what
        else {
            unreachable!()
        };
        assert_eq!(
            to.as_str(),
            provider,
            "what: the segment handed to {provider}"
        );
        assert_eq!(
            effect.when.source,
            TimeSource::CoreClock,
            "when, and by whose clock"
        );
        let settled_by = log
            .iter()
            .find_map(|v| match &v.what {
                What::Settlement {
                    order: o,
                    segment,
                    provider: p,
                    ..
                } if o == &order && segment == &accounted.segment => Some(p.clone()),
                _ => None,
            })
            .expect("how long it took is recorded");
        assert_eq!(settled_by.as_str(), provider);
    }

    // Nothing the providers did inside the private node reached a public network: no request went to a host of the
    // node's.
    assert!(
        core.world()
            .delivered()
            .iter()
            .all(|d| !d.host.contains("sim-join-ledger")),
        "{:?}",
        core.world().delivered()
    );

    // The final executed price is reported as the price, from what each segment did, and recorded once.
    let (digest, _) = core
        .report_price(&order, &ran)
        .expect("the price is reported");
    let shown = |core: &Core| -> Vec<_> {
        entries(core)
            .into_iter()
            .filter_map(|v| match v.what {
                What::Shown {
                    order: o,
                    shown,
                    shown_as,
                } if o == order => Some((shown, shown_as)),
                _ => None,
            })
            .collect()
    };
    assert!(
        shown(&core).contains(&(digest.clone(), ShownAs::Price)),
        "the price is recorded as the price: {:?}",
        shown(&core)
    );
    // Reported again, it is the same price, and no second entry: an order has one.
    assert_eq!(core.report_price(&order, &ran).expect("again").0, digest);
    assert_eq!(
        shown(&core)
            .iter()
            .filter(|(_, as_)| *as_ == ShownAs::Price)
            .count(),
        1
    );
}

// verifies: AC-2
#[test]
fn changed_objective_changes_the_route_across_a_reserve_an_exchange_and_a_ramp_and_the_record_explains_it_with_no_provider_call()
 {
    let names = [IN, EXCHANGE, RAMP, QUICK, OUT];
    let (mut core, session) = core_with("AC-2", &names);
    let (_, cheapest) = quoted(&mut core, across("req-cost", &session, "lowest_total_cost"));
    let (fast_order, fastest) = quoted(
        &mut core,
        across("req-fast", &session, "fastest_settlement"),
    );
    // The customer changes what it asks for, and the route changes with it: the ramp is the one that costs least, or
    // the one that settles first; the reserve, the exchange and the reserve of the deposit stay.
    assert_eq!(
        providers(&record_of(&core, &cheapest)),
        [IN, EXCHANGE, RAMP, OUT]
    );
    assert_eq!(
        providers(&record_of(&core, &fastest)),
        [IN, EXCHANGE, QUICK, OUT]
    );

    // The record explains the choice: it holds the candidate it chose and the other, dropped as outranked by it.
    let record = record_of(&core, &cheapest);
    let chosen = record.chosen().expect("chosen").candidate_id.clone();
    let other = record
        .candidates()
        .iter()
        .find(|c| route_of(c) == [IN, EXCHANGE, QUICK, OUT])
        .expect("the route through the other ramp was considered");
    // The other ramp takes 5 000 from the amount, so the deposit after it isn't the one the chosen route's was asked
    // at: the choice round confirmed only the chosen route's, and the record says the other wasn't confirmed.
    assert!(
        matches!(
            &other.dropped,
            Some(DropReason::Outranked { by }) if by == &chosen
        ) || matches!(&other.dropped, Some(DropReason::Unconfirmed { .. })),
        "dropped with a reason the record gives: {:?}",
        other.dropped
    );
    assert_eq!(record.request.objective, Objective::LowestTotalCost {});

    // Each decision is a function of its record alone: the router over the snapshot a record holds gives the record
    // again, for the customer who asked for the cheapest route and for the one who asked for the fastest, calling no
    // provider. (The choice round confirmed each route's own amounts, so a record can't be decided again under the
    // other objective: the other route's deposit is no longer confirmed in it.)
    let calls = core.world().delivered().len();
    let books: Vec<usize> = names
        .iter()
        .map(|p| core.world().journal(p).len())
        .collect();
    let fast = record_of(&core, &fastest);
    for held in [&record, &fast] {
        let again = RouteFinder::route(&held.request, &held.snapshot, &held.bounds);
        assert_eq!(&again, held, "the same record again from the record alone");
    }
    assert_eq!(fast.request.objective, Objective::FastestSettlement {});
    assert_ne!(
        route_of(fast.chosen().unwrap()),
        route_of(record.chosen().unwrap())
    );
    assert_eq!(core.world().delivered().len(), calls);
    let after: Vec<usize> = names
        .iter()
        .map(|p| core.world().journal(p).len())
        .collect();
    assert_eq!(after, books, "no provider's books moved");

    // And the route the faster objective chose runs: its ramp takes 5 000 from what it moves, and what its publisher
    // recorded inside the private node, from every provider that served in it, replays as consistent with the public
    // movement out.
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = &fastest
    else {
        panic!("quoted")
    };
    core.accept(&kit::customer(), &fast_order, digest)
        .expect("accepted");
    let ran = core.run(&fast_order).expect("the order ran");
    assert!(all_settled(accounted(&ran)));
    assert_eq!(state(&mut core, &fast_order), OrderState::Complete);
    let found: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Replay { order, found, .. } if order == fast_order => Some(found),
            _ => None,
        })
        .collect();
    assert_eq!(found, [ReplayFound::ConsistentWithTheExit {}]);
}

// verifies: AC-30
#[test]
fn module_with_internal_steps_leaves_the_routes_segments_unchanged() {
    use rutent_prov::catalogue::{Category, Endpoint, Grant, GrantError, TupleKey, check_tuple};

    // The same order over the same route, once through a ramp that serves its segment plainly and once through one
    // that takes internal steps to serve it.
    let (mut plain, session) = core_with("AC-30", &[IN, EXCHANGE, RAMP, OUT]);
    let (plain_order, plain_told) = quoted(
        &mut plain,
        across("req-plain", &session, "lowest_total_cost"),
    );
    let plain_route = record_of(&plain, &plain_told);
    let (mut steps, session) = core_with("AC-30", &[IN, EXCHANGE, STEPS, OUT]);
    let (steps_order, steps_told) = quoted(
        &mut steps,
        across("req-steps", &session, "lowest_total_cost"),
    );
    let steps_route = record_of(&steps, &steps_told);

    // The route's segments are what the route says, and the ramp's internal steps add, remove or reorder none: the
    // same four operations, of the same kinds, over the same nodes and assets, in the same order.
    let shape = |record: &rutent_contracts::router_api::DecisionRecord| -> Vec<_> {
        record.chosen().unwrap().parts[0]
            .operations
            .iter()
            .map(|o| (o.kind, o.from.clone(), o.to.clone(), o.amount_in.clone()))
            .collect()
    };
    assert_eq!(shape(&plain_route).len(), 4);
    assert_eq!(shape(&plain_route), shape(&steps_route));
    assert_eq!(providers(&steps_route), [IN, EXCHANGE, STEPS, OUT]);

    for (core, order, told) in [
        (&mut plain, &plain_order, &plain_told),
        (&mut steps, &steps_order, &steps_told),
    ] {
        let Told::Exists {
            status: Status::Quoted { digest, .. },
            ..
        } = told
        else {
            panic!("quoted")
        };
        core.accept(&kit::customer(), order, digest)
            .expect("accepted");
    }
    let plain_ran = plain.run(&plain_order).expect("ran");
    let steps_ran = steps.run(&steps_order).expect("ran");
    // It complies with its role's contract: its segment is served as the contract says, and settles on the same
    // evidence, with the same grades and the same whose, as the plain ramp's. The order completes, with as many
    // effects of Core's as segments.
    assert_eq!(accounted(&plain_ran).len(), accounted(&steps_ran).len());
    assert!(all_settled(accounted(&steps_ran)));
    assert_eq!(state(&mut steps, &steps_order), OrderState::Complete);
    let graded = |core: &Core, order: &Id| -> Vec<_> {
        evidence_of(core, order)
            .into_iter()
            .map(|e| {
                (
                    e.grade,
                    e.whose.as_str().replace(STEPS, RAMP),
                    e.movement.is_some(),
                )
            })
            .collect()
    };
    assert_eq!(graded(&plain, &plain_order), graded(&steps, &steps_order));
    let effects = |core: &Core, order: &Id| {
        entries(core)
            .iter()
            .filter(|v| matches!(&v.what, What::Effect { action: Action::MoveValue { order: o, .. }, .. } if o == order))
            .count()
    };
    assert_eq!(effects(&plain, &plain_order), 4);
    assert_eq!(effects(&steps, &steps_order), 4);

    // The steps were taken inside the module: its own books, rebuilt from its journal, say so, and the plain ramp's
    // say none. Core's record of the route shows none of them.
    let taken = |core: &mut Core, provider: &str| -> Vec<String> {
        let book = core.world().journal(provider).to_vec();
        let mut sim = Simulated::new(spec_of(provider), &rutent_scenarios::assembly::context());
        sim.restore(&book);
        sim.internal_steps()
    };
    assert_eq!(
        taken(&mut steps, STEPS).len(),
        3,
        "a conversion, a move and a conversion"
    );
    assert!(taken(&mut plain, RAMP).is_empty());

    // An exchange spanning two networks and a ramp within one are refused: no grant admits such a tuple, whatever
    // the module does inside, and the catalogue's check of a tuple's shape names why.
    let exchange = |from: &str, to: &str| TupleKey {
        category: Category::Exchange,
        origin: Endpoint::Inner(from.into()),
        destination: Endpoint::Inner(to.into()),
        asset: "COP".into(),
        to_asset: Some("USD".into()),
    };
    let ramp = |from: &str, to: &str| TupleKey {
        category: Category::Ramp,
        origin: Endpoint::Inner(from.into()),
        destination: Endpoint::Inner(to.into()),
        asset: "USD".into(),
        to_asset: None,
    };
    let installed = |core: &Core, provider: &str| {
        core.installed
            .iter()
            .find(|i| i.spec.provider == provider)
            .expect("installed")
            .clone()
    };
    let refused = |provider: &str, tuple: TupleKey| {
        let mut terms = kit::grant_terms(
            &installed(&plain, provider),
            "operator-admin",
            rutent_prov::catalogue::Instant(1_000),
        );
        terms.tuples = vec![tuple];
        Grant::new(terms)
    };
    assert_eq!(
        check_tuple(&exchange("sim-join-ledger", "sim-join-ledger")),
        Ok(())
    );
    assert!(check_tuple(&exchange("sim-join-ledger", "sim-join-chain")).is_err());
    assert!(matches!(
        refused(EXCHANGE, exchange("sim-join-ledger", "sim-join-chain")),
        Err(GrantError::MalformedTuple(_))
    ));
    assert_eq!(
        check_tuple(&ramp("sim-join-ledger", "sim-join-chain")),
        Ok(())
    );
    assert!(check_tuple(&ramp("sim-join-ledger", "sim-join-ledger")).is_err());
    assert!(matches!(
        refused(RAMP, ramp("sim-join-ledger", "sim-join-ledger")),
        Err(GrantError::MalformedTuple(_))
    ));
}

/// The decision record's route end to end: the outer node it starts at, the outer node it ends at, and each segment's
/// kind, in order.
fn ends_of(
    record: &rutent_contracts::router_api::DecisionRecord,
) -> (NodeRef, NodeRef, Vec<OperationKind>) {
    let operations = &record.chosen().expect("a route").parts[0].operations;
    (
        operations[0].from.node.clone(),
        operations[operations.len() - 1].to.node.clone(),
        operations.iter().map(|o| o.kind).collect(),
    )
}

// verifies: AC-26
#[test]
fn on_chain_origin_with_an_automatic_deposit_and_two_ramps_runs_with_no_exchange() {
    let mut given = given(&[]);
    given.providers = vec![
        spec_of(CHAIN_IN).starting_routes_on_automatic_deposits(),
        spec_of(RAMP_B),
        spec_of(RAMP_C),
        spec_of(OUT_C),
    ];
    let (mut core, session) = core_of("AC-26", given);
    register_address(&core);
    let filer = component(CHAIN_IN, CHAIN_IN, Role::Provider);
    // The deployment's register of actors lists the reserve's component as acting for the reserve.
    core.registers().list(&filer);

    // Value arrives at the reserve from an on-chain transfer, with no order: the reserve declared it starts routes on
    // automatic deposits, so it files the order itself.
    let reported = report(
        &core,
        CHAIN_IN,
        "sim-chain-a",
        unit("USDC"),
        AMOUNT,
        "credit-chain",
    );
    let Arrived::NoOrder { arrival } = core.arrive(&reported).expect("the arrival is taken") else {
        panic!("the arrival is held against no order");
    };
    let asked = request(
        "req-chain",
        &session,
        AMOUNT,
        CHAIN_IN,
        outer("sim-chain-a", "USDC"),
        outer("sim-chain-d", "USDC"),
        OUT_C,
        ADDRESS,
        "lowest_total_cost",
    );
    let filed = core.file(filing(
        "deposit-chain",
        filer.clone(),
        arrival,
        &session,
        asked,
    ));
    let Ok(Filed::Started { order, .. }) = filed else {
        panic!("the deposit starts: {filed:?}");
    };
    // The deposit is the acceptance, and the record says so, and by whom.
    let acceptance = entries(&core)
        .into_iter()
        .find_map(|v| match v.what {
            What::Acceptance {
                order: o, basis, ..
            } if o == order => Some((basis, v.who)),
            _ => None,
        })
        .expect("accepted");
    assert_eq!(acceptance, (AcceptanceBasis::AutomaticDeposit, filer));

    // It is expressed and run: a withdrawal, two ramps and a deposit, with no exchange, and no local money anywhere
    // on it, every segment in USDC.
    let ran = core.run(&order).expect("the order ran");
    let segments = accounted(&ran);
    assert!(all_settled(segments), "{segments:?}");
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    let route = core.route_operations(&order);
    assert_eq!(
        route.iter().map(|o| o.kind).collect::<Vec<_>>(),
        [
            OperationKind::Withdrawal,
            OperationKind::Route,
            OperationKind::Route,
            OperationKind::Deposit
        ],
        "no exchange"
    );
    assert!(
        route
            .iter()
            .all(|o| o.from.asset.id == "USDC" && o.to.asset.id == "USDC"),
        "no local money: {route:?}"
    );
    assert_eq!(
        route
            .iter()
            .map(|o| o.provider.as_str())
            .collect::<Vec<_>>(),
        [CHAIN_IN, RAMP_B, RAMP_C, OUT_C]
    );

    // Its origin and destination are recorded by their outer nodes: the decision record's route starts at the
    // network the transfer arrived on and ends at the destination reserve's, and each effect carries the destination
    // as the order's record names it.
    let record = core
        .decision_records()
        .into_iter()
        .find(|r| {
            r.chosen()
                .is_some_and(|c| route_of(c) == [CHAIN_IN, RAMP_B, RAMP_C, OUT_C])
        })
        .expect("the route's record");
    let (start, end, _) = ends_of(&record);
    assert_eq!(node_name(&start), "sim-chain-a");
    assert!(matches!(start, NodeRef::Outer(_)));
    assert_eq!(node_name(&end), "sim-chain-d");
    assert!(matches!(end, NodeRef::Outer(_)));
    let destinations: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Effect {
                action:
                    Action::MoveValue {
                        order: o, route, ..
                    },
                ..
            } if o == order => Some(route.destination),
            _ => None,
        })
        .collect();
    assert_eq!(destinations.len(), 4);
    assert!(destinations.iter().all(|d| {
        d.node.network == "sim-chain-d"
            && d.account.as_str() == ADDRESS
            && d.reserve.as_str() == OUT_C
    }));

    // It closes on the public record plus the reserve's signed statement.
    let deposit = segments[3].segment.clone();
    let closing: Vec<_> = evidence_of(&core, &order)
        .into_iter()
        .filter(|e| e.segment == deposit)
        .collect();
    assert_eq!(closing.len(), 2);
    assert_eq!(
        (closing[0].whose.as_str(), closing[0].grade),
        ("sim-chain-d", EvidenceGrade::PublicRecord)
    );
    assert!(closing[0].movement.is_some());
    assert_eq!(closing[1].whose.as_str(), OUT_C);
    // Where the value left the reserve's private node by the first ramp, what its publisher recorded inside the node
    // was replayed against the public movement out, and found consistent with it, and no more than that.
    let found: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Replay {
                order: o, found, ..
            } if o == order => Some(found),
            _ => None,
        })
        .collect();
    assert_eq!(found, [ReplayFound::ConsistentWithTheExit {}]);
}

// verifies: AC-27
#[test]
fn deposit_to_the_requesters_address_shows_the_outer_node_and_the_address() {
    let (mut core, session) = core_with("AC-27", &[CHAIN_IN, RAMP_B, OUT_B]);
    register_address(&core);
    let asked = request(
        "req-address",
        &session,
        AMOUNT,
        CHAIN_IN,
        outer("sim-chain-a", "USDC"),
        outer("sim-chain-e", "USDC"),
        OUT_B,
        ADDRESS,
        "lowest_total_cost",
    );
    let (order, ran) = run(&mut core, asked);
    let segments = accounted(&ran);
    assert!(all_settled(segments), "{segments:?}");
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // An on-chain origin, one ramp, and a deposit into a reserve: a withdrawal, a ramp and a deposit.
    let route = core.route_operations(&order);
    assert_eq!(
        route.iter().map(|o| o.kind).collect::<Vec<_>>(),
        [
            OperationKind::Withdrawal,
            OperationKind::Route,
            OperationKind::Deposit
        ]
    );
    assert_eq!(
        route
            .iter()
            .map(|o| o.provider.as_str())
            .collect::<Vec<_>>(),
        [CHAIN_IN, RAMP_B, OUT_B]
    );

    // The order's record shows the outer node and the address: the destination the customer's request named is the
    // one the log records for it, and each effect that moved value carries it, so the authorisation it was held to
    // names it.
    let checked: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::DestinationCheck {
                order: o,
                destination,
                ..
            } if o == order => Some(destination),
            _ => None,
        })
        .collect();
    assert!(!checked.is_empty(), "the destination was resolved");
    for destination in checked.iter().chain(
        entries(&core)
            .iter()
            .filter_map(|v| match &v.what {
                What::Effect {
                    action:
                        Action::MoveValue {
                            order: o, route, ..
                        },
                    ..
                } if o == &order => Some(&route.destination),
                _ => None,
            })
            .collect::<Vec<_>>(),
    ) {
        assert_eq!(destination.node.network, "sim-chain-e", "the outer node");
        assert_eq!(destination.account.as_str(), ADDRESS, "the address");
        assert_eq!(destination.reserve.as_str(), OUT_B);
    }
    // The network recorded the deposit's movement to that address, and the order closed on it.
    let deposit = segments[2].segment.clone();
    let closing = evidence_of(&core, &order)
        .into_iter()
        .find(|e| e.segment == deposit && e.movement.is_some())
        .expect("the public record");
    let movement = closing.movement.expect("a movement");
    assert_eq!(movement.network.as_str(), "sim-chain-e");
    let id = rutent_evidence::MovementId::new(movement.id.as_str()).expect("a movement id");
    let network = core
        .world()
        .network("sim-chain-e")
        .expect("the network")
        .clone();
    let rutent_evidence::Status::Final(moved) =
        rutent_evidence::PublicNetwork::status(&network, &id)
    else {
        panic!("the movement is final");
    };
    assert_eq!(moved.destination, ADDRESS);
}
