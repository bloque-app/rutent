//! Builders the state-machine tests share: routes, recorded inputs, and a run that feeds one order its inputs.
#![allow(dead_code)]

pub mod generator;

use rutent_contracts::provider_role::{Denomination, OuterNode};
use rutent_contracts::state_machine::{
    Endpoint, Id, NodeRef, Observed, OperationKind, Order, OrderState, Outcome, Part, PartPlan,
    RecordId, RecordedInput, SegmentPlan, SegmentState, Source, SourceKind, Standing, StateMachine,
    Subject, Timestamp,
};
use rutent_stm::Stm;

pub fn id(text: &str) -> Id {
    Id::new(text).expect("an id is never empty here")
}

pub fn endpoint(node: NodeRef, asset: &str) -> Endpoint {
    let decimals = if asset == "COP" { 2 } else { 6 };
    Endpoint {
        node,
        asset: Denomination {
            id: asset.into(),
            decimals,
        },
    }
}

pub fn inner(node: &str, asset: &str) -> Endpoint {
    endpoint(NodeRef::Inner(node.into()), asset)
}

pub fn outer(network: &str, asset: &str) -> Endpoint {
    endpoint(
        NodeRef::Outer(OuterNode {
            network: network.into(),
        }),
        asset,
    )
}

pub fn plan(
    segment: &str,
    provider: &str,
    kind: OperationKind,
    from: Endpoint,
    to: Endpoint,
) -> SegmentPlan {
    SegmentPlan {
        segment_id: id(segment),
        provider: id(provider),
        kind,
        from,
        to,
    }
}

/// The route of every test: a withdrawal from the outer node `bank`, an exchange at the inner node `ledger`, and a
/// deposit out to the outer node `chain` at the reserve `reserve-wallet`. Segment ids are `<prefix>-s1` to `-s3`.
pub fn route(prefix: &str) -> Vec<SegmentPlan> {
    route_via(prefix, "exchange-a")
}

/// The same route through another exchange: an alternative from the same state (`REQ-STM-7`).
pub fn route_via(prefix: &str, exchange: &str) -> Vec<SegmentPlan> {
    let s = |n: u32| format!("{prefix}-s{n}");
    vec![
        plan(
            &s(1),
            "reserve-bank",
            OperationKind::Withdrawal,
            outer("bank", "COP"),
            inner("ledger", "COP"),
        ),
        plan(
            &s(2),
            exchange,
            OperationKind::Exchange,
            inner("ledger", "COP"),
            inner("ledger", "USDC"),
        ),
        plan(
            &s(3),
            "reserve-wallet",
            OperationKind::Deposit,
            inner("ledger", "USDC"),
            outer("chain", "USDC"),
        ),
    ]
}

/// A route from the inner node `ledger` in USDC to the same destination, through a second inner node: a route
/// that moves the value between nodes and changes no asset (`REQ-STM-7`).
pub fn route_from_ledger(prefix: &str) -> Vec<SegmentPlan> {
    vec![
        plan(
            &format!("{prefix}-s1"),
            "router-x",
            OperationKind::Route,
            inner("ledger", "USDC"),
            inner("vault", "USDC"),
        ),
        plan(
            &format!("{prefix}-s2"),
            "reserve-wallet",
            OperationKind::Deposit,
            inner("vault", "USDC"),
            outer("chain", "USDC"),
        ),
    ]
}

/// A route from `ledger` in USDC to another destination: another reserve, another network.
pub fn route_to_another_destination(prefix: &str) -> Vec<SegmentPlan> {
    vec![plan(
        &format!("{prefix}-s1"),
        "reserve-other",
        OperationKind::Deposit,
        inner("ledger", "USDC"),
        outer("other-chain", "USDC"),
    )]
}

/// A route handed on: it starts at the inner node `at` in USDC, with no withdrawal.
pub fn handed_on_route(prefix: &str, at: &str) -> Vec<SegmentPlan> {
    vec![plan(
        &format!("{prefix}-s1"),
        "reserve-wallet",
        OperationKind::Deposit,
        inner(at, "USDC"),
        outer("chain", "USDC"),
    )]
}

pub fn part(part_id: &str, segments: Vec<SegmentPlan>) -> PartPlan {
    PartPlan {
        part_id: id(part_id),
        segments,
    }
}

pub fn at(tick: u32) -> Timestamp {
    Timestamp::new(format!(
        "2026-11-09T{:02}:{:02}:{:02}Z",
        tick / 3600 % 24,
        tick / 60 % 60,
        tick % 60
    ))
    .expect("a test time is RFC 3339")
}

/// One order and the recorded inputs delivered to it, as a host would: each its own record, in the order sent.
pub struct Run {
    pub order_id: String,
    pub tick: u32,
    pub state: Option<Order>,
    pub sent: Vec<RecordedInput>,
}

impl Run {
    pub fn new(order: &str) -> Run {
        Run {
            order_id: order.into(),
            tick: 0,
            state: None,
            sent: Vec::new(),
        }
    }

    /// A run of an order of one part, `p1`, over [`route`] `p1`.
    pub fn opened(order: &str) -> Run {
        let mut run = Run::new(order);
        run.choose(vec![part("p1", route("p1"))], None);
        run
    }

    pub fn input(
        &mut self,
        part_id: Option<&str>,
        segment: Option<&str>,
        observed: Observed,
    ) -> RecordedInput {
        self.tick += 1;
        let (source, standing) = match observed {
            Observed::RouteChosen { .. } => (SourceKind::Customer, Standing::Stated),
            Observed::SegmentSettled { .. } | Observed::ReversalCompleted { .. } => {
                (SourceKind::EvidenceChecker, Standing::Verified)
            }
            _ => (SourceKind::Orchestrator, Standing::Stated),
        };
        RecordedInput {
            record_id: RecordId::new(format!("rec-{}-{}", self.order_id, self.tick))
                .expect("a record id is never empty here"),
            recorded_at: at(self.tick),
            time_source: id("test clock"),
            source: Source {
                kind: source,
                id: id("test"),
            },
            standing,
            subject: Subject {
                order: id(&self.order_id),
                part: part_id.map(id),
                segment: segment.map(id),
            },
            observed,
        }
    }

    /// Feeds an input as a host does: opens the order if it has no state, else moves it. The outcome of the input.
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

    pub fn choose(&mut self, parts: Vec<PartPlan>, handed_on_from: Option<&str>) -> Outcome {
        let observed = Observed::RouteChosen {
            decision_record: id(&format!("dr-{}", self.order_id)),
            parts,
            handed_on_from: handed_on_from.map(id),
        };
        let input = self.input(None, None, observed);
        self.feed(&input)
    }

    pub fn order_wide(&mut self, observed: Observed) -> Outcome {
        let input = self.input(None, None, observed);
        self.feed(&input)
    }

    pub fn about_part(&mut self, part_id: &str, observed: Observed) -> Outcome {
        let input = self.input(Some(part_id), None, observed);
        self.feed(&input)
    }

    pub fn about_segment(&mut self, part_id: &str, segment: &str, observed: Observed) -> Outcome {
        let input = self.input(Some(part_id), Some(segment), observed);
        self.feed(&input)
    }

    pub fn bind(&mut self, part_id: &str, segment: &str) -> Outcome {
        self.about_segment(part_id, segment, Observed::ProviderBound {})
    }

    pub fn settle(&mut self, part_id: &str, segment: &str) -> Outcome {
        let evidence = id(&format!("ev-{segment}"));
        self.about_segment(part_id, segment, Observed::SegmentSettled { evidence })
    }

    pub fn fail(&mut self, part_id: &str, segment: &str) -> Outcome {
        self.about_segment(part_id, segment, Observed::SegmentFailed {})
    }

    pub fn undo(&mut self, part_id: &str, segment: &str) -> Outcome {
        let evidence = id(&format!("ev-undo-{segment}"));
        self.about_segment(part_id, segment, Observed::ReversalCompleted { evidence })
    }

    /// Binds and settles segments `from..=to` of `p<part>`'s route, each applied.
    pub fn run_segments(&mut self, part_id: &str, from: u32, to: u32) {
        for n in from..=to {
            let segment = format!("{part_id}-s{n}");
            assert_eq!(self.bind(part_id, &segment), Outcome::Applied {});
            assert_eq!(self.settle(part_id, &segment), Outcome::Applied {});
        }
    }

    pub fn order(&self) -> &Order {
        self.state.as_ref().expect("the order is open")
    }

    pub fn part(&self, part_id: &str) -> &Part {
        self.order()
            .parts
            .iter()
            .find(|p| p.part_id.as_str() == part_id)
            .expect("the part exists")
    }

    pub fn part_state(&self, part_id: &str) -> OrderState {
        self.part(part_id).state
    }

    pub fn segment_state(&self, part_id: &str, segment: &str) -> SegmentState {
        self.part(part_id)
            .segments
            .iter()
            .find(|s| s.segment_id.as_str() == segment)
            .expect("the segment exists")
            .state
    }
}

pub fn no_route() -> Observed {
    Observed::NoRoute {
        decision_record: id("dr-no-route"),
    }
}

pub fn recalculated(segments: Vec<SegmentPlan>) -> Observed {
    Observed::RouteRecalculated {
        decision_record: id("dr-recalc"),
        segments,
    }
}

pub fn destination_changed(segments: Vec<SegmentPlan>) -> Observed {
    Observed::DestinationChanged {
        decision_record: id("dr-change"),
        acceptance: id("acceptance-1"),
        segments,
    }
}
