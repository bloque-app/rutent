//! Order intake and acceptance: requester, beneficiary and payer recorded apart, one order per request identifier, a
//! stable class of cause, the quote shown and the acceptance recorded, the destination resolved at acceptance.
//!
//! Declared by `T-020-01`; `T-020-03` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! One [`Intake`] takes an order from its request to its start, in three calls, each on the one contract humans and
//! agents share (`REQ-OPS-5`): the caller is an [`Actor`], whose `kind` says whether a person or an agent acts, and
//! nothing else differs. Only the requester's own actors are served: an actor in the customer's role that the
//! register of actors ([`Actors`], policy's `ActsFor`) says acts for the party it states. To anyone else an order
//! doesn't exist ([`IntakeError::NoSuchOrder`]).
//!
//! 1. **[`Intake::open`]** takes an [`OrderRequest`] under the caller's request identifier, which is the
//!    requester's own: one party's identifiers say nothing of another's. A request that breaks the deployment's
//!    rules, or whose session policy's register ([`SessionRegister`]) doesn't hold open for the requester, is
//!    refused with nothing created: an `order_refusal` naming no order, and the caller is told [`Told::Refused`].
//!    Any other opens one order, told [`Told::Exists`] with its outcome to come. The same request again under the
//!    same identifier, from any actor of the requester's, opens nothing and is told the same (`REQ-OPS-5`);
//!    another request under it is refused as [`IntakeError::Conflicting`], with nothing recorded (`REQ-ORD-19`,
//!    `INV-8`). Its destination is resolved, and the owner it resolves to kept, so a later change of owner shows.
//!    The order is kept before anything is appended, so an append that failed is carried on, under the same
//!    order, by the same request again.
//! 2. **[`Intake::quote`]** runs one routing ([`rounds::route`]). Every question to a provider, the reserve's
//!    request for its yes among them, is the rounds': asked under a fresh transaction for the order, signed with the
//!    operator's key and appended about that order and transaction before it is sent, its answer appended with
//!    that transaction in its `about` (`REQ-ORD-32`, `REQ-ORD-38`). An offered route is shown as a [`Quote`] and
//!    recorded as a `shown` entry, labelled a quote, by its digest. A routing that offers nothing is recorded as an
//!    `order_refusal` of its class (`REQ-ORD-18`).
//! 3. **[`Intake::accept`]** takes the customer's or its agent's acceptance of the quote shown, by its digest. The
//!    destination is resolved again: "not yet" waits, "no" ends the order, and a change of owner, refused of its class,
//!    waits for the customer, whose next acceptance names that owner to confirm it (`REQ-ORD-24`). The order's
//!    authorisation, naming its destination with the account the requester gave (`REQ-ORD-35`, `REQ-AUZ-3`) and the
//!    actor that accepted (`REQ-AUZ-9`), goes to policy. A refusal about the order itself ends it; one about the caller
//!    leaves it as it stood. Only on policy's permit is the `acceptance` recorded, its `who` the actor that accepted,
//!    an agent as an agent (`REQ-ORD-10`), naming its parties apart (`REQ-ORD-17`), the quote shown, the origin
//!    reserve's yes for each part of the route and the user's session (`REQ-ORD-32`). Only then is the order started:
//!    the `route_chosen` input, recorded once (`REQ-ORD-34`, `REQ-ORD-19`). Both entries are built once and appended as
//!    built. Where the log couldn't be written now, the next acceptance by the actor policy permitted, and by no other,
//!    carries them on, without asking policy or resolving the destination again; where the log refuses one, the order
//!    ends, of a class. The order is told accepted only once it has started.
//!
//! **What the caller is told holds no provider's text** (`REQ-ORD-18`). [`Told`] carries identifiers, a class from
//! a closed list and the quote Core built from the router's record; a provider's words stay in its stated entry.
//!
//! **What intake keeps** (every request identifier and its answer) is in memory; keeping it across a restart is the
//! deployment's (`F-030`), as with the rounds' `Issued`.

mod class;
mod record;

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Authorisations, Log};
use rutent_contracts::core_internal::{
    AcceptanceBasis, Actor, ActorKind, Answer, Authorisation, Destination, DestinationOutcome,
    Entry, OrderRefusalClass, PartYes, Parties, Receipt, Role, ShownAs, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{NodeRef, to_canonical};
use rutent_contracts::router_api::{
    Amount, Candidate, Endpoint, Id, PolicyBounds, PositiveAmount, Request, Router, Timestamp,
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

/// The register of actors policy reads (`F-021`'s `ActsFor`): whether an actor, as external authentication
/// established it, acts for the party it states, as the kind of actor it states. The assembly answers it from the
/// same register policy does, which binds the kind too (`T-021-05`), so an agent's login stating itself a person is
/// no one's here either.
pub trait Actors {
    /// Whether `acts` acts for `party` as `kind`; no answer is no.
    fn acts_for(&mut self, acts: &Id, party: &Id, kind: ActorKind) -> bool;
}

/// A session as policy's register holds it (`REQ-CAT-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldSession {
    /// The party that holds the account it speaks for.
    pub holder: Id,
    /// When it ends, by Core's clock.
    pub expires_at: Timestamp,
}

/// Policy's session register, as intake reads it before the reserve is asked anything in a session.
pub trait SessionRegister {
    /// The session `session`, if policy opened it.
    fn held(&mut self, session: &Id) -> Option<HeldSession>;
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

/// Why a call answered nothing to tell. Nothing after the failing step was recorded, unless the error says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeError {
    /// The requester made another request under this identifier: it opens nothing.
    Conflicting {
        /// The order the identifier opened, if it opened one.
        order: Option<Id>,
    },
    /// No order of that id that the caller may see: none exists, or the caller isn't its requester, a person or
    /// an agent acting for its party in the customer's role.
    NoSuchOrder,
    /// The caller isn't acting for the party it states, in the customer's role: it makes no request.
    NotTheRequester,
    /// An acceptance under way is carried on only by the actor whose authorisation policy permitted.
    OtherActor,
    /// Policy refused the caller, of this class; the order stands as it was.
    Refused {
        /// The class.
        class: OrderRefusalClass,
    },
    /// The acceptance names no quote shown for the order, or one since replaced.
    NotTheQuoteShown,
    /// The order isn't at the step the call needs: no quote to accept, no owner change to confirm or one left
    /// unconfirmed, or nothing to quote once it is accepted, ended or being accepted.
    NotNow,
    /// The routing stopped.
    Routing(RoutingError),
    /// The log, or policy, refused or failed. Where the log couldn't be written now
    /// ([`AppendFailure::Unavailable`]), the same call again carries on from where it stopped.
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
/// register, policy's authorisations, the register of actors and policy's session register, and what the router
/// runs under. The assembly builds it.
pub struct Desk<'a, 'p> {
    /// What the rounds reach.
    pub parts: &'a mut Parts<'p>,
    /// Where destinations resolve.
    pub destinations: &'a mut dyn Destinations,
    /// Policy's authorisations.
    pub authorisations: &'a mut dyn Authorisations,
    /// The register of actors.
    pub actors: &'a mut dyn Actors,
    /// Policy's session register.
    pub sessions: &'a mut dyn SessionRegister,
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
    /// Each part's yes, in part order.
    reserve_yes: Vec<Receipt>,
    session: Id,
}

/// An acceptance under way, once policy permitted its authorisation: who policy permitted, and the entries built,
/// each appended as built, so a retry after an append that failed, or whose answer was lost, makes nothing twice.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Accepting {
    /// The actor who accepted, whose authorisation policy is asked: the only one who carries it on.
    by: Actor,
    /// The acceptance's id.
    acceptance: Id,
    /// The `route_chosen` input the start records.
    input: RecordedInput,
    /// The `acceptance` entry and then the `route_chosen` one, those not yet appended.
    pending: Vec<Entry>,
    /// The authorisation the acceptance gives, until policy permits it: asked once the acceptance is in the log, so
    /// policy reads the order's parties from it (the payer among them), and before the start is appended.
    authorisation: Option<Authorisation>,
}

/// One request identifier's record, under its requester.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Kept {
    /// The request as the caller made it: a repeat is the same request only if it equals this, but for who acts.
    asked: OrderRequest,
    /// The request as intake runs it, the destination's account set as the deposit's input.
    request: OrderRequest,
    /// Absent where the request was refused with nothing created.
    order: Option<Id>,
    status: Status,
    history: History,
    /// The entries the request's opening built and hasn't appended yet: a retry appends these, as built.
    opening: Vec<Entry>,
    /// The owner the destination resolved to, as the customer last accepted it.
    owner: Option<Id>,
    /// The owner it resolved to since, waiting for the customer's confirmation.
    changed_owner: Option<Id>,
    offered: Option<Offered>,
    accepting: Option<Accepting>,
    /// The order ended after policy permitted its authorisation: nothing is quoted or accepted again.
    ended: bool,
    /// The `route_chosen` input the log took: the order's start, once it has one.
    started: Option<RecordedInput>,
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

/// A request identifier, under the party that requested it: one party's identifiers say nothing of another's.
type Key = (String, String);

fn key(party: &Id, request: &Id) -> Key {
    (party.as_str().to_owned(), request.as_str().to_owned())
}

/// Core's intake: every request identifier each requester gave, and the order each opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Intake {
    requests: BTreeMap<Key, Kept>,
    orders: BTreeMap<String, Key>,
}

impl Intake {
    /// No request taken yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// What `caller` is told now about its request `request`: nothing unless the caller acts for the party it
    /// states, in the customer's role, and that party used the identifier.
    pub fn told(&self, actors: &mut dyn Actors, caller: &Actor, request: &Id) -> Option<Told> {
        if caller.role != Role::Customer
            || !actors.acts_for(&caller.acts, &caller.party, caller.kind)
        {
            return None;
        }
        let kept = self.requests.get(&key(&caller.party, request))?;
        kept.opening.is_empty().then(|| kept.told())
    }

    /// The record of `order`, for the assembly to answer policy's sources from.
    pub fn record(&self, order: &Id) -> Option<OrderRecord> {
        let kept = self.requests.get(self.orders.get(order.as_str())?)?;
        Some(OrderRecord {
            order: order.clone(),
            request: kept.request.request.clone(),
            parties: kept.request.parties.clone(),
            origin_reserve: kept.request.origin_reserve.clone(),
        })
    }

    /// The `route_chosen` input that started `order`, as the log took it: what `T-020-08`'s segments follow
    /// (`Segments::follow`), and the only start of an order they act on. An order whose acceptance the log took but
    /// whose start it refused has none: it ended half-accepted, and nothing acts on it.
    pub fn start(&self, order: &Id) -> Option<&RecordedInput> {
        self.requests
            .get(self.orders.get(order.as_str())?)?
            .started
            .as_ref()
    }

    /// The record of `order`, if `caller` speaks for its requester; to anyone else it doesn't exist.
    fn kept_for(
        &mut self,
        actors: &mut dyn Actors,
        caller: &Actor,
        order: &Id,
    ) -> Result<&mut Kept, IntakeError> {
        let key = self
            .orders
            .get(order.as_str())
            .ok_or(IntakeError::NoSuchOrder)?;
        let kept = self.requests.get_mut(key).ok_or(IntakeError::NoSuchOrder)?;
        if !speaks_for(actors, &kept.request, caller) {
            return Err(IntakeError::NoSuchOrder);
        }
        Ok(kept)
    }

    /// Takes `request` (see the module's docs, step 1).
    pub fn open(
        &mut self,
        desk: &mut Desk<'_, '_>,
        request: OrderRequest,
    ) -> Result<Told, IntakeError> {
        let caller = &request.caller;
        if caller.role != Role::Customer
            || !desk
                .actors
                .acts_for(&caller.acts, &caller.party, caller.kind)
        {
            return Err(IntakeError::NotTheRequester);
        }
        let key = key(&caller.party, &request.request);
        if let Some(kept) = self.requests.get_mut(&key) {
            if !same_request(&kept.asked, &request) {
                return Err(IntakeError::Conflicting {
                    order: kept.order.clone(),
                });
            }
            flush(&mut *desk.parts.log, &mut kept.opening)?;
            return Ok(kept.told());
        }
        let kept = opened(desk, request)?;
        if let Some(order) = &kept.order {
            // A fresh identifier is never one an order already has.
            if self.orders.contains_key(order.as_str()) {
                return Err(IntakeError::Fresh);
            }
            self.orders.insert(order.as_str().to_owned(), key.clone());
        }
        // Kept before anything is appended, so a retry carries on with the same order.
        let kept = self.requests.entry(key).or_insert(kept);
        flush(&mut *desk.parts.log, &mut kept.opening)?;
        Ok(kept.told())
    }

    /// Routes `order` with router `R` and shows its quote, or records why it can't proceed (step 2). Only the
    /// order's requester quotes it.
    pub fn quote<R: Router>(
        &mut self,
        desk: &mut Desk<'_, '_>,
        caller: &Actor,
        order: &Id,
    ) -> Result<Told, IntakeError> {
        let kept = self.kept_for(desk.actors, caller, order)?;
        if let Status::Accepted { .. } = kept.status {
            return Ok(kept.told());
        }
        if !kept.opening.is_empty() || kept.ended || kept.accepting.is_some() {
            // Not yet open, ended, or an acceptance under way on the quote shown, carried on by accepting again.
            return Err(IntakeError::NotNow);
        }
        if let Status::CantProceed { class } = kept.status
            && class != OrderRefusalClass::NoRouteRightNow
        {
            return Err(IntakeError::NotNow);
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
        let (candidate, reserve_yes, session) = match &routed.offer {
            Offer::NotOffered(why) => {
                let class = class::not_offered(&routed, why);
                refuse(desk, &kept.request.request, Some(order), class)?;
                kept.status = Status::CantProceed { class };
                return Ok(kept.told());
            }
            Offer::Offered {
                candidate,
                reserve_yes,
                session,
            } => (candidate, reserve_yes, session),
        };
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
        // The acceptance cites each part's yes, in part order, each its own receipt (`REQ-ORD-32`). Two parts the
        // rounds answered with one yes, an even split at one reserve, would cite one statement twice: not offered.
        let mut distinct = reserve_yes.clone();
        distinct.sort_by_key(|r| r.index.get());
        distinct.dedup_by_key(|r| r.index.get());
        if reserve_yes.len() != candidate.parts.len() || distinct.len() != reserve_yes.len() {
            let class = OrderRefusalClass::NoRouteRightNow;
            refuse(desk, &kept.request.request, Some(order), class)?;
            kept.status = Status::CantProceed { class };
            return Ok(kept.told());
        }
        let reserve_yes = reserve_yes.clone();
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
        Ok(kept.told())
    }

    /// `by` accepts the quote of `order` shown with digest `quote` (step 3). Where the order waits on its
    /// destination's new owner, `owner` is that owner, which the acceptance confirms; otherwise it is `None`.
    /// Accepting again what was accepted, or what ended, records nothing more and is told the same.
    pub fn accept(
        &mut self,
        desk: &mut Desk<'_, '_>,
        order: &Id,
        by: &Actor,
        quote: &Digest,
        owner: Option<&Id>,
    ) -> Result<Told, IntakeError> {
        let kept = self.kept_for(desk.actors, by, order)?;
        if matches!(kept.status, Status::Accepted { .. }) || kept.ended {
            return Ok(kept.told());
        }
        if let Some(accepting) = &kept.accepting {
            // Carried on from where it stopped, by the actor policy permitted alone; the destination isn't
            // resolved again, since policy permitted on it.
            if &accepting.by != by {
                return Err(IntakeError::OtherActor);
            }
            return carry_on(desk, kept);
        }
        if !matches!(kept.status, Status::Quoted { .. } | Status::Waiting { .. }) {
            return Err(IntakeError::NotNow);
        }
        let offered = kept.offered.clone().ok_or(IntakeError::NotNow)?;
        if &offered.digest != quote {
            return Err(IntakeError::NotTheQuoteShown);
        }
        // A change of owner is confirmed only by naming the owner it changed to.
        if kept.changed_owner.as_ref() != owner {
            return Err(IntakeError::NotNow);
        }
        let request = kept.request.clone();

        // The destination, resolved when the route is accepted (`REQ-ORD-24`), against the owner the customer
        // accepted: the first resolved, or the one it now confirms.
        let accepted_owner = kept.changed_owner.clone().or_else(|| kept.owner.clone());
        let resolution = desk.destinations.resolve(&request.destination);
        let (outcome, resolved) = match resolution {
            Resolution::Owner(now) => match &accepted_owner {
                Some(owner) if owner != &now => (DestinationOutcome::OwnerChanged, Some(now)),
                _ => (DestinationOutcome::Resolved, Some(now)),
            },
            Resolution::NotYet => (DestinationOutcome::NotYet, None),
            Resolution::Refused => (DestinationOutcome::Refused, None),
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
                // The order waits for the customer: refused, of its class, until the customer confirms the owner.
                let class = OrderRefusalClass::DestinationOwnerChanged;
                refuse(desk, &request.request, Some(order), class)?;
                kept.changed_owner = resolved;
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
                kept.changed_owner = None;
                return Ok(kept.told());
            }
        }

        // The acceptance, then the order's start, each built once, here, and appended as built: the acceptance first,
        // then the authorisation it gives is asked of policy, which reads the order's parties from it, the payer among
        // them (the coordinator's ruling on the payer gap), and only on policy's permit the start. Both are built and
        // checked before anything is appended, so an acceptance that fails its own shape records nothing. The acceptance names each part of the start's plan with its yes and its origin reserve, the
        // provider of its first move, and the authorisation it gives.
        let authorisation_id = desk.parts.fresh.id();
        let input = route_chosen(desk, order, by, &offered)?;
        let Observed::RouteChosen { parts, .. } = &input.observed else {
            return Err(IntakeError::Unofferable);
        };
        let yeses = parts
            .iter()
            .zip(&offered.reserve_yes)
            .map(|(part, yes)| {
                let first = part.segments.first().ok_or(IntakeError::Unofferable)?;
                Ok(PartYes {
                    part: Id::new(part.part_id.as_str()).ok_or(IntakeError::Unofferable)?,
                    origin_reserve: Id::new(first.provider.as_str())
                        .ok_or(IntakeError::Unofferable)?,
                    reserve_yes: yes.clone(),
                })
            })
            .collect::<Result<Vec<_>, IntakeError>>()?;
        let acceptance = desk.parts.fresh.id();
        let accepted = record::build(
            desk.parts.clock,
            by,
            What::Acceptance {
                acceptance: acceptance.clone(),
                order: order.clone(),
                request: request.request.clone(),
                quote: offered.digest.clone(),
                authorisation: authorisation_id.clone(),
                basis: AcceptanceBasis::Given,
                parties: request.parties.clone(),
                origin_reserve: request.origin_reserve.clone(),
                yeses,
                session: Some(offered.session.clone()),
            },
        )?;
        let start = record::build(
            desk.parts.clock,
            desk.core,
            What::Input {
                input: input.clone(),
            },
        )?;

        // The authorisation, naming the destination with the account the requester gave (`REQ-ORD-35`) and the
        // actor that accepted (`REQ-AUZ-9`), held by policy to the register of actors.
        let authorisation = Authorisation {
            authorisation: authorisation_id,
            order: order.clone(),
            origin_reserve: request.origin_reserve.clone(),
            // The payer's account at the origin reserve, as the order's inputs name it: where a stuck resolution may
            // return the value (`delivery-goes-to-the-parts-ends`). None where the order names none, and then no
            // delivery back to the origin is admitted.
            origin_account: request
                .inputs
                .get(crate::stuck::PAYER_ACCOUNT)
                .and_then(|account| Id::new(account.as_str())),
            destination: request.destination.clone(),
            to_asset: request.route.to.asset.clone(),
            floor: request
                .route
                .floor
                .as_ref()
                .and_then(|f| PositiveAmount::new(f.as_str())),
            parts: None,
            given_by: by.clone(),
            at: record::now(desk.parts.clock)?,
        };
        kept.owner = resolved;
        kept.changed_owner = None;
        kept.accepting = Some(Accepting {
            by: by.clone(),
            acceptance,
            input,
            pending: vec![accepted, start],
            authorisation: Some(authorisation),
        });
        carry_on(desk, kept)
    }
}

/// Appends an acceptance's entries not yet appended, as built. The order is told accepted only once its start is
/// appended. Where the log can't be written now, the next acceptance by the same actor carries on; where it refuses
/// an entry, no retry can succeed, so the order ends, of the class `no_route_right_now`.
fn carry_on(desk: &mut Desk<'_, '_>, kept: &mut Kept) -> Result<Told, IntakeError> {
    let accepting = kept.accepting.as_mut().ok_or(IntakeError::NotNow)?;
    if let Some(authorisation) = accepting.authorisation.clone() {
        // The acceptance first, alone, where it isn't in the log yet: policy reads the order's parties from it. The
        // start stays pending behind it until policy permits.
        if accepting.pending.len() > 1 {
            let mut acceptance: Vec<Entry> = accepting.pending.drain(..1).collect();
            let appended = flush(&mut *desk.parts.log, &mut acceptance);
            accepting.pending.splice(0..0, acceptance);
            match appended {
                Ok(()) => {}
                Err(AppendFailure::Refused(_)) => {
                    return ended(desk, kept, OrderRefusalClass::NoRouteRightNow);
                }
                Err(failure) => return Err(IntakeError::Log(failure)),
            }
        }
        let decision = desk.authorisations.authorise(&authorisation)?;
        if decision.permit().is_some() {
            accepting.authorisation = None;
        } else if matches!(decision.outcome, Answer::NotKnown { .. }) {
            // Not known: the order waits, and accepting again asks the same authorisation of policy again.
            kept.status = Status::Waiting {
                wait: Wait::PolicyNotKnown,
            };
            return Ok(kept.told());
        } else {
            // Refused once the acceptance is in the log, the order ends with its refusal recorded, of the class
            // policy's refusal gives its caller: no part starts and nothing moves.
            let class = match decision.outcome {
                Answer::Refuse { class } => class::refused(class),
                _ => OrderRefusalClass::RefusedByPolicy,
            };
            return ended(desk, kept, class);
        }
    }
    let accepting = kept.accepting.as_mut().ok_or(IntakeError::NotNow)?;
    match flush(&mut *desk.parts.log, &mut accepting.pending) {
        Ok(()) => {
            kept.status = Status::Accepted {
                acceptance: accepting.acceptance.clone(),
            };
            kept.started = Some(accepting.input.clone());
            Ok(kept.told())
        }
        Err(AppendFailure::Refused(_)) => ended(desk, kept, OrderRefusalClass::NoRouteRightNow),
        Err(failure) => Err(IntakeError::Log(failure)),
    }
}

/// Ends `kept`'s order, accepted or not, with its refusal of `class` recorded: nothing started, nothing moved.
fn ended(
    desk: &mut Desk<'_, '_>,
    kept: &mut Kept,
    class: OrderRefusalClass,
) -> Result<Told, IntakeError> {
    let order = kept.order.clone().ok_or(IntakeError::NotNow)?;
    refuse(desk, &kept.request.request, Some(&order), class)?;
    kept.status = Status::CantProceed { class };
    kept.ended = true;
    kept.offered = None;
    kept.accepting = None;
    Ok(kept.told())
}

/// Appends `pending`, in order, each as built, taking each off once the log took it.
fn flush(log: &mut dyn Log, pending: &mut Vec<Entry>) -> Result<(), AppendFailure> {
    while let Some(entry) = pending.first() {
        record::append_entry(log, entry)?;
        pending.remove(0);
    }
    Ok(())
}

/// A request's record before anything is appended: the entries its opening appends, and where it stands once they
/// are.
fn opened(desk: &mut Desk<'_, '_>, request: OrderRequest) -> Result<Kept, IntakeError> {
    let asked = request.clone();
    let mut kept = Kept {
        request: asked.clone(),
        asked,
        order: None,
        status: Status::OutcomeToCome,
        history: History::new(),
        opening: Vec::new(),
        owner: None,
        changed_owner: None,
        offered: None,
        accepting: None,
        ended: false,
        started: None,
    };
    let refused = |desk: &mut Desk<'_, '_>, kept: &mut Kept, class| {
        let what = refusal(&kept.asked.request, None, class);
        kept.opening = vec![record::build(desk.parts.clock, desk.core, what)?];
        kept.status = Status::CantProceed { class };
        Ok::<(), IntakeError>(())
    };
    let Some(request) = well_formed(request) else {
        refused(desk, &mut kept, OrderRefusalClass::MalformedRequest)?;
        return Ok(kept);
    };
    // The reserve is asked about the holder's funds in this session: it must be the requester's, and open.
    if !session_is_the_requesters(desk, &request)? {
        refused(desk, &mut kept, OrderRefusalClass::RefusedByPolicy)?;
        return Ok(kept);
    }
    let order = desk.parts.fresh.id();
    kept.request = request;
    kept.order = Some(order.clone());
    let outcome = match desk.destinations.resolve(&kept.request.destination) {
        Resolution::Owner(owner) => {
            kept.owner = Some(owner);
            DestinationOutcome::Resolved
        }
        Resolution::NotYet => DestinationOutcome::NotYet,
        Resolution::Refused => DestinationOutcome::Refused,
    };
    let check = What::DestinationCheck {
        order: order.clone(),
        destination: kept.request.destination.clone(),
        outcome,
    };
    kept.opening
        .push(record::build(desk.parts.clock, desk.core, check)?);
    if outcome == DestinationOutcome::Refused {
        let class = OrderRefusalClass::DestinationRefused;
        let what = refusal(&kept.request.request, Some(&order), class);
        kept.opening
            .push(record::build(desk.parts.clock, desk.core, what)?);
        kept.status = Status::CantProceed { class };
    }
    Ok(kept)
}

/// Whether the request's session is one policy opened for its requester, still open by Core's clock.
fn session_is_the_requesters(
    desk: &mut Desk<'_, '_>,
    request: &OrderRequest,
) -> Result<bool, IntakeError> {
    let now = record::now(desk.parts.clock)?;
    Ok(desk.sessions.held(&request.session).is_some_and(|held| {
        held.holder == request.parties.requester && held.expires_at.unix_nanos() > now.unix_nanos()
    }))
}

/// Whether `again` repeats `first`: the same request in every field but who acts for the party, which the key
/// already holds, so any actor of the requester's, a person or an agent, repeating it is told the same
/// (`REQ-OPS-5`, `REQ-ORD-19`).
fn same_request(first: &OrderRequest, again: &OrderRequest) -> bool {
    let caller = Actor {
        acts: first.caller.acts.clone(),
        kind: first.caller.kind,
        ..again.caller.clone()
    };
    first
        == &OrderRequest {
            caller,
            ..again.clone()
        }
}

/// Whether `caller` speaks for the request's requester: an actor for its party, in the customer's role, a person or
/// an agent alike (`REQ-OPS-5`), that the register of actors says acts for that party.
fn speaks_for(actors: &mut dyn Actors, request: &OrderRequest, caller: &Actor) -> bool {
    is_requester(request, caller) && actors.acts_for(&caller.acts, &caller.party, caller.kind)
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
    // Only Core recalculates: a request's route never follows another routing (`REQ-ORD-2`, `T-020-10`).
    if request.route.follows.is_some() {
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

/// An `order_refusal` of `class`, naming the order where one exists.
fn refusal(request: &Id, order: Option<&Id>, class: OrderRefusalClass) -> What {
    What::OrderRefusal {
        request: request.clone(),
        order: order.cloned(),
        class,
    }
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
        refusal(request, order, class),
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
