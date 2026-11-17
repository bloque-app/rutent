//! A reserve's demand that a person be present, through the assembled Core with an agent acting (`T-024-11`,
//! `REQ-CPL-9`): the customer's agent, listed and scoped in the deployment's registers and acting for an accountable
//! party, requests, quotes and accepts an order, and Core runs it to the reserve's demand. The reserve decides what an
//! agent may do there; Core only reports the outcome, as the reserve's own attributed statement, and takes no step of
//! its own after it. The same scenario with a person acting gives the same outcome (`REQ-OPS-5`: one contract, two
//! kinds of caller). `person_records.rs` has the same demand over the segments kit, where the caller is Core and the
//! customer a person.

use rutent_contracts::core_internal::calls::is_decision;
use rutent_contracts::core_internal::{
    Actor, ActorKind, Attribution, Entry, Role, VerifiedEntry, What, Work,
};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::{Observed, OrderState};
use rutent_core_authz::parties::Scope;
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::segments::{Ran, SegmentOutcome};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Fault, Given, Scenario, base_request, plant};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};
use serde_json::Value;

/// The customer's agent, acting for the customer's party: a login of its own that the register lists as an agent.
fn the_customers_agent() -> Actor {
    Actor {
        party: kit::customer().party,
        acts: id("agent-1"),
        role: Role::Customer,
        kind: ActorKind::Agent,
    }
}

/// What a run to the reserve's demand left, as read once it stood still: what matters, and nothing of who asked.
#[derive(Debug, Clone, PartialEq)]
struct Left {
    /// The reserve's statement as Core reported it: its maker, the statement's own body and who the log attributes it
    /// to.
    reported: (String, Value, String),
    /// The order's state, as the customer reads it.
    state: OrderState,
    /// What the reserve moved: nothing, since it refused.
    books: usize,
    /// The inputs Core took, in order, as their kinds say.
    inputs: Vec<String>,
    /// How many decisions Core's log holds.
    decisions: usize,
}

/// A run: Core as it stands, the order and what running it said.
struct Run {
    core: Core,
    order: Id,
    ran: Vec<Ran>,
}

/// The order of `acting`, who is the customer's person or its agent, requested, quoted and accepted by it, with the
/// reserve scripted to demand a person's presence on its withdrawal, then run by Core.
fn demand(acting: &Actor) -> Run {
    let (mut core, _) = Scenario::new(
        "a reserve demands presence",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
    .core(1, &mut Reference);
    if acting.kind == ActorKind::Agent {
        // Its party lists it and sets its scope: the calls the order takes, and no call to ask about a person.
        let mut registers = core.registers();
        registers.list(acting);
        for scope in [
            Scope::OpenSession,
            Scope::Authorise,
            Scope::Charge(Work::Request),
            Scope::Charge(Work::FanOut),
        ] {
            registers.scope(acting, scope);
        }
    }
    let session = core.open_session(acting).expect("the actor's session");
    let mut request = base_request("req-presence", AMOUNT, &session);
    request.caller = acting.clone();
    let told = core.request(request).expect("the request opens an order");
    let order = Core::order_of(&told).expect("an order");
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = core.quote(acting, &order).expect("the order is quoted")
    else {
        panic!("a quote is shown");
    };
    core.accept(acting, &order, &digest).expect("accepted");
    plant(
        &mut core,
        &Fault::WaitsOnPerson {
            provider: RESERVE.to_owned(),
            kind: "withdrawal".to_owned(),
        },
    );
    let ran = core.run(&order).expect("the order runs to the demand");
    Run { core, order, ran }
}

/// The verified entries of the log, in order, each with its index among all of the log's entries.
fn verified(core: &Core) -> Vec<(usize, VerifiedEntry)> {
    core.log()
        .entries()
        .iter()
        .enumerate()
        .filter_map(|(at, (_, e))| match e {
            Entry::Verified(v) => Some((at, v.clone())),
            Entry::Stated(_) => None,
        })
        .collect()
}

/// The kind of an input, as its observation says.
fn kind_of(observed: &Observed) -> String {
    serde_json::to_value(observed).expect("an observation serialises")["kind"]
        .as_str()
        .map_or_else(|| format!("{observed:?}"), str::to_owned)
}

/// The inputs Core took, after the order's route was chosen, as kinds.
fn inputs_after_the_route(core: &Core) -> Vec<String> {
    verified(core)
        .into_iter()
        .filter_map(|(_, v)| match v.what {
            What::Input { input } => Some(input.observed),
            _ => None,
        })
        .filter(|o| !matches!(o, Observed::RouteChosen { .. }))
        .map(|o| kind_of(&o))
        .collect()
}

/// The demand the reserve's statement states, as Core reported it: the one segment of the one part that ran.
fn reported(ran: &[Ran]) -> &rutent_core_orch::segments::ProvidersStatement {
    let [Ran::Segments(segments)] = ran else {
        panic!("one part ran its segments: {ran:?}");
    };
    let [one] = segments.as_slice() else {
        panic!("the first segment alone ran, and then it waited: {segments:?}");
    };
    let SegmentOutcome::WaitingOnThePerson(statement) = &one.outcome else {
        panic!(
            "the reserve's demand, reported as its own: {:?}",
            one.outcome
        );
    };
    statement
}

/// Reads what the run left.
fn read(run: &mut Run) -> Left {
    let statement = reported(&run.ran).clone();
    let admitted = run.core.log();
    let index = usize::try_from(u64::from(statement.statement.index)).expect("an index");
    let Entry::Stated(stated) = &admitted.entries()[index].1 else {
        panic!("the reported statement is a stated entry");
    };
    let attributed = match &stated.attribution {
        Attribution::Verified { who } => who.acts.as_str().to_owned(),
        other => format!("{other:?}"),
    };
    drop(admitted);
    // What the reserve stated, from the bytes it signed: its statement's own content, apart from the signature.
    let said: Value = serde_json::from_slice(&statement.bytes).expect("the reserve's statement");
    let body = said["statement"].clone();
    let books = run.core.world().journal(RESERVE).len();
    let decisions = verified(&run.core)
        .iter()
        .filter(|(_, v)| is_decision(&v.what))
        .count();
    let state = run
        .core
        .status(&kit::customer(), &run.order)
        .expect("the customer reads its order")
        .state;
    Left {
        reported: (statement.maker.as_str().to_owned(), body, attributed),
        state,
        books,
        inputs: inputs_after_the_route(&run.core),
        decisions,
    }
}

/// An agent of the customer's party, listed and scoped, requests, quotes and accepts an order, and the reserve
/// demands that a person be present: Core reports the demand as the reserve's own statement, attributed to it,
/// decides nothing in its place, and leaves the order where the demand found it. What an agent may do there is the
/// reserve's to say (`REQ-CPL-9`).
// verifies: REQ-CPL-9, REQ-OPS-5
#[test]
fn presence_demanded_of_an_agent_through_the_assembly_is_reported_as_the_reserves_outcome() {
    let agent = the_customers_agent();
    let mut run = demand(&agent);
    let left = read(&mut run);

    // The reserve's: its own statement, made by it and attributed to it by the log.
    let (maker, body, attributed) = &left.reported;
    assert_eq!(maker, RESERVE, "the reserve's statement");
    assert_eq!(attributed, RESERVE, "the log attributes it to the reserve");
    // And what it states is the refusal the reserve scripted, of the class that waits on the person, over the
    // withdrawal the agent's order made it carry out.
    assert_eq!(
        body["outcome"]["status"], "refused",
        "the reserve refused: {body}"
    );
    assert_eq!(
        body["outcome"]["refusal"]["class"], "waits_on_person",
        "it waits on the person: {body}"
    );
    // Core's report is nothing but that: the segment is waiting, none failed, settled or moved on, and the order
    // stands where the demand found it.
    assert_eq!(left.books, 0, "the reserve moved nothing");
    assert!(
        !matches!(left.state, OrderState::Complete | OrderState::Failed),
        "the order neither closed nor failed on it: {:?}",
        left.state
    );
    assert_eq!(
        left.inputs,
        vec!["provider_bound".to_owned()],
        "Core decided nothing in the reserve's place: the segment's start is its only input after the route"
    );
}

/// The same scenario with the customer's person acting in place of its agent gives the same outcome: the reserve's
/// statement as reported, the order's state, what moved, every input Core took and how many decisions it made
/// (`REQ-CPL-9`, `REQ-OPS-5`). Who acts is in the log's records of who asked, and in nothing Core did after.
// verifies: REQ-CPL-9, REQ-OPS-5
#[test]
fn presence_demanded_of_a_person_gives_the_same_outcome_as_of_an_agent() {
    let by_agent = read(&mut demand(&the_customers_agent()));
    let by_person = read(&mut demand(&kit::customer()));
    // Each a real run to a real demand, not two that stopped short alike.
    assert_eq!(by_person.reported.0, RESERVE);
    assert_eq!(by_person.inputs, vec!["provider_bound".to_owned()]);
    assert_eq!(by_agent, by_person);
}

/// After the reserve's demand Core takes no step of its own: no input follows the segment's start, no decision follows
/// the move's, and nothing else is recorded after the reserve's statement but what the platform's own bookkeeping of
/// the call holds. Run with the agent acting, whose scope holds no call that would let Core ask on, and with the
/// person acting.
// verifies: REQ-CPL-9
#[test]
fn core_takes_no_step_after_a_reserves_demand_of_presence() {
    for acting in [the_customers_agent(), kit::customer()] {
        let run = demand(&acting);
        let all = verified(&run.core);
        let last_of = |wanted: &dyn Fn(&VerifiedEntry) -> bool| {
            all.iter().rev().find(|(_, v)| wanted(v)).map(|(at, _)| *at)
        };
        let start = last_of(&|v| {
            matches!(&v.what, What::Input { input }
                if matches!(input.observed, Observed::ProviderBound {}))
        })
        .expect("the segment's start at the reserve is an input");
        let last_input = last_of(&|v| matches!(&v.what, What::Input { .. })).unwrap();
        assert_eq!(
            last_input, start,
            "{:?}: the segment's start is Core's last input",
            acting.kind
        );
        let moved =
            last_of(&|v| matches!(&v.what, What::Effect { .. })).expect("the move's effect");
        let decision = last_of(&|v| is_decision(&v.what)).expect("the move's decision");
        assert!(
            decision < moved,
            "{:?}: the move's decision comes before its effect",
            acting.kind
        );
        assert!(
            all.iter()
                .filter(|(_, v)| is_decision(&v.what))
                .all(|(at, _)| *at <= decision),
            "{:?}: no decision follows the move's",
            acting.kind
        );
        // Nothing of Core's after the reserve's statement: the entries that follow it are the reserve's own words or
        // the move's result, none an input and none a decision.
        let statement =
            usize::try_from(u64::from(reported(&run.ran).statement.index)).expect("an index");
        assert!(
            matches!(&run.core.log().entries()[statement].1, Entry::Stated(s)
                if matches!(&s.attribution, Attribution::Verified { who } if who.acts.as_str() == RESERVE)),
            "the reserve's statement is the one Core reported"
        );
        assert!(
            all.iter()
                .filter(|(at, _)| *at > statement)
                .all(|(_, v)| !matches!(&v.what, What::Input { .. }) && !is_decision(&v.what)),
            "{:?}: no input and no decision after the reserve's statement",
            acting.kind
        );
    }
}
