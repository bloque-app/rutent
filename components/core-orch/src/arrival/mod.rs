//! Arrivals and automatic deposits: value arriving with no order opens none, and a reserve that declared it may start a
//! route on an automatic deposit.
//!
//! Declared by `T-020-01`; `T-020-04` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! One [`Arrivals`] takes value arriving at a reserve, in three calls, reaching what intake reaches (its [`Desk`]):
//!
//! 1. **[`Arrivals::arrive`]** takes a reserve's [`Report`]: what arrived, where, to whose account, and the reserve's
//!    notice, its signed statement, kept and appended about nothing, since no order is bound to it. One notice is one
//!    arrival: the same notice again, by its bytes or as the evidence store finds it restated, is the same arrival, and
//!    with other figures it is refused. The orders open there ([`OpenOrders`]), read again on every call, decide what
//!    it is:
//!    - **none open** for its holder at its reserve, outer node, asset and account, other than orders that already
//!      have their arrival: an `arrival` with no order, citing the notice. It opens no order and holds nothing, so
//!      nothing counts against the operator (`REQ-ORD-14`);
//!    - **one open**: Core asks the reserve, under a fresh transaction for that order, whether the account's funds
//!      cover what arrived (`confirm.rs`), appending the question about the order and transaction before it is sent
//!      and the answer with that transaction in its `about`, so the log binds the answer to the order before it exists
//!      (`REQ-ORD-32`, `REQ-ORD-38`). On the reserve's yes, an `arrival` naming the order, `matches` where the order
//!      states the amount that arrived and `differs` otherwise, citing that answer. Either way it settles nothing by
//!      itself: the order's first segment settles only on its own result (`T-020-08`), and a short, an excess or a
//!      piece of a split arrival is recorded and nothing more (`REQ-ORD-23`). Without the yes (a no, silence, or a
//!      check the catalogue can't let out yet) it waits: the same report again asks again, of the orders open then.
//! 2. **[`Arrivals::refile`]** files a differing arrival anew against its order, by the deployment's operator or the
//!    reserve it arrived at, and by no one else: a `refiling`, its `who` the filer, which moves nothing
//!    (`REQ-ORD-23`).
//! 3. **[`Arrivals::file`]** files an order on an automatic deposit, by the reserve that declared it starts routes on
//!    them or the operator filing for it (`REQ-ORD-13`), and records the deposit as the order's acceptance, apart from
//!    a given one (`REQ-ORD-15`). Its steps are `filing.rs`'s.
//!
//! **Each step is kept**, and every entry built once and appended as built, so the same call again, after the log
//! couldn't take an entry or lost its answer, carries on from where it stopped, asking no one twice and recording
//! nothing twice (`INV-8`).

mod confirm;
mod filing;
mod journal;

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::core_internal::{
    Actor, ArrivalMatch, Destination, Entry, OrderRefusalClass, Parties, Receipt, Role, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{Denomination, OuterNode};
use rutent_contracts::router_api::{Id, PositiveAmount, Request, Router};
use rutent_contracts::state_machine::RecordedInput;
use serde::Deserialize;

use crate::intake::{Desk, OrderRecord};
use crate::rounds::{Need, RoutingError};

use confirm::{Confirmation, Confirming};

/// The orders open at a reserve, as the assembly reads them from intake's records and the orders the segments follow:
/// an order accepted and started whose first segment hasn't settled.
pub trait OpenOrders {
    /// The orders open at `reserve` for its account `account`.
    fn open_at(&mut self, reserve: &Id, account: &Id) -> Vec<OpenOrder>;
}

/// Where the assembly learns of an order a filing opens, before policy is asked about it: policy's sources
/// (`Requester`, `OrderOrigin`, `OrderParties`) answer from the order's record, as they do from intake's for an order
/// a customer requested (`Intake::record`).
pub trait FiledOrders {
    /// The order `record` names was opened on an automatic deposit; its authorisation is asked next.
    fn opened(&mut self, record: &OrderRecord);

    /// The order handed over by [`FiledOrders::opened`] never opened: policy refused it, couldn't decide, or its
    /// answer couldn't be recorded. Its record is dropped.
    fn withdrawn(&mut self, order: &Id);
}

/// An order open at a reserve, waiting on value there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenOrder {
    /// The order.
    pub order: Id,
    /// Its origin reserve.
    pub reserve: Id,
    /// The outer node its value leaves from.
    pub node: OuterNode,
    /// The asset it moves.
    pub asset: Denomination,
    /// The holder's account at the reserve.
    pub account: Id,
    /// The party that holds the account: the order's payer.
    pub holder: Id,
    /// The amount it states, in the asset's unit.
    pub amount: PositiveAmount,
    /// The session the reserve is asked in about the account (`REQ-CAT-3`).
    pub session: Id,
}

/// Value arriving at a reserve, as the reserve's module reported it: what arrived, where, to whose account, and the
/// reserve's own notice, its signed statement as received, naming no transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The reserve.
    pub reserve: Id,
    /// The outer node it arrived at.
    pub node: OuterNode,
    /// The asset.
    pub asset: Denomination,
    /// How much arrived, in the asset's unit.
    pub amount: PositiveAmount,
    /// The account it reached.
    pub account: Id,
    /// The party that holds the account.
    pub holder: Id,
    /// The reserve's notice, as received.
    pub notice: Vec<u8>,
}

/// What an arrival came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arrived {
    /// No order is open for it: it opens none, and counts against no one (`REQ-ORD-14`).
    NoOrder {
        /// The receipt of its `arrival` entry.
        arrival: Receipt,
    },
    /// Held against an open order, which it matched or differed from (`REQ-ORD-23`).
    Held {
        /// The order.
        order: Id,
        /// How it met it.
        matched: ArrivalMatch,
        /// The receipt of its `arrival` entry.
        arrival: Receipt,
    },
    /// The reserve didn't confirm it yet when Core asked about the order: only its notice is recorded as an arrival's,
    /// and the same report again asks again.
    Waiting {
        /// The order it was asked about.
        order: Id,
        /// The receipt of the notice's stated entry.
        notice: Receipt,
    },
}

/// An order filed on an automatic deposit (`REQ-ORD-13`): by the reserve it arrived at, or the operator filing for
/// it, filling out the details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filing {
    /// The filer's identifier for it.
    pub request: Id,
    /// Who files: the reserve the deposit arrived at, or the operator.
    pub filer: Actor,
    /// The receipt of the arrival, held against no order, the order is filed on.
    pub arrival: Receipt,
    /// The order's three parties: its requester and beneficiary the account's holder.
    pub parties: Parties,
    /// The session opened through policy for the deposit's account (`REQ-CAT-3`).
    pub session: Id,
    /// Where the value arrives: an account the beneficiary owns.
    pub destination: Destination,
    /// The route: from where the deposit arrived, in its asset, at its amount.
    pub route: Request,
    /// The inputs the order carries for tuples that require them.
    pub inputs: BTreeMap<String, String>,
    /// The judgements the route's tuples need.
    pub needs: Vec<Need>,
}

/// Why a filing waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiledWait {
    /// Its destination isn't resolved yet: file again later.
    DestinationNotYetResolved,
    /// Policy couldn't decide its authorisation: file again later (`REQ-AUZ-5`).
    PolicyNotKnown,
    /// The reserve hasn't confirmed the deposit yet: silent, or its check couldn't go yet (`REQ-CAT-19`).
    ReserveNotYetConfirmed,
}

/// Where a filed order stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filed {
    /// Accepted on the automatic deposit and started.
    Started {
        /// The order.
        order: Id,
        /// The acceptance's id.
        acceptance: Id,
    },
    /// It waits: file it again later.
    Waiting {
        /// The order, once one exists.
        order: Option<Id>,
        /// Why.
        wait: FiledWait,
    },
    /// It can't proceed, of this class. The deposit is free to be filed on again.
    CantProceed {
        /// The order, where one exists.
        order: Option<Id>,
        /// The class.
        class: OrderRefusalClass,
    },
}

/// Why a call answered nothing. Nothing after the failing step was recorded, unless the error says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrivalError {
    /// The log, the evidence store or policy refused or failed. Where the log couldn't be written now, the same call
    /// again carries on from where it stopped.
    Log(AppendFailure),
    /// The reserve's notice names no shape, time or key a stated entry can hold.
    Unrecordable,
    /// The notice isn't the reserve's own, about nothing: another maker, or a transaction named in its body.
    NotItsNotice,
    /// The notice was reported before with other figures: one notice is one arrival.
    OtherFigures,
    /// The operator key didn't sign.
    Unsigned,
    /// A fresh identifier was one Core used before, or names the order.
    Fresh,
    /// The caller isn't the deployment's operator, nor a reserve, that the register says acts for
    /// the party it states.
    NotTheFiler,
    /// No arrival the filer may file on at that receipt: none recorded, one at another reserve, or none of the kind the
    /// call needs.
    NoSuchArrival,
    /// The reserve never declared that it starts routes on automatic deposits: nothing starts (`REQ-ORD-13`).
    NotDeclared,
    /// The deposit is filed on no more: its reserve said its funds aren't there, or an order was accepted on it. A new
    /// notice is a new arrival, and may be filed on.
    NotFileable,
    /// The arrival was filed on before, otherwise.
    Conflicting {
        /// The order that filing opened.
        order: Id,
    },
    /// Policy refused the filer, of this class: nothing was opened, and the deposit stays free.
    Refused {
        /// The class.
        class: OrderRefusalClass,
    },
    /// The routing stopped.
    Routing(RoutingError),
    /// The order isn't at a step the call can take.
    NotNow,
}

impl From<AppendFailure> for ArrivalError {
    fn from(failure: AppendFailure) -> Self {
        ArrivalError::Log(failure)
    }
}

impl From<RoutingError> for ArrivalError {
    fn from(error: RoutingError) -> Self {
        ArrivalError::Routing(error)
    }
}

/// An arrival Core recorded: what arrived, where, and the order it was held against, by its `arrival` entry's
/// receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Recorded {
    receipt: Receipt,
    reserve: Id,
    node: OuterNode,
    asset: Denomination,
    amount: PositiveAmount,
    account: Id,
    holder: Id,
    order: Option<Id>,
    matched: ArrivalMatch,
}

/// The open order an arrival is held against, and how it met it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    order: OpenOrder,
    matched: ArrivalMatch,
}

/// One notice taken, kept from its first call until its arrival is recorded.
#[derive(Debug, Clone)]
struct Noted {
    report: Report,
    /// The commitment the evidence store keeps its bytes under.
    commitment: Digest,
    notice: Entry,
    notice_receipt: Option<Receipt>,
    /// A question under way, for the order it asks about: carried on until it has an answer, or none.
    confirming: Option<(Target, Confirming)>,
    arrival: Option<(Entry, Option<Receipt>)>,
    /// The arrival's outcome, once it is recorded: no order, or held.
    outcome: Option<Arrived>,
}

/// A refiling, built once.
#[derive(Debug, Clone)]
struct Refiled {
    entry: Entry,
    receipt: Option<Receipt>,
}

/// A key for a receipt.
fn key(receipt: &Receipt) -> String {
    serde_json::to_string(receipt).unwrap_or_default()
}

/// Whether `filer` may file at all: the deployment's operator (its party the operator's), or a reserve, and in either
/// case an actor the register of actors says acts for the party it states (`REQ-ORD-13`,
/// `REQ-ORD-23`). Asked before anything about an arrival is looked up, so a stranger learns nothing of a receipt.
fn listed_filer(desk: &mut Desk<'_, '_>, filer: &Actor) -> bool {
    let role = (filer.role == Role::Operator && filer.party == desk.operator.party)
        || filer.role == Role::Provider;
    role && desk.actors.acts_for(&filer.acts, &filer.party)
}

/// Whether a listed filer may file at `reserve`: the operator anywhere, a reserve only where it is the reserve.
fn files_at(filer: &Actor, reserve: &Id) -> bool {
    filer.role == Role::Operator || &filer.party == reserve
}

/// The parts of a notice's envelope Core reads: its maker, and whether its body names a transaction.
#[derive(Deserialize)]
struct NoticeEnvelope {
    shape: String,
    maker: String,
    statement: serde_json::Value,
}

/// The shapes a notice of value arriving is not: every statement shape a contract names for something else, a
/// reserve's catalogue statements (its advertisement, its declarations, its reserve declaration) among them.
const NOT_A_NOTICE: &[&str] = &[
    "urn:rutent:provider-role:statement-envelope",
    rutent_contracts::provider_role::ADVERTISEMENT_SHAPE,
    "urn:rutent:provider-role:declarations",
    "urn:rutent:provider-role:reserve-declaration",
    rutent_contracts::provider_role::OPERATION_SHAPE,
    rutent_contracts::provider_role::JUDGEMENT_SHAPE,
    rutent_contracts::provider_role::REVERSAL_CONFIRMATION_SHAPE,
    "urn:rutent:provider-role:common",
    rutent_contracts::module_interface::RELEASE_ASSERTION_SHAPE,
    rutent_contracts::router_api::DECISION_RECORD_SHAPE,
];

/// The kinds of provider-role result: each answers an operation, so none is a notice of value arriving.
const RESULT_KINDS: &[&str] = &[
    "withdrawal",
    "deposit",
    "transfer",
    "conversion",
    "balance_verification",
    "judgement",
    "reversal",
    "reversal_confirmation",
    "holding_balance",
];

/// Whether `report`'s notice is its reserve's own, about nothing: made by the reserve, its body naming no
/// transaction, and of no shape a contract names for something else (an advertisement, a declaration, an operation, a
/// result of a known kind, a judgement, a confirmation, a release assertion or a decision record). provider-role has
/// no shape for a notice of value arriving, so nothing else of its body is read.
fn its_own_notice(report: &Report) -> bool {
    serde_json::from_slice::<NoticeEnvelope>(&report.notice).is_ok_and(|n| {
        let kind = n.statement.get("kind").and_then(|k| k.as_str());
        n.maker == report.reserve.as_str()
            && n.statement.get("transaction").is_none()
            && !NOT_A_NOTICE.contains(&n.shape.as_str())
            && !(n.shape == rutent_contracts::provider_role::RESULT_SHAPE
                && kind.is_some_and(|k| RESULT_KINDS.contains(&k)))
    })
}

/// Whether two filings are the same but for which login of the filer's party made them (`REQ-OPS-5`).
fn same_filing(first: &Filing, again: &Filing) -> bool {
    let filer = Actor {
        acts: first.filer.acts.clone(),
        kind: first.filer.kind,
        ..again.filer.clone()
    };
    first
        == &Filing {
            filer,
            ..again.clone()
        }
}

/// Core's record of arrivals and the orders filed on automatic deposits: every notice taken, every arrival recorded,
/// every refiling and filing, and the transactions Core asked the reserves under. In memory; keeping it across a
/// restart is the deployment's (`F-030`), as with intake's records.
#[derive(Debug, Clone, Default)]
pub struct Arrivals {
    notices: Vec<Noted>,
    recorded: Vec<Recorded>,
    refilings: BTreeMap<String, Refiled>,
    /// Each permitted filing under way or started, by the arrival it files on: it holds the arrival.
    runs: BTreeMap<String, filing::Run>,
    /// Each permitted filing that ended: it holds nothing, and the same filing again is told how it ended.
    ended: Vec<filing::Run>,
    /// Each filing refused before an order opened, by its filer's party and identifier: what it was told, and its
    /// refusal not yet appended. It holds nothing.
    refused: BTreeMap<(String, String), (Filing, Filed, Option<Entry>)>,
    /// The deposits filed on no more: their reserve said the funds aren't there, or an acceptance was recorded on them.
    spent: BTreeSet<String>,
    used: BTreeSet<String>,
}

impl Arrivals {
    /// None recorded yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Notes an arrival entry the log took.
    fn note(&mut self, recorded: Recorded) {
        if !self.recorded.iter().any(|r| r.receipt == recorded.receipt) {
            self.recorded.push(recorded);
        }
    }

    /// The orders that already have their arrival, or a question under way about one that would match them.
    fn taken(&self) -> BTreeSet<String> {
        let recorded = self
            .recorded
            .iter()
            .filter(|r| r.matched == ArrivalMatch::Matches)
            .filter_map(|r| r.order.as_ref());
        let asking = self
            .notices
            .iter()
            .filter_map(|n| n.confirming.as_ref())
            .filter(|(t, _)| t.matched == ArrivalMatch::Matches)
            .map(|(t, _)| &t.order.order);
        recorded
            .chain(asking)
            .map(|o| o.as_str().to_owned())
            .collect()
    }

    /// Records value arriving at a reserve (see the module's docs).
    pub fn arrive(
        &mut self,
        desk: &mut Desk<'_, '_>,
        open: &mut dyn OpenOrders,
        report: &Report,
    ) -> Result<Arrived, ArrivalError> {
        let at = match self
            .notices
            .iter()
            .position(|n| n.report.notice == report.notice)
        {
            Some(at) if &self.notices[at].report != report => {
                return Err(ArrivalError::OtherFigures);
            }
            Some(at) => at,
            None => {
                if !its_own_notice(report) {
                    return Err(ArrivalError::NotItsNotice);
                }
                let maker =
                    confirm::reserve_maker(desk.parts.catalogue, desk.operator, &report.reserve);
                // The notice is about nothing: no order is bound to it, and its body names no transaction.
                let (notice, restates) = journal::stated(
                    &mut *desk.parts.evidence,
                    desk.parts.clock,
                    &report.notice,
                    maker,
                    None,
                )?;
                // The same signed content, restated in other bytes, is the same notice.
                if let Some(restated) = restates
                    && let Some(at) = self.notices.iter().position(|n| n.commitment == restated)
                {
                    if !same_figures(&self.notices[at].report, report) {
                        return Err(ArrivalError::OtherFigures);
                    }
                    let mut noted = self.notices[at].clone();
                    let result = self.carry_on(desk, open, &mut noted);
                    self.notices[at] = noted;
                    return result;
                }
                let Entry::Stated(stated) = &notice else {
                    return Err(ArrivalError::Unrecordable);
                };
                self.notices.push(Noted {
                    report: report.clone(),
                    commitment: stated.commitment.clone(),
                    notice,
                    notice_receipt: None,
                    confirming: None,
                    arrival: None,
                    outcome: None,
                });
                self.notices.len() - 1
            }
        };
        let mut noted = self.notices[at].clone();
        let result = self.carry_on(desk, open, &mut noted);
        self.notices[at] = noted;
        result
    }

    fn carry_on(
        &mut self,
        desk: &mut Desk<'_, '_>,
        open: &mut dyn OpenOrders,
        noted: &mut Noted,
    ) -> Result<Arrived, ArrivalError> {
        if let Some(outcome) = &noted.outcome {
            return Ok(outcome.clone());
        }
        let notice = journal::once(
            &mut *desk.parts.log,
            &noted.notice,
            &mut noted.notice_receipt,
        )?;
        let report = noted.report.clone();
        if noted.arrival.is_none() {
            // A question under way is carried on for its own order; otherwise the orders open now decide.
            let target = match &noted.confirming {
                Some((target, _)) => Some(target.clone()),
                None => {
                    let taken = self.taken();
                    target(
                        &report,
                        open.open_at(&report.reserve, &report.account),
                        &taken,
                    )
                }
            };
            let (order, matched, cites) = match target {
                // No order open for it: it opens none, and rests on the reserve's notice.
                None => (None, ArrivalMatch::NoOrder, notice.clone()),
                // Held against the open order, on the reserve's answer to Core's question about that order.
                Some(target) => {
                    if noted.confirming.is_none() {
                        let asking = confirm::Asking {
                            order: &target.order.order,
                            reserve: &report.reserve,
                            account: &report.account,
                            session: &target.order.session,
                            asset: &report.asset,
                            amount: &report.amount,
                        };
                        match Confirming::ask(desk, &mut self.used, asking)? {
                            Ok(confirming) => noted.confirming = Some((target.clone(), confirming)),
                            // The check can't go yet: it waits, and the same report again asks again.
                            Err(_) => {
                                return Ok(Arrived::Waiting {
                                    order: target.order.order,
                                    notice,
                                });
                            }
                        }
                    }
                    let answered = match &mut noted.confirming {
                        Some((_, confirming)) => confirming.go_on(desk)?,
                        None => Confirmation::Unanswered,
                    };
                    let Confirmation::Confirmed(answer) = answered else {
                        // Answered no, or not at all: it waits, and nothing of it is kept but the notice.
                        noted.confirming = None;
                        return Ok(Arrived::Waiting {
                            order: target.order.order,
                            notice,
                        });
                    };
                    (Some(target.order.order.clone()), target.matched, answer)
                }
            };
            let what = What::Arrival {
                reserve: report.reserve.clone(),
                node: report.node.clone(),
                asset: report.asset.clone(),
                amount: report.amount.amount().clone(),
                order,
                matched,
                report: cites,
            };
            noted.arrival = Some((journal::build(desk.parts.clock, desk.core, what)?, None));
        }
        let Some((entry, receipt)) = &mut noted.arrival else {
            return Err(ArrivalError::NotNow);
        };
        let arrival = journal::once(&mut *desk.parts.log, entry, receipt)?;
        let Entry::Verified(verified) = &*entry else {
            return Err(ArrivalError::NotNow);
        };
        let What::Arrival { order, matched, .. } = &verified.what else {
            return Err(ArrivalError::NotNow);
        };
        let (order, matched) = (order.clone(), *matched);
        noted.confirming = None;
        self.note(Recorded {
            receipt: arrival.clone(),
            reserve: report.reserve.clone(),
            node: report.node.clone(),
            asset: report.asset.clone(),
            amount: report.amount.clone(),
            account: report.account.clone(),
            holder: report.holder.clone(),
            order: order.clone(),
            matched,
        });
        let outcome = match order {
            None => Arrived::NoOrder { arrival },
            Some(order) => Arrived::Held {
                order,
                matched,
                arrival,
            },
        };
        noted.outcome = Some(outcome.clone());
        Ok(outcome)
    }

    /// The arrival at `receipt` a listed `filer` may file on: one Core recorded, at a reserve the filer files at. To
    /// a filer that may not, there is none, whether one is recorded there or not.
    fn arrival_for(&self, filer: &Actor, receipt: &Receipt) -> Result<Recorded, ArrivalError> {
        self.recorded
            .iter()
            .find(|r| &r.receipt == receipt && files_at(filer, &r.reserve))
            .cloned()
            .ok_or(ArrivalError::NoSuchArrival)
    }

    /// `filer` files the arrival at `arrival` anew, against the order it differed from (`REQ-ORD-23`): only the
    /// deployment's operator, or the reserve it arrived at, acting for the party it states. Only an arrival held
    /// against an order it differed from is filed anew; one that met no order is `REQ-ORD-14`'s. The refiling is
    /// recorded, its `who` the filer, and moves nothing: the order's first segment still settles only on its own
    /// result. Filed anew again, it is the same refiling.
    pub fn refile(
        &mut self,
        desk: &mut Desk<'_, '_>,
        filer: &Actor,
        arrival: &Receipt,
    ) -> Result<Receipt, ArrivalError> {
        if !listed_filer(desk, filer) {
            return Err(ArrivalError::NotTheFiler);
        }
        let recorded = self.arrival_for(filer, arrival)?;
        let (Some(order), ArrivalMatch::Differs) = (&recorded.order, recorded.matched) else {
            return Err(ArrivalError::NoSuchArrival);
        };
        let refiled = match self.refilings.get_mut(&key(arrival)) {
            Some(refiled) => refiled,
            None => {
                let what = What::Refiling {
                    arrival: arrival.clone(),
                    order: order.clone(),
                };
                let entry = journal::build(desk.parts.clock, filer, what)?;
                self.refilings.entry(key(arrival)).or_insert(Refiled {
                    entry,
                    receipt: None,
                })
            }
        };
        Ok(journal::once(
            &mut *desk.parts.log,
            &refiled.entry,
            &mut refiled.receipt,
        )?)
    }

    /// Files an order on an automatic deposit, routed with router `R` (`filing.rs`'s steps), telling `orders` of the
    /// order before policy is asked about it. The filer is held first; then the arrival must be one Core recorded with
    /// no order at a reserve the filer files at, and that reserve's latest declaration must say it starts routes on
    /// automatic deposits: otherwise nothing starts, and nothing is asked or recorded (`REQ-ORD-13`). A filing for
    /// another amount, place, asset, holder or destination than the deposit's, or in a session not the holder's, is
    /// refused before an order exists, as is one policy refuses; none of those holds the deposit. One permitted filing
    /// holds the deposit while it is under way or started: the same filing again, by any listed login of the filer's
    /// party, is told how it stands or carries it on, and another opens nothing, unless no login of that party that
    /// filed or carried it on is listed any more, which ends it. A permitted filing that ends frees the deposit again,
    /// except where the reserve said its funds aren't there or an acceptance was recorded on it: then the deposit is
    /// filed on no more (`NotFileable`), and only a new notice is.
    pub fn file<R: Router>(
        &mut self,
        desk: &mut Desk<'_, '_>,
        orders: &mut dyn FiledOrders,
        filing: Filing,
    ) -> Result<Filed, ArrivalError> {
        if !listed_filer(desk, &filing.filer) {
            return Err(ArrivalError::NotTheFiler);
        }
        let recorded = self.arrival_for(&filing.filer, &filing.arrival)?;
        let arrival = key(&filing.arrival);
        if let Some(run) = self.runs.get_mut(&arrival) {
            if same_filing(&run.asked, &filing) {
                if let Some(outcome) = run.outcome() {
                    return Ok(outcome);
                }
                // Under way: carried on by any listed login of the permitted filer's party that policy permits.
                if let Some(told) = run.carried_on_by(desk, &filing.filer)? {
                    return Ok(told);
                }
            } else if run.should_end(desk)? {
                // Its session over, its reserve past its time, or no login of its party left: the filing ends,
                // recorded, and frees the deposit for this one.
                let Some(mut run) = self.runs.remove(&arrival) else {
                    return Err(ArrivalError::NotNow);
                };
                run.abandon(desk)?;
                self.ended.push(run);
            } else {
                return Err(ArrivalError::Conflicting {
                    order: run.order.clone(),
                });
            }
        }
        if !self.runs.contains_key(&arrival) {
            if let Some(ended) = self
                .ended
                .iter()
                .rev()
                .find(|r| same_filing(&r.asked, &filing))
            {
                return ended.outcome().ok_or(ArrivalError::NotNow);
            }
            let refused = (
                filing.filer.party.as_str().to_owned(),
                filing.request.as_str().to_owned(),
            );
            if let Some((asked, filed, _)) = self.refused.get(&refused)
                && same_filing(asked, &filing)
            {
                let filed = filed.clone();
                self.flush_refused(desk)?;
                return Ok(filed);
            }
            // Only value that met no order is filed on: one held against an open order is that order's.
            if recorded.matched != ArrivalMatch::NoOrder {
                return Err(ArrivalError::NoSuchArrival);
            }
            if self.spent.contains(&arrival) {
                return Err(ArrivalError::NotFileable);
            }
            if !filing::declared(desk, &recorded.reserve) {
                return Err(ArrivalError::NotDeclared);
            }
            match self.open_run(desk, orders, filing, recorded)? {
                Ok(run) => {
                    self.runs.insert(arrival.clone(), run);
                }
                Err(filed) => return Ok(filed),
            }
        }
        let Some(mut run) = self.runs.remove(&arrival) else {
            return Err(ArrivalError::NotNow);
        };
        let result = run.go_on::<R>(desk, self);
        // A filing that ended holds the deposit no more; one that ended on the reserve's no, or after its acceptance was
        // recorded, leaves it filed on no more.
        if matches!(run.outcome(), Some(Filed::CantProceed { .. })) {
            if run.spends() {
                self.spent.insert(arrival.clone());
            }
            self.ended.push(run);
        } else {
            self.runs.insert(arrival, run);
        }
        result
    }

    /// The run that opened `order`, under way, started or ended.
    fn run_of(&self, order: &Id) -> Option<&filing::Run> {
        self.runs
            .values()
            .chain(self.ended.iter())
            .find(|r| &r.order == order)
    }

    /// The `route_chosen` input that started `order`, as the log took it: what the segments follow
    /// (`Segments::follow`), beside intake's `Intake::start`. An order whose start the log hasn't taken has none.
    pub fn start(&self, order: &Id) -> Option<&RecordedInput> {
        self.run_of(order)?.started.as_ref()
    }

    /// The record of an order filed on an automatic deposit, for the assembly to answer policy's sources from
    /// (`Requester`, `OrderOrigin`, `OrderParties`), as `Intake::record` does for an order a customer requested; the
    /// same [`FiledOrders::opened`] was handed.
    pub fn record(&self, order: &Id) -> Option<OrderRecord> {
        self.run_of(order).map(filing::Run::record)
    }
}

/// Whether two reports state the same figures.
fn same_figures(first: &Report, again: &Report) -> bool {
    first
        == &Report {
            notice: first.notice.clone(),
            ..again.clone()
        }
}

/// The open order `report` is held against, among `open`, leaving out the orders in `taken`: only one of its holder's,
/// at its reserve, outer node and asset, for its account; the first stating the amount that arrived matches, and
/// otherwise the first differs. None open there, it meets no order.
fn target(report: &Report, open: Vec<OpenOrder>, taken: &BTreeSet<String>) -> Option<Target> {
    let here: Vec<OpenOrder> = open
        .into_iter()
        .filter(|o| {
            o.reserve == report.reserve
                && o.node == report.node
                && o.asset == report.asset
                && o.account == report.account
                && o.holder == report.holder
                && !taken.contains(o.order.as_str())
        })
        .collect();
    if let Some(order) = here.iter().find(|o| o.amount == report.amount) {
        return Some(Target {
            order: order.clone(),
            matched: ArrivalMatch::Matches,
        });
    }
    here.into_iter().next().map(|order| Target {
        order,
        matched: ArrivalMatch::Differs,
    })
}
