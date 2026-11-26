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

/// An order quoted, Core reopened, and its inputs statement unreadable as intake rebuilds the order from the log:
/// the order is held, never quoted again with its inputs missing nor accepted on the quote shown, and nothing is
/// recorded for it (`NFR-6`, `T-020-46`). (A disk that lost a verified statement isn't opened at all, since the log
/// refuses it, so the hook reads the statement as unreadable.)
// verifies: NFR-6
#[test]
fn order_whose_inputs_cannot_be_read_back_is_neither_quoted_nor_accepted_after_core_reopens() {
    let (mut core, session) = core_with_a_session();
    let Told::Exists { order, .. } = core
        .request(base_request("req-held", AMOUNT, &session))
        .expect("the request is taken")
    else {
        panic!("an order");
    };
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = core.quote(&kit::customer(), &order).expect("quoted")
    else {
        panic!("quoted");
    };
    let inputs = core
        .verified_whats()
        .into_iter()
        .find_map(|w| match w {
            rutent_contracts::core_internal::What::OrderOpened {
                order: o,
                inputs: Some(inputs),
                ..
            } if o == order => Some(inputs),
            _ => None,
        })
        .expect("the order's inputs statement");
    let mut core = Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce)).reopen(
        core,
        1,
        &mut Reference,
    );
    core.intake_reopened_without(&inputs);
    assert!(
        matches!(
            core.quote(&kit::customer(), &order),
            Err(StepError::Intake(IntakeError::NotNow))
        ),
        "not quoted again"
    );
    let before = core.verified_whats().len();
    assert!(
        matches!(
            core.accept(&kit::customer(), &order, &digest),
            Err(StepError::Intake(IntakeError::NotNow))
        ),
        "not accepted"
    );
    assert_eq!(core.verified_whats().len(), before, "nothing recorded");
}
