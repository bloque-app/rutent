//! Holding a reader's answer to the question Core asked (`DEC-28`).

use alloc::vec::Vec;

use super::types::{Asked, ReaderAnswer, ReaderQuestion};
use crate::provider_role::{PublicKey, Statement};
use crate::router_api::{Timestamp, Version};

/// How a reader's answer differs from the question it answers, or from the reader listed for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderMismatch {
    /// Its envelope names another shape than a reader's answer.
    NotAnAnswer,
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
/// in it (`DEC-28`, ADR-0007 as amended): its envelope's shape; its maker, the reader asked; its signing key, the
/// reader's listed statement key `key`; its version, the listed `version`; the request, network and question asked,
/// with the movement or transaction it is about; and its arrival by the question's deadline at `now`, Core's own clock
/// when the answer reached it, never the host's (the answer's `made_at` is not read). An answer that fails any of them
/// is refused, and Core reads it as unreadable.
///
/// The signature itself, and the host's certificate against the operator's CA and the listed subject name, are Core's
/// to verify; this holds what the signature covers. An unreadable answer that passes is still unreadable: it settles
/// nothing.
pub fn check_reader_answer(
    question: &ReaderQuestion,
    answer: &Statement<ReaderAnswer>,
    key: &PublicKey,
    version: &Version,
    now: &Timestamp,
) -> Result<(), Vec<ReaderMismatch>> {
    let mut out = Vec::new();
    if answer.check_shape().is_err() {
        out.push(ReaderMismatch::NotAnAnswer);
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
