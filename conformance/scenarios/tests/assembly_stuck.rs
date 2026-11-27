//! A stuck order of the assembled Core resolved through the production mover and the real engine (`T-020-16`): the
//! order runs on the simulated reserve, its deposit is never handed over and the part is declared stuck, its value
//! resting at the reserve's ledger. One operator desk decides, naming evidence the log holds about the order, and
//! another moves; the engine, over the deployment's registers and the log's records, decides each step, and the log
//! that admits every append is the real one. Nothing moves unless the engine permitted both steps, by two actors:
//! a mover the engine refuses, by chain, by permission or as the provider's own desk, hands the reserve nothing and
//! records no effect; the permitted one's move is carried out on the platform, once, and the part is resolved.

use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, Decided, Destination, Entry, Outcome, RefusalClass,
    Resolution, Role, What,
};
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::PositiveAmount;
use rutent_contracts::state_machine::{Observed, ResolvedTo};
use rutent_core_authz::roles::Permission;
use rutent_core_orch::stuck::{Moved, ResolveError, Resolved};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Given, Scenario, Step};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};
use rutent_scenarios::on_tick::OnTick;

/// An operator login: a person of its own desk, which acts for the operator.
fn staff(core: &Core, desk: &str, login: &str) -> Actor {
    let actor = Actor {
        party: id(desk),
        acts: id(login),
        role: Role::Operator,
        kind: ActorKind::Person,
    };
    let mut registers = core.registers();
    registers.list(&Actor {
        party: kit::operator().party,
        acts: id(desk),
        role: Role::Operator,
        kind: ActorKind::Component,
    });
    registers.list(&actor);
    actor
}

fn destination(core: &Core) -> Destination {
    core.verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::Authorise { authorisation, .. } => Some(authorisation.destination),
            _ => None,
        })
        .expect("the order's authorisation")
}

/// The commitment of a provider's result the log holds about `order`: the evidence a decision names, a party's word
/// (Core's own questions are no evidence, `T-020-40`).
fn evidence_about(
    core: &Core,
    order: &rutent_contracts::router_api::Id,
) -> Vec<rutent_contracts::module_interface::Digest> {
    let log = core.log();
    let found = log
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
                    && s.about
                        .as_ref()
                        .is_some_and(|a| a.order.as_ref() == Some(order)) =>
            {
                Some(s.commitment.clone())
            }
            _ => None,
        })
        .expect("a statement about the order");
    vec![found]
}

fn effects(core: &Core) -> usize {
    core.verified_whats()
        .iter()
        .filter(|w| {
            matches!(
                w,
                What::Effect {
                    action: Action::ResolveStuck { .. },
                    ..
                }
            )
        })
        .count()
}

fn moves_by_the_engine(core: &Core) -> Vec<Outcome> {
    core.log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) if matches!(v.what, What::StuckMove { .. }) => Some(v.outcome),
            _ => None,
        })
        .collect()
}

/// A stuck order of the assembled Core is resolved through the production mover, on the real engine's permits for
/// both steps. The engine refuses a mover whose chain reaches the decider's party (`same_actor`), one holding no
/// permission to move, and the desk of the provider holding the funds: each is answered not permitted, no effect is
/// recorded and the reserve is handed nothing. The mover it permits carries the decision out: the reserve receives
/// one deposit to the order's destination, under the transaction the decision named, and the part is resolved at its
/// destination, once.
// verifies: REQ-AUZ-4, AC-6, REQ-ORD-28
#[test]
fn stuck_order_of_the_assembled_core_is_resolved_through_the_mover_and_the_real_engine() {
    let (mut core, orders) = Scenario::new(
        "a stuck order, resolved",
        "AC-6",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .core(1, &mut Reference);
    let order = orders[0].clone();
    let part = core.first_part(&order).expect("a started order");
    // The withdrawal settles; the platform won't begin the deposit: its value rests at the reserve's ledger.
    core.faults.refuse_begin = Some(1);
    core.ran_on_tick(&order).expect("the order runs");
    core.leave_no_route(&order).expect("the part is stuck");
    assert!(core.is_stuck(&order, &part));

    // Who acts. Alice's desk decides; bob's moves; carol's desk acts for alice's, so carol is alice's by chain;
    // dave holds no permission to move; the reserve's own desk may not move what the reserve holds.
    let alice = staff(&core, "desk-a", "alice");
    let bob = staff(&core, "desk-b", "bob");
    let carol = staff(&core, "desk-c", "carol");
    let dave = staff(&core, "desk-d", "dave");
    let reserve_desk = Actor {
        party: id(RESERVE),
        acts: id("reserve-desk-1"),
        role: Role::Operator,
        kind: ActorKind::Person,
    };
    {
        let mut registers = core.registers();
        registers.list(&Actor {
            party: id("desk-a"),
            acts: id("desk-c"),
            role: Role::Operator,
            kind: ActorKind::Component,
        });
        registers.list(&reserve_desk);
        registers.permit(&alice, Permission::DecideStuck);
        for mover in [&bob, &carol, &reserve_desk] {
            registers.permit(mover, Permission::MoveStuck);
        }
    }
    let resolution = Resolution::Deliver {
        by: id(RESERVE),
        to: destination(&core),
        asset: Denomination {
            id: "COP".into(),
            decimals: 2,
        },
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    let evidence = evidence_about(&core, &order);

    // A decider the engine doesn't permit (no permission to decide) decides nothing.
    let refused = core
        .decide_stuck_in_core(&order, &dave, &resolution, &evidence)
        .expect("asked");
    assert!(
        matches!(refused.outcome, Answer::Refuse { .. }),
        "{:?}",
        refused.outcome
    );

    let decision = core
        .decide_stuck_in_core(&order, &alice, &resolution, &evidence)
        .expect("asked");
    assert_eq!(decision.outcome, Answer::Permit {});
    let carries_out = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };

    let handled = core.operations_received(RESERVE);
    for (mover, why) in [
        (&carol, "one actor by chain"),
        (&dave, "no permission to move"),
        (&reserve_desk, "the desk of the provider holding the funds"),
    ] {
        let moved = core
            .move_stuck_in_core(&order, &carries_out, mover)
            .unwrap_or_else(|e| panic!("{why}: asked: {e:?}"));
        assert!(
            matches!(moved, Moved::NotPermitted(Answer::Refuse { .. })),
            "{why}: {moved:?}"
        );
        assert_eq!(core.operations_received(RESERVE), handled, "{why}");
        assert_eq!(effects(&core), 0, "{why}: no effect is recorded");
        assert!(core.is_stuck(&order, &part), "{why}");
    }
    // The engine's own reasons are on the log: the chain is `same_actor`.
    let refused: Vec<Outcome> = moves_by_the_engine(&core);
    assert_eq!(
        refused[0],
        Outcome::Refused {
            class: RefusalClass::SameActor
        }
    );
    assert!(
        refused[1..].iter().all(|o| *o
            == Outcome::Refused {
                class: RefusalClass::NotPermitted
            }),
        "{refused:?}"
    );

    // Core's own rule holds whatever the engine says: the decider doesn't move its own decision.
    assert!(matches!(
        core.move_stuck_in_core(&order, &carries_out, &alice),
        Err(ResolveError::SameParty)
    ));

    // The permitted mover's move is carried out on the platform, once.
    let moved = core.move_stuck_in_core(&order, &carries_out, &bob);
    let moved = moved.expect("moved");
    assert!(
        matches!(
            moved,
            Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            }
        ),
        "{moved:?}"
    );
    assert_eq!(
        core.operations_received(RESERVE),
        handled + 1,
        "the reserve is handed the deposit once"
    );
    assert_eq!(effects(&core), 1);
    assert!(!core.is_stuck(&order, &part));
    let resolved: Vec<ResolvedTo> = core
        .inputs()
        .iter()
        .filter_map(|i| match &i.observed {
            Observed::StuckResolved { to, .. } => Some(*to),
            _ => None,
        })
        .collect();
    assert_eq!(resolved, vec![ResolvedTo::Destination]);
    // Nothing moves a second time.
    assert!(core.move_stuck_in_core(&order, &carries_out, &bob).is_err());
    assert_eq!(core.operations_received(RESERVE), handled + 1);
}

/// The operator's decided commands on a stuck order are Core's own entry points (`T-020-61`, item 4): the deployment hands
/// each to `rutent_core_orch::stuck::{decide_stuck, move_stuck}` over its ports, and Core reads the stuck part from the order
/// it started, asks policy for each step and carries the move out through its mover. An order Core didn't start has no
/// stuck part to decide; the decider never moves its own decision; the permitted mover's move is carried out once.
// verifies: REQ-AUZ-4, AC-6
#[test]
fn stuck_command_is_a_core_entry_point() {
    let (mut core, orders) = Scenario::new(
        "a stuck order, resolved through Core's entry points",
        "AC-6",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .core(1, &mut Reference);
    let order = orders[0].clone();
    let part = core.first_part(&order).expect("a started order");
    core.faults.refuse_begin = Some(1);
    core.tick();
    core.leave_no_route(&order).expect("the part is stuck");
    let alice = staff(&core, "desk-a", "alice");
    let bob = staff(&core, "desk-b", "bob");
    {
        let mut registers = core.registers();
        registers.permit(&alice, Permission::DecideStuck);
        registers.permit(&bob, Permission::MoveStuck);
    }
    let resolution = Resolution::Deliver {
        by: id(RESERVE),
        to: destination(&core),
        asset: Denomination {
            id: "COP".into(),
            decimals: 2,
        },
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    let evidence = evidence_about(&core, &order);
    // No stuck part of an order Core never started.
    assert_eq!(
        rutent_core_orch::stuck::decide_stuck(
            &mut core.ports(),
            &id("ord-never-started"),
            &alice,
            &resolution,
            &evidence
        ),
        Err(ResolveError::NotStuck)
    );
    let decides = |core: &Core| {
        core.verified_whats()
            .iter()
            .filter(|w| matches!(w, What::StuckDecide { .. }))
            .count()
    };
    let before = decides(&core);
    let decision = rutent_core_orch::stuck::decide_stuck(
        &mut core.ports(),
        &order,
        &alice,
        &resolution,
        &evidence,
    )
    .expect("asked");
    assert_eq!(decision.outcome, Answer::Permit {});
    assert_eq!(decides(&core), before + 1, "policy decided it, on the log");
    let carries_out = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    assert_eq!(
        rutent_core_orch::stuck::move_stuck(&mut core.ports(), &order, &carries_out, &alice),
        Err(ResolveError::SameParty),
        "the decider doesn't move its own decision"
    );
    let handled = core.operations_received(RESERVE);
    let moved = rutent_core_orch::stuck::move_stuck(&mut core.ports(), &order, &carries_out, &bob)
        .expect("moved");
    assert!(
        matches!(
            moved,
            Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            }
        ),
        "{moved:?}"
    );
    assert_eq!(core.operations_received(RESERVE), handled + 1);
    assert!(!core.is_stuck(&order, &part));
}
