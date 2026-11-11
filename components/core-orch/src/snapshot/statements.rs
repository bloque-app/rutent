//! Statements tied to their makers' keys before a record is formed (router-api's "Unchecked by either",
//! `REQ-AUD-4`, `REQ-PRT-1`, `THR-3`).
//!
//! A decision record carries each fresh quote, dry run and eligibility judgement whole, and its contract checks that
//! a statement's `maker` is its figure's provider, but the snapshot carries no keys, so nothing in a record ties a
//! statement's `signature.key` to its maker. Core does, before the statement enters a snapshot:
//!
//! - **A provider's statement** verifies through the catalogue ([`Catalogue::verify_statement`]): it names the
//!   catalogue's deployment and protocol version, and its key is listed for `statements` by the maker's
//!   advertisement that was in force at the statement's `made_at` and by the one the catalogue holds now, so a key
//!   listed later can't sign for an earlier time and a retired key can't sign a backdated statement.
//! - **The operator's own statement**, the recorded decision among them, and one made by a module the operator
//!   publishes, verifies against the deployment's `operator_key` ([`Catalogue::verify_operator_statement`]).
//! - **Every statement nested in another**, at any depth (a judgement inside a result, a reversal confirmation), is
//!   tied the same way to its own maker, as its text appears in the outer statement's bytes
//!   ([`provider_role::member`](rutent_contracts::provider_role::member)). A nested statement Core can't reach by
//!   member names, such as one inside a list, refuses the whole statement: what can't be checked isn't taken.
//!
//! Keys compare by their decoded bytes (`PublicKey`). The time each check reads is Core's clock, the one reading the
//! snapshot is formed at. A statement refused here never reaches a snapshot; it is recorded as its claimed maker's
//! word, unverified ([`record_refused`]), as core-internal's table of homes has it.

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence};

use super::journal::{self, OrchestrationLog};
use rutent_contracts::core_internal::{
    About, Actor, Attribution, Entry, ShapeId, SignedWith, StatedEntry, StatedKind, StatedOutcome,
    StatedStatus, StatedTimeSource, StatedWhat, StatedWhen,
};
use rutent_contracts::provider_role::{Signature, member};
use rutent_contracts::router_api::{Timestamp, TupleKey, Violation};
use rutent_prov::catalogue::{Catalogue, Instant, StatementRefusal, Unusable};
use serde::Deserialize;
use serde_json::Value;

/// Why a statement was kept out of the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The bytes aren't a statement of the kind asked for: not its envelope, or another shape.
    Unreadable(String),
    /// A statement's key didn't tie it to its maker, or it belongs to another deployment or protocol version:
    /// the outer statement where `path` is empty, else the one nested at `path`, by member names.
    Key {
        /// Where the statement that failed sits in the outer one; empty for the outer one.
        path: Vec<String>,
        /// What the catalogue refused it for.
        refusal: StatementRefusal,
    },
    /// A statement is nested where member names can't reach it, inside a list, so its bytes can't be checked.
    NestedOutOfReach,
    /// A quote's or dry run's maker isn't the provider of the tuple it is about.
    NotItsProvider,
    /// The snapshot holds no tuple figure for the tuple it is about.
    NoTuple,
    /// A verifier's grant isn't in force for judgements at Core's clock (`REQ-CAT-2`).
    MakerNotUsable(Unusable),
    /// The verifier's figure has no origin: no accepted advertisement or no accepted release is known for it.
    NoOrigin,
    /// It is dated after the snapshot's `taken_at`, though within the clock's skew: its age can't be stated.
    DatedAfterSnapshot,
    /// The figure it would make breaks a rule of the router-api contract.
    Contract(Violation),
}

/// The parts of an envelope a refusal record names, read without trusting anything in it.
#[derive(Debug, Clone, Deserialize)]
struct Claimed {
    shape: String,
    maker: String,
    made_at: String,
    signature: Signature,
}

/// A statement kept out of a snapshot, with its bytes as received, to record as its claimed maker's unverified
/// word ([`record_refused`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedStatement {
    /// The tuple it was offered for.
    pub tuple: TupleKey,
    /// Its bytes, exactly as received.
    pub bytes: Vec<u8>,
    /// Why.
    pub reason: Refusal,
}

impl RefusedStatement {
    /// The maker the statement claims, where its envelope can be read.
    pub fn claimed_maker(&self) -> Option<String> {
        serde_json::from_slice::<Claimed>(&self.bytes)
            .ok()
            .map(|c| c.maker)
    }
}

/// Why a refused statement couldn't be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// Its envelope doesn't name a shape, a time and a key a stated entry can hold.
    Unrecordable,
    /// The evidence store or the log refused or failed.
    Append(AppendFailure),
}

/// Ties a provider's statement, given as received, and every statement nested in it, to its maker's keys at `at`:
/// the catalogue's deployment and protocol version, and a key listed for its purpose by the maker's advertisement
/// in force at its `made_at` and by the one the catalogue holds. A statement made by a module the operator
/// publishes is tied to the deployment's `operator_key` instead.
pub fn bind_statement(catalogue: &Catalogue, received: &[u8], at: Instant) -> Result<(), Refusal> {
    bind(catalogue, received, at, false)
}

/// Ties one of the operator's own statements, the recorded decision among them, to the deployment's
/// `operator_key`, and every statement nested in it to its own maker, as [`bind_statement`] does.
pub fn bind_operator_statement(
    catalogue: &Catalogue,
    received: &[u8],
    at: Instant,
) -> Result<(), Refusal> {
    bind(catalogue, received, at, true)
}

fn bind(
    catalogue: &Catalogue,
    received: &[u8],
    at: Instant,
    operator: bool,
) -> Result<(), Refusal> {
    let value: Value =
        serde_json::from_slice(received).map_err(|e| Refusal::Unreadable(e.to_string()))?;
    let outer = maker_of(&value).ok_or_else(|| Refusal::Unreadable("not an envelope".into()))?;
    let by_operator = operator || catalogue.publishers().published_by_operator(outer);
    verify(catalogue, received, at, by_operator).map_err(|refusal| Refusal::Key {
        path: Vec::new(),
        refusal,
    })?;
    let mut nested = Vec::new();
    find_nested(&value, &mut Vec::new(), false, &mut nested);
    for (path, reachable) in nested {
        if !reachable {
            return Err(Refusal::NestedOutOfReach);
        }
        let names: Vec<&str> = path.iter().map(String::as_str).collect();
        let bytes = member(received, &names)
            .ok()
            .flatten()
            .ok_or(Refusal::NestedOutOfReach)?;
        let inner: Value =
            serde_json::from_slice(bytes).map_err(|e| Refusal::Unreadable(e.to_string()))?;
        let maker = maker_of(&inner).ok_or(Refusal::NestedOutOfReach)?;
        let by_operator = catalogue.publishers().published_by_operator(maker);
        verify(catalogue, bytes, at, by_operator).map_err(|refusal| Refusal::Key {
            path: path.clone(),
            refusal,
        })?;
    }
    Ok(())
}

fn verify(
    catalogue: &Catalogue,
    bytes: &[u8],
    at: Instant,
    by_operator: bool,
) -> Result<(), StatementRefusal> {
    if by_operator {
        catalogue.verify_operator_statement(bytes, at)
    } else {
        catalogue.verify_statement(bytes, at)
    }
}

/// The maker of an envelope: an object with a `shape`, a `maker` and a `signature` object.
fn maker_of(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    object.get("shape")?.as_str()?;
    object.get("signature")?.as_object()?;
    object.get("maker")?.as_str()
}

/// Every envelope nested in `value`, by its path of member names, and whether member names reach it (none on the
/// way is a list).
fn find_nested(
    value: &Value,
    path: &mut Vec<String>,
    in_list: bool,
    found: &mut Vec<(Vec<String>, bool)>,
) {
    match value {
        Value::Object(members) => {
            if !path.is_empty() && maker_of(value).is_some() {
                found.push((path.clone(), !in_list));
            }
            for (name, inner) in members {
                path.push(name.clone());
                find_nested(inner, path, in_list, found);
                path.pop();
            }
        }
        Value::Array(items) => {
            for item in items {
                find_nested(item, path, true, found);
            }
        }
        _ => {}
    }
}

/// Records a refused statement as its claimed maker's word, unverified (`REQ-AUD-2`): its bytes go to the evidence
/// store, which answers their salted commitment, and a stated entry naming that commitment, with
/// `signature: unverified`, is appended to the log. `claimed_by` is the actor the statement claims made it;
/// `received_at` is when Core received it, by Core's clock.
pub fn record_refused(
    evidence: &mut dyn Evidence,
    log: &mut dyn OrchestrationLog,
    refused: &RefusedStatement,
    claimed_by: Actor,
    received_at: Timestamp,
    about: Option<About>,
) -> Result<rutent_contracts::core_internal::Receipt, RecordError> {
    let claimed: Claimed =
        serde_json::from_slice(&refused.bytes).map_err(|_| RecordError::Unrecordable)?;
    let shape = ShapeId::new(claimed.shape).ok_or(RecordError::Unrecordable)?;
    let made_at = Timestamp::new(claimed.made_at).ok_or(RecordError::Unrecordable)?;
    let kept = evidence.keep(&refused.bytes).map_err(RecordError::Append)?;
    let entry = Entry::Stated(StatedEntry {
        kind: StatedKind::Stated,
        attribution: Attribution::Unverified { claimed_by },
        what: StatedWhat { statement: shape },
        when: StatedWhen {
            at: made_at,
            source: StatedTimeSource::PartyStated,
        },
        received_at,
        outcome: StatedOutcome {
            status: StatedStatus::Stated,
        },
        signed_with: SignedWith {
            scheme: claimed.signature.scheme,
            key: claimed.signature.key,
        },
        commitment: kept.commitment,
        about,
        relayed_by: None,
    });
    entry.check().map_err(|_| RecordError::Unrecordable)?;
    journal::append(log, &entry).map_err(RecordError::Append)
}
