//! A private node's publisher's record, asked of its provider and taken (`REQ-ORD-41`, `T-020-58`).
//!
//! Until `T-020-58` no operation asked a provider for its record of the segments inside a private node, and the
//! deployment read it from the world it ran in. Now Core asks: for each private node a segment's route passes, it puts
//! provider-role's `inner_segments_query` to each provider of the node's publisher that declares the node, in the
//! operator's name and under a transaction of its own, through the platform ([`Modules`](crate::segments::Modules)
//! where closing grades a segment, [`Ports`](crate::schedule::Ports) where the tick's job reads them). The answer is taken
//! only as a record of that node ([`taken`]): the answer is the provider's own and signed with a key its advertisement
//! lists, it answers this question about this node, and the record it carries is its maker's, signed, of a node its
//! maker declares for its publisher. Anything else is not taken, and the node's books say nothing of it.
//!
//! Nothing is kept between askings that a restart could lose: closing asks again each time it grades a segment, and the
//! record it relies on is appended to the log as a stated entry (`T-020-32`).

use rutent_contracts::provider_role::{
    InnerSegments, InnerSegmentsMismatch, InnerSegmentsOutcome, InnerSegmentsQuery, Operation,
    ResultBody, Statement, check_inner_segments, check_inner_segments_answer,
};
use rutent_prov::catalogue::{Catalogue, Instant, StatementRefusal};

/// The providers of the publisher of private node `node` whose accepted advertisement declares it, in name order:
/// those Core asks for the node's record. None where no publisher declares the node.
pub fn serving(catalogue: &Catalogue, node: &str) -> Vec<String> {
    let publishers = catalogue.publishers();
    let Some(publisher) = publishers.publisher_of_node(node) else {
        return Vec::new();
    };
    publishers
        .providers_of(publisher)
        .into_iter()
        .filter(|provider| {
            catalogue
                .accepted_advertisement(provider)
                .is_some_and(|ad| ad.statement.private_nodes.iter().any(|n| n == node))
        })
        .map(str::to_owned)
        .collect()
}

/// The question of `provider`'s record of `node`, under `transaction`, about the segments run under `about`.
pub fn question(provider: &str, node: &str, transaction: &str, about: &[String]) -> Operation {
    Operation::InnerSegmentsQuery(InnerSegmentsQuery {
        transaction: transaction.to_owned(),
        provider: provider.to_owned(),
        node: node.to_owned(),
        about: about.to_vec(),
    })
}

/// The transaction an operation names, if it names one.
pub fn transaction_of(operation: &Operation) -> Option<String> {
    serde_json::to_value(operation).ok()?["transaction"]
        .as_str()
        .map(str::to_owned)
}

/// Why an answer is not taken as a record of the private node asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotTaken {
    /// The answer's bytes aren't a result.
    Unreadable,
    /// The answer isn't signed by the provider asked with a key its advertisement lists, or is for another deployment.
    NotItsMakers(StatementRefusal),
    /// The answer isn't to this question: another kind of question, transaction, node or provider.
    AnotherQuestion(Vec<InnerSegmentsMismatch>),
    /// The provider says it holds no record of the node, or refused.
    NoRecord,
    /// The record isn't signed with a key its maker's advertisement lists.
    RecordNotItsMakers(StatementRefusal),
    /// The record isn't held to its maker's accepted advertisement: none is accepted, or the publisher or node isn't
    /// one it declares.
    NotDeclared,
}

/// `answer`, as received, taken as the record of the private node `question` asks about, or why it isn't. `at` is Core's
/// clock.
///
/// # Errors
///
/// Why the answer is not a record of the node asked about: see [`NotTaken`].
pub fn taken(
    catalogue: &Catalogue,
    at: Instant,
    question: &Operation,
    answer: &[u8],
) -> Result<Statement<InnerSegments>, NotTaken> {
    let parsed: Statement<ResultBody> =
        serde_json::from_slice(answer).map_err(|_| NotTaken::Unreadable)?;
    check_inner_segments_answer(question, &parsed).map_err(NotTaken::AnotherQuestion)?;
    catalogue
        .verify_statement(answer, at)
        .map_err(NotTaken::NotItsMakers)?;
    let ResultBody::InnerSegmentsQuery(result) = parsed.statement else {
        return Err(NotTaken::AnotherQuestion(vec![
            InnerSegmentsMismatch::NotAnInnerSegmentsQuestion,
        ]));
    };
    let InnerSegmentsOutcome::Answered(answered) = result.outcome else {
        return Err(NotTaken::NoRecord);
    };
    let record = *answered.record;
    let bytes = serde_json::to_vec(&record).map_err(|_| NotTaken::Unreadable)?;
    catalogue
        .verify_statement(&bytes, at)
        .map_err(NotTaken::RecordNotItsMakers)?;
    let advertisement = catalogue
        .accepted_advertisement(&record.maker)
        .ok_or(NotTaken::NotDeclared)?;
    check_inner_segments(&record, advertisement).map_err(|_| NotTaken::NotDeclared)?;
    Ok(record)
}
