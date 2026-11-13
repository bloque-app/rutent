//! Order requests and the acceptance, as scenarios over the assembled Core against the simulated providers
//! (`T-024-03`): a repeated request leaves one order (`AC-23`), the three parties recorded apart and checked by their
//! providers' rules (`AC-24`), a balance check answered yes or no, logged and counted (`AC-15`), and a dry run that
//! changes nothing and a destination refused (`AC-43`).

use rutent_contracts::core_internal::{Actor, ActorKind, Entry, Role, What};
use rutent_contracts::router_api::Id;
use rutent_core_orch::intake::{IntakeError, Status, Told};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged, StepError};
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT, id};

/// Core alone, its one reserve granted and advertised, a session open for the customer.
fn core_with_a_session(accepts: &'static str) -> (Core, Id) {
    let (mut core, _) = Scenario::new("Core alone", accepts, Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    (core, session)
}

/// The orders the log holds an acceptance for, and the requests that opened an order, as the log says.
fn acceptances(core: &Core) -> Vec<Id> {
    core.verified_whats()
        .into_iter()
        .filter_map(|w| match w {
            What::Acceptance { order, .. } => Some(order),
            _ => None,
        })
        .collect()
}

/// Every order the log names a quote shown for.
fn shown(core: &Core) -> Vec<Id> {
    core.verified_whats()
        .into_iter()
        .filter_map(|w| match w {
            What::Shown { order, .. } => Some(order),
            _ => None,
        })
        .collect()
}

fn the_order(told: &Told) -> Id {
    Core::order_of(told).expect("an order exists")
}

/// The agent of the customer's party, a second actor the register lists.
fn the_customers_agent(core: &Core) -> Actor {
    let agent = Actor {
        party: kit::customer().party,
        acts: id("agent-1"),
        role: Role::Customer,
        kind: ActorKind::Agent,
    };
    core.registers().list(&agent);
    agent
}

// verifies: AC-23, REQ-ORD-19
#[test]
fn repeated_order_request_leaves_one_order() {
    let (mut core, session) = core_with_a_session("AC-23");
    let first = core
        .request(base_request("req-once", AMOUNT, &session))
        .expect("the request opens an order");
    let order = the_order(&first);

    // The same request again, from the same actor and from another of the requester's: no second order, and the
    // caller is told the same, which is how it can tell what happened.
    let again = core
        .request(base_request("req-once", AMOUNT, &session))
        .expect("the repeat is answered");
    assert_eq!(again, first, "the repeat is told what the first was");
    let agent = the_customers_agent(&core);
    let mut by_agent = base_request("req-once", AMOUNT, &session);
    by_agent.caller = agent;
    let by_agent = core.request(by_agent).expect("the agent's repeat is answered");
    assert_eq!(by_agent, first, "an actor of the same party is told the same");

    // Quoted and accepted, and then asked again: the order is told as it stands, accepted, and nothing more is
    // opened, quoted or accepted.
    let told = core.quote(&kit::customer(), &order).expect("quoted");
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("a quote is shown: {told:?}");
    };
    let accepted = core
        .accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    assert!(
        matches!(
            &accepted,
            Told::Exists {
                status: Status::Accepted { .. },
                ..
            }
        ),
        "{accepted:?}"
    );
    let after = core
        .request(base_request("req-once", AMOUNT, &session))
        .expect("the repeat after acceptance is answered");
    assert_eq!(after, accepted, "told that it was accepted");
    assert_eq!(the_order(&after), order);
    let repeated_acceptance = core
        .accept(&kit::customer(), &order, &digest)
        .expect("a repeated acceptance is answered");
    assert_eq!(repeated_acceptance, accepted);
    assert_eq!(acceptances(&core), vec![order.clone()], "one acceptance");
    assert_eq!(shown(&core), vec![order.clone()], "one quote shown");
    assert_eq!(
        core.log()
            .entries()
            .iter()
            .filter(|(_, e)| matches!(e, Entry::Verified(v)
                if matches!(&v.what, What::Input { input }
                    if matches!(input.observed, rutent_contracts::state_machine::Observed::RouteChosen { .. }))))
            .count(),
        1,
        "the order started once"
    );

    // Another request under the same identifier is no repeat: it opens nothing, and says which order the
    // identifier is.
    let other = core.request(base_request("req-once", "2000000", &session));
    assert!(
        matches!(
            &other,
            Err(StepError::Intake(IntakeError::Conflicting { order: Some(o) })) if o == &order
        ),
        "{other:?}"
    );
    assert_eq!(acceptances(&core), vec![order], "still one order");

    // A fresh identifier is another order.
    let second = core
        .request(base_request("req-twice", AMOUNT, &session))
        .expect("another request opens another order");
    assert_ne!(the_order(&second), the_order(&first));
    let _ = kit::customer();
}

// verifies: AC-15, REQ-CAT-3, REQ-CAT-4, INV-9
#[test]
fn balance_check_answers_yes_or_no_and_is_logged_and_counted() {
    use rutent_prov::catalogue::{CheckKind, CheckRefusal, Verdict};
    use rutent_scenarios::assembly::{CHECKS_PER_WINDOW, context};
    let account = context().account;
    let (mut core, session) = core_with_a_session("AC-15");

    // An order's own routing asks its reserve whether the funds suffice, by a dry-run withdrawal: that is a check,
    // logged with the account and the order it was for. The holder's own order is never counted against it.
    let told = core
        .request(base_request("req-own", AMOUNT, &session))
        .expect("opens");
    let order = the_order(&told);
    core.quote(&kit::customer(), &order).expect("quoted");
    let logged: Vec<_> = core.catalogue().check_log(&account);
    assert_eq!(logged.len(), 1, "the routing's check is logged");
    assert_eq!(logged[0].kind, CheckKind::DryRunWithdrawal);
    assert_eq!(logged[0].own_order.as_deref(), Some(order.as_str()));
    assert!(!logged[0].counted && logged[0].refused.is_none());

    // A check outside an order answers yes or no for the amount it states, and nothing more, and is counted.
    let now = core.clock.instant();
    assert_eq!(
        core.check_balance(kit::RESERVE, &session, "40000000"),
        Ok(Verdict::Yes),
        "the reserve holds 50 million"
    );
    assert_eq!(
        core.check_balance(kit::RESERVE, &session, "60000000"),
        Ok(Verdict::No)
    );
    assert_eq!(core.catalogue().counted_checks(&account, now), 2);
    assert_eq!(core.catalogue().check_log(&account).len(), 3, "all logged");

    // A dry run that reveals the same is a check too, and counts.
    assert!(
        core.check_by_dry_run(kit::RESERVE, &session, "1000000")
            .is_ok()
    );
    let kinds: Vec<CheckKind> = core
        .catalogue()
        .check_log(&account)
        .iter()
        .map(|c| c.kind)
        .collect();
    assert_eq!(kinds.last(), Some(&CheckKind::DryRunWithdrawal));
    assert_eq!(core.catalogue().counted_checks(&account, now), 3);

    // The bound is the deployment's: once it is reached, the next check isn't sent, and is logged as refused.
    for _ in 3..CHECKS_PER_WINDOW {
        core.check_balance(kit::RESERVE, &session, "1000000")
            .expect("within the bound");
    }
    let books = core.world().journal(kit::RESERVE).len();
    assert_eq!(
        core.check_balance(kit::RESERVE, &session, "1000000"),
        Err(CheckRefusal::OverBound)
    );
    let counted = core.catalogue().counted_checks(&account, core.clock.instant());
    assert_eq!(counted, CHECKS_PER_WINDOW, "the refused one isn't counted");
    assert_eq!(
        core.catalogue().check_log(&account).last().map(|c| c.refused.clone()),
        Some(Some(CheckRefusal::OverBound))
    );
    assert_eq!(core.world().journal(kit::RESERVE).len(), books);

    // And with no session behind it, none is sent.
    assert_eq!(
        core.check_balance(kit::RESERVE, &id("no-such-session"), "1000000"),
        Err(CheckRefusal::NoSession)
    );
}

// verifies: AC-43, REQ-CAT-14, REQ-ORD-35
#[test]
fn dry_run_changes_nothing_and_refuses_an_undepositable_destination() {
    use rutent_conformance::sim::{Act, Behaviour};
    let (mut core, session) = core_with_a_session("AC-43");
    let moved = |core: &Core| -> usize {
        core.verified_whats()
            .iter()
            .filter(|w| matches!(w, What::Effect { .. }))
            .count()
    };
    let effects = moved(&core);

    // Routing and quoting an order asks the reserve its dry runs, and changes nothing at it: its books hold no
    // entry, it was handed nothing to write to its rail, and Core carried out no effect.
    let told = core
        .request(base_request("req-dry", AMOUNT, &session))
        .expect("opens");
    let order = the_order(&told);
    let quoted = core.quote(&kit::customer(), &order).expect("quoted");
    assert!(
        matches!(&quoted, Told::Exists { status: Status::Quoted { .. }, .. }),
        "{quoted:?}"
    );
    assert!(core.world().journal(kit::RESERVE).is_empty(), "no book entry");
    assert!(core.world().delivered().is_empty(), "nothing reached its rail");
    assert_eq!(moved(&core), effects, "Core carried nothing out");

    // A destination the register doesn't hold is refused at the request: nothing is asked of any provider.
    let asked = core.catalogue().check_log(&rutent_scenarios::assembly::context().account).len();
    let mut nowhere = base_request("req-nowhere", AMOUNT, &session);
    nowhere.destination.account = id("acct-of-no-one");
    nowhere.inputs.insert(
        "beneficiary.bank_account".to_owned(),
        "acct-of-no-one".to_owned(),
    );
    let told = core.request(nowhere).expect("answered");
    assert!(
        matches!(
            told,
            Told::Exists {
                status: Status::CantProceed {
                    class: rutent_contracts::core_internal::OrderRefusalClass::DestinationRefused
                },
                ..
            }
        ),
        "{told:?}"
    );
    assert_eq!(core.catalogue().check_log(&rutent_scenarios::assembly::context().account).len(), asked);

    // A destination the reserve itself can't deposit to is dropped at its dry run: no route, and still nothing
    // changed at the reserve.
    core.world()
        .script_dry_run(kit::RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let told = core
        .request(base_request("req-undepositable", AMOUNT, &session))
        .expect("opens");
    let order = the_order(&told);
    let quoted = core.quote(&kit::customer(), &order).expect("answered");
    assert!(
        matches!(&quoted, Told::Exists { status: Status::CantProceed { .. }, .. }),
        "{quoted:?}"
    );
    assert!(core.world().journal(kit::RESERVE).is_empty());
    assert_eq!(moved(&core), effects);
}

// verifies: AC-24, REQ-ORD-17
#[test]
fn requester_beneficiary_and_payer_are_recorded_apart_and_checked_by_their_providers_rules() {
    use rutent_contracts::core_internal::Parties;
    let (mut core, session) = core_with_a_session("AC-24");
    // Three parties, each with the account its role at the reserve names: the payer's, which the withdrawal's rules
    // name, and the beneficiary's, which the deposit's do.
    {
        let mut registers = core.registers();
        registers
            .accounts
            .insert("acct-payer".to_owned(), "acme-payer".to_owned());
        registers
            .accounts
            .insert("acct-payee".to_owned(), "acme-payee".to_owned());
    }
    let parties = Parties {
        requester: kit::customer().party,
        beneficiary: id("acme-payee"),
        settlement_account_at: None,
        payer: id("acme-payer"),
    };
    let apart = |name: &str| {
        let mut request = base_request(name, AMOUNT, &session);
        request.parties = parties.clone();
        request.destination.account = id("acct-payee");
        request.inputs.insert("payer.bank_account".to_owned(), "acct-payer".to_owned());
        request
            .inputs
            .insert("beneficiary.bank_account".to_owned(), "acct-payee".to_owned());
        request
    };
    let told = core.request(apart("req-apart")).expect("opens");
    let order = the_order(&told);
    let told = core.quote(&kit::customer(), &order).expect("quoted");
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("a quote is shown: {told:?}");
    };
    core.accept(&kit::customer(), &order, &digest).expect("accepted");
    let recorded = core
        .verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::Acceptance { order: o, parties, .. } if o == order => Some(parties),
            _ => None,
        })
        .expect("the acceptance records its parties");
    assert_eq!(recorded, parties, "each recorded apart, as stated");
    assert_ne!(recorded.requester, recorded.beneficiary);
    assert_ne!(recorded.requester, recorded.payer);
    assert_ne!(recorded.beneficiary, recorded.payer);
    assert_eq!(
        core.record(&order).expect("intake's record").parties,
        parties
    );

    // The reserve checks what its rules name: its withdrawal names the payer's account and its deposit the
    // beneficiary's. An order that doesn't give one is dropped at that provider's dry run, and none is offered.
    for (name, missing) in [("req-no-payer", "payer.bank_account"), ("req-no-payee", "beneficiary.bank_account")] {
        let mut request = apart(name);
        request.inputs.remove(missing);
        let told = core.request(request).expect("opens");
        let order = the_order(&told);
        let told = core.quote(&kit::customer(), &order).expect("answered");
        assert!(
            matches!(&told, Told::Exists { status: Status::CantProceed { .. }, .. }),
            "{name}: {told:?}"
        );
    }
}
