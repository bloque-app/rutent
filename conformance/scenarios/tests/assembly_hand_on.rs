//! What the assembled Core gives a stuck order's hand-on (`T-020-26`; `REQ-ORD-43`, `REQ-ORD-40`, `INV-5`): the
//! operator's holding accounts, the linked orders' accepted routes and the log's own handle, wired to the production
//! mover over the real engine and the real log. The assembly holds no plain wallet as a node's holding account yet
//! (the module, its release and a public node on a route's path are the kit's to give; see `F-020`'s notes), so a
//! hand-on here, for a linked order the log holds no acceptance of, is refused by the log and nothing is handed to
//! anyone.

use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, Decided, Entry, Resolution, Role, What,
};
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::PositiveAmount;
use rutent_contracts::state_machine::{self as stm, OperationKind, PartPlan, SegmentPlan};
use rutent_core_authz::roles::Permission;
use rutent_core_orch::stuck::{AcceptedRoute, LogRead};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Given, Scenario, Step};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};
use rutent_scenarios::on_tick::OnTick;

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

fn hand_on_effects(core: &Core) -> usize {
    core.verified_whats()
        .iter()
        .filter(|w| {
            matches!(
                w,
                What::Effect {
                    action: Action::ResolveStuck {
                        resolution: Resolution::HandOn { .. },
                        ..
                    },
                    ..
                }
            )
        })
        .count()
}

/// A stuck order of the assembled Core is handed on where the operator decided, by the production mover wired with
/// the assembly's holding accounts, accepted routes and log: with no holding account at the node, and a linked order
/// the log holds no acceptance of, the mover answers `Failed` before anything is handed to anyone, the part stays
/// stuck, and the reserve receives nothing. The engine decided and permitted both steps; the effect is recorded and
/// its result is failed.
// verifies: REQ-ORD-43, REQ-AUZ-4
#[test]
fn stuck_order_of_the_assembled_core_is_handed_on_only_where_a_holding_account_holds_it() {
    let (mut core, orders) = Scenario::new(
        "a stuck order, handed on",
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
    core.ran_on_tick(&order).expect("the order runs");
    // Core's own run leaves the part with no route (`REQ-ORD-28`, `T-020-61`, the review's L6): the reserve refuses the
    // deposit's dry run as Core recalculates the part from its ledger. No test caller declares it stuck.
    core.world()
        .script_dry_run(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    for _ in 0..4 {
        if core.is_stuck(&order, &part) {
            break;
        }
        core.tick();
    }
    assert!(core.is_stuck(&order, &part));

    let (alice, bob) = (
        staff(&core, "desk-a", "alice"),
        staff(&core, "desk-b", "bob"),
    );
    {
        let mut registers = core.registers();
        registers.permit(&alice, Permission::DecideStuck);
        registers.permit(&bob, Permission::MoveStuck);
    }
    // The linked order's accepted route, which the mover holds the hand-on's route to: starting where the value rests.
    let ledger = serde_json::from_value(kit::ledger(RESERVE)).unwrap();
    let bank = serde_json::from_value(kit::bank(RESERVE)).unwrap();
    core.accepted_linked(AcceptedRoute {
        order: id("ord-2"),
        decision_record: id("dr-linked-ord-2"),
        acceptance: id("acc-ord-2"),
        plan: vec![PartPlan {
            part_id: stm::Id::new(part.as_str()).unwrap(),
            segments: vec![SegmentPlan {
                segment_id: stm::Id::new("p1-s1-of-2").unwrap(),
                provider: stm::Id::new(RESERVE).unwrap(),
                kind: OperationKind::Deposit,
                from: ledger,
                to: bank,
            }],
        }],
    });
    let resolution = Resolution::HandOn {
        by: id(RESERVE),
        linked_order: id("ord-2"),
        asset: Denomination {
            id: "COP".into(),
            decimals: 2,
        },
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    let evidence = evidence_about(&core, &order);
    let decision = core
        .decide_stuck_in_core(&order, &alice, &resolution, &evidence)
        .expect("asked");
    assert_eq!(decision.outcome, Answer::Permit {}, "{decision:?}");
    let carries_out = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    let handled = core.operations_received(RESERVE);
    // The log admits a hand-on's effect only for a linked order it holds an acceptance of: the move is refused at the
    // log, before the mover is asked, so no effect is recorded and nothing is handed to anyone.
    let moved = core.move_stuck_in_core(&order, &carries_out, &bob);
    assert!(
        matches!(
            &moved,
            Err(rutent_core_orch::stuck::ResolveError::Append(
                    rutent_contracts::core_internal::calls::AppendFailure::Refused(why)
                )) if why.0 == "a stuck order's value is handed on only to an order accepted in this log"
        ),
        "{moved:?}"
    );
    assert_eq!(hand_on_effects(&core), 0, "no effect is recorded");
    assert_eq!(
        core.operations_received(RESERVE),
        handled,
        "the reserve is handed nothing"
    );
    assert!(core.is_stuck(&order, &part), "the part stays held");
    assert!(
        core.handed_on_on_tick(&order).is_err(),
        "no withdrawal is done, so no hand-on is recorded"
    );
}

/// What the mover reads the linked order's acceptance and decision record from is the log's own handle: an entry
/// appended after the handle was made is read through it, which a copy of the entries taken then would not show.
// verifies: REQ-ORD-43
#[test]
fn the_assemblys_read_of_the_log_is_the_log_handle_itself() {
    let (mut core, orders) =
        Scenario::new("the log handle", "AC-6", Given::one_reserve(Staged::AtOnce))
            .when(Step::Order {
                request: "req-1".to_owned(),
                amount: AMOUNT.to_owned(),
            })
            .core(1, &mut Reference);
    let read = core.log_read();
    let before = read.entries().len();
    core.ran_on_tick(&orders[0]).expect("the order runs");
    let after = read.entries().len();
    assert!(
        after > before,
        "what the log took since is read through the handle"
    );
    assert_eq!(after, core.log().entries().len());
}
