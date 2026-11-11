//! The `calls` figure (`F-011`'s notes, router-api 3.0.0, `REQ-MDL-21`): what each provider's accepted release
//! declares it may call, written into the snapshot so that a route names its segments' calls and the platform lets
//! through no other (`THR-86`).
//!
//! Core records each release once it is accepted, from its admission artefact. A snapshot carries the calls figure
//! of the release its provider's tuple figures name, by `module_version`: a route's operation cites the calls figure
//! of its own tuple's release, so a figure of any other release would be refused by the record's check. A release
//! that declares no call, or only calls the snapshot can't carry, gives no figure; a call left out is one the route
//! doesn't name and the platform refuses (`not_named_by_route`), so leaving one out fails closed.

use rutent_contracts::module_interface::{AdmissionArtefact, OperationKind};
use rutent_contracts::router_api::{DeclaredCall, Id, NonEmpty, Timestamp, Version};

/// One accepted release, as the snapshot reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Its provider.
    pub provider: Id,
    /// Its version: the `module_version` its provider's figures name.
    pub version: Version,
    /// When the provider made its release assertion.
    pub made_at: Timestamp,
    /// The calls it declares, as the snapshot carries them; `None` when there are none to carry.
    pub calls: Option<NonEmpty<DeclaredCall>>,
}

/// Why a release wasn't recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseError {
    /// It hasn't been accepted: only an accepted release runs.
    NotAccepted,
    /// Its provider, version, time or a callee can't be written into the router's snapshot.
    NotRepresentable(&'static str),
}

/// The accepted releases Core knows, oldest first.
#[derive(Debug, Clone, Default)]
pub struct Releases {
    accepted: Vec<Release>,
}

impl Releases {
    /// No release recorded.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an accepted release from its admission artefact. A call for a holding balance, which no module may
    /// make, and a call to the provider itself are left out, as the snapshot can carry neither; a callee left with no
    /// kind is left out.
    pub fn record(&mut self, artefact: &AdmissionArtefact) -> Result<(), ReleaseError> {
        if artefact.acceptance.accepted.is_none() {
            return Err(ReleaseError::NotAccepted);
        }
        let declaration = artefact.declaration();
        let bad = ReleaseError::NotRepresentable;
        let provider =
            Id::new(declaration.provider.as_str()).ok_or(bad("a provider is an identifier"))?;
        let version = Version::new(declaration.version.as_str())
            .ok_or(bad("a module version is a version"))?;
        let made_at = Timestamp::new(artefact.assertion.made_at.as_str())
            .ok_or(bad("a release assertion's made_at is a timestamp"))?;
        let mut calls = Vec::new();
        for call in &declaration.outside.calls {
            let callee = Id::new(call.callee.as_str()).ok_or(bad("a callee is an identifier"))?;
            if callee == provider {
                continue;
            }
            let mut kinds: Vec<OperationKind> = Vec::new();
            for kind in &call.kinds {
                if *kind != OperationKind::HoldingBalance && !kinds.contains(kind) {
                    kinds.push(*kind);
                }
            }
            if calls.iter().any(|c: &DeclaredCall| c.callee == callee) {
                continue;
            }
            if let Some(kinds) = NonEmpty::new(kinds) {
                calls.push(DeclaredCall { callee, kinds });
            }
        }
        let release = Release {
            provider,
            version,
            made_at,
            calls: NonEmpty::new(calls),
        };
        self.accepted
            .retain(|r| !(r.provider == release.provider && r.version == release.version));
        self.accepted.push(release);
        Ok(())
    }

    /// The accepted release of `provider` at `version`, if Core recorded one.
    pub fn release(&self, provider: &Id, version: &Version) -> Option<&Release> {
        self.accepted
            .iter()
            .find(|r| &r.provider == provider && &r.version == version)
    }

    /// The release of `provider` accepted last, if any.
    pub fn latest(&self, provider: &Id) -> Option<&Release> {
        self.accepted.iter().rev().find(|r| &r.provider == provider)
    }
}
