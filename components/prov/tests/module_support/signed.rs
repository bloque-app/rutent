//! Releases as a provider sends them, signed, and a platform admitting them against a catalogue that holds the
//! providers' advertisements (`T-013-04`). The catalogue's keys and advertisements are `tests/common`'s; the modules
//! are the module-boundary suite's stand-ins.

#![allow(dead_code)]

#[path = "../common/mod.rs"]
pub mod catalogue_support;

use catalogue_support::{
    Rng, advertisement_bytes, advertising_key, deployment, grant, join_made_at, public_key,
    sign_json, signing_key, statements_key,
};
use ed25519_dalek::SigningKey;
use rutent_contracts::module_interface::{
    Declaration, Digest, DigestAlgorithm, DigestValue, Parts,
};
use rutent_contracts::provider_role::{
    Advertisement, KeyPurpose, SignatureScheme, SigningKey as ListedKey, VERSION,
};
use rutent_prov::catalogue::{Catalogue, Instant, tuples_of};
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::{self, Admitted};
use rutent_prov::module::wire::sha256;

use crate::common::{TestWorld, build_standin, declaration, languages, paths, spec};

/// The reserve provider of provider-role's fixtures.
pub const RESERVE: &str = "polygon-wallet-reserve";

/// The ramp provider of provider-role's fixtures.
pub const RAMP: &str = "bridge-pay";

/// The caller's clock at every admission: 2026-11-07T00:00:00Z.
pub const AT: Instant = Instant(1_794_009_600);

/// When the releases are asserted.
pub const MADE_AT: &str = "2026-11-07T00:00:00Z";

/// When the providers' first advertisements were made, a day before.
const ADVERTISED_AT: Instant = Instant(AT.0 - 86_400);

/// A platform serving the tests' deployment and the catalogue it admits against, which holds the reserve's and
/// the ramp's advertisements, each under a grant and signed with the provider's advertising key.
pub struct Signed {
    /// The platform.
    pub platform: Platform<TestWorld>,
    /// The catalogue.
    pub catalogue: Catalogue,
}

impl Signed {
    pub fn new() -> Self {
        let mut catalogue = Catalogue::new();
        catalogue.add_public_node("polygon").expect("a public node");
        for (i, ad) in [reserve(), ramp()].iter().enumerate() {
            catalogue
                .grant(grant(
                    &format!("g-{i}"),
                    &ad.provider,
                    tuples_of(ad),
                    4_000_000_000,
                ))
                .expect("a grant");
            join_made_at(&mut catalogue, ad, ADVERTISED_AT, AT).expect("the advertisement joins");
        }
        let mut platform = Platform::new(
            paths(),
            TestWorld::default(),
            types::Envelope {
                deployment: deployment(),
                protocol_version: VERSION.to_owned(),
            },
        )
        .expect("a platform");
        platform.set_output_limit(4096);
        // The platform signs each result with the provider's statements key, which the world holds.
        for provider in [RESERVE, RAMP] {
            platform
                .world
                .hold_statement_key(provider, &statements_key(provider));
        }
        Self {
            platform,
            catalogue,
        }
    }

    /// Admits a release, its assertion given as received, at [`AT`].
    pub fn admit(&mut self, received: &[u8], binaries: &Parts<Vec<u8>>) -> Admitted {
        self.platform
            .admit(received, binaries, &mut self.catalogue, AT)
    }

    /// Gives the provider's waiting release with this assertion digest its fresh acceptance, judged again at
    /// [`AT`].
    pub fn accept(
        &mut self,
        provider: &str,
        assertion_digest: &Digest,
        by: &str,
    ) -> Option<rutent_contracts::module_interface::AdmissionArtefact> {
        self.platform
            .accept(provider, assertion_digest, by, &self.catalogue, AT)
    }

    /// The provider replaces its advertising key with [`rotated_key`]: a successor advertisement, signed with the
    /// key it retires, lists the new key for advertising and keeps its statements key.
    pub fn rotate_advertising_key(&mut self, provider: &str) {
        let ad = self
            .catalogue
            .accepted_advertisement(provider)
            .expect("an accepted advertisement")
            .statement
            .clone();
        // An advertisement lists the key it is signed with, so the rotation takes two: the old key adds the new
        // one, and the new one drops the old.
        let (old, new) = (advertising_key(provider), rotated_key(provider));
        let both = vec![
            listed(&old, KeyPurpose::Advertising),
            listed(&new, KeyPurpose::Advertising),
            listed(&statements_key(provider), KeyPurpose::Statements),
        ];
        let only_new = vec![
            listed(&new, KeyPurpose::Advertising),
            listed(&statements_key(provider), KeyPurpose::Statements),
        ];
        for (n, (keys, by)) in [(both, &old), (only_new, &new)].into_iter().enumerate() {
            let made_at = Instant(ADVERTISED_AT.0 + 1 + n as u64);
            let bytes = advertisement_bytes(&ad, Some(keys), made_at, by);
            self.catalogue
                .join(&bytes, AT)
                .expect("the successor joins");
        }
    }

    /// Whether the provider's module answers a withdrawal as `version`.
    pub fn answers_as(&mut self, provider: &str, version: &str, n: usize) -> bool {
        crate::common::answers_as(&mut self.platform, provider, version, n)
    }
}

impl Default for Signed {
    fn default() -> Self {
        Self::new()
    }
}

fn reserve() -> Advertisement {
    catalogue_support::reserve_advertisement()
}

fn ramp() -> Advertisement {
    catalogue_support::ramp_advertisement()
}

fn listed(key: &SigningKey, purpose: KeyPurpose) -> ListedKey {
    ListedKey {
        scheme: SignatureScheme::Ed25519,
        public_key: public_key(key),
        purpose,
    }
}

/// The advertising key a provider rotates to.
pub fn rotated_key(provider: &str) -> SigningKey {
    signing_key(provider, 3)
}

/// The language the stand-ins are built in: Rust, if the tests run it.
fn language() -> String {
    let languages = languages();
    if languages.iter().any(|l| l == "rs") {
        "rs".to_owned()
    } else {
        languages[0].clone()
    }
}

/// A release of `provider` at `version` handling `handles`: its declaration, whose statement key is the
/// provider's statements key, and the reserve stand-in's binaries built for it.
pub fn release(provider: &str, version: &str, handles: &[&str]) -> (Declaration, Parts<Vec<u8>>) {
    let mut value = serde_json::to_value(declaration(provider, version, handles)).expect("JSON");
    value["outside"]["statement_key"] = public_key(&statements_key(provider)).as_str().into();
    let declaration: Declaration = serde_json::from_value(value).expect("a valid declaration");
    let mut module = spec(provider, version, Vec::new(), true);
    module.declaration = declaration.clone();
    let binaries =
        build_standin(&module, &language(), "reserve", &[]).expect("the stand-in builds");
    (declaration, binaries)
}

/// The assertion of `binaries` under `declaration`, made by its provider at [`MADE_AT`] in the tests' deployment
/// and signed with `key`, as the bytes a provider sends.
pub fn assertion(
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
    key: &SigningKey,
) -> Vec<u8> {
    assertion_with(
        declaration,
        binaries,
        declaration.provider.as_str(),
        key,
        &deployment().name,
        MADE_AT,
    )
}

/// [`assertion`], made at `made_at`.
pub fn assertion_at(
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
    key: &SigningKey,
    made_at: &str,
) -> Vec<u8> {
    let maker = declaration.provider.as_str();
    assertion_with(
        declaration,
        binaries,
        maker,
        key,
        &deployment().name,
        made_at,
    )
}

/// [`assertion`], in the deployment named `name`.
pub fn assertion_in(
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
    key: &SigningKey,
    name: &str,
) -> Vec<u8> {
    let maker = declaration.provider.as_str();
    assertion_with(declaration, binaries, maker, key, name, MADE_AT)
}

/// [`assertion`], made by `maker`.
pub fn assertion_by(
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
    maker: &str,
    key: &SigningKey,
) -> Vec<u8> {
    assertion_with(
        declaration,
        binaries,
        maker,
        key,
        &deployment().name,
        MADE_AT,
    )
}

fn assertion_with(
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
    maker: &str,
    key: &SigningKey,
    deployment_name: &str,
    made_at: &str,
) -> Vec<u8> {
    let mut statement =
        serde_json::to_value(crate::common::assertion(declaration, binaries)).expect("JSON");
    let mut named = deployment();
    named.name = deployment_name.to_owned();
    statement["deployment"] = serde_json::to_value(named).expect("JSON");
    statement["protocol_version"] = VERSION.into();
    statement["maker"] = maker.into();
    statement["made_at"] = made_at.into();
    sign_json(statement, key)
}

/// The SHA-256 digest of some bytes, as the contract writes one.
pub fn digest_of(bytes: &[u8]) -> Digest {
    Digest {
        algorithm: DigestAlgorithm::Sha256,
        value: DigestValue::new(sha256(bytes)).expect("a digest"),
    }
}

/// Where `needle` first appears in `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A digest drawn from `rng`, in the contract's form.
pub fn random_digest(rng: &mut Rng) -> Digest {
    let hex: String = (0..64)
        .map(|_| char::from_digit(rng.below(16) as u32, 16).expect("a hex digit"))
        .collect();
    Digest {
        algorithm: DigestAlgorithm::Sha256,
        value: DigestValue::new(hex).expect("a digest"),
    }
}

/// Whether the platform's answer is an artefact.
pub fn is_artefact(admitted: &Admitted) -> bool {
    matches!(admitted, Admitted::Artefact(_))
}
