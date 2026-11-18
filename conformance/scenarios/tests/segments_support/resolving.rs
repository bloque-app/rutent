//! Stuck orders around the kit's Core (`T-020-11`): Core's stuck record over the kit's log, segments and exposures,
//! the operator's channel and a resolution's mover as the tests watch them, and a stand-in for policy's stuck rules
//! that permits every step, so Core's own rules are seen to hold where policy wrongly permits.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{
    AppendFailure, DecisionEntry, Delivery, GeneralEntry, Operator, PolicyLog, StuckOrders,
};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, Decision, Destination, Effect, Raise, Receipt, Resolution,
    Role, StuckDecide, StuckMove, TimeSource, VerifiedEntry, VerifiedKind, What, When,
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
/// named once the script runs out; and, where it states ([`Moves::stating`]), the moving provider's own word for
/// each move it answers done.
#[derive(Default)]
pub struct Moves(pub Vec<Effect>, pub VecDeque<Resolved>, pub Option<Stating>);

/// What a [`Moves`] states for a `deliver` it answers done, as a provider's answer would: the operator's question
/// under the resolution's transaction and the moving provider's result through it, signed, kept in the kit's store
/// and appended, so the part's `stuck_resolved` cites an evidence entry resting on it (`T-020-30`, the log's
/// `stuck-resolved-cites-its-parts-evidence`; `T-020-20`'s gap 2).
pub struct Stating {
    store: super::Store,
    operator: Actor,
    /// Each effect it stated for, with its statement's receipt.
    stated: Vec<(Receipt, Receipt)>,
}

impl Moves {
    /// A [`Moves`] that states the moving provider's word for each `deliver` it answers done, kept in `core`'s
    /// store; scripted as `script`.
    pub fn stating(core: &Core, script: impl IntoIterator<Item = Resolved>) -> Self {
        Moves(
            Vec::new(),
            script.into_iter().collect(),
            Some(Stating {
                store: core.store.clone(),
                operator: core.operator.clone(),
                stated: Vec::new(),
            }),
        )
    }
}

impl Resolutions for Moves {
    fn resolve(
        &mut self,
        parts: &mut Parts<'_>,
        _carrying: &Carrying<'_>,
        effect: &Effect,
    ) -> Result<Resolved, AppendFailure> {
        self.0.push(effect.clone());
        let resolved = self.1.pop_front().unwrap_or(Resolved::Done);
        if resolved == Resolved::Done
            && let Some(stating) = self.2.as_mut()
            && !stating.stated.iter().any(|(e, _)| e == &effect.receipt)
            && let Action::ResolveStuck {
                order,
                transaction,
                resolution: Resolution::Deliver { by, .. },
                ..
            } = &effect.action
        {
            let made_at = parts
                .clock
                .now()
                .timestamp()
                .ok_or(AppendFailure::Unavailable)?;
            let (question, results) = super::answered_under(
                &mut stating.store,
                &stating.operator,
                &made_at,
                order,
                None,
                transaction.as_str(),
                &[by.as_str()],
                true,
            );
            for entry in question.into_iter() {
                parts.log.append(&GeneralEntry::try_from(entry).unwrap())?;
            }
            let mut receipt = None;
            for entry in results {
                receipt = Some(parts.log.append(&GeneralEntry::try_from(entry).unwrap())?);
            }
            if let Some(receipt) = receipt {
                stating.stated.push((effect.receipt.clone(), receipt));
            }
        }
        Ok(resolved)
    }

    fn evidence_of(&self, effect: &Receipt) -> Option<Receipt> {
        self.2.as_ref().and_then(|stating| {
            stating
                .stated
                .iter()
                .find(|(e, _)| e == effect)
                .map(|(_, statement)| statement.clone())
        })
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
    /// The log's rules refuse every confirmation and decline of an outcome, as for a report already settled: the
    /// engine's call comes back `Err(Refused)` (`T-021-09`'s gap 3).
    pub log_refuses_outcomes: bool,
}

impl PermitsEveryStep {
    pub fn new(log: &Rc<RefCell<TestLog>>) -> Self {
        PermitsEveryStep {
            log: PolicyPath(Rc::clone(log)),
            asked: 0,
            answers_another: false,
            refuses_moves: None,
            log_refuses_outcomes: false,
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
        if self.log_refuses_outcomes {
            return Err(AppendFailure::Refused(
                rutent_contracts::core_internal::Violation("the report is settled already"),
            ));
        }
        let what = What::ConfirmOutcome {
            request: request.clone(),
            decision: request.request.clone(),
        };
        self.permit(&request.request, &request.confirmer, what, &request.at)
    }
}

/// A decline of a report, as the engine records it (`T-021-09`): the kit's policy permits it, and the log's rules are
/// what refuse one it should (a settled or declined report), at replay.
impl rutent_contracts::core_internal::calls::OutcomeDeclines for PermitsEveryStep {
    fn decline_outcome(
        &mut self,
        request: &rutent_contracts::core_internal::OutcomeDecline,
    ) -> Result<Decision, AppendFailure> {
        if self.log_refuses_outcomes {
            return Err(AppendFailure::Refused(
                rutent_contracts::core_internal::Violation("the report is settled already"),
            ));
        }
        let what = What::DeclineOutcome {
            request: request.clone(),
            decision: request.request.clone(),
        };
        self.permit(&request.request, &request.decliner, what, &request.at)
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

/// Runs `with` over Core's stuck parts and what settling a segment's move confirmed done reaches (`T-020-20`): the
/// kit's evidence store, policy, operator key and the order's part as the segments run it, over `modules` and
/// `grading`, with no credential proxy.
pub fn on_settling<T>(
    core: &mut Core,
    catalogue: &rutent_prov::catalogue::Catalogue,
    modules: &mut dyn rutent_core_orch::segments::Modules,
    grading: &mut dyn rutent_core_orch::segments::Grading,
    with: impl FnOnce(&mut Parts<'_>, &mut rutent_core_orch::stuck::Settling<'_>) -> T,
) -> T {
    let (order, part, auth, origin, end) = (
        id(&core.named.order),
        id(PART),
        id(&core.named.authorisation()),
        id(super::RESERVE),
        super::route_end(),
    );
    let segments = [id(&core.named.withdrawal), id(&core.named.deposit)];
    let (caller, core_actor, operator_actor) =
        (super::core(), super::core(), core.operator.clone());
    let session = id(&rutent_conformance::role::SuiteContext::default().session);
    let live = rutent_core_orch::segments::Live {
        order: &order,
        part: &part,
        segments: &segments,
        record: &core.record,
        at: 0,
        authorisation: &auth,
        origin_reserve: &origin,
        route: &end,
        caller: &caller,
        core: &core_actor,
        operator: &operator_actor,
        session: Some(&session),
        inputs: &core.inputs,
        floor: core.floor.as_ref(),
    };
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
    let mut channel = rutent_core_orch::notice::Unbuilt;
    let mut settling = rutent_core_orch::stuck::Settling {
        catalogue,
        evidence: &mut core.store,
        decisions: &mut core.policy,
        modules,
        uses: &mut no_proxy,
        grading,
        operator_key: &mut core.key,
        channel: &mut channel,
        live: &live,
    };
    with(&mut parts, &mut settling)
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
