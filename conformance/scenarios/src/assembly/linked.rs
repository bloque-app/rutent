//! A linked order's acceptance in the assembly (`T-024-17`; `REQ-ORD-43`, `REQ-ORD-34`).
//!
//! A linked order starts at an inner node, where an earlier order's value rests, which no request through intake
//! does: intake routes from where a customer's money is, withdraws from the origin reserve and asks its yes. The
//! customer's acceptance of a linked order is the customer's own, given in its session, on a quote shown for it, and
//! it rests on the yes of the provider that starts its route (`REQ-ORD-32`).
//!
//! [`Core::accept_linked`] routes the linked order from the node with the rounds Core runs for every order, so the
//! router's record is in the log, signed with the operator's key, and then writes what intake writes for an order
//! and the log's relations read: the quote shown and the customer's acceptance, on the yes of the provider the route
//! starts with, which is its answer to the dry run the rounds asked it. It keeps the route the order was accepted on
//! ([`Core::accepted_linked`]), for the mover to hold a hand-on's route to. It starts nothing: the order's
//! `route_chosen` is the hand-on's, recorded once the value is handed on ([`Core::hand_on`]).
//!
//! The route is the router's, from the node: its first segment is a tuple's provider's, never the node's holding
//! account's, since a wallet advertises no tuple and no segment kind runs from a node to itself.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Authorisations, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    AcceptanceBasis, Actor, Attribution, Authorisation, Entry, Outcome, PartYes, Parties, Receipt,
    ShownAs, TimeSource, VerifiedEntry, VerifiedKind, What, When,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::{Id, Request};
use rutent_contracts::state_machine::{self as stm, PartPlan, SegmentPlan};
use rutent_core_orch::intake::OrderRecord;
use rutent_core_orch::rounds::{self, Fresh, History, Offer, Presence, Routing};
use rutent_core_orch::segments::Ran;
use rutent_core_orch::stuck::AcceptedRoute;
use rutent_router::RouteFinder;

use super::{Asking, Core, Started, StepError};
use crate::kit;

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
    /// The inputs the route's tuples need.
    pub inputs: BTreeMap<String, String>,
}

/// What accepting a linked order recorded.
#[derive(Debug, Clone)]
pub struct AcceptedLinked {
    /// The linked order.
    pub order: Id,
    /// The customer's acceptance of it.
    pub acceptance: Id,
    /// The quote's shown entry.
    pub shown: Receipt,
    /// The starting reserve's stated yes.
    pub yes: Receipt,
    /// The acceptance's entry.
    pub entry: Receipt,
    /// The route it was accepted on, as the mover holds a hand-on to it.
    pub route: AcceptedRoute,
}

impl Core {
    /// The customer `linked.customer` accepts a linked order: its route chosen from the node by the rounds, the quote
    /// shown, the yes of the provider the route starts with and the acceptance given in the customer's session; and
    /// the route kept as the order's accepted route.
    ///
    /// # Errors
    ///
    /// If the rounds offer no route from the node, the wallet doesn't answer, or the log refuses a step.
    pub fn accept_linked(&mut self, linked: &LinkedOrder) -> Result<AcceptedLinked, StepError> {
        let order = self.fresh.id();
        // The route continues the original order's, from where its value rests: a recalculation's request, which the
        // rounds ask no reserve's yes for (`T-020-10`), as the value is already in a holding account's book.
        let mut route = linked.route.clone();
        route.follows = Some(
            self.started
                .get(linked.from.as_str())
                .ok_or(StepError::NotNow("the order handed on started"))?
                .record
                .request
                .request_id
                .clone(),
        );
        let core = kit::core();
        let operator = kit::operator();
        // The router's route from the node, in the rounds every order runs.
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
        let routed = {
            let mut asking = Asking {
                fleet: &mut self.fleet,
                clock: &self.clock,
            };
            let mut charges = self.policy.clone();
            let mut persons = self.policy.clone();
            let mut parts = rounds::Parts {
                catalogue: &mut self.catalogue,
                clock: &self.clock,
                exposures: &self.exposures,
                releases: &self.releases,
                log: &mut self.general,
                evidence: &mut self.keeper,
                charges: &mut charges,
                persons: &mut persons,
                providers: &mut asking,
                operator_key: &mut self.signer,
                fresh: &mut self.fresh,
                issued: &mut self.issued,
            };
            rounds::route::<RouteFinder>(
                &mut parts,
                Routing {
                    request: &route,
                    bounds: &self.bounds,
                    prices: &self.prices,
                    order: &routing_order,
                    core: &core,
                    operator: &operator,
                },
                &mut History::new(),
            )
            .map_err(|_| StepError::NotNow("the rounds route the linked order"))?
        };
        let Offer::Offered { candidate, .. } = &routed.offer else {
            return Err(StepError::NotNow(
                "the rounds offer the linked order a route",
            ));
        };
        let chosen = routed
            .choices
            .last()
            .ok_or(StepError::NotNow("the rounds chose"))?;
        let candidate = chosen
            .record
            .candidates()
            .iter()
            .find(|c| &c.candidate_id == candidate)
            .cloned()
            .ok_or(StepError::NotNow("the offer names a candidate"))?;
        // The plan is the router's route from the node, each part and segment under a fresh identifier, as intake plans
        // an order's.
        let [only] = candidate.parts.as_slice() else {
            return Err(StepError::NotNow("the linked order has one part"));
        };
        let first = only.operations.first();
        let NodeRef::Inner(_) = &first.from.node else {
            return Err(StepError::NotNow(
                "the linked route starts at an inner node",
            ));
        };
        let mut id = |_: ()| -> Result<stm::Id, StepError> {
            stm::Id::new(self.fresh.id().as_str()).ok_or(StepError::NotNow("a fresh identifier"))
        };
        let part_id = id(())?;
        let mut segments = Vec::new();
        for operation in only.operations.iter() {
            segments.push(SegmentPlan {
                segment_id: id(())?,
                provider: stm::Id::new(operation.provider.as_str())
                    .ok_or(StepError::NotNow("a provider is an identifier"))?,
                kind: operation.kind,
                from: operation.from.clone(),
                to: operation.to.clone(),
            });
        }
        let starting = first.provider.clone();
        let plan = vec![PartPlan {
            part_id: part_id.clone(),
            segments,
        }];

        // The starting reserve's yes: its answer to the dry run Core asked it for the route's first operation, a
        // verified statement about the linked order under the transaction Core asked it under.
        let asked = routed
            .sent
            .iter()
            .find(|s| {
                s.provider == starting && s.tuple.from == first.from && s.tuple.to == first.to
            })
            .and_then(|s| s.transaction.clone())
            .ok_or(StepError::NotNow("Core asked the starting reserve"))?;
        let yes = self
            .log
            .borrow()
            .entries()
            .iter()
            .find_map(|(receipt, entry)| match entry {
                Entry::Stated(s)
                    if matches!(&s.attribution, Attribution::Verified { who } if who.acts == starting)
                        && s.about.as_ref().is_some_and(|a| {
                            a.order.as_ref() == Some(&order) && a.transaction.as_ref() == Some(&asked)
                        }) =>
                {
                    Some(receipt.clone())
                }
                _ => None,
            })
            .ok_or(StepError::NotNow("the starting reserve's answer is the log's"))?;

        // The quote shown, and the acceptance on it.
        let shown = rounds_digest(&order, &linked.route)?;
        let shown_entry = self.appended(
            &core,
            What::Shown {
                order: order.clone(),
                shown: shown.clone(),
                shown_as: ShownAs::Quote,
                awaiting: None,
            },
        )?;
        let parties = self.parties_of(&linked.from)?;
        let acceptance = self.fresh.id();
        let authorisation = self.fresh.id();
        let entry = self.appended(
            &linked.customer,
            What::Acceptance {
                acceptance: acceptance.clone(),
                order: order.clone(),
                request: kit::id(linked.route.request_id.as_str()),
                quote: shown,
                authorisation,
                basis: AcceptanceBasis::Given,
                parties,
                origin_reserve: starting.clone(),
                yeses: vec![PartYes {
                    part: kit::id(part_id.as_str()),
                    origin_reserve: starting.clone(),
                    reserve_yes: yes.clone(),
                }],
                session: Some(linked.session.clone()),
                held: None,
            },
        )?;
        let route = AcceptedRoute {
            order: order.clone(),
            decision_record: chosen.record.record_id().clone(),
            acceptance: acceptance.clone(),
            plan,
        };
        self.accepted_linked(route.clone());
        self.feed()?;
        Ok(AcceptedLinked {
            order,
            acceptance,
            shown: shown_entry,
            yes,
            entry,
            route,
        })
    }

    /// Runs the first move of the linked order `accepted`, handed on from `from` once its start is proven
    /// ([`Core::hand_on`]) (`T-020-33`, `T-020-36`'s item 4): the customer's authorisation of it, as policy registers it
    /// (the original order's destination, the holding account as its origin reserve), and then its route, each segment
    /// by the provider its plan names, which needn't be the holding account.
    ///
    /// # Errors
    ///
    /// If the order handed on from didn't start, policy doesn't permit the authorisation, or a step stops.
    pub fn run_linked(
        &mut self,
        from: &Id,
        linked: &LinkedOrder,
        accepted: &AcceptedLinked,
    ) -> Result<Vec<Ran>, StepError> {
        let original = self
            .started
            .get(from.as_str())
            .cloned()
            .ok_or(StepError::NotNow("the order handed on from started"))?;
        let authorisation_id = self
            .verified_whats()
            .into_iter()
            .find_map(|what| match what {
                What::Acceptance {
                    order,
                    authorisation,
                    ..
                } if order == accepted.order => Some(authorisation),
                _ => None,
            })
            .ok_or(StepError::NotNow(
                "the log holds the linked order's acceptance",
            ))?;
        let first = accepted
            .route
            .plan
            .first()
            .and_then(|part| part.segments.first())
            .ok_or(StepError::NotNow("the linked route has a first segment"))?;
        let authorisation = Authorisation {
            authorisation: authorisation_id.clone(),
            order: accepted.order.clone(),
            origin_reserve: linked.holder.clone(),
            origin_account: None,
            destination: original.route.destination.clone(),
            to_asset: original.route.to_asset.clone(),
            floor: None,
            parts: None,
            given_by: linked.customer.clone(),
            at: self.clock.timestamp(),
        };
        // Policy reads the order's requester from intake's records: the linked order is recorded there, as its
        // acceptance names its parties (the original customer the requester), its origin the holding account.
        let parties = self.parties_of(from)?;
        self.records.orders.borrow_mut().insert(
            accepted.order.as_str().to_owned(),
            OrderRecord {
                order: accepted.order.clone(),
                request: kit::id(linked.route.request_id.as_str()),
                parties,
                origin_reserve: linked.holder.clone(),
            },
        );
        let decision = Authorisations::authorise(&mut self.policy, &authorisation)
            .map_err(|_| StepError::NotNow("policy takes the linked order's authorisation"))?;
        if decision.permit().is_none() {
            return Err(StepError::NotNow(
                "policy permits the linked order's authorisation",
            ));
        }
        let record = self.record_of(&accepted.order, &accepted.route.decision_record)?;
        self.started.insert(
            accepted.order.as_str().to_owned(),
            Started {
                record,
                plan: accepted.route.plan.clone(),
                authorisation: authorisation_id,
                origin: kit::id(first.provider.as_str()),
                route: original.route.clone(),
                session: Some(linked.session.clone()),
                inputs: linked.inputs.clone(),
                floor: None,
            },
        );
        self.run(&accepted.order)
    }

    /// The parties of `order`'s acceptance, as the log holds them: the original customer, kept as the linked
    /// order's requester (`REQ-ORD-13`).
    fn parties_of(&self, order: &Id) -> Result<Parties, StepError> {
        self.verified_whats()
            .into_iter()
            .find_map(|what| match what {
                What::Acceptance {
                    order: o, parties, ..
                } if &o == order => Some(parties),
                _ => None,
            })
            .ok_or(StepError::NotNow("the log holds the order's acceptance"))
    }

    /// Appends `what`, observed by `who` at Core's clock, as orchestration's own record.
    fn appended(&mut self, who: &Actor, what: What) -> Result<Receipt, AppendFailure> {
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: who.clone(),
            what,
            when: When {
                at: self.clock.timestamp(),
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::Observed {},
        });
        let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
        self.general.append(&entry)
    }
}

/// The digest the linked order's quote is shown by: of the request it answers, canonical.
fn rounds_digest(order: &Id, route: &Request) -> Result<Digest, StepError> {
    let canonical = rutent_contracts::provider_role::to_canonical(&serde_json::json!({
        "order": order,
        "request": route,
    }))
    .map_err(|_| StepError::NotNow("a quote has a canonical form"))?;
    serde_json::from_value(serde_json::json!({
        "algorithm": "sha-256",
        "value": rutent_prov::module::wire::sha256(&canonical),
    }))
    .map_err(|_| StepError::NotNow("a quote's digest"))
}
