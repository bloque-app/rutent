//! The quote shown (`T-020-07`): the operator's fee by its formula and shown apart from the quotes' sum, every other
//! difference named; each segment's slippage limit shown; a route with a segment over three days shown without a
//! firm rate, naming it, and started only on acceptance; a quote before execution and the price after it; terms
//! binding from a segment's start; a held quote past its validity starting nothing; and a charge above its formula
//! raised to the operator. Every log a test writes is replayed as the log itself would admit it.

mod intake_support;
mod quote_support;
mod rounds_support;
mod snapshot_support;

use intake_support::*;
use quote_support::{
    EXCHANGE, Leg, Net, THREE_DAYS, bounds, chosen, exchange_limit, exchange_route, fee, request,
    reserve_route, routed,
};
use rounds_support::customer;
use rutent_contracts::core_internal::calls::GeneralEntry;
use rutent_contracts::core_internal::calls::{Delivery, Evidence, Log, Operator};
use rutent_contracts::core_internal::{
    About, Attribution, Difference, Entry, OrderRefusalClass, Outcome, Raise, Receipt, ShapeId,
    ShownAs, SignedWith, StatedEntry, StatedKind, StatedOutcome, StatedStatus, StatedTimeSource,
    StatedWhat, StatedWhen, What,
};
use rutent_contracts::provider_role::{Fee, RESULT_SHAPE, SignatureScheme};
use rutent_contracts::router_api::{Amount, FeeFormula, Id, StatedTerms, Timestamp, TupleKey};
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::quote::{
    ChargeDesk, Charged, Charges, Checked, HeldTo, HoldError, Holds, NotShown, PriceError, Priced,
    Prices, SlowSegment, Terms, digest, executed, show,
};
use rutent_prov::catalogue::Instant;
use rutent_router::quote::{Binding, Executed, HeldQuoteRefused};
use serde_json::json;
use snapshot_support::{
    RESERVE, core_actor, deployment, deposit_tuple, envelope, id, sign, statements_key,
};

/// The log replays as the log itself would admit it.
fn assert_admitted(env: &Env) {
    if let Err((place, why)) = env.admitted() {
        panic!("the log refuses entry {place}: {why}");
    }
}

fn amount(text: &str) -> Amount {
    Amount::new(text).unwrap()
}

fn at(text: &str) -> Timestamp {
    Timestamp::new(text).unwrap()
}

fn formula(flat: &str, numerator: &str, denominator: &str, maximum: Option<&str>) -> FeeFormula {
    let mut formula = fee(flat, numerator, denominator);
    if let Some(maximum) = maximum {
        formula["maximum"] = json!(maximum);
    }
    serde_json::from_value(formula).unwrap()
}

/// The quote the customer was told for `order`, and its digest.
fn quoted(
    env: &mut Env,
    sim: &mut rounds_support::Sim,
    order: &Id,
) -> (
    rutent_core_orch::intake::Quote,
    rutent_contracts::module_interface::Digest,
) {
    match env.quote(sim, order).expect("the routing runs") {
        Told::Exists {
            status: Status::Quoted { quote, digest },
            ..
        } => (*quote, digest),
        other => panic!("no quote: {other:?}"),
    }
}

/// Every `shown` entry the log holds: its order, digest and label.
fn shown_entries(env: &Env) -> Vec<(Id, rutent_contracts::module_interface::Digest, ShownAs)> {
    env.book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::Shown {
                order,
                shown,
                shown_as,
            } => Some((order.clone(), shown.clone(), *shown_as)),
            _ => None,
        })
        .collect()
}

// REQ-QTE-2 ---------------------------------------------------------------------------------------------------------

/// The quote shown names each provider's fee and what the providers' terms deliver, the sum of the quotes, and the
/// operator's own fee apart from them, by its formula from the bounds the route ran under; what the customer is
/// shown differs from the quotes' sum by that fee alone, and the `shown` entry names the digest of all of it.
// verifies: REQ-QTE-2
#[test]
fn operators_fee_is_shown_apart_from_the_quotes_sum() {
    // Over the providers' own fees: 3 and 2 on 1 000, net, and an operator's 5 + 1%.
    let record = reserve_route(
        1_000,
        &Leg {
            answer_fee: 3,
            ..Leg::free("USDC")
        },
        &Leg {
            answer_fee: 2,
            ..Leg::free("USDC")
        },
        fee("5", "1", "100"),
        None,
    );
    let price = show(&record, chosen(&record), None).expect("the route is shown");
    let fees: Vec<&str> = price
        .segments
        .iter()
        .map(|s| s.fee.amount.as_str())
        .collect();
    assert_eq!(
        fees,
        ["3", "2"],
        "each provider's fee, as its quote states it"
    );
    assert_eq!(price.amount_in, amount("1000"));
    assert_eq!(
        price.providers_out,
        amount("995"),
        "the quotes' sum, composed"
    );
    assert_eq!(price.operator_fee.formula, record.bounds.operator_fee);
    // 5 + ceil(995 / 100) = 15, on what arrives.
    assert_eq!(price.operator_fee.amount, amount("15"));
    assert!(
        price.within.is_empty(),
        "no charge is reported within an amount"
    );
    assert_eq!(price.shown_out, amount("980"));
    assert_eq!(
        price.difference,
        amount("15"),
        "the operator's fee, and nothing else"
    );
    assert_eq!(price.shown_as, ShownAs::Quote);

    // Through intake: the quote the customer is told, and the `shown` entry naming it.
    let mut env = Env::new();
    env.bounds.operator_fee = formula("5", "1", "100", None);
    let mut sim = env.sim();
    let request = order_request("client-req-fee", customer());
    let order = order_of(&env.open(&mut sim, &request).expect("the request opens"));
    let (quote, told_digest) = quoted(&mut env, &mut sim, &order);
    let price = &quote.price;
    assert_eq!(price.providers_out, amount("1000"));
    assert_eq!(
        quote.delivers, price.providers_out,
        "what the route delivers is the quotes' sum"
    );
    assert_eq!(price.operator_fee.formula, env.bounds.operator_fee);
    assert_eq!(price.operator_fee.amount, amount("15"));
    assert_eq!(price.shown_out, amount("985"));
    assert_eq!(price.difference, price.operator_fee.amount);
    assert_eq!(
        shown_entries(&env),
        vec![(order.clone(), told_digest.clone(), ShownAs::Quote)]
    );
    assert_eq!(
        digest(&quote),
        Some(told_digest),
        "the entry names what was shown, its fee included"
    );
    assert_admitted(&env);
}

/// A part's last segment whose tuple reports gross holds its charge within what it gives out: the charge is its own
/// line, taken off what arrives before the operator's fee, so the difference is named in full.
// verifies: REQ-QTE-2, REQ-CAT-11
#[test]
fn charge_reported_within_what_a_segment_gives_out_is_a_line_of_its_own() {
    let record = reserve_route(
        1_000,
        &Leg::free("USDC"),
        &Leg {
            answer_fee: 4,
            gross: true,
            ..Leg::free("USDC")
        },
        fee("0", "1", "100"),
        None,
    );
    let price = show(&record, chosen(&record), None).expect("the route is shown");
    assert_eq!(
        price.providers_out,
        amount("1000"),
        "the deposit says it gives out 1 000, its charge within"
    );
    assert_eq!(price.within.len(), 1);
    assert_eq!(price.within[0].charge, amount("4"));
    assert_eq!(price.arrives, amount("996"));
    // ceil(996 / 100) = 10.
    assert_eq!(price.operator_fee.amount, amount("10"));
    assert_eq!(price.shown_out, amount("986"));
    assert_eq!(
        price.difference,
        amount("14"),
        "the charge within and the operator's fee"
    );
}

/// A segment that reports gross, before another, hands on less than its quote gives out, and the next segment was
/// quoted at what it gives out: the route can't deliver as composed, so it is never shown (`T-020-08`'s hand-off).
// verifies: REQ-QTE-2, REQ-ORD-21
#[test]
fn route_that_cant_hand_on_what_its_next_segment_was_quoted_at_is_not_shown() {
    let record = reserve_route(
        1_000,
        &Leg {
            answer_fee: 4,
            gross: true,
            ..Leg::free("USDC")
        },
        &Leg::free("USDC"),
        fee("0", "0", "1"),
        None,
    );
    assert_eq!(
        show(&record, chosen(&record), None),
        Err(NotShown::Undeliverable {
            part: 0,
            operation: 0
        })
    );
    // And one the router kept at the floor on what the deposit says it gives out, 1 000 less the operator's 10, is
    // below it once the deposit's charge within is taken off: never shown either.
    let record = reserve_route(
        1_000,
        &Leg::free("USDC"),
        &Leg {
            answer_fee: 4,
            gross: true,
            ..Leg::free("USDC")
        },
        fee("0", "1", "100"),
        Some(990),
    );
    assert_eq!(
        show(&record, chosen(&record), Some(&amount("990"))),
        Err(NotShown::BelowFloor)
    );
}

/// A segment that keeps its asset gives out no more than it takes in less its charge, as `segments::judge` counts it.
/// The reviewer's sequence: USDC to EURC through an exchange, the customer's floor 950 000; the EURC deposit takes in
/// 900 000, net, with no fee, and its dry run claims 950 000 out. The router keeps it at the floor on that claim, and
/// Core never shows it.
// verifies: REQ-QTE-2, INV-1
#[test]
fn segment_claiming_more_out_than_it_takes_in_is_never_shown() {
    let mut net = Net::new(1_000_000);
    let out = net.withdrawal(&Leg::free("USDC"));
    let converted = net.exchange(out, 3_600, json!({ "regime": "bounded", "max_bps": 20 }));
    assert_eq!(converted, 900_000);
    net.deposit(
        &Leg {
            claims_out: Some(950_000),
            ..Leg::free("EURC")
        },
        converted,
    );
    let record = routed(
        &request("USDC", "EURC", 1_000_000, Some(950_000)),
        &net.build(),
        &bounds(fee("0", "0", "1"), &[exchange_limit(50)]),
    );
    assert_eq!(
        show(&record, chosen(&record), Some(&amount("950000"))),
        Err(NotShown::Undeliverable {
            part: 0,
            operation: 2
        })
    );
}

/// Through intake: a route the router chose but can't deliver as composed (the reserve reports gross and charges 4
/// on each segment, so its withdrawal hands on less than its deposit was quoted to take in) is never shown. The
/// customer is told the order can't proceed now, one `order_refusal` of that class is recorded, and no `shown`.
// verifies: REQ-QTE-2, REQ-ORD-18
#[test]
fn route_intake_cant_deliver_is_refused_and_never_shown() {
    let mut env = Env::new();
    let mut sim = env.sim();
    sim.fees.insert(RESERVE.into(), 4);
    let request = order_request("client-req-undeliverable", customer());
    let order = order_of(&env.open(&mut sim, &request).expect("the request opens"));
    let told = env.quote(&mut sim, &order).expect("the routing runs");
    assert!(
        matches!(
            told,
            Told::Exists {
                status: Status::CantProceed {
                    class: OrderRefusalClass::NoRouteRightNow
                },
                ..
            }
        ),
        "told {told:?}"
    );
    let refusals: Vec<OrderRefusalClass> = env
        .refusals()
        .into_iter()
        .filter_map(|w| match w {
            What::OrderRefusal {
                order: Some(o),
                class,
                ..
            } if o == order => Some(class),
            _ => None,
        })
        .collect();
    assert_eq!(refusals, vec![OrderRefusalClass::NoRouteRightNow]);
    assert!(shown_entries(&env).is_empty(), "nothing is shown");
    assert!(env.started().is_empty());
    assert_admitted(&env);
}

// REQ-QTE-9 ---------------------------------------------------------------------------------------------------------

/// A segment settling within three days that sets a rate is shown held to the tighter of its quote's own slippage
/// and the operator's limit for its tuple, the limit the record's bounds name; one past three days is shown free; a
/// segment that sets no rate is committed with no rate; and one the bounds set no limit for is never shown.
// verifies: REQ-QTE-9
#[test]
fn segments_slippage_limit_is_recorded_and_shown() {
    let exchange_of = |price: &rutent_core_orch::quote::Shown| {
        price
            .segments
            .iter()
            .find(|s| s.provider.as_str() == EXCHANGE)
            .cloned()
            .expect("the exchange is shown")
    };
    // The quote allows 80, the operator 50: 50 binds.
    let record = exchange_route(
        1_000_000,
        3_600,
        json!({ "regime": "bounded", "max_bps": 80 }),
        &[exchange_limit(50)],
    );
    assert_eq!(
        record.bounds.slippage_limits[0].max_bps.get(),
        50,
        "the record names the limit"
    );
    let price = show(&record, chosen(&record), None).expect("the route is shown");
    assert_eq!(
        exchange_of(&price).held_to,
        HeldTo::Committed {
            rate_bps: Some(50),
            operator_limit_bps: Some(50)
        }
    );
    for reserve in price
        .segments
        .iter()
        .filter(|s| s.provider.as_str() != EXCHANGE)
    {
        assert_eq!(
            reserve.held_to,
            HeldTo::Committed {
                rate_bps: None,
                operator_limit_bps: None
            }
        );
    }
    // The quote's own 20 is tighter.
    let record = exchange_route(
        1_000_000,
        3_600,
        json!({ "regime": "bounded", "max_bps": 20 }),
        &[exchange_limit(50)],
    );
    let price = show(&record, chosen(&record), None).expect("the route is shown");
    assert_eq!(
        exchange_of(&price).held_to,
        HeldTo::Committed {
            rate_bps: Some(20),
            operator_limit_bps: Some(50)
        }
    );
    // Exactly three days is within them; one second more is free of them.
    let record = exchange_route(
        1_000_000,
        THREE_DAYS,
        json!({ "regime": "bounded", "max_bps": 80 }),
        &[exchange_limit(50)],
    );
    let price = show(&record, chosen(&record), None).expect("the route is shown");
    assert!(matches!(
        exchange_of(&price).held_to,
        HeldTo::Committed {
            rate_bps: Some(50),
            ..
        }
    ));
    let record = exchange_route(
        1_000_000,
        THREE_DAYS + 1,
        json!({ "regime": "bounded", "max_bps": 80 }),
        &[exchange_limit(50)],
    );
    let price = show(&record, chosen(&record), None).expect("the route is shown");
    assert_eq!(exchange_of(&price).held_to, HeldTo::Free);
    // A record whose bounds lost the limit, built field by field: never shown.
    let mut record = exchange_route(
        1_000_000,
        3_600,
        json!({ "regime": "bounded", "max_bps": 80 }),
        &[exchange_limit(50)],
    );
    let candidate = chosen(&record).clone();
    record.bounds.slippage_limits.clear();
    assert_eq!(
        show(&record, &candidate, None),
        Err(NotShown::NoSlippageLimit {
            part: 0,
            operation: 1
        })
    );
}

// REQ-QTE-10, AC-13 -------------------------------------------------------------------------------------------------

/// A route whose deposit declares four days is shown as a quote without a firm rate, naming that segment, recorded
/// so; nothing starts until the customer accepts that quote, and then the order starts once.
// verifies: AC-13, REQ-QTE-10
#[test]
fn route_with_a_segment_over_three_days_starts_only_after_acceptance() {
    let mut env = Env::new();
    let four_days = 4 * 86_400;
    for price in env.prices.iter_mut() {
        if price.tuple == deposit_tuple(RESERVE) {
            price.settlement_seconds = four_days.try_into().unwrap();
        }
    }
    let mut sim = env.sim();
    let request = order_request("client-req-slow", customer());
    let order = order_of(&env.open(&mut sim, &request).expect("the request opens"));
    let (quote, told_digest) = quoted(&mut env, &mut sim, &order);
    assert_eq!(quote.shown_as, ShownAs::QuoteWithoutFirmRate);
    assert_eq!(quote.price.shown_as, ShownAs::QuoteWithoutFirmRate);
    assert_eq!(
        quote.price.no_firm_rate.len(),
        1,
        "the slow segment is named"
    );
    let SlowSegment {
        part,
        operation,
        provider,
        declared_seconds,
        ..
    } = &quote.price.no_firm_rate[0];
    assert_eq!((*part, *operation, provider.as_str()), (0, 1, RESERVE));
    assert_eq!(*declared_seconds, Some(four_days));
    assert_eq!(quote.price.segments[1].held_to, HeldTo::Free);
    assert_eq!(
        shown_entries(&env),
        vec![(
            order.clone(),
            told_digest.clone(),
            ShownAs::QuoteWithoutFirmRate
        )]
    );
    assert!(
        env.started().is_empty(),
        "nothing starts before the customer accepts"
    );

    let told = env
        .accept(&mut sim, &order, &customer(), &told_digest)
        .expect("the acceptance runs");
    assert!(matches!(
        told,
        Told::Exists {
            status: Status::Accepted { .. },
            ..
        }
    ));
    assert_eq!(env.started(), vec![order.as_str().to_owned()]);
    let acceptance = env.acceptances();
    assert!(matches!(&acceptance[0].what, What::Acceptance { quote, .. } if quote == &told_digest));
    assert_admitted(&env);
}

// REQ-QTE-7 ---------------------------------------------------------------------------------------------------------

/// Before every segment has executed, what is shown is a quote and no price is given; once each has, the price is
/// what they did, labelled the price, and recorded as a `shown` entry of `price`, once.
// verifies: REQ-QTE-7
#[test]
fn price_before_execution_is_labelled_a_quote_and_the_executed_one_the_price() {
    let record = reserve_route(
        1_000,
        &Leg {
            answer_fee: 3,
            ..Leg::free("USDC")
        },
        &Leg {
            answer_fee: 2,
            ..Leg::free("USDC")
        },
        fee("5", "1", "100"),
        None,
    );
    let candidate = chosen(&record);
    let quote = show(&record, candidate, None).expect("the route is shown");
    assert_eq!(quote.shown_as, ShownAs::Quote);
    let usdc = |n: &str| Fee {
        asset: "USDC".into(),
        amount: n.into(),
        billed_separately: None,
    };
    let withdrew = Executed {
        amount_in: amount("1000"),
        fee: usdc("4"),
        amount_out: amount("996"),
    };
    let deposited = Executed {
        amount_in: amount("996"),
        fee: usdc("2"),
        amount_out: amount("994"),
    };
    assert_eq!(
        executed(&record, candidate, std::slice::from_ref(&withdrew)),
        Err(NotShown::NotExecuted {
            part: 0,
            operation: 1
        }),
        "until the deposit executes, there is no price"
    );
    let price = executed(&record, candidate, &[withdrew, deposited]).expect("both executed");
    assert_eq!(price.shown_as, ShownAs::Price);
    assert_eq!(
        price.providers_out,
        amount("994"),
        "what they did, not what they quoted"
    );
    // 5 + ceil(994 / 100) = 15.
    assert_eq!(price.operator_fee.amount, amount("15"));
    assert_eq!(price.shown_out, amount("979"));

    let mut env = Env::new();
    let mut prices = Prices::new();
    let priced = Priced {
        order: id("order-priced"),
        decision_record: record.record_id().clone(),
        candidate: candidate.candidate_id.clone(),
        price,
    };
    let (named, _) = prices
        .report(&mut env.log, &*env.clock, &core_actor(), &priced)
        .expect("the price is reported");
    assert_eq!(digest(&priced), Some(named.clone()));
    assert_eq!(
        shown_entries(&env),
        vec![(id("order-priced"), named.clone(), ShownAs::Price)]
    );
    prices
        .report(&mut env.log, &*env.clock, &core_actor(), &priced)
        .expect("reported again");
    assert_eq!(shown_entries(&env).len(), 1, "reported once");
    // A quote is never reported as the price.
    let as_quote = Priced {
        order: id("order-quoted"),
        price: quote,
        ..priced
    };
    assert_eq!(
        prices.report(&mut env.log, &*env.clock, &core_actor(), &as_quote),
        Err(PriceError::NotThePrice)
    );
    assert_admitted(&env);
}

// REQ-QTE-6 ---------------------------------------------------------------------------------------------------------

/// The provider's terms for a tuple bind a segment from its start: the last stated at or before it, whatever order
/// they were kept in; terms stated after it never apply to it, and before it starts nothing binds. A charge on the
/// segment is checked against the terms that bound it, not the later, looser ones.
// verifies: REQ-QTE-6
#[test]
fn terms_stated_after_a_segment_started_do_not_apply_to_it() {
    let tuple = deposit_tuple(RESERVE);
    let stated = |when: &str, maximum: &str| StatedTerms {
        tuple: tuple.clone(),
        stated_at: at(when),
        fee: formula("0", "1", "1000", Some(maximum)),
        slippage: None,
    };
    let mut terms = Terms::new();
    assert!(terms.state(stated("1970-01-01T00:20:00Z", "1000")));
    let started = at("1970-01-01T00:25:00Z");
    assert!(
        terms.state(stated("1970-01-01T00:30:00Z", "5000")),
        "stated after the start"
    );
    assert!(
        terms.state(stated("1970-01-01T00:24:00Z", "1200")),
        "stated before it, kept later"
    );
    assert!(
        !terms.state(stated("1970-01-01T00:24:00Z", "1200")),
        "the same terms are kept once"
    );
    assert_eq!(
        terms.binding(&tuple, None),
        Binding::Quote,
        "before its start, a quote"
    );
    match terms.binding(&tuple, Some(&started)) {
        Binding::Binds(bound) => {
            assert_eq!(bound.stated_at, at("1970-01-01T00:24:00Z"));
            assert_eq!(bound.fee.maximum, Some(amount("1200")));
        }
        other => panic!("nothing binds: {other:?}"),
    }

    // A charge of 2 000: within the terms stated after the start, above those that bound it.
    let mut env = Env::new();
    let result = stated_result(&mut env, "seg-terms", 2_000);
    let mut channel = Channel::delivering();
    let mut charges = Charges::new();
    let checked = charges
        .check(
            desk(&mut env, &terms, &mut channel),
            &Charged {
                order: id("order-terms"),
                segment: id("seg-terms"),
                provider: id(RESERVE),
                tuple: tuple.clone(),
                started_at: started,
                amount_in: amount("1000000"),
                charged: amount("2000"),
                advertised: None,
                result,
            },
        )
        .expect("the charge is checked");
    let Checked::Flagged(flagged) = checked else {
        panic!("the charge above the terms that bound it isn't flagged");
    };
    assert!(matches!(
        &flagged.raise.difference,
        Difference::Charge { maximum, .. } if maximum == &amount("1200")
    ));
    assert_admitted(&env);
}

// REQ-QTE-15 --------------------------------------------------------------------------------------------------------

/// A reserve holds the quote of a choice round for an order until a validity it states, within its figures'
/// validity: held, it is a quote, never the price; at its validity its route may still start; past it, it starts
/// nothing. Only the reserve the route starts at holds it, and a validity before the quote is refused.
// verifies: REQ-QTE-15
#[test]
fn held_quote_past_its_validity_starts_nothing() {
    let record = reserve_route(
        1_000,
        &Leg::free("USDC"),
        &Leg::free("USDC"),
        fee("0", "0", "1"),
        None,
    );
    let candidate = chosen(&record).clone();
    // The dry runs were 20 seconds old at 15:02:00, valid for 120: usable until 15:03:40. Held until 15:03:00.
    let until = at("2026-11-06T15:03:00Z");
    let (reserve, order) = (id(quote_support::RESERVE), id("order-held"));
    let mut holds = Holds::new();
    let held = holds
        .hold(
            &id("hold-1"),
            &reserve,
            &order,
            record.clone(),
            until.clone(),
        )
        .expect("the reserve holds its quote");
    assert_eq!(held.shown.shown_as, ShownAs::Quote, "held, it is a quote");
    assert_eq!(
        held.quote.valid_until, until,
        "within its figures' validity, as stated"
    );

    let (route, shown) = holds
        .redeem(&id("hold-1"), &reserve, &order, &until)
        .expect("at its validity, it is still usable");
    assert_eq!(route, &candidate);
    assert_eq!(shown.shown_as, ShownAs::Quote);
    assert_eq!(
        holds
            .redeem(&id("hold-1"), &reserve, &order, &at("2026-11-06T15:03:01Z"))
            .map(|(c, _)| c.clone()),
        Err(HoldError::Redeem(HeldQuoteRefused::PastValidity {
            valid_until: until.clone()
        })),
        "past its validity, it starts nothing"
    );

    assert_eq!(
        holds
            .hold(
                &id("hold-2"),
                &id("p-other"),
                &order,
                record.clone(),
                until.clone()
            )
            .map(|h| h.reserve.clone()),
        Err(HoldError::NotTheReserve)
    );
    assert!(matches!(
        holds.hold(
            &id("hold-3"),
            &reserve,
            &order,
            record.clone(),
            at("2026-11-06T15:01:00Z")
        ),
        Err(HoldError::Refused(_))
    ));
    assert!(matches!(
        holds.hold(&id("hold-1"), &reserve, &order, record, until),
        Err(HoldError::AlreadyHeld)
    ));
    assert!(
        holds.drop_hold(&id("hold-1")),
        "dropped, with no effect beyond the hold"
    );
    assert!(matches!(
        holds.redeem(&id("hold-1"), &reserve, &order, &at("2026-11-06T15:02:30Z")),
        Err(HoldError::NoSuchHold)
    ));
}

/// A hold stated for longer than the figures its route stands on is held no longer than they are usable: until the
/// dry runs' expiry, 15:03:40, and past it the route starts nothing, though the reserve asked for 15:30.
// verifies: REQ-QTE-15, REQ-QTE-4
#[test]
fn long_hold_is_capped_at_its_figures_validity() {
    let record = reserve_route(
        1_000,
        &Leg::free("USDC"),
        &Leg::free("USDC"),
        fee("0", "0", "1"),
        None,
    );
    let (reserve, order) = (id(quote_support::RESERVE), id("order-held"));
    let mut holds = Holds::new();
    let held = holds
        .hold(
            &id("hold-long"),
            &reserve,
            &order,
            record,
            at("2026-11-06T15:30:00Z"),
        )
        .expect("held, capped");
    assert_eq!(held.quote.valid_until, at("2026-11-06T15:03:40Z"));
    assert!(
        holds
            .redeem(
                &id("hold-long"),
                &reserve,
                &order,
                &at("2026-11-06T15:03:40Z")
            )
            .is_ok()
    );
    assert!(matches!(
        holds.redeem(
            &id("hold-long"),
            &reserve,
            &order,
            &at("2026-11-06T15:10:00Z")
        ),
        Err(HoldError::Redeem(HeldQuoteRefused::PastValidity { .. }))
    ));
}

/// A held quote is redeemed only by the reserve that holds it, for the order it is held for: another reserve, or the
/// same reserve for another order, is refused within its validity.
// verifies: REQ-QTE-15
#[test]
fn held_quote_is_redeemed_only_by_its_reserve_for_its_order() {
    let record = reserve_route(
        1_000,
        &Leg::free("USDC"),
        &Leg::free("USDC"),
        fee("0", "0", "1"),
        None,
    );
    let (reserve, order) = (id(quote_support::RESERVE), id("order-held"));
    let now = at("2026-11-06T15:02:30Z");
    let mut holds = Holds::new();
    holds
        .hold(
            &id("hold-1"),
            &reserve,
            &order,
            record,
            at("2026-11-06T15:03:00Z"),
        )
        .expect("held");
    assert!(holds.redeem(&id("hold-1"), &reserve, &order, &now).is_ok());
    assert!(matches!(
        holds.redeem(&id("hold-1"), &id("p-other"), &order, &now),
        Err(HoldError::NotTheHolder)
    ));
    assert!(matches!(
        holds.redeem(&id("hold-1"), &reserve, &id("order-other"), &now),
        Err(HoldError::NotTheHolder)
    ));
}

// REQ-QTE-5, AC-14 --------------------------------------------------------------------------------------------------

/// A charge known only after execution, above its formula's maximum, is raised to the operator as a `charge`
/// difference resting on the result's stated entry, and the raise recorded `done`; checked again, it isn't raised
/// again; a charge at the maximum raises nothing; and a raise that doesn't reach the operator is recorded `failed`.
// verifies: AC-14, REQ-QTE-5
#[test]
fn charge_above_the_formulas_maximum_is_flagged_to_the_operator() {
    let mut env = Env::new();
    let terms = Terms::new();
    let mut charges = Charges::new();
    let mut channel = Channel::delivering();
    let advertised = formula("0", "1", "1000", Some("1000"));
    let charged = |segment: &str, charge: &str, result: Receipt| Charged {
        order: id("order-charged"),
        segment: id(segment),
        provider: id(RESERVE),
        tuple: deposit_tuple(RESERVE),
        started_at: at("1970-01-01T00:30:00Z"),
        amount_in: amount("1000000"),
        charged: amount(charge),
        advertised: Some(advertised.clone()),
        result,
    };

    let result = stated_result(&mut env, "seg-over", 1_001);
    let over = charged("seg-over", "1001", result.clone());
    let Checked::Flagged(flagged) = charges
        .check(desk(&mut env, &terms, &mut channel), &over)
        .expect("the charge is checked")
    else {
        panic!("a charge above the maximum isn't flagged");
    };
    assert_eq!(
        flagged.raise.difference,
        Difference::Charge {
            order: id("order-charged"),
            segment: id("seg-over"),
            provider: id(RESERVE),
            charged: amount("1001"),
            maximum: amount("1000"),
            asset: deposit_tuple(RESERVE).from.asset,
        }
    );
    assert_eq!(flagged.raise.cites, vec![result]);
    assert!(flagged.delivered);
    assert_eq!(
        channel.raised,
        vec![flagged.raise.clone()],
        "it reached the operator"
    );
    assert_eq!(raises(&env), vec![(flagged.raise.clone(), true)]);

    // Checked again: the same answer, raised once.
    assert_eq!(
        charges.check(desk(&mut env, &terms, &mut channel), &over),
        Ok(Checked::Flagged(Box::new((*flagged).clone())))
    );
    assert_eq!(channel.raised.len(), 1);
    assert_eq!(raises(&env).len(), 1);

    // At the maximum: nothing raised.
    let result = stated_result(&mut env, "seg-at", 1_000);
    assert_eq!(
        charges.check(
            desk(&mut env, &terms, &mut channel),
            &charged("seg-at", "1000", result)
        ),
        Ok(Checked::Within)
    );
    assert_eq!(channel.raised.len(), 1);

    // Undelivered: recorded failed.
    let result = stated_result(&mut env, "seg-lost", 5_000);
    let mut lost = Channel {
        raised: Vec::new(),
        deliver: false,
    };
    let Ok(Checked::Flagged(undelivered)) = charges.check(
        desk(&mut env, &terms, &mut lost),
        &charged("seg-lost", "5000", result),
    ) else {
        panic!("a charge above the maximum isn't flagged");
    };
    assert!(!undelivered.delivered);
    assert_eq!(raises(&env).last(), Some(&(undelivered.raise, false)));
    assert_admitted(&env);
}

// The operator's channel and the log's records ----------------------------------------------------------------------

/// The operator's channel: every raise it is handed, delivered or not as set.
struct Channel {
    raised: Vec<Raise>,
    deliver: bool,
}

impl Channel {
    fn delivering() -> Self {
        Channel {
            raised: Vec::new(),
            deliver: true,
        }
    }
}

impl Operator for Channel {
    fn raise(&mut self, raise: &Raise) -> Delivery {
        self.raised.push(raise.clone());
        if self.deliver {
            Delivery::Delivered
        } else {
            Delivery::Undelivered
        }
    }
}

fn desk<'a>(env: &'a mut Env, terms: &'a Terms, operator: &'a mut Channel) -> ChargeDesk<'a> {
    ChargeDesk {
        terms,
        log: &mut env.log,
        clock: &*env.clock,
        fresh: &mut env.ids,
        operator,
        core: Box::leak(Box::new(core_actor())),
    }
}

/// Every `raise` recorded, and whether it was `done`.
fn raises(env: &Env) -> Vec<(Raise, bool)> {
    env.book
        .borrow()
        .verified_entries()
        .into_iter()
        .filter_map(|v| match &v.what {
            What::Raise { raise } => Some((raise.clone(), matches!(v.outcome, Outcome::Done {}))),
            _ => None,
        })
        .collect()
}

/// The reserve's signed result of `segment`'s deposit with a charge of `charge`, kept and appended as its stated
/// entry about the order, unverified for the log to attribute: what a charge's raise rests on.
fn stated_result(env: &mut Env, segment: &str, charge: u64) -> Receipt {
    let made_at = 1_900;
    let body = json!({
        "kind": "deposit",
        "transaction": format!("tx-{segment}"),
        "outcome": {
            "status": "succeeded",
            "amount_in": "1000000",
            "amount_out": (1_000_000 - charge).to_string(),
            "fee": { "asset": "USDC", "amount": charge.to_string() },
            "evidence": [],
        },
    });
    let bytes = sign(
        envelope(&deployment(), RESULT_SHAPE, RESERVE, made_at, body),
        &statements_key(RESERVE, 1),
    );
    let kept = env.evidence.keep(&bytes).expect("the store keeps it");
    let key: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let entry = Entry::Stated(StatedEntry {
        kind: StatedKind::Stated,
        attribution: Attribution::Unverified {
            claimed_by: rutent_contracts::core_internal::Actor {
                party: id("acme"),
                acts: id(RESERVE),
                role: rutent_contracts::core_internal::Role::Provider,
                kind: rutent_contracts::core_internal::ActorKind::Component,
            },
        },
        what: StatedWhat {
            statement: ShapeId::new(RESULT_SHAPE).unwrap(),
        },
        when: StatedWhen {
            at: Timestamp::new(Instant(made_at).rfc3339().unwrap()).unwrap(),
            source: StatedTimeSource::PartyStated,
        },
        received_at: Timestamp::new(Instant(START).rfc3339().unwrap()).unwrap(),
        outcome: StatedOutcome {
            status: StatedStatus::Stated,
        },
        signed_with: SignedWith {
            scheme: SignatureScheme::Ed25519,
            key: serde_json::from_value(key["signature"]["key"].clone()).unwrap(),
        },
        commitment: kept.commitment,
        about: Some(About {
            order: Some(id("order-charged")),
            segment: None,
            transaction: None,
        }),
        relayed_by: None,
    });
    env.log
        .append(&GeneralEntry::try_from(entry).expect("a stated entry"))
        .expect("the log takes the result")
}

#[allow(dead_code)]
fn unused(_: TupleKey) {}
