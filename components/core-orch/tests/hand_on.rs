//! The hand-on's rules over the order Core follows (`T-020-13`): the customer's new destination held to where the
//! value sits and to the state machine's rules, and the customer's new terms asked of policy only for a part that
//! waits for it. The flows on the running platform are in `conformance/scenarios/tests/hand_on.rs`.

#[path = "../../stm/tests/common/mod.rs"]
mod common;

use common::*;
use rutent_contracts::core_internal::{
    Actor, ActorKind, Authorisation, Destination, Role,
};
use rutent_contracts::provider_role::{Denomination, OuterNode};
use rutent_contracts::router_api::Timestamp;
use rutent_contracts::state_machine::{
    Observed, OperationKind, OrderState, Outcome, SegmentPlan, StateMachine,
    recalculation_keeps_destination,
};
use rutent_core_orch::hand_on::{Refused, destination_change, new_terms};
use rutent_stm::Stm;

/// An order whose final deposit failed at `ledger`, with no route left: the part waits for the customer there.
fn waiting() -> Run {
    let mut run = Run::opened("ord-1");
    run.run_segments("p1", 1, 2);
    assert_eq!(run.bind("p1", "p1-s3"), Outcome::Applied {});
    assert_eq!(run.fail("p1", "p1-s3"), Outcome::Applied {});
    assert_eq!(run.about_part("p1", no_route()), Outcome::Applied {});
    assert_eq!(run.part_state("p1"), OrderState::WaitingForCustomer);
    run
}

/// A route from `at` in USDC to another reserve's network.
fn to_another_destination_from(prefix: &str, at: &str) -> Vec<SegmentPlan> {
    vec![plan(
        &format!("{prefix}-s1"),
        "reserve-other",
        OperationKind::Deposit,
        inner(at, "USDC"),
        outer("other-chain", "USDC"),
    )]
}

fn cust() -> Actor {
    Actor {
        party: rutent_contracts::router_api::Id::new("cust-1").unwrap(),
        acts: rutent_contracts::router_api::Id::new("user-1").unwrap(),
        role: Role::Customer,
        kind: ActorKind::Person,
    }
}

fn rid(text: &str) -> rutent_contracts::router_api::Id {
    rutent_contracts::router_api::Id::new(text).unwrap()
}

/// The order's authorisation in force: to `reserve-wallet` on `chain`, in USDC.
fn in_force() -> Authorisation {
    Authorisation {
        authorisation: rid("auth-1"),
        order: rid("ord-1"),
        origin_reserve: rid("reserve-bank"),
        origin_account: Some(rid("acct-0")),
        destination: Destination {
            reserve: rid("reserve-wallet"),
            node: OuterNode {
                network: "chain".into(),
            },
            account: rid("acct-9"),
        },
        to_asset: Denomination {
            id: "USDC".into(),
            decimals: 6,
        },
        floor: None,
        parts: None,
        given_by: cust(),
        at: Timestamp::new("2026-11-10T09:00:00Z").unwrap(),
    }
}

/// The customer's new terms for `p1`: to `reserve-other` on `other-chain`.
fn to_another() -> Authorisation {
    let mut new = in_force();
    new.authorisation = rid("auth-2");
    new.destination = Destination {
        reserve: rid("reserve-other"),
        node: OuterNode {
            network: "other-chain".into(),
        },
        account: rid("acct-8"),
    };
    new.parts = Some(vec![rid("p1")]);
    new
}

/// The customer's new destination is a route from where the value sits, at the ledger the failed deposit left it
/// at, to another destination. One starting anywhere else is refused (`INV-4`); one keeping the destination is a
/// recalculation, not the customer's choice; and none is taken from a part that doesn't wait for the customer, or
/// waits for another reason than a failed final deposit (`INV-10`). The state machine takes the one Core takes, and
/// the part's destination is then the customer's; no recalculation reaches that destination.
// verifies: REQ-ORD-43, INV-10
#[test]
fn customers_new_destination_starts_from_where_the_value_sits() {
    let mut run = waiting();
    let part = run.part("p1").clone();
    let new = to_another_destination_from("p1-n", "ledger");
    assert_eq!(destination_change(&part, &new), Ok(()));
    assert_eq!(
        destination_change(&part, &to_another_destination_from("p1-n", "elsewhere")),
        Err(Refused::StartsAwayFromTheValue)
    );
    assert_eq!(
        destination_change(&part, &route_from_ledger("p1-n")),
        Err(Refused::KeepsDestination)
    );
    // A recalculation never reaches the new destination: only the customer's choice does.
    assert!(!recalculation_keeps_destination(&part, &new));
    let recalculation = run.input(Some("p1"), None, recalculated(new.clone()));
    assert_ne!(
        Stm::transition(run.order(), &recalculation).last_input.outcome,
        Outcome::Applied {}
    );
    assert_eq!(
        run.about_part("p1", destination_changed(new)),
        Outcome::Applied {}
    );
    let moved = run.part("p1");
    assert_eq!(moved.destination.provider.as_str(), "reserve-other");
    assert_eq!(
        moved.segments.last().map(|s| &s.from),
        Some(&inner("ledger", "USDC")),
        "the new route starts where the value sat"
    );

    // A part whose deposit failed but that still has a route to its destination doesn't wait: recalculation first.
    let mut failed = Run::opened("ord-2");
    failed.run_segments("p1", 1, 2);
    failed.bind("p1", "p1-s3");
    failed.fail("p1", "p1-s3");
    let part = failed.part("p1").clone();
    assert_eq!(
        destination_change(&part, &to_another_destination_from("p1-n", "ledger")),
        Err(Refused::NotWaiting(OrderState::FirstSegmentSettled))
    );

    // A part waiting at its floor, not at a failed deposit, keeps its destination (`INV-10`).
    let mut floor = Run::opened("ord-3");
    floor.run_segments("p1", 1, 2);
    assert_eq!(
        floor.about_part("p1", Observed::WaitingAtFloor {}),
        Outcome::Applied {}
    );
    let part = floor.part("p1").clone();
    assert_eq!(part.state, OrderState::WaitingForCustomer);
    assert_eq!(
        destination_change(&part, &to_another_destination_from("p1-n", "ledger")),
        Err(Refused::NoFailedDeposit)
    );
}

/// Policy holds who gives new terms, never when: Core asks it only for the customer's own new terms, under an id of
/// their own, for parts that wait for the customer, a new destination only where a final deposit failed, and a
/// new floor alone for any wait (`T-021-04`'s hand-off).
// verifies: REQ-AUZ-3, INV-10
#[test]
fn new_terms_are_asked_only_for_a_part_waiting_for_the_customer() {
    let customer = rid("cust-1");
    let waiting = waiting();
    let order = waiting.order();
    assert_eq!(new_terms(order, &in_force(), &to_another(), &customer), Ok(()));

    let mut operator = to_another();
    operator.given_by.role = Role::Operator;
    assert_eq!(
        new_terms(order, &in_force(), &operator, &customer),
        Err(Refused::NotTheCustomer)
    );
    let mut someone = to_another();
    someone.given_by.party = rid("cust-2");
    assert_eq!(
        new_terms(order, &in_force(), &someone, &customer),
        Err(Refused::NotTheCustomer)
    );
    let mut same_id = to_another();
    same_id.authorisation = rid("auth-1");
    assert_eq!(
        new_terms(order, &in_force(), &same_id, &customer),
        Err(Refused::SameId)
    );
    let mut restated = in_force();
    restated.authorisation = rid("auth-3");
    assert_eq!(
        new_terms(order, &in_force(), &restated, &customer),
        Err(Refused::NothingNew)
    );
    let mut origin = to_another();
    origin.origin_reserve = rid("reserve-other-bank");
    assert_eq!(
        new_terms(order, &in_force(), &origin, &customer),
        Err(Refused::ChangesOrigin)
    );
    let mut unknown = to_another();
    unknown.parts = Some(vec![rid("p1"), rid("p7")]);
    assert_eq!(
        new_terms(order, &in_force(), &unknown, &customer),
        Err(Refused::CoversAPartThatGoesOn(rid("p7")))
    );

    // Not from a part that goes on.
    let running = Run::opened("ord-1");
    assert_eq!(
        new_terms(running.order(), &in_force(), &to_another(), &customer),
        Err(Refused::CoversAPartThatGoesOn(rid("p1")))
    );

    // At its floor: a new floor, but no new destination.
    let mut floor = Run::opened("ord-1");
    floor.run_segments("p1", 1, 2);
    floor.about_part("p1", Observed::WaitingAtFloor {});
    assert_eq!(
        new_terms(floor.order(), &in_force(), &to_another(), &customer),
        Err(Refused::NoFailedDeposit)
    );
    let mut lower = in_force();
    lower.authorisation = rid("auth-4");
    lower.floor = rutent_contracts::router_api::PositiveAmount::new(
        rutent_contracts::router_api::Amount::new("100").unwrap(),
    );
    assert_eq!(
        new_terms(floor.order(), &in_force(), &lower, &customer),
        Ok(())
    );
}
