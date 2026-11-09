//! The shortlist (`T-011-02`): fresh quotes, dry runs and eligibility asked only of shortlisted providers, their
//! answers entering the choice round as recorded figures, a refused dry run dropping its option for another
//! provider of the tuple, and a routing with no path arriving with a positive balance refused.

mod common;

use common::{
    Net, WIDE, bounds, choice_of, endpoint, fee, on, outer, parse, request, routed, shortlist,
};
use rutent_conformance::router::recompute;
use rutent_contracts::router_api::{
    CatalogueSnapshot, DecisionRecord, DropReason, Id, OperationKind, PolicyBounds,
};
use rutent_router::shortlist::{Refusal, Wanted, asks};
use serde_json::{Value, json};

const SHORTLIST_AT: &str = "2026-11-06T15:00:00Z";
const CHOICE_AT: &str = "2026-11-06T15:01:00Z";
const MADE_AT: &str = "2026-11-06T15:00:30Z";
const VALID_UNTIL: &str = "2026-11-06T15:05:30Z";

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn priced(flat: &str, numerator: &str, denominator: &str) -> Value {
    json!({ "fee": fee(flat, numerator, denominator), "settlement_seconds": 60 })
}

/// A statement figure of `provider`'s, `MADE_AT`, 30 seconds old at `CHOICE_AT`.
fn statement(net: &mut Net, id: &str, provider: &str, valid: Option<u64>, value: Value) {
    let mut figure = json!({
        "figure_id": id,
        "origin": Net::origin(provider),
        "age_seconds": 30,
        "value": value,
    });
    if let Some(valid) = valid {
        figure["valid_for_seconds"] = json!(valid);
    }
    net.figures.push(figure);
}

/// The provider of tuple `tuple`, as `Net` names it.
fn provider_of(tuple: &str) -> String {
    format!("p-{tuple}")
}

/// An accepted dry run of `tuple`'s provider: `withdrawal` or `deposit` for a reserve, `transfer` for a ramp (its
/// fresh quote).
fn accepted(
    net: &mut Net,
    id: &str,
    (kind, tuple): (&str, &str),
    (amount_in, fee_asset, fee, amount_out): (&str, &str, &str, &str),
) {
    let provider = provider_of(tuple);
    let mut outcome = json!({
        "status": "dry_run_accepted",
        "amount_in": amount_in,
        "amount_out": amount_out,
        "fee": { "asset": fee_asset, "amount": fee },
        "valid_until": VALID_UNTIL,
        "voided_by": ["validity_passes", "amount_changes"],
        "settlement_seconds": 30,
    });
    let body = |outcome: Value| json!({ "kind": kind, "transaction": format!("tx-{id}"), "outcome": outcome });
    let value = if kind == "transfer" {
        outcome["direction"] = json!("exact_in");
        json!({
            "kind": "fresh_quote",
            "tuple": tuple,
            "quote": Net::envelope(&provider, MADE_AT, "urn:rutent:provider-role:result", body(outcome)),
        })
    } else {
        json!({
            "kind": "dry_run",
            "tuple": tuple,
            "amount_in": amount_in,
            "result": Net::envelope(&provider, MADE_AT, "urn:rutent:provider-role:result", body(outcome)),
        })
    };
    statement(net, id, &provider, Some(300), value);
}

/// A refused dry run of `tuple`'s provider at `amount_in`.
fn refused(net: &mut Net, id: &str, (kind, tuple): (&str, &str), amount_in: &str) {
    let provider = provider_of(tuple);
    let body = json!({
        "kind": kind,
        "transaction": format!("tx-{id}"),
        "outcome": { "status": "refused", "refusal": { "class": "final" } },
    });
    let value = json!({
        "kind": "dry_run",
        "tuple": tuple,
        "amount_in": amount_in,
        "result": Net::envelope(&provider, MADE_AT, "urn:rutent:provider-role:result", body),
    });
    statement(net, id, &provider, None, value);
}

/// A verifier's judgement of yes for `tuple`.
fn eligible(net: &mut Net, id: &str, tuple: &str) {
    let verifier = "verifier-1";
    let body = json!({
        "subject": "subj-42",
        "class": "kyc.natural_person",
        "answer": "yes",
        "valid_until": VALID_UNTIL,
    });
    let value = json!({
        "kind": "eligibility",
        "tuple": tuple,
        "judgement": Net::envelope(verifier, MADE_AT, "urn:rutent:provider-role:judgement", body),
    });
    statement(net, id, verifier, Some(300), value);
}

/// The choice round's snapshot: the shortlist round's advertised figures, a minute older, and `net`'s statements.
fn choice_snapshot(advertised: &Net, answers: Net) -> CatalogueSnapshot {
    let mut figures: Vec<Value> = advertised
        .figures
        .iter()
        .cloned()
        .map(|mut f| {
            f["age_seconds"] = json!(f["age_seconds"].as_u64().unwrap() + 60);
            f
        })
        .collect();
    figures.extend(answers.figures);
    parse(
        "choice snapshot",
        json!({ "snapshot_id": format!("{}-choice", advertised.id), "taken_at": CHOICE_AT, "figures": figures }),
    )
}

fn passes(record: &DecisionRecord) {
    let recomputed = recompute(record);
    assert!(recomputed.passes(), "{:#?}", recomputed.findings);
}

fn route_both(
    net: &Net,
    answers: Net,
    (from, to, amount): (Value, Value, &str),
    bounds: &PolicyBounds,
) -> (DecisionRecord, DecisionRecord) {
    let first = routed(
        &request("r-short", shortlist(), from.clone(), to.clone(), amount),
        &net.build(),
        bounds,
    );
    let second = routed(
        &request(
            "r-choice",
            choice_of("r-short", SHORTLIST_AT),
            from,
            to,
            amount,
        ),
        &choice_snapshot(net, answers),
        bounds,
    );
    passes(&first);
    passes(&second);
    (first, second)
}

// verifies: AC-3, REQ-CAT-14
#[test]
fn provider_whose_dry_run_rejects_is_dropped_for_another_serving_the_tuple() {
    // Two reserves take USDC from chain-a into the hub. The cheaper one refuses the amount on its dry run.
    let mut net = Net::new("snap-refusal", SHORTLIST_AT, 60);
    let ends = || (outer("chain-a"), json!("hub"));
    net.reserve(
        "f-cheap",
        ends(),
        "USDC",
        (6, 6),
        ("1", WIDE),
        priced("0", "0", "1"),
    );
    net.reserve(
        "f-dear",
        ends(),
        "USDC",
        (6, 6),
        ("1", WIDE),
        priced("0", "1", "100"),
    );
    let from = endpoint(outer("chain-a"), "USDC", 6);
    let to = endpoint(json!("hub"), "USDC", 6);
    let shortlisted = routed(
        &request(
            "r-short",
            shortlist(),
            from.clone(),
            to.clone(),
            "100000000",
        ),
        &net.build(),
        &bounds(1),
    );
    let asked = asks(&shortlisted).expect("both reserves are shortlisted");
    for reserve in ["f-cheap", "f-dear"] {
        let questions = asked.of(&id(&provider_of(reserve)));
        assert_eq!(questions.len(), 1, "{reserve} is asked once");
        assert_eq!(questions[0].tuple, id(reserve));
        assert_eq!(questions[0].wants, Wanted::DryRun);
        assert_eq!(questions[0].kind, OperationKind::Withdrawal);
        assert_eq!(questions[0].amount_in.amount().as_str(), "100000000");
    }

    let mut answers = Net::new("answers", CHOICE_AT, 0);
    refused(
        &mut answers,
        "d-cheap",
        ("withdrawal", "f-cheap"),
        "100000000",
    );
    accepted(
        &mut answers,
        "d-dear",
        ("withdrawal", "f-dear"),
        ("100000000", "USDC", "1000000", "99000000"),
    );
    let (_, chosen) = route_both(&net, answers, (from, to, "100000000"), &bounds(1));
    assert_eq!(
        on(&chosen, &["f-cheap"]).dropped,
        Some(DropReason::DryRunRefused {
            figure: id("d-cheap")
        })
    );
    let route = chosen
        .chosen()
        .expect("the other reserve serving the tuple");
    assert_eq!(common::tuples(route), vec!["f-dear"]);
    assert_eq!(route.parts[0].operations[0].amount_out.as_str(), "99000000");
}

// verifies: REQ-CAT-5
#[test]
fn provider_off_the_shortlist_is_never_asked() {
    let mut net = Net::new("snap-asked", SHORTLIST_AT, 120);
    let free = || priced("0", "0", "1");
    net.ramp(
        "f-kept",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        free(),
    );
    net.ramp(
        "f-excluded",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        free(),
    );
    net.ramp(
        "f-narrow",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", "1000"),
        free(),
    );
    net.ramp(
        "f-stale",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        free(),
    );
    let stale_price = net.figures.len() - 1;
    net.figures[stale_price]["valid_for_seconds"] = json!(60);
    // Reaches no endpoint the request asks about.
    net.ramp(
        "f-elsewhere",
        ("hub-c", "hub-d"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        free(),
    );
    let mut policy = serde_json::to_value(bounds(1)).unwrap();
    policy["excluded_providers"] = json!([provider_of("f-excluded")]);
    let policy: PolicyBounds = parse("bounds", policy);
    let from = endpoint(json!("hub"), "USDC", 6);
    let to = endpoint(json!("hub-b"), "USDC", 6);
    let record = routed(
        &request(
            "r-short",
            shortlist(),
            from.clone(),
            to.clone(),
            "100000000",
        ),
        &net.build(),
        &policy,
    );
    passes(&record);
    assert_eq!(record.candidates.len(), 4);
    let asked = asks(&record).expect("one ramp is shortlisted");

    // Only the shortlisted ramp's provider is asked, and only about its own tuple at the amount reaching it.
    let providers: Vec<&str> = asked.providers().map(Id::as_str).collect();
    assert_eq!(providers, vec!["p-f-kept"]);
    let questions = asked.of(&id("p-f-kept"));
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].tuple, id("f-kept"));
    assert_eq!(questions[0].wants, Wanted::FreshQuote);
    assert_eq!(questions[0].amount_in.amount().as_str(), "100000000");
    for other in ["f-excluded", "f-narrow", "f-stale", "f-elsewhere"] {
        assert!(
            asked.of(&id(&provider_of(other))).is_empty(),
            "{other} is asked"
        );
    }

    // An answer enters the choice round only where it was asked for: the shortlisted provider, its tuple, at the
    // amount asked. Advertised figures always enter.
    let mut answers = Net::new("answers", CHOICE_AT, 0);
    let free_quote = ("100000000", "USDC", "0", "100000000");
    accepted(&mut answers, "q-kept", ("transfer", "f-kept"), free_quote);
    accepted(
        &mut answers,
        "q-kept-other-amount",
        ("transfer", "f-kept"),
        ("200000000", "USDC", "0", "200000000"),
    );
    accepted(
        &mut answers,
        "q-excluded",
        ("transfer", "f-excluded"),
        free_quote,
    );
    accepted(&mut answers, "q-stale", ("transfer", "f-stale"), free_quote);
    refused(
        &mut answers,
        "d-narrow",
        ("transfer", "f-narrow"),
        "100000000",
    );
    eligible(&mut answers, "e-kept", "f-kept");
    eligible(&mut answers, "e-elsewhere", "f-elsewhere");
    let snapshot = choice_snapshot(&net, answers);
    let admitted: Vec<&str> = snapshot
        .figures
        .iter()
        .filter(|f| asked.admits(f))
        .map(|f| f.figure_id.as_str())
        .collect();
    let advertised: Vec<String> = net
        .figures
        .iter()
        .map(|f| f["figure_id"].as_str().unwrap().to_owned())
        .collect();
    let mut expected: Vec<&str> = advertised.iter().map(String::as_str).collect();
    expected.extend(["q-kept", "e-kept"]);
    assert_eq!(admitted, expected);

    // A choice round's record is never asked from: its candidates are not a shortlist.
    let mut kept = snapshot.clone();
    kept.figures.retain(|f| asked.admits(f));
    let choice = routed(
        &request(
            "r-choice",
            choice_of("r-short", SHORTLIST_AT),
            from,
            to,
            "100000000",
        ),
        &kept,
        &policy,
    );
    passes(&choice);
    assert_eq!(common::tuples(choice.chosen().unwrap()), vec!["f-kept"]);
    assert_eq!(asks(&choice).err(), Some(Refusal::NotAShortlistRound));
}

// verifies: REQ-QTE-13
#[test]
fn route_with_no_positive_arrival_is_refused() {
    // A ramp whose flat fee takes the whole amount arrives with nothing.
    let mut net = Net::new("snap-nothing", SHORTLIST_AT, 60);
    net.ramp(
        "f-zero",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        priced("100", "0", "1"),
    );
    let from = endpoint(json!("hub"), "USDC", 6);
    let to = endpoint(json!("hub-b"), "USDC", 6);
    let ends = || (from.clone(), to.clone(), "100");
    let answers = Net::new("answers", CHOICE_AT, 0);
    let (first, second) = route_both(&net, answers, ends(), &bounds(1));
    for record in [&first, &second] {
        assert_eq!(
            on(record, &["f-zero"]).dropped,
            Some(DropReason::NoPositiveOutcome {})
        );
        assert_eq!(record.chosen, None);
    }
    // Nothing is offered, so nobody is asked anything.
    assert_eq!(asks(&first).err(), Some(Refusal::NoRoute));

    // With an alternative that arrives with something, the alternative is shortlisted, asked and chosen.
    net.ramp(
        "f-alt",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        priced("1", "0", "1"),
    );
    let mut answers = Net::new("answers", CHOICE_AT, 0);
    accepted(
        &mut answers,
        "q-alt",
        ("transfer", "f-alt"),
        ("100", "USDC", "1", "99"),
    );
    let (first, second) = route_both(&net, answers, ends(), &bounds(1));
    let asked = asks(&first).expect("the alternative is shortlisted");
    let providers: Vec<&str> = asked.providers().map(Id::as_str).collect();
    assert_eq!(providers, vec!["p-f-alt"]);
    assert_eq!(
        on(&second, &["f-zero"]).dropped,
        Some(DropReason::NoPositiveOutcome {})
    );
    assert_eq!(common::tuples(second.chosen().unwrap()), vec!["f-alt"]);
}

// verifies: REQ-CAT-5
#[test]
fn provider_reached_at_two_amounts_is_asked_at_one_only() {
    // Two reserves bring USDC from chain-a into the hub, and one ramp takes it on. Through the free reserve 100
    // USDC reach the ramp; through the dearer one, 99. Asked at both, the ramp would learn that a rival came before
    // it, and their fees' difference.
    let mut net = Net::new("snap-two-amounts", SHORTLIST_AT, 60);
    let ends = || (outer("chain-a"), json!("hub"));
    // The dearer reserve's tuple id sorts first, so it is `c001`; the search ranks the other route first.
    net.reserve(
        "f-r1",
        ends(),
        "USDC",
        (6, 6),
        ("1", WIDE),
        priced("0", "1", "100"),
    );
    net.reserve(
        "f-r2",
        ends(),
        "USDC",
        (6, 6),
        ("1", WIDE),
        priced("0", "0", "1"),
    );
    net.ramp(
        "f-ramp",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        priced("0", "0", "1"),
    );
    let from = endpoint(outer("chain-a"), "USDC", 6);
    let to = endpoint(json!("hub-b"), "USDC", 6);
    let record = routed(
        &request(
            "r-short",
            shortlist(),
            from.clone(),
            to.clone(),
            "100000000",
        ),
        &net.build(),
        &bounds(2),
    );
    passes(&record);
    assert_eq!(record.shortlist().len(), 2);
    let asked = asks(&record).expect("both routes are shortlisted");
    let questions = asked.of(&id(&provider_of("f-ramp")));
    assert_eq!(questions.len(), 1, "the ramp is asked once: {questions:#?}");
    // At the amount of the route the search ranks first.
    assert_eq!(questions[0].amount_in.amount().as_str(), "100000000");

    // The other route's ramp stays unconfirmed until Core asks again.
    let mut answers = Net::new("answers", CHOICE_AT, 0);
    let (r1, r2) = (
        ("100000000", "USDC", "1000000", "99000000"),
        ("100000000", "USDC", "0", "100000000"),
    );
    accepted(&mut answers, "d-r1", ("withdrawal", "f-r1"), r1);
    accepted(&mut answers, "d-r2", ("withdrawal", "f-r2"), r2);
    accepted(&mut answers, "q-ramp", ("transfer", "f-ramp"), r2);
    let (_, choice) = route_both(&net, answers, (from, to, "100000000"), &bounds(2));
    assert_eq!(
        on(&choice, &["f-r1", "f-ramp"]).dropped,
        Some(DropReason::Unconfirmed {
            part: 0,
            operation: 1
        })
    );
    assert_eq!(
        common::tuples(choice.chosen().unwrap()),
        vec!["f-r2", "f-ramp"]
    );
}

/// The figure `id` of `answers`, read alone.
fn figure(answers: &Net, id: &str) -> rutent_contracts::router_api::Figure {
    let found = answers
        .figures
        .iter()
        .find(|f| f["figure_id"] == id)
        .unwrap()
        .clone();
    parse("figure", found)
}

// verifies: REQ-CAT-14, REQ-CAT-5
#[test]
fn ramp_that_refuses_its_quote_is_dropped_as_refused_not_unconfirmed() {
    let mut net = Net::new("snap-ramp-refusal", SHORTLIST_AT, 60);
    let free = || priced("0", "0", "1");
    net.ramp(
        "f-ramp",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        free(),
    );
    let from = endpoint(json!("hub"), "USDC", 6);
    let to = endpoint(json!("hub-b"), "USDC", 6);
    let record = routed(
        &request(
            "r-short",
            shortlist(),
            from.clone(),
            to.clone(),
            "100000000",
        ),
        &net.build(),
        &bounds(1),
    );
    let asked = asks(&record).unwrap();
    assert_eq!(asked.of(&id("p-f-ramp"))[0].wants, Wanted::FreshQuote);

    // A ramp's refusal of a quote is its refused transfer dry run, and is admitted.
    let mut answers = Net::new("answers", CHOICE_AT, 0);
    refused(&mut answers, "d-ramp", ("transfer", "f-ramp"), "100000000");
    assert!(asked.admits(&figure(&answers, "d-ramp")));
    // A fresh quote whose result refuses is no quote: the contract keeps it out of every snapshot.
    let mut quote = answers.figures[0].clone();
    quote["figure_id"] = json!("q-ramp-refused");
    quote["value"] = json!({
        "kind": "fresh_quote",
        "tuple": "f-ramp",
        "quote": quote["value"]["result"].clone(),
    });
    let mut with_quote = Net::new("answers", CHOICE_AT, 0);
    with_quote.figures.push(quote);
    assert!(!asked.admits(&figure(&with_quote, "q-ramp-refused")));
    let mut snapshot =
        serde_json::to_value(choice_snapshot(&net, Net::new("a", CHOICE_AT, 0))).unwrap();
    snapshot["figures"]
        .as_array_mut()
        .unwrap()
        .push(with_quote.figures[0].clone());
    assert!(serde_json::from_value::<CatalogueSnapshot>(snapshot).is_err());

    let (_, choice) = route_both(&net, answers, (from, to, "100000000"), &bounds(1));
    assert_eq!(
        on(&choice, &["f-ramp"]).dropped,
        Some(DropReason::DryRunRefused {
            figure: id("d-ramp")
        })
    );
}

// verifies: REQ-CAT-5
#[test]
fn answer_of_another_kind_than_asked_is_not_admitted() {
    let mut net = Net::new("snap-kinds", SHORTLIST_AT, 60);
    let free = || priced("0", "0", "1");
    net.reserve(
        "f-in",
        (outer("chain-a"), json!("hub")),
        "USDC",
        (6, 6),
        ("1", WIDE),
        free(),
    );
    net.ramp(
        "f-ramp",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        free(),
    );
    let record = routed(
        &request(
            "r-short",
            shortlist(),
            endpoint(outer("chain-a"), "USDC", 6),
            endpoint(json!("hub-b"), "USDC", 6),
            "100000000",
        ),
        &net.build(),
        &bounds(2),
    );
    let asked = asks(&record).unwrap();
    let terms = ("100000000", "USDC", "0", "100000000");
    let mut answers = Net::new("answers", CHOICE_AT, 0);
    accepted(&mut answers, "d-in", ("withdrawal", "f-in"), terms);
    accepted(&mut answers, "q-ramp", ("transfer", "f-ramp"), terms);
    assert!(asked.admits(&figure(&answers, "d-in")));
    assert!(asked.admits(&figure(&answers, "q-ramp")));

    // The reserve was asked for a dry run, the ramp for a fresh quote: each answer in the other's form stays out.
    let mut swapped = Net::new("answers", CHOICE_AT, 0);
    let mut as_quote = answers.figures[0].clone();
    as_quote["figure_id"] = json!("q-in");
    as_quote["value"] = json!({
        "kind": "fresh_quote",
        "tuple": "f-in",
        "quote": as_quote["value"]["result"].clone(),
    });
    let mut as_dry_run = answers.figures[1].clone();
    as_dry_run["figure_id"] = json!("d-ramp");
    as_dry_run["value"] = json!({
        "kind": "dry_run",
        "tuple": "f-ramp",
        "amount_in": "100000000",
        "result": as_dry_run["value"]["quote"].clone(),
    });
    swapped.figures.extend([as_quote, as_dry_run]);
    assert!(!asked.admits(&figure(&swapped, "q-in")));
    assert!(!asked.admits(&figure(&swapped, "d-ramp")));
}
