//! The parties an order involves, read from the log's history (`confirmer-evidence-party`, `T-020-40`).
//!
//! The statement a stuck resolution rests on (a decision's evidence, a report's, a confirmation's or a decline's)
//! must come from a party the order involves: a provider of a segment on any route the order was given, across its
//! recalculations, changed destinations and hand-ons; the holder of a holding account its value rested at (the
//! reserve of an arrival of the order, a stuck resolution's mover where it is one of these or its permitted decide step
//! records the node the register of holding accounts placed it at (`held_at`, `T-021-16`), a holding
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
    holder_of_the_order,
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

/// The `held_at` of the permitted decide step of `order` that `carries_out` names, where it records one.
fn held_at_of<'a>(
    order: &Id,
    carries_out: &rutent_contracts::core_internal::Decided,
    history: &'a Recorded,
) -> Option<&'a Id> {
    history
        .iter()
        .filter_map(|(_, e)| verified(e))
        .filter(|v| matches!(v.outcome, Outcome::Permitted {}))
        .find_map(|v| match &v.what {
            What::StuckDecide {
                request,
                decision,
                held_at,
            } if &request.order == order
                && request.request == carries_out.request
                && *decision == carries_out.decision =>
            {
                held_at.as_ref()
            }
            _ => None,
        })
}

/// Whether `statement` is a verified statement by a party `order` involves: an unverified one names only who it
/// claims made it, and rests on no key.
pub(super) fn verified_party(statement: &StatedEntry, order: &Id, history: &Recorded) -> bool {
    matches!(statement.attribution, Attribution::Verified { .. })
        && involves(statement, order, history)
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
            // A resolution's mover, and a permitted decide step's (a refused one names no one), only where it is a
            // holder the order involves by the log's other entries, or the decide step records where the register of
            // holding accounts placed it (`held_at`, `T-021-16`), as the log reads them.
            What::Effect {
                action:
                    Action::ResolveStuck {
                        order: o,
                        carries_out,
                        resolution: Resolution::Deliver { by, .. } | Resolution::HandOn { by, .. },
                        ..
                    },
                ..
            } => {
                ours(o)
                    && by == maker
                    && (holder_of_the_order(o, by, history)
                        || held_at_of(o, carries_out, history).is_some())
            }
            What::StuckDecide {
                request, held_at, ..
            } if matches!(v.outcome, Outcome::Permitted {}) => {
                ours(&request.order)
                    && matches!(&request.resolution,
                        Resolution::Deliver { by, .. } | Resolution::HandOn { by, .. } if by == maker
                            && (holder_of_the_order(&request.order, by, history) || held_at.is_some()))
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

    use rutent_contracts::core_internal::{DeploymentOperator, Through, Violation, admit_entry};
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

    const ROW: &str = "outcome-confirmation-cites-verified-evidence";

    /// The evidence a report, confirmation, decline or part-evidence entry cites, and the order it speaks of.
    fn cited(entry: &Entry) -> Option<(Id, Vec<Receipt>)> {
        let Entry::Verified(v) = entry else {
            return None;
        };
        match &v.what {
            What::ReportOutcome { request, .. } => {
                Some((request.order.clone(), vec![request.evidence.clone()]))
            }
            What::ConfirmOutcome { request, .. } => {
                Some((request.order.clone(), vec![request.confirmed_with.clone()]))
            }
            What::DeclineOutcome { request, .. } => {
                Some((request.order.clone(), vec![request.declined_with.clone()]))
            }
            What::PartEvidence {
                order, rests_on, ..
            } => Some((order.clone(), rests_on.clone())),
            _ => None,
        }
    }

    /// The log's verdict on `entry` after `history`, through either of its handles: admitted through one, or else the
    /// refusal naming the party rule where either handle gives it, and the first handle's otherwise.
    fn verdict(case: &Value, history: &[(Receipt, Entry)], entry: &Entry) -> Result<(), Violation> {
        let operator = match &case["operator"] {
            Value::Null => DeploymentOperator {
                maker: Id::new("operator").expect("an id"),
                party: Id::new("operator").expect("an id"),
            },
            named => DeploymentOperator {
                maker: Id::new(named["maker"].as_str().expect("a maker")).expect("an id"),
                party: Id::new(named["party"].as_str().expect("a party")).expect("an id"),
            },
        };
        let verdicts: Vec<Result<(), Violation>> = [Through::Policy, Through::General]
            .into_iter()
            .map(|through| admit_entry(history, entry, through, &operator))
            .collect();
        if verdicts.iter().any(Result::is_ok) {
            return Ok(());
        }
        let refusals: Vec<Violation> = verdicts.into_iter().filter_map(Result::err).collect();
        let party = refusals
            .iter()
            .find(|v| v.0.contains("a party its order involves"));
        Err(party.or(refusals.first()).expect("a refusal").clone())
    }

    /// Orchestration's reading of "a party the order involves" follows the log's, case by case, over every
    /// admission case the contract holds whose entry cites evidence a stuck order rests on (a report, a confirmation,
    /// a decline, a part's evidence), `world--*` and each row's alike: where the log admits the entry, the maker of
    /// each statement it cites is a party to its order; where it refuses it on the `party` field, one is not. A rule
    /// the contract widens or narrows turns this red, and so does a new case, with nobody editing it here.
    // verifies: REQ-AUZ-4
    #[test]
    fn orchestrations_reading_of_a_party_follows_the_logs_over_every_admission_case() {
        let dir = format!(
            "{}/../../contracts/core-internal/fixtures/admission",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut read = 0;
        let (mut admitted_cases, mut party_cases) = (0, 0);
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .expect("the fixtures")
            .map(|f| f.expect("a file").path())
            .collect();
        names.sort();
        for path in names {
            let case: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("a file"))
                    .expect("JSON");
            let Ok(entry) = serde_json::from_value::<Entry>(case["entry"].clone()) else {
                continue;
            };
            let Some((order, receipts)) = cited(&entry) else {
                continue;
            };
            let history = history_of(&case);
            let party = |receipt: &Receipt| {
                statement_at(receipt, &history)
                    .map(|statement| involves(statement, &order, &history))
            };
            let name = path.display();
            read += 1;
            match verdict(&case, &history, &entry) {
                Ok(()) => {
                    admitted_cases += 1;
                    for receipt in &receipts {
                        assert_ne!(party(receipt), Some(false), "{name}: admitted, so a party");
                    }
                }
                // Refused by the party rule, whatever the fixture says it breaks: some cited maker is no party.
                Err(refusal) if refusal.0.contains("a party its order involves") => {
                    party_cases += 1;
                    assert!(
                        receipts.iter().any(|r| party(r) == Some(false)),
                        "{name}: the log refuses it for its party, so a statement of no party"
                    );
                }
                Err(refusal) => assert!(
                    !case["breaks"].is_null(),
                    "{name}: a case that breaks nothing is refused: {}",
                    refusal.0
                ),
            }
        }
        // The walk reads the cases it means to: none of them silently skipped.
        assert!(read >= 60, "{read} cases read");
        assert!(admitted_cases >= 25, "{admitted_cases} admitted");
        assert!(party_cases >= 8, "{party_cases} refused for their party");
    }

    /// Whether the statement the case's entry cites is made by a party to its order.
    fn party_to_it(case: &Value, history: &[(Receipt, Entry)]) -> bool {
        let entry: Entry = serde_json::from_value(case["entry"].clone()).expect("an entry");
        let (order, receipts) = cited(&entry).expect("an entry citing evidence");
        let statement = statement_at(&receipts[0], history).expect("a statement of the history");
        involves(statement, &order, history)
    }

    /// A decision's evidence digests are held to a party whatever the statement's kind, a question's shape included,
    /// and only where the log verified the maker: an outside provider's judgement is refused as its result is, and
    /// a statement claiming a party's name with no verified signature is refused too.
    // verifies: REQ-AUZ-4
    #[test]
    fn decision_evidence_digests_are_held_to_a_verified_party_whatever_their_kind() {
        use crate::stuck::{ResolveError, digests_from_a_party};
        use rutent_contracts::provider_role::{JUDGEMENT_SHAPE, OPERATION_SHAPE};
        // The statement the case's entry cites, after `edit`, named as a decision's evidence.
        let probe = |case: &str, edit: &dyn Fn(&mut Value)| {
            let case = read(&format!("{ROW}--{case}"));
            let mut history = history_of(&case);
            let entry: Entry = serde_json::from_value(case["entry"].clone()).expect("an entry");
            let (order, receipts) = cited(&entry).expect("evidence");
            let at = history
                .iter()
                .position(|(r, _)| *r == receipts[0])
                .expect("the cited statement");
            let mut value = serde_json::to_value(&history[at].1).expect("JSON");
            edit(&mut value);
            history[at].1 = serde_json::from_value(value).expect("an entry");
            let Entry::Stated(s) = &history[at].1 else {
                panic!("a statement");
            };
            digests_from_a_party(std::slice::from_ref(&s.commitment), &order, &history)
        };
        let nothing = |_: &mut Value| {};
        let shape =
            |shape: &'static str| move |v: &mut Value| v["what"]["statement"] = shape.into();
        // An outside provider's result is refused; so is its judgement, which only the result's shape was held to.
        assert_eq!(probe("party", &nothing), Err(ResolveError::OutsideTheOrder));
        assert_eq!(
            probe("party", &shape(JUDGEMENT_SHAPE)),
            Err(ResolveError::OutsideTheOrder)
        );
        // A statement in the shape of Core's question is no exception: no log rule ties the shape to the operator, so
        // an outsider's verified statement in it is refused as any other.
        assert_eq!(
            probe("party", &shape(OPERATION_SHAPE)),
            Err(ResolveError::OutsideTheOrder)
        );
        // A party's statement passes where verified, and not where it only claims its maker.
        let route = "a-confirmation-on-another-provider-of-its-route";
        assert_eq!(probe(route, &nothing), Ok(()));
        assert_eq!(
            probe(route, &shape(JUDGEMENT_SHAPE)),
            Ok(()),
            "a party's judgement is a party's word"
        );
        let claimed = |v: &mut Value| {
            let who = v["attribution"]["who"].clone();
            v["attribution"] = serde_json::json!({"signature": "unverified", "claimed_by": who});
        };
        assert_eq!(probe(route, &claimed), Err(ResolveError::OutsideTheOrder));
    }

    /// One statement entered twice, with another attribution each time, is refused in either order of the history
    /// (`T-020-53`): every entry naming the digest is held to a verified party, not the first.
    // verifies: REQ-AUZ-4
    #[test]
    fn a_statement_entered_twice_with_another_attribution_is_refused_in_either_order() {
        use crate::stuck::{ResolveError, digests_from_a_party};
        let case = read(&format!(
            "{ROW}--a-confirmation-on-another-provider-of-its-route"
        ));
        let history = history_of(&case);
        let entry: Entry = serde_json::from_value(case["entry"].clone()).expect("an entry");
        let (order, receipts) = cited(&entry).expect("evidence");
        let at = history
            .iter()
            .position(|(r, _)| *r == receipts[0])
            .expect("the cited statement");
        let mut value = serde_json::to_value(&history[at].1).expect("JSON");
        let who = value["attribution"]["who"].clone();
        value["attribution"] = serde_json::json!({"signature": "unverified", "claimed_by": who});
        let claimed: Entry = serde_json::from_value(value).expect("an entry");
        let Entry::Stated(s) = &history[at].1 else {
            panic!("a statement");
        };
        let digest = s.commitment.clone();
        let (receipt, verified) = history[at].clone();
        assert_eq!(
            digests_from_a_party(std::slice::from_ref(&digest), &order, &history),
            Ok(())
        );
        for claimed_first in [false, true] {
            let mut twice = history.clone();
            let extra = (receipt.clone(), claimed.clone());
            if claimed_first {
                twice[at] = extra;
                twice.push((receipt.clone(), verified.clone()));
            } else {
                twice.push(extra);
            }
            assert_eq!(
                digests_from_a_party(std::slice::from_ref(&digest), &order, &twice),
                Err(ResolveError::OutsideTheOrder),
                "claimed first: {claimed_first}"
            );
        }
    }

    /// A decision's evidence fails closed (`T-020-53`): no digest at all, and a digest the history holds no
    /// statement under (alone or beside a party's), name no maker and are refused.
    // verifies: REQ-AUZ-5
    #[test]
    fn decision_evidence_that_names_no_logged_statement_fails_closed() {
        use crate::stuck::{ResolveError, digests_from_a_party};
        let case = read(&format!(
            "{ROW}--a-confirmation-on-another-provider-of-its-route"
        ));
        let history = history_of(&case);
        let entry: Entry = serde_json::from_value(case["entry"].clone()).expect("an entry");
        let (order, receipts) = cited(&entry).expect("evidence");
        let own = statement_at(&receipts[0], &history)
            .expect("a statement")
            .commitment
            .clone();
        let unlogged: rutent_contracts::module_interface::Digest = serde_json::from_value(
            serde_json::json!({"algorithm": "sha-256", "value": "ab".repeat(32)}),
        )
        .expect("a digest");
        assert_eq!(
            digests_from_a_party(std::slice::from_ref(&own), &order, &history),
            Ok(())
        );
        assert_eq!(
            digests_from_a_party(&[], &order, &history),
            Err(ResolveError::NoEvidence)
        );
        for digests in [
            vec![unlogged.clone()],
            vec![own.clone(), unlogged.clone()],
            vec![unlogged, own],
        ] {
            assert_eq!(
                digests_from_a_party(&digests, &order, &history),
                Err(ResolveError::NoEvidence),
                "{digests:?}"
            );
        }
    }

    /// A report's, confirmation's or decline's evidence is held to a verified statement too: one that only claims a
    /// party's name is refused, and another statement of the report's own party is the reporter's.
    // verifies: REQ-AUZ-4
    #[test]
    fn evidence_of_an_outcome_must_be_a_verified_statement_of_another_party_than_the_reporters() {
        use crate::stuck::{ResolveError, from_a_party};
        let case = read(&format!(
            "{ROW}--a-confirmation-on-another-provider-of-its-route"
        ));
        let mut history = history_of(&case);
        let entry: Entry = serde_json::from_value(case["entry"].clone()).expect("an entry");
        let (order, receipts) = cited(&entry).expect("evidence");
        assert_eq!(from_a_party(&receipts[0], &order, None, &history), Ok(()));
        // Against itself, as the report's own evidence, its party is the reporter's.
        assert_eq!(
            from_a_party(&receipts[0], &order, Some(&receipts[0]), &history),
            Err(ResolveError::ReportersOwnParty)
        );
        let at = history
            .iter()
            .position(|(r, _)| *r == receipts[0])
            .expect("the statement");
        let mut value = serde_json::to_value(&history[at].1).expect("JSON");
        let who = value["attribution"]["who"].clone();
        value["attribution"] = serde_json::json!({"signature": "unverified", "claimed_by": who});
        history[at].1 = serde_json::from_value(value).expect("an entry");
        assert_eq!(
            from_a_party(&receipts[0], &order, None, &history),
            Err(ResolveError::OutsideTheOrder)
        );
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
