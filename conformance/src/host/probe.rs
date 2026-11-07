//! The probe: a small state machine the host-contract suite runs every candidate with, and the scenario it
//! drives. It is not `CMP-STM` (that comes in M2); it is just enough of the state-machine contract to give each
//! input an outcome a host can't fake. Every order it returns passes [`Order::check`].

use rutent_contracts::state_machine::Outcome as InputOutcome;
use rutent_contracts::state_machine::{
    Endpoint, Id, LastInput, NotApplicableReason, Observed, Operation, Order, OrderState, Part,
    PartPlan, RecordId, RecordedInput, Scope, Segment, SegmentPlan, SegmentState, Source,
    SourceKind, Standing, StateMachine, Subject, Timestamp,
};

/// The probe state machine. A route opens an order; binding moves a segment from pending to started; settling moves
/// it from started to settled, and the part and order follow; anything else changes nothing and says why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe;

impl StateMachine for Probe {
    fn open(input: &RecordedInput) -> Option<Order> {
        let Observed::RouteChosen {
            parts,
            handed_on_from,
            ..
        } = &input.observed
        else {
            return None;
        };
        let parts = parts
            .iter()
            .map(|p| Part {
                part_id: p.part_id.clone(),
                state: OrderState::NotPastFirstSegment,
                linked_order: None,
                segments: p
                    .segments
                    .iter()
                    .map(|s| Segment::new(s.clone(), SegmentState::Pending))
                    .collect(),
                retired_segments: Vec::new(),
            })
            .collect();
        Some(Order {
            order_id: input.subject.order.clone(),
            state: OrderState::NotPastFirstSegment,
            last_record: input.record_id.clone(),
            last_input: LastInput {
                record_id: input.record_id.clone(),
                outcome: InputOutcome::Applied {},
            },
            handed_on_from: handed_on_from.clone(),
            parts,
        })
    }

    fn transition(state: &Order, input: &RecordedInput) -> Order {
        let mut next = state.clone();
        let outcome = apply(&mut next, input);
        let mut result = if outcome == (InputOutcome::Applied {}) {
            next.last_record = input.record_id.clone();
            next
        } else {
            state.clone()
        };
        result.last_input = LastInput {
            record_id: input.record_id.clone(),
            outcome,
        };
        result
    }
}

fn not_applicable(reason: NotApplicableReason) -> InputOutcome {
    InputOutcome::NotApplicable { reason }
}

fn apply(order: &mut Order, input: &RecordedInput) -> InputOutcome {
    if input.subject.order != order.order_id {
        return not_applicable(NotApplicableReason::OtherOrder);
    }
    if input.record_id == order.last_record || input.record_id == order.last_input.record_id {
        return InputOutcome::Repeat {};
    }
    if order.state == OrderState::Complete {
        return not_applicable(NotApplicableReason::Ended);
    }
    let (Some(part_id), Some(segment_id)) = (&input.subject.part, &input.subject.segment) else {
        return not_applicable(NotApplicableReason::NotFromThisState);
    };
    let Some(part) = order.parts.iter_mut().find(|p| &p.part_id == part_id) else {
        return not_applicable(NotApplicableReason::UnknownSubject);
    };
    let Some(index) = part
        .segments
        .iter()
        .position(|s| &s.segment_id == segment_id)
    else {
        return if part.retired_segments.contains(segment_id) {
            InputOutcome::Late {}
        } else {
            not_applicable(NotApplicableReason::UnknownSubject)
        };
    };
    let segment = &mut part.segments[index];
    match (&input.observed, segment.state) {
        (Observed::ProviderBound {}, SegmentState::Pending) => {
            segment.state = SegmentState::Started
        }
        (Observed::SegmentSettled { .. }, SegmentState::Started) => {
            segment.state = SegmentState::Settled;
            part.state = if part
                .segments
                .iter()
                .all(|s| s.state == SegmentState::Settled)
            {
                OrderState::Complete
            } else if part.segments[0].state == SegmentState::Settled {
                OrderState::FirstSegmentSettled
            } else {
                part.state
            };
            order.state = if order.parts.iter().all(|p| p.state == OrderState::Complete) {
                OrderState::Complete
            } else if order
                .parts
                .iter()
                .any(|p| p.state != OrderState::NotPastFirstSegment)
            {
                OrderState::FirstSegmentSettled
            } else {
                OrderState::NotPastFirstSegment
            };
        }
        _ => return not_applicable(NotApplicableReason::NotFromThisState),
    }
    InputOutcome::Applied {}
}

fn id(text: &str) -> Id {
    Id::new(text).expect("a probe id is never empty")
}

fn endpoint(node: &str, asset: &str) -> Endpoint {
    Endpoint {
        node: id(node),
        asset: id(asset),
    }
}

/// The three segments of every probe route: a withdrawal, an exchange and a deposit.
pub fn route() -> Vec<SegmentPlan> {
    let segment = |n: u8, provider: &str, operation, from, to| SegmentPlan {
        segment_id: id(&format!("p1-s{n}")),
        provider: id(provider),
        operation,
        from,
        to,
    };
    vec![
        segment(
            1,
            "reserve-bank",
            Operation::Withdrawal,
            endpoint("bank", "COP"),
            endpoint("ledger", "COP"),
        ),
        segment(
            2,
            "exchange-a",
            Operation::Exchange,
            endpoint("ledger", "COP"),
            endpoint("ledger", "USDC"),
        ),
        segment(
            3,
            "reserve-wallet",
            Operation::Deposit,
            endpoint("ledger", "USDC"),
            endpoint("chain", "USDC"),
        ),
    ]
}

/// A recorded input of the probe scenario. `tick` gives it a distinct `recorded_at`, one second apart.
pub fn input(
    order: &str,
    record: &str,
    tick: u32,
    segment: Option<&str>,
    observed: Observed,
) -> RecordedInput {
    let (source, standing) = match observed {
        Observed::RouteChosen { .. } => (SourceKind::Customer, Standing::Stated),
        Observed::SegmentSettled { .. } => (SourceKind::EvidenceChecker, Standing::Verified),
        _ => (SourceKind::Orchestrator, Standing::Stated),
    };
    let at = format!(
        "2026-11-06T{:02}:{:02}:{:02}Z",
        10 + tick / 3600,
        tick / 60 % 60,
        tick % 60
    );
    RecordedInput {
        record_id: RecordId::new(record).expect("a probe record id is never empty"),
        recorded_at: Timestamp::new(at).expect("a probe time is RFC 3339"),
        time_source: id("probe clock"),
        source: Source {
            kind: source,
            id: id("probe"),
        },
        standing,
        subject: Subject {
            order: id(order),
            part: segment.map(|_| id("p1")),
            segment: segment.map(id),
        },
        observed,
    }
}

/// The seven inputs that take one order from its route to complete, in their recorded order: the route, then each
/// segment bound and settled. `first_tick` is the first input's time.
pub fn order_inputs(order: &str, first_tick: u32) -> Vec<RecordedInput> {
    let record = |n: u32| format!("rec-{order}-{n}");
    let mut inputs = vec![input(
        order,
        &record(1),
        first_tick,
        None,
        Observed::RouteChosen {
            decision_record: id(&format!("dr-{order}")),
            parts: vec![PartPlan {
                part_id: id("p1"),
                segments: route(),
            }],
            handed_on_from: None,
        },
    )];
    for n in 1..=3u32 {
        let segment = format!("p1-s{n}");
        let tick = first_tick + 2 * n - 1;
        inputs.push(input(
            order,
            &record(2 * n),
            tick,
            Some(&segment),
            Observed::ProviderBound {},
        ));
        inputs.push(input(
            order,
            &record(2 * n + 1),
            tick + 1,
            Some(&segment),
            Observed::SegmentSettled {
                evidence: id(&format!("ev-{order}-{n}")),
            },
        ));
    }
    inputs
}

/// The order of [`orphan`]'s input, which no route opens.
pub const ORPHAN_ORDER: &str = "ord-orphan";

/// An input about an order no route ever opened: recorded, with no state.
pub fn orphan(tick: u32) -> RecordedInput {
    input(
        ORPHAN_ORDER,
        "rec-orphan-1",
        tick,
        Some("p1-s1"),
        Observed::SegmentSettled {
            evidence: id("ev-orphan"),
        },
    )
}

/// Another input under a record id `taken` already names: the same subject, a different observation of the same
/// scope, so the input is still valid.
pub fn reusing(taken: &RecordedInput) -> RecordedInput {
    let mut other = taken.clone();
    other.observed = match taken.observed.scope() {
        Scope::Segment => Observed::SegmentFailed {},
        Scope::Part => Observed::WithdrawalCancelled {},
        Scope::Order => Observed::EndRequested {},
    };
    other
}

/// The same input under the same record id, stamped again at another time: equal in everything but `recorded_at`.
pub fn restamped(taken: &RecordedInput) -> RecordedInput {
    let mut other = taken.clone();
    other.recorded_at = Timestamp::new("2026-11-07T00:00:00Z").expect("a probe time is RFC 3339");
    other
}

/// The same input under the same record id, about another order: equal in everything but its subject's order.
pub fn for_another_order(taken: &RecordedInput) -> RecordedInput {
    let mut other = taken.clone();
    other.subject.order = id("ord-other");
    other
}

/// A fixed-seed shuffle (a linear congruential generator, std only), so a reordered delivery is the same on every
/// run.
pub fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    for i in (1..items.len()).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let j = (state >> 33) as usize % (i + 1);
        items.swap(i, j);
    }
}

/// The states a journal's inputs reach when folded through the probe alone: the expected state of each record.
pub fn fold<'a>(inputs: impl IntoIterator<Item = &'a RecordedInput>) -> Vec<Option<Order>> {
    let mut state: Option<Order> = None;
    inputs
        .into_iter()
        .map(|input| {
            state = match &state {
                Some(order) => Some(Probe::transition(order, input)),
                None => Probe::open(input),
            };
            state.clone()
        })
        .collect()
}
