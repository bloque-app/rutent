//! What a scenario reads of Core's own run (`T-020-61`): the deployment's timer fires ([`Core::tick`]) and the round says
//! what Core did, and an operator's command is handed to Core's entry point over the deployment's ports
//! ([`Core::ports`]). Nothing here carries an order out: every step is Core's, in `rutent_core_orch`; this reads the
//! round a tick answered, as the operator's view would.

use rutent_contracts::core_internal::{Actor, Decided, Decision, Resolution};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::Id;
use rutent_core_orch::hand_on::HandedOn;
use rutent_core_orch::recalc::Recalculation;
use rutent_core_orch::segments::Ran;
use rutent_core_orch::stuck::{Moved, ResolveError};

use crate::assembly::Core;

/// One tick of Core's schedule, read for one order.
pub trait OnTick {
    /// One tick, and what Core's run of `order` came to on it: each part that had work left, as it ran, or why it
    /// stopped. Empty where no part of the order had work left.
    ///
    /// # Errors
    ///
    /// Why the order's run stopped, as the round says it.
    fn ran_on_tick(&mut self, order: &Id) -> Result<Vec<Ran>, String>;

    /// [`OnTick::ran_on_tick`], for an order of one part: what its one part's run came to.
    ///
    /// # Errors
    ///
    /// As [`OnTick::ran_on_tick`], and where nothing of the order ran.
    fn ran_once_on_tick(&mut self, order: &Id) -> Result<Ran, String>;

    /// The recalculation Core's run tried for `order`, where nothing of its part was left to run and its route failed
    /// (`REQ-ORD-22`): the one a tick since the last read tried, as its round told it, else one tick's. The new route runs
    /// on the tick after it.
    ///
    /// # Errors
    ///
    /// Why the recalculation couldn't be tried, or that none was.
    fn recalculated_on_tick(&mut self, order: &Id) -> Result<Recalculation, String>;

    /// One tick, and the hand-on Core's run recorded for `order`'s stuck part on it, its decided withdrawal done
    /// (`REQ-ORD-43`). The linked order runs on the next tick.
    ///
    /// # Errors
    ///
    /// Why the hand-on wasn't recorded, or that none was.
    fn handed_on_on_tick(&mut self, order: &Id) -> Result<HandedOn, String>;

    /// The operator's decide step of `order`'s stuck part, handed to Core's entry point
    /// ([`rutent_core_orch::stuck::decide_stuck`], `REQ-AUZ-4`).
    ///
    /// # Errors
    ///
    /// As the entry point.
    fn decide_stuck_in_core(
        &mut self,
        order: &Id,
        decider: &Actor,
        resolution: &Resolution,
        evidence: &[Digest],
    ) -> Result<Decision, ResolveError>;

    /// The operator's move step of `order`'s stuck part, handed to Core's entry point
    /// ([`rutent_core_orch::stuck::move_stuck`], `REQ-AUZ-4`).
    ///
    /// # Errors
    ///
    /// As the entry point.
    fn move_stuck_in_core(
        &mut self,
        order: &Id,
        carries_out: &Decided,
        mover: &Actor,
    ) -> Result<Moved, ResolveError>;
}

impl OnTick for Core {
    fn ran_on_tick(&mut self, order: &Id) -> Result<Vec<Ran>, String> {
        self.tick().runs.ran(order)
    }

    fn ran_once_on_tick(&mut self, order: &Id) -> Result<Ran, String> {
        self.ran_on_tick(order)?
            .into_iter()
            .next()
            .ok_or_else(|| "nothing of the order was left to run".to_owned())
    }

    fn recalculated_on_tick(&mut self, order: &Id) -> Result<Recalculation, String> {
        // One a tick since the last read tried, else the next tick's.
        if let Some(told) = self.take_recalculation(order) {
            return told;
        }
        self.tick();
        self.take_recalculation(order)
            .unwrap_or_else(|| Err("no recalculation was tried".to_owned()))
    }

    fn handed_on_on_tick(&mut self, order: &Id) -> Result<HandedOn, String> {
        self.tick()
            .runs
            .handed_on(order)
            .cloned()
            .unwrap_or_else(|| Err("no hand-on was recorded".to_owned()))
    }

    fn decide_stuck_in_core(
        &mut self,
        order: &Id,
        decider: &Actor,
        resolution: &Resolution,
        evidence: &[Digest],
    ) -> Result<Decision, ResolveError> {
        rutent_core_orch::stuck::decide_stuck(
            &mut self.ports(),
            order,
            decider,
            resolution,
            evidence,
        )
    }

    fn move_stuck_in_core(
        &mut self,
        order: &Id,
        carries_out: &Decided,
        mover: &Actor,
    ) -> Result<Moved, ResolveError> {
        rutent_core_orch::stuck::move_stuck(&mut self.ports(), order, carries_out, mover)
    }
}
