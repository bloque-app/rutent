//! A module release's lifecycle actions, each through policy (`T-020-49`, `M2`'s item 6 and `F-025`'s L6 of `M3`'s
//! milestone review): `accept`, `withdraw`, `revoke`, `declare_exploited` and `mark_stale` (core-internal's
//! `LIFECYCLE_ACTIONS`) and the platform's `confirm` of a provider's releases after an unclean end (`CONFIRM_RELEASES`,
//! `T-013-11`) are carried out as [`Segments::promote`] and [`Segments::roll_back`] are: on a permitted decision's effect
//! alone, the effect recorded before the platform is asked and its result after.
//!
//! **Policy first.** Core asks the decision before it reads anything of the platform's releases (what waits, what is
//! held), so a refused caller is told it is not permitted whether or not anything is there, and its attempt is the
//! decision's entry (`F-025`'s L6, `REQ-AUZ-1`). Only a permitted caller learns that nothing is there to act on, or that
//! what is there isn't what it named.
//!
//! **A permit names what it acts on.** An acceptance names the waiting release's version and statement key, a withdrawal
//! its version, and both are held to the release that waits right before the platform is asked, so a permit to accept
//! one release accepts no other; a confirmation names the versions the platform holds the provider's releases at, and
//! confirms nothing where it holds others. Revoking, declaring a version exploited and marking one stale act on the
//! version named and no other, which the platform holds them to.
//!
//! **Taken unrecorded.** A decision that only restricts what runs (revoking, declaring exploited, marking stale) is taken
//! by the platform even where its own record can't hold it (`LifecycleRefusal::TakenUnrecorded`, `T-013-10`): its result
//! here is `done`, since it took effect, and the answer says it isn't durable on the platform, so the operator takes it
//! again once the platform reopens. Recording it `failed` would leave the log trusting a release declared exploited.
//!
//! The platform is reached through [`Releases`], apart from [`super::Modules`], whose carrying methods each take the
//! [`Effect`] they carry out, so `CMP-CORE-AUTHZ`'s scan holds each to a decision (`REQ-AUZ-1`); the functions here that
//! call them are visible in this crate alone (the scan's rule 4), so a caller outside `core-orch` reaches them only through
//! [`Segments::change_release`].

use std::collections::BTreeSet;

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, Log};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, CatalogueChange, DecisionRequest, Effect, Entry, Outcome, Permit,
    Receipt, VerifiedEntry, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::Id;
use rutent_prov::catalogue::{Catalogue, Instant};
use rutent_prov::module::lifecycle::{LifecycleRefusal, Mark};
use rutent_prov::module::platform::{Platform, World};

use super::{Segments, journal, key, now};
use crate::rounds::Fresh;
use crate::snapshot::Clock;

/// Core's log, as orchestration both appends to it and reads it: every entry it holds now, each with its receipt, in
/// log order. A confirmation reads the restrictions it takes again from the same log its effect is appended to, so no
/// caller can hand it another history, or none (`T-020-49`, L5 of #337's review); the settings' reopening reads it too.
pub trait Journal: Log {
    /// Every entry the log holds now, with its receipt, in log order.
    fn history(&self) -> Vec<(Receipt, Entry)>;
}

/// A release waiting for its fresh acceptance, as the platform holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    /// Its version.
    pub version: String,
    /// The digest of its assertion as received, which the platform accepts it by.
    pub digest: Digest,
    /// The key its module signs its statements with.
    pub statement_key: PublicKey,
}

/// The platform's releases, as the lifecycle actions reach them. Each carrying method takes the [`Effect`] it carries
/// out, whose entry the log admitted only on a permit.
pub trait Releases {
    /// The provider's release waiting for its fresh acceptance, if one waits.
    fn waiting(&self, provider: &Id) -> Option<Waiting>;

    /// The versions of the provider's releases the platform holds out of force until the operator confirms them, after
    /// a restart whose record didn't end with its clean close (`T-013-11`); `None` where nothing of it waits.
    fn held(&self, provider: &Id) -> Option<BTreeSet<String>>;

    /// Gives the waiting release under `digest` its fresh acceptance, by `by` at `at` (`Platform::accept`).
    ///
    /// # Errors
    ///
    /// `NoSuchVersion` where no release waits under `digest`, or the platform's judge no longer passes its assertion.
    fn accept(
        &mut self,
        effect: &Effect,
        provider: &Id,
        digest: &Digest,
        by: &Id,
        at: Instant,
    ) -> Result<(), LifecycleRefusal>;

    /// Withdraws the waiting release under `digest`, by `by` at `at` (`Platform::withdraw`).
    ///
    /// # Errors
    ///
    /// `NoSuchVersion` where no release waits under `digest`, or the platform's record couldn't hold the withdrawal.
    fn withdraw(
        &mut self,
        effect: &Effect,
        provider: &Id,
        digest: &Digest,
        by: &Id,
        at: Instant,
    ) -> Result<(), LifecycleRefusal>;

    /// Revokes the provider's `version` (`Platform::revoke`): the version put back in force, if any.
    ///
    /// # Errors
    ///
    /// As the platform refuses.
    fn revoke(
        &mut self,
        effect: &Effect,
        provider: &Id,
        version: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<Option<String>, LifecycleRefusal>;

    /// Declares the provider's `version` actively exploited (`Platform::declare_exploited`): the version put back in
    /// force, if any.
    ///
    /// # Errors
    ///
    /// As the platform refuses.
    fn declare_exploited(
        &mut self,
        effect: &Effect,
        provider: &Id,
        version: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<Option<String>, LifecycleRefusal>;

    /// Marks the provider's `version` stale (`Platform::mark_stale`): how many segments moved.
    ///
    /// # Errors
    ///
    /// As the platform refuses.
    fn mark_stale(
        &mut self,
        effect: &Effect,
        provider: &Id,
        version: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<usize, LifecycleRefusal>;

    /// The provider's staged version, if one is (`Lifecycle::staged`).
    fn staged(&self, provider: &Id) -> Option<String>;

    /// The mark the platform holds on the provider's `version` (stale, revoked, exploited), if any (`Lifecycle::mark`).
    fn mark(&self, provider: &Id, version: &Id) -> Option<Mark>;

    /// The provider's version in force, if one is (`Lifecycle::in_force`).
    fn in_force(&self, provider: &Id) -> Option<String>;

    /// Ends the provider's staging, the version in force kept (`Platform::roll_back`): the version whose staging ended.
    ///
    /// # Errors
    ///
    /// As the platform refuses.
    fn roll_back(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<String, LifecycleRefusal>;

    /// Confirms the provider's releases as the platform's record holds them (`Platform::confirm`).
    ///
    /// # Errors
    ///
    /// As the platform refuses.
    fn confirm(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<(), LifecycleRefusal>;
}

/// The platform, with the judge it holds a waiting release's assertion to again when it is accepted (the catalogue,
/// `ReleaseJudge`).
pub struct Judged<'a, W: World> {
    /// The platform.
    pub platform: &'a mut Platform<W>,
    /// The judge.
    pub judge: &'a Catalogue,
}

impl<W: World> Releases for Judged<'_, W> {
    fn waiting(&self, provider: &Id) -> Option<Waiting> {
        let artefact = self.platform.pending(provider.as_str())?;
        Some(Waiting {
            version: artefact.declaration().version.as_str().to_owned(),
            digest: artefact.assertion_digest.clone(),
            statement_key: artefact.declaration().outside.statement_key.clone(),
        })
    }

    fn held(&self, provider: &Id) -> Option<BTreeSet<String>> {
        self.platform.awaiting_confirmation(provider.as_str())
    }

    fn accept(
        &mut self,
        effect: &Effect,
        provider: &Id,
        digest: &Digest,
        by: &Id,
        at: Instant,
    ) -> Result<(), LifecycleRefusal> {
        let _permitted = effect;
        self.platform
            .accept(provider.as_str(), digest, by.as_str(), self.judge, at)
            .map(|_| ())
            .ok_or(LifecycleRefusal::NoSuchVersion)
    }

    fn withdraw(
        &mut self,
        effect: &Effect,
        provider: &Id,
        digest: &Digest,
        by: &Id,
        at: Instant,
    ) -> Result<(), LifecycleRefusal> {
        let _permitted = effect;
        self.platform
            .withdraw(provider.as_str(), digest, by.as_str(), at)
            .map(|_| ())
            .ok_or(LifecycleRefusal::NoSuchVersion)
    }

    fn revoke(
        &mut self,
        effect: &Effect,
        provider: &Id,
        version: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<Option<String>, LifecycleRefusal> {
        let _permitted = effect;
        self.platform
            .revoke(provider.as_str(), version.as_str(), by.as_str(), at)
    }

    fn declare_exploited(
        &mut self,
        effect: &Effect,
        provider: &Id,
        version: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<Option<String>, LifecycleRefusal> {
        let _permitted = effect;
        self.platform
            .declare_exploited(provider.as_str(), version.as_str(), by.as_str(), at)
    }

    fn mark_stale(
        &mut self,
        effect: &Effect,
        provider: &Id,
        version: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<usize, LifecycleRefusal> {
        let _permitted = effect;
        self.platform
            .mark_stale(provider.as_str(), version.as_str(), by.as_str(), at)
    }

    fn staged(&self, provider: &Id) -> Option<String> {
        self.platform
            .lifecycle()
            .staged(provider.as_str())
            .map(str::to_owned)
    }

    fn mark(&self, provider: &Id, version: &Id) -> Option<Mark> {
        self.platform
            .lifecycle()
            .mark(provider.as_str(), version.as_str())
    }

    fn in_force(&self, provider: &Id) -> Option<String> {
        self.platform
            .lifecycle()
            .in_force(provider.as_str())
            .map(str::to_owned)
    }

    fn roll_back(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<String, LifecycleRefusal> {
        let _permitted = effect;
        self.platform.roll_back(provider.as_str(), by.as_str(), at)
    }

    fn confirm(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<(), LifecycleRefusal> {
        let _permitted = effect;
        self.platform.confirm(provider.as_str(), by.as_str(), at)
    }
}

/// Which lifecycle action is asked, and what it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseChange<'a> {
    /// Accepting the release waiting at the provider, of this version and statement key (`accept_module`).
    Accept {
        /// The waiting release's version.
        version: &'a Id,
        /// The key its module signs its statements with.
        statement_key: &'a PublicKey,
    },
    /// Withdrawing the release waiting at the provider, of this version (`withdraw_release`).
    Withdraw {
        /// The waiting release's version.
        version: &'a Id,
    },
    /// Revoking this version (`revoke_release`, `REQ-MDL-20`).
    Revoke {
        /// The version.
        version: &'a Id,
    },
    /// Declaring this version actively exploited (`declare_exploited`, `REQ-MDL-20`).
    DeclareExploited {
        /// The version.
        version: &'a Id,
    },
    /// Marking this version stale, its segments moving to the version in force (`mark_stale`, `REQ-MDL-5`,
    /// `REQ-MDL-19`).
    MarkStale {
        /// The version.
        version: &'a Id,
    },
    /// Confirming the provider's releases held after an unclean end, at these versions (`confirm_releases`,
    /// `T-013-11`).
    Confirm {
        /// The versions the platform holds them at.
        versions: &'a [Id],
    },
}

impl ReleaseChange<'_> {
    /// The decided action, for `provider`.
    fn action(&self, provider: &Id) -> Action {
        let (change, version, statement_key, versions) = match *self {
            ReleaseChange::Accept {
                version,
                statement_key,
            } => (
                CatalogueChange::AcceptModule,
                Some(version.clone()),
                Some(statement_key.clone()),
                Vec::new(),
            ),
            ReleaseChange::Withdraw { version } => (
                CatalogueChange::WithdrawRelease,
                Some(version.clone()),
                None,
                Vec::new(),
            ),
            ReleaseChange::Revoke { version } => (
                CatalogueChange::RevokeRelease,
                Some(version.clone()),
                None,
                Vec::new(),
            ),
            ReleaseChange::DeclareExploited { version } => (
                CatalogueChange::DeclareExploited,
                Some(version.clone()),
                None,
                Vec::new(),
            ),
            ReleaseChange::MarkStale { version } => (
                CatalogueChange::MarkStale,
                Some(version.clone()),
                None,
                Vec::new(),
            ),
            ReleaseChange::Confirm { versions } => (
                CatalogueChange::ConfirmReleases,
                None,
                None,
                versions.to_vec(),
            ),
        };
        Action::ChangeCatalogue {
            change,
            provider: provider.clone(),
            version,
            tuple: None,
            class: None,
            advertising_keys: Vec::new(),
            statement_key,
            setting: None,
            versions,
        }
    }
}

/// A lifecycle action to carry out, asked by `caller`.
#[derive(Debug, Clone, Copy)]
pub struct ReleaseAsked<'a> {
    /// The provider.
    pub provider: &'a Id,
    /// The action and what it names.
    pub change: ReleaseChange<'a>,
    /// Who asks: the operator's login, which policy decides and the platform records as `by`.
    pub caller: &'a Actor,
    /// Core, which writes the effect and its result.
    pub core: &'a Actor,
}

/// What a lifecycle action came to on the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// The waiting release was accepted, and runs.
    Accepted,
    /// The waiting release was withdrawn.
    Withdrawn,
    /// The version was revoked; the version put back in force, if any.
    Revoked(Option<String>),
    /// The version was declared exploited; the version put back in force, if any.
    DeclaredExploited(Option<String>),
    /// The version was marked stale; how many segments moved.
    MarkedStale(usize),
    /// The provider's releases were confirmed.
    Confirmed,
    /// The platform took a decision that only restricts what runs but couldn't write it to its record
    /// (`LifecycleRefusal::TakenUnrecorded`): it is in force now, its result `done`, and a reopened platform doesn't know
    /// it, so the operator takes it again then (`T-013-10`).
    TakenUnrecorded,
}

/// Why a lifecycle action wasn't carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseRefused {
    /// Policy didn't permit it: no effect was recorded, and nothing of the platform's releases was read. The answer is
    /// the same whatever the platform holds (`F-025`'s L6).
    NotPermitted(Answer),
    /// Permitted, but there is nothing to act on: no release waits at the provider, or nothing of it is held for
    /// confirmation. No effect was recorded.
    NothingToDo,
    /// Permitted, but what the platform holds isn't what the permit names: the waiting release is of another version or
    /// statement key, or the provider's releases are held at other versions. No effect was recorded, and a permit to
    /// act on one release acts on no other.
    NotWhatItNames {
        /// What the platform holds, by version: the waiting release's, or the held versions.
        holds: Vec<String>,
    },
    /// The log refused or failed an append: before the effect's receipt nothing changed.
    Append(AppendFailure),
    /// The platform refused: the effect's result is `failed`, and nothing changed.
    Lifecycle(LifecycleRefusal),
    /// Permitted, but a restriction Core's log holds done for the provider couldn't be taken again on the platform
    /// before its releases came back into force: nothing is confirmed and no effect recorded (it fails closed).
    Unrestored {
        /// The restriction: its change and the version it names.
        change: CatalogueChange,
        /// The version.
        version: String,
        /// Why the platform refused it.
        refusal: LifecycleRefusal,
    },
}

impl From<AppendFailure> for ReleaseRefused {
    fn from(failure: AppendFailure) -> Self {
        ReleaseRefused::Append(failure)
    }
}

/// What the permitted change acts on, read once permitted: the waiting release's digest for an acceptance or a
/// withdrawal, nothing for the others. A release, or a held set, other than the permit names is refused.
fn target(
    releases: &dyn Releases,
    provider: &Id,
    change: &ReleaseChange<'_>,
) -> Result<Option<Digest>, ReleaseRefused> {
    match *change {
        ReleaseChange::Accept {
            version,
            statement_key,
        } => {
            let waiting = releases
                .waiting(provider)
                .ok_or(ReleaseRefused::NothingToDo)?;
            if waiting.version != version.as_str() || &waiting.statement_key != statement_key {
                return Err(ReleaseRefused::NotWhatItNames {
                    holds: vec![waiting.version],
                });
            }
            Ok(Some(waiting.digest))
        }
        ReleaseChange::Withdraw { version } => {
            let waiting = releases
                .waiting(provider)
                .ok_or(ReleaseRefused::NothingToDo)?;
            if waiting.version != version.as_str() {
                return Err(ReleaseRefused::NotWhatItNames {
                    holds: vec![waiting.version],
                });
            }
            Ok(Some(waiting.digest))
        }
        ReleaseChange::Confirm { versions } => {
            let held = releases.held(provider).ok_or(ReleaseRefused::NothingToDo)?;
            let named: BTreeSet<String> = versions.iter().map(|v| v.as_str().to_owned()).collect();
            if named != held {
                return Err(ReleaseRefused::NotWhatItNames {
                    holds: held.into_iter().collect(),
                });
            }
            Ok(None)
        }
        ReleaseChange::Revoke { .. }
        | ReleaseChange::DeclareExploited { .. }
        | ReleaseChange::MarkStale { .. } => Ok(None),
    }
}

/// The lifecycle changes that only restrict what runs or what may come into force: each is taken again from Core's log
/// before a provider's releases are confirmed back into force after the platform's unclean end (`T-020-49`).
const RESTRICTIVE: [CatalogueChange; 5] = [
    CatalogueChange::RevokeRelease,
    CatalogueChange::WithdrawRelease,
    CatalogueChange::DeclareExploited,
    CatalogueChange::MarkStale,
    CatalogueChange::RollBack,
];

/// Each restrictive lifecycle effect `history` holds for `provider` whose result isn't `failed`, in log order, as the log
/// holds it (its action, permit and receipt), with who its decision's request names as its caller (where the log holds
/// that decision). An effect with no result is taken: it was permitted and recorded to take effect, and whether the
/// platform took it before its result could be appended, or before Core and the platform stopped together, is not known,
/// so it fails closed (M1 of #337's review). No platform action puts a version back in force (`reinstate` has none), so
/// nothing here undoes a restriction (L3).
fn restrictions(history: &[(Receipt, Entry)], provider: &Id) -> Vec<(Effect, Option<Id>)> {
    let failed = |receipt: &Receipt| {
        history.iter().any(|(_, e)| {
            matches!(e, Entry::Verified(VerifiedEntry {
                what: What::Result { effect },
                outcome: Outcome::Failed {},
                ..
            }) if effect == receipt)
        })
    };
    let caller_of = |decision: &Receipt| {
        history.iter().find_map(|(r, e)| match e {
            Entry::Verified(VerifiedEntry {
                what: What::Decide { request, .. },
                ..
            }) if r == decision => Some(request.caller.acts.clone()),
            _ => None,
        })
    };
    let mut found: Vec<(Effect, Option<Id>)> = Vec::new();
    for (receipt, entry) in history {
        let Entry::Verified(VerifiedEntry {
            what: What::Effect { action, decision },
            ..
        }) = entry
        else {
            continue;
        };
        let Action::ChangeCatalogue {
            change,
            provider: p,
            ..
        } = action
        else {
            continue;
        };
        if p != provider || failed(receipt) {
            continue;
        }
        if RESTRICTIVE.contains(change) {
            found.push((
                logged_effect(action, decision, receipt),
                caller_of(decision.receipt()),
            ));
        }
    }
    found
}

/// The effect the log holds at `receipt`: `action`, on the permit its entry names, which the log admitted only on that
/// permit's decision. Nothing new is decided or recorded.
fn logged_effect(action: &Action, decision: &Permit, receipt: &Receipt) -> Effect {
    Effect {
        action: action.clone(),
        decision: decision.clone(),
        receipt: receipt.clone(),
    }
}

/// What the platform's standing says of the restriction `effect`, whose result Core's log doesn't hold: `done` where it
/// holds now, `failed` where it can't have taken effect, `None` where the platform's standing can't tell (it fails
/// closed: the effect stays without a result and the confirmation takes it again).
fn standing_of(releases: &dyn Releases, effect: &Effect) -> Option<Outcome> {
    let Action::ChangeCatalogue {
        change,
        provider,
        version: Some(version),
        ..
    } = &effect.action
    else {
        return None;
    };
    let mark = releases.mark(provider, version);
    match change {
        CatalogueChange::DeclareExploited => {
            matches!(mark, Some(Mark::Exploited)).then_some(Outcome::Done {})
        }
        CatalogueChange::RevokeRelease => {
            matches!(mark, Some(Mark::Revoked | Mark::Exploited)).then_some(Outcome::Done {})
        }
        CatalogueChange::MarkStale => match (mark, releases.in_force(provider)) {
            // Marked already, however: marking it stale holds nothing more.
            (Some(_), _) => Some(Outcome::Done {}),
            // Nothing newer in force to move its segments to: the platform refuses a stale mark there, so it never
            // took effect.
            (None, None) => Some(Outcome::Failed {}),
            (None, Some(in_force)) if in_force == version.as_str() => Some(Outcome::Failed {}),
            (None, Some(_)) => None,
        },
        // The staging, or the waiting release, it ends is gone: no such release runs or waits.
        CatalogueChange::RollBack => (releases.staged(provider).as_deref()
            != Some(version.as_str()))
        .then_some(Outcome::Done {}),
        CatalogueChange::WithdrawRelease => releases
            .waiting(provider)
            .is_none_or(|w| w.version != version.as_str())
            .then_some(Outcome::Done {}),
        _ => None,
    }
}

/// Appends, by `core`, the result the platform's standing gives each restrictive lifecycle effect Core's log holds with
/// none, of `provider` or of every provider ([`standing_of`]). How many it appended.
fn complete_restrictions(
    log: &mut dyn Journal,
    releases: &dyn Releases,
    provider: Option<&Id>,
    clock: &dyn Clock,
    core: &Actor,
) -> Result<usize, AppendFailure> {
    let history = log.history();
    let resulted: Vec<&Receipt> = history
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry {
                what: What::Result { effect },
                ..
            }) => Some(effect),
            _ => None,
        })
        .collect();
    let mut completed = Vec::new();
    for (receipt, entry) in &history {
        let Entry::Verified(VerifiedEntry {
            what: What::Effect { action, decision },
            ..
        }) = entry
        else {
            continue;
        };
        let Action::ChangeCatalogue {
            change,
            provider: p,
            ..
        } = action
        else {
            continue;
        };
        if !RESTRICTIVE.contains(change)
            || resulted.contains(&receipt)
            || provider.is_some_and(|provider| provider != p)
        {
            continue;
        }
        if let Some(outcome) = standing_of(releases, &logged_effect(action, decision, receipt)) {
            completed.push((receipt.clone(), outcome));
        }
    }
    for (effect, outcome) in &completed {
        journal::append(
            log,
            journal::verified(
                core,
                What::Result {
                    effect: effect.clone(),
                },
                &now(clock)?,
                *outcome,
            ),
        )?;
    }
    Ok(completed.len())
}

/// Takes the restriction `effect`, one Core's log holds done, again on the platform, by `by`, as a reopened platform
/// that lost it must hold it before its provider's releases come back into force. One the platform holds already (the
/// version marked, nothing of it known, its staging or its waiting release gone) needs nothing; any other refusal is the
/// answer, and nothing is confirmed. Visible in this crate alone: the confirmation's decided path is its one caller.
pub(crate) fn retake_restriction(
    releases: &mut dyn Releases,
    effect: &Effect,
    by: &Id,
    clock: &dyn Clock,
) -> Result<(), ReleaseRefused> {
    let Action::ChangeCatalogue {
        change,
        provider,
        version: Some(version),
        ..
    } = &effect.action
    else {
        return Ok(());
    };
    let at = clock.now().floor();
    let held = |refusal: LifecycleRefusal| match refusal {
        LifecycleRefusal::AlreadyMarked
        | LifecycleRefusal::NoSuchVersion
        | LifecycleRefusal::TakenUnrecorded => Ok(()),
        refusal => Err(ReleaseRefused::Unrestored {
            change: *change,
            version: version.as_str().to_owned(),
            refusal,
        }),
    };
    match change {
        CatalogueChange::RevokeRelease => releases
            .revoke(effect, provider, version, by, at)
            .map(|_| ())
            .or_else(held),
        CatalogueChange::DeclareExploited => releases
            .declare_exploited(effect, provider, version, by, at)
            .map(|_| ())
            .or_else(held),
        CatalogueChange::MarkStale => releases
            .mark_stale(effect, provider, version, by, at)
            .map(|_| ())
            .or_else(held),
        // A staging, or a waiting release, ended is taken again only where that version is staged, or waits, again:
        // a later staging or release of another version is no part of it.
        CatalogueChange::RollBack => match releases.staged(provider) {
            Some(staged) if staged == version.as_str() => releases
                .roll_back(effect, provider, by, at)
                .map(|_| ())
                .or_else(held),
            _ => Ok(()),
        },
        CatalogueChange::WithdrawRelease => match releases.waiting(provider) {
            Some(waiting) if waiting.version == version.as_str() => releases
                .withdraw(effect, provider, &waiting.digest, by, at)
                .or_else(held),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

/// Carries out the lifecycle action `effect` permits, by `by`, on what it names: only where its action is the change
/// `asked` is and names the same release, held to what the platform holds right before it is asked. Anything else asks
/// nothing of the platform. Visible in this crate alone: the decided path is its one caller.
pub(crate) fn carry_out_release_change(
    releases: &mut dyn Releases,
    effect: &Effect,
    asked: &ReleaseAsked<'_>,
    by: &Id,
    clock: &dyn Clock,
) -> Result<Changed, ReleaseRefused> {
    if effect.action != asked.change.action(asked.provider) {
        return Err(ReleaseRefused::NotWhatItNames { holds: Vec::new() });
    }
    // Held again right before the platform is asked. A confirmation's held versions may have narrowed since it was
    // permitted, by the restrictions taken again first, never widened.
    let digest = match asked.change {
        ReleaseChange::Confirm { versions } => {
            let held = releases
                .held(asked.provider)
                .ok_or(ReleaseRefused::NothingToDo)?;
            if !held
                .iter()
                .all(|h| versions.iter().any(|v| v.as_str() == h))
            {
                return Err(ReleaseRefused::NotWhatItNames {
                    holds: held.into_iter().collect(),
                });
            }
            None
        }
        _ => target(releases, asked.provider, &asked.change)?,
    };
    let at = clock.now().floor();
    let provider = asked.provider;
    let taken = |refusal: LifecycleRefusal| match refusal {
        LifecycleRefusal::TakenUnrecorded => Ok(Changed::TakenUnrecorded),
        other => Err(ReleaseRefused::Lifecycle(other)),
    };
    match (asked.change, digest) {
        (ReleaseChange::Accept { .. }, Some(digest)) => releases
            .accept(effect, provider, &digest, by, at)
            .map(|()| Changed::Accepted)
            .map_err(ReleaseRefused::Lifecycle),
        (ReleaseChange::Withdraw { .. }, Some(digest)) => releases
            .withdraw(effect, provider, &digest, by, at)
            .map(|()| Changed::Withdrawn)
            .map_err(ReleaseRefused::Lifecycle),
        (ReleaseChange::Revoke { version }, _) => releases
            .revoke(effect, provider, version, by, at)
            .map(Changed::Revoked)
            .or_else(taken),
        (ReleaseChange::DeclareExploited { version }, _) => releases
            .declare_exploited(effect, provider, version, by, at)
            .map(Changed::DeclaredExploited)
            .or_else(taken),
        (ReleaseChange::MarkStale { version }, _) => releases
            .mark_stale(effect, provider, version, by, at)
            .map(Changed::MarkedStale)
            .or_else(taken),
        (ReleaseChange::Confirm { .. }, _) => releases
            .confirm(effect, provider, by, at)
            .map(|()| Changed::Confirmed)
            .map_err(ReleaseRefused::Lifecycle),
        (ReleaseChange::Accept { .. } | ReleaseChange::Withdraw { .. }, None) => {
            Err(ReleaseRefused::NothingToDo)
        }
    }
}

impl Segments {
    /// Completes, as a Core that reopens does, the result of each restrictive lifecycle effect Core's log holds with
    /// none (`T-020-65`, the milestone reviewer's check of `T-020-49`), from what the platform holds now, as
    /// `Settings::reopen` completes a setting's: `done` where the restriction holds, `failed` where the platform's
    /// standing means it never took effect (a version marked stale with nothing newer in force to move to), and nothing
    /// where it can't tell (fail closed: the confirmation takes it again). It reads the platform and carries nothing out.
    /// How many results it appended.
    ///
    /// # Errors
    ///
    /// The log's, appending a result.
    pub fn reopen_lifecycle(
        log: &mut dyn Journal,
        releases: &dyn Releases,
        clock: &dyn Clock,
        core: &Actor,
    ) -> Result<usize, AppendFailure> {
        complete_restrictions(log, releases, None, clock, core)
    }

    /// Carries out one of a release's lifecycle actions, as policy decides, by the decision's caller (`REQ-AUZ-6`,
    /// `REQ-MDL-5`, `REQ-MDL-20`, `T-020-49`): Core asks the decision first, before it reads anything of the platform's
    /// releases, so a refused caller is told the same whatever is there and its attempt is recorded; then, permitted,
    /// it holds the action to what the platform holds, records the effect before it calls the platform, and records its
    /// result after, `done` where the platform took it (unrecorded on its side included) and `failed` where it
    /// refused. An effect is marked acted on only once the platform took it.
    pub fn change_release(
        &mut self,
        log: &mut dyn Journal,
        decisions: &mut dyn Decisions,
        releases: &mut dyn Releases,
        clock: &dyn Clock,
        fresh: &mut dyn Fresh,
        asked: &ReleaseAsked<'_>,
    ) -> Result<Changed, ReleaseRefused> {
        let action = asked.change.action(asked.provider);
        let request = DecisionRequest {
            request: fresh.id(),
            caller: asked.caller.clone(),
            action: action.clone(),
            at: now(clock)?,
        };
        let decision = decisions.decide(&request)?;
        let Some(permit) = decision.permit() else {
            return Err(ReleaseRefused::NotPermitted(decision.outcome));
        };
        // Read only once permitted: what the platform holds is the permitted caller's to learn.
        target(releases, asked.provider, &asked.change)?;
        // A confirmation brings a provider's releases back into force after the platform's unclean end, whose record may
        // have lost a restriction it took but couldn't write: each restriction Core's log holds done for the provider is
        // taken again first, so it holds after the crash (gap 3 of `T-020-49`'s first build).
        if matches!(asked.change, ReleaseChange::Confirm { .. }) {
            // A restriction whose result was never written is completed first from the platform's standing, so one that
            // never took effect is `failed` and holds the provider no more (`T-020-65`).
            complete_restrictions(log, releases, Some(asked.provider), clock, asked.core)?;
            let history = log.history();
            for (restriction, by) in restrictions(&history, asked.provider) {
                let by = by.unwrap_or_else(|| request.caller.acts.clone());
                retake_restriction(releases, &restriction, &by, clock)?;
            }
        }
        let receipt = journal::append(
            log,
            journal::verified(
                asked.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &now(clock)?,
                Outcome::ToTakeEffect {},
            ),
        )?;
        let effect = Effect {
            action,
            decision: permit,
            receipt,
        };
        let changed =
            carry_out_release_change(releases, &effect, asked, &request.caller.acts, clock);
        let outcome = match changed {
            Ok(_) => Outcome::Done {},
            Err(_) => Outcome::Failed {},
        };
        journal::append(
            log,
            journal::verified(
                asked.core,
                What::Result {
                    effect: effect.receipt.clone(),
                },
                &now(clock)?,
                outcome,
            ),
        )?;
        if changed.is_ok() {
            self.acted.insert(key(&effect.receipt));
        }
        changed
    }
}
