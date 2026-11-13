//! Party statements: what a party stated kept apart from what rutent verified, attributed to its maker.
//!
//! Declared by `T-020-01`; `T-022-02` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! The log already attributes a stated entry from the bytes the evidence store keeps, never from its writer
//! (`core-internal`'s `attribute`, held by `T-022-01`), and the evidence store already keeps each statement as
//! received, signature included, and finds a restatement by the content its signature covers (`T-022-03`). What this
//! module adds is what neither can do alone (`REQ-AUD-2`):
//!
//! - [`Statements`], the way a party's statement enters: keep the bytes, then append the stated entry, **naming
//!   whoever passed it on** where that isn't its maker, and answer what the evidence store said it restates. A relay's
//!   copy of another party's statement stays that party's, with the relay recorded apart (`relayed_by`); a relay that
//!   claims the copy as its own is recorded unverified, since the signature is the other's.
//! - [`restatements`], a read over the log and the evidence store's own read that finds every stated entry holding
//!   the same signed content as an earlier one, after the fact. `Kept::restates` reaches only the keeper that kept the
//!   bytes, once; this reads it again from what the log and the store hold, so a copy appended with no relay named is
//!   found too.
//! - [`unproven`], a read that holds each entry the log marks `verified` against the statement the store keeps, so an
//!   attribution that nothing kept bears out is found (`EXP-1`'s `G-09`). The log checks it on append; its opening
//!   reads no statement but an advertisement's (`T-022-01`), and the outside checker (`T-022-04`) holds none.
//!
//! Neither read changes the log or the store.

use rutent_contracts::core_internal::calls::{
    AppendFailure, Evidence, GeneralEntry, KeptStatements, Log, Signatures,
};
use rutent_contracts::core_internal::{
    Actor, Attribution, Entry, KeptStatement, Receipt, StatedEntry, Violation,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::signing_input;
use rutent_contracts::router_api::{Id, Timestamp};
use serde_json::{Value, json};

use crate::log::Ed25519Strict;
use crate::log::merkle::sha256;
use crate::receipts::bearing_out;

/// The refusal of bytes that aren't a statement envelope: nothing is kept, so no orphan is left behind.
pub const NOT_A_STATEMENT: &str =
    "a party's statement is an envelope with a shape, a maker, a time and a signature";

/// The refusal of a statement handed in about another transaction than its body names.
pub const ABOUT_ANOTHER_TRANSACTION: &str =
    "a statement is about the transaction its body names, never another";

/// A party's statement as it reached Core: the bytes exactly as received, who the writer says made it, and who
/// handed it over.
#[derive(Debug, Clone)]
pub struct Presented {
    /// The statement exactly as it arrived.
    pub bytes: Vec<u8>,
    /// The maker the statement is claimed to be by: the actor named in its envelope, as the catalogue knows it. The
    /// log attributes the entry to it only where the kept bytes bear the claim out; otherwise it stays a claim.
    pub maker: Actor,
    /// Who handed the statement to Core, as external authentication established it. Where this isn't the maker, the
    /// statement is a relayed one, and the entry says so.
    pub presented_by: Actor,
    /// When Core received it, by Core's clock.
    pub received_at: Timestamp,
    /// The order the statement is about, where Core knows it.
    pub order: Option<Id>,
    /// The segment the statement is about, where Core knows it.
    pub segment: Option<Id>,
}

/// What a statement's entry became.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// The receipt of the stated entry.
    pub receipt: Receipt,
    /// The salted commitment the statement is kept under.
    pub commitment: Digest,
    /// The commitment of the statement kept earlier whose signed content this one repeats, where it repeats one.
    pub restates: Option<Digest>,
    /// Who passed it on, where someone other than its maker did: recorded in the entry, apart from its maker.
    pub relayed_by: Option<Actor>,
}

/// The way party statements enter: the evidence store keeps the bytes, then the log records the stated entry.
pub struct Statements<E: Evidence, L: Log> {
    evidence: E,
    log: L,
}

impl<E: Evidence, L: Log> Statements<E, L> {
    /// The entry of statements through `evidence`, the evidence store's handle, and `log`, the general handle.
    pub fn new(evidence: E, log: L) -> Self {
        Statements { evidence, log }
    }

    /// Keeps `presented`'s bytes, then appends their stated entry, unverified, naming the maker it claims and, where
    /// someone other than that maker presented it, who did. The log attributes it, never this: the entry is
    /// `verified` only where the kept bytes bear the claim out.
    ///
    /// Bytes that aren't an envelope are refused before anything is kept. A failure keeping or appending changes
    /// nothing the caller can't repeat: the keep is idempotent in what it records, and a retry appends the entry again
    /// (`REQ-MDL-8`).
    pub fn receive(&mut self, presented: &Presented) -> Result<Received, AppendFailure> {
        let envelope: Value = serde_json::from_slice(&presented.bytes)
            .map_err(|_| AppendFailure::Refused(Violation(NOT_A_STATEMENT)))?;
        let field = |value: &Value| value.as_str().map(str::to_owned);
        let (Some(shape), Some(made_at), Some(scheme), Some(key)) = (
            field(&envelope["shape"]),
            field(&envelope["made_at"]),
            field(&envelope["signature"]["scheme"]),
            field(&envelope["signature"]["key"]),
        ) else {
            return Err(AppendFailure::Refused(Violation(NOT_A_STATEMENT)));
        };
        let transaction = field(&envelope["statement"]["transaction"]);
        let kept = self.evidence.keep(&presented.bytes)?;
        let relayed_by = (presented.presented_by.acts != presented.maker.acts)
            .then(|| presented.presented_by.clone());
        let mut entry = json!({
            "kind": "stated",
            "attribution": {"signature": "unverified", "claimed_by": presented.maker},
            "what": {"statement": shape},
            "when": {"at": made_at, "source": "party_stated"},
            "received_at": presented.received_at,
            "outcome": {"status": "stated"},
            "signed_with": {"scheme": scheme, "key": key},
            "commitment": kept.commitment,
        });
        if presented.order.is_some() || presented.segment.is_some() || transaction.is_some() {
            entry["about"] = json!({
                "order": presented.order,
                "segment": presented.segment,
                "transaction": transaction,
            });
            // `null` stands for nothing named.
            entry["about"]
                .as_object_mut()
                .expect("an object")
                .retain(|_, v| !v.is_null());
        }
        if let Some(relay) = &relayed_by {
            entry["relayed_by"] = json!(relay);
        }
        let entry: Entry = serde_json::from_value(entry)
            .map_err(|_| AppendFailure::Refused(Violation(ABOUT_ANOTHER_TRANSACTION)))?;
        let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
        let receipt = self.log.append(&entry)?;
        Ok(Received {
            receipt,
            commitment: kept.commitment,
            restates: kept.restates,
            relayed_by,
        })
    }
}

/// What the restatement index files a statement by: SHA-256 of its signed content, provider-role's `signing_input`,
/// or of the bytes themselves where they have none. The same key the evidence store files by (`T-022-03`'s
/// `content_of`, private to it, so repeated here: a test holds that the two agree).
fn content_of(received: &[u8]) -> [u8; 32] {
    sha256(&signing_input(received).unwrap_or_else(|_| received.to_vec()))
}

/// The bytes `kept` holds, where they bear out its commitment.
fn kept_bytes(kept: &KeptStatement) -> Option<Vec<u8>> {
    bearing_out(kept)
}

/// A stated entry that holds the signed content of an earlier one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restatement {
    /// The earlier stated entry whose content this repeats: the first of that content in the log.
    pub first: Receipt,
    /// The later stated entry.
    pub again: Receipt,
    /// Who the earlier entry names as its maker, attributed or claimed.
    pub restated: Actor,
    /// Who the later entry names as its maker, attributed or claimed.
    pub restater: Actor,
    /// Who the later entry says passed it on, where it says.
    pub relayed_by: Option<Actor>,
}

impl Restatement {
    /// Whether the later entry is the same actor's own repeat of what it stated: a resend by the maker, not a relay.
    pub fn is_resent_by_its_maker(&self) -> bool {
        self.restater.acts == self.restated.acts && self.relayed_by.is_none()
    }

    /// Whether the later entry names another maker than the earlier and no relay: someone's copy recorded as another's
    /// own word, with nothing saying who passed it on. A finding for the operator (`REQ-AUD-2`).
    pub fn is_passed_off(&self) -> bool {
        self.restater.acts != self.restated.acts && self.relayed_by.is_none()
    }
}

/// What a read of the log and the evidence store found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found<T> {
    /// What it found, in log order.
    pub found: Vec<T>,
    /// The stated entries whose bytes the store answered none for, an erasure's or a loss: neither could be held to
    /// anything, so what they might repeat or bear out isn't known (`REQ-AUD-13`).
    pub unread: Vec<Receipt>,
}

/// Every stated entry of `entries` whose signed content an earlier stated entry holds, read from the statements `kept`
/// answers for them (`REQ-AUD-2`). Two copies of one signed statement are found whatever salt each was kept under and
/// whoever the entry says presented it; a statement re-signed by anyone else has other signed content and is no
/// restatement of it. A failure of the store's read is the caller's to retry.
pub fn restatements(
    entries: &[(Receipt, Entry)],
    kept: &mut dyn KeptStatements,
) -> Result<Found<Restatement>, AppendFailure> {
    let mut first: Vec<([u8; 32], Receipt, Actor)> = Vec::new();
    let mut out = Found {
        found: Vec::new(),
        unread: Vec::new(),
    };
    for (receipt, entry) in entries {
        let Entry::Stated(stated) = entry else {
            continue;
        };
        let Some(bytes) = kept
            .kept(&stated.commitment)?
            .filter(|k| k.commitment == stated.commitment)
            .and_then(|k| kept_bytes(&k))
        else {
            out.unread.push(receipt.clone());
            continue;
        };
        let content = content_of(&bytes);
        let maker = stated.attribution.maker().clone();
        match first.iter().find(|(c, ..)| *c == content) {
            None => first.push((content, receipt.clone(), maker)),
            Some((_, earlier, restated)) => out.found.push(Restatement {
                first: earlier.clone(),
                again: receipt.clone(),
                restated: restated.clone(),
                restater: maker,
                relayed_by: stated.relayed_by.clone(),
            }),
        }
    }
    Ok(out)
}

/// The refusal an attribution the kept statement doesn't bear out stands for.
pub const NOT_BORNE_OUT: &str = "a stated entry attributed verified is borne out by the statement kept under its commitment: its maker, shape, time, key and signature";

/// Every stated entry of `entries` the log marks `verified` that the statement `kept` answers for it doesn't bear out:
/// the bytes don't hash, after their salt, to the commitment; the envelope names another maker, shape, time or key
/// than the entry's; or its signature doesn't verify, strictly, over its signing input (`EXP-1`'s `G-09`). What this
/// can't check is that the key is the maker's: that is the log's register, which a reader outside the log doesn't
/// hold.
pub fn unproven(
    entries: &[(Receipt, Entry)],
    kept: &mut dyn KeptStatements,
) -> Result<Found<(Receipt, Violation)>, AppendFailure> {
    let mut out = Found {
        found: Vec::new(),
        unread: Vec::new(),
    };
    for (receipt, entry) in entries {
        let Entry::Stated(stated) = entry else {
            continue;
        };
        let Attribution::Verified { who } = &stated.attribution else {
            continue;
        };
        let Some(statement) = kept
            .kept(&stated.commitment)?
            .filter(|k| k.commitment == stated.commitment)
        else {
            out.unread.push(receipt.clone());
            continue;
        };
        if !borne_out(stated, who, &statement) {
            out.found.push((receipt.clone(), Violation(NOT_BORNE_OUT)));
        }
    }
    Ok(out)
}

/// Whether `statement` bears out `entry`'s attribution to `who`.
fn borne_out(entry: &StatedEntry, who: &Actor, statement: &KeptStatement) -> bool {
    let Some(bytes) = kept_bytes(statement) else {
        return false;
    };
    let Ok(envelope) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    let Ok(signed_with) = serde_json::to_value(&entry.signed_with) else {
        return false;
    };
    envelope["maker"] == who.acts.as_str()
        && envelope["shape"] == entry.what.statement.as_str()
        && envelope["made_at"] == entry.when.at.as_str()
        && envelope["signature"]["scheme"] == signed_with["scheme"]
        && envelope["signature"]["key"] == signed_with["key"]
        && Ed25519Strict.verifies(&bytes)
}
