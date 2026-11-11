//! The quote rules (`T-011-03`) as pure checks: a quote's fields; the price shown against the providers' terms,
//! the operator's fee apart; declared settlement time against actual; a charge against its formula and maximum;
//! a formula binding from segment start; quote and price labels; slippage within three days; no firm rate past
//! them; held quotes and their validity. Property tests (`DEC-18`) close the file, drawn from a fixed seed so a
//! failure names the seed that reproduces it.

mod common;

use common::{
    Net, WIDE, bounds, choice_of, endpoint, fee, limited, on, parse, request, routed, shortlist,
    tuples, unit,
};
use rutent_contracts::provider_role::{ResultBody, TransferOutcome, TransferQuote};
use rutent_contracts::router_api::{
    Amount, Decimal, DecisionRecord, FeeFormula, Figure, FigureValue, Id, StatedTerms, Timestamp,
    TupleKey,
};
use rutent_router::quote::{
    Binding, Bound, ChargeFlag, Commitment, Executed, Firmness, HeldQuoteRefused, HoldRefused,
    Label, Measured, NeedsAcceptance, NoSlippageLimit, PriceRefused, QuoteField, QuoteRefused,
    SegmentTime, THREE_DAYS_SECONDS, UnexplainedDifference, binding, check_charge, commitments,
    declared_times, executed_price, firmness, formula_fee, held_label, hold, may_start,
    quote_fields, quoted_price, rate_within, redeem,
};
use serde_json::{Value, json};

const TAKEN_AT: &str = "2026-11-06T15:02:00Z";

fn amount(text: &str) -> Amount {
    Amount::new(text).unwrap()
}

fn at(text: &str) -> Timestamp {
    Timestamp::new(text).unwrap()
}

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn decimal(text: &str) -> Decimal {
    Decimal::new(text).unwrap()
}

fn formula(flat: &str, numerator: &str, denominator: &str, maximum: Option<&str>) -> FeeFormula {
    let mut formula = fee(flat, numerator, denominator);
    if let Some(maximum) = maximum {
        formula["maximum"] = json!(maximum);
    }
    parse("fee formula", formula)
}

/// A chain of ramps `r0`, `r1`, … from `n0` to `n{k}` in USDC of 10^-6, each with a fee of `flat + a × 1/1000`
/// and the given declared settlement seconds, routed in a shortlist round for 100 USDC; and the candidate's tuples.
fn chain(name: &str, ramps: &[(&str, u64)]) -> (DecisionRecord, Vec<String>) {
    let mut net = Net::new(&format!("snap-{name}"), TAKEN_AT, 60);
    let nodes: Vec<String> = (0..=ramps.len()).map(|i| format!("n{i}")).collect();
    for (i, (flat, seconds)) in ramps.iter().enumerate() {
        net.ramp(
            &format!("r{i}"),
            (&nodes[i], &nodes[i + 1]),
            "USDC",
            (6, 6),
            ("1", WIDE),
            json!({ "fee": fee(flat, "1", "1000"), "settlement_seconds": seconds }),
        );
    }
    let record = routed(
        &request(
            &format!("r-{name}"),
            shortlist(),
            endpoint(json!("n0"), "USDC", 6),
            endpoint(json!(nodes[ramps.len()]), "USDC", 6),
            "100000000",
        ),
        &net.build(),
        &bounds(u8::try_from(ramps.len()).unwrap()),
    );
    let path = (0..ramps.len()).map(|i| format!("r{i}")).collect();
    (record, path)
}

fn path_of(path: &[String]) -> Vec<&str> {
    path.iter().map(String::as_str).collect()
}

fn time(part: u32, operation: u32, tuple: &str, declared_seconds: u64) -> SegmentTime {
    SegmentTime {
        part,
        operation,
        tuple: id(tuple),
        declared_seconds,
    }
}

/// A choice round over one ramp `r0` from `hub` to `hub-b`, priced at 1% and declaring 60 seconds, confirmed by
/// a fresh quote of 0.5% that declares 30 seconds; it chooses `r0`.
fn confirmed() -> DecisionRecord {
    let mut net = Net::new("snap-held", TAKEN_AT, 120);
    net.ramp(
        "r0",
        ("hub", "hub-b"),
        "USDC",
        (6, 6),
        ("1", WIDE),
        json!({ "fee": fee("0", "1", "100"), "settlement_seconds": 60 }),
    );
    net.transfer_quote(
        "q-r0",
        "r0",
        ("100000000", "USDC", "500000", "99500000"),
        30,
        ("2026-11-06T15:01:40Z", "2026-11-06T15:03:40Z"),
        (20, 120),
    );
    routed(
        &request(
            "r-held",
            choice_of("r-held-shortlist", "2026-11-06T15:00:00Z"),
            endpoint(json!("hub"), "USDC", 6),
            endpoint(json!("hub-b"), "USDC", 6),
            "100000000",
        ),
        &net.build(),
        &bounds(1),
    )
}

/// An exchange `x0` at `hub` from USDC to EURC, both of 10^-6, at a fixed 0.9 with no fee, declaring `seconds`;
/// and, if `slippage` is given, a fresh conversion quote for 100 USDC with that regime, in a choice round.
fn exchange(name: &str, seconds: u64, slippage: Option<Value>) -> DecisionRecord {
    let mut net = Net::new(&format!("snap-{name}"), TAKEN_AT, 60);
    net.exchange(
        "x0",
        "hub",
        (unit("USDC", 6), unit("EURC", 6)),
        ("1", WIDE),
        json!({
            "fee": fee("0", "0", "1"),
            "rate": { "kind": "fixed", "rate": "0.9" },
            "settlement_seconds": seconds,
        }),
    );
    let round = match &slippage {
        Some(slippage) => {
            let provider = "p-x0";
            let body = json!({
                "kind": "conversion",
                "transaction": format!("tx-{name}"),
                "outcome": {
                    "status": "dry_run_accepted",
                    "amount_in": "100000000",
                    "amount_out": "90000000",
                    "rate": "0.9",
                    "slippage": slippage,
                    "fee": { "asset": "USDC", "amount": "0" },
                    "direction": "exact_in",
                    "valid_until": "2026-11-06T15:03:40Z",
                    "voided_by": ["validity_passes", "amount_changes"],
                    "settlement_seconds": seconds,
                },
            });
            net.figures.push(json!({
                "figure_id": "q-x0",
                "origin": Net::origin(provider),
                "age_seconds": 20,
                "valid_for_seconds": 120,
                "value": {
                    "kind": "fresh_quote",
                    "tuple": "x0",
                    "quote": Net::envelope(
                        provider,
                        "2026-11-06T15:01:40Z",
                        "urn:rutent:provider-role:result",
                        body,
                    ),
                },
            }));
            choice_of(&format!("r-{name}-shortlist"), "2026-11-06T15:00:00Z")
        }
        None => shortlist(),
    };
    let snapshot = net.build();
    // The operator limits the exchange's slippage to 50 basis points, in the bounds the record names.
    let bounds = limited(bounds(1), &snapshot, 50);
    routed(
        &request(
            &format!("r-{name}"),
            round,
            endpoint(json!("hub"), "USDC", 6),
            endpoint(json!("hub"), "EURC", 6),
            "100000000",
        ),
        &snapshot,
        &bounds,
    )
}

// verifies: AC-13, REQ-QTE-10
#[test]
fn route_with_a_segment_over_three_days_has_no_firm_rate_and_names_it() {
    let (record, path) = chain(
        "slow",
        &[("0", 3600), ("0", THREE_DAYS_SECONDS + 1), ("0", 60)],
    );
    let candidate = on(&record, &path_of(&path));
    let slow = vec![time(0, 1, "r1", THREE_DAYS_SECONDS + 1)];
    let firmness_of = firmness(&record, candidate);
    assert_eq!(firmness_of, Firmness::NoFirmRate { slow: slow.clone() });
    assert_eq!(
        may_start(&firmness_of, false),
        Err(NeedsAcceptance { slow })
    );
    assert_eq!(may_start(&firmness_of, true), Ok(()));

    // At exactly three days the segment settles within them: the route is firm and starts without acceptance.
    let (record, path) = chain("firm", &[("0", 3600), ("0", THREE_DAYS_SECONDS), ("0", 60)]);
    let firm = firmness(&record, on(&record, &path_of(&path)));
    assert_eq!(firm, Firmness::Firm);
    assert_eq!(may_start(&firm, false), Ok(()));
}

// verifies: AC-14, REQ-QTE-5
#[test]
fn charge_above_the_formulas_maximum_is_flagged() {
    // Known only after execution: 1% of 249,000,750 would be 2,490,008, but the maximum is 2,000,000.
    let capped = formula("0", "1", "100", Some("2000000"));
    let amount_in = amount("249000750");
    assert_eq!(
        check_charge(&capped, &amount_in, &amount("2000000")),
        Ok(())
    );
    assert_eq!(
        check_charge(&capped, &amount_in, &amount("1500000")),
        Ok(())
    );
    assert_eq!(
        check_charge(&capped, &amount_in, &amount("2000001")),
        Err(ChargeFlag {
            charged: amount("2000001"),
            bound: amount("2000000"),
            against: Bound::Maximum,
            over_by: amount("1"),
        })
    );
    // With no maximum, the formula's own fee bounds the charge: 1 + ceil(10 × 1 / 3) = 5.
    let plain = formula("1", "1", "3", None);
    assert_eq!(formula_fee(&plain, &amount("10")), Some(amount("5")));
    assert_eq!(formula_fee(&capped, &amount_in), Some(amount("2490008")));
    assert_eq!(check_charge(&plain, &amount("10"), &amount("5")), Ok(()));
    assert_eq!(
        check_charge(&plain, &amount("10"), &amount("6")),
        Err(ChargeFlag {
            charged: amount("6"),
            bound: amount("5"),
            against: Bound::Formula,
            over_by: amount("1"),
        })
    );
}

/// The tuple the terms in these tests are for: an exchange `x0` at `hub`, USDC to EURC.
fn x0() -> TupleKey {
    parse(
        "tuple",
        json!({
            "provider": "p-x0",
            "from": endpoint(json!("hub"), "USDC", 6),
            "to": endpoint(json!("hub"), "EURC", 6),
        }),
    )
}

fn terms(stated_at: &str, fee: FeeFormula, slippage: Option<Value>) -> StatedTerms {
    StatedTerms {
        tuple: x0(),
        stated_at: at(stated_at),
        fee,
        slippage: slippage.map(|s| parse("slippage", s)),
    }
}

// verifies: REQ-QTE-6
#[test]
fn formula_changed_after_segment_start_does_not_apply_to_it() {
    let history = vec![
        terms(
            "2026-11-06T10:00:00Z",
            formula("0", "1", "1000", None),
            Some(json!({ "regime": "bounded", "max_bps": 20 })),
        ),
        terms(
            "2026-11-06T12:00:00Z",
            formula("0", "5", "1000", None),
            Some(json!({ "regime": "floating" })),
        ),
    ];
    // Started a second before the change: the formula and slippage stated before it bind, the new ones don't.
    assert_eq!(
        binding(&history, Some(&at("2026-11-06T11:59:59Z"))),
        Binding::Binds(&history[0])
    );
    // Started at the change or after it: the new terms bind.
    assert_eq!(
        binding(&history, Some(&at("2026-11-06T12:00:00Z"))),
        Binding::Binds(&history[1])
    );
    // Not started: everything is a quote, binding nobody.
    assert_eq!(binding(&history, None), Binding::Quote);
    // Started before any terms were stated.
    assert_eq!(
        binding(&history, Some(&at("2026-11-06T09:00:00Z"))),
        Binding::Unstated
    );
}

// verifies: REQ-QTE-15
#[test]
fn held_quote_past_its_validity_is_refused() {
    let record = confirmed();
    let held = hold(record.clone(), at("2026-11-06T15:30:00Z")).unwrap();
    assert_eq!(held_label(&held), Label::Quote);
    assert_eq!(held.valid_until, at("2026-11-06T15:30:00Z"));
    assert_eq!(
        redeem(&held, &at("2026-11-06T15:30:00Z")).map(tuples),
        Ok(vec!["r0".to_owned()])
    );
    assert_eq!(
        redeem(&held, &at("2026-11-06T15:30:00.001Z")),
        Err(HeldQuoteRefused::PastValidity {
            valid_until: at("2026-11-06T15:30:00Z")
        })
    );
    // A validity before the quote was made can't be stated, and a record that chose nothing holds no quote.
    assert_eq!(
        hold(record, at("2026-11-06T15:01:59Z")),
        Err(HoldRefused::ValidityBeforeQuote)
    );
    let (unchosen, _) = chain("unchosen", &[("0", 60)]);
    assert_eq!(
        hold(unchosen, at("2026-11-06T15:30:00Z")),
        Err(HoldRefused::NothingChosen)
    );
}

// verifies: REQ-QTE-9
#[test]
fn a_segment_settling_in_exactly_three_days_is_bound_and_one_second_more_is_free() {
    let within = exchange("three-days", THREE_DAYS_SECONDS, None);
    let candidate = on(&within, &["x0"]);
    assert_eq!(
        commitments(&within, candidate, &within.bounds.slippage_limits),
        vec![(
            time(0, 0, "x0", THREE_DAYS_SECONDS),
            Ok(Commitment::Bound { rate_bps: Some(50) })
        )]
    );
    // Within three days and setting a rate, a tuple the operator set no slippage for is named.
    assert_eq!(
        commitments(&within, candidate, &[]),
        vec![(
            time(0, 0, "x0", THREE_DAYS_SECONDS),
            Err(NoSlippageLimit { tuple: id("x0") })
        )]
    );
    let over = exchange("over-three-days", THREE_DAYS_SECONDS + 1, None);
    assert_eq!(
        commitments(&over, on(&over, &["x0"]), &[]),
        vec![(
            time(0, 0, "x0", THREE_DAYS_SECONDS + 1),
            Ok(Commitment::Free)
        )]
    );
    // A ramp sets no rate: within three days its fees are committed, and no slippage is asked of it.
    let (ramps, path) = chain("ramp-bound", &[("0", 60)]);
    assert_eq!(
        commitments(&ramps, on(&ramps, &path_of(&path)), &[]),
        vec![(
            time(0, 0, "r0", 60),
            Ok(Commitment::Bound { rate_bps: None })
        )]
    );

    // 0.9 less 50 basis points is 0.8955: at it the rate is within, a hair below it is not, and a better rate is.
    let bound = Commitment::Bound { rate_bps: Some(50) };
    assert!(rate_within(&bound, &decimal("0.9"), &decimal("0.8955")));
    assert!(!rate_within(&bound, &decimal("0.9"), &decimal("0.895499")));
    assert!(rate_within(&bound, &decimal("0.9"), &decimal("0.95")));
    // A bound past 10,000 basis points, which no regime states, allows any move and never overflows.
    let past = Commitment::Bound {
        rate_bps: Some(10_001),
    };
    assert!(rate_within(&past, &decimal("0.9"), &decimal("0.0001")));
    let firm = Commitment::Bound { rate_bps: Some(0) };
    assert!(rate_within(&firm, &decimal("0.9"), &decimal("0.9")));
    assert!(!rate_within(&firm, &decimal("0.9"), &decimal("0.8999")));
    assert!(rate_within(
        &Commitment::Free,
        &decimal("0.9"),
        &decimal("0.1")
    ));
}

// verifies: REQ-QTE-9
#[test]
fn an_exchanges_own_tighter_slippage_binds_and_a_floating_one_leaves_the_operators() {
    let cases = [
        (json!({ "regime": "bounded", "max_bps": 20 }), Some(20)),
        (json!({ "regime": "bounded", "max_bps": 80 }), Some(50)),
        (json!({ "regime": "firm" }), Some(0)),
        (json!({ "regime": "floating" }), Some(50)),
    ];
    for (i, (slippage, rate_bps)) in cases.into_iter().enumerate() {
        let record = exchange(&format!("quoted-{i}"), 3600, Some(slippage));
        let chosen = record.chosen().expect("the quote confirms the exchange");
        assert_eq!(
            commitments(&record, chosen, &record.bounds.slippage_limits),
            vec![(time(0, 0, "x0", 3600), Ok(Commitment::Bound { rate_bps }))]
        );
    }
}

/// A change to a quote's figure and its statement.
type Change = fn(&mut Figure, &mut TransferQuote);

/// The fresh quote figure `q-r0` of [`confirmed`], with `change` made to its quote.
fn quote_with(change: impl FnOnce(&mut Figure, &mut TransferQuote)) -> Figure {
    let record = confirmed();
    let mut figure = record
        .snapshot
        .figures
        .iter()
        .find(|f| f.figure_id.as_str() == "q-r0")
        .unwrap()
        .clone();
    let mut copy = figure.clone();
    let FigureValue::FreshQuote { quote, .. } = &mut copy.value else {
        unreachable!()
    };
    let ResultBody::Transfer(result) = &mut quote.statement else {
        unreachable!()
    };
    let TransferOutcome::DryRunAccepted(q) = &mut result.outcome else {
        unreachable!()
    };
    change(&mut figure, q);
    figure.value = copy.value;
    figure
}

// verifies: REQ-QTE-1
#[test]
fn a_quote_lacking_any_element_is_refused() {
    let whole = quote_with(|_, _| {});
    let fields = quote_fields(&whole).expect("a whole quote");
    assert_eq!(fields.provider.as_str(), "p-r0");
    assert_eq!(fields.tuple.as_str(), "r0");
    assert_eq!(fields.amount_in.amount().as_str(), "100000000");
    assert_eq!(fields.amount_out.as_str(), "99500000");
    assert_eq!(fields.fee.amount, "500000");
    assert_eq!(fields.valid_until, at("2026-11-06T15:03:40Z"));
    assert_eq!(fields.voided_by.len(), 2);
    assert_eq!(fields.rate, None);
    assert_eq!(fields.settlement_seconds, 30);

    let lacking: [(QuoteField, Change); 8] = [
        (QuoteField::AmountIn, |_, q| q.amount_in = String::new()),
        (QuoteField::AmountIn, |_, q| q.amount_in = "0".to_owned()),
        (QuoteField::AmountOut, |_, q| {
            q.amount_out = "1.5".to_owned()
        }),
        (QuoteField::Fee, |_, q| q.fee.asset = String::new()),
        (QuoteField::Fee, |_, q| q.fee.amount = String::new()),
        (QuoteField::Validity, |_, q| q.valid_until = String::new()),
        (QuoteField::Validity, |f, _| f.valid_for_seconds = None),
        (QuoteField::VoidConditions, |_, q| q.voided_by.clear()),
    ];
    for (field, change) in lacking {
        assert_eq!(
            quote_fields(&quote_with(change)),
            Err(QuoteRefused::Lacks(field)),
            "{field:?}"
        );
    }

    // Made by another party than the figure's provider, it can't be held against the provider.
    let mut forged = quote_with(|_, _| {});
    if let FigureValue::FreshQuote { quote, .. } = &mut forged.value {
        quote.maker = "p-somebody-else".to_owned();
    }
    assert_eq!(quote_fields(&forged), Err(QuoteRefused::NotAttributable));

    // A price is no quote.
    let record = confirmed();
    let price = record
        .snapshot
        .figures
        .iter()
        .find(|f| f.figure_id.as_str() == "r0-price")
        .unwrap();
    assert_eq!(quote_fields(price), Err(QuoteRefused::NotAQuote));

    // A conversion's quote states its rate and slippage regime.
    let record = exchange("fields", 3600, Some(json!({ "regime": "firm" })));
    let figure = record
        .snapshot
        .figures
        .iter()
        .find(|f| f.figure_id.as_str() == "q-x0")
        .unwrap();
    let fields = quote_fields(figure).expect("a whole conversion quote");
    let (rate, slippage) = fields.rate.expect("a rate");
    assert_eq!(rate, decimal("0.9"));
    assert_eq!(slippage, &parse("slippage", json!({ "regime": "firm" })));
}

// verifies: REQ-QTE-2, REQ-QTE-8
#[test]
fn shown_price_differs_from_the_quotes_by_the_operators_fee_alone() {
    // 100 USDC: r0 takes 100,000 and delivers 99,900,000; r1 takes ceil(99,900) and delivers 99,800,100.
    let (record, path) = chain("shown", &[("0", 60), ("0", 60)]);
    let candidate = on(&record, &path_of(&path));
    // The operator's fee: 100 + ceil(99,800,100 / 1000) = 99,901, on what the providers deliver.
    let operator = formula("100", "1", "1000", None);
    let shown = quoted_price(candidate, &operator).unwrap();
    assert_eq!(shown.label, Label::Quote);
    assert_eq!(shown.amount_in, amount("100000000"));
    let fees: Vec<(u32, &str, &str)> = shown
        .provider_fees
        .iter()
        .map(|f| (f.operation, f.provider.as_str(), f.fee.amount.as_str()))
        .collect();
    assert_eq!(fees, vec![(0, "p-r0", "100000"), (1, "p-r1", "99900")]);
    assert_eq!(shown.providers_out, amount("99800100"));
    assert_eq!(shown.operator_fee, amount("99901"));
    assert_eq!(shown.shown_out, amount("99700199"));
    assert_eq!(shown.difference(), shown.operator_fee);
    assert_eq!(shown.check_shown(&amount("99700199")), Ok(()));
    assert_eq!(
        shown.check_shown(&amount("99700000")),
        Err(UnexplainedDifference {
            shown: amount("99700000"),
            explained: amount("99700199"),
        })
    );
    // A maximum caps the operator's fee but never raises it: the fee shown is its formula's where that is lower.
    let above = formula("100", "1", "1000", Some("150000"));
    assert_eq!(
        quoted_price(candidate, &above).unwrap().operator_fee,
        amount("99901")
    );
    let below = formula("100", "1", "1000", Some("50000"));
    let capped = quoted_price(candidate, &below).unwrap();
    assert_eq!(capped.operator_fee, amount("50000"));
    assert_eq!(capped.shown_out, amount("99750100"));
    // An operator's fee above what the providers deliver can't be shown.
    assert_eq!(
        quoted_price(candidate, &formula("99800101", "0", "1", None)),
        Err(PriceRefused::OperatorFeeAboveArrival {
            operator_fee: amount("99800101"),
            providers_out: amount("99800100"),
        })
    );
}

// verifies: REQ-QTE-9, REQ-QTE-10
#[test]
fn a_segment_whose_settlement_time_is_unknown_is_neither_firm_nor_free() {
    // A record whose snapshot lost the price its operations cite: no record the contract accepts, but one a check
    // may still be handed. Its time is unknown, so it is taken as over three days for firmness and as bound for
    // commitment: neither check fails open.
    let (mut record, path) = chain("unknown", &[("0", 60)]);
    record
        .snapshot
        .figures
        .retain(|f| f.figure_id.as_str() != "r0-price");
    let candidate = on(&record, &path_of(&path)).clone();
    assert_eq!(
        firmness(&record, &candidate),
        Firmness::NoFirmRate {
            slow: vec![time(0, 0, "r0", u64::MAX)]
        }
    );
    assert_eq!(
        commitments(&record, &candidate, &[]),
        vec![(
            time(0, 0, "r0", u64::MAX),
            Ok(Commitment::Bound { rate_bps: None })
        )]
    );
}

// verifies: REQ-QTE-7
#[test]
fn a_price_shown_before_execution_is_a_quote_and_after_it_the_price() {
    let (record, path) = chain("labels", &[("0", 60), ("0", 60)]);
    let candidate = on(&record, &path_of(&path));
    let operator = formula("0", "0", "1", None);
    assert_eq!(
        quoted_price(candidate, &operator).unwrap().label,
        Label::Quote
    );
    let usdc = |amount: &str| rutent_contracts::provider_role::Fee {
        asset: "USDC".to_owned(),
        amount: amount.to_owned(),
    };
    let first = Executed {
        amount_in: amount("100000000"),
        fee: usdc("90000"),
        amount_out: amount("99910000"),
    };
    // One segment executed of two: still a quote.
    assert_eq!(
        executed_price(candidate, std::slice::from_ref(&first), &operator),
        Err(PriceRefused::NotExecuted {
            part: 0,
            operation: 1
        })
    );
    let second = Executed {
        amount_in: amount("99910000"),
        fee: usdc("99910"),
        amount_out: amount("99810090"),
    };
    let price = executed_price(candidate, &[first, second], &operator).unwrap();
    assert_eq!(price.label, Label::Price);
    assert_eq!(price.providers_out, amount("99810090"));
    assert_eq!(price.shown_out, amount("99810090"));
    assert_eq!(price.provider_fees[1].fee, usdc("99910"));
}

// verifies: REQ-QTE-3
#[test]
fn declared_and_actual_settlement_time_are_both_readable_per_segment() {
    let (record, path) = chain("times", &[("0", 3600), ("0", 60)]);
    let times = declared_times(&record, on(&record, &path_of(&path)));
    assert_eq!(times, vec![time(0, 0, "r0", 3600), time(0, 1, "r1", 60)]);
    assert_eq!(
        times[0].measure(&at("2026-11-06T15:00:00Z"), &at("2026-11-06T16:00:01Z")),
        Some(Measured {
            declared_seconds: 3600,
            actual_seconds: 3601,
            over_seconds: 1,
        })
    );
    // Actual time rounds up, so it is never understated.
    assert_eq!(
        times[1].measure(&at("2026-11-06T16:00:01.500Z"), &at("2026-11-06T16:00:30Z")),
        Some(Measured {
            declared_seconds: 60,
            actual_seconds: 29,
            over_seconds: 0,
        })
    );
    assert_eq!(
        times[1].measure(&at("2026-11-06T16:00:30Z"), &at("2026-11-06T16:00:29Z")),
        None
    );
    // Where the operation cites a usable fresh quote, its declared time is the quote's, not the price's.
    let record = confirmed();
    let chosen = record.chosen().unwrap();
    assert_eq!(declared_times(&record, chosen), vec![time(0, 0, "r0", 30)]);
}

// --- Property tests (DEC-18) ------------------------------------------------------------------------------------

/// splitmix64: a small, fixed generator, so the draws are the same on every machine.
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn seeds(count: u64) -> impl Iterator<Item = (u64, Draw)> {
    (0..count).map(|seed| (seed, Draw(0x5EED_0000 + seed)))
}

/// A time on 2026-11-06, `seconds` after midnight.
fn on_the_day(seconds: u64) -> Timestamp {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    at(&format!("2026-11-06T{h:02}:{m:02}:{s:02}Z"))
}

// verifies: REQ-QTE-5, AC-14
#[test]
fn a_charge_at_its_bound_is_never_flagged_and_one_unit_more_always_is() {
    for (seed, mut draw) in seeds(500) {
        let (flat, numerator, denominator) = (
            draw.below(1_000_000),
            draw.below(1_000),
            1 + draw.below(100_000),
        );
        let amount_in = 1 + draw.below(1_000_000_000_000);
        let maximum = (draw.below(3) == 0).then(|| draw.below(10_000_000_000));
        let formula = formula(
            &flat.to_string(),
            &numerator.to_string(),
            &denominator.to_string(),
            maximum.map(|m| m.to_string()).as_deref(),
        );
        let computed = u128::from(flat)
            + (u128::from(amount_in) * u128::from(numerator)).div_ceil(u128::from(denominator));
        let a = amount(&amount_in.to_string());
        assert_eq!(
            formula_fee(&formula, &a),
            Some(amount(&computed.to_string())),
            "seed {seed}"
        );
        let (bound, against) = match maximum {
            Some(m) => (u128::from(m), Bound::Maximum),
            None => (computed, Bound::Formula),
        };
        let under = bound.saturating_sub(u128::from(draw.below(1_000_000)));
        assert_eq!(
            check_charge(&formula, &a, &amount(&bound.to_string())),
            Ok(()),
            "seed {seed}"
        );
        assert_eq!(
            check_charge(&formula, &a, &amount(&under.to_string())),
            Ok(()),
            "seed {seed}"
        );
        assert_eq!(
            check_charge(&formula, &a, &amount(&(bound + 1).to_string())),
            Err(ChargeFlag {
                charged: amount(&(bound + 1).to_string()),
                bound: amount(&bound.to_string()),
                against,
                over_by: amount("1"),
            }),
            "seed {seed}"
        );
    }
}

// verifies: REQ-QTE-10, AC-13
#[test]
fn a_route_is_firm_exactly_when_no_segment_is_over_three_days_and_names_each_that_is() {
    let near = [
        1,
        THREE_DAYS_SECONDS - 1,
        THREE_DAYS_SECONDS,
        THREE_DAYS_SECONDS + 1,
    ];
    for (seed, mut draw) in seeds(120) {
        let hops = 1 + draw.below(4) as usize;
        let seconds: Vec<u64> = (0..hops)
            .map(|_| match draw.below(5) {
                4 => draw.below(2 * THREE_DAYS_SECONDS),
                i => near[i as usize],
            })
            .collect();
        let ramps: Vec<(&str, u64)> = seconds.iter().map(|&s| ("0", s)).collect();
        let (record, path) = chain(&format!("firm-{seed}"), &ramps);
        let firmness_of = firmness(&record, on(&record, &path_of(&path)));
        let slow: Vec<SegmentTime> = seconds
            .iter()
            .enumerate()
            .filter(|(_, s)| **s > THREE_DAYS_SECONDS)
            .map(|(i, &s)| time(0, i as u32, &format!("r{i}"), s))
            .collect();
        if slow.is_empty() {
            assert_eq!(firmness_of, Firmness::Firm, "seed {seed}");
            assert_eq!(may_start(&firmness_of, false), Ok(()), "seed {seed}");
        } else {
            assert_eq!(
                firmness_of,
                Firmness::NoFirmRate { slow: slow.clone() },
                "seed {seed}"
            );
            assert_eq!(
                may_start(&firmness_of, false),
                Err(NeedsAcceptance { slow }),
                "seed {seed}"
            );
        }
        assert_eq!(may_start(&firmness_of, true), Ok(()), "seed {seed}");
    }
}

// verifies: REQ-QTE-6
#[test]
fn no_terms_stated_after_a_segment_starts_ever_bind_it() {
    for (seed, mut draw) in seeds(500) {
        let day = 86_400;
        let history: Vec<StatedTerms> = (0..1 + draw.below(6))
            .map(|i| StatedTerms {
                tuple: x0(),
                stated_at: on_the_day(draw.below(day)),
                fee: formula(&i.to_string(), "1", "1000", None),
                slippage: None,
            })
            .collect();
        let start = draw.below(day);
        let started = on_the_day(start);
        let expected = history
            .iter()
            .enumerate()
            .filter(|(_, t)| t.stated_at.unix_nanos() <= started.unix_nanos())
            .max_by_key(|(i, t)| (t.stated_at.unix_nanos(), *i))
            .map(|(_, t)| t);
        let bound = binding(&history, Some(&started));
        match expected {
            Some(terms) => assert_eq!(bound, Binding::Binds(terms), "seed {seed}"),
            None => assert_eq!(bound, Binding::Unstated, "seed {seed}"),
        }
        assert_eq!(binding(&history, None), Binding::Quote, "seed {seed}");
        // Terms stated after the start, appended in any number, change nothing that binds it.
        let mut later = history.clone();
        if start + 1 < day {
            for _ in 0..1 + draw.below(3) {
                later.push(StatedTerms {
                    tuple: x0(),
                    stated_at: on_the_day(start + 1 + draw.below(day - start - 1)),
                    fee: formula("999999", "999", "1000", None),
                    slippage: Some(parse("slippage", json!({ "regime": "floating" }))),
                });
            }
        }
        let rebound = binding(&later, Some(&started));
        let same = match (bound, rebound) {
            (Binding::Binds(a), Binding::Binds(b)) => a == b,
            (a, b) => a == b,
        };
        assert!(same, "seed {seed}: {bound:?} became {rebound:?}");
    }
}

// verifies: REQ-QTE-2, REQ-QTE-8
#[test]
fn what_is_shown_differs_from_the_providers_terms_by_the_operators_fee_alone() {
    for (seed, mut draw) in seeds(150) {
        let hops = 1 + draw.below(3) as usize;
        let flats: Vec<String> = (0..hops).map(|_| draw.below(100_000).to_string()).collect();
        let ramps: Vec<(&str, u64)> = flats.iter().map(|f| (f.as_str(), 60)).collect();
        let (record, path) = chain(&format!("shown-{seed}"), &ramps);
        let candidate = on(&record, &path_of(&path));
        let arrives: u128 = candidate.parts[0]
            .operations
            .last()
            .amount_out
            .as_str()
            .parse()
            .unwrap();
        let (flat, numerator, denominator) = (
            u128::from(draw.below(1_000)),
            u128::from(draw.below(100)),
            u128::from(1 + draw.below(10_000)),
        );
        let maximum = (draw.below(3) == 0).then(|| u128::from(draw.below(200_000)));
        let operator = formula(
            &flat.to_string(),
            &numerator.to_string(),
            &denominator.to_string(),
            maximum.map(|m| m.to_string()).as_deref(),
        );
        // The formula's fee, capped by its maximum where it gives one, never raised to it.
        let by_formula = flat + (arrives * numerator).div_ceil(denominator);
        let operator_fee = maximum.map_or(by_formula, |m| by_formula.min(m));
        let shown = quoted_price(candidate, &operator);
        if operator_fee > arrives {
            assert!(
                matches!(shown, Err(PriceRefused::OperatorFeeAboveArrival { .. })),
                "seed {seed}"
            );
            continue;
        }
        let shown = shown.unwrap();
        assert_eq!(shown.label, Label::Quote, "seed {seed}");
        assert_eq!(
            shown.providers_out,
            amount(&arrives.to_string()),
            "seed {seed}"
        );
        assert_eq!(
            shown.operator_fee,
            amount(&operator_fee.to_string()),
            "seed {seed}"
        );
        assert_eq!(shown.difference(), shown.operator_fee, "seed {seed}");
        assert_eq!(
            shown.shown_out,
            amount(&(arrives - operator_fee).to_string()),
            "seed {seed}"
        );
        assert_eq!(shown.check_shown(&shown.shown_out), Ok(()), "seed {seed}");
        assert_eq!(shown.provider_fees.len(), hops, "seed {seed}");
    }
}

// verifies: REQ-QTE-15
#[test]
fn a_held_quote_is_refused_exactly_past_its_validity() {
    let record = confirmed();
    let made = 15 * 3600 + 2 * 60;
    for (seed, mut draw) in seeds(500) {
        let until = made + draw.below(86_400 - made);
        let now = draw.below(86_400);
        let held = hold(record.clone(), on_the_day(until)).unwrap();
        let redeemed = redeem(&held, &on_the_day(now));
        if now <= until {
            assert_eq!(redeemed, Ok(record.chosen().unwrap()), "seed {seed}");
        } else {
            assert_eq!(
                redeemed,
                Err(HeldQuoteRefused::PastValidity {
                    valid_until: on_the_day(until)
                }),
                "seed {seed}"
            );
        }
        assert_eq!(held_label(&held), Label::Quote, "seed {seed}");
    }
}

/// `digits / 10^places` as a decimal, with no trailing zeros, as the contract writes one.
fn as_decimal(digits: u128, places: usize) -> Decimal {
    let text = format!("{digits:0>width$}", width = places + 1);
    let (whole, fraction) = text.split_at(text.len() - places);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        decimal(whole)
    } else {
        decimal(&format!("{whole}.{fraction}"))
    }
}

// verifies: REQ-QTE-9
#[test]
fn a_rate_at_its_committed_slippage_is_within_and_one_just_past_it_is_not() {
    for (seed, mut draw) in seeds(500) {
        let quoted = 1 + u128::from(draw.below(10_000_000));
        let b = u16::try_from(draw.below(10_000)).unwrap();
        let commitment = Commitment::Bound { rate_bps: Some(b) };
        let q = as_decimal(quoted, 4);
        // quoted / 10^4 × (10000 − b) / 10^4, exactly, at 8 places, and a step of 10^-12 below it.
        let edge = quoted * u128::from(10_000 - b);
        assert!(
            rate_within(&commitment, &q, &as_decimal(edge, 8)),
            "seed {seed}"
        );
        assert!(
            !rate_within(&commitment, &q, &as_decimal(edge * 10_000 - 1, 12)),
            "seed {seed}"
        );
        assert!(
            rate_within(&Commitment::Free, &q, &as_decimal(1, 12)),
            "seed {seed}"
        );
    }
}
