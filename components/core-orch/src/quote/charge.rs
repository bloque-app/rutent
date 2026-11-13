//! A segment's charge against the formula that bound it, and a charge above it raised to the operator
//! (`REQ-QTE-5`, `AC-14`).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Delivery, Log, Operator};
use rutent_contracts::core_internal::{Actor, Difference, Outcome, Raise, Receipt, What};
use rutent_contracts::router_api::{Amount, FeeFormula, Id, Timestamp, TupleKey};
use rutent_router::quote::{Binding, check_charge};

use super::journal::{self, Step};
use super::terms::Terms;
use crate::rounds::Fresh;
use crate::snapshot::Clock;

/// One segment's charge, as its result that counted states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charged {
    /// The order.
    pub order: Id,
    /// The segment.
    pub segment: Id,
    /// Its provider.
    pub provider: Id,
    /// Its tuple, by its provider and ends: the charge is in its `from` asset.
    pub tuple: TupleKey,
    /// When it started: the terms stated last at or before then bind it (`REQ-QTE-6`).
    pub started_at: Timestamp,
    /// What it took in.
    pub amount_in: Amount,
    /// What its provider charged.
    pub charged: Amount,
    /// The formula its route was priced on, the price figure its operation cites, for a tuple whose provider
    /// stated no terms before the segment started.
    pub advertised: Option<FeeFormula>,
    /// The receipt of its result's stated entry, which a raise rests on.
    pub result: Receipt,
}

/// What a charge came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// Within the formula that bound it.
    Within,
    /// Above it, raised to the operator.
    Flagged(Box<Flagged>),
}

/// A charge above its formula, raised to the operator and recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flagged {
    /// The raise.
    pub raise: Raise,
    /// Whether it reached the operator.
    pub delivered: bool,
    /// The receipt of its `raise` entry.
    pub receipt: Receipt,
}

/// Why a charge couldn't be checked or its raise recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChargeError {
    /// No formula binds the segment: no terms stated before it started and none advertised.
    NoFormula,
    /// The segment was checked already on another result, or another charge: a segment has one result that counts,
    /// so the second is refused, never taken as the first's answer.
    AnotherCharge,
    /// The log didn't take the raise's entry, or Core's clock couldn't be read; a raise made is kept and its entry
    /// appended as built on the next check, never raised again.
    Append(AppendFailure),
}

impl From<AppendFailure> for ChargeError {
    fn from(failure: AppendFailure) -> Self {
        ChargeError::Append(failure)
    }
}

/// What a charge check reaches.
pub struct ChargeDesk<'a> {
    /// The history of stated terms.
    pub terms: &'a Terms,
    /// The log, through orchestration's handle.
    pub log: &'a mut dyn Log,
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// Fresh identifiers, for each raise.
    pub fresh: &'a mut dyn Fresh,
    /// The operator's channel.
    pub operator: &'a mut dyn Operator,
    /// Core's actor, who records the raise.
    pub core: &'a Actor,
}

/// Core's record of the charges checked: each segment's checked once, each raise made once and its entry built
/// once, appended until the log takes it.
#[derive(Debug, Clone, Default)]
pub struct Charges {
    checked: BTreeMap<(String, String), (Seen, Kept)>,
}

/// What a segment's check was made on: its result, what it took in and what it was charged.
type Seen = (Receipt, Amount, Amount);

#[derive(Debug, Clone)]
enum Kept {
    Within,
    Raised(Box<Raised>),
}

/// A raise made, whether it reached the operator, and its entry, built once.
#[derive(Debug, Clone)]
struct Raised {
    raise: Raise,
    delivered: bool,
    step: Step,
}

impl Charges {
    /// No charge checked yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks `charged` against the formula that bound its segment at its start: the terms its provider stated last
    /// at or before then, or else the formula advertised (`REQ-QTE-5`, `REQ-QTE-6`). Above it, the charge is raised
    /// to the operator as a `charge` difference resting on the result's stated entry, and the raise recorded, `done`
    /// where it reached the operator and `failed` where it didn't (`AC-14`).
    pub fn check(
        &mut self,
        desk: ChargeDesk<'_>,
        charged: &Charged,
    ) -> Result<Checked, ChargeError> {
        let key = (
            charged.order.as_str().to_owned(),
            charged.segment.as_str().to_owned(),
        );
        let seen: Seen = (
            charged.result.clone(),
            charged.amount_in.clone(),
            charged.charged.clone(),
        );
        if let Some((first, _)) = self.checked.get(&key)
            && first != &seen
        {
            return Err(ChargeError::AnotherCharge);
        }
        if !self.checked.contains_key(&key) {
            // The formula that bound the segment: the terms stated last at or before its start, or else the one its
            // route was priced on; a later statement never applies to it (`REQ-QTE-6`).
            let formula = match desk
                .terms
                .binding(&charged.tuple, Some(&charged.started_at))
            {
                Binding::Binds(terms) => &terms.fee,
                Binding::Unstated | Binding::Quote => {
                    charged.advertised.as_ref().ok_or(ChargeError::NoFormula)?
                }
            };
            let kept = match check_charge(formula, &charged.amount_in, &charged.charged) {
                Ok(()) => Kept::Within,
                Err(flag) => {
                    let raise = Raise {
                        raise: desk.fresh.id(),
                        difference: Difference::Charge {
                            order: charged.order.clone(),
                            segment: charged.segment.clone(),
                            provider: charged.provider.clone(),
                            charged: flag.charged,
                            maximum: flag.bound,
                            asset: charged.tuple.from.asset.clone(),
                        },
                        cites: vec![charged.result.clone()],
                        at: journal::now(desk.clock)?,
                    };
                    raise
                        .check()
                        .map_err(|v| ChargeError::Append(AppendFailure::Refused(v)))?;
                    // Raised once; whether it arrived is what its entry records (`T-023-02` retries it).
                    let delivered = matches!(desk.operator.raise(&raise), Delivery::Delivered);
                    let outcome = if delivered {
                        Outcome::Done {}
                    } else {
                        Outcome::Failed {}
                    };
                    let entry = journal::built(
                        desk.core,
                        What::Raise {
                            raise: raise.clone(),
                        },
                        &raise.at,
                        outcome,
                    )?;
                    Kept::Raised(Box::new(Raised {
                        raise,
                        delivered,
                        step: Step::new(entry),
                    }))
                }
            };
            self.checked.insert(key.clone(), (seen, kept));
        }
        match self.checked.get_mut(&key) {
            Some((_, Kept::Raised(raised))) => {
                let receipt = raised.step.append(desk.log)?;
                Ok(Checked::Flagged(Box::new(Flagged {
                    raise: raised.raise.clone(),
                    delivered: raised.delivered,
                    receipt,
                })))
            }
            _ => Ok(Checked::Within),
        }
    }
}
