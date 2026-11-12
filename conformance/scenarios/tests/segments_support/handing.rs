//! A failed final deposit's resolution around the kit's Core (`T-020-13`): Core's hand-on record over the kit's log,
//! identifiers, segments and exposures, and the linked order's route as the kit runs it.

use std::rc::Rc;

use rutent_contracts::core_internal::{Entry, Receipt};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::{self as stm, OperationKind, PartPlan, SegmentPlan};
use rutent_core_orch::hand_on::Parts;

use super::{Core, General, PART, RESERVE, id, ledger};

/// Runs `with` over Core's hand-on parts: the kit's clock, log, identifiers, segments and exposures, and `stuck`.
pub fn hand_parts<T>(
    core: &mut Core,
    stuck: &mut rutent_core_orch::stuck::Stuck,
    with: impl FnOnce(&mut Parts<'_>) -> T,
) -> T {
    let mut general = General(Rc::clone(&core.log));
    let mut parts = Parts {
        clock: &*core.clock,
        log: &mut general,
        fresh: &mut core.ids,
        segments: &mut core.segments,
        exposures: &mut core.exposures,
        stuck,
    };
    with(&mut parts)
}

/// A reader of the kit's log, its entries as the log holds them whenever it is called.
pub fn log_entries(core: &Core) -> impl FnMut() -> Vec<Entry> + use<> {
    let log = Rc::clone(&core.log);
    move || {
        log.borrow()
            .entries
            .iter()
            .map(|(e, _)| e.clone())
            .collect()
    }
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
    route_from_the_ledger(world, clock, &linked_record(order), None, super::AMOUNT)
}

/// The router's choice of the reserve's deposit from its ledger to its bank, under `record_id`, its request following
/// the choice round of request `follows` where one is named, moving `amount`.
pub fn route_from_the_ledger(
    world: &mut super::World,
    clock: &super::TestClock,
    record_id: &Id,
    follows: Option<&str>,
    amount: &str,
) -> rutent_contracts::router_api::DecisionRecord {
    use rutent_contracts::router_api::{PositiveAmount, Request, Router};
    use rutent_core_orch::snapshot::{Ask, Asked, Clock, Exposures, Received, Releases, Snapshots};
    use rutent_prov::module::types::Handled;
    use rutent_router::RouteFinder;
    use serde_json::json;

    use super::{bank, bounds, price, tuple};

    let context = world.context();
    let bounds = bounds(&context);
    let deposit = tuple(RESERVE, ledger(RESERVE), bank(RESERVE));
    let prices = [price(deposit.clone())];
    let (exposures, releases) = (Exposures::new(), Releases::new());
    let record_id = record_id.clone();
    let request = |round: serde_json::Value, request_id: &str| -> Request {
        let mut request = json!({
            "request_id": request_id,
            "round": round,
            "from": ledger(RESERVE),
            "to": bank(RESERVE),
            "amount": amount,
            "objective": { "kind": "lowest_total_cost" },
        });
        if let Some(follows) = follows {
            request["follows"] = follows.into();
        }
        serde_json::from_value(request).unwrap()
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
        "amount": amount,
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
                rutent_contracts::router_api::Amount::new(amount).unwrap(),
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
    assert!(
        record.chosen().is_some(),
        "the router chose the linked route"
    );
    assert_eq!(record.record_id(), &record_id);
    record
}

/// Appends a decision record of the kit's order `order` under `record_id`, the one a route the kit's router chose for
/// it outside the rounds cites.
pub fn seed_record(core: &Core, order: &str, record_id: &Id) {
    use rutent_contracts::core_internal::{
        Outcome, RoutingRound, Through, TimeSource, VerifiedEntry, VerifiedKind, What, When,
    };
    let at = rutent_contracts::router_api::Timestamp::new(
        rutent_prov::catalogue::Instant(super::START)
            .rfc3339()
            .unwrap(),
    )
    .unwrap();
    core.log
        .borrow_mut()
        .take(
            &Entry::Verified(VerifiedEntry {
                kind: VerifiedKind::Verified,
                who: super::core(),
                what: What::DecisionRecord {
                    decision_record: record_id.clone(),
                    order: id(order),
                    round: RoutingRound::Choice,
                    record: super::digest(&"cf".repeat(32)),
                    previous: None,
                    shortlisted: None,
                    verifiers: Vec::new(),
                },
                when: When {
                    at,
                    source: TimeSource::CoreClock,
                },
                outcome: Outcome::Observed {},
            }),
            Through::General,
        )
        .unwrap();
}

/// Appends the customer's acceptance `acceptance` of `order` under `authorisation`, as the kit's automatic deposit's
/// (the kit's order holds one acceptance basis, `order-holds-one-acceptance-basis`): the acceptance of new terms that
/// each move under them rests on (`move-rests-on-its-orders-acceptance`).
pub fn seed_acceptance(core: &Core, order: &str, acceptance: &str, authorisation: &Id) {
    use rutent_contracts::core_internal::{
        AcceptanceBasis, Outcome, Parties, Through, TimeSource, VerifiedEntry, VerifiedKind, What,
        When,
    };
    let at = rutent_contracts::router_api::Timestamp::new(
        rutent_prov::catalogue::Instant(super::START)
            .rfc3339()
            .unwrap(),
    )
    .unwrap();
    let requester = super::customer().party;
    core.log
        .borrow_mut()
        .take(
            &Entry::Verified(VerifiedEntry {
                kind: VerifiedKind::Verified,
                who: super::core(),
                what: What::Acceptance {
                    acceptance: id(acceptance),
                    order: id(order),
                    request: id(&format!("req-{acceptance}")),
                    quote: super::digest(&"ab".repeat(32)),
                    authorisation: authorisation.clone(),
                    basis: AcceptanceBasis::AutomaticDeposit,
                    parties: Parties {
                        requester: requester.clone(),
                        beneficiary: requester.clone(),
                        settlement_account_at: None,
                        payer: requester,
                    },
                    origin_reserve: id(RESERVE),
                    yeses: Vec::new(),
                    session: None,
                },
                when: When {
                    at,
                    source: TimeSource::CoreClock,
                },
                outcome: Outcome::Observed {},
            }),
            Through::General,
        )
        .unwrap();
}
