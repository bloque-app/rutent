//! Results a provider gives for an operation (`urn:rutent:provider-role:result`, `REQ-CAT-9`, `REQ-CAT-14`,
//! `REQ-CAT-15`, `REQ-CAT-18`, `REQ-ORD-1`, `INV-9`), the verifier's standalone judgement
//! (`urn:rutent:provider-role:judgement`), and the reserve's reversal confirmation
//! (`urn:rutent:provider-role:reversal-confirmation`, ADR-0008). On the wire each is its maker's [`Statement`].

use alloc::{boxed::Box, format, string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use super::envelope::{
    ContextMismatch, Deployment, JUDGEMENT_SHAPE, ProtocolVersions, RESULT_SHAPE,
    REVERSAL_CONFIRMATION_SHAPE, ShapeMismatch, Statement, StatementShape,
};
use super::inner::InnerSegments;
use super::operation::Operation;
use super::stated::{Hold, TupleTerms};
use super::types::{EvidenceGrade, SafeInteger};
use super::validate::made_after;

/// A provider's result for one operation, by the operation's kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResultBody {
    /// A withdrawal's result.
    Withdrawal(ReserveMovementResult),
    /// A deposit's result.
    Deposit(ReserveMovementResult),
    /// A transfer's result.
    Transfer(TransferResult),
    /// A conversion's result.
    Conversion(ConversionResult),
    /// A balance verification's answer.
    BalanceVerification(BalanceVerificationResult),
    /// A judgement.
    Judgement(JudgementResult),
    /// An exchange's or ramp's reversal's result.
    Reversal(ReversalResult),
    /// The origin reserve's answer about one reversed credit.
    ReversalConfirmation(ReversalConfirmationResult),
    /// A holding account's balance.
    HoldingBalance(HoldingBalanceResult),
    /// A provider's answer to a status question (4.0.0).
    StatusQuery(StatusQueryResult),
    /// A reserve's answer to a hold request (4.0.0).
    Hold(HoldResult),
    /// A provider's answer to the question of a private node's record (4.0.0).
    InnerSegmentsQuery(InnerSegmentsQueryResult),
}

impl ResultBody {
    /// The transaction the result answers: every result's but a judgement's, which names none (`REQ-CPL-4`).
    pub fn transaction(&self) -> Option<&str> {
        match self {
            Self::Withdrawal(r) | Self::Deposit(r) => Some(&r.transaction),
            Self::Transfer(r) => Some(&r.transaction),
            Self::Conversion(r) => Some(&r.transaction),
            Self::BalanceVerification(r) => Some(&r.transaction),
            Self::Judgement(_) => None,
            Self::Reversal(r) => Some(&r.transaction),
            Self::ReversalConfirmation(r) => Some(&r.transaction),
            Self::HoldingBalance(r) => Some(&r.transaction),
            Self::StatusQuery(r) => Some(&r.transaction),
            Self::Hold(r) => Some(&r.transaction),
            Self::InnerSegmentsQuery(r) => Some(&r.transaction),
        }
    }

    /// The provider's signed terms the result hands back, if it carries any (4.0.0): a withdrawal's, deposit's,
    /// transfer's or conversion's dry-run answer's.
    pub fn terms(&self) -> Option<&Statement<TupleTerms>> {
        match self {
            Self::Withdrawal(ReserveMovementResult {
                outcome: ReserveMovementOutcome::DryRunAccepted(a),
                ..
            })
            | Self::Deposit(ReserveMovementResult {
                outcome: ReserveMovementOutcome::DryRunAccepted(a),
                ..
            }) => a.terms.as_deref(),
            Self::Transfer(TransferResult {
                outcome: TransferOutcome::DryRunAccepted(q),
                ..
            }) => q.terms.as_deref(),
            Self::Conversion(ConversionResult {
                outcome: ConversionOutcome::DryRunAccepted(q),
                ..
            }) => q.terms.as_deref(),
            _ => None,
        }
    }
}

impl StatementShape for ResultBody {
    const SHAPE: &'static str = RESULT_SHAPE;

    fn check_nested_shapes(&self) -> Result<(), ShapeMismatch> {
        match self {
            Self::Judgement(JudgementResult {
                outcome: JudgementOutcome::Judged(judged),
            }) => judged.judgement.check_shape(),
            Self::ReversalConfirmation(ReversalConfirmationResult {
                outcome: ReversalConfirmationOutcome::Confirmed(confirmed),
                ..
            }) => confirmed.confirmation.check_shape(),
            Self::StatusQuery(StatusQueryResult {
                outcome: StatusOutcome::Answered(answered),
                ..
            }) => answered.result.check_shape(),
            Self::Hold(HoldResult {
                outcome: HoldOutcome::Held(held),
                ..
            }) => held.hold.check_shape(),
            Self::InnerSegmentsQuery(InnerSegmentsQueryResult {
                outcome: InnerSegmentsOutcome::Answered(answered),
                ..
            }) => answered.record.check_shape(),
            _ => self.terms().map_or(Ok(()), Statement::check_shape),
        }
    }

    fn check_nested_context(
        &self,
        maker: &str,
        deployment: &Deployment,
        versions: &ProtocolVersions,
        at: &str,
    ) -> Result<(), ContextMismatch> {
        match self {
            Self::Judgement(JudgementResult {
                outcome: JudgementOutcome::Judged(judged),
            }) => judged
                .judgement
                .check_as_nested(maker, deployment, versions, at),
            Self::ReversalConfirmation(ReversalConfirmationResult {
                outcome: ReversalConfirmationOutcome::Confirmed(confirmed),
                ..
            }) => confirmed
                .confirmation
                .check_as_nested(maker, deployment, versions, at),
            Self::StatusQuery(StatusQueryResult {
                outcome: StatusOutcome::Answered(answered),
                ..
            }) => answered
                .result
                .check_as_nested(maker, deployment, versions, at),
            Self::Hold(HoldResult {
                outcome: HoldOutcome::Held(held),
                ..
            }) => held.hold.check_as_nested(maker, deployment, versions, at),
            Self::InnerSegmentsQuery(InnerSegmentsQueryResult {
                outcome: InnerSegmentsOutcome::Answered(answered),
                ..
            }) => answered
                .record
                .check_as_nested(maker, deployment, versions, at),
            _ => self.terms().map_or(Ok(()), |terms| {
                terms.check_as_nested(maker, deployment, versions, at)
            }),
        }
    }
}

/// The result of a withdrawal or deposit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveMovementResult {
    /// The transaction this answers; it counts only if it matches (`REQ-ORD-1`).
    pub transaction: String,
    /// The outcome.
    pub outcome: ReserveMovementOutcome,
}

/// The result of a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferResult {
    /// The transaction this answers; it counts only if it matches (`REQ-ORD-1`).
    pub transaction: String,
    /// The outcome.
    pub outcome: TransferOutcome,
}

/// The result of a conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversionResult {
    /// The transaction this answers; it counts only if it matches (`REQ-ORD-1`).
    pub transaction: String,
    /// The outcome.
    pub outcome: ConversionOutcome,
}

/// The answer to a balance verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BalanceVerificationResult {
    /// The transaction this answers.
    pub transaction: String,
    /// The outcome.
    pub outcome: BalanceOutcome,
}

/// The answer to a holding account's balance question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldingBalanceResult {
    /// The transaction this answers; it counts only if it matches (`REQ-ORD-1`).
    pub transaction: String,
    /// The outcome.
    pub outcome: HoldingBalanceOutcome,
}

/// A holding account's balance outcome: the figure it holds, in the asset asked, or a refusal. The figure is the
/// module's own word (`REQ-AUD-15`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HoldingBalanceOutcome {
    /// The balance.
    Answered(HoldingBalanceAnswer),
    /// The module refused, with its class (`REQ-CAT-18`).
    Refused(Refused),
}

/// The balance a holding account states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldingBalanceAnswer {
    /// What the account holds at the node, in whole units of the asset asked.
    pub balance: String,
}

/// A verifier's result. Like the question (`REQ-CPL-4`), it names no transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JudgementResult {
    /// The outcome.
    pub outcome: JudgementOutcome,
}

/// The result of a reversal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReversalResult {
    /// The transaction this answers: the reversal's own (`REQ-ORD-1`).
    pub transaction: String,
    /// The outcome.
    pub outcome: ReversalOutcome,
}

/// The origin reserve's answer about one reversed credit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReversalConfirmationResult {
    /// The transaction this answers: the question's own (`REQ-ORD-1`).
    pub transaction: String,
    /// The outcome.
    pub outcome: ReversalConfirmationOutcome,
}

/// A withdrawal's or deposit's outcome. Succeeded and failed each rest on stated evidence; unknown is its own
/// result (`REQ-ORD-1`). A dry run is answered by `DryRunAccepted` or a refusal (`REQ-CAT-14`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReserveMovementOutcome {
    /// The segment succeeded.
    Succeeded(MovementSettlement),
    /// The segment failed.
    Failed(Failure),
    /// Outcome unknown: neither success nor failure.
    Unknown(Unknown),
    /// The provider refused, with its class (`REQ-CAT-18`).
    Refused(Refused),
    /// A dry run the reserve would serve.
    DryRunAccepted(ReserveDryRunAnswer),
}

/// A transfer's outcome: as [`ReserveMovementOutcome`], with the ramp's quote for a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TransferOutcome {
    /// The segment succeeded.
    Succeeded(MovementSettlement),
    /// The segment failed.
    Failed(Failure),
    /// Outcome unknown: neither success nor failure.
    Unknown(Unknown),
    /// The provider refused, with its class (`REQ-CAT-18`).
    Refused(Refused),
    /// The ramp's quote, for a dry run.
    DryRunAccepted(TransferQuote),
}

/// A conversion's outcome: as [`ReserveMovementOutcome`], with the rate an exchange exposes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConversionOutcome {
    /// The conversion succeeded.
    Succeeded(ConversionSettlement),
    /// The conversion failed.
    Failed(Failure),
    /// Outcome unknown: neither success nor failure.
    Unknown(Unknown),
    /// The provider refused, with its class (`REQ-CAT-18`).
    Refused(Refused),
    /// The exchange's detailed quote, for a dry run.
    DryRunAccepted(ConversionQuote),
}

/// A balance verification's outcome: yes or no and nothing else (`INV-9`), or a refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BalanceOutcome {
    /// The answer.
    Answered(BalanceAnswer),
    /// The reserve refused, without saying when it lifts.
    Refused(BalanceRefused),
}

/// A judgement's outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JudgementOutcome {
    /// The verifier's judgement, whole.
    Judged(Judged),
    /// The verifier refused, with its class.
    Refused(Refused),
}

/// A reversal's outcome: a true undo or nothing (`REQ-CAT-15`). A refusal leaves the reversal owed
/// (`REQ-CAT-7`). The provider's own word that it succeeded doesn't complete it (`REQ-ORD-3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReversalOutcome {
    /// The provider says the value is back where it left, in full.
    Succeeded(ReversalSettlement),
    /// Outcome unknown: neither success nor failure.
    Unknown(Unknown),
    /// The provider refused, with its class (`REQ-CAT-18`); the reversal stays owed.
    Refused(Refused),
}

/// The origin reserve's answer about one reversed credit, and nothing about sufficiency (ADR-0008, `REQ-CAT-4`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReversalConfirmationOutcome {
    /// The credit arrived, whole: the reserve's own signed statement.
    Confirmed(Confirmed),
    /// No credit matching the segment, amount and asset has arrived. It says nothing else.
    NotConfirmed(NoDetail),
    /// The reserve refused, with its class.
    Refused(Refused),
}

/// A succeeded reversal, as the provider states it. Completion needs another party's statement, such as the
/// origin reserve's confirmation (`REQ-ORD-3`, ADR-0008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReversalSettlement {
    /// Where the value is back: the segment's origin node.
    pub node: String,
    /// The asset it is back in: the one it left in.
    pub asset: String,
    /// How much is back, in full, in the origin unit.
    pub amount: String,
    /// The evidence for the outcome: at least one piece.
    pub evidence: Vec<EvidencePiece>,
}

/// A confirmed credit, carried whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirmed {
    /// The reserve's own enveloped confirmation, boxed since it is a whole statement.
    pub confirmation: Box<Statement<ReversalConfirmation>>,
}

/// That one reversed credit arrived (ADR-0008): the segment's transaction, amount and asset, and nothing else.
/// Its maker and time made are its envelope's. It is recorded as the reserve's word (`REQ-ORD-3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReversalConfirmation {
    /// The transaction of the reversed segment.
    pub segment: String,
    /// The asset the credit arrived in.
    pub asset: String,
    /// The amount that arrived.
    pub amount: String,
}

impl StatementShape for ReversalConfirmation {
    const SHAPE: &'static str = REVERSAL_CONFIRMATION_SHAPE;
}

/// A succeeded withdrawal, deposit or transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementSettlement {
    /// What left the origin, in the origin unit, the charge included whether the tuple reports gross or net.
    pub amount_in: String,
    /// What it gave out, in the destination unit, gross or net as its tuple declares.
    pub amount_out: String,
    /// Its fee.
    pub fee: Fee,
    /// The evidence for the outcome: at least one piece.
    pub evidence: Vec<EvidencePiece>,
}

/// A succeeded conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversionSettlement {
    /// What left the origin, in the from unit, the charge included.
    pub amount_in: String,
    /// What it gave out, in the to unit.
    pub amount_out: String,
    /// Whole to-assets per whole from-asset, before the fee.
    pub rate: String,
    /// Its fee.
    pub fee: Fee,
    /// The evidence for the outcome: at least one piece.
    pub evidence: Vec<EvidencePiece>,
}

/// A failed segment, on stated evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    /// The evidence for the failure: at least one piece.
    pub evidence: Vec<EvidencePiece>,
    /// Whose fault the provider shows the failure was, where it shows one: the customer's or the operator's. Until
    /// the party it names accepts it, the provider carries the reversal (`REQ-ORD-11`). Absent where it shows none.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub fault: Option<Fault>,
}

/// Whose fault a provider shows a failure was (`REQ-ORD-11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    /// The customer's.
    Customer,
    /// The operator's.
    Operator,
}

/// Outcome unknown. It carries nothing, and is left only on a later result with stated evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unknown {}

/// A refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refused {
    /// Why, by class.
    pub refusal: Refusal,
}

/// A refusal's class (`REQ-CAT-18`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum Refusal {
    /// It lifts at a known time.
    LiftsAtKnownTime(LiftsAt),
    /// It waits on the person.
    WaitsOnPerson(NoDetail),
    /// It is final.
    Final(NoDetail),
}

/// A refused balance verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BalanceRefused {
    /// Why, by class.
    pub refusal: BalanceRefusal,
}

/// A balance refusal's class. It never says when it lifts, since that would reveal timing (`INV-9`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum BalanceRefusal {
    /// It waits on the person.
    WaitsOnPerson(NoDetail),
    /// It is final.
    Final(NoDetail),
}

/// When a refusal lifts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiftsAt {
    /// The time, as an RFC 3339 UTC time.
    pub lifts_at: String,
}

/// A class that carries nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoDetail {}

/// What a withdrawal or deposit would do (`REQ-CAT-14`), with what voids the answer and the settlement time
/// (`REQ-QTE-1`, `REQ-QTE-3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveDryRunAnswer {
    /// What the reserve would take in.
    pub amount_in: String,
    /// What it would give out.
    pub amount_out: String,
    /// Its fee.
    pub fee: Fee,
    /// Until when the answer holds.
    pub valid_until: String,
    /// What voids it: at least one.
    pub voided_by: Vec<VoidedBy>,
    /// The declared settlement time, in seconds from segment start.
    pub settlement_seconds: SafeInteger,
    /// The provider's own signed terms for the tuple the dry run asked about, handed back with its answer (finding 4
    /// of `M3`'s milestone review, 4.0.0), so a reader holds them stated before the segment starts (`REQ-QTE-6`). Made
    /// by the result's maker, as a nested statement. Absent where it hands back none.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub terms: Option<Box<Statement<TupleTerms>>>,
}

/// Which amount a quote or operation fixes (`REQ-QTE-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// The amount taken in is fixed.
    ExactIn,
    /// The amount given out is fixed.
    ExactOut,
}

/// What voids a quote, besides its validity passing (`REQ-QTE-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoidedBy {
    /// The validity passes.
    ValidityPasses,
    /// The amount changes.
    AmountChanges,
    /// The inputs change.
    InputsChange,
    /// The rate moves beyond the slippage regime.
    RateMovesBeyondSlippage,
    /// The provider stops advertising the tuple.
    ProviderStopsAdvertising,
}

/// A ramp's quote, signed by the provider (`REQ-CAT-9`, `REQ-QTE-1`, `REQ-QTE-3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferQuote {
    /// What the ramp would take in.
    pub amount_in: String,
    /// What it would give out.
    pub amount_out: String,
    /// Its fee.
    pub fee: Fee,
    /// Which amount the quote fixes.
    pub direction: Direction,
    /// Until when the quote holds.
    pub valid_until: String,
    /// What voids it: at least one.
    pub voided_by: Vec<VoidedBy>,
    /// The declared settlement time, in seconds from segment start.
    pub settlement_seconds: SafeInteger,
    /// The provider's own signed terms for the tuple the dry run asked about, handed back with its answer (finding 4
    /// of `M3`'s milestone review, 4.0.0), so a reader holds them stated before the segment starts (`REQ-QTE-6`). Made
    /// by the result's maker, as a nested statement. Absent where it hands back none.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub terms: Option<Box<Statement<TupleTerms>>>,
}

/// An exchange's detailed quote, signed by the provider (`REQ-CAT-9`, `REQ-QTE-1`, `REQ-QTE-3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversionQuote {
    /// What the exchange would take in.
    pub amount_in: String,
    /// What it would give out.
    pub amount_out: String,
    /// Whole to-assets per whole from-asset, before the fee.
    pub rate: String,
    /// The rate's slippage regime.
    pub slippage: Slippage,
    /// Its fee.
    pub fee: Fee,
    /// Which amount the quote fixes.
    pub direction: Direction,
    /// Until when the quote holds.
    pub valid_until: String,
    /// What voids it: at least one.
    pub voided_by: Vec<VoidedBy>,
    /// The declared settlement time, in seconds from segment start.
    pub settlement_seconds: SafeInteger,
    /// The provider's own signed terms for the tuple the dry run asked about, handed back with its answer (finding 4
    /// of `M3`'s milestone review, 4.0.0), so a reader holds them stated before the segment starts (`REQ-QTE-6`). Made
    /// by the result's maker, as a nested statement. Absent where it hands back none.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub terms: Option<Box<Statement<TupleTerms>>>,
}

/// A rate's slippage regime (`REQ-QTE-1`, `REQ-QTE-9`, `REQ-QTE-10`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "regime", rename_all = "snake_case")]
pub enum Slippage {
    /// The rate is firm.
    Firm(NoDetail),
    /// The rate moves at most this far.
    Bounded(BoundedSlippage),
    /// No firm rate.
    Floating(NoDetail),
}

/// A bounded slippage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundedSlippage {
    /// The largest move, in basis points, 1 to 10000.
    pub max_bps: BasisPoints,
}

/// Basis points, 1 to 10000.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct BasisPoints(u16);

impl BasisPoints {
    /// The value.
    pub fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for BasisPoints {
    type Error = String;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if (1..=10000).contains(&value) {
            Ok(Self(value))
        } else {
            Err(format!("{value} basis points is outside 1 to 10000"))
        }
    }
}

impl From<BasisPoints> for u16 {
    fn from(value: BasisPoints) -> Self {
        value.0
    }
}

/// Whether funds suffice for the stated amount, and nothing else (`INV-9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BalanceAnswer {
    /// Yes or no.
    pub sufficient: bool,
}

/// A judgement, carried whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Judged {
    /// The verifier's own enveloped judgement, boxed since it is a whole statement.
    pub judgement: Box<Statement<Judgement>>,
}

/// A judgement for one class (ADR-0012, `REQ-CPL-1`). Its maker and time made are its envelope's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Judgement {
    /// The subject reference, the one given to this relying provider.
    pub subject: String,
    /// The class judged.
    pub class: String,
    /// Yes or no.
    pub answer: JudgementAnswer,
    /// The judgement's validity, as an RFC 3339 UTC time.
    pub valid_until: String,
    /// The holder's public key, when the judgement is handed to its holder to carry.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub holder_key: Option<super::types::PublicKey>,
}

impl StatementShape for Judgement {
    const SHAPE: &'static str = JUDGEMENT_SHAPE;
}

/// A verifier's answer. "Not known" and "not asked" are rutent's own states, never a verifier's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgementAnswer {
    /// Yes.
    Yes,
    /// No.
    No,
}

/// A fee (`REQ-CAT-9`, `REQ-QTE-5`), in the asset the segment takes in: a withdrawal's, deposit's or transfer's
/// `asset`, a conversion's `from`. [`check_amounts`](super::check_amounts) holds it to that asset and to at most
/// `amount_in`, as the router does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fee {
    /// The asset: the one the segment takes in.
    pub asset: String,
    /// The amount, in the origin unit (a conversion's `from` unit).
    pub amount: String,
    /// Whether the provider bills it separately, outside the amounts it reports (`REQ-AUD-11`, since 3.1.0). Absent
    /// where it doesn't say: a fee record then says nothing either way.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub billed_separately: Option<bool>,
}

/// One piece of evidence for an outcome, graded, naming whose it is (`REQ-ORD-33`, `REQ-ORD-39`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencePiece {
    /// The grade.
    pub grade: EvidenceGrade,
    /// Where the evidence is.
    pub reference: String,
    /// Whose evidence it is.
    pub by: String,
}

/// A provider's answer to a status question (`REQ-ORD-1`; 4.0.0): the question's own transaction, the transaction
/// asked about, and where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusQueryResult {
    /// The transaction this answers: the question's own (`REQ-ORD-1`).
    pub transaction: String,
    /// The transaction asked about.
    pub about: String,
    /// Where it stands.
    pub outcome: StatusOutcome,
}

/// Where a transaction asked about stands, by `status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StatusOutcome {
    /// The provider's own result for the transaction asked about, whole: what it answers of the operation it was
    /// handed under it, as it would have answered it then.
    Answered(StatusAnswered),
    /// The provider knows no outcome of it yet, or no such transaction. It says nothing else.
    NotKnown(NoDetail),
    /// The provider refused, with its class (`REQ-CAT-18`).
    Refused(Refused),
}

/// A status answered, carried whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusAnswered {
    /// The provider's own enveloped result for the transaction asked about, made by the answer's maker. Never a
    /// status answer or a hold of its own.
    pub result: Box<Statement<ResultBody>>,
}

/// A reserve's answer to a hold request (`REQ-QTE-15`; 4.0.0): the request's transaction, which is the hold's own,
/// and the hold or a refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldResult {
    /// The transaction this answers: the request's, the hold's own id.
    pub transaction: String,
    /// The hold, or a refusal.
    pub outcome: HoldOutcome,
}

/// A hold request's outcome, by `status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HoldOutcome {
    /// The reserve's own signed hold, whole.
    Held(Held),
    /// The reserve refused, with its class (`REQ-CAT-18`): nothing is held.
    Refused(Refused),
}

/// A hold, carried whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Held {
    /// The reserve's own enveloped hold, made by the answer's maker.
    pub hold: Box<Statement<Hold>>,
}

/// How a status answer differs from the question it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusMismatch {
    /// It names another question's transaction.
    AnswersAnotherQuestion,
    /// It is about another transaction than the one asked about.
    AboutAnotherTransaction,
    /// The result it carries answers another transaction than the one asked about.
    CarriesAnotherTransactionsResult,
    /// The result it carries is a status answer or a hold, not a result of the transaction's own operation.
    CarriesNoOperationsResult,
    /// It was made by another provider than the one asked (the review of `1bb5f4cb`, as `check_answer`'s
    /// `OfAnotherReserve`).
    AnsweredByAnotherProvider,
    /// The question isn't a status question, or the answer isn't a status answer.
    NotAStatusQuestion,
}

/// Holds a status answer to the question it answers (`REQ-ORD-1`): both signed statements, so the answer is the
/// provider asked's own; the question's transaction, the transaction asked about, and, where it answers, a result of
/// that transaction's own operation. The carried result's maker and context are the answer's
/// ([`Statement::check_context`]).
pub fn check_status_answer(
    question: &Statement<Operation>,
    answer: &Statement<ResultBody>,
) -> Result<(), Vec<StatusMismatch>> {
    let (Operation::StatusQuery(asked), ResultBody::StatusQuery(answered)) =
        (&question.statement, &answer.statement)
    else {
        return Err(alloc::vec![StatusMismatch::NotAStatusQuestion]);
    };
    let mut out = Vec::new();
    if answer.maker != asked.provider {
        out.push(StatusMismatch::AnsweredByAnotherProvider);
    }
    if answered.transaction != asked.transaction {
        out.push(StatusMismatch::AnswersAnotherQuestion);
    }
    if answered.about != asked.about {
        out.push(StatusMismatch::AboutAnotherTransaction);
    }
    if let StatusOutcome::Answered(carried) = &answered.outcome {
        let result = &carried.result.statement;
        if matches!(
            result,
            ResultBody::StatusQuery(_) | ResultBody::Hold(_) | ResultBody::InnerSegmentsQuery(_)
        ) {
            out.push(StatusMismatch::CarriesNoOperationsResult);
        } else if result.transaction() != Some(asked.about.as_str()) {
            out.push(StatusMismatch::CarriesAnotherTransactionsResult);
        }
    }
    if out.is_empty() { Ok(()) } else { Err(out) }
}

/// How a hold answer differs from the request it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldMismatch {
    /// It names another request's transaction, or carries a hold under another.
    AnswersAnotherRequest,
    /// The hold it carries is of another record, candidate, order or session than the one asked.
    HoldsAnotherQuote,
    /// The hold it carries holds past the validity asked, or names a validity that can't be ordered.
    PastTheValidityAsked,
    /// It was made by another reserve than the one asked (the review of `1bb5f4cb`, as `check_answer`'s
    /// `OfAnotherReserve`).
    OfAnotherReserve,
    /// The request isn't a hold request, or the answer isn't a hold answer.
    NotAHoldRequest,
}

/// Holds a reserve's answer to the hold request it answers (`REQ-QTE-15`): both signed statements, so the answer is
/// the reserve asked's own; its transaction, and, where it holds, a hold under that transaction of exactly the record,
/// candidate and order or session asked, valid no later than asked. The hold's maker and context are the answer's
/// ([`Statement::check_context`]).
pub fn check_hold_answer(
    request: &Statement<Operation>,
    answer: &Statement<ResultBody>,
) -> Result<(), Vec<HoldMismatch>> {
    let (Operation::Hold(asked), ResultBody::Hold(answered)) =
        (&request.statement, &answer.statement)
    else {
        return Err(alloc::vec![HoldMismatch::NotAHoldRequest]);
    };
    let mut out = Vec::new();
    if answer.maker != asked.provider {
        out.push(HoldMismatch::OfAnotherReserve);
    }
    let held = match &answered.outcome {
        HoldOutcome::Held(held) => Some(&held.hold.statement),
        HoldOutcome::Refused(_) => None,
    };
    if answered.transaction != asked.transaction
        || held.is_some_and(|h| h.transaction != asked.transaction)
    {
        out.push(HoldMismatch::AnswersAnotherRequest);
    }
    if let Some(hold) = held {
        if hold.record != asked.record
            || hold.candidate != asked.candidate
            || hold.held_for != asked.held_for
        {
            out.push(HoldMismatch::HoldsAnotherQuote);
        }
        if hold.valid_until != asked.valid_until
            && !made_after(&asked.valid_until, &hold.valid_until)
        {
            out.push(HoldMismatch::PastTheValidityAsked);
        }
    }
    if out.is_empty() { Ok(()) } else { Err(out) }
}

/// A provider's answer to the question of a private node's record (`REQ-ORD-41`; `T-020-58`, 4.0.0): the question's own
/// transaction, the node asked about, and the record, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InnerSegmentsQueryResult {
    /// The transaction this answers: the question's own.
    pub transaction: String,
    /// The private node asked about.
    pub node: String,
    /// The record, or why there is none.
    pub outcome: InnerSegmentsOutcome,
}

/// What a provider answers to the question of a private node's record, by `status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InnerSegmentsOutcome {
    /// The provider's own record of the node's segments, whole: a statement of its own, in its name, labelled its
    /// publisher's word.
    Answered(InnerSegmentsAnswered),
    /// The provider has recorded no segment inside the node. It says nothing else.
    NotKnown(NoDetail),
    /// The provider refused, with its class (`REQ-CAT-18`).
    Refused(Refused),
}

/// A record of a private node's segments, carried whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InnerSegmentsAnswered {
    /// The provider's own enveloped record of the node, made by the answer's maker.
    pub record: Box<Statement<InnerSegments>>,
}

/// How an answer to the question of a private node's record differs from the question it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InnerSegmentsMismatch {
    /// It was made by another provider than the one asked.
    AnsweredByAnotherProvider,
    /// It names another question's transaction.
    AnswersAnotherQuestion,
    /// It is about another node than the one asked about, or carries a record of one.
    OfAnotherNode,
    /// The record it carries was made by another provider than the one that answers.
    RecordOfAnotherMaker,
    /// The record it carries lists a segment under a transaction the question didn't ask about.
    SegmentNotAsked,
    /// The question names no transaction it is about, or one twice: it asks for nothing, or the same thing twice, and
    /// no answer to it is held.
    AboutNotAList,
    /// The question isn't a question of a node's record, or the answer isn't an answer to one: an answer to another
    /// question is no record of a private node.
    NotAnInnerSegmentsQuestion,
}

/// Holds an answer to the question of a private node's record to the question it answers (`REQ-ORD-41`): the question
/// and the signed answer, so the answer is the provider asked's own; the question's transaction, the node asked about, and, where it
/// answers, a record of that node made by the provider that answers. That the record is signed with a key its maker's
/// advertisement lists, and that the node is one the maker declares for its publisher, is the reader's, against the
/// advertisement its catalogue accepted ([`check_inner_segments`](super::check_inner_segments)). That its segments add
/// up is the replay's: the contract refuses no figure.
pub fn check_inner_segments_answer(
    question: &Operation,
    answer: &Statement<ResultBody>,
) -> Result<(), Vec<InnerSegmentsMismatch>> {
    let (Operation::InnerSegmentsQuery(asked), ResultBody::InnerSegmentsQuery(answered)) =
        (question, &answer.statement)
    else {
        return Err(alloc::vec![
            InnerSegmentsMismatch::NotAnInnerSegmentsQuestion
        ]);
    };
    let mut out = Vec::new();
    if !asked.asks_about_each_once() {
        out.push(InnerSegmentsMismatch::AboutNotAList);
    }
    if answer.maker != asked.provider {
        out.push(InnerSegmentsMismatch::AnsweredByAnotherProvider);
    }
    if answered.transaction != asked.transaction {
        out.push(InnerSegmentsMismatch::AnswersAnotherQuestion);
    }
    let carried = match &answered.outcome {
        InnerSegmentsOutcome::Answered(carried) => Some(&carried.record),
        _ => None,
    };
    if answered.node != asked.node || carried.is_some_and(|r| r.statement.node != asked.node) {
        out.push(InnerSegmentsMismatch::OfAnotherNode);
    }
    if carried.is_some_and(|r| r.maker != answer.maker) {
        out.push(InnerSegmentsMismatch::RecordOfAnotherMaker);
    }
    if carried.is_some_and(|r| {
        r.statement
            .segments
            .iter()
            .any(|s| !asked.about.contains(&s.transaction))
    }) {
        out.push(InnerSegmentsMismatch::SegmentNotAsked);
    }
    if out.is_empty() { Ok(()) } else { Err(out) }
}
