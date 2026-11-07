//! The router suite run against its reference double (`tests/router_double/`), correct and with one flaw at a
//! time, and against records changed by hand, to show each check refuses what it should and only that.

mod router_double;

use router_double::{
    BREAKS_TIES_DESCENDING, Correct, FEE_IGNORES_MAXIMUM, FOLLOWS_RATE_CYCLES, IGNORES_DECIMALS,
    IGNORES_OBJECTIVE, OUTRANKS_UNCONFIRMED, ROUNDS_EACH_LEVEL, Reference,
};
use rutent_conformance::router::{Case, Cell, Check, cases, path, recompute, run};
use rutent_contracts::router_api::{
    Candidate, DecisionRecord, DropReason, Id, Objective, Round, Router,
};

fn case(name: &str) -> Case {
    cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no case {name}"))
}

/// Routes a case and reads the record back from its bytes, which runs the contract's check.
fn routed<R: Router>(case: &Case) -> DecisionRecord {
    let record = R::route(&case.request, &case.snapshot, &case.bounds);
    let bytes = serde_json::to_vec(&record).expect("a record serialises");
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("{}: the contract refuses the record: {e}", case.name))
}

fn routed_case<R: Router>(name: &str) -> DecisionRecord {
    routed::<R>(&case(name))
}

/// The record passes the contract, and the suite refuses it on `check` and on nothing else.
fn assert_fails_only(record: &DecisionRecord, check: Check) {
    record.check().expect("the contract accepts the record");
    let recomputed = recompute(record);
    assert_eq!(
        recomputed.failed_checks(),
        vec![check],
        "findings: {:#?}",
        recomputed.findings
    );
}

fn on_path<'a>(record: &'a DecisionRecord, route: &[&str]) -> &'a Candidate {
    let want = vec![route.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()];
    record
        .candidates
        .iter()
        .find(|c| path(record, c) == want)
        .unwrap_or_else(|| panic!("no candidate on {route:?}"))
}

fn index_on_path(record: &DecisionRecord, route: &[&str]) -> usize {
    let id = on_path(record, route).candidate_id.clone();
    record
        .candidates
        .iter()
        .position(|c| c.candidate_id == id)
        .expect("the candidate is in the record")
}

const SLOW_ROUTE: [&str; 4] = ["f-w", "f-ex-slow", "f-ramp-x", "f-dep"];
const FAST_ROUTE: [&str; 4] = ["f-w", "f-ex-fast", "f-ramp-y", "f-dep"];

// verifies: AC-2, REQ-AUD-4
#[test]
fn choice_is_recomputed_from_its_record_with_no_provider_call() {
    for case in cases() {
        // `recompute` takes the record and nothing else: no router, no provider, no clock.
        let record = routed::<Correct>(&case);
        let recomputed = recompute(&record);
        assert!(
            recomputed.passes(),
            "{}: {:#?}",
            case.name,
            recomputed.findings
        );
        assert_eq!(recomputed.choice, record.chosen, "{}", case.name);
    }
    let report = run::<Correct>();
    assert!(report.passes(), "{}", report.summary());
}

// verifies: AC-2, REQ-AUD-4
#[test]
fn same_request_and_snapshot_give_the_same_result() {
    for case in cases() {
        let first =
            serde_json::to_vec(&Correct::route(&case.request, &case.snapshot, &case.bounds))
                .unwrap();
        let second =
            serde_json::to_vec(&Correct::route(&case.request, &case.snapshot, &case.bounds))
                .unwrap();
        assert_eq!(first, second, "{}", case.name);
        let record: DecisionRecord = serde_json::from_slice(&first).unwrap();
        let replayed = serde_json::to_vec(&Correct::route(
            &record.request,
            &record.snapshot,
            &record.bounds,
        ))
        .unwrap();
        assert_eq!(first, replayed, "{}", case.name);
    }
    let report = run::<Correct>();
    for cell in [
        Cell::SameInputsGiveTheSameRecord,
        Cell::ReplaysFromItsOwnInputs,
    ] {
        assert!(
            report.outcomes.iter().any(|o| o.cell == cell)
                && report
                    .outcomes
                    .iter()
                    .filter(|o| o.cell == cell)
                    .all(|o| o.result.is_ok()),
            "{}",
            report.summary()
        );
    }
}

// verifies: AC-2, REQ-QTE-11
#[test]
fn a_changed_objective_changes_the_choice() {
    let cheapest = routed_case::<Correct>("ac2-choice-lowest-total-cost");
    let fastest = routed_case::<Correct>("ac2-choice-fastest-settlement");
    assert_eq!(cheapest.snapshot, fastest.snapshot);
    assert_eq!(cheapest.bounds, fastest.bounds);
    let chosen = |r: &DecisionRecord| path(r, r.chosen().expect("a route"));
    let want = |route: [&str; 4]| vec![route.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()];
    assert_eq!(chosen(&cheapest), want(SLOW_ROUTE));
    assert_eq!(chosen(&fastest), want(FAST_ROUTE));
    assert!(recompute(&cheapest).passes() && recompute(&fastest).passes());
    // Each route is outranked by the other under the other objective.
    let by = |r: &DecisionRecord, route| match &on_path(r, route).dropped {
        Some(DropReason::Outranked { by }) => Some(by.clone()),
        _ => None,
    };
    assert_eq!(by(&cheapest, &FAST_ROUTE), cheapest.chosen);
    assert_eq!(by(&fastest, &SLOW_ROUTE), fastest.chosen);
    let report = run::<Correct>();
    let cell = report
        .outcomes
        .iter()
        .find(|o| o.cell == Cell::ObjectiveChangesTheChoice)
        .expect("the cell runs");
    assert!(cell.result.is_ok(), "{:?}", cell.result);
}

// verifies: REQ-QTE-11, REQ-AUD-4
#[test]
fn a_record_whose_objective_changed_after_routing_is_refused() {
    let mut record = routed_case::<Correct>("ac2-choice-lowest-total-cost");
    record.request.objective = Objective::FastestSettlement {};
    assert_fails_only(&record, Check::RankingFollowsTheObjective);
}

// verifies: AC-3, REQ-CAT-14
#[test]
fn a_refused_dry_run_drops_its_option_and_another_provider_serves_the_tuple() {
    let record = routed_case::<Correct>("ac2-choice-lowest-total-cost");
    let refused = on_path(&record, &["f-w", "f-ex-slow", "f-ramp-x", "f-dep-2"]);
    assert_eq!(
        refused.dropped,
        Some(DropReason::DryRunRefused {
            figure: Id::new("f-dep-2-dry-run-refused").unwrap()
        })
    );
    assert_eq!(
        record.chosen.as_ref(),
        Some(&on_path(&record, &SLOW_ROUTE).candidate_id)
    );
}

// verifies: REQ-QTE-5
#[test]
fn a_fee_counted_by_its_formula_where_a_maximum_is_set_is_refused() {
    assert_fails_only(
        &routed_case::<Reference<FEE_IGNORES_MAXIMUM>>("ac2-shortlist"),
        Check::AmountsFollowTheirFigures,
    );
}

// verifies: REQ-QTE-5, REQ-QTE-12
#[test]
fn levels_rounded_one_by_one_are_refused() {
    assert_fails_only(
        &routed_case::<Reference<ROUNDS_EACH_LEVEL>>("ac2-shortlist"),
        Check::AmountsFollowTheirFigures,
    );
}

// verifies: REQ-QTE-12
#[test]
fn amounts_not_scaled_between_units_are_refused() {
    // In the shortlist case, unscaled amounts are too small for the ramps' fees, so nothing reaches Polygon and the
    // record has no candidate to judge. In the choice round the answers carry the confirmed routes, and the routes
    // priced from their prices show the flaw.
    let record = routed_case::<Reference<IGNORES_DECIMALS>>("ac2-choice-lowest-total-cost");
    assert!(!record.candidates.is_empty());
    assert_fails_only(&record, Check::AmountsFollowTheirFigures);
}

// verifies: REQ-QTE-5
#[test]
fn a_dropped_candidates_misstated_fee_is_refused_too() {
    let mut record = routed_case::<Correct>("ac2-choice-lowest-total-cost");
    // Unconfirmed: its deposit is priced from f-dep-price, not from an answer.
    let i = index_on_path(&record, &["f-w", "f-ex-slow", "f-ramp-y", "f-dep"]);
    assert!(matches!(
        record.candidates[i].dropped,
        Some(DropReason::Unconfirmed { .. })
    ));
    let mut parts: Vec<_> = record.candidates[i].parts.to_vec();
    let mut operations: Vec<_> = parts[0].operations.to_vec();
    let last = operations.last_mut().unwrap();
    let fee: u128 = last.fee.amount.parse().unwrap();
    last.fee.amount = (fee + 1).to_string();
    parts[0].operations = rutent_contracts::router_api::NonEmpty::new(operations).unwrap();
    record.candidates[i].parts = rutent_contracts::router_api::NonEmpty::new(parts).unwrap();
    assert_fails_only(&record, Check::AmountsFollowTheirFigures);
}

// verifies: REQ-QTE-11
#[test]
fn a_choice_that_ignores_the_objective_is_refused() {
    let record = routed_case::<Reference<IGNORES_OBJECTIVE>>("ac2-choice-fastest-settlement");
    assert_eq!(
        record.chosen.as_ref(),
        Some(&on_path(&record, &SLOW_ROUTE).candidate_id)
    );
    assert_fails_only(&record, Check::RankingFollowsTheObjective);
}

// verifies: REQ-QTE-11
#[test]
fn a_tie_broken_toward_the_higher_candidate_id_is_refused() {
    let record = routed_case::<Correct>("tie-choice");
    assert_eq!(
        record.chosen.as_ref(),
        Some(&on_path(&record, &["f-w", "f-ex-t1"]).candidate_id)
    );
    let flawed = routed_case::<Reference<BREAKS_TIES_DESCENDING>>("tie-choice");
    assert_eq!(
        flawed.chosen.as_ref(),
        Some(&on_path(&flawed, &["f-w", "f-ex-t2"]).candidate_id)
    );
    assert_fails_only(&flawed, Check::RankingFollowsTheObjective);
}

// verifies: REQ-QTE-11
#[test]
fn a_worse_candidate_chosen_over_a_better_one_is_refused() {
    let mut record = routed_case::<Correct>("ac2-choice-lowest-total-cost");
    let (slow, fast) = (
        index_on_path(&record, &SLOW_ROUTE),
        index_on_path(&record, &FAST_ROUTE),
    );
    let fast_id = record.candidates[fast].candidate_id.clone();
    record.candidates[fast].dropped = None;
    // The fast route was the only one outranked; now it is chosen, and the slow one outranked by it.
    record.candidates[slow].dropped = Some(DropReason::Outranked {
        by: fast_id.clone(),
    });
    record.chosen = Some(fast_id);
    assert_fails_only(&record, Check::RankingFollowsTheObjective);
}

// verifies: REQ-QTE-11
#[test]
fn a_choice_round_that_chooses_none_of_its_eligible_candidates_is_refused() {
    let mut record = routed_case::<Correct>("ac2-choice-fastest-settlement");
    let (slow, fast) = (
        index_on_path(&record, &SLOW_ROUTE),
        index_on_path(&record, &FAST_ROUTE),
    );
    let slow_id = record.candidates[slow].candidate_id.clone();
    record.candidates[fast].dropped = Some(DropReason::Outranked { by: slow_id });
    record.chosen = None;
    assert_fails_only(&record, Check::RankingFollowsTheObjective);
}

// verifies: ADR-0006
#[test]
fn a_followed_rate_cycle_is_refused() {
    let record = routed_case::<Reference<FOLLOWS_RATE_CYCLES>>("ac2-shortlist");
    let cycle = on_path(
        &record,
        &[
            "f-w",
            "f-ex-fast",
            "f-ex-back",
            "f-ex-slow",
            "f-ramp-x",
            "f-dep",
        ],
    );
    assert_eq!(cycle.dropped, None);
    assert_fails_only(&record, Check::RateCyclesAreNotFollowed);
    let correct = routed_case::<Correct>("ac2-shortlist");
    let cycle = on_path(
        &correct,
        &[
            "f-w",
            "f-ex-fast",
            "f-ex-back",
            "f-ex-slow",
            "f-ramp-x",
            "f-dep",
        ],
    );
    assert_eq!(cycle.dropped, Some(DropReason::SuspectRateCycle {}));
}

// verifies: ADR-0006
#[test]
fn a_suspect_rate_cycle_reason_on_a_route_without_one_is_refused() {
    let mut record = routed_case::<Correct>("ac2-choice-lowest-total-cost");
    let i = index_on_path(&record, &["f-w", "f-ex-c", "f-ramp-x", "f-dep"]);
    assert!(matches!(
        record.candidates[i].dropped,
        Some(DropReason::StaleFigure { .. })
    ));
    record.candidates[i].dropped = Some(DropReason::SuspectRateCycle {});
    assert_fails_only(&record, Check::RateCyclesAreNotFollowed);
}

// verifies: REQ-QTE-11, REQ-AUD-4
#[test]
fn an_unconfirmed_candidate_outranked_rather_than_dropped_is_refused() {
    let record = routed_case::<Reference<OUTRANKS_UNCONFIRMED>>("ac2-choice-lowest-total-cost");
    let unconfirmed = on_path(&record, &["f-w", "f-ex-slow", "f-ramp-y", "f-dep"]);
    assert!(matches!(
        unconfirmed.dropped,
        Some(DropReason::Outranked { .. })
    ));
    assert_fails_only(&record, Check::OutrankedCandidatesAreEligible);
}

// verifies: REQ-CAT-5
#[test]
fn a_shortlist_round_over_a_snapshot_with_statements_is_refused() {
    let mut over_answers = case("ac2-choice-lowest-total-cost");
    over_answers.request.round = Round::Shortlist {};
    let record = routed::<Correct>(&over_answers);
    assert_fails_only(&record, Check::ShortlistReadsAdvertisedFigures);
}

// verifies: REQ-ORD-8, REQ-QTE-13, REQ-QTE-11
#[test]
fn the_policy_case_drops_by_policy_by_ceilings_on_output_and_by_outcome() {
    let record = routed_case::<Correct>("policy-shortlist");
    let reason = |route: &[&str]| on_path(&record, route).dropped.clone();
    assert_eq!(
        reason(&["f-w", "f-ex-banned", "f-ramp-x", "f-dep"]),
        Some(DropReason::ExcludedByPolicy {})
    );
    assert!(matches!(
        reason(&["f-w", "f-ex-capped", "f-ramp-x", "f-dep"]),
        Some(DropReason::OverCeiling { .. })
    ));
    assert_eq!(
        reason(&["f-w", "f-ex-fast", "f-ramp-x", "f-dep-greedy"]),
        Some(DropReason::NoPositiveOutcome {})
    );
    assert!(matches!(
        reason(&["f-w", "f-ex-fast", "f-ramp-narrow", "f-dep"]),
        Some(DropReason::OutsideBand { .. })
    ));
    assert_eq!(
        reason(&["f-w", "f-ex-fast", "f-ramp-y", "f-dep"]),
        Some(DropReason::BelowFloor {})
    );
    let shortlist: Vec<_> = record
        .shortlist()
        .iter()
        .map(|c| path(&record, c))
        .collect();
    assert_eq!(
        shortlist,
        vec![vec![
            vec!["f-w", "f-ex-fast", "f-ramp-x", "f-dep"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        ]]
    );
    assert!(recompute(&record).passes());
}

// verifies: DEC-5
#[test]
fn every_flaw_fails_the_suite_run() {
    let fails = |report: rutent_conformance::router::Report| !report.passes();
    assert!(fails(run::<Reference<FEE_IGNORES_MAXIMUM>>()));
    assert!(fails(run::<Reference<ROUNDS_EACH_LEVEL>>()));
    assert!(fails(run::<Reference<IGNORES_DECIMALS>>()));
    assert!(fails(run::<Reference<IGNORES_OBJECTIVE>>()));
    assert!(fails(run::<Reference<BREAKS_TIES_DESCENDING>>()));
    assert!(fails(run::<Reference<FOLLOWS_RATE_CYCLES>>()));
    assert!(fails(run::<Reference<OUTRANKS_UNCONFIRMED>>()));
}

// verifies: REQ-AUD-4, DEC-5
#[test]
fn the_contracts_valid_decision_records_recompute() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../contracts/router-api/fixtures/valid");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("the router-api fixtures") {
        let path = entry.expect("a fixture").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.starts_with("decision-record-") {
            continue;
        }
        let recorded: rutent_contracts::router_api::RecordedDecision =
            serde_json::from_slice(&std::fs::read(&path).unwrap())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        let recomputed = recompute(&recorded.statement);
        assert!(recomputed.passes(), "{name}: {:#?}", recomputed.findings);
        seen += 1;
    }
    assert!(seen >= 7, "only {seen} decision-record fixtures found");
}

fn set_figures(record: &mut DecisionRecord, candidate: usize, operation: usize, figures: Vec<Id>) {
    use rutent_contracts::router_api::NonEmpty;
    let mut parts = record.candidates[candidate].parts.to_vec();
    let mut operations = parts[0].operations.to_vec();
    operations[operation].figures = NonEmpty::new(figures).unwrap();
    parts[0].operations = NonEmpty::new(operations).unwrap();
    record.candidates[candidate].parts = NonEmpty::new(parts).unwrap();
}

// verifies: AC-2, REQ-AUD-4, REQ-QTE-11
#[test]
fn the_best_route_can_not_be_dropped_unconfirmed_by_leaving_its_quote_out() {
    let mut record = routed_case::<Correct>("ac2-choice-lowest-total-cost");
    let (slow, fast) = (
        index_on_path(&record, &SLOW_ROUTE),
        index_on_path(&record, &FAST_ROUTE),
    );
    let exchange = record.candidates[slow].parts[0].operations[1].clone();
    let without_quote: Vec<Id> = exchange
        .figures
        .iter()
        .filter(|id| id.as_str() != "f-ex-slow-quote")
        .cloned()
        .collect();
    set_figures(&mut record, slow, 1, without_quote);
    record.candidates[slow].dropped = Some(DropReason::Unconfirmed {
        part: 0,
        operation: 1,
    });
    record.candidates[fast].dropped = None;
    record.chosen = Some(record.candidates[fast].candidate_id.clone());
    let refusal = record
        .check()
        .expect_err("the contract refuses the hidden quote");
    assert_eq!(
        refusal.to_string(),
        "every operation cites every usable answer and judgement about its tuple at its amount in"
    );
    assert_eq!(recompute(&record).failed_checks(), vec![Check::Contract]);
}

// verifies: AC-2, REQ-QTE-11, REQ-QTE-3
#[test]
fn a_quote_at_another_amount_can_not_slow_the_fastest_route() {
    use rutent_contracts::provider_role::{ConversionOutcome, ResultBody, SafeInteger};
    use rutent_contracts::router_api::FigureValue;
    let mut record = routed_case::<Correct>("ac2-choice-fastest-settlement");
    // A quote of the fast exchange for half a million COP, declaring a day to settle.
    let mut slow_quote = record
        .snapshot
        .figures
        .iter()
        .find(|f| f.figure_id.as_str() == "f-ex-fast-quote")
        .unwrap()
        .clone();
    slow_quote.figure_id = Id::new("f-ex-fast-quote-50m").unwrap();
    let FigureValue::FreshQuote { quote, .. } = &mut slow_quote.value else {
        unreachable!()
    };
    let ResultBody::Conversion(conversion) = &mut quote.statement else {
        unreachable!()
    };
    let ConversionOutcome::DryRunAccepted(terms) = &mut conversion.outcome else {
        unreachable!()
    };
    terms.amount_in = "50000000".to_owned();
    terms.settlement_seconds = SafeInteger::try_from(86_400u64).unwrap();
    record.snapshot.figures.push(slow_quote);
    // Cited on the fast route's exchange, it would make that route take a day, and the slow one would win.
    let (slow, fast) = (
        index_on_path(&record, &SLOW_ROUTE),
        index_on_path(&record, &FAST_ROUTE),
    );
    let mut figures: Vec<Id> = record.candidates[fast].parts[0].operations[1]
        .figures
        .to_vec();
    figures.push(Id::new("f-ex-fast-quote-50m").unwrap());
    set_figures(&mut record, fast, 1, figures);
    let slow_id = record.candidates[slow].candidate_id.clone();
    record.candidates[slow].dropped = None;
    record.candidates[fast].dropped = Some(DropReason::Outranked {
        by: slow_id.clone(),
    });
    record.chosen = Some(slow_id);
    let refusal = record
        .check()
        .expect_err("the contract refuses the quote at another amount");
    assert_eq!(
        refusal.to_string(),
        "a fresh quote an operation cites is for its amount in"
    );
    assert_eq!(recompute(&record).failed_checks(), vec![Check::Contract]);
}
