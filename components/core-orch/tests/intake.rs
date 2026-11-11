//! Order intake and acceptance (`T-020-03`): one order per request identifier, refused apart from existing, a stable
//! class of cause with no provider text, the quote shown and accepted before the order starts, an agent's acceptance
//! as an agent's, three parties apart, the destination resolved at acceptance and its owner's change waited on, the
//! requester's address inside the authorisation, and one contract for humans and agents. Every log a test writes is
//! replayed as the log itself would admit it.

mod intake_support;
mod rounds_support;
mod snapshot_support;

use intake_support::*;
use proptest::prelude::*;
use rounds_support::{Sim, customer};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Answer, Entry, NotKnownCause, OrderRefusalClass, RefusalClass, Role, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::OuterNode;
use rutent_contracts::router_api::Id;
use rutent_core_orch::intake::{
    DESTINATION_INPUT, IntakeError, OrderRequest, Resolution, Status, Told, Wait,
};
use rutent_core_orch::rounds::Providers;
use serde_json::Value;
use snapshot_support::{RESERVE, id, sign, statements_key};

/// The log replays as the log itself would admit it.
fn assert_admitted(env: &Env) {
    if let Err((place, why)) = env.admitted() {
        panic!("the log refuses entry {place}: {why}");
    }
}

fn status_of(told: &Told) -> &Status {
    match told {
        Told::Exists { status, .. } => status,
        Told::Refused { class } => panic!("refused: {class:?}"),
    }
}

/// The current quote's digest of `order`, as its caller was told.
fn digest_of(env: &Env, request: &Id) -> Option<Digest> {
    match env.intake.told(request)? {
        Told::Exists {
            status: Status::Quoted { digest, .. },
            ..
        } => Some(digest),
        _ => None,
    }
}

// One order per request identifier ---------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Step {
    /// Opens request `n`, as first asked or with another amount.
    Open { n: usize, other: bool },
    /// Quotes request `n`'s order.
    Quote { n: usize },
    /// Accepts request `n`'s quote, by the customer or its agent.
    Accept { n: usize, by_agent: bool },
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0..2usize, any::<bool>()).prop_map(|(n, other)| Step::Open { n, other }),
        (0..2usize).prop_map(|n| Step::Quote { n }),
        (0..2usize, any::<bool>()).prop_map(|(n, by_agent)| Step::Accept { n, by_agent }),
    ]
}

fn request_n(n: usize, other: bool) -> OrderRequest {
    let mut request = order_request(&format!("client-req-{n}"), customer());
    if other {
        request.route = route("999");
    }
    request
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

    // verifies: REQ-ORD-19, INV-8
    #[test]
    fn repeated_request_under_one_identifier_creates_one_order(steps in prop::collection::vec(step(), 1..12)) {
        let mut env = Env::new();
        let mut sim = env.sim();
        let mut first: [Option<(Id, bool)>; 2] = [None, None];
        for step in steps {
            match step {
                Step::Open { n, other } => {
                    let request = request_n(n, other);
                    let before = env.len();
                    let answer = env.open(&mut sim, &request);
                    match (&first[n], answer) {
                        (None, Ok(told)) => first[n] = Some((order_of(&told), other)),
                        (Some((order, body)), Ok(told)) => {
                            // The same request again: the same order, and nothing recorded.
                            prop_assert_eq!(*body, other);
                            prop_assert_eq!(&order_of(&told), order);
                            prop_assert_eq!(env.len(), before);
                        }
                        (Some((order, body)), Err(IntakeError::Conflicting { order: named })) => {
                            // Another request under a used identifier opens nothing.
                            prop_assert_ne!(*body, other);
                            prop_assert_eq!(named.as_ref(), Some(order));
                            prop_assert_eq!(env.len(), before);
                        }
                        (seen, answer) => prop_assert!(false, "{:?}: {:?}", seen, answer),
                    }
                }
                Step::Quote { n } => {
                    if let Some((order, _)) = first[n].clone() {
                        let _ = env.quote(&mut sim, &order);
                    }
                }
                Step::Accept { n, by_agent } => {
                    let Some((order, _)) = first[n].clone() else { continue };
                    let request = id(&format!("client-req-{n}"));
                    let by = if by_agent { agent() } else { customer() };
                    let accepted_before = matches!(
                        env.intake.told(&request),
                        Some(Told::Exists { status: Status::Accepted { .. }, .. })
                    );
                    if !accepted_before && digest_of(&env, &request).is_none() {
                        let _ = env.quote(&mut sim, &order);
                    }
                    let Some(digest) = digest_of(&env, &request).or_else(|| accepted_before.then(|| {
                        serde_json::from_value(serde_json::json!({
                            "algorithm": "sha-256", "value": "00".repeat(32)
                        })).unwrap()
                    })) else { continue };
                    let before = env.len();
                    let answer = env.accept(&mut sim, &order, &by, &digest);
                    if accepted_before {
                        // Accepting again records nothing, and is told the same.
                        let again = matches!(answer, Ok(Told::Exists { status: Status::Accepted { .. }, .. }));
                        prop_assert!(again, "{:?}", answer);
                        prop_assert_eq!(env.len(), before);
                    }
                }
            }
        }
        // One order per identifier, each accepted and started at most once, and the log admits every entry.
        let orders: Vec<&Id> = first.iter().flatten().map(|(o, _)| o).collect();
        prop_assert!(orders.len() < 2 || orders[0] != orders[1]);
        for order in &orders {
            let accepted = env.acceptances().iter().filter(|a| matches!(&a.what, What::Acceptance { order: o, .. } if o == *order)).count();
            let started = env.started().iter().filter(|o| o.as_str() == order.as_str()).count();
            prop_assert!(accepted <= 1);
            prop_assert_eq!(started, accepted);
        }
        prop_assert!(env.admitted().is_ok(), "{:?}", env.admitted().err());
    }
}

// verifies: REQ-ORD-19
#[test]
fn caller_tells_refused_from_exists_with_outcome_to_come() {
    let mut env = Env::new();
    let mut sim = env.sim();

    // A request that breaks the rules for one, a caller asking in another party's name, is refused and nothing
    // exists: its refusal names no order, and asking again is told the same with nothing more recorded.
    let mut malformed = order_request("client-req-bad", customer());
    malformed.parties.requester = id("someone-else");
    let told = env.open(&mut sim, &malformed).unwrap();
    assert_eq!(
        told,
        Told::Refused {
            class: OrderRefusalClass::MalformedRequest
        }
    );
    assert_eq!(
        env.refusals(),
        vec![What::OrderRefusal {
            request: id("client-req-bad"),
            order: None,
            class: OrderRefusalClass::MalformedRequest,
        }]
    );
    let before = env.len();
    assert_eq!(env.open(&mut sim, &malformed).unwrap(), told);
    assert_eq!(env.len(), before);
    assert_eq!(env.intake.told(&id("client-req-bad")), Some(told.clone()));

    // A well-formed request exists, its outcome to come, and no refusal names it.
    let told = env
        .open(&mut sim, &order_request("client-req-1", customer()))
        .unwrap();
    let order = order_of(&told);
    assert_eq!(status_of(&told), &Status::OutcomeToCome);
    assert!(env.intake.record(&order).is_some());
    assert_eq!(env.refusals().len(), 1);

    // An order that then can't proceed still exists: its refusal names it.
    let mut sim = env.sim();
    sim.silent.insert(RESERVE.into());
    let told = env.quote(&mut sim, &order).unwrap();
    assert_eq!(order_of(&told), order);
    assert!(matches!(status_of(&told), Status::CantProceed { .. }));
    assert!(matches!(
        env.refusals().last(),
        Some(What::OrderRefusal { order: Some(o), .. }) if o == &order
    ));

    // On the wire the two answers can't be confused.
    let wire = |t: &Told| serde_json::to_value(t).unwrap()["told"].clone();
    assert_eq!(
        wire(&env.intake.told(&id("client-req-bad")).unwrap()),
        "refused"
    );
    assert_eq!(wire(&told), "exists");
    assert_admitted(&env);
}

// Classes of cause ---------------------------------------------------------------------------------------------------

// verifies: REQ-ORD-18
#[test]
fn no_route_for_this_amount_differs_from_no_route_right_now() {
    // Above every band the reserve advertises, no route runs at this amount: another amount may have one.
    let mut env = Env::new();
    let mut sim = env.sim();
    let mut request = order_request("client-req-1", customer());
    request.route = route("2000000000000");
    let order = order_of(&env.open(&mut sim, &request).unwrap());
    let told = env.quote(&mut sim, &order).unwrap();
    assert_eq!(
        status_of(&told),
        &Status::CantProceed {
            class: OrderRefusalClass::NoRouteForAmount
        }
    );
    assert_admitted(&env);

    // Inside the band with the reserve silent, there is no route right now: the same request may have one later,
    // and it is quoted once the reserve answers.
    let mut env = Env::new();
    let mut sim = env.sim();
    sim.silent.insert(RESERVE.into());
    let order = order_of(
        &env.open(&mut sim, &order_request("client-req-1", customer()))
            .unwrap(),
    );
    let told = env.quote(&mut sim, &order).unwrap();
    assert_eq!(
        status_of(&told),
        &Status::CantProceed {
            class: OrderRefusalClass::NoRouteRightNow
        }
    );
    let classes: Vec<OrderRefusalClass> = env
        .refusals()
        .iter()
        .filter_map(|w| match w {
            What::OrderRefusal {
                class, order: o, ..
            } if o.as_ref() == Some(&order) => Some(*class),
            _ => None,
        })
        .collect();
    assert_eq!(classes, vec![OrderRefusalClass::NoRouteRightNow]);
    let mut sim = env.sim();
    assert!(matches!(
        status_of(&env.quote(&mut sim, &order).unwrap()),
        Status::Quoted { .. }
    ));
    assert_admitted(&env);
}

/// What a provider might say when it fails, with markup and third parties' names.
const PROVIDER_TEXT: [&str; 4] = ["<script>", "Banco Ejemplo", "John Smith", "account frozen"];

/// Providers that answer as the simulation does, but every refusal carrying the provider's own failure text.
struct Texting(Sim);

impl Providers for Texting {
    fn ask(&mut self, provider: &Id, question: &[u8]) -> Option<Vec<u8>> {
        let bytes = self.0.ask(provider, question)?;
        let mut answer: Value = serde_json::from_slice(&bytes).unwrap();
        answer["statement"]["outcome"] = serde_json::json!({
            "status": "refused",
            "refusal": { "class": "final" },
            "message": "<script>alert(1)</script> account frozen at Banco Ejemplo, ask John Smith",
        });
        Some(sign(answer, &statements_key(provider.as_str(), 1)))
    }
}

// verifies: REQ-ORD-18
#[test]
fn provider_failure_text_never_reaches_the_customer() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let order = order_of(
        &env.open(&mut sim, &order_request("client-req-1", customer()))
            .unwrap(),
    );
    let mut texting = Texting(env.sim());
    let told = quote_with(&mut env, &mut texting, &order);
    // The provider's words reached Core: its answer is kept as received.
    assert!(
        env.evidence
            .kept
            .iter()
            .any(|b| String::from_utf8_lossy(b).contains("Banco Ejemplo"))
    );
    // The customer is told a class, and nothing the provider wrote, however it reads it.
    assert_eq!(
        status_of(&told),
        &Status::CantProceed {
            class: OrderRefusalClass::NoRouteRightNow
        }
    );
    let wire = serde_json::to_string(&told).unwrap();
    let debug = format!("{told:?}");
    let told_again = format!("{:?}", env.intake.told(&id("client-req-1")));
    for text in PROVIDER_TEXT {
        assert!(!wire.contains(text), "{wire}");
        assert!(!debug.contains(text), "{debug}");
        assert!(!told_again.contains(text), "{told_again}");
    }
    // Nor does any record Core wrote itself carry it: the log holds the statement by commitment only.
    let book = format!("{:?}", env.book.borrow().entries);
    for text in PROVIDER_TEXT {
        assert!(!book.contains(text));
    }
    assert_admitted(&env);
}

fn quote_with(env: &mut Env, providers: &mut Texting, order: &Id) -> Told {
    env.with_providers(providers, |intake, desk| {
        intake.quote::<rutent_router::RouteFinder>(desk, order)
    })
    .unwrap()
}

// The quote shown and its acceptance ---------------------------------------------------------------------------------

// verifies: REQ-ORD-34
#[test]
fn order_is_not_started_without_acceptance() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let request = order_request("client-req-1", customer());
    let (order, digest) = env.quoted(&mut sim, &request);

    // Shown, not started.
    assert!(env.book.borrow().verified().iter().any(
        |w| matches!(w, What::Shown { order: o, shown, .. } if o == &order && shown == &digest)
    ));
    assert!(env.started().is_empty());

    // An acceptance of another quote than the one shown, or by anyone but the requester, starts nothing.
    let other: Digest = serde_json::from_value(
        serde_json::json!({ "algorithm": "sha-256", "value": "00".repeat(32) }),
    )
    .unwrap();
    assert_eq!(
        env.accept(&mut sim, &order, &customer(), &other),
        Err(IntakeError::NotTheQuoteShown)
    );
    let stranger = Actor {
        party: id("stranger"),
        ..customer()
    };
    assert_eq!(
        env.accept(&mut sim, &order, &stranger, &digest),
        Err(IntakeError::NotTheRequester)
    );
    let operator = Actor {
        role: Role::Operator,
        ..customer()
    };
    assert_eq!(
        env.accept(&mut sim, &order, &operator, &digest),
        Err(IntakeError::NotTheRequester)
    );
    assert!(env.acceptances().is_empty());
    assert!(env.started().is_empty());

    // Policy not knowing moves nothing; policy refusing ends it, of its class.
    env.policy.answer = Answer::NotKnown {
        cause: NotKnownCause::PolicyUnreachable,
    };
    let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert_eq!(
        status_of(&told),
        &Status::Waiting {
            wait: Wait::PolicyNotKnown
        }
    );
    assert!(env.acceptances().is_empty() && env.started().is_empty());

    // Accepted on policy's permit: recorded, and only then started, once.
    env.policy.answer = Answer::Permit {};
    let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert!(matches!(status_of(&told), Status::Accepted { .. }));
    assert_eq!(env.acceptances().len(), 1);
    assert_eq!(env.started(), vec![order.as_str().to_owned()]);
    let calls: Vec<&str> = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .map(call_of)
        .collect();
    let at = |call: &str| calls.iter().rposition(|c| *c == call).unwrap();
    assert!(at("shown") < at("acceptance") && at("acceptance") < at("input"));
    assert_admitted(&env);

    // The log itself refuses a start with no acceptance before it.
    let mut env2 = Env::new();
    let mut sim2 = env2.sim();
    env2.accepted(&mut sim2, &request, &customer());
    env2.book.borrow_mut().entries.retain(
        |(_, e)| !matches!(e, Entry::Verified(v) if matches!(v.what, What::Acceptance { .. })),
    );
    let (_, why) = env2.admitted().expect_err("a start without its acceptance");
    assert!(why.contains("accept"), "{why}");

    // A refusal by policy is the order's end, told as a class.
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &request);
    env.policy.answer = refusal(RefusalClass::NotPermitted);
    let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert_eq!(
        status_of(&told),
        &Status::CantProceed {
            class: OrderRefusalClass::RefusedByPolicy
        }
    );
    assert!(env.acceptances().is_empty() && env.started().is_empty());
    assert_admitted(&env);
}

// verifies: REQ-ORD-19, INV-8
#[test]
fn acceptance_whose_append_failed_is_carried_on_once() {
    // The start's append fails once: the order isn't told accepted, and accepting again appends the start alone.
    for failing in ["input", "acceptance"] {
        let mut env = Env::new();
        let mut sim = env.sim();
        let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));
        env.book.borrow_mut().fail_next = Some(failing);
        assert!(matches!(
            env.accept(&mut sim, &order, &customer(), &digest),
            Err(IntakeError::Log(_))
        ));
        assert!(env.started().is_empty());
        assert!(!matches!(
            env.intake.told(&id("client-req-1")),
            Some(Told::Exists {
                status: Status::Accepted { .. },
                ..
            })
        ));
        let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
        assert!(
            matches!(status_of(&told), Status::Accepted { .. }),
            "{failing}"
        );
        // One authorisation asked of policy, one acceptance, one start, and the log admits them.
        assert_eq!(env.policy.asked.len(), 1, "{failing}");
        assert_eq!(env.acceptances().len(), 1, "{failing}");
        assert_eq!(env.started(), vec![order.as_str().to_owned()], "{failing}");
        assert_admitted(&env);
    }
}

// verifies: REQ-ORD-10
#[test]
fn agents_acceptance_is_recorded_as_an_agents() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", agent()));
    env.accept(&mut sim, &order, &agent(), &digest).unwrap();
    let acceptance = env.acceptances().pop().expect("an acceptance");
    assert_eq!(acceptance.who, agent());
    assert_eq!(acceptance.who.kind, ActorKind::Agent);
    assert_eq!(acceptance.who.acts, id("agent-7"));
    let What::Acceptance { quote, .. } = &acceptance.what else {
        unreachable!()
    };
    // What it accepted is the quote shown, and the authorisation names the agent as who gave it.
    assert_eq!(quote, &digest);
    assert_eq!(env.policy.asked[0].given_by, agent());
    assert_admitted(&env);
}

// verifies: REQ-ORD-17
#[test]
fn requester_beneficiary_and_payer_are_recorded_apart() {
    let mut env = Env::new();
    let mut sim = env.sim();
    env.accepted(
        &mut sim,
        &order_request("client-req-1", customer()),
        &customer(),
    );
    let acceptance = env.acceptances().pop().expect("an acceptance");
    let What::Acceptance { parties, .. } = &acceptance.what else {
        unreachable!()
    };
    assert_eq!(parties, &three_parties());
    assert!(
        parties.requester != parties.beneficiary
            && parties.beneficiary != parties.payer
            && parties.requester != parties.payer
    );
    assert_eq!(parties.settlement_account_at, None);
    assert_admitted(&env);

    // A settlement account's beneficiary: the record says its payees' checks fall to the reserve that holds it.
    let mut env = Env::new();
    let mut sim = env.sim();
    let mut request = order_request("client-req-2", customer());
    request.parties.settlement_account_at = Some(id(RESERVE));
    env.accepted(&mut sim, &request, &customer());
    let acceptance = env.acceptances().pop().expect("an acceptance");
    let What::Acceptance { parties, .. } = &acceptance.what else {
        unreachable!()
    };
    assert_eq!(parties.settlement_account_at, Some(id(RESERVE)));
    assert_admitted(&env);

    // A settlement account held at a reserve other than the destination's is refused, nothing created.
    let mut request = order_request("client-req-3", customer());
    request.parties.settlement_account_at = Some(id("another-reserve"));
    assert_eq!(
        env.open(&mut sim, &request).unwrap(),
        Told::Refused {
            class: OrderRefusalClass::MalformedRequest
        }
    );
}

// The destination ----------------------------------------------------------------------------------------------------

// verifies: REQ-ORD-24
#[test]
fn destinations_change_of_owner_makes_the_order_wait() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));

    // The account now resolves to another owner: the order waits for the customer, and nothing is accepted.
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::Owner(id("owner-2")));
    let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert_eq!(
        status_of(&told),
        &Status::Waiting {
            wait: Wait::OwnerChanged
        }
    );
    assert!(env.book.borrow().verified().iter().any(|w| matches!(
        w,
        What::DestinationCheck {
            outcome: rutent_contracts::core_internal::DestinationOutcome::OwnerChanged,
            ..
        }
    )));
    assert!(env.acceptances().is_empty() && env.started().is_empty());
    assert_eq!(
        env.accept(&mut sim, &order, &customer(), &digest),
        Err(IntakeError::NotNow)
    );

    // Only the requester confirms the owner it now resolves to; then the quote is shown again, and accepted.
    let stranger = Actor {
        party: id("stranger"),
        ..customer()
    };
    assert_eq!(
        env.intake.confirm_owner(&order, &stranger, &id("owner-2")),
        Err(IntakeError::NotTheRequester)
    );
    assert_eq!(
        env.intake
            .confirm_owner(&order, &customer(), &id("owner-3")),
        Err(IntakeError::NotNow)
    );
    let told = env
        .intake
        .confirm_owner(&order, &customer(), &id("owner-2"))
        .unwrap();
    assert!(matches!(status_of(&told), Status::Quoted { .. }));
    let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert!(matches!(status_of(&told), Status::Accepted { .. }));
    assert_admitted(&env);

    // "Not yet" waits and differs from "no", which ends the order.
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::NotYet);
    let not_yet = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert_eq!(
        status_of(&not_yet),
        &Status::Waiting {
            wait: Wait::DestinationNotYetResolved
        }
    );
    // A late destination: once it resolves, to the owner first resolved, the waiting order is accepted.
    let mut late = Env::new();
    let mut late_sim = late.sim();
    let (late_order, late_digest) =
        late.quoted(&mut late_sim, &order_request("client-req-1", customer()));
    late.register
        .answers
        .insert(ADDRESS.into(), Resolution::NotYet);
    late.accept(&mut late_sim, &late_order, &customer(), &late_digest)
        .unwrap();
    late.register
        .answers
        .insert(ADDRESS.into(), Resolution::Owner(id(OWNER)));
    let told = late
        .accept(&mut late_sim, &late_order, &customer(), &late_digest)
        .unwrap();
    assert!(matches!(status_of(&told), Status::Accepted { .. }));
    assert_admitted(&late);

    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::Refused);
    let no = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert_eq!(
        status_of(&no),
        &Status::CantProceed {
            class: OrderRefusalClass::DestinationRefused
        }
    );
    let classes: Vec<OrderRefusalClass> = env
        .refusals()
        .iter()
        .filter_map(|w| match w {
            What::OrderRefusal { class, .. } => Some(*class),
            _ => None,
        })
        .collect();
    assert_eq!(
        classes,
        vec![
            OrderRefusalClass::DestinationNotYetResolved,
            OrderRefusalClass::DestinationRefused
        ]
    );
    assert!(env.acceptances().is_empty() && env.started().is_empty());
    assert_admitted(&env);
}

// verifies: REQ-ORD-35
#[test]
fn deposit_to_the_requesters_address_is_part_of_the_authorised_destination() {
    let mut env = Env::new();
    let mut sim = env.sim();
    env.accepted(
        &mut sim,
        &order_request("client-req-1", customer()),
        &customer(),
    );

    // The reserve's deposit is asked to the address the requester gave.
    let deposits: Vec<&Value> = sim
        .asked(RESERVE)
        .into_iter()
        .filter(|op| op["kind"] == "deposit")
        .collect();
    assert!(!deposits.is_empty());
    for deposit in deposits {
        assert_eq!(deposit["inputs"][DESTINATION_INPUT], ADDRESS);
    }
    // The authorisation names it as the destination's account, and the acceptance names that authorisation.
    let authorisation = env.policy.asked.pop().expect("an authorisation");
    assert_eq!(authorisation.destination, destination());
    assert_eq!(authorisation.destination.account, id(ADDRESS));
    let acceptance = env.acceptances().pop().expect("an acceptance");
    let What::Acceptance {
        authorisation: named,
        ..
    } = &acceptance.what
    else {
        unreachable!()
    };
    assert_eq!(named, &authorisation.authorisation);
    assert_admitted(&env);

    // A deposit input naming another address than the destination's, or a destination on another network than
    // the route's, is refused with nothing created.
    let mut request = order_request("client-req-2", customer());
    request
        .inputs
        .insert(DESTINATION_INPUT.into(), "0xdead".into());
    let mut on_base = order_request("client-req-3", customer());
    on_base.destination.node = OuterNode {
        network: "base".into(),
    };
    for request in [request, on_base] {
        assert_eq!(
            env.open(&mut sim, &request).unwrap(),
            Told::Refused {
                class: OrderRefusalClass::MalformedRequest
            }
        );
    }
}

// verifies: REQ-OPS-5
#[test]
fn human_and_agent_callers_use_the_same_contract() {
    // The same request, through the same calls, by a person and by an agent of the same party.
    let run = |caller: Actor| {
        let mut env = Env::new();
        let mut sim = env.sim();
        let (_, told) = env.accepted(
            &mut sim,
            &order_request("client-req-1", caller.clone()),
            &caller,
        );
        assert!(matches!(status_of(&told), Status::Accepted { .. }));
        assert_admitted(&env);
        let calls: Vec<&str> = env
            .book
            .borrow()
            .verified()
            .into_iter()
            .map(call_of)
            .collect();
        let mut acceptance = env.acceptances().pop().expect("an acceptance");
        let who = acceptance.who.clone();
        acceptance.who = customer();
        (calls, acceptance.what, who)
    };
    let (person_calls, person_acceptance, person) = run(customer());
    let (agent_calls, agent_acceptance, agent_who) = run(agent());
    // Every record is the same but who acted, which says which kind it was.
    assert_eq!(person_calls, agent_calls);
    assert_eq!(person_acceptance, agent_acceptance);
    assert_eq!(person.kind, ActorKind::Person);
    assert_eq!(agent_who.kind, ActorKind::Agent);
    assert_eq!(person.party, agent_who.party);
}
