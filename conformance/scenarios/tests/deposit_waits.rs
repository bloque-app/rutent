//! An automatic deposit whose only route has no firm rate waits for its customer (`T-020-41`, the owner's ruling
//! `deposit-no-firm-rate`), run on the assembled Core against the simulated providers: the filing shows the quote
//! with its slow segments named and starts nothing; the customer's acceptance, given in the session the deposit was
//! filed in and recorded as `given_on_deposit`, starts the route, which then runs to its end on that acceptance, its
//! later segment moving, and the log, the real one, admitting every entry (`REQ-QTE-10`, `REQ-ORD-15`, `AC-13`,
//! `GUA-1`).

mod execution_support;

use execution_support::*;
use rutent_conformance::sim::SLOW_SECONDS;
use rutent_contracts::core_internal::{AcceptanceBasis, Action, Role, ShownAs, What};
use rutent_contracts::state_machine::{Observed, OrderState};
use rutent_core_orch::arrival::{Arrived, Filed, FiledWait};
use rutent_core_orch::segments::SegmentOutcome;
use rutent_scenarios::assembly::component;
use rutent_scenarios::kit::{self, AMOUNT};

const SLOW: &str = "sim-reserve-slow";

// verifies: REQ-QTE-10, REQ-ORD-15, AC-13, GUA-1
#[test]
fn a_deposit_with_no_firm_rate_waits_and_runs_on_its_customers_acceptance() {
    let bank = format!("{SLOW}-bank");
    let mut given = given(&[]);
    given.providers = vec![spec_of(SLOW).starting_routes_on_automatic_deposits()];
    let (mut core, session) = core_of("AC-13", given);
    let filer = component(SLOW, SLOW, Role::Provider);
    core.registers().list(&filer);

    let reported = report(&core, SLOW, &bank, unit("COP"), AMOUNT, "credit-slow");
    let Arrived::NoOrder { arrival } = core.arrive(&reported).expect("the arrival is taken") else {
        panic!("the arrival is held against no order");
    };
    let asked = within("req-deposit", &session, AMOUNT, SLOW, OTHER);
    let filed = core
        .file(filing(
            "deposit-slow",
            filer.clone(),
            arrival,
            &session,
            asked,
        ))
        .expect("the filing is taken");
    let Filed::Waiting {
        order: Some(order),
        wait: FiledWait::CustomerAcceptance,
    } = filed
    else {
        panic!("the deposit waits for its customer: {filed:?}");
    };

    // The quote is shown with each slow segment named, and nothing is authorised to move: no start, no acceptance,
    // no effect, and the reserve holds the funds as it did.
    let (quote, digest) = core.awaiting(&order).expect("its quote is shown");
    assert_eq!(quote.shown_as, ShownAs::QuoteWithoutFirmRate);
    assert_eq!(quote.price.no_firm_rate.len(), 2, "both its segments");
    assert!(
        quote
            .price
            .no_firm_rate
            .iter()
            .all(|s| s.provider.as_str() == SLOW && s.declared_seconds == Some(SLOW_SECONDS))
    );
    let moved = |core: &rutent_scenarios::assembly::Core| {
        entries(core)
            .into_iter()
            .filter(|v| {
                matches!(
                    &v.what,
                    What::Effect {
                        action: Action::MoveValue { .. },
                        ..
                    } | What::Acceptance { .. }
                ) || matches!(&v.what, What::Input { input } if matches!(input.observed, Observed::RouteChosen { .. }))
            })
            .count()
    };
    assert_eq!(moved(&core), 0);
    assert!(core.world().journal(SLOW).is_empty(), "nothing moved");
    assert!(
        core.run(&order).is_err(),
        "an order not accepted has no route to run"
    );

    // The customer accepts the slow route: it starts, on the customer's own acceptance of it.
    let started = core
        .accept_filed(&kit::customer(), &order, &digest)
        .expect("the acceptance is taken");
    assert!(matches!(started, Filed::Started { .. }), "{started:?}");
    let acceptance = entries(&core)
        .into_iter()
        .find_map(|v| match v.what {
            What::Acceptance {
                order: o,
                basis,
                yeses,
                session,
                quote,
                ..
            } if o == order => Some((basis, yeses.len(), session, quote, v.who)),
            _ => None,
        })
        .expect("accepted");
    assert_eq!(
        acceptance,
        (
            AcceptanceBasis::GivenOnDeposit,
            0,
            Some(session),
            digest,
            kit::customer()
        )
    );

    // It runs to its end on that acceptance: its withdrawal and its later segment both move, and it completes.
    let ran = core.run(&order).expect("the order ran");
    let [rutent_core_orch::segments::Ran::Segments(segments)] = ran.as_slice() else {
        panic!("one part ran its segments: {ran:?}");
    };
    assert_eq!(segments.len(), 2);
    assert!(
        segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{segments:?}"
    );
    let moves = entries(&core)
        .into_iter()
        .filter(|v| {
            matches!(&v.what, What::Effect { action: Action::MoveValue { order: o, .. }, .. } if o == &order)
        })
        .count();
    assert_eq!(moves, 2, "each segment moved once");
    assert_eq!(state(&mut core, &order), OrderState::Complete);
}
