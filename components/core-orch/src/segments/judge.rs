//! Whether a result counts for the segment it answers (`REQ-ORD-1`, `INV-1`; provider-role's README, "Rules across
//! statements" and "Amounts against the operation").
//!
//! A result is judged from its bytes as received, in this order, and the first rule it breaks is why it doesn't
//! count:
//!
//! 1. **Attributable**: its signature verifies, under a `statements` key its maker's advertisement listed at its
//!    `made_at` and lists now, in this deployment and protocol version (`Catalogue::verify_statement`).
//! 2. **Its provider's**: its maker is the segment's provider.
//! 3. **This transaction's**: it is of the operation's kind and names the segment's transaction, and it isn't a
//!    dry-run answer, which no real operation is answered by.
//! 4. **Its amounts hold** against the operation and the tuple's terms (`check_amounts`).
//! 5. **It gives out no more than it could**: a segment that keeps its asset gives out at most what it took in, and a
//!    conversion at most what the quote its operation cites gives for what it took in, moved by no more than the
//!    tighter of that quote's slippage and the operator's limit for the tuple (`REQ-ORD-21`, `INV-1`, `REQ-QTE-9`).
//!    A conversion with no such quote, or with neither a bounded rate nor an operator's limit, has no bound and
//!    doesn't count. `check_amounts` holds what is taken in, never what is given out, so without this a result
//!    could hand the next segment more than arrived.
//!
//! A result that doesn't count is no result: the segment waits for one that does, and no outcome is recorded on it.

use rutent_contracts::provider_role::{
    AmountMismatch, ConversionOutcome, Operation, RESULT_SHAPE, Refusal, Remainder, Reporting,
    ReserveMovementOutcome, ResultBody, Statement, Terms, TransferOutcome, less_the_charge,
};
use rutent_contracts::router_api::Id;
use rutent_prov::catalogue::{Catalogue, Instant};

/// Why a result doesn't count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotCounted {
    /// Its signature doesn't verify under a key its maker published for statements, or it is of another
    /// deployment or protocol version: it is attributable to no one.
    Unattributable,
    /// It isn't a result in its envelope.
    Unreadable,
    /// Its maker isn't the segment's provider.
    AnotherProvider,
    /// It answers another operation: another kind or another transaction.
    AnotherOperation,
    /// It is a dry run's answer, and the operation wasn't a dry run.
    DryRunAnswer,
    /// Its amounts break the rules against the operation (`check_amounts`).
    Amounts(Vec<AmountMismatch>),
    /// It gives out more than it took in, or, for a conversion, more than its cited quote and slippage allow, or
    /// a conversion it cites no bounded quote for.
    GivesOutMore,
}

/// The quote a conversion's operation cites: what it takes in, what it gives out, and how far its rate may move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quoted {
    /// What the quote takes in.
    pub amount_in: String,
    /// What it gives out for that.
    pub amount_out: String,
    /// How far the rate may move, in basis points: the tighter of the quote's own (0 for a firm rate) and the
    /// operator's limit for the tuple; `None` where neither bounds it.
    pub slippage_bps: Option<u16>,
}

/// Whether a conversion that took in `taken_in` may give out `claimed_out`: at most [`conversion_bound`], and never
/// without a bounded quote.
pub fn converts_within(taken_in: &str, claimed_out: &str, quoted: Option<&Quoted>) -> bool {
    match (
        quoted.and_then(|q| conversion_bound(taken_in, q)),
        whole(claimed_out),
    ) {
        (Some(bound), Some(out)) => out <= bound,
        _ => false,
    }
}

/// The most a conversion may give out for `taken_in`, by `quoted`: what the quote gives for what was taken in, moved
/// by its slippage at most, rounded down. `None` where nothing bounds it, or the arithmetic can't be carried.
pub fn conversion_bound(taken_in: &str, quoted: &Quoted) -> Option<u128> {
    let bps = u128::from(quoted.slippage_bps?);
    whole(&quoted.amount_out)?
        .checked_mul(whole(taken_in)?)?
        .checked_mul(10_000 + bps)?
        .checked_div(whole(&quoted.amount_in)?.checked_mul(10_000)?)
}

/// What a result that counts says of its segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Counted {
    /// It settled.
    Succeeded {
        /// What left the origin, the charge included.
        amount_in: String,
        /// What it says it gave out at its destination, as its tuple's terms report it.
        amount_out: String,
        /// What arrived net of its charge: the amount the next segment is handed (`REQ-ORD-21`). Under `net`
        /// reporting, `amount_out`; under `gross`, `amount_out` less the charge.
        handed_on: String,
        /// What stayed at the origin, where the operation fixed what it takes in.
        remainder: Option<Remainder>,
    },
    /// It failed.
    Failed,
    /// Its outcome is unknown, by the provider's own word.
    Unknown,
    /// The provider refused it, with its class.
    Refused(Refusal),
}

/// Judges `received`, the bytes of a result, as an answer to `operation`, the segment's, at `provider`, against the
/// tuple's `terms`, at Core's clock `at`; `units` are the decimals its two ends count in.
#[allow(clippy::too_many_arguments)]
pub fn judge(
    catalogue: &Catalogue,
    at: Instant,
    provider: &Id,
    operation: &Operation,
    terms: &Terms,
    quoted: Option<&Quoted>,
    units: Units,
    received: &[u8],
) -> Result<Counted, NotCounted> {
    catalogue
        .verify_statement(received, at)
        .map_err(|_| NotCounted::Unattributable)?;
    let result: Statement<ResultBody> =
        serde_json::from_slice(received).map_err(|_| NotCounted::Unreadable)?;
    if result.shape != RESULT_SHAPE {
        return Err(NotCounted::Unreadable);
    }
    if result.maker != provider.as_str() {
        return Err(NotCounted::AnotherProvider);
    }
    if transaction_of(operation) != transaction_named(&result.statement) {
        return Err(NotCounted::AnotherOperation);
    }
    let remainder =
        rutent_contracts::provider_role::check_amounts(operation, &result.statement, terms)
            .map_err(|broken| {
                if broken.contains(&AmountMismatch::AnswersAnotherOperation) {
                    NotCounted::AnotherOperation
                } else {
                    NotCounted::Amounts(broken)
                }
            })?;
    let settled = |amount_in: &str, amount_out: &str, handed_on: String| Counted::Succeeded {
        amount_in: amount_in.to_owned(),
        amount_out: amount_out.to_owned(),
        handed_on,
        remainder: remainder.clone(),
    };
    // Same asset: no more out than in, the charge counted as the tuple's terms report it.
    let kept = |amount_in: &str, amount_out: &str, fee: &str| match handed_on(
        terms.reporting,
        amount_in,
        amount_out,
        fee,
        units,
    ) {
        Some(handed) => Ok(settled(amount_in, amount_out, handed)),
        None => Err(NotCounted::GivesOutMore),
    };
    // A conversion: no more out than its quote gives for what it took in, moved by its slippage at most. Its charge
    // is in the asset it takes in, never the one it gives out, so under `gross` and `net` alike what it gives out is
    // handed on as it is: the charge can't be taken off an amount in another asset, and the quote and the slippage
    // bound already limit what it gives out for what it took in, the charge included.
    let converted = |amount_in: &str, amount_out: &str| {
        if converts_within(amount_in, amount_out, quoted) {
            Ok(settled(amount_in, amount_out, amount_out.to_owned()))
        } else {
            Err(NotCounted::GivesOutMore)
        }
    };
    match &result.statement {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match &r.outcome {
            ReserveMovementOutcome::Succeeded(s) => {
                kept(&s.amount_in, &s.amount_out, &s.fee.amount)
            }
            ReserveMovementOutcome::Failed(_) => Ok(Counted::Failed),
            ReserveMovementOutcome::Unknown(_) => Ok(Counted::Unknown),
            ReserveMovementOutcome::Refused(r) => Ok(Counted::Refused(r.refusal.clone())),
            ReserveMovementOutcome::DryRunAccepted(_) => Err(NotCounted::DryRunAnswer),
        },
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::Succeeded(s) => kept(&s.amount_in, &s.amount_out, &s.fee.amount),
            TransferOutcome::Failed(_) => Ok(Counted::Failed),
            TransferOutcome::Unknown(_) => Ok(Counted::Unknown),
            TransferOutcome::Refused(r) => Ok(Counted::Refused(r.refusal.clone())),
            TransferOutcome::DryRunAccepted(_) => Err(NotCounted::DryRunAnswer),
        },
        ResultBody::Conversion(r) => match &r.outcome {
            ConversionOutcome::Succeeded(s) => converted(&s.amount_in, &s.amount_out),
            ConversionOutcome::Failed(_) => Ok(Counted::Failed),
            ConversionOutcome::Unknown(_) => Ok(Counted::Unknown),
            ConversionOutcome::Refused(r) => Ok(Counted::Refused(r.refusal.clone())),
            ConversionOutcome::DryRunAccepted(_) => Err(NotCounted::DryRunAnswer),
        },
        _ => Err(NotCounted::AnotherOperation),
    }
}

/// The units a segment counts in: its origin's decimals, the unit `amount_in` and the fee are stated in, and its out
/// asset's, the unit `amount_out` is stated in. A segment that keeps its asset may count it in different decimals at
/// its two ends (`REQ-CAT-11`), so no amount of one unit is compared with, or subtracted from, one of the other
/// without restating it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Units {
    /// The origin's decimals: `amount_in` and the fee are counted in them.
    pub origin_decimals: u8,
    /// The out asset's decimals: `amount_out` is counted in them.
    pub out_decimals: u8,
}

/// What a segment that keeps its asset hands on, by how its tuple reports its charge (`REQ-CAT-11`): under `net`,
/// what it gave out, which with its charge is at most what it took in; under `gross`, what it gave out, at most what
/// it took in, less its charge, which it includes, restated in the out unit and rounded up, so what arrives is never
/// overstated (provider-role's `less_the_charge`). Every comparison is made in the finer of the two units, exactly.
/// `None` where the amounts break that, or can't be read.
pub fn handed_on(
    reporting: Reporting,
    amount_in: &str,
    amount_out: &str,
    fee: &str,
    units: Units,
) -> Option<String> {
    let (taken, out, charge) = (whole(amount_in)?, whole(amount_out)?, whole(fee)?);
    // Both in the finer unit: the origin's amounts scaled up to it, or the out amount.
    let finer = units.origin_decimals.max(units.out_decimals);
    let scaled = |amount: u128, decimals: u8| -> Option<u128> {
        amount.checked_mul(10u128.checked_pow(u32::from(finer - decimals))?)
    };
    let (taken, charge) = (
        scaled(taken, units.origin_decimals)?,
        scaled(charge, units.origin_decimals)?,
    );
    let out_scaled = scaled(out, units.out_decimals)?;
    match reporting {
        Reporting::Net => (out_scaled.checked_add(charge)? <= taken).then(|| out.to_string()),
        Reporting::Gross => {
            if out_scaled > taken {
                return None;
            }
            less_the_charge(amount_out, fee, units.origin_decimals, units.out_decimals)
        }
    }
}

/// A whole amount in decimal digits, if it fits.
fn whole(text: &str) -> Option<u128> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// The transaction a value-moving operation names.
fn transaction_of(operation: &Operation) -> Option<&str> {
    match operation {
        Operation::Withdrawal(o) => Some(&o.transaction),
        Operation::Deposit(o) => Some(&o.transaction),
        Operation::Transfer(o) => Some(&o.transaction),
        Operation::Conversion(o) => Some(&o.transaction),
        _ => None,
    }
}

/// The transaction a result names, if it is a value-moving operation's.
fn transaction_named(result: &ResultBody) -> Option<&str> {
    match result {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => Some(&r.transaction),
        ResultBody::Transfer(r) => Some(&r.transaction),
        ResultBody::Conversion(r) => Some(&r.transaction),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Quoted, Units, conversion_bound, converts_within, handed_on};
    use rutent_contracts::provider_role::Reporting;

    /// Both ends count in six decimals.
    const SAME: Units = Units {
        origin_decimals: 6,
        out_decimals: 6,
    };

    /// What a segment that keeps its asset hands on, with a charge: under `net`, what it gave out, its charge on top
    /// within what it took in; under `gross`, what it gave out less the charge it includes.
    // verifies: REQ-ORD-21, INV-1
    #[test]
    fn what_a_segment_hands_on_follows_how_its_tuple_reports_its_charge() {
        // Net: 999 000 out with a charge of 1 000 from 1 000 000 in.
        assert_eq!(
            handed_on(Reporting::Net, "1000000", "999000", "1000", SAME),
            Some("999000".into())
        );
        // Net: out and charge together more than came in.
        assert_eq!(
            handed_on(Reporting::Net, "1000000", "999500", "1000", SAME),
            None
        );
        // Gross: 1 000 000 out, the charge of 1 000 within it, so 999 000 is handed on.
        assert_eq!(
            handed_on(Reporting::Gross, "1000000", "1000000", "1000", SAME),
            Some("999000".into())
        );
        // Gross: more out than in.
        assert_eq!(
            handed_on(Reporting::Gross, "1000000", "1000001", "1000", SAME),
            None
        );
        // Gross: a charge above what it gave out.
        assert_eq!(
            handed_on(Reporting::Gross, "1000", "500", "1000", SAME),
            None
        );
    }

    /// A segment whose two ends count its asset in different decimals compares and subtracts in like units: a charge
    /// stated in the origin's is restated in the out unit, multiplied out where that is finer and divided and rounded up
    /// where it is coarser, so what arrives is never overstated.
    // verifies: REQ-CAT-11, INV-1
    #[test]
    fn a_segment_whose_ends_count_in_different_decimals_hands_on_in_like_units() {
        // Out counts in eight decimals, origin in six: 1.000000 in, a charge of 0.001000 (1 000 origin units, 100 000
        // out units), and 1.00000000 (100 000 000) out under gross, so 99 900 000 is handed on, not 99 999 000.
        let finer = Units {
            origin_decimals: 6,
            out_decimals: 8,
        };
        assert_eq!(
            handed_on(Reporting::Gross, "1000000", "100000000", "1000", finer),
            Some("99900000".into())
        );
        // More out than in, once both are in the finer unit: 1.00000001.
        assert_eq!(
            handed_on(Reporting::Gross, "1000000", "100000001", "1000", finer),
            None
        );
        // Net: 0.999 out with a charge of 0.001 is exactly what came in; one finer unit more is not.
        assert_eq!(
            handed_on(Reporting::Net, "1000000", "99900000", "1000", finer),
            Some("99900000".into())
        );
        assert_eq!(
            handed_on(Reporting::Net, "1000000", "99900001", "1000", finer),
            None
        );
        // Out counts in fewer decimals, origin in eight: 1.00000000 in, 1.000000 out under gross, a charge of one
        // origin unit rounds up to one out unit, so 999 999 is handed on.
        let coarser = Units {
            origin_decimals: 8,
            out_decimals: 6,
        };
        assert_eq!(
            handed_on(Reporting::Gross, "100000000", "1000000", "1", coarser),
            Some("999999".into())
        );
        // Net: 1.000000 out with a charge of one origin unit is more than 1.00000000 in.
        assert_eq!(
            handed_on(Reporting::Net, "100000000", "1000000", "1", coarser),
            None
        );
        assert_eq!(
            handed_on(Reporting::Net, "100000001", "1000000", "1", coarser),
            Some("1000000".into())
        );
        // A charge above what was given out, once restated: 0.5 out at six decimals, a charge of 0.6 at eight.
        assert_eq!(
            handed_on(Reporting::Gross, "100000000", "500000", "60000000", coarser),
            None
        );
    }

    fn quoted(slippage_bps: Option<u16>) -> Quoted {
        Quoted {
            amount_in: "1000000".into(),
            amount_out: "250000".into(),
            slippage_bps,
        }
    }

    /// A conversion gives out at most what its quote gives for what it took in, moved by its slippage: a firm rate not
    /// at all, a bounded one by its basis points, the operator's limit where tighter (folded into `slippage_bps` by
    /// the caller), and nothing bounds a floating rate with no limit.
    // verifies: REQ-ORD-21, INV-1
    #[test]
    fn a_conversion_gives_out_no_more_than_its_quote_and_slippage_allow() {
        // Firm: exactly the quote's rate, scaled to what was taken in.
        assert_eq!(conversion_bound("1000000", &quoted(Some(0))), Some(250_000));
        assert_eq!(conversion_bound("500000", &quoted(Some(0))), Some(125_000));
        // Bounded, 50 basis points: 250 000 × 1.005.
        assert_eq!(
            conversion_bound("1000000", &quoted(Some(50))),
            Some(251_250)
        );
        // A tighter operator's limit is the slippage the caller passes.
        assert_eq!(
            conversion_bound("1000000", &quoted(Some(10))),
            Some(250_250)
        );
        // Floating, with no operator's limit: no bound.
        assert_eq!(conversion_bound("1000000", &quoted(None)), None);
        // What can't be read bounds nothing.
        assert_eq!(conversion_bound("ten", &quoted(Some(0))), None);
        // At the bound counts; one unit over doesn't; no quote, or a floating one, never does.
        let bounded = quoted(Some(50));
        assert!(converts_within("1000000", "251250", Some(&bounded)));
        assert!(!converts_within("1000000", "251251", Some(&bounded)));
        assert!(!converts_within("1000000", "1", None));
        assert!(!converts_within("1000000", "1", Some(&quoted(None))));
        // The operator's tighter limit, as the caller folds it in, binds where the quote's own wouldn't.
        assert!(!converts_within(
            "1000000",
            "251250",
            Some(&quoted(Some(10)))
        ));
    }
}
