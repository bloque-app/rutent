//! The catalogue's grants, bounds, joins, declarations, publishers and reversal capacity (`T-012-01`).

mod common;

use common::*;
use rutent_prov::catalogue::{
    AcceptRefusal, Catalogue, CatalogueError, Category, DeclarationError, GrantError, Instant,
    JoinRefusal, Observation, OwedError, PublisherError, RecordOutcome, ReversalError, Standing,
    TupleBound, TupleKey, Unusable, tuples_of,
};

const NOW: Instant = Instant(2_000);

fn ramp_keys() -> Vec<TupleKey> {
    tuples_of(&ramp_advertisement())
}

/// A catalogue holding one grant of `bridge-pay` over its ramp tuples, expiring at `expires_at`.
fn catalogue_with_grant(expires_at: u64) -> Catalogue {
    let mut catalogue = Catalogue::new();
    catalogue
        .grant(grant("g-1", "bridge-pay", ramp_keys(), expires_at))
        .expect("grant is new");
    catalogue
}

// verifies: REQ-CAT-1
#[test]
fn restricted_tuple_is_not_usable() {
    let mut catalogue = catalogue_with_grant(10_000);
    let keys = ramp_keys();
    assert_eq!(catalogue.usable("bridge-pay", &keys[0], NOW), Ok(()));

    catalogue.bound("bridge-pay", keys[0].clone(), TupleBound::Restricted);

    assert_eq!(
        catalogue.usable("bridge-pay", &keys[0], NOW),
        Err(Unusable::Restricted)
    );
    assert_eq!(catalogue.usable("bridge-pay", &keys[1], NOW), Ok(()));
}

// verifies: REQ-CAT-1
#[test]
fn amount_over_the_operators_bound_for_a_tuple_is_not_usable() {
    let mut catalogue = catalogue_with_grant(10_000);
    let key = ramp_keys().remove(0);
    catalogue.bound("bridge-pay", key.clone(), TupleBound::Max(5_000));

    assert_eq!(catalogue.usable_for("bridge-pay", &key, 5_000, NOW), Ok(()));
    assert_eq!(
        catalogue.usable_for("bridge-pay", &key, 5_001, NOW),
        Err(Unusable::OverBound { max: 5_000 })
    );
}

// verifies: REQ-CAT-1, REQ-CAT-2
#[test]
fn tuple_outside_the_grants_scope_is_not_usable() {
    let mut catalogue = Catalogue::new();
    let keys = ramp_keys();
    catalogue
        .grant(grant("g-1", "bridge-pay", vec![keys[0].clone()], 10_000))
        .unwrap();

    assert_eq!(catalogue.usable("bridge-pay", &keys[0], NOW), Ok(()));
    assert_eq!(
        catalogue.usable("bridge-pay", &keys[1], NOW),
        Err(Unusable::OutsideGrantScope)
    );
}

// verifies: REQ-CAT-2
#[test]
fn provider_with_no_grant_is_not_usable() {
    let catalogue = Catalogue::new();
    let key = ramp_keys().remove(0);
    assert_eq!(
        catalogue.usable("bridge-pay", &key, NOW),
        Err(Unusable::NoGrant)
    );
}

// verifies: REQ-CAT-2
#[test]
fn expired_grant_stops_the_provider_being_routed() {
    let catalogue = catalogue_with_grant(3_000);
    let key = ramp_keys().remove(0);

    assert_eq!(catalogue.usable("bridge-pay", &key, Instant(2_999)), Ok(()));
    assert_eq!(
        catalogue.usable("bridge-pay", &key, Instant(3_000)),
        Err(Unusable::NotInForce(Standing::Expired))
    );
}

// verifies: REQ-CAT-2
#[test]
fn revoked_grant_stops_the_provider_being_routed() {
    let mut catalogue = catalogue_with_grant(10_000);
    let key = ramp_keys().remove(0);
    catalogue.revoke("g-1", Instant(2_500)).unwrap();

    assert_eq!(catalogue.usable("bridge-pay", &key, Instant(2_499)), Ok(()));
    assert_eq!(
        catalogue.usable("bridge-pay", &key, Instant(2_500)),
        Err(Unusable::NotInForce(Standing::Revoked))
    );
    assert_eq!(
        catalogue.revoke("nope", Instant(1)),
        Err(CatalogueError::UnknownGrant("nope".into()))
    );
}

// verifies: REQ-CAT-2
#[test]
fn earlier_grants_stay_visible_at_re_entry() {
    let mut catalogue = catalogue_with_grant(3_000);
    catalogue.revoke("g-1", Instant(2_100)).unwrap();
    let mut second = grant("g-2", "bridge-pay", ramp_keys(), 20_000);
    second = rutent_prov::catalogue::Grant::new(rutent_prov::catalogue::GrantTerms {
        granted_at: Instant(5_000),
        ..second.terms()
    })
    .unwrap();
    catalogue.grant(second).unwrap();

    let history: Vec<&str> = catalogue
        .grants_of_party("Acme Payments Ltd")
        .iter()
        .map(|g| g.id())
        .collect();
    assert_eq!(history, ["g-1", "g-2"]);
    assert_eq!(
        catalogue.grants_of_party("Acme Payments Ltd")[0].standing(Instant(6_000)),
        Standing::Revoked
    );
    let key = ramp_keys().remove(0);
    assert_eq!(catalogue.usable("bridge-pay", &key, Instant(6_000)), Ok(()));
}

// verifies: REQ-CAT-2
#[test]
fn grant_must_be_attributable_scoped_and_expire_after_it_starts() {
    let base = grant("g", "bridge-pay", ramp_keys(), 9_000).terms();
    let refused = |terms| rutent_prov::catalogue::Grant::new(terms).err();

    assert_eq!(
        refused(rutent_prov::catalogue::GrantTerms {
            granted_by: " ".into(),
            ..base.clone()
        }),
        Some(GrantError::NotAttributable)
    );
    assert_eq!(
        refused(rutent_prov::catalogue::GrantTerms {
            party: "".into(),
            ..base.clone()
        }),
        Some(GrantError::NoNamedParty)
    );
    assert_eq!(
        refused(rutent_prov::catalogue::GrantTerms {
            expires_at: Instant(1_000),
            ..base.clone()
        }),
        Some(GrantError::ExpiresBeforeItStarts)
    );
    assert_eq!(
        refused(rutent_prov::catalogue::GrantTerms {
            categories: vec![],
            ..base.clone()
        }),
        Some(GrantError::NoCategories)
    );
    assert_eq!(
        refused(rutent_prov::catalogue::GrantTerms {
            tuples: vec![],
            ..base.clone()
        }),
        Some(GrantError::CategoryWithoutTuples(Category::Ramp))
    );
    let wrong = ramp_keys().remove(0);
    assert_eq!(
        refused(rutent_prov::catalogue::GrantTerms {
            categories: vec![Category::Exchange],
            tuples: vec![wrong],
            ..base
        }),
        Some(GrantError::TupleOutsideCategories(Category::Ramp))
    );
}

// verifies: REQ-CAT-2
#[test]
fn grant_id_is_never_reused() {
    let mut catalogue = catalogue_with_grant(10_000);
    assert_eq!(
        catalogue.grant(grant("g-1", "bridge-pay", ramp_keys(), 99_999)),
        Err(CatalogueError::DuplicateGrant("g-1".into()))
    );
}

// verifies: DEC-11
#[test]
fn ceiling_above_the_bond_is_refused() {
    assert_eq!(
        rutent_prov::catalogue::ReversalCapacity::check(bond(100), 101, "ops-lead", Instant(1)),
        Err(ReversalError::CeilingAboveBond {
            ceiling: 101,
            bond: 100
        })
    );
    let mut catalogue = catalogue_with_grant(10_000);
    assert_eq!(
        catalogue.set_ceiling("g-1", 1_001),
        Err(CatalogueError::Reversal(ReversalError::CeilingAboveBond {
            ceiling: 1_001,
            bond: 1_000
        }))
    );
    assert_eq!(catalogue.ceiling_of("bridge-pay", NOW), Some(500));
    catalogue.set_ceiling("g-1", 1_000).unwrap();
    assert_eq!(catalogue.ceiling_of("bridge-pay", NOW), Some(1_000));
}

// verifies: DEC-11
#[test]
fn ceiling_follows_the_bond_when_it_falls() {
    let mut catalogue = catalogue_with_grant(10_000);
    catalogue
        .rebond("g-1", bond(300), "ops-lead", Instant(2_000))
        .unwrap();
    assert_eq!(catalogue.ceiling_of("bridge-pay", NOW), Some(300));

    catalogue
        .rebond("g-1", bond(900), "ops-lead", Instant(2_100))
        .unwrap();
    assert_eq!(
        catalogue.ceiling_of("bridge-pay", NOW),
        Some(300),
        "a larger bond doesn't raise the ceiling on its own"
    );
}

// verifies: DEC-11
#[test]
fn reversal_capacity_check_is_recorded_with_the_grant() {
    let mut catalogue = catalogue_with_grant(10_000);
    let before = catalogue.grants_of_provider("bridge-pay")[0]
        .reversal_capacity()
        .clone();
    assert_eq!(before.checked_by(), "ops-lead");
    assert_eq!(before.checked_at(), Instant(1_000));

    catalogue
        .rebond("g-1", bond(800), "risk-officer", Instant(4_000))
        .unwrap();
    let after = catalogue.grants_of_provider("bridge-pay")[0].reversal_capacity();
    assert_eq!(
        (after.checked_by(), after.checked_at()),
        ("risk-officer", Instant(4_000))
    );
    assert_eq!(after.bond().held_by, "escrow-co");
}

// verifies: DEC-11
#[test]
fn rebond_with_no_holder_or_no_checker_is_refused_and_changes_nothing() {
    let mut cap = capacity(1_000, 500);
    let mut no_holder = bond(100);
    no_holder.held_by = " ".into();
    assert_eq!(
        cap.rebond(no_holder, "ops-lead", Instant(2)),
        Err(ReversalError::NotAttributable)
    );
    assert_eq!(
        cap.rebond(bond(100), "", Instant(2)),
        Err(ReversalError::NotAttributable)
    );
    assert_eq!(
        (cap.bond().amount, cap.ceiling(), cap.checked_at()),
        (1_000, 500, Instant(1_000))
    );
}

// verifies: REQ-CAT-7
#[test]
fn silence_leaves_an_owed_reversal_owed() {
    let mut catalogue = catalogue_with_grant(3_000);
    let owed = catalogue.owed_mut();
    owed.owe("rev-1", "bridge-pay", "USDC", 40).unwrap();

    owed.observe(
        "rev-1",
        Observation::Silence {
            since: Instant(2_100),
        },
    )
    .unwrap();
    owed.observe("rev-1", Observation::StandingLost).unwrap();
    assert!(catalogue.owed().is_owed("rev-1"));

    // Expiry and revocation of the grant don't discharge it either.
    catalogue.revoke("g-1", Instant(2_200)).unwrap();
    assert!(catalogue.owed().is_owed("rev-1"));
    assert_eq!(catalogue.owed().owed_by("bridge-pay").len(), 1);
}

// verifies: REQ-CAT-7
#[test]
fn only_a_confirmation_discharges_an_owed_reversal() {
    let mut catalogue = Catalogue::new();
    let owed = catalogue.owed_mut();
    owed.owe("rev-1", "bridge-pay", "USDC", 40).unwrap();
    owed.observe("rev-1", Observation::Confirmed).unwrap();
    assert!(!owed.is_owed("rev-1"));
    assert!(
        owed.owe("rev-1", "bridge-pay", "USDC", 40).is_err(),
        "an identifier is never reused"
    );
    assert_eq!(
        owed.observe("unknown", Observation::Confirmed),
        Err(OwedError::UnknownOwed("unknown".into()))
    );
}

// verifies: REQ-CAT-6
#[test]
fn declaration_is_recorded_once_and_shown_before_acceptance() {
    let mut catalogue = Catalogue::new();
    let ad = reserve_advertisement();
    let first = catalogue
        .declarations_mut()
        .record(&ad, Instant(2_000))
        .unwrap()
        .unwrap();
    let again = catalogue
        .declarations_mut()
        .record(&ad, Instant(2_500))
        .unwrap()
        .unwrap();
    let RecordOutcome::Recorded(id) = first else {
        panic!("first is recorded")
    };
    assert_eq!(again, RecordOutcome::AlreadyRecorded(id));
    assert_eq!(
        catalogue
            .declarations()
            .records_of("polygon-wallet-reserve")
            .len(),
        1
    );

    let record = catalogue
        .declarations()
        .records_of("polygon-wallet-reserve")[0];
    assert_eq!(record.maker, "polygon-wallet-reserve");
    assert!(record.starts_routes_on_automatic_deposits);

    assert_eq!(
        catalogue
            .declarations()
            .accept("alice", "polygon-wallet-reserve"),
        Err(AcceptRefusal::NotShown)
    );
    let shown = catalogue
        .declarations_mut()
        .show_to("alice", "polygon-wallet-reserve")
        .unwrap();
    assert_eq!(shown.id, id);
    assert_eq!(
        catalogue
            .declarations()
            .accept("alice", "polygon-wallet-reserve"),
        Ok(())
    );
    assert_eq!(
        catalogue
            .declarations()
            .accept("bob", "polygon-wallet-reserve"),
        Err(AcceptRefusal::NotShown),
        "shown to one user, not to another"
    );
    assert_eq!(
        catalogue.declarations().accept("alice", "unrecorded"),
        Err(AcceptRefusal::NoDeclaration)
    );
}

// verifies: REQ-CAT-6
#[test]
fn changed_declaration_in_a_later_version_is_recorded_again_and_shown_again() {
    let mut catalogue = Catalogue::new();
    let ad = reserve_advertisement();
    catalogue
        .declarations_mut()
        .record(&ad, Instant(1))
        .unwrap();
    catalogue
        .declarations_mut()
        .show_to("alice", &ad.provider)
        .unwrap();

    let mut unchanged_later = ad.clone();
    unchanged_later.version = "0.3.0".into();
    let outcome = catalogue
        .declarations_mut()
        .record(&unchanged_later, Instant(2))
        .unwrap()
        .unwrap();
    assert!(matches!(outcome, RecordOutcome::AlreadyRecorded(_)));
    assert_eq!(
        catalogue.declarations().accept("alice", &ad.provider),
        Ok(())
    );

    let mut changed = ad.clone();
    changed.version = "0.4.0".into();
    let reserve = changed.offers.reserve.as_mut().unwrap();
    reserve.declaration.can_require_user_initiated_withdrawal = true;
    reserve.declaration.declared_in_version = "0.4.0".into();
    let outcome = catalogue
        .declarations_mut()
        .record(&changed, Instant(3))
        .unwrap()
        .unwrap();
    assert!(matches!(outcome, RecordOutcome::Recorded(_)));
    assert_eq!(catalogue.declarations().records_of(&ad.provider).len(), 2);
    assert_eq!(
        catalogue.declarations().accept("alice", &ad.provider),
        Err(AcceptRefusal::NotShown)
    );
}

// verifies: REQ-CAT-6
#[test]
fn declaration_changed_within_one_version_is_refused() {
    let mut catalogue = Catalogue::new();
    let ad = reserve_advertisement();
    catalogue
        .declarations_mut()
        .record(&ad, Instant(1))
        .unwrap();

    let mut changed = ad.clone();
    changed
        .offers
        .reserve
        .as_mut()
        .unwrap()
        .declaration
        .starts_routes_on_automatic_deposits = false;
    assert_eq!(
        catalogue.declarations_mut().record(&changed, Instant(2)),
        Err(DeclarationError::ChangedWithinVersion {
            provider: ad.provider,
            version: "0.2.0".into()
        })
    );
    assert_eq!(
        catalogue
            .declarations_mut()
            .record(&ramp_advertisement(), Instant(2)),
        Ok(None),
        "a provider with no reserve declares nothing"
    );
}

// verifies: REQ-CAT-16
#[test]
fn provider_with_neither_identifier_nor_status_query_cannot_join() {
    let mut catalogue = catalogue_with_grant(10_000);
    let mut ad = ramp_advertisement();
    ad.declarations.tracking.clear();
    assert_eq!(join(&mut catalogue, &ad, NOW), Err(JoinRefusal::NoTracking));
    assert!(
        catalogue
            .publishers()
            .publisher_of_provider("bridge-pay")
            .is_none()
    );

    let ad = ramp_advertisement();
    assert!(join(&mut catalogue, &ad, NOW).is_ok());
}

// verifies: REQ-CAT-2
#[test]
fn provider_without_a_grant_in_force_cannot_join() {
    let mut catalogue = catalogue_with_grant(3_000);
    assert_eq!(
        join(&mut catalogue, &ramp_advertisement(), Instant(3_000)),
        Err(JoinRefusal::NoGrantInForce)
    );
    let mut other = Catalogue::new();
    assert_eq!(
        join(&mut other, &ramp_advertisement(), NOW),
        Err(JoinRefusal::NoGrantInForce)
    );
}

// verifies: REQ-CAT-20
#[test]
fn publisher_is_recorded_behind_each_provider_and_private_node() {
    let mut catalogue = Catalogue::new();
    catalogue
        .grant(grant("g-1", "bridge-pay", ramp_keys(), 10_000))
        .unwrap();
    let reserve_keys = tuples_of(&reserve_advertisement());
    catalogue
        .grant(grant("g-2", "polygon-wallet-reserve", reserve_keys, 10_000))
        .unwrap();
    join(&mut catalogue, &ramp_advertisement(), NOW).unwrap();
    join(&mut catalogue, &reserve_advertisement(), NOW).unwrap();

    let publishers = catalogue.publishers();
    assert_eq!(publishers.publisher_of_provider("bridge-pay"), Some("acme"));
    assert_eq!(
        publishers.publisher_of_provider("polygon-wallet-reserve"),
        Some("acme")
    );
    assert_eq!(publishers.publisher_of_node("acme-ledger"), Some("acme"));
    assert_eq!(
        publishers.providers_of("acme"),
        ["bridge-pay", "polygon-wallet-reserve"]
    );
    assert_eq!(publishers.nodes_of("acme"), ["acme-ledger"]);
}

// verifies: REQ-CAT-20
#[test]
fn provider_and_node_cannot_change_publisher_unseen() {
    let mut catalogue = Catalogue::new();
    catalogue
        .grant(grant("g-1", "bridge-pay", ramp_keys(), 10_000))
        .unwrap();
    join(&mut catalogue, &ramp_advertisement(), NOW).unwrap();

    let mut moved = ramp_advertisement();
    moved.publisher = "mallory".into();
    assert_eq!(
        join(&mut catalogue, &moved, NOW),
        Err(JoinRefusal::Publisher(
            PublisherError::ProviderChangedPublisher {
                provider: "bridge-pay".into(),
                recorded: "acme".into(),
                offered: "mallory".into(),
            }
        ))
    );

    let mut other = ramp_advertisement();
    other.provider = "other-pay".into();
    other.publisher = "mallory".into();
    catalogue
        .grant(grant("g-3", "other-pay", ramp_keys(), 10_000))
        .unwrap();
    assert_eq!(
        join(&mut catalogue, &other, NOW),
        Err(JoinRefusal::Publisher(
            PublisherError::NodeDeclaredByAnotherPublisher {
                node: "acme-ledger".into(),
                recorded: "acme".into(),
            }
        ))
    );
    assert!(
        catalogue
            .publishers()
            .publisher_of_provider("other-pay")
            .is_none(),
        "a refused join leaves no trace"
    );
}

// verifies: REQ-MOD-4
#[test]
fn stand_in_provider_is_treated_like_any_other_through_the_catalogue() {
    let mut catalogue = Catalogue::new();
    let key = ramp_keys().remove(0);
    for name in ["stand-in", "bloque-own-provider"] {
        catalogue
            .grant(grant(&format!("g-{name}"), name, vec![key.clone()], 10_000))
            .unwrap();
    }
    catalogue.bound("bloque-own-provider", key.clone(), TupleBound::Restricted);

    assert_eq!(catalogue.usable("stand-in", &key, NOW), Ok(()));
    assert_eq!(
        catalogue.usable("bloque-own-provider", &key, NOW),
        Err(Unusable::Restricted),
        "our own provider is bound by the same rules as a stand-in"
    );
    assert_eq!(
        catalogue.usable("a-third", &key, NOW),
        Err(Unusable::NoGrant)
    );
}
