//! Property tests of signed admission (`DEC-18`; `REQ-MDL-1`, `THR-4`), each over seeded cases, so a failure names
//! the seed that replays it. None of them compiles a module per case: what they refuse is refused before the
//! admitting compiler runs, and each test checks that it was.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;

use rutent_contracts::module_interface::AdmissionRefusal;
use rutent_prov::module::types::Admitted;
use signed::catalogue_support::{CASES, Rng, advertising_key};
use signed::{RESERVE, Signed, is_artefact, release};

// verifies: REQ-MDL-1, THR-4
#[test]
fn an_assertion_changed_in_any_one_byte_is_never_admitted_nor_compiled() {
    let mut s = Signed::new();
    let (declaration, binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let received = signed::assertion(&declaration, &binaries, &advertising_key(RESERVE));
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut bytes = received.clone();
        let at = rng.below(bytes.len() as u64) as usize;
        let was = bytes[at];
        bytes[at] = loop {
            let b = rng.below(256) as u8;
            if b != was {
                break b;
            }
        };
        let admitted = s.admit(&bytes, &binaries);
        assert!(
            !is_artefact(&admitted),
            "seed {seed}: byte {at} changed was admitted"
        );
        assert!(
            s.platform.measurements.compiles.is_empty(),
            "seed {seed}: byte {at} changed was compiled"
        );
        assert!(
            s.platform.pending(RESERVE).is_none(),
            "seed {seed}: byte {at} changed was kept"
        );
    }
    // The assertion as signed is admitted.
    assert!(is_artefact(&s.admit(&received, &binaries)));
}

// verifies: REQ-MDL-1
#[test]
fn no_digest_but_the_admitted_assertions_accepts_the_release() {
    let mut s = Signed::new();
    let (declaration, binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let received = signed::assertion(&declaration, &binaries, &advertising_key(RESERVE));
    let Admitted::Artefact(artefact) = s.admit(&received, &binaries) else {
        panic!("the release wasn't admitted");
    };
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let wrong = signed::random_digest(&mut rng);
        if wrong == artefact.assertion_digest {
            continue;
        }
        assert!(
            s.accept(RESERVE, &wrong, "test-reviewer").is_none(),
            "seed {seed}: another digest's acceptance admitted the release"
        );
        assert_eq!(
            s.platform.active_version(RESERVE),
            None,
            "seed {seed}: the release ran"
        );
        assert!(
            s.platform.pending(RESERVE).is_some(),
            "seed {seed}: the release stopped waiting"
        );
    }
    assert!(
        s.accept(RESERVE, &artefact.assertion_digest, "test-reviewer")
            .is_some()
    );
}

// verifies: REQ-MDL-1
#[test]
fn while_a_release_waits_no_other_release_of_its_provider_is_admitted_or_compiled() {
    let mut s = Signed::new();
    let key = advertising_key(RESERVE);
    let (declaration, binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let Admitted::Artefact(waiting) =
        s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
    else {
        panic!("the release wasn't admitted");
    };
    let compiles = s.platform.measurements.compiles.len();
    let kinds = ["withdrawal", "deposit", "balance_verification"];
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let version = format!("{}.{}.{}", rng.below(4), rng.below(10), rng.below(10));
        let handles: Vec<&str> = kinds
            .iter()
            .copied()
            .filter(|_| rng.chance(2))
            .chain(["withdrawal"])
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        // Only the declaration changes: the binaries are the waiting release's, so no case builds a stand-in.
        let mut value = serde_json::to_value(&declaration).expect("JSON");
        value["version"] = version.clone().into();
        value["sandboxed"]["handles"] = serde_json::to_value(&handles).expect("JSON");
        let other = serde_json::from_value(value).expect("a valid declaration");
        match s.admit(&signed::assertion(&other, &binaries, &key), &binaries) {
            Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted) => {}
            got => panic!("seed {seed}: release {version} {handles:?} was not refused: {got:?}"),
        }
        assert_eq!(
            s.platform.measurements.compiles.len(),
            compiles,
            "seed {seed}: a refused release was compiled"
        );
        assert_eq!(
            s.platform.pending(RESERVE).map(|a| &a.assertion_digest),
            Some(&waiting.assertion_digest),
            "seed {seed}: the waiting release changed"
        );
    }
}
