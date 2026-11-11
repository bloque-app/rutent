//! Holding accounts the operator publishes (`T-016-04`; `REQ-MDL-1`, `REQ-CAT-23`, `INV-5`): a public node's plain
//! wallet is the operator's own module, admitted on the operator's release, signed with the deployment's operator
//! key, with no provider advertisement behind it; and only such a module is ever a node's holding account.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;
#[path = "holding_support/mod.rs"]
mod support;

use rutent_contracts::module_interface::RuleViolation;
use rutent_contracts::provider_role::VERSION;
use rutent_prov::catalogue::CatalogueError;
use rutent_prov::catalogue::{
    HoldingSource, JoinRefusal, Publisher, PublisherError, StatementRefusal,
};
use rutent_prov::holding::wallet::{Holdings, RegisterError};
use rutent_prov::module::admission::Unverified;
use rutent_prov::module::types::Admitted;
use signed::catalogue_support::{advertising_key, operator_key, public_key, statements_key};
use signed::{RAMP, RESERVE, Signed};
use support::{OPERATOR, POLYGON_WALLET, TestOperator, admit_providers_wallet, wallet_release};

/// How many compilations have succeeded so far.
fn compiled(s: &Signed) -> usize {
    s.platform.measurements.compiles.len()
}

// verifies: REQ-MDL-1, REQ-CAT-23
#[test]
fn operator_published_holding_module_is_admitted_on_the_operators_own_release() {
    let mut s = Signed::new();
    // The operator publishes the wallet: the catalogue records the operator as its publisher, and it has no
    // advertisement of any provider behind it.
    s.catalogue
        .publish_operator_module(POLYGON_WALLET)
        .expect("the operator publishes its wallet");
    assert_eq!(
        s.catalogue.publishers().publisher(POLYGON_WALLET),
        Some(Publisher::Operator)
    );
    assert!(s.catalogue.accepted_advertisement(POLYGON_WALLET).is_none());
    assert!(
        s.catalogue
            .publishers()
            .publisher_of_provider(POLYGON_WALLET)
            .is_none(),
        "the operator is no provider's publisher"
    );

    let (declaration, binaries) = wallet_release(POLYGON_WALLET, "1.0.0");
    let before = compiled(&s);

    // Signed with any key but the deployment's operator key, it is refused before compiling: the wallet's own
    // advertising key, a statements key, and a ramp provider's advertising key.
    for key in [
        advertising_key(POLYGON_WALLET),
        statements_key(POLYGON_WALLET),
        advertising_key(RAMP),
    ] {
        let assertion = signed::assertion_by(&declaration, &binaries, OPERATOR, &key);
        assert_eq!(
            s.admit(&assertion, &binaries),
            Admitted::Unverified(Unverified::Signature(StatementRefusal::NotSignedByOperator)),
            "signed with {}",
            public_key(&key).as_str()
        );
    }
    // Signed with the operator key, but declaring the operator key as its statement key.
    let mut handing_over = declaration.clone();
    handing_over.outside.statement_key = public_key(&operator_key());
    let assertion = signed::assertion_by(&handing_over, &binaries, OPERATOR, &operator_key());
    assert_eq!(
        s.admit(&assertion, &binaries),
        Admitted::Unverified(Unverified::OperatorAssertion(vec![
            RuleViolation::StatementKeyIsOperatorKey
        ]))
    );
    // Signed with the operator key, but altered after signing.
    let mut altered = signed::assertion_by(&declaration, &binaries, OPERATOR, &operator_key());
    let at = signed::find(&altered, b"1.0.0").expect("the version");
    altered[at + 4] = b'1';
    assert!(matches!(
        s.admit(&altered, &binaries),
        Admitted::Unverified(Unverified::Signature(StatementRefusal::BadSignature(_)))
    ));
    assert_eq!(compiled(&s), before, "nothing refused was compiled");
    assert!(s.platform.active_version(POLYGON_WALLET).is_none());

    // The operator's own release, signed with the deployment's operator key, is admitted, waits for its fresh
    // acceptance like any first release, and runs once accepted, judged again at acceptance.
    let assertion = signed::assertion_by(&declaration, &binaries, OPERATOR, &operator_key());
    let Admitted::Artefact(artefact) = s.admit(&assertion, &binaries) else {
        panic!("the operator's release wasn't admitted");
    };
    assert_eq!(
        artefact.assertion.signature.key,
        public_key(&operator_key())
    );
    assert_eq!(
        artefact.assertion.deployment.operator_key,
        signed::catalogue_support::deployment().operator_key
    );
    assert!(artefact.acceptance.accepted.is_none());
    assert!(s.platform.active_version(POLYGON_WALLET).is_none());

    // Acceptance judges the release again: once the catalogue's settings name another operator key, the release
    // signed with the old one is no longer accepted, and it stays waiting.
    let mut rekeyed = signed::catalogue_support::deployment();
    rekeyed.operator_key = public_key(&signed::catalogue_support::signing_key("operator", 2));
    s.catalogue.set_context(rekeyed, VERSION);
    assert_eq!(
        s.accept(POLYGON_WALLET, &artefact.assertion_digest, "test-reviewer"),
        None
    );
    assert!(s.platform.pending(POLYGON_WALLET).is_some());
    assert!(s.platform.active_version(POLYGON_WALLET).is_none());

    // Under the operator key its settings name, it is accepted.
    s.catalogue
        .set_context(signed::catalogue_support::deployment(), VERSION);
    s.accept(POLYGON_WALLET, &artefact.assertion_digest, "test-reviewer")
        .expect("the operator's release is accepted");
    assert_eq!(
        s.platform.active_version(POLYGON_WALLET).as_deref(),
        Some("1.0.0")
    );

    // It is a node's holding account: registered at a public node, it takes the value landing there.
    let mut holdings = Holdings::default();
    holdings
        .register(
            &mut s.catalogue,
            &s.platform,
            "polygon",
            POLYGON_WALLET,
            "USDC",
        )
        .expect("the operator registers its wallet");
    let account = s.catalogue.nodes().holding("polygon").expect("registered");
    assert_eq!(account.reference, POLYGON_WALLET);
    assert_eq!(account.source, HoldingSource::Operator);
    holdings
        .land(
            &mut s.platform,
            &TestOperator,
            "polygon",
            "USDC",
            "o-1",
            "100",
        )
        .expect("the wallet takes the value");
}

// verifies: REQ-MDL-1, THR-4
#[test]
fn operator_module_declaring_a_key_a_provider_holds_as_its_statement_key_is_refused() {
    let mut s = Signed::new();
    s.catalogue
        .publish_operator_module(POLYGON_WALLET)
        .expect("the operator publishes its wallet");
    let (declaration, binaries) = wallet_release(POLYGON_WALLET, "1.0.0");
    let before = compiled(&s);

    // A key that signs a provider's advertisements, and a key a provider's accepted advertisement lists for its
    // statements, which that provider's outside part can resolve: neither is the operator module's to declare.
    for (key, refusal) in [
        (
            advertising_key(RESERVE),
            Unverified::StatementKey(CatalogueError::KeyIsAnAdvertisingKey),
        ),
        (statements_key(RAMP), Unverified::StatementKeyAdvertised),
    ] {
        let mut declaring = declaration.clone();
        declaring.outside.statement_key = public_key(&key);
        let assertion = signed::assertion_by(&declaring, &binaries, OPERATOR, &operator_key());
        assert_eq!(
            s.admit(&assertion, &binaries),
            Admitted::Unverified(refusal),
            "declaring {}",
            public_key(&key).as_str()
        );
    }
    assert_eq!(compiled(&s), before, "nothing refused was compiled");
    assert!(s.platform.pending(POLYGON_WALLET).is_none());
    assert!(s.platform.active_version(POLYGON_WALLET).is_none());
}

// verifies: REQ-MDL-1
#[test]
fn operator_and_providers_cannot_release_each_others_modules() {
    let mut s = Signed::new();
    s.catalogue
        .publish_operator_module(POLYGON_WALLET)
        .expect("the operator publishes its wallet");
    let before = compiled(&s);

    // A provider's module signed with the operator key is held to the provider's rules, which refuse it.
    let (declaration, binaries) = wallet_release(RESERVE, "1.0.0");
    // Made by the provider, the operator key isn't one its advertisement lists; made by the operator, the catalogue
    // holds no advertisement of that maker at all.
    for (maker, refusal) in [
        (RESERVE, StatementRefusal::KeyNotInForce),
        (OPERATOR, StatementRefusal::NoAdvertisementThen),
    ] {
        let assertion = signed::assertion_by(&declaration, &binaries, maker, &operator_key());
        assert_eq!(
            s.admit(&assertion, &binaries),
            Admitted::Unverified(Unverified::Signature(refusal)),
            "made by {maker}"
        );
    }
    assert_eq!(compiled(&s), before);
    assert!(s.platform.active_version(RESERVE).is_none());

    // The operator can't take a provider's name, and a provider can't advertise under the operator's module.
    assert_eq!(
        s.catalogue.publish_operator_module(RESERVE),
        Err(PublisherError::ProviderOfAPublisher {
            provider: RESERVE.into(),
            publisher: s
                .catalogue
                .publishers()
                .publisher_of_provider(RESERVE)
                .expect("the reserve's publisher")
                .to_owned(),
        })
    );
    let mut ad = signed::catalogue_support::reserve_advertisement();
    ad.provider = POLYGON_WALLET.to_owned();
    ad.private_nodes = Vec::new();
    if let Some(reserve) = ad.offers.reserve.as_mut() {
        reserve.declaration.maker = POLYGON_WALLET.to_owned();
    }
    s.catalogue
        .grant(signed::catalogue_support::grant(
            "g-impostor",
            POLYGON_WALLET,
            rutent_prov::catalogue::tuples_of(&ad),
            4_000_000_000,
        ))
        .expect("a grant");
    assert_eq!(
        signed::catalogue_support::join_made_at(
            &mut s.catalogue,
            &ad,
            rutent_prov::catalogue::Instant(signed::AT.0 - 60),
            signed::AT
        ),
        Err(JoinRefusal::Publisher(
            PublisherError::ProviderIsTheOperators(POLYGON_WALLET.into())
        ))
    );
    assert!(s.catalogue.accepted_advertisement(POLYGON_WALLET).is_none());
    assert_eq!(
        s.catalogue.publishers().publisher(POLYGON_WALLET),
        Some(Publisher::Operator)
    );
}

// verifies: REQ-CAT-23
#[test]
fn provider_module_cannot_be_registered_as_a_holding_account() {
    let mut s = Signed::new();
    // A reserve's and a ramp's own wallets, each admitted on its provider's signed release and running.
    for provider in [RESERVE, RAMP] {
        let Admitted::Artefact(artefact) = admit_providers_wallet(&mut s, provider, "1.0.0") else {
            panic!("{provider}'s release wasn't admitted");
        };
        s.accept(provider, &artefact.assertion_digest, "test-reviewer")
            .expect("accepted");
        assert!(s.platform.active_version(provider).is_some(), "{provider}");
    }

    let mut holdings = Holdings::default();
    for provider in [RESERVE, RAMP] {
        assert_eq!(
            holdings.register(&mut s.catalogue, &s.platform, "polygon", provider, "USDC"),
            Err(RegisterError::NotOperatorPublished(provider.to_owned())),
            "{provider}"
        );
    }
    assert!(s.catalogue.nodes().holding("polygon").is_none());
    assert!(holdings.account("polygon").is_none());
}
