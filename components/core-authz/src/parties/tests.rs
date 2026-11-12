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
    /// Each actor listed, by its `acts`, with the party it acts for. An actor not listed is a missing input.
    acts_for: Vec<(Id, Id)>,
    /// Each agent a party listed, with the calls in its scope. An agent not listed is a missing input.
    scopes: Vec<(Id, Id, Vec<Scope>)>,
    /// The scopes asked, in order.
    asked: Vec<Need>,
}

impl Registers {
    /// Registers listing every actor of the fixtures as acting for its own party.
    fn of_the_fixtures() -> Self {
        let mut registers = Registers::default();
        for (acts, party) in [
            ("core-orch", "rutent"),
            ("op-login-2", "operator"),
            ("op-login-5", "operator-desk-b"),
            ("examiner-3", "fin-regulator"),
        ] {
            registers.acts_for.push((id(acts), id(party)));
        }
        registers
    }

    /// `actor` listed as acting for its party, and, where `scope` is some, in that scope of it.
    fn with(mut self, actor: &Actor, scope: Option<Vec<Scope>>) -> Self {
        self.acts_for
            .push((actor.acts.clone(), actor.party.clone()));
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
            Source::ActsFor { acts, party } => {
                match self.acts_for.iter().find(|(a, _)| a == acts) {
                    Some((_, p)) => yes(p == party),
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
        };
        calls.decide.caller = caller.clone();
        calls.authorise.given_by = caller.clone();
        calls.decide_stuck.decider = caller.clone();
        calls.move_stuck.mover = caller.clone();
        calls.open_session.requester = caller.clone();
        calls.check_read.reader = caller.clone();
        calls.grant.by = caller.clone();
        calls.charge.caller = caller.clone();
        calls.charge.party = caller.party.clone();
        calls.ask.asker = caller.clone();
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
    assert_eq!(answer.outcome, ChargeOutcome::Over { bound: safe(0) });
    assert_eq!(
        policy.log.last(),
        Outcome::Refused {
            class: RefusalClass::OverBound
        }
    );
    assert_eq!(policy.charged().used(&id("agent-7"), Limit::Work), 0);

    // Listed by the register for another party, or not at all.
    let elsewhere = agent("acme-treasury", "agent-8");
    let unknown = agent("acme-treasury", "agent-9");
    let mut registers = Registers::of_the_fixtures();
    registers.acts_for.push((id("agent-8"), id("other-co")));
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
        assert_eq!(answer.outcome, ChargeOutcome::Over { bound: safe(0) });
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
    assert_eq!(answer.outcome, ChargeOutcome::Over { bound: safe(0) });
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

    let calls = Calls::by(&wide);
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
    assert_eq!(answer.outcome, ChargeOutcome::Over { bound: safe(0) });
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
        ChargeOutcome::Over { bound: safe(0) }
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

/// One definition of an actor's accountable party, for the rows that compare actors there: two agents of one party,
/// or its person and its agent, are one; agents of two parties are two.
// verifies: REQ-AUZ-8
#[test]
fn actors_are_compared_at_their_accountable_party() {
    let seven = agent("acme-treasury", "agent-7");
    let eight = agent("acme-treasury", "agent-8");
    let own = person("acme-treasury", "treasurer-1");
    let other = agent("other-co", "agent-7");
    assert_eq!(accountable(&seven), &id("acme-treasury"));
    assert!(same_party(&seven, &eight));
    assert!(same_party(&seven, &own));
    assert!(
        !same_party(&seven, &other),
        "the same agent id, another party"
    );
}
