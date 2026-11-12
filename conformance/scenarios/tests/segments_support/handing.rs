//! A failed final deposit's resolution around the kit's Core (`T-020-13`): Core's hand-on record over the kit's log,
//! identifiers, segments and exposures, and the linked order's route as the kit runs it.

use std::rc::Rc;

use rutent_contracts::core_internal::{Entry, Receipt};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::{self as stm, OperationKind, PartPlan, SegmentPlan};
use rutent_core_orch::hand_on::Parts;

use super::{Core, General, PART, RESERVE, id, ledger};

/// Runs `with` over Core's hand-on parts: the kit's clock, log, identifiers, segments and exposures.
pub fn hand_parts<T>(core: &mut Core, with: impl FnOnce(&mut Parts<'_>) -> T) -> T {
    let mut general = General(Rc::clone(&core.log));
    let mut parts = Parts {
        clock: &*core.clock,
        log: &mut general,
        fresh: &mut core.ids,
        segments: &mut core.segments,
        exposures: &mut core.exposures,
    };
    with(&mut parts)
}

/// The kit's log, each entry with its receipt, as the log holds them.
pub fn recorded_with_receipts(core: &Core) -> Vec<(Receipt, Entry)> {
    let log = core.log.borrow();
    log.entries
        .iter()
        .enumerate()
        .map(|(i, (e, _))| (log.receipt_at(i), e.clone()))
        .collect()
}

/// The linked order's one part: the kit's deposit, from `from` to the reserve's bank, under `segment`, with no
/// withdrawal before it, as a route handed on starts (`REQ-ORD-16`).
pub fn linked_plan(core: &Core, from: serde_json::Value, segment: &str) -> Vec<PartPlan> {
    let operations = &core.record.chosen().unwrap().parts[0].operations;
    let deposit = operations.last();
    assert_eq!(deposit.kind, OperationKind::Deposit);
    vec![PartPlan {
        part_id: stm::Id::new(PART).unwrap(),
        segments: vec![SegmentPlan {
            segment_id: stm::Id::new(segment).unwrap(),
            provider: stm::Id::new(deposit.provider.as_str()).unwrap(),
            kind: OperationKind::Deposit,
            from: serde_json::from_value(from).unwrap(),
            to: deposit.to.clone(),
        }],
    }]
}

/// The linked order's part starting where the kit's order's value rests: the reserve's ledger.
pub fn from_the_ledger(core: &Core, segment: &str) -> Vec<PartPlan> {
    linked_plan(core, ledger(RESERVE), segment)
}

/// The kit's linked order's decision record id.
pub fn linked_record(order: &str) -> Id {
    id(&format!("dr-linked-{order}"))
}

/// The router's choice of a route handed on for the kit's linked order: from the reserve's ledger, where the kit's
/// order's value rests, to its bank, by its deposit alone, over the reserve's dry run of it. Its record's id is the
/// one the linked order's `route_chosen` cites ([`linked_record`]'s, for `order`).
pub fn linked_route(
    world: &mut super::World,
    clock: &super::TestClock,
    order: &str,
) -> rutent_contracts::router_api::DecisionRecord {
    use rutent_contracts::router_api::{PositiveAmount, Request, Router};
    use rutent_core_orch::snapshot::{Ask, Asked, Clock, Exposures, Received, Releases, Snapshots};
    use rutent_prov::module::types::Handled;
    use rutent_router::RouteFinder;
    use serde_json::json;

    use super::{AMOUNT, bank, bounds, price, tuple};

    let context = world.context();
    let bounds = bounds(&context);
    let deposit = tuple(RESERVE, ledger(RESERVE), bank(RESERVE));
    let prices = [price(deposit.clone())];
    let (exposures, releases) = (Exposures::new(), Releases::new());
    let record_id = linked_record(order);
    let request = |round: serde_json::Value, request_id: &str| -> Request {
        serde_json::from_value(json!({
            "request_id": request_id,
            "round": round,
            "from": ledger(RESERVE),
            "to": bank(RESERVE),
            "amount": AMOUNT,
            "objective": { "kind": "lowest_total_cost" },
        }))
        .unwrap()
    };
    let started = clock.now().seconds;
    clock.set(started - 7_200);
    let shortlist = Snapshots::new(&world.catalogue, clock, &exposures, &releases)
        .form(&Ask {
            snapshot_id: "snap-linked-1",
            bounds: &bounds,
            prices: &prices,
            statements: &[],
        })
        .expect("a shortlist snapshot");
    let shortlist = RouteFinder::route(
        &request(json!({ "kind": "shortlist" }), "req-linked-shortlist"),
        &shortlist.snapshot,
        &bounds,
    );
    let statement = json!({
        "kind": "deposit",
        "transaction": "tx-dry-linked",
        "provider": RESERVE,
        "asset": "COP",
        "amount": AMOUNT,
        "dry_run": true,
        "inputs": { "beneficiary.bank_account": "acct-2" },
        "origin": ledger(RESERVE)["node"].clone(),
        "destination": bank(RESERVE)["node"].clone(),
    });
    let operation = rutent_conformance::sim::operation(&context, statement);
    let bytes = match world.fleet.handle(RESERVE, &operation) {
        Handled::Answered(result) => serde_json::to_vec(&*result).unwrap(),
        Handled::Refused(why) => panic!("the dry run was refused: {why:?}"),
    };
    let statements = vec![Received {
        tuple: deposit,
        asked: Asked::DryRun {
            amount_in: PositiveAmount::new(
                rutent_contracts::router_api::Amount::new(AMOUNT).unwrap(),
            )
            .unwrap(),
        },
        bytes,
    }];
    let _ = world.fleet.platform.take_events();
    clock.set(started);
    let choice = Snapshots::new(&world.catalogue, clock, &exposures, &releases)
        .form(&Ask {
            snapshot_id: "snap-linked-2",
            bounds: &bounds,
            prices: &prices,
            statements: &statements,
        })
        .expect("a choice snapshot");
    let record = RouteFinder::route(
        &request(
            json!({ "kind": "choice", "refines": {
                "request_id": "req-linked-shortlist",
                "taken_at": shortlist.snapshot.taken_at,
            }}),
            record_id.as_str(),
        ),
        &choice.snapshot,
        &bounds,
    );
    assert!(record.chosen().is_some(), "the router chose the linked route");
    assert_eq!(record.record_id(), &record_id);
    record
}
