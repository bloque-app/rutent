//! Quotes over the assembled Core against the simulated providers (`T-024-03`): a segment declaring more than three
//! days leaves the quote without a firm rate and the order waiting for the customer's acceptance (`AC-13`).

mod routing_support;

use routing_support::*;
use rutent_conformance::sim::{Role, Spec};
use rutent_contracts::core_internal::{ShownAs, What};
use rutent_contracts::state_machine::Observed;
use rutent_core_orch::intake::{Status, Told};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Given, Scenario};
use rutent_scenarios::kit;

const SLOW: &str = "sim-reserve-slow";

fn alone(provider: &str, accepts: &'static str) -> (Core, rutent_contracts::router_api::Id) {
    let given = Given {
        providers: vec![Spec::of(Role::Reserve, provider)],
        staging: Staged::AtOnce,
        deadline: std::time::Duration::from_secs(2),
        delivery: rutent_scenarios::assembly::machine::Delivery::Once,
    };
    let (mut core, _) = Scenario::new("quoting", accepts, given).core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    (core, session)
}

fn started(core: &Core) -> usize {
    core.verified_whats()
        .iter()
        .filter(|w| matches!(w, What::Input { input } if matches!(input.observed, Observed::RouteChosen { .. })))
        .count()
}

// verifies: AC-13, REQ-QTE-10, REQ-ORD-10
#[test]
fn segment_over_three_days_has_no_firm_rate_and_waits_for_acceptance() {
    let (mut core, session) = alone(SLOW, "AC-13");
    let (order, told) = quoted(
        &mut core,
        request_at(SLOW, "req-slow", AMOUNT, &session, "lowest_total_cost"),
    );
    let quote = quote_of(&told).expect("a quote is shown");
    // Shown without a firm rate, naming each segment that declares more than three days.
    assert_eq!(quote.shown_as, ShownAs::QuoteWithoutFirmRate);
    assert_eq!(quote.price.shown_as, ShownAs::QuoteWithoutFirmRate);
    let named: Vec<(String, Option<u64>)> = quote
        .price
        .no_firm_rate
        .iter()
        .map(|s| (s.provider.as_str().to_owned(), s.declared_seconds))
        .collect();
    assert_eq!(
        named,
        vec![
            (SLOW.to_owned(), Some(rutent_conformance::sim::SLOW_SECONDS)),
            (SLOW.to_owned(), Some(rutent_conformance::sim::SLOW_SECONDS)),
        ],
        "both its segments"
    );
    assert!(core.verified_whats().iter().any(|w| matches!(
        w,
        What::Shown {
            shown_as: ShownAs::QuoteWithoutFirmRate,
            ..
        }
    )));

    // Nothing starts until the customer accepts it: no start in the log, no effect, nothing at the reserve.
    assert_eq!(started(&core), 0);
    assert!(core.world().journal(SLOW).is_empty());
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted");
    };
    core.run(&order)
        .expect_err("an order not accepted has no route to run");
    assert!(core.world().journal(SLOW).is_empty(), "still nothing moved");
    let accepted = core
        .accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    assert!(matches!(
        accepted,
        Told::Exists {
            status: Status::Accepted { .. },
            ..
        }
    ));
    assert_eq!(started(&core), 1, "it starts on the acceptance, once");

    // A route whose segments declare three days or less is shown with a firm rate.
    let (mut quick, session) = alone(kit::RESERVE, "AC-13");
    let (_, told) = quoted(
        &mut quick,
        request_at(
            kit::RESERVE,
            "req-firm",
            AMOUNT,
            &session,
            "lowest_total_cost",
        ),
    );
    let quote = quote_of(&told).expect("a quote is shown");
    assert_ne!(quote.shown_as, ShownAs::QuoteWithoutFirmRate);
    assert!(quote.price.no_firm_rate.is_empty());
}

// verifies: AC-14, REQ-QTE-5
#[test]
fn charge_above_the_formulas_maximum_is_flagged() {
    use rutent_contracts::core_internal::{Difference, Outcome};
    use rutent_core_orch::quote::Checked;
    const CHARGING: &str = "sim-reserve-charging";
    let (mut core, session) = alone(CHARGING, "AC-14");
    let (order, told) = quoted(
        &mut core,
        request_at(
            CHARGING,
            "req-charged",
            AMOUNT,
            &session,
            "lowest_total_cost",
        ),
    );
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted");
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    // Core's own segments check what they settle (`T-020-14`): nothing here calls a check.
    core.run(&order).expect("the order runs");

    // The provider quoted no charge and charged 7 on each movement, which only the movement tells.
    let segments = core.charged_segments(&order);
    assert_eq!(segments.len(), 2, "both segments settled and were checked");
    for segment in &segments {
        let Some(Checked::Flagged(flagged)) = core.charge_checked(&order, segment) else {
            panic!("{segment:?} isn't flagged");
        };
        assert!(flagged.delivered, "it reached the operator");
        let Difference::Charge {
            charged,
            maximum,
            provider,
            ..
        } = &flagged.raise.difference
        else {
            panic!("a charge difference");
        };
        assert_eq!(charged.as_str(), "7");
        assert_eq!(maximum.as_str(), "0");
        assert_eq!(provider.as_str(), CHARGING);
    }
    assert_eq!(core.raised().len(), 2);
    // Recorded as raised and done, once each; run again, nothing more is raised.
    let raises = |core: &Core| {
        core.log()
            .entries()
            .iter()
            .filter(|(_, e)| {
                matches!(e, rutent_contracts::core_internal::Entry::Verified(v)
                if matches!(v.what, What::Raise { .. }) && v.outcome == Outcome::Done {})
            })
            .count()
    };
    assert_eq!(raises(&core), 2);
    core.run(&order).expect("runs again");
    assert_eq!(core.raised().len(), 2);
    assert_eq!(raises(&core), 2);

    // A reserve that charges what it quoted raises nothing.
    let (mut honest, session) = alone(kit::RESERVE, "AC-14");
    let (order, told) = quoted(
        &mut honest,
        request_at(
            kit::RESERVE,
            "req-honest",
            AMOUNT,
            &session,
            "lowest_total_cost",
        ),
    );
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted");
    };
    honest
        .accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    honest.run(&order).expect("runs");
    let segments = honest.charged_segments(&order);
    assert_eq!(segments.len(), 2);
    assert!(
        segments
            .iter()
            .all(|s| matches!(honest.charge_checked(&order, s), Some(Checked::Within)))
    );
    assert!(honest.raised().is_empty());
}
