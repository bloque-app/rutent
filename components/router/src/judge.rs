//! Why a path is dropped before ranking (`REQ-AUD-4`): the first of these that holds, in this order.
//!
//! Excluded by policy; a void statement; a figure past its validity (`REQ-QTE-4`); a refused dry run; a
//! judgement of no; an operation outside its band or its price's levels, judged on what enters it after the fees
//! before it, in its own unit (`REQ-QTE-12`); over a ceiling, counting what its operations produce and the
//! exposure already in flight (`REQ-ORD-8`); arriving with nothing (`REQ-QTE-13`); below the floor; a rate cycle
//! implying free profit (ADR-0006); and, in a choice round, an operation no answer confirmed.

use num_bigint::BigUint;
use rutent_contracts::provider_role::{Denomination, JudgementAnswer};
use rutent_contracts::router_api::{
    CeilingScope, DropReason, Endpoint, Figure, FigureValue, Id, Operation,
};

use crate::price::{deepest, whole};
use crate::search::{Path, Search, refuses};

fn amount_of(text: &str) -> BigUint {
    whole(text).unwrap_or_default()
}

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

impl Search<'_> {
    /// Why `path` is dropped before ranking, or `None` if it may be ranked.
    pub(crate) fn fault(&self, path: &Path) -> Option<DropReason> {
        if path.beyond_levels {
            return Some(DropReason::BeyondLevels {
                part: 0,
                operation: index(path.steps.len() - 1),
            });
        }
        let operations: Vec<&Operation> = path.steps.iter().map(|s| &s.operation).collect();
        let cited: Vec<&Figure> = operations
            .iter()
            .flat_map(|o| o.figures.iter())
            .filter_map(|id| self.figure(id.as_str()))
            .collect();
        if operations.iter().any(|o| self.is_excluded(o)) {
            return Some(DropReason::ExcludedByPolicy {});
        }
        if let Some(f) = cited.iter().find(|f| self.is_void(f)) {
            return Some(DropReason::VoidStatement {
                figure: f.figure_id.clone(),
            });
        }
        if let Some(f) = cited.iter().find(|f| !f.is_within_validity()) {
            return Some(DropReason::StaleFigure {
                figure: f.figure_id.clone(),
            });
        }
        if let Some(f) = cited.iter().find(|f| {
            matches!(&f.value, FigureValue::DryRun { result, .. } if refuses(&result.statement))
        }) {
            return Some(DropReason::DryRunRefused {
                figure: f.figure_id.clone(),
            });
        }
        if let Some(f) = cited.iter().find(|f| {
            matches!(&f.value, FigureValue::Eligibility { judgement, .. }
                if judgement.statement.answer == JudgementAnswer::No)
        }) {
            return Some(DropReason::Ineligible {
                figure: f.figure_id.clone(),
            });
        }
        for (o, &s) in path.segments.iter().enumerate() {
            if !self.within_band(s, operations[o]) {
                return Some(DropReason::OutsideBand {
                    part: 0,
                    operation: index(o),
                });
            }
        }
        if let Some(reason) = self.over_ceiling(&operations) {
            return Some(reason);
        }
        let arrives = operations
            .last()
            .map(|o| amount_of(o.amount_out.as_str()))
            .unwrap_or_default();
        if arrives == BigUint::ZERO {
            return Some(DropReason::NoPositiveOutcome {});
        }
        if let Some(floor) = &self.request.floor
            && arrives < amount_of(floor.as_str())
        {
            return Some(DropReason::BelowFloor {});
        }
        if follows_a_profitable_cycle(&operations) {
            return Some(DropReason::SuspectRateCycle {});
        }
        if self.is_choice()
            && let Some(o) = path.steps.iter().position(|s| !s.confirmed)
        {
            return Some(DropReason::Unconfirmed {
                part: 0,
                operation: index(o),
            });
        }
        None
    }

    fn is_excluded(&self, operation: &Operation) -> bool {
        self.bounds.excluded_providers.contains(&operation.provider)
            || self.bounds.excluded_tuples.iter().any(|t| {
                t.provider == operation.provider && t.from == operation.from && t.to == operation.to
            })
    }

    /// Whether an operation's amount in is within one of its tuple's bands, in the tuple's own input unit, and
    /// its net, after its fee, within its price's levels (`REQ-QTE-12`).
    fn within_band(&self, segment: usize, operation: &Operation) -> bool {
        let segment = &self.segments[segment];
        let amount_in = amount_of(operation.amount_in.amount().as_str());
        let fee = amount_of(&operation.fee.amount);
        let net = if fee <= amount_in {
            &amount_in - fee
        } else {
            BigUint::ZERO
        };
        segment.serves(&amount_in) && deepest(segment.rate).is_none_or(|last| net <= last)
    }

    fn publisher_of(&self, provider: &Id) -> Option<&Id> {
        self.segments
            .iter()
            .find(|s| s.provider == provider)
            .and_then(|s| s.tuple.origin.publisher())
    }

    /// The exposure already in flight in `asset` at the parties `at` selects, from the operator's records.
    fn in_flight(&self, at: impl Fn(&Id, &Id) -> bool, asset: &Denomination) -> BigUint {
        self.exposures()
            .filter_map(|f| match &f.value {
                FigureValue::Exposure {
                    provider,
                    publisher,
                    asset: held,
                    in_flight,
                } if at(provider, publisher) && held == asset => {
                    Some(amount_of(in_flight.as_str()))
                }
                _ => None,
            })
            .sum()
    }

    /// The first ceiling the operations would pass, with what is already in flight there (`REQ-ORD-8`). Value at
    /// a party, in a ceiling's asset, is what its operations take in of that asset, or else what they produce of
    /// it, so value an exchange makes counts too.
    fn over_ceiling(&self, operations: &[&Operation]) -> Option<DropReason> {
        let placed = |o: &Operation, asset: &Denomination| {
            if &o.from.asset == asset {
                amount_of(o.amount_in.amount().as_str())
            } else if &o.to.asset == asset {
                amount_of(o.amount_out.as_str())
            } else {
                BigUint::ZERO
            }
        };
        for ceiling in &self.bounds.provider_ceilings {
            let value: BigUint = operations
                .iter()
                .filter(|o| o.provider == ceiling.provider)
                .map(|o| placed(o, &ceiling.asset))
                .sum();
            if value > BigUint::ZERO
                && value + self.in_flight(|p, _| p == &ceiling.provider, &ceiling.asset)
                    > amount_of(ceiling.amount.as_str())
            {
                return Some(DropReason::OverCeiling {
                    scope: CeilingScope::Provider,
                    party: ceiling.provider.clone(),
                    asset: ceiling.asset.clone(),
                });
            }
        }
        for ceiling in &self.bounds.publisher_ceilings {
            let value: BigUint = operations
                .iter()
                .filter(|o| self.publisher_of(&o.provider) == Some(&ceiling.publisher))
                .map(|o| placed(o, &ceiling.asset))
                .sum();
            if value > BigUint::ZERO
                && value + self.in_flight(|_, q| q == &ceiling.publisher, &ceiling.asset)
                    > amount_of(ceiling.amount.as_str())
            {
                return Some(DropReason::OverCeiling {
                    scope: CeilingScope::Publisher,
                    party: ceiling.publisher.clone(),
                    asset: ceiling.asset.clone(),
                });
            }
        }
        None
    }
}

/// Whether the path comes back to a node and asset it held value at with more than it held there: a rate cycle
/// implying free profit, possibly stale or false, never followed (ADR-0006). The two holdings may be counted in
/// different units of the asset, so `a` units of `10^-da` are compared with `b` units of `10^-db` as `a × 10^db`
/// against `b × 10^da`.
fn follows_a_profitable_cycle(operations: &[&Operation]) -> bool {
    let Some(first) = operations.first() else {
        return false;
    };
    let ten = |d: u8| BigUint::from(10u8).pow(u32::from(d));
    let mut held: Vec<(&Endpoint, BigUint)> =
        vec![(&first.from, amount_of(first.amount_in.amount().as_str()))];
    for operation in operations {
        let now = amount_of(operation.amount_out.as_str());
        let at = &operation.to;
        if held.iter().any(|(was, then)| {
            was.node == at.node
                && was.asset.id == at.asset.id
                && &now * ten(was.asset.decimals) > then * ten(at.asset.decimals)
        }) {
            return true;
        }
        held.push((at, now));
    }
    false
}
