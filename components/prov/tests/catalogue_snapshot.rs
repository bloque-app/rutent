//! Tuples, nodes and the snapshot the router reads (`T-012-03`).

mod common;

use std::path::Path;

use common::*;
use rutent_contracts::provider_role::{EvidenceGrade, NodeRef};
use rutent_contracts::router_api::{CatalogueSnapshot, FigureValue, Id, Origin};
use rutent_prov::catalogue::{
    Catalogue, Category, Endpoint, GrantError, HoldingSource, Instant, JoinRefusal, LeftOutReason,
    NodeError, SnapshotError, TupleKey, TupleRefusal, Unusable, check_tuple, tuples_of,
};
use serde_json::Value;

const NOW: Instant = Instant(2_000);

/// The four `acme` advertisements that make a route across public and private nodes: withdraw from a reserve,
/// ramp into the private node, convert, deposit.
fn acme() -> Vec<rutent_contracts::provider_role::Advertisement> {
    vec![
        reserve_advertisement(),
        ramp_advertisement(),
        exchange_advertisement(),
    ]
}

fn left_out_reasons(catalogue: &Catalogue) -> Vec<LeftOutReason> {
    catalogue
        .snapshot("snap-1", NOW)
        .unwrap()
        .left_out
        .into_iter()
        .map(|l| l.reason)
        .collect()
}

// verifies: REQ-CAT-22, AC-34
#[test]
fn node_neither_public_nor_declared_private_cannot_be_on_a_route() {
    // `polygon` is never put on the operator's public list, and no provider declares it private.
    let mut catalogue = Catalogue::new();
    let ad = exchange_advertisement();
    let key = tuples_of(&ad).remove(0);
    catalogue
        .grant(grant("g-1", &ad.provider, tuples_of(&ad), 100_000))
        .unwrap();
    catalogue.join(&ad, JOINED_AT).unwrap();
    catalogue.set_module_version(
        &ad.provider,
        rutent_contracts::router_api::Version::new("1").unwrap(),
    );
    catalogue.set_evidence_floor(key, EvidenceGrade::Statement);

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    assert!(built.snapshot.figures.is_empty());
    assert!(
        built
            .left_out
            .iter()
            .all(|l| l.reason == LeftOutReason::NodeNotListed("polygon".into()))
    );
    assert_eq!(built.left_out.len(), 2);
}

// verifies: REQ-CAT-22
#[test]
fn private_node_a_provider_declares_is_listed_without_the_operators_public_list() {
    let catalogue = acme_catalogue(&[internal_rail_advertisement()]);

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    assert!(!catalogue.nodes().is_public("acme-ledger"));
    assert_eq!(built.snapshot.figures.len(), 1);
    assert!(built.left_out.is_empty());
}

// verifies: REQ-CAT-22
#[test]
fn node_cannot_be_both_public_and_declared_private() {
    let mut catalogue = Catalogue::new();
    let ad = ramp_advertisement();
    catalogue
        .grant(grant("g-1", &ad.provider, tuples_of(&ad), 100_000))
        .unwrap();
    catalogue.join(&ad, JOINED_AT).unwrap();
    assert_eq!(
        catalogue.add_public_node("acme-ledger"),
        Err(NodeError::DeclaredPrivate("acme-ledger".into()))
    );

    let mut catalogue = Catalogue::new();
    catalogue.add_public_node("acme-ledger").unwrap();
    catalogue
        .grant(grant("g-1", &ad.provider, tuples_of(&ad), 100_000))
        .unwrap();
    assert_eq!(
        catalogue.join(&ad, JOINED_AT),
        Err(JoinRefusal::PrivateNodeIsPublic("acme-ledger".into()))
    );
}

// verifies: REQ-ORD-39
#[test]
fn provider_below_the_tuples_evidence_floor_is_left_out_of_the_snapshot() {
    let mut catalogue = acme_catalogue(&acme());
    // bridge-pay declares grade 2; a floor of grade 1 on its first tuple rules it out, and grade 2 keeps it.
    let keys = tuples_of(&ramp_advertisement());
    catalogue.set_evidence_floor(keys[0].clone(), EvidenceGrade::PublicRecord);
    catalogue.set_evidence_floor(keys[1].clone(), EvidenceGrade::SignedByAnotherParty);

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    let out: Vec<_> = built.left_out.iter().collect();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].tuple, keys[0]);
    assert_eq!(
        out[0].reason,
        LeftOutReason::BelowEvidenceFloor {
            floor: EvidenceGrade::PublicRecord,
            declared: Some(EvidenceGrade::SignedByAnotherParty),
        }
    );
    assert!(origins(&built.snapshot).contains(&("bridge-pay".into(), "acme".into())));
    assert_eq!(built.snapshot.figures.len(), 5);
}

// verifies: REQ-ORD-39
#[test]
fn tuple_with_no_evidence_floor_is_left_out_of_the_snapshot() {
    let mut catalogue = Catalogue::new();
    catalogue.add_public_node("polygon").unwrap();
    catalogue
        .register_public_holding("polygon", "wallet")
        .unwrap();
    let ad = exchange_advertisement();
    catalogue
        .grant(grant("g-1", &ad.provider, tuples_of(&ad), 100_000))
        .unwrap();
    catalogue.join(&ad, JOINED_AT).unwrap();
    catalogue.set_module_version(
        &ad.provider,
        rutent_contracts::router_api::Version::new("1").unwrap(),
    );

    assert_eq!(
        left_out_reasons(&catalogue),
        vec![LeftOutReason::NoEvidenceFloor; 2]
    );
}

// verifies: AC-29, REQ-CAT-20
#[test]
fn one_publisher_with_several_providers_and_a_private_node_is_shown_behind_each() {
    let catalogue = acme_catalogue(&acme());

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    assert!(built.left_out.is_empty());
    let origins = origins(&built.snapshot);
    for provider in ["polygon-wallet-reserve", "bridge-pay", "dex-pool"] {
        assert!(
            origins.contains(&(provider.into(), "acme".into())),
            "{provider} is shown behind acme"
        );
    }
    assert!(origins.iter().all(|(_, publisher)| publisher == "acme"));
    assert_eq!(catalogue.publishers().nodes_of("acme"), vec!["acme-ledger"]);
    assert_eq!(catalogue.publishers().providers_of("acme").len(), 3);
    // Each conversion is a segment of its own: one figure per exchange tuple, one of them changing no value.
    let exchanges = built
        .snapshot
        .figures
        .iter()
        .filter(|f| matches!(f.value, FigureValue::ExchangeTuple { .. }))
        .count();
    assert_eq!(exchanges, 2);
}

// verifies: AC-30, REQ-CAT-13
#[test]
fn exchange_spanning_two_networks_is_refused() {
    let exchange = |from: &str, to: &str| TupleKey {
        category: Category::Exchange,
        origin: Endpoint::Inner(from.into()),
        destination: Endpoint::Inner(to.into()),
        asset: "USDC".into(),
        to_asset: Some("USDT".into()),
    };

    assert_eq!(check_tuple(&exchange("polygon", "polygon")), Ok(()));
    assert_eq!(
        check_tuple(&exchange("polygon", "solana")),
        Err(TupleRefusal("an exchange stays at one node"))
    );
    // A grant can't admit it either.
    let mut terms = grant(
        "g-1",
        "dex-pool",
        tuples_of(&exchange_advertisement()),
        5_000,
    )
    .terms();
    terms.tuples = vec![exchange("polygon", "solana")];
    assert!(matches!(
        rutent_prov::catalogue::Grant::new(terms),
        Err(GrantError::MalformedTuple(_))
    ));
}

// verifies: AC-30, REQ-CAT-13
#[test]
fn ramp_within_one_network_and_reserve_between_two_inner_nodes_are_refused() {
    let ramp = TupleKey {
        category: Category::Ramp,
        origin: Endpoint::Inner("polygon".into()),
        destination: Endpoint::Inner("polygon".into()),
        asset: "USDC".into(),
        to_asset: None,
    };
    let reserve = TupleKey {
        category: Category::Reserve,
        origin: Endpoint::Inner("polygon".into()),
        destination: Endpoint::Inner("acme-ledger".into()),
        asset: "USDC".into(),
        to_asset: None,
    };
    assert!(check_tuple(&ramp).is_err());
    assert!(check_tuple(&reserve).is_err());
    for key in tuples_of(&acme_catalogue_ads()) {
        assert_eq!(check_tuple(&key), Ok(()), "{key:?}");
    }
}

fn acme_catalogue_ads() -> rutent_contracts::provider_role::Advertisement {
    advertisement("advertisement-provider-under-several-categories")
}

// verifies: REQ-CAT-23
#[test]
fn node_without_a_holding_account_cannot_be_on_a_route() {
    let mut catalogue = Catalogue::new();
    catalogue.add_public_node("polygon").unwrap();
    let ad = ramp_advertisement();
    let keys = tuples_of(&ad);
    catalogue
        .grant(grant("g-1", &ad.provider, keys.clone(), 100_000))
        .unwrap();
    catalogue.join(&ad, JOINED_AT).unwrap();
    catalogue.set_module_version(
        &ad.provider,
        rutent_contracts::router_api::Version::new("1").unwrap(),
    );
    for key in &keys {
        catalogue.set_evidence_floor(key.clone(), EvidenceGrade::Statement);
    }
    // Neither `polygon` (public) nor `acme-ledger` (private) has one yet.
    let reasons = left_out_reasons(&catalogue);
    assert_eq!(reasons.len(), 2);
    assert!(
        reasons
            .iter()
            .all(|r| matches!(r, LeftOutReason::NoHoldingAccount(_)))
    );

    catalogue
        .register_public_holding("polygon", "wallet")
        .unwrap();
    assert_eq!(left_out_reasons(&catalogue).len(), 2);

    catalogue
        .register_private_holding("acme-ledger", "acme", "record-interface")
        .unwrap();
    assert!(left_out_reasons(&catalogue).is_empty());
}

// verifies: REQ-CAT-23
#[test]
fn holding_account_of_a_private_node_comes_from_its_publisher() {
    let mut catalogue = acme_catalogue(&[ramp_advertisement()]);

    assert_eq!(
        catalogue.register_private_holding("acme-ledger", "other-co", "x"),
        Err(NodeError::NotItsPublisher {
            node: "acme-ledger".into(),
            recorded: "acme".into()
        })
    );
    assert_eq!(
        catalogue.register_private_holding("nowhere", "acme", "x"),
        Err(NodeError::NotDeclaredPrivate("nowhere".into()))
    );
    assert_eq!(
        catalogue.register_public_holding("acme-ledger", "x"),
        Err(NodeError::NotPublic("acme-ledger".into()))
    );
    assert_eq!(
        catalogue.nodes().holding("acme-ledger").unwrap().source,
        HoldingSource::Publisher("acme".into())
    );
    assert_eq!(
        catalogue.nodes().holding("polygon").unwrap().source,
        HoldingSource::Operator
    );
}

// verifies: REQ-CAT-11, REQ-CAT-12
#[test]
fn snapshot_carries_each_tuples_bands_increments_units_reporting_and_inputs() {
    let ad = ramp_advertisement();
    let catalogue = acme_catalogue(std::slice::from_ref(&ad));

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    let offered = &ad.offers.ramp.as_ref().unwrap().tuples;
    let carried: Vec<_> = built
        .snapshot
        .figures
        .iter()
        .map(|f| match &f.value {
            FigureValue::RampTuple { tuple } => tuple.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(&carried, offered);
}

// verifies: REQ-CAT-10
#[test]
fn evidence_availability_and_safe_retry_stay_readable_per_provider() {
    let catalogue = acme_catalogue(&[ramp_advertisement(), reserve_advertisement()]);

    let ramp = catalogue.declared_by("bridge-pay").unwrap();
    assert_eq!(ramp.safe_retry, Some(true));
    assert_eq!(
        ramp.evidence.as_ref().unwrap().grade,
        EvidenceGrade::SignedByAnotherParty
    );
    let reserve = catalogue.declared_by("polygon-wallet-reserve").unwrap();
    assert_eq!(reserve.safe_retry, Some(false));
    assert!(catalogue.declared_by("unknown").is_none());
}

// verifies: REQ-CAT-17
#[test]
fn outer_node_is_named_for_its_network_and_the_reserve_behind_it() {
    let catalogue = acme_catalogue(&[reserve_advertisement()]);

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    let figure = built
        .snapshot
        .figures
        .iter()
        .find(|f| matches!(&f.value, FigureValue::ReserveTuple { tuple } if matches!(&tuple.origin, NodeRef::Outer(o) if o.network == "polygon")))
        .expect("the withdrawal from the polygon network");
    assert_eq!(
        figure.origin.provider(),
        Id::new("polygon-wallet-reserve").as_ref()
    );
}

// verifies: REQ-QTE-4
#[test]
fn every_figure_carries_its_age_and_origin() {
    let catalogue = acme_catalogue(&[ramp_advertisement()]);

    let built = catalogue
        .snapshot("snap-1", Instant(JOINED_AT.0 + 90))
        .unwrap();

    for figure in &built.snapshot.figures {
        assert_eq!(figure.age_seconds.get(), 90);
        assert_eq!(figure.valid_for_seconds, None);
        match &figure.origin {
            Origin::Provider {
                provider,
                publisher,
                provider_version,
                module_version,
            } => {
                assert_eq!(provider.as_str(), "bridge-pay");
                assert_eq!(publisher.as_str(), "acme");
                assert_eq!(provider_version.as_str(), "2.0.0");
                assert_eq!(module_version.as_str(), "1.0.0");
            }
            Origin::Operator { .. } => panic!("an advertisement is a provider's figure"),
        }
    }
    assert_eq!(built.snapshot.taken_at.as_str(), "1970-01-01T00:26:30Z");
}

// verifies: REQ-CAT-2
#[test]
fn tuple_of_a_lapsed_grant_is_left_out_of_the_snapshot() {
    let catalogue = acme_catalogue(&[ramp_advertisement()]);

    let built = catalogue.snapshot("snap-1", Instant(200_000)).unwrap();

    assert!(built.snapshot.figures.is_empty());
    assert!(
        built
            .left_out
            .iter()
            .all(|l| matches!(l.reason, LeftOutReason::Unusable(Unusable::NotInForce(_))))
    );
}

// verifies: REQ-CAT-13
#[test]
fn every_advertised_tuple_is_in_the_snapshot_or_left_out_with_a_reason() {
    let mut catalogue = acme_catalogue(&acme());
    catalogue.set_evidence_floor(
        tuples_of(&exchange_advertisement()).remove(1),
        EvidenceGrade::PublicRecord,
    );
    catalogue.revoke("g-2", Instant(1_800)).unwrap();

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    let advertised: usize = acme().iter().map(|a| tuples_of(a).len()).sum();
    assert_eq!(
        built.snapshot.figures.len() + built.left_out.len(),
        advertised
    );
}

// verifies: DEC-24, AC-34
#[test]
fn route_across_two_private_nodes_of_one_publisher_is_in_the_snapshot() {
    let catalogue = acme_catalogue(&[internal_rail_advertisement()]);

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    assert_eq!(built.snapshot.figures.len(), 1);
    assert_eq!(
        catalogue.publishers().nodes_of("acme"),
        vec!["acme-card-book", "acme-ledger"]
    );
}

// verifies: REQ-CAT-8
#[test]
fn snapshot_with_a_bad_identifier_or_time_is_refused() {
    let catalogue = acme_catalogue(&[ramp_advertisement()]);

    assert_eq!(catalogue.snapshot("", NOW), Err(SnapshotError::BadId));
    assert_eq!(
        catalogue.snapshot("snap-1", Instant(u64::MAX)),
        Err(SnapshotError::TimeOutOfRange)
    );
}

// verifies: REQ-CAT-13
#[test]
fn snapshot_passes_the_router_apis_schema_and_round_trips() {
    let catalogue = acme_catalogue(&acme());

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    let value = serde_json::to_value(&built.snapshot).unwrap();
    let validator = snapshot_validator();
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let back: CatalogueSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(back, built.snapshot);
    back.check().unwrap();
}

fn snapshot_validator() -> jsonschema::Validator {
    let contracts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts");
    let read =
        |p: &Path| -> Value { serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap() };
    let mut registry = jsonschema::Registry::new();
    for contract in ["provider-role", "router-api"] {
        for entry in std::fs::read_dir(contracts.join(contract).join("schemas")).unwrap() {
            let path = entry.unwrap().path();
            if path.to_string_lossy().ends_with(".schema.json") {
                let schema = read(&path);
                let id = schema["$id"].as_str().unwrap().to_owned();
                registry = registry.add(id, schema).unwrap();
            }
        }
    }
    let registry = registry.prepare().unwrap();
    let schema = read(&contracts.join("router-api/schemas/catalogue-snapshot.schema.json"));
    jsonschema::options()
        .offline()
        .with_registry(&registry)
        .build(&schema)
        .unwrap()
}

// verifies: REQ-CAT-22, REQ-CAT-23
#[test]
fn provider_of_another_publisher_cannot_route_through_a_private_node() {
    // `bridge-pay` is acme's and declares `acme-ledger`; the same ramp under another publisher doesn't get it.
    let mut other = ramp_advertisement();
    other.provider = "rival-pay".into();
    other.publisher = "rival".into();
    other.private_nodes.clear();
    let mut catalogue = acme_catalogue(&[ramp_advertisement()]);
    let keys = tuples_of(&other);
    catalogue
        .grant(grant("g-rival", "rival-pay", keys.clone(), 100_000))
        .unwrap();
    catalogue.join(&other, JOINED_AT).unwrap();
    catalogue.set_module_version(
        "rival-pay",
        rutent_contracts::router_api::Version::new("1").unwrap(),
    );
    for key in keys {
        catalogue.set_evidence_floor(key, EvidenceGrade::Statement);
    }

    let built = catalogue.snapshot("snap-1", NOW).unwrap();

    assert!(
        origins(&built.snapshot)
            .iter()
            .all(|(p, _)| p == "bridge-pay")
    );
    assert!(built.left_out.iter().all(|l| l.provider == "rival-pay"
        && l.reason == LeftOutReason::NodeNotListed("acme-ledger".into())));
    assert_eq!(built.left_out.len(), 2);
}
