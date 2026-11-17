//! The comparison of a segment's two reports (`REQ-ORD-9`, ADR-0009): a pure function of the pair.
//!
//! This module imports data types of the contracts and nothing else: no log, no operator channel, no clock and no
//! module of `core-orch`. A function here takes shared references and returns a value, so it can reach no ledger,
//! store or provider, and nothing that moves value or changes an order (ADR-0009). A test reads this file's source
//! and holds that (`comparison_modules_reach_nothing_that_moves_value`).

use rutent_contracts::core_internal::{Difference, ReportPair, Reported};

/// What comparing a segment's two reports finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportsVerdict {
    /// The two reports state one amount of one asset.
    Match,
    /// They state two amounts of one asset: the difference to raise.
    Differs(Difference),
    /// They can't be compared, being of two assets or of one asset in two units (`THR-107`): the difference to
    /// raise, a `ReportsNotComparable` carrying each figure as it was stated.
    NotComparable(Difference),
}

/// Compares what the sending party reports sent with what the receiving party reports received.
///
/// The amounts are compared exactly: the pair carries neither a provider's fill increment (`REQ-CAT-11`) nor its
/// gross or net convention, so no tolerance is applied, which fails closed (a rounding gap is raised, never hidden).
/// Reports of two assets, or of one asset in two units, aren't comparable, whatever their amounts, and that is raised
/// as well. The verdict depends on the two amounts and the two assets alone, never on the parties or the
/// statements; the order and segment are only named in the difference.
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
    if sent.amount == received.amount {
        return ReportsVerdict::Match;
    }
    ReportsVerdict::Differs(Difference::Reports {
        order: pair.order.clone(),
        segment: pair.segment.clone(),
        sent: sent.amount.clone(),
        received: received.amount.clone(),
        asset: sent.asset.clone(),
    })
}
