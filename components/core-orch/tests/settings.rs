//! The operator's settings through policy, rebuilt from the log (`T-020-49`): the operator's command asks policy for
//! core-internal's `set_setting`, records the effect before it applies and its result after, and a reopened Core
//! rebuilds every setting from the effects the log holds done.

use std::cell::Cell;

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, CatalogueChange, Count, Decision, DecisionRequest, Entry,
    NotKnownCause, OperatorSetting, Outcome, Receipt, RefusalClass, Role, What,
};
use rutent_contracts::router_api::{Id, PolicyBounds};
use rutent_core_orch::arrival::{Arrivals, DEFAULT_DEPOSIT_WAIT_SECONDS};
use rutent_core_orch::rounds::Fresh;
use rutent_core_orch::schedule::{Cadences, Job};
use rutent_core_orch::settings::{
    DEPOSIT_WAIT_CEILING_SECONDS, SettingAsked, SettingRefused, Settings, Targets,
};
use rutent_core_orch::snapshot::{Clock, Reading};
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

/// A log that keeps what it is handed, each with its receipt; refusing from the `n`th append, where set.
#[derive(Default)]
struct KeptLog {
    entries: Vec<(Receipt, Entry)>,
    refuse_from: Option<usize>,
}

impl rutent_core_orch::segments::lifecycle::Journal for KeptLog {
    fn history(&self) -> Vec<(Receipt, Entry)> {
        self.entries.clone()
    }
}

impl Log for KeptLog {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        if self.refuse_from.is_some_and(|n| self.entries.len() >= n) {
            return Err(AppendFailure::Refused(
                rutent_contracts::core_internal::Violation("refused"),
            ));
        }
        let r = receipt(self.entries.len());
        self.entries.push((r.clone(), entry.entry().clone()));
        Ok(r)
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

fn bounds() -> PolicyBounds {
    serde_json::from_value(json!({
        "deployment": { "name": "rutent-test", "operator_key": "PT4_QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1w" },
        "protocol_version": "1.0.0",
        "policy_version": "policy-1",
        "max_hops": 4,
        "max_parts": 1,
        "excluded_providers": [],
        "excluded_tuples": [],
        "provider_ceilings": [],
        "publisher_ceilings": [],
        "operator_fee": { "flat": "0", "proportional": { "numerator": "0", "denominator": "1" } },
        "slippage_limits": [],
    }))
    .unwrap()
}

fn fee(flat: &str) -> OperatorSetting {
    serde_json::from_value(json!({
        "setting": "operator_fee",
        "formula": { "flat": flat, "proportional": { "numerator": "0", "denominator": "1" } },
    }))
    .unwrap()
}

fn cadence(job: &str, seconds: u64) -> OperatorSetting {
    serde_json::from_value(json!({"setting": "cadence", "job": job, "seconds": seconds})).unwrap()
}

fn deposit_wait(seconds: u64) -> OperatorSetting {
    OperatorSetting::DepositWait {
        seconds: Count::new(seconds).unwrap(),
    }
}

/// Sets `setting` over `log` and `policy`, the targets given.
fn set(
    settings: &mut Settings,
    log: &mut KeptLog,
    policy: &mut Answering,
    targets: &mut Targets<'_>,
    setting: &OperatorSetting,
) -> Result<Receipt, SettingRefused> {
    let (caller, core) = (operator(), core());
    settings.set(
        log,
        policy,
        &clock(),
        &mut Ids::default(),
        targets,
        &SettingAsked {
            setting,
            caller: &caller,
            core: &core,
        },
    )
}

/// An operator's setting takes effect only on a permitted decision (`REQ-AUZ-6`, `REQ-AUZ-1`): a refused or unknown
/// decision sets nothing and records no effect; a permitted one is asked of policy as `set_setting` naming the setting,
/// recorded as an effect before it applies and its result after, and reaches what reads it (the router's bounds, the
/// schedule's cadences). A reopened Core rebuilds every setting from the log, the later of two for one thing standing.
// verifies: REQ-AUZ-6, REQ-AUZ-1
#[test]
fn operator_setting_takes_effect_only_on_a_permitted_decision_and_is_rebuilt_from_the_log() {
    for answer in [
        Answer::Refuse {
            class: RefusalClass::NotPermitted,
        },
        Answer::NotKnown {
            cause: NotKnownCause::VerifierUnreachable,
        },
    ] {
        let (mut log, mut policy) = (KeptLog::default(), Answering::with(answer));
        let mut settings = Settings::new();
        let mut cadences = Cadences::new();
        let refused = set(
            &mut settings,
            &mut log,
            &mut policy,
            &mut Targets {
                cadences: Some(&mut cadences),
                ..Targets::default()
            },
            &cadence("holding_check", 60),
        );
        assert_eq!(refused, Err(SettingRefused::NotPermitted(answer)));
        assert!(log.entries.is_empty(), "no effect is recorded");
        assert_eq!(settings, Settings::new());
        assert_eq!(cadences.of(Job::HoldingCheck), 3600, "the default stands");
    }

    let (mut log, mut policy) = (KeptLog::default(), Answering::with(Answer::Permit {}));
    let mut settings = Settings::new();
    let mut cadences = Cadences::new();
    for setting in [fee("7"), cadence("holding_check", 60), fee("9")] {
        set(
            &mut settings,
            &mut log,
            &mut policy,
            &mut Targets {
                cadences: Some(&mut cadences),
                ..Targets::default()
            },
            &setting,
        )
        .expect("permitted");
    }
    assert_eq!(policy.asked.len(), 3);
    assert!(matches!(
        &policy.asked[0].action,
        Action::ChangeCatalogue { change: CatalogueChange::SetSetting, setting: Some(s), provider, .. }
            if **s == fee("7") && provider.as_str() == "operator"
    ));
    let kinds: Vec<(&str, Outcome)> = log
        .entries
        .iter()
        .map(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Effect { .. } => ("effect", v.outcome),
                What::Result { .. } => ("result", v.outcome),
                other => panic!("{other:?}"),
            },
            Entry::Stated(_) => panic!("a verified entry"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("effect", Outcome::ToTakeEffect {}),
            ("result", Outcome::Done {}),
            ("effect", Outcome::ToTakeEffect {}),
            ("result", Outcome::Done {}),
            ("effect", Outcome::ToTakeEffect {}),
            ("result", Outcome::Done {}),
        ]
    );
    assert_eq!(settings.bounds(&bounds()).operator_fee.flat.as_str(), "9");
    assert_eq!(cadences.of(Job::HoldingCheck), 60);
    assert_eq!(settings.cadence("holding_check"), Some(60));

    // Rebuilt from the log alone, it is the same, and reaches the schedule a reopened Core puts together.
    let reopened = Settings::reopened(&log.entries);
    assert_eq!(reopened, settings);
    let mut fresh = Cadences::new();
    reopened.apply(&mut Targets {
        cadences: Some(&mut fresh),
        ..Targets::default()
    });
    assert_eq!(fresh.of(Job::HoldingCheck), 60);
}

/// A setting whose result the log didn't take stands after a restart as it stood before it, and its result is completed
/// (`T-020-49`, M2 of #337's review): set live, the effect appended and applied, its result refused; the reopened Core
/// reads the effect, holds the setting, and appends the missing `done` result once, so every later reopening reads the
/// same, and appends nothing more. In the other order, a log that took the result and a reopening read the same. A
/// setting whose result is `failed` is not taken.
// verifies: REQ-AUZ-6, NFR-6
#[test]
fn setting_whose_result_the_log_did_not_take_stands_after_a_restart_and_its_result_is_completed() {
    let mut policy = Answering::with(Answer::Permit {});
    let mut cut = KeptLog {
        refuse_from: Some(1),
        ..KeptLog::default()
    };
    let mut live = Settings::new();
    assert!(matches!(
        set(
            &mut live,
            &mut cut,
            &mut policy,
            &mut Targets::default(),
            &fee("5")
        ),
        Err(SettingRefused::Append(_))
    ));
    assert_eq!(cut.entries.len(), 1, "the effect, and no result");
    assert_eq!(
        live.bounds(&bounds()).operator_fee.flat.as_str(),
        "5",
        "in force live"
    );

    // Reopened: the same setting, and its result completed once.
    cut.refuse_from = None;
    let (reopened, completed) =
        Settings::reopen(&mut cut, &clock(), &core(), &mut Targets::default());
    assert_eq!(completed, Ok(1));
    assert_eq!(reopened, live, "it stands after the restart as before it");
    assert!(matches!(
        cut.entries.last(),
        Some((_, Entry::Verified(v))) if matches!(v.what, What::Result { .. }) && v.outcome == Outcome::Done {}
    ));
    let (again, completed) = Settings::reopen(&mut cut, &clock(), &core(), &mut Targets::default());
    assert_eq!(completed, Ok(0), "nothing more to complete");
    assert_eq!(again, live);

    // The other order: the result taken first, then the reopening; it reads the same.
    let mut whole = KeptLog::default();
    let mut set_whole = Settings::new();
    set(
        &mut set_whole,
        &mut whole,
        &mut policy,
        &mut Targets::default(),
        &fee("5"),
    )
    .unwrap();
    let (reopened, completed) =
        Settings::reopen(&mut whole, &clock(), &core(), &mut Targets::default());
    assert_eq!((reopened, completed), (set_whole, Ok(0)));

    // A failed result sets nothing.
    let mut failed = whole.entries.clone();
    let Some((_, Entry::Verified(result))) = failed.last_mut() else {
        panic!("a result")
    };
    result.outcome = Outcome::Failed {};
    assert_eq!(Settings::reopened(&failed), Settings::new());
}

/// A deposit wait above its ceiling (`DEPOSIT_WAIT_CEILING_SECONDS`, thirty days) is refused after policy is asked, so
/// the attempt is policy's decision, with no effect recorded and nothing applied; one at the ceiling is set; a log
/// holding one above it, which no command writes, sets nothing on reopening (`REQ-AUZ-6`, L4 of #337's review).
// verifies: REQ-AUZ-6
#[test]
fn deposit_wait_above_its_ceiling_is_refused() {
    let (mut log, mut policy) = (KeptLog::default(), Answering::with(Answer::Permit {}));
    let mut settings = Settings::new();
    let mut arrivals = Arrivals::new();
    let refused = set(
        &mut settings,
        &mut log,
        &mut policy,
        &mut Targets {
            arrivals: Some(&mut arrivals),
            ..Targets::default()
        },
        &deposit_wait(DEPOSIT_WAIT_CEILING_SECONDS + 1),
    );
    assert_eq!(
        refused,
        Err(SettingRefused::AboveCeiling {
            ceiling: DEPOSIT_WAIT_CEILING_SECONDS
        })
    );
    assert_eq!(
        policy.asked.len(),
        1,
        "policy is asked first: the attempt is its decision"
    );
    assert!(log.entries.is_empty(), "no effect recorded");
    assert_eq!(settings.deposit_wait(), None);
    assert_eq!(DEFAULT_DEPOSIT_WAIT_SECONDS, 86_400);
    set(
        &mut settings,
        &mut log,
        &mut policy,
        &mut Targets {
            arrivals: Some(&mut arrivals),
            ..Targets::default()
        },
        &deposit_wait(DEPOSIT_WAIT_CEILING_SECONDS),
    )
    .expect("at its ceiling");
    assert_eq!(
        settings.deposit_wait().map(Count::get),
        Some(DEPOSIT_WAIT_CEILING_SECONDS)
    );
    // A recorded effect above the ceiling is not rebuilt.
    let mut forged = log.entries.clone();
    let Some((_, Entry::Verified(effect))) = forged
        .iter_mut()
        .find(|(_, e)| matches!(e, Entry::Verified(v) if matches!(v.what, What::Effect { .. })))
    else {
        panic!("an effect")
    };
    let What::Effect {
        action: Action::ChangeCatalogue { setting, .. },
        ..
    } = &mut effect.what
    else {
        panic!("a setting's effect")
    };
    *setting = Some(Box::new(deposit_wait(DEPOSIT_WAIT_CEILING_SECONDS + 1)));
    assert_eq!(Settings::reopened(&forged).deposit_wait(), None);
}

/// A public node's setting the operator sets through policy takes effect on its node's segments (`REQ-AUZ-6`,
/// `T-020-49`): the node is on the catalogue's public list, so a holding account can hold the value its segments leave
/// there (`INV-5`, `REQ-CAT-23`), which before it no account could; its confirmations reach Core with it. A reopened
/// Core, its catalogue put together again, lists it again from the log.
// verifies: REQ-AUZ-6
#[test]
fn public_node_setting_set_by_the_operator_takes_effect_on_its_nodes_segments() {
    use rutent_prov::catalogue::Catalogue;
    let node = "ledger-z";
    let mut catalogue = Catalogue::new();
    let _ = catalogue.publish_operator_module("operator-wallet");
    assert!(
        catalogue
            .register_public_holding(node, "operator-wallet")
            .is_err(),
        "no account holds value at a node the operator doesn't list"
    );
    let setting: OperatorSetting = serde_json::from_value(json!({
        "setting": "public_node",
        "node": { "network": node },
        "listed": true,
        "confirmations": 6,
    }))
    .unwrap();

    // Refused, nothing reaches the catalogue.
    let (mut log, mut policy) = (
        KeptLog::default(),
        Answering::with(Answer::Refuse {
            class: RefusalClass::NotPermitted,
        }),
    );
    let mut settings = Settings::new();
    assert!(
        set(
            &mut settings,
            &mut log,
            &mut policy,
            &mut Targets {
                catalogue: Some(&mut catalogue),
                ..Targets::default()
            },
            &setting,
        )
        .is_err()
    );
    assert!(!catalogue.nodes().is_public(node));

    let mut policy = Answering::with(Answer::Permit {});
    set(
        &mut settings,
        &mut log,
        &mut policy,
        &mut Targets {
            catalogue: Some(&mut catalogue),
            ..Targets::default()
        },
        &setting,
    )
    .expect("permitted");
    assert!(catalogue.nodes().is_public(node));
    assert!(
        catalogue
            .register_public_holding(node, "operator-wallet")
            .is_ok(),
        "the node's segments can leave their value in its holding account"
    );
    let set_to = settings.public_node(node).expect("its settings");
    assert!(set_to.listed);
    assert_eq!(set_to.confirmations, Some(6));

    let mut again = Catalogue::new();
    Settings::reopened(&log.entries).apply(&mut Targets {
        catalogue: Some(&mut again),
        ..Targets::default()
    });
    assert!(again.nodes().is_public(node), "rebuilt from the log");
}

/// A public node the operator unlists through its setting is no longer on the catalogue's public list (`REQ-AUZ-6`,
/// `T-020-49`): listed by the deployment's default or an earlier setting, a decided `listed: false` takes it off, so no
/// holding account can be registered there any more; a reopened Core, its catalogue listing the node again at the
/// advertisement's join, takes it off again from the log. Until `T-020-59`'s listing change replaces it, it is the
/// operator's setting.
// verifies: REQ-AUZ-6
#[test]
fn public_node_unlisted_by_the_operator_is_no_longer_on_the_catalogues_public_list() {
    use rutent_prov::catalogue::Catalogue;
    let node = "ledger-z";
    let listing = |listed: bool| -> OperatorSetting {
        serde_json::from_value(json!({
            "setting": "public_node",
            "node": { "network": node },
            "listed": listed,
        }))
        .unwrap()
    };
    let mut catalogue = Catalogue::new();
    let _ = catalogue.add_public_node(node);
    let (mut log, mut policy) = (KeptLog::default(), Answering::with(Answer::Permit {}));
    let mut settings = Settings::new();
    // Core's segments, nothing of them at the node: an unlist is held to them (`T-020-65`), and refused where they
    // aren't read.
    let segments = rutent_core_orch::segments::Segments::new();
    set(
        &mut settings,
        &mut log,
        &mut policy,
        &mut Targets {
            catalogue: Some(&mut catalogue),
            ..Targets::default()
        },
        &listing(false),
    )
    .expect_err("no segments read: refused");
    set(
        &mut settings,
        &mut log,
        &mut policy,
        &mut Targets {
            catalogue: Some(&mut catalogue),
            segments: Some(&segments),
            ..Targets::default()
        },
        &listing(false),
    )
    .expect("permitted");
    assert!(!catalogue.nodes().is_public(node), "taken off the list");
    let _ = catalogue.publish_operator_module("operator-wallet");
    assert!(
        catalogue
            .register_public_holding(node, "operator-wallet")
            .is_err()
    );
    assert!(!settings.public_node(node).expect("its settings").listed);

    // Reopened: the deployment's join lists it again, the log's setting takes it off.
    let mut again = Catalogue::new();
    let _ = again.add_public_node(node);
    Settings::reopened(&log.entries).apply(&mut Targets {
        catalogue: Some(&mut again),
        ..Targets::default()
    });
    assert!(!again.nodes().is_public(node), "rebuilt from the log");

    // Listed again by a later setting, it is public again.
    set(
        &mut settings,
        &mut log,
        &mut policy,
        &mut Targets {
            catalogue: Some(&mut catalogue),
            ..Targets::default()
        },
        &listing(true),
    )
    .expect("permitted");
    assert!(catalogue.nodes().is_public(node));
}
