//! Reversal capacity (`DEC-11`, ADR-0010) and the reversals providers owe (`REQ-CAT-7`).

use std::collections::BTreeMap;

use super::grant::Instant;

/// A bond held by a third party, which a provider's ceiling follows (ADR-0010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bond {
    /// The asset the bond and the ceiling are counted in.
    pub asset: String,
    /// The bond's size, in the asset's smallest unit.
    pub amount: u64,
    /// The third party holding it.
    pub held_by: String,
}

/// Why a reversal-capacity rule refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReversalError {
    /// The ceiling is above the bond (`DEC-11`).
    CeilingAboveBond {
        /// The ceiling asked for.
        ceiling: u64,
        /// The bond.
        bond: u64,
    },
    /// The bond names no holder, or the check names no one who made it.
    NotAttributable,
}

/// A grant's reversal-capacity check, recorded: the bond, the ceiling sized to it, and who checked when.
///
/// The ceiling can never pass the bond: it is checked when built, bounded when set, and follows the bond down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReversalCapacity {
    bond: Bond,
    ceiling: u64,
    checked_by: String,
    checked_at: Instant,
}

impl ReversalCapacity {
    /// Records a check of `bond` with `ceiling` sized to it.
    pub fn check(
        bond: Bond,
        ceiling: u64,
        checked_by: &str,
        checked_at: Instant,
    ) -> Result<Self, ReversalError> {
        if bond.held_by.trim().is_empty() || checked_by.trim().is_empty() {
            return Err(ReversalError::NotAttributable);
        }
        if ceiling > bond.amount {
            return Err(ReversalError::CeilingAboveBond {
                ceiling,
                bond: bond.amount,
            });
        }
        Ok(Self {
            bond,
            ceiling,
            checked_by: checked_by.to_owned(),
            checked_at,
        })
    }

    /// The bond.
    pub fn bond(&self) -> &Bond {
        &self.bond
    }

    /// The ceiling on value exposed to the provider's reversals, in the bond's asset.
    pub fn ceiling(&self) -> u64 {
        self.ceiling
    }

    /// Who made the check.
    pub fn checked_by(&self) -> &str {
        &self.checked_by
    }

    /// When the check was made.
    pub fn checked_at(&self) -> Instant {
        self.checked_at
    }

    /// Sets the ceiling, refusing one above the bond.
    pub fn set_ceiling(&mut self, ceiling: u64) -> Result<(), ReversalError> {
        if ceiling > self.bond.amount {
            return Err(ReversalError::CeilingAboveBond {
                ceiling,
                bond: self.bond.amount,
            });
        }
        self.ceiling = ceiling;
        Ok(())
    }

    /// Records a new check of a changed bond. The ceiling follows the bond down; a bigger bond doesn't raise it,
    /// and a bond in another asset leaves no comparable ceiling, so the ceiling falls to zero.
    ///
    /// Refused, changing nothing, if the bond names no holder or the check names no one who made it.
    pub fn rebond(
        &mut self,
        bond: Bond,
        checked_by: &str,
        at: Instant,
    ) -> Result<(), ReversalError> {
        if bond.held_by.trim().is_empty() || checked_by.trim().is_empty() {
            return Err(ReversalError::NotAttributable);
        }
        self.ceiling = if bond.asset == self.bond.asset {
            self.ceiling.min(bond.amount)
        } else {
            0
        };
        self.bond = bond;
        self.checked_by = checked_by.to_owned();
        self.checked_at = at;
        Ok(())
    }
}

/// What the catalogue sees of a reversal it is owed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    /// The provider has said nothing since this moment.
    Silence {
        /// Since when.
        since: Instant,
    },
    /// The provider lost its standing.
    StandingLost,
    /// The reversal was confirmed (ADR-0008): the only thing that discharges it.
    Confirmed,
}

/// A reversal a provider owes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owed {
    /// The reversal's identifier.
    pub id: String,
    /// The provider that owes it.
    pub provider: String,
    /// The asset.
    pub asset: String,
    /// The amount, in the asset's smallest unit.
    pub amount: u64,
    discharged: bool,
}

/// Why the list of owed reversals refused a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwedError {
    /// A reversal with this identifier is recorded already.
    DuplicateOwed(String),
    /// No reversal has this identifier.
    UnknownOwed(String),
}

/// The reversals providers owe. Silence, lost standing, expiry and revocation never discharge one
/// (`REQ-CAT-7`); a confirmation does.
#[derive(Debug, Clone, Default)]
pub struct OwedReversals(BTreeMap<String, Owed>);

impl OwedReversals {
    /// Records a reversal owed. An identifier is never reused.
    pub fn owe(
        &mut self,
        id: &str,
        provider: &str,
        asset: &str,
        amount: u64,
    ) -> Result<(), OwedError> {
        if self.0.contains_key(id) {
            return Err(OwedError::DuplicateOwed(id.to_owned()));
        }
        self.0.insert(
            id.to_owned(),
            Owed {
                id: id.to_owned(),
                provider: provider.to_owned(),
                asset: asset.to_owned(),
                amount,
                discharged: false,
            },
        );
        Ok(())
    }

    /// Takes note of what was seen of a reversal. Only [`Observation::Confirmed`] changes it.
    pub fn observe(&mut self, id: &str, observation: Observation) -> Result<(), OwedError> {
        let owed = self
            .0
            .get_mut(id)
            .ok_or_else(|| OwedError::UnknownOwed(id.to_owned()))?;
        if observation == Observation::Confirmed {
            owed.discharged = true;
        }
        Ok(())
    }

    /// Whether the reversal is still owed.
    pub fn is_owed(&self, id: &str) -> bool {
        self.0.get(id).is_some_and(|o| !o.discharged)
    }

    /// The reversals a provider still owes.
    pub fn owed_by(&self, provider: &str) -> Vec<&Owed> {
        self.0
            .values()
            .filter(|o| o.provider == provider && !o.discharged)
            .collect()
    }
}
