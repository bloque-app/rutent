//! How the log attributes a stated entry: from the bytes the evidence store keeps, never from its writer.
//!
//! A stated entry reaches the general handle unverified ([`GeneralEntry`](super::calls::GeneralEntry) holds no
//! other), naming the maker it claims. [`attribute`] reads the kept bytes by the entry's commitment and attributes
//! the entry `verified` only where all of these hold; otherwise it stays unverified, attributed to no one
//! (`REQ-AUD-2`):
//!
//! 1. the bytes are a statement envelope;
//! 2. its `deployment` is this deployment (`REQ-PRT-1`);
//! 3. its `maker` is the actor the entry claims (`claimed_by.acts`: the provider, the operator or the module whose
//!    key signs), and the party the entry claims is the one that maker states for, its publisher, or the operator's
//!    party for the operator and a module it publishes ([`StatementKeys::publisher`]);
//! 4. its `shape` is the entry's `what.statement`, and its `made_at` the entry's `when.at`;
//! 5. its signature's scheme and key are the entry's `signed_with`;
//! 6. its `about` names a transaction exactly when its body names one, and the same one; and where the entry is
//!    about an order or a segment, its `about` names a transaction, so a statement is verified for an order only
//!    through a transaction its maker signed, and one whose body names a transaction never about nothing
//!    (`REQ-ORD-32`, `REQ-ORD-38`). A judgement names no transaction, so it is verified about nothing;
//! 7. its signature verifies over its signing input;
//! 8. its key signs the maker's statements at its `made_at` (`THR-3`): for a module the operator publishes, a
//!    statement key that a release of it the log accepted names ([`release_statement_keys`]); for any other maker,
//!    one [`StatementKeys::signs_for`] answers, whose first trust in a provider is the advertising keys of its latest
//!    grant in the log ([`granted_advertising_keys`]), never a deployment setting the log doesn't hold.
//!
//! So a verified entry's `acts` and `party` are both bound to its key. Its `role` stays the writer's label: no
//! relation may read a stated entry's role.
//!
//! So a verified entry's shape, time, key and transaction are the bytes' own, and one about an order is tied to it
//! through a signed transaction, which [`STATED_RELATIONS`](super::STATED_RELATIONS) keeps to one order. An entry
//! whose commitment names no kept statement is refused: it would rest on nothing anyone can check.

use alloc::vec::Vec;

use super::Violation;
use super::calls::{AppendFailure, KeptStatements, Signatures, StatementKeys};
use super::relations::History;
use super::types::*;
use crate::provider_role::{Deployment, PublicKey, SignatureScheme};
use crate::router_api::Id;
use serde::Deserialize;

/// The catalogue changes `history` carried out: each effect's change, in order.
fn catalogue_changes(history: &History) -> impl Iterator<Item = &Action> {
    history.iter().filter_map(|(_, e)| match e {
        Entry::Verified(VerifiedEntry {
            what: What::Effect { action, .. },
            ..
        }) if matches!(action, Action::ChangeCatalogue { .. }) => Some(action),
        _ => None,
    })
}

/// The advertising keys of `provider`'s latest grant in `history` that no later `revoke_grant` of its tuple took
/// back: the log's first trust in the provider's advertisements. A new grant replaces the keys, which is how a
/// provider recovers a lost one; with no grant in force there are none.
pub fn granted_advertising_keys<'a>(history: &'a History, provider: &Id) -> &'a [PublicKey] {
    let mut in_force: Vec<(&Id, &[PublicKey])> = Vec::new();
    for action in catalogue_changes(history) {
        let Action::ChangeCatalogue {
            change,
            provider: p,
            tuple: Some(tuple),
            advertising_keys,
            ..
        } = action
        else {
            continue;
        };
        if p != provider {
            continue;
        }
        in_force.retain(|(t, _)| *t != tuple);
        if *change == CatalogueChange::Grant {
            in_force.push((tuple, advertising_keys));
        }
    }
    in_force.last().map_or(&[], |(_, keys)| keys)
}

/// The statement keys of the releases of `module` the log accepted and no later `revoke_release` took back: the
/// keys a module the operator publishes signs with.
pub fn release_statement_keys<'a>(history: &'a History, module: &Id) -> Vec<&'a PublicKey> {
    let mut in_force: Vec<(&Id, &PublicKey)> = Vec::new();
    for action in catalogue_changes(history) {
        let Action::ChangeCatalogue {
            change,
            provider,
            version: Some(version),
            statement_key,
            ..
        } = action
        else {
            continue;
        };
        if provider != module {
            continue;
        }
        match (change, statement_key) {
            (CatalogueChange::AcceptModule, Some(key)) => in_force.push((version, key)),
            (CatalogueChange::RevokeRelease, _) => in_force.retain(|(v, _)| *v != version),
            _ => {}
        }
    }
    in_force.into_iter().map(|(_, key)| key).collect()
}

/// Whether `claim`'s key signs `maker`'s statements: a module the operator publishes (another maker than the operator
/// that states for the operator's party) by a statement key its accepted release names in `history`; any other
/// maker as `keys` answers.
fn signed_by_its_maker(
    claim: &StatedEntry,
    maker: &Id,
    history: &History,
    keys: &mut dyn StatementKeys,
) -> Result<bool, AppendFailure> {
    let operator = keys.operator()?;
    if maker != &operator && keys.publisher(maker)? == keys.publisher(&operator)? {
        return Ok(release_statement_keys(history, maker).contains(&&claim.signed_with.key));
    }
    keys.signs_for(maker, &claim.signed_with, &claim.when.at)
}

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
    history: &History,
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
            let about = claim.about.as_ref();
            let about_transaction = about
                .and_then(|a| a.transaction.as_ref())
                .map(|t| t.as_str());
            let bound = about.is_some_and(|a| a.order.is_some() || a.segment.is_some());
            envelope.deployment == keys.deployment()?
                && envelope.maker == claimed_by.acts.as_str()
                && envelope.shape == claim.what.statement.as_str()
                && envelope.made_at == claim.when.at.as_str()
                && envelope.signature.scheme == claim.signed_with.scheme
                && envelope.signature.key == claim.signed_with.key
                && match about_transaction {
                    Some(about) => body_transaction == Some(about),
                    None => body_transaction.is_none() && !bound,
                }
                && signatures.verifies(&bytes)
                && signed_by_its_maker(claim, &claimed_by.acts, history, keys)?
                && keys.publisher(&claimed_by.acts)?.as_ref() == Some(&claimed_by.party)
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
