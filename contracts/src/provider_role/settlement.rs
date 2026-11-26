//! The amounts a result states, held against the operation it answers and the tuple's terms (`INV-1`,
//! `REQ-CAT-11`, `REQ-QTE-5`, `THR-108`). Added after review round 1 on the M1 pull request.
//!
//! A result's `amount_in` is what left the segment's origin, the provider's charge included, whether its tuple
//! reports gross or net: `reporting` says only whether `amount_out` includes the charge. Its fee is in the asset
//! the segment takes in, in the origin unit, as the router counts it. Where a segment keeps its asset and its tuple
//! reports gross, what it delivers is its `amount_out` less its charge, the fee restated in the out unit
//! ([`less_the_charge`]).

use alloc::{borrow::ToOwned, string::String, vec, vec::Vec};
use core::cmp::Ordering;

use super::operation::Operation;
use super::outcome::{
    ConversionOutcome, Direction, Fee, ReserveMovementOutcome, ResultBody, TransferOutcome,
};
use super::types::{NodeRef, OuterNode, Terms};

/// What stayed at a segment's origin node: the part of an `exact_in` operation's amount the provider didn't take
/// in, as one that fills in increments leaves it. It never left the origin, so `INV-1` counts it as the reported
/// remainder there. At an inner node, `REQ-AUD-15` counts it at that node's holding account; a withdrawal's
/// remainder never left its outer node, so it never entered the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remainder {
    /// The segment's origin node: the reserve's outer node for a withdrawal, the one node of a conversion, and
    /// the inner node the value leaves for a deposit or a transfer.
    pub node: NodeRef,
    /// The asset taken in: the operation's `asset`, or a conversion's `from`.
    pub asset: String,
    /// How much, in the origin unit (a conversion's `from` unit); `0` when the provider took in all it was asked.
    pub amount: String,
}

/// A rule a result's amounts break against the operation it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmountMismatch {
    /// The result is of another kind than the operation, or names another transaction (`REQ-ORD-1`).
    AnswersAnotherOperation,
    /// An amount that isn't a whole number in decimal digits with no leading zeros, so nothing can be compared.
    /// The schemas refuse it; a value built in Rust may not.
    NotAnAmount,
    /// The fee is in another asset than the one the segment takes in, so it can't be counted against the
    /// tuple's formula (`REQ-QTE-5`).
    FeeInAnotherAsset,
    /// The fee is more than the provider took in.
    FeeAboveAmountIn,
    /// An `exact_in` segment took in more than its operation stated: a fee counted on top of the amount, or value
    /// that was never the segment's (`THR-108`).
    TookInMoreThanStated,
    /// An `exact_in` segment took in less than stated, at a tuple that declares no fill increment
    /// (`REQ-CAT-11`, `THR-108`).
    UndeclaredRemainder,
    /// An `exact_in` segment left a remainder of at least its tuple's fill increment, which it could have filled
    /// (`REQ-CAT-11`, `THR-108`).
    RemainderOfAWholeIncrement,
}

/// The segment an operation names, as far as its amounts go.
struct Segment<'a> {
    transaction: &'a str,
    origin: NodeRef,
    asset: &'a str,
    direction: Direction,
    amount: &'a str,
}

/// The amounts a result states: a settlement's or an accepted dry run's.
struct Stated<'a> {
    amount_in: &'a str,
    fee: &'a Fee,
    settled: bool,
}

/// Checks a result's amounts against the operation it answers and the terms of the tuple it runs through, and
/// returns every rule broken:
///
/// - the result is of the operation's kind and names its transaction (`REQ-ORD-1`);
/// - its fee is in the asset the segment takes in (a withdrawal's, deposit's or transfer's `asset`, a
///   conversion's `from`), and at most its `amount_in` (`REQ-QTE-5`), as the router asks of an operation and a
///   quote;
/// - where the operation fixes what it takes in (`exact_in`; a withdrawal and a deposit always state their origin
///   amount), `amount_in` is at most the stated amount, and the difference is below the tuple's fill increment,
///   or zero where it declares none (`REQ-CAT-11`, `THR-108`).
///
/// A succeeded `exact_in` segment's remainder is returned, `0` included, at its origin node. A dry-run answer is
/// held to the same rules, since its figures price the execution, and returns none. So does an `exact_out`
/// operation: it states what is given out, not what is taken in, so what stays at its origin follows from the
/// quote it was priced on, which is the state machine's to hold (review round 1, finding 2). Any other result
/// states no amounts, and passes.
///
/// It assumes both statements passed their schemas, and checks amounts only. That the result is a dry-run answer
/// exactly when the operation is a dry run, and is attributable to the operation's provider, are `CMP-CORE-ORCH`'s
/// rules across statements (`contracts/provider-role/README.md`).
pub fn check_amounts(
    operation: &Operation,
    result: &ResultBody,
    terms: &Terms,
) -> Result<Option<Remainder>, Vec<AmountMismatch>> {
    let Some((segment, answered)) = pair(operation, result)? else {
        return Ok(None);
    };
    let Some(stated) = answered else {
        return Ok(None);
    };
    let mut violations = Vec::new();
    let (Some(amount_in), Some(fee), Some(asked)) = (
        whole(stated.amount_in),
        whole(&stated.fee.amount),
        whole(segment.amount),
    ) else {
        return Err(vec![AmountMismatch::NotAnAmount]);
    };
    if stated.fee.asset != segment.asset {
        violations.push(AmountMismatch::FeeInAnotherAsset);
    }
    if compare(fee, amount_in) == Ordering::Greater {
        violations.push(AmountMismatch::FeeAboveAmountIn);
    }
    let mut remainder = None;
    if segment.direction == Direction::ExactIn {
        match subtract(asked, amount_in) {
            None => violations.push(AmountMismatch::TookInMoreThanStated),
            Some(left) => {
                match terms.increment.as_deref() {
                    None if left != "0" => violations.push(AmountMismatch::UndeclaredRemainder),
                    None => {}
                    Some(increment) => match whole(increment) {
                        None => violations.push(AmountMismatch::NotAnAmount),
                        Some(increment) if compare(&left, increment) != Ordering::Less => {
                            violations.push(AmountMismatch::RemainderOfAWholeIncrement);
                        }
                        Some(_) => {}
                    },
                }
                remainder = Some(left);
            }
        }
    }
    if !violations.is_empty() {
        return Err(violations);
    }
    Ok(remainder
        .filter(|_| stated.settled)
        .map(|amount| Remainder {
            node: segment.origin,
            asset: segment.asset.to_owned(),
            amount,
        }))
}

/// A fee restated in the unit of the asset a segment gives out: the operation's out asset, at `out_decimals`. A fee is
/// stated in the origin unit (`origin_decimals`), the unit the segment takes in. Where the out unit is finer, the fee
/// is multiplied out exactly; where it is coarser, it is divided and rounded up, so a charge is never counted below
/// what was charged. `None` where the fee isn't a whole number in canonical decimal text.
///
/// For a segment that keeps its asset (a withdrawal, a deposit or a transfer) whose two ends count it in different
/// decimals, this is the unit "less the charge" is stated in: see [`less_the_charge`].
pub fn charge_in_out_unit(fee: &str, origin_decimals: u8, out_decimals: u8) -> Option<String> {
    let fee = whole(fee)?;
    if fee == "0" {
        return Some("0".to_owned());
    }
    if out_decimals >= origin_decimals {
        let mut out = fee.to_owned();
        out.extend(core::iter::repeat_n(
            '0',
            usize::from(out_decimals - origin_decimals),
        ));
        return Some(out);
    }
    let drop = usize::from(origin_decimals - out_decimals);
    if fee.len() <= drop {
        // Below one whole out unit, and not zero: it rounds up to one.
        return Some("1".to_owned());
    }
    let (kept, dropped) = fee.split_at(fee.len() - drop);
    if dropped.bytes().all(|b| b == b'0') {
        Some(kept.to_owned())
    } else {
        Some(add_one(kept))
    }
}

/// What a segment that keeps its asset delivers, under a tuple that reports `gross` (`REQ-CAT-11`): the
/// `amount_out` it states, which includes its charge, less that charge, the fee restated in the operation's out unit
/// ([`charge_in_out_unit`]). Both are then in the out unit, so the subtraction is of like units whatever decimals the
/// two ends count in. `None` where the charge is above `amount_out`, or a figure isn't a whole number in canonical
/// decimal text. Under `net`, what it delivers is the `amount_out` it states, with no subtraction. A conversion's
/// output isn't reduced: its fee is in its `from` asset, which it doesn't give out.
pub fn less_the_charge(
    amount_out: &str,
    fee: &str,
    origin_decimals: u8,
    out_decimals: u8,
) -> Option<String> {
    subtract(
        whole(amount_out)?,
        &charge_in_out_unit(fee, origin_decimals, out_decimals)?,
    )
}

/// `a + 1` in canonical decimal text.
fn add_one(a: &str) -> String {
    let mut digits: Vec<u8> = a.bytes().collect();
    for d in digits.iter_mut().rev() {
        if *d == b'9' {
            *d = b'0';
        } else {
            *d += 1;
            return String::from_utf8(digits).expect("ASCII digits");
        }
    }
    let mut out = String::from("1");
    out.push_str(core::str::from_utf8(&digits).expect("ASCII digits"));
    out
}

/// Pairs a value-moving operation with its result's stated amounts. `Ok(None)` for an operation that moves no
/// value; `Ok(Some((segment, None)))` for a result that states no amounts, such as a failure or a refusal.
#[allow(clippy::type_complexity)]
fn pair<'a>(
    operation: &'a Operation,
    result: &'a ResultBody,
) -> Result<Option<(Segment<'a>, Option<Stated<'a>>)>, Vec<AmountMismatch>> {
    let mismatch = || Err(vec![AmountMismatch::AnswersAnotherOperation]);
    let reserve = |outcome: &'a ReserveMovementOutcome| match outcome {
        ReserveMovementOutcome::Succeeded(s) => Some(stated(&s.amount_in, &s.fee, true)),
        ReserveMovementOutcome::DryRunAccepted(q) => Some(stated(&q.amount_in, &q.fee, false)),
        _ => None,
    };
    let (segment, transaction, answered) = match (operation, result) {
        (Operation::Withdrawal(o), ResultBody::Withdrawal(r)) => (
            Segment {
                transaction: &o.transaction,
                origin: NodeRef::Outer(OuterNode {
                    network: o.origin.network.clone(),
                }),
                asset: &o.asset,
                direction: Direction::ExactIn,
                amount: &o.amount,
            },
            &r.transaction,
            reserve(&r.outcome),
        ),
        (Operation::Deposit(o), ResultBody::Deposit(r)) => (
            Segment {
                transaction: &o.transaction,
                origin: NodeRef::Inner(o.origin.clone()),
                asset: &o.asset,
                direction: Direction::ExactIn,
                amount: &o.amount,
            },
            &r.transaction,
            reserve(&r.outcome),
        ),
        (Operation::Transfer(o), ResultBody::Transfer(r)) => (
            Segment {
                transaction: &o.transaction,
                origin: NodeRef::Inner(o.origin.clone()),
                asset: &o.asset,
                direction: o.direction,
                amount: &o.amount,
            },
            &r.transaction,
            match &r.outcome {
                TransferOutcome::Succeeded(s) => Some(stated(&s.amount_in, &s.fee, true)),
                TransferOutcome::DryRunAccepted(q) => Some(stated(&q.amount_in, &q.fee, false)),
                _ => None,
            },
        ),
        (Operation::Conversion(o), ResultBody::Conversion(r)) => (
            Segment {
                transaction: &o.transaction,
                origin: NodeRef::Inner(o.node.clone()),
                asset: &o.from,
                direction: o.direction,
                amount: &o.amount,
            },
            &r.transaction,
            match &r.outcome {
                ConversionOutcome::Succeeded(s) => Some(stated(&s.amount_in, &s.fee, true)),
                ConversionOutcome::DryRunAccepted(q) => Some(stated(&q.amount_in, &q.fee, false)),
                _ => None,
            },
        ),
        (
            Operation::Withdrawal(_)
            | Operation::Deposit(_)
            | Operation::Transfer(_)
            | Operation::Conversion(_),
            _,
        ) => return mismatch(),
        _ => return Ok(None),
    };
    if *transaction != segment.transaction {
        return mismatch();
    }
    Ok(Some((segment, answered)))
}

fn stated<'a>(amount_in: &'a str, fee: &'a Fee, settled: bool) -> Stated<'a> {
    Stated {
        amount_in,
        fee,
        settled,
    }
}

/// The text, if it is a whole number in decimal digits with no leading zeros, as the schemas' `amount` is.
fn whole(text: &str) -> Option<&str> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    (digits && (text == "0" || !text.starts_with('0'))).then_some(text)
}

/// Compares two whole numbers in canonical decimal text.
fn compare(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// `a − b` in canonical decimal text, or `None` if `b` is greater. Amounts may have 78 digits, so this works on
/// the digits rather than a machine integer.
fn subtract(a: &str, b: &str) -> Option<String> {
    if compare(a, b) == Ordering::Less {
        return None;
    }
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut digits = Vec::with_capacity(a.len());
    let mut borrow = 0;
    for i in 0..a.len() {
        let x = i32::from(a[a.len() - 1 - i] - b'0');
        let y = if i < b.len() {
            i32::from(b[b.len() - 1 - i] - b'0')
        } else {
            0
        };
        let mut d = x - y - borrow;
        borrow = i32::from(d < 0);
        if d < 0 {
            d += 10;
        }
        digits.push(b'0' + u8::try_from(d).expect("a digit"));
    }
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }
    digits.reverse();
    Some(String::from_utf8(digits).expect("ASCII digits"))
}

#[cfg(test)]
mod tests {
    use super::{
        AmountMismatch, charge_in_out_unit, check_amounts, less_the_charge, subtract, whole,
    };
    use crate::provider_role::{Operation, ResultBody, Terms};
    use alloc::{format, vec};

    // verifies: INV-1, REQ-CAT-11
    #[test]
    fn an_amount_built_in_rust_that_is_no_whole_number_is_refused() {
        let operation: Operation = serde_json::from_value(serde_json::json!({
            "kind": "conversion", "transaction": "tx-1", "provider": "acme-co", "node": "acme-ledger",
            "from": "COP", "to": "USDC", "direction": "exact_in", "amount": "100", "dry_run": false,
            "inputs": {}
        }))
        .expect("an operation");
        let result = |amount_in: &str| -> ResultBody {
            serde_json::from_value(serde_json::json!({
                "kind": "conversion", "transaction": "tx-1",
                "outcome": {
                    "status": "succeeded", "amount_in": amount_in, "amount_out": "1", "rate": "1",
                    "fee": { "asset": "COP", "amount": "0" },
                    "evidence": [{ "grade": 3, "reference": "r", "by": "acme-co" }]
                }
            }))
            .expect("a result")
        };
        let terms: Terms = serde_json::from_value(serde_json::json!({
            "bands": [{ "min": "1", "max": "1000" }], "reporting": "gross", "required_inputs": []
        }))
        .expect("terms");
        assert!(check_amounts(&operation, &result("100"), &terms).is_ok());
        for bad in ["99.5", "0100", "-1", ""] {
            assert_eq!(
                check_amounts(&operation, &result(bad), &terms),
                Err(vec![AmountMismatch::NotAnAmount]),
                "{bad:?}"
            );
        }
    }

    // verifies: INV-1
    #[test]
    fn a_difference_of_amounts_is_exact_at_any_length() {
        assert_eq!(subtract("25050000", "25000000").as_deref(), Some("50000"));
        assert_eq!(subtract("100", "100").as_deref(), Some("0"));
        assert_eq!(subtract("1000", "1").as_deref(), Some("999"));
        assert_eq!(subtract("10", "9").as_deref(), Some("1"));
        assert_eq!(subtract("9", "10"), None);
        assert_eq!(subtract("100", "101"), None);
        let big = "9".repeat(78);
        assert_eq!(subtract(&big, &big).as_deref(), Some("0"));
        assert_eq!(
            subtract(&format!("1{}", "0".repeat(77)), "1").as_deref(),
            Some("9".repeat(77).as_str())
        );
    }

    // verifies: REQ-CAT-11, INV-1
    #[test]
    fn less_the_charge_is_stated_in_the_out_unit() {
        // One unit: a plain subtraction.
        assert_eq!(
            less_the_charge("244508252", "1000000", 6, 6).as_deref(),
            Some("243508252")
        );
        // A finer out unit: the fee is multiplied out exactly. 1.00 out of 2-decimal origin units, 0.01 fee.
        assert_eq!(charge_in_out_unit("1", 2, 6).as_deref(), Some("10000"));
        assert_eq!(
            less_the_charge("1000000", "1", 2, 6).as_deref(),
            Some("990000")
        );
        // A coarser out unit: divided and rounded up, never below the charge. 0.015 at 6 decimals is 2 at 2.
        assert_eq!(charge_in_out_unit("15000", 6, 2).as_deref(), Some("2"));
        assert_eq!(charge_in_out_unit("10000", 6, 2).as_deref(), Some("1"));
        assert_eq!(charge_in_out_unit("1", 6, 2).as_deref(), Some("1"));
        assert_eq!(charge_in_out_unit("999999", 6, 2).as_deref(), Some("100"));
        assert_eq!(charge_in_out_unit("0", 6, 2).as_deref(), Some("0"));
        assert_eq!(less_the_charge("100", "15000", 6, 2).as_deref(), Some("98"));
        // A charge above what it states it gave out is no amount delivered.
        assert_eq!(less_the_charge("1", "15000", 6, 2), None);
        assert_eq!(less_the_charge("100", "01", 6, 2), None);
    }

    // verifies: INV-1
    #[test]
    fn only_a_canonical_whole_number_is_an_amount() {
        for good in ["0", "1", "250000", &"9".repeat(78)] {
            assert_eq!(whole(good), Some(good));
        }
        for bad in ["", "01", "00", "1.5", "-1", "1e3", " 1", "+1"] {
            assert_eq!(whole(bad), None, "{bad:?}");
        }
    }
}
