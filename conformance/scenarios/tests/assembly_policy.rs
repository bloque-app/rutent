//! The policy calls answered beside the engine (`T-024-02`) are decided by the engine's own fixed rules and the
//! deployment's policy, over the same records: nothing the engine would refuse is permitted beside it.

use rutent_contracts::core_internal::calls::{Authorisations, Charges, StuckOrders};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Answer, ChargeOutcome, Entry, Outcome, RefusalClass, Role, VerifiedEntry,
    What,
};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Given, Scenario, Step};
use rutent_scenarios::kit::{self, AMOUNT, id};

fn accepted() -> Core {
    Scenario::new(
        "one order, accepted",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .core(1, &mut Reference)
    .0
}

fn authorisation(core: &Core) -> rutent_contracts::core_internal::Authorisation {
    core.verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::Authorise { authorisation, .. } => Some(authorisation),
            _ => None,
        })
        .expect("the order's authorisation")
}

fn charge(core: &Core) -> rutent_contracts::core_internal::Charge {
    core.verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::Charge { charge, .. } => Some(charge),
            _ => None,
        })
        .expect("the routing's charge")
}

fn regulator_of_the_customers_party() -> Actor {
    Actor {
        party: kit::customer().party,
        acts: id("reg-1"),
        role: Role::Regulator,
        kind: ActorKind::Person,
    }
}

// verifies: DEC-18
#[test]
fn authorisation_given_in_a_role_the_fixed_rule_refuses_is_refused_beside_the_engine() {
    let core = accepted();
    let regulator = regulator_of_the_customers_party();
    core.registers().list(&regulator);
    let mut a = authorisation(&core);
    a.authorisation = id("auth-by-a-regulator");
    a.given_by = regulator;
    let decision = core.policy().authorise(&a).unwrap();
    assert!(
        matches!(decision.outcome, Answer::Refuse { .. }),
        "{:?}",
        decision.outcome
    );
    // The customer that requested it, as the order's own authorisation was given, is still permitted.
    let mut own = authorisation(&core);
    own.authorisation = id("auth-again");
    assert_eq!(
        core.policy().authorise(&own).unwrap().outcome,
        Answer::Permit {}
    );
}

/// The last verified entry the log took.
fn last(core: &Core) -> VerifiedEntry {
    core.log()
        .entries()
        .iter()
        .rev()
        .find_map(|(_, e)| match e {
            Entry::Verified(v) => Some(v.clone()),
            Entry::Stated(_) => None,
        })
        .expect("a verified entry")
}

// verifies: DEC-18, REQ-AUZ-8
#[test]
fn charge_to_an_actor_the_register_doesnt_list_is_refused_for_its_missing_input() {
    let core = accepted();
    let mut c = charge(&core);
    c.request = id("charge-of-a-stranger");
    c.caller.acts = id("someone-else");
    let answer = core.policy().charge(&c).unwrap();
    let missing = RefusalClass::MissingInput;
    assert_eq!(answer.outcome, ChargeOutcome::Refused { class: missing });
    // The log holds policy's own answer, never a bound passed.
    assert_eq!(last(&core).outcome, Outcome::Refused { class: missing });
    // The register listing the login for another party: no accountable party, on the record too.
    let mut elsewhere = Actor {
        party: id("other-party"),
        ..c.caller.clone()
    };
    core.registers().list(&elsewhere);
    elsewhere.party = c.caller.party.clone();
    c.request = id("charge-of-another-partys-login");
    c.caller = elsewhere;
    let answer = core.policy().charge(&c).unwrap();
    let none = RefusalClass::NoAccountableParty;
    assert_eq!(answer.outcome, ChargeOutcome::Refused { class: none });
    assert_eq!(last(&core).outcome, Outcome::Refused { class: none });
}

// verifies: DEC-18, REQ-OPS-1
#[test]
fn routings_charge_says_when_core_counted_it() {
    let core = accepted();
    let counted: Vec<_> = core
        .log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry {
                what: What::Charge { counted_at, .. },
                outcome,
                ..
            }) => Some((counted_at.is_some(), *outcome)),
            _ => None,
        })
        .collect();
    assert!(!counted.is_empty(), "the routing charged its questions");
    assert!(
        counted
            .iter()
            .all(|(at, outcome)| *at == (*outcome == Outcome::Permitted {})),
        "{counted:?}"
    );
    assert!(counted.iter().any(|(at, _)| *at), "{counted:?}");
}

/// A move's outcome reported or confirmed through the assembly's policy reaches the engine, which appends its entry
/// before it answers: here the log refuses it, as it must, since this order has no move whose outcome is unknown
/// (`outcome-report-rests-on-its-move`, `unknown-outcome-settles-on-its-confirmation`), so nothing is answered and
/// nothing recorded. A report of a real unknown outcome is `T-021-06`'s and the stuck scenarios' to drive.
// verifies: DEC-18, REQ-AUZ-4
#[test]
fn a_moves_outcome_reported_or_confirmed_reaches_the_engine_and_its_log() {
    use rutent_contracts::core_internal::calls::AppendFailure;
    let core = accepted();
    let request_of = |entry: &str| {
        let path = format!(
            "{}/../../contracts/core-internal/fixtures/valid/{entry}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let entry: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        entry["what"]["request"].clone()
    };
    let report: rutent_contracts::core_internal::OutcomeReport =
        serde_json::from_value(request_of("verified-entry-of-an-outcome-reported")).unwrap();
    let confirmation: rutent_contracts::core_internal::OutcomeConfirmation =
        serde_json::from_value(request_of("verified-entry-of-an-outcome-confirmed")).unwrap();
    let before = core.log().entries().len();
    assert!(matches!(
        core.policy().report_outcome(&report),
        Err(AppendFailure::Refused(_))
    ));
    assert!(matches!(
        core.policy().confirm_outcome(&confirmation),
        Err(AppendFailure::Refused(_))
    ));
    assert_eq!(core.log().entries().len(), before, "nothing recorded");
}

// verifies: DEC-18
#[test]
fn deployment_refusing_reaches_the_calls_answered_beside_the_engine() {
    let core = accepted();
    core.refuse(Some(RefusalClass::NotPermitted));
    let mut a = authorisation(&core);
    a.authorisation = id("auth-refused");
    assert_eq!(
        core.policy().authorise(&a).unwrap().outcome,
        Answer::Refuse {
            class: RefusalClass::NotPermitted
        }
    );
    let mut c = charge(&core);
    c.request = id("charge-refused");
    let refused = RefusalClass::NotPermitted;
    assert_eq!(
        core.policy().charge(&c).unwrap().outcome,
        ChargeOutcome::Refused { class: refused }
    );
    assert_eq!(last(&core).outcome, Outcome::Refused { class: refused });
}

/// A question about a person on the order's shortlisted route reaches the engine through the assembly's policy, which
/// holds it to the order's shortlist and to its party's charge, and records it on the log's policy handle before it
/// answers (`T-021-07`; the stand-in answered not known). One asking a provider off the shortlist is refused by the
/// log itself, and nothing is recorded; one its party wasn't charged for is refused over the party's bound.
// verifies: DEC-18, REQ-CPL-2, REQ-CPL-6
#[test]
fn question_about_a_person_on_the_orders_shortlist_reaches_the_engine_and_its_log() {
    use rutent_contracts::core_internal::calls::PersonQuestions;
    use rutent_contracts::core_internal::{
        Authority, Count, PersonOutcome, PersonQuestion, RoutingRound, Work,
    };
    let core = accepted();
    let (order, shortlist, shortlisted) = core
        .verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::DecisionRecord {
                order,
                round: RoutingRound::Shortlist,
                record,
                shortlisted,
                ..
            } => Some((order, record, shortlisted.unwrap_or_default())),
            _ => None,
        })
        .expect("the order's shortlist");
    let provider = shortlisted
        .first()
        .cloned()
        .expect("a shortlisted provider");
    let caller = kit::customer();
    let at = charge(&core).at;
    let question = |request: &str, ask: &str, providers: Vec<rutent_contracts::router_api::Id>| {
        PersonQuestion {
            request: id(request),
            asker: caller.clone(),
            // One of the order's parties, as the route requires: here its customer, its requester and payer.
            person: caller.party.clone(),
            order: order.clone(),
            authority: Authority::ShortlistedRoute {
                shortlist: shortlist.clone(),
                providers,
                verifiers: Vec::new(),
            },
            ask: vec![id(ask)],
            asked_before: Vec::new(),
            at: at.clone(),
        }
    };

    // Not charged for: refused over the party's bound, on the record.
    let answer = core
        .policy()
        .ask_about_person(&question(
            "pq-uncharged",
            provider.as_str(),
            shortlisted.clone(),
        ))
        .unwrap();
    let over = RefusalClass::OverBound;
    assert_eq!(answer.outcome, PersonOutcome::Refused { class: over });
    assert_eq!(last(&core).outcome, Outcome::Refused { class: over });

    // Charged for, as rounds charges before it asks: allowed, and recorded permitted.
    let mut c = charge(&core);
    c.request = id("charge-of-one-person-question");
    c.work = Work::PersonQuestion;
    c.count = Count::new(1).unwrap();
    c.order = Some(order.clone());
    assert!(matches!(
        core.policy().charge(&c).unwrap().outcome,
        ChargeOutcome::Within { .. }
    ));
    let answer = core
        .policy()
        .ask_about_person(&question("pq-1", provider.as_str(), shortlisted.clone()))
        .unwrap();
    assert_eq!(answer.outcome, PersonOutcome::Allowed {});
    assert_eq!(last(&core).outcome, Outcome::Permitted {});

    // A provider off the shortlist: the question is refused, and the refusal is recorded (round 3, item t): the log's
    // rows hold only a permitted question to its shortlist.
    let before = core.log().entries().len();
    let mut off = shortlisted.clone();
    off.push(id("off-the-shortlist"));
    let refused = core
        .policy()
        .ask_about_person(&question("pq-off", "off-the-shortlist", off))
        .unwrap();
    assert!(matches!(refused.outcome, PersonOutcome::Refused { .. }));
    assert_eq!(
        core.log().entries().len(),
        before + 1,
        "the refusal is recorded"
    );
}
