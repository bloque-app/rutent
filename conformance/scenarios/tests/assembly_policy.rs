//! The policy calls answered beside the engine (`T-024-02`) are decided by the engine's own fixed rules and the
//! deployment's policy, over the same records: nothing the engine would refuse is permitted beside it.

use rutent_contracts::core_internal::calls::{Authorisations, Charges};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Answer, ChargeOutcome, RefusalClass, Role, What,
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

// verifies: DEC-18
#[test]
fn charge_to_an_actor_the_register_doesnt_list_is_over_its_bound() {
    let core = accepted();
    let mut c = charge(&core);
    c.request = id("charge-of-a-stranger");
    c.caller.acts = id("someone-else");
    let answer = core.policy().charge(&c).unwrap();
    assert!(
        matches!(answer.outcome, ChargeOutcome::Over { .. }),
        "{:?}",
        answer.outcome
    );
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
    assert!(matches!(
        core.policy().charge(&c).unwrap().outcome,
        ChargeOutcome::Over { .. }
    ));
}
