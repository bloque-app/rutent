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
    Deciding, Moved, Mover, Moving, NoRouteLeft, NotTaken, ResolveError, Resolved, Stuck,
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
    let mut world = World::of(&[RESERVE, OTHER_RESERVE]);
    let mut core = Core::over(&mut world);
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
        .script_dry_run(RESERVE, "deposit", Behaviour::act(Act::Refuse));
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
        by: id(RESERVE),
        to: origin(),
        asset: cop_unit(),
        amount: PositiveAmount::new(amount).unwrap(),
    }
}

fn to_destination(core: &Core, amount: &str) -> Resolution {
    Resolution::Deliver {
        by: id(RESERVE),
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
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let resolution = to_destination(&core, AMOUNT);
    let permitted = decide(&mut core, &mut stuck, &mut policy, &alice, &resolution).expect("asked");
    let handled = world.fleet.platform.operations_received(RESERVE);
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
        world.fleet.platform.operations_received(RESERVE),
        handled + 1,
        "the reserve is handed one operation"
    );
    let transaction = decided_transaction(&core);
    let operation = sent(&delivered);
    assert_eq!(operation["kind"], "deposit");
    assert_eq!(operation["provider"], RESERVE);
    assert_eq!(operation["transaction"], transaction.as_str());
    assert_eq!(operation["origin"], format!("{RESERVE}-ledger"));
    assert_eq!(
        operation["destination"]["network"],
        format!("{RESERVE}-bank")
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
            (RESERVE, rutent_contracts::provider_role::RESULT_SHAPE, true),
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
    assert_eq!(operation["origin"], format!("{RESERVE}-ledger"));
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
    let handled = world.fleet.platform.operations_received(RESERVE);
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
        world.fleet.platform.operations_received(RESERVE),
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
        world.fleet.platform.operations_received(RESERVE),
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
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Unknown));
    let permitted = decide(
        &mut core,
        &mut stuck,
        &mut policy,
        &alice,
        &to_origin(AMOUNT),
    )
    .expect("asked");
    let handled = world.fleet.platform.operations_received(RESERVE);
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
    resign(&mut other, RESERVE);
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
    assert_eq!(stated.outcome, Some(Resolved::Done));
    let again = take(&mut core, &mut world, &mut mover, &bytes).expect("taken again");
    assert_eq!(again, stated, "a statement tried again is one entry");
    let another = result_without_a_question(&mut core, OTHER_RESERVE, transaction.as_str());
    let reported = report_and_confirm(
        &mut core,
        &mut stuck,
        &mut policy,
        &permitted,
        (&bob, &carol),
        (&stated.receipt, &another),
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
        world.fleet.platform.operations_received(RESERVE),
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
    resign(&mut answer, RESERVE);
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
    evidence: (&Receipt, &Receipt),
) -> Moved {
    let (order, carries_out, actor) = (
        id(&core.named.order),
        decided(decision),
        segments_support::core(),
    );
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
            Resolved::Done,
            evidence.0,
        )
    })
    .expect("reported");
    let Moved::AwaitsConfirmation { report } = reported else {
        panic!("awaits its confirmation: {reported:?}");
    };
    on_parts(core, |parts| {
        stuck.confirm_outcome(
            parts,
            policy,
            rutent_core_orch::stuck::Confirming {
                order: &order,
                carries_out: &carries_out,
                confirmer,
                outcome: Resolved::Done,
                report,
                reported_with: evidence.0,
                evidence: evidence.1,
                core: &actor,
            },
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
    let handled = world.fleet.platform.operations_received(RESERVE);
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
    assert_eq!(world.fleet.platform.operations_received(RESERVE), handled);
    // The question can't be appended: still nothing is handed over.
    failing(&core, On::StatedOperation);
    let tried = try_move(&mut core, &mut world, &mut stuck, &mut mover, &mut policy);
    assert!(
        matches!(tried.moved, Err(ResolveError::Append(_))),
        "{:?}",
        tried.moved
    );
    assert_eq!(
        world.fleet.platform.operations_received(RESERVE),
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
        world.fleet.platform.operations_received(RESERVE),
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
        world.fleet.platform.operations_received(RESERVE),
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
        world.fleet.platform.operations_received(RESERVE),
        handled + 1
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A move that fails leaves the value held and the part stuck: the reserve says it failed (nothing moved), or the
/// platform won't begin the segment (nothing passed out): the effect's result is `failed`, no `stuck_resolved` is
/// recorded, the part's value is still held and a new decision may be asked and moved, at the same reserve, and
/// it is, once the platform answers.
// verifies: INV-2
#[test]
fn failed_move_leaves_the_value_held_and_the_part_stuck() {
    let (mut world, mut core, mut stuck) = stuck_by_no_route();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let held_before = core.exposures.stuck().len();
    assert!(held_before > 0);

    // The reserve says it failed.
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let first = decide(
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
        &decided(&first),
        &bob,
    );
    let Ok(Moved::CarriedOut {
        effect,
        resolved: Resolved::Failed,
        ..
    }) = delivered.moved.clone()
    else {
        panic!("failed: {:?}", delivered.moved);
    };
    assert_eq!(results_of(&core, &effect), vec![Outcome::Failed {}]);
    assert!(resolved_to(&core).is_empty());
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    assert_eq!(core.exposures.stuck().len(), held_before);

    // The platform won't begin the segment: nothing is handed over, and the move failed.
    let dest = to_destination(&core, AMOUNT);
    let second = decide(&mut core, &mut stuck, &mut policy, &alice, &dest)
        .expect("a failed move spends its decision, not the part");
    let handled = world.fleet.platform.operations_received(RESERVE);
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
    assert!(
        matches!(
            moved,
            Ok(Moved::CarriedOut {
                resolved: Resolved::Failed,
                ..
            })
        ),
        "{moved:?}"
    );
    assert_eq!(world.fleet.platform.operations_received(RESERVE), handled);
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    assert!(resolved_to(&core).is_empty());

    // A third decision is carried out once the platform answers, and resolves the part.
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
