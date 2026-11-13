//! A segment's charge against the formula that bound it, and a charge above it raised to the operator
//! (`REQ-QTE-5`, `AC-14`).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Delivery, Log, Operator};
use rutent_contracts::core_internal::{
    Actor, Difference, Entry, Outcome, Raise, Receipt, VerifiedEntry, What,
};
use rutent_contracts::router_api::{Amount, FeeFormula, Id, Timestamp, TupleKey};
use rutent_router::quote::check_charge;

use super::journal::{self, Step};
use super::terms::{InForce, Terms};
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
    /// The formula its route was priced on, the price figure its operation cites: what binds a tuple whose provider
    /// stated no terms before the segment started. Every operation of a route cites a price, so there is always one,
    /// and no charge goes unchecked for want of a formula.
    pub advertised: FeeFormula,
    /// The receipt of its result's stated entry, which a raise rests on (`charge-raise-rests-on-its-result`).
    pub result: Receipt,
}

/// What a charge came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// Within the formula that bound it.
    Within,
    /// Above it, raised to the operator.
    Flagged(Box<Flagged>),
    /// Above it, and raised already: the log holds the raise, from before this register was rebuilt
    /// ([`Charges::recorded`]). It isn't raised again, and its entry is the log's.
    AlreadyRaised,
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
    /// The charge names another provider than its tuple's.
    NotTheTuplesProvider,
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
    /// The charges the log holds a raise of, by order and segment: the result the raise rests on and the charge it
    /// names. A segment in it is raised already, whatever this register has seen.
    recorded: BTreeMap<(String, String), (Receipt, Amount)>,
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

    /// The register of a Core that stopped, rebuilt from the log's `entries`: each charge the log holds a raise of is
    /// raised already and is raised again by no check, so a restart that lost the register doesn't warn the operator
    /// twice of one charge (`REQ-QTE-5`). A raise whose entry the log never took isn't in it: it is raised afresh,
    /// and a raise delivered but not recorded before the stop is raised again (the gap this leaves, in the notes).
    pub fn recorded(entries: &[(Receipt, Entry)]) -> Self {
        let recorded: BTreeMap<(String, String), (Receipt, Amount)> = entries
            .iter()
            .filter_map(|(_, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what: What::Raise { raise },
                    ..
                }) => match &raise.difference {
                    Difference::Charge {
                        order,
                        segment,
                        charged,
                        result,
                        ..
                    } => Some((
                        (order.as_str().to_owned(), segment.as_str().to_owned()),
                        (result.clone(), charged.clone()),
                    )),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        Self {
            checked: BTreeMap::new(),
            recorded,
        }
    }

    /// What the check of `segment` of `order` came to, once it was recorded.
    pub fn result(&self, order: &Id, segment: &Id) -> Option<Checked> {
        let key = (order.as_str().to_owned(), segment.as_str().to_owned());
        match self.checked.get(&key) {
            Some((_, Kept::Within)) => Some(Checked::Within),
            Some((_, Kept::Raised(raised))) => raised.step.receipt.clone().map(|receipt| {
                Checked::Flagged(Box::new(Flagged {
                    raise: raised.raise.clone(),
                    delivered: raised.delivered,
                    receipt,
                }))
            }),
            None => self
                .recorded
                .contains_key(&key)
                .then_some(Checked::AlreadyRaised),
        }
    }

    /// Checks `charged` against the formula that bound its segment at its start: the terms its provider stated last
    /// at or before then, or else the formula advertised (`REQ-QTE-5`, `REQ-QTE-6`). Above it, the charge is raised
    /// to the operator as a `charge` difference resting on the result's stated entry and, where the provider's terms
    /// bound it, on their stated entry (`terms`: absent where the advertised price bound it), and the raise recorded,
    /// `done` where it reached the operator and `failed` where it didn't (`AC-14`).
    pub fn check(
        &mut self,
        desk: ChargeDesk<'_>,
        charged: &Charged,
    ) -> Result<Checked, ChargeError> {
        if charged.provider != charged.tuple.provider {
            return Err(ChargeError::NotTheTuplesProvider);
        }
        let key = (
            charged.order.as_str().to_owned(),
            charged.segment.as_str().to_owned(),
        );
        let seen: Seen = (
            charged.result.clone(),
            charged.amount_in.clone(),
            charged.charged.clone(),
        );
        if let Some((result, amount)) = self.recorded.get(&key) {
            return if result == &charged.result && amount == &charged.charged {
                Ok(Checked::AlreadyRaised)
            } else {
                Err(ChargeError::AnotherCharge)
            };
        }
        if let Some((first, _)) = self.checked.get(&key)
            && first != &seen
        {
            return Err(ChargeError::AnotherCharge);
        }
        if !self.checked.contains_key(&key) {
            // The formula that bound the segment: the terms stated last at or before its start, or else the one its
            // route was priced on; a later statement never applies to it (`REQ-QTE-6`).
            let (formula, terms) = match desk
                .terms
                .in_force(&charged.tuple, Some(&charged.started_at))
            {
                InForce::Binds { terms, receipt } => (&terms.fee, Some(receipt)),
                InForce::Unstated | InForce::Quote => (&charged.advertised, None),
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
                            result: charged.result.clone(),
                            terms: terms.cloned(),
                        },
                        cites: std::iter::once(charged.result.clone())
                            .chain(terms.cloned())
                            .collect(),
                        at: journal::now(desk.clock)?,
                    };
                    raise
                        .check()
                        .map_err(|v| ChargeError::Append(AppendFailure::Refused(v)))?;
                    // Both entries it may need are built before it is raised, so nothing that fails after the raise
                    // can have it raised again; whether it arrived is what its entry records (`T-023-02` retries it).
                    let entry = |outcome| {
                        journal::built(
                            desk.core,
                            What::Raise {
                                raise: raise.clone(),
                            },
                            &raise.at,
                            outcome,
                        )
                    };
                    let (done, failed) = (entry(Outcome::Done {})?, entry(Outcome::Failed {})?);
                    let delivered = matches!(desk.operator.raise(&raise), Delivery::Delivered);
                    Kept::Raised(Box::new(Raised {
                        raise,
                        delivered,
                        step: Step::new(if delivered { done } else { failed }),
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
