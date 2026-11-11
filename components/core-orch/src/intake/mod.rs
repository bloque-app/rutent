//! Order intake and acceptance: requester, beneficiary and payer recorded apart, one order per request identifier, a
//! stable class of cause, the quote shown and the acceptance recorded, the destination resolved at acceptance.
//!
//! Declared by `T-020-01`; `T-020-03` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! One [`Intake`] takes an order from its request to its start, in three calls, each on the one contract humans and
//! agents share (`REQ-OPS-5`): the caller is an [`Actor`], whose `kind` says whether a person or an agent acts, and
//! nothing else differs.
//!
//! 1. **[`Intake::open`]** takes an [`OrderRequest`] under the caller's request identifier. A request that breaks
//!    the deployment's rules is refused with nothing created: an `order_refusal` naming no order, and the caller is
//!    told [`Told::Refused`]. Any other opens one order, told [`Told::Exists`] with its outcome to come. The same
//!    request again under the same identifier opens nothing and is told the same; another request under it is
//!    refused as [`IntakeError::Conflicting`], with nothing recorded (`REQ-ORD-19`, `INV-8`). Its destination is
//!    resolved, and the owner it resolves to kept, so a later change of owner shows.
//! 2. **[`Intake::quote`]** runs one routing ([`rounds::route`]). Every question to a provider, the reserve's
//!    request for its yes among them, is the rounds': asked under a fresh transaction for the order, signed with the
//!    operator's key and appended about that order and transaction before it is sent, its answer appended with
//!    that transaction in its `about` (`REQ-ORD-32`, `REQ-ORD-38`). An offered route is shown as a [`Quote`] and
//!    recorded as a `shown` entry, labelled a quote, by its digest. A routing that offers nothing is recorded as an
//!    `order_refusal` of its class (`REQ-ORD-18`).
//! 3. **[`Intake::accept`]** takes the customer's or its agent's acceptance of the quote shown, by its digest, and
//!    only from the order's requester. The destination is resolved again: "not yet" waits, "no" refuses, and a
//!    change of owner waits for the customer to confirm it ([`Intake::confirm_owner`]) (`REQ-ORD-24`). The order's
//!    authorisation, naming its destination with the account the requester gave (`REQ-ORD-35`, `REQ-AUZ-3`) and
//!    the actor that accepted (`REQ-AUZ-9`), goes to policy; only on its permit is the `acceptance` recorded, its
//!    `who` the actor that accepted, an agent as an agent (`REQ-ORD-10`), naming its parties apart (`REQ-ORD-17`),
//!    the quote shown, the origin reserve's yes and the user's session (`REQ-ORD-32`). Only then is the order started:
//!    the `route_chosen` input, recorded once (`REQ-ORD-34`, `REQ-ORD-19`).
//!    An acceptance whose append, or whose start's, failed is carried on by accepting again, from where it stopped:
//!    policy isn't asked again, nothing is appended twice, and the order is told accepted only once it has started.
//!
//! **What the caller is told holds no provider's text** (`REQ-ORD-18`). [`Told`] carries identifiers, a class from
//! a closed list and the quote Core built from the router's record; a provider's words stay in its stated entry.
//!
//! **What intake keeps** (every request identifier and its answer) is in memory; keeping it across a restart is the
//! deployment's (`F-030`), as with the rounds' `Issued`.

mod class;
mod record;

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Authorisations};
use rutent_contracts::core_internal::{
    AcceptanceBasis, Actor, Answer, Authorisation, Destination, DestinationOutcome,
    OrderRefusalClass, Parties, Receipt, Role, ShownAs, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{NodeRef, to_canonical};
use rutent_contracts::router_api::{
    Amount, Candidate, Endpoint, Id, PolicyBounds, PositiveAmount, Request, Router,
};
use rutent_contracts::state_machine::{
    self as stm, Observed, PartPlan, RecordId, RecordedInput, SegmentPlan, Source, SourceKind,
    Standing, Subject,
};
use serde::{Deserialize, Serialize};

use crate::rounds::{self, History, Need, Offer, Order, Parts, Presence, Routing, RoutingError};
use crate::snapshot::{Price, check_part};

/// The input a reserve's deposit names the account or address it deposits to (provider-role's
/// `beneficiary.address`): intake sets it from the order's destination, so the deposit asked is the one authorised
/// (`REQ-ORD-35`).
pub const DESTINATION_INPUT: &str = "beneficiary.address";

/// What a destination resolves to now (`REQ-ORD-24`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// It resolves, to this owner, by reference.
    Owner(Id),
    /// No answer yet, which is not a no.
    NotYet,
    /// It doesn't resolve: the reserve can't deposit there.
    Refused,
}

/// Where a destination is resolved. No provider-role operation resolves one or names its owner, so the assembly
/// answers it, from the destination reserve's own register; Core records each answer as a `destination_check`.
pub trait Destinations {
    /// Resolves `destination` now.
    fn resolve(&mut self, destination: &Destination) -> Resolution;
}

/// An order request, as a person or an agent makes it (`REQ-OPS-5`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRequest {
    /// The caller's identifier for the request: one order per identifier (`REQ-ORD-19`).
    pub request: Id,
    /// Who asks: the requester's person or agent, in the customer's role.
    pub caller: Actor,
    /// The order's three parties, recorded apart (`REQ-ORD-17`). A settlement account's reserve, where named, is
    /// the destination's.
    pub parties: Parties,
    /// The reserve the value leaves from, where the holder's account is.
    pub origin_reserve: Id,
    /// The user's session at the origin reserve, opened through policy: the reserve's yes is asked in it
    /// (`REQ-ORD-32`).
    pub session: Id,
    /// Where the value arrives, its account or address the requester's own if it gives one (`REQ-ORD-35`).
    pub destination: Destination,
    /// The route asked for: its `from`, its `to` (the destination's node and the to asset), its amount, its
    /// objective and any floor. Its `request_id` and `round` are Core's to set for each round.
    pub route: Request,
    /// The inputs the order carries for tuples that require them. The destination's account goes as
    /// [`DESTINATION_INPUT`], set by intake; one named here must be that account.
    pub inputs: BTreeMap<String, String>,
    /// The judgements the route's tuples need.
    pub needs: Vec<Need>,
}

/// The quote shown to the customer: the route Core offers, built from the router's record, and nothing a provider
/// wrote (`REQ-ORD-10`, `REQ-ORD-18`). Its digest is what the `shown` entry and the acceptance name. `T-020-07`
/// adds the operator's fee shown apart, the slippage limits and a quote without a firm rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    /// The order.
    pub order: Id,
    /// The choice round's decision record, by id, that chose the route.
    pub decision_record: Id,
    /// The chosen candidate.
    pub candidate: Id,
    /// Where the value starts.
    pub from: Endpoint,
    /// Where it arrives.
    pub to: Endpoint,
    /// What leaves `from`, in its unit.
    pub amount: PositiveAmount,
    /// What the route delivers at `to`, in its unit, before the operator's fee: the sum of its parts'.
    pub delivers: Amount,
    /// How it is shown: always a quote before execution (`REQ-QTE-7`).
    pub shown_as: ShownAs,
}

/// Why an order waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wait {
    /// Its destination isn't resolved yet (`destination_not_yet_resolved`): accept again later.
    DestinationNotYetResolved,
    /// Its destination's owner changed since it was asked for: the customer confirms the new owner, then accepts
    /// anew (`REQ-ORD-24`).
    OwnerChanged,
    /// Policy couldn't decide its authorisation: nothing moves on it (`REQ-AUZ-5`).
    PolicyNotKnown,
}

/// Where an order stands, as its caller is told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    /// Opened; no quote shown yet.
    OutcomeToCome,
    /// A quote is shown, to accept or decline.
    Quoted {
        /// The quote.
        quote: Box<Quote>,
        /// Its digest: what an acceptance names.
        digest: Digest,
    },
    /// It waits.
    Waiting {
        /// Why.
        wait: Wait,
    },
    /// It can't proceed, for this class.
    CantProceed {
        /// The class.
        class: OrderRefusalClass,
    },
    /// Accepted and started.
    Accepted {
        /// The acceptance's id.
        acceptance: Id,
    },
}

/// What a caller is told about its request: refused with nothing existing, or an order that exists with its outcome
/// to come (`REQ-ORD-19`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "told", rename_all = "snake_case")]
pub enum Told {
    /// Refused: no order exists.
    Refused {
        /// The class.
        class: OrderRefusalClass,
    },
    /// An order exists.
    Exists {
        /// The order.
        order: Id,
        /// Where it stands.
        status: Status,
    },
}

/// An order's record, as the assembly answers policy's sources from it: who requested it, its three parties and
/// its origin reserve (`F-021`'s notes: a customer authorises only an order its party requested).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRecord {
    /// The order.
    pub order: Id,
    /// The request identifier it was opened under.
    pub request: Id,
    /// Its parties.
    pub parties: Parties,
    /// Its origin reserve.
    pub origin_reserve: Id,
}

/// Why a call answered nothing to tell. Nothing after the failing step was recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeError {
    /// Another request was made under this identifier: it opens nothing.
    Conflicting {
        /// The order the identifier opened, if it opened one.
        order: Option<Id>,
    },
    /// No order of that id.
    NoSuchOrder,
    /// Only the order's requester, a person or an agent acting for its party in the customer's role, accepts it or
    /// confirms its destination.
    NotTheRequester,
    /// The acceptance names no quote shown for the order, or one since replaced.
    NotTheQuoteShown,
    /// The order isn't at the step the call needs: no quote to accept, no owner change to confirm, or nothing to
    /// quote once it is accepted or can't proceed.
    NotNow,
    /// The routing stopped.
    Routing(RoutingError),
    /// The log, or policy, refused or failed.
    Log(AppendFailure),
    /// A fresh identifier was one Core used before.
    Fresh,
    /// The offered route has no reserve's yes to cite, or a part Core can't run.
    Unofferable,
}

impl From<AppendFailure> for IntakeError {
    fn from(failure: AppendFailure) -> Self {
        IntakeError::Log(failure)
    }
}

impl From<RoutingError> for IntakeError {
    fn from(error: RoutingError) -> Self {
        IntakeError::Routing(error)
    }
}

/// What intake reaches: the rounds' parts (the log, the clock, the providers, fresh identifiers), the destination
/// register, policy's authorisations, and what the router runs under. The assembly builds it.
pub struct Desk<'a, 'p> {
    /// What the rounds reach.
    pub parts: &'a mut Parts<'p>,
    /// Where destinations resolve.
    pub destinations: &'a mut dyn Destinations,
    /// Policy's authorisations.
    pub authorisations: &'a mut dyn Authorisations,
    /// The bounds the router runs under.
    pub bounds: &'a PolicyBounds,
    /// The prices Core holds.
    pub prices: &'a [Price],
    /// Core, acting on its own account: who writes intake's own records.
    pub core: &'a Actor,
    /// The operator, in whose name Core signs its questions and records.
    pub operator: &'a Actor,
}

/// The route offered, kept until it is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Offered {
    quote: Quote,
    digest: Digest,
    candidate: Candidate,
    reserve_yes: Receipt,
    session: Id,
}

/// One request identifier's record.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Kept {
    /// The request as the caller made it: a repeat is the same request only if it equals this.
    asked: OrderRequest,
    /// The request as intake runs it, the destination's account set as the deposit's input.
    request: OrderRequest,
    /// Absent where the request was refused with nothing created.
    order: Option<Id>,
    status: Status,
    history: History,
    /// The owner the destination first resolved to.
    owner: Option<Id>,
    /// The owner it resolved to since, waiting for the customer's confirmation.
    changed_owner: Option<Id>,
    offered: Option<Offered>,
    /// An acceptance under way, once policy permitted its authorisation.
    accepting: Option<Accepting>,
}

/// An acceptance under way: what was made, so a retry after a failed append makes nothing twice.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Accepting {
    /// The authorisation policy permitted.
    authorisation: Id,
    /// The acceptance's id.
    acceptance: Id,
    /// Whether its entry was appended.
    recorded: bool,
    /// The `route_chosen` input, once built: appended as built, its record id fixed.
    input: Option<RecordedInput>,
}

impl Kept {
    fn told(&self) -> Told {
        match (&self.order, &self.status) {
            (Some(order), status) => Told::Exists {
                order: order.clone(),
                status: status.clone(),
            },
            (None, Status::CantProceed { class }) => Told::Refused { class: *class },
            (None, _) => Told::Refused {
                class: OrderRefusalClass::MalformedRequest,
            },
        }
    }
}

/// Core's intake: every request identifier it was given, and the order each opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Intake {
    requests: BTreeMap<String, Kept>,
    orders: BTreeMap<String, String>,
}

impl Intake {
    /// No request taken yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the caller of `request` is told now, if that identifier was used.
    pub fn told(&self, request: &Id) -> Option<Told> {
        self.requests.get(request.as_str()).map(Kept::told)
    }

    /// The record of `order`, for the assembly to answer policy's sources from.
    pub fn record(&self, order: &Id) -> Option<OrderRecord> {
        let kept = self.kept(order)?;
        Some(OrderRecord {
            order: order.clone(),
            request: kept.request.request.clone(),
            parties: kept.request.parties.clone(),
            origin_reserve: kept.request.origin_reserve.clone(),
        })
    }

    fn kept(&self, order: &Id) -> Option<&Kept> {
        self.requests.get(self.orders.get(order.as_str())?)
    }

    fn kept_mut(&mut self, order: &Id) -> Result<&mut Kept, IntakeError> {
        let request = self
            .orders
            .get(order.as_str())
            .ok_or(IntakeError::NoSuchOrder)?;
        self.requests
            .get_mut(request)
            .ok_or(IntakeError::NoSuchOrder)
    }

    /// Takes `request` (see the module's docs, step 1).
    pub fn open(
        &mut self,
        desk: &mut Desk<'_, '_>,
        request: OrderRequest,
    ) -> Result<Told, IntakeError> {
        if let Some(kept) = self.requests.get(request.request.as_str()) {
            return if kept.asked == request {
                Ok(kept.told())
            } else {
                Err(IntakeError::Conflicting {
                    order: kept.order.clone(),
                })
            };
        }
        let asked = request.clone();
        let Some(request) = well_formed(request) else {
            refuse(
                desk,
                &asked.request,
                None,
                OrderRefusalClass::MalformedRequest,
            )?;
            let kept = Kept {
                request: asked.clone(),
                asked,
                order: None,
                status: Status::CantProceed {
                    class: OrderRefusalClass::MalformedRequest,
                },
                history: History::new(),
                owner: None,
                changed_owner: None,
                offered: None,
                accepting: None,
            };
            let told = kept.told();
            self.requests
                .insert(kept.request.request.as_str().to_owned(), kept);
            return Ok(told);
        };
        let order = desk.parts.fresh.id();
        if self.orders.contains_key(order.as_str()) || self.requests.contains_key(order.as_str()) {
            return Err(IntakeError::Fresh);
        }
        let mut kept = Kept {
            asked,
            request,
            order: Some(order.clone()),
            status: Status::OutcomeToCome,
            history: History::new(),
            owner: None,
            changed_owner: None,
            offered: None,
            accepting: None,
        };
        let resolution = desk.destinations.resolve(&kept.request.destination);
        let outcome = match &resolution {
            Resolution::Owner(owner) => {
                kept.owner = Some(owner.clone());
                DestinationOutcome::Resolved
            }
            Resolution::NotYet => DestinationOutcome::NotYet,
            Resolution::Refused => DestinationOutcome::Refused,
        };
        check_destination(desk, &order, &kept.request.destination, outcome)?;
        if outcome == DestinationOutcome::Refused {
            refuse(
                desk,
                &kept.request.request,
                Some(&order),
                OrderRefusalClass::DestinationRefused,
            )?;
            kept.status = Status::CantProceed {
                class: OrderRefusalClass::DestinationRefused,
            };
        }
        let told = kept.told();
        self.orders.insert(
            order.as_str().to_owned(),
            kept.request.request.as_str().to_owned(),
        );
        self.requests
            .insert(kept.request.request.as_str().to_owned(), kept);
        Ok(told)
    }

    /// Routes `order` with router `R` and shows its quote, or records why it can't proceed (step 2).
    pub fn quote<R: Router>(
        &mut self,
        desk: &mut Desk<'_, '_>,
        order: &Id,
    ) -> Result<Told, IntakeError> {
        let kept = self.kept_mut(order)?;
        if kept.accepting.is_some() && !matches!(kept.status, Status::Accepted { .. }) {
            // An acceptance is under way on the quote shown: it is carried on by accepting again, never re-quoted.
            return Err(IntakeError::NotNow);
        }
        match kept.status {
            Status::Accepted { .. } => return Ok(kept.told()),
            Status::CantProceed { class } if class != OrderRefusalClass::NoRouteRightNow => {
                return Err(IntakeError::NotNow);
            }
            _ => {}
        }
        let routing_order = Order {
            order: order.clone(),
            caller: kept.request.caller.clone(),
            origin_reserve: kept.request.origin_reserve.clone(),
            presence: Presence::User {
                session: kept.request.session.clone(),
            },
            inputs: kept.request.inputs.clone(),
            needs: kept.request.needs.clone(),
        };
        let routing = Routing {
            request: &kept.request.route,
            bounds: desk.bounds,
            prices: desk.prices,
            order: &routing_order,
            core: desk.core,
            operator: desk.operator,
        };
        let routed = rounds::route::<R>(desk.parts, routing, &mut kept.history)?;
        kept.offered = None;
        match &routed.offer {
            Offer::NotOffered(why) => {
                let class = class::not_offered(&routed, why);
                refuse(desk, &kept.request.request, Some(order), class)?;
                kept.status = Status::CantProceed { class };
            }
            Offer::Offered {
                candidate,
                reserve_yes,
                session,
            } => {
                let chosen = routed.choices.last().ok_or(IntakeError::Unofferable)?;
                let candidate = chosen
                    .record
                    .candidates()
                    .iter()
                    .find(|c| &c.candidate_id == candidate)
                    .cloned()
                    .ok_or(IntakeError::Unofferable)?;
                // A part Core can't run is refused before it is shown (`REQ-ORD-16`).
                for part in candidate.parts.iter() {
                    if let Err(refused) = check_part(part) {
                        refuse(desk, &kept.request.request, Some(order), refused.class)?;
                        kept.status = Status::CantProceed {
                            class: refused.class,
                        };
                        return Ok(kept.told());
                    }
                }
                // Each part withdraws at the origin reserve, each on its yes; the acceptance cites the first
                // part's (a contract gap: core-internal's acceptance names one yes).
                let reserve_yes = reserve_yes
                    .first()
                    .cloned()
                    .ok_or(IntakeError::Unofferable)?;
                let session = session.clone().ok_or(IntakeError::Unofferable)?;
                let quote = Quote {
                    order: order.clone(),
                    decision_record: chosen.record.record_id().clone(),
                    candidate: candidate.candidate_id.clone(),
                    from: kept.request.route.from.clone(),
                    to: kept.request.route.to.clone(),
                    amount: kept.request.route.amount.clone(),
                    delivers: delivered(&candidate).ok_or(IntakeError::Unofferable)?,
                    shown_as: ShownAs::Quote,
                };
                let digest = digest_of(&quote).ok_or(IntakeError::Unofferable)?;
                record::append(
                    &mut *desk.parts.log,
                    desk.parts.clock,
                    desk.core,
                    What::Shown {
                        order: order.clone(),
                        shown: digest.clone(),
                        shown_as: ShownAs::Quote,
                    },
                )?;
                kept.status = Status::Quoted {
                    quote: Box::new(quote.clone()),
                    digest: digest.clone(),
                };
                kept.offered = Some(Offered {
                    quote,
                    digest,
                    candidate,
                    reserve_yes,
                    session,
                });
            }
        }
        Ok(kept.told())
    }

    /// `by` accepts the quote of `order` shown with digest `quote` (step 3). Accepting again what was accepted
    /// records nothing more and is told the same.
    pub fn accept(
        &mut self,
        desk: &mut Desk<'_, '_>,
        order: &Id,
        by: &Actor,
        quote: &Digest,
    ) -> Result<Told, IntakeError> {
        let kept = self.kept_mut(order)?;
        if !is_requester(&kept.request, by) {
            return Err(IntakeError::NotTheRequester);
        }
        if let Status::Accepted { .. } = kept.status {
            return Ok(kept.told());
        }
        match kept.status {
            Status::Quoted { .. }
            | Status::Waiting {
                wait: Wait::DestinationNotYetResolved | Wait::PolicyNotKnown,
            } => {}
            _ => return Err(IntakeError::NotNow),
        }
        let offered = kept.offered.clone().ok_or(IntakeError::NotNow)?;
        if &offered.digest != quote {
            return Err(IntakeError::NotTheQuoteShown);
        }
        let request = kept.request.clone();

        // An acceptance whose append, or whose start's, failed is carried on from where it stopped, never asked of
        // policy again: the authorisation permitted, the acceptance and the start each made once (`REQ-ORD-19`).
        if kept.accepting.is_none() {
            // The destination, resolved when the route is accepted (`REQ-ORD-24`).
            let resolution = desk.destinations.resolve(&request.destination);
            let outcome = match &resolution {
                Resolution::Owner(owner) => match &kept.owner {
                    Some(first) if first != owner => {
                        kept.changed_owner = Some(owner.clone());
                        DestinationOutcome::OwnerChanged
                    }
                    _ => {
                        kept.owner = Some(owner.clone());
                        DestinationOutcome::Resolved
                    }
                },
                Resolution::NotYet => DestinationOutcome::NotYet,
                Resolution::Refused => DestinationOutcome::Refused,
            };
            check_destination(desk, order, &request.destination, outcome)?;
            match outcome {
                DestinationOutcome::Resolved => {}
                DestinationOutcome::NotYet => {
                    let class = OrderRefusalClass::DestinationNotYetResolved;
                    refuse(desk, &request.request, Some(order), class)?;
                    kept.status = Status::Waiting {
                        wait: Wait::DestinationNotYetResolved,
                    };
                    return Ok(kept.told());
                }
                DestinationOutcome::OwnerChanged => {
                    kept.status = Status::Waiting {
                        wait: Wait::OwnerChanged,
                    };
                    return Ok(kept.told());
                }
                DestinationOutcome::Refused => {
                    let class = OrderRefusalClass::DestinationRefused;
                    refuse(desk, &request.request, Some(order), class)?;
                    kept.status = Status::CantProceed { class };
                    kept.offered = None;
                    return Ok(kept.told());
                }
            }

            // The authorisation, naming the destination with the account the requester gave (`REQ-ORD-35`) and the
            // actor that accepted (`REQ-AUZ-9`).
            let authorisation = Authorisation {
                authorisation: desk.parts.fresh.id(),
                order: order.clone(),
                origin_reserve: request.origin_reserve.clone(),
                destination: request.destination.clone(),
                to_asset: request.route.to.asset.clone(),
                floor: request
                    .route
                    .floor
                    .as_ref()
                    .and_then(|f| PositiveAmount::new(f.as_str())),
                given_by: by.clone(),
                at: record::now(desk.parts.clock)?,
            };
            let decision = desk.authorisations.authorise(&authorisation)?;
            if decision.permit().is_none() {
                match decision.outcome {
                    Answer::Refuse { class } => {
                        let class = class::refused(class);
                        refuse(desk, &request.request, Some(order), class)?;
                        kept.status = Status::CantProceed { class };
                        kept.offered = None;
                    }
                    _ => {
                        kept.status = Status::Waiting {
                            wait: Wait::PolicyNotKnown,
                        };
                    }
                }
                return Ok(kept.told());
            }
            kept.accepting = Some(Accepting {
                authorisation: authorisation.authorisation,
                acceptance: desk.parts.fresh.id(),
                recorded: false,
                input: None,
            });
        }
        let mut accepting = kept.accepting.clone().ok_or(IntakeError::NotNow)?;

        // The acceptance, then the order's start, each once; the order is accepted only once it has started.
        if !accepting.recorded {
            record::append(
                &mut *desk.parts.log,
                desk.parts.clock,
                by,
                What::Acceptance {
                    acceptance: accepting.acceptance.clone(),
                    order: order.clone(),
                    request: request.request.clone(),
                    quote: offered.digest.clone(),
                    authorisation: accepting.authorisation.clone(),
                    basis: AcceptanceBasis::Given,
                    parties: request.parties.clone(),
                    origin_reserve: request.origin_reserve.clone(),
                    reserve_yes: Some(offered.reserve_yes.clone()),
                    session: Some(offered.session.clone()),
                },
            )?;
            accepting.recorded = true;
            kept.accepting = Some(accepting.clone());
        }
        let input = match accepting.input.clone() {
            Some(input) => input,
            None => {
                let input = route_chosen(desk, order, by, &offered)?;
                accepting.input = Some(input.clone());
                kept.accepting = Some(accepting.clone());
                input
            }
        };
        record::append(
            &mut *desk.parts.log,
            desk.parts.clock,
            desk.core,
            What::Input { input },
        )?;
        kept.status = Status::Accepted {
            acceptance: accepting.acceptance,
        };
        Ok(kept.told())
    }

    /// `by`, the order's requester, confirms its destination's new owner; the order's quote is shown again, to
    /// accept anew (`REQ-ORD-24`).
    pub fn confirm_owner(
        &mut self,
        order: &Id,
        by: &Actor,
        owner: &Id,
    ) -> Result<Told, IntakeError> {
        let kept = self.kept_mut(order)?;
        if !is_requester(&kept.request, by) {
            return Err(IntakeError::NotTheRequester);
        }
        let waiting = matches!(
            kept.status,
            Status::Waiting {
                wait: Wait::OwnerChanged
            }
        );
        if !waiting || kept.changed_owner.as_ref() != Some(owner) {
            return Err(IntakeError::NotNow);
        }
        let offered = kept.offered.clone().ok_or(IntakeError::NotNow)?;
        kept.owner = kept.changed_owner.take();
        kept.status = Status::Quoted {
            quote: Box::new(offered.quote),
            digest: offered.digest,
        };
        Ok(kept.told())
    }
}

/// Whether `by` acts for the order's requester, in the customer's role, a person or an agent alike (`REQ-OPS-5`).
fn is_requester(request: &OrderRequest, by: &Actor) -> bool {
    by.role == Role::Customer && by.party == request.parties.requester
}

/// The request with the destination's account set as the deposit's input, or `None` where it breaks the
/// deployment's rules for one: a caller not the requester in the customer's role, a route not ending at the
/// destination's node, a deposit input naming another account, or a settlement account held at another reserve
/// than the destination's.
fn well_formed(mut request: OrderRequest) -> Option<OrderRequest> {
    if !is_requester(&request, &request.caller) {
        return None;
    }
    if request.route.to.node != NodeRef::Outer(request.destination.node.clone()) {
        return None;
    }
    if let Some(at) = &request.parties.settlement_account_at
        && at != &request.destination.reserve
    {
        return None;
    }
    let account = request.destination.account.as_str().to_owned();
    match request.inputs.get(DESTINATION_INPUT) {
        Some(given) if given != &account => return None,
        _ => {}
    }
    request.inputs.insert(DESTINATION_INPUT.to_owned(), account);
    Some(request)
}

/// Records an `order_refusal` of `class`, naming the order where one exists.
fn refuse(
    desk: &mut Desk<'_, '_>,
    request: &Id,
    order: Option<&Id>,
    class: OrderRefusalClass,
) -> Result<Receipt, AppendFailure> {
    record::append(
        &mut *desk.parts.log,
        desk.parts.clock,
        desk.core,
        What::OrderRefusal {
            request: request.clone(),
            order: order.cloned(),
            class,
        },
    )
}

/// Records a `destination_check`.
fn check_destination(
    desk: &mut Desk<'_, '_>,
    order: &Id,
    destination: &Destination,
    outcome: DestinationOutcome,
) -> Result<Receipt, AppendFailure> {
    record::append(
        &mut *desk.parts.log,
        desk.parts.clock,
        desk.core,
        What::DestinationCheck {
            order: order.clone(),
            destination: destination.clone(),
            outcome,
        },
    )
}

/// What a candidate delivers at the destination: the sum of its parts' last operations' amounts out.
fn delivered(candidate: &Candidate) -> Option<Amount> {
    candidate
        .parts
        .iter()
        .try_fold(Amount::zero(), |sum, part| {
            sum.checked_add(&part.operations.last().amount_out)
        })
}

/// The digest of what was shown: SHA-256 of its RFC 8785 canonical JSON, the recorded decision's own rule.
fn digest_of(quote: &Quote) -> Option<Digest> {
    let canonical = to_canonical(quote).ok()?;
    let value = rutent_prov::module::wire::sha256(&canonical);
    serde_json::from_value(serde_json::json!({ "algorithm": "sha-256", "value": value })).ok()
}

/// The `route_chosen` input that opens the order: the chosen candidate's parts as the state machine plans them,
/// each part and segment under a fresh identifier, citing the choice round's decision record, stated by the
/// customer who accepted.
fn route_chosen(
    desk: &mut Desk<'_, '_>,
    order: &Id,
    by: &Actor,
    offered: &Offered,
) -> Result<RecordedInput, IntakeError> {
    let fresh = &mut *desk.parts.fresh;
    let mut stm_id = || stm::Id::new(fresh.id().as_str()).ok_or(IntakeError::Fresh);
    let mut parts = Vec::new();
    for part in offered.candidate.parts.iter() {
        let part_id = stm_id()?;
        let mut segments = Vec::new();
        for operation in part.operations.iter() {
            segments.push(SegmentPlan {
                segment_id: stm_id()?,
                provider: stm::Id::new(operation.provider.as_str())
                    .ok_or(IntakeError::Unofferable)?,
                kind: operation.kind,
                from: operation.from.clone(),
                to: operation.to.clone(),
            });
        }
        parts.push(PartPlan { part_id, segments });
    }
    let record_id = RecordId::new(desk.parts.fresh.id().as_str()).ok_or(IntakeError::Fresh)?;
    let recorded_at = stm::Timestamp::new(record::now(desk.parts.clock)?.as_str())
        .ok_or(IntakeError::Unofferable)?;
    let input = RecordedInput {
        record_id,
        recorded_at,
        time_source: stm::Id::new("core_clock").ok_or(IntakeError::Unofferable)?,
        source: Source {
            kind: SourceKind::Customer,
            id: stm::Id::new(by.acts.as_str()).ok_or(IntakeError::Unofferable)?,
        },
        standing: Standing::Stated,
        subject: Subject {
            order: stm::Id::new(order.as_str()).ok_or(IntakeError::Unofferable)?,
            part: None,
            segment: None,
        },
        observed: Observed::RouteChosen {
            decision_record: stm::Id::new(offered.quote.decision_record.as_str())
                .ok_or(IntakeError::Unofferable)?,
            parts,
            handed_on_from: None,
        },
    };
    input.check().map_err(|_| IntakeError::Unofferable)?;
    Ok(input)
}
