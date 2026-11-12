//! The scenario assembly: Core's three parts put together, with the libraries, against simulated providers, to run
//! `M3`'s scenarios (`PLAN.md` §4.4).
//!
//! Core's three crates import none of one another (`DEC-2`); this package is where they meet, each handed the
//! others' calls of `rutent_contracts::core_internal::calls`. It may import every crate of ours, and nothing in the
//! workspace imports it. `F-024` owns it; `T-020-01` put in it the one wiring every later row relies on, the log's two
//! handles, and `T-024-02` the rest:
//!
//! - [`assembly`]: Core put together, [`assembly::Core`], with the policy engine, the log and the evidence store, the
//!   platform and the simulated providers, and the state machine on its host.
//! - [`harness`]: scenarios as given, when and then, each accepting an `AC-*`, with faults injected and runs seeded.
//! - [`checks`]: `T-024-02`'s four checks, which the reference host's run and the VOS replicas' run share.
//! - [`kit`]: what every scenario kit shares.
//! - [`sim`]: the simulated providers' platform side (`T-024-01`).
//!
//! The policy calls whose rows haven't built them on the engine yet (authorisations, charges, questions about
//! people) are answered beside it on policy's handle, the engine's own ([`assembly::policy::PolicySide`]): the handle
//! the engine holds is still the only policy handle.
//!
//! **Nothing outside this crate reaches the log but to read it.** The assembled Core hands out the log behind a
//! shared borrow ([`assembly::Core::log`]), policy's sources with their records private
//! ([`assembly::policy::Records`]), and policy's calls ([`assembly::policy::PolicySide`]), whose engine is the crate's.
//! The disk under the log, the reading of it back into a log, and the engine holding policy's handle are the crate's
//! alone, so a scenario test can append to the log only through policy's calls and Core's own steps:
//!
//! ```compile_fail
//! fn raw(core: &rutent_scenarios::assembly::Core) { let _ = &core.disk; }
//! ```
//!
//! ```compile_fail
//! fn raw(core: &rutent_scenarios::assembly::Core) { let _ = &core.sources().log; }
//! ```
//!
//! ```compile_fail
//! use rutent_scenarios::assembly::read_back;
//! ```
//!
//! ```compile_fail
//! fn raw(core: &rutent_scenarios::assembly::Core) { let _ = core.policy().engine(); }
//! ```
//!
//! Core's catalogue and its platform are read through views ([`assembly::Core::catalogue`],
//! [`assembly::Core::fleet`]), never changed from outside, so a test grants nothing the log didn't record:
//!
//! ```compile_fail
//! fn raw(core: &mut rutent_scenarios::assembly::Core) { let _ = &mut core.catalogue; }
//! ```
//!
//! ```compile_fail
//! fn raw(core: &mut rutent_scenarios::assembly::Core) { let _ = &mut core.fleet; }
//! ```
//!
//! **Who holds which handle on the log.** The assembly alone holds the log. It hands policy's handle,
//! [`PolicyLog`], to the policy engine and to no other part, and orchestration's, [`Log`], to orchestration. A
//! [`PolicyHandle`] can't be built outside this crate and isn't `Clone`, so the one the engine holds is the only one;
//! a [`GeneralHandle`] implements [`Log`] alone, so a part holding it can append no decision entry, which its
//! [`GeneralEntry`] can't hold anyway. This holds while Core's parts share one process; split across processes, a
//! decision entry would need the engine's signature (`T-025-05`).

pub mod assembly;
pub mod checks;
pub mod harness;
pub mod kit;
pub mod sim;

use rutent_contracts::core_internal::Receipt;
use rutent_contracts::core_internal::calls::{
    AppendFailure, DecisionEntry, GeneralEntry, Log, PolicyLog,
};
use std::cell::{Ref, RefCell};
use std::rc::Rc;

/// Core's parts, put together around one log.
pub struct Assembly<L> {
    log: Rc<RefCell<L>>,
    /// The policy engine, which holds policy's handle on the log.
    pub policy: rutent_core_authz::engine::Unbuilt<PolicyHandle<L>>,
}

impl<L: Log + PolicyLog> Assembly<L> {
    /// Puts the parts together around `log`, handing policy's handle to the policy engine.
    pub fn new(log: L) -> Self {
        let log = Rc::new(RefCell::new(log));
        let policy = rutent_core_authz::engine::Unbuilt::new(PolicyHandle(Rc::clone(&log)));
        Assembly { log, policy }
    }

    /// The general handle on the log, orchestration's: it takes no decision entry.
    pub fn orchestration_log(&self) -> GeneralHandle<L> {
        GeneralHandle(Rc::clone(&self.log))
    }

    /// The log, to read.
    pub fn log(&self) -> Ref<'_, L> {
        self.log.borrow()
    }
}

impl Assembly<rutent_core_audit::log::Unbuilt> {
    /// The parts around the log before `T-022-01` builds it.
    pub fn unbuilt() -> Self {
        Assembly::new(rutent_core_audit::log::Unbuilt)
    }
}

/// The log's general handle, orchestration's: it implements [`Log`] and nothing else.
pub struct GeneralHandle<L>(Rc<RefCell<L>>);

impl<L: Log> Log for GeneralHandle<L> {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        self.0.borrow_mut().append(entry)
    }
}

/// The log's policy handle: it implements [`PolicyLog`] and nothing else, can't be built outside this crate and
/// isn't `Clone`, so the one [`Assembly::new`] hands the policy engine is the only one.
pub struct PolicyHandle<L>(Rc<RefCell<L>>);

impl<L: PolicyLog> PolicyLog for PolicyHandle<L> {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        self.0.borrow_mut().append_decision(entry)
    }
}
