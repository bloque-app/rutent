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

use super::Core;
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
}
