//! Arrivals and automatic deposits: value arriving with no order opens none, and a reserve that declared it may start a
//! route on an automatic deposit.
//!
//! Declared by `T-020-01`; `T-020-04` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! One [`Arrivals`] takes value arriving at a reserve, in three calls, reaching what intake reaches (its [`Desk`]):
//!
//! 1. **[`Arrivals::arrive`]** takes a reserve's [`Report`]: what arrived, where, to whose account, and the reserve's
//!    notice, its signed statement, kept and appended about nothing, since no order is bound to it. The orders open
//!    there ([`OpenOrders`]) decide what it is:
//!    - **none open** for its reserve, outer node, asset and account: an `arrival` with no order, citing the notice.
//!      It opens no order and holds nothing, so nothing counts against the operator (`REQ-ORD-14`);
//!    - **one open**: Core asks the reserve, under a fresh transaction for that order, whether the account holds what
//!      arrived (`confirm.rs`), appending the question about the order and transaction before it is sent and the
//!      answer with that transaction in its `about`, so the log binds the answer to the order before it exists
//!      (`REQ-ORD-32`, `REQ-ORD-38`). On the reserve's yes, an `arrival` naming the order, `matches` where the order
//!      states the amount that arrived and `differs` otherwise, citing that answer. Either way it settles nothing by
//!      itself: the order's first segment settles only on its own result (`T-020-08`), and a short, an excess or a
//!      piece of a split arrival is recorded and nothing more (`REQ-ORD-23`). Without the yes, only the notice, the
//!      question and any answer are recorded.
//! 2. **[`Arrivals::refile`]** files a differing arrival anew against its order, by the operator or the reserve it
//!    arrived at, and by no one else: a `refiling`, its `who` the filer, which moves nothing (`REQ-ORD-23`).
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
use rutent_contracts::provider_role::{Denomination, OuterNode};
use rutent_contracts::router_api::{Id, PositiveAmount, Request, Router};
use rutent_contracts::state_machine::RecordedInput;

use crate::intake::{Desk, OrderRecord, Wait};
use crate::rounds::{Need, RoutingError};

use confirm::{Confirmation, Confirming};

/// The orders open at a reserve, as the assembly reads them from intake's records and the orders the segments follow:
/// an order accepted and started whose first segment hasn't settled.
pub trait OpenOrders {
    /// The orders open at `reserve` for its account `account`.
    fn open_at(&mut self, reserve: &Id, account: &Id) -> Vec<OpenOrder>;
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
    /// The reserve didn't confirm it when Core asked about the order: only its notice is recorded.
    Unconfirmed {
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
    /// The order's three parties, its requester the account's holder.
    pub parties: Parties,
    /// The session opened through policy for the deposit's account (`REQ-CAT-3`).
    pub session: Id,
    /// Where the value arrives.
    pub destination: Destination,
    /// The route: from where the deposit arrived, in its asset, at its amount.
    pub route: Request,
    /// The inputs the order carries for tuples that require them.
    pub inputs: BTreeMap<String, String>,
    /// The judgements the route's tuples need.
    pub needs: Vec<Need>,
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
        /// The order.
        order: Id,
        /// Why.
        wait: Wait,
    },
    /// It can't proceed, of this class.
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
    /// The operator key didn't sign.
    Unsigned,
    /// A fresh identifier was one Core used before, or names the order.
    Fresh,
    /// The caller isn't the operator, nor the reserve the value arrived at, acting for the party it states.
    NotTheFiler,
    /// No arrival Core recorded at that receipt, or none of the kind the call needs.
    NoSuchArrival,
    /// The reserve never declared that it starts routes on automatic deposits: nothing starts (`REQ-ORD-13`).
    NotDeclared,
    /// The arrival was filed on before, otherwise.
    Conflicting {
        /// The order that filing opened, if any.
        order: Option<Id>,
    },
    /// Policy refused the filer, of this class; the order stands as it was.
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

/// The open order an arrival is held against: the first that states the amount that arrived, else the first open,
/// else none.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    order: OpenOrder,
    matched: ArrivalMatch,
}

/// One notice taken, kept from its first call until its arrival is recorded, so the same notice again carries it on
/// and records nothing twice.
#[derive(Debug, Clone)]
struct Noted {
    report: Report,
    /// The open order it is held against, read once, when it was first taken.
    target: Option<Target>,
    notice: Entry,
    notice_receipt: Option<Receipt>,
    confirming: Option<Confirming>,
    arrival: Option<(Entry, Option<Receipt>)>,
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

/// Whether `filer` may file an arrival at `reserve`: the operator, or that reserve acting for itself, and in either
/// case an actor the register of actors says acts for the party it states (`REQ-ORD-13`, `REQ-ORD-23`).
fn may_file(desk: &mut Desk<'_, '_>, filer: &Actor, reserve: &Id) -> bool {
    let role =
        filer.role == Role::Operator || (filer.role == Role::Provider && &filer.party == reserve);
    role && desk.actors.acts_for(&filer.acts, &filer.party)
}

/// Core's record of arrivals and the orders filed on automatic deposits: every notice taken, every arrival recorded,
/// every refiling and filing, and the transactions Core asked the reserves under. In memory; keeping it across a
/// restart is the deployment's (`F-030`), as with intake's records.
#[derive(Debug, Clone, Default)]
pub struct Arrivals {
    notices: Vec<Noted>,
    recorded: Vec<Recorded>,
    refilings: BTreeMap<String, Refiled>,
    /// Each filing that opened an order, by the arrival it files on.
    runs: BTreeMap<String, filing::Run>,
    /// Each filing refused before an order existed, with what it was told and its refusal not yet appended.
    refused: Vec<(Filing, Filed, Option<Entry>)>,
    /// Each order a filing opened, and the arrival it was filed on.
    orders: BTreeMap<String, String>,
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

    /// Records value arriving at a reserve (see the module's docs). The same report again carries it on from where
    /// it stopped and records nothing twice.
    pub fn arrive(
        &mut self,
        desk: &mut Desk<'_, '_>,
        open: &mut dyn OpenOrders,
        report: &Report,
    ) -> Result<Arrived, ArrivalError> {
        let at = match self.notices.iter().position(|n| &n.report == report) {
            Some(at) => at,
            None => {
                let maker =
                    confirm::reserve_maker(desk.parts.catalogue, desk.operator, &report.reserve);
                // The notice is about nothing: no order is bound to it, and its body names no transaction.
                let notice = journal::stated(
                    &mut *desk.parts.evidence,
                    desk.parts.clock,
                    &report.notice,
                    maker,
                    None,
                )?;
                let target = target(report, open.open_at(&report.reserve, &report.account));
                self.notices.push(Noted {
                    report: report.clone(),
                    target,
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
        let result = self.carry_on(desk, &mut noted);
        self.notices[at] = noted;
        result
    }

    fn carry_on(
        &mut self,
        desk: &mut Desk<'_, '_>,
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
        let (order, matched, cites) = match noted.target.clone() {
            // No order open for it: it opens none, and rests on the reserve's notice.
            None => (None, ArrivalMatch::NoOrder, notice.clone()),
            // Held against the open order, on the reserve's answer to Core's question about that order.
            Some(Target {
                order: open,
                matched,
            }) => {
                if noted.confirming.is_none() {
                    let asking = confirm::Asking {
                        order: &open.order,
                        reserve: &report.reserve,
                        account: &report.account,
                        session: &open.session,
                        asset: &report.asset,
                        amount: &report.amount,
                    };
                    match Confirming::ask(desk, &mut self.used, asking)? {
                        Ok(confirming) => noted.confirming = Some(confirming),
                        Err(_) => {
                            let outcome = Arrived::Unconfirmed {
                                order: open.order,
                                notice,
                            };
                            noted.outcome = Some(outcome.clone());
                            return Ok(outcome);
                        }
                    }
                }
                let answered = match &mut noted.confirming {
                    Some(confirming) => confirming.go_on(desk)?,
                    None => Confirmation::NotConfirmed,
                };
                let Confirmation::Confirmed(answer) = answered else {
                    let outcome = Arrived::Unconfirmed {
                        order: open.order,
                        notice,
                    };
                    noted.outcome = Some(outcome.clone());
                    return Ok(outcome);
                };
                (Some(open.order), matched, answer)
            }
        };
        if noted.arrival.is_none() {
            let what = What::Arrival {
                reserve: report.reserve.clone(),
                node: report.node.clone(),
                asset: report.asset.clone(),
                amount: report.amount.amount().clone(),
                order: order.clone(),
                matched,
                report: cites,
            };
            noted.arrival = Some((journal::build(desk.parts.clock, desk.core, what)?, None));
        }
        let Some((entry, receipt)) = &mut noted.arrival else {
            return Err(ArrivalError::NotNow);
        };
        let arrival = journal::once(&mut *desk.parts.log, entry, receipt)?;
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

    /// `filer` files the arrival at `arrival` anew, against the order it differed from (`REQ-ORD-23`): only the
    /// operator, or the reserve it arrived at, acting for the party it states. Only an arrival held against an order
    /// it differed from is filed anew; one that met no order is `REQ-ORD-14`'s. The refiling is recorded, its `who`
    /// the filer, and moves nothing: the order's first segment still settles only on its own result. Filed anew
    /// again, it is the same refiling.
    pub fn refile(
        &mut self,
        desk: &mut Desk<'_, '_>,
        filer: &Actor,
        arrival: &Receipt,
    ) -> Result<Receipt, ArrivalError> {
        let recorded = self
            .recorded
            .iter()
            .find(|r| &r.receipt == arrival)
            .cloned()
            .ok_or(ArrivalError::NoSuchArrival)?;
        let (Some(order), ArrivalMatch::Differs) = (&recorded.order, recorded.matched) else {
            return Err(ArrivalError::NoSuchArrival);
        };
        if !may_file(desk, filer, &recorded.reserve) {
            return Err(ArrivalError::NotTheFiler);
        }
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

    /// Files an order on an automatic deposit, routed with router `R` (`filing.rs`'s steps). The arrival must be one
    /// Core recorded held against no order (`REQ-ORD-14`), its filer the reserve it arrived at or the operator, acting
    /// for the party it states, and that reserve's latest declaration must say it starts routes on automatic
    /// deposits: otherwise nothing starts, and nothing is asked or recorded (`REQ-ORD-13`). A filing for another
    /// amount, place or asset than what arrived, or for another than the account's holder, is refused before an order
    /// exists. One order per arrival: the same filing again carries it on, and another filing on the same arrival
    /// opens nothing.
    pub fn file<R: Router>(
        &mut self,
        desk: &mut Desk<'_, '_>,
        filing: Filing,
    ) -> Result<Filed, ArrivalError> {
        let arrival = key(&filing.arrival);
        if let Some(run) = self.runs.get(&arrival) {
            if run.asked != filing {
                return Err(ArrivalError::Conflicting {
                    order: Some(run.order.clone()),
                });
            }
        } else {
            if let Some((_, filed, _)) = self.refused.iter().find(|(f, _, _)| f == &filing) {
                let filed = filed.clone();
                self.flush_refused(desk)?;
                return Ok(filed);
            }
            let recorded = self
                .recorded
                .iter()
                .find(|r| r.receipt == filing.arrival && r.matched == ArrivalMatch::NoOrder)
                .cloned()
                .ok_or(ArrivalError::NoSuchArrival)?;
            if !may_file(desk, &filing.filer, &recorded.reserve) {
                return Err(ArrivalError::NotTheFiler);
            }
            if !filing::declared(desk, &recorded.reserve) {
                return Err(ArrivalError::NotDeclared);
            }
            let Some(run) = self.open_run(desk, filing, recorded)? else {
                let (_, filed, _) = self.refused.last().ok_or(ArrivalError::NotNow)?;
                return Ok(filed.clone());
            };
            self.orders
                .insert(run.order.as_str().to_owned(), arrival.clone());
            self.runs.insert(arrival.clone(), run);
        }
        let Some(mut run) = self.runs.remove(&arrival) else {
            return Err(ArrivalError::NotNow);
        };
        let result = run.go_on::<R>(desk, self);
        self.runs.insert(arrival, run);
        result
    }

    /// The `route_chosen` input that started `order`, as the log took it: what the segments follow
    /// (`Segments::follow`), beside intake's `Intake::start`. An order whose start the log hasn't taken has none.
    pub fn start(&self, order: &Id) -> Option<&RecordedInput> {
        self.runs
            .get(self.orders.get(order.as_str())?)?
            .started
            .as_ref()
    }

    /// The record of an order filed on an automatic deposit, for the assembly to answer policy's sources from
    /// (`Requester`, `OrderOrigin`, `OrderParties`), as `Intake::record` does for an order a customer requested.
    pub fn record(&self, order: &Id) -> Option<OrderRecord> {
        let run = self.runs.get(self.orders.get(order.as_str())?)?;
        Some(OrderRecord {
            order: order.clone(),
            request: run.asked.request.clone(),
            parties: run.asked.parties.clone(),
            origin_reserve: run.arrival_reserve().clone(),
        })
    }
}

/// The open order `report` is held against, among `open`: only one at its reserve, outer node and asset, for its
/// account; the first stating the amount that arrived matches, and otherwise the first differs. None open there, it
/// meets no order.
fn target(report: &Report, open: Vec<OpenOrder>) -> Option<Target> {
    let here: Vec<OpenOrder> = open
        .into_iter()
        .filter(|o| {
            o.reserve == report.reserve
                && o.node == report.node
                && o.asset == report.asset
                && o.account == report.account
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
