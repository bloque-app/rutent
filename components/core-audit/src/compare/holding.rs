//! The comparison of a holding account's balance with the value held there for its orders (`REQ-AUD-15`, `AC-39`):
//! a pure function of the reading.
//!
//! Like [`super::report`], this module imports data types of the contracts and nothing else, so the check reaches
//! nothing that moves value; a test holds that.

use rutent_contracts::core_internal::{Difference, Direction, HoldingReading, Word};
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, Id, PositiveAmount};

use super::Incomparable;

/// What checking a holding account finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldingVerdict {
    /// The balance equals the sum held for the orders.
    Match,
    /// It is short of the sum or over it: the difference to raise, labelled with whose word the balance is.
    Differs(Difference),
    /// It can't be compared; nothing is raised for it.
    NotComparable(Incomparable),
}

/// The sum of `amounts`, or `None` where it passes what an amount can state (78 digits).
pub fn sum<'a>(amounts: impl IntoIterator<Item = &'a Amount>) -> Option<Amount> {
    amounts
        .into_iter()
        .try_fold(Amount::zero(), |total, a| total.checked_add(a))
}

/// Compares `balance` with `sum` for `account`, in `asset`, the balance being `word`'s: the one comparison every
/// check of a holding account makes.
pub fn judge(
    account: &Id,
    asset: &Denomination,
    word: Word,
    balance: &Amount,
    sum: &Amount,
) -> HoldingVerdict {
    let (direction, by) = if balance < sum {
        (Direction::Short, sum.checked_sub(balance))
    } else if balance > sum {
        (Direction::Over, balance.checked_sub(sum))
    } else {
        return HoldingVerdict::Match;
    };
    match by.and_then(|by| PositiveAmount::new(by.as_str())) {
        Some(by) => HoldingVerdict::Differs(Difference::Holding {
            account: account.clone(),
            direction,
            by,
            asset: asset.clone(),
            word,
        }),
        // The two differ, so their difference is above zero and inside what an amount states.
        None => HoldingVerdict::NotComparable(Incomparable::SumPastTheBound),
    }
}

/// Checks a holding account's balance against the sum of what each order holds there. The balance keeps the word it
/// was read under: a private node's is its publisher's, and a difference says so (`REQ-AUD-15`).
pub fn check_holding(reading: &HoldingReading) -> HoldingVerdict {
    let Some(held) = sum(reading.held_for.iter().map(|h| h.amount.amount())) else {
        return HoldingVerdict::NotComparable(Incomparable::SumPastTheBound);
    };
    judge(
        &reading.account,
        &reading.asset,
        reading.word,
        &reading.balance,
        &held,
    )
}
