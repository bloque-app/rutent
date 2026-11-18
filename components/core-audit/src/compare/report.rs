//! The comparison of a segment's two reports (`REQ-ORD-9`, ADR-0009): a pure function of the pair.
//!
//! This module imports data types of the contracts and nothing else: no log, no operator channel, no clock and no
//! module of `core-orch`. A function here takes shared references and returns a value, so it can reach no ledger,
//! store or provider, and nothing that moves value or changes an order (ADR-0009). A test reads this file's source
//! and holds that (`comparison_modules_reach_nothing_that_moves_value`).

use rutent_contracts::core_internal::{Difference, ReportPair, Reported, ReportedAs};

/// What comparing a segment's two reports finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportsVerdict {
    /// The two reports state one amount of one asset, like with like, or two less than one increment apart.
    Match,
    /// They state two amounts of one asset one increment or more apart: the difference to raise.
    Differs(Difference),
    /// They can't be compared, being of two assets or of one asset in two units (`THR-107`): the difference to
    /// raise, a `ReportsNotComparable` carrying each figure as it was stated.
    NotComparable(Difference),
}

/// Compares what the sending party reports sent with what the receiving party reports received, under the sending
/// party's tuple's terms the pair carries (`REQ-CAT-11`, ADR-0009).
///
/// Like is compared with like: a figure reported gross holds its charge, which the next party never receives, so
/// the charge is taken off it first; a net figure is compared as it stands. Two reports less than one of the tuple's
/// declared fill increments apart, either way, raise nothing; one increment or more is a difference, and where the
/// tuple declares no increment they are compared exactly, which fails closed. A difference names the figure compared,
/// net of the charge where it was reported gross; the pair's record, which the raise cites, holds both figures as
/// stated and the terms. Reports of two assets, or of one asset in two units, aren't comparable, whatever their
/// amounts, and that is raised as well, each figure as it was stated. The verdict depends on the two amounts, the
/// two assets and the terms alone, never on the parties or the statements; the order and segment are only named in
/// the difference.
pub fn compare_reports(pair: &ReportPair) -> ReportsVerdict {
    let (sent, received) = (&pair.sent, &pair.received);
    if sent.asset != received.asset {
        return ReportsVerdict::NotComparable(Difference::ReportsNotComparable {
            order: pair.order.clone(),
            segment: pair.segment.clone(),
            sent: Reported {
                amount: sent.amount.clone(),
                asset: sent.asset.clone(),
            },
            received: Reported {
                amount: received.amount.clone(),
                asset: received.asset.clone(),
            },
        });
    }
    let delivered = match &pair.terms.reporting {
        ReportedAs::Net {} => sent.amount.clone(),
        // The contract holds a gross figure's charge to no more than it (`ReportPair::check`).
        ReportedAs::Gross { charge } => sent
            .amount
            .checked_sub(charge)
            .expect("a gross figure holds no more charge than it states"),
    };
    let apart = delivered
        .checked_sub(&received.amount)
        .or_else(|| received.amount.checked_sub(&delivered))
        .expect("one of two amounts is at least the other");
    let within = match &pair.terms.increment {
        Some(increment) => &apart < increment.amount(),
        None => apart.is_zero(),
    };
    if within {
        return ReportsVerdict::Match;
    }
    ReportsVerdict::Differs(Difference::Reports {
        order: pair.order.clone(),
        segment: pair.segment.clone(),
        sent: delivered,
        received: received.amount.clone(),
        asset: sent.asset.clone(),
    })
}
