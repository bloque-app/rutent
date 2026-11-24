//! The production stuck-resolution mover (`T-020-16`): a decided resolution carried out on the running platform with
//! simulated providers, in place of the tests' `Moves`. A deposit from the node where the part's value rests to the
//! order's destination or back to its origin account, handed the provider as an operation under the resolution's own
//! transaction, its answer judged as a segment's is and kept with what the platform and the proxy handed over, and
//! the outcome answered only from it: `Done` for a result that counts and says it settled, `Failed` only where
//! nothing passed out or the provider says it failed, `Unknown` for the rest. Over `T-020-08`'s kit and the real
//! log's rules (every history is replayed through `admit_entry`); the engine deciding each step is the scenario
//! at the end of `assembly_stuck.rs`'s.

mod segments_support;

use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::calls::StuckOrders;
use rutent_contracts::core_internal::{
    Action, Decided, Entry, Outcome, Receipt, RefusalClass, Resolution, What,
};
use rutent_contracts::provider_role::{Denomination, Operation as ProviderOperation, Statement};
use rutent_contracts::router_api::{Id, PositiveAmount};
use rutent_contracts::state_machine::{Observed, OrderState, ResolvedTo};
use rutent_core_orch::recalc::Recalculation;
use rutent_core_orch::segments::SegmentOutcome;
use rutent_core_orch::snapshot::Clock;
use rutent_core_orch::stuck::{
    Confirming, Deciding, Declining, Moved, Mover, Moving, NoRouteLeft, NotTaken, ResolveError,
    Resolved, Stated, Stuck,
};
use segments_support::rerouting::Rerouting;
use segments_support::resolving::{
    PermitsEveryStep, destination, follow, on_parts, on_platform, origin, staff,
};
use segments_support::*;

fn cop_unit() -> Denomination {
    Denomination {
        id: "COP".into(),
        decimals: 2,
    }
}

fn count(core: &Core, pick: impl Fn(&What) -> bool) -> usize {
    core.log.borrow().count(pick)
}

fn is_resolve_stuck(what: &What) -> bool {
    matches!(
        what,
        What::Effect {
            action: Action::ResolveStuck { .. },
            ..
        }
    )
}

fn is_hand_over(what: &What) -> bool {
    matches!(what, What::HandOver { .. })
}

/// The kit's order with its withdrawal settled and its deposit never begun, then recalculated from the reserve's
/// ledger, where its value sits, and offered no route: the reserve refuses every dry run of the deposit. Recorded,
/// the part is stuck, its value resting at the ledger.
fn stuck_by_no_route() -> (World, Core, Stuck) {
    stuck_by_no_route_in(World::two_parties())
}

/// [`stuck_by_no_route`] at one reserve, which runs the whole route: for a delivery on to the destination, which
/// only the reserve holding the value can make, and which no other party need confirm.
fn stuck_by_no_route_at_one_reserve() -> (World, Core, Stuck) {
    stuck_by_no_route_in(World::of(&[RESERVE, OTHER_RESERVE]))
}

/// The kit's order stuck by no route, in `world`.
fn stuck_by_no_route_in(mut world: World) -> (World, Core, Stuck) {
    let mut core = Core::over(&mut world);
    // The log attributes each statement, as the real one does: its receipt is of the entry it recorded.
    core.log.borrow_mut().attribute_from_now();
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.refuse_begin = Some(1);
    let ran = core.run_over(&world.catalogue, &mut watched);
    drop(watched);
    let rutent_core_orch::segments::Ran::Segments(done) = &ran else {
        panic!("the part ran: {ran:?}");
    };
    assert!(matches!(done[0].outcome, SegmentOutcome::Settled { .. }));
    assert!(matches!(done[1].outcome, SegmentOutcome::NotHandedOver(_)));
    world
        .fleet
        .world()
        .script_dry_run(last_reserve(), "deposit", Behaviour::act(Act::Refuse));
    let mut rerouting = Rerouting::of(&core);
    let outcome = rerouting
        .recalculate(&mut core, &mut world)
        .expect("the recalculation runs");
    assert!(
        matches!(outcome, Recalculation::NoRoute { .. }),
        "{outcome:?}"
    );
    let mut stuck = Stuck::new();
    let (order, part, last) = (
        id(&core.named.order),
        id(PART),
        core.record.record_id().clone(),
    );
    let actor = segments_support::core();
    let recorded = on_parts(&mut core, |parts| {
        stuck.no_route(
            parts,
            NoRouteLeft {
                order: &order,
                part: &part,
                outcome: &outcome,
                last_record: &last,
                core: &actor,
            },
        )
    })
    .expect("recorded");
    assert_eq!(recorded.state, OrderState::Stuck);
    follow(&core, &mut stuck).expect("its ends are Core's");
    (world, core, stuck)
}

fn to_origin(amount: &str) -> Resolution {
    Resolution::Deliver {
        by: id(origin_reserve()),
        to: origin(),
        asset: cop_unit(),
        amount: PositiveAmount::new(amount).unwrap(),
    }
}

/// The stuck value delivered on to the order's destination, by the reserve that serves it: the route's last
/// reserve, which deposits from the ledger the value rests at (`T-020-39`'s two-party world).
fn to_destination(core: &Core, amount: &str) -> Resolution {
    Resolution::Deliver {
        by: id(last_reserve()),
        to: destination(core),
        asset: cop_unit(),
        amount: PositiveAmount::new(amount).unwrap(),
    }
}

fn evidence() -> Vec<rutent_contracts::module_interface::Digest> {
    vec![digest(&"ab".repeat(32))]
}

fn decide(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut dyn StuckOrders,
    decider: &rutent_contracts::core_internal::Actor,
    resolution: &Resolution,
) -> Result<rutent_contracts::core_internal::Decision, ResolveError> {
    let (order, part) = (id(&core.named.order), id(PART));
    let evidence = evidence();
    let history = core.recorded();
    on_parts(core, |parts| {
        stuck.decide(
            parts,
            policy,
            Deciding {
                order: &order,
                part: &part,
                decider,
                resolution,
                evidence: &evidence,
            },
            &history,
        )
    })
}

fn decided(decision: &rutent_contracts::core_internal::Decision) -> Decided {
    Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    }
}

/// What a move step did on the platform: its answer, and each operation the mover handed a provider.
struct Delivered {
    moved: Result<Moved, ResolveError>,
    carried: Vec<(Id, Statement<ProviderOperation>)>,
}

/// Asks the move step of the kit's order, by `by`, carrying out `carries_out`, with the production mover over the
/// world's platform.
fn deliver(
    core: &mut Core,
    world: &mut World,
    stuck: &mut Stuck,
    mover: &mut Mover,
    policy: &mut dyn StuckOrders,
    carries_out: &Decided,
    by: &rutent_contracts::core_internal::Actor,
) -> Delivered {
    let order = id(&core.named.order);
    let actor = segments_support::core();
    let mut watched = Watched::new(&mut world.fleet.platform);
    let moved = on_platform(
        core,
        &world.catalogue,
        &mut watched,
        mover,
        |parts, delivering| {
            stuck.carry_out(
                parts,
                policy,
                delivering,
                Moving {
                    order: &order,
                    carries_out,
                    mover: by,
                    core: &actor,
                },
            )
        },
    );
    Delivered {
        moved,
        carried: watched.carried.clone(),
    }
}

/// The transaction the kit's stuck decision named.
fn decided_transaction(core: &Core) -> Id {
    core.log
        .borrow()
        .verified()
        .iter()
        .find_map(|v| match &v.what {
            What::StuckDecide { request, .. } => Some(request.transaction.clone()),
            _ => None,
        })
        .expect("a stuck decision")
}

/// The stated entries the log holds about the current order under `transaction`, each with its claimed maker's `acts`
/// and its shape, and the segment it names: the operator's question first, then the providers' answers.
fn stated_under(core: &Core, transaction: &Id) -> Vec<(String, String, Option<Id>)> {
    core.log
        .borrow()
        .entries
        .iter()
        .filter_map(|(e, _)| match e {
            Entry::Stated(s)
                if s.about
                    .as_ref()
                    .is_some_and(|a| a.transaction.as_ref() == Some(transaction)) =>
            {
                let claimed = match &s.attribution {
                    rutent_contracts::core_internal::Attribution::Unverified { claimed_by } => {
                        claimed_by.acts.as_str().to_owned()
                    }
                    other => format!("{other:?}"),
                };
                Some((
                    claimed,
                    s.what.statement.as_str().to_owned(),
                    s.about.as_ref().and_then(|a| a.segment.clone()),
                ))
            }
            _ => None,
        })
        .collect()
}

fn resolved_to(core: &Core) -> Vec<ResolvedTo> {
    core.log
        .borrow()
        .inputs()
        .iter()
        .filter_map(|i| match &i.observed {
            Observed::StuckResolved { to, .. } => Some(*to),
            _ => None,
        })
        .collect()
}

/// The results the log holds for `effect`, in order.
fn results_of(core: &Core, effect: &Receipt) -> Vec<Outcome> {
    core.log
        .borrow()
        .verified()
        .iter()
        .filter(|v| matches!(&v.what, What::Result { effect: e } if e == effect))
        .map(|v| v.outcome)
        .collect()
}

/// The operation the mover handed the provider, as JSON.
fn sent(delivered: &Delivered) -> serde_json::Value {
    assert_eq!(
        delivered.carried.len(),
        1,
        "one operation reaches the provider"
    );
    serde_json::to_value(&delivered.carried[0].1.statement).unwrap()
}

// The tests -----------------------------------------------------------------------------------------------------

/// A stuck part's value is delivered to the order's authorised destination on a decision of one party and a move of
/// another: the production mover hands the reserve a deposit from the ledger node the value rests at to the
/// destination's bank, under the transaction the decision named, with the destination's account as its input, and
/// the move answers `Done` from the reserve's result. Before it reaches the provider the operator's question is in
/// the log, naming the order and the transaction and no segment; the reserve's answer follows it as the reserve's
/// stated entry, with the hand-over of what the platform saw, before the result. The part is resolved at its
/// destination, once.
// verifies: REQ-AUZ-4, REQ-ORD-28
#[test]
fn decided_delivery_to_the_destination_is_carried_out_on_the_platform() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route_at_one_reserve();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let resolution = to_destination(&core, AMOUNT);
    let permitted = decide(&mut core, &mut stuck, &mut policy, &alice, &resolution).expect("asked");
    let handled = world.fleet.platform.operations_received(origin_reserve());
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    let moved = delivered.moved.clone().expect("moved");
    let Moved::CarriedOut {
        effect,
        resolved,
        result,
    } = moved
    else {
        panic!("carried out: {moved:?}");
    };
    assert_eq!(resolved, Resolved::Done);
    assert!(result.is_some());
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled + 1,
        "the reserve is handed one operation"
    );
    let transaction = decided_transaction(&core);
    let operation = sent(&delivered);
    assert_eq!(operation["kind"], "deposit");
    assert_eq!(operation["provider"], origin_reserve());
    assert_eq!(operation["transaction"], transaction.as_str());
    assert_eq!(operation["origin"], format!("{}-ledger", origin_reserve()));
    assert_eq!(
        operation["destination"]["network"],
        format!("{}-bank", origin_reserve())
    );
    assert_eq!(operation["asset"], "COP");
    assert_eq!(operation["amount"], AMOUNT);
    assert_eq!(operation["dry_run"], false);
    assert_eq!(
        operation["inputs"],
        serde_json::json!({ "beneficiary.bank_account": destination(&core).account.as_str() })
    );
    // The question, then the answer, each once, about the order and the transaction, naming no segment.
    let stated = stated_under(&core, &transaction);
    assert_eq!(
        stated
            .iter()
            .map(|(m, shape, segment)| (m.as_str(), shape.as_str(), segment.is_none()))
            .collect::<Vec<_>>(),
        vec![
            (
                core.operator.acts.as_str(),
                rutent_contracts::provider_role::OPERATION_SHAPE,
                true
            ),
            (
                origin_reserve(),
                rutent_contracts::provider_role::RESULT_SHAPE,
                true
            ),
        ]
    );
    assert!(
        count(&core, is_hand_over) > 0,
        "what the platform saw is recorded"
    );
    assert_eq!(results_of(&core, &effect), vec![Outcome::Done {}]);
    assert_eq!(count(&core, is_resolve_stuck), 1);
    assert_eq!(resolved_to(&core), vec![ResolvedTo::Destination]);
    assert!(!stuck.is_stuck(&id(ORDER), &id(PART)));
    assert!(core.exposures.stuck().is_empty());
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The same resolution back to the origin account: the deposit goes to the account the payer's value came from, at
/// the origin's bank, and the part is resolved at its origin.
// verifies: REQ-AUZ-4, INV-2
#[test]
fn decided_delivery_back_to_the_origin_account_is_carried_out_on_the_platform() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    assert!(
        matches!(
            delivered.moved,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            })
        ),
        "{:?}",
        delivered.moved
    );
    let operation = sent(&delivered);
    assert_eq!(operation["kind"], "deposit");
    assert_eq!(operation["origin"], format!("{}-ledger", origin_reserve()));
    assert_eq!(
        operation["destination"]["network"],
        origin().node.network.as_str()
    );
    assert_eq!(operation["amount"], AMOUNT);
    assert_eq!(
        operation["inputs"],
        serde_json::json!({ "beneficiary.bank_account": origin().account.as_str() })
    );
    assert_eq!(resolved_to(&core), vec![ResolvedTo::Origin]);
    assert!(core.exposures.stuck().is_empty());
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Nothing is handed the provider unless policy permitted the move, and the move is another actor's than the
/// decision's: a move policy refuses records no effect and hands nothing over; the decider moving its own decision
/// is refused before policy is asked, by party and by login; a decision policy never permitted backs no move.
// verifies: REQ-AUZ-4, AC-6
#[test]
fn resolution_is_carried_out_only_on_a_move_policy_permitted() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, alice_again, bob) = (
        staff("desk-a", "alice"),
        staff("desk-a", "alice-second-login"),
        staff("desk-b", "bob"),
    );
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let handled = world.fleet.platform.operations_received(origin_reserve());
    let asked = policy.asked;
    for (by, why) in [
        (&alice_again, ResolveError::SameParty),
        (&staff("desk-b", "alice"), ResolveError::SameLogin),
    ] {
        let delivered = deliver(
            &mut core,
            &mut world,
            &mut stuck,
            &mut mover,
            &mut policy,
            &decided(&permitted),
            by,
        );
        assert_eq!(delivered.moved.map(|_| ()), Err(why));
    }
    assert_eq!(policy.asked, asked, "policy isn't asked for a refused move");
    let forged = Decided {
        request: permitted.request.clone(),
        decision: id("forged-decision"),
    };
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &forged,
        &bob,
    );
    assert_eq!(delivered.moved.map(|_| ()), Err(ResolveError::NoDecision));
    // Policy refuses the move: nothing is recorded as an effect, and nothing is handed over.
    policy.refuses_moves = Some(RefusalClass::NotPermitted);
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    assert!(
        matches!(delivered.moved, Ok(Moved::NotPermitted(_))),
        "{:?}",
        delivered.moved
    );
    assert!(delivered.carried.is_empty());
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled,
        "the provider is handed nothing"
    );
    assert_eq!(count(&core, is_resolve_stuck), 0, "no effect is recorded");
    assert!(stated_under(&core, &decided_transaction(&core)).is_empty());
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    // The same move, once policy permits it, is carried out.
    policy.refuses_moves = None;
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    assert!(
        matches!(
            delivered.moved,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            })
        ),
        "{:?}",
        delivered.moved
    );
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled + 1
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A move whose outcome is unknown, the reserve reporting it can't tell: the effect's result is `unanswered`, the
/// part stays held and nothing more is decided for it. The reserve's later statement for the move's transaction is
/// evidence Core checks (attributable, a result, naming the transaction, judged against the operation it signed): the
/// mover's reporter reports `Done` on it and another actor, of another party, confirms on another maker's
/// statement; only then is the effect's final result recorded, and the part resolved, once. A statement of another
/// transaction, or one a key never published signed, is not taken.
// verifies: REQ-ORD-1
#[test]
fn move_left_unknown_takes_its_known_outcome_from_the_providers_stated_result() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob, carol) = (
        staff("desk-a", "alice"),
        staff("desk-b", "bob"),
        staff("desk-c", "carol"),
    );
    world
        .fleet
        .world()
        .script_for(origin_reserve(), "deposit", Behaviour::act(Act::Unknown));
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let handled = world.fleet.platform.operations_received(origin_reserve());
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    let Ok(Moved::CarriedOut {
        effect,
        resolved: Resolved::Unknown,
        result: Some(_),
    }) = delivered.moved.clone()
    else {
        panic!("unknown: {:?}", delivered.moved);
    };
    assert_eq!(results_of(&core, &effect), vec![Outcome::Unanswered {}]);
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &alice,
            &to_origin(AMOUNT)
        )
        .map(|_| ()),
        Err(ResolveError::OutcomeUnknown)
    );
    // The reserve's later result for the transaction: its answer, with the outcome settled.
    let transaction = decided_transaction(&core);
    let late = settled_after_unknown(&core, &transaction);
    let carrying_core = segments_support::core();
    let take = |core: &mut Core, world: &mut World, mover: &mut Mover, bytes: &[u8]| {
        let (order, part) = (id(&core.named.order), id(PART));
        let mut watched = Watched::new(&mut world.fleet.platform);
        on_platform(
            core,
            &world.catalogue,
            &mut watched,
            mover,
            |parts, delivering| {
                delivering.statement(
                    parts,
                    &rutent_core_orch::stuck::Carrying {
                        order: &order,
                        part: &part,
                        core: &carrying_core,
                        again: false,
                    },
                    &effect,
                    bytes,
                )
            },
        )
    };
    // Not taken: a statement of another transaction; one signed by a key its maker never published.
    let mut other = late.clone();
    other["statement"]["transaction"] = "tx-another".into();
    resign(&mut other, origin_reserve());
    assert_eq!(
        take(
            &mut core,
            &mut world,
            &mut mover,
            &serde_json::to_vec(&other).unwrap()
        ),
        Err(NotTaken::OfAnotherTransaction)
    );
    let mut forged = late.clone();
    forged["signature"]["value"] = rutent_conformance::signing::base64url(&[7u8; 64]).into();
    assert_eq!(
        take(
            &mut core,
            &mut world,
            &mut mover,
            &serde_json::to_vec(&forged).unwrap()
        ),
        Err(NotTaken::Unattributable)
    );
    let bytes = serde_json::to_vec(&late).unwrap();
    let stated = take(&mut core, &mut world, &mut mover, &bytes).expect("taken");
    assert_eq!(stated.outcome(), Some(Resolved::Done));
    let again = take(&mut core, &mut world, &mut mover, &bytes).expect("taken again");
    assert_eq!(again, stated, "a statement tried again is one entry");
    let another = result_without_a_question(&mut core, second_reserve(), transaction.as_str());
    let reported = report_and_confirm(
        &mut core,
        &mut stuck,
        &mut policy,
        &permitted,
        (&bob, &carol),
        (&stated, &another),
        Resolved::Done,
    );
    assert!(matches!(
        reported,
        Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        }
    ));
    assert_eq!(
        results_of(&core, &effect),
        vec![Outcome::Unanswered {}, Outcome::Done {}]
    );
    assert_eq!(resolved_to(&core), vec![ResolvedTo::Origin]);
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled + 1,
        "the provider is handed the operation once, never again for its outcome"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// An answer to the move's operation, as the reserve's status would settle it now: the answer the mover kept, its
/// outcome the settled one a withdrawal's result states, made at Core's clock now and signed with the reserve's
/// statements key.
fn settled_after_unknown(core: &Core, transaction: &Id) -> serde_json::Value {
    let kept: Vec<serde_json::Value> = core
        .store
        .kept
        .borrow()
        .iter()
        .filter_map(|b| serde_json::from_slice(b).ok())
        .collect();
    let mut answer = kept
        .iter()
        .rev()
        .find(|v| {
            v["statement"]["transaction"] == transaction.as_str()
                && v["shape"] == rutent_contracts::provider_role::RESULT_SHAPE
        })
        .expect("the reserve's answer is kept")
        .clone();
    assert_eq!(answer["statement"]["outcome"]["status"], "unknown");
    let settled = kept
        .iter()
        .find(|v| {
            v["statement"]["kind"] == "withdrawal"
                && v["statement"]["outcome"]["status"] == "succeeded"
        })
        .expect("a settled withdrawal")["statement"]["outcome"]
        .clone();
    answer["statement"]["outcome"] = settled;
    answer["made_at"] = core.clock.now().timestamp().unwrap().as_str().into();
    resign(&mut answer, origin_reserve());
    answer
}

fn resign(envelope: &mut serde_json::Value, maker: &str) {
    rutent_conformance::signing::sign_value(envelope, &statements_key_of(maker));
}

/// The mover's `bob` reports `outcome` on `evidence.0`, and `carol` confirms it on `evidence.1`.
fn report_and_confirm(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut PermitsEveryStep,
    decision: &rutent_contracts::core_internal::Decision,
    (reporter, confirmer): (
        &rutent_contracts::core_internal::Actor,
        &rutent_contracts::core_internal::Actor,
    ),
    evidence: (&rutent_core_orch::stuck::Stated, &Receipt),
    outcome: Resolved,
) -> Moved {
    let (order, carries_out, actor) = (
        id(&core.named.order),
        decided(decision),
        segments_support::core(),
    );
    let history = core.recorded();
    let reported = on_parts(core, |parts| {
        stuck.known_outcome(
            parts,
            policy,
            Moving {
                order: &order,
                carries_out: &carries_out,
                mover: reporter,
                core: &actor,
            },
            outcome,
            evidence.0,
            &history,
        )
    })
    .expect("reported");
    let Moved::AwaitsConfirmation { report } = reported else {
        panic!("awaits its confirmation: {reported:?}");
    };
    let history = core.recorded();
    on_parts(core, |parts| {
        stuck.confirm_outcome(
            parts,
            policy,
            rutent_core_orch::stuck::Confirming {
                order: &order,
                carries_out: &carries_out,
                confirmer,
                outcome,
                report,
                reported_with: evidence.0.receipt(),
                evidence: &Stated::for_tests(evidence.1.clone(), None),
                core: &actor,
            },
            &history,
        )
    })
    .expect("confirmed")
}

/// An effect acted on once is never acted on again, however often an append of the move was retried: the log can't
/// take the question, then the hand-over of what the platform saw, then the answer, then the result, each time on
/// every try; the move goes on from where it stopped each time, the question, the answer and every hand-over entry
/// are each one entry, and the provider is handed the operation exactly once.
// verifies: INV-8
#[test]
fn resolution_acted_on_once_is_not_acted_on_again_after_a_retried_append() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let handled = world.fleet.platform.operations_received(origin_reserve());
    let failing = |core: &Core, on: On| {
        for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
            core.log
                .borrow_mut()
                .faults
                .push_back((on, Fault::Unavailable));
        }
    };
    let try_move = |core: &mut Core,
                    world: &mut World,
                    stuck: &mut Stuck,
                    mover: &mut Mover,
                    policy: &mut PermitsEveryStep| {
        deliver(
            core,
            world,
            stuck,
            mover,
            policy,
            &decided(&permitted),
            &bob,
        )
    };
    // The effect itself can't be appended: nothing is handed over.
    failing(&core, On::Effect);
    let tried = try_move(&mut core, &mut world, &mut stuck, &mut mover, &mut policy);
    assert!(matches!(tried.moved, Err(ResolveError::Append(_))));
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled
    );
    // The question can't be appended: still nothing is handed over.
    failing(&core, On::StatedOperation);
    let tried = try_move(&mut core, &mut world, &mut stuck, &mut mover, &mut policy);
    assert!(
        matches!(tried.moved, Err(ResolveError::Append(_))),
        "{:?}",
        tried.moved
    );
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled,
        "nothing is handed over before the question is on record"
    );
    // What the platform saw can't be appended: the operation was handed over, once.
    failing(&core, On::HandOver);
    let tried = try_move(&mut core, &mut world, &mut stuck, &mut mover, &mut policy);
    assert!(
        matches!(tried.moved, Err(ResolveError::Append(_))),
        "{:?}",
        tried.moved
    );
    assert_eq!(tried.carried.len(), 1);
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled + 1
    );
    // The answer can't be appended: it is kept, and nothing is handed over again.
    failing(&core, On::StatedResult);
    let tried = try_move(&mut core, &mut world, &mut stuck, &mut mover, &mut policy);
    assert!(
        matches!(tried.moved, Err(ResolveError::Append(_))),
        "{:?}",
        tried.moved
    );
    assert!(tried.carried.is_empty(), "never handed over again");
    // The result can't be appended.
    failing(&core, On::LaterResult);
    let tried = try_move(&mut core, &mut world, &mut stuck, &mut mover, &mut policy);
    assert!(
        matches!(tried.moved, Err(ResolveError::Append(_))),
        "{:?}",
        tried.moved
    );
    assert!(tried.carried.is_empty());
    let tried = try_move(&mut core, &mut world, &mut stuck, &mut mover, &mut policy);
    assert!(
        matches!(
            tried.moved,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            })
        ),
        "{:?}",
        tried.moved
    );
    assert!(tried.carried.is_empty());
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled + 1,
        "the provider is handed the operation exactly once"
    );
    let transaction = decided_transaction(&core);
    assert_eq!(
        stated_under(&core, &transaction).len(),
        2,
        "a question and an answer, once each"
    );
    assert_eq!(count(&core, is_resolve_stuck), 1);
    assert_eq!(resolved_to(&core), vec![ResolvedTo::Origin]);
    // A fresh mover and a fresh record of the stuck order, as after a restart, move nothing more.
    let mut restarted = Stuck::new();
    follow(&core, &mut restarted).expect("followed again");
    assert_eq!(
        decide(
            &mut core,
            &mut restarted,
            &mut policy,
            &alice,
            &to_origin(AMOUNT)
        )
        .map(|_| ()),
        Err(ResolveError::NotStuck)
    );
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled + 1
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A move Core itself saw fail, because the platform won't begin the provider's segment (nothing passed out), leaves
/// the value held and the part stuck: the effect's result is `failed`, no `stuck_resolved` is recorded, and a new
/// decision may be asked and moved, once the platform answers.
// verifies: INV-2
#[test]
fn failed_move_leaves_the_value_held_and_the_part_stuck() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route_at_one_reserve();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let held_before = core.exposures.stuck().len();
    assert!(held_before > 0);

    let dest = to_destination(&core, AMOUNT);
    let second = decide(&mut core, &mut stuck, &mut policy, &alice, &dest).expect("asked");
    let handled = world.fleet.platform.operations_received(origin_reserve());
    let (order, actor) = (id(&core.named.order), segments_support::core());
    let carries_out = decided(&second);
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.refuse_begin = Some(0);
    let moved = on_platform(
        &mut core,
        &world.catalogue,
        &mut watched,
        &mut mover,
        |parts, delivering| {
            stuck.carry_out(
                parts,
                &mut policy,
                delivering,
                Moving {
                    order: &order,
                    carries_out: &carries_out,
                    mover: &bob,
                    core: &actor,
                },
            )
        },
    );
    assert!(watched.carried.is_empty());
    drop(watched);
    let Ok(Moved::CarriedOut {
        effect,
        resolved: Resolved::Failed,
        ..
    }) = moved
    else {
        panic!("failed: {moved:?}");
    };
    assert_eq!(results_of(&core, &effect), vec![Outcome::Failed {}]);
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled
    );
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    assert!(resolved_to(&core).is_empty());
    assert_eq!(core.exposures.stuck().len(), held_before);

    // A new decision is carried out once the platform answers, and resolves the part.
    let dest = to_destination(&core, AMOUNT);
    let third = decide(&mut core, &mut stuck, &mut policy, &alice, &dest).expect("asked again");
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&third),
        &bob,
    );
    assert!(
        matches!(
            delivered.moved,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            })
        ),
        "{:?}",
        delivered.moved
    );
    assert_eq!(resolved_to(&core), vec![ResolvedTo::Destination]);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The provider's own word that the move failed (it refuses finally) is its word alone: a provider that paid could say
/// so to be paid twice. The move answers `Unknown`, its result is `unanswered`, and the part stays held, nothing more
/// decided for it, until the failure is reported on a statement Core checked and another actor confirms it on another
/// maker's; only then is the part freed, and a new decision moves it.
// verifies: REQ-AUZ-4, REQ-ORD-1
#[test]
fn provider_stated_failure_does_not_free_the_part_until_another_party_confirms_it() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob, carol) = (
        staff("desk-a", "alice"),
        staff("desk-b", "bob"),
        staff("desk-c", "carol"),
    );
    world
        .fleet
        .world()
        .script_for(origin_reserve(), "deposit", Behaviour::act(Act::Refuse));
    let first = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let handled = world.fleet.platform.operations_received(origin_reserve());
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&first),
        &bob,
    );
    let Ok(Moved::CarriedOut {
        effect,
        resolved: Resolved::Unknown,
        ..
    }) = delivered.moved.clone()
    else {
        panic!("unknown: {:?}", delivered.moved);
    };
    assert_eq!(results_of(&core, &effect), vec![Outcome::Unanswered {}]);
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &alice,
            &to_origin(AMOUNT)
        )
        .map(|_| ()),
        Err(ResolveError::OutcomeUnknown),
        "the provider's word frees nothing"
    );
    // Reported on the provider's statement, confirmed on another maker's: then the part is free.
    let transaction = decided_transaction(&core);
    let bytes = restated(&core, &transaction);
    let carrying_core = segments_support::core();
    let (order, part) = (id(&core.named.order), id(PART));
    let mut watched = Watched::new(&mut world.fleet.platform);
    let stated = on_platform(
        &mut core,
        &world.catalogue,
        &mut watched,
        &mut mover,
        |parts, delivering| {
            delivering.statement(
                parts,
                &rutent_core_orch::stuck::Carrying {
                    order: &order,
                    part: &part,
                    core: &carrying_core,
                    again: false,
                },
                &effect,
                &bytes,
            )
        },
    )
    .expect("taken");
    drop(watched);
    assert_eq!(stated.outcome(), Some(Resolved::Failed));
    let another = result_without_a_question(&mut core, second_reserve(), transaction.as_str());
    let confirmed = report_and_confirm(
        &mut core,
        &mut stuck,
        &mut policy,
        &first,
        (&bob, &carol),
        (&stated, &another),
        Resolved::Failed,
    );
    assert!(matches!(
        confirmed,
        Moved::CarriedOut {
            resolved: Resolved::Failed,
            ..
        }
    ));
    assert_eq!(
        results_of(&core, &effect),
        vec![Outcome::Unanswered {}, Outcome::Failed {}]
    );
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    let second = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("freed once another party confirmed");
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&second),
        &bob,
    );
    assert!(matches!(
        delivered.moved,
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled + 2,
        "one operation for each decision"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The answer the reserve gave, stated again now, signed with its statements key: its statement of how the move went,
/// made after the move.
fn restated(core: &Core, transaction: &Id) -> Vec<u8> {
    let mut answer = core
        .store
        .kept
        .borrow()
        .iter()
        .filter_map(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
        .rev()
        .find(|v| {
            v["statement"]["transaction"] == transaction.as_str()
                && v["shape"] == rutent_contracts::provider_role::RESULT_SHAPE
                && v["maker"] == origin_reserve()
        })
        .expect("the reserve's answer is kept");
    answer["made_at"] = core.clock.now().timestamp().unwrap().as_str().into();
    resign(&mut answer, origin_reserve());
    serde_json::to_vec(&answer).unwrap()
}

/// The operation reached the provider, and then the log couldn't take its hand-over: the first call stops with the
/// provider holding the operation. Taken on again by a mover with no memory of it (the process restarted, the stuck
/// record kept), the move is not handed over again and does not read as failed: it is unknown, the part stays held
/// and the provider has received the operation once.
// verifies: INV-8
#[test]
fn move_taken_on_by_a_mover_with_no_memory_of_it_is_never_handed_over_again() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let handled = world.fleet.platform.operations_received(origin_reserve());
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::HandOver, Fault::Unavailable));
    }
    let first = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    assert!(matches!(first.moved, Err(ResolveError::Append(_))));
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled + 1
    );
    // The platform restarted too, so it can't say the transaction began already: only the mover's own rule holds.
    let mut fresh = Mover::new();
    let (order, actor, by) = (id(&core.named.order), segments_support::core(), &bob);
    let carries_out = decided(&permitted);
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.forgets_begins = true;
    let moved = on_platform(
        &mut core,
        &world.catalogue,
        &mut watched,
        &mut fresh,
        |parts, delivering| {
            stuck.carry_out(
                parts,
                &mut policy,
                delivering,
                Moving {
                    order: &order,
                    carries_out: &carries_out,
                    mover: by,
                    core: &actor,
                },
            )
        },
    );
    let again = Delivered {
        moved,
        carried: watched.carried.clone(),
    };
    drop(watched);
    assert!(again.carried.is_empty(), "never handed over again");
    assert!(
        matches!(
            again.moved,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Unknown,
                ..
            })
        ),
        "{:?}",
        again.moved
    );
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled + 1
    );
    assert!(resolved_to(&core).is_empty());
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    assert_eq!(
        decide(
            &mut core,
            &mut stuck,
            &mut policy,
            &alice,
            &to_origin(AMOUNT)
        )
        .map(|_| ()),
        Err(ResolveError::OutcomeUnknown)
    );
}

/// What the platform says is read cautiously. A module that passed something out and then gave no answer
/// (`OutcomeUnknown`), and a platform that says the transaction began already (`DuplicateTransaction`), are the
/// outcome unknown, never a failure that frees the part; a refusal that proves nothing passed out (`Unavailable`) is
/// a failure.
// verifies: INV-2
#[test]
fn platform_refusals_that_prove_nothing_passed_out_are_the_only_failures() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    use rutent_contracts::module_interface::PlatformRefusal as Refusal;
    // The module passed something out and gave no answer.
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let (order, actor) = (id(&core.named.order), segments_support::core());
    let carries_out = decided(&permitted);
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.refuse_carry = Some(Refusal::OutcomeUnknown);
    let moved = on_platform(
        &mut core,
        &world.catalogue,
        &mut watched,
        &mut mover,
        |parts, delivering| {
            stuck.carry_out(
                parts,
                &mut policy,
                delivering,
                Moving {
                    order: &order,
                    carries_out: &carries_out,
                    mover: &bob,
                    core: &actor,
                },
            )
        },
    );
    drop(watched);
    assert!(
        matches!(
            moved,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Unknown,
                ..
            })
        ),
        "{moved:?}"
    );
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    // A transaction the platform says began already: nothing is handed over, and it isn't a failure either.
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let (order, carries_out) = (id(&core.named.order), decided(&permitted));
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.refuse_begin = Some(0);
    watched.begin_refusal = Refusal::DuplicateTransaction;
    let moved = on_platform(
        &mut core,
        &world.catalogue,
        &mut watched,
        &mut mover,
        |parts, delivering| {
            stuck.carry_out(
                parts,
                &mut policy,
                delivering,
                Moving {
                    order: &order,
                    carries_out: &carries_out,
                    mover: &bob,
                    core: &actor,
                },
            )
        },
    );
    assert!(watched.carried.is_empty());
    drop(watched);
    assert!(
        matches!(
            moved,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Unknown,
                ..
            })
        ),
        "{moved:?}"
    );
}

/// A resolution in a unit the provider's tuple doesn't count in, or whose question the log records unattributed, is not
/// asked of the provider at all: the move fails before anything is handed over, though a decision Core refused could
/// not reach it (the effect here is forged, as only the mover is under test).
// verifies: REQ-AUZ-4
#[test]
fn move_the_provider_cant_be_asked_fails_before_anything_is_handed_over() {
    use rutent_contracts::core_internal::Effect;
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let alice = staff("desk-a", "alice");
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let transaction = decided_transaction(&core);
    let handled = world.fleet.platform.operations_received(origin_reserve());
    let receipts = [
        core.log.borrow().receipt_at(0),
        core.log.borrow().receipt_at(1),
    ];
    let effect = |decimals: u8, index: usize| Effect {
        action: Action::ResolveStuck {
            order: id(ORDER),
            carries_out: decided(&permitted),
            resolution: Resolution::Deliver {
                by: id(origin_reserve()),
                to: origin(),
                asset: Denomination {
                    id: "COP".into(),
                    decimals,
                },
                amount: PositiveAmount::new(AMOUNT).unwrap(),
            },
            transaction: transaction.clone(),
        },
        decision: permitted.permit().expect("a permit"),
        receipt: receipts[index].clone(),
    };
    let (order, part, actor) = (id(ORDER), id(PART), segments_support::core());
    let carrying = rutent_core_orch::stuck::Carrying {
        order: &order,
        part: &part,
        core: &actor,
        again: false,
    };
    use rutent_core_orch::stuck::Resolutions as _;
    // Another unit than the tuple counts in.
    let forged = effect(6, 0);
    let mut watched = Watched::new(&mut world.fleet.platform);
    let said = on_platform(
        &mut core,
        &world.catalogue,
        &mut watched,
        &mut mover,
        |parts, delivering| delivering.resolve(parts, &carrying, &forged),
    );
    assert_eq!(said, Ok(Resolved::Failed));
    // The right unit, but the log records the question unattributed (its key unknown to it): nothing is handed over.
    core.log.borrow_mut().stop_attributing();
    let other = effect(2, 1);
    let said = on_platform(
        &mut core,
        &world.catalogue,
        &mut watched,
        &mut mover,
        |parts, delivering| delivering.resolve(parts, &carrying, &other),
    );
    assert_eq!(said, Ok(Resolved::Failed));
    assert!(watched.carried.is_empty());
    drop(watched);
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled
    );
}

/// A move that stopped before anything was handed over (the log couldn't take the operator's question) and is taken
/// on again after the customer replaced the part's destination does not deliver to the old one: its ends are read
/// again, the move is dropped and its effect is closed `failed`, and the provider is handed nothing.
// verifies: REQ-AUZ-3, REQ-AUZ-4
#[test]
fn stalled_move_before_any_hand_over_is_dropped_when_the_customer_replaced_the_destination() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route_at_one_reserve();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let dest = to_destination(&core, AMOUNT);
    let permitted = decide(&mut core, &mut stuck, &mut policy, &alice, &dest).expect("asked");
    let handled = world.fleet.platform.operations_received(origin_reserve());
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::StatedOperation, Fault::Unavailable));
    }
    let first = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    assert!(matches!(first.moved, Err(ResolveError::Append(_))));
    // The customer authorises the part to another account, and Core follows the order again.
    authorise_in_the_log(
        &core,
        authorised_anew(
            &core,
            "auth-p1-new",
            Some(vec![id(PART)]),
            rutent_scenarios::kit::customer(),
            "acct-new",
            "acct-1",
        ),
    );
    follow(&core, &mut stuck).expect("the part's new destination replaces its ends");
    let again = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    assert_eq!(
        again.moved.map(|_| ()),
        Err(ResolveError::ElsewhereThanItsEnds)
    );
    assert!(again.carried.is_empty());
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled
    );
    assert!(resolved_to(&core).is_empty());
    assert_eq!(open_effects(&core), 0, "the effect is closed");
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

fn open_effects(core: &Core) -> usize {
    let log = core.log.borrow();
    let receipts: Vec<_> = (0..log.entries.len())
        .filter(|&i| matches!(&log.entries[i].0, Entry::Verified(v) if is_resolve_stuck(&v.what)))
        .map(|i| log.receipt_at(i))
        .collect();
    receipts
        .iter()
        .filter(|r| {
            !log.verified()
                .iter()
                .any(|v| matches!(&v.what, What::Result { effect } if effect == *r))
        })
        .count()
}

fn authorise_in_the_log(core: &Core, entry: Entry) {
    use rutent_contracts::core_internal::calls::{DecisionEntry, PolicyLog};
    let Entry::Verified(entry) = entry else {
        panic!("an authorisation is a verified entry");
    };
    PolicyPath(std::rc::Rc::clone(&core.log))
        .append_decision(&DecisionEntry::try_from(entry).expect("a decision"))
        .expect("the log takes the customer's authorisation");
}

fn authorised_anew(
    core: &Core,
    authorisation: &str,
    parts: Option<Vec<Id>>,
    given_by: rutent_contracts::core_internal::Actor,
    account: &str,
    origin_account: &str,
) -> Entry {
    let mut first = segments_support::resolving::recorded(core)
        .into_iter()
        .find_map(|e| match e {
            Entry::Verified(v)
                if matches!(v.what, What::Authorise { .. })
                    && matches!(v.outcome, Outcome::Permitted {}) =>
            {
                Some(v)
            }
            _ => None,
        })
        .expect("the order's authorisation");
    if let What::Authorise {
        authorisation: a, ..
    } = &mut first.what
    {
        a.authorisation = id(authorisation);
        a.parts = parts;
        a.destination.account = id(account);
        a.origin_account = Some(id(origin_account));
        a.given_by = given_by.clone();
    }
    first.who = given_by;
    Entry::Verified(first)
}

// The reserve half of `REQ-CPL-12` on the mover's path (`T-020-23`) ---------------------------------------------------

/// The reliance entries the log holds: the place of each, with its judgement's receipt, judge and relying provider.
fn reliances(core: &Core) -> Vec<(usize, Receipt, String, String)> {
    core.log
        .borrow()
        .records()
        .into_iter()
        .filter_map(|(at, what)| match what {
            What::Reliance {
                judgement,
                judge,
                relied_on_by,
                ..
            } => Some((
                at,
                judgement.clone(),
                judge.as_str().to_owned(),
                relied_on_by.as_str().to_owned(),
            )),
            _ => None,
        })
        .collect()
}

/// The place of the operator's question under the decided transaction.
fn question_place(core: &Core) -> usize {
    let transaction = decided_transaction(core);
    let log = core.log.borrow();
    log.entries
        .iter()
        .position(|(e, _)| {
            matches!(e, Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::OPERATION_SHAPE
                    && s.about.as_ref().is_some_and(|a| a.transaction.as_ref() == Some(&transaction)))
        })
        .expect("the operator's question")
}

/// An input declared to rest on a judgement, after the segment that carried it was handed over (so refused as too
/// late, and kept), is relied on when the stuck mover hands the provider it: the reliance names the judge and the
/// provider, is in the log before the question and so before the operation, and the history is admitted.
// verifies: REQ-CPL-12
#[test]
fn stuck_delivery_of_an_input_resting_on_a_judgement_leaves_a_reliance_before_the_send() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route_at_one_reserve();
    let (_, theirs) = results_after_the_move(&mut core, None, "tx-judged");
    let judgement = rutent_core_orch::reliance::Judgement {
        receipt: theirs,
        judge: id(second_reserve()),
    };
    // The deposit's segment was handed over (and never begun) already: the declaration is late for it, and stands.
    assert_eq!(
        core.segments.rests_on(
            &id(&core.named.order),
            "beneficiary.bank_account",
            judgement.clone()
        ),
        Err(rutent_core_orch::reliance::Rests::TooLate)
    );
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let resolution = to_destination(&core, AMOUNT);
    let permitted = decide(&mut core, &mut stuck, &mut policy, &alice, &resolution).expect("asked");
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    assert!(matches!(
        delivered.moved,
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    assert_eq!(delivered.carried.len(), 1);
    let held = reliances(&core);
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(
        (held[0].1.clone(), held[0].2.as_str(), held[0].3.as_str()),
        (judgement.receipt, second_reserve(), origin_reserve())
    );
    assert!(
        held[0].0 < question_place(&core),
        "the reliance precedes the question, and so the send"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A judgement the log won't take as its judge's stops the delivery: nothing is handed the provider, and the move
/// is failed.
// verifies: REQ-CPL-12
#[test]
fn stuck_delivery_whose_reliance_the_log_refuses_sends_nothing() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    core.log.borrow_mut().guards_reliance = true;
    let (_, theirs) = results_after_the_move(&mut core, None, "tx-judged");
    let wrong = rutent_core_orch::reliance::Judgement {
        receipt: theirs,
        judge: id(origin_reserve()),
    };
    let _ = core
        .segments
        .rests_on(&id(&core.named.order), "beneficiary.bank_account", wrong);
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let resolution = to_destination(&core, AMOUNT);
    let permitted = decide(&mut core, &mut stuck, &mut policy, &alice, &resolution).expect("asked");
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &bob,
    );
    assert!(delivered.carried.is_empty(), "nothing was sent");
    assert!(reliances(&core).is_empty());
    assert!(
        !matches!(
            delivered.moved,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            })
        ),
        "{:?}",
        delivered.moved
    );
}

// A report that is wrong, and the evidence a report states (`T-020-20`) --------------------------------------------

/// A move the reserve left unknown by its own word (it refused finally): the mover's `bob` may report it, the decider's
/// `alice` and `carol` confirm or decline it. Two statements of the reserve's are to hand, its failure and, as the
/// reserve might state it later, its settling; another maker's states none of the move's.
struct Refused {
    world: World,
    core: Core,
    stuck: Stuck,
    mover: Mover,
    policy: PermitsEveryStep,
    permitted: rutent_contracts::core_internal::Decision,
    effect: Receipt,
    transaction: Id,
}

const BOB: (&str, &str) = ("desk-b", "bob");

fn refused_move() -> Refused {
    refused_move_in(stuck_by_no_route())
}

/// [`refused_move`] at one reserve, which runs the whole route, with [`OTHER_RESERVE`] installed beside it, running
/// nothing of the order: a provider outside it (`T-020-40`).
fn refused_move_at_one_reserve() -> Refused {
    refused_move_in(stuck_by_no_route_at_one_reserve())
}

fn refused_move_in((mut world, mut core, mut stuck): (World, Core, Stuck)) -> Refused {
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    world
        .fleet
        .world()
        .script_for(origin_reserve(), "deposit", Behaviour::act(Act::Refuse));
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &staff("desk-a", "alice"),
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &staff(BOB.0, BOB.1),
    );
    let Ok(Moved::CarriedOut {
        effect,
        resolved: Resolved::Unknown,
        ..
    }) = delivered.moved
    else {
        panic!("unknown: {:?}", delivered.moved);
    };
    let transaction = decided_transaction(&core);
    Refused {
        world,
        core,
        stuck,
        mover,
        policy,
        permitted,
        effect,
        transaction,
    }
}

/// The reserve's answer to the move with its outcome replaced by the one a settled withdrawal states, made now and
/// signed with its statements key: as it might state the move's settling later.
fn answer_settled(core: &Core, transaction: &Id) -> Vec<u8> {
    let kept: Vec<serde_json::Value> = core
        .store
        .kept
        .borrow()
        .iter()
        .filter_map(|b| serde_json::from_slice(b).ok())
        .collect();
    let mut answer = kept
        .iter()
        .rev()
        .find(|v| {
            v["statement"]["transaction"] == transaction.as_str()
                && v["shape"] == rutent_contracts::provider_role::RESULT_SHAPE
                && v["maker"] == origin_reserve()
        })
        .expect("the reserve's answer is kept")
        .clone();
    answer["statement"]["outcome"] = kept
        .iter()
        .find(|v| {
            v["statement"]["kind"] == "withdrawal"
                && v["statement"]["outcome"]["status"] == "succeeded"
        })
        .expect("a settled withdrawal")["statement"]["outcome"]
        .clone();
    answer["made_at"] = core.clock.now().timestamp().unwrap().as_str().into();
    resign(&mut answer, origin_reserve());
    serde_json::to_vec(&answer).unwrap()
}

impl Refused {
    fn took(&mut self, bytes: &[u8]) -> Stated {
        let carrying_core = segments_support::core();
        let (order, part) = (id(&self.core.named.order), id(PART));
        let mut watched = Watched::new(&mut self.world.fleet.platform);
        let effect = self.effect.clone();
        on_platform(
            &mut self.core,
            &self.world.catalogue,
            &mut watched,
            &mut self.mover,
            |parts, delivering| {
                delivering.statement(
                    parts,
                    &rutent_core_orch::stuck::Carrying {
                        order: &order,
                        part: &part,
                        core: &carrying_core,
                        again: false,
                    },
                    &effect,
                    bytes,
                )
            },
        )
        .expect("taken")
    }

    /// The reserve's own statement that the move failed, made now.
    fn failed(&mut self) -> Stated {
        let bytes = restated(&self.core, &self.transaction);
        let stated = self.took(&bytes);
        assert_eq!(stated.outcome(), Some(Resolved::Failed));
        stated
    }

    /// The reserve's own statement that the move settled, made now.
    fn settled(&mut self) -> Stated {
        let bytes = answer_settled(&self.core, &self.transaction);
        let stated = self.took(&bytes);
        assert_eq!(stated.outcome(), Some(Resolved::Done));
        stated
    }

    /// Another maker's statement about the order and the transaction: it states nothing of the move.
    fn others(&mut self) -> Stated {
        Stated::for_tests(
            result_without_a_question(&mut self.core, second_reserve(), self.transaction.as_str()),
            None,
        )
    }

    fn moving<T>(
        &mut self,
        with: impl FnOnce(
            &mut Stuck,
            &mut rutent_core_orch::stuck::Parts<'_>,
            &mut PermitsEveryStep,
            &[(Receipt, Entry)],
        ) -> T,
    ) -> T {
        let (stuck, policy) = (&mut self.stuck, &mut self.policy);
        let history = self.core.recorded();
        on_parts(&mut self.core, |parts| with(stuck, parts, policy, &history))
    }

    fn report(
        &mut self,
        who: &rutent_contracts::core_internal::Actor,
        outcome: Resolved,
        evidence: &Stated,
    ) -> Result<Moved, ResolveError> {
        let (order, carries_out, actor) = (
            id(&self.core.named.order),
            decided(&self.permitted),
            segments_support::core(),
        );
        self.moving(|stuck, parts, policy, history| {
            stuck.known_outcome(
                parts,
                policy,
                Moving {
                    order: &order,
                    carries_out: &carries_out,
                    mover: who,
                    core: &actor,
                },
                outcome,
                evidence,
                history,
            )
        })
    }

    fn confirm(
        &mut self,
        who: &rutent_contracts::core_internal::Actor,
        report: (Resolved, u64, &Stated),
        evidence: &Stated,
    ) -> Result<Moved, ResolveError> {
        let (order, carries_out, actor) = (
            id(&self.core.named.order),
            decided(&self.permitted),
            segments_support::core(),
        );
        self.moving(|stuck, parts, policy, history| {
            stuck.confirm_outcome(
                parts,
                policy,
                Confirming {
                    order: &order,
                    carries_out: &carries_out,
                    confirmer: who,
                    outcome: report.0,
                    report: report.1,
                    reported_with: report.2.receipt(),
                    evidence,
                    core: &actor,
                },
                history,
            )
        })
    }

    fn decline(
        &mut self,
        who: &rutent_contracts::core_internal::Actor,
        report: (Resolved, u64, &Stated),
        evidence: &Stated,
    ) -> Result<Moved, ResolveError> {
        let (order, carries_out) = (id(&self.core.named.order), decided(&self.permitted));
        self.moving(|stuck, parts, policy, history| {
            stuck.decline_outcome(
                parts,
                policy,
                Declining {
                    order: &order,
                    carries_out: &carries_out,
                    decliner: who,
                    outcome: report.0,
                    report: report.1,
                    reported_with: report.2.receipt(),
                    evidence,
                },
                history,
            )
        })
    }

    /// How many reports, confirmations and declines of an outcome the log holds.
    fn decisions(&self) -> (usize, usize, usize) {
        (
            count(&self.core, |w| matches!(w, What::ReportOutcome { .. })),
            count(&self.core, |w| matches!(w, What::ConfirmOutcome { .. })),
            count(&self.core, |w| matches!(w, What::DeclineOutcome { .. })),
        )
    }

    fn held(&mut self) {
        assert!(self.stuck.is_stuck(&id(ORDER), &id(PART)));
        assert_eq!(
            decide(
                &mut self.core,
                &mut self.stuck,
                &mut self.policy,
                &staff("desk-a", "alice"),
                &to_origin(AMOUNT)
            )
            .map(|_| ()),
            Err(ResolveError::OutcomeUnknown),
            "the part is held, nothing more decided for it"
        );
        assert!(resolved_to(&self.core).is_empty(), "nothing is resolved");
    }

    fn admitted(&self) {
        self.core
            .admitted(&self.world.catalogue)
            .expect("the log admits every entry");
    }
}

/// A report that is wrong can't be corrected by its mover: its confirmer declines it, naming the report and its own
/// evidence, through policy, and only then is a new report of the same move admitted, in the declined one's place,
/// counting one version more. The new report stands until it is confirmed, and a confirmation names the version it
/// confirms. The declined report settled nothing: the move's result is recorded once, on the new one's confirmation.
// verifies: REQ-AUZ-4
#[test]
fn report_declined_by_its_confirmer_is_followed_by_a_new_report_that_stands_until_it_is_confirmed()
{
    let mut m = refused_move();
    let (bob, carol, dave) = (
        staff(BOB.0, BOB.1),
        staff("desk-c", "carol"),
        staff("desk-d", "dave"),
    );
    // The mover reports the move failed, on the reserve's own statement of it.
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    // Another party declines it on another maker's statement; the report no longer stands.
    let others = m.others();
    assert_eq!(
        m.decline(&carol, (Resolved::Failed, 1, &failed), &others),
        Ok(Moved::Declined { report: 1 })
    );
    assert_eq!(m.decisions(), (1, 0, 1));
    // The declined report is not made again as it stands, but a new one, of what the reserve states now, is.
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Err(ResolveError::ReportDeclined)
    );
    let settled = m.settled();
    assert_eq!(
        m.report(&bob, Resolved::Done, &settled),
        Ok(Moved::AwaitsConfirmation { report: 2 }),
        "a new report counts one version beyond the declined"
    );
    // It stands until it is confirmed: nothing is recorded for the move's outcome by it.
    assert_eq!(results_of(&m.core, &m.effect), vec![Outcome::Unanswered {}]);
    m.held();
    // Another party confirms the new one, naming its version, on another maker's statement.
    let again = m.others();
    let confirmed = m.confirm(&dave, (Resolved::Done, 2, &settled), &again);
    assert!(
        matches!(
            confirmed,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            })
        ),
        "{confirmed:?}"
    );
    assert_eq!(
        results_of(&m.core, &m.effect),
        vec![Outcome::Unanswered {}, Outcome::Done {}]
    );
    assert_eq!(resolved_to(&m.core), vec![ResolvedTo::Origin]);
    assert_eq!(m.decisions(), (2, 1, 1));
    m.admitted();
}

/// A move's outcome is reported once while its report stands: a second report with no decline between is refused,
/// whatever its outcome and evidence, and nothing is asked of policy for it.
// verifies: REQ-AUZ-4
#[test]
fn second_report_with_no_decline_between_is_refused() {
    let mut m = refused_move();
    let bob = staff(BOB.0, BOB.1);
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    let asked = m.policy.asked;
    let settled = m.settled();
    for (outcome, evidence) in [(Resolved::Failed, &failed), (Resolved::Done, &settled)] {
        assert_eq!(
            m.report(&bob, outcome, evidence),
            Err(ResolveError::AlreadyReported)
        );
    }
    assert_eq!(m.policy.asked, asked, "policy isn't asked for it");
    assert_eq!(m.decisions(), (1, 0, 0));
    m.held();
    m.admitted();
}

/// A mover can't withdraw its own report: the decline is of another party and login than its reporter's, so one actor
/// would otherwise swap its own word after the fact (`THR-70`). Its own decline is refused, policy asked nothing, and
/// no new report is taken in the report's place.
// verifies: REQ-AUZ-4
#[test]
fn mover_cannot_withdraw_its_own_report() {
    let mut m = refused_move();
    let bob = staff(BOB.0, BOB.1);
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    let asked = m.policy.asked;
    let others = m.others();
    assert_eq!(
        m.decline(&bob, (Resolved::Failed, 1, &failed), &others),
        Err(ResolveError::SameParty)
    );
    let settled = m.settled();
    assert_eq!(
        m.report(&bob, Resolved::Done, &settled),
        Err(ResolveError::AlreadyReported),
        "the report still stands"
    );
    assert_eq!(m.policy.asked, asked);
    assert_eq!(m.decisions(), (1, 0, 0));
    m.held();
    m.admitted();
}

/// A decline by the reporter's own party, under another login, or by its own login under another party, is refused: the
/// report stands and policy is asked nothing.
// verifies: REQ-AUZ-4
#[test]
fn decline_by_the_reporters_own_party_is_refused() {
    let mut m = refused_move();
    let bob = staff(BOB.0, BOB.1);
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    let asked = m.policy.asked;
    let others = m.others();
    for (decliner, why) in [
        (staff(BOB.0, "bobby"), ResolveError::SameParty),
        (staff("desk-x", BOB.1), ResolveError::SameLogin),
    ] {
        assert_eq!(
            m.decline(&decliner, (Resolved::Failed, 1, &failed), &others),
            Err(why)
        );
    }
    assert_eq!(m.policy.asked, asked);
    // The report stands: another party's decline of it is taken.
    assert_eq!(
        m.decline(
            &staff("desk-c", "carol"),
            (Resolved::Failed, 1, &failed),
            &others
        ),
        Ok(Moved::Declined { report: 1 })
    );
    m.admitted();
}

/// A declined report is never confirmed: a confirmation of it, whether no new report was made or a new one stands, is
/// refused and records nothing; only the new report's own version is confirmed.
// verifies: REQ-AUZ-4, REQ-ORD-1
#[test]
fn confirmation_of_a_declined_report_is_refused() {
    let mut m = refused_move();
    let (bob, carol, dave) = (
        staff(BOB.0, BOB.1),
        staff("desk-c", "carol"),
        staff("desk-d", "dave"),
    );
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    let others = m.others();
    assert_eq!(
        m.decline(&carol, (Resolved::Failed, 1, &failed), &others),
        Ok(Moved::Declined { report: 1 })
    );
    let asked = m.policy.asked;
    let more = m.others();
    assert_eq!(
        m.confirm(&dave, (Resolved::Failed, 1, &failed), &more),
        Err(ResolveError::ReportDeclined),
        "no report stands"
    );
    // A new report stands; the declined one's version still confirms nothing.
    let settled = m.settled();
    assert_eq!(
        m.report(&bob, Resolved::Done, &settled),
        Ok(Moved::AwaitsConfirmation { report: 2 })
    );
    let asked_since = m.policy.asked;
    assert_eq!(
        m.confirm(&dave, (Resolved::Failed, 1, &failed), &more),
        Err(ResolveError::ReportDeclined)
    );
    assert_eq!(
        m.confirm(&dave, (Resolved::Done, 1, &settled), &more),
        Err(ResolveError::ReportDeclined),
        "nor the new report under the declined one's version"
    );
    assert_eq!(
        m.confirm(&dave, (Resolved::Failed, 2, &failed), &more),
        Err(ResolveError::NotTheReport),
        "nor the declined one's outcome and evidence under the new version"
    );
    assert_eq!(m.policy.asked, asked_since, "policy is asked nothing");
    assert!(asked < asked_since);
    assert_eq!(m.decisions(), (2, 0, 1));
    assert_eq!(results_of(&m.core, &m.effect), vec![Outcome::Unanswered {}]);
    m.held();
    m.admitted();
}

/// A part stays held while its report is declined: the decline settles nothing and frees nothing, no new decision or
/// move is taken for it, no result is recorded for the move, and its value stays counted among the stuck.
// verifies: INV-2
#[test]
fn part_stays_held_while_its_report_is_declined() {
    let mut m = refused_move();
    let (bob, carol) = (staff(BOB.0, BOB.1), staff("desk-c", "carol"));
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    let others = m.others();
    assert_eq!(
        m.decline(&carol, (Resolved::Failed, 1, &failed), &others),
        Ok(Moved::Declined { report: 1 })
    );
    m.held();
    assert_eq!(
        results_of(&m.core, &m.effect),
        vec![Outcome::Unanswered {}],
        "the failed report freed nothing"
    );
    assert_eq!(
        m.stuck
            .stuck_values(&m.core.exposures, &*m.core.clock)
            .len(),
        1,
        "its value is still counted among the stuck"
    );
    // The move is not carried out again for the declined report either.
    let handled = m.world.fleet.platform.operations_received(origin_reserve());
    let again = deliver(
        &mut m.core,
        &mut m.world,
        &mut m.stuck,
        &mut m.mover,
        &mut m.policy,
        &decided(&m.permitted),
        &bob,
    );
    assert!(again.moved.is_err() || again.carried.is_empty());
    assert_eq!(
        m.world.fleet.platform.operations_received(origin_reserve()),
        handled
    );
    m.admitted();
}

/// The evidence a report names states the outcome it reports: a statement of the move's own provider stating another
/// outcome, or another maker's, which states none of the move's, backs no report, and policy is asked nothing. A
/// confirmation's evidence is held to the report's outcome too, where the move's own provider made it; another
/// maker's states none and backs the outcome the report named.
// verifies: REQ-ORD-1, REQ-AUZ-4
#[test]
fn report_whose_evidence_states_another_outcome_than_the_one_reported_is_refused() {
    let mut m = refused_move();
    let (bob, carol) = (staff(BOB.0, BOB.1), staff("desk-c", "carol"));
    let (failed, settled, others) = (m.failed(), m.settled(), m.others());
    let asked = m.policy.asked;
    for (outcome, evidence) in [
        (Resolved::Done, &failed),
        (Resolved::Failed, &settled),
        (Resolved::Done, &others),
        (Resolved::Failed, &others),
    ] {
        assert_eq!(
            m.report(&bob, outcome, evidence),
            Err(ResolveError::EvidenceStatesAnother),
            "{outcome:?} on {evidence:?}"
        );
    }
    assert_eq!(m.policy.asked, asked, "policy is asked nothing");
    assert_eq!(m.decisions(), (0, 0, 0));
    // Reported on what it states, then confirmed on the reserve's own contrary statement: refused.
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    assert_eq!(
        m.confirm(&carol, (Resolved::Failed, 1, &failed), &settled),
        Err(ResolveError::EvidenceStatesAnother),
        "the reserve's own statement that it settled doesn't bear out a failure"
    );
    assert_eq!(m.decisions(), (1, 0, 0));
    assert!(matches!(
        m.confirm(&carol, (Resolved::Failed, 1, &failed), &others),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Failed,
            ..
        })
    ));
    m.admitted();
}

/// A part's `stuck_resolved` cites an evidence entry the log holds: a `part_evidence` of the order and the part,
/// appended before it, resting on the statement the done rests on: the provider's own answer where its move settled,
/// the statement the confirmed report named where it was left unknown.
// verifies: REQ-AUZ-4, REQ-ORD-39
#[test]
fn stuck_resolved_cites_an_evidence_entry_the_log_holds() {
    fn part_evidence(core: &Core) -> Vec<(usize, What)> {
        core.log
            .borrow()
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, (e, _))| match e {
                Entry::Verified(v) if matches!(v.what, What::PartEvidence { .. }) => {
                    Some((i, v.what.clone()))
                }
                _ => None,
            })
            .collect()
    }
    fn resolved_evidence(core: &Core) -> Vec<(usize, String)> {
        core.log
            .borrow()
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, (e, _))| match e {
                Entry::Verified(v) => match &v.what {
                    What::Input { input } => match &input.observed {
                        Observed::StuckResolved { evidence, .. } => {
                            Some((i, evidence.as_str().to_owned()))
                        }
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }
    // A move that settled in the provider's own answer.
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &staff("desk-a", "alice"),
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let delivered = deliver(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &decided(&permitted),
        &staff(BOB.0, BOB.1),
    );
    let Ok(Moved::CarriedOut {
        effect,
        resolved: Resolved::Done,
        ..
    }) = delivered.moved
    else {
        panic!("done: {:?}", delivered.moved);
    };
    let transaction = decided_transaction(&core);
    let answer = stated_under(&core, &transaction);
    assert_eq!(answer.len(), 2, "the question and the provider's answer");
    let evidence = part_evidence(&core);
    let resolved = resolved_evidence(&core);
    assert_eq!((evidence.len(), resolved.len()), (1, 1));
    let (
        at,
        What::PartEvidence {
            evidence: cited,
            order,
            part,
            whose,
            rests_on,
            ..
        },
    ) = evidence[0].clone()
    else {
        unreachable!()
    };
    assert_eq!(cited.as_str(), resolved[0].1, "the input cites this entry");
    assert!(at < resolved[0].0, "the entry reaches the log first");
    assert_eq!(
        (order.as_str(), part.as_str(), whose.as_str()),
        (ORDER, PART, origin_reserve())
    );
    assert_eq!(rests_on.len(), 1);
    assert!(
        rests_on[0].index > effect.index,
        "on the provider's answer, which follows the move"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
    // A move left unknown, reported done on the reserve's later statement and confirmed.
    let mut m = refused_move();
    let (bob, carol) = (staff(BOB.0, BOB.1), staff("desk-c", "carol"));
    let settled = m.settled();
    assert_eq!(
        m.report(&bob, Resolved::Done, &settled),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    assert!(
        part_evidence(&m.core).is_empty(),
        "nothing before it is done"
    );
    let others = m.others();
    assert!(matches!(
        m.confirm(&carol, (Resolved::Done, 1, &settled), &others),
        Ok(Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        })
    ));
    let (evidence, resolved) = (part_evidence(&m.core), resolved_evidence(&m.core));
    assert_eq!((evidence.len(), resolved.len()), (1, 1));
    let What::PartEvidence {
        evidence: cited,
        rests_on,
        whose,
        ..
    } = &evidence[0].1
    else {
        unreachable!()
    };
    assert_eq!(cited.as_str(), resolved[0].1);
    assert!(evidence[0].0 < resolved[0].0);
    assert_eq!(whose.as_str(), origin_reserve());
    assert_eq!(
        rests_on,
        &vec![settled.receipt().clone()],
        "on the statement the confirmed report named, not the confirmer's"
    );
    m.admitted();
}

/// A statement the log recorded unattributed, its signing key unknown to it, rests on no key: it is evidence of
/// nothing, and an entry of the log's may not rest on it (`part-evidence-rests-on-statements`).
// verifies: REQ-ORD-1
#[test]
fn statement_the_log_recorded_unattributed_is_not_taken_as_evidence() {
    let mut m = refused_move();
    m.core.log.borrow_mut().stop_attributing();
    let bytes = restated(&m.core, &m.transaction);
    let carrying_core = segments_support::core();
    let (order, part) = (id(&m.core.named.order), id(PART));
    let effect = m.effect.clone();
    let mut watched = Watched::new(&mut m.world.fleet.platform);
    let taken = on_platform(
        &mut m.core,
        &m.world.catalogue,
        &mut watched,
        &mut m.mover,
        |parts, delivering| {
            delivering.statement(
                parts,
                &rutent_core_orch::stuck::Carrying {
                    order: &order,
                    part: &part,
                    core: &carrying_core,
                    again: false,
                },
                &effect,
                &bytes,
            )
        },
    );
    assert_eq!(taken, Err(NotTaken::Unattributable));
}

/// A decline or a confirmation the log's rules refuse, of a report already settled, comes back from policy as
/// `Err(Refused)`: a refusal, not a crash. Nothing changes: the report stands, no outcome is recorded, and the same
/// report is declined or confirmed once the log takes it (`T-021-09`'s gap 3).
// verifies: REQ-AUZ-4
#[test]
fn decline_or_confirmation_the_log_refuses_is_a_refusal_that_changes_nothing() {
    use rutent_contracts::core_internal::calls::AppendFailure;
    let mut m = refused_move();
    let (bob, carol) = (staff(BOB.0, BOB.1), staff("desk-c", "carol"));
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    let others = m.others();
    m.policy.log_refuses_outcomes = true;
    assert!(matches!(
        m.decline(&carol, (Resolved::Failed, 1, &failed), &others),
        Err(ResolveError::Append(AppendFailure::Refused(_)))
    ));
    assert!(matches!(
        m.confirm(&carol, (Resolved::Failed, 1, &failed), &others),
        Err(ResolveError::Append(AppendFailure::Refused(_)))
    ));
    m.policy.log_refuses_outcomes = false;
    assert_eq!(m.decisions(), (1, 0, 0));
    assert_eq!(results_of(&m.core, &m.effect), vec![Outcome::Unanswered {}]);
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Err(ResolveError::AlreadyReported),
        "the report still stands"
    );
    assert_eq!(
        m.decline(&carol, (Resolved::Failed, 1, &failed), &others),
        Ok(Moved::Declined { report: 1 })
    );
    m.held();
    m.admitted();
}

/// A confirmation policy permitted whose result the log couldn't take is the confirmer's alone: the move is no longer
/// open to another confirmation, so no other actor is carried on the permit (`THR-70`), and the log holds one
/// confirmation.
// verifies: REQ-AUZ-4
#[test]
fn confirmation_whose_result_the_log_could_not_take_is_not_open_to_another_confirmer() {
    use segments_support::{Fault, On};
    let mut m = refused_move();
    let (bob, carol, dave) = (
        staff(BOB.0, BOB.1),
        staff("desk-c", "carol"),
        staff("desk-d", "dave"),
    );
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    let others = m.others();
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        m.core
            .log
            .borrow_mut()
            .faults
            .push_back((On::LaterResult, Fault::Unavailable));
    }
    assert!(matches!(
        m.confirm(&carol, (Resolved::Failed, 1, &failed), &others),
        Err(ResolveError::Append(_))
    ));
    assert_eq!(m.decisions(), (1, 1, 0));
    assert_eq!(
        m.confirm(&dave, (Resolved::Failed, 1, &failed), &others),
        Err(ResolveError::NoDecision)
    );
    assert_eq!(m.decisions(), (1, 1, 0), "policy is asked nothing more");
}

/// A decline of a report rests on a statement by a party the order involves, another than the reporter's: the word of
/// a provider outside the order declines nothing, however verified, and nor does another statement by the reporter's
/// own party. Core refuses each before policy is asked, the report stands, and a party's word then declines it
/// (`confirmer-evidence-party`, `T-020-40`).
// verifies: REQ-AUZ-4
#[test]
fn decline_resting_on_an_outside_providers_word_or_the_reporters_own_party_is_refused() {
    // At one reserve, which runs the whole route, the other reserve runs nothing of the order.
    let mut m = refused_move_at_one_reserve();
    let (bob, carol) = (staff(BOB.0, BOB.1), staff("desk-c", "carol"));
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    let outside = m.others();
    let (asked, entries) = (m.policy.asked, m.core.log.borrow().entries.len());
    assert_eq!(
        m.decline(&carol, (Resolved::Failed, 1, &failed), &outside),
        Err(ResolveError::OutsideTheOrder)
    );
    assert_eq!(m.policy.asked, asked, "policy is asked nothing");
    assert_eq!(
        m.core.log.borrow().entries.len(),
        entries,
        "nothing recorded"
    );
    assert_eq!(m.decisions(), (1, 0, 0));
    m.held();
    m.admitted();
    drop(m);

    // At two parties, another statement of the reporter's own party declines nothing; the other party's does.
    let mut m = refused_move();
    let failed = m.failed();
    assert_eq!(
        m.report(&bob, Resolved::Failed, &failed),
        Ok(Moved::AwaitsConfirmation { report: 1 })
    );
    let asked = m.policy.asked;
    let settled = m.settled();
    assert_eq!(
        m.decline(&carol, (Resolved::Failed, 1, &failed), &settled),
        Err(ResolveError::ReportersOwnParty)
    );
    assert_eq!(m.policy.asked, asked, "policy is asked nothing");
    let others = m.others();
    assert_eq!(
        m.decline(&carol, (Resolved::Failed, 1, &failed), &others),
        Ok(Moved::Declined { report: 1 })
    );
    m.held();
    m.admitted();
}
