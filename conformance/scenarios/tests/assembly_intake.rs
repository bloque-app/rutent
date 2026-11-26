//! Intake's ports in the assembly (`T-024-02`): the register of actors, policy's session register and the
//! destination's register answer for the one actor, session and owner they hold, and for no other.

use rutent_contracts::core_internal::{
    Actor, ActorKind, Entry, Outcome, ReadCheck, Reads as Read, Role, What, Work,
};
use rutent_core_authz::parties::Scope;
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
    let before = recorded_but_reads(&core);
    assert!(
        matches!(
            core.accept(&kit::customer(), &order, &digest),
            Err(StepError::Intake(IntakeError::NotNow))
        ),
        "not accepted"
    );
    assert_eq!(recorded_but_reads(&core), before, "nothing recorded");
}

/// How many verified entries the log holds, but for policy's recorded reads: telling a status is checked and recorded
/// (`T-020-52`), which is no change to an order.
fn recorded_but_reads(core: &Core) -> usize {
    core.verified_whats()
        .iter()
        .filter(|w| !matches!(w, What::CheckRead { .. }))
        .count()
}

/// The customer's agent: a login of its own the deployment's register lists, as an agent of the customer's party.
fn the_customers_agent() -> Actor {
    Actor {
        party: kit::customer().party,
        acts: id("agent-read-1"),
        role: Role::Customer,
        kind: ActorKind::Agent,
    }
}

/// Lists `agent` and sets the calls its party lets it make: opening a session, authorising and the work an order
/// charges, and reading an order's status only where `reads` says its party scoped it for that.
fn list_and_scope(core: &Core, agent: &Actor, reads: bool) {
    let mut registers = core.registers();
    registers.list(agent);
    let mut scopes = vec![
        Scope::OpenSession,
        Scope::Authorise,
        Scope::Charge(Work::Request),
        Scope::Charge(Work::FanOut),
    ];
    if reads {
        scopes.push(Scope::CheckRead);
    }
    for scope in scopes {
        registers.scope(agent, scope);
    }
}

/// The reads policy recorded, in the log's order: what they read, who read, and whether it was permitted.
fn recorded_reads(core: &Core) -> Vec<(ReadCheck, bool)> {
    core.log()
        .entries()
        .iter()
        .filter_map(|(_, entry)| match entry {
            Entry::Verified(v) => match &v.what {
                What::CheckRead { check } => {
                    Some((check.clone(), v.outcome == (Outcome::Permitted {})))
                }
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect()
}

/// An agent its party didn't scope for reading is told no order's status through intake, by any call: the order
/// opens, and the agent learns neither its identifier nor its state, only that the read was refused. Policy's check
/// is recorded, refused, naming the requester intake's own record holds (`REQ-AUZ-8`, `REQ-AUZ-7`; finding 6 of `M3`'s
/// milestone review). The same party's person, whom the agent's scope doesn't bind, is told as ever.
// verifies: REQ-AUZ-8, REQ-AUZ-7
#[test]
fn agent_its_party_did_not_scope_to_read_learns_no_order_status_through_intake() {
    let (mut core, _) = Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    let agent = the_customers_agent();
    list_and_scope(&core, &agent, false);
    let session = core.open_session(&agent).expect("the agent's session");
    let mut request = base_request("req-unscoped", AMOUNT, &session);
    request.caller = agent.clone();
    let told = core.request(request.clone());
    assert!(
        matches!(told, Err(StepError::Intake(IntakeError::ReadRefused))),
        "{told:?}"
    );
    // The order opened, as the log holds; the agent isn't told which, nor how it stands.
    let opened: Vec<_> = core
        .verified_whats()
        .into_iter()
        .filter_map(|w| match w {
            What::OrderOpened { order, caller, .. } if caller.acts == agent.acts => Some(order),
            _ => None,
        })
        .collect();
    assert_eq!(opened.len(), 1, "the order opened");
    let order = opened[0].clone();
    assert!(
        matches!(
            core.quote(&agent, &order),
            Err(StepError::Intake(IntakeError::ReadRefused))
        ),
        "quoting tells nothing either"
    );
    // Every read of the agent's was checked and recorded, refused, of the filed order and the requester intake's own
    // record names, the reader as it stated itself.
    let reads = recorded_reads(&core);
    assert!(!reads.is_empty());
    for (check, allowed) in &reads {
        assert!(!allowed, "{check:?}");
        assert_eq!(check.reader, agent);
        assert_eq!(
            check.reads,
            Read::FiledOrder {
                order: order.clone(),
                requester: kit::customer().party
            }
        );
    }
    // Its party's person is told, the same order.
    let told = core.request({
        let mut again = request;
        again.caller = kit::customer();
        again
    });
    assert!(
        matches!(&told, Ok(Told::Exists { order: o, .. }) if o == &order),
        "{told:?}"
    );
}

/// An agent its party didn't scope for reading is told no order status through an accept either: the person opens and
/// quotes, the agent (whom the party scoped to authorise, not to read) accepts, and is told `ReadRefused`, neither the
/// order's state nor an error that shows it. The acceptance stands, and policy's read check is recorded, refused, of the
/// order's state once the acceptance is in the log (`T-020-62`; `REQ-AUZ-8`).
// verifies: REQ-AUZ-8
#[test]
fn agent_its_party_did_not_scope_to_read_learns_no_order_status_through_accept() {
    let (mut core, session) = core_with_a_session();
    let agent = the_customers_agent();
    list_and_scope(&core, &agent, false);
    let told = core
        .request(base_request("req-accept-unscoped", AMOUNT, &session))
        .expect("the person is told");
    let order = Core::order_of(&told).expect("an order");
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = core
        .quote(&kit::customer(), &order)
        .expect("the person is told the quote")
    else {
        panic!("a quote is shown");
    };
    let accepted = core.accept(&agent, &order, &digest);
    assert!(
        matches!(accepted, Err(StepError::Intake(IntakeError::ReadRefused))),
        "{accepted:?}"
    );
    assert!(
        core.verified_whats()
            .iter()
            .any(|w| matches!(w, What::Acceptance { order: o, .. } if o == &order)),
        "the acceptance stands"
    );
    let reads = recorded_reads(&core);
    let ours: Vec<_> = reads.iter().filter(|(c, _)| c.reader == agent).collect();
    assert_eq!(ours.len(), 1, "{ours:?}");
    assert!(!ours[0].1, "refused");
    assert_eq!(
        ours[0].0.reads,
        Read::OrderState {
            order: order.clone()
        }
    );
    // The person is told the order as it stands.
    assert!(matches!(
        core.request(base_request("req-accept-unscoped", AMOUNT, &session)),
        Ok(Told::Exists {
            status: Status::Accepted { .. },
            ..
        })
    ));
}

/// An agent its party scoped for reading is told its party's order status at each call and the check is recorded,
/// allowed: a filed order's while the acceptance isn't in the log, the order's state once it is (`REQ-AUZ-8`,
/// `REQ-ORD-5`, `REQ-AUD-12`).
// verifies: REQ-AUZ-8, REQ-ORD-5, REQ-AUD-12
#[test]
fn agent_scoped_to_read_is_told_its_partys_order_status_and_the_check_is_recorded() {
    let (mut core, _) = Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    let agent = the_customers_agent();
    list_and_scope(&core, &agent, true);
    let session = core.open_session(&agent).expect("the agent's session");
    let mut request = base_request("req-scoped", AMOUNT, &session);
    request.caller = agent.clone();
    let told = core.request(request).expect("the agent is told");
    let order = Core::order_of(&told).expect("an order");
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = core
        .quote(&agent, &order)
        .expect("the agent is told the quote")
    else {
        panic!("a quote is shown");
    };
    let accepted = core
        .accept(&agent, &order, &digest)
        .expect("the agent is told");
    assert!(
        matches!(
            accepted,
            Told::Exists {
                status: Status::Accepted { .. },
                ..
            }
        ),
        "{accepted:?}"
    );
    let reads = recorded_reads(&core);
    let ours: Vec<_> = reads.iter().filter(|(c, _)| c.reader == agent).collect();
    assert_eq!(ours.len(), 3, "open, quote, accept: {ours:?}");
    assert!(ours.iter().all(|(_, allowed)| *allowed), "{ours:?}");
    let filed = Read::FiledOrder {
        order: order.clone(),
        requester: kit::customer().party,
    };
    assert_eq!(ours[0].0.reads, filed);
    assert_eq!(ours[1].0.reads, filed);
    assert_eq!(
        ours[2].0.reads,
        Read::OrderState {
            order: order.clone()
        },
        "the acceptance is in the log: the order's state"
    );
}
