//! A mismatch at a non-final public end, recorded and raised (`T-020-22`; `REQ-ORD-33`, `REQ-ORD-9`, `REQ-AUD-1`,
//! ADR-0009).
//!
//! A non-final segment whose public record contradicts what its provider said it moved settles on its provider's
//! declared grade as it always did, since only a part's final deposit is held to a public record. What was dropped
//! without a trace is the contradiction itself. Grading now answers it as a `public_mismatch` entry among the entries
//! the segment settles on, resting on the provider's result, and this module raises it: a `public_end` difference
//! citing that entry, to the operator, which can move nothing (ADR-0009), and the raise recorded, `done` where it
//! reached the operator and `failed` where it didn't. Nothing else follows from it: no input, no state change, no
//! effect.
//!
//! The raise needs the receipt of the mismatch's entry, which only the segments' append makes known, so the grading
//! is told the receipts ([`Grading::recorded`](crate::segments::Grading::recorded)) and asked to raise on every run
//! of a part ([`Grading::raise`](crate::segments::Grading::raise)): a raise whose record the log couldn't take is
//! recorded when it can, as built, and none is raised twice. After a restart, [`Unraised::rebuilt`] reads the log:
//! a mismatch it holds with no raise citing it is raised, and one raised is not.

use rutent_contracts::core_internal::calls::{AppendFailure, Delivery, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Actor, Difference, Entry, Outcome, PublicEndMismatch, PublicFigures, Raise, Receipt,
    TimeSource, VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::provider_role::{NodeRef, OuterNode};
use rutent_contracts::router_api::{Id, Operation};
use rutent_evidence::{Expected, Movement, Unmatched};

use crate::segments::RaiseDesk;

/// The figure of a movement's check that differs, as the log records it: only a contradiction of what the provider
/// said (`WrongDestination`, `WrongAsset`, `WrongAmount`) is one; a movement the network can't yet vouch for
/// (unknown, not final, reorged out) or one on another network contradicts nothing.
pub(super) fn found(why: Option<Unmatched>) -> Option<PublicEndMismatch> {
    match why? {
        Unmatched::WrongDestination => Some(PublicEndMismatch::WrongDestination),
        Unmatched::WrongAsset => Some(PublicEndMismatch::WrongAsset),
        Unmatched::WrongAmount => Some(PublicEndMismatch::WrongAmount),
        Unmatched::UnknownMovement
        | Unmatched::NotFinal
        | Unmatched::ReorgedOut
        | Unmatched::WrongNetwork => None,
    }
}

/// The segment's planned public end on `network`: the outer node its plan names at one of its ends, which the
/// movement checked is on. The log holds a mismatch to its movement's network but can't tell the node is the
/// segment's own end, which the plan names (`T-020-21`'s README); Core holds it here, taking the node from the
/// operation and never from the network's record.
pub(super) fn planned_end(operation: &Operation, network: &str) -> Option<OuterNode> {
    [&operation.to.node, &operation.from.node]
        .into_iter()
        .find_map(|node| match node {
            NodeRef::Outer(outer) if outer.network == network => Some(outer.clone()),
            _ => None,
        })
}

/// What the network's record of the movement says it moved, as the log records figures.
pub(super) fn network_figures(record: &Movement) -> Option<PublicFigures> {
    Some(PublicFigures {
        destination: Id::new(&record.destination)?,
        asset: Id::new(&record.asset)?,
        amount: rutent_contracts::router_api::Amount::new(record.amount.clone())?,
    })
}

/// What the provider said it moved: the figures the movement's check was held to.
pub(super) fn stated_figures(expected: &Expected) -> Option<PublicFigures> {
    Some(PublicFigures {
        destination: Id::new(&expected.destination)?,
        asset: Id::new(&expected.asset)?,
        amount: rutent_contracts::router_api::Amount::new(expected.amount.clone())?,
    })
}

/// A mismatch graded or read from the log, and whether its raise is made.
#[derive(Debug, Clone)]
struct Pending {
    order: Id,
    segment: Id,
    found: PublicEndMismatch,
    moved: PublicFigures,
    stated: PublicFigures,
    /// Its entry's place among the entries the grading answered, until their receipts are told; `None` for one read
    /// from the log, whose receipt is known.
    index: Option<usize>,
    /// The receipt of its entry.
    receipt: Option<Receipt>,
    /// The raise made, and its entry, built once.
    raised: Option<Raised>,
}

#[derive(Debug, Clone)]
struct Raised {
    entry: GeneralEntry,
    /// Its entry's receipt, once the log took it.
    receipt: Option<Receipt>,
}

/// Core's record of the mismatches it found and has not raised and recorded yet, those it has, and the raises the log
/// refused.
#[derive(Debug, Clone, Default)]
pub(super) struct Unraised {
    pending: Vec<Pending>,
    /// The segments (order, segment) whose mismatch is raised and recorded, or whose raise the log refused: a segment
    /// graded again, as after a restart, is never raised a second time.
    settled: Vec<(Id, Id)>,
    /// The raises the log refused, with why: kept readable, since a refusal is deterministic and the raise is
    /// advisory, so it stops nothing ([`Closing::refused_raises`](super::Closing::refused_raises)).
    refused: Vec<(Id, Id, Violation)>,
}

impl Unraised {
    /// A mismatch graded, its entry the `index`th the grading answered. Ignored where this segment's mismatch is
    /// pending already (a segment graded again after a restart rebuilt it from the log) or raised.
    pub(super) fn expect(
        &mut self,
        (order, segment): (Id, Id),
        (found, moved, stated): (PublicEndMismatch, PublicFigures, PublicFigures),
        index: usize,
    ) {
        let key = (order.clone(), segment.clone());
        if self.settled.contains(&key)
            || self
                .pending
                .iter()
                .any(|p| p.order == order && p.segment == segment && p.found == found)
        {
            // A pending one read from the log takes the new receipts' place: the entries are equal, so the receipts
            // are the same.
            for p in &mut self.pending {
                if p.order == order && p.segment == segment && p.index.is_none() {
                    p.index = Some(index);
                }
            }
            return;
        }
        self.pending.push(Pending {
            order,
            segment,
            found,
            moved,
            stated,
            index: Some(index),
            receipt: None,
            raised: None,
        });
    }

    /// The receipts of the entries the grading answered for `segment` of `order`, once appended.
    pub(super) fn bind(&mut self, order: &Id, segment: &Id, receipts: &[Receipt]) {
        for pending in &mut self.pending {
            if &pending.order == order
                && &pending.segment == segment
                && pending.receipt.is_none()
                && let Some(receipt) = pending.index.and_then(|i| receipts.get(i))
            {
                pending.receipt = Some(receipt.clone());
            }
        }
    }

    /// The raises the log refused: the segment's order and segment, and why.
    pub(super) fn refused(&self) -> &[(Id, Id, Violation)] {
        &self.refused
    }

    /// The mismatches the log holds and no raise cites, after a restart that lost Core's record: raised on the next
    /// call, as one whose raise the log never took. Those it holds a raise of are not raised again.
    pub(super) fn rebuilt(history: &[(Receipt, Entry)]) -> Self {
        let raised = |receipt: &Receipt| {
            history.iter().any(|(_, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what: What::Raise { raise },
                    ..
                }) => {
                    matches!(raise.difference, Difference::PublicEnd { .. })
                        && raise.cites.contains(receipt)
                }
                _ => false,
            })
        };
        let mut rebuilt = Self::default();
        for (receipt, entry) in history {
            let Entry::Verified(VerifiedEntry {
                what:
                    What::PublicMismatch {
                        order,
                        segment,
                        found,
                        moved,
                        stated,
                        ..
                    },
                ..
            }) = entry
            else {
                continue;
            };
            if raised(receipt) {
                rebuilt.settled.push((order.clone(), segment.clone()));
            } else {
                rebuilt.pending.push(Pending {
                    order: order.clone(),
                    segment: segment.clone(),
                    found: *found,
                    moved: moved.clone(),
                    stated: stated.clone(),
                    index: None,
                    receipt: Some(receipt.clone()),
                    raised: None,
                });
            }
        }
        rebuilt
    }

    /// Raises each mismatch whose entry the log took, once, and records the raise as built, `done` where it reached
    /// the operator and `failed` where it didn't. Both entries a raise may need are built before it is raised, so
    /// nothing that fails after the raise can have it raised again. Each is tried even where an earlier one couldn't
    /// be recorded. Only a log that can't take an entry now (`Unavailable`) is answered, and the raise is tried again
    /// next time; a refusal is deterministic, the raise advisory and moving nothing, so it is kept in
    /// [`Self::refused`] and the mismatch is not tried again.
    pub(super) fn raise(&mut self, desk: RaiseDesk<'_>) -> Result<(), AppendFailure> {
        let RaiseDesk {
            log,
            clock,
            fresh,
            operator,
            core,
        } = desk;
        let mut first = Ok(());
        let mut finished = Vec::new();
        for (at, pending) in self.pending.iter_mut().enumerate() {
            let Some(receipt) = pending.receipt.clone() else {
                continue;
            };
            let done = (|| {
                if pending.raised.is_none() {
                    let at = clock
                        .now()
                        .timestamp()
                        .ok_or(AppendFailure::Refused(Violation(
                            "Core's clock reads a time a timestamp can hold",
                        )))?;
                    let raise = Raise {
                        raise: fresh.id(),
                        difference: Difference::PublicEnd {
                            order: pending.order.clone(),
                            segment: pending.segment.clone(),
                            found: pending.found,
                            moved: pending.moved.clone(),
                            stated: pending.stated.clone(),
                        },
                        cites: vec![receipt],
                        at,
                    };
                    raise.check().map_err(AppendFailure::Refused)?;
                    let entry = |outcome| built(core, &raise, outcome);
                    let (done, failed) = (entry(Outcome::Done {})?, entry(Outcome::Failed {})?);
                    let delivered = matches!(operator.raise(&raise), Delivery::Delivered);
                    pending.raised = Some(Raised {
                        entry: if delivered { done } else { failed },
                        receipt: None,
                    });
                }
                match &mut pending.raised {
                    Some(raised) if raised.receipt.is_none() => append(log, raised),
                    _ => Ok(()),
                }
            })();
            match done {
                Ok(()) => finished.push(at),
                Err(AppendFailure::Refused(why)) => {
                    self.refused
                        .push((pending.order.clone(), pending.segment.clone(), why));
                    finished.push(at);
                }
                Err(failure) => {
                    if first.is_ok() {
                        first = Err(failure);
                    }
                }
            }
        }
        for at in finished.into_iter().rev() {
            let gone = self.pending.remove(at);
            self.settled.push((gone.order, gone.segment));
        }
        first
    }
}

fn built(core: &Actor, raise: &Raise, outcome: Outcome) -> Result<GeneralEntry, AppendFailure> {
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: core.clone(),
        what: What::Raise {
            raise: raise.clone(),
        },
        when: When {
            at: raise.at.clone(),
            source: TimeSource::CoreClock,
        },
        outcome,
    });
    entry.check().map_err(AppendFailure::Refused)?;
    GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)
}

/// Appends the raise's entry as built, tried again while the log can't take it now (`INV-8`): the log appends an equal
/// entry once, so a retried append whose first answer was lost gives the same receipt.
fn append(log: &mut dyn Log, raised: &mut Raised) -> Result<(), AppendFailure> {
    let mut last = AppendFailure::Unavailable;
    for _ in 0..crate::segments::APPEND_TRIES {
        match log.append(&raised.entry) {
            Ok(receipt) => {
                raised.receipt = Some(receipt);
                return Ok(());
            }
            Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
            Err(refused) => return Err(refused),
        }
    }
    Err(last)
}
