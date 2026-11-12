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
    Actor, ActorKind, Answer, Decision, Destination, Effect, Outcome, Raise, Role, StuckDecide,
    StuckMove, TimeSource, VerifiedEntry, VerifiedKind, What, When,
};
use rutent_contracts::provider_role::OuterNode;
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_orch::stuck::{Parts, Resolutions, Resolved, Stuck, Watch, Watching};

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
    fn resolve(&mut self, effect: &Effect) -> Resolved {
        self.0.push(effect.clone());
        self.1.pop_front().unwrap_or(Resolved::Done)
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
}

impl PermitsEveryStep {
    pub fn new(log: &Rc<RefCell<TestLog>>) -> Self {
        PermitsEveryStep {
            log: PolicyPath(Rc::clone(log)),
            asked: 0,
            answers_another: false,
        }
    }

    fn permit(
        &mut self,
        request: &Id,
        who: &Actor,
        what: What,
        at: &Timestamp,
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
            outcome: Outcome::Permitted {},
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
            outcome: Answer::Permit {},
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
        self.permit(&request.request, &request.mover, what, &request.at)
    }
}

/// Runs `with` over Core's stuck parts: the kit's clock, log, identifiers, segments and exposures.
pub fn on_parts<T>(core: &mut Core, with: impl FnOnce(&mut Parts<'_>) -> T) -> T {
    let mut general = General(Rc::clone(&core.log));
    let mut parts = Parts {
        clock: &*core.clock,
        log: &mut general,
        fresh: &mut core.ids,
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

/// Follows the kit's current order for its resolution, from its authorisation and its payer's account (`acct-1`).
pub fn follow(core: &Core, stuck: &mut Stuck) -> Result<(), rutent_core_orch::stuck::StuckError> {
    stuck.follow(
        &core.segments,
        &authorisation_of(&core.named, None),
        &id("acct-1"),
    )
}
