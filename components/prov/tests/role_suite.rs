//! The role suite with signatures verified (`T-012-02`; `REQ-CAT-8`, `REQ-PRT-1`, `THR-4`).
//!
//! The stand-ins sign with placeholders, which no Ed25519 verifier accepts. Each is run behind [`Signing`], which
//! gives every statement it sends a real signature, derived from the placeholder key it names, and re-signs a
//! statement nested in another first. The catalogue, holding the provider's advertisement as accepted under the
//! operator's grant, is the suite's verifier.

mod common;

use common::*;
use ed25519_dalek::SigningKey;
use rutent_conformance::role::stand_in::{Exchange, Ramp, Reserve, Verifier};
use rutent_conformance::role::{
    Check, Received, RoleHarness, RoleProvider, SignatureVerifier, SuiteContext, Verification,
    run_role_suite,
};
use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Advertisement, Operation, PublicKey, ResultBody, Signature, Statement,
};
use rutent_prov::catalogue::{Catalogue, Grant, GrantTerms, Instant, tuples_of};
use serde_json::Value;

/// The signing key a placeholder key stands for: its bytes are the seed.
fn real(placeholder: &str) -> SigningKey {
    SigningKey::from_bytes(&PublicKey::new(placeholder).expect("a key").bytes())
}

/// Signs `value` and every statement inside it, innermost first, with the keys its placeholders stand for.
fn resign(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(resign),
        Value::Object(map) => {
            map.values_mut().for_each(resign);
            let is_statement = map.get("signature").is_some_and(|s| s["key"].is_string())
                && map.contains_key("maker")
                && map.contains_key("shape");
            if !is_statement {
                return;
            }
            let key = real(map["signature"]["key"].as_str().expect("a key"));
            if map["shape"] == ADVERTISEMENT_SHAPE {
                for entry in map["statement"]["keys"].as_array_mut().expect("keys") {
                    let own = real(entry["public_key"].as_str().expect("a key"));
                    entry["public_key"] = public_key(&own).as_str().into();
                }
            }
            sign_value(value, &key);
        }
        _ => {}
    }
}

/// The bytes of a statement as sent, signed anew.
fn signed(sent: &[u8]) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(sent).expect("json");
    resign(&mut value);
    serde_json::to_vec(&value).unwrap()
}

/// A provider whose statements carry real signatures.
struct Signing<P>(P);

impl<P: RoleProvider> RoleProvider for Signing<P> {
    fn advertisement(&self) -> Received<Advertisement> {
        Received::from_bytes(signed(self.0.advertisement().bytes())).expect("reads back")
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Received<ResultBody> {
        Received::from_bytes(signed(self.0.handle(operation).bytes())).expect("reads back")
    }

    fn state(&self) -> String {
        self.0.state()
    }

    fn status(&mut self, transaction: &str) -> Option<Received<ResultBody>> {
        self.0
            .status(transaction)
            .map(|r| Received::from_bytes(signed(r.bytes())).expect("reads back"))
    }
}

impl<P: RoleHarness> RoleHarness for Signing<P> {
    fn force_outcome_unknown(&mut self) -> bool {
        self.0.force_outcome_unknown()
    }

    fn force_failure(&mut self) -> bool {
        self.0.force_failure()
    }

    fn credit_reversal(&mut self, segment: &str, node: &str, asset: &str, amount: &str) -> bool {
        self.0.credit_reversal(segment, node, asset, amount)
    }
}

/// The catalogue as the suite's verifier.
struct CatalogueVerifier<'a> {
    catalogue: &'a Catalogue,
    at: Instant,
}

impl SignatureVerifier for CatalogueVerifier<'_> {
    fn verify(&self, _maker: &str, _signature: &Signature, received: &[u8]) -> Verification {
        match self.catalogue.verify_statement(received, self.at) {
            Ok(()) => Verification::Verified,
            Err(why) => Verification::Failed(format!("{why:?}")),
        }
    }
}

const AT: Instant = Instant(4_000_000_000);

/// A catalogue serving the suite's deployment that has accepted the signed advertisement of `provider`, under a
/// grant whose trusted key is the one the advertisement is signed with.
fn catalogue_holding(provider: &dyn RoleProvider, context: &SuiteContext) -> Catalogue {
    let received = provider.advertisement();
    let ad = received.statement().statement.clone();
    let mut categories = Vec::new();
    for (offered, category) in [
        (
            ad.offers.reserve.is_some(),
            rutent_prov::catalogue::Category::Reserve,
        ),
        (
            ad.offers.exchange.is_some(),
            rutent_prov::catalogue::Category::Exchange,
        ),
        (
            ad.offers.ramp.is_some(),
            rutent_prov::catalogue::Category::Ramp,
        ),
        (
            ad.offers.verifier.is_some(),
            rutent_prov::catalogue::Category::Verifier,
        ),
    ] {
        if offered {
            categories.push(category);
        }
    }
    let mut catalogue = Catalogue::new();
    catalogue.set_context(context.deployment.clone(), &context.protocol_version);
    catalogue
        .grant(
            Grant::new(GrantTerms {
                id: "g-1".into(),
                provider: ad.provider.clone(),
                party: "Acme Payments Ltd".into(),
                granted_by: "operator-admin".into(),
                granted_at: Instant(1_000),
                expires_at: Instant(5_000_000_000),
                categories,
                tuples: tuples_of(&ad),
                reversal_capacity: capacity(1_000, 500),
            })
            .expect("a grant"),
        )
        .unwrap();
    catalogue
        .trust_advertising_key("g-1", received.statement().signature.key.clone())
        .unwrap();
    catalogue
        .join(received.bytes(), AT)
        .expect("the stand-in's advertisement is accepted");
    catalogue
}

/// Makes a fresh provider.
type Maker = Box<dyn Fn() -> Box<dyn RoleHarness>>;

/// The stand-ins, each as a fresh provider.
fn stand_ins(context: &SuiteContext) -> Vec<(&'static str, Maker)> {
    let c = context.clone();
    let (a, b, d, e) = (c.clone(), c.clone(), c.clone(), c);
    vec![
        (
            "the reserve",
            Box::new(move || Box::new(Signing(Reserve::new(&a)))),
        ),
        (
            "the exchange",
            Box::new(move || Box::new(Signing(Exchange::new(&b)))),
        ),
        (
            "the ramp",
            Box::new(move || Box::new(Signing(Ramp::new(&d)))),
        ),
        (
            "the verifier",
            Box::new(move || Box::new(Signing(Verifier::new(&e)))),
        ),
    ]
}

// verifies: REQ-CAT-8, REQ-PRT-1, THR-4
#[test]
fn stand_in_of_each_category_passes_its_role_suite_with_signatures_verified() {
    let context = SuiteContext::default();
    for (name, provider) in stand_ins(&context) {
        let mut stand_in = provider();
        let catalogue = catalogue_holding(stand_in.as_ref(), &context);
        let verifier = CatalogueVerifier {
            catalogue: &catalogue,
            at: AT,
        };
        let report = run_role_suite(stand_in.as_mut(), &context, &verifier);
        assert!(
            report.passed(),
            "{name}: findings {:?}, uncovered {:?}, deferred {:?}",
            report.findings,
            report.uncovered,
            report.deferred
        );
        assert_eq!(
            report.signatures_deferred, 0,
            "{name}: no signature deferred"
        );
        assert!(report.deferred.is_empty(), "{name}");
    }
}

// verifies: REQ-PRT-1, THR-4
#[test]
fn stand_in_signing_with_placeholders_is_flagged_by_the_real_verifier() {
    let context = SuiteContext::default();
    let mut raw = Reserve::new(&context);
    let signed_copy = Signing(Reserve::new(&context));
    let catalogue = catalogue_holding(&signed_copy, &context);
    let verifier = CatalogueVerifier {
        catalogue: &catalogue,
        at: AT,
    };
    let report = run_role_suite(&mut raw, &context, &verifier);
    assert!(!report.passed());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::SignatureVerifies),
        "{:?}",
        report.findings
    );
}
