//! The property tests (`DEC-18`): one for each invariant the library carries, over a seeded generator
//! ([`common::generator`]) driving orders through the reference host.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashSet};

use common::generator::{Answer, Step, Trace, drive, traces};
use rutent_contracts::state_machine::{
    NotApplicableReason, Observed, OperationKind, Order, OrderState, Outcome, Part, RecordedInput,
    SegmentState, StateMachine, recalculation_starts_where_value_sits, value_at,
};
use rutent_stm::{Stm, is_ended, order_state_of};

const SEEDS: u64 = 600;

fn segment_map(order: &Order) -> BTreeMap<String, (SegmentState, String)> {
    order
        .parts
        .iter()
        .flat_map(|p| {
            p.segments.iter().map(|s| {
                (
                    s.segment_id.as_str().to_string(),
                    (s.state, p.part_id.as_str().to_string()),
                )
            })
        })
        .collect()
}

fn all_ids(order: &Order) -> BTreeSet<String> {
    order
        .parts
        .iter()
        .flat_map(|p| {
            p.segments
                .iter()
                .map(|s| s.segment_id.as_str().to_string())
                .chain(p.retired_segments.iter().map(|s| s.as_str().to_string()))
        })
        .collect()
}

fn same_but_last_input(before: &Order, after: &Order) -> bool {
    let mut a = before.clone();
    a.last_input = after.last_input.clone();
    a == *after
}

fn part_after<'a>(order: &'a Order, part: &Part) -> &'a Part {
    order
        .parts
        .iter()
        .find(|p| p.part_id == part.part_id)
        .expect("a part is never removed")
}

/// The steps that changed the order: applied by the state machine.
fn applied(step: &Step) -> bool {
    step.answer == Answer::Recorded
        && step
            .next
            .as_ref()
            .is_some_and(|n| n.last_input.outcome == (Outcome::Applied {}))
        && step.prev.is_some()
}

// verifies: REQ-STM-1, DEC-18
#[test]
fn no_sequence_of_inputs_gives_an_order_zero_or_two_states() {
    for Trace { seed, steps, .. } in traces(SEEDS) {
        let seed = *seed;
        for step in steps {
            let Some(order) = &step.next else { continue };
            let at = format!("seed {seed}, input {}", step.input.record_id.as_str());
            order.check().unwrap_or_else(|v| panic!("{at}: {v}"));
            // The order's state follows its parts; with one part, it is that part's.
            assert_eq!(order.state, order_state_of(&order.parts), "{at}");
            if order.parts.len() == 1 {
                assert_eq!(order.state, order.parts[0].state, "{at}");
            }
            // Written out, the order and each part and segment carry one `state`, from the published lists.
            let json = serde_json::to_value(order).expect("an order serialises");
            let names = |list: &[OrderState]| -> Vec<String> {
                list.iter()
                    .map(|s| {
                        serde_json::to_value(s)
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_string()
                    })
                    .collect()
            };
            let order_names = names(&OrderState::ALL);
            let state = json["state"]
                .as_str()
                .unwrap_or_else(|| panic!("{at}: no state"));
            assert!(order_names.iter().any(|n| n == state), "{at}");
            for part in json["parts"].as_array().unwrap() {
                assert!(
                    order_names
                        .iter()
                        .any(|n| Some(n.as_str()) == part["state"].as_str()),
                    "{at}"
                );
                for segment in part["segments"].as_array().unwrap() {
                    let segment_names: Vec<String> = SegmentState::ALL
                        .iter()
                        .map(|s| {
                            serde_json::to_value(s)
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .to_string()
                        })
                        .collect();
                    assert!(
                        segment_names
                            .iter()
                            .any(|n| Some(n.as_str()) == segment["state"].as_str()),
                        "{at}"
                    );
                }
            }
            let again: Order = serde_json::from_value(json).unwrap_or_else(|e| panic!("{at}: {e}"));
            assert_eq!(&again, order, "{at}");
        }
    }
}

// verifies: REQ-STM-4, INV-8, DEC-18
#[test]
fn repeated_and_reordered_inputs_change_no_state_twice() {
    // A segment only moves along the contract's edges, forward, so a repeat or a late input finds nothing to move.
    let edges: HashSet<(SegmentState, SegmentState)> = {
        use SegmentState::*;
        [
            (Pending, Started),
            (Started, Settled),
            (Started, FailedReversalOwed),
            (Started, OutcomeUnknown),
            (OutcomeUnknown, Settled),
            (OutcomeUnknown, FailedReversalOwed),
            (OutcomeUnknown, ReversalComplete),
            (FailedReversalOwed, ReversalComplete),
            (Settled, ReversalComplete),
        ]
        .into_iter()
        .collect()
    };
    for trace in traces(SEEDS) {
        let seed = trace.seed;
        // The host keeps one record per record id, and the state machine one application per record.
        let ids: BTreeSet<_> = trace
            .journal
            .iter()
            .map(|r| r.input.record_id.clone())
            .collect();
        assert_eq!(
            ids.len(),
            trace.journal.len(),
            "seed {seed}: one record per record id"
        );
        let mut seen_ids = BTreeSet::new();
        for step in &trace.steps {
            let at = format!("seed {seed}, input {}", step.input.record_id.as_str());
            if step.answer == Answer::AlreadyRecorded {
                assert_eq!(step.prev, step.next, "{at}: a repeat changes no state");
                continue;
            }
            assert!(
                seen_ids.insert(step.input.record_id.clone()),
                "{at}: recorded twice"
            );
            let (Some(prev), Some(next)) = (&step.prev, &step.next) else {
                continue;
            };
            // No segment moves backward, and no id is ever reused.
            let (before, after) = (segment_map(prev), segment_map(next));
            for (segment, (state, _)) in &before {
                if let Some((now, _)) = after.get(segment) {
                    assert!(
                        *state == *now || edges.contains(&(*state, *now)),
                        "{at}: {segment} {state:?} to {now:?}"
                    );
                }
            }
            assert!(
                all_ids(prev).is_subset(&all_ids(next)),
                "{at}: an id was lost"
            );
            let retired: BTreeSet<_> = next
                .parts
                .iter()
                .flat_map(|p| &p.retired_segments)
                .collect();
            assert!(
                next.parts
                    .iter()
                    .flat_map(|p| &p.segments)
                    .all(|s| !retired.contains(&s.segment_id)),
                "{at}: a retired id is current again"
            );
            // The state machine alone, with the same input delivered straight after: a repeat, nothing else.
            let again = Stm::transition(next, &step.input);
            assert_eq!(again.last_input.outcome, Outcome::Repeat {}, "{at}");
            assert!(same_but_last_input(next, &again), "{at}");
        }
    }
}

// verifies: REQ-STM-3, DEC-18
#[test]
fn only_an_applied_input_changes_the_state_and_only_by_what_it_names() {
    for trace in traces(SEEDS) {
        for step in &trace.steps {
            let (Some(prev), Some(next)) = (&step.prev, &step.next) else {
                continue;
            };
            if step.answer == Answer::AlreadyRecorded {
                continue;
            }
            let at = format!(
                "seed {}, input {}",
                trace.seed,
                step.input.record_id.as_str()
            );
            if applied(step) {
                assert_eq!(next.last_record, step.input.record_id, "{at}");
            } else {
                assert!(
                    same_but_last_input(prev, next),
                    "{at}: a refused input changed the state"
                );
                assert_eq!(next.last_record, prev.last_record, "{at}");
            }
            // A segment changes state only by an input that names it: time, and anything about another
            // segment, resolves nothing (`REQ-STM-5`).
            let (before, after) = (segment_map(prev), segment_map(next));
            for (segment, (state, _)) in &before {
                if let Some((now, _)) = after.get(segment)
                    && state != now
                {
                    {
                        assert_eq!(
                            step.input.subject.segment.as_ref().map(|s| s.as_str()),
                            Some(segment.as_str()),
                            "{at}: {segment} moved by an input about something else"
                        );
                    }
                }
            }
        }
    }
}

// verifies: REQ-STM-5, DEC-18
#[test]
fn a_provider_that_never_answers_leaves_its_segment_unresolved_whatever_arrives() {
    for trace in traces(SEEDS) {
        for step in &trace.steps {
            let (Some(prev), Some(next)) = (&step.prev, &step.next) else {
                continue;
            };
            if step.answer == Answer::AlreadyRecorded {
                continue;
            }
            let at = format!(
                "seed {}, input {}",
                trace.seed,
                step.input.record_id.as_str()
            );
            let resolves = matches!(
                step.input.observed,
                Observed::SegmentSettled { .. }
                    | Observed::SegmentFailed {}
                    | Observed::SegmentOutcomeUnknown {}
            );
            for (segment, (state, _)) in segment_map(prev) {
                if state != SegmentState::Started {
                    continue;
                }
                let moved =
                    segment_map(next).get(&segment).map(|(s, _)| *s) != Some(SegmentState::Started);
                if moved {
                    let named = step.input.subject.segment.as_ref().map(|s| s.as_str())
                        == Some(segment.as_str());
                    assert!(
                        resolves && named,
                        "{at}: {segment} left started without a report about it"
                    );
                }
            }
        }
    }
}

// verifies: INV-3, DEC-18
#[test]
fn every_segment_ends_settled_or_reversed_in_full_or_its_order_stuck() {
    for trace in traces(SEEDS) {
        let Some(order) = trace.last() else { continue };
        let at = format!("seed {}", trace.seed);
        for part in &order.parts {
            let states: Vec<SegmentState> = part.segments.iter().map(|s| s.state).collect();
            // At every moment: the value is moved from one place at a time. At most one segment is active, with
            // every one before it settled and every one after it pending.
            let active = states.iter().position(|s| {
                matches!(
                    s,
                    SegmentState::Started
                        | SegmentState::OutcomeUnknown
                        | SegmentState::FailedReversalOwed
                )
            });
            if let Some(i) = active
                && part.state != OrderState::Failed
            {
                {
                    assert!(
                        states[..i].iter().all(|s| *s == SegmentState::Settled),
                        "{at}: {states:?}"
                    );
                    assert!(
                        states[i + 1..].iter().all(|s| *s == SegmentState::Pending),
                        "{at}: {states:?}"
                    );
                }
            }
            // At the end, once the evidence has arrived: a part is done or held. Nothing is left started, of
            // unknown outcome or owing a reversal unless the part is stuck (or waiting for the customer).
            match part.state {
                OrderState::Complete => {
                    assert!(
                        states.iter().all(|s| *s == SegmentState::Settled),
                        "{at}: {states:?}"
                    )
                }
                OrderState::EndedWithoutCompleting => {
                    assert_ne!(
                        states[0],
                        SegmentState::Settled,
                        "{at}: cancelled after its withdrawal settled"
                    )
                }
                OrderState::Failed => assert_eq!(states[0], SegmentState::ReversalComplete, "{at}"),
                OrderState::HandedOn => assert!(
                    value_at(part).is_some(),
                    "{at}: handed on with no value at a node"
                ),
                OrderState::Stuck | OrderState::WaitingForCustomer => {}
                OrderState::NotPastFirstSegment | OrderState::FirstSegmentSettled => {
                    panic!(
                        "{at}: a part is left in progress after its evidence arrived: {states:?}"
                    )
                }
            }
        }
    }
}

// verifies: INV-4, DEC-18
#[test]
fn no_route_restarts_before_the_value_is_evidenced_at_a_node() {
    let mut restarts = 0;
    for trace in traces(SEEDS) {
        for step in &trace.steps {
            if !applied(step) {
                continue;
            }
            let (Some(prev), Some(next)) = (&step.prev, &step.next) else {
                continue;
            };
            let route = match &step.input.observed {
                Observed::RouteRecalculated { segments, .. }
                | Observed::DestinationChanged { segments, .. } => segments,
                _ => continue,
            };
            restarts += 1;
            let at = format!(
                "seed {}, input {}",
                trace.seed,
                step.input.record_id.as_str()
            );
            let part_id = step.input.subject.part.as_ref().unwrap();
            let before = prev.parts.iter().find(|p| &p.part_id == part_id).unwrap();
            assert!(
                value_at(before).is_some(),
                "{at}: restarted with the value in flight"
            );
            assert!(recalculation_starts_where_value_sits(before, route), "{at}");
            // And the new route starts exactly there: nothing moves from a second place.
            let after = part_after(next, before);
            let kept = after.segments.len() - route.len();
            assert_eq!(
                after.segments[kept].from,
                *value_at(before).unwrap(),
                "{at}"
            );
            assert!(
                after.segments[..kept]
                    .iter()
                    .all(|s| s.state == SegmentState::Settled),
                "{at}"
            );
        }
    }
    assert!(
        restarts > 50,
        "the generator restarted only {restarts} routes"
    );
}

// verifies: REQ-STM-6, DEC-18
#[test]
fn an_ended_order_changes_only_by_a_complete_part_failing_when_its_starting_segment_is_undone() {
    let mut late_reports = 0;
    for trace in traces(SEEDS) {
        for step in &trace.steps {
            let (Some(prev), Some(next)) = (&step.prev, &step.next) else {
                continue;
            };
            if step.answer == Answer::AlreadyRecorded {
                continue;
            }
            let at = format!(
                "seed {}, input {}",
                trace.seed,
                step.input.record_id.as_str()
            );
            // Every part that had ended, in an order that has ended or not, is as it was, but for a complete
            // part whose starting segment this input proves undone.
            for before in prev.parts.iter().filter(|p| is_ended(p.state)) {
                let after = part_after(next, before);
                let undone = applied(step)
                    && prev.state == OrderState::Complete
                    && before.state == OrderState::Complete
                    && matches!(step.input.observed, Observed::ReversalCompleted { .. })
                    && step.input.subject.part.as_ref() == Some(&before.part_id)
                    && step.input.subject.segment.as_ref() == Some(&before.segments[0].segment_id);
                if undone {
                    assert_eq!(after.state, OrderState::Failed, "{at}");
                } else {
                    assert_eq!(after, before, "{at}: an ended part changed");
                }
            }
            if is_ended(prev.state) {
                if applied(step) {
                    assert!(
                        matches!(step.input.observed, Observed::ReversalCompleted { .. }),
                        "{at}"
                    );
                    assert_eq!(next.state, OrderState::Failed, "{at}");
                } else {
                    late_reports += 1;
                    assert!(
                        same_but_last_input(prev, next),
                        "{at}: a report changed an ended order"
                    );
                }
            }
        }
    }
    assert!(
        late_reports > 100,
        "only {late_reports} late reports reached an ended order"
    );
}

// verifies: INV-10, DEC-18
#[test]
fn no_recalculation_changes_the_destination_or_to_asset() {
    let mut recalculations = 0;
    let mut changes = 0;
    for trace in traces(SEEDS) {
        for step in &trace.steps {
            let (Some(prev), Some(next)) = (&step.prev, &step.next) else {
                continue;
            };
            if step.answer == Answer::AlreadyRecorded {
                continue;
            }
            let at = format!(
                "seed {}, input {}",
                trace.seed,
                step.input.record_id.as_str()
            );
            let changed_by_the_customer =
                applied(step) && matches!(step.input.observed, Observed::DestinationChanged { .. });
            for before in &prev.parts {
                let after = part_after(next, before);
                if before.destination == after.destination {
                    continue;
                }
                assert!(
                    changed_by_the_customer,
                    "{at}: the destination changed without the customer's accepted change"
                );
                assert_eq!(
                    step.input.subject.part.as_ref(),
                    Some(&before.part_id),
                    "{at}"
                );
                changes += 1;
            }
            if applied(step) && matches!(step.input.observed, Observed::RouteRecalculated { .. }) {
                recalculations += 1;
                let part_id = step.input.subject.part.as_ref().unwrap();
                let before = prev.parts.iter().find(|p| &p.part_id == part_id).unwrap();
                let after = part_after(next, before);
                assert_eq!(after.destination, before.destination, "{at}");
                let last = after.segments.last().unwrap();
                assert_eq!(last.kind, OperationKind::Deposit, "{at}");
                assert_eq!(last.provider, before.destination.provider, "{at}");
                assert_eq!(last.to, before.destination.to, "{at}");
                assert_eq!(last.to.asset, before.destination.to.asset, "{at}: to asset");
            }
        }
    }
    assert!(
        recalculations > 30 && changes > 10,
        "{recalculations} recalculations, {changes} changes"
    );
}

// verifies: NFR-4, REQ-STM-3, DEC-18
#[test]
fn replaying_a_recorded_run_reaches_the_same_states() {
    for trace in traces(SEEDS) {
        let at = format!("seed {}", trace.seed);
        let fold = |inputs: &[&RecordedInput]| -> Vec<Option<Order>> {
            let mut state: Option<Order> = None;
            inputs
                .iter()
                .map(|input| {
                    state = match &state {
                        Some(order) => Some(Stm::transition(order, input)),
                        None => Stm::open(input),
                    };
                    state.clone()
                })
                .collect()
        };
        let inputs: Vec<&RecordedInput> = trace.journal.iter().map(|r| &r.input).collect();
        let first = fold(&inputs);
        let second = fold(&inputs);
        assert_eq!(first, second, "{at}: two folds differ");
        for (record, state) in trace.journal.iter().zip(&first) {
            assert_eq!(&record.state_after, state, "{at}: record {}", record.seq);
        }
        assert_eq!(
            trace.journal.last().and_then(|r| r.state_after.as_ref()),
            trace.last(),
            "{at}"
        );
    }
}

// verifies: DEC-18
#[test]
fn the_generator_reaches_every_state_and_every_outcome() {
    let mut order_states = HashSet::new();
    let mut part_states = HashSet::new();
    let mut segment_states = HashSet::new();
    let mut reasons = HashSet::new();
    let (mut applied_n, mut repeats, mut lates) = (0, 0, 0);
    let (
        mut recalcs,
        mut changes,
        mut hand_ons,
        mut failed_after_complete,
        mut multi,
        mut handed_on_orders,
    ) = (0, 0, 0, 0, 0, 0);
    for trace in traces(SEEDS) {
        for step in &trace.steps {
            if let Some(order) = &step.next {
                order_states.insert(order.state);
                part_states.extend(order.parts.iter().map(|p| p.state));
                segment_states.extend(
                    order
                        .parts
                        .iter()
                        .flat_map(|p| p.segments.iter().map(|s| s.state)),
                );
                multi += usize::from(order.parts.len() > 1);
                handed_on_orders += usize::from(order.handed_on_from.is_some());
                if step.answer == Answer::Recorded {
                    match order.last_input.outcome {
                        Outcome::Applied {} => applied_n += 1,
                        Outcome::Repeat {} => repeats += 1,
                        Outcome::Late {} => lates += 1,
                        Outcome::NotApplicable { reason } => {
                            reasons.insert(reason);
                        }
                    }
                }
            }
            if applied(step) {
                match &step.input.observed {
                    Observed::RouteRecalculated { .. } => recalcs += 1,
                    Observed::DestinationChanged { .. } => changes += 1,
                    Observed::HandedOn { .. } => hand_ons += 1,
                    Observed::ReversalCompleted { .. }
                        if step
                            .prev
                            .as_ref()
                            .is_some_and(|p| p.state == OrderState::Complete) =>
                    {
                        failed_after_complete += 1
                    }
                    _ => {}
                }
            }
        }
    }
    for state in OrderState::ALL {
        assert!(order_states.contains(&state), "order never {state:?}");
        assert!(part_states.contains(&state), "part never {state:?}");
    }
    for state in SegmentState::ALL {
        assert!(segment_states.contains(&state), "segment never {state:?}");
    }
    for reason in [
        // `OtherOrder` can't come through a host, which finds an order by its subject: the scenarios hold it.
        NotApplicableReason::UnknownSubject,
        NotApplicableReason::Ended,
        NotApplicableReason::NotFromThisState,
        NotApplicableReason::ChangesDestination,
        NotApplicableReason::KeepsDestination,
        NotApplicableReason::ReusesSegmentId,
        NotApplicableReason::ValueNotAtStart,
    ] {
        assert!(reasons.contains(&reason), "never {reason:?}");
    }
    let floor = |n: usize, what: &str| assert!(n >= 10, "only {n} {what}");
    floor(applied_n, "applied inputs");
    floor(lates, "late reports");
    floor(recalcs, "recalculations");
    floor(changes, "destination changes");
    floor(hand_ons, "hand-ons");
    floor(
        failed_after_complete,
        "complete orders failed by an undone start",
    );
    floor(multi, "orders of several parts");
    floor(handed_on_orders, "orders handed on from another");
    let _ = repeats; // a repeat of the last input only reaches the state machine from the host's own replays
    // A run is a pure function of its seed.
    let (a, b) = (drive(7), drive(7));
    assert_eq!(a.journal, b.journal);
}

// verifies: REQ-STM-1
#[test]
fn a_run_reads_the_same_through_the_host_and_through_the_state_machine_alone() {
    let trace = drive(42);
    let host_final = trace.journal.last().and_then(|r| r.state_after.clone());
    assert_eq!(host_final.as_ref(), trace.last());
    assert!(host_final.is_some_and(|o| o.check().is_ok()));
}
