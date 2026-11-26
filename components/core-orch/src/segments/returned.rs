//! A linked order's first move out of a held node, reversed back into it, lands the value there again (`T-020-56`,
//! `INV-1`): over the segments' own record, the order Core follows taking each input as the state machine does.

use std::cell::Cell;
use std::collections::BTreeMap;

use rutent_contracts::core_internal::{Actor, ActorKind, Role};
use rutent_contracts::provider_role::{Denomination, NodeRef, Operation, OuterNode, Statement};
use rutent_contracts::router_api::{Amount, Endpoint, Id};
use rutent_contracts::state_machine::{
    self as stm, Observed, OperationKind, PartPlan, RecordedInput, SegmentPlan, Source, SourceKind,
    Standing, StateMachine as _, Subject,
};
use rutent_stm::Stm;

use super::landing::{Landed, Landings};
use super::{InFlight, SegmentOutcome, Segments, Tracked, measured};
use crate::snapshot::{Clock, Reading};

const LINKED: &str = "ord-linked";
const FIRST: &str = "ord-1";
const PART: &str = "part-1";
const HELD: &str = "held-node";
const NEXT: &str = "next-node";
const AMOUNT: &str = "1000";

struct Fixed(Cell<u64>);

impl Clock for Fixed {
    fn now(&self) -> Reading {
        Reading::at(self.0.get())
    }
}

/// The held node's holding account: it holds `HELD` alone, places and releases everything, and keeps what it was
/// asked.
#[derive(Default)]
struct Account {
    calls: Vec<String>,
    /// Whether the next node has a holding account too.
    next_held: bool,
}

impl Landings for Account {
    fn holds(&self, node: &str) -> bool {
        node == HELD || (self.next_held && node == NEXT)
    }

    fn land(&mut self, node: &str, asset: &str, order: &str, amount: &Amount) -> Landed {
        self.calls
            .push(format!("land {node} {asset} {order} {}", amount.as_str()));
        Landed::Placed
    }

    fn release(&mut self, node: &str, asset: &str, order: &str, amount: &Amount) -> Landed {
        self.calls.push(format!(
            "release {node} {asset} {order} {}",
            amount.as_str()
        ));
        Landed::Placed
    }

    fn restore(&mut self, node: &str, asset: &str, order: &str, amount: &Amount) -> Landed {
        self.calls.push(format!(
            "restore {node} {asset} {order} {}",
            amount.as_str()
        ));
        Landed::Placed
    }
}

fn id(text: &str) -> stm::Id {
    stm::Id::new(text).expect("an id")
}

fn cop() -> Denomination {
    Denomination {
        id: "COP".into(),
        decimals: 2,
    }
}

fn inner(node: &str) -> Endpoint {
    Endpoint {
        node: NodeRef::Inner(node.into()),
        asset: cop(),
    }
}

/// The linked order's route from the held node: a ramp's transfer to the next node, then a reserve's deposit to its
/// bank, under segment ids `transfer` and `deposit`.
fn route(transfer: &str, deposit: &str) -> Vec<SegmentPlan> {
    vec![
        SegmentPlan {
            segment_id: id(transfer),
            provider: id("ramp"),
            kind: OperationKind::Route,
            from: inner(HELD),
            to: inner(NEXT),
        },
        SegmentPlan {
            segment_id: id(deposit),
            provider: id("out"),
            kind: OperationKind::Deposit,
            from: inner(NEXT),
            to: Endpoint {
                node: NodeRef::Outer(OuterNode {
                    network: "out-bank".into(),
                }),
                asset: cop(),
            },
        },
    ]
}

fn input(n: u32, segment: Option<&str>, observed: Observed) -> RecordedInput {
    // An order's opening is about the order alone; every other input here is about the part, or its segment.
    let part = (!matches!(observed, Observed::RouteChosen { .. })).then(|| id(PART));
    RecordedInput {
        record_id: stm::RecordId::new(format!("rec-{n}")).expect("a record id"),
        recorded_at: stm::Timestamp::new("2026-11-25T00:00:00Z").expect("a time"),
        time_source: id("core_clock"),
        source: Source {
            kind: SourceKind::Orchestrator,
            id: id("core-orch"),
        },
        standing: Standing::Stated,
        subject: Subject {
            order: id(LINKED),
            part,
            segment: segment.map(id),
        },
        observed,
    }
}

/// Applies `input` to the order the segments follow, as the state machine takes it.
fn take(segments: &mut Segments, input: &RecordedInput) {
    let tracked = segments.orders.get_mut(LINKED).expect("followed");
    let next = Stm::transition(&tracked.order, input);
    assert_eq!(
        next.last_input.outcome,
        stm::Outcome::Applied {},
        "{:?}",
        input.observed
    );
    tracked.order = next;
}

/// The flight of `segment`, a transfer out of the held node of `AMOUNT`, ended with `outcome`.
fn flight(segment: &str, outcome: SegmentOutcome) -> InFlight {
    let statement: Statement<Operation> = serde_json::from_value(serde_json::json!({
        "deployment": { "name": "rutent-test", "operator_key": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA" },
        "protocol_version": "2.0.1",
        "shape": "urn:rutent:provider-role:operation",
        "maker": "rutent-operator",
        "made_at": "2026-11-25T00:00:00Z",
        "signature": { "scheme": "ed25519", "key": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA",
            "value": "BwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyAhIiMkJSYnKCkqKywtLi8wMTIzNDU2Nzg5Ojs8PT4_QEFCQ0RFRg" },
        "statement": {
            "kind": "transfer", "transaction": format!("tx-{segment}"), "provider": "ramp",
            "origin": HELD, "destination": NEXT, "asset": "COP", "direction": "exact_in",
            "amount": AMOUNT, "dry_run": false, "inputs": {},
        },
    }))
    .expect("a transfer");
    InFlight {
        place: 0,
        transaction: Id::new(format!("tx-{segment}")).expect("an id"),
        provider: Id::new("ramp").expect("an id"),
        caller: Actor {
            party: Id::new("operator").expect("an id"),
            acts: Id::new("rutent-core").expect("an id"),
            role: Role::Operator,
            kind: ActorKind::Component,
        },
        statement,
        effect: None,
        begun: true,
        carried: true,
        answered: None,
        started: None,
        unknown: Vec::new(),
        steps: BTreeMap::new(),
        evidence: None,
        graded: None,
        kept: None,
        fees: None,
        ended: true,
        outcome: Some(outcome),
        charged: None,
        stated_out: None,
        reported: None,
        begin_refused: None,
        verdict: None,
        drains: Vec::new(),
        record: None,
        late: BTreeMap::new(),
        carries: Vec::new(),
        relies: Vec::new(),
        priced: measured::Priced {
            quoted: Amount::new(AMOUNT).expect("an amount"),
            asset: cop(),
        },
        heard: Vec::new(),
        record_entry: None,
        reopened: false,
    }
}

/// The linked order followed from its start at the held node, its first move handed over and failed, owing its
/// reversal.
fn first_move_failed(clock: &Fixed) -> Segments {
    let mut segments = Segments::new();
    let chosen = input(
        1,
        None,
        Observed::RouteChosen {
            decision_record: id("dr-linked"),
            parts: vec![PartPlan {
                part_id: id(PART),
                segments: route("seg-transfer", "seg-deposit"),
            }],
            handed_on_from: Some(id(FIRST)),
        },
    );
    let order = Stm::open(&chosen).expect("the linked order opens at the held node");
    segments.orders.insert(
        LINKED.into(),
        Tracked {
            order,
            since: clock.now(),
        },
    );
    take(
        &mut segments,
        &input(2, Some("seg-transfer"), Observed::ProviderBound {}),
    );
    take(
        &mut segments,
        &input(3, Some("seg-transfer"), Observed::SegmentFailed {}),
    );
    segments.flights.insert(
        (LINKED.into(), "seg-transfer".into()),
        flight("seg-transfer", SegmentOutcome::Failed),
    );
    segments
}

/// The linked order's first move out of the held node fails and is reversed: the value is back at the node, the linked
/// order's now, and the next landing places it in the node's holding account for the linked order, what the move took
/// in, once. A reopened Core that kept nothing of the reversal reads the same from the order it follows. Recalculated,
/// the reversed move retired, the value rests there still until the new route's first move settles, and then the
/// landing is released (`INV-1`, `REQ-AUD-15`, `T-020-56`).
// verifies: INV-1
#[test]
fn reversal_out_of_a_held_node_lands_the_value_there_again() {
    let clock = Fixed(Cell::new(1_795_000_000));
    let mut segments = first_move_failed(&clock);
    let mut account = Account::default();
    assert_eq!(
        segments.land_settled(&mut account),
        0,
        "a failed move owing its reversal leaves nothing to land"
    );
    assert!(account.calls.is_empty(), "{:?}", account.calls);

    // The reversal completes, as `Reversals::complete` records it: the landing noted, then the input taken.
    segments.note_reversed(
        &Id::new(LINKED).expect("an id"),
        &Id::new("seg-transfer").expect("an id"),
        &Amount::new(AMOUNT).expect("an amount"),
    );
    take(
        &mut segments,
        &input(
            4,
            Some("seg-transfer"),
            Observed::ReversalCompleted {
                evidence: id("ev-back"),
            },
        ),
    );
    let reopened = segments.clone();
    assert_eq!(segments.land_settled(&mut account), 1, "landed again");
    assert_eq!(
        account.calls,
        [format!("land {HELD} COP {LINKED} {AMOUNT}")],
        "for the linked order, whose value it is now, what its move took in"
    );
    assert_eq!(
        segments
            .resting_totals()
            .get(&(HELD.to_owned(), LINKED.to_owned())),
        Some(&Amount::new(AMOUNT).expect("an amount"))
    );
    segments.land_settled(&mut account);
    assert_eq!(account.calls.len(), 1, "landed once");

    // A Core that reopened keeps nothing of the reversal's completion: it reads the value back at the node from the
    // order it follows, and restores it once.
    let mut reopened = reopened;
    reopened.returned.clear();
    let mut restarted = Account::default();
    assert_eq!(reopened.reconcile_landings(&mut restarted), 1);
    assert_eq!(
        restarted.calls,
        [format!("restore {HELD} COP {LINKED} {AMOUNT}")]
    );

    // Recalculated from the held node: the reversed move retired, the value rests there still.
    take(
        &mut segments,
        &input(
            5,
            None,
            Observed::RouteRecalculated {
                decision_record: id("dr-linked-2"),
                segments: route("seg-transfer-2", "seg-deposit-2"),
            },
        ),
    );
    assert!(segments.retired(
        &Id::new(LINKED).expect("an id"),
        &Id::new("seg-transfer").expect("an id")
    ));
    segments.land_settled(&mut account);
    assert_eq!(account.calls.len(), 1, "nothing released while it rests");
    // The new route's first move settles: the value left the node, and its landing is released, once.
    take(
        &mut segments,
        &input(6, Some("seg-transfer-2"), Observed::ProviderBound {}),
    );
    take(
        &mut segments,
        &input(
            7,
            Some("seg-transfer-2"),
            Observed::SegmentSettled {
                evidence: id("ev-moved"),
            },
        ),
    );
    segments.flights.insert(
        (LINKED.into(), "seg-transfer-2".into()),
        flight(
            "seg-transfer-2",
            SegmentOutcome::Settled {
                amount_out: Amount::new(AMOUNT).expect("an amount"),
                evidence: Id::new("ev-moved").expect("an id"),
            },
        ),
    );
    segments.land_settled(&mut account);
    segments.land_settled(&mut account);
    assert_eq!(
        account.calls,
        [
            format!("land {HELD} COP {LINKED} {AMOUNT}"),
            format!("release {HELD} COP {LINKED} {AMOUNT}"),
        ]
    );
    assert!(
        !segments
            .resting_totals()
            .contains_key(&(HELD.to_owned(), LINKED.to_owned())),
        "nothing rests there for it any more"
    );
}

/// A move that isn't a linked order's first out of a held node lands nothing when it is reversed: the reversal of a
/// segment whose value came from where no hand-on left it is the segment before it's to hold (`INV-1`).
// verifies: INV-1
#[test]
fn reversal_of_a_move_that_is_not_a_linked_orders_first_lands_nothing() {
    let clock = Fixed(Cell::new(1_795_000_000));
    let mut segments = first_move_failed(&clock);
    // The same order, as though it weren't handed on: no value of its own came to the node by a hand-on.
    let tracked = segments.orders.get_mut(LINKED).expect("followed");
    tracked.order.handed_on_from = None;
    segments.note_reversed(
        &Id::new(LINKED).expect("an id"),
        &Id::new("seg-transfer").expect("an id"),
        &Amount::new(AMOUNT).expect("an amount"),
    );
    let mut account = Account::default();
    assert_eq!(segments.land_settled(&mut account), 0);
    assert!(account.calls.is_empty(), "{:?}", account.calls);
}

/// The linked order's first move out of the held node fails, and its reversal completes on a confirmation that less
/// came back than the move took in: what is landed again is what the confirmation says came back, so less returned
/// attributes less; and a confirmation of more than the move took in lands what it took in, never more (`INV-1`,
/// `T-020-57`).
// verifies: INV-1
#[test]
fn value_landed_again_is_what_the_wallet_confirms_came_back() {
    for (back, landed) in [("900", "900"), (AMOUNT, AMOUNT), ("1100", AMOUNT)] {
        let clock = Fixed(Cell::new(1_795_000_000));
        let mut segments = first_move_failed(&clock);
        segments.note_reversed(
            &Id::new(LINKED).expect("an id"),
            &Id::new("seg-transfer").expect("an id"),
            &Amount::new(back).expect("an amount"),
        );
        take(
            &mut segments,
            &input(
                4,
                Some("seg-transfer"),
                Observed::ReversalCompleted {
                    evidence: id("ev-back"),
                },
            ),
        );
        let mut account = Account::default();
        assert_eq!(segments.land_settled(&mut account), 1, "confirmed {back}");
        assert_eq!(
            account.calls,
            [format!("land {HELD} COP {LINKED} {landed}")],
            "confirmed {back}"
        );
        assert_eq!(
            segments
                .resting_totals()
                .get(&(HELD.to_owned(), LINKED.to_owned())),
            Some(&Amount::new(landed).expect("an amount")),
            "confirmed {back}"
        );
    }
}

/// The linked order's first move out of the held node settles at the next node, where it is landed, and is then
/// proven undone, as the state machine lets a starting segment be: its value left the next node, whose landing is
/// released, and is back at the held node for the linked order, where it is landed again, each once (`INV-1`,
/// `T-020-57`, the milestone reviewer's check of `T-020-56`).
// verifies: INV-1
#[test]
fn linked_orders_settled_then_reversed_first_move_is_returned_to_its_node() {
    let clock = Fixed(Cell::new(1_795_000_000));
    let mut segments = Segments::new();
    let chosen = input(
        1,
        None,
        Observed::RouteChosen {
            decision_record: id("dr-linked"),
            parts: vec![PartPlan {
                part_id: id(PART),
                segments: route("seg-transfer", "seg-deposit"),
            }],
            handed_on_from: Some(id(FIRST)),
        },
    );
    let order = Stm::open(&chosen).expect("the linked order opens at the held node");
    segments.orders.insert(
        LINKED.into(),
        Tracked {
            order,
            since: clock.now(),
        },
    );
    take(
        &mut segments,
        &input(2, Some("seg-transfer"), Observed::ProviderBound {}),
    );
    take(
        &mut segments,
        &input(
            3,
            Some("seg-transfer"),
            Observed::SegmentSettled {
                evidence: id("ev-moved"),
            },
        ),
    );
    segments.flights.insert(
        (LINKED.into(), "seg-transfer".into()),
        flight(
            "seg-transfer",
            SegmentOutcome::Settled {
                amount_out: Amount::new(AMOUNT).expect("an amount"),
                evidence: Id::new("ev-moved").expect("an id"),
            },
        ),
    );
    let mut account = Account {
        next_held: true,
        ..Account::default()
    };
    assert_eq!(segments.land_settled(&mut account), 1);
    assert_eq!(
        account.calls,
        [format!("land {NEXT} COP {LINKED} {AMOUNT}")],
        "landed where it settled"
    );

    // The starting segment is proven undone: the value is back at the held node.
    segments.note_reversed(
        &Id::new(LINKED).expect("an id"),
        &Id::new("seg-transfer").expect("an id"),
        &Amount::new(AMOUNT).expect("an amount"),
    );
    take(
        &mut segments,
        &input(
            4,
            Some("seg-transfer"),
            Observed::ReversalCompleted {
                evidence: id("ev-back"),
            },
        ),
    );
    segments.land_settled(&mut account);
    segments.land_settled(&mut account);
    let mut calls = account.calls.clone();
    calls.sort();
    assert_eq!(
        calls,
        [
            format!("land {HELD} COP {LINKED} {AMOUNT}"),
            format!("land {NEXT} COP {LINKED} {AMOUNT}"),
            format!("release {NEXT} COP {LINKED} {AMOUNT}"),
        ],
        "released where it left, landed again where it is back, each once"
    );
    let totals = segments.resting_totals();
    assert_eq!(
        totals.get(&(HELD.to_owned(), LINKED.to_owned())),
        Some(&Amount::new(AMOUNT).expect("an amount"))
    );
    assert!(!totals.contains_key(&(NEXT.to_owned(), LINKED.to_owned())));
}
