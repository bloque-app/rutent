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
    reserve_route, routed, split_route,
};
use rounds_support::customer;
use rutent_contracts::core_internal::calls::GeneralEntry;
use rutent_contracts::core_internal::calls::{Evidence, Log};
use rutent_contracts::core_internal::{
    About, Actor, ActorKind, Attribution, Entry, OrderRefusalClass, Receipt, Role, ShapeId,
    ShownAs, SignedWith, StatedEntry, StatedKind, StatedOutcome, StatedStatus, StatedTimeSource,
    StatedWhat, StatedWhen, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    Fee, HOLD_SHAPE, Hold, RESULT_SHAPE, RedemptionRefused, SignatureScheme, Statement,
    TERMS_SHAPE, TupleTerms,
};
use rutent_contracts::router_api::{Amount, DecisionRecord, FeeFormula, Id, Timestamp, TupleKey};
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::quote::{
    ExecutedSegment, HeldTo, HoldError, Holds, InForce, NotShown, NotTerms, PriceError, Priced,
    Prices, RecordedAs, Signed, SlowSegment, Terms, digest, executed, show,
};
use rutent_prov::catalogue::Instant;
use rutent_router::quote::{Binding, Executed, HeldQuoteRefused};
use serde_json::{Value, json};
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
        quote.delivers, price.arrives,
        "what arrives, before the operator's fee"
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

/// A segment that reports gross, before another, hands on less than it gives out, and the next segment was quoted at
/// what the first hands on: the router quotes it so (a record's operation states what arrives), and the route is
/// shown as composed. One that ran and handed on less than the next segment was quoted at can't deliver as composed,
/// so it is never shown (`T-020-08`'s hand-off), and the floor the router judged is the floor the price is shown
/// against.
// verifies: REQ-QTE-2
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
    let candidate = chosen(&record);
    let quote =
        show(&record, candidate, None).expect("quoted at what the first hands on, it is shown");
    assert_eq!(
        quote.segments[1].amount_in,
        amount("996"),
        "the deposit was quoted at what the withdrawal hands on"
    );
    assert_eq!(quote.arrives, amount("996"));

    // The withdrawal ran and charged 10, not the 4 it was quoted at: it hands on 990, and the deposit was quoted at
    // 996.
    let mut env = Env::new();
    let usdc = |n: &str| Fee {
        asset: "USDC".into(),
        amount: n.into(),
        billed_separately: None,
    };
    let ran = [
        ExecutedSegment {
            executed: Executed {
                amount_in: amount("1000"),
                fee: usdc("10"),
                amount_out: amount("1000"),
            },
            result: stated_result(&mut env, "seg-w", 10),
        },
        ExecutedSegment {
            executed: Executed {
                amount_in: amount("996"),
                fee: usdc("0"),
                amount_out: amount("996"),
            },
            result: stated_result(&mut env, "seg-d", 0),
        },
    ];
    assert_eq!(
        executed(&record, candidate, &ran),
        Err(NotShown::Undeliverable {
            part: 0,
            operation: 0
        })
    );

    // The router keeps a route to the floor on what arrives, and the price shown agrees with it: a deposit that gives
    // out 1 000 with 4 within it arrives 996, 986 after the operator's 10; a floor of 986 is met exactly, and one of
    // 987 leaves the router no route.
    let route = |floor| {
        reserve_route(
            1_000,
            &Leg::free("USDC"),
            &Leg {
                answer_fee: 4,
                gross: true,
                ..Leg::free("USDC")
            },
            fee("0", "1", "100"),
            Some(floor),
        )
    };
    let at_floor = route(986);
    let met = show(&at_floor, chosen(&at_floor), Some(&amount("986"))).expect("the floor is met");
    assert_eq!(met.shown_out, amount("986"));
    assert!(route(987).chosen().is_none(), "a floor above what arrives");
}

/// An operation priced from its tuple's advertised price, whose reserve reports gross and counts its asset in eight
/// decimals at its origin and six at its out end, states what arrives as it stands: the charge is not added back and
/// taken off again. The reviewer's probe: 100 000 050 in, a price of 50, so 1 000 000 arrive, and the route is shown as
/// that, not refused for giving out 1 000 001 of what it took in.
// verifies: REQ-QTE-2, REQ-CAT-11
#[test]
fn a_priced_gross_operation_with_a_coarser_out_unit_is_shown_as_it_states_what_arrives() {
    let origin = |provider: &str| {
        json!({ "kind": "provider", "provider": provider, "publisher": format!("{provider}-group"),
            "provider_version": "1.0.0", "module_version": "1.0.0" })
    };
    let tuple = |id: &str, from: Value, to: Value, decimals: (u8, u8)| {
        json!({ "figure_id": id, "origin": origin(RESERVE), "age_seconds": 60,
            "value": { "kind": "reserve_tuple", "tuple": {
                "origin": from, "destination": to, "asset": "USDC",
                "decimals": { "origin": decimals.0, "destination": decimals.1 },
                "terms": { "bands": [{ "min": "1", "max": quote_support::WIDE }],
                    "reporting": "gross", "required_inputs": [] } } } })
    };
    let price = |id: &str, tuple: &str, flat: &str| {
        json!({ "figure_id": id, "origin": origin(RESERVE), "age_seconds": 60,
            "value": { "kind": "price", "tuple": tuple, "fee": fee(flat, "0", "1"),
                "settlement_seconds": 60 } })
    };
    let snapshot: rutent_contracts::router_api::CatalogueSnapshot = serde_json::from_value(json!({
        "snapshot_id": "snap-priced",
        "taken_at": quote_support::TAKEN_AT,
        "figures": [
            tuple("w0", json!({ "network": "polygon" }), json!("hub"), (8, 8)),
            price("w0-price", "w0", "0"),
            tuple("d0", json!("hub"), json!({ "network": "polygon" }), (8, 6)),
            price("d0-price", "d0", "50"),
        ],
    }))
    .unwrap();
    let request: rutent_contracts::router_api::Request = serde_json::from_value(json!({
        "request_id": "req-shortlist",
        "round": { "kind": "shortlist" },
        "from": { "node": { "network": "polygon" }, "asset": { "id": "USDC", "decimals": 8 } },
        "to": { "node": { "network": "polygon" }, "asset": { "id": "USDC", "decimals": 6 } },
        "amount": "100000050",
        "objective": { "kind": "lowest_total_cost" },
    }))
    .unwrap();
    let record = routed(&request, &snapshot, &bounds(fee("0", "0", "1"), &[]));
    let candidate = record
        .candidates()
        .iter()
        .find(|c| c.dropped.is_none())
        .expect("the router lists the route");
    assert_priced_deposit_is_shown_as_it_states(&record, candidate);

    // The same route with a provider's answer cited that didn't set the amount: a dry run of the deposit, past its
    // validity, which the router cites and doesn't bind. It reproduces nothing, so the deposit is still the price's.
    let mut net = Net::new(100_000_050);
    net.dry_run(
        "d0",
        RESERVE,
        "deposit",
        &Leg {
            answer_fee: 50,
            gross: true,
            ..Leg::free("USDC")
        },
        100_000_050,
    );
    // Made at 15:01:40, valid for 120 seconds, and the snapshot taken at 15:04:00: 140 seconds old, past its validity.
    let mut stale = net.figures.pop().expect("the dry run's figure");
    stale["age_seconds"] = json!(140);
    let mut figures = serde_json::to_value(&snapshot).unwrap();
    figures["taken_at"] = json!("2026-11-06T15:04:00Z");
    figures["figures"].as_array_mut().unwrap().push(stale);
    let snapshot: rutent_contracts::router_api::CatalogueSnapshot =
        serde_json::from_value(figures).unwrap();
    let record = routed(&request, &snapshot, &bounds(fee("0", "0", "1"), &[]));
    let candidate = record
        .candidates()
        .first()
        .expect("the router lists the route");
    assert!(
        candidate
            .parts
            .first()
            .operations
            .last()
            .figures
            .iter()
            .any(|f| f.as_str() == "d0-dry-100000050"),
        "the deposit cites the answer that didn't set its amount"
    );
    assert_priced_deposit_is_shown_as_it_states(&record, candidate);
}

fn assert_priced_deposit_is_shown_as_it_states(
    record: &DecisionRecord,
    candidate: &rutent_contracts::router_api::Candidate,
) {
    assert_eq!(
        candidate.parts.first().operations.last().amount_out,
        amount("1000000"),
        "the router states what arrives by the priced deposit"
    );
    let shown = show(record, candidate, None).expect("what the priced operation states arrives");
    assert_eq!(shown.arrives, amount("1000000"));
    assert!(shown.within.is_empty(), "no charge is taken off twice");
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

/// Through intake: a route the router offers but can't be delivered as composed is never shown. The reserve reports
/// net: its withdrawal says it gives out 500 more than it takes in, which the router confirms as stated, and its
/// deposit takes that in at a fee of 500, so the route comes back to the origin with no more than it left with and
/// is no rate cycle the router drops. `show` counts the withdrawal as `segments::judge` would and refuses it. The
/// customer is told the order can't proceed now, one `order_refusal` of that class is recorded, and no `shown`.
// verifies: REQ-QTE-2, REQ-ORD-18
#[test]
fn route_intake_cant_deliver_is_refused_and_never_shown() {
    let mut env = Env::new();
    let mut sim = env.sim();
    sim.overclaim.insert(RESERVE.into(), 500);
    sim.deposit_fees.insert(RESERVE.into(), 500);
    // The deposit is priced at that fee too, so the route as first planned comes back to the origin with no more than
    // it left with, and the router asks the deposit again at what the withdrawal claims.
    for price in env.prices.iter_mut() {
        if price.tuple == deposit_tuple(RESERVE) {
            price.fee.flat = amount("500");
        }
    }
    let request = order_request("client-req-undeliverable", customer());
    let order = order_of(&env.open(&mut sim, &request).expect("the request opens"));
    let told = env.quote(&mut sim, &order).expect("the routing runs");
    // The router offered the route: both its segments were asked, the deposit at what the withdrawal claimed.
    let asked: Vec<(String, String)> = sim
        .received
        .iter()
        .map(|(p, op)| {
            (
                p.clone(),
                op["amount"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert!(
        asked.contains(&(RESERVE.to_owned(), "1500".to_owned())),
        "the deposit was asked at what the withdrawal claimed: {asked:?}"
    );
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

/// A route split in two parts is shown and held whole: what leaves the origin is both parts' (600 and 400), and so
/// is what arrives; each part's segments are shown under its own index; and only the reserve every part starts at
/// holds its quote, so a split whose second part starts at another reserve is held by neither alone.
// verifies: REQ-QTE-2, REQ-QTE-15, REQ-QTE-16
#[test]
fn split_route_is_shown_and_held_whole() {
    let record = split_route(quote_support::RESERVE, fee("0", "1", "100"));
    let price = show(&record, chosen(&record), None).expect("the split is shown");
    assert_eq!(
        price.amount_in,
        amount("1000"),
        "both parts leave the origin"
    );
    assert_eq!(price.providers_out, amount("1000"));
    // ceil(1 000 / 100) = 10, on what both parts deliver.
    assert_eq!(price.operator_fee.amount, amount("10"));
    let parts: Vec<(u32, u32)> = price
        .segments
        .iter()
        .map(|s| (s.part, s.operation))
        .collect();
    assert_eq!(parts, [(0, 0), (0, 1), (1, 0), (1, 1)]);
    let mut holds = Holds::new();
    let held = hold_of(
        &record,
        quote_support::RESERVE,
        "hold-split",
        order_hold("order-split"),
        "2026-11-06T15:03:00Z",
    );
    assert!(
        holds
            .hold(
                held.signed(),
                held.recorded(),
                record,
                at("2026-11-06T15:03:00Z")
            )
            .is_ok()
    );

    let record = split_route("p-res2", fee("0", "0", "1"));
    assert!(show(&record, chosen(&record), None).is_ok());
    for (hold, reserve) in [
        ("hold-across", quote_support::RESERVE),
        ("hold-across-2", "p-res2"),
    ] {
        let held = hold_of(
            &record,
            reserve,
            hold,
            order_hold("order-split"),
            "2026-11-06T15:03:00Z",
        );
        assert!(matches!(
            holds.hold(
                held.signed(),
                held.recorded(),
                record.clone(),
                at("2026-11-06T15:03:00Z")
            ),
            Err(HoldError::NotTheReserve)
        ));
    }
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
    let mut env = Env::new();
    let usdc = |n: &str| Fee {
        asset: "USDC".into(),
        amount: n.into(),
        billed_separately: None,
    };
    let withdrew = ExecutedSegment {
        executed: Executed {
            amount_in: amount("1000"),
            fee: usdc("4"),
            amount_out: amount("996"),
        },
        result: stated_result(&mut env, "seg-w", 4),
    };
    let deposited = ExecutedSegment {
        executed: Executed {
            amount_in: amount("996"),
            fee: usdc("2"),
            amount_out: amount("994"),
        },
        result: stated_result(&mut env, "seg-d", 2),
    };
    let order = id("order-priced");
    assert_eq!(
        executed(&record, candidate, std::slice::from_ref(&withdrew)),
        Err(NotShown::NotExecuted {
            part: 0,
            operation: 1
        }),
        "until the deposit executes, there is no price"
    );
    let both = [withdrew.clone(), deposited.clone()];
    let price = executed(&record, candidate, &both).expect("both executed");
    assert_eq!(price.shown_as, ShownAs::Price);
    assert_eq!(
        price.providers_out,
        amount("994"),
        "what they did, not what they quoted"
    );
    // 5 + ceil(994 / 100) = 15.
    assert_eq!(price.operator_fee.amount, amount("15"));
    assert_eq!(price.shown_out, amount("979"));

    let priced = Priced::new(&order, &record, candidate, &both).expect("priced");
    assert_eq!(
        priced.results(),
        &[withdrew.result.clone(), deposited.result.clone()]
    );
    let mut prices = Prices::new();
    let (named, _) = prices
        .report(&mut env.log, &*env.clock, &core_actor(), &priced)
        .expect("the price is reported");
    assert_eq!(digest(&priced), Some(named.clone()));
    assert_eq!(
        shown_entries(&env),
        vec![(order.clone(), named.clone(), ShownAs::Price)]
    );
    prices
        .report(&mut env.log, &*env.clock, &core_actor(), &priced)
        .expect("reported again");
    assert_eq!(shown_entries(&env).len(), 1, "reported once");
    // Another price for the order is refused: an order has one.
    let other = ExecutedSegment {
        executed: Executed {
            amount_out: amount("993"),
            ..deposited.executed.clone()
        },
        result: stated_result(&mut env, "seg-d2", 3),
    };
    let another =
        Priced::new(&order, &record, candidate, &[withdrew.clone(), other]).expect("priced");
    assert_eq!(
        prices.report(&mut env.log, &*env.clock, &core_actor(), &another),
        Err(PriceError::AnotherPrice)
    );
    assert_eq!(shown_entries(&env).len(), 1);
    assert_admitted(&env);
}

/// A price is made only from every segment's result that counted: with nothing executed there is no price, and one
/// result cited for two segments is no execution of the second. `Priced` has no public field, so a quote can't be
/// relabelled the price and reported.
// verifies: REQ-QTE-7
#[test]
fn price_rests_on_each_segments_own_result() {
    let record = reserve_route(
        1_000,
        &Leg::free("USDC"),
        &Leg::free("USDC"),
        fee("0", "0", "1"),
        None,
    );
    let candidate = chosen(&record);
    let order = id("order-priced");
    assert_eq!(
        Priced::new(&order, &record, candidate, &[]).map(|p| p.order().clone()),
        Err(NotShown::NotExecuted {
            part: 0,
            operation: 0
        }),
        "nothing executed, no price"
    );
    let mut env = Env::new();
    let usdc = Fee {
        asset: "USDC".into(),
        amount: "0".into(),
        billed_separately: None,
    };
    let result = stated_result(&mut env, "seg-once", 0);
    let segment = |amount_in: &str| ExecutedSegment {
        executed: Executed {
            amount_in: amount(amount_in),
            fee: usdc.clone(),
            amount_out: amount(amount_in),
        },
        result: result.clone(),
    };
    assert_eq!(
        Priced::new(
            &order,
            &record,
            candidate,
            &[segment("1000"), segment("1000")]
        )
        .map(|p| p.order().clone()),
        Err(NotShown::ResultCitedTwice {
            part: 0,
            operation: 1
        })
    );
}

// REQ-QTE-6 ---------------------------------------------------------------------------------------------------------

/// The provider's terms for a tuple bind a segment from its start, from the later of when they were made and when Core
/// received them: the last binding at or before it, whatever order they were kept in; terms stated after it never
/// apply to it, nor terms dated before it that arrived after it, and before it starts nothing binds. The entry that
/// holds the terms in force is the one named. A statement that isn't a verified entry of a tuple's provider's own
/// terms is kept as none.
// verifies: REQ-QTE-6
#[test]
fn terms_stated_after_a_segment_started_do_not_apply_to_it() {
    let tuple = deposit_tuple(RESERVE);
    let kept = |terms: &mut Terms, made: u64, received: u64, maximum: &str, n: usize| {
        let (stated, statement) = terms_of(&tuple, made, received, maximum);
        terms.state(&stated, &receipt(n), &statement)
    };
    let mut terms = Terms::new();
    assert_eq!(kept(&mut terms, 1_200, 1_200, "1000", 1), Ok(true));
    let started = at("1970-01-01T00:25:00Z");
    assert_eq!(
        kept(&mut terms, 1_800, 1_800, "5000", 2),
        Ok(true),
        "stated after the start"
    );
    assert_eq!(
        kept(&mut terms, 1_440, 1_440, "1200", 3),
        Ok(true),
        "stated before it, kept later"
    );
    assert_eq!(
        kept(&mut terms, 1_440, 1_440, "1200", 4),
        Ok(false),
        "the same terms are kept once"
    );
    assert_eq!(
        kept(&mut terms, 1_400, 1_560, "9000", 5),
        Ok(true),
        "dated before the start, received after it"
    );
    assert_eq!(
        terms.binding(&tuple, None),
        Binding::Quote,
        "before its start, a quote"
    );
    assert_eq!(terms.in_force(&tuple, None), InForce::Quote);
    match terms.in_force(&tuple, Some(&started)) {
        InForce::Binds { terms, receipt: r } => {
            assert_eq!(terms.stated_at, at("1970-01-01T00:24:00Z"));
            assert_eq!(terms.fee.maximum, Some(amount("1200")));
            assert_eq!(r, &receipt(3), "the entry that holds them");
        }
        other => panic!("nothing binds: {other:?}"),
    }
    // Received at 00:26, it binds from then, though dated 00:23:20: a segment started at 00:26 is bound by it.
    match terms.in_force(&tuple, Some(&at("1970-01-01T00:26:00Z"))) {
        InForce::Binds { terms, receipt: r } => {
            assert_eq!(terms.stated_at, at("1970-01-01T00:26:00Z"));
            assert_eq!(r, &receipt(5));
        }
        other => panic!("nothing binds: {other:?}"),
    }
    assert_eq!(
        terms.in_force(&tuple, Some(&at("1970-01-01T00:10:00Z"))),
        InForce::Unstated,
        "started before any were stated"
    );

    // What isn't a provider's verified terms is none.
    let (stated, statement) = terms_of(&tuple, 1_900, 1_900, "1000");
    let mut unattributed = stated.clone();
    unattributed.attribution = Attribution::Unverified {
        claimed_by: provider(RESERVE),
    };
    assert_eq!(
        terms.state(&unattributed, &receipt(6), &statement),
        Err(NotTerms::Unattributed)
    );
    let mut result = stated.clone();
    result.what.statement = ShapeId::new(rutent_contracts::provider_role::RESULT_SHAPE).unwrap();
    assert_eq!(
        terms.state(&result, &receipt(6), &statement),
        Err(NotTerms::NotTheTermsShape)
    );
    let mut another_maker = stated.clone();
    another_maker.attribution = Attribution::Verified {
        who: provider("p-other"),
    };
    assert_eq!(
        terms.state(&another_maker, &receipt(6), &statement),
        Err(NotTerms::NotItsStatement)
    );
    let mut another_time = stated.clone();
    another_time.when.at = at("1970-01-01T00:40:00Z");
    assert_eq!(
        terms.state(&another_time, &receipt(6), &statement),
        Err(NotTerms::NotItsTimes)
    );
    let mut theirs: Statement<TupleTerms> = statement.clone();
    theirs.statement.slippage = Some(serde_json::from_value(json!({ "regime": "firm" })).unwrap());
    assert_eq!(
        terms.state(&stated, &receipt(6), &theirs),
        Err(NotTerms::Refused),
        "a slippage regime where its tuple sets no rate"
    );
    assert_eq!(terms.history(&tuple).len(), 4, "none of them was kept");
}

// REQ-QTE-15 --------------------------------------------------------------------------------------------------------

/// A reserve holds the quote of a choice round on its own signed hold until a validity it states, within its figures'
/// validity: held, it is a quote, never the price; at its validity its route may still start; past it, it starts
/// nothing. Only the reserve the route starts at holds it, a validity before the quote is refused, and so is a hold
/// that isn't of this record, or isn't the entry of its own transaction.
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
    let hold = hold_of(
        &record,
        quote_support::RESERVE,
        "hold-1",
        order_hold("order-held"),
        "2026-11-06T15:03:00Z",
    );
    let mut holds = Holds::new();
    let held = holds
        .hold(
            hold.signed(),
            hold.recorded(),
            record.clone(),
            until.clone(),
        )
        .expect("the reserve holds its quote");
    assert_eq!(held.shown.shown_as, ShownAs::Quote, "held, it is a quote");
    assert_eq!(
        *held.valid_until(),
        until,
        "within its figures' validity, as stated"
    );
    assert_eq!(held.hold, hold.receipt, "its stated entry is what it cites");
    assert_eq!(held.transaction, id("hold-1"));

    let transaction = id("hold-1");
    let redeemed = holds
        .redeem(&transaction, &reserve, &order, None, &until)
        .expect("at its validity, it is still usable");
    assert_eq!(redeemed.candidate, &candidate);
    assert_eq!(redeemed.shown.shown_as, ShownAs::Quote);
    assert_eq!(
        redeemed.hold, &hold.receipt,
        "the acceptance that redeems it cites its entry"
    );
    assert_eq!(
        holds
            .redeem(
                &transaction,
                &reserve,
                &order,
                None,
                &at("2026-11-06T15:03:01Z")
            )
            .map(|r| r.candidate.clone()),
        Err(HoldError::Redeem(HeldQuoteRefused::PastValidity {
            valid_until: until.clone()
        })),
        "past its validity, it starts nothing"
    );

    // Another reserve's hold of the same record, or one for a quote it doesn't start.
    let other = hold_of(
        &record,
        "p-other",
        "hold-2",
        order_hold("order-held"),
        "2026-11-06T15:03:00Z",
    );
    assert_eq!(
        holds
            .hold(
                other.signed(),
                other.recorded(),
                record.clone(),
                until.clone()
            )
            .map(|h| h.reserve.clone()),
        Err(HoldError::NotTheReserve)
    );
    let before = hold_of(
        &record,
        quote_support::RESERVE,
        "hold-3",
        order_hold("order-held"),
        "2026-11-06T15:01:00Z",
    );
    assert!(matches!(
        holds.hold(
            before.signed(),
            before.recorded(),
            record.clone(),
            at("2026-11-06T15:01:00Z")
        ),
        Err(HoldError::Refused(_))
    ));
    assert!(matches!(
        holds.hold(
            hold.signed(),
            hold.recorded(),
            record.clone(),
            until.clone()
        ),
        Err(HoldError::AlreadyHeld)
    ));
    // The same hold kept again under a new commitment, appended as a new entry, is the same hold.
    let again = SignedHold {
        receipt: receipt(900),
        ..hold.clone()
    };
    assert!(matches!(
        holds.hold(
            again.signed(),
            again.recorded(),
            record.clone(),
            until.clone()
        ),
        Err(HoldError::AlreadyHeld)
    ));

    // A hold of another record's candidate of the same id: it names the record by another digest than the log does.
    let elsewhere = hold_of(
        &record,
        quote_support::RESERVE,
        "hold-4",
        order_hold("order-held"),
        "2026-11-06T15:03:00Z",
    );
    let wrong = holds.hold(
        elsewhere.signed(),
        RecordedAs {
            id: &elsewhere.record_id,
            digest: &digest_of_text("cd"),
        },
        record.clone(),
        until.clone(),
    );
    assert!(
        matches!(wrong, Err(HoldError::NotThisQuotesHold(_))),
        "{wrong:?}"
    );
    let another_record = RecordedAs {
        id: &id("req-elsewhere"),
        digest: &elsewhere.record_digest,
    };
    assert!(matches!(
        holds.hold(
            elsewhere.signed(),
            another_record,
            record.clone(),
            until.clone()
        ),
        Err(HoldError::NotThisQuotesHold(_))
    ));

    // Not the entry of a hold: another shape, a signature the log didn't verify, another transaction or an order.
    let hold5 = hold_of(
        &record,
        quote_support::RESERVE,
        "hold-5",
        order_hold("order-held"),
        "2026-11-06T15:03:00Z",
    );
    let mut ill = [hold5.clone(), hold5.clone(), hold5.clone(), hold5.clone()];
    ill[0].stated.what.statement = ShapeId::new(TERMS_SHAPE).unwrap();
    ill[1].stated.attribution = Attribution::Unverified {
        claimed_by: provider(quote_support::RESERVE),
    };
    ill[2].stated.about = Some(About {
        order: None,
        segment: None,
        transaction: Some(id("hold-elsewhere")),
    });
    ill[3].stated.about = Some(About {
        order: Some(id("order-held")),
        segment: None,
        transaction: Some(id("hold-5")),
    });
    for ill in &ill {
        assert_eq!(
            holds
                .hold(ill.signed(), ill.recorded(), record.clone(), until.clone())
                .map(|h| h.reserve.clone()),
            Err(HoldError::NotAHold)
        );
    }

    assert!(
        holds.drop_hold(&transaction),
        "dropped, with no effect beyond the hold"
    );
    assert!(matches!(
        holds.redeem(
            &transaction,
            &reserve,
            &order,
            None,
            &at("2026-11-06T15:02:30Z")
        ),
        Err(HoldError::NoSuchHold)
    ));
}

/// A hold stated for longer than the figures its route stands on is refused, as the contract refuses it; a quote
/// asked for longer than they are usable is held only as long as they are: until the dry runs' expiry, 15:03:40, and
/// past it the route starts nothing, though Core was asked for 15:30.
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
    // The reserve signed 15:30: past what its figures stand on.
    let long = hold_of(
        &record,
        quote_support::RESERVE,
        "hold-over",
        order_hold("order-held"),
        "2026-11-06T15:30:00Z",
    );
    assert!(matches!(
        holds.hold(
            long.signed(),
            long.recorded(),
            record.clone(),
            at("2026-11-06T15:30:00Z")
        ),
        Err(HoldError::NotThisQuotesHold(_))
    ));
    // Signed to their expiry, and asked to be held longer: held to it.
    let hold = hold_of(
        &record,
        quote_support::RESERVE,
        "hold-long",
        order_hold("order-held"),
        "2026-11-06T15:03:40Z",
    );
    let held = holds
        .hold(
            hold.signed(),
            hold.recorded(),
            record,
            at("2026-11-06T15:30:00Z"),
        )
        .expect("held, capped");
    assert_eq!(*held.valid_until(), at("2026-11-06T15:03:40Z"));
    let transaction = id("hold-long");
    assert!(
        holds
            .redeem(
                &transaction,
                &reserve,
                &order,
                None,
                &at("2026-11-06T15:03:40Z")
            )
            .is_ok()
    );
    assert!(matches!(
        holds.redeem(
            &transaction,
            &reserve,
            &order,
            None,
            &at("2026-11-06T15:10:00Z")
        ),
        Err(HoldError::Redeem(HeldQuoteRefused::PastValidity { .. }))
    ));
}

/// A held quote is redeemed only by the reserve that holds it, for the order its signed hold names, or an order
/// accepted in the session it names, and by one: another reserve, the same reserve for another order, a session
/// hold redeemed outside its session, and a second order of the session are each refused within its validity.
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
    let for_an_order = hold_of(
        &record,
        quote_support::RESERVE,
        "hold-1",
        order_hold("order-held"),
        "2026-11-06T15:03:00Z",
    );
    holds
        .hold(
            for_an_order.signed(),
            for_an_order.recorded(),
            record.clone(),
            at("2026-11-06T15:03:00Z"),
        )
        .expect("held");
    let one = id("hold-1");
    assert!(holds.redeem(&one, &reserve, &order, None, &now).is_ok());
    assert!(matches!(
        holds.redeem(&one, &id("p-other"), &order, None, &now),
        Err(HoldError::NotTheHolder)
    ));
    assert!(matches!(
        holds.redeem(&one, &reserve, &id("order-other"), None, &now),
        Err(HoldError::NotForThisOrder(
            RedemptionRefused::ForAnotherOrder
        ))
    ));

    // A hold for a session: any order accepted in it, the first one only.
    let for_a_session = hold_of(
        &record,
        quote_support::RESERVE,
        "hold-2",
        json!({ "kind": "session", "session": "sess-9" }),
        "2026-11-06T15:03:00Z",
    );
    holds
        .hold(
            for_a_session.signed(),
            for_a_session.recorded(),
            record,
            at("2026-11-06T15:03:00Z"),
        )
        .expect("held");
    let two = id("hold-2");
    let session = id("sess-9");
    assert!(matches!(
        holds.redeem(&two, &reserve, &order, None, &now),
        Err(HoldError::NotForThisOrder(
            RedemptionRefused::ForAnotherSession
        ))
    ));
    assert!(matches!(
        holds.redeem(&two, &reserve, &order, Some(&id("sess-1")), &now),
        Err(HoldError::NotForThisOrder(
            RedemptionRefused::ForAnotherSession
        ))
    ));
    assert!(
        holds
            .redeem(&two, &reserve, &order, Some(&session), &now)
            .is_ok()
    );
    assert!(matches!(
        holds.redeem(&two, &reserve, &id("order-second"), Some(&session), &now),
        Err(HoldError::RedeemedByAnother)
    ));
    assert!(
        holds
            .redeem(&two, &reserve, &order, Some(&session), &now)
            .is_ok(),
        "the same order again"
    );
}

// What the tests above hold -----------------------------------------------------------------------------------------

/// A receipt of the log, number `n`.
fn receipt(n: usize) -> Receipt {
    serde_json::from_value(json!({
        "log": "log-1",
        "index": n,
        "entry": { "algorithm": "sha-256", "value": format!("{n:064x}") },
    }))
    .unwrap()
}

fn digest_of_text(pair: &str) -> Digest {
    serde_json::from_value(json!({ "algorithm": "sha-256", "value": pair.repeat(32) })).unwrap()
}

/// A provider component.
fn provider(acts: &str) -> Actor {
    Actor {
        party: id("acme"),
        acts: id(acts),
        role: Role::Provider,
        kind: ActorKind::Component,
    }
}

/// A stated entry as the log attributes it: verified, `shape`'s, made by `maker` at `made_at` seconds and received at
/// `received_at`, about `about`.
fn stated_entry(
    shape: &str,
    maker: &str,
    made_at: u64,
    received_at: u64,
    about: Option<About>,
) -> StatedEntry {
    StatedEntry {
        kind: StatedKind::Stated,
        attribution: Attribution::Verified {
            who: provider(maker),
        },
        what: StatedWhat {
            statement: ShapeId::new(shape).unwrap(),
        },
        when: StatedWhen {
            at: Timestamp::new(Instant(made_at).rfc3339().unwrap()).unwrap(),
            source: StatedTimeSource::PartyStated,
        },
        received_at: Timestamp::new(Instant(received_at).rfc3339().unwrap()).unwrap(),
        outcome: StatedOutcome {
            status: StatedStatus::Stated,
        },
        signed_with: SignedWith {
            scheme: SignatureScheme::Ed25519,
            key: snapshot_support::public_key(&statements_key(maker, 1)),
        },
        commitment: digest_of_text("11"),
        about,
        relayed_by: None,
    }
}

/// `tuple`'s provider's signed terms with a fee of at most `maximum`, made at `made_at` seconds and received at
/// `received_at`, and their stated entry.
fn terms_of(
    tuple: &TupleKey,
    made_at: u64,
    received_at: u64,
    maximum: &str,
) -> (StatedEntry, Statement<TupleTerms>) {
    let body = json!({
        "tuple": tuple,
        "fee": { "flat": "0", "proportional": { "numerator": "1", "denominator": "1000" },
            "maximum": maximum },
    });
    let bytes = sign(
        envelope(
            &deployment(),
            TERMS_SHAPE,
            tuple.provider.as_str(),
            made_at,
            body,
        ),
        &statements_key(tuple.provider.as_str(), 1),
    );
    (
        stated_entry(
            TERMS_SHAPE,
            tuple.provider.as_str(),
            made_at,
            received_at,
            None,
        ),
        serde_json::from_slice(&bytes).expect("a terms statement"),
    )
}

/// A reserve's signed hold, its stated entry and the record it names.
#[derive(Clone)]
struct SignedHold {
    statement: Statement<Hold>,
    stated: StatedEntry,
    receipt: Receipt,
    record_id: Id,
    record_digest: Digest,
}

impl SignedHold {
    fn signed(&self) -> Signed<'_> {
        Signed {
            hold: &self.statement,
            stated: &self.stated,
            receipt: &self.receipt,
        }
    }

    fn recorded(&self) -> RecordedAs<'_> {
        RecordedAs {
            id: &self.record_id,
            digest: &self.record_digest,
        }
    }
}

/// What a hold is for: `order`.
fn order_hold(order: &str) -> Value {
    json!({ "kind": "order", "order": order })
}

/// `reserve`'s signed hold of `record`'s chosen candidate, under `transaction`, for `held_for`, until `valid_until`,
/// and its stated entry: about its transaction and no order.
fn hold_of(
    record: &DecisionRecord,
    reserve: &str,
    transaction: &str,
    held_for: Value,
    valid_until: &str,
) -> SignedHold {
    let record_digest = digest_of_text("ab");
    let body = json!({
        "transaction": transaction,
        "record": { "id": record.record_id(), "digest": record_digest },
        "candidate": chosen(record).candidate_id,
        "for": held_for,
        "valid_until": valid_until,
    });
    let bytes = sign(
        envelope(&deployment(), HOLD_SHAPE, reserve, 1_900, body),
        &statements_key(reserve, 1),
    );
    SignedHold {
        statement: serde_json::from_slice(&bytes).expect("a hold statement"),
        stated: stated_entry(
            HOLD_SHAPE,
            reserve,
            1_900,
            2_000,
            Some(About {
                order: None,
                segment: None,
                transaction: Some(id(transaction)),
            }),
        ),
        receipt: receipt(7),
        record_id: record.record_id().clone(),
        record_digest,
    }
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
