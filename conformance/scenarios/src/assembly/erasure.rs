//! Core carrying out an erasure through the assembly (`T-020-37`, `AC-36`): orchestration's
//! [`erase_statement`](rutent_core_orch::erasure::erase_statement) over Core's own policy, general handle of the log
//! and clock, and the evidence store bound to that log, the only handle that erases.

use rutent_contracts::core_internal::Token;
use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::module_interface::Digest;
use rutent_core_orch::erasure::{
    Erased, Erasure, NotErased, Statements, erase_statement, erase_statement_overriding,
};

use super::{BoundStore, Core, LogDisk, LogsRead};
use crate::kit;

/// The bound store as orchestration reaches it: the checks it runs before it erases, and the erasure on a done
/// result.
struct Bound<'a>(&'a BoundStore<LogDisk, LogsRead>);

impl Statements for Bound<'_> {
    fn erasable(&mut self, commitment: &Digest) -> Result<(), AppendFailure> {
        self.0.erasable(commitment)
    }

    fn erasable_overriding(&mut self, commitment: &Digest) -> Result<(), AppendFailure> {
        self.0.erasable_overriding(commitment)
    }

    fn erasable_overriding_since(
        &mut self,
        commitment: &Digest,
        decision: &rutent_contracts::core_internal::Receipt,
    ) -> Result<(), AppendFailure> {
        self.0.erasable_overriding_since(commitment, decision)
    }

    fn erase(
        &mut self,
        result: &rutent_contracts::core_internal::Receipt,
    ) -> Result<(), AppendFailure> {
        self.0.erase(result)
    }
}

impl Core {
    /// The evidence store bound again to the log as the disk holds it now: what binding finds of an erasure carried
    /// out (the statements missing, the erasures refused or left unsettled), as a restart reads it (`T-022-04`).
    ///
    /// # Errors
    ///
    /// If the log or the store won't open or bind.
    pub fn rebound_store(&self) -> Result<BoundStore<LogDisk, LogsRead>, AppendFailure> {
        let (store, log) = super::read_back(&self.disk, &self.clock)?;
        store.bind(log)
    }

    /// Erases the statement kept under `commitment`, as `caller` asks and policy decides: the decision, the effect,
    /// its done result, the store's erasure and the `erasure` entry (`REQ-AUD-13`, `DEC-18`). The commitment stays in
    /// the log, and the statement's bytes go from the store alone.
    ///
    /// # Errors
    ///
    /// Why nothing was erased, or not yet ([`NotErased`]).
    pub fn erase_statement_as(
        &mut self,
        caller: &rutent_contracts::core_internal::Actor,
        commitment: &Digest,
        reason: &str,
    ) -> Result<Erased, Box<NotErased>> {
        let reason = Token::new(reason).expect("a token");
        let mut decisions = self.policy.clone();
        let core = kit::core();
        erase_statement(
            &mut self.general,
            &mut decisions,
            &mut Bound(&self.bound),
            &self.clock,
            &mut self.fresh,
            &Erasure {
                commitment,
                reason: &reason,
                caller,
                core: &core,
            },
        )
    }

    /// [`Core::erase_statement_as`] where the operator asked to override the citations of later entries
    /// (`T-020-42`, item `erasure-of-cited-evidence`): policy is asked for an `erase_statement` that sets
    /// `overrides_citations`, which it permits an operator's login holding both `EraseStatement` and
    /// `EraseOverridingCitations` alone (`T-021-13`). The entries citing the statement stay as they are.
    ///
    /// # Errors
    ///
    /// Why nothing was erased, or not yet ([`NotErased`]).
    pub fn erase_statement_overriding_as(
        &mut self,
        caller: &rutent_contracts::core_internal::Actor,
        commitment: &Digest,
        reason: &str,
    ) -> Result<Erased, Box<NotErased>> {
        let reason = Token::new(reason).expect("a token");
        let mut decisions = self.policy.clone();
        let core = kit::core();
        erase_statement_overriding(
            &mut self.general,
            &mut decisions,
            &mut Bound(&self.bound),
            &self.clock,
            &mut self.fresh,
            &Erasure {
                commitment,
                reason: &reason,
                caller,
                core: &core,
            },
        )
    }

    /// [`Core::erase_statement_overriding_as`] by the operator's login.
    ///
    /// # Errors
    ///
    /// Why nothing was erased, or not yet ([`NotErased`]).
    pub fn erase_statement_overriding(
        &mut self,
        commitment: &Digest,
        reason: &str,
    ) -> Result<Erased, Box<NotErased>> {
        self.erase_statement_overriding_as(&kit::operator_login(), commitment, reason)
    }

    /// The erasures a Core that reopened found decided and done but not recorded (`Unsettled::reopened`), settled: the
    /// store erases each (one erased already is `Ok`) and the `erasure` entry is appended, once (`T-020-46`). Answers
    /// how many it settled; one that can't be settled now is left to the next reopening.
    pub(super) fn settle_reopened_erasures(&mut self) -> usize {
        let history: Vec<_> = self.log.borrow().entries().to_vec();
        let core = kit::core();
        let mut settled = 0;
        for unsettled in rutent_core_orch::erasure::Unsettled::reopened(&history, &core) {
            if unsettled
                .settle(&mut self.general, &mut Bound(&self.bound), &self.clock)
                .is_ok()
            {
                settled += 1;
            }
        }
        settled
    }

    /// [`Core::erase_statement_as`] by the operator's login.
    ///
    /// # Errors
    ///
    /// Why nothing was erased, or not yet ([`NotErased`]).
    pub fn erase_statement(
        &mut self,
        commitment: &Digest,
        reason: &str,
    ) -> Result<Erased, Box<NotErased>> {
        self.erase_statement_as(&kit::operator_login(), commitment, reason)
    }
}
