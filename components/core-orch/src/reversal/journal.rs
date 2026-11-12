//! The one place reversals and endings reach the log: orchestration's handle, each entry built once as a
//! `GeneralEntry` and appended again unchanged while the log can't take it now, so a retried append is one entry
//! (`INV-8`). A receipt is taken only where it is this entry's, by the receipt rule (`crate::stuck::is_receipt_of`).
//!
//! - **A stated entry** (a provider's statement, Core's question, a reserve's answer) is appended as its claimed
//!   maker's, unverified: attributing it is the log's. Its bytes go to the evidence store when it is built.
//! - **A verified entry** (an `evidence`, an `afterwards`, a state-machine input) is orchestration's own record,
//!   observed at Core's clock.

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    About, Actor, ActorKind, Attribution, Entry, Outcome, Receipt, Role, ShapeId, SignedWith,
    StatedEntry, StatedKind, StatedOutcome, StatedStatus, StatedTimeSource, StatedWhat, StatedWhen,
    TimeSource, VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::provider_role::Signature;
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_prov::catalogue::{Catalogue, Publisher};
use serde::Deserialize;

use super::ReversalError;
use crate::snapshot::Clock;
use crate::stuck::{APPEND_TRIES, is_receipt_of};

/// The refusal of a time Core's clock reads that no timestamp holds.
const NO_TIME: Violation = Violation("Core's clock reads a time a timestamp can hold");

/// Core's clock now, as an entry's time.
pub(super) fn now(clock: &dyn Clock) -> Result<Timestamp, AppendFailure> {
    clock
        .now()
        .timestamp()
        .ok_or(AppendFailure::Refused(NO_TIME))
}

/// One of Core's own records: a verified entry of `what` by `who` at Core's clock, observed, checked.
pub(super) fn verified(
    clock: &dyn Clock,
    who: &Actor,
    what: What,
) -> Result<GeneralEntry, AppendFailure> {
    let entry = Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: who.clone(),
        what,
        when: When {
            at: now(clock)?,
            source: TimeSource::CoreClock,
        },
        outcome: Outcome::Observed {},
    });
    entry.check().map_err(AppendFailure::Refused)?;
    GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)
}

/// The parts of an envelope a stated entry names, read without trusting anything in it.
#[derive(Deserialize)]
struct Claimed {
    shape: String,
    maker: String,
    made_at: String,
    signature: Signature,
}

/// The maker `bytes` name in their envelope, if they name one.
pub(super) fn maker_named(bytes: &[u8]) -> Option<Id> {
    serde_json::from_slice::<Claimed>(bytes)
        .ok()
        .and_then(|c| Id::new(c.maker))
}

/// The actor a provider's statement claims made it: the provider, stating for its publisher, the operator's party for
/// one the operator publishes.
pub(super) fn provider_maker(catalogue: &Catalogue, operator: &Actor, provider: &Id) -> Actor {
    let party = match catalogue.publishers().publisher(provider.as_str()) {
        Some(Publisher::Operator) => Some(operator.party.clone()),
        Some(Publisher::Party(publisher)) => Id::new(publisher),
        None => None,
    };
    Actor {
        party: party.unwrap_or_else(|| provider.clone()),
        acts: provider.clone(),
        role: Role::Provider,
        kind: ActorKind::Component,
    }
}

/// Keeps `bytes`, a statement as received or as Core signed it, and builds its stated entry, claimed by `maker`,
/// unverified, received at Core's clock, about `about`.
pub(super) fn stated(
    evidence: &mut dyn Evidence,
    clock: &dyn Clock,
    bytes: &[u8],
    maker: Actor,
    about: About,
) -> Result<GeneralEntry, ReversalError> {
    let claimed: Claimed =
        serde_json::from_slice(bytes).map_err(|_| ReversalError::Unrecordable)?;
    let shape = ShapeId::new(claimed.shape).ok_or(ReversalError::Unrecordable)?;
    let made_at = Timestamp::new(claimed.made_at).ok_or(ReversalError::Unrecordable)?;
    let received_at = now(clock)?;
    let kept = evidence.keep(bytes)?;
    let entry = Entry::Stated(StatedEntry {
        kind: StatedKind::Stated,
        attribution: Attribution::Unverified { claimed_by: maker },
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
        about: Some(about),
        relayed_by: None,
    });
    entry.check().map_err(AppendFailure::Refused)?;
    Ok(GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?)
}

/// Appends an entry built already, trying again while the log can't take it now or answers with a receipt that
/// isn't this entry's. A refusal is never retried.
fn append(log: &mut dyn Log, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
    let mut last = AppendFailure::Unavailable;
    for _ in 0..APPEND_TRIES {
        match log.append(entry) {
            Ok(receipt) if is_receipt_of(&receipt, entry) => return Ok(receipt),
            Ok(_) | Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
            Err(refused) => return Err(refused),
        }
    }
    Err(last)
}

/// An entry built once, and its receipt once the log took it.
#[derive(Debug, Clone)]
pub(super) struct Step {
    pub(super) entry: GeneralEntry,
    pub(super) receipt: Option<Receipt>,
}

impl Step {
    pub(super) fn new(entry: GeneralEntry) -> Self {
        Step {
            entry,
            receipt: None,
        }
    }

    /// Appends the step unless the log took it already, keeping its receipt.
    pub(super) fn append(&mut self, log: &mut dyn Log) -> Result<Receipt, AppendFailure> {
        if let Some(receipt) = &self.receipt {
            return Ok(receipt.clone());
        }
        let receipt = append(log, &self.entry)?;
        self.receipt = Some(receipt.clone());
        Ok(receipt)
    }
}
