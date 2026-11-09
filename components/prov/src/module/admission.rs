//! Who vouches for a release (`T-013-04`; `REQ-MDL-1`, `THR-4`): reading an assertion from the bytes it arrived
//! in, the judge of a release assertion, and the catalogue as that judge.
//!
//! The platform admits a release only on the word of a [`ReleaseJudge`], and has none of its own: every
//! admission names one, and the trait is sealed (`T-010-10`), so the only judge there is, in production and in
//! tests, is the [`Catalogue`], which holds each provider's accepted
//! advertisements and the key history behind them (`T-012-02`). It judges an assertion, given as the bytes it
//! arrived in, by:
//!
//! - [`Catalogue::verify_statement`]: the deployment and protocol version are the catalogue's (`THR-8`); the
//!   signing key is listed for `advertising` by the provider's last accepted advertisement and by the one in force
//!   at the assertion's `made_at`, so a retired key can't sign a backdated release (`THR-4`); and the Ed25519
//!   signature verifies, strictly, over the assertion's signing input;
//! - [`validate_assertion_key`] against the advertisement the catalogue accepted for the releasing provider, never
//!   one that arrives with the release: the maker is the provider, the key isn't a statements key nor the
//!   operator's, and the declared `statement_key` is listed for `statements`.
//!
//! A release assertion carries no statement nested in it, so the context check `F-012`'s notes ask of admission
//! for every nested statement it accepts is the assertion's own: [`read`] makes it against the deployment the
//! platform serves, and the catalogue again against its own.
//!
//! Once a release is admitted, its `statement_key` is recorded as a statements key everywhere
//! ([`Catalogue::record_statement_key`]), so it is never accepted for advertising afterwards (`THR-4`).
//!
//! This is module admission and signature checking, a `DEC-20` path.

use rutent_contracts::module_interface::{
    AdmissionArtefact, ReleaseAssertion, RuleViolation, validate_assertion_key,
};
use rutent_contracts::provider_role::{
    CanonicalError, ContextMismatch, PublicKey, ShapeMismatch, Statement, canonicalize,
};

use crate::catalogue::{Catalogue, CatalogueError, Instant, StatementRefusal};
use crate::module::types::Envelope;

/// Why the platform refused a release on its assertion, before reading its binaries. The module-interface
/// contract's `AdmissionRefusal` has no reason for an assertion that doesn't verify, so the platform names its
/// own (F-013's notes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unverified {
    /// The bytes aren't a release assertion statement.
    Unreadable,
    /// The bytes have no canonical form, so no signing input (`REQ-PRT-1`).
    NoSigningInput(CanonicalError),
    /// The envelope names another shape than a release assertion's.
    WrongShape(ShapeMismatch),
    /// The assertion is another deployment's or protocol version's than the platform serves (`THR-8`).
    WrongContext(ContextMismatch),
    /// The signature, or the key it was made with, isn't accepted (`REQ-MDL-1`, `THR-4`).
    Signature(StatementRefusal),
    /// The judge holds no advertisement of the releasing provider.
    NoAdvertisement,
    /// The assertion's maker or keys break the contract's rules against the accepted advertisement.
    AssertionKey(Vec<RuleViolation>),
    /// The release, as the platform would record it, breaks the contract's rules on its own or against the
    /// release it follows: an invalid declaration, or an assertion signed with a statement key an earlier
    /// release declared (`THR-4`).
    Release(Vec<RuleViolation>),
    /// The declared statement key can't be recorded as one: it signs advertisements somewhere.
    StatementKey(CatalogueError),
}

/// Who may vouch for a release's assertion: the [`Catalogue`], and no one else. The trait is sealed, so a crate
/// can't write a judge that trusts every assertion (`T-010-10`; `REQ-MDL-1`, `THR-4`): the platform asks the
/// catalogue's verifier, which checks each assertion's signature over the contract's signing input, and tests
/// bring their own catalogue, with the keys and advertisements they need.
///
/// The platform asks it before it reads the binaries, and records the release's statement key with it once the
/// release is admitted.
///
/// ```compile_fail,E0277
/// use rutent_prov::catalogue::Instant;
/// use rutent_prov::module::admission::{ReleaseJudge, Unverified};
///
/// struct TrustsEveryAssertion;
///
/// impl ReleaseJudge for TrustsEveryAssertion {
///     fn judge(
///         &self,
///         _: &[u8],
///         _: &rutent_contracts::module_interface::AdmissionArtefact,
///         _: Instant,
///     ) -> Result<(), Unverified> {
///         Ok(())
///     }
///
///     fn record_statement_key(
///         &mut self,
///         _: &rutent_contracts::provider_role::PublicKey,
///     ) -> Result<(), Unverified> {
///         Ok(())
///     }
/// }
/// ```
pub trait ReleaseJudge: sealed::Sealed {
    /// Judges the assertion of `artefact`, which the platform read from `received`, the assertion's bytes as they
    /// arrived, at the caller's clock `at`. `artefact`'s build is not yet filled in.
    fn judge(
        &self,
        received: &[u8],
        artefact: &AdmissionArtefact,
        at: Instant,
    ) -> Result<(), Unverified>;

    /// Records the statement key an admitted release declared.
    fn record_statement_key(&mut self, key: &PublicKey) -> Result<(), Unverified>;
}

/// The seal: only the catalogue implements it, and it can't be named outside this module.
mod sealed {
    pub trait Sealed {}

    impl Sealed for crate::catalogue::Catalogue {}
}

impl ReleaseJudge for Catalogue {
    fn judge(
        &self,
        received: &[u8],
        artefact: &AdmissionArtefact,
        at: Instant,
    ) -> Result<(), Unverified> {
        self.verify_statement(received, at)
            .map_err(Unverified::Signature)?;
        let advertisement = self
            .accepted_advertisement(artefact.declaration().provider.as_str())
            .ok_or(Unverified::NoAdvertisement)?;
        validate_assertion_key(artefact, advertisement).map_err(Unverified::AssertionKey)
    }

    fn record_statement_key(&mut self, key: &PublicKey) -> Result<(), Unverified> {
        Catalogue::record_statement_key(self, key).map_err(Unverified::StatementKey)
    }
}

/// Reads an assertion from the bytes it arrived in: a release assertion statement, with a signing input, for the
/// deployment and protocol version the platform serves (`REQ-PRT-1`, `THR-8`). The platform admits what these bytes
/// say, and judges these same bytes.
pub fn read(
    received: &[u8],
    envelope: &Envelope,
) -> Result<Statement<ReleaseAssertion>, Unverified> {
    let assertion: Statement<ReleaseAssertion> =
        serde_json::from_slice(received).map_err(|_| Unverified::Unreadable)?;
    canonicalize(received).map_err(Unverified::NoSigningInput)?;
    assertion.check_shape().map_err(Unverified::WrongShape)?;
    assertion
        .check_context(&envelope.deployment, &envelope.protocol_version)
        .map_err(Unverified::WrongContext)?;
    Ok(assertion)
}
