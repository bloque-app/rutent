//! Recalculation's invariants as property tests (`T-020-10`): over every sequence of segment reports a part can get,
//! a route is recalculated only from where the value is evidenced and a late report moves nothing (`INV-4`), and no
//! recalculation Core takes changes the destination or the to asset (`INV-10`). The platform tests are in
//! `conformance/scenarios/tests/recalc.rs`.

use proptest::prelude::*;
use rutent_contracts::core_internal::{Actor, ActorKind, Authorisation, Destination, Role};
use rutent_contracts::provider_role::{Denomination, NodeRef, OuterNode};
use rutent_contracts::router_api::{
    Endpoint, FeeFormula, Id, OperationKind, Part as RoutePart, PositiveAmount, Timestamp,
};
use rutent_contracts::state_machine::{
    self as stm, Observed, Order, Outcome, PartPlan, RecordedInput, SegmentPlan, SegmentState,
    Source, SourceKind, Standing, StateMachine, Subject,
};
use rutent_core_orch::recalc::{
    NotRecalculable, Rejected, check_route, evidenced, left_after, passed_again, route_end,
};
use rutent_stm::Stm;
use serde_json::{Value, json};

const PART: &str = "p1";

fn sid(text: &str) -> stm::Id {
    stm::Id::new(text).unwrap()
}

fn cop() -> Denomination {
    Denomination {
        id: "COP".into(),
        decimals: 2,
    }
}

fn outer(network: &str, asset: Denomination) -> Endpoint {
    Endpoint {
        node: NodeRef::Outer(OuterNode {
            network: network.into(),
        }),
        asset,
    }
}

fn inner(node: &str, asset: Denomination) -> Endpoint {
    Endpoint {
        node: NodeRef::Inner(node.into()),
        asset,
    }
}

fn plan(
    id: &str,
    provider: &str,
    kind: OperationKind,
    from: Endpoint,
    to: Endpoint,
) -> SegmentPlan {
    SegmentPlan {
        segment_id: sid(id),
        provider: sid(provider),
        kind,
        from,
        to,
    }
}

/// The chosen route: a withdrawal at `res-a` into its ledger, a route by `hop-b` to `res-c`'s ledger, and `res-c`'s
/// deposit out to its bank network.
fn chosen() -> Vec<SegmentPlan> {
    vec![
        plan(
            "w",
            "res-a",
            OperationKind::Withdrawal,
            outer("bank-a", cop()),
            inner("ledger-a", cop()),
        ),
        plan(
            "x",
            "hop-b",
            OperationKind::Route,
            inner("ledger-a", cop()),
            inner("ledger-c", cop()),
        ),
        plan(
            "d",
            "res-c",
            OperationKind::Deposit,
            inner("ledger-c", cop()),
            outer("bank-c", cop()),
        ),
    ]
}

fn input(n: usize, segment: Option<&str>, observed: Observed) -> RecordedInput {
    RecordedInput {
        record_id: stm::RecordId::new(format!("rec-{n}")).unwrap(),
        recorded_at: stm::Timestamp::new("2026-11-09T01:00:00Z").unwrap(),
        time_source: sid("core_clock"),
        source: Source {
            kind: SourceKind::Orchestrator,
            id: sid("core"),
        },
        standing: Standing::Stated,
        subject: Subject {
            order: sid("ord-1"),
            part: match observed {
                Observed::RouteChosen { .. } => None,
                _ => Some(sid(PART)),
            },
            segment: segment.map(sid),
        },
        observed,
    }
}

fn open() -> (Order, Vec<RecordedInput>) {
    let chosen = input(
        0,
        None,
        Observed::RouteChosen {
            decision_record: sid("rec-choice"),
            parts: vec![PartPlan {
                part_id: sid(PART),
                segments: chosen(),
            }],
            handed_on_from: None,
        },
    );
    (
        Stm::open(&chosen).expect("the route opens the order"),
        vec![chosen],
    )
}

/// A route from `at` to the part's destination, by `res-c`'s deposit (and, from another ledger, a hop there first),
/// under segment ids no one used.
fn recalculation_from(at: &Endpoint, n: usize) -> Vec<SegmentPlan> {
    let to_c = || inner("ledger-c", cop());
    let deposit = |id: String| {
        plan(
            &id,
            "res-c",
            OperationKind::Deposit,
            to_c(),
            outer("bank-c", cop()),
        )
    };
    if *at == to_c() {
        vec![deposit(format!("d-again-{n}"))]
    } else {
        vec![
            plan(
                &format!("x-again-{n}"),
                "hop-e",
                OperationKind::Route,
                at.clone(),
                to_c(),
            ),
            deposit(format!("d-again-{n}")),
        ]
    }
}

fn part(order: &Order) -> &stm::Part {
    &order.parts[0]
}

/// Whether some segment of `part` holds its value in flight.
fn in_flight(part: &stm::Part) -> bool {
    part.segments.iter().any(|s| match s.state {
        SegmentState::Started | SegmentState::OutcomeUnknown => true,
        SegmentState::FailedReversalOwed => s.kind != OperationKind::Deposit,
        _ => false,
    })
}

/// What happens to a part next.
#[derive(Debug, Clone)]
enum Step {
    /// The provider is silent, stops advertising, or is marked stale: no input comes.
    Silence,
    /// A segment's provider is bound: its value is handed over.
    Bound(usize),
    /// A segment settles.
    Settled(usize),
    /// A segment fails.
    Failed(usize),
    /// A segment's outcome is unknown.
    Unknown(usize),
    /// A segment's reversal completes.
    Reversed(usize),
    /// Core recalculates, if it may.
    Recalculate,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => Just(Step::Silence),
        3 => (0usize..4).prop_map(Step::Bound),
        3 => (0usize..4).prop_map(Step::Settled),
        2 => (0usize..4).prop_map(Step::Failed),
        2 => (0usize..4).prop_map(Step::Unknown),
        2 => (0usize..4).prop_map(Step::Reversed),
        2 => Just(Step::Recalculate),
    ]
}

/// The input `step` names, about the part's segment at that place, or `None` for silence or a recalculation.
fn report(order: &Order, step: &Step, n: usize) -> Option<RecordedInput> {
    let at = |i: usize| {
        let segments = &part(order).segments;
        segments[i % segments.len()].segment_id.as_str().to_owned()
    };
    let (i, observed) = match step {
        Step::Silence | Step::Recalculate => return None,
        Step::Bound(i) => (*i, Observed::ProviderBound {}),
        Step::Settled(i) => (
            *i,
            Observed::SegmentSettled {
                evidence: sid(&format!("ev-{n}")),
            },
        ),
        Step::Failed(i) => (*i, Observed::SegmentFailed {}),
        Step::Unknown(i) => (*i, Observed::SegmentOutcomeUnknown {}),
        Step::Reversed(i) => (
            *i,
            Observed::ReversalCompleted {
                evidence: sid(&format!("rev-{n}")),
            },
        ),
    };
    Some(input(n, Some(&at(i)), observed))
}

/// Whether `input` is evidence of where a value is: a settlement, a completed reversal, or a deposit's failure, which
/// leaves the value where it was.
fn evidences(order: &Order, input: &RecordedInput) -> bool {
    let kind = input.subject.segment.as_ref().and_then(|s| {
        part(order)
            .segments
            .iter()
            .find(|seg| &seg.segment_id == s)
            .map(|seg| seg.kind)
    });
    match input.observed {
        Observed::SegmentSettled { .. } | Observed::ReversalCompleted { .. } => true,
        Observed::SegmentFailed {} => kind == Some(OperationKind::Deposit),
        _ => false,
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Over every sequence of reports, a part's value is evidenced only where no segment holds it in flight, at the
    /// end of a settled segment, from where a route the state machine takes can start; and silence (a provider that
    /// doesn't answer, stops advertising or is marked stale), a provider bound or an outcome unknown never release
    /// it: only evidence does.
    // verifies: REQ-ORD-2, INV-4
    #[test]
    fn silence_never_releases_a_route(steps in prop::collection::vec(step(), 1..24)) {
        let (mut order, mut inputs) = open();
        let mut was = evidenced(part(&order)).map(|(at, _)| at.clone());
        prop_assert_eq!(was.clone(), Err(NotRecalculable::NotWithdrawn));
        let mut n = 1;
        for step in steps {
            n += 1;
            let reported = report(&order, &step, n);
            if let Step::Recalculate = step {
                if let Ok(at) = &was {
                    let route = recalculation_from(at, n);
                    let recalculated = input(n, None, Observed::RouteRecalculated {
                        decision_record: sid("rec-again"),
                        segments: route,
                    });
                    let next = Stm::transition(&order, &recalculated);
                    prop_assert_eq!(&next.last_input.outcome, &Outcome::Applied {});
                    order = next;
                    inputs.push(recalculated);
                }
            } else if let Some(reported) = &reported {
                order = Stm::transition(&order, reported);
                inputs.push(reported.clone());
            }
            let now = evidenced(part(&order)).map(|(at, settled)| {
                (at.clone(), settled.clone())
            });
            if let Ok((at, settled)) = &now {
                prop_assert!(!in_flight(part(&order)), "{:?}", part(&order));
                prop_assert_eq!(Some(at), stm::value_at(part(&order)));
                prop_assert_eq!(settled.state, SegmentState::Settled);
                prop_assert_eq!(&settled.to, at);
                prop_assert!(stm::recalculation_starts_where_value_sits(
                    part(&order),
                    &recalculation_from(at, n),
                ));
                // The value was released by evidence, never by silence, a bound provider or an unknown outcome.
                if was.is_err() {
                    let released = reported.as_ref().is_some_and(|r| evidences(&order, r));
                    prop_assert!(released, "released by {:?}", step);
                }
            }
            if in_flight(part(&order)) {
                prop_assert!(
                    matches!(now, Err(NotRecalculable::InFlight { .. })),
                    "{:?}",
                    now
                );
            }
            if let Step::Silence = step {
                prop_assert_eq!(now.clone().map(|(at, _)| at), was.clone());
            }
            was = now.map(|(at, _)| at);
        }
    }

    /// Once a route is recalculated, every report about a segment it retired, a late delivery included, is late:
    /// it moves no segment, and the value is where it was. No two segments of the part are ever started at once, so
    /// the value is never moved from two places, and no segment is started twice.
    // verifies: INV-4
    #[test]
    fn late_delivery_after_a_recalculation_moves_nothing_twice(
        deposit_fails in any::<bool>(),
        steps in prop::collection::vec((any::<bool>(), step()), 1..24),
    ) {
        let (mut order, _) = open();
        let mut n = 0;
        let apply = |order: &mut Order, input: RecordedInput| {
            let next = Stm::transition(order, &input);
            let outcome = next.last_input.outcome;
            *order = next;
            outcome
        };
        // The withdrawal settles; then either the deposit fails at its node, or the hop fails and is reversed.
        let mut feed = |order: &mut Order, segment: &str, observed: Observed| {
            n += 1;
            let reported = input(n, Some(segment), observed);
            prop_assert_eq!(apply(order, reported), Outcome::Applied {});
            Ok(())
        };
        feed(&mut order, "w", Observed::ProviderBound {})?;
        feed(&mut order, "w", Observed::SegmentSettled { evidence: sid("ev-w") })?;
        feed(&mut order, "x", Observed::ProviderBound {})?;
        if deposit_fails {
            feed(&mut order, "x", Observed::SegmentSettled { evidence: sid("ev-x") })?;
            feed(&mut order, "d", Observed::ProviderBound {})?;
            feed(&mut order, "d", Observed::SegmentFailed {})?;
        } else {
            feed(&mut order, "x", Observed::SegmentFailed {})?;
            feed(&mut order, "x", Observed::ReversalCompleted { evidence: sid("rev-x") })?;
        }
        let (at, _) = evidenced(part(&order)).expect("the value is evidenced at the failed segment's start");
        let at = at.clone();
        let route = recalculation_from(&at, 0);
        let recalculated = input(100, None, Observed::RouteRecalculated {
            decision_record: sid("rec-again"),
            segments: route.clone(),
        });
        prop_assert_eq!(apply(&mut order, recalculated), Outcome::Applied {});
        let retired: Vec<stm::Id> = part(&order).retired_segments.clone();
        prop_assert!(!retired.is_empty());
        let mut bound: Vec<stm::Id> = Vec::new();
        let mut m = 200;
        for (late, step) in steps {
            m += 1;
            let reported = if late {
                // A late report about a retired segment: a delivery, a failure, a bind, an unknown outcome.
                let observed = match step {
                    Step::Settled(_) | Step::Silence | Step::Recalculate => {
                        Observed::SegmentSettled { evidence: sid(&format!("late-{m}")) }
                    }
                    Step::Bound(_) => Observed::ProviderBound {},
                    Step::Failed(_) => Observed::SegmentFailed {},
                    Step::Unknown(_) => Observed::SegmentOutcomeUnknown {},
                    Step::Reversed(_) => Observed::ReversalCompleted { evidence: sid(&format!("late-{m}")) },
                };
                let segment = retired[m % retired.len()].as_str().to_owned();
                input(m, Some(&segment), observed)
            } else {
                match report(&order, &step, m) {
                    Some(reported) => reported,
                    None => continue,
                }
            };
            let before = part(&order).clone();
            let outcome = apply(&mut order, reported.clone());
            let named = reported.subject.segment.clone().unwrap();
            if retired.contains(&named) {
                // Late while the part is live; once it has ended, every report is after its end (`REQ-STM-6`).
                let ended = Outcome::NotApplicable { reason: stm::NotApplicableReason::Ended };
                prop_assert!(outcome == Outcome::Late {} || outcome == ended, "{:?}", outcome);
                prop_assert_eq!(part(&order), &before, "a late report moved nothing");
            } else if reported.observed == (Observed::ProviderBound {}) && outcome == (Outcome::Applied {}) {
                prop_assert!(!bound.contains(&named), "{:?} started twice", named);
                bound.push(named);
            }
            let started = part(&order)
                .segments
                .iter()
                .filter(|s| matches!(s.state, SegmentState::Started | SegmentState::OutcomeUnknown))
                .count();
            prop_assert!(started <= 1, "the value moved from two places: {:?}", part(&order));
        }
    }

    /// Whatever the router's choice, Core takes a recalculated route only where it ends at the authorisation's
    /// destination, in its to asset, from where the value is evidenced, moving all of it; and the state machine,
    /// given a route Core took, keeps the part's destination. A route that changes either is refused by Core, and by
    /// the state machine too, before anything is recorded.
    // verifies: INV-10, REQ-AUZ-3
    #[test]
    fn no_recalculation_changes_destination_or_to_asset(
        reserve in prop_oneof![Just("res-c"), Just("res-z")],
        network in prop_oneof![Just("bank-c"), Just("bank-z")],
        asset in prop_oneof![Just(("COP", 2u8)), Just(("COP", 0u8)), Just(("USD", 2u8))],
        starts_at_the_value in any::<bool>(),
        whole in any::<bool>(),
    ) {
        let (mut order, _) = open();
        for (n, (segment, observed)) in [
            ("w", Observed::ProviderBound {}),
            ("w", Observed::SegmentSettled { evidence: sid("ev-w") }),
            ("x", Observed::ProviderBound {}),
            ("x", Observed::SegmentSettled { evidence: sid("ev-x") }),
            ("d", Observed::ProviderBound {}),
            ("d", Observed::SegmentFailed {}),
        ].into_iter().enumerate() {
            order = Stm::transition(&order, &input(n + 1, Some(segment), observed));
        }
        let (at, _) = evidenced(part(&order)).expect("the failed deposit leaves the value at its node");
        let at = at.clone();
        let authorisation = authorisation();
        let to_asset = Denomination { id: asset.0.into(), decimals: asset.1 };
        let from = if starts_at_the_value { at.clone() } else { inner("ledger-a", cop()) };
        let amount = if whole { "1000000" } else { "999000" };
        let route = route_part(reserve, network, &from, amount, &to_asset);
        let checked = check_route(&zero_fee(), std::slice::from_ref(&route), &at, &positive("1000000"), &authorisation);
        let keeps = reserve == "res-c" && network == "bank-c" && to_asset == cop();
        match &checked {
            Ok(()) => {
                prop_assert!(keeps && starts_at_the_value && whole);
                let last = route.operations.last();
                prop_assert_eq!(last.provider.as_str(), authorisation.destination.reserve.as_str());
                prop_assert_eq!(&last.to.node, &NodeRef::Outer(authorisation.destination.node.clone()));
                prop_assert_eq!(&last.to.asset, &authorisation.to_asset);
            }
            Err(why) if !keeps => prop_assert!(
                matches!(why, Rejected::ChangesDestination | Rejected::ChangesToAsset | Rejected::StartsElsewhere | Rejected::MovesAnotherAmount),
                "{:?}", why
            ),
            Err(why) => prop_assert!(
                matches!(why, Rejected::StartsElsewhere | Rejected::MovesAnotherAmount),
                "{:?}", why
            ),
        }
        // What each move carries to policy and the log is read from the route, so the log's
        // `move-rests-on-its-authorisation`, a third guard, sees a route that ends elsewhere for what it is.
        let end = route_end(&route, &zero_fee(), &authorisation).expect("it ends in a deposit to an outer node");
        prop_assert_eq!(
            end.destination == authorisation.destination && end.to_asset == authorisation.to_asset,
            keeps
        );
        // The state machine's own rule, the second guard, on the same route as a plan.
        let plan: Vec<SegmentPlan> = route
            .operations
            .as_slice()
            .iter()
            .enumerate()
            .map(|(i, o)| plan(&format!("again-{i}"), o.provider.as_str(), o.kind, o.from.clone(), o.to.clone()))
            .collect();
        let destination_before = part(&order).destination.clone();
        let next = Stm::transition(&order, &input(50, None, Observed::RouteRecalculated {
            decision_record: sid("rec-again"),
            segments: plan,
        }));
        if next.last_input.outcome == (Outcome::Applied {}) {
            prop_assert!(keeps);
            prop_assert_eq!(&part(&next).destination, &destination_before);
        } else if !keeps {
            prop_assert_eq!(
                next.last_input.outcome,
                Outcome::NotApplicable { reason: stm::NotApplicableReason::ChangesDestination }
            );
        }
    }
}

/// A provider passed again from the node where its segment failed is read from the recorded inputs alone.
// verifies: REQ-ORD-4
#[test]
fn provider_passed_again_is_read_from_the_recorded_inputs() {
    let (mut order, mut inputs) = open();
    for (n, (segment, observed)) in [
        ("w", Observed::ProviderBound {}),
        (
            "w",
            Observed::SegmentSettled {
                evidence: sid("ev-w"),
            },
        ),
        ("x", Observed::ProviderBound {}),
        (
            "x",
            Observed::SegmentSettled {
                evidence: sid("ev-x"),
            },
        ),
        ("d", Observed::ProviderBound {}),
        ("d", Observed::SegmentFailed {}),
    ]
    .into_iter()
    .enumerate()
    {
        let reported = input(n + 1, Some(segment), observed);
        order = Stm::transition(&order, &reported);
        inputs.push(reported);
    }
    let at = evidenced(part(&order)).unwrap().0.clone();
    let route = recalculation_from(&at, 1);
    let passed = passed_again(&inputs, &sid(PART), &at, &route);
    assert_eq!(passed.len(), 1);
    assert_eq!(passed[0].provider, sid("res-c"));
    assert_eq!(passed[0].before, vec![sid("d")]);
    // A route through another reserve passes no one again; nor does a provider whose segment never started.
    let other = vec![plan(
        "d-other",
        "res-y",
        OperationKind::Deposit,
        at.clone(),
        outer("bank-c", cop()),
    )];
    assert!(passed_again(&inputs, &sid(PART), &at, &other).is_empty());
    let unbound: Vec<RecordedInput> = inputs
        .iter()
        .filter(|i| {
            !(i.observed == (Observed::ProviderBound {}) && i.subject.segment == Some(sid("d")))
        })
        .cloned()
        .collect();
    assert!(passed_again(&unbound, &sid(PART), &at, &route).is_empty());
}

fn zero_fee() -> FeeFormula {
    serde_json::from_value(json!({
        "flat": "0", "proportional": { "numerator": "0", "denominator": "1" }
    }))
    .unwrap()
}

fn positive(text: &str) -> PositiveAmount {
    PositiveAmount::new(text).unwrap()
}

fn customer() -> Actor {
    Actor {
        party: Id::new("customer-1").unwrap(),
        acts: Id::new("user-1").unwrap(),
        role: Role::Customer,
        kind: ActorKind::Person,
    }
}

fn authorisation() -> Authorisation {
    Authorisation {
        authorisation: Id::new("auth-1").unwrap(),
        order: Id::new("ord-1").unwrap(),
        origin_reserve: Id::new("res-a").unwrap(),
        destination: Destination {
            reserve: Id::new("res-c").unwrap(),
            node: OuterNode {
                network: "bank-c".into(),
            },
            account: Id::new("acct-2").unwrap(),
        },
        to_asset: cop(),
        floor: None,
        parts: None,
        given_by: customer(),
        at: Timestamp::new("2026-11-09T01:00:00Z").unwrap(),
    }
}

/// One part from `from`, moving `amount`, ending in a deposit by `reserve` to `network` in `asset`: an exchange at
/// `fx-d`, at `from`'s node, first where the asset is another.
fn route_part(
    reserve: &str,
    network: &str,
    from: &Endpoint,
    amount: &str,
    asset: &Denomination,
) -> RoutePart {
    let operation = |kind: &str, provider: &str, from: &Endpoint, to: &Endpoint| -> Value {
        json!({
            "kind": kind,
            "provider": provider,
            "from": from,
            "to": to,
            "amount_in": amount,
            "fee": { "asset": from.asset.id, "amount": "0" },
            "amount_out": amount,
            "figures": ["fig-1"],
        })
    };
    let to = outer(network, asset.clone());
    let operations = if asset.id == "COP" {
        vec![operation("deposit", reserve, from, &to)]
    } else {
        let converted = Endpoint {
            node: from.node.clone(),
            asset: asset.clone(),
        };
        vec![
            operation("exchange", "fx-d", from, &converted),
            operation("deposit", reserve, &converted, &to),
        ]
    };
    serde_json::from_value(json!({ "operations": operations })).unwrap()
}

/// A route that would leave the customer less than its floor after the operator's fee is refused, even where the
/// router kept it.
// verifies: REQ-ORD-4, REQ-AUZ-3
#[test]
fn route_leaving_less_than_the_floor_is_refused() {
    let at = inner("ledger-c", cop());
    let route = route_part("res-c", "bank-c", &at, "1000000", &cop());
    let mut floored = authorisation();
    floored.floor = Some(positive("995000"));
    let fee: FeeFormula = serde_json::from_value(json!({
        "flat": "10000", "proportional": { "numerator": "0", "denominator": "1" }
    }))
    .unwrap();
    let check = |fee: &FeeFormula| {
        check_route(
            fee,
            std::slice::from_ref(&route),
            &at,
            &positive("1000000"),
            &floored,
        )
    };
    assert_eq!(check(&fee), Err(Rejected::BelowFloor));
    assert_eq!(check(&zero_fee()), Ok(()));
}

/// A recalculated part starts where its value is evidenced, at an inner node, by no withdrawal: the guard Core runs
/// before any segment of it is handed over takes such a route, which the chosen route's guard refuses, and refuses
/// one starting with a withdrawal or outside the book, with every other rule of a route still held.
// verifies: REQ-ORD-2, REQ-ORD-16
#[test]
fn recalculated_route_starts_inside_the_book_by_no_withdrawal() {
    use rutent_core_orch::snapshot::{RouteRefusal, check_part, check_recalculated_part};
    let from_ledger = route_part(
        "res-c",
        "bank-c",
        &inner("ledger-c", cop()),
        "1000000",
        &cop(),
    );
    assert!(check_recalculated_part(&from_ledger).is_ok());
    assert_eq!(
        check_part(&from_ledger).unwrap_err().reason,
        RouteRefusal::NoWithdrawalFirst
    );
    let operation = |kind: &str, provider: &str, from: Endpoint, to: Endpoint| {
        json!({
            "kind": kind, "provider": provider, "from": from, "to": to,
            "amount_in": "1000000", "fee": { "asset": "COP", "amount": "0" },
            "amount_out": "1000000", "figures": ["fig-1"],
        })
    };
    let withdrawal_first: RoutePart = serde_json::from_value(json!({ "operations": [
        operation("withdrawal", "res-c", outer("bank-c", cop()), inner("ledger-c", cop())),
        operation("deposit", "res-c", inner("ledger-c", cop()), outer("bank-c", cop())),
    ]}))
    .unwrap();
    assert!(check_part(&withdrawal_first).is_ok());
    assert_eq!(
        check_recalculated_part(&withdrawal_first)
            .unwrap_err()
            .reason,
        RouteRefusal::NotFromAnInnerNode
    );
    let no_deposit: RoutePart = serde_json::from_value(json!({ "operations": [
        operation("route", "hop-b", inner("ledger-a", cop()), inner("ledger-c", cop())),
    ]}))
    .unwrap();
    assert_eq!(
        check_recalculated_part(&no_deposit).unwrap_err().reason,
        RouteRefusal::NoDepositLast
    );
}

/// What the quotes still ahead leave the customer: a segment that keeps its asset takes off what its quote charged,
/// an exchange gives out in its quote's proportion rounded down, and the operator's fee comes off what arrives.
// verifies: REQ-ORD-27
#[test]
fn quotes_still_ahead_are_read_from_what_arrived() {
    let usd = Denomination {
        id: "USD".into(),
        decimals: 2,
    };
    let operation = |kind: &str,
                     provider: &str,
                     from: Endpoint,
                     to: Endpoint,
                     amount_in: &str,
                     amount_out: &str| {
        let charge = if from.asset == to.asset {
            (amount_in.parse::<u64>().unwrap() - amount_out.parse::<u64>().unwrap()).to_string()
        } else {
            "0".to_owned()
        };
        json!({
            "kind": kind, "provider": provider, "from": from, "to": to,
            "amount_in": amount_in, "fee": { "asset": from.asset.id, "amount": charge },
            "amount_out": amount_out, "figures": ["fig-1"],
        })
    };
    let route: RoutePart = serde_json::from_value(json!({ "operations": [
        operation("withdrawal", "res-a", outer("bank-a", cop()), inner("ledger-a", cop()), "1000000", "1000000"),
        operation("route", "hop-b", inner("ledger-a", cop()), inner("ledger-c", cop()), "1000000", "998000"),
        operation("exchange", "fx-c", inner("ledger-c", cop()), inner("ledger-c", usd.clone()), "998000", "249500"),
        operation("deposit", "res-c", inner("ledger-c", usd.clone()), outer("bank-c", usd), "249500", "249400"),
    ]}))
    .unwrap();
    let operations = route.operations.as_slice();
    let fee: FeeFormula = serde_json::from_value(json!({
        "flat": "100", "proportional": { "numerator": "0", "denominator": "1" }
    }))
    .unwrap();
    // As priced: 1 000 000 at the ledger leaves 249 400 at the destination, less the operator's 100.
    let left = |arrived: &str| {
        left_after(operations, &fee, 0, &positive(arrived)).map(|a| a.as_str().to_owned())
    };
    assert_eq!(left("1000000").as_deref(), Some("249300"));
    // 10 000 short: the hop still charges 2 000, the exchange converts 988 000 at a quarter, rounded down.
    assert_eq!(left("990000").as_deref(), Some("246800"));
    // After the exchange settled, only the deposit's charge and the operator's fee are ahead.
    assert_eq!(
        left_after(operations, &fee, 2, &positive("200000"))
            .map(|a| a.as_str().to_owned())
            .as_deref(),
        Some("199800")
    );
    // Past what Core counts in, no projection, which holds no floor.
    let huge = "9".repeat(60);
    assert_eq!(left_after(operations, &fee, 1, &positive(&huge)), None);
}
