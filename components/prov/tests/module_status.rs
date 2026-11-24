//! The platform's status query (`T-013-11`; `REQ-ORD-1`, `INV-4`): a module whose release lists `status_query` in
//! `handles` is handed a question about a transaction the platform carried out at its provider, under a transaction
//! of the question's own, and its answer is passed on signed as any result is, the transaction asked about left as
//! it was. A module that lists none is never run. The module is the role stand-in of a reserve, which answers
//! every operation outside its role with a refusal of its kind: a valid answer, and one that says nothing of the
//! transaction, which is all these tests need of it.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;
#[path = "holding_support/mod.rs"]
mod support;

use common::{TestWorld, standins, withdrawal};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{ResultBody, StatusOutcome};
use rutent_prov::module::lifecycle::Staging;
use rutent_prov::module::record::MemoryRecord;
use rutent_prov::module::types::{Admitted, Handled};
use signed::{RESERVE, Signed, release};

/// Admits `provider`'s release of `version`, handling `handles`, whose sandboxed part is the reserve's role
/// stand-in, and gives it its acceptance.
fn admit(s: &mut Signed, provider: &str, version: &str, handles: &[&str]) {
    let key = signed::catalogue_support::advertising_key(provider);
    let (declaration, mut binaries) = release(provider, version, handles);
    binaries.sandboxed =
        std::fs::read(standins().join("role-reserve-sandboxed.wasm")).expect("the role stand-in");
    let Admitted::Artefact(artefact) =
        s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
    else {
        panic!("the release wasn't admitted");
    };
    if artefact.acceptance.accepted.is_none() {
        s.accept(provider, &artefact.assertion_digest, "test-reviewer")
            .expect("the release is accepted");
    }
}

fn id(text: &str) -> Identifier {
    Identifier::new(text).expect("an identifier")
}

/// The record's lines that name `transaction`.
fn mentioning(record: &MemoryRecord, transaction: &str) -> usize {
    record
        .lines()
        .iter()
        .filter(|line| line.contains(transaction))
        .count()
}

// verifies: REQ-ORD-1, INV-4
#[test]
fn status_question_is_handed_to_a_module_that_handles_it_and_moves_nothing() {
    let record = MemoryRecord::new();
    let mut s = Signed::on(Box::new(record.clone()), TestWorld::default());
    admit(&mut s, RESERVE, "1.0.0", &["withdrawal", "status_query"]);
    let about = withdrawal(RESERVE, 1);
    let about_tx = id(&format!("tx-{RESERVE}-1"));
    s.platform
        .begin_segment(RESERVE, &about_tx)
        .expect("a segment");
    assert!(matches!(
        s.platform.handle(RESERVE, &about),
        Handled::Answered(_)
    ));
    let run_before = s.platform.operations_run(RESERVE);
    let segment_before = s.platform.lifecycle().segment(about_tx.as_str()).cloned();
    assert!(s.platform.carries(about_tx.as_str()));

    let asking = id("status-question-1");
    let Handled::Answered(answer) = s.platform.status(RESERVE, &asking, &about) else {
        panic!("the module was not asked");
    };
    // The provider's answer, signed as a result of the platform's: the question's own transaction, naming the
    // transaction asked about.
    let ResultBody::StatusQuery(status) = &answer.statement else {
        panic!("not a status answer: {:?}", answer.statement);
    };
    assert_eq!(status.transaction, asking.as_str());
    assert_eq!(status.about, about_tx.as_str());
    assert!(matches!(status.outcome, StatusOutcome::Refused(_)));
    assert_eq!(answer.maker, RESERVE);
    assert_ne!(
        answer.signature.value,
        rutent_conformance::role::placeholder_signature(),
        "the answer is signed with the provider's key"
    );
    assert_eq!(s.platform.operations_run(RESERVE), run_before + 1);

    // It moved nothing: nothing was carried out under the question's transaction, none written to the record, and
    // the transaction asked about is as it was.
    assert!(!s.platform.carries(asking.as_str()));
    assert_eq!(mentioning(&record, asking.as_str()), 0);
    assert!(!s.platform.lifecycle().has_ended(asking.as_str()));
    assert!(s.platform.lifecycle().segment(asking.as_str()).is_none());
    assert!(s.platform.carries(about_tx.as_str()));
    assert_eq!(
        s.platform.lifecycle().segment(about_tx.as_str()).cloned(),
        segment_before
    );
    // Asked again, under another transaction, it is answered again; asked about the very transaction it asks
    // about, or one the platform holds, it isn't a question of its own.
    assert!(matches!(
        s.platform.status(RESERVE, &id("status-question-2"), &about),
        Handled::Answered(_)
    ));
    assert_eq!(
        s.platform.status(RESERVE, &about_tx, &about),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    // The transaction asked about is still the platform's: a repeat of its operation is refused, not run.
    assert_eq!(
        s.platform.handle(RESERVE, &about),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    // Nothing carried out under a transaction asked about, nothing asked.
    let never = withdrawal(RESERVE, 99);
    let run = s.platform.operations_run(RESERVE);
    assert_eq!(
        s.platform.status(RESERVE, &id("status-question-3"), &never),
        Handled::Refused(PlatformRefusal::Unavailable)
    );
    assert_eq!(s.platform.operations_run(RESERVE), run);
}

// verifies: REQ-ORD-1
#[test]
fn module_that_handles_no_status_question_is_not_asked_one() {
    let mut s = Signed::new();
    admit(&mut s, RESERVE, "1.0.0", &["withdrawal"]);
    let about = withdrawal(RESERVE, 1);
    assert!(matches!(
        s.platform.handle(RESERVE, &about),
        Handled::Answered(_)
    ));
    let run = s.platform.operations_run(RESERVE);
    let received = s.platform.operations_received(RESERVE);
    assert_eq!(
        s.platform.status(RESERVE, &id("status-question-1"), &about),
        Handled::Refused(PlatformRefusal::NotHandled)
    );
    // The module was never run: the platform refused it before it got there.
    assert_eq!(s.platform.operations_run(RESERVE), run);
    assert_eq!(s.platform.operations_received(RESERVE), received + 1);
}

/// A release staged on every new segment, beside the version in force, both handling status questions, and a
/// segment begun on the staged one in which a withdrawal was carried out.
fn staged_with_a_segment() -> (
    Signed,
    rutent_contracts::provider_role::Statement<rutent_contracts::provider_role::Operation>,
) {
    let mut s = Signed::new();
    admit(&mut s, RESERVE, "1.0.0", &["withdrawal", "status_query"]);
    s.platform
        .set_staging(Some(Staging::new(100, 0, 1).expect("a policy")));
    admit(&mut s, RESERVE, "1.1.0", &["withdrawal", "status_query"]);
    assert_eq!(s.platform.lifecycle().staged(RESERVE), Some("1.1.0"));
    let about = withdrawal(RESERVE, 1);
    let about_tx = id(&format!("tx-{RESERVE}-1"));
    assert_eq!(
        s.platform.begin_segment(RESERVE, &about_tx).as_deref(),
        Ok("1.1.0")
    );
    assert!(matches!(
        s.platform.handle(RESERVE, &about),
        Handled::Answered(_)
    ));
    (s, about)
}

// verifies: REQ-ORD-1
#[test]
fn status_question_runs_on_the_version_its_transaction_runs_on() {
    let (mut s, about) = staged_with_a_segment();
    assert_eq!(s.platform.operations_run_on(RESERVE, "1.1.0"), 1);
    assert!(matches!(
        s.platform.status(RESERVE, &id("status-question-1"), &about),
        Handled::Answered(_)
    ));
    // Not the version in force, which didn't carry the transaction out: the staged one, where the segment runs.
    assert_eq!(s.platform.operations_run_on(RESERVE, "1.1.0"), 2);
    assert_eq!(s.platform.operations_run_on(RESERVE, "1.0.0"), 0);
}

// verifies: REQ-MDL-4, REQ-ORD-1
#[test]
fn status_questions_count_for_nothing_towards_a_promotion() {
    let (mut s, about) = staged_with_a_segment();
    let counted = |s: &Signed| {
        let counts = s
            .platform
            .lifecycle()
            .observation(RESERVE)
            .expect("staged")
            .staged_counts;
        (counts.answered, counts.refused)
    };
    let before = counted(&s);
    for n in 1..=3 {
        let asking = id(&format!("status-question-{n}"));
        assert!(matches!(
            s.platform.status(RESERVE, &asking, &about),
            Handled::Answered(_)
        ));
    }
    assert_eq!(counted(&s), before);
    assert_eq!(s.platform.operations_run_on(RESERVE, "1.1.0"), 4);
}

/// `about`'s status question, as a caller that signed its own would hand it: the question itself.
fn question(
    provider: &str,
    asking: &str,
    about: &str,
) -> rutent_contracts::provider_role::Statement<rutent_contracts::provider_role::Operation> {
    let mut value = serde_json::to_value(withdrawal(provider, 1)).expect("JSON");
    value["statement"] = serde_json::json!({
        "kind": "status_query", "transaction": asking, "provider": provider, "about": about
    });
    serde_json::from_value(value).expect("a status question")
}

// verifies: REQ-ORD-1
#[test]
fn status_question_handed_to_handle_is_refused_and_the_module_not_run() {
    let mut s = Signed::new();
    admit(&mut s, RESERVE, "1.0.0", &["withdrawal", "status_query"]);
    let carried = withdrawal(RESERVE, 1);
    assert!(matches!(
        s.platform.handle(RESERVE, &carried),
        Handled::Answered(_)
    ));
    let run = s.platform.operations_run(RESERVE);
    // About a transaction carried out here, and about one never carried out anywhere: the same refusal, because the
    // checks are `Platform::status`'s alone.
    for about in [&format!("tx-{RESERVE}-1"), "tx-elsewhere-7"] {
        assert_eq!(
            s.platform
                .handle(RESERVE, &question(RESERVE, "status-question-1", about)),
            Handled::Refused(PlatformRefusal::NotHandled),
            "{about}"
        );
    }
    assert_eq!(s.platform.operations_run(RESERVE), run);
}

// verifies: REQ-ORD-1, INV-8
#[test]
fn status_question_about_an_unsegmented_transaction_runs_on_the_version_that_carried_it() {
    let mut s = Signed::new();
    admit(&mut s, RESERVE, "1.0.0", &["withdrawal", "status_query"]);
    let about = withdrawal(RESERVE, 1);
    assert!(matches!(
        s.platform.handle(RESERVE, &about),
        Handled::Answered(_)
    ));
    // Asked of the version that carried it out, which is in force: it runs.
    assert!(matches!(
        s.platform.status(RESERVE, &id("status-question-1"), &about),
        Handled::Answered(_)
    ));
    assert_eq!(s.platform.operations_run_on(RESERVE, "1.0.0"), 2);
    // The next release is put in force at once and nothing runs the first any longer: the question is refused, not
    // put to the version in force now, which never saw the transaction.
    admit(&mut s, RESERVE, "1.1.0", &["withdrawal", "status_query"]);
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.1.0"));
    assert_eq!(
        s.platform.status(RESERVE, &id("status-question-2"), &about),
        Handled::Refused(PlatformRefusal::Unavailable)
    );
    assert_eq!(s.platform.operations_run_on(RESERVE, "1.1.0"), 0);
}

// verifies: REQ-ORD-1
#[test]
fn status_question_about_a_segment_with_nothing_carried_out_in_it_is_refused() {
    let mut s = Signed::new();
    admit(&mut s, RESERVE, "1.0.0", &["withdrawal", "status_query"]);
    // Another transaction's operation is carried out; the one asked about has a segment and no operation in the record,
    // so its kind would be the caller's word.
    assert!(matches!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 2)),
        Handled::Answered(_)
    ));
    s.platform
        .begin_segment(RESERVE, &id(&format!("tx-{RESERVE}-1")))
        .expect("a segment");
    let run = s.platform.operations_run(RESERVE);
    assert_eq!(
        s.platform
            .status(RESERVE, &id("status-question-1"), &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::Unavailable)
    );
    assert_eq!(s.platform.operations_run(RESERVE), run);
}
