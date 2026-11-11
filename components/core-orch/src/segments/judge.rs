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
    match &result.statement {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match &r.outcome {
            ReserveMovementOutcome::Succeeded(s) => Ok(settled(&s.amount_in, &s.amount_out)),
            ReserveMovementOutcome::Failed(_) => Ok(Counted::Failed),
            ReserveMovementOutcome::Unknown(_) => Ok(Counted::Unknown),
            ReserveMovementOutcome::Refused(r) => Ok(Counted::Refused(r.refusal.clone())),
            ReserveMovementOutcome::DryRunAccepted(_) => Err(NotCounted::DryRunAnswer),
        },
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::Succeeded(s) => Ok(settled(&s.amount_in, &s.amount_out)),
            TransferOutcome::Failed(_) => Ok(Counted::Failed),
            TransferOutcome::Unknown(_) => Ok(Counted::Unknown),
            TransferOutcome::Refused(r) => Ok(Counted::Refused(r.refusal.clone())),
            TransferOutcome::DryRunAccepted(_) => Err(NotCounted::DryRunAnswer),
        },
        ResultBody::Conversion(r) => match &r.outcome {
            ConversionOutcome::Succeeded(s) => Ok(settled(&s.amount_in, &s.amount_out)),
            ConversionOutcome::Failed(_) => Ok(Counted::Failed),
            ConversionOutcome::Unknown(_) => Ok(Counted::Unknown),
            ConversionOutcome::Refused(r) => Ok(Counted::Refused(r.refusal.clone())),
            ConversionOutcome::DryRunAccepted(_) => Err(NotCounted::DryRunAnswer),
        },
        _ => Err(NotCounted::AnotherOperation),
    }
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
