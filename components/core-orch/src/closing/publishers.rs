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

/// The question of `provider`'s record of `node`, under `transaction`.
pub fn question(provider: &str, node: &str, transaction: &str) -> Operation {
    Operation::InnerSegmentsQuery(InnerSegmentsQuery {
        transaction: transaction.to_owned(),
        provider: provider.to_owned(),
        node: node.to_owned(),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(kind: &str, name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/provider-role/fixtures")
            .join(kind)
            .join(format!("{name}.json"));
        serde_json::from_slice(&std::fs::read(path).expect("the fixture")).expect("JSON")
    }

    fn asked() -> Operation {
        serde_json::from_value::<Statement<Operation>>(fixture(
            "valid",
            "operation-inner-segments-query",
        ))
        .expect("a question")
        .statement
    }

    /// What Core takes of an answer is a record of the node it asked about, from the provider it asked: an answer to
    /// another kind of question, another question, another node or another provider is none, whatever else it
    /// carries (`REQ-ORD-41`, `T-020-58`).
    // verifies: REQ-ORD-41
    #[test]
    fn answer_to_another_question_is_not_taken_as_a_private_node_record() {
        let catalogue = Catalogue::default();
        let at = Instant(1_794_009_600);
        let not_a_record = |answer: &Value| {
            taken(
                &catalogue,
                at,
                &asked(),
                &serde_json::to_vec(answer).expect("bytes"),
            )
        };
        // An answer to a status question, a holding balance or a withdrawal: another kind of question.
        for other in [
            "result-status-query-not-known",
            "result-holding-balance-answered",
            "result-withdrawal-succeeded",
        ] {
            let mut answer = fixture("valid", other);
            answer["maker"] = "acme-internal-rail".into();
            assert_eq!(
                not_a_record(&answer),
                Err(NotTaken::AnotherQuestion(vec![
                    InnerSegmentsMismatch::NotAnInnerSegmentsQuestion
                ])),
                "{other}"
            );
        }
        // An answer to another question of this kind: another transaction, another node, or another provider's.
        let record = fixture(
            "valid",
            "result-inner-segments-query-answered-with-the-providers-record",
        );
        let mut other_transaction = record.clone();
        other_transaction["statement"]["transaction"] = "tx-inner-2".into();
        let mut other_node = record.clone();
        other_node["statement"]["node"] = "acme-card-book".into();
        let mut other_provider = record.clone();
        other_provider["maker"] = "other-co".into();
        for (answer, why) in [
            (
                other_transaction,
                InnerSegmentsMismatch::AnswersAnotherQuestion,
            ),
            (other_node, InnerSegmentsMismatch::OfAnotherNode),
            (
                other_provider,
                InnerSegmentsMismatch::AnsweredByAnotherProvider,
            ),
        ] {
            let Err(NotTaken::AnotherQuestion(found)) = not_a_record(&answer) else {
                panic!("{why:?}: taken as a record");
            };
            assert!(found.contains(&why), "{why:?}: {found:?}");
        }
        // The answer that is to this question still isn't taken from a catalogue that holds no advertisement of its
        // maker: a record counts only as its maker's, signed with a key it lists.
        assert!(matches!(
            not_a_record(&record),
            Err(NotTaken::NotItsMakers(_))
        ));
    }
}
