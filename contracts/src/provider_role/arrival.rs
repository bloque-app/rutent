//! A reserve's signed notice of value arriving (`REQ-ORD-14`, `REQ-ORD-23`, `T-020-18`).
//!
//! Before it, provider-role had no statement a reserve makes of value arriving, so Core took what arrived (outer
//! node, asset, amount, account, holder) from the assembly's port and kept the reserve's notice as bytes it couldn't
//! read, with nothing tying the figures to the signed bytes (`T-020-04`'s gap).
//!
//! - **The notice** (`urn:rutent:provider-role:arrival-notice`) is the reserve's own statement: the transaction the
//!   reserve gives the credit, its per-credit reference, the outer node the value arrived at, the asset with its
//!   unit, the amount, and the account and its holder. Core's log keys the deposit on that transaction
//!   (core-internal's `arrival-names-its-notice`, `one-order-per-deposit`), and the notice's stated entry is about
//!   it.
//! - **One credit, one arrival.** Every notice of one credit names its one transaction, whenever signed, and a reader
//!   knows two such notices as one credit ([`check_one_credit`]): the same reserve, the same transaction, the same
//!   figures. The same transaction with other figures is refused, never taken as a second credit.
//! - **Two references are two claims.** Two notices under different transactions are two signed claims, each
//!   attributable to the reserve that made it (`REQ-AUD-2`). Neither this contract nor the log can tell them from two
//!   real credits: a reserve that states one credit twice, under two references, has signed for both, and the loss is
//!   that reserve's.
//! - **The question** (`urn:rutent:provider-role:arrival-question`) is the operator's, asking a reserve what arrived
//!   at an account for an order, under a transaction fresh for that order, in the holder's session.
//! - **The answer** (`urn:rutent:provider-role:arrival-answer`) is the reserve's, naming the question's transaction,
//!   so it counts only as that question's (`REQ-ORD-1`). It carries the reserve's notice of each credit whole, the
//!   same notice it sends of its own accord, so the answer is that notice under the question's transaction; or says
//!   nothing arrived; or refuses. [`check_answer`] holds it to the question.

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use super::envelope::{
    ContextMismatch, Deployment, NotSignedByOperator, ShapeMismatch, Statement, StatementShape,
};
use super::outcome::{BalanceRefused, NoDetail};
use super::types::{Denomination, OuterNode};

/// The `shape` of a reserve's arrival notice.
pub const ARRIVAL_NOTICE_SHAPE: &str = "urn:rutent:provider-role:arrival-notice";

/// A reserve's notice that value arrived at one of its accounts (`REQ-ORD-14`, `REQ-ORD-23`): the body of a
/// statement of shape [`ARRIVAL_NOTICE_SHAPE`], made by the reserve. Schema:
/// `urn:rutent:provider-role:arrival-notice#/$defs/notice`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrivalNotice {
    /// The transaction the reserve gives this one credit: its per-credit reference, the same in every notice of the
    /// credit, whenever signed. The deposit is known by it, and the notice's stated entry is about it.
    pub transaction: String,
    /// The reserve's outer node the value arrived at: the node belongs to the notice's maker.
    pub node: OuterNode,
    /// The asset, and the unit the amount is counted in.
    pub asset: Denomination,
    /// How much arrived, in the asset's unit: a positive decimal integer string.
    pub amount: String,
    /// The account at the reserve the value reached.
    pub account: String,
    /// The party that holds the account.
    pub holder: String,
}

impl StatementShape for ArrivalNotice {
    const SHAPE: &'static str = ARRIVAL_NOTICE_SHAPE;
}

impl ArrivalNotice {
    /// Whether `other` states the same arrival: the same outer node, asset and unit, amount, account and holder.
    /// The transaction, the credit's reference, is left aside.
    pub fn same_figures(&self, other: &Self) -> bool {
        self.node == other.node
            && self.asset == other.asset
            && self.amount == other.amount
            && self.account == other.account
            && self.holder == other.holder
    }
}

/// Whether two arrival notices are of one credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credits {
    /// One credit: the same reserve's, by the same transaction, with the same figures. A reader knows the two as one
    /// arrival, whenever each was signed.
    One,
    /// Two credits: another reserve's, or another transaction.
    Two,
}

/// A notice that restates a credit with figures another notice of it doesn't state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditRestated {
    /// The same reserve and transaction, with another node, asset, unit, amount, account or holder.
    OtherFigures,
}

/// Compares two arrival notices by their credit (`REQ-ORD-23`). A credit's transaction is its reserve's own, so
/// notices of two makers are never one credit, whatever they name. The same maker's notices naming one transaction
/// are one credit when they state the same figures, and refused when they don't: a credit doesn't change once it
/// arrived.
pub fn check_one_credit(
    earlier: &Statement<ArrivalNotice>,
    later: &Statement<ArrivalNotice>,
) -> Result<Credits, CreditRestated> {
    if earlier.maker != later.maker || earlier.statement.transaction != later.statement.transaction
    {
        return Ok(Credits::Two);
    }
    if earlier.statement.same_figures(&later.statement) {
        Ok(Credits::One)
    } else {
        Err(CreditRestated::OtherFigures)
    }
}

/// The `shape` of the operator's question to a reserve of what arrived.
pub const ARRIVAL_QUESTION_SHAPE: &str = "urn:rutent:provider-role:arrival-question";

/// The `shape` of a reserve's answer to the question of what arrived.
pub const ARRIVAL_ANSWER_SHAPE: &str = "urn:rutent:provider-role:arrival-answer";

/// The operator's question to a reserve of what arrived at one of its accounts for an order (`REQ-ORD-23`): the body
/// of a statement of shape [`ARRIVAL_QUESTION_SHAPE`], signed with the deployment's operator key. It names no order:
/// Core's log binds it to the order it is asked for, as it binds every operation. The reserve answers with an
/// [`ArrivalAnswer`] naming this question's transaction ([`check_answer`]). Schema:
/// `urn:rutent:provider-role:arrival-question#/$defs/question`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrivalQuestion {
    /// The question's own transaction, fresh for the order it is asked for: its answer names it (`REQ-ORD-1`).
    pub transaction: String,
    /// The reserve asked.
    pub provider: String,
    /// The session of the account's holder or their delegate (`REQ-CAT-3`). What arrived at an account tells
    /// whether its funds suffice for that much, so the question counts as a balance check (`REQ-CAT-4`), outside the
    /// bound only for the holder's own order.
    pub session: String,
    /// The account at the reserve.
    pub account: String,
    /// The reserve's outer node.
    pub node: OuterNode,
    /// The asset, and the unit it is counted in there: an answer's notice is of this asset in this unit.
    pub asset: Denomination,
}

impl StatementShape for ArrivalQuestion {
    const SHAPE: &'static str = ARRIVAL_QUESTION_SHAPE;
}

impl Statement<ArrivalQuestion> {
    /// Checks that the question names the deployment's operator key as its signing key, since it is the operator's
    /// statement, as an operation is.
    pub fn check_signed_by_operator(&self) -> Result<(), NotSignedByOperator> {
        if self.signature.key == self.deployment.operator_key {
            Ok(())
        } else {
            Err(NotSignedByOperator {
                operator_key: self.deployment.operator_key.clone(),
                key: self.signature.key.clone(),
            })
        }
    }
}

/// A reserve's answer to the question of what arrived (`REQ-ORD-1`, `REQ-ORD-23`): the body of a statement of shape
/// [`ARRIVAL_ANSWER_SHAPE`], made by the reserve asked. It names the question's transaction, so its stated entry is
/// about it, and carries the reserve's notice of each credit whole: the same notice it sends of its own accord,
/// under the question's transaction. Schema: `urn:rutent:provider-role:arrival-answer#/$defs/answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrivalAnswer {
    /// The question's own transaction, which this answers.
    pub transaction: String,
    /// What arrived, nothing, or a refusal.
    pub outcome: ArrivalOutcome,
}

/// What a reserve answers of what arrived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ArrivalOutcome {
    /// Credits arrived: the reserve's notice of each.
    Arrived(ArrivedCredits),
    /// Nothing arrived at the account, node and asset asked about. It says nothing else.
    NothingArrived(NoDetail),
    /// The reserve refused, without saying when it lifts: the question counts as a balance check (`INV-9`).
    Refused(BalanceRefused),
}

/// The credits that arrived, each by the reserve's own enveloped notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrivedCredits {
    /// The notices, whole: at least one.
    pub notices: Vec<Statement<ArrivalNotice>>,
}

impl StatementShape for ArrivalAnswer {
    const SHAPE: &'static str = ARRIVAL_ANSWER_SHAPE;

    fn check_nested_shapes(&self) -> Result<(), ShapeMismatch> {
        match &self.outcome {
            ArrivalOutcome::Arrived(arrived) => arrived
                .notices
                .iter()
                .try_for_each(|notice| notice.check_shape()),
            _ => Ok(()),
        }
    }

    fn check_nested_context(
        &self,
        maker: &str,
        deployment: &Deployment,
        protocol_version: &str,
    ) -> Result<(), ContextMismatch> {
        match &self.outcome {
            ArrivalOutcome::Arrived(arrived) => arrived
                .notices
                .iter()
                .try_for_each(|notice| notice.check_as_nested(maker, deployment, protocol_version)),
            _ => Ok(()),
        }
    }
}

/// A way an arrival answer doesn't answer an arrival question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerMismatch {
    /// It names another transaction: it answers another question (`REQ-ORD-1`).
    AnswersAnotherQuestion,
    /// Its maker isn't the reserve asked.
    OfAnotherReserve,
    /// A notice it carries, by its position, is of another outer node.
    AtAnotherNode {
        /// The notice's position in the answer.
        notice: usize,
    },
    /// A notice it carries is of another asset, or of the asset counted in another unit.
    InAnotherAsset {
        /// The notice's position in the answer.
        notice: usize,
    },
    /// A notice it carries is of another account.
    ToAnotherAccount {
        /// The notice's position in the answer.
        notice: usize,
    },
}

/// Checks that `answer` answers `question` (`REQ-ORD-1`, `REQ-ORD-23`): it names the question's transaction, is made
/// by the reserve asked, and each notice it carries is of the outer node, asset and unit, and account asked about. An
/// amount is read in its unit, so the same asset counted in another unit is another asset here. Every rule
/// broken is returned. That each notice it carries is its own maker's and of the reader's deployment and protocol
/// version is the answer's `check_context`; that the question was signed by the operator is its
/// [`check_signed_by_operator`](Statement::check_signed_by_operator).
pub fn check_answer(
    question: &Statement<ArrivalQuestion>,
    answer: &Statement<ArrivalAnswer>,
) -> Result<(), Vec<AnswerMismatch>> {
    let asked = &question.statement;
    let mut broken = Vec::new();
    if answer.statement.transaction != asked.transaction {
        broken.push(AnswerMismatch::AnswersAnotherQuestion);
    }
    if answer.maker != asked.provider {
        broken.push(AnswerMismatch::OfAnotherReserve);
    }
    if let ArrivalOutcome::Arrived(arrived) = &answer.statement.outcome {
        for (at, notice) in arrived.notices.iter().enumerate() {
            let notice = &notice.statement;
            if notice.node != asked.node {
                broken.push(AnswerMismatch::AtAnotherNode { notice: at });
            }
            if notice.asset != asked.asset {
                broken.push(AnswerMismatch::InAnotherAsset { notice: at });
            }
            if notice.account != asked.account {
                broken.push(AnswerMismatch::ToAnotherAccount { notice: at });
            }
        }
    }
    if broken.is_empty() {
        Ok(())
    } else {
        Err(broken)
    }
}
