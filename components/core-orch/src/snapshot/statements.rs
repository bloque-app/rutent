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
//! - **Every statement nested in another**, at any depth and in any list (the provider statements a decision
//!   record carries in `snapshot.figures`, a judgement inside a result), is tied the same way to its own maker, over
//!   its own text exactly as it appears in the outer statement's bytes ([`PathStep`]).
//!
//! Keys compare by their decoded bytes (`PublicKey`). Every time read here is Core's clock: [`super::Snapshots`]
//! passes its one reading, and the functions here read the [`Clock`] they are handed, never a time a caller
//! chooses. A statement refused before the record never reaches a snapshot; it is recorded ([`record_refused`])
//! unverified, naming its claimed maker and the transaction its bytes sign, and the log sets its attribution from the
//! kept bytes (core-internal's README, "The log, not the writer, attributes a statement").

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence};
use rutent_contracts::core_internal::{
    About, Actor, ActorKind, Attribution, Entry, Receipt, Role, ShapeId, SignedWith, StatedEntry,
    StatedKind, StatedOutcome, StatedStatus, StatedTimeSource, StatedWhat, StatedWhen,
};
use rutent_contracts::provider_role::{CanonicalError, JUDGEMENT_SHAPE, Signature, canonicalize};
use rutent_contracts::router_api::{Id, Timestamp, TupleKey, Violation};
use rutent_prov::catalogue::{Catalogue, Instant, Publisher, StatementRefusal, Unusable};
use serde::Deserialize;
use serde_json::Value;

use super::clock::Clock;
use super::journal::{self, OrchestrationLog};
use super::span::{PathStep, value_at};

/// Why a statement was kept out of the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The bytes aren't a statement of the kind asked for: not its envelope, or another shape.
    Unreadable(String),
    /// An object in the bytes names a member twice, at this byte offset (RFC 7493 §2.3): which of the two a reader
    /// takes is not decided, so nothing in it is taken.
    DuplicateMember {
        /// Where the second name starts, in bytes from the start of the statement as received.
        at: usize,
    },
    /// The bytes are no I-JSON text the signing rule can make canonical (RFC 8785).
    NotIJson(CanonicalError),
    /// A statement's key didn't tie it to its maker, or it belongs to another deployment or protocol version:
    /// the outer statement where `path` is empty, else the one nested at `path`.
    Key {
        /// Where the statement that failed sits in the outer one; empty for the outer one.
        path: Vec<PathStep>,
        /// What the catalogue refused it for.
        refusal: StatementRefusal,
    },
    /// A nested statement's text couldn't be found in the outer statement's bytes.
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
    /// The statement's body, read only for the transaction it names.
    #[serde(default)]
    statement: Value,
}

/// A statement kept out of a snapshot, with its bytes as received, to record ([`record_refused`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedStatement {
    /// The tuple it was offered for.
    pub tuple: TupleKey,
    /// Its bytes, exactly as received.
    pub bytes: Vec<u8>,
    /// Why.
    pub reason: Refusal,
    /// Whether its own signature verified against a key of its maker's when the snapshot was formed. Informational
    /// only: [`record_refused`] never reads it, and the log attributes the statement from its kept bytes.
    pub verified: bool,
}

impl RefusedStatement {
    /// The maker the statement names, where its envelope can be read.
    pub fn maker(&self) -> Option<String> {
        serde_json::from_slice::<Claimed>(&self.bytes)
            .ok()
            .map(|c| c.maker)
    }
}

/// Why a refused statement couldn't be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// Its envelope doesn't name a shape, a maker, a time and a key a stated entry can hold.
    Unrecordable,
    /// The evidence store or the log refused or failed.
    Append(AppendFailure),
}

/// Ties a provider's statement, given as received, and every statement nested in it, to its maker's keys at Core's
/// clock: the catalogue's deployment and protocol version, and a key listed for its purpose by the maker's
/// advertisement in force at its `made_at` and by the one the catalogue holds. A statement made by a module the
/// operator publishes is tied to the deployment's `operator_key` instead.
pub fn bind_statement(
    catalogue: &Catalogue,
    clock: &dyn Clock,
    received: &[u8],
) -> Result<(), Refusal> {
    bind_at(catalogue, received, clock.now().floor(), false)
}

/// Ties one of the operator's own statements, the recorded decision among them, to the deployment's
/// `operator_key`, and every statement nested in it to its own maker, as [`bind_statement`] does.
pub fn bind_operator_statement(
    catalogue: &Catalogue,
    clock: &dyn Clock,
    received: &[u8],
) -> Result<(), Refusal> {
    bind_at(catalogue, received, clock.now().floor(), true)
}

/// [`bind_statement`] at one reading of Core's clock, the snapshot's.
pub(super) fn bind_at(
    catalogue: &Catalogue,
    received: &[u8],
    at: Instant,
    operator: bool,
) -> Result<(), Refusal> {
    // The whole text, nested statements included, must be I-JSON before anything in it is read: `serde_json` would
    // take the last of a member named twice, and the signing rule refuses the text.
    match canonicalize(received) {
        Ok(_) => {}
        Err(CanonicalError::DuplicateMember { at }) => return Err(Refusal::DuplicateMember { at }),
        Err(error) => return Err(Refusal::NotIJson(error)),
    }
    let value: Value =
        serde_json::from_slice(received).map_err(|e| Refusal::Unreadable(e.to_string()))?;
    let outer = maker_of(&value).ok_or_else(|| Refusal::Unreadable("not an envelope".into()))?;
    let by_operator = operator || catalogue.publishers().published_by_operator(outer);
    verify(catalogue, received, at, by_operator).map_err(|refusal| Refusal::Key {
        path: Vec::new(),
        refusal,
    })?;
    let mut nested = Vec::new();
    find_nested(&value, &mut Vec::new(), &mut nested);
    for path in nested {
        let bytes = value_at(received, &path).ok_or(Refusal::NestedOutOfReach)?;
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

/// Whether a statement's own signature, the outer one only, ties it to its maker at `at`.
pub(super) fn outer_verifies(catalogue: &Catalogue, received: &[u8], at: Instant) -> bool {
    if canonicalize(received).is_err() {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<Value>(received) else {
        return false;
    };
    let Some(maker) = maker_of(&value) else {
        return false;
    };
    let by_operator = catalogue.publishers().published_by_operator(maker);
    verify(catalogue, received, at, by_operator).is_ok()
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

/// Every envelope nested in `value`, by its path.
fn find_nested(value: &Value, path: &mut Vec<PathStep>, found: &mut Vec<Vec<PathStep>>) {
    match value {
        Value::Object(members) => {
            if !path.is_empty() && maker_of(value).is_some() {
                found.push(path.clone());
            }
            for (name, inner) in members {
                path.push(PathStep::Member(name.clone()));
                find_nested(inner, path, found);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (place, item) in items.iter().enumerate() {
                path.push(PathStep::Item(place));
                find_nested(item, path, found);
                path.pop();
            }
        }
        _ => {}
    }
}

/// Records a refused statement (`REQ-AUD-2`): its bytes go to the evidence store, which answers their salted
/// commitment, and a stated entry naming that commitment is appended to the log, received at Core's clock. The entry
/// is appended unverified, naming the maker its envelope names as its claimed maker, and the log sets its attribution
/// from the kept bytes (core-internal's `attribute`): orchestration never attributes a statement, and the log's
/// general handle refuses one it did. Its `about` is the one handed in, with the transaction the bytes sign where
/// they name one, since the log verifies a statement naming a transaction only about that transaction. The maker
/// acts for its publisher, or for the operator where the operator publishes it; a verifier's judgement is made in
/// the verifier's role.
pub fn record_refused(
    evidence: &mut dyn Evidence,
    log: &mut dyn OrchestrationLog,
    clock: &dyn Clock,
    catalogue: &Catalogue,
    refused: &RefusedStatement,
    about: Option<About>,
) -> Result<Receipt, RecordError> {
    let claimed: Claimed =
        serde_json::from_slice(&refused.bytes).map_err(|_| RecordError::Unrecordable)?;
    let received_at = clock.now().timestamp().ok_or(RecordError::Unrecordable)?;
    let acts = Id::new(claimed.maker.as_str()).ok_or(RecordError::Unrecordable)?;
    let (party, role) = match catalogue.publishers().publisher(acts.as_str()) {
        Some(Publisher::Operator) => (Id::new("operator"), Role::Operator),
        Some(Publisher::Party(publisher)) => (Id::new(publisher), Role::Provider),
        None => (Some(acts.clone()), Role::Provider),
    };
    let role = if claimed.shape == JUDGEMENT_SHAPE {
        Role::Verifier
    } else {
        role
    };
    let maker = Actor {
        party: party.ok_or(RecordError::Unrecordable)?,
        acts,
        role,
        kind: ActorKind::Component,
    };
    let shape = ShapeId::new(claimed.shape).ok_or(RecordError::Unrecordable)?;
    let made_at = Timestamp::new(claimed.made_at).ok_or(RecordError::Unrecordable)?;
    let attribution = Attribution::Unverified { claimed_by: maker };
    let signed_transaction = match claimed.statement.get("transaction") {
        None => None,
        Some(t) => Some(
            t.as_str()
                .and_then(Id::new)
                .ok_or(RecordError::Unrecordable)?,
        ),
    };
    let about = match signed_transaction {
        None => about,
        Some(transaction) => Some(match about {
            Some(about) => About {
                transaction: Some(transaction),
                ..about
            },
            None => About {
                order: None,
                segment: None,
                transaction: Some(transaction),
            },
        }),
    };
    let kept = evidence.keep(&refused.bytes).map_err(RecordError::Append)?;
    let entry = Entry::Stated(StatedEntry {
        kind: StatedKind::Stated,
        attribution,
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
