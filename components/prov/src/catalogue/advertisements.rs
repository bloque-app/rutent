//! Advertisements and the key history the catalogue keeps (`T-012-02`; `REQ-CAT-2`, `REQ-PRT-1`, `THR-4`,
//! `THR-8`), as `contracts/provider-role/README.md` ("Key purposes, and what the catalogue must hold") gives it.
//!
//! An advertisement is joined from its bytes as received, and only when all of these hold. A refusal changes
//! nothing.
//!
//! - It reads as an advertisement statement for the catalogue's deployment and protocol version, has a signing
//!   input, and follows the contract's rules (`validate_statement`).
//! - Its `made_at` is no more than the catalogue's skew ahead of the caller's clock `at`: the same `at` as every
//!   other check, since the catalogue holds no clock of its own.
//! - Every key it lists is a canonical point of the curve that isn't of small order.
//! - **First trust** is the operator's: a provider's first advertisement is signed with a key a grant in force
//!   names ([`Catalogue::trust_advertising_key`]), and every advertising key it lists is named by one. Listing a
//!   key vouches for nothing. A provider that lost every advertising key is let in the same way, by a grant made
//!   after its last accepted advertisement.
//! - **Every later advertisement** passes `validate_successor` against the last one accepted, and the signature
//!   verifies ([`verify_envelope`]) against a key that one lists for advertising.
//! - It is strictly newer, by `made_at`, than the last accepted (no replay), and lists no `statements` key an
//!   accepted advertisement of the provider dropped.
//! - A key is never an advertising key where it was a statements key, nor a statements key where it is an
//!   advertising key, for any provider in any deployment: whoever resolved a statements key holds it.
//!
//! The history is fed only by accepted advertisements, so a refused one can't poison it.

use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Advertisement, CanonicalError, ContextMismatch, KeyPurpose, PublicKey,
    RuleViolation, ShapeMismatch, Statement, canonicalize, validate_statement, validate_successor,
};

use super::signature::{SignatureError, check_public_key, verify_envelope};
use super::time::Stamp;
use super::{Catalogue, Instant, Joined, Standing};

/// How far ahead of the catalogue's clock an advertisement may be dated, unless the operator says otherwise
/// (`T-012-02`'s reading: `SPEC.md` gives no figure).
pub const DEFAULT_CLOCK_SKEW_SECONDS: u64 = 300;

/// An advertisement as accepted, and when.
#[derive(Debug, Clone)]
pub(super) struct Accepted {
    statement: Statement<Advertisement>,
    made_at: Stamp,
    accepted_at: Instant,
}

/// Why an advertisement, or a statement signed by a provider's key, isn't accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinRefusal {
    /// It offers neither an identifier nor a status query to follow an outcome with.
    NoTracking,
    /// The advertisement breaks one of the contract's rules.
    Invalid(Vec<RuleViolation>),
    /// The provider has no grant in force: entry is by the operator's grant.
    NoGrantInForce,
    /// A publisher rule refused it.
    Publisher(super::PublisherError),
    /// A declaration rule refused it.
    Declaration(super::DeclarationError),
    /// It declares a private node the operator lists as public (`REQ-CAT-22`).
    PrivateNodeIsPublic(String),
    /// The catalogue was never told which deployment and protocol version it serves ([`Catalogue::set_context`]).
    NoContext,
    /// The bytes aren't a statement of this contract.
    Unreadable(String),
    /// The bytes have no canonical form, so no signing input (`REQ-PRT-1`).
    NoSigningInput(CanonicalError),
    /// The envelope names another shape than an advertisement's.
    WrongShape(ShapeMismatch),
    /// The statement is another deployment's or protocol version's (`REQ-PRT-1`, `THR-8`).
    WrongContext(ContextMismatch),
    /// `made_at` isn't an RFC 3339 UTC time in the contract's form.
    BadTime,
    /// `made_at` is more than the skew ahead of the clock the caller passed.
    DatedTooFarAhead {
        /// The latest `made_at`, in whole seconds, the clock allowed.
        latest_allowed: Instant,
    },
    /// A key it lists is not one to accept signatures from.
    ListedKeyRefused {
        /// The key.
        key: PublicKey,
        /// Why.
        why: SignatureError,
    },
    /// A first advertisement, or one after a fresh grant, isn't signed with a key a grant in force names.
    NotSignedWithAGrantedKey,
    /// A first advertisement lists an advertising key no grant in force names.
    AdvertisingKeyNotGranted(PublicKey),
    /// A later advertisement fails `validate_successor` against the last one accepted.
    NotASuccessor(Vec<RuleViolation>),
    /// The signature doesn't verify.
    BadSignature(SignatureError),
    /// It is no newer than the last advertisement accepted: a replay.
    NotNewer,
    /// It lists a `statements` key an accepted advertisement of the provider dropped.
    DroppedKeyRelisted(PublicKey),
    /// It lists as advertising a key that was listed for statements by some provider.
    AdvertisingKeyIsAStatementsKey(PublicKey),
    /// It lists as statements a key that is an advertising key of some provider.
    StatementsKeyIsAnAdvertisingKey(PublicKey),
}

/// Why a statement signed with a provider's key doesn't verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementRefusal {
    /// The bytes aren't a statement envelope, or its maker, time or key aren't readable.
    Unreadable(String),
    /// `made_at` isn't an RFC 3339 UTC time in the contract's form.
    BadTime,
    /// `made_at` is more than the skew ahead of the clock the caller passed.
    DatedTooFarAhead,
    /// The catalogue held no advertisement of the maker at that time.
    NoAdvertisementThen,
    /// The advertisement in force then doesn't list the key for what the statement is.
    KeyNotValidThen,
    /// The signature doesn't verify.
    BadSignature(SignatureError),
}

impl Catalogue {
    /// Tells the catalogue which deployment and protocol version it serves. A statement of another is refused
    /// (`REQ-PRT-1`, `THR-8`). Until this is called no advertisement is joined.
    pub fn set_context(
        &mut self,
        deployment: rutent_contracts::provider_role::Deployment,
        protocol_version: &str,
    ) {
        self.context = Some((deployment, protocol_version.to_owned()));
    }

    /// Sets how far ahead of the caller's clock an advertisement may be dated, in seconds
    /// ([`DEFAULT_CLOCK_SKEW_SECONDS`] until set).
    pub fn set_clock_skew(&mut self, seconds: u64) {
        self.skew_secs = Some(seconds);
    }

    /// The skew allowed, in seconds.
    pub fn clock_skew(&self) -> u64 {
        self.skew_secs.unwrap_or(DEFAULT_CLOCK_SKEW_SECONDS)
    }

    /// Names a key the operator trusts to sign a provider's advertisements, under one of that provider's grants
    /// (`REQ-CAT-2`). It is how a provider's first advertisement is trusted: never by listing its own key.
    pub fn trust_advertising_key(
        &mut self,
        grant_id: &str,
        key: PublicKey,
    ) -> Result<(), super::CatalogueError> {
        self.grant_mut(grant_id)?;
        if self.statements_seen.contains(&key) {
            return Err(super::CatalogueError::KeyIsAStatementsKey);
        }
        self.grant_mut(grant_id)?.trust_advertising_key(key);
        Ok(())
    }

    /// Records the `statement_key` an admitted release declared (`F-013`): from then on the key is a statements
    /// key everywhere, and is never accepted for advertising (`THR-4`).
    pub fn record_statement_key(&mut self, key: &PublicKey) -> Result<(), super::CatalogueError> {
        if self.advertising_seen.contains(key) {
            return Err(super::CatalogueError::KeyIsAnAdvertisingKey);
        }
        self.statements_seen.insert(key.clone());
        Ok(())
    }

    /// The advertisement last accepted for a provider.
    pub fn accepted_advertisement(&self, provider: &str) -> Option<&Statement<Advertisement>> {
        self.ads
            .get(provider)
            .and_then(|history| history.last())
            .map(|a| &a.statement)
    }

    /// Every advertisement accepted for a provider, oldest first: the key history.
    pub fn advertisement_history(&self, provider: &str) -> Vec<&Statement<Advertisement>> {
        self.ads
            .get(provider)
            .map(|history| history.iter().map(|a| &a.statement).collect())
            .unwrap_or_default()
    }

    /// Admits an advertisement, given as the bytes it arrived in, under the rules in this module's
    /// documentation, and those of `REQ-CAT-16` and `REQ-CAT-2`: it follows the contract's rules, offers a way to
    /// follow an outcome, comes from a provider with a grant in force, and its publisher and reserve declaration
    /// are recorded. `at` is the caller's clock, the one the skew check counts from. A refusal changes nothing.
    pub fn join(&mut self, received: &[u8], at: Instant) -> Result<(), JoinRefusal> {
        let (deployment, protocol_version) = self.context.clone().ok_or(JoinRefusal::NoContext)?;
        let statement: Statement<Advertisement> =
            serde_json::from_slice(received).map_err(|e| JoinRefusal::Unreadable(e.to_string()))?;
        canonicalize(received).map_err(JoinRefusal::NoSigningInput)?;
        statement.check_shape().map_err(JoinRefusal::WrongShape)?;
        statement
            .check_context(&deployment, &protocol_version)
            .map_err(JoinRefusal::WrongContext)?;
        let ad = &statement.statement;
        if ad.declarations.tracking.is_empty() {
            return Err(JoinRefusal::NoTracking);
        }
        validate_statement(&statement).map_err(JoinRefusal::Invalid)?;
        let made_at = Stamp::parse(&statement.made_at).ok_or(JoinRefusal::BadTime)?;
        let latest_allowed = at.0.saturating_add(self.clock_skew());
        if made_at.ceil_secs() > latest_allowed {
            return Err(JoinRefusal::DatedTooFarAhead {
                latest_allowed: Instant(latest_allowed),
            });
        }
        let in_force = self
            .grants_of_provider(&ad.provider)
            .iter()
            .any(|g| g.standing(at) == Standing::InForce);
        if !in_force {
            return Err(JoinRefusal::NoGrantInForce);
        }
        for entry in &ad.keys {
            check_public_key(&entry.public_key.bytes()).map_err(|why| {
                JoinRefusal::ListedKeyRefused {
                    key: entry.public_key.clone(),
                    why,
                }
            })?;
        }
        let previous = self.ads.get(&ad.provider).and_then(|h| h.last());
        self.check_authority(&statement, previous, at)?;
        verify_envelope(
            &statement.signature.key,
            &statement.signature.value,
            received,
        )
        .map_err(JoinRefusal::BadSignature)?;
        if previous.is_some_and(|p| made_at <= p.made_at) {
            return Err(JoinRefusal::NotNewer);
        }
        self.check_key_history(&statement)?;
        if let Some(node) = ad.private_nodes.iter().find(|n| self.nodes.is_public(n)) {
            return Err(JoinRefusal::PrivateNodeIsPublic(node.clone()));
        }
        let backup = (self.publishers.clone(), self.declarations.clone());
        let done = self
            .publishers
            .register(ad)
            .map_err(JoinRefusal::Publisher)
            .and_then(|()| {
                self.declarations
                    .record(ad, at)
                    .map(|_| ())
                    .map_err(JoinRefusal::Declaration)
            });
        if done.is_err() {
            (self.publishers, self.declarations) = backup;
            return done;
        }
        for entry in &ad.keys {
            match entry.purpose {
                KeyPurpose::Advertising => self.advertising_seen.insert(entry.public_key.clone()),
                KeyPurpose::Statements => self.statements_seen.insert(entry.public_key.clone()),
            };
        }
        self.joined.insert(
            ad.provider.clone(),
            Joined {
                advertisement: ad.clone(),
                made_at: Instant(made_at.secs),
            },
        );
        self.ads
            .entry(ad.provider.clone())
            .or_default()
            .push(Accepted {
                statement,
                made_at,
                accepted_at: at,
            });
        Ok(())
    }

    /// Who vouches for the signing key: the operator's grant, or the last accepted advertisement.
    fn check_authority(
        &self,
        statement: &Statement<Advertisement>,
        previous: Option<&Accepted>,
        at: Instant,
    ) -> Result<(), JoinRefusal> {
        let ad = &statement.statement;
        let granted = |key: &PublicKey, fresh_after: Option<Instant>| {
            self.grants_of_provider(&ad.provider).iter().any(|g| {
                g.standing(at) == Standing::InForce
                    && g.advertising_keys().contains(key)
                    && fresh_after.is_none_or(|t| g.granted_at() > t)
            })
        };
        // The first advertisement, or one after a grant made since the last accepted, whose key is granted.
        let by_grant = granted(&statement.signature.key, previous.map(|p| p.accepted_at));
        if previous.is_none() || by_grant {
            if !by_grant {
                return Err(JoinRefusal::NotSignedWithAGrantedKey);
            }
            let fresh_after = previous.map(|p| p.accepted_at);
            for entry in &ad.keys {
                if entry.purpose == KeyPurpose::Advertising
                    && !granted(&entry.public_key, fresh_after)
                {
                    return Err(JoinRefusal::AdvertisingKeyNotGranted(
                        entry.public_key.clone(),
                    ));
                }
            }
            return Ok(());
        }
        let previous = previous.expect("checked above");
        validate_successor(&previous.statement, statement).map_err(JoinRefusal::NotASuccessor)
    }

    /// The rules across history: a dropped statements key stays dropped, and a key's purpose holds everywhere.
    fn check_key_history(&self, statement: &Statement<Advertisement>) -> Result<(), JoinRefusal> {
        let ad = &statement.statement;
        let history = self.ads.get(&ad.provider).map(Vec::as_slice).unwrap_or(&[]);
        if let Some(last) = history.last() {
            let kept = |key: &PublicKey| {
                last.statement
                    .statement
                    .keys
                    .iter()
                    .any(|k| &k.public_key == key)
            };
            for old in history.iter().flat_map(|a| &a.statement.statement.keys) {
                if old.purpose == KeyPurpose::Statements
                    && !kept(&old.public_key)
                    && ad.keys.iter().any(|k| k.public_key == old.public_key)
                {
                    return Err(JoinRefusal::DroppedKeyRelisted(old.public_key.clone()));
                }
            }
        }
        for entry in &ad.keys {
            match entry.purpose {
                KeyPurpose::Advertising if self.statements_seen.contains(&entry.public_key) => {
                    return Err(JoinRefusal::AdvertisingKeyIsAStatementsKey(
                        entry.public_key.clone(),
                    ));
                }
                KeyPurpose::Statements if self.advertising_seen.contains(&entry.public_key) => {
                    return Err(JoinRefusal::StatementsKeyIsAnAdvertisingKey(
                        entry.public_key.clone(),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Verifies a statement a provider made, given as received, against the key that was valid at the statement's
    /// `made_at`: the advertisement in force then must list `signature.key`, for advertising if the statement is
    /// an advertisement or a release assertion, for statements otherwise. Then the signature must verify over the
    /// statement's signing input. `at` is the caller's clock, as everywhere: a statement dated more than the skew
    /// ahead of it is refused.
    ///
    /// A key retired later still verifies a statement dated before its retirement, and a leaked retired key can
    /// sign a statement dated in the past: callers that need more bind the statement to a time they know.
    pub fn verify_statement(&self, received: &[u8], at: Instant) -> Result<(), StatementRefusal> {
        let unreadable = |what: &str| StatementRefusal::Unreadable(what.to_owned());
        let text = |path: &[&str]| -> Result<String, StatementRefusal> {
            let bytes = rutent_contracts::provider_role::member(received, path)
                .map_err(|e| StatementRefusal::Unreadable(format!("{e:?}")))?
                .ok_or_else(|| unreadable(&path.join("/")))?;
            serde_json::from_slice::<String>(bytes).map_err(|_| unreadable(&path.join("/")))
        };
        let maker = text(&["maker"])?;
        let shape = text(&["shape"])?;
        let made_at = Stamp::parse(&text(&["made_at"])?).ok_or(StatementRefusal::BadTime)?;
        let value = text(&["signature", "value"])?;
        let key = PublicKey::new(text(&["signature", "key"])?)
            .ok_or_else(|| unreadable("signature/key"))?;
        if made_at.ceil_secs() > at.0.saturating_add(self.clock_skew()) {
            return Err(StatementRefusal::DatedTooFarAhead);
        }
        let in_force = self
            .ads
            .get(&maker)
            .and_then(|h| h.iter().rev().find(|a| a.made_at <= made_at))
            .ok_or(StatementRefusal::NoAdvertisementThen)?;
        let purpose = if shape == ADVERTISEMENT_SHAPE
            || shape == rutent_contracts::module_interface::RELEASE_ASSERTION_SHAPE
        {
            KeyPurpose::Advertising
        } else {
            KeyPurpose::Statements
        };
        let listed = in_force
            .statement
            .statement
            .keys
            .iter()
            .any(|k| k.public_key == key && k.purpose == purpose);
        if !listed {
            return Err(StatementRefusal::KeyNotValidThen);
        }
        verify_envelope(&key, &value, received).map_err(StatementRefusal::BadSignature)
    }
}
