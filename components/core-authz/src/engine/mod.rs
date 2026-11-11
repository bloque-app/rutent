//! The policy engine: every action that moves value, posts a route or changes the catalogue reaches one decision, with
//! no path around it, and each decision is appended to the log before it answers.
//!
//! Declared by `T-020-01`; `T-021-01` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::calls::{AppendFailure, DecisionEntry, Decisions, PolicyLog};
use rutent_contracts::core_internal::{
    Actor, Answer, Decision, DecisionRequest, NotKnownCause, Outcome, Receipt, TimeSource,
    VerifiedEntry, VerifiedKind, What, When,
};
use rutent_contracts::router_api::{Id, Timestamp};

/// Policy before its rows build it: it permits nothing. It holds policy's handle on the log, [`PolicyLog`], which
/// the assembly hands to it alone. Each call is appended to the log first, as every answer
/// must be, and answered not known where its answer can say so, refused or over its bound where it can't
/// (`REQ-AUZ-5`). Where the log takes no entry it answers nothing at all. Each policy trait's stub sits in the
/// module of the row that owns that trait; this one, and its [`Decisions`], are `T-021-01`'s.
#[derive(Debug, Clone, Default)]
pub struct Unbuilt<L> {
    pub(crate) log: L,
}

/// Why the unbuilt engine knows nothing.
pub(crate) const UNREACHABLE: NotKnownCause = NotKnownCause::PolicyUnreachable;

impl<L: PolicyLog> Unbuilt<L> {
    /// Policy that appends to `log`.
    pub fn new(log: L) -> Self {
        Unbuilt { log }
    }

    /// The log it appends to.
    pub fn log(&self) -> &L {
        &self.log
    }

    /// Appends the verified entry of one answer, timed by Core's clock.
    pub(crate) fn record(
        &mut self,
        who: &Actor,
        what: What,
        at: &Timestamp,
        outcome: Outcome,
    ) -> Result<Receipt, AppendFailure> {
        let entry = VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: who.clone(),
            what,
            when: When {
                at: at.clone(),
                source: TimeSource::CoreClock,
            },
            outcome,
        };
        entry.check().map_err(AppendFailure::Refused)?;
        let entry = DecisionEntry::try_from(entry).map_err(AppendFailure::Refused)?;
        self.log.append_decision(&entry)
    }

    /// Answers not known to a decision-shaped call, its decision id its request's.
    pub(crate) fn not_known(
        &mut self,
        request: &Id,
        who: &Actor,
        what: impl FnOnce(Id) -> What,
        at: &Timestamp,
    ) -> Result<Decision, AppendFailure> {
        let outcome = Answer::NotKnown { cause: UNREACHABLE };
        let receipt = self.record(who, what(request.clone()), at, outcome.outcome())?;
        Ok(Decision {
            request: request.clone(),
            decision: request.clone(),
            outcome,
            receipt,
        })
    }
}

impl<L: PolicyLog> Decisions for Unbuilt<L> {
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure> {
        request.check().map_err(AppendFailure::Refused)?;
        let what = |decision| What::Decide {
            request: request.clone(),
            decision,
        };
        self.not_known(&request.request, &request.caller, what, &request.at)
    }
}

#[cfg(test)]
mod tests {
    /// A valid fixture of the contract, read as whatever type the call site wants.
    macro_rules! fixture {
        ($name:expr $(,)?) => {{
            let path = format!(
                "{}/../../contracts/core-internal/fixtures/valid/{}.json",
                env!("CARGO_MANIFEST_DIR"),
                $name
            );
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
        }};
    }

    use super::*;
    use rutent_contracts::core_internal::calls::{
        Authorisations, Charges, PersonQuestions, Reads, Sessions, StuckOrders,
    };
    use rutent_contracts::core_internal::{
        ChargeOutcome, PersonOutcome, ReadOutcome, RefusalClass, SessionOutcome,
    };
    use rutent_contracts::core_internal::{Entry, PermissionChange};

    /// A log that takes every entry, so the stub's answers can be seen.
    #[derive(Default)]
    struct Taking(Vec<Entry>);

    impl PolicyLog for Taking {
        fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
            self.0.push(Entry::Verified(entry.entry().clone()));
            Ok(fixture!("receipt-of-an-appended-entry"))
        }
    }

    /// A log that takes nothing.
    struct Refusing;

    impl PolicyLog for Refusing {
        fn append_decision(&mut self, _: &DecisionEntry) -> Result<Receipt, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
    }

    /// The outcome of the one entry the log took since the last look, which must pass its checks.
    fn last(policy: &mut Unbuilt<Taking>) -> Outcome {
        let entries: [Entry; 1] = std::mem::take(&mut policy.log.0).try_into().unwrap();
        let [Entry::Verified(entry)] = entries else {
            panic!("one verified entry");
        };
        assert_eq!(entry.check(), Ok(()));
        entry.outcome
    }

    /// The unbuilt engine permits nothing on any call, records each answer first, and answers nothing when the log
    /// takes no entry.
    // verifies: REQ-AUZ-1, REQ-AUZ-5, INV-7
    #[test]
    fn the_unbuilt_engine_permits_nothing_on_any_call_and_records_first() {
        let not_known = Outcome::NotKnown { cause: UNREACHABLE };
        let refused = |class| Outcome::Refused { class };
        let mut policy = Unbuilt::new(Taking::default());

        let decision = policy
            .decide(&fixture!("decision-request-to-move-value"))
            .unwrap();
        assert!(decision.permit().is_none());
        assert_eq!(last(&mut policy), not_known);
        let decision = policy
            .authorise(&fixture!("authorisation-of-one-order-with-a-floor"))
            .unwrap();
        assert!(decision.permit().is_none());
        assert_eq!(last(&mut policy), not_known);
        let decision = policy
            .decide_stuck(&fixture!("stuck-decide-naming-its-evidence"))
            .unwrap();
        assert!(decision.permit().is_none());
        assert_eq!(last(&mut policy), not_known);
        let decision = policy
            .move_stuck(&fixture!("stuck-move-carrying-out-a-decision"))
            .unwrap();
        assert!(decision.permit().is_none());
        assert_eq!(last(&mut policy), not_known);

        let session = policy
            .open_session(&fixture!("session-request-of-a-delegate"))
            .unwrap();
        assert!(matches!(session.outcome, SessionOutcome::NotKnown { .. }));
        assert_eq!(last(&mut policy), not_known);
        let read = policy
            .check_read(&fixture!("read-check-by-a-provider-of-its-segment"))
            .unwrap();
        assert!(matches!(read.outcome, ReadOutcome::Refused { .. }));
        assert_eq!(last(&mut policy), refused(RefusalClass::NotPermitted));
        let grant: PermissionChange = fixture!("permission-change-granting-a-regulator-the-trail");
        let granted = policy.grant_permission(&grant).unwrap();
        assert!(granted.permit().is_none());
        assert_eq!(last(&mut policy), not_known);
        let revoked = policy.revoke_permission(&grant).unwrap();
        assert!(revoked.permit().is_none());
        assert_eq!(last(&mut policy), not_known);
        let charge = policy
            .charge(&fixture!("charge-of-fanned-out-work"))
            .unwrap();
        assert!(matches!(charge.outcome, ChargeOutcome::Over { .. }));
        assert_eq!(last(&mut policy), refused(RefusalClass::OverBound));
        let answer = policy
            .ask_about_person(&fixture!(
                "person-question-on-the-persons-authority-first-asked",
            ))
            .unwrap();
        assert!(matches!(answer.outcome, PersonOutcome::NotKnown { .. }));
        assert_eq!(last(&mut policy), not_known);

        let mut policy = Unbuilt::new(Refusing);
        assert_eq!(
            policy.decide(&fixture!("decision-request-to-move-value")),
            Err(AppendFailure::Unavailable)
        );
        assert_eq!(
            policy.charge(&fixture!("charge-of-fanned-out-work")),
            Err(AppendFailure::Unavailable)
        );
    }
}
