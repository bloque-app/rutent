//! How the log attributes a stated entry: from the bytes the evidence store keeps, never from its writer.
//!
//! A stated entry reaches the general handle unverified ([`GeneralEntry`](super::calls::GeneralEntry) holds no
//! other), naming the maker it claims. [`attribute`] reads the kept bytes by the entry's commitment and attributes
//! the entry `verified` only where all of these hold; otherwise it stays unverified, attributed to no one
//! (`REQ-AUD-2`):
//!
//! 1. the bytes are a statement envelope;
//! 2. its `deployment` is this deployment (`REQ-PRT-1`);
//! 3. its `maker` is the party the entry claims;
//! 4. its `shape` is the entry's `what.statement`, and its `made_at` the entry's `when.at`;
//! 5. its signature's scheme and key are the entry's `signed_with`;
//! 6. where its body names a `transaction` and the entry's `about` names one, they are the same;
//! 7. its signature verifies over its signing input;
//! 8. its key signs the maker's statements at its `made_at` (`THR-3`).
//!
//! So a verified entry's shape, time and key are the bytes' own, and its transaction too where both name one. An
//! entry whose commitment names no kept statement is refused: it would rest on nothing anyone can check.

use alloc::vec::Vec;

use super::Violation;
use super::calls::{AppendFailure, KeptStatements, Signatures, StatementKeys};
use super::types::*;
use crate::provider_role::{Deployment, PublicKey, SignatureScheme};
use serde::Deserialize;

/// The refusal of a stated entry whose commitment names no statement the evidence store keeps.
pub const STATED_WITHOUT_ITS_KEPT_BYTES: &str =
    "a stated entry names a statement the evidence store keeps";

/// The fields of a statement envelope the attribution reads (provider-role's `statement-envelope`).
#[derive(Deserialize)]
struct Envelope {
    deployment: Deployment,
    shape: alloc::string::String,
    maker: alloc::string::String,
    made_at: alloc::string::String,
    signature: EnvelopeSignature,
    statement: serde_json::Value,
}

#[derive(Deserialize)]
struct EnvelopeSignature {
    scheme: SignatureScheme,
    key: PublicKey,
}

/// The bytes `text` writes in base64url without padding, or `None` where it doesn't.
fn decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in text.bytes() {
        acc = (acc << 6) | u32::from(value(c)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Attributes `claim`, a stated entry as its writer handed it to the log, from the statement kept under its
/// commitment: the entry the log records, `verified` to its claimed maker only where the kept bytes bear the claim
/// out (see the module's steps), and otherwise unverified. It refuses an entry its writer attributed `verified`,
/// and one whose commitment names no kept statement; a key or store it can't reach fails the append, and nothing is
/// recorded.
pub fn attribute(
    claim: &StatedEntry,
    store: &mut dyn KeptStatements,
    keys: &mut dyn StatementKeys,
    signatures: &dyn Signatures,
) -> Result<StatedEntry, AppendFailure> {
    let Attribution::Unverified { claimed_by } = &claim.attribution else {
        return Err(AppendFailure::Refused(Violation(
            super::calls::ATTRIBUTED_BY_ITS_WRITER,
        )));
    };
    let kept = store
        .kept(&claim.commitment)?
        .filter(|k| k.commitment == claim.commitment)
        .ok_or(AppendFailure::Refused(Violation(
            STATED_WITHOUT_ITS_KEPT_BYTES,
        )))?;
    let verified = match decode(kept.bytes.as_str()).and_then(|bytes| {
        serde_json::from_slice::<Envelope>(&bytes)
            .ok()
            .map(|e| (bytes, e))
    }) {
        None => false,
        Some((bytes, envelope)) => {
            let body_transaction = envelope
                .statement
                .get("transaction")
                .and_then(|t| t.as_str());
            let about_transaction = claim
                .about
                .as_ref()
                .and_then(|a| a.transaction.as_ref())
                .map(|t| t.as_str());
            envelope.deployment == keys.deployment()?
                && envelope.maker == claimed_by.party.as_str()
                && envelope.shape == claim.what.statement.as_str()
                && envelope.made_at == claim.when.at.as_str()
                && envelope.signature.scheme == claim.signed_with.scheme
                && envelope.signature.key == claim.signed_with.key
                && match (body_transaction, about_transaction) {
                    (Some(body), Some(about)) => body == about,
                    _ => true,
                }
                && signatures.verifies(&bytes)
                && keys.signs_for(&claimed_by.party, &claim.signed_with, &claim.when.at)?
        }
    };
    let mut entry = claim.clone();
    if verified {
        entry.attribution = Attribution::Verified {
            who: claimed_by.clone(),
        };
    }
    Ok(entry)
}
