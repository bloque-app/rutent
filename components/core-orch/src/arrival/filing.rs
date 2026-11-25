//! An order filed on an automatic deposit (`REQ-ORD-13`, `REQ-ORD-15`), from the arrival held against no order to the
//! order's start.
//!
//! **Before an order exists**, nothing is opened, held, asked or routed, and a filing that stops here holds nothing:
//! 1. the filing is for what arrived (from its outer node, in its asset, at its amount, with no `follows`), for the
//!    account's holder as requester and beneficiary, naming no other account for the deposit; the entry Core keeps of
//!    the arrival states what the reserve's kept notice states (holder and destination); and its session is one
//!    policy opened for that holder, still open. Otherwise an `order_refusal` naming no order;
//! 2. the destination is where the reserve's arrival notice names it, from the customer's standing instruction the
//!    reserve holds (`T-020-50`; the owner's answer `automatic-deposit-destination`), or the holder's own account where
//!    it names none; the filing is for that one and no other, and a destination set for another party than the
//!    holder is `destination_refused`. It is resolved (`REQ-ORD-24`) to an owner, and that owner is the beneficiary:
//!    another owner, or a refusal, is `destination_refused`; not yet resolved waits, with nothing recorded;
//! 3. the order is drawn under a fresh identifier, its record handed to the assembly ([`super::FiledOrders`]) so
//!    policy's sources can answer for it, and the authorisation, given by the filer, goes to policy, which holds a
//!    reserve to its declaration and the operator to its permission (`F-021`). A refusal about the filer is answered
//!    as [`ArrivalError::Refused`]; one about the order (`outside_authorisation`) is an `order_refusal` naming no
//!    order; policy's not known waits.
//!
//! **Once policy permits**, the filing holds the deposit, and each step is kept, so a filing that stopped is carried
//! on by filing it again:
//! 1. **Opened.** The `destination_check` that resolved.
//! 2. **The deposit held against it.** Core asks the reserve, under a fresh transaction for the order, whether the
//!    account's funds cover what arrived (`confirm.rs`), and records the deposit's `arrival` naming the order,
//!    `matches`, on the reserve's yes: the arrival the acceptance rests on (`automatic-deposit-rests-on-its-arrival`).
//!    A silent reserve, or a check that can't go yet, waits; a no ends the order.
//! 3. **Routed.** One routing through `rounds::route`, with no user present (`Presence::AutomaticDeposit`), in the
//!    session opened for the deposit's account: every question the rounds' own, asked under a fresh transaction for
//!    the order, and no reserve's yes asked (`REQ-ORD-32`). The quote is recorded as `shown`, labelled a quote, and
//!    its digest is what the acceptance names.
//! 4. **Accepted and started.** The `acceptance`, `basis: automatic_deposit`, its `who` the filer, with no yes and no
//!    session, naming the authorisation policy permitted, then the `route_chosen` input, the filer's statement. Both
//!    are built once and appended as built.
//!
//! **A route with no firm rate** (`REQ-QTE-10`) starts only on its customer's acceptance, which an automatic deposit
//! isn't. Its quote is shown, its slow segments named, as step 3 records any, and the order waits for its customer
//! instead of reaching step 4 ([`FiledWait::CustomerAcceptance`]; the owner's ruling `deposit-no-firm-rate`). Nothing
//! is accepted or authorised to move while it waits, and the funds stay in the holder's account at the reserve. The
//! customer's acceptance of the quote shown ([`super::Arrivals::accept`]), given in the filing's session, asks policy
//! for the customer's own authorisation and records the `acceptance` on `basis: given_on_deposit`, its `who` the
//! customer, then the start, stated by the customer; past the quote's validity it is asked again, and the customer
//! accepts the one shown anew. The wait outlives the session the deposit was filed in (`T-020-51`): the customer accepts
//! in any session policy opened for it that is still open, and the wait has a lifetime of its own, the operator's
//! deposit-wait setting counted from the first quote shown, which no requote renews and no quote outlasts. Past it the
//! wait ends, recorded as `wait_ended` and the order's refusal, and the deposit is free again: nothing moved, so the
//! funds are where they were, and the reserve's next filing may file it. Core's schedule ends it
//! ([`super::Arrivals::end_expired`]), or the next call that reads it does.
//!
//! An order that ends at routing is refused of its class, naming it. Once the deposit was held against its order
//! (and `one-order-per-deposit` lets one deposit open only one order), or the reserve said no, it is `NotFileable`,
//! and only a new arrival notice is a new arrival; an end before that leaves it free to be filed on again. Nothing has
//! moved, either way, so the customer's funds stay in the holder's own account at the reserve.
//! Arrivals takes no exposures, so ending holds nothing more to release.

use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::core_internal::{
    AcceptanceBasis, Answer, ArrivalMatch, Authorisation, Awaiting, Count, DestinationOutcome,
    Entry, OrderRefusalClass, Receipt, RefusalClass, Role, SlowSegment, WaitEnd, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{NodeRef, to_canonical};
use rutent_contracts::router_api::{
    Candidate, DecisionRecord, DropReason, Endpoint, Id, PositiveAmount, Router,
};
use rutent_contracts::state_machine::{
    self as stm, Observed, PartPlan, RecordId, RecordedInput, SegmentPlan, Source, SourceKind,
    Standing, Subject,
};

use super::confirm::{Asking, Confirmation, Confirming};
use super::{ArrivalError, Arrivals, Filed, FiledOrders, FiledWait, Filing, Recorded, journal};
use crate::intake::{DESTINATION_INPUT, Desk, OrderRecord, Quote, Resolution};
use crate::rounds::{self, History, NotOffered, Offer, Order, Presence, Routed, Routing};
use crate::snapshot::check_part;
use rutent_contracts::core_internal::{
    Actor, FILING_INPUTS_SHAPE, FilingInputs, FilingNeed, Parties,
};
use rutent_contracts::router_api::Objective;
use rutent_prov::catalogue::{CheckRefusal, Eligibility};
use std::collections::BTreeMap;

/// Where a permitted filing stands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// The deposit is to be held against it.
    Hold,
    /// It is to be routed.
    Route,
    /// Its route has no firm rate: it waits for its customer to accept it (`REQ-QTE-10`).
    Customer,
    /// Its acceptance and start are to be recorded.
    Accept,
    /// It started, or ended.
    Done,
}

/// A filing as the operator's kept statement holds it, read back for a wait rebuilt from the log (`T-020-51`).
pub(super) struct StatedFiling {
    /// The request identifier the filing was made under.
    pub(super) request: Id,
    /// Its inputs.
    pub(super) inputs: BTreeMap<String, String>,
    /// Its objective.
    pub(super) objective: Objective,
    /// The judgements its route's tuples need.
    pub(super) needs: Vec<crate::rounds::Need>,
}

/// The route offered, kept until its acceptance is recorded.
#[derive(Debug, Clone)]
struct Offered {
    quote: Quote,
    digest: Digest,
    candidate: Candidate,
    /// When the figures the route stands on stop being usable, in nanoseconds since the Unix epoch, where any states a
    /// validity: the quote is asked again past it (`REQ-QTE-15`).
    expires: Option<i128>,
}

/// A quote an order waits on its customer to accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Awaited<'a> {
    /// The quote, its slow segments named.
    pub quote: &'a Quote,
    /// Its digest: what the acceptance names.
    pub digest: &'a Digest,
    /// When it stops standing, to the whole second below, where any figure it rests on states a validity.
    pub valid_until: Option<rutent_contracts::router_api::Timestamp>,
    /// Whether it stopped standing already: accepting it asks the route again, and the customer accepts the quote
    /// shown anew.
    pub expired: bool,
}

/// One permitted filing, kept from policy's permit: what it asked, the order it opened, the authorisation policy
/// permitted, the step it reached, and the entries built and not yet appended.
#[derive(Debug, Clone)]
pub(super) struct Run {
    /// The filing as asked: a repeat is the same filing only if it equals this, but for which login of the filer's
    /// party made it.
    pub(super) asked: Filing,
    /// The filing as Core runs it, the destination's account set as the deposit's input.
    filing: Filing,
    /// The arrival it files on.
    arrival: Recorded,
    pub(super) order: Id,
    /// The authorisation policy permitted, which the acceptance names: the filer's, or that of the login that carried it
    /// on since, each given by its own login (`REQ-AUZ-9`).
    authorisation: Authorisation,
    step: Step,
    /// Entries built and not yet appended, appended in order, each as built.
    pending: Vec<Entry>,
    confirming: Option<Confirming>,
    /// The deposit's `arrival` naming the order, built once.
    held: Option<(Entry, Option<Receipt>)>,
    /// The operator's statement of the filing's inputs and objective, built once and appended before the arrival that
    /// names it (`filed-orders-inputs-are-a-kept-statement`, `T-020-51`).
    held_inputs: Option<(Entry, Option<Receipt>)>,
    history: History,
    offered: Option<Offered>,
    /// The acceptance's id, the start, and the entries recording them not yet appended.
    accepting: Option<(Id, RecordedInput, Vec<Entry>)>,
    outcome: Option<Filed>,
    /// The customer whose acceptance of a route with no firm rate was taken, once policy permitted its authorisation:
    /// the acceptance is theirs, on the deposit (`given_on_deposit`).
    customer: Option<Actor>,
    /// Whether the route's quote was shown as one to accept: once asked of the customer, every quote of it is, a
    /// requoted one too, firm or not.
    asks_the_customer: bool,
    /// When the first quote it waits on was shown, in nanoseconds since the Unix epoch: its wait's lifetime counts from
    /// here and no requote renews it (`T-020-51`).
    waiting_since: Option<i128>,
    /// The session the customer accepted in, once policy permitted its authorisation: the acceptance cites it.
    customer_session: Option<Id>,
    /// Rebuilt from the log by a Core that reopened (`T-020-51`): `asked` and `filing` hold what the log and the
    /// operator's kept statement hold of the filing, and the quote shown before is the digest the log last shows, the
    /// quote itself gone with the Core. Until the filer states its filing again, which takes the same request.
    reopened: bool,
    /// Whether the filing's statement couldn't be read back, or isn't the filing's: the wait is held and never accepted,
    /// and ends at its lifetime (`T-020-51`).
    held_back: bool,
    /// The digest of the quote the log last shows the order waiting on, for a wait rebuilt from it.
    last_shown: Option<Digest>,
    pub(super) started: Option<RecordedInput>,
    /// Every login of the filer's party policy permitted on it, with the authorisation each gave: while one of them is
    /// listed, it holds, and each carries it on under its own authorisation without asking policy again.
    logins: Vec<(Actor, Authorisation)>,
    /// Whether a question about the deposit went unanswered: then, once the reserve is past its time to answer, the
    /// filing ends.
    unanswered: bool,
    /// Whether it ended on the reserve's no, or after the deposit was held against it: the deposit is filed on no more.
    spent: bool,
}

/// Whether the reserve's latest declaration says it starts routes on automatic deposits (`REQ-CAT-6`): one with no
/// declaration on record never declared it.
pub(super) fn declared(desk: &Desk<'_, '_>, reserve: &Id) -> bool {
    desk.parts
        .catalogue
        .declarations()
        .records_of(reserve.as_str())
        .last()
        .is_some_and(|d| d.starts_routes_on_automatic_deposits)
}

/// The filing with the destination's account set as the deposit's input, or `None` where it isn't one for what
/// arrived: from the outer node and in the asset it arrived in, at the amount that arrived, for the account's holder
/// as requester and beneficiary, ending at the destination's node, and naming no other account for the deposit.
fn well_formed(mut filing: Filing, arrival: &Recorded) -> Option<Filing> {
    let route = &filing.route;
    let from = Endpoint {
        node: NodeRef::Outer(arrival.node.clone()),
        asset: arrival.asset.clone(),
    };
    if route.from != from
        || route.amount != arrival.amount
        || route.follows.is_some()
        || route.to.node != NodeRef::Outer(filing.destination.node.clone())
        || filing.parties.requester != arrival.holder
        || filing.parties.beneficiary != arrival.holder
    {
        return None;
    }
    if let Some(at) = &filing.parties.settlement_account_at
        && at != &filing.destination.reserve
    {
        return None;
    }
    // Where the reserve's notice names a destination, the filing is for that one and no other (`T-020-50`): the
    // reserve, its outer node, the account, and the asset delivered there.
    if let Some(named) = &arrival.destination
        && (filing.destination.reserve.as_str() != named.reserve
            || filing.destination.node != named.node
            || filing.destination.account.as_str() != named.account
            || filing.route.to.asset != named.asset)
    {
        return None;
    }
    let account = filing.destination.account.as_str().to_owned();
    match filing.inputs.get(DESTINATION_INPUT) {
        Some(given) if given != &account => return None,
        _ => {}
    }
    filing.inputs.insert(DESTINATION_INPUT.to_owned(), account);
    Some(filing)
}

/// An `order_refusal` of `class`.
fn refusal(request: &Id, order: Option<&Id>, class: OrderRefusalClass) -> What {
    What::OrderRefusal {
        request: request.clone(),
        order: order.cloned(),
        class,
    }
}

/// Appends `pending`, in order, each as built, taking each off once the log took it.
fn flush(desk: &mut Desk<'_, '_>, pending: &mut Vec<Entry>) -> Result<(), AppendFailure> {
    while let Some(entry) = pending.first() {
        journal::append(&mut *desk.parts.log, entry)?;
        pending.remove(0);
    }
    Ok(())
}

impl Arrivals {
    /// A filing refused before an order opened: an `order_refusal` naming no order, appended and kept by the filer's
    /// party and identifier, so the same filing again is told the same and records nothing. It holds nothing.
    fn refuse_filing(
        &mut self,
        desk: &mut Desk<'_, '_>,
        filing: Filing,
        class: OrderRefusalClass,
    ) -> Result<Filed, ArrivalError> {
        let entry = journal::build(
            desk.parts.clock,
            desk.core,
            refusal(&filing.request, None, class),
        )?;
        let filed = Filed::CantProceed { order: None, class };
        let key = (
            filing.filer.party.as_str().to_owned(),
            filing.request.as_str().to_owned(),
        );
        self.refused
            .insert(key, (filing, filed.clone(), Some(entry)));
        self.flush_refused(desk)?;
        Ok(filed)
    }

    /// Appends every refused filing's refusal not yet appended.
    pub(super) fn flush_refused(&mut self, desk: &mut Desk<'_, '_>) -> Result<(), AppendFailure> {
        for (_, _, entry) in self.refused.values_mut() {
            if let Some(e) = entry {
                journal::append(&mut *desk.parts.log, e)?;
                *entry = None;
            }
        }
        Ok(())
    }

    /// Whether `order` is an identifier no filing, question or the rounds used.
    fn fresh_order(&self, desk: &Desk<'_, '_>, order: &Id) -> bool {
        !desk.parts.issued.contains(order.as_str())
            && !self.used.contains(order.as_str())
            && !self
                .runs
                .values()
                .chain(self.ended.iter())
                .any(|r| &r.order == order)
    }

    /// Takes `filing` on `arrival` up to policy's permit (the module's docs, before an order exists): the run it
    /// opens, or what it is told where it stops before one, holding nothing.
    pub(super) fn open_run(
        &mut self,
        desk: &mut Desk<'_, '_>,
        orders: &mut dyn FiledOrders,
        filing: Filing,
        arrival: Recorded,
    ) -> Result<Result<Run, Filed>, ArrivalError> {
        let Some(well) = well_formed(filing.clone(), &arrival) else {
            return Ok(Err(self.refuse_filing(
                desk,
                filing,
                OrderRefusalClass::MalformedRequest,
            )?));
        };
        // The entry Core records for the deposit states what the reserve's kept notice states (`T-020-50`, gap 10 of
        // `T-020-45`): the log holds the entry to the order, and reads no notice's body.
        if !super::kept_notice_agrees(desk, &arrival)? {
            return Ok(Err(self.refuse_filing(
                desk,
                filing,
                OrderRefusalClass::MalformedRequest,
            )?));
        }
        // A destination set for another party is no destination for this holder's deposit (gap 11).
        if arrival
            .destination
            .as_ref()
            .is_some_and(|named| named.set_for != arrival.holder.as_str())
        {
            return Ok(Err(self.refuse_filing(
                desk,
                filing,
                OrderRefusalClass::DestinationRefused,
            )?));
        }
        // The reserve is asked about the holder's funds in this session: it must be the holder's, and open.
        let now = journal::now(desk.parts.clock)?;
        if let Some(class) = crate::intake::session_refusal(
            desk.sessions,
            &well.session,
            &well.parties.requester,
            &now,
        ) {
            return Ok(Err(self.refuse_filing(desk, filing, class)?));
        }
        // The deposit goes to the destination its reserve names, or to its holder where it names none, and the
        // destination's owner is the beneficiary, who is the holder, either way.
        match desk.destinations.resolve(&well.destination) {
            Resolution::Owner(owner) if owner == well.parties.beneficiary => {}
            Resolution::NotYet => {
                return Ok(Err(Filed::Waiting {
                    order: None,
                    wait: FiledWait::DestinationNotYetResolved,
                }));
            }
            Resolution::Owner(_) | Resolution::Refused => {
                return Ok(Err(self.refuse_filing(
                    desk,
                    filing,
                    OrderRefusalClass::DestinationRefused,
                )?));
            }
        }
        let order = desk.parts.fresh.id();
        if !self.fresh_order(desk, &order) {
            return Err(ArrivalError::Fresh);
        }
        let record = OrderRecord {
            order: order.clone(),
            request: well.request.clone(),
            parties: well.parties.clone(),
            origin_reserve: arrival.reserve.clone(),
        };
        orders.opened(&record);
        let authorisation = Authorisation {
            authorisation: desk.parts.fresh.id(),
            order: order.clone(),
            origin_reserve: arrival.reserve.clone(),
            origin_account: Some(arrival.account.clone()),
            destination: well.destination.clone(),
            to_asset: well.route.to.asset.clone(),
            floor: well
                .route
                .floor
                .as_ref()
                .and_then(|f| PositiveAmount::new(f.as_str())),
            parts: None,
            given_by: well.filer.clone(),
            at: now,
        };
        let decision = match desk.authorisations.authorise(&authorisation) {
            Ok(decision) => decision,
            Err(failure) => {
                orders.withdrawn(&order);
                return Err(ArrivalError::Log(failure));
            }
        };
        if decision.permit().is_none() {
            // No order opens: the assembly drops the record it was handed.
            orders.withdrawn(&order);
            return match decision.outcome {
                // A refusal about the order itself is the filing's answer, recorded.
                Answer::Refuse {
                    class: RefusalClass::OutsideAuthorisation,
                } => Ok(Err(self.refuse_filing(
                    desk,
                    filing,
                    OrderRefusalClass::RefusedByPolicy,
                )?)),
                // A refusal about the filer: another filer may file.
                Answer::Refuse { class } => Err(ArrivalError::Refused {
                    class: refused(class),
                }),
                _ => Ok(Err(Filed::Waiting {
                    order: None,
                    wait: FiledWait::PolicyNotKnown,
                })),
            };
        }
        let check = What::DestinationCheck {
            order: order.clone(),
            destination: well.destination.clone(),
            outcome: DestinationOutcome::Resolved,
        };
        let check = match journal::build(desk.parts.clock, desk.core, check) {
            Ok(check) => check,
            // Permitted, but no order opens: the assembly drops the record it was handed.
            Err(failure) => {
                orders.withdrawn(&order);
                return Err(ArrivalError::Log(failure));
            }
        };
        let pending = vec![check];
        Ok(Ok(Run {
            asked: filing,
            arrival,
            order,
            authorisation: authorisation.clone(),
            step: Step::Hold,
            pending,
            confirming: None,
            held: None,
            held_inputs: None,
            history: History::new(),
            offered: None,
            accepting: None,
            outcome: None,
            customer: None,
            asks_the_customer: false,
            waiting_since: None,
            customer_session: None,
            reopened: false,
            held_back: false,
            last_shown: None,
            started: None,
            logins: vec![(well.filer.clone(), authorisation.clone())],
            unanswered: false,
            spent: false,
            filing: well,
        }))
    }
}

impl Run {
    /// The wait of `order` for its customer, rebuilt from the log (`T-020-51`, `reopen.rs`): the deposit `arrival`
    /// held against it, the authorisation policy permitted, when its first quote was shown (`since`, in nanoseconds
    /// since the Unix epoch) and the digest of the one the log shows last. The filing is what the log holds of it and
    /// the operator's statement of its inputs and objective, read back (`stated`: the request identifier the arrival
    /// names, and the statement's inputs and objective); where that couldn't be read, or isn't the filing's, the wait is
    /// held and never accepted.
    pub(super) fn rebuilt(
        arrival: Recorded,
        order: &Id,
        authorisation: Authorisation,
        since: i128,
        last: Digest,
        held_request: Option<Id>,
        stated: Option<StatedFiling>,
    ) -> Option<Self> {
        let held_back = stated.is_none();
        // A held wait has no statement to read: it is held and never asked the route, so what it holds is only its
        // place. Its request is the arrival's where the arrival names one.
        let (request, mut inputs, objective, needs) = match stated {
            Some(s) => (s.request, s.inputs, s.objective, s.needs),
            None => (
                held_request.unwrap_or_else(|| order.clone()),
                BTreeMap::new(),
                Objective::LowestTotalCost {},
                Vec::new(),
            ),
        };
        let holder = arrival.holder.clone();
        let account = authorisation.destination.account.as_str().to_owned();
        let filing = Filing {
            request,
            filer: authorisation.given_by.clone(),
            arrival: arrival.receipt.clone(),
            parties: Parties {
                requester: holder.clone(),
                beneficiary: holder.clone(),
                settlement_account_at: None,
                payer: holder,
            },
            // The session isn't in the log: the customer's own, where it accepts, takes its place.
            session: order.clone(),
            destination: authorisation.destination.clone(),
            route: rutent_contracts::router_api::Request {
                request_id: order.clone(),
                round: rutent_contracts::router_api::Round::Shortlist {},
                from: Endpoint {
                    node: NodeRef::Outer(arrival.node.clone()),
                    asset: arrival.asset.clone(),
                },
                to: Endpoint {
                    node: NodeRef::Outer(authorisation.destination.node.clone()),
                    asset: authorisation.to_asset.clone(),
                },
                amount: arrival.amount.clone(),
                objective,
                floor: authorisation.floor.as_ref().map(|f| f.amount().clone()),
                follows: None,
            },
            inputs: {
                inputs.insert(DESTINATION_INPUT.to_owned(), account);
                inputs
            },
            needs,
        };
        Some(Run {
            asked: filing.clone(),
            filing: filing.clone(),
            arrival,
            order: order.clone(),
            logins: vec![(filing.filer.clone(), authorisation.clone())],
            authorisation,
            step: Step::Customer,
            pending: Vec::new(),
            confirming: None,
            held: None,
            held_inputs: None,
            history: History::new(),
            offered: None,
            accepting: None,
            outcome: None,
            customer: None,
            asks_the_customer: true,
            waiting_since: Some(since.div_euclid(1_000_000_000) * 1_000_000_000),
            customer_session: None,
            reopened: true,
            held_back,
            last_shown: Some(last),
            started: None,
            unanswered: false,
            spent: true,
        })
    }

    /// `filer`, a listed login of the permitted filer's party, carries it on: it is known as one of the party's logins,
    /// and, where the acceptance isn't built yet, the acceptance names it.
    pub(super) fn carried_on_by(
        &mut self,
        desk: &mut Desk<'_, '_>,
        filer: &Actor,
    ) -> Result<Option<Filed>, ArrivalError> {
        // An acceptance already built: its last entries are appended as built, asking nothing.
        if self.accepting.is_some() {
            return Ok(None);
        }
        // A login policy permitted on it before carries it on under the authorisation it gave.
        if let Some((_, given)) = self.logins.iter().find(|(l, _)| l == filer) {
            self.authorisation = given.clone();
            self.filing.filer = filer.clone();
            return Ok(None);
        }
        // Another login gives its own authorisation: policy holds it to its own permission (`REQ-AUZ-9`).
        let authorisation = Authorisation {
            authorisation: desk.parts.fresh.id(),
            given_by: filer.clone(),
            at: journal::now(desk.parts.clock)?,
            ..self.authorisation.clone()
        };
        let decision = desk.authorisations.authorise(&authorisation)?;
        if decision.permit().is_none() {
            // Refused or not known: nothing is carried on, and the filing stays with the logins it had.
            return match decision.outcome {
                Answer::Refuse { class } => Err(ArrivalError::Refused {
                    class: refused(class),
                }),
                _ => Ok(Some(Filed::Waiting {
                    order: Some(self.order.clone()),
                    wait: FiledWait::PolicyNotKnown,
                })),
            };
        }
        self.logins.push((filer.clone(), authorisation.clone()));
        self.filing.filer = filer.clone();
        self.authorisation = authorisation;
        Ok(None)
    }

    /// Whether it should end before another filer is told it holds the deposit: under way, its session over, its
    /// reserve past its time to answer a question it left unanswered, or no login of its party left listed. The same
    /// conditions end it when its own filer carries it on.
    pub(super) fn should_end(
        &self,
        desk: &mut Desk<'_, '_>,
        wait: i128,
    ) -> Result<bool, ArrivalError> {
        // Ended already, or finishing: an acceptance that may be in the log is finished, never ended.
        if self.outcome.is_some() || self.accepting.is_some() {
            return Ok(false);
        }
        let now = journal::now(desk.parts.clock)?;
        // A wait for the customer belongs to the customer, not to the filer: it holds until its lifetime is over, whether
        // the session it was filed in is over or no login of the filer's party is left (`T-020-51`).
        if self.step == Step::Customer {
            return Ok(self.wait_is_over(now.unix_nanos(), wait));
        }
        let session_over = !desk
            .sessions
            .held(&self.filing.session)
            .is_some_and(|held| {
                held.holder == self.filing.parties.requester
                    && held.expires_at.unix_nanos() > now.unix_nanos()
            });
        let past_its_time = self.held.is_none()
            && self.unanswered
            && matches!(
                desk.parts.catalogue.reserve_eligibility(
                    self.arrival.reserve.as_str(),
                    desk.parts.clock.now().floor()
                ),
                Eligibility::TemporarilyUnavailable { .. }
            );
        Ok(session_over || past_its_time || !self.has_a_listed_login(desk))
    }

    /// Whether a login of the filer's party that filed it or carried it on is still listed as acting for its party.
    pub(super) fn has_a_listed_login(&self, desk: &mut Desk<'_, '_>) -> bool {
        self.logins
            .iter()
            .any(|(l, _)| desk.actors.acts_for(&l.acts, &l.party, l.kind, l.role))
    }

    /// The customer `by` accepts the route of the order waiting for it, the quote of digest `quote` (`REQ-QTE-10`):
    /// only the order's own customer, in the customer's role and as the register of actors lists it, and only the
    /// quote shown. Past its validity the quote is asked again, shown anew, and the order waits for it; in a session
    /// that is over the order ends. Otherwise policy is asked the customer's own authorisation, and on its permit the
    /// acceptance, `given_on_deposit`, and the start are recorded. Accepting again what started is told the same.
    pub(super) fn customer_accepts<R: Router>(
        &mut self,
        desk: &mut Desk<'_, '_>,
        arrivals: &mut Arrivals,
        by: &Actor,
        session: &Id,
        quote: &Digest,
    ) -> Result<Filed, ArrivalError> {
        if by.role != Role::Customer
            || by.party != self.filing.parties.requester
            || !desk.actors.acts_for(&by.acts, &by.party, by.kind, by.role)
        {
            return Err(ArrivalError::NotTheFiler);
        }
        if let Some(outcome) = &self.outcome {
            return Ok(outcome.clone());
        }
        // Its acceptance was built before and may be in the log: it is finished, by the customer who gave it.
        if self.accepting.is_some() {
            if self.customer.as_ref() != Some(by) {
                return Err(ArrivalError::NotTheFiler);
            }
            return self.go_on::<R>(desk, arrivals);
        }
        if self.step != Step::Customer {
            return Err(ArrivalError::NotNow);
        }
        let now = journal::now(desk.parts.clock)?;
        let wait = arrivals.wait_nanos();
        // A wait past its lifetime ends, whatever the customer was shown: it is told so.
        if self.wait_is_over(now.unix_nanos(), wait) {
            self.end_wait(desk)?;
            return self.go_on::<R>(desk, arrivals);
        }
        // A wait whose filing couldn't be read back is held, never accepted, until its lifetime ends.
        if self.held_back {
            return Ok(Filed::Waiting {
                order: Some(self.order.clone()),
                wait: FiledWait::CustomerAcceptance,
            });
        }
        let shown = match &self.offered {
            Some(offered) => &offered.digest,
            None => self.last_shown.as_ref().ok_or(ArrivalError::NotNow)?,
        };
        if shown != quote {
            return Err(ArrivalError::NotTheQuoteShown);
        }
        // The acceptance is given in a session policy opened for the customer and still open: the wait outlives the one
        // the deposit was filed in (`T-020-51`).
        if !held_by(
            desk,
            session,
            &self.filing.parties.requester,
            now.unix_nanos(),
        ) {
            return Err(ArrivalError::NoSession);
        }
        // A quote past its validity binds nobody, nor does one a reopened Core no longer holds: it is asked again, and
        // the customer accepts the one shown anew.
        if self
            .offered
            .as_ref()
            .is_none_or(|o| o.expires.is_some_and(|at| at <= now.unix_nanos()))
        {
            if self.offered.is_none() {
                self.filing.session = session.clone();
            }
            self.route::<R>(desk, wait)?;
            return self.go_on::<R>(desk, arrivals);
        }
        let authorisation = Authorisation {
            authorisation: desk.parts.fresh.id(),
            given_by: by.clone(),
            at: now,
            ..self.authorisation.clone()
        };
        let decision = desk.authorisations.authorise(&authorisation)?;
        if decision.permit().is_none() {
            return match decision.outcome {
                Answer::Refuse { class } => Err(ArrivalError::Refused {
                    class: refused(class),
                }),
                _ => Ok(Filed::Waiting {
                    order: Some(self.order.clone()),
                    wait: FiledWait::PolicyNotKnown,
                }),
            };
        }
        self.authorisation = authorisation;
        self.customer = Some(by.clone());
        self.customer_session = Some(session.clone());
        self.step = Step::Accept;
        self.go_on::<R>(desk, arrivals)
    }

    /// When its wait ends, in nanoseconds since the Unix epoch, where it waits for its customer: its lifetime counted
    /// from the first quote shown (`T-020-51`).
    pub(super) fn wait_end(&self, wait: i128) -> Option<i128> {
        (self.step == Step::Customer && self.outcome.is_none())
            .then_some(self.waiting_since? + wait)
    }

    /// Whether it waits for its customer past its lifetime at `now`.
    pub(super) fn wait_is_over(&self, now: i128, wait: i128) -> bool {
        self.wait_end(wait).is_some_and(|end| end <= now)
    }

    /// Whether its end is built and not all appended: it is carried on to the log by the next call that reads it.
    pub(super) fn ending(&self) -> bool {
        self.outcome.is_some() && !self.pending.is_empty()
    }

    /// Whether `filing` restates the filing of this wait, rebuilt from the log: the filer's party files the same deposit
    /// under the same request.
    pub(super) fn is_the_filing_reopened(&self, filing: &Filing) -> bool {
        self.reopened
            && !self.held_back
            && self.outcome.is_none()
            && self.filing.request == filing.request
            && self.filing.filer.party == filing.filer.party
            && self.asked.arrival == filing.arrival
            && self.filing.parties == filing.parties
    }

    /// Chains the next routing to the order's last decision record, and to the verifiers already asked about the person
    /// for it (`REQ-AUD-4`), as a wait rebuilt from the log holds them.
    pub(super) fn chain_to(&mut self, previous: Receipt, asked_about_person: Vec<Id>) {
        self.history = History::recorded(previous, asked_about_person);
    }

    /// Takes the filing the filer states again for the filing of the wait rebuilt from the log, so it is told the wait as
    /// it stands. It changes nothing the log, the operator's statement and the authorisation fixed (the destination,
    /// route, objective, inputs and judgements): the wait goes on as rebuilt (`T-020-51`).
    pub(super) fn restated(&mut self, filing: &Filing) {
        self.asked = filing.clone();
    }

    /// Ends its wait, recorded, and appends what it recorded: the order's end the log takes once and no more.
    pub(super) fn end_wait_recorded(
        &mut self,
        desk: &mut Desk<'_, '_>,
    ) -> Result<(), ArrivalError> {
        if self.outcome.is_none() {
            self.end_wait(desk)?;
        }
        flush(desk, &mut self.pending)?;
        Ok(())
    }

    /// Ends the order's wait past its lifetime (`T-020-51`): `wait_ended`, then the order's refusal, which tells the
    /// customer once (`T-023-04`). The deposit is free again: its order never took an acceptance, so `one-order-per-deposit`
    /// lets the next filing take it, and nothing moved.
    fn end_wait(&mut self, desk: &mut Desk<'_, '_>) -> Result<(), ArrivalError> {
        let ended = journal::build(
            desk.parts.clock,
            desk.core,
            What::WaitEnded {
                order: self.order.clone(),
                why: WaitEnd::PastItsLifetime,
            },
        )?;
        self.pending.push(ended);
        self.end(desk, OrderRefusalClass::NoRouteRightNow)?;
        self.spent = false;
        Ok(())
    }

    /// The quote the order waits on its customer to accept, as of `now`.
    pub(super) fn awaiting(&self, now: i128) -> Option<Awaited<'_>> {
        if self.step != Step::Customer || self.outcome.is_some() {
            return None;
        }
        let offered = self.offered.as_ref()?;
        Some(Awaited {
            quote: &offered.quote,
            digest: &offered.digest,
            valid_until: offered.expires.and_then(timestamp_of),
            expired: offered.expires.is_some_and(|at| at <= now),
        })
    }

    /// Ends it, recorded, with no login of its party left to carry it on.
    pub(super) fn abandon(&mut self, desk: &mut Desk<'_, '_>) -> Result<(), ArrivalError> {
        // A wait for the customer ends as a wait does, freeing its deposit.
        if self.step == Step::Customer {
            return self.end_wait_recorded(desk);
        }
        self.end(desk, OrderRefusalClass::NoRouteRightNow)?;
        flush(desk, &mut self.pending)?;
        Ok(())
    }

    /// Whether it is only finishing: its acceptance and start built, their appends not all taken.
    pub(super) fn finishing(&self) -> bool {
        self.outcome.is_none() && self.accepting.is_some()
    }

    /// Whether the deposit is filed on no more once it ended.
    pub(super) fn spends(&self) -> bool {
        self.spent
    }

    /// How it stands, once it started or ended.
    pub(super) fn outcome(&self) -> Option<Filed> {
        self.outcome.clone()
    }

    /// The filing as Core runs it.
    pub(super) fn filing(&self) -> &Filing {
        &self.filing
    }

    /// The order's record, as [`super::FiledOrders::opened`] was handed it.
    pub(super) fn record(&self) -> OrderRecord {
        OrderRecord {
            order: self.order.clone(),
            request: self.filing.request.clone(),
            parties: self.filing.parties.clone(),
            origin_reserve: self.arrival.reserve.clone(),
        }
    }

    /// Carries the filing on from the step it reached, with router `R`.
    pub(super) fn go_on<R: Router>(
        &mut self,
        desk: &mut Desk<'_, '_>,
        arrivals: &mut Arrivals,
    ) -> Result<Filed, ArrivalError> {
        loop {
            flush(desk, &mut self.pending)?;
            let wait = arrivals.wait_nanos();
            let waits = match self.step {
                Step::Hold => self.hold(desk, arrivals)?,
                Step::Route => {
                    self.route::<R>(desk, wait)?;
                    None
                }
                // It waits for its lifetime, whatever becomes of the session it was filed in: past it, the order
                // ends, its funds where they were and its deposit free (`T-020-51`).
                Step::Customer
                    if self.wait_is_over(journal::now(desk.parts.clock)?.unix_nanos(), wait) =>
                {
                    self.end_wait(desk)?;
                    None
                }
                Step::Customer => Some(Filed::Waiting {
                    order: Some(self.order.clone()),
                    wait: FiledWait::CustomerAcceptance,
                }),
                Step::Accept => {
                    self.take_acceptance(desk)?;
                    None
                }
                Step::Done => return self.outcome.clone().ok_or(ArrivalError::NotNow),
            };
            if let Some(waits) = waits {
                return Ok(waits);
            }
        }
    }

    /// Ends the order, of `class`. Once the deposit was held against it, the deposit is filed on no more: one deposit
    /// opens one order.
    fn end(
        &mut self,
        desk: &mut Desk<'_, '_>,
        class: OrderRefusalClass,
    ) -> Result<(), ArrivalError> {
        let what = refusal(&self.filing.request, Some(&self.order), class);
        self.pending
            .push(journal::build(desk.parts.clock, desk.core, what)?);
        self.outcome = Some(Filed::CantProceed {
            order: Some(self.order.clone()),
            class,
        });
        self.step = Step::Done;
        Ok(())
    }

    /// Holds the deposit against the order, on the reserve's yes to Core's question about it; waits where the reserve
    /// is silent or the check can't go yet, and ends the order on its no.
    fn hold(
        &mut self,
        desk: &mut Desk<'_, '_>,
        arrivals: &mut Arrivals,
    ) -> Result<Option<Filed>, ArrivalError> {
        let waiting = Filed::Waiting {
            order: Some(self.order.clone()),
            wait: FiledWait::ReserveNotYetConfirmed,
        };
        if self.held.is_none() {
            if self.confirming.is_none() {
                // A question that went unanswered, the reserve now past its time to answer: it waits no more
                // (`REQ-CAT-3` waits up to `REQ-CAT-19`).
                let now = desk.parts.clock.now().floor();
                if self.unanswered
                    && matches!(
                        desk.parts
                            .catalogue
                            .reserve_eligibility(self.arrival.reserve.as_str(), now),
                        Eligibility::TemporarilyUnavailable { .. }
                    )
                {
                    self.end(desk, OrderRefusalClass::NoRouteRightNow)?;
                    return Ok(None);
                }
                let asking = Asking {
                    order: &self.order,
                    reserve: &self.arrival.reserve,
                    account: &self.arrival.account,
                    session: &self.filing.session,
                    asset: &self.arrival.asset,
                    amount: &self.arrival.amount,
                };
                match Confirming::ask(desk, &mut arrivals.used, asking)? {
                    Ok(confirming) => self.confirming = Some(confirming),
                    // A check that can't go for this filing's session ever: it ends.
                    Err(
                        CheckRefusal::NoSession
                        | CheckRefusal::SessionNotForAccount
                        | CheckRefusal::NotACheck,
                    ) => {
                        self.end(desk, OrderRefusalClass::NoRouteRightNow)?;
                        return Ok(None);
                    }
                    Err(_) => return Ok(Some(waiting)),
                }
            }
            let Some(confirming) = &mut self.confirming else {
                return Err(ArrivalError::NotNow);
            };
            let report = match confirming.go_on(desk)? {
                Confirmation::Confirmed(report) => report,
                Confirmation::No => {
                    self.confirming = None;
                    self.spent = true;
                    self.end(desk, OrderRefusalClass::NoRouteRightNow)?;
                    return Ok(None);
                }
                Confirmation::Unanswered => {
                    self.confirming = None;
                    self.unanswered = true;
                    return Ok(Some(waiting));
                }
            };
            // The filing's inputs and objective, which may carry accounts, as the operator's statement of them, kept in
            // the evidence store: the log holds its salted commitment alone (`REQ-AUD-6`, `T-020-51`).
            let inputs = match &self.held_inputs {
                Some((Entry::Stated(entry), _)) => entry.commitment.clone(),
                _ => {
                    let made_at = journal::now(desk.parts.clock)?;
                    let bytes = crate::rounds::sign_statement(
                        desk.parts.operator_key,
                        desk.bounds,
                        FILING_INPUTS_SHAPE,
                        &desk.operator.acts,
                        &made_at,
                        FilingInputs {
                            order: self.order.clone(),
                            inputs: self.filing.inputs.clone(),
                            objective: self.filing.route.objective,
                            needs: self
                                .filing
                                .needs
                                .iter()
                                .map(|n| FilingNeed {
                                    tuple: n.tuple.clone(),
                                    person: n.person.clone(),
                                    verifier: n.verifier.clone(),
                                    subject: n.subject.clone(),
                                    class: n.class.clone(),
                                })
                                .collect(),
                        },
                    )
                    .map_err(|_| ArrivalError::Unrecordable)?;
                    let stated = crate::segments::stated_entry(
                        desk.parts.evidence,
                        &made_at,
                        &bytes,
                        desk.operator.clone(),
                        desk.operator,
                        None,
                    )
                    .map_err(|why| match why {
                        crate::segments::Unappended::Append(failure) => ArrivalError::Log(failure),
                        crate::segments::Unappended::Unrecordable => ArrivalError::Unrecordable,
                    })?;
                    let Entry::Stated(entry) = &stated else {
                        return Err(ArrivalError::Unrecordable);
                    };
                    let commitment = entry.commitment.clone();
                    self.held_inputs = Some((stated, None));
                    commitment
                }
            };
            let what = What::Arrival {
                reserve: self.arrival.reserve.clone(),
                node: self.arrival.node.clone(),
                asset: self.arrival.asset.clone(),
                amount: self.arrival.amount.amount().clone(),
                order: Some(self.order.clone()),
                matched: ArrivalMatch::Matches,
                report,
                // The deposit, by its reserve's notice: one deposit opens one order (`one-order-per-deposit`). A
                // deposit's order is routed as one part, so its arrival names none.
                notice: Some(self.arrival.notice.clone()),
                part: None,
                // The deposit was recorded before this order opened, against no order: this arrival holds it against
                // the order, and names that one, so a checker reads one deposit, not two
                // (`arrival-names-its-earlier-arrival`).
                earlier: Some(self.arrival.receipt.clone()),
                // The account's holder, the filing's requester: whom a quote the order waits on its customer to accept
                // waits for, which the log holds the waiting quote to (`waiting-quote-is-for-its-arrivals-holder`,
                // `T-020-45`).
                holder: Some(self.arrival.holder.clone()),
                // Where the reserve's notice names the deposit to go, which the order's authorisation is held to
                // (`automatic-deposit-rests-on-its-arrival`, `T-020-50`).
                destination: self.arrival.destination.clone().map(Box::new),
                // The request the filing was made under, and its inputs and objective as the operator stated them, so
                // a Core that reopens reads the filing back (`filed-orders-inputs-are-a-kept-statement`, `T-020-51`).
                request: Some(self.filing.request.clone()),
                inputs: Some(inputs),
            };
            self.held = Some((journal::build(desk.parts.clock, desk.core, what)?, None));
        }
        if let Some((stated, receipt)) = &mut self.held_inputs {
            journal::once(&mut *desk.parts.log, stated, receipt)?;
        }
        let Some((entry, receipt)) = &mut self.held else {
            return Err(ArrivalError::NotNow);
        };
        let receipt = journal::once(&mut *desk.parts.log, entry, receipt)?;
        // The deposit opened this order: whatever comes of it, it opens no other.
        self.spent = true;
        arrivals.note(Recorded {
            receipt,
            order: Some(self.order.clone()),
            matched: ArrivalMatch::Matches,
            ..self.arrival.clone()
        });
        self.step = Step::Route;
        Ok(None)
    }

    /// Records the acceptance on the automatic deposit, naming the authorisation policy permitted, and the order's
    /// start.
    fn take_acceptance(&mut self, desk: &mut Desk<'_, '_>) -> Result<(), ArrivalError> {
        if self.accepting.is_none() {
            // A permit doesn't expire, so it holds no longer than the session the filing was made in: past it, the
            // order ends rather than be accepted.
            let now = journal::now(desk.parts.clock)?;
            // The deposit's own acceptance holds in the session it was filed in; its customer's is given in the
            // customer's own (`T-020-51`). Past it the order ends rather than be accepted.
            let given_in = self
                .customer_session
                .clone()
                .unwrap_or_else(|| self.filing.session.clone());
            if !held_by(
                desk,
                &given_in,
                &self.filing.parties.requester,
                now.unix_nanos(),
            ) {
                return self.end(desk, OrderRefusalClass::NoRouteRightNow);
            }
            let offered = self.offered.clone().ok_or(ArrivalError::NotNow)?;
            // The deposit's own acceptance, or its customer's of a route with no firm rate, no reserve's yes with it
            // (`given_on_deposit`).
            let by = self.customer.clone().unwrap_or(self.filing.filer.clone());
            let (basis, session) = match self.customer {
                Some(_) => (AcceptanceBasis::GivenOnDeposit, Some(given_in)),
                None => (AcceptanceBasis::AutomaticDeposit, None),
            };
            let input = self.route_chosen(desk, &offered, &by)?;
            let acceptance = desk.parts.fresh.id();
            let accepted = journal::build(
                desk.parts.clock,
                &by,
                What::Acceptance {
                    acceptance: acceptance.clone(),
                    order: self.order.clone(),
                    request: self.filing.request.clone(),
                    quote: offered.digest.clone(),
                    authorisation: self.authorisation.authorisation.clone(),
                    basis,
                    parties: self.filing.parties.clone(),
                    origin_reserve: self.arrival.reserve.clone(),
                    yeses: Vec::new(),
                    session,
                    held: None,
                },
            )?;
            let start = journal::build(
                desk.parts.clock,
                desk.core,
                What::Input {
                    input: input.clone(),
                },
            )?;
            self.accepting = Some((acceptance, input, vec![accepted, start]));
        }
        let Some((acceptance, input, pending)) = &mut self.accepting else {
            return Err(ArrivalError::NotNow);
        };
        match flush(desk, pending) {
            Ok(()) => {
                self.started = Some(input.clone());
                self.outcome = Some(Filed::Started {
                    order: self.order.clone(),
                    acceptance: acceptance.clone(),
                });
                self.step = Step::Done;
                Ok(())
            }
            // A refused acceptance or start can't be carried on: the order ends, and its deposit, held against it, is
            // filed on no more.
            Err(AppendFailure::Refused(_)) => {
                self.accepting = None;
                self.end(desk, OrderRefusalClass::NoRouteRightNow)
            }
            Err(failure) => Err(ArrivalError::Log(failure)),
        }
    }

    /// Routes the order with no user present, and records the quote shown, or why it can't proceed.
    fn route<R: Router>(
        &mut self,
        desk: &mut Desk<'_, '_>,
        wait: i128,
    ) -> Result<(), ArrivalError> {
        let order = Order {
            order: self.order.clone(),
            caller: self.filing.filer.clone(),
            origin_reserve: self.arrival.reserve.clone(),
            presence: Presence::AutomaticDeposit {
                session: self.filing.session.clone(),
            },
            inputs: self.filing.inputs.clone(),
            needs: self.filing.needs.clone(),
        };
        // One deposit funds one part: a split route would need an arrival naming each part
        // (`split-deposit-arrival-names-its-part`), and one deposit has one arrival naming an order
        // (`one-order-per-deposit`).
        let mut bounds = desk.bounds.clone();
        bounds.max_parts = std::num::NonZeroU8::MIN;
        let routing = Routing {
            request: &self.filing.route,
            bounds: &bounds,
            prices: desk.prices,
            order: &order,
            core: desk.core,
            operator: desk.operator,
        };
        let routed = rounds::route::<R>(desk.parts, routing, &mut self.history)?;
        let candidate = match &routed.offer {
            Offer::NotOffered(why) => return self.end(desk, not_offered(&routed, why)),
            Offer::Offered { candidate, .. } => candidate,
        };
        let chosen = routed.choices.last().ok_or(ArrivalError::NotNow)?;
        let Some(candidate) = chosen
            .record
            .candidates()
            .iter()
            .find(|c| &c.candidate_id == candidate)
            .cloned()
        else {
            return self.end(desk, OrderRefusalClass::NoRouteRightNow);
        };
        // A part Core can't run is refused before it is shown (`REQ-ORD-16`).
        for part in candidate.parts.iter() {
            if let Err(refused) = check_part(part) {
                return self.end(desk, refused.class);
            }
        }
        // Its price, as intake shows one (`T-020-07`): a price the route can't deliver is never shown.
        let price = match crate::quote::show(
            &chosen.record,
            &candidate,
            self.filing.route.floor.as_ref(),
        ) {
            Ok(price) => price,
            Err(not_shown) => return self.end(desk, not_shown.class()),
        };
        // A route with no firm rate starts only on its customer's acceptance (`REQ-QTE-10`), which an automatic
        // deposit isn't: it is shown with its slow segments named, and the order waits for its customer (the owner's
        // ruling `deposit-no-firm-rate`, "Wait for the customer"). Asked once, every quote of it is, a requoted one
        // too, firm or not.
        self.asks_the_customer |= !price.no_firm_rate.is_empty();
        // The whole second below it, so the quote shown and the one asked again past it agree on the instant.
        let mut expires = chosen
            .record
            .earliest_expiry(&candidate)
            .map(|nanos| nanos.div_euclid(1_000_000_000) * 1_000_000_000);
        // A quote the order waits on its customer to accept stands no longer than the wait: its lifetime counts from
        // the first quote shown, and no requote renews it (`T-020-51`).
        if self.asks_the_customer {
            let since = match self.waiting_since {
                Some(since) => since,
                None => {
                    let now = journal::now(desk.parts.clock)?.unix_nanos();
                    let since = now.div_euclid(1_000_000_000) * 1_000_000_000;
                    self.waiting_since = Some(since);
                    since
                }
            };
            let lasts = since + wait;
            expires = Some(expires.map_or(lasts, |at| at.min(lasts)));
        }
        let slow = price.no_firm_rate.clone();
        let shown_as = price.shown_as;
        let quote = Quote {
            order: self.order.clone(),
            decision_record: chosen.record.record_id().clone(),
            candidate: candidate.candidate_id.clone(),
            from: self.filing.route.from.clone(),
            to: self.filing.route.to.clone(),
            amount: self.filing.route.amount.clone(),
            delivers: price.arrives.clone(),
            shown_as,
            price,
        };
        let Some(digest) = digest_of(&quote) else {
            return self.end(desk, OrderRefusalClass::NoRouteRightNow);
        };
        // A quote the order waits on its customer to accept says so, for the customer's notice (`REQ-ORD-20`): whom
        // it waits for, until when it stands and which segments are slow, no provider among them.
        let awaiting = if self.asks_the_customer {
            Some(Awaiting {
                requester: self.filing.parties.requester.clone(),
                valid_until: expires.and_then(timestamp_of),
                slow: slow_places(&quote.price, &slow),
            })
        } else {
            None
        };
        let shown = What::Shown {
            order: self.order.clone(),
            shown: digest.clone(),
            shown_as,
            awaiting,
        };
        self.pending
            .push(journal::build(desk.parts.clock, desk.core, shown)?);
        self.last_shown = Some(digest.clone());
        self.offered = Some(Offered {
            quote,
            digest,
            candidate,
            expires,
        });
        self.step = if self.asks_the_customer {
            Step::Customer
        } else {
            Step::Accept
        };
        Ok(())
    }

    /// The `route_chosen` input that opens the order: the chosen candidate's parts as the state machine plans them,
    /// each part and segment under a fresh identifier, citing the choice round's decision record, stated by the filer.
    fn route_chosen(
        &self,
        desk: &mut Desk<'_, '_>,
        offered: &Offered,
        by: &Actor,
    ) -> Result<RecordedInput, ArrivalError> {
        let fresh = &mut *desk.parts.fresh;
        let mut stm_id = || stm::Id::new(fresh.id().as_str()).ok_or(ArrivalError::Fresh);
        let mut parts = Vec::new();
        for part in offered.candidate.parts.iter() {
            let part_id = stm_id()?;
            let mut segments = Vec::new();
            for operation in part.operations.iter() {
                segments.push(SegmentPlan {
                    segment_id: stm_id()?,
                    provider: stm::Id::new(operation.provider.as_str())
                        .ok_or(ArrivalError::NotNow)?,
                    kind: operation.kind,
                    from: operation.from.clone(),
                    to: operation.to.clone(),
                });
            }
            parts.push(PartPlan { part_id, segments });
        }
        let filer = by;
        let kind = match filer.role {
            Role::Operator => SourceKind::Operator,
            Role::Customer => SourceKind::Customer,
            _ => SourceKind::Provider,
        };
        let record_id = RecordId::new(desk.parts.fresh.id().as_str()).ok_or(ArrivalError::Fresh)?;
        let recorded_at = stm::Timestamp::new(journal::now(desk.parts.clock)?.as_str())
            .ok_or(ArrivalError::NotNow)?;
        let input = RecordedInput {
            record_id,
            recorded_at,
            time_source: stm::Id::new("core_clock").ok_or(ArrivalError::NotNow)?,
            source: Source {
                kind,
                id: stm::Id::new(filer.acts.as_str()).ok_or(ArrivalError::NotNow)?,
            },
            standing: Standing::Stated,
            subject: Subject {
                order: stm::Id::new(self.order.as_str()).ok_or(ArrivalError::NotNow)?,
                part: None,
                segment: None,
            },
            observed: Observed::RouteChosen {
                decision_record: stm::Id::new(offered.quote.decision_record.as_str())
                    .ok_or(ArrivalError::NotNow)?,
                parts,
                handed_on_from: None,
            },
        };
        input.check().map_err(|_| ArrivalError::NotNow)?;
        Ok(input)
    }
}

/// Whether `session` is one policy opened for `holder` and still open at `now`, in nanoseconds since the Unix epoch.
fn held_by(desk: &mut Desk<'_, '_>, session: &Id, holder: &Id, now: i128) -> bool {
    desk.sessions
        .held(session)
        .is_some_and(|held| &held.holder == holder && held.expires_at.unix_nanos() > now)
}

/// `nanos` since the Unix epoch, as a timestamp of whole seconds.
pub(super) fn timestamp_of(nanos: i128) -> Option<rutent_contracts::router_api::Timestamp> {
    let seconds = u64::try_from(nanos.div_euclid(1_000_000_000)).ok()?;
    rutent_contracts::router_api::Timestamp::new(
        rutent_prov::catalogue::Instant(seconds).rfc3339()?,
    )
}

/// The slow segments of `price`'s route by their place in it, part after part, counted from 1.
fn slow_places(
    price: &crate::quote::Shown,
    slow: &[crate::quote::SlowSegment],
) -> Vec<SlowSegment> {
    let mut places = Vec::new();
    for named in slow {
        let before = price
            .segments
            .iter()
            .position(|s| s.part == named.part && s.operation == named.operation);
        if let Some(index) = before
            && let Some(position) = Count::new(index as u64 + 1)
        {
            places.push(SlowSegment {
                position,
                declared_seconds: named.declared_seconds.and_then(Count::new),
            });
        }
    }
    places
}

/// The digest of the quote: SHA-256 of its RFC 8785 canonical JSON, intake's own rule for the quote it shows.
fn digest_of(quote: &Quote) -> Option<Digest> {
    let canonical = to_canonical(quote).ok()?;
    let value = rutent_prov::module::wire::sha256(&canonical);
    serde_json::from_value(serde_json::json!({ "algorithm": "sha-256", "value": value })).ok()
}

/// The class a routing that offered nothing gives the filer, as intake gives a customer (`REQ-ORD-18`).
fn not_offered(routed: &Routed, why: &NotOffered) -> OrderRefusalClass {
    match why {
        NotOffered::NoRoute => {
            let record: &DecisionRecord = routed
                .choices
                .last()
                .map_or(&routed.shortlist.record, |c| &c.record);
            let candidates = record.candidates();
            let for_the_amount = !candidates.is_empty()
                && candidates.iter().all(|c| {
                    matches!(
                        c.dropped,
                        Some(
                            DropReason::OutsideBand { .. }
                                | DropReason::BeyondLevels { .. }
                                | DropReason::FeeAboveAmount { .. }
                                | DropReason::NothingDelivered { .. }
                                | DropReason::NoPositiveOutcome {}
                                | DropReason::BelowFloor {}
                        )
                    )
                });
            if for_the_amount {
                OrderRefusalClass::NoRouteForAmount
            } else {
                OrderRefusalClass::NoRouteRightNow
            }
        }
        NotOffered::OverBound { .. } => OrderRefusalClass::OverBound,
        NotOffered::PersonQuestionsPastBound => OrderRefusalClass::RefusedByPolicy,
        NotOffered::NoReservesYes | NotOffered::NotEligible { .. } => {
            OrderRefusalClass::NoRouteRightNow
        }
        NotOffered::ChargeRefused { .. } => OrderRefusalClass::RefusedByPolicy,
        NotOffered::ChargeNotKnown { .. } => OrderRefusalClass::NoRouteRightNow,
    }
}

/// The class a policy refusal of the filer's authorisation gives it.
fn refused(class: RefusalClass) -> OrderRefusalClass {
    match class {
        RefusalClass::OverBound => OrderRefusalClass::OverBound,
        _ => OrderRefusalClass::RefusedByPolicy,
    }
}
