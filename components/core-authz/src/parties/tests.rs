//! `T-021-05`'s tests: every agent acts for an accountable party other than itself, within the scope that party set,
//! and the limits on work and on asking about people bind at that party, fanned out requests included.

use super::*;
use crate::session::Stopped;
use rutent_contracts::core_internal::calls::{DecisionEntry, Decisions};
use rutent_contracts::core_internal::{
    Authorisation, Count, DecisionRequest, Entry, NotKnownCause, Outcome, PermissionChange,
    PersonQuestion, ReadCheck, Receipt, Role, SessionRequest, StuckDecide, StuckMove,
};
use rutent_contracts::router_api::Timestamp;
use serde_json::{Value, json};

fn read_fixture(path: &str) -> Value {
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/{path}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A valid fixture of the contract, read as whatever type the call site wants.
macro_rules! valid {
    ($name:expr) => {
        serde_json::from_value(read_fixture(&format!("valid/{}", $name))).unwrap()
    };
}

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn at(text: &str) -> Timestamp {
    Timestamp::new(text).unwrap()
}

/// The moment the fixtures are asked at, which Core's clock reads in these tests.
const NOW: &str = "2026-11-10T09:00:00Z";

fn agent(party: &str, acts: &str) -> Actor {
    Actor {
        party: id(party),
        acts: id(acts),
        role: Role::Customer,
        kind: ActorKind::Agent,
    }
}

fn person(party: &str, acts: &str) -> Actor {
    Actor {
        kind: ActorKind::Person,
        ..agent(party, acts)
    }
}

fn safe(n: u64) -> SafeInteger {
    SafeInteger::try_from(n).unwrap()
}

/// A log reached through policy's handle that keeps every entry it took, and takes none once it is set to refuse.
#[derive(Default)]
struct Kept {
    entries: Vec<Entry>,
    refusing: bool,
    /// Its read fails.
    unreadable: bool,
    /// What its read of charges answers in place of what it holds, where set: a stale, truncated or other log's.
    read_instead: Option<Vec<Entry>>,
}

/// The log's read, on policy's handle: what it holds as permitted (`T-021-04`) and as counted.
impl crate::funds::Recorded for Kept {
    fn permitted(&mut self) -> Result<Vec<Authorisation>, crate::funds::ReadFailure> {
        if self.unreadable {
            return Err(crate::funds::ReadFailure::Unavailable);
        }
        Ok(crate::funds::permitted_in(&self.entries))
    }

    fn charged(&mut self) -> Result<Vec<Counted>, crate::funds::ReadFailure> {
        if self.unreadable {
            return Err(crate::funds::ReadFailure::Unavailable);
        }
        Ok(charged_in(
            self.read_instead.as_ref().unwrap_or(&self.entries),
        ))
    }
}

impl PolicyLog for Kept {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        if self.refusing {
            return Err(AppendFailure::Unavailable);
        }
        let entry = Entry::Verified(entry.entry().clone());
        entry.check().map_err(AppendFailure::Refused)?;
        let index = 2_000 + self.entries.len();
        self.entries.push(entry);
        Ok(serde_json::from_value(json!({
            "log": "log-main",
            "index": index,
            "entry": {"algorithm": "sha-256", "value": format!("{index:064x}")},
        }))
        .unwrap())
    }
}

impl Kept {
    /// The outcome of the last entry the log took.
    fn last(&self) -> Outcome {
        match self.entries.last() {
            Some(Entry::Verified(entry)) => entry.outcome,
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

/// The deployment's registers: who each actor acts for (`ActsFor`) and what scope each party set for its agents
/// (`AgentScope`), each asked kept. Every other row's input says yes, so what these tests decide is this row's rule.
#[derive(Default)]
struct Registers {
    /// Each actor listed, by its `acts`, with the party it acts for and the kind of actor it is. An actor not listed
    /// is a missing input.
    acts_for: Vec<(Id, Id, ActorKind)>,
    /// Each agent a party listed, with the calls in its scope. An agent not listed is a missing input.
    scopes: Vec<(Id, Id, Vec<Scope>)>,
    /// The scopes asked, in order.
    asked: Vec<Need>,
}

impl Registers {
    /// Registers listing every actor of the fixtures as acting for its own party.
    fn of_the_fixtures() -> Self {
        let mut registers = Registers::default();
        for (acts, party, kind) in [
            ("core-orch", "rutent", ActorKind::Component),
            ("op-login-2", "operator", ActorKind::Person),
            ("op-login-5", "operator-desk-b", ActorKind::Person),
            ("examiner-3", "fin-regulator", ActorKind::Person),
        ] {
            registers.acts_for.push((id(acts), id(party), kind));
        }
        registers
    }

    /// `actor` listed as acting for its party, and, where `scope` is some, in that scope of it.
    fn with(mut self, actor: &Actor, scope: Option<Vec<Scope>>) -> Self {
        self.acts_for
            .push((actor.acts.clone(), actor.party.clone(), actor.kind));
        if let Some(scope) = scope {
            self.scopes
                .push((actor.party.clone(), actor.acts.clone(), scope));
        }
        self
    }
}

/// Every call an agent can make.
fn every_scope() -> Vec<Scope> {
    vec![
        Scope::MoveValue,
        Scope::Authorise,
        Scope::DecideStuck,
        Scope::MoveStuck,
        Scope::ReportOutcome,
        Scope::ConfirmOutcome,
        Scope::OpenSession,
        Scope::CheckRead,
        Scope::GrantPermission,
        Scope::RevokePermission,
        Scope::Charge(Work::Request),
        Scope::Charge(Work::FanOut),
        Scope::Charge(Work::PersonQuestion),
        Scope::AskAboutPerson,
    ]
}

impl Sources for Registers {
    fn supply(&mut self, need: &Need) -> Reply<Said> {
        let yes = |b: bool| Reply::Given(if b { Said::Yes } else { Said::No });
        match &need.from {
            Source::ActsFor { acts, party, kind } => {
                match self.acts_for.iter().find(|(a, _, _)| a == acts) {
                    Some((_, p, k)) => yes(p == party && k == kind),
                    None => Reply::Missing,
                }
            }
            Source::AgentScope { party, scope } => {
                self.asked.push(need.clone());
                match self
                    .scopes
                    .iter()
                    .find(|(p, a, _)| p == party && a == &need.input)
                {
                    Some((_, _, scopes)) => yes(scopes.contains(scope)),
                    None => Reply::Missing,
                }
            }
            // The read fixture's order, `ord-19`, is its reader's; no caller here is a party of the stuck fixtures'
            // orders, whose decider and confirmer never are (`T-021-06`).
            Source::OrderParties(order) => yes(order.as_str() == "ord-19"),
            _ => Reply::Given(Said::Yes),
        }
    }
}

type Tested = Engine<Kept, Permits, Registers>;

/// The engine over `registers`, its clock stopped at [`NOW`], with `bounds`.
fn engine(registers: Registers, bounds: Uniform) -> Tested {
    let mut engine = Engine::with_policy(Kept::default(), Permits, registers);
    engine.set_clock(Stopped::at(&at(NOW)));
    engine.set_bounds(Box::new(bounds));
    engine
}

fn bounds(work: u64, people: u64) -> Uniform {
    Uniform {
        work: safe(work),
        people: safe(people),
    }
}

/// A charge of `count` of `work` by `caller`, to its party, under the id `request`.
fn charge(request: &str, caller: &Actor, work: Work, count: u64) -> Charge {
    Charge {
        request: id(request),
        party: caller.party.clone(),
        caller: caller.clone(),
        work,
        count: Count::new(count).unwrap(),
        order: (work == Work::FanOut).then(|| id("ord-19")),
        at: at(NOW),
    }
}

/// One request of each policy call, each from the fixtures, with `caller` in the place of who asks.
struct Calls {
    decide: DecisionRequest,
    authorise: Authorisation,
    decide_stuck: StuckDecide,
    move_stuck: StuckMove,
    open_session: SessionRequest,
    check_read: ReadCheck,
    grant: PermissionChange,
    charge: Charge,
    ask: PersonQuestion,
    report: rutent_contracts::core_internal::OutcomeReport,
    confirm: rutent_contracts::core_internal::OutcomeConfirmation,
}

/// The request a fixture's verified entry holds, as JSON, which the call site reads as its type.
fn request_of(entry: &str) -> Value {
    read_fixture(&format!("valid/{entry}"))["what"]["request"].clone()
}

impl Calls {
    fn by(caller: &Actor) -> Self {
        let mut calls = Calls {
            decide: valid!("decision-request-to-move-value"),
            authorise: valid!("authorisation-of-one-order-given-by-an-agent"),
            decide_stuck: valid!("stuck-decide-naming-its-evidence"),
            move_stuck: valid!("stuck-move-carrying-out-a-decision"),
            open_session: valid!("session-request-of-a-delegate"),
            check_read: valid!("read-check-by-an-agent-in-its-partys-role"),
            grant: valid!("permission-change-granting-a-regulator-the-trail"),
            charge: valid!("charge-of-fanned-out-work"),
            ask: valid!("person-question-on-the-persons-authority-first-asked"),
            report: serde_json::from_value(request_of("verified-entry-of-an-outcome-reported"))
                .unwrap(),
            confirm: serde_json::from_value(request_of("verified-entry-of-an-outcome-confirmed"))
                .unwrap(),
        };
        calls.decide.caller = caller.clone();
        calls.authorise.given_by = caller.clone();
        // A stuck order's steps are the operator's (`T-021-06`): the caller takes them in that role, as the party and
        // the kind it is.
        let operator = Actor {
            role: Role::Operator,
            ..caller.clone()
        };
        calls.decide_stuck.decider = operator.clone();
        calls.move_stuck.mover = operator;
        calls.open_session.requester = caller.clone();
        calls.check_read.reader = caller.clone();
        calls.grant.by = caller.clone();
        calls.charge.caller = caller.clone();
        calls.charge.party = caller.party.clone();
        calls.ask.asker = caller.clone();
        calls.report.reporter = caller.clone();
        calls.confirm.confirmer = caller.clone();
        calls
    }

    /// Each call, by its name.
    fn each(&self) -> Vec<(&'static str, Asked<'_>)> {
        vec![
            ("decide", Asked::Decide(&self.decide)),
            ("authorise", Asked::Authorise(&self.authorise)),
            ("decide_stuck", Asked::DecideStuck(&self.decide_stuck)),
            ("move_stuck", Asked::MoveStuck(&self.move_stuck)),
            ("open_session", Asked::OpenSession(&self.open_session)),
            ("check_read", Asked::CheckRead(&self.check_read)),
            ("grant_permission", Asked::GrantPermission(&self.grant)),
            ("revoke_permission", Asked::RevokePermission(&self.grant)),
            ("charge", Asked::Charge(&self.charge)),
            ("ask_about_person", Asked::AskAboutPerson(&self.ask)),
            ("report_outcome", Asked::ReportOutcome(&self.report)),
            ("confirm_outcome", Asked::ConfirmOutcome(&self.confirm)),
        ]
    }
}

const fn refused(class: RefusalClass) -> Answer {
    Answer::Refuse { class }
}

/// An agent that states itself its own accountable party is refused as acting for none on every call, even where the
/// register of actors and its own scope would say yes, and no scope is asked of it; so is an agent the register lists
/// for another party. One the register says nothing of is refused for the missing input. Its charge is over a bound
/// of 0 and counts nothing. A person that is its own party, and an agent of another party within its scope, pass.
// verifies: REQ-AUZ-8
#[test]
fn agent_without_an_accountable_party_is_refused() {
    let itself = agent("agent-7", "agent-7");
    let mut policy = engine(
        Registers::of_the_fixtures().with(&itself, Some(every_scope())),
        Uniform::default(),
    );
    let calls = Calls::by(&itself);
    for (name, asked) in calls.each() {
        assert_eq!(
            policy.consult(asked),
            refused(RefusalClass::NoAccountableParty),
            "{name}"
        );
    }
    assert!(policy.sources.asked.is_empty(), "no scope asked of it");
    let decision = policy.decide(&calls.decide).unwrap();
    assert!(decision.permit().is_none());
    assert_eq!(decision.outcome, refused(RefusalClass::NoAccountableParty));
    let answer = policy.charge(&calls.charge).unwrap();
    assert_eq!(
        answer.outcome,
        ChargeOutcome::Refused {
            class: RefusalClass::NoAccountableParty
        }
    );
    assert_eq!(
        policy.log.last(),
        Outcome::Refused {
            class: RefusalClass::NoAccountableParty
        },
        "the refusal's own class on the record, not a bound passed"
    );
    assert_eq!(policy.charged().used(&id("agent-7"), Limit::Work), 0);

    // Listed by the register for another party, or not at all.
    let elsewhere = agent("acme-treasury", "agent-8");
    let unknown = agent("acme-treasury", "agent-9");
    let mut registers = Registers::of_the_fixtures();
    registers
        .acts_for
        .push((id("agent-8"), id("other-co"), ActorKind::Agent));
    registers
        .scopes
        .push((id("acme-treasury"), id("agent-8"), every_scope()));
    registers
        .scopes
        .push((id("acme-treasury"), id("agent-9"), every_scope()));
    let mut policy = engine(registers, Uniform::default());
    for (who, class) in [
        (&elsewhere, RefusalClass::NoAccountableParty),
        (&unknown, RefusalClass::MissingInput),
    ] {
        let calls = Calls::by(who);
        for (name, asked) in calls.each() {
            assert_eq!(policy.consult(asked), refused(class), "{who:?} {name}");
        }
        let answer = policy.charge(&calls.charge).unwrap();
        assert_eq!(answer.outcome, ChargeOutcome::Refused { class });
    }
    assert_eq!(policy.charged().used(&id("acme-treasury"), Limit::Work), 0);

    // A person is its own party, and an agent of another party within its scope acts for it.
    let own = person("cust-1", "cust-1");
    let acme = agent("acme-treasury", "agent-7");
    let mut policy = engine(
        Registers::of_the_fixtures()
            .with(&own, None)
            .with(&acme, Some(every_scope())),
        Uniform::default(),
    );
    for who in [&own, &acme] {
        let calls = Calls::by(who);
        for (name, asked) in calls.each() {
            assert_eq!(policy.consult(asked), Answer::Permit {}, "{who:?} {name}");
        }
        // Each charge its own id: one id names one charge.
        let mut own_charge = calls.charge.clone();
        own_charge.request = id(&format!("req-of-{}", who.acts.as_str()));
        let answer = policy.charge(&own_charge).unwrap();
        assert!(matches!(answer.outcome, ChargeOutcome::Within { .. }));
    }
}

/// A thousand agents of one party, each charging its own work or its own question about a person, share the party's
/// one bound on each: the work past the bound is refused whichever agent asks, the party's person included, and the
/// questions past theirs the same. Another party's agent is held to its own bound alone.
// verifies: AC-18, REQ-AUZ-8
#[test]
fn thousand_agents_under_one_party_hit_one_limit() {
    let agents: Vec<Actor> = (0..1_000)
        .map(|n| agent("acme-treasury", &format!("agent-{n}")))
        .collect();
    let late = agent("acme-treasury", "agent-1000");
    let own = person("acme-treasury", "treasurer-1");
    let other = agent("other-co", "agent-0");
    let mut registers = Registers::of_the_fixtures()
        .with(&late, Some(every_scope()))
        .with(&own, None);
    for a in &agents {
        registers = registers.with(a, Some(every_scope()));
    }
    let mut policy = engine(registers, bounds(1_000, 10));

    for (n, a) in agents.iter().enumerate() {
        let n = u64::try_from(n).unwrap();
        let answer = policy
            .charge(&charge(&format!("work-{n}"), a, Work::Request, 1))
            .unwrap();
        assert_eq!(
            answer.outcome,
            ChargeOutcome::Within {
                remaining: safe(999 - n)
            },
            "agent {n}"
        );
    }
    for (request, who) in [
        ("work-late", &late),
        ("work-own", &own),
        ("work-0b", &agents[0]),
    ] {
        let answer = policy
            .charge(&charge(request, who, Work::Request, 1))
            .unwrap();
        assert_eq!(
            answer.outcome,
            ChargeOutcome::Over { bound: safe(1_000) },
            "{who:?}"
        );
        assert_eq!(
            policy.log.last(),
            Outcome::Refused {
                class: RefusalClass::OverBound
            }
        );
    }
    assert_eq!(
        policy.charged().used(&id("acme-treasury"), Limit::Work),
        1_000
    );

    let mut within = 0;
    for (n, a) in agents.iter().enumerate() {
        let answer = policy
            .charge(&charge(&format!("person-{n}"), a, Work::PersonQuestion, 1))
            .unwrap();
        if matches!(answer.outcome, ChargeOutcome::Within { .. }) {
            within += 1;
        } else {
            assert_eq!(answer.outcome, ChargeOutcome::Over { bound: safe(10) });
        }
    }
    assert_eq!(
        within, 10,
        "ten questions about people, whichever agents ask"
    );
    assert_eq!(
        policy.charged().used(&id("acme-treasury"), Limit::People),
        10
    );

    // `other-co`'s agent shares an id with one of acme's, and the register lists actors by `acts`, so it is listed in
    // a register of its own: its party's count starts at nothing.
    let mut policy_of_other = engine(
        Registers::of_the_fixtures().with(&other, Some(every_scope())),
        bounds(1_000, 10),
    );
    let answer = policy_of_other
        .charge(&charge("work-other", &other, Work::Request, 1))
        .unwrap();
    assert_eq!(
        answer.outcome,
        ChargeOutcome::Within {
            remaining: safe(999)
        }
    );
    // In one engine too, another party's count is its own.
    let mut both = Registers::of_the_fixtures().with(&agents[1], Some(every_scope()));
    let elsewhere = agent("other-co", "agent-x");
    both = both.with(&elsewhere, Some(every_scope()));
    let mut policy = engine(both, bounds(3, 10));
    for n in 0..3 {
        let answer = policy
            .charge(&charge(&format!("a-{n}"), &agents[1], Work::Request, 1))
            .unwrap();
        assert!(matches!(answer.outcome, ChargeOutcome::Within { .. }));
    }
    let answer = policy
        .charge(&charge("x-0", &elsewhere, Work::Request, 1))
        .unwrap();
    assert_eq!(answer.outcome, ChargeOutcome::Within { remaining: safe(2) });
}

/// The requests an order fans out to providers count against the same bound as its caller's own requests, at the
/// caller's party, whichever of its agents caused them; a fan-out past the bound counts nothing and its work is not
/// done, and the party's questions about people are a bound of their own. Another party's work is apart.
// verifies: REQ-OPS-1
#[test]
fn requests_fanned_out_to_providers_count_against_the_callers_party() {
    let first = agent("acme-treasury", "agent-7");
    let second = agent("acme-treasury", "agent-8");
    let other = agent("other-co", "agent-1");
    let mut policy = engine(
        Registers::of_the_fixtures()
            .with(&first, Some(every_scope()))
            .with(&second, Some(every_scope()))
            .with(&other, Some(every_scope())),
        bounds(10, 2),
    );
    let within = |n| ChargeOutcome::Within { remaining: safe(n) };

    let mut steps = vec![
        (charge("req-1", &first, Work::Request, 1), within(9)),
        (charge("fan-1", &first, Work::FanOut, 4), within(5)),
        // Past the bound by one: refused, and nothing of it counted.
        (
            charge("fan-2", &second, Work::FanOut, 6),
            ChargeOutcome::Over { bound: safe(10) },
        ),
        (charge("fan-3", &second, Work::FanOut, 5), within(0)),
        (
            charge("req-2", &first, Work::Request, 1),
            ChargeOutcome::Over { bound: safe(10) },
        ),
        // Questions about people are held to a bound of their own, which the fan-out didn't touch.
        (charge("ask-1", &second, Work::PersonQuestion, 2), within(0)),
        // Another party's fan-out is its own.
        (charge("fan-4", &other, Work::FanOut, 10), within(0)),
    ];
    for (n, (asked, expected)) in steps.drain(..).enumerate() {
        let answer = policy.charge(&asked).unwrap();
        assert_eq!(answer.outcome, expected, "step {n}");
        assert_eq!(policy.log.last(), expected.outcome(), "step {n}");
    }
    assert_eq!(policy.charged().used(&id("acme-treasury"), Limit::Work), 10);
    assert_eq!(
        policy.charged().used(&id("acme-treasury"), Limit::People),
        2
    );
    assert_eq!(policy.charged().used(&id("other-co"), Limit::Work), 10);

    // The fixture's fan-out, by acme's agent, counts against acme.
    let mut policy = engine(
        Registers::of_the_fixtures().with(&first, Some(every_scope())),
        bounds(10, 2),
    );
    let fanned: Charge = valid!("charge-of-fanned-out-work");
    assert_eq!(policy.charge(&fanned).unwrap().outcome, within(6));
    assert_eq!(policy.charged().used(&id("acme-treasury"), Limit::Work), 4);
}

/// An agent acts only within the scope its party set: a call outside it is refused as not permitted, the
/// authorisation to move funds and the customer's new destination among them, and its charge is over a bound of 0;
/// a party that set no scope for the agent gives a missing input; an unreachable register gives not known. Another
/// agent of the party, scoped for the call, and its person, who is asked no scope, pass.
// verifies: REQ-AUZ-8
#[test]
fn agent_outside_its_partys_scope_is_refused() {
    let narrow = agent("acme-treasury", "agent-7");
    let wide = agent("acme-treasury", "agent-8");
    let unscoped = agent("acme-treasury", "agent-9");
    let own = person("acme-treasury", "treasurer-1");
    let mut policy = engine(
        Registers::of_the_fixtures()
            .with(&narrow, Some(vec![Scope::Charge(Work::Request)]))
            .with(&wide, Some(vec![Scope::Authorise, Scope::MoveValue]))
            .with(&unscoped, None)
            .with(&own, None),
        Uniform::default(),
    );

    let calls = Calls::by(&narrow);
    for (name, asked) in calls.each() {
        assert_eq!(
            policy.consult(asked),
            refused(RefusalClass::NotPermitted),
            "{name}"
        );
    }
    let decision = policy.decide(&calls.decide).unwrap();
    assert_eq!(decision.outcome, refused(RefusalClass::NotPermitted));
    assert!(decision.permit().is_none());
    let mut new_destination = calls.authorise.clone();
    new_destination.authorisation = id("auth-20");
    new_destination.destination.account = id("acct-ref-10");
    assert_eq!(
        policy.consult(Asked::Authorise(&new_destination)),
        refused(RefusalClass::NotPermitted)
    );
    let answer = policy
        .charge(&charge("fan-1", &narrow, Work::FanOut, 1))
        .unwrap();
    assert_eq!(
        answer.outcome,
        ChargeOutcome::Refused {
            class: RefusalClass::NotPermitted
        }
    );
    let answer = policy
        .charge(&charge("req-1", &narrow, Work::Request, 1))
        .unwrap();
    assert!(matches!(answer.outcome, ChargeOutcome::Within { .. }));
    let scope_of = |need: &Need| match &need.from {
        Source::AgentScope { party, scope } => (need.input.clone(), party.clone(), *scope),
        _ => panic!("only scopes are kept"),
    };
    assert_eq!(
        scope_of(policy.sources.asked.last().unwrap()),
        (
            id("agent-7"),
            id("acme-treasury"),
            Scope::Charge(Work::Request)
        ),
        "the scope is asked of the agent, within its party"
    );

    // Through the engine's own `authorise` (`T-021-04`): the agent outside the scope is refused and recorded so, its
    // party's other agent, scoped for it, authorises.
    use rutent_contracts::core_internal::calls::Authorisations;
    let narrowly = policy.authorise(&Calls::by(&narrow).authorise).unwrap();
    assert_eq!(narrowly.outcome, refused(RefusalClass::NotPermitted));
    assert!(narrowly.permit().is_none());
    let calls = Calls::by(&wide);
    assert_eq!(
        policy.authorise(&calls.authorise).unwrap().outcome,
        Answer::Permit {}
    );
    assert_eq!(
        policy.consult(Asked::Authorise(&calls.authorise)),
        Answer::Permit {}
    );
    assert_eq!(
        policy.consult(Asked::Authorise(&new_destination_by(&wide))),
        Answer::Permit {}
    );
    assert_eq!(
        policy.consult(Asked::Decide(&calls.decide)),
        Answer::Permit {}
    );
    assert_eq!(
        policy.consult(Asked::OpenSession(&calls.open_session)),
        refused(RefusalClass::NotPermitted)
    );

    for (name, asked) in Calls::by(&unscoped).each() {
        assert_eq!(
            policy.consult(asked),
            refused(RefusalClass::MissingInput),
            "{name}"
        );
    }
    let asked = policy.sources.asked.len();
    for (name, asked) in Calls::by(&own).each() {
        assert_eq!(policy.consult(asked), Answer::Permit {}, "{name}");
    }
    assert_eq!(
        policy.sources.asked.len(),
        asked,
        "no scope asked of a person"
    );

    /// A register of scopes out of reach.
    struct Unreachable(Registers);
    impl Sources for Unreachable {
        fn supply(&mut self, need: &Need) -> Reply<Said> {
            match need.from {
                Source::AgentScope { .. } => Reply::Unreachable,
                _ => self.0.supply(need),
            }
        }
    }
    let mut policy = Engine::with_policy(
        Kept::default(),
        Permits,
        Unreachable(Registers::of_the_fixtures().with(&wide, Some(every_scope()))),
    );
    policy.set_clock(Stopped::at(&at(NOW)));
    let calls = Calls::by(&wide);
    for (name, asked) in calls.each() {
        assert_eq!(
            policy.consult(asked),
            Answer::NotKnown {
                cause: NotKnownCause::VerifierUnreachable
            },
            "{name}"
        );
    }
    let answer = policy.charge(&calls.charge).unwrap();
    assert_eq!(
        answer.outcome,
        ChargeOutcome::NotKnown {
            cause: NotKnownCause::VerifierUnreachable
        }
    );
    assert_eq!(
        policy.log.last(),
        Outcome::NotKnown {
            cause: NotKnownCause::VerifierUnreachable
        },
        "not known on the record, never a refusal"
    );
}

/// The fixture's agent authorisation with a new destination, given by `by`.
fn new_destination_by(by: &Actor) -> Authorisation {
    let mut a: Authorisation = valid!("authorisation-of-one-order-given-by-an-agent");
    a.authorisation = id("auth-21");
    a.destination.account = id("acct-ref-11");
    a.given_by = by.clone();
    a
}

/// The window is Core's clock's: a charge stating a later moment still counts in the window now, and the count starts
/// again only when Core's clock reaches the next window. A charge given again counts once; anything else under its id
/// is refused and counts nothing.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn bound_is_held_over_core_clocks_window_and_a_charge_counts_once() {
    let a = agent("acme-treasury", "agent-7");
    let mut policy = engine(
        Registers::of_the_fixtures().with(&a, Some(every_scope())),
        bounds(3, 1),
    );
    let first = charge("req-1", &a, Work::Request, 2);
    let within = |n| ChargeOutcome::Within { remaining: safe(n) };
    assert_eq!(policy.charge(&first).unwrap().outcome, within(1));
    assert_eq!(policy.charge(&first).unwrap().outcome, within(1), "once");
    let mut other = first.clone();
    other.count = Count::new(1).unwrap();
    assert_eq!(
        policy.charge(&other).unwrap().outcome,
        ChargeOutcome::Refused {
            class: RefusalClass::NotPermitted
        }
    );
    assert_eq!(policy.charged().used(&id("acme-treasury"), Limit::Work), 2);

    // A charge stating a moment a window on counts in the window now.
    let mut later = charge("req-2", &a, Work::Request, 1);
    later.at = at("2026-11-10T11:00:00Z");
    assert_eq!(policy.charge(&later).unwrap().outcome, within(0));
    let mut again = charge("req-3", &a, Work::Request, 1);
    again.at = at("2026-11-10T11:00:00Z");
    assert_eq!(
        policy.charge(&again).unwrap().outcome,
        ChargeOutcome::Over { bound: safe(3) }
    );

    // Core's clock in the same window: still over. In the next: the count starts again.
    policy.set_clock(Stopped::at(&at("2026-11-10T09:59:59Z")));
    assert_eq!(
        policy.charge(&again).unwrap().outcome,
        ChargeOutcome::Over { bound: safe(3) }
    );
    policy.set_clock(Stopped::at(&at("2026-11-10T10:00:00Z")));
    assert_eq!(policy.charge(&again).unwrap().outcome, within(2));
    assert_eq!(policy.charged().used(&id("acme-treasury"), Limit::Work), 1);
}

/// A charge is counted only once the log took its entry: where the log takes none, the call answers nothing and the
/// party's count is as it was.
// verifies: REQ-OPS-1, INV-7
#[test]
fn charge_the_log_does_not_take_counts_nothing() {
    let a = agent("acme-treasury", "agent-7");
    let mut policy = engine(
        Registers::of_the_fixtures().with(&a, Some(every_scope())),
        bounds(3, 1),
    );
    policy.log.refusing = true;
    let first = charge("req-1", &a, Work::Request, 2);
    assert_eq!(policy.charge(&first), Err(AppendFailure::Unavailable));
    assert_eq!(policy.charged().used(&id("acme-treasury"), Limit::Work), 0);
    policy.log.refusing = false;
    assert_eq!(
        policy.charge(&first).unwrap().outcome,
        ChargeOutcome::Within { remaining: safe(1) }
    );
    assert_eq!(policy.log.entries.len(), 1);
}

/// One definition of one accountable actor, for the rows that compare actors there: two agents of one party, or its
/// person and its agent, are one; so is one login stating two parties, since one login can act for more than one.
/// Only two logins of two parties are two.
// verifies: REQ-AUZ-8, REQ-AUZ-4
#[test]
fn actors_are_compared_at_their_accountable_party() {
    let seven = agent("acme-treasury", "agent-7");
    let eight = agent("acme-treasury", "agent-8");
    let own = person("acme-treasury", "treasurer-1");
    let seven_elsewhere = agent("other-co", "agent-7");
    let decider = person("operator", "op-login-2");
    let mover = person("operator-desk-b", "op-login-2");
    let other = agent("other-co", "agent-9");
    assert_eq!(accountable(&seven), &id("acme-treasury"));
    assert!(same_accountable_actor(&seven, &eight));
    assert!(same_accountable_actor(&seven, &own));
    assert!(
        same_accountable_actor(&seven, &seven_elsewhere),
        "one login, two parties stated"
    );
    assert!(
        same_accountable_actor(&decider, &mover),
        "the operator's login deciding for one desk and moving for another"
    );
    assert!(!same_accountable_actor(&seven, &other));
    assert!(!same_accountable_actor(&decider, &other));
}

/// The rule itself, handed none of the inputs it named, takes the agent's scope as missing, never as a yes: a second
/// guard behind the engine, which refuses a missing input before any rule rules. An agent that is its own party is
/// refused with nothing asked, and anyone else passes with nothing named.
// verifies: REQ-AUZ-8, REQ-AUZ-5
#[test]
fn scope_rule_takes_an_input_it_was_not_given_as_missing() {
    let acme = agent("acme-treasury", "agent-7");
    let asked = charge("req-1", &acme, Work::FanOut, 1);
    let Reply::Given(named) = needs(Asked::Charge(&asked)) else {
        panic!("the rule names its inputs");
    };
    assert_eq!(named.len(), 1);
    assert_eq!(rule(Asked::Charge(&asked), &[]), Reply::Missing);
    let no = [Input {
        need: named[0].clone(),
        said: Said::No,
    }];
    assert_eq!(
        rule(Asked::Charge(&asked), &no),
        Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted))
    );

    let itself = charge("req-2", &agent("agent-7", "agent-7"), Work::FanOut, 1);
    assert_eq!(needs(Asked::Charge(&itself)), Reply::Given(Vec::new()));
    assert_eq!(
        rule(Asked::Charge(&itself), &[]),
        Reply::Given(Ruling::Refuse(RefusalClass::NoAccountableParty))
    );
    let own = charge("req-3", &person("cust-1", "cust-1"), Work::Request, 1);
    assert_eq!(needs(Asked::Charge(&own)), Reply::Given(Vec::new()));
    assert_eq!(rule(Asked::Charge(&own), &[]), Reply::Given(Ruling::Permit));
}

/// An actor's kind is the register's, not the request's: an agent's login stating itself a person or a component is
/// held to the kind the register lists for it, and acts for no one, so it escapes neither the scope its party set nor
/// the rule that an agent is never its own party. A login the register lists as a person passes as one, and stating
/// itself an agent it is held to its kind too.
// verifies: REQ-AUZ-8, REQ-AUZ-1
#[test]
fn agent_stating_another_kind_acts_for_no_one() {
    let scoped = agent("acme-treasury", "agent-7");
    let treasurer = person("acme-treasury", "treasurer-1");
    let mut policy = engine(
        Registers::of_the_fixtures()
            .with(&scoped, Some(vec![Scope::Charge(Work::Request)]))
            .with(&treasurer, None),
        Uniform::default(),
    );
    let moves = |by: &Actor| {
        let mut request: DecisionRequest = valid!("decision-request-to-move-value");
        request.caller = by.clone();
        request
    };
    assert_eq!(
        policy.consult(Asked::Decide(&moves(&scoped))),
        refused(RefusalClass::NotPermitted),
        "outside its scope as the agent it is"
    );
    for kind in [ActorKind::Person, ActorKind::Component] {
        let stated = Actor {
            kind,
            ..scoped.clone()
        };
        for (name, asked) in Calls::by(&stated).each() {
            assert_eq!(
                policy.consult(asked),
                refused(RefusalClass::NoAccountableParty),
                "{kind:?} {name}"
            );
        }
        let decision = policy.decide(&moves(&stated)).unwrap();
        assert!(decision.permit().is_none(), "{kind:?}");
    }
    assert_eq!(
        policy.consult(Asked::Decide(&moves(&treasurer))),
        Answer::Permit {}
    );
    let treasurer_as_agent = Actor {
        kind: ActorKind::Agent,
        ..treasurer.clone()
    };
    // Refused: the register says no to the kind, and, its party having set no scope for it, the scope is missing
    // too, which the engine finds first, as it asks every input before any rule rules.
    assert!(
        matches!(
            policy.consult(Asked::Decide(&moves(&treasurer_as_agent))),
            Answer::Refuse { .. }
        ),
        "a person's login stating an agent is held to its kind too"
    );

    // An agent its register lists as its own party, stating itself a person, is still an agent: no one's.
    let itself = agent("agent-7", "agent-7");
    let mut policy = engine(
        Registers::of_the_fixtures().with(&itself, Some(every_scope())),
        Uniform::default(),
    );
    let as_person = Actor {
        kind: ActorKind::Person,
        ..itself.clone()
    };
    for (name, asked) in Calls::by(&as_person).each() {
        assert_eq!(
            policy.consult(asked),
            refused(RefusalClass::NoAccountableParty),
            "{name}"
        );
    }
}

/// Each call is its own scope: an agent its party scoped to exactly one call is permitted that call alone, by the
/// rule, and refused every other. The table names each call's scope by hand, so no call can borrow another's.
// verifies: REQ-AUZ-8
#[test]
fn each_scope_permits_its_own_call_alone() {
    let a = agent("acme-treasury", "agent-7");
    let decide = |fixture: &str| {
        let mut request: DecisionRequest =
            serde_json::from_value(read_fixture(&format!("valid/{fixture}"))).unwrap();
        request.caller = a.clone();
        request
    };
    let moved = decide("decision-request-to-move-value");
    let posted = decide("decision-request-to-post-a-suggested-route");
    let stale = decide("decision-request-to-mark-a-release-stale");
    let promoted = decide("decision-request-to-promote-a-staged-release");
    let erased = decide("decision-request-to-erase-a-kept-statement");
    let Action::ChangeCatalogue { change: marks, .. } = stale.action else {
        panic!("a catalogue change");
    };
    let Action::ChangeCatalogue {
        change: promotes, ..
    } = promoted.action
    else {
        panic!("a catalogue change");
    };
    // A request to resolve a stuck order fails its check and never reaches the consultation; were it to, it is a
    // stuck move.
    let calls = Calls::by(&a);
    let mut resolved = moved.clone();
    resolved.action = Action::ResolveStuck {
        order: calls.move_stuck.order.clone(),
        carries_out: calls.move_stuck.carries_out.clone(),
        resolution: calls.decide_stuck.resolution.clone(),
        // A transaction no other request of this test names.
        transaction: id("txn-t-021-05-scope-resolve-stuck"),
    };
    let charge_of = |work| {
        let mut c = calls.charge.clone();
        c.work = work;
        c
    };
    let (request, fan_out, person_question) = (
        charge_of(Work::Request),
        charge_of(Work::FanOut),
        charge_of(Work::PersonQuestion),
    );
    let table: Vec<(&str, Asked<'_>, Scope)> = vec![
        ("move_value", Asked::Decide(&moved), Scope::MoveValue),
        ("post_route", Asked::Decide(&posted), Scope::PostRoute),
        (
            "mark_stale",
            Asked::Decide(&stale),
            Scope::ChangeCatalogue(marks),
        ),
        (
            "promote",
            Asked::Decide(&promoted),
            Scope::ChangeCatalogue(promotes),
        ),
        ("erase", Asked::Decide(&erased), Scope::EraseStatement),
        (
            "authorise",
            Asked::Authorise(&calls.authorise),
            Scope::Authorise,
        ),
        (
            "decide_stuck",
            Asked::DecideStuck(&calls.decide_stuck),
            Scope::DecideStuck,
        ),
        (
            "move_stuck",
            Asked::MoveStuck(&calls.move_stuck),
            Scope::MoveStuck,
        ),
        ("resolve_stuck", Asked::Decide(&resolved), Scope::MoveStuck),
        (
            "report_outcome",
            Asked::ReportOutcome(&calls.report),
            Scope::ReportOutcome,
        ),
        (
            "confirm_outcome",
            Asked::ConfirmOutcome(&calls.confirm),
            Scope::ConfirmOutcome,
        ),
        (
            "open_session",
            Asked::OpenSession(&calls.open_session),
            Scope::OpenSession,
        ),
        (
            "check_read",
            Asked::CheckRead(&calls.check_read),
            Scope::CheckRead,
        ),
        (
            "grant",
            Asked::GrantPermission(&calls.grant),
            Scope::GrantPermission,
        ),
        (
            "revoke",
            Asked::RevokePermission(&calls.grant),
            Scope::RevokePermission,
        ),
        (
            "request",
            Asked::Charge(&request),
            Scope::Charge(Work::Request),
        ),
        (
            "fan_out",
            Asked::Charge(&fan_out),
            Scope::Charge(Work::FanOut),
        ),
        (
            "person_question",
            Asked::Charge(&person_question),
            Scope::Charge(Work::PersonQuestion),
        ),
        (
            "ask",
            Asked::AskAboutPerson(&calls.ask),
            Scope::AskAboutPerson,
        ),
    ];
    assert_ne!(marks, promotes);
    let mut scopes: Vec<Scope> = Vec::new();
    for (_, _, scope) in &table {
        if !scopes.contains(scope) {
            scopes.push(*scope);
        }
    }
    for only in &scopes {
        let mut sources = Registers::of_the_fixtures().with(&a, Some(vec![*only]));
        for (name, asked, scope) in &table {
            let Reply::Given(named) = needs(*asked) else {
                panic!("the rule names its inputs");
            };
            let inputs: Vec<Input> = named
                .into_iter()
                .map(|need| {
                    let Reply::Given(said) = sources.supply(&need) else {
                        panic!("the register answers {need:?}");
                    };
                    Input { need, said }
                })
                .collect();
            let expected = if scope == only {
                Reply::Given(Ruling::Permit)
            } else {
                Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted))
            };
            assert_eq!(rule(*asked, &inputs), expected, "{name} under {only:?}");
        }
    }
}

/// Core's clock moving back doesn't start the bound again: a charge at a moment before the latest window counts in
/// the latest window, and the count goes on when the clock comes forward again within it.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn clock_moving_back_keeps_counting_in_the_latest_window() {
    let a = agent("acme-treasury", "agent-7");
    let mut policy = engine(
        Registers::of_the_fixtures().with(&a, Some(every_scope())),
        bounds(3, 1),
    );
    let within = |n| ChargeOutcome::Within { remaining: safe(n) };
    let over = ChargeOutcome::Over { bound: safe(3) };
    let mut charge_at = |moment: &str, request: &str, count| {
        policy.set_clock(Stopped::at(&at(moment)));
        policy
            .charge(&charge(request, &a, Work::Request, count))
            .unwrap()
            .outcome
    };
    assert_eq!(charge_at("2026-11-10T10:00:00Z", "req-1", 2), within(1));
    assert_eq!(charge_at("2026-11-10T09:59:59Z", "req-2", 1), within(0));
    assert_eq!(charge_at("2026-11-10T09:59:59Z", "req-3", 1), over);
    assert_eq!(charge_at("2026-11-10T10:00:01Z", "req-4", 1), over);
    assert_eq!(policy.charged().used(&id("acme-treasury"), Limit::Work), 3);
    // A charge counted while the clock stood back is counted at the latest window's start, so a rebuild finds it in
    // the window it counted in.
    let ten = std::time::Duration::from_secs(1_794_304_800);
    let back = ten - std::time::Duration::from_secs(1);
    assert_eq!(policy.charged().counted_at(back), ten);
    let later = ten + std::time::Duration::from_secs(1);
    assert_eq!(policy.charged().counted_at(later), later);
}

/// A charge's id is its party's own: two parties' charges under one id are two charges, each counted at its party.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn charge_ids_are_each_partys_own() {
    let acme = agent("acme-treasury", "agent-7");
    let other = agent("other-co", "agent-1");
    let mut policy = engine(
        Registers::of_the_fixtures()
            .with(&acme, Some(every_scope()))
            .with(&other, Some(every_scope())),
        bounds(3, 1),
    );
    for who in [&acme, &other] {
        assert_eq!(
            policy
                .charge(&charge("req-1", who, Work::Request, 2))
                .unwrap()
                .outcome,
            ChargeOutcome::Within { remaining: safe(1) },
            "{who:?}"
        );
    }
    assert_eq!(policy.charged().used(&id("acme-treasury"), Limit::Work), 2);
    assert_eq!(policy.charged().used(&id("other-co"), Limit::Work), 2);
}

/// The counter is a function of what it counted: rebuilt from the charges counted before, each once by its party's
/// id and those of an earlier window left out, it stands as the counter that counted them did, and holds the next
/// charge to the same bound.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn counter_is_rebuilt_from_the_charges_counted() {
    let acme = agent("acme-treasury", "agent-7");
    let other = agent("other-co", "agent-1");
    // 2026-11-10T10:00:00Z, and the window before it.
    let now = std::time::Duration::from_secs(1_794_304_800);
    let earlier = now - std::time::Duration::from_secs(WINDOW_SECS);
    let counted = |charge: Charge, at| Counted { charge, at };
    let mut rebuilt = Charged::default();
    rebuilt.rebuild(
        now,
        vec![
            counted(charge("req-1", &acme, Work::Request, 2), now),
            // The same charge recorded again counts once.
            counted(charge("req-1", &acme, Work::Request, 2), now),
            counted(charge("req-1", &other, Work::FanOut, 4), now),
            counted(charge("req-2", &acme, Work::PersonQuestion, 1), now),
            // A window gone counts nothing now.
            counted(charge("req-0", &acme, Work::Request, 5), earlier),
        ],
    );
    assert_eq!(rebuilt.used(&id("acme-treasury"), Limit::Work), 2);
    assert_eq!(rebuilt.used(&id("acme-treasury"), Limit::People), 1);
    assert_eq!(rebuilt.used(&id("other-co"), Limit::Work), 4);
    let again = charge("req-1", &acme, Work::Request, 2);
    assert!(!rebuilt.reissues(&again));
    assert_eq!(
        rebuilt.standing(&again),
        Standing::Fits {
            remaining: safe(WORK_BOUND - 2)
        },
        "counted already"
    );
    let mut changed = again.clone();
    changed.count = Count::new(3).unwrap();
    assert!(rebuilt.reissues(&changed));
    // The window before is gone, so its charge again is a new one.
    let gone = charge("req-0", &acme, Work::Request, 5);
    assert!(!rebuilt.reissues(&gone));

    // Rebuilt, a counter holds nothing of what it held before; and a charge counted in a window later than the one
    // Core's clock now stands in, as after the clock moved back, puts the counter in that latest window.
    let mut held = Charged::default();
    held.rebuild(now, Vec::new());
    held.take(&charge("req-9", &acme, Work::Request, 7));
    held.rebuild(
        earlier,
        vec![
            counted(charge("req-1", &acme, Work::Request, 2), now),
            counted(charge("req-0", &acme, Work::Request, 5), earlier),
        ],
    );
    assert_eq!(held.used(&id("acme-treasury"), Limit::Work), 2);
}

/// The counter is the log's: an engine opened again over the same log holds every party to what the log holds as
/// counted, the same charge again counting once, and one whose read of the log fails refuses every charge and counts
/// nothing.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn counter_is_the_logs_across_a_restart() {
    let a = agent("acme-treasury", "agent-7");
    let registers = || Registers::of_the_fixtures().with(&a, Some(every_scope()));
    let within = |n| ChargeOutcome::Within { remaining: safe(n) };
    let mut first = engine(registers(), bounds(3, 1));
    let counted = charge("req-1", &a, Work::Request, 2);
    assert_eq!(first.charge(&counted).unwrap().outcome, within(1));

    // Opened again over the same log.
    let mut again = engine(registers(), bounds(3, 1));
    again.log.entries = first.log.entries.clone();
    assert_eq!(
        again
            .charge(&charge("req-2", &a, Work::Request, 2))
            .unwrap()
            .outcome,
        ChargeOutcome::Over { bound: safe(3) }
    );
    assert_eq!(again.charge(&counted).unwrap().outcome, within(1), "once");
    assert_eq!(
        again
            .charge(&charge("req-3", &a, Work::Request, 1))
            .unwrap()
            .outcome,
        within(0)
    );

    // A read that fails: no charge is within.
    let mut blind = engine(registers(), bounds(3, 1));
    blind.log.unreadable = true;
    assert_eq!(
        blind
            .charge(&charge("req-4", &a, Work::Request, 1))
            .unwrap()
            .outcome,
        ChargeOutcome::Refused {
            class: RefusalClass::MissingInput
        }
    );
    assert_eq!(blind.charged().used(&id("acme-treasury"), Limit::Work), 0);
}

/// A read of the log is held to the one before it: one that drops what the last read held, or lacks the engine's own
/// last charge within its bound, is an input the decision doesn't have, and every charge on it is refused, so a stale,
/// truncated or other log's read never starts a party's bound again. Once the read is whole again, the bound holds.
// verifies: REQ-OPS-1, REQ-AUZ-8, REQ-AUZ-5
#[test]
fn stale_read_of_the_log_starts_no_bound_again() {
    let a = agent("acme-treasury", "agent-7");
    let registers = || Registers::of_the_fixtures().with(&a, Some(every_scope()));
    let refused = ChargeOutcome::Refused {
        class: RefusalClass::MissingInput,
    };
    let mut policy = engine(registers(), bounds(3, 1));
    let charge_of = |policy: &mut Tested, request: &str| {
        policy
            .charge(&charge(request, &a, Work::Request, 3))
            .unwrap()
            .outcome
    };
    assert_eq!(
        charge_of(&mut policy, "req-1"),
        ChargeOutcome::Within { remaining: safe(0) }
    );

    // The reviewer's sequence: the read answers nothing.
    policy.log.read_instead = Some(Vec::new());
    assert_eq!(charge_of(&mut policy, "req-2"), refused);
    assert_eq!(charge_of(&mut policy, "req-3"), refused);

    // A truncated read: what the log took before the engine's own charge is dropped.
    let mut other = engine(registers(), bounds(3, 1));
    assert!(matches!(
        charge_of(&mut other, "req-9"),
        ChargeOutcome::Within { .. }
    ));
    let mut whole = other.log.entries.clone();
    whole.extend(policy.log.entries.clone());
    policy.log.read_instead = Some(whole.clone());
    assert_eq!(
        charge_of(&mut policy, "req-4"),
        ChargeOutcome::Over { bound: safe(3) }
    );
    policy.log.read_instead = Some(whole[1..].to_vec());
    assert_eq!(charge_of(&mut policy, "req-5"), refused, "truncated");

    // Another log's read, which lacks the engine's own charge.
    policy.log.read_instead = Some(other.log.entries.clone());
    assert_eq!(charge_of(&mut policy, "req-6"), refused, "another log");

    // Whole again: the party is still at its bound.
    policy.log.read_instead = Some(whole);
    assert_eq!(
        charge_of(&mut policy, "req-7"),
        ChargeOutcome::Over { bound: safe(3) }
    );
}

/// A rebuild never counts a window below what this engine itself counted in it, whatever the read held.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn rebuild_counts_no_window_below_the_engines_own() {
    let a = agent("acme-treasury", "agent-7");
    let now = std::time::Duration::from_secs(1_794_304_800);
    let mut counter = Charged::default();
    counter.rebuild(now, Vec::new());
    counter.counted(Counted {
        charge: charge("req-1", &a, Work::Request, 2),
        at: now,
    });
    counter.rebuild(now, Vec::new());
    assert_eq!(counter.used(&id("acme-treasury"), Limit::Work), 2);
    // Its own charge of a window gone counts nothing now.
    let next = now + std::time::Duration::from_secs(WINDOW_SECS);
    counter.rebuild(next, Vec::new());
    assert_eq!(counter.used(&id("acme-treasury"), Limit::Work), 0);
}

/// Core's clock jumping far ahead, once, freezes nothing: a charge counted more than a window ahead of Core's now
/// doesn't choose the window, which follows Core's now, but it still counts in it, so the glitch only takes what it
/// charged off the bound, failing closed, and is set apart for the operator. The window moves on with Core's clock, for
/// this engine and for a fresh one over the same log.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn clock_jumping_far_ahead_freezes_no_window() {
    let a = agent("acme-treasury", "agent-7");
    let registers = || Registers::of_the_fixtures().with(&a, Some(every_scope()));
    let within = |n| ChargeOutcome::Within { remaining: safe(n) };
    let over = ChargeOutcome::Over { bound: safe(3) };
    let mut policy = engine(registers(), bounds(3, 1));
    let charge_at = |policy: &mut Tested, moment: &str, request: &str, count| {
        policy.set_clock(Stopped::at(&at(moment)));
        policy
            .charge(&charge(request, &a, Work::Request, count))
            .unwrap()
            .outcome
    };
    assert_eq!(
        charge_at(&mut policy, "2030-01-01T00:00:00Z", "req-1", 2),
        within(1)
    );
    // Back to now: the charge of 2030 is ahead and set apart, and still counts.
    assert_eq!(charge_at(&mut policy, NOW, "req-2", 1), within(0));
    assert_eq!(policy.charged().ahead().len(), 1);
    assert_eq!(policy.charged().ahead()[0].charge.request, id("req-1"));
    assert_eq!(charge_at(&mut policy, NOW, "req-3", 1), over);
    // The next window by Core's clock: the window moved on, now's charge is gone, the glitch's still counts.
    assert_eq!(
        charge_at(&mut policy, "2026-11-10T10:00:00Z", "req-4", 1),
        within(0)
    );
    // A fresh engine over the same log follows Core's clock the same.
    let mut fresh = engine(registers(), bounds(3, 1));
    fresh.log.entries = policy.log.entries.clone();
    assert_eq!(
        charge_at(&mut fresh, "2026-11-10T11:00:00Z", "req-5", 1),
        within(0)
    );
    assert_eq!(fresh.charged().ahead().len(), 1);
}

/// Core's clock stepping back more than a window gives no fresh bound: the charges of the later window are ahead of
/// Core's now and still count, so the party spent its bound and the next charge is over.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn clock_stepping_back_more_than_a_window_gives_no_fresh_bound() {
    let a = agent("acme-treasury", "agent-7");
    let mut policy = engine(
        Registers::of_the_fixtures().with(&a, Some(every_scope())),
        bounds(3, 1),
    );
    policy.set_clock(Stopped::at(&at("2026-11-10T10:00:00Z")));
    assert!(matches!(
        policy
            .charge(&charge("req-1", &a, Work::Request, 3))
            .unwrap()
            .outcome,
        ChargeOutcome::Within { .. }
    ));
    // One hour and one second back: two windows behind.
    policy.set_clock(Stopped::at(&at("2026-11-10T08:59:59Z")));
    assert_eq!(
        policy
            .charge(&charge("req-2", &a, Work::Request, 1))
            .unwrap()
            .outcome,
        ChargeOutcome::Over { bound: safe(3) }
    );
}

/// Two engines over one log whose clocks are more than a window apart hold a party to one bound: what one counted is
/// ahead of the other's now, and still counts there.
// verifies: REQ-OPS-1, REQ-AUZ-8, AC-18
#[test]
fn engines_with_skewed_clocks_hold_a_party_to_one_bound() {
    let a = agent("acme-treasury", "agent-7");
    let registers = || Registers::of_the_fixtures().with(&a, Some(every_scope()));
    let mut ahead = engine(registers(), bounds(3, 1));
    ahead.set_clock(Stopped::at(&at("2026-11-10T12:00:00Z")));
    assert!(matches!(
        ahead
            .charge(&charge("req-1", &a, Work::Request, 3))
            .unwrap()
            .outcome,
        ChargeOutcome::Within { .. }
    ));
    let mut behind = engine(registers(), bounds(3, 1));
    behind.log.entries = ahead.log.entries.clone();
    behind.set_clock(Stopped::at(&at(NOW)));
    assert_eq!(
        behind
            .charge(&charge("req-2", &a, Work::Request, 1))
            .unwrap()
            .outcome,
        ChargeOutcome::Over { bound: safe(3) }
    );
}

/// A charge Core's clock gives no time for that an entry can hold is refused, nothing recorded and nothing counted,
/// never recorded at the time the charge states.
// verifies: REQ-OPS-1, INV-7
#[test]
fn charge_at_a_time_no_entry_can_hold_is_refused() {
    let a = agent("acme-treasury", "agent-7");
    let mut policy = engine(
        Registers::of_the_fixtures().with(&a, Some(every_scope())),
        bounds(3, 1),
    );
    // Past the year 9999, which no RFC 3339 time holds.
    policy.set_clock(Box::new(Stopped(std::time::Duration::from_secs(
        400_000 * 365 * 86_400,
    ))));
    assert!(matches!(
        policy.charge(&charge("req-1", &a, Work::Request, 1)),
        Err(AppendFailure::Refused(_))
    ));
    assert!(policy.log.entries.is_empty());
    assert_eq!(policy.charged().used(&id("acme-treasury"), Limit::Work), 0);
}

/// A charge counted within its bound says when the engine counted it, by Core's clock (`counted_at`), beside its
/// entry's time, which is the charge's own as every call's is; one not counted says nothing. The counter is rebuilt by
/// `counted_at`, so a charge stating any moment counts in the window Core's clock counted it in.
// verifies: REQ-OPS-1, REQ-AUZ-8
#[test]
fn a_counted_charge_says_when_core_counted_it() {
    let a = agent("acme-treasury", "agent-7");
    let mut policy = engine(
        Registers::of_the_fixtures().with(&a, Some(every_scope())),
        bounds(3, 1),
    );
    policy.set_clock(Stopped::at(&at("2026-11-10T10:00:05Z")));
    let mut stated = charge("req-1", &a, Work::Request, 2);
    stated.at = at("2026-11-09T00:00:00Z");
    assert!(matches!(
        policy.charge(&stated).unwrap().outcome,
        ChargeOutcome::Within { .. }
    ));
    let Some(Entry::Verified(entry)) = policy.log.entries.last() else {
        panic!("a verified entry");
    };
    assert_eq!(
        entry.when.at, stated.at,
        "the entry's time is the charge's own"
    );
    assert_eq!(
        entry.what,
        What::Charge {
            charge: stated.clone(),
            counted_at: Some(at("2026-11-10T10:00:05Z")),
        }
    );
    // Over the bound: counted nowhere, so it says nothing.
    assert!(matches!(
        policy
            .charge(&charge("req-2", &a, Work::Request, 2))
            .unwrap()
            .outcome,
        ChargeOutcome::Over { .. }
    ));
    let Some(Entry::Verified(entry)) = policy.log.entries.last() else {
        panic!("a verified entry");
    };
    assert!(matches!(
        entry.what,
        What::Charge {
            counted_at: None,
            ..
        }
    ));
    // Rebuilt by when it was counted, not by when it says it was asked.
    assert_eq!(
        charged_in(&policy.log.entries)[0].at,
        std::time::Duration::from_secs(1_794_304_805)
    );
}
