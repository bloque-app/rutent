//! `T-021-06`'s tests: a stuck order's decide and move steps are separate permissions, their actors compared at the
//! accountable party, the decision naming evidence the store keeps; a move's outcome confirmed only by an actor whose
//! `ActsFor` chain doesn't meet its reporter's; neither the order's customer nor the desk of the provider holding the
//! funds deciding, moving or confirming where `THR-70` would have them.
//!
//! The logs are the contract's own coherent world (`fixtures/admission/world--*.json`), as the log holds it before
//! each step, so every request is one the log would admit, and the register is these tests' own.

use super::*;
use crate::engine::{Asked, Input, Need, Policy, Reply, Ruling, Said, Source, Sources};
use crate::parties::Scope;
use crate::roles::Permission;
use rutent_contracts::core_internal::calls::DecisionEntry;
use rutent_contracts::core_internal::{
    Actor, ActorKind, Answer, Decided, NotKnownCause, Outcome, RefusalClass, Role,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::{Id, Timestamp};
use serde_json::{Value, json};

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn digest(byte: &str) -> Digest {
    serde_json::from_value(json!({"algorithm": "sha-256", "value": byte.repeat(32)})).unwrap()
}

/// One case of the contract's world: the log as it stands before the entry, each entry with its receipt, and the
/// request the entry records.
fn world(name: &str) -> (Vec<(Receipt, Entry)>, Value) {
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/admission/world--{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let case: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let history = case["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                serde_json::from_value(item["receipt"].clone()).unwrap(),
                serde_json::from_value(item["entry"].clone()).unwrap(),
            )
        })
        .collect();
    (history, case["entry"]["what"]["request"].clone())
}

fn person(party: &str, acts: &str) -> Actor {
    Actor {
        party: id(party),
        acts: id(acts),
        role: Role::Operator,
        kind: ActorKind::Person,
    }
}

fn agent(party: &str, acts: &str) -> Actor {
    Actor {
        kind: ActorKind::Agent,
        ..person(party, acts)
    }
}

/// The log policy appends to, through its handle, each entry with its receipt; its read fails where it is set to.
#[derive(Default)]
struct Log {
    entries: Vec<(Receipt, Entry)>,
    unreadable: bool,
}

impl PolicyLog for Log {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        let entry = Entry::Verified(entry.entry().clone());
        entry.check().map_err(AppendFailure::Refused)?;
        let index = 9_000 + self.entries.len();
        let receipt: Receipt = serde_json::from_value(json!({
            "log": "log-main",
            "index": index,
            "entry": {"algorithm": "sha-256", "value": format!("{index:064x}")},
        }))
        .unwrap();
        self.entries.push((receipt.clone(), entry));
        Ok(receipt)
    }
}

impl Receipted for Log {
    fn receipted(&mut self) -> Result<Vec<(Receipt, Entry)>, ReadFailure> {
        if self.unreadable {
            return Err(ReadFailure::Unavailable);
        }
        Ok(self.entries.clone())
    }
}

impl Log {
    /// The outcome of the last entry the log took, and how many it holds.
    fn last(&self) -> (Outcome, usize) {
        match self.entries.last() {
            Some((_, Entry::Verified(entry))) => (entry.outcome, self.entries.len()),
            _ => panic!("a verified entry"),
        }
    }
}

/// The deployment's policy: it needs nothing and permits whatever the fixed rules let through.
struct Permits;

impl Policy for Permits {
    fn needs(&mut self, _: Asked<'_>) -> Reply<Vec<Need>> {
        Reply::Given(Vec::new())
    }

    fn rule(&mut self, _: Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        Reply::Given(Ruling::Permit)
    }
}

/// The deployment's registers, as these tests set them. Every source this row doesn't ask answers yes.
#[derive(Default)]
struct Register {
    /// Each login, the parties it acts for and as what kind of actor; one not listed is a missing input.
    acts_for: Vec<(Id, Id, ActorKind)>,
    /// Each party that acts for another: the links of an `ActsFor` chain past a login's own parties.
    links: Vec<(Id, Id)>,
    /// Each login's permissions.
    permissions: Vec<(Id, Permission)>,
    /// Each agent's scopes, by its party and its login.
    scopes: Vec<(Id, Id, Scope)>,
    /// Each order's parties: its requester, beneficiary and payer.
    order_parties: Vec<(Id, Id)>,
    /// What the evidence store keeps: each commitment, and the order it is about where it is about one.
    kept: Vec<(Digest, Option<Id>)>,
    /// The register of chains can't be reached.
    chains_unreachable: bool,
    /// The register of chains says every two chains are apart, so only the rules that need no chain are left.
    chains_apart: bool,
}

impl Register {
    /// The world's actors, each acting for its own party: its operator's two people, `op-login-2` for `operator` and
    /// `op-login-5` for `operator-desk-b`, each holding every permission of a stuck order's steps; the customer
    /// `cust-1` of `ord-1`; the provider `bank-co`; and Core's `core-orch`. The store keeps the world's decision's
    /// evidence, about `ord-1`.
    fn of_the_world() -> Self {
        let mut register = Register::default();
        for actor in [
            person("operator", "op-login-2"),
            person("operator-desk-b", "op-login-5"),
        ] {
            register = register.with(&actor);
        }
        register.list("cust-1", "cust-1", ActorKind::Person);
        register.list("bank-co", "bank-co", ActorKind::Component);
        register.list("core-orch", "rutent", ActorKind::Component);
        register.order_parties.push((id("ord-1"), id("cust-1")));
        register.kept.push((digest("db"), Some(id("ord-1"))));
        register
    }

    fn list(&mut self, acts: &str, party: &str, kind: ActorKind) {
        self.acts_for.push((id(acts), id(party), kind));
    }

    /// `actor` listed for its party, holding every permission of a stuck order's steps, and, an agent, scoped to
    /// every one of them.
    fn with(mut self, actor: &Actor) -> Self {
        self.acts_for
            .push((actor.acts.clone(), actor.party.clone(), actor.kind));
        for permission in [
            Permission::DecideStuck,
            Permission::MoveStuck,
            Permission::ConfirmOutcome,
        ] {
            self.permissions.push((actor.acts.clone(), permission));
        }
        for scope in [
            Scope::DecideStuck,
            Scope::MoveStuck,
            Scope::ReportOutcome,
            Scope::ConfirmOutcome,
        ] {
            self.scopes
                .push((actor.party.clone(), actor.acts.clone(), scope));
        }
        self
    }

    /// Everything `start`, a login or a party, reaches: itself, the parties it is listed for, and on along the links.
    fn chain(&self, start: &Id) -> Vec<Id> {
        let mut reached = vec![start.clone()];
        let mut i = 0;
        while i < reached.len() {
            let at = reached[i].clone();
            let next = self
                .acts_for
                .iter()
                .filter(|(a, _, _)| a == &at)
                .map(|(_, p, _)| p.clone())
                .chain(
                    self.links
                        .iter()
                        .filter(|(from, _)| from == &at)
                        .map(|(_, to)| to.clone()),
                );
            for party in next.collect::<Vec<_>>() {
                if !reached.contains(&party) {
                    reached.push(party);
                }
            }
            i += 1;
        }
        reached
    }
}

impl Sources for Register {
    fn supply(&mut self, need: &Need) -> Reply<Said> {
        let said = |yes: bool| Reply::Given(if yes { Said::Yes } else { Said::No });
        match &need.from {
            Source::ActsFor { acts, party, kind } => {
                if !self.acts_for.iter().any(|(a, _, _)| a == acts) {
                    return Reply::Missing;
                }
                said(
                    self.acts_for
                        .iter()
                        .any(|(a, p, k)| a == acts && p == party && k == kind),
                )
            }
            Source::AgentScope { party, scope } => said(
                self.scopes
                    .iter()
                    .any(|(p, a, s)| p == party && a == &need.input && s == scope),
            ),
            Source::Permissions { permission, .. } => said(
                self.permissions
                    .iter()
                    .any(|(a, p)| a == &need.input && p == permission),
            ),
            Source::OrderParties(order) => said(
                self.order_parties
                    .iter()
                    .any(|(o, p)| o == order && p == &need.input),
            ),
            Source::Kept(commitment) => said(self.kept.iter().any(|(c, about)| {
                c == commitment && about.as_ref().is_none_or(|o| o == &need.input)
            })),
            Source::CommonParty { with } => {
                if self.chains_unreachable {
                    return Reply::Unreachable;
                }
                if self.chains_apart {
                    return Reply::Given(Said::No);
                }
                let theirs = self.chain(with);
                said(self.chain(&need.input).iter().any(|p| theirs.contains(p)))
            }
            _ => Reply::Given(Said::Yes),
        }
    }
}

type Tested = Engine<Log, Permits, Register>;

/// The engine over the world as it stood before `case`, with `register`.
fn engine(case: &str, register: Register) -> (Tested, Value) {
    let (entries, request) = world(case);
    let log = Log {
        entries,
        unreadable: false,
    };
    (Engine::with_policy(log, Permits, register), request)
}

fn refused(class: RefusalClass) -> Outcome {
    Outcome::Refused { class }
}

/// The world's decision (`req-200`, by `op-login-2` for `operator`, of `ord-1`), decided by `decider`.
fn decision_by(request: &Value, decider: &Actor) -> StuckDecide {
    let mut decide: StuckDecide = serde_json::from_value(request.clone()).unwrap();
    decide.decider = decider.clone();
    decide
}

/// A move of `decided`'s decision of `ord-1`, by `mover`, as request `request`.
fn move_of(decided: &Decision, request: &str, mover: &Actor) -> StuckMove {
    StuckMove {
        request: id(request),
        order: id("ord-1"),
        carries_out: Decided {
            request: decided.request.clone(),
            decision: decided.decision.clone(),
        },
        mover: mover.clone(),
        at: Timestamp::new("2026-11-10T09:00:00Z").unwrap(),
    }
}

/// One actor attempting both steps of a resolution is refused at the move, its refusal recorded: the decider itself,
/// another login of its party, and its own login stating another party, which the register lists it for too. An actor
/// of another party and another login moves it.
// verifies: REQ-AUZ-4, AC-6
#[test]
fn one_actor_attempting_both_decide_and_move_is_refused() {
    let alice = person("operator", "op-login-2");
    let register = Register::of_the_world()
        .with(&person("operator", "op-login-7"))
        .with(&person("operator-desk-c", "op-login-2"));
    let (mut policy, request) = engine("sd", register);
    let decided = policy.decide_stuck(&decision_by(&request, &alice)).unwrap();
    assert!(decided.permit().is_some(), "{decided:?}");

    for (n, mover) in [
        alice.clone(),
        person("operator", "op-login-7"),
        person("operator-desk-c", "op-login-2"),
    ]
    .iter()
    .enumerate()
    {
        let moved = policy
            .move_stuck(&move_of(&decided, &format!("req-move-{n}"), mover))
            .unwrap();
        assert_eq!(
            moved.outcome,
            Answer::Refuse {
                class: RefusalClass::SameActor
            },
            "{mover:?}"
        );
        assert_eq!(policy.log.last().0, refused(RefusalClass::SameActor));
    }

    let bob = person("operator-desk-b", "op-login-5");
    let moved = policy
        .move_stuck(&move_of(&decided, "req-move-bob", &bob))
        .unwrap();
    assert!(moved.permit().is_some(), "{moved:?}");
}

/// Two agents of one accountable party are one actor: one decides, and the other's move is refused, as its party's
/// person's is; an agent of another party moves it. Their report and confirmation of a move's outcome are held the
/// same way.
// verifies: REQ-AUZ-4, REQ-AUZ-8
#[test]
fn two_agents_of_one_party_count_as_one_actor() {
    let (first, second) = (agent("desk-a", "agent-a1"), agent("desk-a", "agent-a2"));
    let register = Register::of_the_world()
        .with(&first)
        .with(&second)
        .with(&person("desk-a", "desk-a-lead"))
        .with(&agent("desk-z", "agent-z1"));
    let (mut policy, request) = engine("sd", register);
    let decided = policy.decide_stuck(&decision_by(&request, &first)).unwrap();
    assert!(decided.permit().is_some(), "{decided:?}");
    for (n, mover) in [second.clone(), person("desk-a", "desk-a-lead")]
        .iter()
        .enumerate()
    {
        let moved = policy
            .move_stuck(&move_of(&decided, &format!("req-move-{n}"), mover))
            .unwrap();
        assert_eq!(
            moved.outcome,
            Answer::Refuse {
                class: RefusalClass::SameActor
            },
            "{mover:?}"
        );
    }
    let moved = policy
        .move_stuck(&move_of(
            &decided,
            "req-move-z",
            &agent("desk-z", "agent-z1"),
        ))
        .unwrap();
    assert!(moved.permit().is_some(), "{moved:?}");

    // The world's report is `op-login-5`'s; here an agent of `desk-a` reported it, and another of `desk-a` confirms,
    // even where the register's chains would keep them apart: one party is one actor, whatever its chains.
    let mut register = Register::of_the_world().with(&first).with(&second);
    register.chains_apart = true;
    let (mut policy, request) = engine("settled_es", register);
    let mut confirmation: OutcomeConfirmation = serde_json::from_value(request).unwrap();
    report_by(&mut policy.log, &confirmation.report, &first);
    confirmation.confirmer = second;
    assert_eq!(
        policy.confirm_outcome(&confirmation).unwrap().outcome,
        Answer::Refuse {
            class: RefusalClass::SameActor
        }
    );
}

/// The world's report at `receipt`, as if `reporter` had made it.
fn report_by(log: &mut Log, receipt: &Receipt, reporter: &Actor) {
    let (_, Entry::Verified(entry)) = log
        .entries
        .iter_mut()
        .find(|(r, _)| r == receipt)
        .expect("the report")
    else {
        panic!("a verified report");
    };
    entry.who = reporter.clone();
    let What::ReportOutcome { request, .. } = &mut entry.what else {
        panic!("a report");
    };
    request.reporter = reporter.clone();
}

/// A resolution naming no evidence is refused: one naming none at all fails its check, unrecorded, and one naming
/// evidence the store keeps nothing under, or keeps about another order, is refused and recorded, even beside
/// evidence it keeps. One naming only evidence the store keeps about its order is permitted.
// verifies: REQ-AUZ-4
#[test]
fn resolution_naming_no_evidence_is_refused() {
    let mut register = Register::of_the_world();
    register.kept.push((digest("ee"), Some(id("ord-2"))));
    register.kept.push((digest("ef"), None));
    let (mut policy, request) = engine("sd", register);
    let alice = person("operator", "op-login-2");
    let named = |evidence: Vec<Digest>, request_id: &str| StuckDecide {
        request: id(request_id),
        evidence,
        ..decision_by(&request, &alice)
    };

    let before = policy.log.entries.len();
    assert!(matches!(
        policy.decide_stuck(&named(Vec::new(), "req-none")),
        Err(AppendFailure::Refused(_))
    ));
    assert_eq!(policy.log.entries.len(), before, "nothing recorded");

    for (n, evidence) in [
        vec![digest("aa")],
        vec![digest("db"), digest("aa")],
        vec![digest("ee")],
    ]
    .into_iter()
    .enumerate()
    {
        let decided = policy
            .decide_stuck(&named(evidence.clone(), &format!("req-{n}")))
            .unwrap();
        assert_eq!(
            decided.outcome,
            Answer::Refuse {
                class: RefusalClass::MissingInput
            },
            "{evidence:?}"
        );
        assert_eq!(
            policy.log.last(),
            (refused(RefusalClass::MissingInput), before + n + 1)
        );
    }

    let decided = policy
        .decide_stuck(&named(vec![digest("db"), digest("ef")], "req-kept"))
        .unwrap();
    assert!(decided.permit().is_some(), "{decided:?}");
    let Some((_, Entry::Verified(entry))) = policy.log.entries.last() else {
        panic!("a verified entry");
    };
    let What::StuckDecide { request, .. } = &entry.what else {
        panic!("a decide step");
    };
    assert_eq!(
        request.evidence,
        [digest("db"), digest("ef")],
        "the evidence relied on is recorded"
    );
}

/// Deciding, moving and confirming are each a permission of its own, held login by login: one alone allows only its
/// own step, and none is the operator's party's. A stuck order's steps are the operator's: no other role decides or
/// moves, whatever its register says.
// verifies: REQ-AUZ-4, THR-77
#[test]
fn deciding_moving_and_confirming_are_separate_permissions() {
    let mut register = Register::of_the_world();
    let (decides, moves, confirms) = (
        person("desk-d", "only-decides"),
        person("desk-m", "only-moves"),
        person("desk-c", "only-confirms"),
    );
    for (actor, permission) in [
        (&decides, Permission::DecideStuck),
        (&moves, Permission::MoveStuck),
        (&confirms, Permission::ConfirmOutcome),
    ] {
        register.list(actor.acts.as_str(), actor.party.as_str(), ActorKind::Person);
        register.permissions.push((actor.acts.clone(), permission));
    }
    let provider = Actor {
        role: Role::Provider,
        ..person("desk-p", "provider-desk")
    };
    register = register.with(&provider);
    let (mut policy, request) = engine("sd", register);
    let not_permitted = Answer::Refuse {
        class: RefusalClass::NotPermitted,
    };

    for decider in [&moves, &confirms, &provider] {
        let mut decide = decision_by(&request, decider);
        decide.request = id(&format!("req-by-{}", decider.acts.as_str()));
        assert_eq!(policy.decide_stuck(&decide).unwrap().outcome, not_permitted);
    }
    let decided = policy
        .decide_stuck(&decision_by(&request, &decides))
        .unwrap();
    assert!(decided.permit().is_some(), "{decided:?}");
    for mover in [&decides, &confirms, &provider] {
        let request = format!("req-move-by-{}", mover.acts.as_str());
        assert_eq!(
            policy
                .move_stuck(&move_of(&decided, &request, mover))
                .unwrap()
                .outcome,
            not_permitted,
            "{mover:?}"
        );
    }
    let moved = policy
        .move_stuck(&move_of(&decided, "req-move", &moves))
        .unwrap();
    assert!(moved.permit().is_some(), "{moved:?}");

    let mut register = Register::of_the_world();
    for (actor, permission) in [
        (&decides, Permission::DecideStuck),
        (&moves, Permission::MoveStuck),
        (&confirms, Permission::ConfirmOutcome),
    ] {
        register.list(actor.acts.as_str(), actor.party.as_str(), ActorKind::Person);
        register.permissions.push((actor.acts.clone(), permission));
    }
    let (mut policy, request) = engine("settled_es", register);
    let confirmation: OutcomeConfirmation = serde_json::from_value(request).unwrap();
    for (n, confirmer) in [&decides, &moves].into_iter().enumerate() {
        let c = OutcomeConfirmation {
            request: id(&format!("req-confirm-{n}")),
            confirmer: confirmer.clone(),
            ..confirmation.clone()
        };
        assert_eq!(policy.confirm_outcome(&c).unwrap().outcome, not_permitted);
    }
    let c = OutcomeConfirmation {
        confirmer: confirms.clone(),
        ..confirmation
    };
    let confirmed = policy.confirm_outcome(&c).unwrap();
    assert!(confirmed.permit().is_some(), "{confirmed:?}");
}

/// `THR-70`'s colluding customer and provider: no party of the order decides its resolution or confirms a move's
/// outcome, even through a login listed for the operator's role and every permission; and no actor that reaches the
/// provider holding the funds, its own desk or one whose chain reaches it, moves the value or confirms its outcome.
// verifies: REQ-AUZ-4, THR-70
#[test]
fn neither_the_customer_nor_the_holders_desk_resolves_its_order() {
    let customer = person("cust-1", "cust-1-staff");
    let (holders, chained) = (
        person("bank-co", "bank-co-desk"),
        person("bank-co-ops", "bank-co-ops-1"),
    );
    let register = || {
        let mut register = Register::of_the_world()
            .with(&customer)
            .with(&holders)
            .with(&chained);
        register.links.push((id("bank-co-ops"), id("bank-co")));
        register
    };
    let not_permitted = Answer::Refuse {
        class: RefusalClass::NotPermitted,
    };

    let (mut policy, request) = engine("sd", register());
    let mut decide = decision_by(&request, &customer);
    decide.request = id("req-by-customer");
    assert_eq!(policy.decide_stuck(&decide).unwrap().outcome, not_permitted);
    let decided = policy
        .decide_stuck(&decision_by(&request, &person("operator", "op-login-2")))
        .unwrap();
    assert!(decided.permit().is_some(), "{decided:?}");
    for mover in [&holders, &chained] {
        let request = format!("req-move-by-{}", mover.acts.as_str());
        assert_eq!(
            policy
                .move_stuck(&move_of(&decided, &request, mover))
                .unwrap()
                .outcome,
            not_permitted,
            "{mover:?}"
        );
    }

    // The provider's own desk is its desk by its party, even where the register's chains say nothing of it.
    let mut apart = register();
    apart.chains_apart = true;
    let (mut policy, request) = engine("sd", apart);
    let decided = policy
        .decide_stuck(&decision_by(&request, &person("operator", "op-login-2")))
        .unwrap();
    assert_eq!(
        policy
            .move_stuck(&move_of(&decided, "req-move-by-the-holder", &holders))
            .unwrap()
            .outcome,
        not_permitted
    );

    // The world's resolution is delivered by `bank-co`; its report is `op-login-5`'s.
    let (mut policy, request) = engine("settled_es", register());
    let confirmation: OutcomeConfirmation = serde_json::from_value(request).unwrap();
    for (n, confirmer) in [&customer, &holders, &chained].into_iter().enumerate() {
        let c = OutcomeConfirmation {
            request: id(&format!("req-confirm-{n}")),
            confirmer: confirmer.clone(),
            ..confirmation.clone()
        };
        assert_eq!(
            policy.confirm_outcome(&c).unwrap().outcome,
            not_permitted,
            "{confirmer:?}"
        );
    }
    let confirmed = policy.confirm_outcome(&confirmation).unwrap();
    assert!(confirmed.permit().is_some(), "{confirmed:?}");

    // A segment's silent `move_value`, the world's withdrawal at `us-bank`, reported by Core's own actor: the provider
    // it moved through holds the funds, and its desk doesn't confirm it.
    let us_bank = person("us-bank", "us-bank-desk");
    let (mut policy, request) = engine("settled_es", register().with(&us_bank));
    let world: OutcomeConfirmation = serde_json::from_value(request).unwrap();
    let effect = policy
        .log
        .entries
        .iter()
        .find(|(_, e)| {
            matches!(e, Entry::Verified(v) if matches!(&v.what, What::Effect {
                action: rutent_contracts::core_internal::Action::MoveValue { provider, .. }, ..
            } if provider.as_str() == "us-bank"))
        })
        .map(|(r, _)| r.clone())
        .expect("the world's withdrawal");
    let core = Actor {
        party: id("rutent"),
        acts: id("core-orch"),
        role: Role::Core,
        kind: ActorKind::Component,
    };
    let reported = policy
        .report_outcome(&OutcomeReport {
            request: id("req-report-withdrawal"),
            order: id("ord-1"),
            effect: effect.clone(),
            outcome: world.outcome,
            reporter: core,
            evidence: world.confirmed_with.clone(),
            at: world.at.clone(),
        })
        .unwrap();
    assert!(reported.permit().is_some(), "{reported:?}");
    let of_the_withdrawal = |n: &str, confirmer: &Actor| OutcomeConfirmation {
        request: id(n),
        effect: effect.clone(),
        report: reported.receipt.clone(),
        confirmer: confirmer.clone(),
        ..world.clone()
    };
    assert_eq!(
        policy
            .confirm_outcome(&of_the_withdrawal("req-by-us-bank", &us_bank))
            .unwrap()
            .outcome,
        not_permitted
    );
    let confirmed = policy
        .confirm_outcome(&of_the_withdrawal(
            "req-by-op",
            &person("operator", "op-login-2"),
        ))
        .unwrap();
    assert!(confirmed.permit().is_some(), "{confirmed:?}");
}

/// A move's outcome is confirmed only by an actor whose `ActsFor` chain meets its reporter's nowhere: the reporter's
/// own login or party, a party the reporter's acts for, and a third party both act for, through the register's links,
/// are each refused; the decider of another chain confirms it (core-internal's README, `T-021-06`).
// verifies: REQ-AUZ-4, THR-77
#[test]
fn confirmer_whose_chain_meets_the_reporters_is_refused() {
    let same = Answer::Refuse {
        class: RefusalClass::SameActor,
    };
    // The world's report is `op-login-5`'s, for `operator-desk-b`; its confirmer `op-login-2`, for `operator`.
    let ask = |register: Register, confirmer: &Actor| {
        let (mut policy, request) = engine("settled_es", register);
        let confirmation: OutcomeConfirmation = serde_json::from_value(request).unwrap();
        policy
            .confirm_outcome(&OutcomeConfirmation {
                confirmer: confirmer.clone(),
                ..confirmation
            })
            .unwrap()
            .outcome
    };
    let alice = person("operator", "op-login-2");
    assert!(matches!(
        ask(Register::of_the_world(), &alice),
        Answer::Permit {}
    ));

    for confirmer in [
        person("operator-desk-b", "op-login-9"),
        person("operator-desk-x", "op-login-5"),
    ] {
        let register = Register::of_the_world().with(&confirmer);
        assert_eq!(ask(register, &confirmer), same, "{confirmer:?}");
    }

    // The reporter's desk acts for the operator.
    let mut register = Register::of_the_world();
    register.links.push((id("operator-desk-b"), id("operator")));
    assert_eq!(ask(register, &alice), same);

    // Both desks act for one umbrella party, two links up from the confirmer.
    let mut register = Register::of_the_world();
    register.links.push((id("operator-desk-b"), id("umbrella")));
    register.links.push((id("operator"), id("operator-group")));
    register.links.push((id("operator-group"), id("umbrella")));
    assert_eq!(ask(register, &alice), same);
}

/// A move that carries out no permitted decision of the log is refused: one naming a decision the log never took, one
/// of another order, and one carrying out a refused decision. A confirmation is refused citing what is no report, a
/// report of another outcome, one of another move than its own, or one the log holds as refused.
// verifies: REQ-AUZ-4
#[test]
fn step_with_nothing_permitted_behind_it_is_refused() {
    let not_permitted = Answer::Refuse {
        class: RefusalClass::NotPermitted,
    };
    let (mut policy, request) = engine("sd", Register::of_the_world());
    let bob = person("operator-desk-b", "op-login-5");
    let decided = policy
        .decide_stuck(&decision_by(&request, &person("operator", "op-login-2")))
        .unwrap();
    let mut never = move_of(&decided, "req-never", &bob);
    never.carries_out.decision = id("dec-never");
    let mut elsewhere = move_of(&decided, "req-elsewhere", &bob);
    elsewhere.order = id("ord-2");
    let mut unkept = decision_by(&request, &person("operator", "op-login-2"));
    unkept.request = id("req-unkept");
    unkept.evidence = vec![digest("aa")];
    let refused_decision = policy.decide_stuck(&unkept).unwrap();
    assert!(refused_decision.permit().is_none());
    for step in [
        never,
        elsewhere,
        move_of(&refused_decision, "req-of-refused", &bob),
    ] {
        assert_eq!(
            policy.move_stuck(&step).unwrap().outcome,
            not_permitted,
            "{step:?}"
        );
    }

    let (mut policy, request) = engine("settled_es", Register::of_the_world());
    let confirmation: OutcomeConfirmation = serde_json::from_value(request).unwrap();
    let report_of_another = policy
        .log
        .entries
        .iter()
        .find(|(_, e)| matches!(e, Entry::Verified(v) if matches!(v.what, What::Effect { .. })))
        .map(|(r, _)| r.clone())
        .unwrap();
    let mut failed_reported_done = confirmation.clone();
    failed_reported_done.outcome = rutent_contracts::core_internal::MoveOutcome::Done;
    // The world's report, cited for another move than its own: the world's withdrawal.
    let of_another_move = OutcomeConfirmation {
        effect: report_of_another.clone(),
        ..confirmation.clone()
    };
    // A report the log holds as refused: the world's, by a login the register doesn't list.
    let world_report = policy
        .log
        .entries
        .iter()
        .find_map(|(r, e)| match e {
            Entry::Verified(v) if r == &confirmation.report => match &v.what {
                What::ReportOutcome { request, .. } => Some(request.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("the world's report");
    let refused_report = policy
        .report_outcome(&OutcomeReport {
            request: id("req-report-unlisted"),
            reporter: person("operator-desk-b", "op-login-unlisted"),
            ..world_report
        })
        .unwrap();
    assert!(refused_report.permit().is_none());
    let of_a_refused_report = OutcomeConfirmation {
        report: refused_report.receipt.clone(),
        ..confirmation.clone()
    };
    for c in [
        OutcomeConfirmation {
            report: report_of_another,
            ..confirmation.clone()
        },
        failed_reported_done,
        of_another_move,
        of_a_refused_report,
    ] {
        assert_eq!(
            policy.confirm_outcome(&c).unwrap().outcome,
            not_permitted,
            "{c:?}"
        );
    }
}

/// What policy can't read or reach moves nothing: a log it can't read refuses a move and a confirmation as missing
/// the input they need, and a register of chains out of reach leaves a confirmation not known.
// verifies: REQ-AUZ-5, REQ-AUZ-4
#[test]
fn unread_log_or_unreached_register_moves_nothing() {
    let (mut policy, request) = engine("sd", Register::of_the_world());
    let decided = policy
        .decide_stuck(&decision_by(&request, &person("operator", "op-login-2")))
        .unwrap();
    policy.log.unreadable = true;
    let moved = policy
        .move_stuck(&move_of(
            &decided,
            "req-move",
            &person("operator-desk-b", "op-login-5"),
        ))
        .unwrap();
    assert_eq!(
        moved.outcome,
        Answer::Refuse {
            class: RefusalClass::MissingInput
        }
    );

    let (mut policy, request) = engine("settled_es", Register::of_the_world());
    let confirmation: OutcomeConfirmation = serde_json::from_value(request).unwrap();
    policy.log.unreadable = true;
    assert_eq!(
        policy.confirm_outcome(&confirmation).unwrap().outcome,
        Answer::Refuse {
            class: RefusalClass::MissingInput
        }
    );
    policy.log.unreadable = false;
    policy.sources.chains_unreachable = true;
    assert_eq!(
        policy.confirm_outcome(&confirmation).unwrap().outcome,
        Answer::NotKnown {
            cause: NotKnownCause::VerifierUnreachable
        }
    );
}

/// A move's outcome is reported on the rules every call is held to, its reporter acting for the party it states and,
/// an agent, within its scope; the log holds it to the move's own mover. The world's report is permitted and
/// recorded before it is answered.
// verifies: REQ-AUZ-4
#[test]
fn outcome_report_is_decided_and_recorded() {
    let (mut policy, request) = engine("report_es", Register::of_the_world());
    let report: OutcomeReport = serde_json::from_value(request).unwrap();
    let reported = policy.report_outcome(&report).unwrap();
    assert!(reported.permit().is_some(), "{reported:?}");
    assert_eq!(policy.log.last().0, Outcome::Permitted {});

    let stranger = person("operator-desk-b", "op-login-unlisted");
    let refused_report = policy
        .report_outcome(&OutcomeReport {
            request: id("req-report-stranger"),
            reporter: stranger,
            ..report
        })
        .unwrap();
    assert!(refused_report.permit().is_none());
}
