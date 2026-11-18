//! An order's state machine, fed as a host feeds it: the inputs a scenario's run of simulated providers gives it
//! (`REQ-STM-5`).
//!
//! `T-024-02` assembles Core, which gives the state machine its inputs from the platform's answers. Until then a
//! scenario that needs to show what an answer, or no answer, does to a segment gives them itself, here.

use rutent_contracts::provider_role::{Denomination, OuterNode};
use rutent_contracts::state_machine::{
    Endpoint, Id, NodeRef, Observed, OperationKind, Order, OrderState, Outcome, PartPlan, RecordId,
    RecordedInput, SegmentPlan, SegmentState, Source, SourceKind, Standing, StateMachine, Subject,
    Timestamp,
};
use rutent_stm::Stm;

/// An id that is never empty.
pub fn id(text: &str) -> Id {
    Id::new(text).expect("an id is never empty here")
}

/// One order and the recorded inputs delivered to it, each its own record, in the order sent.
pub struct Run {
    order: String,
    tick: u32,
    state: Option<Order>,
    /// Every input sent, in order.
    pub sent: Vec<RecordedInput>,
}

impl Run {
    /// An order of one part, `p1`, of two segments: `provider`'s withdrawal from the outer node `bank` into the inner
    /// node `ledger`, in COP, as `p1-s1`, and its deposit back out to `bank` as `p1-s2`. Its route is chosen.
    pub fn withdrawing(order: &str, provider: &str, bank: &str, ledger: &str) -> Self {
        let endpoint = |node: NodeRef| Endpoint {
            node,
            asset: Denomination {
                id: "COP".into(),
                decimals: 2,
            },
        };
        let outer = || {
            endpoint(NodeRef::Outer(OuterNode {
                network: bank.into(),
            }))
        };
        let segments = vec![
            SegmentPlan {
                segment_id: id("p1-s1"),
                provider: id(provider),
                kind: OperationKind::Withdrawal,
                from: outer(),
                to: endpoint(NodeRef::Inner(ledger.into())),
            },
            SegmentPlan {
                segment_id: id("p1-s2"),
                provider: id(provider),
                kind: OperationKind::Deposit,
                from: endpoint(NodeRef::Inner(ledger.into())),
                to: outer(),
            },
        ];
        let mut run = Run {
            order: order.into(),
            tick: 0,
            state: None,
            sent: Vec::new(),
        };
        let chosen = Observed::RouteChosen {
            decision_record: id(&format!("dr-{order}")),
            parts: vec![PartPlan {
                part_id: id("p1"),
                segments,
            }],
            handed_on_from: None,
        };
        let input = run.input(None, None, chosen);
        assert_eq!(run.feed(&input), Outcome::Applied {});
        run
    }

    /// A recorded input, as the host forms it.
    pub fn input(
        &mut self,
        part: Option<&str>,
        segment: Option<&str>,
        observed: Observed,
    ) -> RecordedInput {
        self.tick += 1;
        let (kind, standing) = match observed {
            Observed::RouteChosen { .. } => (SourceKind::Customer, Standing::Stated),
            Observed::SegmentSettled { .. } | Observed::ReversalCompleted { .. } => {
                (SourceKind::EvidenceChecker, Standing::Verified)
            }
            _ => (SourceKind::Orchestrator, Standing::Stated),
        };
        RecordedInput {
            record_id: RecordId::new(format!("rec-{}-{}", self.order, self.tick))
                .expect("a record id is never empty here"),
            recorded_at: at(self.tick),
            time_source: id("scenario clock"),
            source: Source {
                kind,
                id: id("scenario"),
            },
            standing,
            subject: Subject {
                order: id(&self.order),
                part: part.map(id),
                segment: segment.map(id),
            },
            observed,
        }
    }

    /// Feeds an input as a host does: opens the order if it has no state, else moves it.
    pub fn feed(&mut self, input: &RecordedInput) -> Outcome {
        self.sent.push(input.clone());
        match &self.state {
            None => {
                self.state = Stm::open(input);
                if self.state.is_some() {
                    Outcome::Applied {}
                } else {
                    Outcome::NotApplicable {
                        reason:
                            rutent_contracts::state_machine::NotApplicableReason::UnknownSubject,
                    }
                }
            }
            Some(order) => {
                let next = Stm::transition(order, input);
                let outcome = next.last_input.outcome;
                self.state = Some(next);
                outcome
            }
        }
    }

    /// Feeds an input about the segment `p1-s1`.
    pub fn about_the_segment(&mut self, observed: Observed) -> Outcome {
        let input = self.input(Some("p1"), Some("p1-s1"), observed);
        self.feed(&input)
    }

    /// Feeds an input about the part `p1`.
    pub fn about_the_part(&mut self, observed: Observed) -> Outcome {
        let input = self.input(Some("p1"), None, observed);
        self.feed(&input)
    }

    /// The state of the segment `p1-s1`.
    pub fn segment_state(&self) -> SegmentState {
        self.state.as_ref().expect("the order is open").parts[0].segments[0].state
    }

    /// The state of the part `p1`.
    pub fn part_state(&self) -> OrderState {
        self.state.as_ref().expect("the order is open").parts[0].state
    }
}

/// A time on 2026-11-09, `tick` seconds past midnight.
pub fn at(tick: u32) -> Timestamp {
    Timestamp::new(format!(
        "2026-11-09T{:02}:{:02}:{:02}Z",
        tick / 3600 % 24,
        tick / 60 % 60,
        tick % 60
    ))
    .expect("a time is RFC 3339")
}
