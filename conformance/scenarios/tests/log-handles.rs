//! The assembly hands policy's handle on the log to the policy engine alone (`T-020-01`).

use rutent_contracts::core_internal::calls::{
    ATTRIBUTED_BY_ITS_WRITER, AppendFailure, DECISION_ON_THE_GENERAL_HANDLE, DecisionEntry,
    Decisions, GeneralEntry, Log, PolicyLog,
};
use rutent_contracts::core_internal::{
    DecisionRequest, Entry, Receipt, VerifiedEntry, Violation, What,
};
use rutent_scenarios::{Assembly, GeneralHandle, PolicyHandle};

fn valid(name: &str) -> String {
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/valid/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap()
}

/// A log that takes every entry and keeps which handle it came through.
#[derive(Default)]
struct Recording {
    general: Vec<Entry>,
    decisions: Vec<VerifiedEntry>,
}

impl Log for Recording {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        self.general.push(entry.entry().clone());
        Ok(serde_json::from_str(&valid("receipt-of-an-appended-entry")).unwrap())
    }
}

impl PolicyLog for Recording {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        self.decisions.push(entry.entry().clone());
        Ok(serde_json::from_str(&valid("receipt-of-an-appended-entry")).unwrap())
    }
}

/// What policy reads of the log (`T-021-04`): its decisions, the only entries it reads.
impl rutent_scenarios::Entries for Recording {
    fn entries(&self) -> Option<Vec<Entry>> {
        Some(
            self.decisions
                .iter()
                .cloned()
                .map(Entry::Verified)
                .collect(),
        )
    }
}

/// Names an item only for a type that doesn't implement [`PolicyLog`]: for one that does, the two impls make the
/// name ambiguous and the test doesn't build.
trait NotPolicyLog<A> {
    fn holds() {}
}
impl<T: ?Sized> NotPolicyLog<()> for T {}
impl<T: ?Sized + PolicyLog> NotPolicyLog<u8> for T {}

/// The same for `Clone`: policy's handle isn't, so the one the engine holds is the only one.
trait NotClone<A> {
    fn holds() {}
}
impl<T: ?Sized> NotClone<()> for T {}
impl<T: Clone> NotClone<u8> for T {}

// verifies: REQ-AUZ-1, INV-7, DEC-2
#[test]
fn the_assembly_hands_policys_handle_to_the_policy_engine_alone() {
    let mut core = Assembly::new(Recording::default());

    // Policy's answer reaches the log through policy's handle.
    let request: DecisionRequest =
        serde_json::from_str(&valid("decision-request-to-move-value")).unwrap();
    let decision = core.policy.decide(&request).unwrap();
    assert_eq!(core.log().decisions.len(), 1);
    assert!(
        matches!(&core.log().decisions[0].what, What::Decide { decision: d, .. } if *d == decision.decision)
    );
    assert!(core.log().general.is_empty());

    // Orchestration's handle takes its records, and lands them on the general path.
    let mut orchestration: GeneralHandle<Recording> = core.orchestration_log();
    let record: GeneralEntry = serde_json::from_str(&valid("entry-verified")).unwrap();
    orchestration.append(&record).unwrap();
    assert_eq!(core.log().general, vec![record.entry().clone()]);
    assert_eq!(core.log().decisions.len(), 1);

    // It can't carry a decision: orchestration's handle is no policy handle, and its entry type holds no decision
    // kind, however it is built or read. The forged decision is the entry policy just wrote.
    <GeneralHandle<Recording> as NotPolicyLog<_>>::holds();
    <PolicyHandle<Recording> as NotClone<_>>::holds();
    let forged = Entry::Verified(core.log().decisions[0].clone());
    assert_eq!(
        GeneralEntry::try_from(forged.clone()),
        Err(Violation(DECISION_ON_THE_GENERAL_HANDLE))
    );
    let read = serde_json::from_str::<GeneralEntry>(&serde_json::to_string(&forged).unwrap());
    let error = read.unwrap_err().to_string();
    assert!(error.contains(DECISION_ON_THE_GENERAL_HANDLE), "{error}");
    assert_eq!(core.log().general.len(), 1);

    // Nor can it attribute a statement: a stated entry its writer calls verified is no general entry.
    let attributed = serde_json::from_str::<GeneralEntry>(&valid("stated-entry-of-a-quote"));
    let error = attributed.unwrap_err().to_string();
    assert!(error.contains(ATTRIBUTED_BY_ITS_WRITER), "{error}");
    let claim: GeneralEntry =
        serde_json::from_str(&valid("stated-entry-relayed-and-unverified")).unwrap();
    orchestration.append(&claim).unwrap();
    assert_eq!(core.log().general.len(), 2);
}

/// The assembly around the log before its row builds it: policy holds the handle and the log takes nothing.
// verifies: INV-7
#[test]
fn the_unbuilt_assembly_records_nothing_and_permits_nothing() {
    let mut core = Assembly::unbuilt();
    let request: DecisionRequest =
        serde_json::from_str(&valid("decision-request-to-move-value")).unwrap();
    assert_eq!(
        core.policy.decide(&request),
        Err(AppendFailure::Unavailable)
    );
    let record: GeneralEntry = serde_json::from_str(&valid("entry-verified")).unwrap();
    assert_eq!(
        core.orchestration_log().append(&record),
        Err(AppendFailure::Unavailable)
    );
}
