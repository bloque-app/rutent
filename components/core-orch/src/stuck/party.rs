//! The parties an order involves, read from the log's history (`confirmer-evidence-party`, `T-020-40`).
//!
//! The statement a stuck resolution rests on (a decision's evidence, a report's, a confirmation's or a decline's)
//! must come from a party the order involves: a provider of a segment on any route the order was given, across its
//! recalculations, changed destinations and hand-ons; the holder of a holding account its value rested at (the
//! reserve of an arrival of the order, a stuck resolution's mover where policy permitted its decide step, a holding
//! withdrawal's holder and its publisher); or the publisher of a node it crossed, as the history binds the node
//! (a replay or evidence of the order resting on a statement stating for the publisher). Never the customer, whom no
//! statement is of, and never the reporter's own party, whose word a confirmation can't be.
//!
//! This is the reading `core-internal`'s rows `outcome-confirmation-cites-verified-evidence` and
//! `part-evidence-rests-on-statements` make of their `party` field, taken in orchestration before an entry is
//! built, so a statement from outside the order is refused here and the log is never asked. The log's rule stays
//! the guard: a history this reading and the log's read differently is the log's to refuse.

use rutent_contracts::core_internal::{
    Action, Attribution, Entry, Outcome, Receipt, Resolution, StatedEntry, VerifiedEntry, What,
};
use rutent_contracts::provider_role::{INNER_SEGMENTS_SHAPE, NodeRef};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::{Observed, RecordedInput, SegmentPlan};

/// The log's history as Core is handed it: every entry with the receipt it was given, in order, as the log holds it.
pub type Recorded = [(Receipt, Entry)];

fn verified(entry: &Entry) -> Option<&VerifiedEntry> {
    match entry {
        Entry::Verified(v) => Some(v),
        Entry::Stated(_) => None,
    }
}

fn stated(entry: &Entry) -> Option<&StatedEntry> {
    match entry {
        Entry::Stated(s) => Some(s),
        Entry::Verified(_) => None,
    }
}

fn input(entry: &VerifiedEntry) -> Option<&RecordedInput> {
    match &entry.what {
        What::Input { input } => Some(input),
        _ => None,
    }
}

/// The entry `receipt` names, where the history holds it.
pub(super) fn entry_at<'a>(receipt: &Receipt, history: &'a Recorded) -> Option<&'a Entry> {
    history.iter().find(|(r, _)| r == receipt).map(|(_, e)| e)
}

/// The statement `receipt` names, where the history holds one there.
pub(super) fn statement_at<'a>(
    receipt: &Receipt,
    history: &'a Recorded,
) -> Option<&'a StatedEntry> {
    entry_at(receipt, history).and_then(stated)
}

/// The orders `order` is joined to by hand-ons, itself first: the orders it handed value on to and the ones it was
/// handed on from, by the hand-on inputs, the stuck resolutions (their effects, and the decide steps policy
/// permitted), the holding accounts' records and the routes handed on, each way, and theirs in turn.
fn linked_orders<'a>(order: &'a str, history: &'a Recorded) -> Vec<&'a str> {
    let mut orders: Vec<&str> = vec![order];
    let pairs: Vec<(&str, &str)> = history
        .iter()
        .filter_map(|(_, e)| verified(e))
        .flat_map(|v| {
            let mut out: Vec<(&str, &str)> = Vec::new();
            if let Some(i) = input(v) {
                match &i.observed {
                    Observed::HandedOn { linked_order, .. } => {
                        out.push((i.subject.order.as_str(), linked_order.as_str()))
                    }
                    Observed::RouteChosen {
                        handed_on_from: Some(from),
                        ..
                    } => out.push((from.as_str(), i.subject.order.as_str())),
                    _ => {}
                }
            }
            match &v.what {
                What::HoldingWithdrawal {
                    order,
                    handed_on_from,
                    ..
                } => out.push((handed_on_from.as_str(), order.as_str())),
                What::Effect {
                    action:
                        Action::ResolveStuck {
                            order,
                            resolution: Resolution::HandOn { linked_order, .. },
                            ..
                        },
                    ..
                } => out.push((order.as_str(), linked_order.as_str())),
                // A decide step joins the orders only where policy permitted it: a refused one decided nothing.
                What::StuckDecide { request, .. } if matches!(v.outcome, Outcome::Permitted {}) => {
                    if let Resolution::HandOn { linked_order, .. } = &request.resolution {
                        out.push((request.order.as_str(), linked_order.as_str()));
                    }
                }
                _ => {}
            }
            out
        })
        .collect();
    let mut grew = true;
    while grew {
        grew = false;
        for (from, to) in &pairs {
            for (a, b) in [(from, to), (to, from)] {
                if orders.contains(a) && !orders.contains(b) {
                    orders.push(b);
                    grew = true;
                }
            }
        }
    }
    orders
}

/// Whether `statement` is made by a party `order` involves (see the module's docs).
pub(super) fn involves(statement: &StatedEntry, order: &Id, history: &Recorded) -> bool {
    let maker = &statement.attribution.maker().acts;
    let party = &statement.attribution.maker().party;
    let orders = linked_orders(order.as_str(), history);
    let ours = |o: &Id| orders.contains(&o.as_str());
    let mut nodes: Vec<&str> = Vec::new();
    let involved = history.iter().filter_map(|(_, e)| verified(e)).any(|v| {
        if let Some(i) = input(v).filter(|i| orders.contains(&i.subject.order.as_str())) {
            let plan: Vec<&SegmentPlan> = match &i.observed {
                Observed::RouteChosen { parts, .. } => {
                    parts.iter().flat_map(|p| p.segments.iter()).collect()
                }
                Observed::RouteRecalculated { segments, .. }
                | Observed::DestinationChanged { segments, .. } => segments.iter().collect(),
                _ => Vec::new(),
            };
            for s in &plan {
                for end in [&s.from.node, &s.to.node] {
                    if let NodeRef::Inner(node) = end {
                        nodes.push(node.as_str());
                    }
                }
            }
            if plan.iter().any(|s| s.provider.as_str() == maker.as_str()) {
                return true;
            }
        }
        match &v.what {
            What::Effect {
                action: Action::MoveValue {
                    order: o, provider, ..
                },
                ..
            } => ours(o) && provider == maker,
            What::Effect {
                action:
                    Action::ResolveStuck {
                        order: o,
                        resolution: Resolution::Deliver { by, .. } | Resolution::HandOn { by, .. },
                        ..
                    },
                ..
            } => ours(o) && by == maker,
            // A permitted decide step's mover; a refused one names no one.
            What::StuckDecide { request, .. } if matches!(v.outcome, Outcome::Permitted {}) => {
                ours(&request.order)
                    && matches!(&request.resolution,
                        Resolution::Deliver { by, .. } | Resolution::HandOn { by, .. } if by == maker)
            }
            What::Arrival {
                reserve,
                order: Some(o),
                ..
            } => ours(o) && reserve == maker,
            What::HoldingWithdrawal {
                order: o,
                handed_on_from,
                holder,
                publisher,
                ..
            } => {
                (ours(o) || ours(handed_on_from))
                    && (holder == maker || publisher.as_ref() == Some(party))
            }
            _ => false,
        }
    });
    involved
        || nodes.iter().any(|node| {
            orders
                .iter()
                .any(|o| private_node_of(o, node, party, history))
        })
}

/// Whether the history binds `node` as a private node of `order` published by `publisher`: a `replay` of the order
/// naming the node, resting on a verified statement about the order stating for the publisher, or evidence of the
/// order whose public movement names the node, resting on one.
fn private_node_of(order: &str, node: &str, publisher: &Id, history: &Recorded) -> bool {
    let for_publisher = |r: &Receipt| {
        statement_at(r, history).is_some_and(|s| {
            matches!(s.attribution, Attribution::Verified { .. })
                && &s.attribution.maker().party == publisher
                && s.about
                    .as_ref()
                    .and_then(|a| a.order.as_ref())
                    .is_some_and(|o| o.as_str() == order)
                && (s.what.statement.as_str() != INNER_SEGMENTS_SHAPE
                    || s.about
                        .as_ref()
                        .and_then(|a| a.node.as_ref())
                        .is_some_and(|n| n.as_str() == node))
        })
    };
    history
        .iter()
        .any(|(_, e)| match verified(e).map(|v| &v.what) {
            Some(What::Replay {
                order: o,
                nodes,
                rests_on,
                ..
            }) => {
                o.as_str() == order
                    && nodes.iter().any(|n| n.as_str() == node)
                    && rests_on.first().is_some_and(for_publisher)
            }
            Some(What::Evidence {
                order: o,
                movement: Some(movement),
                rests_on,
                ..
            }) => {
                o.as_str() == order
                    && movement.node.as_ref().is_some_and(|n| n.as_str() == node)
                    && rests_on.iter().any(for_publisher)
            }
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    //! Orchestration's reading of "a party the order involves" agrees with the log's, case by case: each of the
    //! contract's admission cases for the `party` field of `outcome-confirmation-cites-verified-evidence` is read
    //! here, and the maker of the evidence it cites is a party to its order exactly where the log admits it.

    use serde_json::Value;

    use super::*;

    fn read(name: &str) -> Value {
        let path = format!(
            "{}/../../contracts/core-internal/fixtures/admission/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect("JSON")
    }

    fn history_of(case: &Value) -> Vec<(Receipt, Entry)> {
        case["history"]
            .as_array()
            .expect("a history")
            .iter()
            .map(|item| {
                (
                    serde_json::from_value(item["receipt"].clone()).expect("a receipt"),
                    serde_json::from_value(item["entry"].clone()).expect("an entry"),
                )
            })
            .collect()
    }

    /// Whether the maker of the evidence the case's entry cites is a party to the order it speaks of.
    fn party_to_it(case: &Value, history: &[(Receipt, Entry)]) -> bool {
        let request = &case["entry"]["what"]["request"];
        let cited = ["confirmed_with", "declined_with", "evidence"]
            .iter()
            .find_map(|field| request.get(field).filter(|v| !v.is_null()))
            .expect("an entry citing evidence");
        let receipt: Receipt = serde_json::from_value(cited.clone()).expect("a receipt");
        let order: Id = serde_json::from_value(request["order"].clone()).expect("an order");
        let statement = statement_at(&receipt, history).expect("a statement of the history");
        involves(statement, &order, history)
    }

    const ROW: &str = "outcome-confirmation-cites-verified-evidence";

    // verifies: REQ-AUZ-4
    #[test]
    fn the_makers_the_log_refuses_are_no_party_to_the_order() {
        for name in [
            "party",
            "a-move-values-report-on-an-outside-providers-statement",
            "a-decline-on-an-outside-providers-statement",
            "a-confirmation-on-a-provider-a-holding-check-names",
            "a-confirmation-on-the-mover-a-refused-decide-names",
            "a-confirmation-on-a-provider-of-an-order-no-hand-on-joins",
            "a-confirmation-on-a-provider-of-an-order-a-refused-decide-hands-on-to",
        ] {
            let case = read(&format!("{ROW}--{name}"));
            assert!(!party_to_it(&case, &history_of(&case)), "{name}");
        }
    }

    // verifies: REQ-AUZ-4, REQ-ORD-1
    #[test]
    fn the_makers_the_log_admits_are_each_a_party_to_the_order() {
        for name in [
            "a-confirmation-on-the-mover-a-permitted-decide-names",
            "a-confirmation-on-the-mover-its-resolve-effect-names",
            "a-confirmation-on-the-reserve-its-arrival-came-to",
            "a-confirmation-on-the-holder-a-holding-withdrawal-names",
            "a-confirmation-on-the-publisher-a-holding-withdrawal-names",
            "a-confirmation-on-a-provider-of-an-order-a-hand-on-input-joins",
            "a-confirmation-on-a-provider-of-a-route-handed-on-from-the-order",
            "a-confirmation-on-a-provider-of-an-order-a-holding-withdrawal-joins",
            "a-confirmation-on-a-provider-of-an-order-a-resolve-effect-hands-on-to",
            "a-confirmation-on-a-provider-of-an-order-a-permitted-decide-hands-on-to",
            "a-confirmation-on-another-provider-of-its-route",
            "a-confirmation-on-a-provider-only-a-recalculated-route-names",
            "a-hand-ons-confirmation-on-another-makers-statement",
        ] {
            let case = read(&format!("{ROW}--{name}"));
            assert!(party_to_it(&case, &history_of(&case)), "{name}");
        }
        for name in ["settled_es", "report_es", "decline"] {
            let case = read(&format!("world--{name}"));
            assert!(party_to_it(&case, &history_of(&case)), "{name}");
        }
    }

    /// A statement stating for the publisher of a node the order crossed counts only where the history binds the
    /// node to that publisher, by a replay of the order resting on the publisher's own statement.
    // verifies: REQ-AUZ-4
    #[test]
    fn the_publisher_of_a_node_the_order_crossed_is_a_party_only_where_the_history_binds_it() {
        let case = read(&format!("{ROW}--party"));
        let mut history = history_of(&case);
        let acting = |acts: &str| {
            serde_json::json!({
                "signature": "verified",
                "who": {"party": "wallet-co", "acts": acts, "role": "provider", "kind": "component"},
            })
        };
        let at = history
            .iter()
            .position(|(r, _)| r.index.get() == 175)
            .expect("the confirmation's statement");
        let mut evidence = serde_json::to_value(&history[at].1).expect("JSON");
        evidence["attribution"] = acting("wallet-desk");
        history[at].1 = serde_json::from_value(evidence).expect("an entry");
        assert!(
            !party_to_it(&case, &history),
            "no node is bound to wallet-co"
        );

        let replay = read("world--replay");
        let mut exit = replay["history"]
            .as_array()
            .expect("a history")
            .iter()
            .find(|item| item["receipt"]["index"] == 143)
            .expect("the exit's result")["entry"]
            .clone();
        exit["attribution"] = acting("wallet-ops");
        let receipt = |n: u64| -> Receipt {
            serde_json::from_value(serde_json::json!({
                "log": "log-main", "index": n,
                "entry": {"algorithm": "sha-256", "value": format!("{n:064x}")},
            }))
            .expect("a receipt")
        };
        let mut replayed = replay["entry"].clone();
        replayed["what"]["rests_on"] = serde_json::json!([receipt(451)]);
        history.push((
            receipt(451),
            serde_json::from_value(exit).expect("an entry"),
        ));
        history.push((
            receipt(452),
            serde_json::from_value(replayed).expect("an entry"),
        ));
        assert!(party_to_it(&case, &history), "a replay binds the node");
    }
}
