//! A release's lifecycle actions through policy (`T-020-49`): `promote`, `roll_back`, the five and the platform's
//! `confirm` ask policy before they read anything of the platform's releases, so a refused caller is told the same
//! whatever is there and its attempt is recorded (`F-025`'s L6); and each is carried out on a permitted effect alone,
//! held to what the platform holds.

use std::cell::Cell;
use std::collections::BTreeSet;

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, CatalogueChange, Decision, DecisionRequest, Effect, Entry,
    Outcome, Receipt, RefusalClass, Role, What,
};
use rutent_contracts::module_interface::{Digest, Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{Operation, PublicKey, Statement};
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_core_orch::rounds::Fresh;
use rutent_core_orch::segments::lifecycle::{
    Changed, ReleaseAsked, ReleaseChange, ReleaseRefused, Releases, Waiting,
};
use rutent_core_orch::segments::roll_back::{RollBackAsked, RollBackRefused, RollsBack};
use rutent_core_orch::segments::{Answered, Modules, PromoteRefused, Promotion, Segments};
use rutent_core_orch::snapshot::{Clock, Reading};
use rutent_prov::catalogue::Instant;
use rutent_prov::module::lifecycle::{LifecycleRefusal, SegmentRecord};
use rutent_prov::module::types::Events;
use rutent_prov::proxy::RouteNotNamed;
use serde_json::json;

const KEY: &str = "PT4_QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1w";

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

fn digest() -> Digest {
    serde_json::from_value(json!({"algorithm": "sha-256", "value": "a".repeat(64)})).unwrap()
}

struct TestClock(Cell<Reading>);

impl Clock for TestClock {
    fn now(&self) -> Reading {
        self.0.get()
    }
}

fn clock() -> TestClock {
    TestClock(Cell::new(Reading::at(1_794_200_000)))
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
struct KeptLog(Vec<Entry>);

impl Log for KeptLog {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        self.0.push(entry.entry().clone());
        Ok(receipt(self.0.len() - 1))
    }
}

/// Policy answering every request as set, remembering each attempt: the decision's entry policy records.
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

/// A platform's releases: a staged version, a waiting release and a held set, each counted when read, and every
/// lifecycle action carried out recorded with who carried it out.
#[derive(Default)]
struct Platform {
    staged: Option<String>,
    waiting: Option<Waiting>,
    held: Option<BTreeSet<String>>,
    refuses: Option<LifecycleRefusal>,
    reads: Cell<usize>,
    done: Vec<(&'static str, String)>,
}

impl Platform {
    /// A platform holding something for every action: a staged 2.2.0, a waiting 2.3.0 and a set held at 2.1.0.
    fn holding() -> Self {
        Platform {
            staged: Some("2.2.0".into()),
            waiting: Some(Waiting {
                version: "2.3.0".into(),
                digest: digest(),
                statement_key: PublicKey::new(KEY).unwrap(),
            }),
            held: Some(BTreeSet::from(["2.1.0".to_owned()])),
            ..Platform::default()
        }
    }

    fn carried(&mut self, what: &'static str, by: &Id) -> Result<(), LifecycleRefusal> {
        if let Some(refusal) = self.refuses {
            return Err(refusal);
        }
        self.done.push((what, by.to_string()));
        Ok(())
    }
}

impl Releases for Platform {
    fn waiting(&self, _: &Id) -> Option<Waiting> {
        self.reads.set(self.reads.get() + 1);
        self.waiting.clone()
    }
    fn held(&self, _: &Id) -> Option<BTreeSet<String>> {
        self.reads.set(self.reads.get() + 1);
        self.held.clone()
    }
    fn accept(
        &mut self,
        _: &Effect,
        _: &Id,
        _: &Digest,
        by: &Id,
        _: Instant,
    ) -> Result<(), LifecycleRefusal> {
        self.carried("accept", by)
    }
    fn withdraw(
        &mut self,
        _: &Effect,
        _: &Id,
        _: &Digest,
        by: &Id,
        _: Instant,
    ) -> Result<(), LifecycleRefusal> {
        self.carried("withdraw", by)
    }
    fn revoke(
        &mut self,
        _: &Effect,
        _: &Id,
        _: &Id,
        by: &Id,
        _: Instant,
    ) -> Result<Option<String>, LifecycleRefusal> {
        self.carried("revoke", by).map(|()| None)
    }
    fn declare_exploited(
        &mut self,
        _: &Effect,
        _: &Id,
        _: &Id,
        by: &Id,
        _: Instant,
    ) -> Result<Option<String>, LifecycleRefusal> {
        self.carried("declare_exploited", by).map(|()| None)
    }
    fn mark_stale(
        &mut self,
        _: &Effect,
        _: &Id,
        _: &Id,
        by: &Id,
        _: Instant,
    ) -> Result<usize, LifecycleRefusal> {
        self.carried("mark_stale", by).map(|()| 1)
    }
    fn confirm(&mut self, _: &Effect, _: &Id, by: &Id, _: Instant) -> Result<(), LifecycleRefusal> {
        self.carried("confirm", by)?;
        self.held = None;
        Ok(())
    }
}

impl RollsBack for Platform {
    fn staged(&self, _: &Id) -> Option<String> {
        self.reads.set(self.reads.get() + 1);
        self.staged.clone()
    }
    fn roll_back(
        &mut self,
        _: &Effect,
        _: &Id,
        by: &Id,
        _: Instant,
    ) -> Result<String, LifecycleRefusal> {
        self.carried("roll_back", by)?;
        Ok(self.staged.take().unwrap_or_default())
    }
}

impl Modules for Platform {
    fn begin_segment(&mut self, _: &Id, _: &Identifier) -> Result<String, PlatformRefusal> {
        unreachable!("no segment here")
    }
    fn name_calls(
        &mut self,
        _: &Identifier,
        _: &DecisionRecord,
        _: (usize, usize),
        _: &Identifier,
    ) -> Result<usize, RouteNotNamed> {
        unreachable!("no segment here")
    }
    fn carry_out(&mut self, _: &Effect, _: &Id, _: &Statement<Operation>) -> Answered {
        unreachable!("no segment here")
    }
    fn take_events(&mut self) -> Events {
        Events::default()
    }
    fn end_segment(&mut self, _: &Identifier) -> Option<SegmentRecord> {
        None
    }
    fn end_calls(&mut self, _: &Identifier) {}
    fn staged(&self, provider: &Id) -> Option<String> {
        RollsBack::staged(self, provider)
    }
    fn promote(
        &mut self,
        _: &Effect,
        _: &Id,
        by: &Id,
        _: Instant,
    ) -> Result<String, LifecycleRefusal> {
        self.carried("promote", by)?;
        Ok(self.staged.take().unwrap_or_default())
    }
}

/// What one asking of `action` at `bridge-pay` comes to, over `platform`, `policy` and `log`: the answer as text, so
/// the seven actions' answers compare.
fn ask(action: &str, platform: &mut Platform, policy: &mut Answering, log: &mut KeptLog) -> String {
    let (provider, caller, core) = (id("bridge-pay"), operator(), core());
    let mut segments = Segments::new();
    let (staged, waiting, held) = (id("2.2.0"), id("2.3.0"), [id("2.1.0")]);
    let key = PublicKey::new(KEY).unwrap();
    let release = |change| ReleaseAsked {
        provider: &provider,
        change,
        caller: &caller,
        core: &core,
    };
    let text = |r: Result<String, String>| match r {
        Ok(_) => "carried out".to_owned(),
        Err(e) => e,
    };
    match action {
        "promote" => text(
            segments
                .promote(
                    log,
                    policy,
                    platform,
                    &clock(),
                    &mut Ids::default(),
                    &Promotion {
                        provider: &provider,
                        version: &staged,
                        caller: &caller,
                        core: &core,
                    },
                )
                .map_err(|e| match e {
                    PromoteRefused::NotPermitted(a) => format!("not permitted: {a:?}"),
                    other => format!("{other:?}"),
                }),
        ),
        "roll_back" => text(
            segments
                .roll_back(
                    log,
                    policy,
                    platform,
                    &clock(),
                    &mut Ids::default(),
                    &RollBackAsked {
                        provider: &provider,
                        version: &staged,
                        caller: &caller,
                        core: &core,
                    },
                )
                .map_err(|e| match e {
                    RollBackRefused::NotPermitted(a) => format!("not permitted: {a:?}"),
                    other => format!("{other:?}"),
                }),
        ),
        other => {
            let change = match other {
                "accept" => ReleaseChange::Accept {
                    version: &waiting,
                    statement_key: &key,
                },
                "withdraw" => ReleaseChange::Withdraw { version: &waiting },
                "revoke" => ReleaseChange::Revoke { version: &staged },
                "declare_exploited" => ReleaseChange::DeclareExploited { version: &staged },
                "mark_stale" => ReleaseChange::MarkStale { version: &staged },
                "confirm" => ReleaseChange::Confirm { versions: &held },
                _ => unreachable!(),
            };
            text(
                segments
                    .change_release(
                        log,
                        policy,
                        platform,
                        &clock(),
                        &mut Ids::default(),
                        &release(change),
                    )
                    .map(|c| format!("{c:?}"))
                    .map_err(|e| match e {
                        ReleaseRefused::NotPermitted(a) => format!("not permitted: {a:?}"),
                        other => format!("{other:?}"),
                    }),
            )
        }
    }
}

const ACTIONS: [&str; 8] = [
    "promote",
    "roll_back",
    "accept",
    "withdraw",
    "revoke",
    "declare_exploited",
    "mark_stale",
    "confirm",
];

/// `F-025`'s L6 (`T-020-49`): `promote`, `roll_back`, the five and `confirm` ask policy before they read anything of the
/// platform's releases. A refused caller is told the same, not permitted, whether the platform holds something for the
/// action (staged, waiting or held) or nothing, nothing of the platform is read and no effect is recorded; and each
/// attempt is asked of policy, which records it as its decision's entry, naming the action and the caller. A permitted
/// caller learns the difference.
// verifies: REQ-AUZ-1, REQ-MDL-4
#[test]
fn refused_caller_is_told_the_same_whether_or_not_anything_is_staged_and_its_attempt_is_recorded() {
    let refused = Answer::Refuse {
        class: RefusalClass::NotPermitted,
    };
    for action in ACTIONS {
        let mut answers = Vec::new();
        for mut platform in [Platform::holding(), Platform::default()] {
            let (mut policy, mut log) = (Answering::with(refused), KeptLog::default());
            answers.push(ask(action, &mut platform, &mut policy, &mut log));
            assert_eq!(
                policy.asked.len(),
                1,
                "{action}: the attempt is asked, and recorded"
            );
            assert_eq!(policy.asked[0].caller, operator(), "{action}");
            assert!(
                matches!(policy.asked[0].action, Action::ChangeCatalogue { provider: ref p, .. } if p.as_str() == "bridge-pay"),
                "{action}"
            );
            assert_eq!(
                platform.reads.get(),
                0,
                "{action}: nothing of the platform is read"
            );
            assert!(platform.done.is_empty(), "{action}");
            assert!(log.0.is_empty(), "{action}: no effect");
        }
        assert_eq!(answers[0], answers[1], "{action}: told the same");
        assert_eq!(
            answers[0],
            format!("not permitted: {refused:?}"),
            "{action}"
        );

        // Permitted, the caller learns whether anything is there (revoking, declaring and marking stale read nothing:
        // the platform holds them to the version named).
        let (mut policy, mut log) = (Answering::with(Answer::Permit {}), KeptLog::default());
        let there = ask(action, &mut Platform::holding(), &mut policy, &mut log);
        let (mut policy, mut log) = (Answering::with(Answer::Permit {}), KeptLog::default());
        let nothing = ask(action, &mut Platform::default(), &mut policy, &mut log);
        assert_eq!(there, "carried out", "{action}");
        if !matches!(action, "revoke" | "declare_exploited" | "mark_stale") {
            assert_ne!(there, nothing, "{action}");
            assert!(log.0.is_empty(), "{action}: nothing there, no effect");
        }
    }
}

/// The platform's `confirm` (`T-013-11`) is carried out only on a permitted decision (`REQ-AUZ-6`, `REQ-AUZ-1`): asked
/// of policy as `confirm_releases` naming the provider and the versions; refused, nothing is confirmed and no effect
/// recorded; permitted, the versions are held to the ones the platform holds the provider's releases at (other versions,
/// or nothing held, confirm nothing and record no effect), then the effect is recorded, the platform confirms by the
/// decision's caller, and the result is recorded done, or failed where the platform refuses.
// verifies: REQ-AUZ-6, REQ-AUZ-1
#[test]
fn operator_confirms_a_providers_releases_only_on_a_permitted_decision() {
    let (provider, caller, core) = (id("bridge-pay"), operator(), core());
    let confirm =
        |platform: &mut Platform, policy: &mut Answering, log: &mut KeptLog, versions: &[Id]| {
            Segments::new().change_release(
                log,
                policy,
                platform,
                &clock(),
                &mut Ids::default(),
                &ReleaseAsked {
                    provider: &provider,
                    change: ReleaseChange::Confirm { versions },
                    caller: &caller,
                    core: &core,
                },
            )
        };
    let held = [id("2.1.0")];

    let refused = Answer::Refuse {
        class: RefusalClass::NotPermitted,
    };
    let (mut platform, mut policy, mut log) = (
        Platform::holding(),
        Answering::with(refused),
        KeptLog::default(),
    );
    assert_eq!(
        confirm(&mut platform, &mut policy, &mut log, &held),
        Err(ReleaseRefused::NotPermitted(refused))
    );
    assert!(platform.done.is_empty() && log.0.is_empty());
    assert!(platform.held.is_some(), "still held");
    assert!(matches!(
        &policy.asked[0].action,
        Action::ChangeCatalogue { change: CatalogueChange::ConfirmReleases, versions, version: None, .. }
            if versions == &held
    ));

    let (mut policy, mut log) = (Answering::with(Answer::Permit {}), KeptLog::default());
    assert_eq!(
        confirm(&mut platform, &mut policy, &mut log, &[id("2.0.0")]),
        Err(ReleaseRefused::NotWhatItNames {
            holds: vec!["2.1.0".into()]
        })
    );
    assert!(
        platform.done.is_empty() && log.0.is_empty(),
        "other versions confirm nothing"
    );

    assert_eq!(
        confirm(&mut platform, &mut policy, &mut log, &held),
        Ok(Changed::Confirmed)
    );
    assert_eq!(platform.done, vec![("confirm", "ops-lead".to_owned())]);
    let whats: Vec<(&What, Outcome)> = log
        .0
        .iter()
        .map(|e| match e {
            Entry::Verified(v) => (&v.what, v.outcome),
            Entry::Stated(_) => panic!("verified"),
        })
        .collect();
    assert_eq!(whats.len(), 2, "the effect, then its result");
    assert!(matches!(
        whats[0],
        (
            What::Effect {
                action: Action::ChangeCatalogue {
                    change: CatalogueChange::ConfirmReleases,
                    ..
                },
                ..
            },
            Outcome::ToTakeEffect {}
        )
    ));
    assert!(matches!(whats[1], (What::Result { .. }, Outcome::Done {})));

    // Nothing held any more: a permitted caller is told there is nothing to confirm, and nothing is recorded.
    let mut log = KeptLog::default();
    assert_eq!(
        confirm(&mut platform, &mut policy, &mut log, &held),
        Err(ReleaseRefused::NothingToDo)
    );
    assert!(log.0.is_empty());

    // The platform's refusal records the result failed.
    let mut platform = Platform {
        refuses: Some(LifecycleRefusal::Unrecorded),
        ..Platform::holding()
    };
    let mut log = KeptLog::default();
    assert_eq!(
        confirm(&mut platform, &mut policy, &mut log, &held),
        Err(ReleaseRefused::Lifecycle(LifecycleRefusal::Unrecorded))
    );
    assert!(matches!(log.0.last(), Some(Entry::Verified(v)) if v.outcome == Outcome::Failed {}));
}

/// A decision that only restricts what runs, which the platform took but couldn't write to its record, took effect:
/// its result is `done`, never `failed`, so the log never trusts a release declared exploited again, and the answer
/// says it isn't durable on the platform (`T-013-10`).
// verifies: REQ-MDL-20
#[test]
fn a_restriction_the_platform_took_unrecorded_is_recorded_done() {
    let (provider, caller, core, version) = (id("bridge-pay"), operator(), core(), id("2.2.0"));
    let mut platform = Platform {
        refuses: Some(LifecycleRefusal::TakenUnrecorded),
        ..Platform::holding()
    };
    let (mut policy, mut log) = (Answering::with(Answer::Permit {}), KeptLog::default());
    let changed = Segments::new().change_release(
        &mut log,
        &mut policy,
        &mut platform,
        &clock(),
        &mut Ids::default(),
        &ReleaseAsked {
            provider: &provider,
            change: ReleaseChange::DeclareExploited { version: &version },
            caller: &caller,
            core: &core,
        },
    );
    assert_eq!(changed, Ok(Changed::TakenUnrecorded));
    assert!(matches!(log.0.last(), Some(Entry::Verified(v)) if v.outcome == Outcome::Done {}));
}
