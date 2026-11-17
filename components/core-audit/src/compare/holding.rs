//! The comparison of a holding account's balance with the value held there for its orders (`REQ-AUD-15`, `AC-39`):
//! a pure function of the reading.
//!
//! Like [`super::report`], this module imports data types of the contracts and nothing else, so the check reaches
//! nothing that moves value; a test holds that.

use rutent_contracts::core_internal::{
    Difference, Direction, HoldingIncomparable, HoldingReading, Word,
};
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, Id, PositiveAmount};

/// What checking a holding account finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldingVerdict {
    /// The balance equals the sum held for the orders.
    Match,
    /// It is short of the sum or over it: the difference to raise, labelled with whose word the balance is.
    Differs(Difference),
    /// It can't be compared: the difference to raise, a `HoldingNotComparable` naming why.
    NotComparable(Difference),
}

/// That `account`'s balance can't be compared, for `reason`; `read` is the balance's word and the account's asset,
/// where a balance was read.
pub fn not_comparable(
    account: &Id,
    read: Option<(Word, &Denomination)>,
    reason: HoldingIncomparable,
) -> HoldingVerdict {
    HoldingVerdict::NotComparable(Difference::HoldingNotComparable {
        account: account.clone(),
        word: read.map(|(word, _)| word),
        asset: read.map(|(_, asset)| asset.clone()),
        reason,
    })
}

/// Compares `balance` with the sum of `held` for `account`, in `asset`, the balance being `word`'s: the one
/// comparison every check of a holding account makes. It walks the amounts against the balance and never forms the
/// sum, so a sum past what an amount states (78 digits) still gives its short where the short is one an amount
/// states; only a short past that bound can't be stated, and it is raised as not comparable. An over is never past
/// it, being at most the balance.
pub fn judge<'a>(
    account: &Id,
    asset: &Denomination,
    word: Word,
    balance: &Amount,
    held: impl IntoIterator<Item = &'a Amount>,
) -> HoldingVerdict {
    // What of the balance the amounts so far leave, and what they hold past it: one of the two is zero.
    let (mut left, mut past) = (balance.clone(), Amount::zero());
    for amount in held {
        match left.checked_sub(amount) {
            Some(rest) => left = rest,
            None => {
                let beyond = amount
                    .checked_sub(&left)
                    .expect("an amount above what is left exceeds it by an amount");
                left = Amount::zero();
                match past.checked_add(&beyond) {
                    Some(total) => past = total,
                    None => {
                        return not_comparable(
                            account,
                            Some((word, asset)),
                            HoldingIncomparable::SumPastTheBound,
                        );
                    }
                }
            }
        }
    }
    let (direction, by) = if !past.is_zero() {
        (Direction::Short, past)
    } else if !left.is_zero() {
        (Direction::Over, left)
    } else {
        return HoldingVerdict::Match;
    };
    HoldingVerdict::Differs(Difference::Holding {
        account: account.clone(),
        direction,
        by: PositiveAmount::new(by.as_str()).expect("a short or an over is above zero"),
        asset: asset.clone(),
        word,
    })
}

/// Checks a holding account's balance against the sum of what each order holds there. The balance keeps the word it
/// was read under: a private node's is its publisher's, and a difference says so (`REQ-AUD-15`). A short past what
/// an amount can state is raised as not comparable, so it fails closed and states no `by` it can't know.
pub fn check_holding(reading: &HoldingReading) -> HoldingVerdict {
    judge(
        &reading.account,
        &reading.asset,
        reading.word,
        &reading.balance,
        reading.held_for.iter().map(|h| h.amount.amount()),
    )
}
