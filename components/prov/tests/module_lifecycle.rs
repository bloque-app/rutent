//! The release lifecycle through the platform (`T-013-06`; `REQ-MDL-4`, `REQ-MDL-5`, `REQ-MDL-10`, `REQ-MDL-14`,
//! `REQ-MDL-19`, `REQ-MDL-20`, `AC-19`, `NFR-2`, `INV-8`, ADR-0014): releases staged on a share of new segments,
//! observed, promoted or rolled back; a failed upgrade; segments pinned to their version; stale, revoked and
//! exploited versions; per-transaction state; and a waiting release withdrawn. The modules are the boundary
//! suite's stand-ins, whose answer names the version that gave it.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;
#[path = "holding_support/mod.rs"]
mod support;

use common::{TestWorld, WasmBinding, binding, install, languages, spec, withdrawal};
use rutent_conformance::boundary::{BoundaryHarness, ModuleBinding, Output, Step};
use rutent_contracts::module_interface::{
    AcceptanceMode, AdmissionRefusal, Identifier, PartName, PlatformRefusal,
};
use rutent_prov::catalogue::Instant;
use rutent_prov::module::lifecycle::{
    DEFAULT_STAGING, DecisionKind, LifecycleRefusal, Mark, MoveReason, Staging,
};
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::{self, Admitted};
use signed::{AT, RESERVE, Signed, release};

const OPERATOR: &str = "operator-on-call";

/// The language the stand-ins are built in: Rust, if the tests run it.
fn language() -> String {
    let languages = languages();
    if languages.iter().any(|l| l == "rs") {
        "rs".to_owned()
    } else {
        languages[0].clone()
    }
}

/// The transaction of [`withdrawal`] `n` of `provider`.
fn tx(provider: &str, n: usize) -> Identifier {
    Identifier::new(format!("tx-{provider}-{n}")).expect("an identifier")
}

/// The version of the provider's module that answered its withdrawal `n`, by the answer's evidence reference, or
/// the platform's refusal.
fn answer(
    platform: &mut Platform<TestWorld>,
    provider: &str,
    n: usize,
) -> Result<String, PlatformRefusal> {
    match platform.handle(provider, &withdrawal(provider, n)) {
        types::Handled::Answered(result) => {
            let value = serde_json::to_value(&result.statement).expect("JSON");
            let reference = value["outcome"]["evidence"][0]["reference"]
                .as_str()
                .expect("a reference");
            let version = reference
                .strip_prefix(&format!("{provider}-"))
                .and_then(|rest| rest.split('-').next())
                .expect("the version");
            Ok(version.to_owned())
        }
        types::Handled::Refused(refusal) => Err(refusal),
    }
}

/// Builds and admits the provider's `version`, starting or not, and gives it its fresh acceptance if it waits.
fn upgrade(binding: &mut WasmBinding, provider: &str, version: &str, starts: bool) {
    let module = spec(provider, version, Vec::new(), starts);
    let admitted = install(binding, &module, &language(), "reserve", &[]);
    assert!(
        matches!(
            admitted,
            rutent_conformance::boundary::Admitted::Artefact(_)
        ),
        "{provider} {version}: {admitted:?}"
    );
}

/// Stages every upgrade on `share` percent of new segments, for `window` seconds and `minimum` operations.
fn stage(binding: &mut WasmBinding, share: u8, window: u64, minimum: u64) {
    binding.set_staging(Some(
        Staging::new(share, window, minimum).expect("a policy"),
    ));
}

fn later(seconds: u64) -> Instant {
    Instant(AT.0 + seconds)
}

// verifies: AC-19, REQ-MDL-4
#[test]
fn failed_upgrade_leaves_the_previous_version_in_force() {
    let provider = "upgraded";
    let mut binding = binding("reserve");
    upgrade(&mut binding, provider, "1.0.0", true);
    let pid = binding.process(provider);

    // An upgrade whose module doesn't start: accepted, but the version in force stays, in its own process.
    upgrade(&mut binding, provider, "1.1.0", false);
    assert_eq!(binding.active_version(provider).as_deref(), Some("1.0.0"));
    assert_eq!(binding.process(provider), pid);
    assert_eq!(answer(&mut binding, provider, 1).as_deref(), Ok("1.0.0"));

    // Staged, the same: a release that doesn't start is never staged.
    stage(&mut binding, 50, 0, 1);
    upgrade(&mut binding, provider, "1.2.0", false);
    assert_eq!(binding.lifecycle().staged(provider), None);
    assert_eq!(binding.active_version(provider).as_deref(), Some("1.0.0"));

    // A release that starts but answers badly is staged on half the new segments, observed, and rolled back.
    let mut bad = spec(provider, "1.3.0", Vec::new(), true);
    bad.output = Output::Unparseable;
    install(&mut binding, &bad, &language(), "reserve", &[]);
    assert_eq!(binding.lifecycle().staged(provider), Some("1.3.0"));
    assert_eq!(binding.active_version(provider).as_deref(), Some("1.0.0"));
    assert_eq!(
        binding.process(provider),
        pid,
        "staging replaced the process"
    );
    assert_eq!(
        binding
            .begin_segment(provider, &tx(provider, 10))
            .as_deref(),
        Ok("1.0.0")
    );
    assert_eq!(
        binding
            .begin_segment(provider, &tx(provider, 11))
            .as_deref(),
        Ok("1.3.0")
    );
    assert_eq!(answer(&mut binding, provider, 10).as_deref(), Ok("1.0.0"));
    assert_eq!(
        answer(&mut binding, provider, 11),
        Err(PlatformRefusal::MalformedOutput)
    );
    let observed = binding
        .lifecycle()
        .observation(provider)
        .expect("an observation");
    assert_eq!(
        (
            observed.staged_counts.refused,
            observed.staged_counts.answered
        ),
        (1, 0)
    );
    assert_eq!(observed.in_force_counts.answered, 1);

    assert_eq!(
        binding.roll_back(provider, OPERATOR, AT).as_deref(),
        Ok("1.3.0")
    );
    assert_eq!(binding.lifecycle().staged(provider), None);
    assert_eq!(
        binding.lifecycle().mark(provider, "1.3.0"),
        Some(Mark::Revoked)
    );
    assert_eq!(binding.active_version(provider).as_deref(), Some("1.0.0"));
    assert_eq!(binding.process(provider), pid);
    // New segments run on the version in force; the one on the rolled back release finishes on it.
    assert_eq!(
        binding
            .begin_segment(provider, &tx(provider, 12))
            .as_deref(),
        Ok("1.0.0")
    );
    assert_eq!(answer(&mut binding, provider, 12).as_deref(), Ok("1.0.0"));
    assert!(binding.version_process(provider, "1.3.0").is_some());
    let record = binding
        .end_segment(tx(provider, 11).as_str())
        .expect("its record");
    assert_eq!(
        (record.began_on.as_str(), record.runs_on.as_str()),
        ("1.3.0", "1.3.0")
    );
    assert!(
        binding.version_process(provider, "1.3.0").is_none(),
        "the rolled back release still runs with nothing on it"
    );
    let last = binding.lifecycle().decisions().last().expect("a decision");
    assert_eq!(
        (last.kind, last.version.as_str(), last.by.as_str()),
        (DecisionKind::RolledBack, "1.3.0", OPERATOR)
    );
}

// verifies: AC-19, NFR-2
#[test]
fn upgrade_of_one_provider_interrupts_no_order_at_another() {
    let (upgraded, steady) = ("upgraded", "steady");
    let mut binding = binding("reserve");
    upgrade(&mut binding, upgraded, "1.0.0", true);
    upgrade(&mut binding, steady, "1.0.0", true);
    let steady_pid = binding.process(steady);
    binding
        .begin_segment(steady, &tx(steady, 1))
        .expect("a segment at the steady provider");
    binding
        .begin_segment(upgraded, &tx(upgraded, 1))
        .expect("a segment at the upgraded provider");
    assert_eq!(answer(&mut binding, steady, 1).as_deref(), Ok("1.0.0"));

    // Staged, promoted, and then an upgrade that fails: through each, the steady provider's order goes on in
    // the same process, and so does the upgraded provider's segment that began before.
    stage(&mut binding, 100, 0, 1);
    upgrade(&mut binding, upgraded, "1.1.0", true);
    let check = |binding: &mut WasmBinding, n: usize| {
        assert_eq!(answer(binding, steady, 1).as_deref(), Ok("1.0.0"));
        assert_eq!(answer(binding, steady, n).as_deref(), Ok("1.0.0"));
        assert_eq!(binding.process(steady), steady_pid);
        assert_eq!(answer(binding, upgraded, 1).as_deref(), Ok("1.0.0"));
    };
    check(&mut binding, 2);
    binding
        .begin_segment(upgraded, &tx(upgraded, 2))
        .expect("a segment on the staged release");
    assert_eq!(answer(&mut binding, upgraded, 2).as_deref(), Ok("1.1.0"));
    assert_eq!(
        binding.promote(upgraded, OPERATOR, AT).as_deref(),
        Ok("1.1.0")
    );
    check(&mut binding, 3);
    binding.set_staging(None);
    upgrade(&mut binding, upgraded, "1.2.0", false);
    check(&mut binding, 4);
    assert_eq!(binding.active_version(upgraded).as_deref(), Some("1.1.0"));
    // The segment that began on 1.0.0 ran on it to its end, and its version stops with it.
    let record = binding
        .end_segment(tx(upgraded, 1).as_str())
        .expect("its record");
    assert_eq!(
        (record.began_on.as_str(), record.runs_on.as_str()),
        ("1.0.0", "1.0.0")
    );
    assert!(record.moves.is_empty());
    assert!(binding.version_process(upgraded, "1.0.0").is_none());
    assert_eq!(binding.process(steady), steady_pid);
}

// verifies: REQ-MDL-4
#[test]
fn low_volume_release_stays_staged_until_its_minimum_is_observed() {
    let provider = "quiet";
    let mut binding = binding("reserve");
    upgrade(&mut binding, provider, "1.0.0", true);
    stage(&mut binding, 100, 60, 3);
    upgrade(&mut binding, provider, "1.1.0", true);
    assert_eq!(binding.lifecycle().staged(provider), Some("1.1.0"));

    assert_eq!(
        binding.promote(provider, OPERATOR, AT),
        Err(LifecycleRefusal::WindowOpen { until: later(60) })
    );
    assert_eq!(
        binding.promote(provider, OPERATOR, later(60)),
        Err(LifecycleRefusal::TooFewObserved {
            observed: 0,
            minimum: 3
        })
    );
    // Two segments in, a day past the window, it is still staged and the version in force stays.
    for n in 1..=2 {
        assert_eq!(
            binding.begin_segment(provider, &tx(provider, n)).as_deref(),
            Ok("1.1.0")
        );
        assert_eq!(answer(&mut binding, provider, n).as_deref(), Ok("1.1.0"));
    }
    assert_eq!(
        binding.promote(provider, OPERATOR, later(86_400)),
        Err(LifecycleRefusal::TooFewObserved {
            observed: 2,
            minimum: 3
        })
    );
    assert_eq!(binding.active_version(provider).as_deref(), Some("1.0.0"));
    assert_eq!(answer(&mut binding, provider, 9).as_deref(), Ok("1.0.0"));
    // A release can't be staged over it meanwhile, nor its predecessor marked stale.
    let module = spec(provider, "1.2.0", Vec::new(), true);
    assert_eq!(
        install(&mut binding, &module, &language(), "reserve", &[]),
        rutent_conformance::boundary::Admitted::Refused(AdmissionRefusal::ReleaseStaged)
    );
    assert_eq!(
        binding.mark_stale(provider, "1.0.0", OPERATOR, later(86_400)),
        Err(LifecycleRefusal::StagingInProgress)
    );

    binding
        .begin_segment(provider, &tx(provider, 3))
        .expect("a third segment");
    assert_eq!(answer(&mut binding, provider, 3).as_deref(), Ok("1.1.0"));
    assert_eq!(
        binding
            .promote(provider, OPERATOR, later(86_400))
            .as_deref(),
        Ok("1.1.0")
    );
    assert_eq!(binding.active_version(provider).as_deref(), Some("1.1.0"));
    assert_eq!(answer(&mut binding, provider, 9).as_deref(), Ok("1.1.0"));
    let last = binding.lifecycle().decisions().last().expect("a decision");
    assert_eq!(
        (last.kind, last.by.as_str(), last.at),
        (DecisionKind::Promoted, OPERATOR, later(86_400))
    );
}

// verifies: REQ-MDL-20
#[test]
fn segment_on_a_revoked_version_finishes_on_it_unless_it_is_exploited() {
    let provider = "revoked";
    let mut binding = binding("reserve");
    upgrade(&mut binding, provider, "1.0.0", true);
    for n in 1..=2 {
        binding
            .begin_segment(provider, &tx(provider, n))
            .expect("a segment");
    }
    upgrade(&mut binding, provider, "1.1.0", true);
    assert_eq!(binding.active_version(provider).as_deref(), Some("1.1.0"));

    // Revoked: it takes no new segment, and the two on it finish on it.
    assert_eq!(binding.revoke(provider, "1.0.0", OPERATOR, AT), Ok(None));
    assert_eq!(
        binding.begin_segment(provider, &tx(provider, 3)).as_deref(),
        Ok("1.1.0")
    );
    assert_eq!(answer(&mut binding, provider, 1).as_deref(), Ok("1.0.0"));
    assert_eq!(answer(&mut binding, provider, 2).as_deref(), Ok("1.0.0"));
    let record = binding
        .end_segment(tx(provider, 1).as_str())
        .expect("its record");
    assert_eq!(record.runs_on, "1.0.0");

    // Declared exploited: its process stops, and the segment still on it is refused, not moved.
    assert!(binding.version_process(provider, "1.0.0").is_some());
    assert_eq!(
        binding.declare_exploited(provider, "1.0.0", OPERATOR, AT),
        Ok(None)
    );
    assert!(binding.version_process(provider, "1.0.0").is_none());
    assert_eq!(
        answer(&mut binding, provider, 2),
        Err(PlatformRefusal::VersionExploited)
    );
    let segment = binding
        .lifecycle()
        .segment(tx(provider, 2).as_str())
        .expect("in flight");
    assert_eq!(segment.runs_on, "1.0.0");
    assert!(segment.moves.is_empty());
    assert_eq!(answer(&mut binding, provider, 3).as_deref(), Ok("1.1.0"));
    let kinds: Vec<_> = binding
        .lifecycle()
        .decisions()
        .iter()
        .map(|d| (d.kind, d.version.as_str(), d.by.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            (DecisionKind::Revoked, "1.0.0", OPERATOR),
            (DecisionKind::DeclaredExploited, "1.0.0", OPERATOR)
        ]
    );
}

// verifies: REQ-MDL-5, REQ-MDL-19, THR-16
#[test]
fn newer_release_never_moves_a_segment_and_a_stale_mark_moves_it_to_the_version_in_force() {
    let provider = "stale";
    let mut binding = binding("reserve");
    upgrade(&mut binding, provider, "1.0.0", true);
    binding
        .begin_segment(provider, &tx(provider, 1))
        .expect("a segment");
    // Publishing newer releases moves nothing and marks nothing.
    upgrade(&mut binding, provider, "1.1.0", true);
    assert_eq!(answer(&mut binding, provider, 1).as_deref(), Ok("1.0.0"));
    stage(&mut binding, 100, 0, 0);
    upgrade(&mut binding, provider, "1.2.0", true);
    assert_eq!(answer(&mut binding, provider, 1).as_deref(), Ok("1.0.0"));
    assert_eq!(binding.lifecycle().mark(provider, "1.0.0"), None);
    // Not while a release is staged: the mark would move the segment past the staging.
    assert_eq!(
        binding.mark_stale(provider, "1.0.0", OPERATOR, AT),
        Err(LifecycleRefusal::StagingInProgress)
    );
    binding
        .roll_back(provider, OPERATOR, AT)
        .expect("rolled back");
    // The operator's mark: the segment moves to the version in force, and the move is in its record.
    assert_eq!(
        binding.mark_stale(provider, "1.1.0", OPERATOR, AT),
        Err(LifecycleRefusal::NoNewerVersion)
    );
    assert_eq!(
        binding.mark_stale(provider, "1.0.0", OPERATOR, later(5)),
        Ok(1)
    );
    assert_eq!(answer(&mut binding, provider, 1).as_deref(), Ok("1.1.0"));
    assert!(binding.version_process(provider, "1.0.0").is_none());
    let record = binding
        .end_segment(tx(provider, 1).as_str())
        .expect("its record");
    assert_eq!(
        (record.began_on.as_str(), record.runs_on.as_str()),
        ("1.0.0", "1.1.0")
    );
    assert_eq!(record.moves.len(), 1);
    let moved = &record.moves[0];
    assert_eq!(
        (
            moved.from.as_str(),
            moved.to.as_str(),
            moved.reason,
            moved.at
        ),
        ("1.0.0", "1.1.0", MoveReason::Stale, later(5))
    );
}

// verifies: REQ-MDL-10
#[test]
fn transaction_cannot_read_another_transactions_state() {
    let provider = "stateful";
    let key = format!("transaction/{}/nonce", tx(provider, 1).as_str());
    let mut binding = binding("reserve");
    binding.seed_state(provider, &key, "own");
    binding.seed_state(provider, "rates", "shared");
    let module = spec(
        provider,
        "1.0.0",
        vec![
            Step::ReadState {
                part: PartName::Outside,
                key: key.clone(),
            },
            Step::ReadState {
                part: PartName::Outside,
                key: "rates".to_owned(),
            },
        ],
        true,
    );
    install(&mut binding, &module, &language(), "reserve", &[]);
    for n in [1, 2] {
        binding.take_events();
        let asked = binding.world.state_asked.len();
        assert!(answer(&mut binding, provider, n).is_ok());
        let reads: Vec<_> = binding
            .take_events()
            .state_reads
            .into_iter()
            .map(|r| (r.key, r.found))
            .collect();
        let own = n == 1;
        assert_eq!(
            reads,
            [(key.clone(), own), ("rates".to_owned(), true)],
            "transaction {n}"
        );
        // Another transaction's entry is never asked of the world.
        let asked: Vec<_> = binding.world.state_asked[asked..].to_vec();
        assert_eq!(asked.contains(&key), own, "transaction {n}: {asked:?}");
    }
}

// verifies: REQ-MDL-14, INV-8
#[test]
fn duplicate_transaction_identifier_is_refused() {
    let (first, second) = ("first", "second");
    let mut binding = binding("reserve");
    upgrade(&mut binding, first, "1.0.0", true);
    upgrade(&mut binding, second, "1.0.0", true);
    let transaction = tx(first, 1);
    binding
        .begin_segment(first, &transaction)
        .expect("a segment");
    for provider in [first, second] {
        assert_eq!(
            binding.begin_segment(provider, &transaction),
            Err(PlatformRefusal::DuplicateTransaction)
        );
    }
    // Asked again within its segment it is the same transaction, answered again; asked of another provider it
    // is refused before any module sees it.
    assert_eq!(answer(&mut binding, first, 1).as_deref(), Ok("1.0.0"));
    assert_eq!(answer(&mut binding, first, 1).as_deref(), Ok("1.0.0"));
    let mut elsewhere = serde_json::to_value(withdrawal(first, 1)).expect("JSON");
    elsewhere["statement"]["provider"] = second.into();
    let elsewhere = serde_json::from_value(elsewhere).expect("an operation");
    assert_eq!(
        Platform::handle(&mut binding, second, &elsewhere),
        types::Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );

    // Ended, its identifier begins nothing and runs nothing again.
    binding
        .end_segment(transaction.as_str())
        .expect("its record");
    assert_eq!(
        binding.begin_segment(first, &transaction),
        Err(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(
        answer(&mut binding, first, 1),
        Err(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(
        binding.begin_segment(first, &tx(first, 2)).as_deref(),
        Ok("1.0.0")
    );
}

// verifies: REQ-MDL-1, REQ-MDL-3, THR-4
#[test]
fn release_waiting_for_acceptance_can_be_withdrawn() {
    let mut s = Signed::new();
    let key = signed::catalogue_support::advertising_key(RESERVE);
    let (first, first_binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let Admitted::Artefact(waiting) = s.admit(
        &signed::assertion(&first, &first_binaries, &key),
        &first_binaries,
    ) else {
        panic!("the first release wasn't admitted");
    };
    // Its signing key is retired before it is accepted: acceptance judges it again and refuses it, and it waits.
    s.rotate_advertising_key(RESERVE);
    assert_eq!(
        s.accept(RESERVE, &waiting.assertion_digest, "test-reviewer"),
        None
    );
    let rotated = signed::rotated_key(RESERVE);
    let (next, next_binaries) = release(RESERVE, "1.0.1", &["withdrawal"]);
    let next_assertion = signed::assertion(&next, &next_binaries, &rotated);
    assert_eq!(
        s.admit(&next_assertion, &next_binaries),
        Admitted::Refused(AdmissionRefusal::AdmissionPending)
    );

    // Only the waiting release's own digest withdraws it, and only by someone.
    let other = signed::digest_of(b"another assertion");
    assert_eq!(s.platform.withdraw(RESERVE, &other, OPERATOR, AT), None);
    assert_eq!(
        s.platform
            .withdraw(RESERVE, &waiting.assertion_digest, "", AT),
        None
    );
    assert!(s.platform.pending(RESERVE).is_some());
    let withdrawn = s
        .platform
        .withdraw(RESERVE, &waiting.assertion_digest, OPERATOR, later(1))
        .expect("withdrawn");
    assert_eq!(withdrawn.assertion_digest, waiting.assertion_digest);
    assert!(s.platform.pending(RESERVE).is_none());
    let last = s
        .platform
        .lifecycle()
        .decisions()
        .last()
        .expect("a decision");
    assert_eq!(
        (last.kind, last.version.as_str(), last.by.as_str(), last.at),
        (DecisionKind::Withdrawn, "1.0.0", OPERATOR, later(1))
    );

    // Its version names it still, so it isn't admitted again; the provider's next release is.
    assert_eq!(
        s.admit(
            &signed::assertion(&first, &first_binaries, &rotated),
            &first_binaries
        ),
        Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted)
    );
    let Admitted::Artefact(next) = s.admit(&next_assertion, &next_binaries) else {
        panic!("the next release wasn't admitted");
    };
    s.accept(RESERVE, &next.assertion_digest, "test-reviewer")
        .expect("accepted");
    assert!(s.answers_as(RESERVE, "1.0.1", 1));
}

// verifies: REQ-MDL-4, THR-17
#[test]
fn publishing_an_older_signed_release_changes_nothing() {
    let mut s = Signed::new();
    let key = signed::catalogue_support::advertising_key(RESERVE);
    let (first, first_binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let first_assertion = signed::assertion(&first, &first_binaries, &key);
    let Admitted::Artefact(artefact) = s.admit(&first_assertion, &first_binaries) else {
        panic!("not admitted");
    };
    s.accept(RESERVE, &artefact.assertion_digest, "test-reviewer")
        .expect("accepted");
    let compiles = s.platform.measurements.compiles.len();

    // The same release again, and one asserted before it under another version: neither is compiled or run.
    assert_eq!(
        s.admit(&first_assertion, &first_binaries),
        Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted)
    );
    let (older, older_binaries) = release(RESERVE, "0.9.0", &["withdrawal"]);
    assert_eq!(
        s.admit(
            &signed::assertion_at(&older, &older_binaries, &key, "2026-11-06T12:00:00Z"),
            &older_binaries
        ),
        Admitted::Refused(AdmissionRefusal::OlderRelease)
    );
    assert_eq!(s.platform.measurements.compiles.len(), compiles);
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    assert!(s.answers_as(RESERVE, "1.0.0", 1));
}

// verifies: REQ-MDL-4, REQ-CAT-23
#[test]
fn operator_published_module_upgrade_is_staged_and_rolled_back_as_a_providers_is() {
    let mut s = Signed::new();
    support::run_wallet(&mut s, support::POLYGON_WALLET, "1.0.0");
    s.platform
        .set_staging(Some(Staging::new(100, 0, 0).expect("a policy")));
    let admitted = support::admit_wallet(&mut s, support::POLYGON_WALLET, "1.1.0");
    assert!(matches!(admitted, Admitted::Artefact(_)), "{admitted:?}");
    let wallet = support::POLYGON_WALLET;
    assert_eq!(s.platform.lifecycle().staged(wallet), Some("1.1.0"));
    assert_eq!(s.platform.active_version(wallet).as_deref(), Some("1.0.0"));
    let segment = Identifier::new("tx-wallet-1").expect("an identifier");
    assert_eq!(
        s.platform.begin_segment(wallet, &segment).as_deref(),
        Ok("1.1.0")
    );
    assert_eq!(
        s.platform.roll_back(wallet, OPERATOR, AT).as_deref(),
        Ok("1.1.0")
    );
    assert_eq!(s.platform.active_version(wallet).as_deref(), Some("1.0.0"));
    assert_eq!(
        s.platform.lifecycle().mark(wallet, "1.1.0"),
        Some(Mark::Revoked)
    );
    let record = s
        .platform
        .end_segment(segment.as_str())
        .expect("its record");
    assert_eq!(record.runs_on, "1.1.0");
    assert!(s.platform.version_process(wallet, "1.1.0").is_none());
}

/// A deposit asked of `provider` in the transaction of [`withdrawal`] `n`: a kind the stand-ins don't handle.
fn deposit(
    provider: &str,
    n: usize,
) -> rutent_contracts::provider_role::Statement<rutent_contracts::provider_role::Operation> {
    let mut value = serde_json::to_value(withdrawal(provider, n)).expect("JSON");
    value["statement"] = serde_json::json!({
        "kind": "deposit",
        "transaction": tx(provider, n).as_str(),
        "provider": provider,
        "origin": "conformance-ledger",
        "destination": { "network": "conformance-network" },
        "asset": "COP",
        "amount": "1000",
        "dry_run": false,
        "inputs": {},
    });
    serde_json::from_value(value).expect("a valid operation")
}

// verifies: REQ-MDL-4
#[test]
fn platform_stages_upgrades_by_default() {
    let platform = Platform::new(
        common::paths(),
        TestWorld::default(),
        types::Envelope {
            deployment: signed::catalogue_support::deployment(),
            protocol_version: rutent_contracts::provider_role::VERSION.to_owned(),
        },
    )
    .expect("a platform");
    let staging = platform.lifecycle().staging().expect("staging is on");
    assert_eq!(staging, DEFAULT_STAGING);
    assert_eq!(
        (staging.share(), staging.window(), staging.minimum()),
        (10, 86_400, 100)
    );
}

// verifies: REQ-MDL-3, AC-21, REQ-MDL-4
#[test]
fn release_after_a_rolled_back_one_is_compared_with_the_version_in_force() {
    let provider = "widening";
    let mut binding = binding("reserve");
    upgrade(&mut binding, provider, "1.0.0", true);
    stage(&mut binding, 100, 0, 0);
    let widened = |version: &str| {
        let mut module = spec(provider, version, Vec::new(), true);
        module.declaration = common::declaration(provider, version, &["withdrawal", "deposit"]);
        module
    };
    // 1.1.0 widens, handling deposits too: accepted afresh, staged, and rolled back.
    install(&mut binding, &widened("1.1.0"), &language(), "reserve", &[]);
    assert_eq!(binding.lifecycle().staged(provider), Some("1.1.0"));
    binding
        .roll_back(provider, OPERATOR, AT)
        .expect("rolled back");
    // 1.2.0 widens as 1.1.0 did. Compared with 1.0.0, the version in force, it needs a fresh acceptance too.
    let module = widened("1.2.0");
    let binaries = binding
        .build_with(&module, &language(), "reserve", &[])
        .expect("a stand-in");
    let admitted = ModuleBinding::admit(
        &mut binding,
        &common::assertion(&module.declaration, &binaries),
        &binaries,
    );
    let rutent_conformance::boundary::Admitted::Artefact(artefact) = admitted else {
        panic!("not admitted: {admitted:?}");
    };
    assert_eq!(
        artefact
            .capabilities
            .compared_with
            .as_ref()
            .map(|v| v.as_str()),
        Some("1.0.0")
    );
    assert!(!artefact.capabilities.widenings.is_empty());
    assert_eq!(artefact.acceptance.required, AcceptanceMode::Fresh);
    assert_eq!(artefact.acceptance.accepted, None);
}

// verifies: REQ-MDL-4
#[test]
fn only_operations_the_staged_version_ran_count_and_it_is_promoted_only_while_it_runs() {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let provider = "counted";
    let mut binding = binding("reserve");
    upgrade(&mut binding, provider, "1.0.0", true);
    stage(&mut binding, 100, 0, 1);
    upgrade(&mut binding, provider, "1.1.0", true);
    assert_eq!(
        binding.begin_segment(provider, &tx(provider, 1)).as_deref(),
        Ok("1.1.0")
    );
    let handled = |binding: &WasmBinding| {
        binding
            .lifecycle()
            .observation(provider)
            .expect("staged")
            .staged_counts
            .handled()
    };

    // A kind it doesn't handle is refused before it reaches the module: not counted.
    assert_eq!(
        Platform::handle(&mut binding, provider, &deposit(provider, 1)),
        types::Handled::Refused(PlatformRefusal::NotHandled)
    );
    assert_eq!(handled(&binding), 0);

    // Its process killed, it isn't promoted, and an operation that finds the process gone isn't counted.
    let pid = binding
        .version_process(provider, "1.1.0")
        .expect("its process");
    // SAFETY: a signal to a child process this test's platform started.
    assert_eq!(unsafe { kill(pid as i32, 9) }, 0);
    let mut refused = Ok(String::new());
    for _ in 0..200 {
        refused = binding.promote(provider, OPERATOR, AT);
        if refused == Err(LifecycleRefusal::StagedNotRunning) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert_eq!(refused, Err(LifecycleRefusal::StagedNotRunning));
    assert_eq!(binding.active_version(provider).as_deref(), Some("1.0.0"));
    assert_eq!(
        answer(&mut binding, provider, 1),
        Err(PlatformRefusal::Unavailable)
    );
    assert_eq!(handled(&binding), 0);

    // Started again in its place, it runs one, which counts, and it is promoted.
    assert_eq!(answer(&mut binding, provider, 1).as_deref(), Ok("1.1.0"));
    assert_eq!(handled(&binding), 1);
    assert_eq!(
        binding.promote(provider, OPERATOR, AT).as_deref(),
        Ok("1.1.0")
    );
    assert_eq!(answer(&mut binding, provider, 9).as_deref(), Ok("1.1.0"));
}
