//! Reserve declarations: recorded once, attributable, shown before acceptance (`REQ-CAT-6`).

use std::collections::HashSet;

use rutent_contracts::provider_role::Advertisement;

use super::grant::Instant;

/// A declaration's identifier in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeclarationId(pub u64);

/// One recorded declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationRecord {
    /// Its identifier.
    pub id: DeclarationId,
    /// The reserve provider.
    pub provider: String,
    /// The provider version it was declared in.
    pub version: String,
    /// The party it is attributable to.
    pub maker: String,
    /// Whether the reserve can require the user to start a withdrawal themselves.
    pub can_require_user_initiated_withdrawal: bool,
    /// Whether the reserve starts routes on automatic deposits.
    pub starts_routes_on_automatic_deposits: bool,
    /// When the catalogue recorded it.
    pub recorded_at: Instant,
}

impl DeclarationRecord {
    fn same_content(&self, other: &Self) -> bool {
        (
            &self.maker,
            self.can_require_user_initiated_withdrawal,
            self.starts_routes_on_automatic_deposits,
        ) == (
            &other.maker,
            other.can_require_user_initiated_withdrawal,
            other.starts_routes_on_automatic_deposits,
        )
    }
}

/// What recording a declaration did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    /// A new declaration.
    Recorded(DeclarationId),
    /// An unchanged declaration, already on record under this identifier.
    AlreadyRecorded(DeclarationId),
}

/// Why a declaration is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationError {
    /// A version's declaration differs from the one already recorded for that version: a change is a new
    /// version's declaration.
    ChangedWithinVersion {
        /// The provider.
        provider: String,
        /// The version.
        version: String,
    },
}

/// Why a user can't accept a route through a reserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptRefusal {
    /// The reserve has no declaration on record.
    NoDeclaration,
    /// The reserve's current declaration wasn't shown to this user.
    NotShown,
}

/// The reserve declarations on record, and which user has been shown which.
#[derive(Debug, Clone, Default)]
pub struct DeclarationLog {
    records: Vec<DeclarationRecord>,
    shown: HashSet<(String, DeclarationId)>,
}

impl DeclarationLog {
    /// Records the declaration in an advertisement's reserve offer, if it has one.
    ///
    /// - A declaration identical to the provider's current one is not recorded again, whatever its version.
    /// - A changed one in a new version is a new record, which users must be shown again.
    /// - A version's declaration, once recorded, can't be changed.
    ///
    /// A provider with no reserve offer declares nothing: `Ok(None)`.
    pub fn record(
        &mut self,
        ad: &Advertisement,
        at: Instant,
    ) -> Result<Option<RecordOutcome>, DeclarationError> {
        let Some(reserve) = &ad.offers.reserve else {
            return Ok(None);
        };
        let d = &reserve.declaration;
        let candidate = DeclarationRecord {
            id: DeclarationId(self.records.len() as u64),
            provider: ad.provider.clone(),
            version: d.declared_in_version.clone(),
            maker: d.maker.clone(),
            can_require_user_initiated_withdrawal: d.can_require_user_initiated_withdrawal,
            starts_routes_on_automatic_deposits: d.starts_routes_on_automatic_deposits,
            recorded_at: at,
        };
        let of_provider = || self.records.iter().filter(|r| r.provider == ad.provider);
        if let Some(same_version) = of_provider().find(|r| r.version == candidate.version) {
            return if same_version.same_content(&candidate) {
                Ok(Some(RecordOutcome::AlreadyRecorded(same_version.id)))
            } else {
                Err(DeclarationError::ChangedWithinVersion {
                    provider: ad.provider.clone(),
                    version: candidate.version,
                })
            };
        }
        if let Some(latest) = of_provider()
            .next_back()
            .filter(|r| r.same_content(&candidate))
        {
            return Ok(Some(RecordOutcome::AlreadyRecorded(latest.id)));
        }
        let id = candidate.id;
        self.records.push(candidate);
        Ok(Some(RecordOutcome::Recorded(id)))
    }

    /// A provider's declarations, oldest first.
    pub fn records_of(&self, provider: &str) -> Vec<&DeclarationRecord> {
        self.records
            .iter()
            .filter(|r| r.provider == provider)
            .collect()
    }

    /// Shows `user` the reserve's current declaration, and notes that they saw it.
    pub fn show_to(&mut self, user: &str, provider: &str) -> Option<&DeclarationRecord> {
        let latest = self.records.iter().rev().find(|r| r.provider == provider)?;
        self.shown.insert((user.to_owned(), latest.id));
        Some(latest)
    }

    /// Whether `user` may accept a route through the reserve: its current declaration was shown to them.
    pub fn accept(&self, user: &str, provider: &str) -> Result<(), AcceptRefusal> {
        let latest = self
            .records
            .iter()
            .rev()
            .find(|r| r.provider == provider)
            .ok_or(AcceptRefusal::NoDeclaration)?;
        if self.shown.contains(&(user.to_owned(), latest.id)) {
            Ok(())
        } else {
            Err(AcceptRefusal::NotShown)
        }
    }
}
