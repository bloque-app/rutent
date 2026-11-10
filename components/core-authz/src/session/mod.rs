//! Sessions and reads: the session register the catalogue reads, and who may read which order or segment.
//!
//! Declared by `T-020-01`; `T-021-02` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::calls::{AppendFailure, Log, Reads, Sessions};
use rutent_contracts::core_internal::{
    Decision, PermissionChange, ReadAnswer, ReadCheck, ReadOutcome, RefusalClass, SessionAnswer,
    SessionOutcome, SessionRequest, What,
};

use crate::engine::{UNREACHABLE, Unbuilt};

/// Until this row builds it: no session is opened.
impl<L: Log> Sessions for Unbuilt<L> {
    fn open_session(&mut self, request: &SessionRequest) -> Result<SessionAnswer, AppendFailure> {
        request.check().map_err(AppendFailure::Refused)?;
        let outcome = SessionOutcome::NotKnown { cause: UNREACHABLE };
        let what = What::OpenSession {
            request: request.clone(),
            opened: None,
        };
        let receipt = self.record(&request.requester, what, &request.at, outcome.outcome())?;
        Ok(SessionAnswer {
            request: request.request.clone(),
            outcome,
            receipt,
        })
    }
}

/// Until this row builds it: every read is refused, and recorded, and no permission is granted or revoked.
impl<L: Log> Reads for Unbuilt<L> {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        let outcome = ReadOutcome::Refused {
            class: RefusalClass::NotPermitted,
        };
        let what = What::CheckRead {
            check: check.clone(),
        };
        let receipt = self.record(&check.reader, what, &check.at, outcome.outcome())?;
        Ok(ReadAnswer {
            request: check.request.clone(),
            outcome,
            receipt,
        })
    }

    fn grant_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        let what = |decision| What::GrantPermission {
            change: change.clone(),
            decision,
        };
        self.not_known(&change.request, &change.by, what, &change.at)
    }

    fn revoke_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        let what = |decision| What::RevokePermission {
            change: change.clone(),
            decision,
        };
        self.not_known(&change.request, &change.by, what, &change.at)
    }
}
