//! Stuck orders around the kit's Core (`T-020-11`): Core's stuck record over the kit's log, segments and exposures,
//! the operator's channel and a resolution's mover as the tests watch them, and a stand-in for policy's stuck rules
//! that permits every step, so Core's own rules are seen to hold where policy wrongly permits.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{
    AppendFailure, DecisionEntry, Delivery, Operator, PolicyLog, StuckOrders,
};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Answer, Decision, Destination, Effect, Raise, Role, StuckDecide, StuckMove,
    TimeSource, VerifiedEntry, VerifiedKind, What, When,
};
use rutent_contracts::provider_role::OuterNode;
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_orch::stuck::{
    Carrying, Delivering, Mover, Parts, Ports, Resolutions, Resolved, Stuck, Watch, Watching,
};

use super::{Core, General, PART, PolicyPath, TestLog, authorisation_of, id};

/// An operator's login, a person acting for `party`: a stuck order's decider or mover. The kit's register holds no
/// such actor; policy's stuck rules are unbuilt (`T-021-06`).
pub fn staff(party: &str, acts: &str) -> Actor {
    Actor {
        party: id(party),
        acts: id(acts),
        role: Role::Operator,
        kind: ActorKind::Person,
    }
}

/// The order's destination, as its authorisation names it.
pub fn destination(core: &Core) -> Destination {
    authorisation_of(&core.named, None).destination
}

/// The order's origin: the payer's account at the reserve's bank, where its value came from.
pub fn origin() -> Destination {
    Destination {
        reserve: id(super::RESERVE),
        node: OuterNode {
            network: format!("{}-bank", super::RESERVE),
        },
        account: id("acct-1"),
    }
}

/// The operator's channel, watched: every raise it was handed, each answered `delivered` or not.
pub struct Raises {
    pub raised: Vec<Raise>,
    pub delivers: bool,
}

impl Default for Raises {
    fn default() -> Self {
        Raises {
            raised: Vec::new(),
            delivers: true,
        }
    }
}

impl Operator for Raises {
    fn raise(&mut self, raise: &Raise) -> Delivery {
        self.raised.push(raise.clone());
        if self.delivers {
            Delivery::Delivered
        } else {
            Delivery::Undelivered
        }
    }
}

/// What carries a resolution out, watched: every effect it was handed, each answered as scripted, or moved as
/// named once the script runs out.
#[derive(Default)]
pub struct Moves(pub Vec<Effect>, pub VecDeque<Resolved>);

impl Resolutions for Moves {
    fn resolve(
        &mut self,
        _parts: &mut Parts<'_>,
        _carrying: &Carrying<'_>,
        effect: &Effect,
    ) -> Result<Resolved, AppendFailure> {
        self.0.push(effect.clone());
        Ok(self.1.pop_front().unwrap_or(Resolved::Done))
    }
}

/// A stand-in for policy's stuck rules (`T-021-06`, unbuilt) that permits every step and records it through
/// policy's handle, as the engine would: so a test sees Core's own rules hold where policy permits wrongly, and the
/// log's rules over what it recorded.
pub struct PermitsEveryStep {
    pub log: PolicyPath,
    pub asked: usize,
    /// It answers as if to another request than the one it was sent.
    pub answers_another: bool,
    /// It refuses every move step for this class, recording the refusal as the engine would (`T-020-16`).
    pub refuses_moves: Option<rutent_contracts::core_internal::RefusalClass>,
}

impl PermitsEveryStep {
    pub fn new(log: &Rc<RefCell<TestLog>>) -> Self {
        PermitsEveryStep {
            log: PolicyPath(Rc::clone(log)),
            asked: 0,
            answers_another: false,
            refuses_moves: None,
        }
    }

    fn permit(
        &mut self,
        request: &Id,
        who: &Actor,
        what: What,
        at: &Timestamp,
    ) -> Result<Decision, AppendFailure> {
        self.answer(request, who, what, at, Answer::Permit {})
    }

    fn answer(
        &mut self,
        request: &Id,
        who: &Actor,
        what: What,
        at: &Timestamp,
        answer: Answer,
    ) -> Result<Decision, AppendFailure> {
        self.asked += 1;
        let entry = VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: who.clone(),
            what,
            when: When {
                at: at.clone(),
                source: TimeSource::CoreClock,
            },
            outcome: answer.outcome(),
        };
        let entry = DecisionEntry::try_from(entry).map_err(AppendFailure::Refused)?;
        let receipt = self.log.append_decision(&entry)?;
        let request = if self.answers_another {
            id("another-request")
        } else {
            request.clone()
        };
        Ok(Decision {
            request: request.clone(),
            decision: request.clone(),
            outcome: answer,
            receipt,
        })
    }
}

impl StuckOrders for PermitsEveryStep {
    fn decide_stuck(&mut self, request: &StuckDecide) -> Result<Decision, AppendFailure> {
        request.check().map_err(AppendFailure::Refused)?;
        let what = What::StuckDecide {
            request: request.clone(),
            decision: request.request.clone(),
        };
        self.permit(&request.request, &request.decider, what, &request.at)
    }

    fn move_stuck(&mut self, request: &StuckMove) -> Result<Decision, AppendFailure> {
        let what = What::StuckMove {
            request: request.clone(),
            decision: request.request.clone(),
        };
        let answer = match self.refuses_moves {
            Some(class) => Answer::Refuse { class },
            None => Answer::Permit {},
        };
        self.answer(&request.request, &request.mover, what, &request.at, answer)
    }

    fn report_outcome(
        &mut self,
        request: &rutent_contracts::core_internal::OutcomeReport,
    ) -> Result<Decision, AppendFailure> {
        let what = What::ReportOutcome {
            request: request.clone(),
            decision: request.request.clone(),
        };
        self.permit(&request.request, &request.reporter, what, &request.at)
    }

    fn confirm_outcome(
        &mut self,
        request: &rutent_contracts::core_internal::OutcomeConfirmation,
    ) -> Result<Decision, AppendFailure> {
        let what = What::ConfirmOutcome {
            request: request.clone(),
            decision: request.request.clone(),
        };
        self.permit(&request.request, &request.confirmer, what, &request.at)
    }
}

/// Runs `with` over Core's stuck parts: the kit's clock, log, identifiers, segments and exposures.
pub fn on_parts<T>(core: &mut Core, with: impl FnOnce(&mut Parts<'_>) -> T) -> T {
    let mut general = General(Rc::clone(&core.log));
    let mut parts = Parts {
        clock: &*core.clock,
        log: &mut general,
        fresh: &mut core.ids,
        issued: &core.issued,
        segments: &mut core.segments,
        exposures: &mut core.exposures,
    };
    with(&mut parts)
}

/// Times the kit's current order's part, raising to `raises`.
pub fn watch(
    core: &mut Core,
    stuck: &mut Stuck,
    raises: &mut Raises,
) -> Result<Watch, rutent_core_orch::stuck::StuckError> {
    let (order, part) = (id(&core.named.order), id(PART));
    let segments = [id(&core.named.withdrawal), id(&core.named.deposit)];
    let record = core.record.clone();
    let actor = super::core();
    on_parts(core, |parts| {
        stuck.watch(
            parts,
            raises,
            Watching {
                order: &order,
                part: &part,
                segments: &segments,
                record: &record,
                at: 0,
                core: &actor,
            },
        )
    })
}

/// The kit's log, its entries as the log holds them.
pub fn recorded(core: &Core) -> Vec<rutent_contracts::core_internal::Entry> {
    core.log
        .borrow()
        .entries
        .iter()
        .map(|(e, _)| e.clone())
        .collect()
}

/// Follows the kit's current order for its resolution, from the log's own record of its authorisation.
pub fn follow(core: &Core, stuck: &mut Stuck) -> Result<(), rutent_core_orch::stuck::StuckError> {
    stuck.follow(&core.segments, &recorded(core), &id(&core.named.order))
}

/// The receipts of the stated entries the kit's log holds about `segment` of the current order, each with the
/// party its claimed maker states for: the operator's signed operations, and the provider's results. A move's outcome
/// is reported and confirmed on two of these, by different makers (`outcome-confirmation-rests-on-its-report`).
pub fn statements_about(
    core: &Core,
    segment: &str,
) -> Vec<(rutent_contracts::core_internal::Receipt, Id)> {
    let log = core.log.borrow();
    log.entries
        .iter()
        .enumerate()
        .filter_map(|(i, (e, _))| match e {
            rutent_contracts::core_internal::Entry::Stated(s)
                if s.about.as_ref().is_some_and(|a| {
                    a.order
                        .as_ref()
                        .is_some_and(|o| o.as_str() == core.named.order)
                        && a.segment.as_ref().is_some_and(|g| g.as_str() == segment)
                }) =>
            {
                let rutent_contracts::core_internal::Attribution::Unverified { claimed_by } =
                    &s.attribution
                else {
                    return None;
                };
                Some((log.receipt_at(i), claimed_by.party.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Two statements about `segment` of the current order by makers of different parties: the operator's operation
/// and the provider's result, the evidence a move's outcome is reported and confirmed on.
pub fn two_makers(
    core: &Core,
    segment: &str,
) -> (
    rutent_contracts::core_internal::Receipt,
    rutent_contracts::core_internal::Receipt,
) {
    let found = statements_about(core, segment);
    let first = found
        .first()
        .expect("a statement about the segment")
        .clone();
    let second = found
        .iter()
        .find(|(_, party)| party != &first.1)
        .expect("a statement by another maker")
        .clone();
    (first.0, second.0)
}

/// Runs `with` over Core's stuck parts and the production mover over `modules`, the world's platform as the segments
/// reach it: the kit's evidence store and operator key, the record's bounds, no credential proxy (`T-020-16`).
pub fn on_platform<T>(
    core: &mut Core,
    catalogue: &rutent_prov::catalogue::Catalogue,
    modules: &mut dyn rutent_core_orch::segments::Modules,
    mover: &mut Mover,
    with: impl FnOnce(&mut Parts<'_>, &mut Delivering<'_>) -> T,
) -> T {
    on_platform_signing(core, catalogue, modules, mover, None, with)
}

/// [`on_platform`], the operation signed with `key` where one is given, and with Core's operator key otherwise.
pub fn on_platform_signing<T>(
    core: &mut Core,
    catalogue: &rutent_prov::catalogue::Catalogue,
    modules: &mut dyn rutent_core_orch::segments::Modules,
    mover: &mut Mover,
    key: Option<&mut dyn rutent_core_orch::rounds::OperatorKey>,
    with: impl FnOnce(&mut Parts<'_>, &mut Delivering<'_>) -> T,
) -> T {
    let mut general = General(Rc::clone(&core.log));
    let mut parts = Parts {
        clock: &*core.clock,
        log: &mut general,
        fresh: &mut core.ids,
        issued: &core.issued,
        segments: &mut core.segments,
        exposures: &mut core.exposures,
    };
    let mut no_proxy = rutent_core_orch::segments::NoProxy;
    let mut delivering = mover.on(Ports {
        modules,
        uses: &mut no_proxy,
        evidence: &mut core.store,
        catalogue,
        operator_key: match key {
            Some(key) => key,
            None => &mut core.key,
        },
        bounds: &core.record.bounds,
        operator: &core.operator,
    });
    with(&mut parts, &mut delivering)
}

/// [`on_platform`], the mover also given what a hand-on needs: the holding accounts and the linked orders' accepted
/// routes (`T-020-17`).
pub fn on_platform_handing<'a, T>(
    core: &'a mut Core,
    catalogue: &'a rutent_prov::catalogue::Catalogue,
    modules: &'a mut (dyn rutent_core_orch::segments::Modules + 'a),
    mover: &'a mut Mover,
    hand: rutent_core_orch::stuck::HandingOn<'a>,
    no_proxy: &'a mut rutent_core_orch::segments::NoProxy,
    with: impl FnOnce(&mut Parts<'_>, &mut Delivering<'_>) -> T,
) -> T {
    let mut general = General(Rc::clone(&core.log));
    let mut parts = Parts {
        clock: &*core.clock,
        log: &mut general,
        fresh: &mut core.ids,
        issued: &core.issued,
        segments: &mut core.segments,
        exposures: &mut core.exposures,
    };
    let mut delivering = mover
        .on(Ports {
            modules,
            uses: no_proxy,
            evidence: &mut core.store,
            catalogue,
            operator_key: &mut core.key,
            bounds: &core.record.bounds,
            operator: &core.operator,
        })
        .handing_on(hand);
    with(&mut parts, &mut delivering)
}
