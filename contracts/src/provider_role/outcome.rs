//! Results a provider gives for an operation (`urn:rutent:provider-role:result`, `REQ-CAT-9`, `REQ-CAT-14`,
//! `REQ-CAT-15`, `REQ-CAT-18`, `REQ-ORD-1`, `INV-9`), the verifier's standalone judgement
//! (`urn:rutent:provider-role:judgement`), and the reserve's reversal confirmation
//! (`urn:rutent:provider-role:reversal-confirmation`, ADR-0008). On the wire each is its maker's [`Statement`].

use alloc::{boxed::Box, format, string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use super::envelope::{
    ContextMismatch, Deployment, JUDGEMENT_SHAPE, RESULT_SHAPE, REVERSAL_CONFIRMATION_SHAPE,
    ShapeMismatch, Statement, StatementShape,
};
use super::types::{EvidenceGrade, SafeInteger};

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
            _ => Ok(()),
        }
    }

    fn check_nested_context(
        &self,
        maker: &str,
        deployment: &Deployment,
        protocol_version: &str,
    ) -> Result<(), ContextMismatch> {
        match self {
            Self::Judgement(JudgementResult {
                outcome: JudgementOutcome::Judged(judged),
            }) => judged
                .judgement
                .check_as_nested(maker, deployment, protocol_version),
            Self::ReversalConfirmation(ReversalConfirmationResult {
                outcome: ReversalConfirmationOutcome::Confirmed(confirmed),
                ..
            }) => confirmed
                .confirmation
                .check_as_nested(maker, deployment, protocol_version),
            _ => Ok(()),
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
