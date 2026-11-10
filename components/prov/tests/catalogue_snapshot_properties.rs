//! Property tests of the snapshot's invariants (`DEC-18`): seeded, so a failure names the seed that replays it.

mod common;

use common::*;
use rutent_contracts::provider_role::{EvidenceGrade, NodeRef};
use rutent_contracts::router_api::{FigureValue, Version};
use rutent_prov::catalogue::{Catalogue, Instant, tuples_of};

const NOW: Instant = Instant(2_000);

const GRADES: [EvidenceGrade; 4] = [
    EvidenceGrade::PublicRecord,
    EvidenceGrade::SignedByAnotherParty,
    EvidenceGrade::SignedByProvider,
    EvidenceGrade::Statement,
];

fn advertisements() -> Vec<rutent_contracts::provider_role::Advertisement> {
    vec![
        reserve_advertisement(),
        ramp_advertisement(),
        exchange_advertisement(),
        internal_rail_advertisement(),
        advertisement("advertisement-provider-under-several-categories"),
    ]
}

/// A catalogue of the fixtures' advertisements with each setting chosen at random: public nodes, holding accounts,
/// floors, module versions, and which grants lapse.
fn random_catalogue(rng: &mut Rng) -> Catalogue {
    let mut catalogue = Catalogue::new();
    for (i, ad) in advertisements().iter().enumerate() {
        let keys = tuples_of(ad);
        let expires = if rng.chance(4) { 1_900 } else { 100_000 };
        catalogue
            .grant(grant(
                &format!("g-{i}"),
                &ad.provider,
                keys.clone(),
                expires,
            ))
            .unwrap();
        // `acme-co` and `bridge-pay` both declare `acme-ledger`: a second join is a repeat of the same publisher.
        let _ = join(&mut catalogue, ad, JOINED_AT);
        if !rng.chance(5) {
            catalogue.set_module_version(&ad.provider, Version::new("1.0.0").unwrap());
        }
        for key in keys {
            if !rng.chance(5) {
                catalogue.set_evidence_floor(key, GRADES[rng.below(4) as usize]);
            }
        }
    }
    for node in ["polygon", "co-bank-transfer", "acme-ledger-public"] {
        if rng.chance(2) {
            catalogue.add_public_node(node).unwrap();
            if rng.chance(3) {
                catalogue.publish_operator_module("wallet").unwrap();
                catalogue.register_public_holding(node, "wallet").unwrap();
            }
        }
    }
    for node in ["acme-ledger", "acme-card-book"] {
        if rng.chance(3) {
            let _ = catalogue.register_private_holding(node, "acme", "interface");
        }
    }
    catalogue
}

fn inner_nodes(value: &FigureValue) -> Vec<String> {
    let inner = |n: &NodeRef| match n {
        NodeRef::Inner(id) => Some(id.clone()),
        NodeRef::Outer(_) => None,
    };
    match value {
        FigureValue::ReserveTuple { tuple } => [&tuple.origin, &tuple.destination]
            .into_iter()
            .filter_map(inner)
            .collect(),
        FigureValue::ExchangeTuple { tuple } => vec![tuple.node.clone()],
        FigureValue::RampTuple { tuple } => vec![tuple.origin.clone(), tuple.destination.clone()],
        other => panic!("the catalogue states no {other:?}"),
    }
}

// verifies: REQ-CAT-22, REQ-CAT-23
#[test]
fn every_inner_node_in_a_snapshot_is_listed_and_has_a_holding_account() {
    for seed in 0..CASES {
        let catalogue = random_catalogue(&mut Rng::new(seed));
        let built = catalogue.snapshot("snap-1", NOW).unwrap();
        for figure in &built.snapshot.figures {
            for node in inner_nodes(&figure.value) {
                let listed = catalogue.nodes().is_public(&node)
                    || catalogue.publishers().publisher_of_node(&node).is_some();
                assert!(listed, "seed {seed}: {node} is on neither list");
                assert!(
                    catalogue.nodes().holding(&node).is_some(),
                    "seed {seed}: {node} has no holding account"
                );
            }
        }
    }
}

// verifies: REQ-ORD-39
#[test]
fn no_provider_below_a_tuples_floor_is_ever_in_a_snapshot() {
    for seed in 0..CASES {
        let catalogue = random_catalogue(&mut Rng::new(seed));
        let built = catalogue.snapshot("snap-1", NOW).unwrap();
        for ad in advertisements() {
            let grade = ad.declarations.evidence.as_ref().unwrap().grade;
            for key in tuples_of(&ad) {
                let left_out = built
                    .left_out
                    .iter()
                    .any(|l| l.provider == ad.provider && l.tuple == key);
                let floor = catalogue.evidence_floor(&key);
                let meets = floor.is_some_and(|f| grade <= f);
                if !meets && catalogue.declared_by(&ad.provider).is_some() {
                    assert!(left_out, "seed {seed}: {} {key:?} is routed", ad.provider);
                }
            }
        }
    }
}

// verifies: REQ-CAT-13
#[test]
fn snapshot_is_complete_valid_and_the_same_every_time() {
    for seed in 0..CASES {
        let catalogue = random_catalogue(&mut Rng::new(seed));
        let first = catalogue.snapshot("snap-1", NOW).unwrap();
        let second = catalogue.clone().snapshot("snap-1", NOW).unwrap();
        assert_eq!(first, second, "seed {seed}");
        first
            .snapshot
            .check()
            .unwrap_or_else(|v| panic!("seed {seed}: {v}"));
        let joined: usize = advertisements()
            .iter()
            .filter(|ad| catalogue.declared_by(&ad.provider).is_some())
            .map(|ad| tuples_of(ad).len())
            .sum();
        assert_eq!(
            first.snapshot.figures.len() + first.left_out.len(),
            joined,
            "seed {seed}"
        );
    }
}
