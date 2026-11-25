//! An automatic deposit whose only route has no firm rate waits for its customer (`T-020-41`, the owner's ruling
//! `deposit-no-firm-rate`), run on the assembled Core against the simulated providers: the filing shows the quote
//! with its slow segments named and starts nothing; the customer's acceptance, given in the session the deposit was
//! filed in and recorded as `given_on_deposit`, starts the route, which then runs to its end on that acceptance, its
//! later segment moving, and the log, the real one, admitting every entry (`REQ-QTE-10`, `REQ-ORD-15`, `AC-13`,
//! `GUA-1`).

mod execution_support;

use ed25519_dalek::{Signer, SigningKey};
use execution_support::*;
use rutent_conformance::sim::SLOW_SECONDS;
use rutent_contracts::core_internal::Role as RoleOf;
use rutent_contracts::core_internal::{AcceptanceBasis, Action, Role, ShownAs, What};
use rutent_contracts::state_machine::{Observed, OrderState};
use rutent_core_orch::arrival::{Arrived, Filed, FiledWait};
use rutent_core_orch::notice::{About, Notice, NoticeKey, Notices, Receiver, Transport};
use rutent_core_orch::segments::SegmentOutcome;
use rutent_scenarios::assembly::component;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::harness::Scenario;
use rutent_scenarios::kit::{self, AMOUNT, id};

const SLOW: &str = "sim-reserve-slow";

/// The slow reserve that starts routes on automatic deposits, as Core is put together with it.
fn slow_given() -> rutent_scenarios::harness::Given {
    let mut given = given(&[]);
    given.providers = vec![spec_of(SLOW).starting_routes_on_automatic_deposits()];
    given
}

/// A deposit of the slow reserve filed by the reserve, waiting for its customer: Core, the session it was filed in, the
/// order, and the digest of the quote shown.
fn waiting(
    accepts: &'static str,
) -> (
    rutent_scenarios::assembly::Core,
    rutent_contracts::router_api::Id,
    rutent_contracts::router_api::Id,
    rutent_contracts::module_interface::Digest,
    rutent_core_orch::arrival::Filing,
) {
    let bank = format!("{SLOW}-bank");
    let (mut core, session) = core_of(accepts, slow_given());
    let filer = component(SLOW, SLOW, Role::Provider);
    core.registers().list(&filer);
    let reported = report(&core, SLOW, &bank, unit("COP"), AMOUNT, "credit-slow");
    let Arrived::NoOrder { arrival } = core.arrive(&reported).expect("the arrival is taken") else {
        panic!("the arrival is held against no order");
    };
    let asked = within("req-deposit", &session, AMOUNT, SLOW, OTHER);
    let one = filing("deposit-slow", filer, arrival, &session, asked);
    let filed = core.file(one.clone()).expect("the filing is taken");
    let Filed::Waiting {
        order: Some(order),
        wait: FiledWait::CustomerAcceptance,
    } = filed
    else {
        panic!("the deposit waits for its customer: {filed:?}");
    };
    let digest = core.awaiting(&order).expect("its quote").digest;
    (core, session, order, digest, one)
}

/// A wait for the customer survives Core stopping and opening again on its disk: rebuilt from the log, it still waits,
/// the customer accepts it in a session of its own opened after the restart (the quote it was shown is asked again, the
/// quote itself being in no log), and the order runs to its end on that acceptance, every entry admitted by the real log
/// (`REQ-QTE-10`, `AC-13`).
// verifies: REQ-QTE-10, AC-13
#[test]
fn a_waiting_deposit_is_accepted_in_a_later_session_after_core_reopens() {
    let (core, filed_in, order, digest, refiling) = waiting("AC-13");
    let lasts = core.wait_ends(&order).expect("a wait has its end");
    let mut core =
        Scenario::new("execution", "AC-13", slow_given()).reopen(core, 1, &mut Reference);
    assert_eq!(core.wait_ends(&order), Some(lasts), "the wait goes on");
    assert!(
        entries(&core)
            .iter()
            .all(|v| !matches!(v.what, What::Acceptance { .. }))
    );

    let later = core
        .open_session(&kit::customer())
        .expect("a session of its own, after the restart");
    let _ = filed_in;
    core.registers()
        .list(&component(SLOW, SLOW, Role::Provider));
    // The log holds no inputs of the filing (`REQ-AUD-6`): the customer's acceptance can't ask the route again, and the
    // wait goes on until the reserve files the deposit again, stating the filing it was told.
    let asked = core
        .accept_filed(&kit::customer(), &later, &order, &digest)
        .expect("the acceptance is told as it stands");
    assert_eq!(
        asked,
        Filed::Waiting {
            order: Some(order.clone()),
            wait: FiledWait::CustomerAcceptance
        }
    );
    assert!(core.awaiting(&order).is_none(), "no quote to show yet");
    let again = core.file(refiling).expect("the filing is taken again");
    assert!(matches!(again, Filed::Waiting { .. }), "{again:?}");
    let asked = core
        .accept_filed(&kit::customer(), &later, &order, &digest)
        .expect("the acceptance is taken");
    assert_eq!(
        asked,
        Filed::Waiting {
            order: Some(order.clone()),
            wait: FiledWait::CustomerAcceptance
        },
        "the route is shown anew"
    );
    let shown = core.awaiting(&order).expect("the quote shown anew");
    assert_ne!(shown.digest, digest);
    let started = core
        .accept_filed(&kit::customer(), &later, &order, &shown.digest)
        .expect("the acceptance is taken");
    assert!(matches!(started, Filed::Started { .. }), "{started:?}");
    let given_in = entries(&core).into_iter().find_map(|v| match v.what {
        What::Acceptance {
            order: o,
            basis,
            session,
            ..
        } if o == order => Some((basis, session)),
        _ => None,
    });
    assert_eq!(
        given_in,
        Some((AcceptanceBasis::GivenOnDeposit, Some(later)))
    );
    let ran = core.run(&order).expect("the order ran");
    let [rutent_core_orch::segments::Ran::Segments(segments)] = ran.as_slice() else {
        panic!("one part ran its segments: {ran:?}");
    };
    assert_eq!(segments.len(), 2, "{segments:?}");
    assert!(
        segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{segments:?}"
    );
    assert_eq!(state(&mut core, &order), OrderState::Complete);
}

/// A wait past its lifetime is ended by Core's schedule, on the assembled Core: `wait_ended` and the order's refusal in
/// the real log, the deposit free, nothing moved at the reserve (`GUA-1`).
// verifies: GUA-1
#[test]
fn a_waiting_deposit_past_its_lifetime_is_ended_and_nothing_moved() {
    let (mut core, _, order, _, _) = waiting("AC-13");
    let lasts = core.wait_ends(&order).expect("a wait has its end");
    assert_eq!(core.end_expired_waits().expect("the schedule runs"), []);
    core.clock
        .set_nanos(u128::try_from(lasts.unix_nanos()).unwrap());
    assert_eq!(
        core.end_expired_waits().expect("the schedule runs"),
        std::slice::from_ref(&order)
    );
    let ends: Vec<_> = entries(&core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::WaitEnded { order: o, why } => Some((o, why)),
            _ => None,
        })
        .collect();
    assert_eq!(
        ends,
        [(
            order.clone(),
            rutent_contracts::core_internal::WaitEnd::PastItsLifetime
        )]
    );
    assert_eq!(core.wait_ends(&order), None);
    assert!(core.world().journal(SLOW).is_empty(), "nothing moved");
    assert!(
        entries(&core)
            .iter()
            .all(|v| !matches!(v.what, What::Acceptance { .. }))
    );
}

struct Key(SigningKey);

impl NoticeKey for Key {
    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(input).to_bytes())
    }
}

#[derive(Default)]
struct Wire(Vec<(Receiver, Vec<u8>)>);

impl Transport for Wire {
    fn send(&mut self, to: &Receiver, notice: &[u8]) -> bool {
        self.0.push((to.clone(), notice.to_vec()));
        true
    }
}

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
    let waiting = core.awaiting(&order).expect("its quote is shown");
    let (quote, digest) = (waiting.quote.clone(), waiting.digest.clone());
    assert!(
        waiting.valid_until.is_some() && !waiting.expired,
        "it is shown until when it stands"
    );
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

    // The customer is told the order waits for it, from the log alone: the digest, until when the quote stands and the
    // slow segments by their place, and nothing of a provider; the operator is told too.
    let log = core.log().entries().to_vec();
    let mut notices = Notices::new(id("deployment-1"), kit::operator().party);
    let (mut key, mut wire) = (Key(SigningKey::from_bytes(&[7u8; 32])), Wire::default());
    assert_eq!(notices.notify(&log, &mut key, &mut wire), Ok(2));
    let customer = Receiver {
        party: kit::customer().party,
        role: RoleOf::Customer,
    };
    let told: Vec<_> = wire.0.iter().filter(|(to, _)| to == &customer).collect();
    assert_eq!(told.len(), 1, "the customer is told once");
    let sealed: serde_json::Value = serde_json::from_slice(&told[0].1).expect("a sealed notice");
    let notice: Notice = serde_json::from_value(sealed["notice"].clone()).expect("a notice");
    let About::Waiting {
        order: told_order,
        quote: told_quote,
        valid_until,
        slow,
        ..
    } = notice.about
    else {
        panic!("a waiting notice: {:?}", notice.about);
    };
    assert_eq!(
        (told_order, told_quote, valid_until),
        (order.clone(), digest.clone(), waiting.valid_until.clone())
    );
    let told_slow: Vec<_> = slow
        .iter()
        .map(|s| (u64::from(s.position), s.declared_seconds.map(u64::from)))
        .collect();
    assert_eq!(
        told_slow,
        [(1, Some(SLOW_SECONDS)), (2, Some(SLOW_SECONDS))]
    );
    assert!(
        !String::from_utf8_lossy(&told[0].1).contains(SLOW),
        "no provider in the notice"
    );

    // The customer accepts the slow route: it starts, on the customer's own acceptance of it.
    let started = core
        .accept_filed(&kit::customer(), &session, &order, &digest)
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
