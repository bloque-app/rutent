//! Arrivals and automatic deposits in the assembly (`T-020-04`): [`Arrivals`] held beside intake, on the desk intake
//! reaches; the orders open at a reserve read from intake's started orders; each order a filing opens handed to policy's
//! order records before policy is asked about it, and dropped where none opens; and an order started on an automatic
//! deposit followed by the segments from its start as arrivals answers it, as intake's are.
//!
//! What it doesn't do: build a reserve's [`Report`] from what a simulated reserve's module reports. No simulated
//! provider reports value arriving yet, so a scenario hands the report in (`T-024-04`, `AC-10`, `AC-11`, `AC-26`).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use rutent_contracts::core_internal::What;
use rutent_contracts::provider_role::NodeRef;
use rutent_contracts::router_api::{Amount, Id};
use rutent_contracts::state_machine::{Observed, RecordedInput};
use rutent_core_orch::arrival::{
    Arrivals, Arrived, Filed, FiledOrders, Filing, OpenOrder, OpenOrders, Report,
};
use rutent_core_orch::intake::{DESTINATION_INPUT, Desk, OrderRecord};
use rutent_router::RouteFinder;

use super::{Begun, Core, StepError, authorised, context};
use crate::kit;

/// Policy's order records, as a filing hands them over (`FiledOrders`).
struct FiledOn(Rc<RefCell<BTreeMap<String, OrderRecord>>>);

impl FiledOrders for FiledOn {
    fn opened(&mut self, record: &OrderRecord) {
        self.0
            .borrow_mut()
            .insert(record.order.as_str().to_owned(), record.clone());
    }

    fn withdrawn(&mut self, order: &Id) {
        self.0.borrow_mut().remove(order.as_str());
    }
}

/// The orders open, as read before the call.
struct Open(Vec<OpenOrder>);

impl OpenOrders for Open {
    fn open_at(&mut self, reserve: &Id, account: &Id) -> Vec<OpenOrder> {
        self.0
            .iter()
            .filter(|o| &o.reserve == reserve && &o.account == account)
            .cloned()
            .collect()
    }
}

impl Core {
    /// Runs `call` on arrivals with the desk intake reaches. `Arrivals` is held beside intake, so it is taken out for
    /// the call, which the desk's closure can't otherwise reach.
    fn with_arrivals<T>(&mut self, call: impl FnOnce(&mut Arrivals, &mut Desk<'_, '_>) -> T) -> T {
        let mut arrivals = std::mem::take(&mut self.arrivals);
        let out = self.with_desk(|_, desk| call(&mut arrivals, desk));
        self.arrivals = arrivals;
        out
    }

    /// The orders open at a reserve: each order a customer requested through intake that started, one for each part
    /// whose first segment hasn't settled, with its payer as the holder of the account the value leaves and its session.
    /// A split order names each part, at that part's amount; an order of one part names none. Every session the
    /// assembly opens speaks for the conformance account, so that is each order's account.
    fn open_orders(&self) -> Vec<OpenOrder> {
        let settled: BTreeSet<String> = self
            .inputs()
            .into_iter()
            .filter(|i| matches!(i.observed, Observed::SegmentSettled { .. }))
            .filter_map(|i| i.subject.segment.map(|s| s.as_str().to_owned()))
            .collect();
        let account = kit::id(&context().account);
        let mut open = Vec::new();
        for (order, started) in &self.started {
            let Some(request) = self.requests.get(order) else {
                continue;
            };
            let NodeRef::Outer(node) = &request.route.from.node else {
                continue;
            };
            let split = started.plan.len() > 1;
            let chosen = started.record.chosen();
            for (at, part) in started.plan.iter().enumerate() {
                let Some(first) = part.segments.first() else {
                    continue;
                };
                if settled.contains(first.segment_id.as_str()) {
                    continue;
                }
                let amount = if split {
                    match chosen.and_then(|c| c.parts.get(at)) {
                        Some(p) => p.operations.first().amount_in.clone(),
                        None => continue,
                    }
                } else {
                    request.route.amount.clone()
                };
                open.push(OpenOrder {
                    order: kit::id(order),
                    reserve: request.origin_reserve.clone(),
                    node: node.clone(),
                    asset: request.route.from.asset.clone(),
                    account: account.clone(),
                    holder: request.parties.payer.clone(),
                    amount,
                    session: request.session.clone(),
                    part: split.then(|| kit::id(part.part_id.as_str())),
                });
            }
        }
        open
    }

    /// Takes value arriving at a reserve, as the reserve's `report` tells it.
    pub fn arrive(&mut self, report: &Report) -> Result<Arrived, StepError> {
        let mut open = Open(self.open_orders());
        let arrived =
            self.with_arrivals(|arrivals, desk| arrivals.arrive(desk, &mut open, report))?;
        self.feed()?;
        Ok(arrived)
    }

    /// `filer` files the arrival at `arrival` anew against the order it differed from.
    pub fn refile(
        &mut self,
        filer: &rutent_contracts::core_internal::Actor,
        arrival: &rutent_contracts::core_internal::Receipt,
    ) -> Result<rutent_contracts::core_internal::Receipt, StepError> {
        let refiled = self.with_arrivals(|arrivals, desk| arrivals.refile(desk, filer, arrival))?;
        self.feed()?;
        Ok(refiled)
    }

    /// Files an order on an automatic deposit; once it starts, its segments follow it.
    pub fn file(&mut self, filing: Filing) -> Result<Filed, StepError> {
        let mut filed_on = FiledOn(Rc::clone(&self.records.orders));
        let filed = self.with_arrivals(|arrivals, desk| {
            arrivals.file::<RouteFinder>(desk, &mut filed_on, filing.clone())
        });
        if let Ok(Filed::Started { order, .. }) = &filed {
            self.filings
                .entry(order.as_str().to_owned())
                .or_insert_with(|| filing.clone());
            if let Some(start) = self.arrivals.start(order).cloned()
                && !self.started.contains_key(order.as_str())
            {
                self.follow_filed(order, &start)?;
            }
        }
        self.feed()?;
        Ok(filed?)
    }

    /// Follows `order`, filed on an automatic deposit, from its start as arrivals answered it.
    fn follow_filed(&mut self, order: &Id, start: &RecordedInput) -> Result<(), StepError> {
        let Observed::RouteChosen {
            decision_record,
            parts,
            ..
        } = &start.observed
        else {
            return Err(StepError::NotNow("an order starts on its route chosen"));
        };
        let record = self.record_of(order, &kit::id(decision_record.as_str()))?;
        let authorisation = self
            .verified_whats()
            .into_iter()
            .find_map(|w| match w {
                What::Acceptance {
                    order: o,
                    authorisation,
                    ..
                } if &o == order => Some(authorisation),
                _ => None,
            })
            .ok_or(StepError::NotNow("an order starts on its acceptance"))?;
        let logged: Vec<_> = self
            .log
            .borrow()
            .entries()
            .iter()
            .map(|(_, e)| e.clone())
            .collect();
        let authorised = authorised(&logged, order, &authorisation).ok_or(StepError::NotNow(
            "an acceptance rests on its authorisation",
        ))?;
        let filing = self
            .filings
            .get(order.as_str())
            .ok_or(StepError::NotNow("an order starts on its filing"))?;
        let mut inputs = filing.inputs.clone();
        inputs.insert(
            DESTINATION_INPUT.to_owned(),
            filing.destination.account.as_str().to_owned(),
        );
        // What the route delivers before the operator's fee: the chosen candidate's, as the filing's quote showed it.
        let delivers = record
            .chosen()
            .and_then(|c| {
                c.parts.iter().try_fold(Amount::zero(), |sum, part| {
                    sum.checked_add(&part.operations.last().amount_out)
                })
            })
            .ok_or(StepError::NotNow("a chosen route delivers an amount"))?;
        let begun = Begun {
            origin: authorised.origin_reserve.clone(),
            session: Some(filing.session.clone()),
            inputs,
            delivers,
        };
        self.follow_on(
            order,
            start,
            record,
            parts.clone(),
            authorisation,
            authorised,
            begun,
        )
    }
}
