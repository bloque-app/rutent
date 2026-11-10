//! The release lifecycle's invariants as property tests (`DEC-18`, `T-013-06`), over the pure lifecycle the platform
//! drives: no process starts, so each property runs many seeded cases, and a failing case replays from its seed.

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;

use common::{CASES, Rng};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_prov::catalogue::Instant;
use rutent_prov::module::lifecycle::{
    DecisionKind, Lifecycle, LifecycleRefusal, Mark, MoveReason, Staging,
};

const PROVIDER: &str = "polygon-wallet-reserve";
const OTHER: &str = "bridge-pay";
const OPERATOR: &str = "operator-on-call";

fn id(text: &str) -> Identifier {
    Identifier::new(text).expect("an identifier")
}

/// A random walk over everything the lifecycle is told or asked, checking after each step that every segment in
/// flight runs on a version that was accepted and started and isn't exploited, and that a segment's version
/// changes only by a recorded move to the version in force when its version is marked stale.
// verifies: DEC-18, REQ-MDL-5, REQ-MDL-19, REQ-MDL-20
#[test]
fn segment_never_runs_a_version_that_was_not_accepted_or_was_declared_exploited() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut lifecycle = Lifecycle::default();
        if rng.chance(4) {
            lifecycle.set_staging(None);
        } else {
            let share = 1 + rng.below(100) as u8;
            lifecycle.set_staging(Staging::new(share, rng.below(5), rng.below(4)));
        }
        let mut at = Instant(1_000);
        // Versions started, accepted or not: a started version that was never accepted must never run.
        let mut started: Vec<String> = Vec::new();
        for step in 0..60 {
            at = Instant(at.0 + rng.below(3));
            let version = format!("1.0.{}", rng.below(6));
            let transaction = format!("tx-{}", rng.below(12));
            let before: BTreeMap<String, String> = (0..12)
                .filter_map(|n| {
                    let tx = format!("tx-{n}");
                    lifecycle.segment(&tx).map(|s| (tx, s.runs_on.clone()))
                })
                .collect();
            let mut stale_to = None;
            match rng.below(10) {
                0 | 1 => {
                    // Accepted and started, unless the platform would have refused it while one is staged; one in
                    // five doesn't start, and stays only accepted. Now and then a version starts that was never
                    // accepted, which the lifecycle ignores.
                    if lifecycle.staged(PROVIDER).is_none()
                        && !lifecycle.is_accepted(PROVIDER, &version)
                    {
                        if !rng.chance(8) {
                            lifecycle.accept(PROVIDER, &version);
                        }
                        if !rng.chance(5) {
                            let placement = lifecycle.placement(PROVIDER);
                            lifecycle.started(PROVIDER, &version, placement, at);
                            started.push(version.clone());
                        }
                    }
                }
                2 | 3 => {
                    if let Ok(v) = lifecycle.begin_segment(PROVIDER, &id(&transaction)) {
                        let in_force = lifecycle.in_force(PROVIDER).map(str::to_owned);
                        let staged = lifecycle.staged(PROVIDER).map(str::to_owned);
                        assert!(
                            Some(&v) == in_force.as_ref() || Some(&v) == staged.as_ref(),
                            "seed {seed} step {step}: a segment began on {v}, neither in force nor staged"
                        );
                        assert_eq!(lifecycle.mark(PROVIDER, &v), None, "seed {seed}");
                    }
                }
                4 => {
                    if let Ok(v) = lifecycle.version_for(PROVIDER, Some(&transaction)) {
                        lifecycle.observe(PROVIDER, &v, rng.chance(2));
                    }
                }
                5 => {
                    lifecycle.end_segment(&transaction);
                }
                6 => {
                    let _ = if rng.chance(2) {
                        lifecycle.promote(PROVIDER, OPERATOR, at).map(drop)
                    } else {
                        lifecycle.roll_back(PROVIDER, OPERATOR, at).map(drop)
                    };
                }
                7 => {
                    let staged = lifecycle.staged(PROVIDER).is_some();
                    let in_force = lifecycle.in_force(PROVIDER).map(str::to_owned);
                    let marked = lifecycle.mark_stale(PROVIDER, &version, OPERATOR, at);
                    if marked.is_ok() {
                        assert!(
                            !staged,
                            "seed {seed}: marked stale while a release was staged"
                        );
                        stale_to = in_force;
                    } else if staged && lifecycle.is_accepted(PROVIDER, &version) {
                        assert!(
                            matches!(
                                marked,
                                Err(LifecycleRefusal::StagingInProgress
                                    | LifecycleRefusal::AlreadyMarked)
                            ),
                            "seed {seed}: {marked:?}"
                        );
                    }
                }
                8 => {
                    let _ = lifecycle.revoke(PROVIDER, &version, OPERATOR, at);
                }
                _ => {
                    let _ = lifecycle.declare_exploited(PROVIDER, &version, OPERATOR, at);
                }
            }

            // In force and staged: accepted, started and unmarked.
            for v in [lifecycle.in_force(PROVIDER), lifecycle.staged(PROVIDER)]
                .into_iter()
                .flatten()
            {
                assert!(lifecycle.is_accepted(PROVIDER, v), "seed {seed}: {v}");
                assert!(started.iter().any(|s| s == v), "seed {seed}: {v}");
                assert_eq!(lifecycle.mark(PROVIDER, v), None, "seed {seed}: {v}");
            }
            for v in lifecycle.wanted(PROVIDER) {
                assert_ne!(
                    lifecycle.mark(PROVIDER, &v),
                    Some(Mark::Exploited),
                    "seed {seed}: an exploited version is kept running"
                );
            }
            for n in 0..12 {
                let tx = format!("tx-{n}");
                let Some(segment) = lifecycle.segment(&tx) else {
                    continue;
                };
                let runs_on = segment.runs_on.clone();
                assert!(
                    lifecycle.is_accepted(PROVIDER, &runs_on) && started.contains(&runs_on),
                    "seed {seed} step {step}: {tx} runs on {runs_on}, never accepted and started"
                );
                match lifecycle.version_for(PROVIDER, Some(&tx)) {
                    Ok(v) => {
                        assert_eq!(v, runs_on, "seed {seed}");
                        assert_ne!(lifecycle.mark(PROVIDER, &v), Some(Mark::Exploited));
                    }
                    Err(refusal) => {
                        assert_eq!(refusal, PlatformRefusal::VersionExploited, "seed {seed}");
                        assert_eq!(lifecycle.mark(PROVIDER, &runs_on), Some(Mark::Exploited));
                    }
                }
                if let Some(was) = before.get(&tx)
                    && *was != runs_on
                {
                    let last = segment.moves.last().expect("a recorded move");
                    assert_eq!(
                        (last.from.as_str(), last.to.as_str(), last.reason),
                        (was.as_str(), runs_on.as_str(), MoveReason::Stale),
                        "seed {seed} step {step}: {tx} moved without its record"
                    );
                    assert_eq!(
                        stale_to.as_deref(),
                        Some(runs_on.as_str()),
                        "seed {seed}: moved, but not to the version in force on a stale mark"
                    );
                }
            }
        }
    }
}

/// Transaction identifiers drawn from a small pool, begun and ended at two providers in any order: an identifier
/// begins at most one segment ever, a repeat is refused as `DuplicateTransaction` and records nothing, and an
/// operation naming it after its segment ended, or at the other provider, is refused the same way (`INV-8`).
// verifies: DEC-18, INV-8, REQ-MDL-14
#[test]
fn transaction_identifier_never_begins_two_segments() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut lifecycle = Lifecycle::default();
        for provider in [PROVIDER, OTHER] {
            lifecycle.accept(provider, "1.0.0");
            lifecycle.started(provider, "1.0.0", lifecycle.placement(provider), Instant(0));
        }
        let mut begun: BTreeMap<String, usize> = BTreeMap::new();
        let mut ended: Vec<String> = Vec::new();
        for _ in 0..80 {
            let transaction = format!("tx-{}", rng.below(10));
            let provider = if rng.chance(2) { PROVIDER } else { OTHER };
            if rng.chance(3) {
                if let Some(record) = lifecycle.end_segment(&transaction) {
                    ended.push(record.transaction);
                }
                continue;
            }
            let seen = begun.contains_key(&transaction);
            match lifecycle.begin_segment(provider, &id(&transaction)) {
                Ok(_) => {
                    assert!(!seen, "seed {seed}: {transaction} began a second segment");
                    *begun.entry(transaction.clone()).or_default() += 1;
                }
                Err(refusal) => {
                    assert!(
                        seen,
                        "seed {seed}: {transaction} refused unseen: {refusal:?}"
                    );
                    assert_eq!(refusal, PlatformRefusal::DuplicateTransaction);
                }
            }
            for tx in &ended {
                for p in [PROVIDER, OTHER] {
                    assert_eq!(
                        lifecycle.version_for(p, Some(tx)),
                        Err(PlatformRefusal::DuplicateTransaction),
                        "seed {seed}: {tx} ran after its segment ended"
                    );
                }
            }
            if let Some(segment) = lifecycle.segment(&transaction) {
                let elsewhere = if segment.provider == PROVIDER {
                    OTHER
                } else {
                    PROVIDER
                };
                assert_eq!(
                    lifecycle.version_for(elsewhere, Some(&transaction)),
                    Err(PlatformRefusal::DuplicateTransaction),
                    "seed {seed}: {transaction} ran at a provider its segment isn't at"
                );
            }
        }
        assert!(begun.values().all(|n| *n == 1), "seed {seed}");
    }
}

/// A staged release, under any share, window and minimum, takes its share of the segments that begin, as near as
/// whole segments allow, and is promoted only once both its window has passed and it has handled its minimum;
/// until then the version in force stays, and a refused promotion changes nothing (`REQ-MDL-4`, ADR-0014).
// verifies: DEC-18, REQ-MDL-4
#[test]
fn staged_release_is_promoted_only_after_its_window_and_its_minimum() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let share = 1 + rng.below(100) as u8;
        let (window, minimum) = (rng.below(20), rng.below(10));
        let policy = Staging::new(share, window, minimum).expect("a policy");
        let mut lifecycle = Lifecycle::default();
        lifecycle.set_staging(Some(policy));
        let since = Instant(5_000);
        for version in ["1.0.0", "1.1.0"] {
            lifecycle.accept(PROVIDER, version);
            let placement = lifecycle.placement(PROVIDER);
            lifecycle.started(PROVIDER, version, placement, since);
        }
        assert_eq!(lifecycle.in_force(PROVIDER), Some("1.0.0"), "seed {seed}");
        assert_eq!(lifecycle.staged(PROVIDER), Some("1.1.0"), "seed {seed}");
        let (mut at, mut handled, mut segments, mut taken) = (since, 0u64, 0u64, 0u64);
        loop {
            at = Instant(at.0 + rng.below(3));
            if rng.chance(2) {
                segments += 1;
                let v = lifecycle
                    .begin_segment(PROVIDER, &id(&format!("tx-{segments}")))
                    .expect("a segment");
                if v == "1.1.0" {
                    taken += 1;
                    lifecycle.observe(PROVIDER, &v, rng.chance(2));
                    handled += 1;
                }
                assert_eq!(
                    taken,
                    u64::from(share) * segments / 100,
                    "seed {seed}: {taken} of {segments} at {share}%"
                );
            }
            let ready = at.0 >= since.0 + window && handled >= minimum;
            match lifecycle.promote(PROVIDER, OPERATOR, at) {
                Ok(v) => {
                    assert!(ready, "seed {seed}: promoted early ({at:?}, {handled})");
                    assert_eq!(v, "1.1.0");
                    assert_eq!(lifecycle.in_force(PROVIDER), Some("1.1.0"));
                    let last = lifecycle.decisions().last().expect("the decision");
                    assert_eq!(last.kind, DecisionKind::Promoted);
                    assert_eq!(last.by.as_str(), OPERATOR);
                    break;
                }
                Err(refusal) => {
                    assert!(!ready, "seed {seed}: not promoted when ready: {refusal:?}");
                    assert_eq!(lifecycle.in_force(PROVIDER), Some("1.0.0"));
                    assert_eq!(lifecycle.staged(PROVIDER), Some("1.1.0"));
                }
            }
            assert!(segments < 10_000, "seed {seed}: never promoted");
        }
    }
}
