//! Whose keys sign statements, as the log reads them to attribute a statement ([`StatementKeys`], `REQ-AUD-2`,
//! `THR-3`, `THR-4`).
//!
//! The catalogue the state machine holds is state orchestration drives, so the log keeps its own register, fed only
//! by what it recorded itself and by the deployment's settings ([`Settings`]: this deployment, its operator and the
//! log's own clock), never by orchestration. A catalogue change counts only once the log holds a `done` result for
//! its effect, as `core-internal` reads it:
//!
//! - **The operator** signs as [`Settings::operator`] with the deployment's `operator_key`, and states for
//!   [`Settings::operator_party`].
//! - **A provider's grant** is in force from a `change_catalogue` effect of `grant` for one of its tuples to one of
//!   `revoke_grant` for it, each at its effect's time, once each is done. The log admits such an effect only on an
//!   unused permit of a permitting decision policy wrote (`admit_entry`), so orchestration can't grant a provider by
//!   itself; only where its time is within the skew of the log's own [`Clock`], which orchestration never writes
//!   (dated ahead of it, it is refused; behind it, as a retry after the skew is, unavailable); and, for a grant or a
//!   revocation, only where it is dated no earlier than its tuple's last change, so a tuple's changes run in the
//!   log's order and none is backdated behind another.
//! - **A provider's advertisement** is accepted from the bytes the evidence store keeps for a stated entry of the
//!   advertisement's shape, received within the skew of the log's clock, once the log has recorded the entry, and
//!   only while the provider's grant is in force at the entry's `received_at`. It is held to provider-role's rules
//!   and the catalogue's rules on keys and publishers, as `F-012` holds them:
//!   - it is this deployment's and protocol version's, of the advertisement's shape, and passes
//!     `validate_statement`, every key it lists a canonical point that isn't of small order;
//!   - it offers a way to follow an outcome, and each private node it declares is on record for no other
//!     publisher; and it names no module the operator publishes;
//!   - it is dated no more than the skew ahead of its `received_at`;
//!   - **first trust is the grant's**: a provider's first advertisement, or one after a grant made since its last
//!     accepted, is signed with an advertising key its latest grant in force names, and every advertising key it
//!     lists is one (`granted_advertising_keys`), and it is made after that grant's effect. Listing a key vouches
//!     for nothing, and no advertisement backdated before its grant rides on it;
//!   - every other one passes `validate_successor` against the one accepted last, and is newer than it;
//!   - it names the publisher the one accepted last named;
//!   - its signature verifies, strictly;
//!   - it lists no `statements` key an accepted advertisement of the provider dropped, and no key with the other
//!     purpose than any accepted advertisement of any provider listed it for.
//!
//!   An advertisement that fails any of these changes nothing.
//! - **A key signs a provider's statements at `made_at`** where a grant of the provider is in force then, and both
//!   the advertisement accepted last and the one in force at `made_at` list it for `statements` (the catalogue's
//!   rule, so a retired key can't sign a statement backdated to before its retirement).
//! - **The party a maker states for** is the publisher its advertisement accepted last names, for a provider, and the
//!   operator's party for the operator and for a module the operator publishes: one the log holds an accepted release
//!   of and no advertisement. `attribute` holds such a module to the statement keys of its accepted releases
//!   (`release_statement_keys`), not to this register.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{AppendFailure, StatementKeys};
use rutent_contracts::core_internal::{
    Action, CatalogueChange, Entry, History, Outcome, Receipt, SignedWith, StatedEntry,
    VerifiedEntry, Violation, What, granted_advertising_keys,
};
use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Advertisement, Deployment, KeyPurpose, PublicKey, Statement, canonicalize,
    validate_statement, validate_successor,
};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_prov::catalogue::{DEFAULT_CLOCK_SKEW_SECONDS, check_public_key, verify_envelope};

/// The log's own clock, which orchestration never writes: what it reads when it holds a catalogue change's time to
/// the time it is appended at.
pub trait Clock: std::fmt::Debug {
    /// Now, in nanoseconds since 1970-01-01T00:00:00Z.
    fn unix_nanos(&self) -> i128;
}

/// The system's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_nanos(&self) -> i128 {
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        i128::try_from(since.as_nanos()).unwrap_or(i128::MAX)
    }
}

/// The refusal of a catalogue change whose effect is dated more than the skew away from the log's clock.
pub const CHANGE_OFF_THE_LOGS_CLOCK: &str = "a catalogue change's effect is dated within the skew of the log's own clock when it is appended";

/// The refusal of an advertisement received at a time more than the skew ahead of the log's clock.
pub const ADVERTISEMENT_OFF_THE_LOGS_CLOCK: &str =
    "an advertisement is received within the skew of the log's own clock when it is appended";

/// The refusal of a grant or a revocation dated before the last change of its tuple.
pub const CHANGE_BEFORE_ITS_TUPLES_LAST: &str =
    "a tuple's grants and revocations are dated in the order the log records them";

/// The deployment's settings the log reads its keys and its clock from, given when the log is opened: the
/// deployment's, never orchestration's.
#[derive(Debug, Clone)]
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
    /// How far ahead of its `received_at` an advertisement may be dated, and how far from the log's clock a
    /// catalogue change's effect or an advertisement's `received_at`, in seconds.
    pub clock_skew_seconds: u64,
    /// The log's own clock.
    pub clock: Rc<dyn Clock>,
}

impl Settings {
    /// The settings of a log named `log` for `deployment`, whose operator signs as `operator` for
    /// `operator_party`, at provider-role's own protocol version and the catalogue's default skew, on the system's
    /// clock.
    pub fn new(log: Id, deployment: Deployment, operator: Id, operator_party: Id) -> Self {
        Settings {
            log,
            deployment,
            protocol_version: rutent_contracts::provider_role::VERSION.to_owned(),
            operator,
            operator_party,
            clock_skew_seconds: DEFAULT_CLOCK_SKEW_SECONDS,
            clock: Rc::new(SystemClock),
        }
    }

    /// These settings, on `clock`.
    pub fn on(mut self, clock: Rc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    fn skew(&self) -> i128 {
        i128::from(self.clock_skew_seconds) * 1_000_000_000
    }

    /// Whether `at` is within the skew of the log's clock: `Ok` where it is, the refusal `ahead` where it is dated
    /// past it, and unavailable where it lies behind it, as a retry after the skew does, which the log can't take
    /// now but which breaks no rule of the entry's.
    fn on_the_clock(&self, at: i128, ahead: &'static str) -> Result<(), AppendFailure> {
        let now = self.clock.unix_nanos();
        if at > now.saturating_add(self.skew()) {
            Err(AppendFailure::Refused(Violation(ahead)))
        } else if at < now.saturating_sub(self.skew()) {
            Err(AppendFailure::Unavailable)
        } else {
            Ok(())
        }
    }
}

/// An advertisement the log accepted.
#[derive(Debug, Clone)]
struct Accepted {
    statement: Statement<Advertisement>,
    made_at: i128,
    /// When the log received it.
    accepted_at: i128,
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
    /// Each provider's tuples, each with its grants and revocations done, in the log's order: the time and whether
    /// it granted.
    grants: BTreeMap<String, BTreeMap<String, Vec<(i128, bool)>>>,
    /// Each provider's tuples, each with the time of its last grant or revocation the log admitted, done or not.
    dated: BTreeMap<String, BTreeMap<String, i128>>,
    /// The modules the log holds an accepted release of, its `accept_module` done.
    released: BTreeSet<String>,
    /// The publisher each private node an accepted advertisement declares is on record for.
    nodes: BTreeMap<String, String>,
}

fn nanos(text: &str) -> Option<i128> {
    Timestamp::new(text).map(|t| t.unix_nanos())
}

/// The effect at `receipt` in `history`, where it holds one.
fn effect_at<'a>(history: &'a History, receipt: &Receipt) -> Option<&'a VerifiedEntry> {
    history.iter().find_map(|(r, e)| match e {
        Entry::Verified(
            v @ VerifiedEntry {
                what: What::Effect { .. },
                ..
            },
        ) if r == receipt => Some(v),
        _ => None,
    })
}

/// Whether `history` holds a `done` result of the effect at `receipt`.
fn done(history: &History, receipt: &Receipt) -> bool {
    history.iter().any(|(_, e)| {
        matches!(e, Entry::Verified(VerifiedEntry {
            what: What::Result { effect },
            outcome: Outcome::Done {},
            ..
        }) if effect == receipt)
    })
}

/// The time of the grant whose advertising keys `granted_advertising_keys` answers for `provider`: the last done
/// `grant` of the provider in `history` naming those keys.
fn granted_at(history: &History, provider: &str, keys: &[PublicKey]) -> Option<i128> {
    history
        .iter()
        .filter(|(receipt, _)| done(history, receipt))
        .filter_map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry {
                what:
                    What::Effect {
                        action:
                            Action::ChangeCatalogue {
                                change: CatalogueChange::Grant,
                                provider: p,
                                advertising_keys,
                                ..
                            },
                        ..
                    },
                when,
                ..
            }) if p.as_str() == provider && advertising_keys.as_slice() == keys => {
                Some(when.at.unix_nanos())
            }
            _ => None,
        })
        .next_back()
}

impl Register {
    /// Takes in an entry the log recorded, given the history before it: a grant's or a revocation's effect, the
    /// `done` result that carries a catalogue change out, or an advertisement, with the bytes the evidence store
    /// keeps for it.
    pub(super) fn record(
        &mut self,
        settings: &Settings,
        history: &History,
        entry: &Entry,
        kept: Option<&[u8]>,
    ) {
        match entry {
            Entry::Verified(VerifiedEntry {
                what:
                    What::Effect {
                        action:
                            Action::ChangeCatalogue {
                                change: CatalogueChange::Grant | CatalogueChange::RevokeGrant,
                                provider,
                                tuple: Some(tuple),
                                ..
                            },
                        ..
                    },
                when,
                ..
            }) => {
                let last = self
                    .dated
                    .entry(provider.as_str().to_owned())
                    .or_default()
                    .entry(tuple.as_str().to_owned())
                    .or_insert(i128::MIN);
                *last = (*last).max(when.at.unix_nanos());
            }
            Entry::Verified(VerifiedEntry {
                what: What::Result { effect },
                outcome: Outcome::Done {},
                ..
            }) => {
                let Some(VerifiedEntry {
                    what:
                        What::Effect {
                            action:
                                Action::ChangeCatalogue {
                                    change,
                                    provider,
                                    tuple,
                                    ..
                                },
                            ..
                        },
                    when,
                    ..
                }) = effect_at(history, effect)
                else {
                    return;
                };
                match (change, tuple) {
                    (CatalogueChange::Grant | CatalogueChange::RevokeGrant, Some(tuple)) => {
                        self.grants
                            .entry(provider.as_str().to_owned())
                            .or_default()
                            .entry(tuple.as_str().to_owned())
                            .or_default()
                            .push((when.at.unix_nanos(), *change == CatalogueChange::Grant));
                    }
                    (CatalogueChange::AcceptModule, _) => {
                        self.released.insert(provider.as_str().to_owned());
                    }
                    _ => {}
                }
            }
            Entry::Stated(StatedEntry {
                what, received_at, ..
            }) if what.statement.as_str() == ADVERTISEMENT_SHAPE => {
                if let Some(bytes) = kept {
                    // A refused advertisement changes nothing.
                    let _ = self.join(settings, history, bytes, received_at.unix_nanos());
                }
            }
            _ => {}
        }
    }

    /// Whether the register takes in `entry`, which the log is about to append: a catalogue change's effect, and an
    /// advertisement's receipt, only where it is dated within the skew of the log's own clock; and a grant's or a
    /// revocation's effect only where it is dated no earlier than its tuple's last change, so a tuple's changes run
    /// in the log's order and on a clock orchestration doesn't hold. Every other entry it takes as it is.
    pub(super) fn admits(&self, settings: &Settings, entry: &Entry) -> Result<(), AppendFailure> {
        match entry {
            Entry::Stated(StatedEntry {
                what, received_at, ..
            }) if what.statement.as_str() == ADVERTISEMENT_SHAPE => {
                settings.on_the_clock(received_at.unix_nanos(), ADVERTISEMENT_OFF_THE_LOGS_CLOCK)
            }
            Entry::Verified(VerifiedEntry {
                what:
                    What::Effect {
                        action:
                            Action::ChangeCatalogue {
                                change,
                                provider,
                                tuple,
                                ..
                            },
                        ..
                    },
                when,
                ..
            }) => {
                let at = when.at.unix_nanos();
                settings.on_the_clock(at, CHANGE_OFF_THE_LOGS_CLOCK)?;
                let last = tuple
                    .as_ref()
                    .filter(|_| {
                        matches!(
                            change,
                            CatalogueChange::Grant | CatalogueChange::RevokeGrant
                        )
                    })
                    .and_then(|t| self.dated.get(provider.as_str())?.get(t.as_str()));
                match last {
                    Some(t) if *t > at => Err(AppendFailure::Refused(Violation(
                        CHANGE_BEFORE_ITS_TUPLES_LAST,
                    ))),
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    /// Whether a grant of `provider` is in force at `at`: for one of its tuples, the last grant or revocation made
    /// at or before `at` granted it. [`Register::admits`] keeps each tuple's changes in time order.
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
    fn join(
        &mut self,
        settings: &Settings,
        history: &History,
        received: &[u8],
        at: i128,
    ) -> Option<()> {
        let statement: Statement<Advertisement> = serde_json::from_slice(received).ok()?;
        canonicalize(received).ok()?;
        statement.check_shape().ok()?;
        statement
            .check_context(&settings.deployment, &settings.protocol_version)
            .ok()?;
        validate_statement(&statement).ok()?;
        let ad = &statement.statement;
        let previous = self.ads.get(&ad.provider).and_then(|h| h.last());
        // The catalogue's rules on what an advertisement declares (`REQ-CAT-16`, `REQ-CAT-20`, `REQ-MDL-1`): a way to
        // follow an outcome, each private node its publisher's alone, and no module the operator publishes.
        if ad.declarations.tracking.is_empty()
            || (previous.is_none() && self.released.contains(&ad.provider))
            || ad
                .private_nodes
                .iter()
                .any(|n| self.nodes.get(n).is_some_and(|p| *p != ad.publisher))
        {
            return None;
        }
        let made_at = nanos(&statement.made_at)?;
        if made_at > at.saturating_add(settings.skew()) || !self.in_force(&ad.provider, at) {
            return None;
        }
        for listed in &ad.keys {
            check_public_key(&listed.public_key.bytes()).ok()?;
        }
        // First trust is the provider's latest grant in force, for an advertisement made after it: its first, or
        // one after a grant made since its last was accepted, which is how a provider recovers a lost key.
        let provider = Id::new(ad.provider.as_str())?;
        let granted = granted_advertising_keys(history, &provider);
        let by_grant = granted.contains(&statement.signature.key)
            && granted_at(history, &ad.provider, granted)
                .is_some_and(|t| made_at > t && previous.is_none_or(|p| t > p.accepted_at));
        if previous.is_none() || by_grant {
            let listed = ad
                .keys
                .iter()
                .all(|k| k.purpose != KeyPurpose::Advertising || granted.contains(&k.public_key));
            if !(by_grant && listed) {
                return None;
            }
        }
        if let Some(previous) = previous {
            // `validate_successor` holds it newer than `previous` too. A provider's publisher never changes
            // (the catalogue's `ProviderChangedPublisher`), so the party it states for doesn't either.
            if !by_grant {
                validate_successor(&previous.statement, &statement).ok()?;
            }
            if previous.statement.statement.publisher != ad.publisher || made_at <= previous.made_at
            {
                return None;
            }
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
        for node in &statement.statement.private_nodes {
            self.nodes
                .insert(node.clone(), statement.statement.publisher.clone());
        }
        let provider = statement.statement.provider.clone();
        self.ads.entry(provider).or_default().push(Accepted {
            statement,
            made_at,
            accepted_at: at,
        });
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
        let register = self.register;
        if let Some(accepted) = register.ads.get(maker.as_str()).and_then(|h| h.last()) {
            return Ok(Id::new(accepted.statement.statement.publisher.as_str()));
        }
        // A module the operator publishes: the log holds an accepted release of it, and no advertisement.
        Ok(register
            .released
            .contains(maker.as_str())
            .then(|| settings.operator_party.clone()))
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
