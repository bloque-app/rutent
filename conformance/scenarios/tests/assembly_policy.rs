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

/// A decline of a move's outcome report through the assembly's policy reaches the engine, which has decided it since
/// `T-021-09`: the answer is the log's refusal (this order has no report to decline), as the report's and the
/// confirmation's are, and no longer unavailable (`T-024-19`).
// verifies: REQ-AUZ-4
#[test]
fn assembled_decline_of_a_stuck_outcome_is_decided_by_the_policy_engine() {
    use rutent_contracts::core_internal::calls::{AppendFailure, OutcomeDeclines};
    let core = accepted();
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/valid/verified-entry-of-an-outcome-report-declined.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let entry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let decline: rutent_contracts::core_internal::OutcomeDecline =
        serde_json::from_value(entry["what"]["request"].clone()).unwrap();
    let before = core.log().entries().len();
    let answer = core.policy().decline_outcome(&decline);
    assert!(
        matches!(answer, Err(AppendFailure::Refused(_))),
        "the engine decides it, and the log refuses a decline of nothing: {answer:?}"
    );
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

/// A read of the whole trail granted to, or revoked from, `grantee` by `by`, under `permission`, at the last entry's
/// time.
fn trail_change(
    core: &Core,
    request: &str,
    permission: &str,
    grantee: &Actor,
    by: &Actor,
) -> rutent_contracts::core_internal::PermissionChange {
    serde_json::from_value(serde_json::json!({
        "request": request,
        "permission": permission,
        "grantee": grantee,
        "scope": {"kind": "trail"},
        "by": by,
        "at": last(core).when.at,
    }))
    .unwrap()
}

/// A read of the whole trail by `reader`, naming `permission`, at the last entry's time.
fn trail_check(
    core: &Core,
    request: &str,
    reader: &Actor,
    permission: &str,
) -> rutent_contracts::core_internal::ReadCheck {
    serde_json::from_value(serde_json::json!({
        "request": request,
        "reader": reader,
        "reads": {"kind": "trail"},
        "permission": permission,
        "at": last(core).when.at,
    }))
    .unwrap()
}

/// The probe of finding 1 of `M3`'s milestone review, through the assembly (`T-021-14`): the customer's own login,
/// listed for its party in the customer's role and holding nothing, states the operator's role and grants itself a
/// read of the trail as a regulator, reads the trail by it and revokes it. Each is refused, and recorded refused; the
/// deployment's policy, which permits whatever the fixed rules let through, never permits it. The operator's login,
/// listed for the permissions to grant and to revoke a read, grants nothing to the customer's login stating a
/// regulator's role, grants a listed regulator the trail, the regulator reads it, other logins of the regulator's party
/// read nothing by it, the operator's login listed as a regulator too is granted nothing, and the customer's login
/// stating the operator's role can't revoke it; the operator's login revokes it.
// verifies: REQ-AUZ-7, REQ-AUD-12, THR-7
#[test]
fn customers_login_stating_the_operators_role_grants_reads_and_revokes_nothing_in_the_assembly() {
    use rutent_contracts::core_internal::ReadOutcome;
    use rutent_contracts::core_internal::calls::Reads;
    use rutent_core_authz::roles::Permission;

    let core = accepted();
    let customer = kit::customer();
    let as_operator = Actor {
        role: Role::Operator,
        ..customer.clone()
    };
    let as_regulator = Actor {
        role: Role::Regulator,
        ..customer.clone()
    };
    let mut policy = core.policy();

    let probe = trail_change(
        &core,
        "req-probe",
        "perm-probe",
        &as_regulator,
        &as_operator,
    );
    let granted = policy.grant_permission(&probe).unwrap();
    assert!(granted.permit().is_none(), "{granted:?}");
    assert!(matches!(last(&core).outcome, Outcome::Refused { .. }));
    assert_eq!(
        last(&core).who,
        as_operator,
        "recorded with the role it stated"
    );
    let read = policy
        .check_read(&trail_check(
            &core,
            "req-probe-read",
            &as_regulator,
            "perm-probe",
        ))
        .unwrap();
    assert!(matches!(read.outcome, ReadOutcome::Refused { .. }));
    let revoked = policy.revoke_permission(&probe).unwrap();
    assert!(revoked.permit().is_none());

    // The operator's login listed for both permissions, and a regulator listed in its role.
    let operator = kit::operator_login();
    let regulator = Actor {
        party: id("fin-regulator"),
        acts: id("examiner-3"),
        role: Role::Regulator,
        kind: ActorKind::Person,
    };
    core.registers().list(&regulator);
    core.registers().permit(&operator, Permission::GrantRead);
    core.registers().permit(&operator, Permission::RevokeRead);
    // Not to the customer's login stating a regulator's role: the register binds the role to the login.
    let to_the_customer = trail_change(&core, "req-to-cust", "perm-cust", &as_regulator, &operator);
    let granted = policy.grant_permission(&to_the_customer).unwrap();
    assert_eq!(
        granted.outcome,
        Answer::Refuse {
            class: RefusalClass::NoAccountableParty
        }
    );
    let grant = trail_change(&core, "req-grant", "perm-reg-1", &regulator, &operator);
    let granted = policy.grant_permission(&grant).unwrap();
    assert!(granted.permit().is_some(), "{granted:?}");
    let read = policy
        .check_read(&trail_check(&core, "req-read", &regulator, "perm-reg-1"))
        .unwrap();
    assert_eq!(read.outcome, ReadOutcome::Allowed {});
    // Fix round 1, findings 1 and 2: other logins of the regulator's party, each listed in its own role, read nothing
    // by its grant; the operator's login, listed as a regulator too, is granted nothing.
    for (acts, role, kind) in [
        ("fr-bot", Role::Verifier, ActorKind::Component),
        ("fr-ops", Role::Operator, ActorKind::Person),
    ] {
        let reader = Actor {
            acts: id(acts),
            role,
            kind,
            ..regulator.clone()
        };
        core.registers().list(&reader);
        let read = policy
            .check_read(&trail_check(
                &core,
                &format!("req-{acts}"),
                &reader,
                "perm-reg-1",
            ))
            .unwrap();
        assert!(
            matches!(read.outcome, ReadOutcome::Refused { .. }),
            "{acts}: {read:?}"
        );
    }
    let operator_as_regulator = Actor {
        role: Role::Regulator,
        ..operator.clone()
    };
    core.registers().list(&operator_as_regulator);
    let itself = trail_change(
        &core,
        "req-itself",
        "perm-itself",
        &operator_as_regulator,
        &operator,
    );
    assert!(policy.grant_permission(&itself).unwrap().permit().is_none());
    let stolen = trail_change(&core, "req-steal", "perm-reg-1", &regulator, &as_operator);
    assert!(
        policy
            .revoke_permission(&stolen)
            .unwrap()
            .permit()
            .is_none()
    );
    let revoke = trail_change(&core, "req-revoke", "perm-reg-1", &regulator, &operator);
    assert!(
        policy
            .revoke_permission(&revoke)
            .unwrap()
            .permit()
            .is_some()
    );
    let read = policy
        .check_read(&trail_check(
            &core,
            "req-read-after",
            &regulator,
            "perm-reg-1",
        ))
        .unwrap();
    assert!(matches!(read.outcome, ReadOutcome::Refused { .. }));
}

/// Policy's register of read permissions across a restart, through the assembly (`T-021-17`): the operator's listed
/// login grants a regulator the trail, revokes it and grants it again under a fresh id; Core is opened again on its
/// disk, and the reopened engine, its register rebuilt from the log, holds the first grant revoked and the second in
/// force, and never grants the revoked id again, as the engine that ran on would.
// verifies: REQ-AUZ-6, NFR-6
#[test]
fn read_grant_revoked_and_granted_again_before_core_reopens_is_held_through_the_assembly() {
    use rutent_contracts::core_internal::ReadOutcome;
    use rutent_contracts::core_internal::calls::Reads;
    use rutent_core_authz::roles::Permission;

    let scenario = Scenario::new(
        "one order, accepted",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    });
    let core = scenario.core(1, &mut Reference).0;
    let operator = kit::operator_login();
    let regulator = Actor {
        party: id("fin-regulator"),
        acts: id("examiner-3"),
        role: Role::Regulator,
        kind: ActorKind::Person,
    };
    // The deployment's registers are set again on each Core put together.
    let listed = |core: &Core| {
        core.registers().list(&regulator);
        core.registers().permit(&operator, Permission::GrantRead);
        core.registers().permit(&operator, Permission::RevokeRead);
    };
    listed(&core);
    {
        let mut policy = core.policy();
        let grant = trail_change(&core, "req-grant", "perm-reg-1", &regulator, &operator);
        assert!(policy.grant_permission(&grant).unwrap().permit().is_some());
        let revoke = trail_change(&core, "req-revoke", "perm-reg-1", &regulator, &operator);
        assert!(policy.revoke_permission(&revoke).unwrap().permit().is_some());
        let again = trail_change(&core, "req-again", "perm-reg-2", &regulator, &operator);
        assert!(policy.grant_permission(&again).unwrap().permit().is_some());
    }

    let core = scenario.reopen(core, 1, &mut Reference);
    listed(&core);
    let mut policy = core.policy();
    let revoked = policy
        .check_read(&trail_check(&core, "req-read-1", &regulator, "perm-reg-1"))
        .unwrap();
    assert!(
        matches!(revoked.outcome, ReadOutcome::Refused { .. }),
        "{revoked:?}"
    );
    let in_force = policy
        .check_read(&trail_check(&core, "req-read-2", &regulator, "perm-reg-2"))
        .unwrap();
    assert_eq!(in_force.outcome, ReadOutcome::Allowed {});
    let reused = trail_change(&core, "req-reused", "perm-reg-1", &regulator, &operator);
    assert!(
        policy.grant_permission(&reused).unwrap().permit().is_none(),
        "a revoked id is never granted again"
    );
}
