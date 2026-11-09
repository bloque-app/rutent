//! The snapshot the router reads (`T-012-03`): every tuple the catalogue may route through, each with its age and
//! origin, and a list of what was left out and why.

use rutent_contracts::provider_role::{Advertisement, EvidenceGrade};
use rutent_contracts::router_api::{
    CatalogueSnapshot, Figure, FigureValue, Id, Origin, Timestamp, Version, Violation,
};

use super::grant::{Endpoint, Instant, TupleKey, tuples_of};
use super::tuples::{TupleRefusal, check_tuple};
use super::{Catalogue, Unusable};

/// Why a tuple isn't in the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftOutReason {
    /// Its shape breaks its role's contract (`REQ-CAT-21`).
    Malformed(TupleRefusal),
    /// The grant or the operator's bound keeps it off routes (`REQ-CAT-1`, `REQ-CAT-2`).
    Unusable(Unusable),
    /// No admitted module version is recorded for its provider.
    NoAdmittedModule,
    /// An inner node on it is neither on the operator's public list nor declared private (`REQ-CAT-22`).
    NodeNotListed(String),
    /// An inner node on it has no holding account (`REQ-CAT-23`).
    NoHoldingAccount(String),
    /// The operator has set no evidence floor for the tuple (`REQ-ORD-39`).
    NoEvidenceFloor,
    /// The provider's declared evidence grade is below the tuple's floor, or it declares none (`REQ-ORD-39`).
    BelowEvidenceFloor {
        /// The floor: the weakest grade allowed.
        floor: EvidenceGrade,
        /// What the provider declares.
        declared: Option<EvidenceGrade>,
    },
    /// A name in it can't be written into the router's snapshot.
    NotRepresentable(&'static str),
}

/// A tuple kept out of the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    /// Its provider.
    pub provider: String,
    /// The tuple.
    pub tuple: TupleKey,
    /// Why.
    pub reason: LeftOutReason,
}

/// A snapshot and what stayed out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotBuild {
    /// What the router reads.
    pub snapshot: CatalogueSnapshot,
    /// Every advertised tuple that didn't make it, in provider and advertisement order.
    pub left_out: Vec<LeftOut>,
}

/// Why no snapshot could be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// The snapshot's identifier isn't an identifier.
    BadId,
    /// The time can't be written as a timestamp.
    TimeOutOfRange,
    /// The result breaks a rule of the router-api contract: a bug in the catalogue, never silent.
    Contract(Violation),
}

impl Catalogue {
    /// The snapshot the router reads, at `at` (whole seconds since the epoch; the caller's own clock).
    ///
    /// A tuple is in it when its provider has a grant in force covering it and the operator hasn't restricted it,
    /// a module version is recorded, every inner node on it is on the public list or declared private and has a
    /// holding account, and its provider's declared evidence grade meets the operator's floor for the tuple.
    /// Everything else is in [`SnapshotBuild::left_out`] with its reason. Each figure's age counts from when its
    /// advertisement was joined, and it states no validity: an advertisement holds until replaced.
    pub fn snapshot(&self, id: &str, at: Instant) -> Result<SnapshotBuild, SnapshotError> {
        let snapshot_id = Id::new(id).ok_or(SnapshotError::BadId)?;
        let taken_at = timestamp_of(at).ok_or(SnapshotError::TimeOutOfRange)?;
        let mut figures = Vec::new();
        let mut left_out = Vec::new();
        for (provider, joined) in &self.joined {
            let ad = &joined.advertisement;
            let age = at.0.saturating_sub(joined.stated_at.0);
            for (tuple, value) in values_of(ad) {
                let verdict = self.admit(provider, &tuple, at).and_then(|()| {
                    origin_of(
                        provider,
                        &ad.publisher,
                        &ad.version,
                        self.modules.get(provider),
                    )
                });
                match verdict {
                    Ok(origin) => figures.push(Figure {
                        figure_id: Id::new(format!("fig-{:04}", figures.len() + 1))
                            .expect("an identifier"),
                        origin,
                        age_seconds: age.try_into().map_err(|_| SnapshotError::TimeOutOfRange)?,
                        valid_for_seconds: None,
                        value,
                    }),
                    Err(reason) => left_out.push(LeftOut {
                        provider: provider.clone(),
                        tuple,
                        reason,
                    }),
                }
            }
        }
        let snapshot = CatalogueSnapshot {
            snapshot_id,
            taken_at,
            figures,
        };
        snapshot.check().map_err(SnapshotError::Contract)?;
        Ok(SnapshotBuild { snapshot, left_out })
    }

    fn admit(&self, provider: &str, tuple: &TupleKey, at: Instant) -> Result<(), LeftOutReason> {
        check_tuple(tuple).map_err(LeftOutReason::Malformed)?;
        self.usable(provider, tuple, at)
            .map_err(LeftOutReason::Unusable)?;
        if !self.modules.contains_key(provider) {
            return Err(LeftOutReason::NoAdmittedModule);
        }
        let publisher = self
            .publishers
            .publisher_of_provider(provider)
            .unwrap_or_default();
        for end in [&tuple.origin, &tuple.destination] {
            if let Endpoint::Inner(node) = end {
                // A private node is its declaring publisher's: another publisher's provider can't use it.
                let listed = self.nodes.is_public(node)
                    || self.publishers.publisher_of_node(node) == Some(publisher);
                if !listed {
                    return Err(LeftOutReason::NodeNotListed(node.clone()));
                }
                if self.nodes.holding(node).is_none() {
                    return Err(LeftOutReason::NoHoldingAccount(node.clone()));
                }
            }
        }
        let floor = *self
            .floors
            .get(tuple)
            .ok_or(LeftOutReason::NoEvidenceFloor)?;
        let declared = self
            .joined
            .get(provider)
            .and_then(|j| j.advertisement.declarations.evidence.as_ref())
            .map(|e| e.grade);
        if declared.is_none_or(|grade| grade > floor) {
            return Err(LeftOutReason::BelowEvidenceFloor { floor, declared });
        }
        Ok(())
    }
}

fn origin_of(
    provider: &str,
    publisher: &str,
    provider_version: &str,
    module_version: Option<&Version>,
) -> Result<Origin, LeftOutReason> {
    let bad = LeftOutReason::NotRepresentable;
    Ok(Origin::Provider {
        provider: Id::new(provider).ok_or(bad("a provider is an identifier"))?,
        publisher: Id::new(publisher).ok_or(bad("a publisher is an identifier"))?,
        provider_version: Version::new(provider_version)
            .ok_or(bad("a provider version is a version"))?,
        module_version: module_version
            .cloned()
            .ok_or(LeftOutReason::NoAdmittedModule)?,
    })
}

/// Every tuple of an advertisement with the figure value that carries it, in the order reserve, exchange, ramp.
fn values_of(ad: &Advertisement) -> Vec<(TupleKey, FigureValue)> {
    let mut values = Vec::new();
    if let Some(reserve) = &ad.offers.reserve {
        values.extend(
            reserve
                .tuples
                .iter()
                .map(|t| FigureValue::ReserveTuple { tuple: t.clone() }),
        );
    }
    if let Some(exchange) = &ad.offers.exchange {
        values.extend(
            exchange
                .tuples
                .iter()
                .map(|t| FigureValue::ExchangeTuple { tuple: t.clone() }),
        );
    }
    if let Some(ramp) = &ad.offers.ramp {
        values.extend(
            ramp.tuples
                .iter()
                .map(|t| FigureValue::RampTuple { tuple: t.clone() }),
        );
    }
    let keys = tuples_of(ad);
    assert_eq!(keys.len(), values.len(), "one figure value per tuple");
    keys.into_iter().zip(values).collect()
}

/// An instant as an RFC 3339 UTC timestamp.
fn timestamp_of(at: Instant) -> Option<Timestamp> {
    let days = i64::try_from(at.0 / 86_400).ok()?;
    let secs = at.0 % 86_400;
    // Days since 1970-01-01 to a civil date (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if !(0..=9_999).contains(&year) {
        return None;
    }
    Timestamp::new(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    ))
}
