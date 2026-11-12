//! A seeded generator of recorded inputs, driving an order through the reference host.
//!
//! "Property test" here means this generator with a few thousand seeds, not a framework: a framework such as
//! `proptest` would bring its whole dependency tree into the licence record (`REQ-MOD-5`), and nothing here needs
//! shrinking, since every failure names its seed and a run is a pure function of it.
//!
//! The generator is biased on purpose. Inputs picked uniformly are almost all not applicable, so a property would
//! pass without testing anything. Most inputs are the next valid one from the order's current state; the rest are
//! the ones that must change nothing: repeats, inputs for another order or an unknown subject, reports about
//! retired segments, routes that change the destination, reuse an id or start away from the value.
#![allow(dead_code)]

use super::*;
use rutent_contracts::state_machine::host::{Delivery, Host, TransitionRecord};
use rutent_contracts::state_machine::{Destination, StuckReason, value_at};
use rutent_stm::reference_host::{MemoryStore, ReferenceHost};

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    pub fn pct(&mut self, p: u64) -> bool {
        self.next() % 100 < p
    }
}

/// How the host answered a submit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Recorded,
    AlreadyRecorded,
}

/// One delivered input, with the order's state before and after.
pub struct Step {
    pub input: RecordedInput,
    pub prev: Option<Order>,
    pub next: Option<Order>,
    pub answer: Answer,
}

/// A whole run of one seed.
pub struct Trace {
    pub seed: u64,
    pub order_id: String,
    pub steps: Vec<Step>,
    pub journal: Vec<TransitionRecord>,
}

impl Trace {
    pub fn last(&self) -> Option<&Order> {
        self.steps.iter().rev().find_map(|s| s.next.as_ref())
    }
}

struct Gen {
    rng: Rng,
    run: Run,
    fresh: u32,
}

impl Gen {
    fn fresh_id(&mut self, tag: &str) -> String {
        self.fresh += 1;
        format!("{tag}{}", self.fresh)
    }

    fn opening(&mut self) -> RecordedInput {
        let n = match self.rng.below(100) {
            0..=54 => 1,
            55..=84 => 2,
            _ => 3,
        };
        let handed = self.rng.pct(15);
        let mut parts = Vec::new();
        for k in 0..n {
            let p = format!("p{k}");
            let segments = if handed {
                if self.rng.pct(50) {
                    handed_on_route(&p, "ledger")
                } else {
                    let mut r = route_from_ledger(&p);
                    r[0].segment_id = id(&format!("{p}-s1"));
                    r
                }
            } else if self.rng.pct(50) {
                route(&p)
            } else {
                route_via(&p, "exchange-b")
            };
            parts.push(part(&p, segments));
        }
        self.run.input(
            None,
            None,
            Observed::RouteChosen {
                decision_record: id("dr"),
                parts,
                handed_on_from: handed.then(|| id("ord-before")),
            },
        )
    }

    /// A route from where the part's value sits (or, rarely, from nowhere in particular), to its destination or
    /// to a new one, with the mistakes the state machine must refuse mixed in.
    fn restart_route(&mut self, part: &Part, new_destination: bool) -> Vec<SegmentPlan> {
        let mut at = value_at(part)
            .cloned()
            .unwrap_or_else(|| inner("ledger", "USDC"));
        if self.rng.pct(12) {
            at = inner("elsewhere", &at.asset.id);
        }
        let mut dest = part.destination.clone();
        if new_destination {
            let k = self.fresh_id("");
            dest = Destination {
                provider: id(&format!("reserve-new{k}")),
                to: outer(&format!("net{k}"), &at.asset.id),
            };
        } else if self.rng.pct(10) {
            dest.to = outer("other-net", &dest.to.asset.id);
        }
        let mut cur = at;
        let mut segments = Vec::new();
        let mut push = |g: &mut Gen, provider: &str, kind, from: Endpoint, to: Endpoint| {
            let segment_id = g.fresh_id("g");
            segments.push(plan(&segment_id, provider, kind, from, to));
        };
        if self.rng.pct(40) {
            let vault = self.fresh_id("vault");
            let to = inner(&vault, &cur.asset.id);
            push(
                self,
                "router-x",
                OperationKind::Route,
                cur.clone(),
                to.clone(),
            );
            cur = to;
        }
        if cur.asset.id != dest.to.asset.id {
            let to = endpoint(cur.node.clone(), &dest.to.asset.id);
            push(
                self,
                "exchange-a",
                OperationKind::Exchange,
                cur.clone(),
                to.clone(),
            );
            cur = to;
        }
        push(
            self,
            dest.provider.as_str(),
            OperationKind::Deposit,
            cur,
            dest.to.clone(),
        );
        if self.rng.pct(8) {
            let used: Vec<&Id> = part
                .segments
                .iter()
                .map(|s| &s.segment_id)
                .chain(&part.retired_segments)
                .collect();
            segments[0].segment_id = used[self.rng.below(used.len())].clone();
        }
        segments
    }

    fn segment_kind(&mut self) -> Observed {
        match self.rng.below(5) {
            0 => Observed::ProviderBound {},
            1 => Observed::SegmentSettled { evidence: id("ev") },
            2 => Observed::SegmentFailed {},
            3 => Observed::SegmentOutcomeUnknown {},
            _ => Observed::ReversalCompleted {
                evidence: id("ev-undo"),
            },
        }
    }

    /// The next valid input for a part from where its segments stand, or `None` when the part is done.
    fn progress(&mut self, part: &Part) -> Option<(Option<String>, Observed)> {
        let index = part
            .segments
            .iter()
            .position(|s| s.state != SegmentState::Settled)?;
        let segment = &part.segments[index];
        let on = |g: &mut Gen, observed: Observed| {
            let _ = g;
            Some((Some(segment.segment_id.as_str().to_string()), observed))
        };
        match segment.state {
            SegmentState::Pending => {
                if self.rng.pct(12) {
                    let later = &part.segments[self.rng.below(part.segments.len())];
                    return Some((
                        Some(later.segment_id.as_str().to_string()),
                        Observed::ProviderBound {},
                    ));
                }
                on(self, Observed::ProviderBound {})
            }
            SegmentState::Started => {
                let observed = match self.rng.below(100) {
                    0..=54 => Observed::SegmentSettled { evidence: id("ev") },
                    55..=79 => Observed::SegmentFailed {},
                    _ => Observed::SegmentOutcomeUnknown {},
                };
                on(self, observed)
            }
            SegmentState::OutcomeUnknown => {
                let observed = match self.rng.below(3) {
                    0 => Observed::SegmentSettled { evidence: id("ev") },
                    1 => Observed::SegmentFailed {},
                    _ => Observed::ReversalCompleted {
                        evidence: id("ev-undo"),
                    },
                };
                on(self, observed)
            }
            SegmentState::FailedReversalOwed if segment.kind != OperationKind::Deposit => on(
                self,
                Observed::ReversalCompleted {
                    evidence: id("ev-undo"),
                },
            ),
            SegmentState::FailedReversalOwed | SegmentState::ReversalComplete => {
                if self.rng.pct(55) {
                    let route = self.restart_route(part, false);
                    Some((None, recalculated(route)))
                } else if segment.kind == OperationKind::Withdrawal && self.rng.pct(60) {
                    Some((None, Observed::WithdrawalCancelled {}))
                } else {
                    Some((None, no_route()))
                }
            }
            SegmentState::Settled => None,
        }
    }

    fn next_input(&mut self, state: &Option<Order>, sent: &[RecordedInput]) -> RecordedInput {
        let Some(order) = state else {
            return if self.rng.pct(10) && sent.is_empty() {
                // An input before any route: recorded, with no state.
                self.run
                    .input(Some("p0"), Some("p0-s1"), Observed::ProviderBound {})
            } else {
                self.opening()
            };
        };
        let roll = self.rng.below(100);
        let part = order.parts[self.rng.below(order.parts.len())].clone();
        let part_id = part.part_id.as_str().to_string();
        let (segment, observed): (Option<String>, Observed) = match roll {
            0..=54 => match self.progress(&part) {
                Some(step) => step,
                None => (
                    None,
                    Observed::StuckDeclared {
                        reason: StuckReason::OverTime,
                    },
                ),
            },
            55..=62 => {
                let route = self.restart_route(&part, false);
                (None, recalculated(route))
            }
            63..=70 => {
                let new_destination = self.rng.pct(80);
                let route = self.restart_route(&part, new_destination);
                (None, destination_changed(route))
            }
            71..=73 => (None, no_route()),
            74..=75 => (None, Observed::WaitingAtFloor {}),
            76..=79 => (
                None,
                Observed::StuckDeclared {
                    reason: if self.rng.pct(50) {
                        StuckReason::OverTime
                    } else {
                        StuckReason::Dispute
                    },
                },
            ),
            80..=84 => {
                let to = if self.rng.pct(90) {
                    "ord-next"
                } else {
                    order.order_id.as_str()
                };
                (
                    None,
                    Observed::HandedOn {
                        linked_order: id(to),
                        acceptance: id("acc"),
                    },
                )
            }
            85..=87 => (None, Observed::WithdrawalCancelled {}),
            88..=90 => return self.run.input(None, None, Observed::EndRequested {}),
            91..=95 => {
                // Noise: a random kind about a random segment of the part.
                let s = &part.segments[self.rng.below(part.segments.len())];
                (Some(s.segment_id.as_str().to_string()), self.segment_kind())
            }
            96..=97 => {
                // Another order, or a part and segment the order never had.
                let kind = self.segment_kind();
                let mut input = self.run.input(Some(&part_id), Some("p0-s1"), kind);
                match self.rng.below(3) {
                    0 => input.subject.order = id("ord-theirs"),
                    1 => input.subject.part = Some(id("no-such-part")),
                    _ => input.subject.segment = Some(id("no-such-segment")),
                }
                return input;
            }
            _ => match part.retired_segments.first() {
                Some(retired) => (Some(retired.as_str().to_string()), self.segment_kind()),
                None => (None, Observed::EndRequested {}),
            },
        };
        // Only a segment-scope input names a segment.
        match observed.scope() {
            rutent_contracts::state_machine::Scope::Order => self.run.input(None, None, observed),
            rutent_contracts::state_machine::Scope::Part => {
                self.run.input(Some(&part_id), None, observed)
            }
            rutent_contracts::state_machine::Scope::Segment => {
                let segment =
                    segment.unwrap_or_else(|| part.segments[0].segment_id.as_str().to_string());
                self.run.input(Some(&part_id), Some(&segment), observed)
            }
        }
    }

    /// The input that resolves what is in flight, as the evidence would arrive: used after the random steps, so an
    /// order is driven to where each part is done or held.
    fn resolve(&mut self, order: &Order) -> Option<RecordedInput> {
        for part in &order.parts {
            if matches!(
                part.state,
                OrderState::Complete
                    | OrderState::Failed
                    | OrderState::HandedOn
                    | OrderState::EndedWithoutCompleting
                    | OrderState::Stuck
                    | OrderState::WaitingForCustomer
            ) {
                continue;
            }
            let p = part.part_id.as_str().to_string();
            let Some(index) = part
                .segments
                .iter()
                .position(|s| s.state != SegmentState::Settled)
            else {
                continue;
            };
            let segment = &part.segments[index];
            let s = segment.segment_id.as_str().to_string();
            return Some(match (segment.state, segment.kind) {
                (SegmentState::Pending, _) => {
                    self.run
                        .input(Some(&p), Some(&s), Observed::ProviderBound {})
                }
                (SegmentState::Started, _) => {
                    let observed = if self.rng.pct(60) {
                        Observed::SegmentSettled { evidence: id("ev") }
                    } else {
                        Observed::SegmentFailed {}
                    };
                    self.run.input(Some(&p), Some(&s), observed)
                }
                (SegmentState::OutcomeUnknown, _) => self.run.input(
                    Some(&p),
                    Some(&s),
                    Observed::SegmentSettled { evidence: id("ev") },
                ),
                (SegmentState::FailedReversalOwed, kind) if kind != OperationKind::Deposit => {
                    self.run.input(
                        Some(&p),
                        Some(&s),
                        Observed::ReversalCompleted {
                            evidence: id("ev-undo"),
                        },
                    )
                }
                (_, OperationKind::Withdrawal) if index == 0 => {
                    self.run
                        .input(Some(&p), None, Observed::WithdrawalCancelled {})
                }
                _ => self.run.input(Some(&p), None, no_route()),
            });
        }
        None
    }
}

fn submit(
    host: &mut ReferenceHost<Stm>,
    input: &RecordedInput,
    steps: &mut Vec<Step>,
    sent: &mut Vec<RecordedInput>,
) {
    let order_id = input.subject.order.clone();
    let prev = host.state(&order_id).expect("a read works");
    let delivery = host.submit(input.clone()).expect("a submit works");
    let next = host.state(&order_id).expect("a read works");
    let answer = match delivery {
        Delivery::Recorded(_) => Answer::Recorded,
        Delivery::AlreadyRecorded(_) => Answer::AlreadyRecorded,
    };
    if answer == Answer::Recorded {
        sent.push(input.clone());
    }
    steps.push(Step {
        input: input.clone(),
        prev,
        next,
        answer,
    });
}

/// One seed's run: some random inputs, then the evidence that resolves what is in flight.
pub fn drive(seed: u64) -> Trace {
    let order_id = format!("ord-{seed}");
    let mut g = Gen {
        rng: Rng::new(seed),
        run: Run::new(&order_id),
        fresh: 0,
    };
    let mut host = ReferenceHost::<Stm>::open(MemoryStore::new()).expect("an empty store opens");
    let mut steps = Vec::new();
    let mut sent: Vec<RecordedInput> = Vec::new();
    let length = 15 + g.rng.below(70);
    for _ in 0..length {
        let state = host.state(&id(&order_id)).expect("a read works");
        let input = if !sent.is_empty() && g.rng.pct(8) {
            // Delivered again, from anywhere in the history: the host answers from its record.
            sent[g.rng.below(sent.len())].clone()
        } else {
            g.next_input(&state, &sent)
        };
        submit(&mut host, &input, &mut steps, &mut sent);
    }
    for _ in 0..200 {
        let state = host.state(&id(&order_id)).expect("a read works");
        let Some(order) = state else { break };
        let Some(input) = g.resolve(&order) else {
            break;
        };
        submit(&mut host, &input, &mut steps, &mut sent);
    }
    let journal = host.journal(&id(&order_id)).expect("a read works");
    Trace {
        seed,
        order_id,
        steps,
        journal,
    }
}

/// The seeds `1..=count`, run once per test binary and shared by every property.
pub fn traces(count: u64) -> &'static [Trace] {
    static TRACES: std::sync::OnceLock<Vec<Trace>> = std::sync::OnceLock::new();
    let all = TRACES.get_or_init(|| (1..=count).map(drive).collect());
    assert_eq!(
        all.len() as u64,
        count,
        "every property runs the same seeds"
    );
    all
}
