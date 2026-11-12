//! An order filed on an automatic deposit (`REQ-ORD-13`, `REQ-ORD-15`), from the arrival held against no order to the
//! order's start, each step kept so a filing that stopped is carried on by filing it again:
//!
//! 1. **Opened.** The order, under a fresh identifier, and its destination resolved (`REQ-ORD-24`): a
//!    `destination_check`, and, where it doesn't resolve, an `order_refusal` of its class.
//! 2. **The deposit held against it.** Core asks the reserve, under a fresh transaction for the order, whether the
//!    account holds what arrived (`confirm.rs`), and records the deposit's `arrival` naming the order, `matches`, on
//!    the reserve's answer: the arrival the acceptance rests on (`automatic-deposit-rests-on-its-arrival`).
//! 3. **Routed.** One routing through `rounds::route`, with no user present (`Presence::AutomaticDeposit`), in the
//!    session opened for the deposit's account: every question the rounds' own, asked under a fresh transaction for
//!    the order, and no reserve's yes asked (`REQ-ORD-32`). The quote is recorded as `shown`, labelled a quote, and
//!    its digest is what the acceptance names.
//! 4. **Accepted and started.** The authorisation, given by the filer, goes to policy, which holds a reserve to its
//!    declaration and the operator to its permission (`F-021`). Only on its permit is the `acceptance` recorded,
//!    `basis: automatic_deposit`, its `who` the filer, with no yes and no session, then the `route_chosen` input, the
//!    filer's statement. Both are built once and appended as built.

use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::core_internal::{
    AcceptanceBasis, Answer, ArrivalMatch, Authorisation, DestinationOutcome, Entry,
    OrderRefusalClass, Receipt, RefusalClass, Role, ShownAs, What,
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
use super::{ArrivalError, Arrivals, Filed, Filing, Recorded, journal};
use crate::intake::{DESTINATION_INPUT, Desk, Quote, Resolution, Wait};
use crate::rounds::{self, History, NotOffered, Offer, Order, Presence, Routed, Routing};
use crate::snapshot::check_part;

/// Where a filing stands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Its destination is to be resolved.
    Open,
    /// The deposit is to be held against it.
    Hold,
    /// It is to be routed.
    Route,
    /// Its authorisation is to be asked, its acceptance and start recorded.
    Accept,
    /// It started, or ended.
    Done,
}

/// The route offered, kept until its authorisation is permitted.
#[derive(Debug, Clone)]
struct Offered {
    quote: Quote,
    digest: Digest,
    candidate: Candidate,
}

/// One filing, kept from its first call: what it asked, the order it opened, the step it reached, and the entries
/// built and not yet appended.
#[derive(Debug, Clone)]
pub(super) struct Run {
    /// The filing as asked: a repeat is the same filing only if it equals this.
    pub(super) asked: Filing,
    /// The filing as Core runs it, the destination's account set as the deposit's input.
    filing: Filing,
    /// The arrival it files on.
    arrival: Recorded,
    pub(super) order: Id,
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
/// arrived: from the outer node and in the asset it arrived in, at the amount that arrived, for the account's
/// holder, ending at the destination's node, and naming no other account for the deposit.
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
    /// A filing refused before any order exists: an `order_refusal` naming no order, appended and kept, so the same
    /// filing again is told the same and records nothing. It holds nothing against the arrival.
    pub(super) fn refuse_filing(
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
        self.refused.push((filing, filed.clone(), Some(entry)));
        self.flush_refused(desk)?;
        Ok(filed)
    }

    /// Appends every refused filing's refusal not yet appended.
    pub(super) fn flush_refused(&mut self, desk: &mut Desk<'_, '_>) -> Result<(), AppendFailure> {
        for (_, _, entry) in &mut self.refused {
            if let Some(e) = entry {
                journal::append(&mut *desk.parts.log, e)?;
                *entry = None;
            }
        }
        Ok(())
    }

    /// Opens a run of `filing` on `arrival`, or refuses it before an order exists.
    pub(super) fn open_run(
        &mut self,
        desk: &mut Desk<'_, '_>,
        filing: Filing,
        arrival: Recorded,
    ) -> Result<Option<Run>, ArrivalError> {
        let Some(well) = well_formed(filing.clone(), &arrival) else {
            self.refuse_filing(desk, filing, OrderRefusalClass::MalformedRequest)?;
            return Ok(None);
        };
        // The reserve is asked about the holder's funds in this session: it must be the requester's, and open.
        let now = journal::now(desk.parts.clock)?;
        let held = desk.sessions.held(&well.session).is_some_and(|held| {
            held.holder == well.parties.requester && held.expires_at.unix_nanos() > now.unix_nanos()
        });
        if !held {
            self.refuse_filing(desk, filing, OrderRefusalClass::RefusedByPolicy)?;
            return Ok(None);
        }
        let order = desk.parts.fresh.id();
        if self.orders.contains_key(order.as_str()) {
            return Err(ArrivalError::Fresh);
        }
        Ok(Some(Run {
            asked: filing,
            filing: well,
            arrival,
            order,
            step: Step::Open,
            pending: Vec::new(),
            confirming: None,
            held: None,
            history: History::new(),
            offered: None,
            accepting: None,
            outcome: None,
            started: None,
        }))
    }
}

impl Run {
    /// Carries the filing on from the step it reached, with router `R`.
    pub(super) fn go_on<R: Router>(
        &mut self,
        desk: &mut Desk<'_, '_>,
        arrivals: &mut Arrivals,
    ) -> Result<Filed, ArrivalError> {
        loop {
            flush(desk, &mut self.pending)?;
            match self.step {
                Step::Open => {
                    if let Some(filed) = self.open(desk)? {
                        flush(desk, &mut self.pending)?;
                        return Ok(filed);
                    }
                }
                Step::Hold => self.hold(desk, arrivals)?,
                Step::Route => self.route::<R>(desk)?,
                Step::Accept => {
                    if let Some(filed) = self.accept(desk)? {
                        return Ok(filed);
                    }
                }
                Step::Done => {
                    return self.outcome.clone().ok_or(ArrivalError::NotNow);
                }
            }
        }
    }

    /// Ends the order, of `class`.
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

    /// Resolves the destination: on to the deposit where it resolves; waiting where it isn't resolved yet; ended
    /// where it is refused.
    fn open(&mut self, desk: &mut Desk<'_, '_>) -> Result<Option<Filed>, ArrivalError> {
        let outcome = match desk.destinations.resolve(&self.filing.destination) {
            Resolution::Owner(_) => DestinationOutcome::Resolved,
            Resolution::NotYet => DestinationOutcome::NotYet,
            Resolution::Refused => DestinationOutcome::Refused,
        };
        let check = What::DestinationCheck {
            order: self.order.clone(),
            destination: self.filing.destination.clone(),
            outcome,
        };
        self.pending
            .push(journal::build(desk.parts.clock, desk.core, check)?);
        match outcome {
            DestinationOutcome::NotYet => {
                let class = OrderRefusalClass::DestinationNotYetResolved;
                let what = refusal(&self.filing.request, Some(&self.order), class);
                self.pending
                    .push(journal::build(desk.parts.clock, desk.core, what)?);
                return Ok(Some(Filed::Waiting {
                    order: self.order.clone(),
                    wait: Wait::DestinationNotYetResolved,
                }));
            }
            DestinationOutcome::Refused => self.end(desk, OrderRefusalClass::DestinationRefused)?,
            _ => self.step = Step::Hold,
        }
        Ok(None)
    }

    /// Holds the deposit against the order, on the reserve's answer to Core's question about it.
    fn hold(
        &mut self,
        desk: &mut Desk<'_, '_>,
        arrivals: &mut Arrivals,
    ) -> Result<(), ArrivalError> {
        if self.held.is_none() {
            if self.confirming.is_none() {
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
                    Err(_) => return self.end(desk, OrderRefusalClass::NoRouteRightNow),
                }
            }
            let Some(confirming) = &mut self.confirming else {
                return Err(ArrivalError::NotNow);
            };
            let Confirmation::Confirmed(report) = confirming.go_on(desk)? else {
                return self.end(desk, OrderRefusalClass::NoRouteRightNow);
            };
            let what = What::Arrival {
                reserve: self.arrival.reserve.clone(),
                node: self.arrival.node.clone(),
                asset: self.arrival.asset.clone(),
                amount: self.arrival.amount.amount().clone(),
                order: Some(self.order.clone()),
                matched: ArrivalMatch::Matches,
                report,
            };
            self.held = Some((journal::build(desk.parts.clock, desk.core, what)?, None));
        }
        let Some((entry, receipt)) = &mut self.held else {
            return Err(ArrivalError::NotNow);
        };
        let receipt = journal::once(&mut *desk.parts.log, entry, receipt)?;
        arrivals.note(Recorded {
            receipt,
            order: Some(self.order.clone()),
            matched: ArrivalMatch::Matches,
            ..self.arrival.clone()
        });
        self.step = Step::Route;
        Ok(())
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
        let routing = Routing {
            request: &self.filing.route,
            bounds: desk.bounds,
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
        let quote = Quote {
            order: self.order.clone(),
            decision_record: chosen.record.record_id().clone(),
            candidate: candidate.candidate_id.clone(),
            from: self.filing.route.from.clone(),
            to: self.filing.route.to.clone(),
            amount: self.filing.route.amount.clone(),
            delivers,
            shown_as: ShownAs::Quote,
        };
        let Some(digest) = digest_of(&quote) else {
            return self.end(desk, OrderRefusalClass::NoRouteRightNow);
        };
        let shown = What::Shown {
            order: self.order.clone(),
            shown: digest.clone(),
            shown_as: ShownAs::Quote,
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

    /// Asks policy for the filer's authorisation, and on its permit records the acceptance on the automatic deposit
    /// and the order's start.
    fn accept(&mut self, desk: &mut Desk<'_, '_>) -> Result<Option<Filed>, ArrivalError> {
        if self.accepting.is_none() {
            let offered = self.offered.clone().ok_or(ArrivalError::NotNow)?;
            // Built and checked before policy is asked, so an acceptance that fails its own shape costs no permit.
            let authorisation_id = desk.parts.fresh.id();
            let input = self.route_chosen(desk, &offered)?;
            let acceptance = desk.parts.fresh.id();
            let filer = &self.filing.filer;
            let accepted = journal::build(
                desk.parts.clock,
                filer,
                What::Acceptance {
                    acceptance: acceptance.clone(),
                    order: self.order.clone(),
                    request: self.filing.request.clone(),
                    quote: offered.digest.clone(),
                    authorisation: authorisation_id.clone(),
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
            let authorisation = Authorisation {
                authorisation: authorisation_id,
                order: self.order.clone(),
                origin_reserve: self.arrival.reserve.clone(),
                destination: self.filing.destination.clone(),
                to_asset: self.filing.route.to.asset.clone(),
                floor: self
                    .filing
                    .route
                    .floor
                    .as_ref()
                    .and_then(|f| PositiveAmount::new(f.as_str())),
                given_by: filer.clone(),
                at: journal::now(desk.parts.clock)?,
            };
            let decision = desk.authorisations.authorise(&authorisation)?;
            if decision.permit().is_none() {
                return match decision.outcome {
                    // Only a refusal about the order itself ends it.
                    Answer::Refuse {
                        class: RefusalClass::OutsideAuthorisation,
                    } => {
                        self.end(desk, OrderRefusalClass::RefusedByPolicy)?;
                        Ok(None)
                    }
                    Answer::Refuse { class } => Err(ArrivalError::Refused {
                        class: refused(class),
                    }),
                    _ => Ok(Some(Filed::Waiting {
                        order: self.order.clone(),
                        wait: Wait::PolicyNotKnown,
                    })),
                };
            }
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
                Ok(None)
            }
            // A refused acceptance or start can't be carried on: the order ends.
            Err(AppendFailure::Refused(_)) => {
                self.accepting = None;
                self.end(desk, OrderRefusalClass::NoRouteRightNow)?;
                Ok(None)
            }
            Err(failure) => Err(ArrivalError::Log(failure)),
        }
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
    }
}

/// The class a policy refusal of the filer's authorisation gives it.
fn refused(class: RefusalClass) -> OrderRefusalClass {
    match class {
        RefusalClass::OverBound => OrderRefusalClass::OverBound,
        _ => OrderRefusalClass::RefusedByPolicy,
    }
}

impl Run {
    /// The reserve the deposit arrived at: the order's origin reserve.
    pub(super) fn arrival_reserve(&self) -> &Id {
        &self.arrival.reserve
    }
}
