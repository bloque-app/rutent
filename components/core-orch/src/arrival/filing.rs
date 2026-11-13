//! An order filed on an automatic deposit (`REQ-ORD-13`, `REQ-ORD-15`), from the arrival held against no order to the
//! order's start.
//!
//! **Before an order exists**, nothing is opened, held, asked or routed, and a filing that stops here holds nothing:
//! 1. the filing is for what arrived (from its outer node, in its asset, at its amount, with no `follows`), for the
//!    account's holder as requester and beneficiary, naming no other account for the deposit; and its session is one
//!    policy opened for that holder, still open. Otherwise an `order_refusal` naming no order;
//! 2. the destination is resolved (`REQ-ORD-24`) to an owner, and that owner is the beneficiary: until the holder's
//!    own standing instruction exists, an automatic deposit goes to its holder alone. Another owner, or a refusal, is
//!    `destination_refused`; not yet resolved waits, with nothing recorded;
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
//! An order that ends after policy permitted it is refused of its class, naming it, and the deposit is free to be
//! filed on again. Arrivals takes no exposures, so ending holds nothing more to release.

use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::core_internal::{
    AcceptanceBasis, Answer, ArrivalMatch, Authorisation, DestinationOutcome, Entry,
    OrderRefusalClass, Receipt, RefusalClass, Role, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{NodeRef, to_canonical};
use rutent_contracts::router_api::{
    Amount, Candidate, DecisionRecord, DropReason, Endpoint, Id, PositiveAmount, Router,
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
use rutent_contracts::core_internal::Actor;
use rutent_prov::catalogue::{CheckRefusal, Eligibility};

/// Where a permitted filing stands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// The deposit is to be held against it.
    Hold,
    /// It is to be routed.
    Route,
    /// Its acceptance and start are to be recorded.
    Accept,
    /// It started, or ended.
    Done,
}

/// The route offered, kept until its acceptance is recorded.
#[derive(Debug, Clone)]
struct Offered {
    quote: Quote,
    digest: Digest,
    candidate: Candidate,
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
    history: History,
    offered: Option<Offered>,
    /// The acceptance's id, the start, and the entries recording them not yet appended.
    accepting: Option<(Id, RecordedInput, Vec<Entry>)>,
    outcome: Option<Filed>,
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
        // The reserve is asked about the holder's funds in this session: it must be the holder's, and open.
        let now = journal::now(desk.parts.clock)?;
        let held = desk.sessions.held(&well.session).is_some_and(|held| {
            held.holder == well.parties.requester && held.expires_at.unix_nanos() > now.unix_nanos()
        });
        if !held {
            return Ok(Err(self.refuse_filing(
                desk,
                filing,
                OrderRefusalClass::RefusedByPolicy,
            )?));
        }
        // The deposit goes to its holder: the destination's owner is the beneficiary, who is the holder.
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
            history: History::new(),
            offered: None,
            accepting: None,
            outcome: None,
            started: None,
            logins: vec![(well.filer.clone(), authorisation.clone())],
            unanswered: false,
            spent: false,
            filing: well,
        }))
    }
}

impl Run {
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
    pub(super) fn should_end(&self, desk: &mut Desk<'_, '_>) -> Result<bool, ArrivalError> {
        // Ended already, or finishing: an acceptance that may be in the log is finished, never ended.
        if self.outcome.is_some() || self.accepting.is_some() {
            return Ok(false);
        }
        let now = journal::now(desk.parts.clock)?;
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
            .any(|(l, _)| desk.actors.acts_for(&l.acts, &l.party, l.kind))
    }

    /// Ends it, recorded, with no login of its party left to carry it on.
    pub(super) fn abandon(&mut self, desk: &mut Desk<'_, '_>) -> Result<(), ArrivalError> {
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
            let waits = match self.step {
                Step::Hold => self.hold(desk, arrivals)?,
                Step::Route => {
                    self.route::<R>(desk)?;
                    None
                }
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
            };
            self.held = Some((journal::build(desk.parts.clock, desk.core, what)?, None));
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
            let open = desk
                .sessions
                .held(&self.filing.session)
                .is_some_and(|held| {
                    held.holder == self.filing.parties.requester
                        && held.expires_at.unix_nanos() > now.unix_nanos()
                });
            if !open {
                return self.end(desk, OrderRefusalClass::NoRouteRightNow);
            }
            let offered = self.offered.clone().ok_or(ArrivalError::NotNow)?;
            let input = self.route_chosen(desk, &offered)?;
            let acceptance = desk.parts.fresh.id();
            let accepted = journal::build(
                desk.parts.clock,
                &self.filing.filer,
                What::Acceptance {
                    acceptance: acceptance.clone(),
                    order: self.order.clone(),
                    request: self.filing.request.clone(),
                    quote: offered.digest.clone(),
                    authorisation: self.authorisation.authorisation.clone(),
                    basis: AcceptanceBasis::AutomaticDeposit,
                    parties: self.filing.parties.clone(),
                    origin_reserve: self.arrival.reserve.clone(),
                    yeses: Vec::new(),
                    session: None,
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
    fn route<R: Router>(&mut self, desk: &mut Desk<'_, '_>) -> Result<(), ArrivalError> {
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
        let Some(delivers) = delivered(&candidate) else {
            return self.end(desk, OrderRefusalClass::NoRouteRightNow);
        };
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
        // deposit isn't: the order ends, as any end at routing does (`T-020-07`'s review; ending or waiting for the
        // customer is the owner's, on the form).
        if !price.no_firm_rate.is_empty() {
            return self.end(desk, OrderRefusalClass::NoRouteRightNow);
        }
        let shown_as = price.shown_as;
        let quote = Quote {
            order: self.order.clone(),
            decision_record: chosen.record.record_id().clone(),
            candidate: candidate.candidate_id.clone(),
            from: self.filing.route.from.clone(),
            to: self.filing.route.to.clone(),
            amount: self.filing.route.amount.clone(),
            delivers,
            shown_as,
            price,
        };
        let Some(digest) = digest_of(&quote) else {
            return self.end(desk, OrderRefusalClass::NoRouteRightNow);
        };
        let shown = What::Shown {
            order: self.order.clone(),
            shown: digest.clone(),
            shown_as,
        };
        self.pending
            .push(journal::build(desk.parts.clock, desk.core, shown)?);
        self.offered = Some(Offered {
            quote,
            digest,
            candidate,
        });
        self.step = Step::Accept;
        Ok(())
    }

    /// The `route_chosen` input that opens the order: the chosen candidate's parts as the state machine plans them,
    /// each part and segment under a fresh identifier, citing the choice round's decision record, stated by the filer.
    fn route_chosen(
        &self,
        desk: &mut Desk<'_, '_>,
        offered: &Offered,
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
        let filer = &self.filing.filer;
        let kind = if filer.role == Role::Operator {
            SourceKind::Operator
        } else {
            SourceKind::Provider
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

/// What a candidate delivers at the destination: the sum of its parts' last operations' amounts out.
fn delivered(candidate: &Candidate) -> Option<Amount> {
    candidate
        .parts
        .iter()
        .try_fold(Amount::zero(), |sum, part| {
            sum.checked_add(&part.operations.last().amount_out)
        })
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
