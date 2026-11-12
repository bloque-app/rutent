//! A part's standing as this log's inputs show it: the one fold the citation table's part rows share, so each row
//! reads where a part stands the same way (`T-020-01`, `T-020-10`, `T-020-11`).
//!
//! The fold follows the state machine's published rules, not a copy of them: [`after_no_route`],
//! [`after_waiting_at_floor`], [`after_stuck_resolved`] and [`deposit_failed_at_after`] place the part, over a
//! [`Part`] the fold builds from the part's inputs. The rest is what every `StateMachine` does with a part's
//! inputs and the contract states: a segment moves on its own input, a recalculation or change of destination keeps
//! the settled prefix and retires the rest, an ended part takes nothing more. The log admits an input only once Core
//! recorded it, so the fold takes each as applied; a stuck resolution's move counts once its result is `done`.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::relations::History;
use super::types::{Action, Entry, Outcome, VerifiedEntry, What};
use crate::state_machine::{
    Destination, Observed, OperationKind, OrderState, Part, RecordedInput, Segment, SegmentState,
    after_no_route, after_stuck_resolved, after_waiting_at_floor, deposit_failed_at_after,
};

/// Where a part stands after a history.
pub(crate) struct Standing {
    /// The part, as the state machine would hold it.
    pub part: Part,
    /// The index in the history of the entry that put the part where it stands: the input that moved it to its
    /// state, or the `done` result of its stuck resolution.
    pub since: usize,
    /// Whether a stuck resolution's move for the part has a `done` result: the part is resolved.
    pub resolved: bool,
    /// Every segment the part ever had, retired ones included, with its kind and its last state.
    pub segments: BTreeMap<String, (OperationKind, SegmentState)>,
}

impl Standing {
    /// Whether the part has ended: complete, failed, handed on or ended without completing (`REQ-STM-6`), or
    /// resolved while stuck, its value moved on its resolution.
    pub fn ended(&self) -> bool {
        self.resolved
            || matches!(
                self.part.state,
                OrderState::Complete
                    | OrderState::Failed
                    | OrderState::HandedOn
                    | OrderState::EndedWithoutCompleting
            )
    }

    /// Whether the part is held: waiting for the customer or stuck, where no provider starts on it.
    pub fn held(&self) -> bool {
        matches!(
            self.part.state,
            OrderState::WaitingForCustomer | OrderState::Stuck
        )
    }

    /// Whether the part's value has left `segment`: it settled, was reversed, or is a deposit that failed, which
    /// leaves the value where it was (`INV-2`, `REQ-ORD-43`).
    pub fn released(&self, segment: &str) -> bool {
        matches!(
            self.segments.get(segment),
            Some((_, SegmentState::Settled | SegmentState::ReversalComplete))
                | Some((OperationKind::Deposit, SegmentState::FailedReversalOwed))
        )
    }
}

fn verified(entry: &Entry) -> Option<&VerifiedEntry> {
    match entry {
        Entry::Verified(v) => Some(v),
        Entry::Stated(_) => None,
    }
}

fn input(entry: &VerifiedEntry) -> Option<&RecordedInput> {
    match &entry.what {
        What::Input { input } => Some(input),
        _ => None,
    }
}

/// The part of `order` a stuck decision of `history` names, by the request and decision `carries` names.
pub(crate) fn decided_part<'a>(
    order: &str,
    carries: &super::types::Decided,
    history: &'a History,
) -> Option<&'a str> {
    history.iter().find_map(|(_, e)| match &verified(e)?.what {
        What::StuckDecide { request, decision }
            if request.order.as_str() == order
                && request.request == carries.request
                && *decision == carries.decision =>
        {
            Some(request.part.as_str())
        }
        _ => None,
    })
}

/// Whether `entry` is the `done` result of a stuck resolution's move for `part` of `order`.
fn resolves(entry: &VerifiedEntry, order: &str, part: &str, history: &History) -> bool {
    let (What::Result { effect }, Outcome::Done {}) = (&entry.what, &entry.outcome) else {
        return false;
    };
    history.iter().any(|(r, e)| {
        r == effect
            && verified(e).is_some_and(|v| match &v.what {
                What::Effect {
                    action:
                        Action::ResolveStuck {
                            order: o,
                            carries_out,
                            ..
                        },
                    ..
                } => o.as_str() == order && decided_part(order, carries_out, history) == Some(part),
                _ => false,
            })
    })
}

/// Where `part` of `order` stands after `history`, or `None` if no `route_chosen` of the order planned it.
pub(crate) fn standing(order: &str, part: &str, history: &History) -> Option<Standing> {
    let mut standing: Option<Standing> = None;
    for (index, (_, entry)) in history.iter().enumerate() {
        let Some(entry) = verified(entry) else {
            continue;
        };
        let Some(input) = input(entry) else {
            if let Some(s) = &mut standing
                && !s.resolved
                && resolves(entry, order, part, history)
            {
                s.resolved = true;
                s.since = index;
            }
            continue;
        };
        if input.subject.order.as_str() != order {
            continue;
        }
        match (&input.observed, &mut standing) {
            (Observed::RouteChosen { parts, .. }, None) => {
                standing = parts
                    .iter()
                    .find(|p| p.part_id.as_str() == part)
                    .and_then(|p| opened(p, index));
            }
            (Observed::EndRequested {}, Some(s)) => {
                if s.part.state == OrderState::NotPastFirstSegment {
                    s.part.state = OrderState::EndedWithoutCompleting;
                    s.since = index;
                }
            }
            (_, Some(s)) if input.subject.part.as_ref().map(|p| p.as_str()) == Some(part) => {
                s.apply(input, index)
            }
            _ => {}
        }
    }
    standing
}

fn opened(plan: &crate::state_machine::PartPlan, index: usize) -> Option<Standing> {
    let destination = Destination::of(&plan.segments)?;
    let segments = plan
        .segments
        .iter()
        .map(|s| {
            (
                s.segment_id.as_str().to_string(),
                (s.kind, SegmentState::Pending),
            )
        })
        .collect();
    Some(Standing {
        part: Part {
            part_id: plan.part_id.clone(),
            state: OrderState::NotPastFirstSegment,
            linked_order: None,
            segments: plan
                .segments
                .iter()
                .cloned()
                .map(|p| Segment::new(p, SegmentState::Pending))
                .collect(),
            retired_segments: Vec::new(),
            destination,
            deposit_failed_at: None,
            waiting_since: None,
            resolved_to: None,
        },
        since: index,
        resolved: false,
        segments,
    })
}

impl Standing {
    fn apply(&mut self, input: &RecordedInput, index: usize) {
        if self.ended() {
            return;
        }
        let before = self.part.state;
        let part = &mut self.part;
        match &input.observed {
            Observed::ProviderBound {}
            | Observed::SegmentSettled { .. }
            | Observed::SegmentFailed {}
            | Observed::SegmentOutcomeUnknown {}
            | Observed::ReversalCompleted { .. } => {
                let Some(id) = &input.subject.segment else {
                    return;
                };
                let Some(at) = part.segments.iter().position(|s| &s.segment_id == id) else {
                    // A report about a retired segment is late: it moves nothing.
                    return;
                };
                let failed_at = deposit_failed_at_after(part, input);
                let next = match &input.observed {
                    Observed::ProviderBound {} => SegmentState::Started,
                    Observed::SegmentSettled { .. } => SegmentState::Settled,
                    Observed::SegmentFailed {} => {
                        part.deposit_failed_at = failed_at;
                        SegmentState::FailedReversalOwed
                    }
                    Observed::SegmentOutcomeUnknown {} => SegmentState::OutcomeUnknown,
                    _ => SegmentState::ReversalComplete,
                };
                part.segments[at].state = next;
                if let Some(entry) = self.segments.get_mut(id.as_str()) {
                    entry.1 = next;
                }
                part.state = if part
                    .segments
                    .iter()
                    .all(|s| s.state == SegmentState::Settled)
                {
                    OrderState::Complete
                } else if matches!(
                    part.state,
                    OrderState::WaitingForCustomer | OrderState::Stuck
                ) {
                    part.state
                } else {
                    by_first_segment(part)
                };
            }
            Observed::RouteRecalculated { segments, .. }
            | Observed::DestinationChanged { segments, .. } => {
                let failed_at = deposit_failed_at_after(part, input);
                let keep = part
                    .segments
                    .iter()
                    .position(|s| s.state != SegmentState::Settled)
                    .unwrap_or(part.segments.len());
                let retired: Vec<Segment> = part.segments.drain(keep..).collect();
                part.retired_segments
                    .extend(retired.into_iter().map(|s| s.segment_id));
                for plan in segments {
                    self.segments.insert(
                        plan.segment_id.as_str().to_string(),
                        (plan.kind, SegmentState::Pending),
                    );
                    part.segments
                        .push(Segment::new(plan.clone(), SegmentState::Pending));
                }
                if matches!(input.observed, Observed::DestinationChanged { .. })
                    && let Some(destination) = Destination::of(segments)
                {
                    part.destination = destination;
                }
                part.deposit_failed_at = failed_at;
                part.state = by_first_segment(part);
            }
            Observed::NoRoute { .. } => {
                if let Some(state) = after_no_route(part) {
                    part.state = state;
                }
            }
            Observed::WaitingAtFloor {} => {
                if let Some(state) = after_waiting_at_floor(part) {
                    part.state = state;
                }
            }
            Observed::StuckDeclared { .. } => part.state = OrderState::Stuck,
            Observed::StuckResolved { to, .. } => {
                if let Some(state) = after_stuck_resolved(part, *to) {
                    part.state = state;
                    part.resolved_to = Some(*to);
                }
            }
            Observed::WithdrawalCancelled {} => {
                if part.state == OrderState::NotPastFirstSegment {
                    part.state = OrderState::EndedWithoutCompleting;
                }
            }
            Observed::HandedOn { linked_order, .. } => {
                part.state = OrderState::HandedOn;
                part.linked_order = Some(linked_order.clone());
            }
            Observed::RouteChosen { .. } | Observed::EndRequested {} => {}
        }
        if self.part.state != before {
            self.since = index;
        }
    }
}

fn by_first_segment(part: &Part) -> OrderState {
    if part.segments[0].state == SegmentState::Settled {
        OrderState::FirstSegmentSettled
    } else {
        OrderState::NotPastFirstSegment
    }
}
