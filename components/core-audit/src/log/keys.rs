//! Whose keys sign statements, as the log reads them to attribute a statement ([`StatementKeys`], `REQ-AUD-2`,
//! `THR-3`, `THR-4`).
//!
//! The catalogue the state machine holds is state orchestration drives, so the log keeps its own register, fed only
//! by what it recorded itself and by the deployment's settings ([`Settings`]), never by orchestration:
//!
//! - **The operator** signs as [`Settings::operator`] with the deployment's `operator_key`, and states for
//!   [`Settings::operator_party`].
//! - **A provider's grant** is in force from a `change_catalogue` effect of `grant` for one of its tuples to one of
//!   `revoke_grant` for it, each at its effect's time. The log admits such an effect only on an unused permit of a
//!   permitting decision policy wrote (`admit_entry`), so orchestration can't grant a provider by itself.
//! - **A provider's advertisement** is accepted from the bytes the evidence store keeps for a stated entry of the
//!   advertisement's shape, once the log has recorded the entry, and only while the provider's grant is in force at
//!   the entry's `received_at`. It is held to provider-role's rules and the catalogue's, as `F-012` holds them:
//!   - it is this deployment's and protocol version's, of the advertisement's shape, and passes
//!     `validate_statement`, every key it lists a canonical point that isn't of small order;
//!   - it is dated no more than the skew ahead of its `received_at`;
//!   - **first trust is the operator's**: a provider's first advertisement is signed with an advertising key the
//!     deployment's settings trust for it, and every advertising key it lists is one ([`Settings::first_trust`]).
//!     Listing a key vouches for nothing;
//!   - every later one passes `validate_successor` against the one accepted last, and is newer than it;
//!   - its signature verifies, strictly;
//!   - it lists no `statements` key an accepted advertisement of the provider dropped, and no key with the other
//!     purpose than any accepted advertisement of any provider listed it for.
//!
//!   An advertisement that fails any of these changes nothing.
//! - **A key signs a provider's statements at `made_at`** where a grant of the provider is in force then, and both
//!   the advertisement accepted last and the one in force at `made_at` list it for `statements` (the catalogue's
//!   rule, so a retired key can't sign a statement backdated to before its retirement).
//! - **The party a provider states for** is the publisher its advertisement accepted last names, and the operator's
//!   party for the operator.
//!
//! A module the operator publishes (`REQ-CAT-23`) has no advertisement, and its release's `statement_key` reaches
//! the log in no entry (an `accept_module` effect names the version, not the key), so the log knows no key that
//! signs for it and records its statements unverified, attributed to no one: a residual for `T-025-05`, and an item
//! for a later `F-020` row of `core-internal` to name the key.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, StatementKeys};
use rutent_contracts::core_internal::{
    Action, CatalogueChange, Entry, SignedWith, StatedEntry, VerifiedEntry, What,
};
use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Advertisement, Deployment, KeyPurpose, PublicKey, Statement, canonicalize,
    validate_statement, validate_successor,
};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_prov::catalogue::{DEFAULT_CLOCK_SKEW_SECONDS, check_public_key, verify_envelope};

/// The deployment's settings the log reads its keys from, given when the log is opened: the deployment's, never
/// orchestration's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The log's name, which every receipt names.
    pub log: Id,
    /// This deployment, which every statement made for it names (`REQ-PRT-1`).
    pub deployment: Deployment,
    /// The provider-role protocol version an advertisement must name.
    pub protocol_version: String,
    /// The maker the operator signs its own statements as, Core's questions among them: a statement's `acts`.
    pub operator: Id,
    /// The operator's party, which the operator states for.
    pub operator_party: Id,
    /// The advertising keys the operator trusts to sign each provider's first advertisement (`REQ-CAT-2`): first
    /// trust, which the grant's effect names no key for.
    pub first_trust: Vec<(Id, PublicKey)>,
    /// How far ahead of its `received_at` an advertisement may be dated, in seconds.
    pub clock_skew_seconds: u64,
}

impl Settings {
    /// The settings of a log named `log` for `deployment`, whose operator signs as `operator` for
    /// `operator_party`, at provider-role's own protocol version and the catalogue's default skew, trusting no
    /// provider's advertising key yet.
    pub fn new(log: Id, deployment: Deployment, operator: Id, operator_party: Id) -> Self {
        Settings {
            log,
            deployment,
            protocol_version: rutent_contracts::provider_role::VERSION.to_owned(),
            operator,
            operator_party,
            first_trust: Vec::new(),
            clock_skew_seconds: DEFAULT_CLOCK_SKEW_SECONDS,
        }
    }

    /// These settings, trusting `key` to sign `provider`'s first advertisement.
    pub fn trusting(mut self, provider: Id, key: PublicKey) -> Self {
        self.first_trust.push((provider, key));
        self
    }

    fn trusts(&self, provider: &str, key: &PublicKey) -> bool {
        self.first_trust
            .iter()
            .any(|(p, k)| p.as_str() == provider && k == key)
    }
}

/// An advertisement the log accepted.
#[derive(Debug, Clone)]
struct Accepted {
    statement: Statement<Advertisement>,
    made_at: i128,
}

impl Accepted {
    fn lists(&self, key: &PublicKey, purpose: KeyPurpose) -> bool {
        self.statement
            .statement
            .keys
            .iter()
            .any(|k| &k.public_key == key && k.purpose == purpose)
    }
}

/// The register the log keeps from its own records.
#[derive(Debug, Clone, Default)]
pub(super) struct Register {
    /// Each provider's accepted advertisements, oldest first.
    ads: BTreeMap<String, Vec<Accepted>>,
    advertising_seen: Vec<PublicKey>,
    statements_seen: Vec<PublicKey>,
    /// Each provider's tuples, each with its grants and revocations in the log's order: the time and whether it
    /// granted.
    grants: BTreeMap<String, BTreeMap<String, Vec<(i128, bool)>>>,
}

fn nanos(text: &str) -> Option<i128> {
    Timestamp::new(text).map(|t| t.unix_nanos())
}

impl Register {
    /// Takes in an entry the log recorded: a grant's or a revocation's effect, or an advertisement, with the bytes
    /// the evidence store keeps for it.
    pub(super) fn record(&mut self, settings: &Settings, entry: &Entry, kept: Option<&[u8]>) {
        match entry {
            Entry::Verified(VerifiedEntry {
                what:
                    What::Effect {
                        action:
                            Action::ChangeCatalogue {
                                change:
                                    change @ (CatalogueChange::Grant | CatalogueChange::RevokeGrant),
                                provider,
                                tuple: Some(tuple),
                                ..
                            },
                        ..
                    },
                when,
                ..
            }) => {
                self.grants
                    .entry(provider.as_str().to_owned())
                    .or_default()
                    .entry(tuple.as_str().to_owned())
                    .or_default()
                    .push((when.at.unix_nanos(), *change == CatalogueChange::Grant));
            }
            Entry::Stated(StatedEntry {
                what, received_at, ..
            }) if what.statement.as_str() == ADVERTISEMENT_SHAPE => {
                if let Some(bytes) = kept {
                    // A refused advertisement changes nothing.
                    let _ = self.join(settings, bytes, received_at.unix_nanos());
                }
            }
            _ => {}
        }
    }

    /// Whether a grant of `provider` is in force at `at`: for one of its tuples, the last grant or revocation made
    /// at or before `at` granted it.
    fn in_force(&self, provider: &str, at: i128) -> bool {
        self.grants.get(provider).is_some_and(|tuples| {
            tuples.values().any(|changes| {
                changes
                    .iter()
                    .rfind(|(t, _)| *t <= at)
                    .is_some_and(|(_, granted)| *granted)
            })
        })
    }

    /// Accepts the advertisement `received`, recorded at `at`, or says nothing about why not: a refusal changes
    /// nothing.
    fn join(&mut self, settings: &Settings, received: &[u8], at: i128) -> Option<()> {
        let statement: Statement<Advertisement> = serde_json::from_slice(received).ok()?;
        canonicalize(received).ok()?;
        statement.check_shape().ok()?;
        statement
            .check_context(&settings.deployment, &settings.protocol_version)
            .ok()?;
        validate_statement(&statement).ok()?;
        let ad = &statement.statement;
        let made_at = nanos(&statement.made_at)?;
        let skew = i128::from(settings.clock_skew_seconds) * 1_000_000_000;
        if made_at > at.saturating_add(skew) || !self.in_force(&ad.provider, at) {
            return None;
        }
        for listed in &ad.keys {
            check_public_key(&listed.public_key.bytes()).ok()?;
        }
        let previous = self.ads.get(&ad.provider).and_then(|h| h.last());
        match previous {
            None => {
                let trusted = settings.trusts(&ad.provider, &statement.signature.key)
                    && ad.keys.iter().all(|k| {
                        k.purpose != KeyPurpose::Advertising
                            || settings.trusts(&ad.provider, &k.public_key)
                    });
                if !trusted {
                    return None;
                }
            }
            // `validate_successor` holds it newer than `previous` too.
            Some(previous) => validate_successor(&previous.statement, &statement).ok()?,
        }
        verify_envelope(
            &statement.signature.key,
            &statement.signature.value,
            received,
        )
        .ok()?;
        self.check_key_history(&statement)?;
        for listed in &statement.statement.keys {
            let seen = match listed.purpose {
                KeyPurpose::Advertising => &mut self.advertising_seen,
                KeyPurpose::Statements => &mut self.statements_seen,
            };
            if !seen.contains(&listed.public_key) {
                seen.push(listed.public_key.clone());
            }
        }
        let provider = statement.statement.provider.clone();
        self.ads
            .entry(provider)
            .or_default()
            .push(Accepted { statement, made_at });
        Some(())
    }

    /// The rules across history: a dropped statements key stays dropped, and a key's purpose holds everywhere.
    fn check_key_history(&self, statement: &Statement<Advertisement>) -> Option<()> {
        let ad = &statement.statement;
        let history = self.ads.get(&ad.provider).map(Vec::as_slice).unwrap_or(&[]);
        if let Some(last) = history.last() {
            let relisted = history
                .iter()
                .flat_map(|a| &a.statement.statement.keys)
                .filter(|old| old.purpose == KeyPurpose::Statements)
                .filter(|old| !last.lists(&old.public_key, KeyPurpose::Statements))
                .any(|old| ad.keys.iter().any(|k| k.public_key == old.public_key));
            if relisted {
                return None;
            }
        }
        let crossed = ad.keys.iter().any(|k| match k.purpose {
            KeyPurpose::Advertising => self.statements_seen.contains(&k.public_key),
            KeyPurpose::Statements => self.advertising_seen.contains(&k.public_key),
        });
        (!crossed).then_some(())
    }
}

/// The register read as [`StatementKeys`], with the settings.
pub(super) struct Keys<'a> {
    pub(super) settings: &'a Settings,
    pub(super) register: &'a Register,
}

impl StatementKeys for Keys<'_> {
    fn deployment(&mut self) -> Result<Deployment, AppendFailure> {
        Ok(self.settings.deployment.clone())
    }

    fn operator(&mut self) -> Result<Id, AppendFailure> {
        Ok(self.settings.operator.clone())
    }

    fn publisher(&mut self, maker: &Id) -> Result<Option<Id>, AppendFailure> {
        let settings = self.settings;
        if maker == &settings.operator {
            return Ok(Some(settings.operator_party.clone()));
        }
        Ok(self
            .register
            .ads
            .get(maker.as_str())
            .and_then(|h| h.last())
            .and_then(|a| Id::new(a.statement.statement.publisher.as_str())))
    }

    fn signs_for(
        &mut self,
        maker: &Id,
        key: &SignedWith,
        at: &Timestamp,
    ) -> Result<bool, AppendFailure> {
        let settings = self.settings;
        if maker == &settings.operator {
            return Ok(key.key == settings.deployment.operator_key);
        }
        let at = at.unix_nanos();
        let Some(history) = self.register.ads.get(maker.as_str()) else {
            return Ok(false);
        };
        let (Some(then), Some(accepted)) =
            (history.iter().rfind(|a| a.made_at <= at), history.last())
        else {
            return Ok(false);
        };
        // Ed25519 is the one scheme, so the key's bytes name it.
        Ok(self.register.in_force(maker.as_str(), at)
            && then.lists(&key.key, KeyPurpose::Statements)
            && accepted.lists(&key.key, KeyPurpose::Statements))
    }
}
