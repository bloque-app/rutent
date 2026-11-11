//! The role suite with every signature verified (`T-010-10`): the stand-ins' statements are signed for real, over
//! the contract's signing input, and the suite's own Ed25519 verifier takes none on trust.

#![cfg(feature = "signing")]

use rutent_conformance::role::stand_in::{Exchange, Ramp, Reserve, Verifier};
use rutent_conformance::role::{
    Check, Received, RoleHarness, RoleProvider, SuiteContext, run_role_suite,
};
use rutent_conformance::signing::{Ed25519Verifier, Signing, another_key, sign_value};
use rutent_contracts::provider_role::{Advertisement, Operation, ResultBody, Statement};

type Maker = Box<dyn Fn() -> Box<dyn RoleHarness>>;

fn stand_ins(context: &SuiteContext) -> Vec<(&'static str, Maker)> {
    let (a, b, c, d) = (
        context.clone(),
        context.clone(),
        context.clone(),
        context.clone(),
    );
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
            Box::new(move || Box::new(Signing(Ramp::new(&c)))),
        ),
        (
            "the verifier",
            Box::new(move || Box::new(Signing(Verifier::new(&d)))),
        ),
    ]
}

// verifies: REQ-MDL-18, REQ-PRT-1
#[test]
fn role_suite_passes_with_every_statement_verified() {
    let context = SuiteContext::default();
    for (name, maker) in stand_ins(&context) {
        let report = run_role_suite(maker().as_mut(), &context, &Ed25519Verifier);
        assert!(
            report.passed(),
            "{name}: findings {:?}, uncovered {:?}, deferred {:?}",
            report.findings,
            report.uncovered,
            report.deferred
        );
        assert_eq!(report.signatures_deferred, 0, "{name}: nothing deferred");
        assert!(report.deferred.is_empty(), "{name}");
    }
}

/// A provider that signs every statement it sends with a key of no one's, naming that key as the signer.
struct SignsAsNoOne<P>(P);

fn as_no_one(sent: &[u8]) -> Received<ResultBody> {
    let mut value: serde_json::Value = serde_json::from_slice(sent).expect("json");
    sign_value(&mut value, &another_key());
    Received::from_bytes(serde_json::to_vec(&value).unwrap()).expect("reads back")
}

impl<P: RoleProvider> RoleProvider for SignsAsNoOne<P> {
    fn advertisement(&self) -> Received<Advertisement> {
        self.0.advertisement()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Received<ResultBody> {
        as_no_one(self.0.handle(operation).bytes())
    }

    fn state(&self) -> String {
        self.0.state()
    }
}

impl<P: RoleHarness> RoleHarness for SignsAsNoOne<P> {}

/// A provider whose results are altered after they were signed.
struct AltersAfterSigning<P>(P);

impl<P: RoleProvider> RoleProvider for AltersAfterSigning<P> {
    fn advertisement(&self) -> Received<Advertisement> {
        self.0.advertisement()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Received<ResultBody> {
        let sent = self.0.handle(operation);
        let mut value: serde_json::Value = serde_json::from_slice(sent.bytes()).expect("json");
        value["made_at"] = "2026-11-07T00:00:01Z".into();
        Received::from_bytes(serde_json::to_vec(&value).unwrap()).expect("reads back")
    }

    fn state(&self) -> String {
        self.0.state()
    }
}

impl<P: RoleHarness> RoleHarness for AltersAfterSigning<P> {}

// verifies: REQ-MDL-18, REQ-PRT-1
#[test]
fn role_suite_flags_a_statement_signed_by_another_key_or_altered_after_signing() {
    let context = SuiteContext::default();
    let signed_by_no_one = run_role_suite(
        &mut SignsAsNoOne(Signing(Reserve::new(&context))),
        &context,
        &Ed25519Verifier,
    );
    assert!(!signed_by_no_one.passed());
    assert!(
        signed_by_no_one
            .findings
            .iter()
            .any(|f| f.check == Check::MakerAndKey),
        "{:?}",
        signed_by_no_one.findings
    );
    let altered = run_role_suite(
        &mut AltersAfterSigning(Signing(Reserve::new(&context))),
        &context,
        &Ed25519Verifier,
    );
    assert!(!altered.passed());
    assert!(
        altered
            .findings
            .iter()
            .any(|f| f.check == Check::SignatureVerifies),
        "{:?}",
        altered.findings
    );
}

// verifies: REQ-PRT-1
#[test]
fn signature_with_unused_bits_set_in_its_last_character_is_not_a_signature() {
    use rutent_conformance::signing::{Rejected, advertisement, verify};
    let genuine = advertisement("some-provider", &SuiteContext::default());
    assert_eq!(verify(&genuine), Ok(()));
    let mut value: serde_json::Value = serde_json::from_slice(&genuine).unwrap();
    let text = value["signature"]["value"].as_str().unwrap().to_owned();
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let last = text.chars().last().unwrap();
    let index = alphabet.find(last).unwrap();
    // The last character carries 2 bits of the signature; flipping a bit below them changes no byte.
    let other = alphabet.chars().nth(index ^ 1).unwrap();
    value["signature"]["value"] = format!("{}{other}", &text[..text.len() - 1]).into();
    assert_eq!(
        verify(&serde_json::to_vec(&value).unwrap()),
        Err(Rejected::NoSignature)
    );
}
