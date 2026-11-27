//! A provider's module upgraded while Core runs (`T-024-07`, `AC-19`, `AC-21`): a release is published to the platform
//! Core's segments run on, waits for its fresh acceptance where it widens what the module may reach, and takes
//! the provider's next segments only once it is in force. Which version each segment began and ran on is Core's own
//! record, a `hand_over` entry of the log ([`Core::segment_versions`]).
//!
//! Core never publishes or accepts a release itself, and nothing here decides one: the operator's acceptance is the
//! catalogue's (`T-013-04`), and a promotion goes through policy and the log as any catalogue change does (`T-020-08`).

use rutent_contracts::core_internal::{Entry, HandedOver, What};
use rutent_contracts::module_interface::AdmissionArtefact;
use rutent_prov::module::types::Admitted;

use rutent_core_orch::segments::lifecycle::{Changed, ReleaseAsked, ReleaseChange, ReleaseRefused};
use rutent_core_orch::segments::roll_back::{RollBackAsked, RollBackRefused};
use rutent_core_orch::segments::{PromoteRefused, Promotion};

use super::{Core, Running};
use crate::sim::Release;
use crate::sim::probes::{self, Probe, Probed, Reach};

/// One segment as Core's log records the module versions it ran on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentVersion {
    /// The provider whose module ran it.
    pub provider: String,
    /// The version in force when it began.
    pub began_on: String,
    /// The version it ran on.
    pub runs_on: String,
}

impl Core {
    /// Publishes `release` of the installed provider `provider` to the platform.
    pub fn publish_release(&mut self, provider: &str, release: &Release) -> Admitted {
        self.fleet.release(provider, release)
    }

    /// Gives `provider`'s waiting release its fresh acceptance, by the platform's test reviewer.
    pub fn accept_waiting_release(
        &mut self,
        provider: &str,
        artefact: &AdmissionArtefact,
    ) -> Option<AdmissionArtefact> {
        self.fleet.accept(provider, artefact)
    }

    /// Every segment Core's log records, in order, with the version of its provider's module it began on and the one
    /// it ran on.
    pub fn segment_versions(&self) -> Vec<SegmentVersion> {
        self.log()
            .entries()
            .iter()
            .filter_map(|(_, entry)| match entry {
                Entry::Verified(v) => match &v.what {
                    What::HandOver {
                        item:
                            HandedOver::SegmentRecord {
                                provider,
                                began_on,
                                runs_on,
                                ..
                            },
                        ..
                    } => Some(SegmentVersion {
                        provider: provider.as_str().to_owned(),
                        began_on: began_on.as_str().to_owned(),
                        runs_on: runs_on.as_str().to_owned(),
                    }),
                    _ => None,
                },
                Entry::Stated(_) => None,
            })
            .collect()
    }

    /// Admits a probe module of `language`, `rs` or `c`, to the platform beside the simulated providers: a stand-in
    /// that attempts `reaches` when it is handed an operation ([`probes`]). It is a provider of the platform's own,
    /// and no provider of Core's catalogue or log.
    pub fn install_probe(&mut self, language: &str, reaches: &[Reach]) -> Probe {
        probes::install(&mut self.fleet, language, reaches)
    }

    /// Hands `probe`'s module the `n`th withdrawal, and takes what the platform recorded of it.
    pub fn handle_probe(&mut self, probe: &Probe, n: usize) -> Probed {
        probes::handle(&mut self.fleet, probe, n)
    }

    /// Puts `provider`'s staged release at `version` in force, as the operator's login asks and policy decides: Core
    /// records the effect before it calls the platform and its result after (`REQ-AUZ-6`, `T-020-08`).
    ///
    /// # Errors
    ///
    /// If nothing is staged, the version isn't the one staged, policy doesn't permit it, or the platform won't.
    pub fn promote(&mut self, provider: &str, version: &str) -> Result<String, PromoteRefused> {
        let readings = self.readings.clone();
        let mut running = Running {
            platform: &mut self.fleet.platform,
            clock: &self.clock,
            faults: &mut self.faults,
            feed: &mut self.feed,
            log: &self.log,
            swap_to: self.swap_to.as_deref_mut(),
            readings: &readings,
        };
        let mut decisions = self.policy.clone();
        let (provider, version) = (crate::kit::id(provider), crate::kit::id(version));
        let (caller, core) = (crate::kit::operator_login(), crate::kit::core());
        self.segments.promote(
            &mut self.general,
            &mut decisions,
            &mut running,
            &self.clock,
            &mut self.fresh,
            &Promotion {
                provider: &provider,
                version: &version,
                caller: &caller,
                core: &core,
            },
        )
    }

    /// Ends `provider`'s staging of `version`, as `caller` asks and policy decides: Core records the effect before it
    /// calls the platform and its result after (`REQ-AUZ-6`, `T-021-12`, `T-020-42`). The version in force stays.
    ///
    /// # Errors
    ///
    /// If nothing is staged, the version isn't the one staged, policy doesn't permit it, or the platform won't.
    pub fn roll_back_as(
        &mut self,
        caller: &rutent_contracts::core_internal::Actor,
        provider: &str,
        version: &str,
    ) -> Result<String, RollBackRefused> {
        let mut decisions = self.policy.clone();
        let (provider, version) = (crate::kit::id(provider), crate::kit::id(version));
        let core = crate::kit::core();
        self.segments.roll_back(
            &mut self.general,
            &mut decisions,
            &mut self.fleet.platform,
            &self.clock,
            &mut self.fresh,
            &RollBackAsked {
                provider: &provider,
                version: &version,
                caller,
                core: &core,
            },
        )
    }

    /// [`Core::roll_back_as`] by the operator's login.
    ///
    /// # Errors
    ///
    /// As [`Core::roll_back_as`].
    pub fn roll_back(&mut self, provider: &str, version: &str) -> Result<String, RollBackRefused> {
        self.roll_back_as(&crate::kit::operator_login(), provider, version)
    }

    /// Carries out one of a release's lifecycle actions at `provider` (accepting or withdrawing the waiting release,
    /// revoking a version, declaring it exploited, marking it stale, or confirming the provider's releases after the
    /// platform's unclean end), as `caller` asks and policy decides, through Core's decided path
    /// (`Segments::change_release`, `T-020-49`): policy first, the effect recorded before the platform is asked and its
    /// result after.
    ///
    /// # Errors
    ///
    /// If policy doesn't permit it, what the platform holds isn't what it names, or the platform refuses.
    pub fn change_release_as(
        &mut self,
        caller: &rutent_contracts::core_internal::Actor,
        provider: &str,
        change: ReleaseChange<'_>,
    ) -> Result<Changed, ReleaseRefused> {
        let mut decisions = self.policy.clone();
        let provider = crate::kit::id(provider);
        let core = crate::kit::core();
        let mut releases = self.fleet.releases();
        self.segments.change_release(
            &mut self.general,
            &mut decisions,
            &mut releases,
            &self.clock,
            &mut self.fresh,
            &ReleaseAsked {
                provider: &provider,
                change,
                caller,
                core: &core,
            },
        )
    }

    /// [`Core::change_release_as`] by the operator's login.
    ///
    /// # Errors
    ///
    /// As [`Core::change_release_as`].
    pub fn change_release(
        &mut self,
        provider: &str,
        change: ReleaseChange<'_>,
    ) -> Result<Changed, ReleaseRefused> {
        self.change_release_as(&crate::kit::operator_login(), provider, change)
    }

    /// Sets the deployment's staging policy (ADR-0014) on the platform, which keeps it across its restarts.
    pub fn set_staging(&mut self, staging: Option<rutent_prov::module::lifecycle::Staging>) {
        self.fleet.set_staging(staging);
    }

    /// The platform's host restarts it on its record (`Fleet::restart_platform`, `T-020-66`), unclean where `unclean`:
    /// then every provider it held a release of waits for the operator's confirmation. Core goes on as it was.
    pub fn restart_platform(&mut self, unclean: bool) {
        self.fleet.restart_platform(unclean);
    }
}
