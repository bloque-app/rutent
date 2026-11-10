//! v0's part of `REQ-CPL`: questions about people identified, bounded and resting on an authority.
//!
//! Declared by `T-020-01`; `T-021-07` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::calls::{AppendFailure, Log, PersonQuestions};
use rutent_contracts::core_internal::{PersonAnswer, PersonOutcome, PersonQuestion, What};

use crate::engine::{UNREACHABLE, Unbuilt};

/// Until this row builds it: no question about a person is asked.
impl<L: Log> PersonQuestions for Unbuilt<L> {
    fn ask_about_person(
        &mut self,
        question: &PersonQuestion,
    ) -> Result<PersonAnswer, AppendFailure> {
        question.check().map_err(AppendFailure::Refused)?;
        let outcome = PersonOutcome::NotKnown { cause: UNREACHABLE };
        let what = What::AskAboutPerson {
            question: question.clone(),
        };
        let receipt = self.record(&question.asker, what, &question.at, outcome.outcome())?;
        Ok(PersonAnswer {
            request: question.request.clone(),
            outcome,
            receipt,
        })
    }
}
