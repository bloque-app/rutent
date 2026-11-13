//! Routing scenarios over the assembled Core against the simulated providers (`T-024-03`): the objective changing the
//! route and the record explaining it (`AC-2`), a rejected dry run falling to another provider of the tuple
//! (`AC-3`), a provider below the operator's floor left out (`AC-33`), and a route with no positive arrival never
//! offered (`GUA-2`, `REQ-QTE-13`).
//!
//! The world is [`routing_support`]'s: the reserve `sim-reserve-a`, whose route runs from its bank into its ledger and
//! out again, and `sim-reserve-a-quick` beside it, serving the same tuples at another price, another time and
//! another grade of evidence, so its deposit has two providers.

mod routing_support;

use rutent_conformance::sim::{Act, Behaviour, Role, Spec};
use rutent_contracts::provider_role::EvidenceGrade;
use rutent_contracts::router_api::{DropReason, Objective, Router};
use rutent_core_orch::intake::{Status, Told};
use rutent_prov::catalogue::{Endpoint, tuples_of};
use rutent_router::RouteFinder;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Given, Scenario};
use rutent_scenarios::kit;
use routing_support::*;

const QUICK: &str = "sim-reserve-a-quick";
const DEAR: &str = "sim-reserve-dear";

/// The deposit of `reserve`'s ledger back to its bank, as the catalogue keys it.
fn deposit_tuple(core: &Core, provider: &str) -> rutent_prov::catalogue::TupleKey {
    let one = core
        .installed
        .iter()
        .find(|one| one.advertisement.statement().statement.provider == provider)
        .expect("installed");
    tuples_of(&one.advertisement.statement().statement)
        .into_iter()
        .find(|t| matches!(t.origin, Endpoint::Inner(_)))
        .expect("a deposit")
}

// verifies: AC-2, REQ-AUD-4, REQ-QTE-11
#[test]
fn changed_objective_can_change_the_route_and_the_record_explains_it_with_no_provider_call() {
    let (mut core, session) = core_with("AC-2", &[QUICK]);
    let (_, cheapest) = quoted(
        &mut core,
        request("req-cost", AMOUNT, &session, "lowest_total_cost"),
    );
    let (_, fastest) = quoted(
        &mut core,
        request("req-fast", AMOUNT, &session, "fastest_settlement"),
    );
    // The customer changes what it asks for, and the route changes with it: the deposit is the reserve that costs
    // least, or the one that settles first.
    assert_eq!(route_shown(&core, &cheapest), [kit::RESERVE, kit::RESERVE]);
    assert_eq!(route_shown(&core, &fastest), [kit::RESERVE, QUICK]);

    // The record explains the choice: it holds the candidate it chose and the other, dropped as outranked by it,
    // so a reader sees which route lost to which.
    let record = record_named(&core, &quote_of(&cheapest).expect("quoted").decision_record);
    let chosen = record.chosen().expect("chosen").candidate_id.clone();
    let other = record
        .candidates()
        .iter()
        .find(|c| providers_of(&record, c) == [kit::RESERVE, QUICK])
        .expect("the route through the other deposit was considered");
    assert_eq!(
        other.dropped,
        Some(DropReason::Outranked { by: chosen.clone() })
    );
    assert_eq!(record.request.objective, Objective::LowestTotalCost {});

    // Changing the objective is a decision over the record alone: the router over the snapshot the record holds
    // chooses the other route and the record's own again, calling no provider (nothing here reaches one).
    let calls = core.world().delivered().len();
    let books: Vec<usize> = [kit::RESERVE, QUICK]
        .iter()
        .map(|p| core.world().journal(p).len())
        .collect();
    let again = RouteFinder::route(&record.request, &record.snapshot, &record.bounds);
    assert_eq!(again, record, "the same record again from the record alone");
    let mut changed = record.request.clone();
    changed.objective = Objective::FastestSettlement {};
    let redecided = RouteFinder::route(&changed, &record.snapshot, &record.bounds);
    let chosen_again = redecided.chosen().expect("a route is chosen");
    assert_eq!(providers_of(&redecided, chosen_again), [kit::RESERVE, QUICK]);
    assert_eq!(core.world().delivered().len(), calls);
    let after: Vec<usize> = [kit::RESERVE, QUICK]
        .iter()
        .map(|p| core.world().journal(p).len())
        .collect();
    assert_eq!(after, books, "no provider's books moved");
}

// verifies: AC-3, REQ-CAT-14, REQ-QTE-11
#[test]
fn rejected_dry_run_falls_to_another_provider_of_the_tuple() {
    let (mut core, session) = core_with("AC-3", &[QUICK]);
    // The reserve that would win rejects its deposit's dry run: that option is dropped, and the other provider of
    // the tuple is used.
    core.world()
        .script_dry_run(kit::RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let (order, told) = quoted(
        &mut core,
        request("req-fall", AMOUNT, &session, "lowest_total_cost"),
    );
    assert_eq!(route_shown(&core, &told), [kit::RESERVE, QUICK]);
    let record = record_named(&core, &quote_of(&told).expect("quoted").decision_record);
    assert!(
        record
            .candidates()
            .iter()
            .any(|c| providers_of(&record, c) == [kit::RESERVE, kit::RESERVE]
                && matches!(c.dropped, Some(DropReason::DryRunRefused { .. }))),
        "the rejected option is dropped for the refusal"
    );
    assert!(statements_about(&core, &order) > 0);

    // With no other provider of the tuple, the same rejection leaves no route.
    let (mut alone, session) = core_with("AC-3", &[]);
    alone
        .world()
        .script_dry_run(kit::RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let (_, told) = quoted(
        &mut alone,
        request("req-alone", AMOUNT, &session, "lowest_total_cost"),
    );
    assert!(
        matches!(
            told,
            Told::Exists {
                status: Status::CantProceed { .. },
                ..
            }
        ),
        "{told:?}"
    );
}

// verifies: AC-33, REQ-ORD-39
#[test]
fn provider_below_the_floor_is_not_routed_through() {
    let (mut core, session) = core_with("AC-33", &[QUICK]);
    let tuple = deposit_tuple(&core, QUICK);
    // The operator wants a public record on the deposit. `sim-reserve-a` declares its own signature only.
    core.set_evidence_floor(tuple, EvidenceGrade::PublicRecord);
    let (_, told) = quoted(
        &mut core,
        request("req-floor", AMOUNT, &session, "lowest_total_cost"),
    );
    // It would win on cost; it isn't routed through, for the floor, and its rival is.
    assert_eq!(route_shown(&core, &told), [kit::RESERVE, QUICK]);
    let record = record_named(&core, &quote_of(&told).expect("quoted").decision_record);
    assert!(
        !record
            .candidates()
            .iter()
            .any(|c| providers_of(&record, c).get(1).map(String::as_str) == Some(kit::RESERVE)),
        "no candidate runs its deposit through it"
    );

    // With no other provider for the tuple, the floor leaves no route.
    let (mut alone, session) = core_with("AC-33", &[]);
    let tuple = deposit_tuple(&alone, kit::RESERVE);
    alone.set_evidence_floor(tuple, EvidenceGrade::PublicRecord);
    let (_, told) = quoted(
        &mut alone,
        request("req-floor-alone", AMOUNT, &session, "lowest_total_cost"),
    );
    assert!(
        matches!(
            told,
            Told::Exists {
                status: Status::CantProceed { .. },
                ..
            }
        ),
        "{told:?}"
    );
}

// verifies: GUA-2, REQ-QTE-13
#[test]
fn route_with_no_positive_arrival_is_not_offered() {
    // `sim-reserve-dear` takes more from a movement than the order moves, so nothing arrives.
    let given = Given {
        providers: vec![Spec::of(Role::Reserve, DEAR)],
        staging: Staged::AtOnce,
        deadline: std::time::Duration::from_secs(2),
        delivery: rutent_scenarios::assembly::machine::Delivery::Once,
    };
    let (mut core, _) = Scenario::new("no arrival", "AC-2", given).core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    let (order, told) = quoted(
        &mut core,
        request_at(DEAR, "req-nothing", AMOUNT, &session, "lowest_total_cost"),
    );
    assert!(
        matches!(
            told,
            Told::Exists {
                status: Status::CantProceed { .. },
                ..
            }
        ),
        "no route is offered: {told:?}"
    );
    // The shortlist kept nothing, so no provider was asked anything, and its record names none: the check is the
    // router's, at routing, over what the providers advertised.
    assert_eq!(statements_about(&core, &order), 0, "no question was asked");
    assert!(core.world().journal(DEAR).is_empty());
    let records = decisions(&core);
    assert!(records.iter().all(|r| r.chosen.is_none()));
    assert!(
        records
            .iter()
            .all(|r| r.candidates().iter().all(|c| c.dropped.is_some())),
        "every candidate dropped"
    );
}
