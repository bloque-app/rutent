//! The lifecycle of each provider's accepted releases (`T-013-06`; `REQ-MDL-4`, `REQ-MDL-5`, `REQ-MDL-14`,
//! `REQ-MDL-19`, `REQ-MDL-20`, `INV-8`, ADR-0014): which version is in force, the release staged beside it, the
//! segments in flight and the version each runs on, the operator's marks on versions, and every decision with its
//! actor. It is pure: it starts and stops no process. The platform asks it which version each operation runs on
//! and which versions must have a process ([`Lifecycle::wanted`]), and makes it so.
//!
//! - **Staging.** With a [`Staging`] policy set, an accepted release of a provider that already has a version in
//!   force is staged beside it, never put in force at once: a share of the segments that begin from then on run
//!   on it, and the rest on the version in force. It is put in force only by the operator's decision,
//!   [`Lifecycle::promote`], and only once both the time window has passed and the staged version has handled the
//!   minimum number of operations, so a provider with little traffic stays staged until its minimum is observed
//!   (`PLAN.md` §6.2 risk 7). [`Lifecycle::roll_back`] ends the staging instead, and the version in force stays.
//!   While a release is staged no other is admitted, and no version is marked stale, since a stale mark would move
//!   segments past the staging (`THR-16`).
//! - **Segments.** A segment begins on one version ([`Lifecycle::begin_segment`]) and runs on it until it ends,
//!   whatever is accepted, staged or put in force meanwhile. It moves only to the version in force, which was
//!   accepted, and only when the operator marks its version stale; each move is recorded in the segment's
//!   [`SegmentRecord`] (`REQ-MDL-5`, `REQ-MDL-19`). A segment on a revoked version finishes on it; one on a version
//!   declared actively exploited is refused and not moved (`REQ-MDL-20`).
//! - **Transactions.** A segment is known by its transaction, which is identified once: a transaction a segment
//!   holds, or held, never begins another, and an operation naming it at another provider or after its segment
//!   ended is refused (`REQ-MDL-14`, `INV-8`).
//! - **Decisions.** Withdrawing a waiting release, promoting, rolling back, revoking, marking stale and declaring
//!   exploited are each recorded with their actor and the caller's clock ([`Decision`]). Going back to an older
//!   version happens only by one of them, never by publishing (`REQ-MDL-4`). Who may take each is the
//!   authorisation policy's (`REQ-AUZ-6`); the platform records who did.
//!
//! An operation whose transaction no segment began runs on the version in force, as every operation did before
//! segments, and doesn't count towards a staging.
//!
//! This is the platform's admission lifecycle, a `DEC-20` path.

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::module_interface::{Identifier, PlatformRefusal};

use crate::catalogue::Instant;

/// The prefix of a per-transaction state key: `transaction/<transaction>/<entry>` is the entry of that transaction
/// alone, readable only while the platform handles one of its operations (`REQ-MDL-10`). Any other key is the
/// provider's shared state.
pub const TRANSACTION_STATE: &str = "transaction/";

/// Whether an operation of `transaction` may read the state entry `key`: a shared entry, or one of its own
/// transaction's. The transaction is the one the platform is handling, never one the module names; an operation
/// that names none (a verifier's judgement) reads no transaction's entry.
pub fn state_readable(key: &str, transaction: &str) -> bool {
    match key.strip_prefix(TRANSACTION_STATE) {
        None => true,
        Some(rest) => {
            !transaction.is_empty()
                && rest
                    .split_once('/')
                    .is_some_and(|(owner, _)| owner == transaction)
        }
    }
}

/// How an upgrade is staged (ADR-0014): the share of new segments the staged release takes, and what must be
/// observed of it before it may be put in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Staging {
    share: u8,
    window: u64,
    minimum: u64,
}

impl Staging {
    /// A policy giving the staged release `share` percent of the segments that begin, from 1 to 100, and
    /// requiring, before it is put in force, `window` seconds of the caller's clock since it was staged and
    /// `minimum` operations it handled. `None` for a share outside 1 to 100.
    pub fn new(share: u8, window: u64, minimum: u64) -> Option<Self> {
        (1..=100).contains(&share).then_some(Self {
            share,
            window,
            minimum,
        })
    }

    /// The share, in percent.
    pub fn share(&self) -> u8 {
        self.share
    }

    /// The time window, in seconds.
    pub fn window(&self) -> u64 {
        self.window
    }

    /// The minimum observation volume, in operations handled.
    pub fn minimum(&self) -> u64 {
        self.minimum
    }
}

/// Where an accepted release goes once its process starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// In force: new segments and unsegmented operations run on it.
    InForce,
    /// Staged beside the version in force, on a share of new segments.
    Staged,
}

/// What one version did during a staging.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Segments that began on it.
    pub segments: u64,
    /// Operations it answered.
    pub answered: u64,
    /// Operations the platform refused for it.
    pub refused: u64,
}

impl Counts {
    /// Operations it handled, answered or refused.
    pub fn handled(&self) -> u64 {
        self.answered + self.refused
    }
}

/// What has been observed of a staged release and of the version in force beside it, since it was staged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// The staged version.
    pub staged: String,
    /// The version in force beside it.
    pub in_force: String,
    /// When it was staged, by the caller's clock.
    pub since: Instant,
    /// The policy it was staged under.
    pub policy: Staging,
    /// What the staged version did.
    pub staged_counts: Counts,
    /// What the version in force did meanwhile.
    pub in_force_counts: Counts,
}

/// The operator's mark on a version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Takes no new segment; segments in flight on it finish on it.
    Revoked,
    /// Revoked, and runs nothing more: segments on it are refused (`REQ-MDL-20`).
    Exploited,
    /// Its segments run on the version in force (`REQ-MDL-19`).
    Stale,
}

/// What a recorded decision did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionKind {
    /// A release waiting for its fresh acceptance was withdrawn.
    Withdrawn,
    /// The staged release was put in force.
    Promoted,
    /// The staged release was rolled back: revoked, the version in force staying.
    RolledBack,
    /// The version was revoked.
    Revoked,
    /// The version was marked stale.
    MarkedStale,
    /// The version was declared actively exploited.
    DeclaredExploited,
    /// The version was put back in force, the one in force having been revoked.
    Reinstated,
}

/// One recorded lifecycle decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The provider.
    pub provider: String,
    /// The version it was about.
    pub version: String,
    /// What it did.
    pub kind: DecisionKind,
    /// Who took it.
    pub by: Identifier,
    /// When, by the caller's clock.
    pub at: Instant,
}

/// Why a segment moved to another version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveReason {
    /// Its version was marked stale (`REQ-MDL-19`).
    Stale,
}

/// One move of a segment from a version to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    /// The version it ran on.
    pub from: String,
    /// The version it runs on from then on, the one in force.
    pub to: String,
    /// Why.
    pub reason: MoveReason,
    /// When, by the caller's clock.
    pub at: Instant,
}

/// The record of one segment: its transaction, its provider, the version it began on, the one it runs on, and
/// every move between them (`REQ-MDL-5`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRecord {
    /// The transaction.
    pub transaction: String,
    /// The provider.
    pub provider: String,
    /// The version it began on.
    pub began_on: String,
    /// The version it runs on.
    pub runs_on: String,
    /// Its moves, in order.
    pub moves: Vec<Move>,
}

/// Why a lifecycle decision was refused. Nothing changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleRefusal {
    /// `by` is no identifier.
    NoActor,
    /// The provider has no accepted release of that version.
    NoSuchVersion,
    /// No release of the provider is staged.
    NotStaged,
    /// The staging's time window hasn't passed: it does at `until`, by the caller's clock.
    WindowOpen {
        /// When it does.
        until: Instant,
    },
    /// The staged release has handled fewer operations than the minimum.
    TooFewObserved {
        /// Operations it handled.
        observed: u64,
        /// The minimum.
        minimum: u64,
    },
    /// A release is staged: no version is marked stale until the staging ends (ADR-0014, `THR-16`).
    StagingInProgress,
    /// No version newer than it is in force for its segments to move to.
    NoNewerVersion,
    /// The version is revoked, exploited or stale already.
    AlreadyMarked,
}

#[derive(Debug, Clone)]
struct Staged {
    version: String,
    since: Instant,
    policy: Staging,
    /// Segments that began since it was staged, and how many of them it took.
    began: u64,
    taken: u64,
    staged_counts: Counts,
    in_force_counts: Counts,
}

#[derive(Debug, Clone, Default)]
struct Releases {
    accepted: BTreeSet<String>,
    in_force: Option<String>,
    /// Every version put in force, in order.
    history: Vec<String>,
    staged: Option<Staged>,
    marks: BTreeMap<String, Mark>,
}

impl Releases {
    fn runs(&self, version: &str) -> bool {
        self.in_force.as_deref() == Some(version)
            || self.staged.as_ref().is_some_and(|s| s.version == version)
    }

    /// Takes `version` out of force or out of staging, as revoking it does: the version in force before it, if
    /// one is unmarked, is put back in force (a recorded decision of its own), and a staging of it ends.
    fn retire(&mut self, version: &str) -> Option<String> {
        if self.staged.as_ref().is_some_and(|s| s.version == version) {
            self.staged = None;
        }
        if self.in_force.as_deref() != Some(version) {
            return None;
        }
        let back = self
            .history
            .iter()
            .rev()
            .find(|v| v.as_str() != version && !self.marks.contains_key(v.as_str()))
            .cloned();
        self.in_force.clone_from(&back);
        if let Some(back) = &back {
            self.history.push(back.clone());
        }
        back
    }
}

/// The lifecycle of every provider's releases on one platform. See the module's documentation.
#[derive(Debug, Clone, Default)]
pub struct Lifecycle {
    policy: Option<Staging>,
    providers: BTreeMap<String, Releases>,
    segments: BTreeMap<String, SegmentRecord>,
    ended: BTreeSet<String>,
    decisions: Vec<Decision>,
}

impl Lifecycle {
    /// Sets how upgrades are staged; `None`, the default, puts an accepted release in force once it starts.
    pub fn set_staging(&mut self, policy: Option<Staging>) {
        self.policy = policy;
    }

    /// How upgrades are staged.
    pub fn staging(&self) -> Option<Staging> {
        self.policy
    }

    /// The provider's release of `version` was accepted. It runs only once [`Lifecycle::started`] says so.
    pub fn accept(&mut self, provider: &str, version: &str) {
        self.providers
            .entry(provider.to_owned())
            .or_default()
            .accepted
            .insert(version.to_owned());
    }

    /// Where the provider's next accepted release goes once it starts: staged, with a policy set and a version in
    /// force, and in force otherwise.
    pub fn placement(&self, provider: &str) -> Placement {
        let in_force = self
            .providers
            .get(provider)
            .is_some_and(|r| r.in_force.is_some());
        if self.policy.is_some() && in_force {
            Placement::Staged
        } else {
            Placement::InForce
        }
    }

    /// The provider's accepted release of `version` started, at the caller's clock `at`, and goes where
    /// `placement` says. A release that didn't start is never told here, so the version in force stays.
    pub fn started(&mut self, provider: &str, version: &str, placement: Placement, at: Instant) {
        let policy = self.policy;
        let releases = self.providers.entry(provider.to_owned()).or_default();
        if !releases.accepted.contains(version) {
            return;
        }
        match (placement, policy) {
            (Placement::Staged, Some(policy)) if releases.in_force.is_some() => {
                releases.staged = Some(Staged {
                    version: version.to_owned(),
                    since: at,
                    policy,
                    began: 0,
                    taken: 0,
                    staged_counts: Counts::default(),
                    in_force_counts: Counts::default(),
                });
            }
            _ => {
                releases.in_force = Some(version.to_owned());
                releases.history.push(version.to_owned());
            }
        }
    }

    /// The version's process could not be started where it was wanted: out of force or staging, with no
    /// replacement, so new segments find none.
    pub fn not_running(&mut self, provider: &str, version: &str) {
        if let Some(releases) = self.providers.get_mut(provider) {
            if releases.in_force.as_deref() == Some(version) {
                releases.in_force = None;
            }
            if releases
                .staged
                .as_ref()
                .is_some_and(|s| s.version == version)
            {
                releases.staged = None;
            }
        }
    }

    /// The provider's version in force.
    pub fn in_force(&self, provider: &str) -> Option<&str> {
        self.providers.get(provider)?.in_force.as_deref()
    }

    /// The provider's staged version.
    pub fn staged(&self, provider: &str) -> Option<&str> {
        self.providers
            .get(provider)?
            .staged
            .as_ref()
            .map(|s| s.version.as_str())
    }

    /// Whether the provider has an accepted release of `version`.
    pub fn is_accepted(&self, provider: &str, version: &str) -> bool {
        self.providers
            .get(provider)
            .is_some_and(|r| r.accepted.contains(version))
    }

    /// The operator's mark on the provider's `version`.
    pub fn mark(&self, provider: &str, version: &str) -> Option<Mark> {
        self.providers.get(provider)?.marks.get(version).copied()
    }

    /// What has been observed of the provider's staged release.
    pub fn observation(&self, provider: &str) -> Option<Observation> {
        let releases = self.providers.get(provider)?;
        let staged = releases.staged.as_ref()?;
        Some(Observation {
            staged: staged.version.clone(),
            in_force: releases.in_force.clone().unwrap_or_default(),
            since: staged.since,
            policy: staged.policy,
            staged_counts: staged.staged_counts,
            in_force_counts: staged.in_force_counts,
        })
    }

    /// The versions of the provider that must have a process: the one in force, the staged one, and every one a
    /// segment in flight runs on, but never one declared exploited.
    pub fn wanted(&self, provider: &str) -> BTreeSet<String> {
        let Some(releases) = self.providers.get(provider) else {
            return BTreeSet::new();
        };
        releases
            .in_force
            .iter()
            .chain(releases.staged.as_ref().map(|s| &s.version))
            .chain(
                self.segments
                    .values()
                    .filter(|s| s.provider == provider)
                    .map(|s| &s.runs_on),
            )
            .filter(|v| releases.marks.get(v.as_str()) != Some(&Mark::Exploited))
            .cloned()
            .collect()
    }

    /// Begins the segment of `transaction` at the provider, on the version it will run on: the staged one, for its
    /// share of the segments that begin while it is staged, and the one in force otherwise. Refused as
    /// `DuplicateTransaction` for a transaction a segment holds or held, and `Unavailable` where the provider has
    /// no version in force; neither records the transaction.
    ///
    /// The share is taken in order: the `n`th segment to begin while a release is staged runs on it if the staged
    /// version's count, with it, stays within its share of the `n` (`(taken + 1) * 100 <= share * n`), so any
    /// prefix of the segments is split as near the share as whole segments allow.
    pub fn begin_segment(
        &mut self,
        provider: &str,
        transaction: &Identifier,
    ) -> Result<String, PlatformRefusal> {
        let transaction = transaction.as_str();
        if self.ended.contains(transaction) || self.segments.contains_key(transaction) {
            return Err(PlatformRefusal::DuplicateTransaction);
        }
        let releases = self
            .providers
            .get_mut(provider)
            .ok_or(PlatformRefusal::Unavailable)?;
        let in_force = releases
            .in_force
            .clone()
            .ok_or(PlatformRefusal::Unavailable)?;
        let version = match &mut releases.staged {
            Some(staged) => {
                staged.began += 1;
                if (staged.taken + 1) * 100 <= u64::from(staged.policy.share) * staged.began {
                    staged.taken += 1;
                    staged.staged_counts.segments += 1;
                    staged.version.clone()
                } else {
                    staged.in_force_counts.segments += 1;
                    in_force
                }
            }
            None => in_force,
        };
        self.segments.insert(
            transaction.to_owned(),
            SegmentRecord {
                transaction: transaction.to_owned(),
                provider: provider.to_owned(),
                began_on: version.clone(),
                runs_on: version.clone(),
                moves: Vec::new(),
            },
        );
        Ok(version)
    }

    /// The version an operation of `transaction` at the provider runs on: its segment's, if a segment holds it,
    /// and the version in force otherwise. Refused as `DuplicateTransaction` where the segment is another
    /// provider's or has ended, `VersionExploited` where its version was declared exploited, and `Unavailable`
    /// where there is no version to run on.
    pub fn version_for(
        &self,
        provider: &str,
        transaction: Option<&str>,
    ) -> Result<String, PlatformRefusal> {
        if let Some(transaction) = transaction {
            if self.ended.contains(transaction) {
                return Err(PlatformRefusal::DuplicateTransaction);
            }
            if let Some(segment) = self.segments.get(transaction) {
                if segment.provider != provider {
                    return Err(PlatformRefusal::DuplicateTransaction);
                }
                return match self.mark(provider, &segment.runs_on) {
                    Some(Mark::Exploited) => Err(PlatformRefusal::VersionExploited),
                    _ => Ok(segment.runs_on.clone()),
                };
            }
        }
        self.in_force(provider)
            .map(str::to_owned)
            .ok_or(PlatformRefusal::Unavailable)
    }

    /// The provider's `version` handled an operation, answered or refused: counted towards a staging.
    pub fn observe(&mut self, provider: &str, version: &str, answered: bool) {
        let Some(releases) = self.providers.get_mut(provider) else {
            return;
        };
        let in_force = releases.in_force.as_deref() == Some(version);
        let Some(staged) = &mut releases.staged else {
            return;
        };
        let counts = if staged.version == version {
            &mut staged.staged_counts
        } else if in_force {
            &mut staged.in_force_counts
        } else {
            return;
        };
        if answered {
            counts.answered += 1;
        } else {
            counts.refused += 1;
        }
    }

    /// The segment of `transaction`, in flight.
    pub fn segment(&self, transaction: &str) -> Option<&SegmentRecord> {
        self.segments.get(transaction)
    }

    /// Ends the segment of `transaction` and hands its record over. Its transaction never begins another.
    pub fn end_segment(&mut self, transaction: &str) -> Option<SegmentRecord> {
        let record = self.segments.remove(transaction)?;
        self.ended.insert(transaction.to_owned());
        Some(record)
    }

    /// Every decision recorded, in order.
    pub fn decisions(&self) -> &[Decision] {
        &self.decisions
    }

    fn record(
        &mut self,
        provider: &str,
        version: &str,
        kind: DecisionKind,
        by: Identifier,
        at: Instant,
    ) {
        self.decisions.push(Decision {
            provider: provider.to_owned(),
            version: version.to_owned(),
            kind,
            by,
            at,
        });
    }

    /// Records that the provider's release of `version`, waiting for its fresh acceptance, was withdrawn by `by`.
    pub fn withdrawn(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Instant,
    ) -> Result<(), LifecycleRefusal> {
        let by = Identifier::new(by).ok_or(LifecycleRefusal::NoActor)?;
        self.record(provider, version, DecisionKind::Withdrawn, by, at);
        Ok(())
    }

    /// Puts the provider's staged release in force, by `by` at the caller's clock `at`, once its window has passed
    /// and it has handled its minimum. The staged version.
    pub fn promote(
        &mut self,
        provider: &str,
        by: &str,
        at: Instant,
    ) -> Result<String, LifecycleRefusal> {
        let by = Identifier::new(by).ok_or(LifecycleRefusal::NoActor)?;
        let releases = self
            .providers
            .get_mut(provider)
            .ok_or(LifecycleRefusal::NotStaged)?;
        let staged = releases
            .staged
            .as_ref()
            .ok_or(LifecycleRefusal::NotStaged)?;
        let until = Instant(staged.since.0.saturating_add(staged.policy.window));
        if at.0 < until.0 {
            return Err(LifecycleRefusal::WindowOpen { until });
        }
        let observed = staged.staged_counts.handled();
        if observed < staged.policy.minimum {
            return Err(LifecycleRefusal::TooFewObserved {
                observed,
                minimum: staged.policy.minimum,
            });
        }
        let version = staged.version.clone();
        releases.staged = None;
        releases.in_force = Some(version.clone());
        releases.history.push(version.clone());
        self.record(provider, &version, DecisionKind::Promoted, by, at);
        Ok(version)
    }

    /// Rolls the provider's staged release back, by `by`: it is revoked, so it takes no new segment and those on it
    /// finish on it, and the version in force stays. The staged version.
    pub fn roll_back(
        &mut self,
        provider: &str,
        by: &str,
        at: Instant,
    ) -> Result<String, LifecycleRefusal> {
        let by = Identifier::new(by).ok_or(LifecycleRefusal::NoActor)?;
        let releases = self
            .providers
            .get_mut(provider)
            .ok_or(LifecycleRefusal::NotStaged)?;
        let version = releases
            .staged
            .take()
            .ok_or(LifecycleRefusal::NotStaged)?
            .version;
        releases.marks.insert(version.clone(), Mark::Revoked);
        self.record(provider, &version, DecisionKind::RolledBack, by, at);
        Ok(version)
    }

    /// Revokes the provider's `version`, by `by`: it takes no new segment, and those on it finish on it. Revoking
    /// the staged version ends its staging; revoking the version in force puts the one in force before it back,
    /// if one is unmarked, recorded as `Reinstated`. The version put back in force, if any.
    pub fn revoke(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Instant,
    ) -> Result<Option<String>, LifecycleRefusal> {
        self.retire(
            provider,
            version,
            Mark::Revoked,
            DecisionKind::Revoked,
            by,
            at,
        )
    }

    /// Declares the provider's `version` actively exploited, by `by`: it is revoked, and runs nothing more, so its
    /// segments are refused and not moved (`REQ-MDL-20`), and its process is stopped. A version marked stale or
    /// revoked may still be declared exploited. The version put back in force, if any.
    pub fn declare_exploited(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Instant,
    ) -> Result<Option<String>, LifecycleRefusal> {
        self.retire(
            provider,
            version,
            Mark::Exploited,
            DecisionKind::DeclaredExploited,
            by,
            at,
        )
    }

    fn retire(
        &mut self,
        provider: &str,
        version: &str,
        mark: Mark,
        kind: DecisionKind,
        by: &str,
        at: Instant,
    ) -> Result<Option<String>, LifecycleRefusal> {
        let by = Identifier::new(by).ok_or(LifecycleRefusal::NoActor)?;
        let releases = self
            .providers
            .get_mut(provider)
            .filter(|r| r.accepted.contains(version))
            .ok_or(LifecycleRefusal::NoSuchVersion)?;
        match (releases.marks.get(version), mark) {
            (Some(Mark::Exploited), _) | (Some(Mark::Revoked), Mark::Revoked) => {
                return Err(LifecycleRefusal::AlreadyMarked);
            }
            _ => {}
        }
        releases.marks.insert(version.to_owned(), mark);
        let back = releases.retire(version);
        self.record(provider, version, kind, by.clone(), at);
        if let Some(back) = &back {
            self.record(provider, back, DecisionKind::Reinstated, by, at);
        }
        Ok(back)
    }

    /// Marks the provider's `version` stale, by `by`: every segment in flight on it moves to the version in force,
    /// the newest version available, and the move is recorded in its segment's record (`REQ-MDL-19`). Refused
    /// while a release is staged (ADR-0014, `THR-16`), for the version in force itself, and where none is in force.
    /// How many segments moved.
    pub fn mark_stale(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Instant,
    ) -> Result<usize, LifecycleRefusal> {
        let by = Identifier::new(by).ok_or(LifecycleRefusal::NoActor)?;
        let releases = self
            .providers
            .get_mut(provider)
            .filter(|r| r.accepted.contains(version))
            .ok_or(LifecycleRefusal::NoSuchVersion)?;
        if releases.staged.is_some() {
            return Err(LifecycleRefusal::StagingInProgress);
        }
        if releases.marks.contains_key(version) {
            return Err(LifecycleRefusal::AlreadyMarked);
        }
        let to = match &releases.in_force {
            Some(in_force) if in_force != version => in_force.clone(),
            _ => return Err(LifecycleRefusal::NoNewerVersion),
        };
        releases.marks.insert(version.to_owned(), Mark::Stale);
        let mut moved = 0;
        for segment in self.segments.values_mut() {
            if segment.provider == provider && segment.runs_on == version {
                segment.moves.push(Move {
                    from: version.to_owned(),
                    to: to.clone(),
                    reason: MoveReason::Stale,
                    at,
                });
                segment.runs_on.clone_from(&to);
                moved += 1;
            }
        }
        self.record(provider, version, DecisionKind::MarkedStale, by, at);
        Ok(moved)
    }

    /// Whether the provider's `version` runs: in force or staged.
    pub fn runs(&self, provider: &str, version: &str) -> bool {
        self.providers
            .get(provider)
            .is_some_and(|r| r.runs(version))
    }
}

#[cfg(test)]
mod tests {
    use super::state_readable;

    /// A transaction reads the shared entries and its own, and never another transaction's, however the key is
    /// spelled.
    // verifies: REQ-MDL-10, REQ-MDL-14
    #[test]
    fn a_transaction_reads_shared_state_and_its_own_only() {
        assert!(state_readable("rates", "tx-1"));
        assert!(state_readable("transaction/tx-1/nonce", "tx-1"));
        assert!(!state_readable("transaction/tx-2/nonce", "tx-1"));
        assert!(
            !state_readable("transaction/tx-1", "tx-1"),
            "no entry named"
        );
        assert!(!state_readable("transaction/tx-1x/nonce", "tx-1"));
        assert!(!state_readable("transaction//nonce", ""), "no transaction");
        assert!(!state_readable("transaction/tx-1/nonce", ""));
    }
}
