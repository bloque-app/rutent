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
//!    conversion at most what the quote its operation cites gives for what it took in, moved by no more than that
//!    quote's slippage (`REQ-ORD-21`, `INV-1`). A conversion with no such quote, or a floating rate, has no bound
//!    and doesn't count. `check_amounts` holds what is taken in, never what is given out, so without this a result
//!    could hand the next segment more than arrived.
//!
//! A result that doesn't count is no result: the segment waits for one that does, and no outcome is recorded on it.

use rutent_contracts::provider_role::{
    AmountMismatch, ConversionOutcome, Operation, RESULT_SHAPE, Refusal, Remainder,
    ReserveMovementOutcome, ResultBody, Statement, Terms, TransferOutcome,
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
    /// How far the rate may move against it, in basis points: 0 for a firm rate, `None` for a floating one.
    pub slippage_bps: Option<u16>,
}

/// What a result that counts says of its segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Counted {
    /// It settled.
    Succeeded {
        /// What left the origin, the charge included.
        amount_in: String,
        /// What it gave out at its destination: the amount the next segment is handed (`REQ-ORD-21`).
        amount_out: String,
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
/// tuple's `terms`, at Core's clock `at`.
pub fn judge(
    catalogue: &Catalogue,
    at: Instant,
    provider: &Id,
    operation: &Operation,
    terms: &Terms,
    quoted: Option<&Quoted>,
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
    let settled = |amount_in: &str, amount_out: &str| Counted::Succeeded {
        amount_in: amount_in.to_owned(),
        amount_out: amount_out.to_owned(),
        remainder: remainder.clone(),
    };
    // Same asset: no more out than in.
    let kept = |amount_in: &str, amount_out: &str| match (whole(amount_in), whole(amount_out)) {
        (Some(i), Some(o)) if o <= i => Ok(settled(amount_in, amount_out)),
        _ => Err(NotCounted::GivesOutMore),
    };
    // A conversion: no more out than its quote gives for what it took in, moved by its slippage at most.
    let converted = |amount_in: &str, amount_out: &str| {
        let bound = quoted.and_then(|q| {
            let bps = u128::from(q.slippage_bps?);
            whole(&q.amount_out)?
                .checked_mul(whole(amount_in)?)?
                .checked_mul(10_000 + bps)?
                .checked_div(whole(&q.amount_in)?.checked_mul(10_000)?)
        });
        match (bound, whole(amount_out)) {
            (Some(bound), Some(o)) if o <= bound => Ok(settled(amount_in, amount_out)),
            _ => Err(NotCounted::GivesOutMore),
        }
    };
    match &result.statement {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match &r.outcome {
            ReserveMovementOutcome::Succeeded(s) => kept(&s.amount_in, &s.amount_out),
            ReserveMovementOutcome::Failed(_) => Ok(Counted::Failed),
            ReserveMovementOutcome::Unknown(_) => Ok(Counted::Unknown),
            ReserveMovementOutcome::Refused(r) => Ok(Counted::Refused(r.refusal.clone())),
            ReserveMovementOutcome::DryRunAccepted(_) => Err(NotCounted::DryRunAnswer),
        },
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::Succeeded(s) => kept(&s.amount_in, &s.amount_out),
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
