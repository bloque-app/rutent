//! Core carrying out an erasure through the assembly (`T-020-37`, `AC-36`): orchestration's
//! [`erase_statement`](rutent_core_orch::erasure::erase_statement) over Core's own policy, general handle of the log
//! and clock, and the evidence store bound to that log, the only handle that erases.

use rutent_contracts::core_internal::Token;
use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::module_interface::Digest;
use rutent_core_orch::erasure::{Erased, Erasure, NotErased, Statements, erase_statement};

use super::{BoundStore, Core, LogDisk, LogsRead};
use crate::kit;

/// The bound store as orchestration reaches it: the checks it runs before it erases, and the erasure on a done
/// result.
struct Bound<'a>(&'a BoundStore<LogDisk, LogsRead>);

impl Statements for Bound<'_> {
    fn erasable(&mut self, commitment: &Digest) -> Result<(), AppendFailure> {
        self.0.erasable(commitment)
    }

    fn erase(
        &mut self,
        result: &rutent_contracts::core_internal::Receipt,
    ) -> Result<(), AppendFailure> {
        self.0.erase(result)
    }
}

impl Core {
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
