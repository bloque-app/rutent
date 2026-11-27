//! Holding a reader's answer to the question Core asked (`DEC-28`).

use alloc::vec::Vec;

use super::types::{Asked, ReaderAnswer, ReaderQuestion};
use crate::provider_role::{ContextMismatch, Deployment, ProtocolVersions, PublicKey, Statement};
use crate::router_api::{Timestamp, Version};

/// What Core expects of an answer beside the question it sent: the deployment and the protocol versions it takes
/// statements under (`REQ-PRT-1`), and the reader the listing names, by its statement key and its version.
#[derive(Debug, Clone, Copy)]
pub struct Expected<'a> {
    /// Core's deployment: an answer naming another is void.
    pub deployment: &'a Deployment,
    /// The protocol versions in force: an answer under another, or past its cutover, is void.
    pub versions: &'a ProtocolVersions,
    /// The reader's listed statement key.
    pub key: &'a PublicKey,
    /// The reader's listed version.
    pub version: &'a Version,
}

/// How a reader's answer differs from the question it answers, or from the reader listed for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderMismatch {
    /// Its envelope names another shape than a reader's answer.
    NotAnAnswer,
    /// It breaks a rule of its own shape, such as answering another kind of question than the one it names
    /// ([`ReaderAnswer::check`]): a value built in memory is held as one read from the wire is.
    BreaksItsShape,
    /// It names another deployment than Core's (`REQ-PRT-1`).
    OfAnotherDeployment,
    /// It names a protocol version not in force at the time Core takes it (`REQ-PRT-1`, `REQ-PRT-4`).
    UnderAProtocolVersionNotInForce,
    /// It was made by another reader than the one asked.
    AnsweredByAnotherReader,
    /// It names another key than the reader's listed statement key: transport identity alone is not authorisation.
    SignedWithAnotherKey,
    /// It names another version of the reader than the one listed (`REQ-AUD-4`).
    OfAnotherVersion,
    /// It answers another request than the one asked.
    AnswersAnotherRequest,
    /// It is about another network than the one asked about.
    AboutAnotherNetwork,
    /// It answers another kind of question than the one asked.
    AnswersAnotherQuestion,
    /// It is about another movement than the one asked about.
    AboutAnotherMovement,
    /// It is about another transaction than the one asked about.
    AboutAnotherTransaction,
    /// It reached Core after the question's deadline, by Core's own clock.
    PastItsDeadline,
}

/// Holds a reader's answer to the question Core sent and to the reader the listing names, before Core reads anything
/// in it (`DEC-28`, ADR-0007 as amended): its envelope's shape and its own rules ([`ReaderAnswer::check`]); its
/// deployment and protocol version, held to `expected`'s at `now` ([`Statement::check_context`]); its maker, the reader
/// asked; its signing key, the reader's listed statement key; its version, the listed version; the request, network
/// and question asked, with the movement or transaction it is about; and its arrival by the question's deadline at
/// `now`, Core's own clock when the answer reached it, never the host's (the answer's `made_at` is not read). An answer
/// that fails any of them is refused, and Core reads it as unreadable.
///
/// The signature itself, and the host's certificate against the operator's CA and the listed subject name, are Core's
/// to verify; this holds what the signature covers. An unreadable answer that passes is still unreadable: it settles
/// nothing.
pub fn check_reader_answer(
    question: &ReaderQuestion,
    answer: &Statement<ReaderAnswer>,
    expected: &Expected<'_>,
    now: &Timestamp,
) -> Result<(), Vec<ReaderMismatch>> {
    let (key, version) = (expected.key, expected.version);
    let mut out = Vec::new();
    if answer.check_shape().is_err() {
        out.push(ReaderMismatch::NotAnAnswer);
    }
    if answer.statement.check().is_err() {
        out.push(ReaderMismatch::BreaksItsShape);
    }
    match answer.check_context(expected.deployment, expected.versions, now.as_str()) {
        Ok(()) => {}
        Err(ContextMismatch::Deployment { .. }) => out.push(ReaderMismatch::OfAnotherDeployment),
        Err(_) => out.push(ReaderMismatch::UnderAProtocolVersionNotInForce),
    }
    if answer.maker != question.reader.as_str() {
        out.push(ReaderMismatch::AnsweredByAnotherReader);
    }
    if &answer.signature.key != key {
        out.push(ReaderMismatch::SignedWithAnotherKey);
    }
    let body = &answer.statement;
    if &body.version != version {
        out.push(ReaderMismatch::OfAnotherVersion);
    }
    if body.request != question.request {
        out.push(ReaderMismatch::AnswersAnotherRequest);
    }
    if body.network != question.network {
        out.push(ReaderMismatch::AboutAnotherNetwork);
    }
    match (&question.asked, &body.asked) {
        (Asked::NetworkRules(_), Asked::NetworkRules(_)) => {}
        (Asked::MovementStatus { movement: asked }, Asked::MovementStatus { movement }) => {
            if movement != asked {
                out.push(ReaderMismatch::AboutAnotherMovement);
            }
        }
        (Asked::MovementOf { transaction: asked }, Asked::MovementOf { transaction }) => {
            if transaction != asked {
                out.push(ReaderMismatch::AboutAnotherTransaction);
            }
        }
        _ => out.push(ReaderMismatch::AnswersAnotherQuestion),
    }
    if now.unix_nanos() > question.deadline.unix_nanos() {
        out.push(ReaderMismatch::PastItsDeadline);
    }
    if out.is_empty() { Ok(()) } else { Err(out) }
}
