//! Intake's ports in the assembly (`T-024-02`): the register of actors, policy's session register and the
//! destination's register answer for the one actor, session and owner they hold, and for no other.

use rutent_contracts::core_internal::{Actor, ActorKind, Role};
use rutent_core_orch::intake::{IntakeError, Status, Told};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged, StepError};
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT, id};

fn core_with_a_session() -> (Core, rutent_contracts::router_api::Id) {
    let (mut core, _) = Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    (core, session)
}

// verifies: DEC-18
#[test]
fn request_by_an_actor_the_register_doesnt_list_is_refused() {
    let (mut core, session) = core_with_a_session();
    let mut request = base_request("req-impostor", AMOUNT, &session);
    request.caller = Actor {
        party: kit::customer().party,
        acts: id("someone-else"),
        role: Role::Customer,
        kind: ActorKind::Person,
    };
    assert!(matches!(
        core.request(request),
        Err(StepError::Intake(IntakeError::NotTheRequester))
    ));
}

// verifies: DEC-18
#[test]
fn request_in_a_session_policy_never_opened_is_refused() {
    let (mut core, _) = core_with_a_session();
    let request = base_request("req-other-session", AMOUNT, &id("no-such-session"));
    let told = core.request(request).expect("the request is taken");
    assert!(matches!(told, Told::Refused { .. }), "{told:?}");
}

// verifies: DEC-18
#[test]
fn destination_no_register_holds_is_never_quoted() {
    let (mut core, session) = core_with_a_session();
    let mut request = base_request("req-nowhere", AMOUNT, &session);
    request.destination.account = id("acct-of-no-one");
    request.inputs.insert(
        "beneficiary.bank_account".to_owned(),
        "acct-of-no-one".to_owned(),
    );
    let quoted = match core.request(request) {
        Ok(Told::Exists { order, .. }) => core.quote(&kit::customer(), &order).ok(),
        _ => None,
    };
    assert!(
        !matches!(
            quoted,
            Some(Told::Exists {
                status: Status::Quoted { .. },
                ..
            })
        ),
        "{quoted:?}"
    );
}
