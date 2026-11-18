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
//!    (`REQ-ORD-32`, `REQ-ORD-38`). A judgement names no transaction, so it is verified about nothing. The one other
//!    way a statement is verified about an order is a publisher's record of the segments inside a private node
//!    (provider-role's `inner-segments`, [`inner_segments_of`]), which names its transactions only inside its
//!    `segments`: about an order, no segment and no transaction, it is verified only where its `about` names the node
//!    its body names, it lists at least one segment, and every segment's transaction is of that order in this log
//!    (first bound to it, and asked under for it by the operator's own verified `operation` question), so a record
//!    naming a transaction of another order, or one Core never asked under for this one, is never verified about it
//!    (`REQ-ORD-41`, `T-020-36`). An `about` names a node only for such a record, and only the node its body names;
//! 7. its signature verifies over its signing input;
//! 8. its key signs the maker's statements at its `made_at` (`THR-3`): for a module the operator publishes, a
//!    statement key that a release of it the log accepted names ([`release_statement_keys`]); for any other maker,
//!    one [`StatementKeys::signs_for`] answers, whose first trust in a provider is the advertising keys of its latest
//!    grant in the log ([`granted_advertising_keys`]), never a deployment setting the log doesn't hold;
//! 9. where its maker has held a verifier's class's grant, in place of a tuple, and no tuple's grant of it was in
//!    force at the statement's own `made_at`, it is a judgement of a class whose grant was in force then: a category
//!    grant attributes its maker's statements for that category alone, never for a tuple, and a revocation widens
//!    nothing backdated into the grant's window (`REQ-CAT-13`, `REQ-CPL-12`, `T-020-30`).
//!
//! So a verified entry's `acts` and `party` are both bound to its key. Its `role` is held to one its maker holds:
//! the maker the operator signs as states as the `operator`, a maker held to a verifier's classes at the statement's
//! own `made_at` (step 9) as a `verifier`, and every other maker, a provider, a verifier or a module the operator publishes, as a `provider` or a
//! `verifier`. A statement whose bytes bear the claim out in any other
//! role is refused, not recorded, since the writer mislabelled a genuine statement (`REQ-AUD-2`, `T-022-02`'s probe).
//! The log can't tell a provider from a verifier, which the catalogue knows, so no relation reads a stated entry's
//! role.
//!
//! So a verified entry's shape, time, key and transaction are the bytes' own, and one about an order is tied to it
//! through a signed transaction, which [`STATED_RELATIONS`](super::STATED_RELATIONS) keeps to one order. An entry
//! whose commitment names no kept statement is refused: it would rest on nothing anyone can check.

use alloc::vec::Vec;

use super::Violation;
use super::calls::{AppendFailure, KeptStatements, Signatures, StatementKeys};
use super::relations::History;
use super::types::*;
use crate::provider_role::{Deployment, JUDGEMENT_SHAPE, PublicKey, SignatureScheme};
use crate::router_api::{Id, Timestamp};
use serde::Deserialize;

/// The catalogue changes `history` carried out, in log order: each effect's change for which this log holds a
/// `done` result. A change whose effect failed or went unanswered changes nothing.
fn catalogue_changes(history: &History) -> impl Iterator<Item = &Action> {
    history.iter().filter_map(|(receipt, e)| match e {
        Entry::Verified(VerifiedEntry {
            what: What::Effect { action, .. },
            ..
        }) if matches!(action, Action::ChangeCatalogue { .. }) && done(history, receipt) => {
            Some(action)
        }
        _ => None,
    })
}

/// Whether `history` holds a `done` result of the effect at `receipt`.
fn done(history: &History, receipt: &Receipt) -> bool {
    history.iter().any(|(_, e)| {
        matches!(e, Entry::Verified(VerifiedEntry {
            what: What::Result { effect },
            outcome: Outcome::Done {},
            ..
        }) if effect == receipt)
    })
}

/// Whether a segment of `module` under `transaction` began or ran on `version`, by the segment record the platform
/// handed over (`began_on`, `runs_on`).
fn ran_on(history: &History, module: &Id, transaction: &Id, version: &Id) -> bool {
    history.iter().any(|(_, e)| {
        matches!(e, Entry::Verified(VerifiedEntry {
            what: What::HandOver {
                item: HandedOver::SegmentRecord { transaction: t, provider, began_on, runs_on, .. },
                ..
            },
            ..
        }) if t == transaction && provider == module && (began_on == version || runs_on == version))
    })
}

/// What a grant is of: a tuple, or a verifier's class in its place (`T-020-30`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Granted<'a> {
    Tuple(&'a Id),
    Class(&'a Id),
}

/// The grants of `provider` in force in `history`, in the order made, each with its advertising keys: a grant's
/// tuple or class until a later `revoke_grant` of the same takes it back, a new grant of it replacing the old.
fn grants_in_force<'a>(history: &'a History, provider: &Id) -> Vec<(Granted<'a>, &'a [PublicKey])> {
    let mut in_force: Vec<(Granted<'a>, &'a [PublicKey])> = Vec::new();
    for action in catalogue_changes(history) {
        let Action::ChangeCatalogue {
            change,
            provider: p,
            tuple,
            class,
            advertising_keys,
            ..
        } = action
        else {
            continue;
        };
        let granted = match (tuple, class) {
            (Some(tuple), None) => Granted::Tuple(tuple),
            (None, Some(class)) => Granted::Class(class),
            _ => continue,
        };
        if p != provider {
            continue;
        }
        in_force.retain(|(g, _)| *g != granted);
        if *change == CatalogueChange::Grant {
            in_force.push((granted, advertising_keys));
        }
    }
    in_force
}

/// The advertising keys of `provider`'s latest grant in `history` that no later `revoke_grant` of its tuple, or of
/// its class for a verifier, took back: the log's first trust in the provider's advertisements. A new grant replaces
/// the keys, which is how a provider recovers a lost one; with no grant in force there are none.
pub fn granted_advertising_keys<'a>(history: &'a History, provider: &Id) -> &'a [PublicKey] {
    grants_in_force(history, provider)
        .last()
        .map_or(&[], |(_, keys)| keys)
}

/// The catalogue changes `history` carried out, as [`catalogue_changes`] gives them, each with its effect's time: the
/// time a grant or a revocation takes force from, as `core-audit`'s key register reads it.
fn dated_catalogue_changes(history: &History) -> impl Iterator<Item = (&Action, i128)> {
    history.iter().filter_map(|(receipt, e)| match e {
        Entry::Verified(VerifiedEntry {
            what: What::Effect { action, .. },
            when,
            ..
        }) if matches!(action, Action::ChangeCatalogue { .. }) && done(history, receipt) => {
            Some((action, when.at.unix_nanos()))
        }
        _ => None,
    })
}

/// The classes `maker` is trusted for alone at `at`, a statement's own `made_at`: the grants as they stood then, each
/// tuple's or class's last change made at or before it (the latest by its effect's time, the later in the log on a
/// tie), as the key register trusts a key at that time, never as they stand when the statement is appended (the
/// review of `a2cde05d`). `None` where a tuple's grant was in force then, or where the maker never held a class's, and
/// its statements are attributed as before; otherwise the classes in force then, none where none was, so a maker
/// that has held classes alone attributes nothing beyond them, and nothing once none is in force.
fn classes_alone<'a>(history: &'a History, maker: &Id, at: &Timestamp) -> Option<Vec<&'a Id>> {
    let at = at.unix_nanos();
    let mut last: Vec<(Granted<'a>, i128, bool)> = Vec::new();
    let mut held_a_class = false;
    for (action, when) in dated_catalogue_changes(history) {
        let Action::ChangeCatalogue {
            change,
            provider,
            tuple,
            class,
            ..
        } = action
        else {
            continue;
        };
        if provider != maker {
            continue;
        }
        let granted = match (tuple, class) {
            (Some(tuple), None) => Granted::Tuple(tuple),
            (None, Some(class)) => Granted::Class(class),
            _ => continue,
        };
        let grant = *change == CatalogueChange::Grant;
        held_a_class |= grant && matches!(granted, Granted::Class(_));
        if when > at {
            continue;
        }
        match last.iter_mut().find(|(g, ..)| *g == granted) {
            Some(change) if change.1 > when => {}
            Some(change) => *change = (granted, when, grant),
            None => last.push((granted, when, grant)),
        }
    }
    let in_force = last.iter().filter(|(.., grant)| *grant).map(|(g, ..)| *g);
    if in_force.clone().any(|g| matches!(g, Granted::Tuple(_))) || !held_a_class {
        return None;
    }
    Some(
        in_force
            .filter_map(|g| match g {
                Granted::Class(c) => Some(c),
                Granted::Tuple(_) => None,
            })
            .collect(),
    )
}

/// Where a release stands, by the changes the log carried out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Standing {
    InForce,
    Revoked,
    Exploited,
}

/// The statement keys a module the operator publishes signs with, for a statement about `transaction` where it
/// names one: the keys of the releases of `module` the log accepted and that stand in force, a `reinstate` putting
/// a release back in force; and of a release since revoked, only for a statement about a transaction whose segment
/// began or ran on that release, which a revocation lets finish on it (`REQ-MDL-20`). A release declared exploited
/// signs nothing more, whatever segment is in flight on it.
pub fn release_statement_keys<'a>(
    history: &'a History,
    module: &Id,
    transaction: Option<&Id>,
) -> Vec<&'a PublicKey> {
    let mut releases: Vec<(&Id, &PublicKey, Standing)> = Vec::new();
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
        if let (CatalogueChange::AcceptModule, Some(key)) = (change, statement_key) {
            releases.retain(|(v, ..)| *v != version);
            releases.push((version, key, Standing::InForce));
            continue;
        }
        let standing = match change {
            CatalogueChange::RevokeRelease => Standing::Revoked,
            CatalogueChange::DeclareExploited => Standing::Exploited,
            CatalogueChange::Reinstate => Standing::InForce,
            _ => continue,
        };
        for release in releases.iter_mut().filter(|(v, ..)| *v == version) {
            release.2 = standing;
        }
    }
    releases
        .into_iter()
        .filter(|(version, _, standing)| match standing {
            Standing::InForce => true,
            Standing::Revoked => transaction.is_some_and(|t| ran_on(history, module, t, version)),
            Standing::Exploited => false,
        })
        .map(|(_, key, _)| key)
        .collect()
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
        let transaction = claim.about.as_ref().and_then(|a| a.transaction.as_ref());
        return Ok(
            release_statement_keys(history, maker, transaction).contains(&&claim.signed_with.key)
        );
    }
    keys.signs_for(maker, &claim.signed_with, &claim.when.at)
}

/// The refusal of a stated entry whose kept bytes bear its claim out, made in a role its maker doesn't hold.
pub const IN_A_ROLE_ITS_MAKER_DOES_NOT_HOLD: &str =
    "a verified statement is recorded only in a role its maker holds";

/// Whether `maker` holds `role`: the operator's own maker the operator's role, a maker granted a verifier's classes
/// alone the verifier's (`T-020-30`), and any other maker a provider's or a verifier's.
fn holds_role(maker: &Id, role: Role, operator: &Id, classes_alone: bool) -> bool {
    if maker == operator {
        role == Role::Operator
    } else if classes_alone {
        role == Role::Verifier
    } else {
        matches!(role, Role::Provider | Role::Verifier)
    }
}

/// Whether a statement of `maker`'s, made at `at`, is one its grants then attribute: for a maker granted a verifier's
/// classes alone ([`classes_alone`]), a judgement of one of those classes and nothing else, so a category grant never
/// attributes a statement about a tuple's segments, nor, revoked, one backdated into its window beyond its class
/// (`REQ-CAT-13`, `REQ-CPL-12`, `T-020-30`); for any other maker, anything, as before.
fn within_its_classes(maker: &Id, envelope: &Envelope, history: &History, at: &Timestamp) -> bool {
    match classes_alone(history, maker, at) {
        None => true,
        Some(classes) => {
            envelope.shape == JUDGEMENT_SHAPE
                && envelope
                    .statement
                    .get("class")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| classes.iter().any(|g| g.as_str() == c))
        }
    }
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

/// Whether `envelope` is a publisher's record of the segments inside a private node that `about` holds it to
/// (step 6, `T-020-36`): an `inner-segments` body, its node the one `about` names. About an order, as a node's record
/// is cited (`replay-cites-its-nodes-records`, an `evidence` naming the node), it names no segment and no transaction
/// of its own, lists at least one segment, and every segment's transaction is of that order in `history`: first
/// bound to it (`binding_of`), and asked under for it by `operator`'s own verified `operation` question, so the order
/// rests on transactions Core asked under for it, never on the record's word or on an entry anyone else appended.
/// `None` where the body is no such record, which step 6 reads as before.
fn inner_segments_of(
    envelope: &Envelope,
    about: Option<&About>,
    history: &History,
    operator: &Id,
) -> Option<bool> {
    if envelope.shape != crate::provider_role::INNER_SEGMENTS_SHAPE {
        return None;
    }
    let record: crate::provider_role::InnerSegments =
        serde_json::from_value(envelope.statement.clone()).ok()?;
    let Some(about) = about else {
        return Some(true);
    };
    let node = about.node.as_ref().map(|n| n.as_str());
    let Some(order) = about.order.as_ref() else {
        // About nothing but its node, or about a segment or a transaction, which it names none of: no order binds.
        return Some(
            about.segment.is_none()
                && about.transaction.is_none()
                && node.is_none_or(|n| n == record.node),
        );
    };
    Some(
        about.segment.is_none()
            && about.transaction.is_none()
            && node == Some(record.node.as_str())
            && !record.segments.is_empty()
            && record.segments.iter().all(|s| {
                Id::new(s.transaction.as_str()).is_some_and(|t| {
                    super::relations::binding_of(&t, history).is_some_and(|(o, _)| o == order)
                        && asked_under(&t, order, operator, history)
                })
            }),
    )
}

/// Whether `history` holds `operator`'s own verified `operation` question under `transaction` about `order`: Core
/// asked under it for that order (`answer-follows-its-question`'s question).
fn asked_under(transaction: &Id, order: &Id, operator: &Id, history: &History) -> bool {
    history.iter().any(|(_, e)| {
        matches!(e, Entry::Stated(s) if matches!(&s.attribution, Attribution::Verified { who } if &who.acts == operator)
            && s.what.statement.as_str() == crate::provider_role::OPERATION_SHAPE
            && s.about.as_ref().is_some_and(|a| {
                a.order.as_ref() == Some(order) && a.transaction.as_ref() == Some(transaction)
            }))
    })
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
/// one whose commitment names no kept statement, and one its bytes bear out in a role its maker doesn't hold; a key
/// or store it can't reach fails the append, and nothing is recorded.
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
            let node = about.and_then(|a| a.node.as_ref());
            let operator = keys.operator()?;
            let bound_as_named = match inner_segments_of(&envelope, about, history, &operator) {
                // A publisher's record of a node's segments: about an order only through its segments' transactions.
                Some(holds) => body_transaction.is_none() && holds,
                None => {
                    node.is_none()
                        && match about_transaction {
                            Some(about) => body_transaction == Some(about),
                            None => body_transaction.is_none() && !bound,
                        }
                }
            };
            envelope.deployment == keys.deployment()?
                && envelope.maker == claimed_by.acts.as_str()
                && envelope.shape == claim.what.statement.as_str()
                && envelope.made_at == claim.when.at.as_str()
                && envelope.signature.scheme == claim.signed_with.scheme
                && envelope.signature.key == claim.signed_with.key
                && bound_as_named
                && signatures.verifies(&bytes)
                && signed_by_its_maker(claim, &claimed_by.acts, history, keys)?
                && keys.publisher(&claimed_by.acts)?.as_ref() == Some(&claimed_by.party)
                && within_its_classes(&claimed_by.acts, &envelope, history, &claim.when.at)
        }
    };
    let alone = classes_alone(history, &claimed_by.acts, &claim.when.at);
    if verified
        && !holds_role(
            &claimed_by.acts,
            claimed_by.role,
            &keys.operator()?,
            alone.is_some(),
        )
    {
        return Err(AppendFailure::Refused(Violation(
            IN_A_ROLE_ITS_MAKER_DOES_NOT_HOLD,
        )));
    }
    let mut entry = claim.clone();
    if verified {
        entry.attribution = Attribution::Verified {
            who: claimed_by.clone(),
        };
    }
    Ok(entry)
}
