//! A linked order's acceptance, Core's own (`T-020-61`, item 10; the review of #355, H2; `REQ-ORD-43`, `REQ-ORD-34`,
//! `REQ-ORD-32`, `REQ-ORD-10`).
//!
//! A linked order starts at an inner node, where an earlier order's value rests, which no request through intake does:
//! intake routes from where a customer's money is, withdraws from the origin reserve and asks its yes. The customer's
//! acceptance of a linked order is the customer's own, given in its session, on a quote shown for it, and it rests on
//! the yes of the provider that starts its route. [`accept_linked`] does it over the deployment's [`Ports`]:
//!
//! - **Routed** from the node with the rounds Core runs for every order, so the router's record is in the log, signed
//!   with the operator's key, the route a continuation of the original order's (`follows`), which asks no reserve's yes.
//! - **Planned**: one part, each segment under its part's identifier and place (`hand_on::linked_segment_id`), so a Core
//!   that reopens derives the plan again from the log.
//! - **The customer's inputs logged**: the operator's statement of them, kept in the evidence store, its commitment in
//!   the log (`ORDER_INPUTS_SHAPE`, as intake keeps an order's, `REQ-AUD-6`). Core's run reads them back when it starts
//!   the linked order, so a route whose tuples need more than the destination's account runs with them.
//! - **Shown and accepted**: the quote's `shown` entry, and the customer's `acceptance` on the starting provider's yes,
//!   its answer to the dry run the rounds asked it.
//! - **Authorised under the customer's own floor**: policy is asked for the authorisation the acceptance names, given by
//!   the customer, with the floor the customer set, to the original order's destination and to asset. Where policy
//!   permits none, the linked order never starts: Core's run composes no authorisation in the customer's name.
//!
//! - **Kept**: the route it was accepted on goes into Core's own register of linked routes ([`Ports::with_linked`]),
//!   which the mover holds a hand-on's route to; the deployment writes nothing there (`T-020-68`).
//!
//! It starts nothing: the order's `route_chosen` is the hand-on's, recorded by Core's run once the value is handed on.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::{
    AcceptanceBasis, Actor, Attribution, Authorisation, Entry, ORDER_INPUTS_SHAPE, OrderInputs,
    PartYes, Parties, Receipt, ShownAs, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::{Id, PositiveAmount, Request};
use rutent_contracts::state_machine::{self as stm, PartPlan, SegmentPlan};
use rutent_router::RouteFinder;

use super::record;
use crate::rounds::{self, History, Offer, Presence, Routing};
use crate::schedule::Ports;
use crate::stuck::AcceptedRoute;

/// A linked order the customer accepts: the value of `from`, resting at the node its route starts at, in the account
/// of `holder`, handed on to the route from the node.
#[derive(Debug, Clone)]
pub struct LinkedOrder {
    /// The order whose value is handed on.
    pub from: Id,
    /// The provider of the holding account at the node, where the order's value rests.
    pub holder: Id,
    /// The customer, the original order's requester, who accepts.
    pub customer: Actor,
    /// The customer's open session.
    pub session: Id,
    /// The route from the node, as the customer asks it.
    pub route: Request,
    /// The inputs the route's tuples need, the customer's.
    pub inputs: BTreeMap<String, String>,
    /// The least the customer will take at the destination, in the to asset's unit, if it sets one.
    pub floor: Option<PositiveAmount>,
}

/// What accepting a linked order recorded.
#[derive(Debug, Clone)]
pub struct AcceptedLinked {
    /// The linked order.
    pub order: Id,
    /// The customer's acceptance of it.
    pub acceptance: Id,
    /// The authorisation the acceptance names, as policy permitted it.
    pub authorisation: Id,
    /// The quote's shown entry.
    pub shown: Receipt,
    /// The starting reserve's stated yes.
    pub yes: Receipt,
    /// The acceptance's entry.
    pub entry: Receipt,
    /// The commitment the customer's inputs are kept under, where it gave any.
    pub inputs: Option<Digest>,
    /// The route it was accepted on, as the mover holds a hand-on to it.
    pub route: AcceptedRoute,
}

/// Why a linked order wasn't accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedError {
    /// The order handed on isn't one Core started.
    NotStarted,
    /// The rounds offered no route from the node.
    NoRoute(String),
    /// The route isn't one part starting at an inner node.
    NotLinked,
    /// The starting provider's yes isn't in the log.
    NoYes,
    /// The log holds no acceptance of the order handed on, to take its parties from.
    NoParties,
    /// Policy didn't permit the authorisation the acceptance names.
    NotAuthorised,
    /// Something couldn't be drawn, signed, kept or appended.
    Unrecorded(&'static str),
}

/// The customer `linked.customer` accepts a linked order (see the module's docs).
///
/// # Errors
///
/// As [`LinkedError`] says.
pub fn accept_linked(
    ports: &mut dyn Ports,
    linked: &LinkedOrder,
) -> Result<AcceptedLinked, LinkedError> {
    let started = ports
        .intake()
        .started(&linked.from)
        .cloned()
        .ok_or(LinkedError::NotStarted)?;
    let actors = ports.actors();
    let bounds = ports.bounds();
    let history = ports.history();
    let parties = parties_of(&history, &linked.from).ok_or(LinkedError::NoParties)?;
    // The route continues the original order's, from where its value rests: a recalculation's request, which the rounds
    // ask no reserve's yes for, as the value is already in a holding account's book.
    let mut route = linked.route.clone();
    route.follows = Some(started.record.request.request_id.clone());
    let mut done: Result<Planned, LinkedError> =
        Err(LinkedError::Unrecorded("the rounds weren't lent"));
    ports.with_recalculations(&mut |_, parts, prices| {
        done = (|| {
            let order = super::draw_order(&mut *parts.fresh)
                .ok_or(LinkedError::Unrecorded("a fresh order identifier"))?;
            let routing_order = rounds::Order {
                order: order.clone(),
                caller: linked.customer.clone(),
                origin_reserve: linked.holder.clone(),
                presence: Presence::User {
                    session: linked.session.clone(),
                },
                inputs: linked.inputs.clone(),
                needs: Vec::new(),
            };
            let mut round = rounds::Parts {
                catalogue: &mut *parts.catalogue,
                clock: parts.clock,
                exposures: &*parts.exposures,
                releases: parts.releases,
                log: &mut *parts.log,
                evidence: &mut *parts.evidence,
                charges: &mut *parts.charges,
                persons: &mut *parts.persons,
                providers: &mut *parts.providers,
                operator_key: &mut *parts.operator_key,
                fresh: &mut *parts.fresh,
                issued: &mut *parts.issued,
            };
            let routed = rounds::route::<RouteFinder>(
                &mut round,
                Routing {
                    request: &route,
                    bounds: &bounds,
                    prices,
                    order: &routing_order,
                    core: &actors.core,
                    operator: &actors.operator,
                },
                &mut History::new(),
            )
            .map_err(|why| LinkedError::NoRoute(format!("{why:?}")))?;
            let Offer::Offered { candidate, .. } = &routed.offer else {
                return Err(LinkedError::NoRoute("no route offered".to_owned()));
            };
            let chosen = routed
                .choices
                .last()
                .ok_or(LinkedError::NoRoute("no choice".to_owned()))?;
            let candidate = chosen
                .record
                .candidates()
                .iter()
                .find(|c| &c.candidate_id == candidate)
                .cloned()
                .ok_or(LinkedError::NoRoute(
                    "the offer names no candidate".to_owned(),
                ))?;
            let [only] = candidate.parts.as_slice() else {
                return Err(LinkedError::NotLinked);
            };
            let first = only.operations.first();
            let NodeRef::Inner(_) = &first.from.node else {
                return Err(LinkedError::NotLinked);
            };
            let part_id = stm::Id::new(parts.fresh.id().as_str())
                .ok_or(LinkedError::Unrecorded("a part identifier"))?;
            let mut segments = Vec::new();
            for (place, operation) in only.operations.iter().enumerate() {
                segments.push(SegmentPlan {
                    segment_id: crate::hand_on::linked_segment_id(&part_id, place)
                        .ok_or(LinkedError::Unrecorded("a segment identifier"))?,
                    provider: stm::Id::new(operation.provider.as_str())
                        .ok_or(LinkedError::Unrecorded("a provider identifier"))?,
                    kind: operation.kind,
                    from: operation.from.clone(),
                    to: operation.to.clone(),
                });
            }
            let asked = routed
                .sent
                .iter()
                .find(|s| {
                    s.provider == first.provider
                        && s.tuple.from == first.from
                        && s.tuple.to == first.to
                })
                .and_then(|s| s.transaction.clone())
                .ok_or(LinkedError::NoYes)?;
            // The customer's inputs, the operator's statement of them, kept: the log holds the commitment alone.
            let inputs = if linked.inputs.is_empty() {
                None
            } else {
                let made_at =
                    record::now(parts.clock).map_err(|_| LinkedError::Unrecorded("a time"))?;
                let bytes = rounds::sign_statement(
                    &mut *parts.operator_key,
                    &bounds,
                    ORDER_INPUTS_SHAPE,
                    &actors.operator.acts,
                    &made_at,
                    OrderInputs {
                        order: order.clone(),
                        inputs: linked.inputs.clone(),
                    },
                )
                .map_err(|_| LinkedError::Unrecorded("the inputs signed"))?;
                let stated = crate::segments::stated_entry(
                    &mut *parts.evidence,
                    &made_at,
                    &bytes,
                    actors.operator.clone(),
                    &actors.operator,
                    None,
                )
                .map_err(|_| LinkedError::Unrecorded("the inputs kept"))?;
                let Entry::Stated(entry) = &stated else {
                    return Err(LinkedError::Unrecorded("the inputs stated"));
                };
                let commitment = entry.commitment.clone();
                record::append_entry(&mut *parts.log, &stated)
                    .map_err(|_| LinkedError::Unrecorded("the inputs appended"))?;
                Some(commitment)
            };
            Ok(Planned {
                order,
                record: chosen.record.record_id().clone(),
                starting: first.provider.clone(),
                asked,
                part: part_id.clone(),
                plan: vec![PartPlan { part_id, segments }],
                inputs,
            })
        })();
    });
    ports.fed();
    let planned = done?;
    let history = ports.history();
    // The starting reserve's yes: its answer to the dry run Core asked it, a verified statement about the linked order
    // under the transaction Core asked it under.
    let yes = history
        .iter()
        .find_map(|(receipt, entry)| match entry {
            Entry::Stated(s)
                if matches!(&s.attribution, Attribution::Verified { who } if who.acts == planned.starting)
                    && s.about.as_ref().is_some_and(|a| {
                        a.order.as_ref() == Some(&planned.order)
                            && a.transaction.as_ref() == Some(&planned.asked)
                    }) =>
            {
                Some(receipt.clone())
            }
            _ => None,
        })
        .ok_or(LinkedError::NoYes)?;
    let shown = shown_digest(&planned.order, &linked.route)
        .ok_or(LinkedError::Unrecorded("the quote's digest"))?;
    let acceptance = ports.fresh();
    let authorisation = ports.fresh();
    let mut appended: Result<(Receipt, Receipt), LinkedError> =
        Err(LinkedError::Unrecorded("the intake weren't lent"));
    let clock_now = record::now(ports.clock()).map_err(|_| LinkedError::Unrecorded("a time"))?;
    ports.with_recalculations(&mut |_, parts, _| {
        appended = (|| {
            let shown_entry = record::append(
                &mut *parts.log,
                parts.clock,
                &actors.core,
                What::Shown {
                    order: planned.order.clone(),
                    shown: shown.clone(),
                    shown_as: ShownAs::Quote,
                    awaiting: None,
                },
            )
            .map_err(|_| LinkedError::Unrecorded("the shown entry"))?;
            let entry = record::append(
                &mut *parts.log,
                parts.clock,
                &linked.customer,
                What::Acceptance {
                    acceptance: acceptance.clone(),
                    order: planned.order.clone(),
                    request: Id::new(linked.route.request_id.as_str())
                        .ok_or(LinkedError::Unrecorded("a request identifier"))?,
                    quote: shown.clone(),
                    authorisation: authorisation.clone(),
                    basis: AcceptanceBasis::Given,
                    parties: parties.clone(),
                    origin_reserve: planned.starting.clone(),
                    yeses: vec![PartYes {
                        part: Id::new(planned.part.as_str())
                            .ok_or(LinkedError::Unrecorded("a part identifier"))?,
                        origin_reserve: planned.starting.clone(),
                        reserve_yes: yes.clone(),
                    }],
                    session: Some(linked.session.clone()),
                    held: None,
                },
            )
            .map_err(|_| LinkedError::Unrecorded("the acceptance"))?;
            Ok((shown_entry, entry))
        })();
    });
    ports.fed();
    let (shown_entry, entry) = appended?;
    // The authorisation the acceptance names, given by the customer under its own floor, to the original order's end.
    let asked = Authorisation {
        authorisation: authorisation.clone(),
        order: planned.order.clone(),
        origin_reserve: linked.holder.clone(),
        origin_account: None,
        destination: started.route.destination.clone(),
        to_asset: started.route.to_asset.clone(),
        floor: linked.floor.clone(),
        parts: None,
        given_by: linked.customer.clone(),
        at: clock_now,
    };
    let mut permitted = false;
    ports.with_authorisations(&mut |authorisations| {
        permitted = authorisations
            .authorise(&asked)
            .ok()
            .is_some_and(|decision| decision.permit().is_some());
    });
    ports.fed();
    if !permitted {
        return Err(LinkedError::NotAuthorised);
    }
    // Core keeps the route the order was accepted on in its own register of linked routes, which the mover holds a
    // hand-on's route to (`T-020-68`, the check of `T-020-60`'s Low-Medium 2): no deployment writes it there. A register
    // the deployment doesn't lend is no acceptance, so nothing is handed on to a route Core holds none of.
    let route = AcceptedRoute {
        order: planned.order.clone(),
        decision_record: planned.record,
        acceptance: acceptance.clone(),
        plan: planned.plan,
    };
    let mut kept = false;
    ports.with_linked(&mut |linked| {
        linked.keep_route(route.clone());
        kept = true;
    });
    if !kept {
        return Err(LinkedError::Unrecorded("the register of linked routes"));
    }
    Ok(AcceptedLinked {
        order: planned.order,
        acceptance,
        authorisation,
        shown: shown_entry,
        yes,
        entry,
        inputs: planned.inputs,
        route,
    })
}

/// What the rounds planned for a linked order, before it is shown.
struct Planned {
    order: Id,
    record: Id,
    starting: Id,
    asked: Id,
    part: stm::Id,
    plan: Vec<PartPlan>,
    inputs: Option<Digest>,
}

/// The parties of `order`'s acceptance, as the log holds them: the original customer, kept as the linked order's
/// requester (`REQ-ORD-13`).
fn parties_of(history: &[(Receipt, Entry)], order: &Id) -> Option<Parties> {
    history.iter().find_map(|(_, e)| match e {
        Entry::Verified(v) => match &v.what {
            What::Acceptance {
                order: o, parties, ..
            } if o == order => Some(parties.clone()),
            _ => None,
        },
        Entry::Stated(_) => None,
    })
}

/// The digest of the quote shown for a linked order: its order and its route request, canonical.
fn shown_digest(order: &Id, route: &Request) -> Option<Digest> {
    let canonical = rutent_contracts::provider_role::to_canonical(&serde_json::json!({
        "order": order,
        "request": route,
    }))
    .ok()?;
    serde_json::from_value(serde_json::json!({
        "algorithm": "sha-256",
        "value": rutent_prov::module::wire::sha256(&canonical),
    }))
    .ok()
}

/// The customer's inputs for the linked `order`, as Core kept them at its acceptance: the operator's statement of them,
/// read back through `kept` from the evidence store. Empty where none were given or none read back.
pub fn linked_inputs(
    history: &[(Receipt, Entry)],
    kept: &mut dyn FnMut(&Digest) -> Option<Vec<u8>>,
    order: &Id,
) -> BTreeMap<String, String> {
    history
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Stated(s) if s.what.statement.as_str() == ORDER_INPUTS_SHAPE => {
                Some(s.commitment.clone())
            }
            _ => None,
        })
        .filter_map(|commitment| kept(&commitment))
        .filter_map(|bytes| {
            serde_json::from_slice::<rutent_contracts::provider_role::Statement<OrderInputs>>(
                &bytes,
            )
            .ok()
        })
        .find(|s| s.shape == ORDER_INPUTS_SHAPE && &s.statement.order == order)
        .map(|s| s.statement.inputs)
        .unwrap_or_default()
}
