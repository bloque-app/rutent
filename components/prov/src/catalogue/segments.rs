//! What the catalogue says of a segment: dry runs, refusals, a true undo, where the value is after a failure
//! (`REQ-CAT-14`, `REQ-CAT-15`, `REQ-CAT-18`, `INV-2`).

use rutent_contracts::provider_role::{
    Category, ConversionOutcome, Operation, Refusal, ReserveMovementOutcome, ResultBody,
    ReversalOutcome, TransferOutcome,
};

use super::{Catalogue, Endpoint, Instant, Standing, TupleKey};

/// A refusal's class (`REQ-CAT-18`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalClass {
    /// It lifts at a known time.
    LiftsAtKnownTime {
        /// The time, as the provider stated it.
        lifts_at: String,
    },
    /// It waits on the person.
    WaitsOnPerson,
    /// It is final.
    Final,
}

impl RefusalClass {
    /// Whether the refusal never lifts.
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Final)
    }
}

impl From<&Refusal> for RefusalClass {
    fn from(refusal: &Refusal) -> Self {
        match refusal {
            Refusal::LiftsAtKnownTime(l) => Self::LiftsAtKnownTime {
                lifts_at: l.lifts_at.clone(),
            },
            Refusal::WaitsOnPerson(_) => Self::WaitsOnPerson,
            Refusal::Final(_) => Self::Final,
        }
    }
}

/// What a provider's answer to a dry run means for the option it was asked about (`REQ-CAT-14`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DryRunAssessment {
    /// The provider would serve it.
    Serves,
    /// The provider rejected it, with its class.
    Rejected(RefusalClass),
    /// It neither served nor refused: failed or unknown.
    NoAnswer,
}

impl DryRunAssessment {
    /// Whether the option comes off the route being priced.
    pub fn removes_option(&self) -> bool {
        !matches!(self, Self::Serves)
    }
}

/// Why a result couldn't be read as a dry run's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRunError {
    /// The operation isn't a dry run (or isn't a movement).
    NotADryRun,
    /// The result is of another kind or names another transaction (`REQ-ORD-1`).
    AnswersAnotherOperation,
    /// A dry run was answered as if it had been carried out.
    NotADryRunAnswer,
}

/// What a segment's result did to the value at the segment's origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentEffect {
    /// Nothing was taken: the value stays at the origin in full, in the asset it is in (`INV-2`).
    NothingTaken {
        /// The origin node.
        node: Endpoint,
        /// The asset at it.
        asset: String,
    },
    /// The segment took this much in, the charge included.
    Taken {
        /// The origin node.
        node: Endpoint,
        /// The asset taken.
        asset: String,
        /// How much, in the origin unit.
        amount_in: String,
    },
    /// The outcome is unknown: the value is not claimed to be anywhere.
    Unresolved,
}

/// Why a segment's effect couldn't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectError {
    /// The operation moves no value through a segment.
    NotAMovement,
    /// The result is of another kind or names another transaction.
    AnswersAnotherOperation,
}

/// How one outcome reads, whatever the kind of operation.
enum Read<'a> {
    Taken(&'a str),
    NotTaken,
    Unknown,
}

struct Moved<'a> {
    transaction: &'a str,
    origin: Endpoint,
    asset: &'a str,
    dry_run: bool,
}

fn moved(operation: &Operation) -> Option<Moved<'_>> {
    Some(match operation {
        Operation::Withdrawal(o) => Moved {
            transaction: &o.transaction,
            origin: Endpoint::Outer(o.origin.network.clone()),
            asset: &o.asset,
            dry_run: o.dry_run,
        },
        Operation::Deposit(o) => Moved {
            transaction: &o.transaction,
            origin: Endpoint::Inner(o.origin.clone()),
            asset: &o.asset,
            dry_run: o.dry_run,
        },
        Operation::Transfer(o) => Moved {
            transaction: &o.transaction,
            origin: Endpoint::Inner(o.origin.clone()),
            asset: &o.asset,
            dry_run: o.dry_run,
        },
        Operation::Conversion(o) => Moved {
            transaction: &o.transaction,
            origin: Endpoint::Inner(o.node.clone()),
            asset: &o.from,
            dry_run: o.dry_run,
        },
        _ => return None,
    })
}

/// A result's outcome: its transaction, and how it reads. A dry-run acceptance and a refusal both take nothing.
fn read(result: &ResultBody) -> Option<(&str, Read<'_>, Option<RefusalClass>, bool)> {
    use ReserveMovementOutcome as R;
    Some(match result {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match &r.outcome {
            R::Succeeded(s) => (&r.transaction, Read::Taken(&s.amount_in), None, false),
            R::Failed(_) => (&r.transaction, Read::NotTaken, None, false),
            R::Unknown(_) => (&r.transaction, Read::Unknown, None, false),
            R::Refused(x) => (
                &r.transaction,
                Read::NotTaken,
                Some((&x.refusal).into()),
                false,
            ),
            R::DryRunAccepted(_) => (&r.transaction, Read::NotTaken, None, true),
        },
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::Succeeded(s) => {
                (&r.transaction, Read::Taken(&s.amount_in), None, false)
            }
            TransferOutcome::Failed(_) => (&r.transaction, Read::NotTaken, None, false),
            TransferOutcome::Unknown(_) => (&r.transaction, Read::Unknown, None, false),
            TransferOutcome::Refused(x) => (
                &r.transaction,
                Read::NotTaken,
                Some((&x.refusal).into()),
                false,
            ),
            TransferOutcome::DryRunAccepted(_) => (&r.transaction, Read::NotTaken, None, true),
        },
        ResultBody::Conversion(r) => match &r.outcome {
            ConversionOutcome::Succeeded(s) => {
                (&r.transaction, Read::Taken(&s.amount_in), None, false)
            }
            ConversionOutcome::Failed(_) => (&r.transaction, Read::NotTaken, None, false),
            ConversionOutcome::Unknown(_) => (&r.transaction, Read::Unknown, None, false),
            ConversionOutcome::Refused(x) => (
                &r.transaction,
                Read::NotTaken,
                Some((&x.refusal).into()),
                false,
            ),
            ConversionOutcome::DryRunAccepted(_) => (&r.transaction, Read::NotTaken, None, true),
        },
        _ => return None,
    })
}

/// Where the value is after a segment's result: a failed, refused or dry-run result takes nothing, so the value
/// stays at the segment's origin in full, whatever the direction the segment was priced in (`INV-2`); a succeeded
/// one took its `amount_in`; an unknown one is not read as either.
pub fn segment_effect(
    operation: &Operation,
    result: &ResultBody,
) -> Result<SegmentEffect, EffectError> {
    let segment = moved(operation).ok_or(EffectError::NotAMovement)?;
    let (transaction, outcome, _, _) = read(result).ok_or(EffectError::AnswersAnotherOperation)?;
    if transaction != segment.transaction {
        return Err(EffectError::AnswersAnotherOperation);
    }
    Ok(match outcome {
        Read::Taken(amount) => SegmentEffect::Taken {
            node: segment.origin,
            asset: segment.asset.to_owned(),
            amount_in: amount.to_owned(),
        },
        Read::NotTaken => SegmentEffect::NothingTaken {
            node: segment.origin,
            asset: segment.asset.to_owned(),
        },
        Read::Unknown => SegmentEffect::Unresolved,
    })
}

/// Whether a reversal counts as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoVerdict {
    /// The value is back at the segment's origin, in full and in the asset it left in (`REQ-CAT-15`).
    TrueUndo,
    /// Anything else: the reversal stays owed (`REQ-CAT-7`).
    NotUndone,
}

/// Judges a reversal's result against the reversal asked: only a succeeded result naming the same node, asset and
/// amount counts. The provider's word alone completes nothing further (`REQ-ORD-3`); that is the order's.
pub fn judge_reversal(operation: &Operation, result: &ResultBody) -> UndoVerdict {
    match (operation, result) {
        (Operation::Reversal(o), ResultBody::Reversal(r)) if o.transaction == r.transaction => {
            match &r.outcome {
                ReversalOutcome::Succeeded(s)
                    if s.node == o.node && s.asset == o.asset && s.amount == o.amount =>
                {
                    UndoVerdict::TrueUndo
                }
                _ => UndoVerdict::NotUndone,
            }
        }
        _ => UndoVerdict::NotUndone,
    }
}

/// Why a segment was declined for want of a true undo (`REQ-CAT-15`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentDecline {
    /// No reversal ceiling stands behind the provider: no grant in force, or a ceiling of nothing.
    NoReversalCapacity,
    /// The amount is over the ceiling, in the bond's own asset.
    OverReversalCeiling {
        /// The ceiling in force.
        ceiling: u64,
    },
}

impl Catalogue {
    /// Reads a provider's answer to a dry run (`REQ-CAT-14`). It takes `&self`: a dry run changes nothing here
    /// either. A rejected one removes the option it was asked about.
    pub fn assess_dry_run(
        &self,
        operation: &Operation,
        result: &ResultBody,
    ) -> Result<DryRunAssessment, DryRunError> {
        let segment = moved(operation)
            .filter(|s| s.dry_run)
            .ok_or(DryRunError::NotADryRun)?;
        let (transaction, outcome, refusal, accepted) =
            read(result).ok_or(DryRunError::AnswersAnotherOperation)?;
        if transaction != segment.transaction {
            return Err(DryRunError::AnswersAnotherOperation);
        }
        match (outcome, refusal, accepted) {
            (_, _, true) => Ok(DryRunAssessment::Serves),
            (_, Some(class), _) => Ok(DryRunAssessment::Rejected(class)),
            (Read::Taken(_), _, _) => Err(DryRunError::NotADryRunAnswer),
            _ => Ok(DryRunAssessment::NoAnswer),
        }
    }

    /// Whether an exchange's or ramp's segment of `amount` (in the tuple's input unit) can be truly undone, so it
    /// may be sent (`REQ-CAT-15`). The undo is backed by the grant's reversal ceiling (`DEC-11`): none in force, or
    /// an amount over it in the bond's own asset, declines it. A ceiling is in the bond's asset, so an amount in
    /// another asset can't be compared and passes the amount check. A reserve is never declined (`ASM-4`).
    pub fn admit_segment(
        &self,
        provider: &str,
        tuple: &TupleKey,
        amount: u128,
        at: Instant,
    ) -> Result<(), SegmentDecline> {
        if tuple.category == Category::Reserve {
            return Ok(());
        }
        let mut best: Option<SegmentDecline> = None;
        for grant in self.grants_of_provider(provider) {
            if grant.standing(at) != Standing::InForce {
                continue;
            }
            let capacity = grant.reversal_capacity();
            let ceiling = capacity.ceiling();
            if ceiling == 0 {
                continue;
            }
            if capacity.bond().asset == tuple.asset && amount > u128::from(ceiling) {
                best = Some(match best {
                    Some(SegmentDecline::OverReversalCeiling { ceiling: c }) => {
                        SegmentDecline::OverReversalCeiling {
                            ceiling: c.max(ceiling),
                        }
                    }
                    _ => SegmentDecline::OverReversalCeiling { ceiling },
                });
                continue;
            }
            return Ok(());
        }
        Err(best.unwrap_or(SegmentDecline::NoReversalCapacity))
    }
}
