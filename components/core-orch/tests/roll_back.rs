//! Rolling a staging back is carried out on a permitted decision's effect and on nothing else (`T-021-12`, ADR-0014):
//! Core asks the decision, records the effect, calls the platform and records the result, and an effect whose action
//! isn't a roll back of the staged version asks nothing of the platform.

use std::cell::Cell;

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, CatalogueChange, Decision, DecisionRequest, Effect, Entry,
    NotKnownCause, Outcome, Receipt, RefusalClass, Role, What,
};
use rutent_contracts::router_api::Id;
use rutent_core_orch::rounds::Fresh;
use rutent_core_orch::segments::Segments;
use rutent_core_orch::segments::roll_back::{RollBackAsked, RollBackRefused, RollsBack};
use rutent_core_orch::snapshot::{Clock, Reading};
use rutent_prov::catalogue::Instant;
use rutent_prov::module::lifecycle::LifecycleRefusal;
use serde_json::json;

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn receipt(index: usize) -> Receipt {
    serde_json::from_value(json!({
        "log": "log-1",
        "index": index,
        "entry": { "algorithm": "sha-256", "value": format!("{index:064x}") },
    }))
    .unwrap()
}

struct TestClock(Cell<Reading>);

impl Clock for TestClock {
    fn now(&self) -> Reading {
        self.0.get()
    }
}

#[derive(Default)]
struct Ids(u64);

impl Fresh for Ids {
    fn id(&mut self) -> Id {
        self.0 += 1;
        id(&format!("id-{:04}", self.0))
    }
}

#[derive(Default)]
struct KeptLog {
    entries: Vec<Entry>,
    /// Appends refused from this one on, where set.
    refuse_from: Option<usize>,
}

impl Log for KeptLog {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        if self.refuse_from.is_some_and(|n| self.entries.len() >= n) {
            return Err(AppendFailure::Unavailable);
        }
        self.entries.push(entry.entry().clone());
        Ok(receipt(self.entries.len() - 1))
    }
}

/// Policy answering every request as set, remembering what it was asked.
struct Answering {
    answer: Answer,
    asked: Vec<DecisionRequest>,
}

impl Answering {
    fn with(answer: Answer) -> Self {
        Self {
            answer,
            asked: Vec::new(),
        }
    }
}

impl Decisions for Answering {
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure> {
        self.asked.push(request.clone());
        Ok(Decision {
            request: request.request.clone(),
            decision: id(&format!("dec-{}", self.asked.len())),
            outcome: self.answer,
            receipt: receipt(10_000 + self.asked.len()),
        })
    }
}

/// A platform whose staging is one version, which a roll back ends unless it refuses.
struct Staging {
    staged: Option<String>,
    refuses: Option<LifecycleRefusal>,
    ended: Vec<(String, String)>,
}

impl Staging {
    fn of(version: &str) -> Self {
        Self {
            staged: Some(version.to_owned()),
            refuses: None,
            ended: Vec::new(),
        }
    }
}

impl RollsBack for Staging {
    fn staged(&self, _: &Id) -> Option<String> {
        self.staged.clone()
    }

    fn roll_back(
        &mut self,
        _: &Effect,
        provider: &Id,
        by: &Id,
        _: Instant,
    ) -> Result<String, LifecycleRefusal> {
        if let Some(refusal) = self.refuses {
            return Err(refusal);
        }
        self.ended.push((provider.to_string(), by.to_string()));
        Ok(self.staged.take().unwrap_or_default())
    }
}

fn core() -> Actor {
    Actor {
        party: id("rutent"),
        acts: id("rutent-core"),
        role: Role::Core,
        kind: ActorKind::Component,
    }
}

fn operator() -> Actor {
    Actor {
        party: id("operator"),
        acts: id("ops-lead"),
        role: Role::Operator,
        kind: ActorKind::Person,
    }
}

fn clock() -> TestClock {
    TestClock(Cell::new(Reading::at(1_794_200_000)))
}

fn change(kind: CatalogueChange, version: &str) -> Action {
    Action::ChangeCatalogue {
        change: kind,
        provider: id("bridge-pay"),
        version: Some(id(version)),
        tuple: None,
        class: None,
        advertising_keys: Vec::new(),
        statement_key: None,
        setting: None,
        versions: Vec::new(),
        public_network: None,
    }
}

/// What one roll back of `version` at `bridge-pay` ends in, over `modules`, `policy` and `log`.
fn rolled_back(
    segments: &mut Segments,
    log: &mut KeptLog,
    policy: &mut Answering,
    modules: &mut Staging,
    version: &str,
) -> Result<String, RollBackRefused> {
    let (provider, version) = (id("bridge-pay"), id(version));
    let (caller, core) = (operator(), core());
    segments.roll_back(
        log,
        policy,
        modules,
        &clock(),
        &mut Ids::default(),
        &RollBackAsked {
            provider: &provider,
            version: &version,
            caller: &caller,
            core: &core,
        },
    )
}

/// A roll back of the staged version is asked of policy as a `roll_back` of that version by the operator's login;
/// on a permit Core records the effect, ends the staging by that login, and records the result done.
// verifies: REQ-AUZ-1, REQ-AUZ-6
#[test]
fn permitted_roll_back_ends_the_staging_and_records_its_effect_and_result() {
    let (mut log, mut policy) = (KeptLog::default(), Answering::with(Answer::Permit {}));
    let mut modules = Staging::of("2.2.0");
    let ended = rolled_back(
        &mut Segments::new(),
        &mut log,
        &mut policy,
        &mut modules,
        "2.2.0",
    );
    assert_eq!(ended, Ok("2.2.0".to_owned()));
    assert_eq!(modules.staged, None);
    assert_eq!(
        modules.ended,
        vec![("bridge-pay".to_owned(), "ops-lead".to_owned())]
    );
    assert_eq!(policy.asked.len(), 1);
    assert_eq!(policy.asked[0].caller, operator());
    assert_eq!(
        policy.asked[0].action,
        change(CatalogueChange::RollBack, "2.2.0")
    );
    let whats: Vec<(&What, &Outcome)> = log
        .entries
        .iter()
        .map(|e| match e {
            Entry::Verified(v) => (&v.what, &v.outcome),
            other => panic!("a verified entry: {other:?}"),
        })
        .collect();
    assert_eq!(whats.len(), 2, "the effect, then its result");
    assert!(
        matches!(whats[0], (What::Effect { action, .. }, Outcome::ToTakeEffect {})
        if *action == change(CatalogueChange::RollBack, "2.2.0"))
    );
    assert!(matches!(whats[1], (What::Result { .. }, Outcome::Done {})));
}

/// A decision that refuses, or isn't known, ends nothing: no effect is recorded and the platform is not asked.
// verifies: REQ-AUZ-1
#[test]
fn refused_or_unknown_roll_back_ends_nothing() {
    for answer in [
        Answer::Refuse {
            class: RefusalClass::NotPermitted,
        },
        Answer::NotKnown {
            cause: NotKnownCause::VerifierUnreachable,
        },
    ] {
        let (mut log, mut policy) = (KeptLog::default(), Answering::with(answer));
        let mut modules = Staging::of("2.2.0");
        let ended = rolled_back(
            &mut Segments::new(),
            &mut log,
            &mut policy,
            &mut modules,
            "2.2.0",
        );
        assert_eq!(ended, Err(RollBackRefused::NotPermitted(answer)));
        assert_eq!(modules.staged.as_deref(), Some("2.2.0"));
        assert!(modules.ended.is_empty());
        assert!(log.entries.is_empty(), "no effect is recorded");
    }
}

/// A log that can't take the effect leaves the staging as it was; one that fails after it leaves the platform's
/// refusal as the result. A version other than the staged one, or none staged, records no effect: policy is asked
/// first, before the staging is read (`F-025`'s L6, `T-020-49`), so the attempt is its decision's entry.
// verifies: REQ-AUZ-1
#[test]
fn roll_back_without_its_effect_or_its_staged_version_ends_nothing() {
    let (mut log, mut policy) = (
        KeptLog {
            refuse_from: Some(0),
            ..KeptLog::default()
        },
        Answering::with(Answer::Permit {}),
    );
    let mut modules = Staging::of("2.2.0");
    let ended = rolled_back(
        &mut Segments::new(),
        &mut log,
        &mut policy,
        &mut modules,
        "2.2.0",
    );
    assert_eq!(
        ended,
        Err(RollBackRefused::Append(AppendFailure::Unavailable))
    );
    assert_eq!(
        modules.staged.as_deref(),
        Some("2.2.0"),
        "no effect, no roll back"
    );

    let (mut log, mut policy) = (KeptLog::default(), Answering::with(Answer::Permit {}));
    let other = rolled_back(
        &mut Segments::new(),
        &mut log,
        &mut policy,
        &mut modules,
        "2.1.0",
    );
    assert_eq!(
        other,
        Err(RollBackRefused::NotTheStagedVersion {
            staged: "2.2.0".into()
        })
    );
    let mut none = Staging {
        staged: None,
        ..Staging::of("x")
    };
    assert_eq!(
        rolled_back(
            &mut Segments::new(),
            &mut log,
            &mut policy,
            &mut none,
            "2.2.0"
        ),
        Err(RollBackRefused::NothingStaged)
    );
    assert_eq!(
        policy.asked.len(),
        2,
        "each attempt is asked of policy first"
    );
    assert!(log.entries.is_empty(), "no effect is recorded");
}

/// The platform's refusal ends nothing and the effect's result is `failed`, never `done`.
// verifies: REQ-AUZ-6
#[test]
fn platform_refusal_records_the_result_failed() {
    let (mut log, mut policy) = (KeptLog::default(), Answering::with(Answer::Permit {}));
    let mut modules = Staging::of("2.2.0");
    modules.refuses = Some(LifecycleRefusal::NotStaged);
    let ended = rolled_back(
        &mut Segments::new(),
        &mut log,
        &mut policy,
        &mut modules,
        "2.2.0",
    );
    assert_eq!(
        ended,
        Err(RollBackRefused::Lifecycle(LifecycleRefusal::NotStaged))
    );
    assert_eq!(modules.staged.as_deref(), Some("2.2.0"));
    let Some(Entry::Verified(last)) = log.entries.last() else {
        panic!("a result is recorded");
    };
    assert!(matches!(
        (&last.what, &last.outcome),
        (What::Result { .. }, Outcome::Failed {})
    ));
}
