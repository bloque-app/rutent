//! What an order runs on (`T-020-61`, #143's first agreed blocker): built on its acceptance, held by intake, and rebuilt
//! from the log by a Core that reopens, through one reading of the log, so a Core that ran on and one that reopened run
//! the same route on the same terms.
//!
//! - **The decision record** is the one Core's key signed and kept in the evidence store as the operator's stated entry,
//!   read back from the store and held to the digest the log's `decision_record` entry names it by ([`held_record`], the
//!   review's L6). A record whose kept bytes aren't the ones the log names is no record: nothing starts on it.
//! - **The plan** is the order's `route_chosen` input, as the log took it.
//! - **The authorisation** is the one the order's acceptance names, with the terms policy holds in force for it: the one
//!   intake asked policy to permit, on an acceptance; the one policy's register reads from the log, on a reopening.
//! - **The origin, the session and the inputs** are the order's own: its request as intake runs it (an order intake
//!   opened), its filing (an order filed on an automatic deposit), or its acceptance and the first order's route (a
//!   linked order).
//! - **What the route delivers** is what its moves name, else what the quote shown priced from the record.
//!
//! Core keeps no store of its own beside the log: what is held here is rebuilt from the log and the evidence store it
//! already keeps (`NFR-6`).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::KeptStatements;
use rutent_contracts::core_internal::{
    Action, Authorisation, Entry, Receipt, RouteEnd, VerifiedEntry, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::{
    Amount, DECISION_RECORD_SHAPE, DecisionRecord, Id, PositiveAmount,
};
use rutent_contracts::state_machine::{Observed, PartPlan, RecordedInput};

use crate::schedule::Actors;
use crate::segments::Live;

/// What Core holds of an order it started, which its route runs on: the record that chose the route, the plan, the
/// authorisation and origin, what the route delivers, the session, the inputs and the floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The router's recorded decision that chose the route.
    pub record: DecisionRecord,
    /// The order's plan: its parts and their segments.
    pub plan: Vec<PartPlan>,
    /// The authorisation the order's value moves under.
    pub authorisation: Id,
    /// The origin reserve.
    pub origin: Id,
    /// What the route ends at.
    pub route: RouteEnd,
    /// The user's session.
    pub session: Option<Id>,
    /// The order's inputs, by name.
    pub inputs: BTreeMap<String, String>,
    /// The least the customer takes at the destination, as its authorisation names it.
    pub floor: Option<PositiveAmount>,
}

/// Where a segment sits in its order's plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// The part's place in the chosen candidate.
    pub at: usize,
    /// The part.
    pub part: Id,
    /// The part's segments, in the route's order.
    pub segments: Vec<Id>,
}

impl Started {
    /// The part `segment` is in, if the plan lists it.
    pub fn placed(&self, segment: &Id) -> Option<Placed> {
        self.plan.iter().enumerate().find_map(|(at, part)| {
            part.segments
                .iter()
                .any(|s| s.segment_id.as_str() == segment.as_str())
                .then(|| placed(at, part))
                .flatten()
        })
    }

    /// The order's first part.
    pub fn first(&self) -> Option<Placed> {
        self.plan.first().and_then(|part| placed(0, part))
    }

    /// Each part of the order's plan, in order.
    pub fn parts(&self) -> Vec<Placed> {
        self.plan
            .iter()
            .enumerate()
            .filter_map(|(at, part)| placed(at, part))
            .collect()
    }

    /// The part of this order at `placed`, as the segments run it: `core` writes Core's entries and asks for each
    /// move, the operator signs, and `record_entry` is the log's entry of the record, where it is named.
    pub fn live<'a>(
        &'a self,
        order: &'a Id,
        placed: &'a Placed,
        actors: &'a Actors,
        record_entry: Option<&'a Receipt>,
    ) -> Live<'a> {
        Live {
            order,
            part: &placed.part,
            segments: &placed.segments,
            record: &self.record,
            at: placed.at,
            authorisation: &self.authorisation,
            origin_reserve: &self.origin,
            route: &self.route,
            caller: &actors.core,
            core: &actors.core,
            operator: &actors.operator,
            session: self.session.as_ref(),
            inputs: &self.inputs,
            floor: self.floor.as_ref(),
            record_entry,
        }
    }
}

fn placed(at: usize, part: &PartPlan) -> Option<Placed> {
    Some(Placed {
        at,
        part: Id::new(part.part_id.as_str())?,
        segments: part
            .segments
            .iter()
            .map(|s| Id::new(s.segment_id.as_str()))
            .collect::<Option<_>>()?,
    })
}

/// What an order's start takes from its intake, its filing or its acceptance: its origin reserve, the session its
/// withdrawal goes in, the inputs its tuples need, and what its route delivers before the operator's fee, where its
/// source names it (else it is read from the log or the record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Begun {
    /// The origin reserve.
    pub origin: Id,
    /// The user's session.
    pub session: Option<Id>,
    /// The order's inputs, the destination's account among them.
    pub inputs: BTreeMap<String, String>,
    /// What the route delivers before the operator's fee, where the source names it.
    pub delivers: Option<Amount>,
}

/// The bytes the evidence store keeps behind `commitment`, as the log's own read gives them: `None` where it keeps
/// none, or keeps them under another commitment.
pub fn kept_bytes(kept: &mut dyn KeptStatements, commitment: &Digest) -> Option<Vec<u8>> {
    kept.kept(commitment)
        .ok()
        .flatten()
        .filter(|k| &k.commitment == commitment)
        .and_then(|k| crate::quote::from_base64url(k.bytes.as_str()))
}

/// The decision record `id`, as Core's key signed it and kept it in the evidence store, held to the digest the log's
/// `decision_record` entry names it by (the review's L6, `REQ-AUD-4`): the operator's stated entry of router-api's
/// decision-record shape whose kept bytes, canonical, hash to that digest, and read only from those bytes. `None` where
/// the log names no such record, or the store keeps no bytes that are it: nothing runs on a record the log doesn't
/// hold to (fails closed).
pub fn held_record(
    history: &[(Receipt, Entry)],
    kept: &mut dyn FnMut(&Digest) -> Option<Vec<u8>>,
    id: &Id,
) -> Option<DecisionRecord> {
    let named = history.iter().find_map(|(_, e)| match e {
        Entry::Verified(v) => match &v.what {
            What::DecisionRecord {
                decision_record,
                record,
                ..
            } if decision_record == id => Some(record.clone()),
            _ => None,
        },
        Entry::Stated(_) => None,
    })?;
    history.iter().find_map(|(_, entry)| {
        let Entry::Stated(stated) = entry else {
            return None;
        };
        if stated.what.statement.as_str() != DECISION_RECORD_SHAPE {
            return None;
        }
        let bytes = kept(&stated.commitment)?;
        let value = serde_json::from_slice::<serde_json::Value>(&bytes).ok()?;
        let canonical = rutent_contracts::provider_role::to_canonical(&value).ok()?;
        if named.value.as_str() != rutent_prov::module::wire::sha256(&canonical) {
            return None;
        }
        let record = serde_json::from_value::<DecisionRecord>(value["statement"].clone()).ok()?;
        (record.record_id() == id).then_some(record)
    })
}

/// The `route_chosen` input that started `order`, as the log took it.
pub fn start_in(history: &[(Receipt, Entry)], order: &Id) -> Option<RecordedInput> {
    history.iter().find_map(|(_, entry)| match entry {
        Entry::Verified(VerifiedEntry {
            what: What::Input { input },
            ..
        }) if input.subject.order.as_str() == order.as_str()
            && matches!(input.observed, Observed::RouteChosen { .. }) =>
        {
            Some(input.clone())
        }
        _ => None,
    })
}

/// The authorisation `order`'s acceptance names, as the log holds it.
pub fn accepted_authorisation(history: &[(Receipt, Entry)], order: &Id) -> Option<Id> {
    history.iter().find_map(|(_, e)| match e {
        Entry::Verified(VerifiedEntry {
            what:
                What::Acceptance {
                    order: o,
                    authorisation,
                    ..
                },
            ..
        }) if o == order => Some(authorisation.clone()),
        _ => None,
    })
}

/// What `order`'s route delivers before the operator's fee: as its moves name it, else as the quote shown priced it
/// from `record`, at the authorisation's floor.
fn delivers_of(
    history: &[(Receipt, Entry)],
    order: &Id,
    record: &DecisionRecord,
    floor: Option<&PositiveAmount>,
) -> Option<Amount> {
    history
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry {
                what:
                    What::Effect {
                        action:
                            Action::MoveValue {
                                order: o, route, ..
                            },
                        ..
                    },
                ..
            }) if o == order => Some(route.delivers.clone()),
            _ => None,
        })
        .or_else(|| {
            let candidate = record.chosen()?;
            let floor = floor.map(|f| f.amount().clone());
            crate::quote::show(record, candidate, floor.as_ref())
                .ok()
                .map(|shown| shown.arrives)
        })
}

/// What `order` runs on, from its `start`, the log's `history`, the evidence store's read `kept`, the authorisation
/// `authorised` its acceptance names as policy holds it in force, and what its source gave (`begun`). `None` where the
/// start isn't a route chosen, its record isn't held to the log's digest ([`held_record`]), or what it delivers can't
/// be read: nothing starts (fails closed).
pub fn started_on(
    history: &[(Receipt, Entry)],
    kept: &mut dyn FnMut(&Digest) -> Option<Vec<u8>>,
    order: &Id,
    start: &RecordedInput,
    authorised: &Authorisation,
    begun: Begun,
) -> Option<Started> {
    let Observed::RouteChosen {
        decision_record,
        parts,
        ..
    } = &start.observed
    else {
        return None;
    };
    let record = held_record(history, kept, &Id::new(decision_record.as_str())?)?;
    let delivers = match begun.delivers {
        Some(delivers) => delivers,
        None => delivers_of(history, order, &record, authorised.floor.as_ref())?,
    };
    Some(Started {
        record,
        plan: parts.clone(),
        // The id the acceptance names: the moves cite it, whichever of the order's authorisations holds its terms in force.
        authorisation: accepted_authorisation(history, order)
            .unwrap_or_else(|| authorised.authorisation.clone()),
        origin: begun.origin,
        route: RouteEnd {
            destination: authorised.destination.clone(),
            to_asset: authorised.to_asset.clone(),
            delivers,
        },
        session: begun.session,
        inputs: begun.inputs,
        floor: authorised.floor.clone(),
    })
}
