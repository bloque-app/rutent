//! `INV-1` to `INV-10` and `NFR-5` over the assembled Core (`T-024-08`), each by a test over seeded runs with
//! `T-024-02`'s faults planted and `T-024-06`'s failure steps taken (`invariants_support`).
//!
//! **Every count is made from the log as it reads back from the disk**, and from the statements the evidence store
//! kept for the commitments that log holds, and from the providers' own books where an invariant is about what a
//! provider did; never from what a driver did or from Core's own records. A failing check names the run by its seed
//! and kind: a seeded run is replayed with `RUTENT_SCENARIO_SEEDS=<seed>`, a pinned one by its plan (`PINNED`).
//!
//! **A test that has nothing to look at fails.** Each states what the runs must have reached (a failed segment, a
//! repeated delivery, a rewrite...) and fails if they reached none, so a green test never means an empty one.

mod execution_support;
mod invariants_support;

use rutent_scenarios::on_tick::OnTick;
use std::collections::BTreeMap;

use invariants_support::*;
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::OperationKind;
use rutent_contracts::state_machine::{Observed, SegmentState};
use serde_json::Value;

/// Collects what each run says is wrong, named by the run.
fn violations(check: impl FnMut(&Sim) -> Vec<String>) -> Vec<String> {
    violations_in(every, check)
}

/// [`violations`], over the runs `which` takes.
fn violations_in(
    which: fn(&Sim) -> bool,
    mut check: impl FnMut(&Sim) -> Vec<String>,
) -> Vec<String> {
    let mut all = Vec::new();
    for sim in runs().iter().filter(|sim| which(sim)) {
        let name = sim.name();
        all.extend(check(sim).into_iter().map(|v| format!("{name}: {v}")));
    }
    all
}

/// Every run.
fn every(_: &Sim) -> bool {
    true
}

/// The runs Core and its platform were restarted in, mid-order, and went on from (`T-024-21`).
fn restarted(sim: &Sim) -> bool {
    sim.run.restarts > 0 && !sim.run.crashed
}

fn none(found: &[String]) {
    assert!(
        found.is_empty(),
        "{} violation(s); replay a seeded run with RUTENT_SCENARIO_SEEDS=<seed>, a pinned one from `PINNED`:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// The body of the statement the evidence store keeps behind `commitment`, as a stated entry names it: its
/// `statement`, without the envelope a later answer is made and signed in. `None` where none is kept.
fn body_of(run: &rutent_scenarios::harness::Run, commitment: &Value) -> Option<Value> {
    let value = commitment["value"].as_str()?;
    let kept = run
        .kept
        .iter()
        .find(|k| k.commitment.value.as_str() == value)?;
    let bytes = rutent_core_audit::log::kept::from_base64url(kept.bytes.as_str())?;
    let statement: Value = serde_json::from_slice(&bytes).ok()?;
    Some(statement["statement"].clone())
}

/// The fees the log's `fee` entries record for each order's segments paid to a provider.
fn logged_fees(sim: &Sim) -> BTreeMap<String, u128> {
    let mut fees = BTreeMap::new();
    for e in entries(&sim.run) {
        if e["kind"] == "verified" && e["what"]["call"] == "fee" && e["what"]["to"] == "provider" {
            *fees
                .entry(e["what"]["order"].as_str().unwrap_or_default().to_owned())
                .or_insert(0) += n(&e["what"]["amount"]);
        }
    }
    fees
}

/// The net each private node holds of each asset by `flows`: `(node, asset)` to a signed figure. A withdrawal puts
/// what it gave out into its node; a conversion takes in one asset and gives out another there; a deposit and a
/// transfer take what they were given in out of the node they start at.
fn flow(net: &mut BTreeMap<(String, String), i128>, op: &Value, amount_in: u128, amount_out: u128) {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    match op["kind"].as_str() {
        Some("withdrawal") => {
            *net.entry((text(&op["destination"]), text(&op["asset"])))
                .or_default() += amount_out as i128;
        }
        Some("conversion") => {
            *net.entry((text(&op["node"]), text(&op["from"])))
                .or_default() -= amount_in as i128;
            *net.entry((text(&op["node"]), text(&op["to"]))).or_default() += amount_out as i128;
        }
        Some("deposit" | "transfer") => {
            *net.entry((text(&op["origin"]), text(&op["asset"])))
                .or_default() -= amount_in as i128;
        }
        _ => {}
    }
}

// verifies: INV-1
#[test]
fn value_in_equals_value_delivered_plus_fees_remainder_and_value_held_across_every_run() {
    let (orders_balanced, nodes_compared, in_flight) = value_balances(every);
    assert!(
        orders_balanced >= 10,
        "only {orders_balanced} orders reached a settled segment"
    );
    assert!(
        nodes_compared >= 5,
        "only {nodes_compared} node balances compared"
    );
    assert!(
        in_flight >= 1,
        "no run left value in motion with its outcome unsettled"
    );
}

/// `INV-1` over the runs Core and its platform were restarted in mid-order (`T-024-21`): what each order took in is
/// what it delivered, its fees and what rests at its last node, counted from the log the restarted Core went on
/// writing, its statements and the providers' books, which the restart left as they were; and, `NFR-5`, no provider
/// acted on anything the log holds no record of, before the restart or after it.
// verifies: INV-1, NFR-5
#[test]
fn value_in_equals_value_delivered_plus_fees_remainder_and_value_held_across_restarted_runs() {
    let (orders_balanced, nodes_compared, _) = value_balances(restarted);
    let found = violations_in(restarted, |sim| {
        let mut bad = sim.run.effects_without_record();
        // Each order its run's kind says completes delivered its value: its last settled segment is a deposit.
        let by = by_order(segments(&sim.run));
        for order in &sim.run.orders {
            let o = order.as_str();
            if sim.expected_end(o) != "complete" {
                continue;
            }
            let delivered = by
                .get(o)
                .and_then(|segs| segs.iter().rev().find(|s| s.settled()))
                .is_some_and(|last| last.kind() == "deposit" && last.amount_out() > 0);
            if !delivered {
                bad.push(format!(
                    "{o}: to complete, and its value was never delivered"
                ));
            }
        }
        bad
    });
    none(&found);
    let runs = runs().iter().filter(|sim| restarted(sim)).count();
    assert!(runs >= 5, "only {runs} restarted runs");
    assert!(
        orders_balanced >= 4,
        "only {orders_balanced} orders of restarted runs reached a settled segment"
    );
    assert!(
        nodes_compared >= 3,
        "only {nodes_compared} node balances of restarted runs compared"
    );
}

/// The `INV-1` counts over the runs `which` takes, each violation failing the test: how many orders balanced, node
/// balances were compared and transactions were in motion.
fn value_balances(which: fn(&Sim) -> bool) -> (usize, usize, usize) {
    let mut orders_balanced = 0;
    let mut nodes_compared = 0;
    let mut in_flight = 0;
    let found = violations_in(which, |sim| {
        let mut bad = Vec::new();
        let book = book_segments(sim);
        let fees_logged = logged_fees(sim);
        let all = segments(&sim.run);
        for (order, segs) in by_order(all.clone()) {
            let settled: Vec<&Seg> = segs.iter().filter(|s| s.settled()).collect();
            let Some(first) = settled.first() else {
                continue;
            };
            for pair in settled.windows(2) {
                if pair[1].amount_in() != pair[0].amount_out() {
                    bad.push(format!(
                        "{order}: {} took in {} but {} had given out {}",
                        pair[1].provider,
                        pair[1].amount_in(),
                        pair[0].provider,
                        pair[0].amount_out()
                    ));
                }
            }
            for s in &settled {
                if s.kind() == "conversion" && s.res.as_ref().unwrap()["outcome"]["rate"] != "1" {
                    bad.push(format!(
                        "{order}: the simulated exchange's rate is not 1: the check can't hold"
                    ));
                }
                if s.amount_in() != s.amount_out() + s.fee() {
                    bad.push(format!(
                        "{order}: {} took in {}, gave out {} and charged {}",
                        s.provider,
                        s.amount_in(),
                        s.amount_out(),
                        s.fee()
                    ));
                }
                // What the provider's own book says it did is what the log's statement says.
                let tx = s.transaction().expect("a transaction");
                match book.iter().find(|(_, _, seg)| seg.transaction == tx) {
                    Some((_, _, seg))
                        if n(&Value::String(seg.amount_in.clone())) == s.amount_in()
                            && n(&Value::String(seg.amount_out.clone())) == s.amount_out()
                            && n(&Value::String(seg.fee.clone())) == s.fee() => {}
                    other => bad.push(format!("{order}: {tx} is in the provider's book as {other:?}, not as the log's statement")),
                }
            }
            let value_in = first.amount_in();
            let last = settled.last().expect("one");
            let delivered = if last.kind() == "deposit" {
                last.amount_out()
            } else {
                0
            };
            let held = if delivered == 0 { last.amount_out() } else { 0 };
            let fees: u128 = settled.iter().map(|s| s.fee()).sum();
            if value_in != delivered + fees + held {
                bad.push(format!(
                    "{order}: value in {value_in}, delivered {delivered}, fees {fees}, held {held}"
                ));
            }
            if fees_logged.get(&order).copied().unwrap_or(0) != fees {
                bad.push(format!(
                    "{order}: the log's fee entries add to {:?}, the statements' fees to {fees}",
                    fees_logged.get(&order)
                ));
            }
            orders_balanced += 1;
        }

        // Across orders: what the log evidences resting at each private node, against what its publisher's own
        // book says it holds, the difference being exactly the value in motion: taken by a provider whose outcome the
        // log never settled.
        let private: Vec<String> = specs_of(sim.plan.shape)
            .iter()
            .flat_map(|s| s.private_nodes())
            .collect();
        let mut evidenced = BTreeMap::new();
        for s in all.iter().filter(|s| s.settled()) {
            flow(
                &mut evidenced,
                s.op.as_ref().unwrap(),
                s.amount_in(),
                s.amount_out(),
            );
        }
        let mut booked = BTreeMap::new();
        let mut moving = BTreeMap::new();
        for (_, op, seg) in &book {
            let (a_in, a_out) = (
                n(&Value::String(seg.amount_in.clone())),
                n(&Value::String(seg.amount_out.clone())),
            );
            flow(&mut booked, op, a_in, a_out);
            let tx = op["transaction"].as_str().unwrap_or_default();
            if !all
                .iter()
                .any(|s| s.settled() && s.transaction().as_deref() == Some(tx))
            {
                flow(&mut moving, op, a_in, a_out);
                in_flight += 1;
                // The log never settled it: its segment is one the log shows unresolved.
                let seg = all.iter().find(|s| s.transaction().as_deref() == Some(tx));
                if seg.is_some_and(|s| s.settled()) {
                    bad.push(format!("{tx} is both settled and in motion"));
                }
            }
        }
        for node in &private {
            for ((at, asset), figure) in &booked {
                if at != node {
                    continue;
                }
                let e = evidenced
                    .get(&(at.clone(), asset.clone()))
                    .copied()
                    .unwrap_or(0);
                let m = moving
                    .get(&(at.clone(), asset.clone()))
                    .copied()
                    .unwrap_or(0);
                nodes_compared += 1;
                if *figure != e + m {
                    bad.push(format!(
                        "{node} {asset}: the publisher's book holds {figure}, the log evidences {e} and {m} is in motion"
                    ));
                }
            }
        }
        bad
    });
    none(&found);
    (orders_balanced, nodes_compared, in_flight)
}

// verifies: INV-2
#[test]
fn every_failure_path_leaves_the_value_at_its_origin_in_full() {
    let mut failures = 0;
    let found = violations(|sim| {
        let mut bad = Vec::new();
        let log = entries(&sim.run);
        let book = book_segments(sim);
        for (order, segs) in by_order(segments(&sim.run)) {
            for (i, s) in segs.iter().enumerate() {
                let failed = s.result.as_deref() == Some("failed") || s.status() == Some("failed");
                if !failed {
                    continue;
                }
                failures += 1;
                // The value is at the node the segment starts from: what the last settled segment before it gave
                // out is what the segment was handed, in full.
                if let Some(held) = segs[..i].iter().rev().find(|p| p.settled()) {
                    let handed = n(&s.op.as_ref().expect("its operation")["amount"]);
                    if handed != held.amount_out() {
                        bad.push(format!(
                            "{order}: the failed {} was handed {handed} of the {} resting at its origin",
                            s.provider,
                            held.amount_out()
                        ));
                    }
                }
                // It moved nothing on the provider's own book.
                let tx = s.transaction().expect("a transaction");
                if book.iter().any(|(_, _, seg)| seg.transaction == tx) {
                    bad.push(format!(
                        "{order}: the failed {tx} moved value in its provider's book"
                    ));
                }
                // Nothing was moved on from it until a recalculation, and then from where the value is.
                let recalculated = |from: usize, to: usize| {
                    log[from..to].iter().any(|e| {
                        e["kind"] == "verified"
                            && e["what"]["call"] == "input"
                            && e["what"]["input"]["observed"]["kind"] == "route_recalculated"
                            && e["what"]["input"]["subject"]["order"] == order.as_str()
                    })
                };
                for later in &segs[i + 1..] {
                    if !recalculated(s.effect, later.effect) {
                        bad.push(format!("{order}: {} was handed value after the failed {} with no recalculation", later.provider, s.provider));
                    }
                }
                // Freed only on another party's confirmation: a reversal counted complete, or a route recalculated
                // from a failed middle segment, rests on evidence of a party that is not the failing provider.
                let completed = log.iter().any(|e| {
                    e["kind"] == "verified"
                        && e["what"]["call"] == "input"
                        && e["what"]["input"]["observed"]["kind"] == "reversal_completed"
                        && e["what"]["input"]["subject"]["segment"] == s.segment.as_str()
                });
                let confirmed = log.iter().any(|e| {
                    e["kind"] == "verified"
                        && e["what"]["call"] == "evidence"
                        && e["what"]["segment"] == s.segment.as_str()
                        && e["what"]["whose"] != s.provider.as_str()
                });
                if completed && !confirmed {
                    bad.push(format!(
                        "{order}: the failed {} was counted reversed with no other party's word",
                        s.provider
                    ));
                }
                let freed_mid_route = s.kind() != "deposit" && recalculated(s.effect, log.len());
                if freed_mid_route && !confirmed {
                    bad.push(format!(
                        "{order}: the route was recalculated from the failed {} on its own word",
                        s.provider
                    ));
                }
            }
        }
        bad
    });
    none(&found);
    assert!(
        failures >= 3,
        "only {failures} failed segments across the runs"
    );
}

// verifies: INV-3
#[test]
fn every_segment_ends_settled_reversed_in_full_or_its_order_stuck() {
    let (complete, stuck, open) = segment_ends(every);
    assert!(
        complete >= 5 && stuck >= 1 && open >= 1,
        "ends reached: {complete} complete, {stuck} stuck, {open} open"
    );
}

/// `INV-3` over the runs Core and its platform were restarted in mid-order (`T-024-21`): a restarted run is no crashed
/// one read as it stands, so its end-state checks run, on the state machine's journal the restarted host holds and the
/// log the restarted Core went on writing.
// verifies: INV-3
#[test]
fn every_segment_of_a_restarted_run_ends_settled_reversed_in_full_or_its_order_stuck() {
    let (complete, stuck, _) = segment_ends(restarted);
    // And the end is the one the run's kind says: stuck only where `stuck_until_t020_61` or a stuck order says so, so an
    // unexpected stuck order is red.
    let found = violations_in(restarted, |sim| {
        let states = sim.run.state_names();
        sim.run
            .orders
            .iter()
            .filter_map(|order| {
                let o = order.as_str();
                let want = sim.expected_end(o);
                let ended = states.get(o).cloned().flatten();
                (ended.as_deref() != Some(want))
                    .then(|| format!("{o}: ended {ended:?}, not {want}"))
            })
            .collect()
    });
    none(&found);
    assert!(
        complete >= 3 && stuck >= 1,
        "ends reached by restarted runs: {complete} complete, {stuck} stuck"
    );
}

/// The `INV-3` checks over the runs `which` takes, each violation failing the test: how many parts ended complete,
/// stuck and open.
fn segment_ends(which: fn(&Sim) -> bool) -> (usize, usize, usize) {
    let (mut complete, mut stuck, mut open) = (0, 0, 0);
    let found = violations_in(which, |sim| {
        let mut bad = Vec::new();
        let states = sim.run.replayed_states();
        for (order, state) in &states {
            let Some(order_state) = state else { continue };
            for part in &order_state.parts {
                let st: Vec<SegmentState> = part.segments.iter().map(|s| s.state).collect();
                let done = |s: &SegmentState| {
                    matches!(s, SegmentState::Settled | SegmentState::ReversalComplete)
                };
                use rutent_contracts::state_machine::OrderState as O;
                if sim.run.crashed {
                    // Where Core stopped, no end was reached: no segment says it settled that the log doesn't.
                    continue;
                }
                match part.state {
                    O::Complete => {
                        complete += 1;
                        if !st.iter().all(|s| *s == SegmentState::Settled) {
                            bad.push(format!("{order}: complete with segments {st:?}"));
                        }
                    }
                    O::Stuck => {
                        stuck += 1;
                        if st.iter().all(done) {
                            bad.push(format!("{order}: stuck with every segment ended: {st:?}"));
                        }
                    }
                    O::EndedWithoutCompleting | O::Failed => {
                        if st.iter().any(|s| !done(s) && *s != SegmentState::Pending) {
                            bad.push(format!("{order}: ended with value unresolved: {st:?}"));
                        }
                    }
                    O::HandedOn => {}
                    O::NotPastFirstSegment | O::FirstSegmentSettled | O::WaitingForCustomer => {
                        open += 1;
                        // A segment whose outcome is unknown past its time is the order stuck, never an open order.
                        if st.contains(&SegmentState::OutcomeUnknown)
                            || st.contains(&SegmentState::Started)
                        {
                            bad.push(format!(
                                "{order}: open with a segment unsettled in flight: {st:?}"
                            ));
                        }
                        if st.iter().all(done) && part.retired_segments.is_empty() {
                            bad.push(format!("{order}: open with every segment ended: {st:?}"));
                        }
                    }
                }
            }
        }
        // What the log says of a segment is the state the machine reached for it, and never two ends.
        for s in segments(&sim.run) {
            let id = s.segment.as_str();
            let state = states
                .values()
                .flatten()
                .flat_map(|o| &o.parts)
                .find_map(|p| {
                    p.segments
                        .iter()
                        .find(|x| x.segment_id.as_str() == id)
                        .map(|x| x.state)
                });
            if s.settled() && state.is_some_and(|x| x != SegmentState::Settled) {
                bad.push(format!(
                    "{id}: settled in the log, {state:?} in the machine"
                ));
            }
            if s.result.as_deref() == Some("failed") && state == Some(SegmentState::Settled) {
                bad.push(format!("{id}: failed in the log, settled in the machine"));
            }
        }
        bad
    });
    none(&found);
    (complete, stuck, open)
}

// verifies: INV-4
#[test]
fn silence_and_late_delivery_never_move_one_value_from_two_places() {
    let (mut moves_checked, mut late, mut contradictions) = (0, 0, 0);
    let found = violations(|sim| {
        let mut bad = Vec::new();
        let all = segments(&sim.run);
        let tx = |s: &Seg| s.transaction().unwrap_or_default();
        // Value is moved out of a node the moment a segment is handed it, and back only by a failure; a segment never
        // takes more from a node than the order has resting there, and an unsettled one keeps its value out.
        let mut events: Vec<(usize, bool, &Seg)> = Vec::new();
        for s in &all {
            events.push((s.effect, true, s));
            if let Some(at) = s.result_at {
                events.push((at, false, s));
            }
        }
        events.sort_by_key(|(at, ..)| *at);
        let mut held: BTreeMap<(String, String, String), i128> = BTreeMap::new();
        for (_, handing, s) in events {
            let Some(op) = &s.op else { continue };
            let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
            let (from, asset) = match op["kind"].as_str() {
                Some("withdrawal") => (None, text(&op["asset"])),
                Some("conversion") => (Some(text(&op["node"])), text(&op["from"])),
                _ => (Some(text(&op["origin"])), text(&op["asset"])),
            };
            if handing {
                moves_checked += 1;
                if let Some(node) = &from {
                    let key = (s.order.clone(), node.clone(), asset.clone());
                    let amount = n(&op["amount"]) as i128;
                    let rests = held.get(&key).copied().unwrap_or(0);
                    if rests < amount {
                        bad.push(format!(
                            "{}: {} was handed {amount} of {asset} from {node}, where {rests} rests",
                            s.order, s.provider
                        ));
                    }
                    *held.entry(key).or_default() -= amount;
                }
            } else if s.settled() {
                let to = match op["kind"].as_str() {
                    Some("withdrawal") => Some((text(&op["destination"]), text(&op["asset"]))),
                    Some("conversion") => Some((text(&op["node"]), text(&op["to"]))),
                    Some("transfer") => Some((text(&op["destination"]), text(&op["asset"]))),
                    _ => None,
                };
                if let Some((node, asset)) = to {
                    *held.entry((s.order.clone(), node, asset)).or_default() +=
                        s.amount_out() as i128;
                }
            } else if let Some(node) = &from {
                // A failure puts the value back where it was.
                *held
                    .entry((s.order.clone(), node.clone(), asset))
                    .or_default() += n(&op["amount"]) as i128;
            }
        }
        // One move for a transaction, from one place: each transaction is handed over once, and moved at most once.
        let mut seen = BTreeMap::new();
        for s in &all {
            *seen.entry(tx(s)).or_insert(0) += 1;
        }
        bad.extend(
            seen.iter()
                .filter(|(_, c)| **c > 1)
                .map(|(t, c)| format!("{t} was handed over {c} times")),
        );
        bad.extend(
            sim.run
                .movements()
                .iter()
                .filter(|(_, c)| **c > 1)
                .map(|(t, c)| format!("{t} moved {c} times")),
        );
        // A late answer is kept once: no more result statements for a segment than it was ever given. A later answer
        // whose body differs from the one kept is a contradiction, kept as a further statement that moves nothing
        // (`T-024-21`): told from a second result by its body, and held to having moved nothing, the log ending the
        // segment's effect once and no book moving its transaction twice. The count is never dropped.
        let log = entries(&sim.run);
        for s in &all {
            let stated: Vec<&Value> = log
                .iter()
                .filter(|e| {
                    e["kind"] == "stated"
                        && e["about"]["segment"] == s.segment.as_str()
                        && e["what"]["statement"]
                            .as_str()
                            .is_some_and(|x| x.ends_with("result"))
                })
                .collect();
            let bodies: Vec<Option<Value>> = stated
                .iter()
                .map(|e| body_of(&sim.run, &e["commitment"]))
                .collect();
            let mut distinct: Vec<&Option<Value>> = Vec::new();
            for body in &bodies {
                if !distinct.contains(&body) {
                    distinct.push(body);
                }
            }
            if bodies.len() > distinct.len() || distinct.iter().any(|b| b.is_none()) {
                bad.push(format!(
                    "{}: {} result statements, {} of them a body kept before",
                    s.segment,
                    bodies.len(),
                    bodies.len() - distinct.len()
                ));
            }
            if distinct.len() > 1 {
                contradictions += 1;
                let ended = log
                    .iter()
                    .filter(|e| {
                        e["kind"] == "verified"
                            && e["what"]["call"] == "result"
                            && e["what"]["effect"]["index"].as_u64() == Some(s.effect as u64)
                    })
                    .count();
                let moved = s
                    .transaction()
                    .and_then(|t| sim.run.movements().get(&t).copied())
                    .unwrap_or(0);
                if ended > 1 || moved > 1 {
                    bad.push(format!(
                        "{}: a contradicting answer moved something: {ended} results, moved {moved} times",
                        s.segment
                    ));
                }
            }
            if s.result.as_deref() == Some("unanswered") {
                late += 1;
            }
        }
        bad
    });
    none(&found);
    assert!(moves_checked >= 20, "{moves_checked} moves checked");
    // The silent deposit's provider states something else later (`invariants_support::contradicting`), so the
    // contradiction branch is reached.
    assert!(
        contradictions >= 1,
        "no run kept a contradicting late answer"
    );
    assert!(
        late >= 1,
        "no run left a segment whose effect the log ended unanswered, whose late delivery could be tried"
    );
}

// verifies: INV-5, GUA-1
#[test]
fn no_value_rests_outside_a_reserve_or_a_holding_account() {
    let (mut routes, mut rests) = (0, 0);
    let found = violations(|sim| {
        let mut bad = Vec::new();
        // The nodes value may rest at: a reserve's inner node.
        let reserves: Vec<String> = specs_of(sim.plan.shape)
            .iter()
            .filter(|s| s.role == rutent_conformance::sim::Role::Reserve)
            .map(|s| s.ledger())
            .collect();
        let end_ok = |segments: &[rutent_contracts::state_machine::SegmentPlan],
                      first_outer: bool,
                      what: &str,
                      bad: &mut Vec<String>| {
            let last = segments.last().expect("a segment");
            if last.kind != OperationKind::Deposit || !matches!(last.to.node, NodeRef::Outer(_)) {
                bad.push(format!(
                    "{what}: a route that doesn't end in a deposit at an outer node"
                ));
            }
            if first_outer
                && (segments[0].kind != OperationKind::Withdrawal
                    || !matches!(segments[0].from.node, NodeRef::Outer(_)))
            {
                bad.push(format!(
                    "{what}: a route that doesn't start at a reserve's outer node"
                ));
            }
            for pair in segments.windows(2) {
                if let NodeRef::Inner(node) = &pair[0].to.node {
                    rests_here(node, &reserves, what, bad);
                }
            }
            if let NodeRef::Inner(node) = &segments[0].from.node {
                rests_here(node, &reserves, what, bad);
            }
        };
        fn rests_here(node: &str, reserves: &[String], what: &str, bad: &mut Vec<String>) {
            if !reserves.iter().any(|r| r == node) {
                bad.push(format!(
                    "{what}: value rests at {node}, a node of no reserve"
                ));
            }
        }
        for input in &sim.run.inputs {
            match &input.observed {
                Observed::RouteChosen { parts, .. } => {
                    for part in parts {
                        routes += 1;
                        end_ok(&part.segments, true, input.subject.order.as_str(), &mut bad);
                    }
                }
                Observed::RouteRecalculated { segments, .. } => {
                    routes += 1;
                    end_ok(segments, false, input.subject.order.as_str(), &mut bad);
                }
                _ => {}
            }
        }
        // And rutent's own position stays empty: no operation moves value to, from or for rutent, and no fee is its.
        let core = rutent_scenarios::kit::core();
        let (party, acts) = (
            core.party.as_str().to_owned(),
            core.acts.as_str().to_owned(),
        );
        for st in &sim.run.statements {
            let text = st["statement"].to_string();
            if st["shape"]
                .as_str()
                .is_some_and(|x| x.ends_with("operation"))
                && (text.contains(&format!("\"{party}\"")) || text.contains(&format!("\"{acts}\"")))
            {
                bad.push(format!("an operation names rutent: {text}"));
            }
            rests += 1;
        }
        for e in entries(&sim.run) {
            if e["kind"] == "verified"
                && e["what"]["call"] == "fee"
                && e["what"]["party"] == party.as_str()
            {
                bad.push("a fee is attributed to rutent".to_owned());
            }
        }
        for (provider, book) in &sim.run.books {
            if book
                .iter()
                .any(|b| b.to_string().contains(&format!("\"{party}\"")))
            {
                bad.push(format!("{provider}'s book names rutent"));
            }
        }
        bad
    });
    none(&found);
    assert!(
        routes >= 15 && rests >= 100,
        "{routes} routes and {rests} statements read"
    );
}

// verifies: INV-6, GUA-3
#[test]
fn history_rewritten_with_operator_access_is_detected() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use ed25519_dalek::SigningKey;
    use rutent_contracts::provider_role::PublicKey;
    use rutent_contracts::router_api::{Id, Timestamp};
    use rutent_core_audit::checkpoint::{Checker, Checkpointer, Finding, Schedule};
    use rutent_core_audit::log::kept::base64url;
    use rutent_core_audit::log::{AuditLog, MemoryStore, record};
    use rutent_scenarios::assembly::{LOG_NAME, settings};
    use rutent_scenarios::kit::{START, SharedClock};

    /// The statements the evidence store kept for the log, as the log reads them to open.
    struct Shelf(Vec<rutent_contracts::core_internal::KeptStatement>);
    impl rutent_contracts::core_internal::calls::KeptStatements for Shelf {
        fn kept(
            &mut self,
            commitment: &rutent_contracts::module_interface::Digest,
        ) -> Result<
            Option<rutent_contracts::core_internal::KeptStatement>,
            rutent_contracts::core_internal::calls::AppendFailure,
        > {
            Ok(self.0.iter().find(|k| &k.commitment == commitment).cloned())
        }
    }

    let (mut rewrites, mut removals, mut inserted) = (0, 0, 0);
    let found = violations(|sim| {
        let mut bad = Vec::new();
        // The history as the log's disk holds it, which is what an operator with full access can change.
        let records: Vec<Vec<u8>> = sim
            .run
            .log
            .iter()
            .map(|e| record(e).expect("an entry in its canonical form"))
            .collect();
        // The checkpoints the log issued for it, as a checker outside the operator received each when it was published.
        let clock = SharedClock::at(START + 4 * 86_400);
        let mut store = MemoryStore::new();
        for r in &records {
            rutent_core_audit::log::Store::append(&mut store, r).expect("a record");
        }
        let log = match AuditLog::open(settings(&clock), store, Shelf(sim.run.kept.clone())) {
            Ok(log) => Rc::new(RefCell::new(log)),
            Err(e) => return vec![format!("the history doesn't open as a log: {e:?}")],
        };
        let schedule =
            Schedule::new(&Timestamp::new("2026-11-05T00:00:00Z").unwrap(), 86_400).unwrap();
        let key = SigningKey::from_bytes(&[9u8; 32]);
        let public = PublicKey::new(base64url(key.verifying_key().as_bytes())).unwrap();
        let mut checkpointer =
            Checkpointer::new(Rc::clone(&log), MemoryStore::new(), key, schedule)
                .expect("a checkpointer");
        let ran = checkpointer.run_due();
        checkpointer.acknowledge(&ran).expect("published");
        let held = ran.published.clone();
        let openings: Vec<Vec<u8>> = checkpointer
            .kept()
            .iter()
            .map(|k| serde_json::to_vec(&k.opening).unwrap())
            .collect();
        if held.len() < 3 {
            bad.push(format!("{} checkpoints", held.len()));
        }
        let checker = Checker::new(public, Id::new(LOG_NAME).unwrap(), schedule);
        let honest = checker.check(&records, &held, &openings, ran.through);
        if !honest.is_clean() {
            bad.push(format!(
                "the untouched history fails: {:?}",
                honest.findings
            ));
        }
        // A record rewritten: its time moved, the entry otherwise whole and canonical, as full access can make it.
        let at = (sim.plan.seed as usize * 7 + 3) % records.len();
        let mut value: Value = serde_json::from_slice(&records[at]).unwrap();
        value["when"]["at"] = Value::String("2026-12-31T00:00:00Z".to_owned());
        let rewritten_entry: rutent_contracts::core_internal::Entry =
            serde_json::from_value(value).unwrap();
        let mut rewritten = records.clone();
        rewritten[at] = record(&rewritten_entry).unwrap();
        let report = checker.check(&rewritten, &held, &openings, ran.through);
        rewrites += 1;
        if !report
            .findings
            .iter()
            .any(|f| matches!(f, Finding::DoesNotHold { .. }))
        {
            bad.push(format!(
                "record {at} rewritten and not detected: {:?}",
                report.findings
            ));
        }
        // A record removed.
        let mut removed = records.clone();
        removed.remove(at);
        let report = checker.check(&removed, &held, &openings, ran.through);
        removals += 1;
        if report.is_clean() {
            bad.push(format!("record {at} removed and not detected"));
        }
        // A record inserted in the middle.
        let mut grown = records.clone();
        grown.insert(at, records[(at + 1) % records.len()].clone());
        let report = checker.check(&grown, &held, &openings, ran.through);
        inserted += 1;
        if report.is_clean() {
            bad.push(format!("a record inserted at {at} and not detected"));
        }
        bad
    });
    none(&found);
    assert!(
        rewrites >= 12 && removals >= 12 && inserted >= 12,
        "{rewrites} {removals} {inserted}"
    );
}

// verifies: INV-7, NFR-5, GUA-3
#[test]
fn no_action_takes_effect_without_its_record_under_injected_faults() {
    let (mut actions, mut crashed, mut effects) = (0, 0, 0);
    let found = violations(|sim| {
        let mut bad = Vec::new();
        if sim.run.crashed {
            crashed += 1;
        }
        let log = entries(&sim.run);
        let segs = segments(&sim.run);
        // Every action a provider took, from its own book, has its effect in the log, recorded before the provider's
        // answer to it (so before it could have taken effect).
        for (provider, book) in &sim.run.books {
            for entry in book {
                let op = &entry["operation"]["statement"];
                let moves = matches!(
                    op["kind"].as_str(),
                    Some("withdrawal" | "deposit" | "transfer" | "conversion" | "reversal")
                );
                if !moves || op["dry_run"] == true {
                    continue;
                }
                actions += 1;
                let tx = op["transaction"].as_str().unwrap_or_default();
                let Some(seg) = segs.iter().find(|s| s.transaction().as_deref() == Some(tx)) else {
                    bad.push(format!("{provider} took {tx} with no effect in the log"));
                    continue;
                };
                let answered = log.iter().position(|e| {
                    e["kind"] == "stated"
                        && e["about"]["transaction"] == tx
                        && e["what"]["statement"]
                            .as_str()
                            .is_some_and(|x| x.ends_with("result"))
                });
                if answered.is_some_and(|at| at < seg.effect) {
                    bad.push(format!(
                        "{provider}'s answer to {tx} is in the log before the effect"
                    ));
                }
            }
        }
        // Every effect names a permit the log holds, recorded before it.
        for (at, e) in log.iter().enumerate() {
            if e["kind"] == "verified" && e["what"]["call"] == "effect" {
                effects += 1;
                let permit = &e["what"]["decision"];
                let permitted = log[..at].iter().any(|p| {
                    p["kind"] == "verified"
                        && p["outcome"]["status"] == "permitted"
                        && (p["what"]["decision"] == permit["decision"]
                            || p["what"]["request"]["request"] == permit["request"])
                });
                if !permitted {
                    bad.push(format!(
                        "the effect at {at} names no permit recorded before it: {permit}"
                    ));
                }
            }
        }
        bad
    });
    none(&found);
    assert!(
        actions >= 20 && effects >= 30,
        "{actions} actions, {effects} effects"
    );
    assert!(
        crashed >= 3,
        "only {crashed} runs crashed between a record and its effect"
    );
}

// verifies: INV-8
#[test]
fn repeated_transaction_identifiers_move_nothing_twice() {
    let (mut twice, mut reruns) = (0, 0);
    let found = violations(|sim| {
        twice += sim.run.delivered_twice;
        if sim.plan.kind == Kind::Rerun {
            reruns += 1;
        }
        moved_once(sim)
    });
    none(&found);
    assert!(
        twice >= 2,
        "{twice} operations delivered twice across the runs"
    );
    assert!(reruns >= 1, "no run ran its route again");
}

/// `INV-8` across restarts of Core and the platform (`T-024-21`): Core restarted draws its transactions afresh and
/// reads the ones it asked under from the log, and the platform restarted reads what it carried out under each from
/// its record, so an operation stated before the restart is never stated again after it, none moves twice in a book,
/// and each segment is handed over once; and segments whose effect the log took before a restart and whose result
/// after it are among them.
// verifies: INV-8
#[test]
fn repeated_transaction_identifiers_move_nothing_twice_across_restarts_of_core_and_the_platform() {
    let mut across = 0;
    let found = violations_in(restarted, |sim| {
        let mut bad = moved_once(sim);
        let log = entries(&sim.run);
        let restarts = confirmations(&log);
        if restarts.is_empty() {
            bad.push(
                "restarted, and the log holds no confirmation of the platform's releases"
                    .to_owned(),
            );
        }
        for s in segments(&sim.run) {
            if let Some(at) = s.result_at
                && restarts.iter().any(|(r, _)| s.effect < *r && *r < at)
            {
                across += 1;
            }
        }
        bad
    });
    none(&found);
    assert!(
        across >= 2,
        "only {across} segments were handed over before a restart and ended after it"
    );
}

/// Where the log holds each confirmation of a provider's releases a restarted platform held, and whose: the first
/// thing a restarted Core does (`T-024-21`).
fn confirmations(log: &[Value]) -> Vec<(usize, String)> {
    log.iter()
        .enumerate()
        .filter(|(_, e)| {
            e["kind"] == "verified"
                && e["what"]["call"] == "effect"
                && e["what"]["action"]["kind"] == "change_catalogue"
                && e["what"]["action"]["change"] == "confirm_releases"
        })
        .map(|(at, e)| {
            (
                at,
                e["what"]["action"]["provider"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

/// The `INV-8` checks over one run.
fn moved_once(sim: &Sim) -> Vec<String> {
    {
        let mut bad = Vec::new();
        // A transaction identifier is one operation's: no two operations the operator stated share one.
        let mut ids = BTreeMap::new();
        for st in &sim.run.statements {
            if st["shape"]
                .as_str()
                .is_some_and(|x| x.ends_with("operation"))
                && st["statement"]["dry_run"] == false
            {
                *ids.entry(
                    st["statement"]["transaction"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
                .or_insert(0) += 1;
            }
        }
        bad.extend(
            ids.iter()
                .filter(|(_, c)| **c > 1)
                .map(|(t, c)| format!("{t} stated {c} times")),
        );
        // However often it reached the provider, it moved once, and was handed over once.
        bad.extend(
            sim.run
                .movements()
                .iter()
                .filter(|(_, c)| **c > 1)
                .map(|(t, c)| format!("{t} moved {c} times in a provider's book")),
        );
        let mut handed = BTreeMap::new();
        for s in segments(&sim.run) {
            *handed.entry(s.segment.clone()).or_insert(0) += 1;
        }
        bad.extend(
            handed
                .iter()
                .filter(|(_, c)| **c > 1)
                .map(|(s, c)| format!("{s} handed over {c} times")),
        );
        // What moved is what the log handed over: each book movement is of a transaction the log's effects name.
        let named: Vec<String> = segments(&sim.run)
            .iter()
            .filter_map(Seg::transaction)
            .collect();
        bad.extend(
            sim.run
                .movements()
                .keys()
                .filter(|t| !named.contains(t))
                .map(|t| format!("{t} moved with no effect")),
        );
        bad
    }
}

// verifies: INV-9, GUA-4
#[test]
fn no_balance_answer_reveals_more_than_sufficiency_across_all_reserves() {
    use rutent_prov::catalogue::Verdict;
    use rutent_scenarios::assembly::machine::Reference;
    use rutent_scenarios::harness::Scenario;
    use rutent_scenarios::kit::{self, Seeded};

    // Every reserve's opening balance is 50 000 000 in the simulated stand-in; the checks never say so.
    const BALANCE: u128 = 50_000_000;
    let (mut answers, mut yes, mut no) = (0, 0, 0);
    for seed in rutent_scenarios::harness::seeds(&DEFAULT_SEEDS) {
        let names = [kit::RESERVE, IN];
        let scenario = Scenario::new("balance answers", "AC-15", execution_support::given(&names));
        let (mut core, _) = scenario.core(seed, &mut Reference);
        let session = core.open_session(&kit::customer()).expect("a session");
        core.set_check_bound(1_000);
        let mut draw = Seeded::new(seed ^ 0xba1a_0c35);
        let mut asked: Vec<u128> = (0..8)
            .map(|_| 1 + u128::from(draw.below(120_000_000)))
            .collect();
        asked.extend([BALANCE - 1, BALANCE, BALANCE + 1]);
        asked.sort_unstable();
        asked.dedup();
        let before = core.log().entries().len();
        let mut by_reserve: BTreeMap<&str, Vec<(u128, Verdict)>> = BTreeMap::new();
        let mut dry: BTreeMap<&str, Vec<(u128, Verdict)>> = BTreeMap::new();
        for reserve in names {
            for amount in &asked {
                let text = amount.to_string();
                let by_check = core
                    .check_balance(reserve, &session, &text)
                    .expect("the check is sent");
                let by_dry_run = core
                    .check_by_dry_run(reserve, &session, &text)
                    .expect("the dry run is sent");
                // The whole answer: yes or no, the same by either way, and only whether the funds suffice.
                let expected = if *amount <= BALANCE {
                    Verdict::Yes
                } else {
                    Verdict::No
                };
                assert_eq!(
                    by_check, expected,
                    "seed {seed}, {reserve}, {amount}: replay with RUTENT_SCENARIO_SEEDS={seed}"
                );
                // A dry run may also be refused, by class alone and with no detail: nothing more than yes or no.
                assert!(
                    matches!(
                        format!("{by_dry_run:?}").as_str(),
                        "Yes" | "No" | "Refused(Final(NoDetail))"
                    ),
                    "the dry run says more: {by_dry_run:?}"
                );
                dry.entry(reserve).or_default().push((*amount, by_dry_run));
                assert!(
                    matches!(format!("{by_check:?}").as_str(), "Yes" | "No"),
                    "the answer says more: {by_check:?}"
                );
                answers += 2;
                if by_check == Verdict::Yes {
                    yes += 1
                } else {
                    no += 1
                }
                by_reserve
                    .entry(reserve)
                    .or_default()
                    .push((*amount, by_check));
            }
        }
        // Monotone in the amount: the answers say where the balance is only by yes turning to no, and the two
        // reserves' answers are the same function of the amount.
        for list in by_reserve.values() {
            assert!(
                list.windows(2)
                    .all(|w| !(w[0].1 == Verdict::No && w[1].1 == Verdict::Yes))
            );
        }
        assert_eq!(
            by_reserve[names[0]], by_reserve[names[1]],
            "the reserves answer alike"
        );
        for list in dry.values() {
            assert!(
                list.windows(2)
                    .all(|w| !(w[0].1 != Verdict::Yes && w[1].1 == Verdict::Yes)),
                "{list:?}"
            );
        }
        // (A dry run is held to the tuple's band as well as the funds, so two reserves' dry runs needn't be alike.)
        // What Core kept of the checks, in the catalogue's check log and in the audit log, holds no figure the asker
        // didn't state: neither the balance nor what it leaves.
        let mut forbidden: Vec<String> = vec![BALANCE.to_string()];
        forbidden.extend(
            asked
                .iter()
                .filter(|a| **a < BALANCE)
                .map(|a| (BALANCE - a).to_string()),
        );
        forbidden.retain(|f| !asked.iter().any(|a| a.to_string() == *f));
        let account = rutent_scenarios::assembly::context().account;
        let logged = core.catalogue().check_log(&account);
        assert!(
            logged.len() >= 2 * asked.len() * names.len(),
            "seed {seed}: {} checks logged",
            logged.len()
        );
        let kept = format!("{logged:?}");
        let audit = core.log().entries()[before..]
            .iter()
            .map(|(_, e)| serde_json::to_string(e).unwrap())
            .collect::<String>();
        for text in [&kept, &audit] {
            for token in text
                .split(|c: char| !c.is_ascii_digit())
                .filter(|t| t.len() >= 5)
            {
                assert!(
                    !forbidden.iter().any(|f| f == token),
                    "seed {seed}: a check's record holds {token}, which no asker stated"
                );
            }
        }
    }
    assert!(yes >= 10 && no >= 10, "{yes} yes, {no} no");
    assert!(answers >= 80);
}

// verifies: INV-10
#[test]
fn no_recalculation_changes_destination_or_to_asset() {
    let (mut recalculated, mut compared) = (0, 0);
    let found = violations(|sim| {
        let mut bad = Vec::new();
        // The end of each route the log chose or recalculated, and of each route an effect carried: one destination.
        let end_of = |segments: &[rutent_contracts::state_machine::SegmentPlan]| {
            let last = segments.last().expect("a segment");
            let node = match &last.to.node {
                NodeRef::Outer(o) => o.network.clone(),
                NodeRef::Inner(n) => n.clone(),
            };
            (
                last.provider.as_str().to_owned(),
                node,
                last.to.asset.id.clone(),
            )
        };
        type End = (String, String, String);
        let mut ends: BTreeMap<String, Vec<(End, &str)>> = BTreeMap::new();
        for input in &sim.run.inputs {
            let order = input.subject.order.as_str().to_owned();
            match &input.observed {
                Observed::RouteChosen { parts, .. } => {
                    for part in parts {
                        ends.entry(order.clone())
                            .or_default()
                            .push((end_of(&part.segments), "chosen"));
                    }
                }
                Observed::RouteRecalculated { segments, .. } => {
                    recalculated += 1;
                    ends.entry(order)
                        .or_default()
                        .push((end_of(segments), "recalculated"));
                }
                _ => {}
            }
        }
        for e in entries(&sim.run) {
            if e["kind"] == "verified"
                && e["what"]["call"] == "effect"
                && e["what"]["action"]["kind"] == "move_value"
            {
                let route = &e["what"]["action"]["route"];
                let end = (
                    route["destination"]["reserve"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    route["destination"]["node"]["network"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    route["to_asset"]["id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                );
                ends.entry(
                    e["what"]["action"]["order"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
                .or_default()
                .push((end, "effect"));
            }
        }
        for (order, list) in &ends {
            let first = &list[0].0;
            for (end, what) in list {
                compared += 1;
                if end != first {
                    bad.push(format!(
                        "{order}: a {what} route ends at {end:?}, the first at {first:?}"
                    ));
                }
            }
        }
        bad
    });
    none(&found);
    assert!(
        recalculated >= 2,
        "only {recalculated} recalculations across the runs"
    );
    assert!(compared >= 60, "{compared} route ends compared");
}

/// `NFR-6` over the runs Core and its platform were restarted in mid-order (`T-024-21`): every order the customer had
/// accepted before the restart is one the restarted Core knows, opened in its log, followed by it for its customer, held
/// by the state machine's host restarted on its journal as the log's own inputs say; each one the crash caught is
/// carried on by a step of its own after the restart, and each ends as its run's kind says (`Sim::expected_end`).
// verifies: NFR-6
#[test]
fn core_and_the_platform_restarted_mid_order_lose_no_order() {
    let (mut kept, mut started_before, mut completed_across) = (0, 0, 0);
    let found = violations_in(restarted, |sim| {
        let mut bad = Vec::new();
        let log = entries(&sim.run);
        let restart = confirmations(&log).first().map_or(log.len(), |(at, _)| *at);
        let replayed = sim.run.replayed_states();
        for order in &sim.run.orders {
            let o = order.as_str();
            kept += 1;
            let opened = log.iter().any(|e| {
                e["kind"] == "verified"
                    && e["what"]["call"] == "order_opened"
                    && e["what"]["order"] == o
            });
            if !opened {
                bad.push(format!("{o}: accepted, and no order_opened in the log"));
            }
            if !sim.run.hosted.iter().any(|h| h == o) {
                bad.push(format!("{o}: the restarted host doesn't hold it"));
            }
            if !sim.run.followed.contains_key(o) {
                bad.push(format!(
                    "{o}: the restarted Core doesn't follow it for its customer"
                ));
            }
            if sim.run.states.get(o) != replayed.get(o) {
                bad.push(format!(
                    "{o}: the restarted host holds {:?}, the log's inputs {:?}",
                    sim.run.states.get(o).map(|s| s.as_ref().map(|s| s.state)),
                    replayed.get(o).map(|s| s.as_ref().map(|s| s.state))
                ));
            }
            // Started before the restart: its route was carried on after it, by an effect or a result the restarted
            // Core wrote.
            let chosen = log.iter().position(|e| {
                e["kind"] == "verified"
                    && e["what"]["call"] == "input"
                    && e["what"]["input"]["observed"]["kind"] == "route_chosen"
                    && e["what"]["input"]["subject"]["order"] == o
            });
            if chosen.is_some_and(|at| at < restart) {
                started_before += 1;
            }
            // An order the crash caught mid-run has a step of its own after the restart's last confirmation, not the
            // confirmation's result: the restarted Core carried it on. One that ended before the restart has nothing
            // left to carry on, and one stuck when the run restarted is the operator's.
            if carried_by_the_restart(sim, o) {
                let after = last_confirmation(&log).unwrap_or(restart);
                if !carried_on_after(&log, o, after) {
                    bad.push(format!(
                        "{o}: caught by the crash, and nothing of it after the restart"
                    ));
                }
            }
            // Each order ends where its run's kind says: a caught order the restarted Core can't yet carry on
            // (`stuck_until_t020_61`) or one stuck when the run restarted is stuck, and every other completes.
            let want = sim.expected_end(o);
            let ended = sim.run.state_names().get(o).cloned().flatten();
            if ended.as_deref() != Some(want) {
                bad.push(format!("{o}: ended {ended:?}, not {want}"));
            }
            if want == "complete" && chosen.is_some_and(|at| at < restart) {
                completed_across += 1;
            }
        }
        bad
    });
    none(&found);
    assert!(kept >= 6, "only {kept} orders of restarted runs read");
    assert!(
        started_before >= 4,
        "only {started_before} orders were started before a restart"
    );
    assert!(
        completed_across >= 2,
        "only {completed_across} orders started before a restart completed after it"
    );
}

/// Whether the log holds a step of `order`'s own after `at`, written by the restarted Core: an effect or an input of
/// the order, or the result of an effect of the order; never another order's, nor the platform's confirmation.
fn carried_on_after(log: &[Value], order: &str, at: usize) -> bool {
    let of_order = |e: &Value| {
        e["kind"] == "verified"
            && (e["what"]["action"]["order"] == order
                || e["what"]["input"]["subject"]["order"] == order)
    };
    log.iter().skip(at + 1).any(|e| {
        of_order(e)
            || (e["kind"] == "verified"
                && e["what"]["call"] == "result"
                && e["what"]["effect"]["index"]
                    .as_u64()
                    .and_then(|i| log.get(usize::try_from(i).ok()?))
                    .is_some_and(of_order))
    })
}

/// Whether `order` is one the restarted Core must carry on: a crash caught it mid-run. An order stuck when a
/// `RestartWhileStuck` run restarted is the operator's, and stays as it was.
fn carried_by_the_restart(sim: &Sim, order: &str) -> bool {
    sim.plan.kind != Kind::RestartWhileStuck && sim.caught.iter().any(|o| o == order)
}

/// Where the log holds the last of the confirmations a restart made, or `None` where it holds none.
fn last_confirmation(log: &[Value]) -> Option<usize> {
    confirmations(log).last().map(|(at, _)| *at)
}

/// The platform restarted mid-run, its record not ending whole, holds every provider's releases until the operator
/// confirms them (`NFR-6`, `T-013-11`, `T-024-21`). A Core and platform restarted with the releases left held: the
/// platform says the reserve's are held, and an operation handed to it reaches no module; the operator's login
/// confirms through Core's decided path, policy's permit, the effect and its result `done` in the log; the same kind
/// of operation is then answered, and the order the crash stopped is carried on to its end. And in every restarted
/// run, each restart's confirmation of each provider of the run is in the log, decided and done.
// verifies: NFR-6
#[test]
fn platform_restarted_mid_run_holds_its_releases_until_the_operator_confirms() {
    use rutent_contracts::provider_role::Statement;
    use rutent_prov::module::types::{Handled, PlatformRefusal};
    use rutent_scenarios::assembly::machine::Reference;
    use rutent_scenarios::assembly::{Staged, context};
    use rutent_scenarios::harness::{Fault, Given, Scenario, Step, plant};
    use rutent_scenarios::kit::{self, AMOUNT, RESERVE};

    let probe = |transaction: &str| -> Statement<rutent_contracts::provider_role::Operation> {
        rutent_conformance::sim::operation(
            &context(),
            serde_json::json!({
                "kind": "balance_verification",
                "transaction": transaction,
                "provider": RESERVE,
                "session": context().session,
                "account": context().account,
                "asset": "COP",
                "amount": "1",
            }),
        )
    };
    for seed in [1, 7] {
        let scenario = Scenario::new("held releases", "AC-28", Given::one_reserve(Staged::AtOnce))
            .when(Step::Order {
                request: "req-held".to_owned(),
                amount: AMOUNT.to_owned(),
            });
        let (mut core, orders) = scenario.core(seed, &mut Reference);
        let order = orders[0].clone();
        // Right after the log took the reserve's answer to the withdrawal, which moved: before Core acted on it.
        plant(
            &mut core,
            &Fault::CrashAfterRecord {
                call: rutent_contracts::provider_role::RESULT_SHAPE,
                nth: 0,
            },
        );
        let crashed =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| core.ran_on_tick(&order)));
        assert!(crashed.is_err(), "seed {seed}: the crash stopped Core");
        let mut core = scenario.restart_holding(core, seed, &mut Reference);
        assert!(
            core.fleet()
                .platform
                .awaiting_confirmation(RESERVE)
                .is_some(),
            "seed {seed}: the restarted platform holds the reserve's releases"
        );
        let book = core.world().journal(RESERVE).len();
        let held = core.fleet_mut().handle(RESERVE, &probe("probe-held"));
        // The hold's own refusal: unavailable, nothing passed out, while the platform says the reserve's releases wait
        // for the operator and runs no process of it (the platform gives the hold no class of its own).
        assert!(
            matches!(held, Handled::Refused(PlatformRefusal::Unavailable))
                && core
                    .fleet()
                    .platform
                    .awaiting_confirmation(RESERVE)
                    .is_some()
                && core.fleet().platform.start_of(RESERVE).is_none(),
            "seed {seed}: an operation handed to a held provider is refused as held: {held:?}"
        );
        assert_eq!(
            core.world().journal(RESERVE).len(),
            book,
            "seed {seed}: nothing reached the reserve's module"
        );
        let before: Vec<Value> = core
            .log()
            .entries()
            .iter()
            .map(|(_, e)| serde_json::to_value(e).expect("JSON"))
            .collect();
        assert!(
            confirmations(&before).is_empty(),
            "seed {seed}: nothing confirmed yet"
        );
        let confirmed = core.confirm_releases().expect("the operator confirms");
        assert_eq!(confirmed, [RESERVE.to_owned()], "seed {seed}");
        assert!(
            core.fleet()
                .platform
                .awaiting_confirmation(RESERVE)
                .is_none()
        );
        let after: Vec<Value> = core
            .log()
            .entries()
            .iter()
            .map(|(_, e)| serde_json::to_value(e).expect("JSON"))
            .collect();
        let decided = confirmation_decided(&after);
        assert_eq!(
            decided,
            [RESERVE.to_owned()],
            "seed {seed}: decided and done in the log"
        );
        let answered = core.fleet_mut().handle(RESERVE, &probe("probe-confirmed"));
        assert!(
            matches!(answered, Handled::Answered(_)),
            "seed {seed}: answered once confirmed: {answered:?}"
        );
        core.ran_on_tick(&order)
            .expect("the stopped order carries on");
        core.flush()
            .expect("the state machine takes what the log did");
        let state = core.status(&kit::customer(), &order).expect("followed");
        assert_eq!(
            serde_json::to_value(state.state).expect("JSON"),
            "complete",
            "seed {seed}: the stopped order completes"
        );
    }
    // In every restarted run: each restart confirmed each provider of the run, decided and done, and each order the
    // crash caught had nothing of its own carried out between the restart and the confirmations, and went on after them.
    let found = violations_in(restarted, |sim| {
        let log = entries(&sim.run);
        let mut bad = Vec::new();
        if let (Some(first), Some(last)) = (
            confirmations(&log).first().map(|(at, _)| *at),
            last_confirmation(&log),
        ) {
            for order in sim.caught.iter().filter(|o| carried_by_the_restart(sim, o)) {
                let between = log[first..last].iter().any(|e| {
                    e["kind"] == "verified"
                        && e["what"]["call"] == "effect"
                        && e["what"]["action"]["order"] == order.as_str()
                });
                if between {
                    bad.push(format!(
                        "{order}: an effect of it between the restart and its confirmations"
                    ));
                }
                if !carried_on_after(&log, order, last) {
                    bad.push(format!(
                        "{order}: caught by the crash, and nothing of it after the confirmations"
                    ));
                }
            }
        }
        let mut want: Vec<String> = specs_of(sim.plan.shape)
            .into_iter()
            .map(|s| s.provider)
            .collect();
        want.sort();
        let mut decided = confirmation_decided(&log);
        decided.sort();
        let mut expected = Vec::new();
        for _ in 0..sim.run.restarts {
            expected.extend(want.iter().cloned());
        }
        expected.sort();
        if decided != expected {
            bad.push(format!(
                "{} restart(s), confirmations decided and done for {decided:?}, not {want:?} each",
                sim.run.restarts
            ));
        }
        bad
    });
    none(&found);
}

/// The providers whose releases the log's confirmations confirmed, each with policy's permit before its effect and its
/// result `done` after: one per confirmation decided and done.
fn confirmation_decided(log: &[Value]) -> Vec<String> {
    confirmations(log)
        .into_iter()
        .filter(|(at, _)| {
            let effect = &log[*at];
            let permit = &effect["what"]["decision"];
            let permitted = log[..*at].iter().any(|p| {
                p["kind"] == "verified"
                    && p["outcome"]["status"] == "permitted"
                    && (p["what"]["decision"] == permit["decision"]
                        || p["what"]["request"]["request"] == permit["request"])
            });
            let done = log[*at..].iter().any(|r| {
                r["kind"] == "verified"
                    && r["what"]["call"] == "result"
                    && r["what"]["effect"]["index"].as_u64() == Some(*at as u64)
                    && r["outcome"]["status"] == "done"
            });
            permitted && done
        })
        .map(|(_, provider)| provider)
        .collect()
}

/// A run longer than a session's life opens another, under an id of its own, and an order asked in it is quoted: the
/// simulated reserve answers its dry run in every session the holder's register issued, as the deployment tells it
/// (`T-024-21`, the review's L2). The first session ended, the open one isn't reused, and the second has another id.
// verifies: REQ-CAT-3
#[test]
fn order_in_a_second_session_of_a_long_run_is_quoted() {
    use rutent_scenarios::assembly::machine::Reference;
    use rutent_scenarios::assembly::{Core, Staged};
    use rutent_scenarios::harness::{Given, Scenario, base_request};
    use rutent_scenarios::kit::{self, AMOUNT};

    let scenario = Scenario::new(
        "a second session",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    );
    let (mut core, _) = scenario.core(1, &mut Reference);
    let customer = kit::customer();
    let quoted = |core: &mut Core, request: &str, session: &rutent_contracts::router_api::Id| {
        let told = core
            .request(base_request(request, AMOUNT, session))
            .expect("the request opens an order");
        let order = Core::order_of(&told).expect("an order");
        let told = core.quote(&customer, &order).expect("asked for a quote");
        matches!(
            told,
            rutent_core_orch::intake::Told::Exists {
                status: rutent_core_orch::intake::Status::Quoted { .. },
                ..
            }
        )
    };
    let first = core.open_session(&customer).expect("a session");
    assert!(
        quoted(&mut core, "req-first-session", &first),
        "quoted in the first session"
    );
    assert_eq!(
        core.open_session(&customer).expect("the open session"),
        first,
        "an open session is the one answered"
    );
    // Past the first session's end (at most 15 minutes).
    core.clock.advance(20 * 60);
    let second = core.open_session(&customer).expect("a second session");
    assert_ne!(second, first, "a session of its own");
    assert!(
        quoted(&mut core, "req-second-session", &second),
        "quoted in the second session"
    );
}

/// The one point a restarted Core could move value twice (`T-020-61`, item 9): the process crashes after the platform
/// carried a segment's operation out, its provider having moved, and before Core logged the answer. The restarted Core
/// carries the segment on (its status asked, the answer the provider's own result), and the order completes; the value moved
/// once in the provider's books, each transaction stated once and each segment handed over once (the platform's record
/// refuses a second carrying out, `DuplicateTransaction`). Every pinned run of a restarting kind asserts its crash fell.
// verifies: INV-8, NFR-6
#[test]
fn restart_after_the_provider_moved_and_before_its_answer_was_logged_moves_nothing_twice() {
    let mut fell = 0;
    for sim in runs() {
        let pinned = PINNED.contains(&sim.plan);
        if pinned && sim.plan.kind.restarts() && sim.plan.kind != Kind::RestartWhileStuck {
            assert!(!sim.caught.is_empty(), "{}: the crash fell", sim.name());
        }
        if !matches!(sim.plan.kind, Kind::RestartAfterEffect(_)) || sim.caught.is_empty() {
            continue;
        }
        fell += 1;
        let bad = moved_once(sim);
        assert!(bad.is_empty(), "{}: {bad:?}", sim.name());
        let states = sim.run.state_names();
        for order in &sim.caught {
            assert_eq!(
                states.get(order).cloned().flatten().as_deref(),
                Some("complete"),
                "{}: the caught order completes",
                sim.name()
            );
        }
    }
    assert!(fell >= 2, "only {fell} runs crashed after an effect");
}
