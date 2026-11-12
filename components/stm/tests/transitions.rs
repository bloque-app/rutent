//! The transition rules, one scenario each: what the state machine does with the inputs the contract lists.

mod common;

use common::*;
use rutent_contracts::state_machine::{
    NotApplicableReason as Why, Observed, Order, OrderState, Outcome, ResolvedTo, SegmentState,
    StateMachine, StuckReason,
};
use rutent_stm::{
    HandOnRefusal, Stm, handed_on_route_starts_where_value_sits, is_ended, order_state_of,
};

fn na(reason: Why) -> Outcome {
    Outcome::NotApplicable { reason }
}

/// An order whose first two segments settled and whose final deposit failed, so the value sits at `ledger`.
fn failed_final_deposit(order: &str) -> Run {
    let mut run = Run::opened(order);
    run.run_segments("p1", 1, 2);
    assert_eq!(run.bind("p1", "p1-s3"), Outcome::Applied {});
    assert_eq!(run.fail("p1", "p1-s3"), Outcome::Applied {});
    run
}

/// The same, with no route left: the part waits for the customer.
fn waiting(order: &str) -> Run {
    let mut run = failed_final_deposit(order);
    assert_eq!(run.about_part("p1", no_route()), Outcome::Applied {});
    assert_eq!(run.part_state("p1"), OrderState::WaitingForCustomer);
    assert_eq!(
        run.part("p1").waiting_since,
        Some(at(run.tick)),
        "it waits since the input that made it wait"
    );
    run
}

fn complete(order: &str) -> Run {
    let mut run = Run::opened(order);
    run.run_segments("p1", 1, 3);
    run
}

fn same_but_last_input(before: &Order, after: &Order) -> bool {
    let mut a = before.clone();
    a.last_input = after.last_input.clone();
    a == *after
}

// verifies: REQ-STM-5
#[test]
fn a_provider_that_never_answers_leaves_its_segment_unresolved() {
    let mut run = Run::opened("ord-silent");
    run.run_segments("p1", 1, 1);
    assert_eq!(run.bind("p1", "p1-s2"), Outcome::Applied {});
    // Time passes: inputs recorded days later, and the part declared stuck over time. None is an outcome.
    let mut late = run.input(
        Some("p1"),
        None,
        Observed::StuckDeclared {
            reason: StuckReason::OverTime,
        },
    );
    late.recorded_at =
        rutent_contracts::state_machine::Timestamp::new("2099-01-01T00:00:00Z").unwrap();
    assert_eq!(run.feed(&late), Outcome::Applied {});
    assert_eq!(run.part_state("p1"), OrderState::Stuck);
    assert_eq!(run.segment_state("p1", "p1-s2"), SegmentState::Started);
    // Only a report about the segment resolves it, and a report of unknown outcome is still not one.
    assert_eq!(
        run.about_segment("p1", "p1-s2", Observed::SegmentOutcomeUnknown {}),
        Outcome::Applied {}
    );
    assert_eq!(
        run.segment_state("p1", "p1-s2"),
        SegmentState::OutcomeUnknown
    );
    assert_eq!(run.part_state("p1"), OrderState::Stuck);
}

// verifies: REQ-STM-5, REQ-STM-3
#[test]
fn the_transition_reads_no_time() {
    let mut run = Run::opened("ord-clock");
    let bound = run.input(Some("p1"), Some("p1-s1"), Observed::ProviderBound {});
    let mut restamped = bound.clone();
    restamped.recorded_at =
        rutent_contracts::state_machine::Timestamp::new("2031-05-05T05:05:05Z").unwrap();
    let before = run.order().clone();
    assert_eq!(
        Stm::transition(&before, &bound),
        Stm::transition(&before, &restamped)
    );
}

// verifies: REQ-STM-6
#[test]
fn an_ended_order_changes_only_when_its_starting_segment_is_proven_undone() {
    let endings: Vec<(&str, Run, OrderState)> = {
        let mut ended = Run::opened("ord-ended");
        assert_eq!(
            ended.order_wide(Observed::EndRequested {}),
            Outcome::Applied {}
        );
        let mut handed = waiting("ord-handed");
        let hand = Observed::HandedOn {
            linked_order: id("ord-next"),
            acceptance: id("acc-1"),
        };
        assert_eq!(handed.about_part("p1", hand), Outcome::Applied {});
        let mut failed = complete("ord-failed");
        assert_eq!(failed.undo("p1", "p1-s1"), Outcome::Applied {});
        vec![
            ("complete", complete("ord-complete"), OrderState::Complete),
            ("ended", ended, OrderState::EndedWithoutCompleting),
            ("handed on", handed, OrderState::HandedOn),
            ("failed", failed, OrderState::Failed),
        ]
    };
    for (name, mut run, expected) in endings {
        assert_eq!(run.order().state, expected, "{name}");
        let reports = [
            Observed::ProviderBound {},
            Observed::SegmentFailed {},
            Observed::SegmentOutcomeUnknown {},
            Observed::SegmentSettled {
                evidence: id("ev-late"),
            },
        ];
        for report in reports {
            let before = run.order().clone();
            assert_eq!(
                run.about_segment("p1", "p1-s3", report),
                na(Why::Ended),
                "{name}"
            );
            assert!(
                same_but_last_input(&before, run.order()),
                "{name}: only last_input changes"
            );
        }
        for observed in [
            Observed::EndRequested {},
            Observed::WithdrawalCancelled {},
            Observed::StuckDeclared {
                reason: StuckReason::Dispute,
            },
            no_route(),
        ] {
            let before = run.order().clone();
            let outcome = match observed.scope() {
                rutent_contracts::state_machine::Scope::Order => run.order_wide(observed),
                _ => run.about_part("p1", observed),
            };
            assert_eq!(outcome, na(Why::Ended), "{name}");
            assert!(same_but_last_input(&before, run.order()), "{name}");
        }
        // A report on a segment the order never had is recorded as ended too: the order is closed to anything.
        assert_eq!(run.bind("p1", "nothing"), na(Why::Ended), "{name}");
    }
}

// verifies: REQ-STM-6, REQ-ORD-12, AC-8
#[test]
fn a_complete_order_becomes_failed_when_its_starting_segment_is_proven_undone() {
    let mut run = complete("ord-undone");
    // Another segment's undo is not the starting segment's.
    assert_eq!(run.undo("p1", "p1-s2"), na(Why::Ended));
    assert_eq!(run.order().state, OrderState::Complete);
    assert_eq!(run.undo("p1", "p1-s1"), Outcome::Applied {});
    assert_eq!(run.order().state, OrderState::Failed);
    assert_eq!(run.part_state("p1"), OrderState::Failed);
    assert_eq!(
        run.segment_state("p1", "p1-s1"),
        SegmentState::ReversalComplete
    );
    run.order().check().expect("the order stays valid");
}

// verifies: REQ-ORD-12
#[test]
fn an_order_may_end_until_its_first_segment_settles_and_not_after() {
    let mut early = Run::opened("ord-early");
    assert_eq!(
        early.order_wide(Observed::EndRequested {}),
        Outcome::Applied {}
    );
    assert_eq!(early.order().state, OrderState::EndedWithoutCompleting);

    let mut late = Run::opened("ord-late");
    late.run_segments("p1", 1, 1);
    assert_eq!(
        late.order_wide(Observed::EndRequested {}),
        na(Why::NotFromThisState)
    );
    assert_eq!(late.order().state, OrderState::FirstSegmentSettled);
}

// verifies: REQ-ORD-40, REQ-ORD-12
#[test]
fn a_cancelled_withdrawal_ends_the_order_and_a_settled_one_is_not_cancelled() {
    let mut run = Run::opened("ord-cancel");
    assert_eq!(run.bind("p1", "p1-s1"), Outcome::Applied {});
    assert_eq!(
        run.about_part("p1", Observed::WithdrawalCancelled {}),
        Outcome::Applied {}
    );
    assert_eq!(run.order().state, OrderState::EndedWithoutCompleting);
    // A later settlement of that withdrawal is recorded, and changes nothing.
    assert_eq!(run.settle("p1", "p1-s1"), na(Why::Ended));

    let mut settled = Run::opened("ord-settled");
    settled.run_segments("p1", 1, 1);
    assert_eq!(
        settled.about_part("p1", Observed::WithdrawalCancelled {}),
        na(Why::NotFromThisState)
    );
}

// verifies: REQ-STM-4
#[test]
fn an_input_delivered_again_is_a_repeat_and_changes_nothing() {
    let mut run = Run::opened("ord-repeat");
    let bind = run.input(Some("p1"), Some("p1-s1"), Observed::ProviderBound {});
    assert_eq!(run.feed(&bind), Outcome::Applied {});
    let before = run.order().clone();
    assert_eq!(run.feed(&bind), Outcome::Repeat {});
    assert!(same_but_last_input(&before, run.order()));
    assert_eq!(run.order().last_input.record_id, bind.record_id);
}

// verifies: REQ-STM-4
#[test]
fn a_report_about_a_retired_segment_is_late() {
    let mut run = failed_final_deposit("ord-late-report");
    assert_eq!(
        run.about_part("p1", recalculated(route_from_ledger("r1"))),
        Outcome::Applied {}
    );
    assert!(
        run.part("p1")
            .retired_segments
            .iter()
            .any(|s| s.as_str() == "p1-s3")
    );
    assert_eq!(run.settle("p1", "p1-s3"), Outcome::Late {});
    assert_eq!(run.settle("p1", "never-had"), na(Why::UnknownSubject));
}

// verifies: REQ-ORD-28, REQ-ORD-43
#[test]
fn no_route_left_sticks_a_part_and_makes_it_wait_where_its_deposit_failed() {
    // Value in the ledger after a failed final deposit: waits for the customer.
    let run = waiting("ord-wait");
    assert_eq!(run.order().state, OrderState::WaitingForCustomer);

    // A reversed middle segment, with no failed deposit: stuck.
    let mut stuck = Run::opened("ord-stuck");
    stuck.run_segments("p1", 1, 1);
    stuck.bind("p1", "p1-s2");
    stuck.fail("p1", "p1-s2");
    assert_eq!(
        stuck.about_part("p1", no_route()),
        na(Why::NotFromThisState),
        "value still in flight"
    );
    assert_eq!(stuck.undo("p1", "p1-s2"), Outcome::Applied {});
    assert_eq!(stuck.about_part("p1", no_route()), Outcome::Applied {});
    assert_eq!(stuck.order().state, OrderState::Stuck);

    // Nothing has moved before the withdrawal settles: no route left is not applicable.
    let mut fresh = Run::opened("ord-fresh");
    assert_eq!(
        fresh.about_part("p1", no_route()),
        na(Why::NotFromThisState)
    );
}

// verifies: REQ-STM-3, REQ-STM-6
#[test]
fn a_dispute_makes_the_order_stuck_and_a_late_settlement_can_complete_it() {
    let mut run = Run::opened("ord-dispute");
    run.run_segments("p1", 1, 2);
    run.bind("p1", "p1-s3");
    let dispute = Observed::StuckDeclared {
        reason: StuckReason::Dispute,
    };
    assert_eq!(run.about_part("p1", dispute.clone()), Outcome::Applied {});
    assert_eq!(run.order().state, OrderState::Stuck);
    assert_eq!(run.about_part("p1", dispute), na(Why::NotFromThisState));
    assert_eq!(run.settle("p1", "p1-s3"), Outcome::Applied {});
    assert_eq!(run.order().state, OrderState::Complete);
}

// verifies: REQ-ORD-43, INV-10
#[test]
fn after_a_failed_final_deposit_a_recalculation_keeps_the_destination_and_a_change_needs_acceptance()
 {
    let mut run = waiting("ord-dest");
    let destination = run.part("p1").destination.clone();
    // The same destination by another route is a recalculation; as a destination change it is refused.
    assert_eq!(
        run.about_part("p1", destination_changed(route_from_ledger("c1"))),
        na(Why::KeepsDestination)
    );
    // Another destination is not a recalculation.
    assert_eq!(
        run.about_part("p1", recalculated(route_to_another_destination("c2"))),
        na(Why::ChangesDestination)
    );
    assert_eq!(run.part("p1").destination, destination);
    assert_eq!(run.part_state("p1"), OrderState::WaitingForCustomer);
    // The customer's accepted change replaces it, and clears where the deposit failed.
    assert_eq!(
        run.about_part(
            "p1",
            destination_changed(route_to_another_destination("c3"))
        ),
        Outcome::Applied {}
    );
    assert_ne!(run.part("p1").destination, destination);
    assert_eq!(run.part("p1").deposit_failed_at, None);
    assert_eq!(run.part_state("p1"), OrderState::FirstSegmentSettled);
    run.order().check().expect("the order stays valid");
}

// verifies: INV-4, REQ-ORD-2
#[test]
fn a_route_restarts_only_from_where_the_value_is_evidenced() {
    // The deposit is started: the value is in flight, so no route starts.
    let mut run = Run::opened("ord-restart");
    run.run_segments("p1", 1, 2);
    run.bind("p1", "p1-s3");
    assert_eq!(
        run.about_part("p1", recalculated(route_from_ledger("r1"))),
        na(Why::ValueNotAtStart)
    );
    // Its outcome is unknown: still in flight.
    run.about_segment("p1", "p1-s3", Observed::SegmentOutcomeUnknown {});
    assert_eq!(
        run.about_part("p1", recalculated(route_from_ledger("r2"))),
        na(Why::ValueNotAtStart)
    );
    // Failed: the value is at the deposit's node, and a route from anywhere else is refused.
    run.fail("p1", "p1-s3");
    assert_eq!(
        run.about_part("p1", recalculated(handed_on_route("r3", "elsewhere"))),
        na(Why::ValueNotAtStart)
    );
    assert_eq!(
        run.about_part("p1", recalculated(route_from_ledger("r4"))),
        Outcome::Applied {}
    );
    assert_eq!(run.segment_state("p1", "r4-s1"), SegmentState::Pending);
    assert_eq!(run.part("p1").segments.len(), 4);
    // Before the withdrawal settles nothing has moved, and no route starts.
    let mut fresh = Run::opened("ord-fresh");
    assert_eq!(
        fresh.about_part("p1", recalculated(route_via("f1", "exchange-b"))),
        na(Why::ValueNotAtStart)
    );
}

// verifies: INV-10
#[test]
fn the_reasons_a_route_is_refused_come_in_the_published_order() {
    let mut run = failed_final_deposit("ord-precedence");
    // Changes the destination and reuses a segment id: the destination is named first.
    let mut route = route_to_another_destination("p1");
    route[0].segment_id = id("p1-s1");
    assert_eq!(
        run.about_part("p1", recalculated(route)),
        na(Why::ChangesDestination)
    );
    // Same destination, reuses an id, and starts away from the value: the id is named first.
    let mut route = handed_on_route("p1", "elsewhere");
    route[0].segment_id = id("p1-s1");
    assert_eq!(
        run.about_part("p1", recalculated(route)),
        na(Why::ReusesSegmentId)
    );
}

// verifies: REQ-STM-7
#[test]
fn two_alternative_routes_from_one_state_are_each_taken_when_recorded_and_a_route_changes_no_value()
{
    let mut a = failed_final_deposit("ord-alt");
    let mut b = failed_final_deposit("ord-alt");
    assert_eq!(a.state, b.state);
    assert_eq!(
        a.about_part("p1", recalculated(route_from_ledger("a"))),
        Outcome::Applied {}
    );
    assert_eq!(
        b.about_part("p1", recalculated(handed_on_route("b", "ledger"))),
        Outcome::Applied {}
    );
    assert_ne!(a.part("p1").segments, b.part("p1").segments);
    // The segment that moves between nodes keeps the asset: a transition that changes no value.
    let moved = a
        .part("p1")
        .segments
        .iter()
        .find(|s| s.segment_id.as_str() == "a-s1")
        .unwrap();
    assert_eq!(moved.from.asset, moved.to.asset);
    assert_ne!(moved.from.node, moved.to.node);
}

// verifies: REQ-ORD-43
#[test]
fn a_waiting_part_is_handed_on_to_a_linked_order_and_the_first_order_ends() {
    let mut run = waiting("ord-first");
    let hand = Observed::HandedOn {
        linked_order: id("ord-second"),
        acceptance: id("acc-1"),
    };
    assert_eq!(run.about_part("p1", hand), Outcome::Applied {});
    assert_eq!(run.order().state, OrderState::HandedOn);
    assert_eq!(run.part("p1").linked_order, Some(id("ord-second")));
    run.order().check().expect("the order stays valid");
    // A part that isn't waiting or stuck is not handed on.
    let mut fresh = Run::opened("ord-fresh");
    let hand = Observed::HandedOn {
        linked_order: id("ord-second"),
        acceptance: id("acc-1"),
    };
    assert_eq!(fresh.about_part("p1", hand), na(Why::NotFromThisState));
}

// verifies: REQ-ORD-43, INV-4
#[test]
fn handed_on_route_starting_away_from_the_failed_deposit_is_refused() {
    let first = waiting("ord-first").order().clone();

    let mut right = Run::new("ord-second");
    right.choose(
        vec![part("q1", handed_on_route("q1", "ledger"))],
        Some("ord-first"),
    );
    assert_eq!(
        handed_on_route_starts_where_value_sits(&first, right.order()),
        Ok(())
    );

    let mut away = Run::new("ord-third");
    away.choose(
        vec![part("q1", handed_on_route("q1", "elsewhere"))],
        Some("ord-first"),
    );
    assert_eq!(
        handed_on_route_starts_where_value_sits(&first, away.order()),
        Err(HandOnRefusal::StartsAwayFromTheValue)
    );

    let mut unlinked = Run::new("ord-fourth");
    unlinked.choose(
        vec![part("q1", handed_on_route("q1", "ledger"))],
        Some("ord-other"),
    );
    assert_eq!(
        handed_on_route_starts_where_value_sits(&first, unlinked.order()),
        Err(HandOnRefusal::NotLinked)
    );

    let mut fresh = Run::new("ord-fifth");
    fresh.choose(vec![part("q1", route("q1"))], None);
    assert_eq!(
        handed_on_route_starts_where_value_sits(&first, fresh.order()),
        Err(HandOnRefusal::NotLinked)
    );

    // An order that holds no value at a node has nothing to hand on.
    let delivered = complete("ord-first").order().clone();
    let mut to_delivered = Run::new("ord-sixth");
    to_delivered.choose(
        vec![part("q1", handed_on_route("q1", "ledger"))],
        Some("ord-first"),
    );
    assert_eq!(
        handed_on_route_starts_where_value_sits(&delivered, to_delivered.order()),
        Err(HandOnRefusal::NoValueToHandOn)
    );
}

// verifies: REQ-ORD-43
#[test]
fn the_rule_across_two_orders_holds_after_the_value_was_handed_on() {
    let mut run = waiting("ord-first");
    let hand = Observed::HandedOn {
        linked_order: id("ord-second"),
        acceptance: id("acc-1"),
    };
    assert_eq!(run.about_part("p1", hand), Outcome::Applied {});
    let mut second = Run::new("ord-second");
    second.choose(
        vec![part("q1", handed_on_route("q1", "ledger"))],
        Some("ord-first"),
    );
    assert_eq!(
        handed_on_route_starts_where_value_sits(run.order(), second.order()),
        Ok(())
    );
    // Handed on to a different order: this one is not the one the value went to.
    let mut other = Run::new("ord-other");
    other.choose(
        vec![part("q1", handed_on_route("q1", "ledger"))],
        Some("ord-first"),
    );
    assert_eq!(
        handed_on_route_starts_where_value_sits(run.order(), other.order()),
        Err(HandOnRefusal::NoValueToHandOn)
    );
}

// verifies: REQ-ORD-42, REQ-STM-1
#[test]
fn an_order_of_two_parts_closes_only_when_both_have_ended() {
    let mut run = Run::new("ord-split");
    run.choose(vec![part("a", route("a")), part("b", route("b"))], None);
    assert_eq!(run.order().state, OrderState::NotPastFirstSegment);
    run.run_segments("a", 1, 3);
    assert_eq!(run.part_state("a"), OrderState::Complete);
    assert_eq!(
        run.order().state,
        OrderState::FirstSegmentSettled,
        "b has not ended"
    );
    // One part sticks while the other is complete: the order is stuck, and the complete part is untouched.
    assert_eq!(
        run.about_part(
            "b",
            Observed::StuckDeclared {
                reason: StuckReason::OverTime
            }
        ),
        Outcome::Applied {}
    );
    assert_eq!(run.order().state, OrderState::Stuck);
    assert_eq!(run.part_state("a"), OrderState::Complete);

    // The other order: a is complete and b's withdrawal is cancelled, so b ends by itself and the order ends
    // without completing, with a untouched.
    let mut other = Run::new("ord-split-2");
    other.choose(vec![part("a", route("a")), part("b", route("b"))], None);
    other.run_segments("a", 1, 3);
    other.bind("b", "b-s1");
    assert_eq!(
        other.about_part("b", Observed::WithdrawalCancelled {}),
        Outcome::Applied {}
    );
    assert_eq!(other.part_state("a"), OrderState::Complete);
    assert_eq!(other.part_state("b"), OrderState::EndedWithoutCompleting);
    assert_eq!(other.order().state, OrderState::EndedWithoutCompleting);
    other.order().check().expect("the order stays valid");
}

// verifies: REQ-STM-1, REQ-ORD-42
#[test]
fn the_order_state_follows_its_parts_by_one_rule() {
    use OrderState::*;
    let states = |parts: &[OrderState]| -> OrderState {
        let mut run = Run::new("ord-rule");
        let plans = (0..parts.len())
            .map(|i| part(&format!("p{i}"), route(&format!("p{i}"))))
            .collect();
        run.choose(plans, None);
        let mut order = run.order().clone();
        for (part, state) in order.parts.iter_mut().zip(parts) {
            part.state = *state;
        }
        order_state_of(&order.parts)
    };
    // One part: the part's own state.
    for state in OrderState::ALL {
        assert_eq!(states(&[state]), state);
    }
    // Several parts: open while any has not ended; stuck over waiting over progress.
    assert_eq!(states(&[Complete, Stuck]), Stuck);
    assert_eq!(states(&[WaitingForCustomer, Stuck]), Stuck);
    assert_eq!(states(&[Complete, WaitingForCustomer]), WaitingForCustomer);
    assert_eq!(
        states(&[Complete, NotPastFirstSegment]),
        FirstSegmentSettled
    );
    assert_eq!(
        states(&[NotPastFirstSegment, NotPastFirstSegment]),
        NotPastFirstSegment
    );
    assert_eq!(
        states(&[EndedWithoutCompleting, NotPastFirstSegment]),
        NotPastFirstSegment
    );
    // Every part ended: failed over handed on over ended without completing over complete.
    assert_eq!(states(&[Complete, Complete]), Complete);
    assert_eq!(
        states(&[Complete, EndedWithoutCompleting]),
        EndedWithoutCompleting
    );
    assert_eq!(states(&[Complete, HandedOn]), HandedOn);
    assert_eq!(states(&[HandedOn, Failed, Complete]), Failed);
    for state in OrderState::ALL {
        assert_eq!(
            is_ended(state),
            matches!(state, Complete | Failed | HandedOn | EndedWithoutCompleting)
        );
    }
}

// verifies: REQ-STM-2
#[test]
fn every_state_of_the_published_lists_is_reached() {
    let mut orders = Vec::new();
    orders.push(Run::opened("o1")); // not past first segment
    let mut run = Run::opened("o2");
    run.run_segments("p1", 1, 1);
    orders.push(run); // first segment settled, segments pending, settled
    orders.push(waiting("o3")); // waiting, failed reversal owed
    let mut run = Run::opened("o4");
    run.about_part(
        "p1",
        Observed::StuckDeclared {
            reason: StuckReason::Dispute,
        },
    );
    orders.push(run); // stuck
    orders.push(complete("o5"));
    let mut run = complete("o6");
    run.undo("p1", "p1-s1");
    orders.push(run); // failed, reversal complete
    let mut run = waiting("o7");
    run.about_part(
        "p1",
        Observed::HandedOn {
            linked_order: id("o8"),
            acceptance: id("acc"),
        },
    );
    orders.push(run); // handed on
    let mut run = Run::opened("o9");
    run.order_wide(Observed::EndRequested {});
    orders.push(run); // ended without completing
    let mut run = Run::opened("o10");
    run.run_segments("p1", 1, 1);
    run.bind("p1", "p1-s2");
    run.about_segment("p1", "p1-s2", Observed::SegmentOutcomeUnknown {});
    orders.push(run); // started, outcome unknown

    let mut order_states = Vec::new();
    let mut segment_states = Vec::new();
    for run in &orders {
        order_states.push(run.order().state);
        for part in &run.order().parts {
            order_states.push(part.state);
            segment_states.extend(part.segments.iter().map(|s| s.state));
        }
    }
    for state in OrderState::ALL {
        assert!(order_states.contains(&state), "{state:?}");
    }
    let mut run = Run::opened("o11");
    run.bind("p1", "p1-s1");
    segment_states.push(run.segment_state("p1", "p1-s1")); // started
    for state in SegmentState::ALL {
        assert!(segment_states.contains(&state), "{state:?}");
    }
}

// verifies: REQ-STM-8
#[test]
fn an_observed_kind_outside_the_closed_list_is_refused() {
    let free_form = r#"{"kind":"run_this_script","script":"anything"}"#;
    assert!(serde_json::from_str::<Observed>(free_form).is_err());
}

// verifies: REQ-STM-1
#[test]
fn an_input_about_another_order_changes_nothing_and_says_so() {
    let mut run = Run::opened("ord-mine");
    let mut stray = run.input(Some("p1"), Some("p1-s1"), Observed::ProviderBound {});
    stray.subject.order = id("ord-theirs");
    let before = run.order().clone();
    assert_eq!(run.feed(&stray), na(Why::OtherOrder));
    assert!(same_but_last_input(&before, run.order()));
}

// verifies: REQ-STM-3
#[test]
fn only_a_route_chosen_input_opens_an_order() {
    let mut run = Run::new("ord-open");
    let bind = run.input(Some("p1"), Some("p1-s1"), Observed::ProviderBound {});
    assert_eq!(Stm::open(&bind), None);
    let chosen = run.input(
        None,
        None,
        Observed::RouteChosen {
            decision_record: id("dr"),
            parts: vec![part("p1", route("p1"))],
            handed_on_from: None,
        },
    );
    let opened = Stm::open(&chosen).expect("a chosen route opens the order");
    opened.check().expect("it is valid");
    assert_eq!(opened.last_record, chosen.record_id);
    assert!(
        opened.parts[0]
            .segments
            .iter()
            .all(|s| s.state == SegmentState::Pending)
    );
    // A route that breaks the contract's rules opens nothing.
    let mut broken = chosen.clone();
    if let Observed::RouteChosen { parts, .. } = &mut broken.observed {
        parts[0].segments.pop();
    }
    assert_eq!(Stm::open(&broken), None);
    // A route chosen again for an open order changes nothing.
    run.feed(&chosen);
    let again = run.input(
        None,
        None,
        Observed::RouteChosen {
            decision_record: id("dr-2"),
            parts: vec![part("p2", route("p2"))],
            handed_on_from: None,
        },
    );
    assert_eq!(run.feed(&again), na(Why::NotFromThisState));
}

// verifies: REQ-STM-6, AC-8
#[test]
fn a_part_that_has_not_completed_is_not_failed_by_its_starting_segment_being_undone() {
    let mut run = Run::opened("ord-open-undo");
    run.run_segments("p1", 1, 1);
    assert_eq!(run.bind("p1", "p1-s2"), Outcome::Applied {});
    let before = run.order().clone();
    assert_eq!(run.undo("p1", "p1-s1"), na(Why::NotFromThisState));
    assert!(same_but_last_input(&before, run.order()));
    assert_eq!(run.part_state("p1"), OrderState::FirstSegmentSettled);
    assert_eq!(run.segment_state("p1", "p1-s1"), SegmentState::Settled);

    let mut stuck = Run::opened("ord-stuck-undo");
    stuck.run_segments("p1", 1, 1);
    stuck.bind("p1", "p1-s2");
    stuck.about_part(
        "p1",
        Observed::StuckDeclared {
            reason: StuckReason::Dispute,
        },
    );
    assert_eq!(stuck.undo("p1", "p1-s1"), na(Why::NotFromThisState));
    assert_eq!(stuck.part_state("p1"), OrderState::Stuck);
}

// verifies: REQ-STM-6
#[test]
fn a_handed_on_order_does_not_change_when_a_complete_sibling_part_is_undone() {
    let mut run = Run::new("ord-handed-split");
    run.choose(vec![part("a", route("a")), part("b", route("b"))], None);
    run.run_segments("a", 1, 3);
    run.run_segments("b", 1, 2);
    run.bind("b", "b-s3");
    run.fail("b", "b-s3");
    run.about_part("b", no_route());
    let hand = Observed::HandedOn {
        linked_order: id("ord-next"),
        acceptance: id("acc"),
    };
    assert_eq!(run.about_part("b", hand), Outcome::Applied {});
    assert_eq!(run.order().state, OrderState::HandedOn);
    let before = run.order().clone();
    assert_eq!(run.undo("a", "a-s1"), na(Why::Ended));
    assert!(same_but_last_input(&before, run.order()));
    assert_eq!(run.order().state, OrderState::HandedOn);
}

/// An order whose withdrawal and exchange settled, its value at `ledger` in USDC, waiting past the customer's floor.
fn at_its_floor(order: &str) -> Run {
    let mut run = Run::opened(order);
    run.run_segments("p1", 1, 2);
    assert_eq!(
        run.about_part("p1", Observed::WaitingAtFloor {}),
        Outcome::Applied {}
    );
    run
}

// verifies: REQ-ORD-27, AC-12, AC-42
#[test]
fn a_part_past_its_floor_waits_for_the_customer_with_its_age_and_no_provider_starts() {
    let mut run = at_its_floor("ord-floor");
    let since = at(run.tick);
    assert_eq!(run.part_state("p1"), OrderState::WaitingForCustomer);
    assert_eq!(run.order().state, OrderState::WaitingForCustomer);
    assert_eq!(run.part("p1").waiting_since, Some(since.clone()));
    run.order().check().expect("the order stays valid");

    // Waiting already: nothing changes, and the wait keeps its start.
    assert_eq!(
        run.about_part("p1", Observed::WaitingAtFloor {}),
        na(Why::NotFromThisState)
    );
    // No provider starts on a waiting part.
    assert_eq!(run.bind("p1", "p1-s3"), na(Why::NotFromThisState));
    // A stuck declaration keeps the order held, with no wait.
    let mut stuck = at_its_floor("ord-floor-stuck");
    let over = Observed::StuckDeclared {
        reason: StuckReason::OverTime,
    };
    assert_eq!(stuck.about_part("p1", over), Outcome::Applied {});
    assert_eq!(stuck.part("p1").waiting_since, None);
    assert_eq!(
        stuck.about_part("p1", Observed::WaitingAtFloor {}),
        na(Why::NotFromThisState),
        "a stuck part is the operator's"
    );

    // The customer's choice resolves it: a recalculation within a new authorisation leaves the wait.
    assert_eq!(
        run.about_part("p1", recalculated(route_from_ledger("r1"))),
        Outcome::Applied {}
    );
    assert_eq!(run.part_state("p1"), OrderState::FirstSegmentSettled);
    assert_eq!(run.part("p1").waiting_since, None);
    run.order().check().expect("the order stays valid");
}

// verifies: REQ-ORD-27, INV-4, REQ-STM-6
#[test]
fn a_part_waits_at_its_floor_only_while_its_value_sits_at_a_node() {
    // Nothing has moved before the withdrawal settles.
    let mut fresh = Run::opened("ord-floor-fresh");
    assert_eq!(
        fresh.about_part("p1", Observed::WaitingAtFloor {}),
        na(Why::NotFromThisState)
    );
    // A segment in flight: the value sits nowhere yet.
    let mut moving = Run::opened("ord-floor-moving");
    moving.run_segments("p1", 1, 1);
    assert_eq!(moving.bind("p1", "p1-s2"), Outcome::Applied {});
    assert_eq!(
        moving.about_part("p1", Observed::WaitingAtFloor {}),
        na(Why::NotFromThisState)
    );
    // An ended part doesn't change.
    let mut done = complete("ord-floor-done");
    assert_eq!(
        done.about_part("p1", Observed::WaitingAtFloor {}),
        na(Why::Ended)
    );
}

// verifies: REQ-ORD-27, REQ-ORD-43, INV-10
#[test]
fn a_part_waiting_at_its_floor_is_neither_handed_on_nor_sent_to_another_destination_by_the_customer()
 {
    // Only the customer's own choice after a failed final deposit hands a waiting part on or changes its
    // destination (`INV-10`): one waiting at its floor, its final deposit not yet tried, takes neither.
    let mut hand = at_its_floor("ord-floor-hand");
    let handed = Observed::HandedOn {
        linked_order: id("ord-floor-next"),
        acceptance: id("acc-floor"),
    };
    let before = hand.order().clone();
    assert_eq!(hand.about_part("p1", handed), na(Why::NotFromThisState));
    assert!(same_but_last_input(&before, hand.order()));

    let mut other = at_its_floor("ord-floor-other");
    let before = other.order().clone();
    assert_eq!(
        other.about_part(
            "p1",
            destination_changed(route_to_another_destination("d1"))
        ),
        na(Why::NotFromThisState)
    );
    assert!(same_but_last_input(&before, other.order()));
}

/// An order whose exchange is in flight when its part is declared stuck over time.
fn stuck_in_flight(order: &str) -> Run {
    let mut run = Run::opened(order);
    run.run_segments("p1", 1, 1);
    assert_eq!(run.bind("p1", "p1-s2"), Outcome::Applied {});
    let over = Observed::StuckDeclared {
        reason: StuckReason::OverTime,
    };
    assert_eq!(run.about_part("p1", over), Outcome::Applied {});
    run
}

fn resolved(to: ResolvedTo) -> Observed {
    Observed::StuckResolved {
        to,
        evidence: id("ev-resolved"),
    }
}

// verifies: REQ-AUZ-4, REQ-STM-6, AC-5, AC-6
#[test]
fn a_stuck_part_s_resolution_ends_it_and_a_late_delivery_moves_nothing() {
    let mut run = stuck_in_flight("ord-resolved");
    assert_eq!(
        run.about_part("p1", resolved(ResolvedTo::Destination)),
        Outcome::Applied {}
    );
    assert_eq!(run.part_state("p1"), OrderState::Complete);
    assert_eq!(run.order().state, OrderState::Complete);
    assert_eq!(run.part("p1").resolved_to, Some(ResolvedTo::Destination));
    run.order().check().expect("the order stays valid");
    // The exchange reports late: the part has ended, and nothing moves twice.
    assert_eq!(run.settle("p1", "p1-s2"), na(Why::Ended));
    assert_eq!(
        run.about_part("p1", resolved(ResolvedTo::Origin)),
        na(Why::Ended)
    );

    let mut back = stuck_in_flight("ord-returned");
    assert_eq!(
        back.about_part("p1", resolved(ResolvedTo::Origin)),
        Outcome::Applied {}
    );
    assert_eq!(back.part_state("p1"), OrderState::EndedWithoutCompleting);
    assert_eq!(back.part("p1").resolved_to, Some(ResolvedTo::Origin));
    back.order().check().expect("the order stays valid");
}

// verifies: REQ-AUZ-4, REQ-ORD-43
#[test]
fn only_a_stuck_part_is_resolved() {
    let mut open = Run::opened("ord-open");
    open.run_segments("p1", 1, 1);
    assert_eq!(
        open.about_part("p1", resolved(ResolvedTo::Destination)),
        na(Why::NotFromThisState)
    );
    // A part waiting for the customer is the customer's.
    let mut wait = waiting("ord-wait-resolved");
    assert_eq!(
        wait.about_part("p1", resolved(ResolvedTo::Destination)),
        na(Why::NotFromThisState)
    );
    assert_eq!(wait.part_state("p1"), OrderState::WaitingForCustomer);
}
