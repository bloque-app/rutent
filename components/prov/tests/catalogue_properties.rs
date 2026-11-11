//! Property tests of the catalogue's invariants (`DEC-18`): seeded, so a failure names the seed that replays it.

mod common;

use std::collections::HashSet;

use common::*;
use rutent_prov::catalogue::{
    Catalogue, Instant, Observation, RecordOutcome, TupleBound, TupleKey, tuples_of,
};

// verifies: DEC-11
#[test]
fn ceiling_never_exceeds_the_bond_under_any_sequence_of_changes() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut cap = capacity(rng.below(10_000), 0);
        for step in 0..40 {
            match rng.below(3) {
                0 => {
                    let _ = cap.set_ceiling(rng.below(12_000));
                }
                1 => cap
                    .rebond(bond(rng.below(10_000)), "risk-officer", Instant(step))
                    .unwrap(),
                _ => {
                    let mut other = bond(rng.below(10_000));
                    other.asset = "EURC".into();
                    cap.rebond(other, "risk-officer", Instant(step)).unwrap();
                }
            }
            assert!(
                cap.ceiling() <= cap.bond().amount,
                "seed {seed}, step {step}"
            );
        }
    }
}

/// The catalogue's answer, checked against a model that knows only the rules written in `REQ-CAT-1` and `REQ-CAT-2`.
// verifies: REQ-CAT-1, REQ-CAT-2
#[test]
fn tuple_is_usable_exactly_when_a_grant_in_force_covers_it_and_it_is_not_restricted() {
    let keys: Vec<TupleKey> = tuples_of(&ramp_advertisement());
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut catalogue = Catalogue::new();
        let mut model: Vec<(Vec<usize>, u64, Option<u64>)> = Vec::new(); // covered tuples, expiry, revoked at
        let mut restricted: HashSet<usize> = HashSet::new();

        for i in 0..rng.below(4) {
            let covered: Vec<usize> = (0..keys.len()).filter(|_| rng.chance(2)).collect();
            if covered.is_empty() {
                continue;
            }
            let expiry = 1_001 + rng.below(5_000);
            let tuples = covered.iter().map(|&k| keys[k].clone()).collect();
            catalogue
                .grant(grant(&format!("g{i}"), "p", tuples, expiry))
                .unwrap();
            let revoked = rng.chance(3).then(|| rng.below(6_000));
            if let Some(at) = revoked {
                catalogue.revoke(&format!("g{i}"), Instant(at)).unwrap();
            }
            model.push((covered, expiry, revoked));
        }
        for (k, key) in keys.iter().enumerate() {
            if rng.chance(3) {
                catalogue.bound("p", key.clone(), TupleBound::Restricted);
                restricted.insert(k);
            }
        }

        let at = 1_000 + rng.below(6_000);
        for (k, key) in keys.iter().enumerate() {
            let expected = !restricted.contains(&k)
                && model.iter().any(|(covered, expiry, revoked)| {
                    covered.contains(&k) && at < *expiry && revoked.is_none_or(|r| at < r)
                });
            assert_eq!(
                catalogue.usable("p", key, Instant(at)).is_ok(),
                expected,
                "seed {seed}, tuple {k}, at {at}"
            );
        }
    }
}

// verifies: REQ-CAT-7
#[test]
fn owed_reversals_are_discharged_only_by_a_confirmation() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut catalogue = Catalogue::new();
        let key = tuples_of(&ramp_advertisement()).remove(0);
        catalogue
            .grant(grant("g", "p", vec![key], 1_001 + rng.below(100)))
            .unwrap();
        let ids: Vec<String> = (0..5).map(|i| format!("rev-{i}")).collect();
        for id in &ids {
            catalogue
                .owed_mut()
                .owe(id, "p", "USDC", 1 + rng.below(100))
                .unwrap();
        }
        let mut confirmed: HashSet<&String> = HashSet::new();
        for step in 0..30 {
            let id = &ids[rng.below(5) as usize];
            match rng.below(4) {
                0 => {
                    catalogue
                        .owed_mut()
                        .observe(id, Observation::Confirmed)
                        .unwrap();
                    confirmed.insert(id);
                }
                1 => catalogue
                    .owed_mut()
                    .observe(id, Observation::StandingLost)
                    .unwrap(),
                2 => catalogue
                    .owed_mut()
                    .observe(
                        id,
                        Observation::Silence {
                            since: Instant(step),
                        },
                    )
                    .unwrap(),
                _ => {
                    let _ = catalogue.revoke("g", Instant(step));
                }
            }
            for id in &ids {
                assert_eq!(
                    catalogue.owed().is_owed(id),
                    !confirmed.contains(id),
                    "seed {seed}, step {step}"
                );
            }
        }
    }
}

// verifies: REQ-CAT-6
#[test]
fn recording_a_declaration_again_never_adds_a_record() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut catalogue = Catalogue::new();
        let mut ad = reserve_advertisement();
        let mut expected = 0usize;
        let mut last = (false, true);
        for round in 0..rng.below(6) + 1 {
            let version = format!("0.{}.0", round + 2);
            ad.version = version.clone();
            let flags = (rng.chance(2), rng.chance(2));
            let decl = &mut ad.offers.reserve.as_mut().unwrap().declaration;
            decl.can_require_user_initiated_withdrawal = flags.0;
            decl.starts_routes_on_automatic_deposits = flags.1;
            decl.declared_in_version = version;
            let changed = round == 0 || flags != last;
            last = flags;
            expected += usize::from(changed);
            for attempt in 0..=rng.below(3) {
                let outcome = catalogue
                    .declarations_mut()
                    .record(&ad, Instant(round))
                    .unwrap()
                    .unwrap();
                let is_new = matches!(outcome, RecordOutcome::Recorded(_));
                assert_eq!(
                    is_new,
                    changed && attempt == 0,
                    "seed {seed}, round {round}"
                );
            }
            assert_eq!(
                catalogue.declarations().records_of(&ad.provider).len(),
                expected,
                "seed {seed}"
            );
        }
    }
}
