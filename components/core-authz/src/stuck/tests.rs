//! `T-021-06`'s tests: a stuck order's decide and move steps are separate permissions of the operator's, their actors
//! one where they share a login or a party or either's `ActsFor` chain reaches the other's party, the decision naming
//! evidence the store keeps about its order; a move's outcome confirmed only by another actor than its reporter on the
//! same reading; neither the order's customer nor the desk of the provider holding the funds deciding, moving or
//! confirming where `THR-70` would have them.
//!
//! The logs are the contract's own coherent world (`fixtures/admission/world--*.json`), as the log holds it before
//! each step, so every request is one the log would admit, and the register is these tests' own. The operator's staff
//! sit on desks under it: `operator-desk-a` (`alice`) and `operator-desk-b` (`op-login-5`, the world's mover and
//! reporter), each acting for `operator`, so each is the operator's and neither reaches the other.

use super::*;
use crate::engine::{Asked, Input, Need, Policy, Reply, Ruling, Said, Source, Sources};
use crate::parties::Scope;
use crate::roles::Permission;
use rutent_contracts::core_internal::calls::DecisionEntry;
use rutent_contracts::core_internal::{
    Actor, ActorKind, Answer, Decided, MoveOutcome, NotKnownCause, Outcome, RefusalClass, Role,
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

/// The decider and confirmer of these tests: a person on the operator's first desk.
fn alice() -> Actor {
    person("operator-desk-a", "alice")
}

/// The world's mover and reporter: a person on the operator's second desk.
fn bob() -> Actor {
    person("operator-desk-b", "op-login-5")
}

/// The log policy appends to, through its handle, each entry with a receipt of its own; its read fails where it is
/// set to.
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
    /// The register of chains says no chain reaches anything, so only the rules that need no chain are left.
    reaches_nothing: bool,
}

/// The deployment operator's party.
const OPERATOR: &str = "operator";

impl Register {
    /// The world's actors, each acting for its own party: the operator's people, `alice` on its first desk,
    /// `op-login-5` on its second and `op-login-2` in its own party, each holding every permission of a stuck
    /// order's steps; the customer `cust-1` of `ord-1`; the provider `bank-co`; and Core's `core-orch`. Both desks act
    /// for the operator. The store keeps the world's decision's evidence, about `ord-1`.
    fn of_the_world() -> Self {
        let mut register = Register::default()
            .with(&alice())
            .with(&bob())
            .with(&person(OPERATOR, "op-login-2"));
        register.link("operator-desk-a", OPERATOR);
        register.link("operator-desk-b", OPERATOR);
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

    /// The party `from` acting for `to`.
    fn link(&mut self, from: &str, to: &str) {
        self.links.push((id(from), id(to)));
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
            let next: Vec<Id> = self
                .acts_for
                .iter()
                .filter(|(a, _, _)| a == &at)
                .map(|(_, p, _)| p.clone())
                .chain(
                    self.links
                        .iter()
                        .filter(|(from, _)| from == &at)
                        .map(|(_, to)| to.clone()),
                )
                .collect();
            for party in next {
                if !reached.contains(&party) {
                    reached.push(party);
                }
            }
            i += 1;
        }
        reached
    }
}

impl Register {
    /// What `start` reaches below the deployment operator's party: its chain, cut where it reaches the operator.
    fn below_operator(&self, start: &Id) -> Vec<Id> {
        let operator = id(OPERATOR);
        let mut reached = vec![start.clone()];
        let mut i = 0;
        while i < reached.len() {
            let at = reached[i].clone();
            let next: Vec<Id> = self
                .acts_for
                .iter()
                .filter(|(a, _, _)| a == &at)
                .map(|(_, p, _)| p.clone())
                .chain(
                    self.links
                        .iter()
                        .filter(|(from, _)| from == &at)
                        .map(|(_, to)| to.clone()),
                )
                .collect();
            for party in next {
                if party != operator && !reached.contains(&party) {
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
            Source::KeptAbout(commitment) => said(
                self.kept
                    .iter()
                    .any(|(c, about)| c == commitment && about.as_ref() == Some(&need.input)),
            ),
            Source::OfOperator => said(self.chain(&need.input).contains(&id(OPERATOR))),
            Source::MeetsBelowOperator { from, with } => {
                if self.chains_unreachable {
                    return Reply::Unreachable;
                }
                let theirs = self.below_operator(with);
                said(
                    !self.reaches_nothing
                        && self.below_operator(from).iter().any(|p| theirs.contains(p)),
                )
            }
            Source::Reaches { from, to } => {
                if self.chains_unreachable {
                    return Reply::Unreachable;
                }
                said(!self.reaches_nothing && self.chain(from).contains(to))
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

const fn refuse(class: RefusalClass) -> Answer {
    Answer::Refuse { class }
}

/// The world's decision (`req-200`, of `ord-1`, delivered by `bank-co`), decided by `decider` as request `request_id`.
fn decision_by(request: &Value, decider: &Actor, request_id: &str) -> StuckDecide {
    let mut decide: StuckDecide = serde_json::from_value(request.clone()).unwrap();
    decide.decider = decider.clone();
    decide.request = id(request_id);
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

/// The world's confirmation of `op-login-5`'s report of its resolution's outcome, by `confirmer`, as `request_id`.
fn confirmation_by(request: &Value, confirmer: &Actor, request_id: &str) -> OutcomeConfirmation {
    let mut confirmation: OutcomeConfirmation = serde_json::from_value(request.clone()).unwrap();
    confirmation.confirmer = confirmer.clone();
    confirmation.request = id(request_id);
    confirmation
}

/// `alice`'s decision of the world's stuck order, permitted.
fn alices_decision(policy: &mut Tested, request: &Value) -> Decision {
    let decided = policy
        .decide_stuck(&decision_by(request, &alice(), "req-alice"))
        .unwrap();
    assert!(decided.permit().is_some(), "{decided:?}");
    decided
}

/// One actor attempting both steps of a resolution is refused at the move, its refusal recorded: the decider itself,
/// another login of its party, its own login stating another party, a bot whose party acts for the decider's, and a
/// person of the operator's own party, which the decider's chain reaches. A person on the operator's other desk, whose
/// chain meets the decider's only at the operator, moves it.
// verifies: REQ-AUZ-4, AC-6
#[test]
fn one_actor_attempting_both_decide_and_move_is_refused() {
    let mut register = Register::of_the_world()
        .with(&person("operator-desk-a", "alice-second"))
        .with(&person("operator-desk-c", "alice"))
        .with(&agent("desk-a-bots", "alice-bot"))
        .with(&agent("bots-d", "alice-d"))
        .with(&agent("bots-m", "alice-m"))
        .with(&person("desk-a1", "alice1"))
        .with(&person("desk-a2", "alice2"));
    register.link("operator-desk-c", OPERATOR);
    register.link("desk-a-bots", "operator-desk-a");
    // Two bot parties under one desk, and two desks under one person's division of the operator: each pair's chains
    // meet below the operator, so each is one actor (the reviewer's round-2 probes).
    register.link("bots-d", "operator-desk-a");
    register.link("bots-m", "operator-desk-a");
    register.link("desk-a1", "alice-person");
    register.link("desk-a2", "alice-person");
    register.link("alice-person", OPERATOR);
    let (mut policy, request) = engine("sd", register);
    for (decider, mover) in [
        (agent("bots-d", "alice-d"), agent("bots-m", "alice-m")),
        (person("desk-a1", "alice1"), person("desk-a2", "alice2")),
    ] {
        let decided = policy
            .decide_stuck(&decision_by(
                &request,
                &decider,
                &format!("req-{}", decider.acts.as_str()),
            ))
            .unwrap();
        assert!(decided.permit().is_some(), "{decided:?}");
        let moved = policy
            .move_stuck(&move_of(
                &decided,
                &format!("req-move-{}", mover.acts.as_str()),
                &mover,
            ))
            .unwrap();
        assert_eq!(moved.outcome, refuse(RefusalClass::SameActor), "{mover:?}");
    }
    let decided = alices_decision(&mut policy, &request);

    for (n, mover) in [
        alice(),
        person("operator-desk-a", "alice-second"),
        person("operator-desk-c", "alice"),
        agent("desk-a-bots", "alice-bot"),
        person(OPERATOR, "op-login-2"),
    ]
    .iter()
    .enumerate()
    {
        let moved = policy
            .move_stuck(&move_of(&decided, &format!("req-move-{n}"), mover))
            .unwrap();
        assert_eq!(moved.outcome, refuse(RefusalClass::SameActor), "{mover:?}");
        assert_eq!(policy.log.last().0, refused(RefusalClass::SameActor));
    }

    let moved = policy
        .move_stuck(&move_of(&decided, "req-move-bob", &bob()))
        .unwrap();
    assert!(moved.permit().is_some(), "{moved:?}");
}

/// Two agents of one accountable party are one actor: one decides, and the other's move is refused, as its party's
/// person's is; an agent of another desk moves it. Their report and confirmation of a move's outcome are held the
/// same way, even where the register's chains reach nothing.
// verifies: REQ-AUZ-4, REQ-AUZ-8
#[test]
fn two_agents_of_one_party_count_as_one_actor() {
    let (first, second) = (agent("desk-x", "agent-x1"), agent("desk-x", "agent-x2"));
    let mut register = Register::of_the_world()
        .with(&first)
        .with(&second)
        .with(&person("desk-x", "desk-x-lead"))
        .with(&agent("desk-z", "agent-z1"));
    register.link("desk-x", OPERATOR);
    register.link("desk-z", OPERATOR);
    let (mut policy, request) = engine("sd", register);
    let decided = policy
        .decide_stuck(&decision_by(&request, &first, "req-by-agent"))
        .unwrap();
    assert!(decided.permit().is_some(), "{decided:?}");
    for (n, mover) in [second.clone(), person("desk-x", "desk-x-lead")]
        .iter()
        .enumerate()
    {
        let moved = policy
            .move_stuck(&move_of(&decided, &format!("req-move-{n}"), mover))
            .unwrap();
        assert_eq!(moved.outcome, refuse(RefusalClass::SameActor), "{mover:?}");
    }
    let moved = policy
        .move_stuck(&move_of(
            &decided,
            "req-move-z",
            &agent("desk-z", "agent-z1"),
        ))
        .unwrap();
    assert!(moved.permit().is_some(), "{moved:?}");

    // The world's report is `op-login-5`'s; here an agent of `desk-x` reported it, and another of `desk-x` confirms.
    let mut register = Register::of_the_world().with(&first).with(&second);
    register.reaches_nothing = true;
    let (mut policy, request) = engine("settled_es", register);
    let confirmation = confirmation_by(&request, &second, "req-confirm");
    report_by(&mut policy.log, &confirmation.report, &first);
    assert_eq!(
        policy.confirm_outcome(&confirmation).unwrap().outcome,
        refuse(RefusalClass::SameActor)
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

/// A resolution naming no evidence is refused: one naming none at all fails its check, unrecorded; one naming
/// evidence the store keeps nothing under, or keeps about another order, is refused and recorded, even beside
/// evidence it keeps; and so is one whose evidence is all about no order in particular. One naming evidence about its
/// order, beside evidence about none, is permitted.
// verifies: REQ-AUZ-4
#[test]
fn resolution_naming_no_evidence_is_refused() {
    let mut register = Register::of_the_world();
    register.kept.push((digest("ee"), Some(id("ord-2"))));
    register.kept.push((digest("ef"), None));
    let (mut policy, request) = engine("sd", register);
    let named = |evidence: Vec<Digest>, request_id: &str| StuckDecide {
        evidence,
        ..decision_by(&request, &alice(), request_id)
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
        vec![digest("ef")],
    ]
    .into_iter()
    .enumerate()
    {
        let decided = policy
            .decide_stuck(&named(evidence.clone(), &format!("req-{n}")))
            .unwrap();
        assert_eq!(
            decided.outcome,
            refuse(RefusalClass::MissingInput),
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
/// own step. A stuck order's steps are the operator's: no other role decides or moves, whatever its register says, and
/// stating the operator's role is not enough for a login the register doesn't give the operator.
// verifies: REQ-AUZ-4, THR-77
#[test]
fn deciding_moving_and_confirming_are_separate_permissions() {
    let (decides, moves, confirms) = (
        person("desk-d", "only-decides"),
        person("desk-m", "only-moves"),
        person("desk-c", "only-confirms"),
    );
    let register = || {
        let mut register = Register::of_the_world();
        for (actor, permission) in [
            (&decides, Permission::DecideStuck),
            (&moves, Permission::MoveStuck),
            (&confirms, Permission::ConfirmOutcome),
        ] {
            register.list(actor.acts.as_str(), actor.party.as_str(), ActorKind::Person);
            register.permissions.push((actor.acts.clone(), permission));
            register.link(actor.party.as_str(), OPERATOR);
        }
        register
    };
    let provider = Actor {
        role: Role::Provider,
        ..person("desk-p", "provider-desk")
    };
    let outsider = person("acme", "acme-staff");
    let mut with_others = register().with(&provider).with(&outsider);
    with_others.link("desk-p", OPERATOR);
    let (mut policy, request) = engine("sd", with_others);
    let not_permitted = refuse(RefusalClass::NotPermitted);

    for decider in [&moves, &confirms, &provider, &outsider] {
        let decide = decision_by(
            &request,
            decider,
            &format!("req-by-{}", decider.acts.as_str()),
        );
        assert_eq!(
            policy.decide_stuck(&decide).unwrap().outcome,
            not_permitted,
            "{decider:?}"
        );
    }
    let decided = policy
        .decide_stuck(&decision_by(&request, &decides, "req-decides"))
        .unwrap();
    assert!(decided.permit().is_some(), "{decided:?}");
    for mover in [&decides, &confirms, &provider, &outsider] {
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

    let (mut policy, request) = engine("settled_es", register());
    for confirmer in [&decides, &moves] {
        let c = confirmation_by(
            &request,
            confirmer,
            &format!("req-{}", confirmer.acts.as_str()),
        );
        assert_eq!(policy.confirm_outcome(&c).unwrap().outcome, not_permitted);
    }
    let confirmed = policy
        .confirm_outcome(&confirmation_by(&request, &confirms, "req-confirms"))
        .unwrap();
    assert!(confirmed.permit().is_some(), "{confirmed:?}");
}

/// `THR-70`'s colluding customer and provider. No party of the order decides its resolution or confirms a move's
/// outcome, nor any actor whose chain reaches one, even one the operator's too and holding every permission. The
/// provider holding the funds decides nothing, nor does an actor reaching it. No actor that is the holder's desk, by
/// its party or by its chain reaching the holder, moves the value or confirms its outcome; an operator's desk whose
/// chain meets the holder's only at the operator moves it.
// verifies: REQ-AUZ-4, THR-70
#[test]
fn neither_the_customer_nor_the_holders_desk_resolves_its_order() {
    let customer = person("cust-1", "cust-1-staff");
    let customers_ops = person("cust-1-ops", "cust-1-ops-1");
    let (holders, chained) = (
        person("bank-co", "bank-co-desk"),
        person("bank-co-ops", "bank-co-ops-1"),
    );
    let register = || {
        let mut register = Register::of_the_world()
            .with(&customer)
            .with(&customers_ops)
            .with(&holders)
            .with(&chained);
        // Each is the operator's too, so only this test's rules refuse them; `bank-co` and the operator are one legal
        // party's, `REQ-CAT-20`.
        register.link("cust-1", OPERATOR);
        register.link("cust-1-ops", "cust-1");
        register.link("bank-co", OPERATOR);
        register.link("bank-co-ops", "bank-co");
        register
    };
    let not_permitted = refuse(RefusalClass::NotPermitted);

    let (mut policy, request) = engine("sd", register());
    for decider in [&customer, &customers_ops, &holders, &chained] {
        let decide = decision_by(
            &request,
            decider,
            &format!("req-by-{}", decider.acts.as_str()),
        );
        assert_eq!(
            policy.decide_stuck(&decide).unwrap().outcome,
            not_permitted,
            "{decider:?}"
        );
    }
    let decided = alices_decision(&mut policy, &request);
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
    let moved = policy
        .move_stuck(&move_of(&decided, "req-move-by-bob", &bob()))
        .unwrap();
    assert!(moved.permit().is_some(), "{moved:?}");

    // The holder's own desk is its desk by its party, and the customer's staff its by theirs, even where the
    // register's chains reach nothing.
    let mut apart = register();
    apart.reaches_nothing = true;
    let (mut policy, request) = engine("sd", apart);
    for decider in [&customer, &holders] {
        let decide = decision_by(
            &request,
            decider,
            &format!("req-apart-{}", decider.acts.as_str()),
        );
        assert_eq!(
            policy.decide_stuck(&decide).unwrap().outcome,
            not_permitted,
            "{decider:?}"
        );
    }
    let decided = alices_decision(&mut policy, &request);
    assert_eq!(
        policy
            .move_stuck(&move_of(&decided, "req-move-by-the-holder", &holders))
            .unwrap()
            .outcome,
        not_permitted
    );

    // The fixed rule alone, asked before the deployment's policy, refuses the order's customer on the order's own
    // record, before any chain is read from the log.
    let (mut policy, request) = engine("settled_es", register());
    let confirmation = confirmation_by(&request, &customer, "req-fixed-confirm");
    assert_eq!(
        policy.consult(Asked::ConfirmOutcome(&confirmation)),
        not_permitted
    );
    let (mut policy, request) = engine("sd", register());
    let decide = decision_by(&request, &customer, "req-fixed-decide");
    assert_eq!(policy.consult(Asked::DecideStuck(&decide)), not_permitted);

    // Both ways: a party of the order, or the holder, whose chain reaches the actor's party is the actor's too. The
    // customer's holding company decides and confirms nothing, the holder's group confirms nothing and decides nothing,
    // but the group's staff may move, the mover's holder check being one way (`REQ-CAT-20`).
    let (holdco, group) = (
        person("cust-holdco", "holdco-staff"),
        person("bank-group", "group-staff"),
    );
    let upward = || {
        let mut register = register().with(&holdco).with(&group);
        register.link("cust-1", "cust-holdco");
        register.link("cust-holdco", OPERATOR);
        register.link("bank-co", "bank-group");
        register.link("bank-group", OPERATOR);
        register
    };
    let (mut policy, request) = engine("sd", upward());
    for decider in [&holdco, &group] {
        let decide = decision_by(
            &request,
            decider,
            &format!("req-up-{}", decider.acts.as_str()),
        );
        assert_eq!(
            policy.decide_stuck(&decide).unwrap().outcome,
            not_permitted,
            "{decider:?}"
        );
    }
    let decided = alices_decision(&mut policy, &request);
    let moved = policy
        .move_stuck(&move_of(&decided, "req-move-by-the-group", &group))
        .unwrap();
    assert!(moved.permit().is_some(), "{moved:?}");
    let (mut policy, request) = engine("settled_es", upward());
    for confirmer in [&holdco, &group] {
        let c = confirmation_by(
            &request,
            confirmer,
            &format!("req-up-{}", confirmer.acts.as_str()),
        );
        assert_eq!(
            policy.confirm_outcome(&c).unwrap().outcome,
            not_permitted,
            "{confirmer:?}"
        );
    }

    // Sisters (the review of `fa19450`, round 3): a company under the customer's holding company, or under the
    // holder's group, meets the customer's or the holder's chain below the operator, and decides and confirms
    // nothing; the group's staff still move, the mover's holder check being one way.
    let (cust_sister, bank_sister) = (
        person("cust-sister", "cust-sister-1"),
        person("bank-sister", "bank-sister-1"),
    );
    let sisters = || {
        let mut register = upward().with(&cust_sister).with(&bank_sister);
        register.link("cust-sister", "cust-holdco");
        register.link("bank-sister", "bank-group");
        register
    };
    let (mut policy, request) = engine("sd", sisters());
    for decider in [&cust_sister, &bank_sister] {
        let decide = decision_by(
            &request,
            decider,
            &format!("req-sister-{}", decider.acts.as_str()),
        );
        assert_eq!(
            policy.decide_stuck(&decide).unwrap().outcome,
            not_permitted,
            "{decider:?}"
        );
    }
    let decided = alices_decision(&mut policy, &request);
    let moved = policy
        .move_stuck(&move_of(&decided, "req-move-by-a-sister", &bank_sister))
        .unwrap();
    assert!(moved.permit().is_some(), "{moved:?}");
    let (mut policy, request) = engine("settled_es", sisters());
    for confirmer in [&cust_sister, &bank_sister] {
        let c = confirmation_by(
            &request,
            confirmer,
            &format!("req-sister-{}", confirmer.acts.as_str()),
        );
        assert_eq!(
            policy.confirm_outcome(&c).unwrap().outcome,
            not_permitted,
            "{confirmer:?}"
        );
    }
    let confirmed = policy
        .confirm_outcome(&confirmation_by(&request, &alice(), "req-beside-sisters"))
        .unwrap();
    assert!(confirmed.permit().is_some(), "{confirmed:?}");

    // The world's resolution is delivered by `bank-co`; its report is `op-login-5`'s.
    let (mut policy, request) = engine("settled_es", register());
    for confirmer in [&customer, &customers_ops, &holders, &chained] {
        let c = confirmation_by(
            &request,
            confirmer,
            &format!("req-{}", confirmer.acts.as_str()),
        );
        assert_eq!(
            policy.confirm_outcome(&c).unwrap().outcome,
            not_permitted,
            "{confirmer:?}"
        );
    }
    let confirmed = policy
        .confirm_outcome(&confirmation_by(&request, &alice(), "req-by-alice"))
        .unwrap();
    assert!(confirmed.permit().is_some(), "{confirmed:?}");

    // A segment's silent `move_value`, the world's withdrawal at `us-bank`, reported by Core's own actor: the provider
    // it moved through holds the funds, and its desk doesn't confirm it.
    let us_bank = person("us-bank", "us-bank-desk");
    let (mut policy, request) = engine("settled_es", register().with(&us_bank));
    let world = confirmation_by(&request, &alice(), "req-world");
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
        .confirm_outcome(&of_the_withdrawal("req-by-alice", &alice()))
        .unwrap();
    assert!(confirmed.permit().is_some(), "{confirmed:?}");
}

/// A move's outcome is confirmed only by another actor than its reporter: one of the reporter's own login or party,
/// one whose chain reaches the reporter's party, and one whose party the reporter's chain reaches are each refused.
/// The operator's staff on sibling desks, whose chains meet the reporter's only at the operator, confirm it
/// (core-internal's README, `T-021-06`).
// verifies: REQ-AUZ-4, THR-77
#[test]
fn confirmer_whose_chain_reaches_the_reporters_is_refused() {
    // The world's report is `op-login-5`'s, for `operator-desk-b`.
    let ask = |register: Register, confirmer: &Actor| {
        let (mut policy, request) = engine("settled_es", register);
        policy
            .confirm_outcome(&confirmation_by(&request, confirmer, "req-confirm"))
            .unwrap()
            .outcome
    };
    for confirmer in [
        person("operator-desk-b", "op-login-9"),
        person("operator-desk-x", "op-login-5"),
        person("desk-b-night", "night-1"),
        person(OPERATOR, "op-login-2"),
        person("desk-b-twin", "twin-1"),
    ] {
        let mut register = Register::of_the_world().with(&confirmer);
        register.link("desk-b-night", "operator-desk-b");
        // The reporter's desk and a twin under one division of the operator meet below it.
        register.link("operator-desk-b", "division-b");
        register.link("desk-b-twin", "division-b");
        register.link("division-b", OPERATOR);
        assert_eq!(
            ask(register, &confirmer),
            refuse(RefusalClass::SameActor),
            "{confirmer:?}"
        );
    }

    for sibling in [alice(), person("operator-desk-c", "carol")] {
        let mut register = Register::of_the_world().with(&sibling);
        register.link("operator-desk-c", OPERATOR);
        assert!(
            matches!(ask(register, &sibling), Answer::Permit {}),
            "{sibling:?}"
        );
    }
}

/// A move that carries out no permitted decision of the log is refused: one naming a decision the log never took, one
/// naming another request than the decision answered, one of another order, and one carrying out a refused decision.
/// A confirmation is refused citing what is no report, a report of another outcome, one of another move than its own,
/// one of another order, or one the log holds as refused.
// verifies: REQ-AUZ-4
#[test]
fn step_with_nothing_permitted_behind_it_is_refused() {
    let not_permitted = refuse(RefusalClass::NotPermitted);
    let (mut policy, request) = engine("sd", Register::of_the_world());
    let decided = alices_decision(&mut policy, &request);
    let mut never = move_of(&decided, "req-never", &bob());
    never.carries_out.decision = id("dec-never");
    let mut other_request = move_of(&decided, "req-other-request", &bob());
    other_request.carries_out.request = id("req-other");
    let mut elsewhere = move_of(&decided, "req-elsewhere", &bob());
    elsewhere.order = id("ord-2");
    let mut unkept = decision_by(&request, &alice(), "req-unkept");
    unkept.evidence = vec![digest("aa")];
    let refused_decision = policy.decide_stuck(&unkept).unwrap();
    assert!(refused_decision.permit().is_none());
    for step in [
        never,
        other_request,
        elsewhere,
        move_of(&refused_decision, "req-of-refused", &bob()),
    ] {
        assert_eq!(
            policy.move_stuck(&step).unwrap().outcome,
            not_permitted,
            "{step:?}"
        );
    }

    let (mut policy, request) = engine("settled_es", Register::of_the_world());
    let confirmation = confirmation_by(&request, &alice(), "req-confirm");
    let first_effect = policy
        .log
        .entries
        .iter()
        .find(|(_, e)| matches!(e, Entry::Verified(v) if matches!(v.what, What::Effect { .. })))
        .map(|(r, _)| r.clone())
        .unwrap();
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
    for c in [
        OutcomeConfirmation {
            report: first_effect.clone(),
            ..confirmation.clone()
        },
        OutcomeConfirmation {
            outcome: MoveOutcome::Done,
            ..confirmation.clone()
        },
        OutcomeConfirmation {
            effect: first_effect,
            ..confirmation.clone()
        },
        OutcomeConfirmation {
            order: id("ord-2"),
            ..confirmation.clone()
        },
        OutcomeConfirmation {
            report: refused_report.receipt.clone(),
            ..confirmation.clone()
        },
    ] {
        assert_eq!(
            policy.confirm_outcome(&c).unwrap().outcome,
            not_permitted,
            "{c:?}"
        );
    }
}

/// What policy can't read or reach moves nothing: a log it can't read refuses a decision, a move and a confirmation as
/// missing the input they need, and so does an order the log has no acceptance of; a register of chains out of reach
/// leaves a confirmation not known.
// verifies: REQ-AUZ-5, REQ-AUZ-4
#[test]
fn unread_log_or_unreached_register_moves_nothing() {
    let missing = refuse(RefusalClass::MissingInput);
    let mut register = Register::of_the_world();
    register.kept.push((digest("dc"), Some(id("ord-9"))));
    let (mut policy, request) = engine("sd", register);
    let decided = alices_decision(&mut policy, &request);
    let never_accepted = StuckDecide {
        order: id("ord-9"),
        evidence: vec![digest("dc")],
        ..decision_by(&request, &alice(), "req-never-accepted")
    };
    assert_eq!(
        policy.decide_stuck(&never_accepted).unwrap().outcome,
        missing
    );
    policy.log.unreadable = true;
    assert_eq!(
        policy
            .decide_stuck(&decision_by(&request, &alice(), "req-unread"))
            .unwrap()
            .outcome,
        missing
    );
    let moved = policy
        .move_stuck(&move_of(&decided, "req-move", &bob()))
        .unwrap();
    assert_eq!(moved.outcome, missing);

    let (mut policy, request) = engine("settled_es", Register::of_the_world());
    let confirmation = confirmation_by(&request, &alice(), "req-confirm");
    policy.log.unreadable = true;
    assert_eq!(
        policy.confirm_outcome(&confirmation).unwrap().outcome,
        missing
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

/// Core has a party of its own, apart from the deployment operator's, so Core is never the operator's: its login never
/// decides a stuck order or moves its value, even stating the operator's role and holding every permission.
// verifies: REQ-AUZ-4, REQ-ORD-28
#[test]
fn core_never_decides_or_moves() {
    let core = Actor {
        party: id("rutent"),
        acts: id("core-orch"),
        role: Role::Operator,
        kind: ActorKind::Component,
    };
    let (mut policy, request) = engine("sd", Register::of_the_world().with(&core));
    let not_permitted = refuse(RefusalClass::NotPermitted);
    let decide = decision_by(&request, &core, "req-by-core");
    assert_eq!(policy.decide_stuck(&decide).unwrap().outcome, not_permitted);
    let decided = alices_decision(&mut policy, &request);
    assert_eq!(
        policy
            .move_stuck(&move_of(&decided, "req-move-by-core", &core))
            .unwrap()
            .outcome,
        not_permitted
    );
}
