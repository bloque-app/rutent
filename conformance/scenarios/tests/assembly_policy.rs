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
        assert!(
            policy
                .revoke_permission(&revoke)
                .unwrap()
                .permit()
                .is_some()
        );
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

/// The regulator of these trail reads, and the operator's login that grants and revokes them, each listed, the login
/// holding the permissions to grant and to revoke a read (`T-021-14`).
fn trail_readers(core: &Core) -> (Actor, Actor) {
    use rutent_core_authz::roles::Permission;
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
    (regulator, operator)
}

/// The record of the last read of the trail the log took: its check and outcome.
fn last_read(core: &Core) -> (rutent_contracts::core_internal::ReadCheck, Outcome) {
    let entry = last(core);
    let What::CheckRead { check } = entry.what else {
        panic!("the last entry records a read: {:?}", entry.what)
    };
    (check, entry.outcome)
}

/// A regulator's read of the trail through Core's own entry point (`REQ-AUD-12`, `T-020-55` (3)): the operator's
/// login grants the regulator the trail, and the regulator, naming that permission, is handed the trail. Policy
/// recorded the read before anything was handed: the trail is the log's entries and their receipts, up to and including
/// that record and no further, and the read added that one entry. No statement's contents come with it: each stated
/// entry names its statement by commitment alone, and none of the evidence store's bytes or salts is in the trail.
// verifies: REQ-AUD-12
#[test]
fn regulator_holding_the_read_permission_is_handed_the_trail_and_the_read_is_recorded() {
    use rutent_contracts::core_internal::calls::Reads;
    use rutent_core_audit::measures::Handed;

    let mut core = accepted();
    let (regulator, operator) = trail_readers(&core);
    let grant = trail_change(&core, "req-grant", "perm-reg-1", &regulator, &operator);
    assert!(
        core.policy()
            .grant_permission(&grant)
            .unwrap()
            .permit()
            .is_some()
    );
    let before = core.log().len();
    let Handed::Trail(trail) = core.read_trail(&regulator, Some(id("perm-reg-1"))) else {
        panic!("a regulator holding the permission is handed the trail")
    };
    assert_eq!(core.log().len(), before + 1, "the read added its record");
    assert_eq!(
        trail.entries,
        core.log().entries().to_vec(),
        "the log's, whole"
    );
    assert_eq!(trail.entries.last().map(|(r, _)| r), Some(&trail.read));
    let (check, outcome) = last_read(&core);
    assert_eq!(outcome, Outcome::Permitted {});
    assert_eq!(check.reader, regulator, "who read");
    assert_eq!(
        check.permission,
        Some(id("perm-reg-1")),
        "by which permission"
    );
    assert_eq!(
        check.reads,
        rutent_contracts::core_internal::Reads::Trail {},
        "what"
    );
    // Statements by commitment alone: the trail's stated entries carry no bytes and no salt.
    assert!(
        trail
            .entries
            .iter()
            .any(|(_, e)| matches!(e, Entry::Stated(_))),
        "the run's statements are in the trail"
    );
    let text = serde_json::to_string(
        &trail
            .entries
            .iter()
            .map(|(_, e)| e.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for field in ["\"bytes\"", "\"salt\""] {
        assert!(!text.contains(field), "a statement's contents: {field}");
    }
}

/// A read of the trail through Core's entry point that policy doesn't allow hands over nothing of it (`REQ-AUD-12`,
/// `REQ-AUZ-7`, `T-020-55` (3)): a regulator naming no permission, or one not granted; another regulator naming the
/// first's grant; the customer's login; the deployment's policy refusing; and the regulator once its grant is revoked.
/// Each is recorded refused, and each is told the same, `Withheld`, with no class, receipt or entry, whoever it is and
/// whatever policy's reason.
// verifies: REQ-AUD-12, REQ-AUZ-7
#[test]
fn caller_without_the_read_permission_is_handed_nothing_of_the_trail() {
    use rutent_contracts::core_internal::calls::Reads;
    use rutent_core_audit::measures::Handed;

    let mut core = accepted();
    let (regulator, operator) = trail_readers(&core);
    let refused = |core: &mut Core, reader: &Actor, permission: Option<&str>| {
        let before = core.log().len();
        let handed = core.read_trail(reader, permission.map(id));
        assert_eq!(handed, Handed::Withheld, "{reader:?} by {permission:?}");
        assert_eq!(core.log().len(), before + 1, "the refusal is recorded");
        let (check, outcome) = last_read(core);
        assert_eq!(&check.reader, reader);
        assert!(matches!(outcome, Outcome::Refused { .. }), "{outcome:?}");
    };
    refused(&mut core, &regulator, None);
    refused(&mut core, &regulator, Some("perm-reg-1"));

    let grant = trail_change(&core, "req-grant", "perm-reg-1", &regulator, &operator);
    assert!(
        core.policy()
            .grant_permission(&grant)
            .unwrap()
            .permit()
            .is_some()
    );
    let other = Actor {
        party: id("other-regulator"),
        acts: id("examiner-9"),
        ..regulator.clone()
    };
    core.registers().list(&other);
    refused(&mut core, &other, Some("perm-reg-1"));
    refused(&mut core, &kit::customer(), Some("perm-reg-1"));
    core.deployment.refuse.set(Some(RefusalClass::NotPermitted));
    refused(&mut core, &regulator, Some("perm-reg-1"));
    core.deployment.refuse.set(None);

    let revoke = trail_change(&core, "req-revoke", "perm-reg-1", &regulator, &operator);
    assert!(
        core.policy()
            .revoke_permission(&revoke)
            .unwrap()
            .permit()
            .is_some()
    );
    refused(&mut core, &regulator, Some("perm-reg-1"));
}

/// Policy's log-backed answers are Core's own (`T-020-55` (1), finding 4): over an order the assembled Core opened,
/// accepted and ran, Core's sources answer all ten of Core's own, each checked for its value (`T-020-49`, the milestone
/// reviewer's check): its requester, origin reserve and parties, the order of a segment and the provider value moved
/// through on it, the maker of a statement, a provider's showing of its result, its keeping (about the order, and about
/// it or no order), and the origin reserve's two declarations as its advertisement makes them, each from the log and
/// the evidence store. With the deployment's registers emptied and its
/// verifier saying yes to anything, each of those answers is unchanged, as the deployment is asked none of them; the
/// registers' own answers, which are all the assembly supplies, change with them.
// verifies: REQ-AUZ-5
#[test]
fn policys_log_backed_answers_are_cores_and_the_assembly_supplies_only_the_deployments_registers() {
    use rutent_contracts::core_internal::{Attribution, Parties};
    use rutent_contracts::router_api::Id;
    use rutent_core_authz::engine::{Need, Reply, Said, Source, Sources};
    use rutent_core_authz::roles::{AUTOMATIC_DEPOSIT, NO_USER_WITHDRAWAL};
    use rutent_scenarios::assembly::policy::{Registers, Verifier};

    let (core, orders) = Scenario::new(
        "one order, run",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .when(Step::Run)
    .core(1, &mut Reference);
    let order = orders.first().cloned().expect("an order");
    let whats = core.verified_whats();
    let (parties, origin): (Parties, _) = whats
        .iter()
        .find_map(|w| match w {
            What::Acceptance {
                order: o,
                parties,
                origin_reserve,
                ..
            } if o == &order => Some((parties.clone(), origin_reserve.clone())),
            _ => None,
        })
        .expect("the order's acceptance");
    let (segment, provider) = whats
        .iter()
        .find_map(|w| match w {
            What::Effect {
                action:
                    rutent_contracts::core_internal::Action::MoveValue {
                        order: o,
                        segment,
                        provider,
                        ..
                    },
                ..
            } if o == &order => Some((segment.clone(), provider.clone())),
            _ => None,
        })
        .expect("a move of the order's value");
    let (commitment, maker) = core
        .log()
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s)
                if matches!(s.attribution, Attribution::Verified { .. })
                    && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(&order) =>
            {
                Some((s.commitment.clone(), s.attribution.maker().party.clone()))
            }
            _ => None,
        })
        .expect("a verified statement about the order");
    // A provider's verified result about one of the order's segments: what a showing names.
    let (shown_segment, showing) = core
        .log()
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s)
                if matches!(s.attribution, Attribution::Verified { .. })
                    && s.what.statement.as_str()
                        == rutent_contracts::provider_role::RESULT_SHAPE
                    && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(&order) =>
            {
                let about = s.about.as_ref()?;
                Some((
                    about.segment.clone()?,
                    rutent_contracts::core_internal::ShowingNamed {
                        transaction: about.transaction.clone()?,
                        commitment: s.commitment.clone(),
                    },
                ))
            }
            _ => None,
        })
        .expect("a provider's result about one of the order's segments");
    // The origin reserve's declaration, as its installed advertisement makes it.
    let declared = core
        .installed
        .iter()
        .find(|i| i.advertisement.statement().statement.provider == origin.as_str())
        .and_then(|i| i.advertisement.statement().statement.offers.reserve.clone())
        .map(|r| r.declaration)
        .expect("the origin reserve declares");
    let statement = commitment.clone();
    let need = |input: &Id, from: Source| Need {
        input: input.clone(),
        from,
    };
    let cores = vec![
        need(&parties.requester, Source::Requester(order.clone())),
        need(&origin, Source::OrderOrigin(order.clone())),
        need(&parties.payer, Source::OrderParties(order.clone())),
        need(&order, Source::SegmentOrder(segment.clone())),
        need(
            &provider,
            Source::SegmentProvider {
                order: order.clone(),
                segment,
            },
        ),
        need(&maker, Source::Statement(commitment.clone())),
        need(
            &order,
            Source::Showing {
                order: order.clone(),
                segment: shown_segment,
                showing,
                decided_at: core.clock.timestamp(),
            },
        ),
        need(&order, Source::Kept(commitment.clone())),
        need(&order, Source::KeptAbout(commitment)),
        need(
            &id(NO_USER_WITHDRAWAL),
            Source::Declarations(origin.clone()),
        ),
        need(&id(AUTOMATIC_DEPOSIT), Source::Declarations(origin.clone())),
    ];
    let said = |yes: bool| Reply::Given(if yes { Said::Yes } else { Said::No });
    let expected: Vec<Reply<Said>> = std::iter::repeat_n(Reply::Given(Said::Yes), 9)
        .chain([
            said(!declared.can_require_user_initiated_withdrawal),
            said(declared.starts_routes_on_automatic_deposits),
        ])
        .collect();
    let mut sources = core.sources();
    let before: Vec<Reply<Said>> = cores.iter().map(|n| sources.supply(n)).collect();
    let asked: std::collections::BTreeSet<String> = cores
        .iter()
        .map(|n| {
            format!("{:?}", n.from)
                .split(['(', ' ', '{'])
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(
        asked.len(),
        10,
        "every one of Core's ten sources is asked: {asked:?}"
    );
    assert_eq!(before, expected, "each answers its value");
    // Another party is no requester, and no maker of the statement.
    let other = id("someone-else");
    assert_eq!(
        sources.supply(&need(&other, Source::Requester(order.clone()))),
        Reply::Given(Said::No)
    );
    assert_eq!(
        sources.supply(&need(&other, Source::Statement(statement.clone()))),
        Reply::Given(Said::No)
    );
    // And a showing, a keeping and a keeping about another order are no one's: each answers no, so a source that said
    // yes to anything would fail here (`T-020-65`, the reviewer's Low on one-sided sources).
    let (showing_of, kept_about) = match (&cores[6].from, &cores[8].from) {
        (
            Source::Showing {
                segment, showing, ..
            },
            Source::KeptAbout(commitment),
        ) => ((segment.clone(), showing.clone()), commitment.clone()),
        other => panic!("the showing and the keeping: {other:?}"),
    };
    let another_order = id("another-order");
    assert_eq!(
        sources.supply(&need(
            &another_order,
            Source::Showing {
                order: another_order.clone(),
                segment: showing_of.0,
                showing: showing_of.1,
                decided_at: core.clock.timestamp(),
            },
        )),
        Reply::Given(Said::No),
        "a showing of another order"
    );
    assert_eq!(
        sources.supply(&need(&another_order, Source::Kept(kept_about.clone()))),
        Reply::Given(Said::No),
        "kept about another order"
    );
    assert_eq!(
        sources.supply(&need(&another_order, Source::KeptAbout(kept_about))),
        Reply::Given(Said::No),
        "kept about another order"
    );
    assert_eq!(
        sources.order_record(&order),
        Reply::Given(parties.clone()),
        "the order's record, from the log"
    );
    let customer = kit::customer();
    let acts = need(
        &customer.acts,
        Source::ActsFor {
            acts: customer.acts.clone(),
            party: customer.party.clone(),
            kind: customer.kind,
            role: customer.role,
        },
    );
    assert_eq!(sources.supply(&acts), Reply::Given(Said::Yes));

    // The deployment's registers emptied, its verifier saying yes to anything: Core's answers stand, the registers'
    // change.
    *core.registers() = Registers::default();
    core.verifier_is(Verifier::Yes);
    let after: Vec<Reply<Said>> = cores.iter().map(|n| sources.supply(n)).collect();
    assert_eq!(after, before, "Core's own answers are no register's");
    assert_eq!(sources.order_record(&order), Reply::Given(parties));
    assert_eq!(sources.supply(&acts), Reply::Missing, "the register's own");
}

/// An order run through the assembled Core, the origin reserve its acceptance names, and the advertisement that
/// reserve's install sent, as JSON.
fn run_with_origins_advertisement(
    given: Given,
) -> (Core, rutent_contracts::router_api::Id, serde_json::Value) {
    let (core, _) = Scenario::new("one order", "AC-28", given)
        .when(Step::Order {
            request: "req-1".to_owned(),
            amount: AMOUNT.to_owned(),
        })
        .core(1, &mut Reference);
    let origin = core
        .verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::Acceptance { origin_reserve, .. } => Some(origin_reserve),
            _ => None,
        })
        .expect("the order's acceptance");
    let ad = core
        .installed
        .iter()
        .find(|i| i.advertisement.statement().statement.provider == origin.as_str())
        .map(|i| serde_json::from_slice(i.advertisement.bytes()).expect("JSON"))
        .expect("the origin reserve installed");
    (core, origin, ad)
}

/// Core's answers to the origin reserve's two declarations.
fn declared(
    core: &Core,
    reserve: &rutent_contracts::router_api::Id,
) -> Vec<rutent_core_authz::engine::Reply<rutent_core_authz::engine::Said>> {
    use rutent_core_authz::engine::{Need, Source, Sources};
    use rutent_core_authz::roles::{AUTOMATIC_DEPOSIT, NO_USER_WITHDRAWAL};
    let mut sources = core.sources();
    [NO_USER_WITHDRAWAL, AUTOMATIC_DEPOSIT]
        .into_iter()
        .map(|d| {
            sources.supply(&Need {
                input: id(d),
                from: Source::Declarations(reserve.clone()),
            })
        })
        .collect()
}

/// The operator's listed login authorising the order's move for its user, under `ActForUser`, as a request names it.
fn operator_authorises(core: &Core, request: &str) -> Answer {
    let operator = kit::operator_login();
    core.registers().list(&operator);
    core.registers()
        .permit(&operator, rutent_core_authz::roles::Permission::ActForUser);
    let mut a = authorisation(core);
    a.authorisation = id(request);
    a.given_by = operator;
    core.policy().authorise(&a).unwrap().outcome
}

/// The signing key behind `provider`'s advertising key, as its install resolves the stand-in's placeholder.
fn advertising_signer(core: &Core, provider: &str) -> ed25519_dalek::SigningKey {
    let installed = core
        .installed
        .iter()
        .find(|i| i.spec.provider == provider)
        .expect("installed");
    let placeholder = installed
        .spec
        .make(&rutent_scenarios::assembly::context())
        .advertisement()
        .into_statement()
        .statement
        .keys
        .into_iter()
        .find(|k| k.purpose == rutent_contracts::provider_role::KeyPurpose::Advertising)
        .expect("an advertising key")
        .public_key;
    rutent_conformance::signing::real(placeholder.as_str())
}

/// `ad` with both of its reserve declarations set to let the operator act for the user and start on a deposit.
fn flipped(mut ad: serde_json::Value) -> serde_json::Value {
    let declaration = &mut ad["statement"]["offers"]["reserve"]["declaration"];
    declaration["can_require_user_initiated_withdrawal"] = serde_json::json!(false);
    declaration["starts_routes_on_automatic_deposits"] = serde_json::json!(true);
    ad
}

/// A forged copy of the origin reserve's advertisement (the review of `8f7ddefd`, probe 1): its declarations flipped,
/// made later, its signature garbage, naming the reserve's granted advertising key. The log takes the stated entry and
/// its key register refuses the advertisement, so Core's sources read nothing of it: the reserve's declarations, and
/// the operator's authorisation for the user that rests on them, are as before.
// verifies: REQ-AUZ-9, REQ-CAT-6
#[test]
fn forged_advertisement_under_a_granted_key_changes_no_declaration() {
    let (mut core, origin, ad) = run_with_origins_advertisement(Given::one_reserve(Staged::AtOnce));
    let before = declared(&core, &origin);
    let refused = operator_authorises(&core, "auth-before");
    assert!(matches!(refused, Answer::Refuse { .. }), "{refused:?}");

    let mut forged = flipped(ad);
    forged["made_at"] = serde_json::json!("2030-01-01T00:00:00Z");
    forged["signature"]["value"] = serde_json::json!("A".repeat(86));
    core.append_advertisement(&serde_json::to_vec(&forged).unwrap())
        .expect("the log takes the stated entry");

    assert_eq!(
        declared(&core, &origin),
        before,
        "a forged advertisement declares nothing"
    );
    assert_eq!(operator_authorises(&core, "auth-after"), refused);
}

/// Another granted reserve's advertisement naming the origin reserve as its provider (the review of `8f7ddefd`, probe
/// 2), with the declarations flipped: once carrying the other reserve's own signature as it stands, once signed anew,
/// validly, with the other reserve's own advertising key. The key register accepts neither as the origin reserve's,
/// so Core's sources read nothing of either.
// verifies: REQ-AUZ-9, REQ-CAT-6
#[test]
fn another_providers_advertisement_naming_the_reserve_changes_no_declaration() {
    use rutent_conformance::signing::{public_key, sign_value};
    let mut given = Given::one_reserve(Staged::AtOnce);
    given.providers.push(rutent_conformance::sim::Spec::of(
        rutent_conformance::sim::Role::Reserve,
        "sim-reserve-b",
    ));
    let (mut core, origin, ad) = run_with_origins_advertisement(given);
    let before = declared(&core, &origin);
    let refused = operator_authorises(&core, "auth-before");
    let other: serde_json::Value = core
        .installed
        .iter()
        .find(|i| i.advertisement.statement().statement.provider != origin.as_str())
        .map(|i| serde_json::from_slice(i.advertisement.bytes()).expect("JSON"))
        .expect("another reserve installed");
    let other_maker = other["maker"].as_str().unwrap().to_owned();

    let mut copied = flipped(ad);
    copied["maker"] = other["maker"].clone();
    copied["signature"] = other["signature"].clone();
    copied["statement"]["publisher"] = other["statement"]["publisher"].clone();
    core.append_advertisement(&serde_json::to_vec(&copied).unwrap())
        .expect("the log takes the stated entry");
    assert_eq!(declared(&core, &origin), before, "its signature, copied");

    let key = advertising_signer(&core, &other_maker);
    assert_eq!(
        other["signature"]["key"].as_str(),
        Some(public_key(&key).as_str()),
        "the other reserve's advertising key"
    );
    let mut signed = copied;
    signed["made_at"] = serde_json::json!(core.clock.timestamp());
    sign_value(&mut signed, &key);
    core.append_advertisement(&serde_json::to_vec(&signed).unwrap())
        .expect("the log takes the stated entry");
    assert_eq!(
        declared(&core, &origin),
        before,
        "signed anew by the other reserve"
    );
    assert_eq!(operator_authorises(&core, "auth-after"), refused);
}

/// The origin reserve advertises anew, validly, its declarations flipped, and the key register accepts it: Core's
/// sources read the new declarations. Its older advertisement, replayed after it, is refused by the register as not
/// newer than the one it accepted, so Core's sources still read the newer one.
// verifies: REQ-AUZ-9, REQ-CAT-6
#[test]
fn old_advertisement_replayed_after_a_newer_accepted_one_changes_nothing() {
    use rutent_conformance::signing::{public_key, sign_value};
    use rutent_core_authz::engine::{Reply, Said};
    let (mut core, origin, ad) = run_with_origins_advertisement(Given::one_reserve(Staged::AtOnce));
    let original = serde_json::to_vec(&ad).unwrap();
    let key = advertising_signer(&core, origin.as_str());
    assert_eq!(
        ad["signature"]["key"].as_str(),
        Some(public_key(&key).as_str()),
        "the reserve's advertising key"
    );
    let before = declared(&core, &origin);
    assert_eq!(before, vec![Reply::Given(Said::No), Reply::Given(Said::No)]);

    let mut newer = flipped(ad);
    newer["made_at"] = serde_json::json!(core.clock.timestamp());
    sign_value(&mut newer, &key);
    core.append_advertisement(&serde_json::to_vec(&newer).unwrap())
        .expect("the log takes the stated entry");
    let accepted = declared(&core, &origin);
    assert_eq!(
        accepted,
        vec![Reply::Given(Said::Yes), Reply::Given(Said::Yes)],
        "the newer advertisement, accepted, declares"
    );

    core.append_advertisement(&original)
        .expect("the log takes the stated entry");
    assert_eq!(
        declared(&core, &origin),
        accepted,
        "the replayed older one changes nothing"
    );

    // Core reopened rebuilds its catalogue from the advertisement the log's key register accepted, the newer one, and
    // not from the older one the log holds after it, nor from the installed bytes (`T-020-61`).
    let core = Scenario::new("one order", "AC-28", Given::one_reserve(Staged::AtOnce)).reopen(
        core,
        1,
        &mut Reference,
    );
    assert_eq!(
        core.catalogue()
            .accepted_advertisement(origin.as_str())
            .map(|a| a.made_at.clone()),
        newer["made_at"].as_str().map(str::to_owned),
        "the register's accepted advertisement is the one listed again"
    );
}

/// Core's `SegmentOrder` against the log, segment by segment (`T-020-55`, the review of `8f7ddefd`: no scenario asked
/// it): over an order run through the assembled Core, each segment of each `route_chosen` the log holds is its order's
/// and no other's, and a segment no plan lists is nothing.
// verifies: REQ-AUZ-7
#[test]
fn segment_order_answer_is_the_logs_plan_for_every_segment() {
    use rutent_contracts::state_machine::Observed;
    use rutent_core_authz::engine::{Need, Reply, Said, Source, Sources};
    let (core, _) = Scenario::new(
        "one order, run",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .when(Step::Run)
    .core(1, &mut Reference);
    let planned: Vec<(String, String)> = core
        .verified_whats()
        .into_iter()
        .filter_map(|w| match w {
            What::Input { input } => match &input.observed {
                Observed::RouteChosen { parts, .. } => Some(
                    parts
                        .iter()
                        .flat_map(|p| &p.segments)
                        .map(|s| {
                            (
                                s.segment_id.as_str().to_owned(),
                                input.subject.order.as_str().to_owned(),
                            )
                        })
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            },
            _ => None,
        })
        .flatten()
        .collect();
    assert!(!planned.is_empty(), "the log holds a plan");
    let mut sources = core.sources();
    let ask = |sources: &mut rutent_scenarios::assembly::policy::CoreSources,
               segment: &str,
               order: &str| {
        sources.supply(&Need {
            input: id(order),
            from: Source::SegmentOrder(id(segment)),
        })
    };
    for (segment, order) in &planned {
        assert_eq!(
            ask(&mut sources, segment, order),
            Reply::Given(Said::Yes),
            "{segment}"
        );
        assert_eq!(
            ask(&mut sources, segment, "another-order"),
            Reply::Given(Said::No),
            "{segment}"
        );
    }
    assert_eq!(
        ask(&mut sources, "no-such-segment", &planned[0].1),
        Reply::Missing
    );
}

/// Core's `Statement` against the log, statement by statement (`T-020-55`, the review of `8f7ddefd`: no scenario asked
/// it): each commitment a verified stated entry names is its first such entry's maker's, as the log attributed it, and
/// no other party's; one only unverified entries name (an advertisement, signed by an advertising key) has no maker Core
/// answers for (`T-020-49`, the milestone reviewer's check); a commitment the log names nowhere is nothing.
// verifies: REQ-AUD-5
#[test]
fn statement_answer_is_the_logs_maker_for_every_statement() {
    use rutent_core_authz::engine::{Need, Reply, Said, Source, Sources};
    let (core, _) = Scenario::new(
        "one order, run",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .when(Step::Run)
    .core(1, &mut Reference);
    let mut first: Vec<(
        rutent_contracts::module_interface::Digest,
        rutent_contracts::router_api::Id,
    )> = Vec::new();
    let mut claimed_only: Vec<rutent_contracts::module_interface::Digest> = Vec::new();
    for (_, e) in core.log().entries() {
        if let Entry::Stated(s) = e {
            match &s.attribution {
                rutent_contracts::core_internal::Attribution::Verified { who }
                    if !first.iter().any(|(c, _)| c == &s.commitment) =>
                {
                    first.push((s.commitment.clone(), who.party.clone()));
                }
                rutent_contracts::core_internal::Attribution::Unverified { .. }
                    if !claimed_only.contains(&s.commitment) =>
                {
                    claimed_only.push(s.commitment.clone());
                }
                _ => {}
            }
        }
    }
    claimed_only.retain(|c| !first.iter().any(|(f, _)| f == c));
    assert!(first.len() > 1, "the log holds statements");
    assert!(
        !claimed_only.is_empty(),
        "and statements only claimed, its advertisements among them"
    );
    let mut sources = core.sources();
    for commitment in &claimed_only {
        let entries = core.log().entries().to_vec();
        let claimed = entries
            .iter()
            .find_map(|(_, e)| match e {
                Entry::Stated(s) if &s.commitment == commitment => {
                    Some(s.attribution.maker().party.clone())
                }
                _ => None,
            })
            .expect("its claim");
        assert_eq!(
            sources.supply(&Need {
                input: claimed,
                from: Source::Statement(commitment.clone()),
            }),
            Reply::Missing,
            "an unverified claim answers for no maker: {commitment:?}"
        );
    }
    for (commitment, party) in &first {
        let ask = |sources: &mut rutent_scenarios::assembly::policy::CoreSources, who: &str| {
            sources.supply(&Need {
                input: id(who),
                from: Source::Statement(commitment.clone()),
            })
        };
        assert_eq!(
            ask(&mut sources, party.as_str()),
            Reply::Given(Said::Yes),
            "{commitment:?}"
        );
        assert_eq!(
            ask(&mut sources, "no-such-party"),
            Reply::Given(Said::No),
            "{commitment:?}"
        );
    }
    let unknown: rutent_contracts::module_interface::Digest =
        serde_json::from_value(serde_json::json!({
            "algorithm": "sha-256",
            "value": "0".repeat(64),
        }))
        .unwrap();
    assert_eq!(
        sources.supply(&Need {
            input: id("anyone"),
            from: Source::Statement(unknown),
        }),
        Reply::Missing
    );
}
