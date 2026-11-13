//! Whether a statement completes a reversal (`REQ-ORD-3`, ADR-0008): judged from its bytes as received, in this
//! order, and the first rule it breaks is why it doesn't.
//!
//! 1. **Attributable**: its signature verifies under a `statements` key its maker listed (`Catalogue::verify_statement`),
//!    and it is a result in its envelope.
//! 2. **Not the failed provider's own word**, unless the failed provider is the origin reserve, the reserve at the
//!    node the value is back at, whose own statement counts, labelled its word.
//! 3. **The origin reserve's**: ADR-0008 takes the origin node's own reserve as the party other than the failed
//!    provider; any other maker's statement is kept and completes nothing.
//! 4. **Not its publisher's**: a maker the catalogue names no publisher for is refused, and so is the origin reserve
//!    published by the failed provider's publisher, that publisher's word about its own provider's work (`THR-13`,
//!    `THR-124`). Inside the publisher's own private node its word counts, labelled, but the log takes the label only
//!    on a statement, the failed provider's own answer when asked to reverse ([`judge_reversal`]).
//! 5. **This question's, about this credit**: the reserve's confirmation answers Core's question, names the reversed
//!    segment's transaction, and says the credit arrived, signed by the reserve itself, in full (the amount the
//!    segment took in) and in the asset it left in, the confirmation inside of its own shape. No credit, a refusal,
//!    another segment, another asset or a short or larger credit completes nothing.
//!
//! What counts is graded: the reserve's own word about its own segment as a statement (4), its signed confirmation of
//! another provider's reversal as signed by its maker (3); the evidence says whose it is.

use rutent_contracts::provider_role::{
    EvidenceGrade, RESULT_SHAPE, REVERSAL_CONFIRMATION_SHAPE, ResultBody,
    ReversalConfirmationOutcome, Statement,
};
use rutent_contracts::provider_role::{ReversalOutcome, ReversalResult};
use rutent_contracts::router_api::Id;
use rutent_prov::catalogue::{Catalogue, HoldingSource, Instant, Publisher};

use super::Word;

/// Why a statement doesn't complete a reversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotCounted {
    /// Its signature doesn't verify under a key its maker published for statements: it is no one's.
    Unattributable,
    /// It isn't a result Core can read as a reversal's confirmation.
    Unreadable,
    /// It is the failed provider's own word about its own reversal (`REQ-ORD-3`).
    FailedProvidersOwn,
    /// Its maker isn't the origin reserve, the one party ADR-0008 takes as confirming a reversal.
    NotTheOriginReserve,
    /// Its maker shares the failed provider's publisher (`REQ-ORD-3`), outside that publisher's own private node.
    SamePublisher,
    /// The catalogue names no publisher for its maker.
    NoPublisher,
    /// It answers another question than Core's.
    AnotherQuestion,
    /// The reserve says no credit matching the segment, amount and asset has arrived.
    NotConfirmed,
    /// The reserve refused the question.
    Refused,
    /// The confirmation inside it isn't the reserve's own signed statement.
    NotItsOwn,
    /// It names another segment's transaction.
    AnotherSegment,
    /// The credit is in another asset than the one the segment took in.
    AnotherAsset,
    /// The credit isn't what the segment took in, in full: `stated` arrived.
    NotInFull {
        /// What the confirmation says arrived.
        stated: String,
    },
    /// Nothing came back to Core's question.
    Unanswered,
    /// It would count, but it answers no question of Core's: the log verifies only an answer under the transaction
    /// Core asked it under, so Core asks the origin reserve ([`super::Reversals::confirm`]).
    Unasked,
}

/// The reversed segment, as Core's records hold it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Reversed<'a> {
    /// The segment's provider: the one that failed, or, for an undone start, the origin reserve.
    pub provider: &'a Id,
    /// The inner node the segment starts at, if it starts at one: where the value is to be back.
    pub node: Option<&'a str>,
    /// The origin reserve: the provider of the part's withdrawal.
    pub origin: &'a Id,
    /// The transaction the segment ran under.
    pub transaction: &'a Id,
    /// The asset it took in.
    pub asset: &'a str,
    /// What it took in.
    pub amount: &'a str,
}

/// Whose statement completes a reversal, and at what grade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Whose {
    pub maker: Id,
    pub grade: EvidenceGrade,
    pub word: Word,
}

/// Judges `received` about `reversed`, as the answer to Core's question under `question` where Core asked one.
pub(super) fn judge(
    catalogue: &Catalogue,
    at: Instant,
    reversed: &Reversed<'_>,
    question: Option<&Id>,
    received: &[u8],
) -> Result<Whose, NotCounted> {
    catalogue
        .verify_statement(received, at)
        .map_err(|_| NotCounted::Unattributable)?;
    let result: Statement<ResultBody> =
        serde_json::from_slice(received).map_err(|_| NotCounted::Unreadable)?;
    if result.shape != RESULT_SHAPE {
        return Err(NotCounted::Unreadable);
    }
    let maker = Id::new(result.maker.as_str()).ok_or(NotCounted::Unreadable)?;
    let own = maker == *reversed.provider;
    if own && maker != *reversed.origin {
        return Err(NotCounted::FailedProvidersOwn);
    }
    if maker != *reversed.origin {
        return Err(NotCounted::NotTheOriginReserve);
    }
    let publishers = catalogue.publishers();
    let (theirs, failed) = (
        publishers.publisher(maker.as_str()),
        publishers.publisher(reversed.provider.as_str()),
    );
    let Some(theirs) = theirs else {
        return Err(NotCounted::NoPublisher);
    };
    // Of the failed provider's publisher, the reserve's word is that publisher's own about its own provider's work.
    // Inside the publisher's own private node the publisher's word counts, labelled, but the log takes the label only
    // on a statement graded a statement, which a reserve's signed confirmation isn't: the publisher's word there is
    // the failed provider's own answer when Core asks it to reverse ([`judge_reversal`]).
    if !own && failed.as_ref() == Some(&theirs) {
        return Err(NotCounted::SamePublisher);
    }
    let word = Word::Reserves;
    let ResultBody::ReversalConfirmation(answer) = &result.statement else {
        return Err(NotCounted::Unreadable);
    };
    let confirmed = match &answer.outcome {
        ReversalConfirmationOutcome::Confirmed(confirmed) => &confirmed.confirmation,
        ReversalConfirmationOutcome::NotConfirmed(_) => return Err(NotCounted::NotConfirmed),
        ReversalConfirmationOutcome::Refused(_) => return Err(NotCounted::Refused),
    };
    if question.is_some_and(|q| q.as_str() != answer.transaction) {
        return Err(NotCounted::AnotherQuestion);
    }
    let inner = serde_json::to_vec(&**confirmed).map_err(|_| NotCounted::Unreadable)?;
    if confirmed.maker != result.maker
        || confirmed.shape != REVERSAL_CONFIRMATION_SHAPE
        || catalogue.verify_statement(&inner, at).is_err()
    {
        return Err(NotCounted::NotItsOwn);
    }
    let credit = &confirmed.statement;
    if credit.segment != reversed.transaction.as_str() {
        return Err(NotCounted::AnotherSegment);
    }
    if credit.asset != reversed.asset {
        return Err(NotCounted::AnotherAsset);
    }
    if credit.amount != reversed.amount {
        return Err(NotCounted::NotInFull {
            stated: credit.amount.clone(),
        });
    }
    if question.is_none() {
        return Err(NotCounted::Unasked);
    }
    let grade = if own {
        EvidenceGrade::Statement
    } else {
        EvidenceGrade::SignedByProvider
    };
    Ok(Whose { maker, grade, word })
}

/// Judges `received`, the failed provider's answer to Core's reversal under `question`, about `reversed`: it counts
/// only inside a private node, the segment's start, that the provider's own publisher holds (it gave the node's
/// holding account, `register_private_holding`) and that the log binds as that publisher's (`bound`, read from the
/// log, [`super::Reversals::follow`]): there its word is its publisher's, labelled, graded a statement
/// (`REQ-ORD-3`'s third sentence). Anywhere else it is the failed provider's own word, which completes nothing. It
/// must say the value is back at that node, in full and in the asset it left in.
pub(super) fn judge_reversal(
    catalogue: &Catalogue,
    at: Instant,
    reversed: &Reversed<'_>,
    question: &Id,
    bound: Option<&str>,
    received: &[u8],
) -> Result<Whose, NotCounted> {
    catalogue
        .verify_statement(received, at)
        .map_err(|_| NotCounted::Unattributable)?;
    let result: Statement<ResultBody> =
        serde_json::from_slice(received).map_err(|_| NotCounted::Unreadable)?;
    if result.shape != RESULT_SHAPE || result.maker != reversed.provider.as_str() {
        return Err(NotCounted::Unreadable);
    }
    let ResultBody::Reversal(ReversalResult {
        transaction,
        outcome,
    }) = &result.statement
    else {
        return Err(NotCounted::Unreadable);
    };
    if transaction != question.as_str() {
        return Err(NotCounted::AnotherQuestion);
    }
    let back = match outcome {
        ReversalOutcome::Succeeded(back) => back,
        ReversalOutcome::Refused(_) => return Err(NotCounted::Refused),
        ReversalOutcome::Unknown(_) => return Err(NotCounted::NotConfirmed),
    };
    if Some(back.node.as_str()) != reversed.node {
        return Err(NotCounted::AnotherSegment);
    }
    if back.asset != reversed.asset {
        return Err(NotCounted::AnotherAsset);
    }
    if back.amount != reversed.amount {
        return Err(NotCounted::NotInFull {
            stated: back.amount.clone(),
        });
    }
    let publisher = match catalogue.publishers().publisher(reversed.provider.as_str()) {
        Some(Publisher::Party(publisher)) => publisher,
        _ => return Err(NotCounted::FailedProvidersOwn),
    };
    let holds = reversed
        .node
        .and_then(|node| catalogue.nodes().holding(node))
        .is_some_and(|h| matches!(&h.source, HoldingSource::Publisher(p) if *p == publisher));
    if !holds || bound != Some(publisher.as_str()) {
        return Err(NotCounted::FailedProvidersOwn);
    }
    Ok(Whose {
        maker: reversed.provider.clone(),
        grade: EvidenceGrade::Statement,
        word: Word::Publishers(Id::new(publisher.as_str()).ok_or(NotCounted::NoPublisher)?),
    })
}
