//! A second operation under a transaction the platform already carried out (`T-013-09`; `INV-8`, `REQ-CAT-10`,
//! `NFR-5`): refused for a provider whose advertisement declares no safe retry, with no module run and nothing
//! moved, and answered from the platform's record for one that declares it. The providers are the catalogue's
//! reserve (`safe_retry` false) and ramp (`safe_retry` true), both running the boundary suite's stand-in.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;
#[path = "holding_support/mod.rs"]
mod support;

use common::withdrawal;
use rutent_contracts::module_interface::{Identifier, PlatformRefusal, RouteReference};
use rutent_prov::catalogue::{Category, Grant, GrantTerms, Instant};
use rutent_prov::module::types::{Admitted, Handled};
use rutent_prov::proxy::NamedCall;
use signed::{AT, RAMP, RESERVE, Signed, release};

/// Admits `provider`'s first release, handling withdrawals, and gives it its acceptance: it runs.
fn admit(s: &mut Signed, provider: &str) {
    admit_handling(s, provider, &["withdrawal"]);
}

fn admit_handling(s: &mut Signed, provider: &str, handles: &[&str]) {
    let key = signed::catalogue_support::advertising_key(provider);
    let (declaration, binaries) = release(provider, "1.0.0", handles);
    let Admitted::Artefact(artefact) =
        s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
    else {
        panic!("the release wasn't admitted");
    };
    if artefact.acceptance.accepted.is_none() {
        s.accept(provider, &artefact.assertion_digest, "test-reviewer")
            .expect("the release is accepted");
    }
    assert_eq!(
        s.platform.active_version(provider).as_deref(),
        Some("1.0.0")
    );
}

/// A platform with `provider`'s first release in force.
fn running(provider: &str) -> Signed {
    let mut s = Signed::new();
    admit(&mut s, provider);
    s
}

fn tx(provider: &str, n: usize) -> Identifier {
    Identifier::new(format!("tx-{provider}-{n}")).expect("an identifier")
}

// verifies: INV-8, REQ-CAT-10
#[test]
fn second_operation_under_a_carried_out_transaction_is_refused_for_a_provider_without_safe_retry() {
    let mut s = running(RESERVE);
    // Held in a segment, as Core holds it, and not.
    s.platform
        .begin_segment(RESERVE, &tx(RESERVE, 1))
        .expect("a segment");
    for n in [1, 2] {
        let first = s.platform.handle(RESERVE, &withdrawal(RESERVE, n));
        assert!(matches!(first, Handled::Answered(_)), "{first:?}");
        assert_eq!(
            s.platform.operations_run(RESERVE),
            if n == 1 { 1 } else { 2 }
        );
        let before = s.platform.operations_run(RESERVE);

        let repeat = s.platform.handle(RESERVE, &withdrawal(RESERVE, n));
        assert_eq!(
            repeat,
            Handled::Refused(PlatformRefusal::DuplicateTransaction)
        );
        assert_eq!(
            s.platform.operations_run(RESERVE),
            before,
            "the module ran the repeat of transaction {n}"
        );
    }
    // The refusal is of the repeat only: another transaction is carried out.
    let other = s.platform.handle(RESERVE, &withdrawal(RESERVE, 3));
    assert!(matches!(other, Handled::Answered(_)), "{other:?}");
    // A dry run moves nothing, so it is no repeat: asked twice, it is answered twice.
    let mut dry = serde_json::to_value(withdrawal(RESERVE, 4)).expect("JSON");
    dry["statement"]["dry_run"] = true.into();
    let dry = serde_json::from_value(dry).expect("an operation");
    for _ in 0..2 {
        let answered = s.platform.handle(RESERVE, &dry);
        assert!(matches!(answered, Handled::Answered(_)), "{answered:?}");
    }
    // Once its segment ends the transaction is spent, as it was.
    s.platform
        .end_segment(tx(RESERVE, 1).as_str())
        .expect("its record");
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
}

// verifies: INV-8
#[test]
fn second_operation_under_a_carried_out_transaction_is_answered_from_the_record_for_a_provider_with_safe_retry()
 {
    let mut s = running(RAMP);
    s.platform
        .begin_segment(RAMP, &tx(RAMP, 1))
        .expect("a segment");
    for n in [1, 2] {
        let first = s.platform.handle(RAMP, &withdrawal(RAMP, n));
        assert!(matches!(first, Handled::Answered(_)), "{first:?}");
        let before = s.platform.operations_run(RAMP);

        // The first answer, byte for byte, and not a second run of the module.
        let repeat = s.platform.handle(RAMP, &withdrawal(RAMP, n));
        assert_eq!(repeat, first);
        assert_eq!(
            s.platform.operations_run(RAMP),
            before,
            "the module ran the repeat of transaction {n}"
        );
    }
}

// verifies: INV-8, REQ-CAT-10
#[test]
fn a_repeat_where_the_platform_cannot_tell_whether_a_repeat_is_safe_is_refused() {
    // A provider whose advertisement declares nothing of safe retry (a verifier's): the platform can't tell, and
    // a repeat is refused as it is for one that declares none.
    let provider = "id-check";
    let mut s = Signed::new();
    let ad = signed::catalogue_support::advertisement(
        "advertisement-verifier-without-evidence-or-retry-declarations",
    );
    s.catalogue
        .grant(
            Grant::new(GrantTerms {
                id: "g-verifier".into(),
                provider: provider.into(),
                party: "Acme Payments Ltd".into(),
                granted_by: "operator-admin".into(),
                granted_at: Instant(1_000),
                expires_at: Instant(4_000_000_000),
                categories: vec![Category::Verifier],
                tuples: Vec::new(),
                reversal_capacity: signed::catalogue_support::capacity(1_000, 500),
            })
            .expect("a grant"),
        )
        .expect("the grant is held");
    signed::catalogue_support::join_made_at(&mut s.catalogue, &ad, Instant(AT.0 - 86_400), AT)
        .expect("the advertisement joins");
    assert_eq!(
        s.catalogue.declared_by(provider).map(|d| d.safe_retry),
        Some(None)
    );
    admit(&mut s, provider);

    let first = s.platform.handle(provider, &withdrawal(provider, 1));
    assert!(matches!(first, Handled::Answered(_)), "{first:?}");
    assert_eq!(
        s.platform.handle(provider, &withdrawal(provider, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(s.platform.operations_run(provider), 1);
}

// verifies: INV-8
#[test]
fn operation_under_another_providers_carried_out_transaction_is_refused_even_where_it_declares_safe_retry()
 {
    let mut s = running(RESERVE);
    admit(&mut s, RAMP);
    let first = s.platform.handle(RESERVE, &withdrawal(RESERVE, 1));
    assert!(matches!(first, Handled::Answered(_)), "{first:?}");

    // The ramp, which declares a repeat safe, is asked the reserve's transaction: not a repeat of its own.
    let mut elsewhere = serde_json::to_value(withdrawal(RAMP, 1)).expect("JSON");
    elsewhere["statement"]["transaction"] = tx(RESERVE, 1).as_str().into();
    let elsewhere = serde_json::from_value(elsewhere).expect("an operation");
    assert_eq!(
        s.platform.handle(RAMP, &elsewhere),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(s.platform.operations_run(RAMP), 0);
}

// verifies: INV-8, REQ-CAT-10
#[test]
fn a_provider_that_withdraws_its_safe_retry_declaration_is_refused_from_its_next_release_on() {
    let mut s = running(RAMP);
    let first = s.platform.handle(RAMP, &withdrawal(RAMP, 1));
    assert_eq!(s.platform.handle(RAMP, &withdrawal(RAMP, 1)), first);

    // The provider advertises again, declaring a repeat unsafe, and releases again.
    let mut ad = s
        .catalogue
        .accepted_advertisement(RAMP)
        .expect("an accepted advertisement")
        .statement
        .clone();
    ad.declarations.safe_retry = Some(false);
    let key = signed::catalogue_support::advertising_key(RAMP);
    let bytes = signed::catalogue_support::advertisement_bytes(
        &ad,
        None,
        Instant(AT.0 - 86_400 + 10),
        &key,
    );
    s.catalogue.join(&bytes, AT).expect("the successor joins");
    assert_eq!(
        s.catalogue.declared_by(RAMP).map(|d| d.safe_retry),
        Some(Some(false))
    );
    let (declaration, binaries) = release(RAMP, "1.0.1", &["withdrawal"]);
    let admitted = s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries);
    assert!(signed::is_artefact(&admitted), "{admitted:?}");

    // What the platform reads is the declaration now in force: the first answer is no longer repeated.
    assert_eq!(
        s.platform.handle(RAMP, &withdrawal(RAMP, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
}

/// `value` as the operation it is.
fn operation(
    value: serde_json::Value,
) -> rutent_contracts::provider_role::Statement<rutent_contracts::provider_role::Operation> {
    serde_json::from_value(value).expect("an operation")
}

// verifies: INV-8, REQ-CAT-10
#[test]
fn a_retry_under_another_key_or_protocol_version_is_refused_for_a_provider_without_safe_retry() {
    for field in ["key", "protocol_version"] {
        let mut s = running(RESERVE);
        let first = s.platform.handle(RESERVE, &withdrawal(RESERVE, 1));
        assert!(matches!(first, Handled::Answered(_)), "{first:?}");
        let mut v = serde_json::to_value(withdrawal(RESERVE, 1)).expect("JSON");
        match field {
            "key" => {
                v["signature"]["key"] = rutent_conformance::role::placeholder_key(61)
                    .as_str()
                    .into();
            }
            _ => v["protocol_version"] = "9.0.0".into(),
        }
        // The platform verifies no signature, so the retry is what it looks like: a second operation.
        assert_eq!(
            s.platform.handle(RESERVE, &operation(v)),
            Handled::Refused(PlatformRefusal::DuplicateTransaction),
            "{field}"
        );
        assert_eq!(
            s.platform.operations_run(RESERVE),
            1,
            "{field}: the module ran the retry"
        );
    }
}

// verifies: INV-8
#[test]
fn a_retry_of_another_envelope_reaches_the_module_of_a_provider_with_safe_retry() {
    let mut s = running(RAMP);
    let first = s.platform.handle(RAMP, &withdrawal(RAMP, 1));
    assert!(matches!(first, Handled::Answered(_)), "{first:?}");
    let mut v = serde_json::to_value(withdrawal(RAMP, 1)).expect("JSON");
    v["protocol_version"] = "9.0.0".into();
    let again = s.platform.handle(RAMP, &operation(v));
    assert!(matches!(again, Handled::Answered(_)), "{again:?}");
    assert_eq!(s.platform.operations_run(RAMP), 2);
}

// verifies: INV-8
#[test]
fn another_kind_under_a_carried_out_transaction_is_refused() {
    let mut s = Signed::new();
    admit_handling(&mut s, RESERVE, &["withdrawal", "deposit"]);
    let first = s.platform.handle(RESERVE, &withdrawal(RESERVE, 1));
    assert!(matches!(first, Handled::Answered(_)), "{first:?}");
    let mut v = serde_json::to_value(withdrawal(RESERVE, 1)).expect("JSON");
    let origin = v["statement"]["origin"].clone();
    v["statement"]["origin"] = v["statement"]["destination"].clone();
    v["statement"]["destination"] = origin;
    v["statement"]["kind"] = "deposit".into();
    assert_eq!(
        s.platform.handle(RESERVE, &operation(v)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(s.platform.operations_run(RESERVE), 1);
}

/// `caller` is admitted and running, its module calling `callee`'s withdrawal twice, one after the other, when it
/// handles a withdrawal.
fn admit_caller(s: &mut Signed, caller: &str, callee: &str) {
    use rutent_conformance::boundary::{ModuleSpec, Output, Step};
    use rutent_contracts::module_interface::OperationKind;
    let language = common::languages().into_iter().next().expect("a language");
    let mut value =
        serde_json::to_value(common::declaration(caller, "1.0.0", &["withdrawal"])).expect("JSON");
    value["outside"]["statement_key"] =
        signed::catalogue_support::public_key(&signed::catalogue_support::statements_key(caller))
            .as_str()
            .into();
    value["outside"]["calls"] = serde_json::json!([{ "callee": callee, "kinds": ["withdrawal"] }]);
    let declaration = serde_json::from_value(value).expect("a declaration");
    let step = Step::CallModule {
        callee: callee.to_owned(),
        kind: OperationKind::Withdrawal,
    };
    let module = ModuleSpec {
        declaration,
        steps: vec![step.clone(), step],
        output: Output::Honest,
        starts: true,
    };
    let binaries = common::build_standin(&module, &language, "reserve", &[]).expect("builds");
    let key = signed::catalogue_support::advertising_key(caller);
    let Admitted::Artefact(artefact) = s.admit(
        &signed::assertion(&module.declaration, &binaries, &key),
        &binaries,
    ) else {
        panic!("the caller wasn't admitted");
    };
    if artefact.acceptance.accepted.is_none() {
        s.accept(caller, &artefact.assertion_digest, "test-reviewer")
            .expect("accepted");
    }
}

/// How many of the two calls `caller` makes into `callee` came back delivered, and how many operations the callee's
/// module ran, when the caller handles its withdrawal 1.
fn calls_made(caller: &str, callee: &str) -> (usize, u64, bool) {
    use rutent_contracts::module_interface::CrossModuleOutcome;
    let mut s = Signed::new();
    s.platform.set_operation_clock(std::sync::Arc::new(
        rutent_prov::module::platform::SteppedClock::new(),
    ));
    admit(&mut s, callee);
    admit_caller(&mut s, caller, callee);
    s.platform.take_events();
    // A route names the calls, as the boundary suite's does: every call the caller declares, in its transaction.
    let transaction = tx(caller, 1);
    s.platform.routes.name(
        &transaction,
        NamedCall {
            route: RouteReference {
                decision_record: Identifier::new("conformance-route").expect("an id"),
                segment: Identifier::new("segment-1").expect("an id"),
            },
            caller: Identifier::new(caller).expect("an id"),
            callee: Identifier::new(callee).expect("an id"),
            kinds: vec![rutent_contracts::module_interface::OperationKind::Withdrawal],
        },
    );
    let handled = s.platform.handle(caller, &withdrawal(caller, 1));
    let events = s.platform.take_events();
    assert_eq!(events.calls.len(), 2, "{handled:?}");
    eprintln!(
        "CALLS {:?}",
        events.calls.iter().map(|c| &c.outcome).collect::<Vec<_>>()
    );
    let delivered = events
        .calls
        .iter()
        .filter(|c| matches!(c.outcome, CrossModuleOutcome::Delivered(_)))
        .count();
    // The caller's own transaction is its own: a repeat of it is the caller's to be answered from its record, or
    // refused, as its declaration says, and not the callee's record's to decide.
    let again = s.platform.handle(caller, &withdrawal(caller, 1));
    (
        delivered,
        s.platform.operations_run(callee),
        matches!(again, Handled::Answered(_)),
    )
}

// verifies: INV-8, REQ-CAT-10
#[test]
fn a_second_call_into_a_callee_without_safe_retry_under_one_transaction_is_refused() {
    // The first call is delivered, which also shows a callee's record collides with nothing of the caller's.
    assert_eq!(calls_made(RAMP, RESERVE), (1, 1, true));
}

// verifies: INV-8
#[test]
fn a_second_call_into_a_callee_with_safe_retry_runs_it_again() {
    assert_eq!(calls_made(RESERVE, RAMP), (2, 2, false));
}
