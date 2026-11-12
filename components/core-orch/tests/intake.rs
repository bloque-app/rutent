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
use rutent_contracts::router_api::{
    CatalogueSnapshot, DecisionRecord, PolicyBounds, Request, Router,
};
use rutent_core_orch::intake::{
    DESTINATION_INPUT, IntakeError, OrderRequest, Resolution, Status, Told, Wait,
};
use rutent_core_orch::rounds::Providers;
use serde_json::{Value, json};
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
fn digest_of(env: &mut Env, request: &Id) -> Option<Digest> {
    match env.told_customer(request.as_str())? {
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
                        env.told_customer(request.as_str()),
                        Some(Told::Exists { status: Status::Accepted { .. }, .. })
                    );
                    if !accepted_before && digest_of(&mut env, &request).is_none() {
                        let _ = env.quote(&mut sim, &order);
                    }
                    let Some(digest) = digest_of(&mut env, &request).or_else(|| accepted_before.then(|| {
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
    assert_eq!(env.told_customer("client-req-bad"), Some(told.clone()));

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
        wire(&env.told_customer("client-req-bad").unwrap()),
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
        env.store
            .borrow()
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
    let told_again = format!("{:?}", env.told_customer("client-req-1"));
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
        intake.quote::<rutent_router::RouteFinder>(desk, &customer(), order)
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

    // An acceptance of another quote than the one shown, or by anyone but the requester, starts nothing; to anyone
    // but the requester the order doesn't exist.
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
    let operator = Actor {
        role: Role::Operator,
        ..customer()
    };
    for other in [stranger, operator, impostor()] {
        assert_eq!(
            env.accept(&mut sim, &order, &other, &digest),
            Err(IntakeError::NoSuchOrder),
            "{other:?}"
        );
    }
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

    // A refusal about the caller leaves the order as it was, to accept by one policy permits; a refusal about the
    // order itself, outside what the authorisation may name, is the order's end, told as a class.
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &request);
    env.policy.answer = refusal(RefusalClass::NotPermitted);
    assert_eq!(
        env.accept(&mut sim, &order, &customer(), &digest),
        Err(IntakeError::Refused {
            class: OrderRefusalClass::RefusedByPolicy
        })
    );
    assert!(matches!(
        env.told_customer("client-req-1"),
        Some(Told::Exists {
            status: Status::Quoted { .. },
            ..
        })
    ));
    assert!(env.refusals().is_empty());
    env.policy.answer = refusal(RefusalClass::OutsideAuthorisation);
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
            env.told_customer("client-req-1"),
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

    // The append is written but its answer lost, and the clock moves on before the retry: the entry appended again
    // is the one built first, so the log holds one acceptance and one start, and admits them.
    for losing in ["input", "acceptance"] {
        let mut env = Env::new();
        let mut sim = env.sim();
        let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));
        env.book.borrow_mut().lose_next = Some(losing);
        assert!(env.accept(&mut sim, &order, &customer(), &digest).is_err());
        env.clock
            .set(rutent_core_orch::snapshot::Clock::now(&*env.clock).seconds + 7);
        let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
        assert!(
            matches!(status_of(&told), Status::Accepted { .. }),
            "{losing}"
        );
        let mut acceptances = env.acceptances();
        acceptances.dedup();
        assert_eq!(acceptances.len(), 1, "{losing}");
        let mut started = env.started();
        started.dedup();
        assert_eq!(started, vec![order.as_str().to_owned()], "{losing}");
        assert_eq!(env.policy.asked.len(), 1, "{losing}");
        assert_admitted(&env);
    }

    // An append the log refuses isn't retried: the session the yes was asked in has ended, so the acceptance is
    // refused, and the order ends with a class instead of waiting on a retry that can't succeed.
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));
    env.clock.set(SESSION_ENDS + 10);
    let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert!(
        matches!(status_of(&told), Status::CantProceed { .. }),
        "{told:?}"
    );
    assert!(env.acceptances().is_empty() && env.started().is_empty());
    assert_eq!(
        env.accept(&mut sim, &order, &customer(), &digest),
        Ok(told.clone())
    );
    assert_eq!(env.quote(&mut sim, &order), Err(IntakeError::NotNow));
    assert_admitted(&env);
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

    // The confirmation is an acceptance naming the new owner, through policy like any other. Anyone but the
    // requester, an actor that only states its party included, isn't told the order exists; another owner than
    // the one it now resolves to confirms nothing; a refusal of the caller leaves the order waiting.
    let stranger = Actor {
        party: id("stranger"),
        ..customer()
    };
    for other in [stranger, impostor()] {
        assert_eq!(
            env.confirm(&mut sim, &order, &other, &digest, &id("owner-2")),
            Err(IntakeError::NoSuchOrder)
        );
    }
    assert_eq!(
        env.confirm(&mut sim, &order, &customer(), &digest, &id("owner-3")),
        Err(IntakeError::NotNow)
    );
    env.policy.answer = refusal(RefusalClass::NotPermitted);
    assert!(matches!(
        env.confirm(&mut sim, &order, &customer(), &digest, &id("owner-2")),
        Err(IntakeError::Refused { .. })
    ));
    assert!(matches!(
        env.told_customer("client-req-1"),
        Some(Told::Exists {
            status: Status::Waiting {
                wait: Wait::OwnerChanged
            },
            ..
        })
    ));
    assert!(env.acceptances().is_empty() && env.started().is_empty());

    // Confirmed by the requester on policy's permit: accepted, the acceptance naming who confirmed, after the
    // destination resolved to the owner confirmed.
    env.policy.answer = Answer::Permit {};
    let told = env
        .confirm(&mut sim, &order, &customer(), &digest, &id("owner-2"))
        .unwrap();
    assert!(matches!(status_of(&told), Status::Accepted { .. }));
    assert_eq!(env.acceptances().pop().unwrap().who, customer());
    assert_eq!(env.policy.asked.last().unwrap().given_by, customer());
    let outcomes: Vec<rutent_contracts::core_internal::DestinationOutcome> = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::DestinationCheck { outcome, .. } => Some(*outcome),
            _ => None,
        })
        .collect();
    assert_eq!(
        outcomes.last(),
        Some(&rutent_contracts::core_internal::DestinationOutcome::Resolved)
    );
    assert_admitted(&env);

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

// verifies: REQ-ORD-10, REQ-AUZ-9
#[test]
fn acceptance_is_carried_on_only_by_the_actor_policy_permitted() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));
    env.book.borrow_mut().fail_next = Some("acceptance");
    assert!(env.accept(&mut sim, &order, &customer(), &digest).is_err());

    // An actor stating the customer's party that doesn't act for it isn't told the order exists; another actor that
    // does act for it, whom policy never checked for this authorisation, can't carry it on.
    assert_eq!(
        env.accept(&mut sim, &order, &impostor(), &digest),
        Err(IntakeError::NoSuchOrder)
    );
    assert_eq!(
        env.accept(&mut sim, &order, &agent(), &digest),
        Err(IntakeError::OtherActor)
    );
    assert!(env.acceptances().is_empty() && env.started().is_empty());

    // The one policy permitted carries it on, and the records name it.
    let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert!(matches!(status_of(&told), Status::Accepted { .. }));
    let acceptance = env.acceptances().pop().expect("an acceptance");
    assert_eq!(acceptance.who, customer());
    let start = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .find_map(|w| match w {
            What::Input { input } => Some(input.source.id.as_str().to_owned()),
            _ => None,
        })
        .expect("a start");
    assert_eq!(start, customer().acts.as_str());
    assert_admitted(&env);
}

// verifies: REQ-ORD-19
#[test]
fn request_identifier_is_the_requesters_own() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let order = order_of(
        &env.open(&mut sim, &order_request("client-req-1", customer()))
            .unwrap(),
    );

    // Another party under the same identifier opens its own order, and learns nothing of the first.
    let other = Actor {
        party: id("other-customer"),
        acts: id("user-9"),
        ..customer()
    };
    env.actors.borrow_mut().list(&other);
    env.sessions.held.insert(
        "session-9".into(),
        rutent_core_orch::intake::HeldSession {
            holder: other.party.clone(),
            expires_at: rutent_contracts::router_api::Timestamp::new("2026-12-01T00:00:00Z")
                .unwrap(),
        },
    );
    let mut theirs = order_request("client-req-1", other.clone());
    theirs.parties.requester = other.party.clone();
    theirs.session = id("session-9");
    let told = env.open(&mut sim, &theirs).unwrap();
    assert_ne!(order_of(&told), order);
    assert!(!format!("{told:?}").contains(order.as_str()));

    // Neither another party nor an actor that only states the customer's party is told anything of it, nor can
    // quote it.
    assert_ne!(
        env.told(&other, "client-req-1"),
        env.told_customer("client-req-1")
    );
    assert_eq!(env.told(&impostor(), "client-req-1"), None);
    for caller in [other, impostor()] {
        assert_eq!(
            env.quote_by(&mut sim, &caller, &order),
            Err(IntakeError::NoSuchOrder)
        );
    }
    // An impostor's request under the customer's identifier opens nothing and tells nothing.
    let before = env.len();
    assert_eq!(
        env.open(&mut sim, &order_request("client-req-1", impostor())),
        Err(IntakeError::NotTheRequester)
    );
    assert_eq!(env.len(), before);
    // The requester itself, through its agent too, is told and quotes.
    assert!(env.told(&agent(), "client-req-1").is_some());
    assert!(env.quote_by(&mut sim, &agent(), &order).is_ok());
    assert_admitted(&env);
}

// verifies: REQ-ORD-19
#[test]
fn open_whose_append_failed_keeps_its_order() {
    // The destination's check is written, and the refusal it leads to fails to append: the same request again
    // carries on with the same order, so every record names the one order its caller is told.
    let mut env = Env::new();
    let mut sim = env.sim();
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::Refused);
    let request = order_request("client-req-1", customer());
    for failing in ["destination_check", "order_refusal"] {
        env.book.borrow_mut().fail_next = Some(failing);
        assert!(env.open(&mut sim, &request).is_err(), "{failing}");
    }
    let order = order_of(&env.open(&mut sim, &request).unwrap());
    let named: Vec<Option<Id>> = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::DestinationCheck { order, .. } => Some(Some(order.clone())),
            What::OrderRefusal { order, .. } => Some(order.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(named, vec![Some(order.clone()), Some(order)]);
    assert_admitted(&env);
}

// verifies: REQ-ORD-32
#[test]
fn session_that_isnt_the_requesters_is_refused_before_the_reserve_is_asked() {
    for case in ["another holder", "ended", "unknown"] {
        let mut env = Env::new();
        let mut sim = env.sim();
        match case {
            "another holder" => {
                env.sessions.held.get_mut(SESSION).unwrap().holder = id("someone-elses-party")
            }
            "ended" => env.clock.set(SESSION_ENDS + 1),
            _ => {
                env.sessions.held.clear();
            }
        }
        let told = env
            .open(&mut sim, &order_request("client-req-1", customer()))
            .unwrap();
        assert_eq!(
            told,
            Told::Refused {
                class: OrderRefusalClass::RefusedByPolicy
            },
            "{case}"
        );
        assert!(sim.received.is_empty(), "{case}");
        assert_admitted(&env);
    }
}

// verifies: REQ-OPS-5, REQ-ORD-19
#[test]
fn agent_repeating_its_persons_request_gets_the_same_answer() {
    // The person opens; its agent sends the same request under the same identifier, and is told the same, with
    // nothing more recorded: who acts for the party is left out when a repeat is compared.
    let mut env = Env::new();
    let mut sim = env.sim();
    let first = env
        .open(&mut sim, &order_request("client-req-1", customer()))
        .unwrap();
    let before = env.len();
    let again = env
        .open(&mut sim, &order_request("client-req-1", agent()))
        .unwrap();
    assert_eq!(again, first);
    assert_eq!(env.len(), before);

    // Another body under the identifier is still another request, whoever sends it.
    let mut other = order_request("client-req-1", agent());
    other.route = route("999");
    assert!(matches!(
        env.open(&mut sim, &other),
        Err(IntakeError::Conflicting { .. })
    ));
    assert_admitted(&env);
}

// Each part's yes, a change of owner's class, a start refused ---------------------------------------------------------

thread_local! {
    /// The two parts' amounts [`Splitter`] splits into.
    static SPLIT: std::cell::Cell<[&'static str; 2]> = const { std::cell::Cell::new(["600", "400"]) };
}

/// A router that splits the route the router finds into two parts, of 600 and 400 unless [`SPLIT`] says otherwise,
/// each citing its own dry runs: unconfirmed until both parts' withdrawals and deposits were asked at their amounts,
/// then chosen.
struct Splitter;

impl Router for Splitter {
    fn route(
        request: &Request,
        snapshot: &CatalogueSnapshot,
        bounds: &PolicyBounds,
    ) -> DecisionRecord {
        let record = rutent_router::RouteFinder::route(request, snapshot, bounds);
        if matches!(
            request.round,
            rutent_contracts::router_api::Round::Shortlist {}
        ) {
            return record;
        }
        let mut value = serde_json::to_value(&record).unwrap();
        let template = value["candidates"][0]["parts"][0]["operations"].clone();
        let figures = serde_json::to_value(&snapshot.figures).unwrap();
        let dry_run = |tuple: &Value, amount: &str| {
            figures.as_array().unwrap().iter().find_map(|f| {
                let v = &f["value"];
                (v["kind"] == "dry_run"
                    && &v["tuple"] == tuple
                    && v["amount_in"] == amount
                    && v["result"]["statement"]["outcome"]["status"] == "dry_run_accepted")
                    .then(|| f["figure_id"].clone())
            })
        };
        let mut unconfirmed = None;
        let mut parts = Vec::new();
        for (p, amount) in SPLIT.with(|s| s.get()).into_iter().enumerate() {
            let mut operations = template.clone();
            for (o, op) in operations.as_array_mut().unwrap().iter_mut().enumerate() {
                op["amount_in"] = amount.into();
                op["amount_out"] = amount.into();
                let mut cited: Vec<Value> = op["figures"].as_array().unwrap()[..2].to_vec();
                match dry_run(&cited[0], amount) {
                    Some(figure) => cited.push(figure),
                    None if unconfirmed.is_none() => unconfirmed = Some((p, o)),
                    None => {}
                }
                op["figures"] = Value::Array(cited);
            }
            parts.push(json!({ "operations": operations }));
        }
        let mut split = json!({ "candidate_id": "c-split", "parts": parts });
        match unconfirmed {
            Some((part, operation)) => {
                split["dropped"] =
                    json!({ "reason": "unconfirmed", "part": part, "operation": operation });
                value.as_object_mut().unwrap().remove("chosen");
            }
            None => value["chosen"] = "c-split".into(),
        }
        value["candidates"] = json!([split]);
        serde_json::from_value(value).unwrap()
    }
}

// verifies: REQ-ORD-32
#[test]
fn split_orders_acceptance_names_each_parts_yes() {
    let mut env = Env::new();
    let mut bounds = serde_json::to_value(&env.bounds).unwrap();
    bounds["max_parts"] = 2.into();
    env.bounds = serde_json::from_value(bounds).unwrap();
    let mut sim = env.sim();
    let order = order_of(
        &env.open(&mut sim, &order_request("client-req-1", customer()))
            .unwrap(),
    );
    let told = env
        .with(&mut sim, |intake, desk| {
            intake.quote::<Splitter>(desk, &customer(), &order)
        })
        .unwrap();
    let Status::Quoted { digest, .. } = status_of(&told).clone() else {
        panic!("no quote: {told:?}");
    };
    let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert!(matches!(status_of(&told), Status::Accepted { .. }));

    // The acceptance names a yes for each part of the start's plan, in its order: each its own statement, of the
    // part's origin reserve, the provider of its first move.
    let acceptance = env.acceptances().pop().expect("an acceptance");
    let What::Acceptance { yeses, .. } = &acceptance.what else {
        unreachable!()
    };
    let start = env.intake.start(&order).expect("a start").clone();
    let rutent_contracts::state_machine::Observed::RouteChosen { parts, .. } = &start.observed
    else {
        unreachable!()
    };
    assert_eq!(parts.len(), 2);
    assert_eq!(yeses.len(), 2);
    for (yes, part) in yeses.iter().zip(parts) {
        assert_eq!(yes.part.as_str(), part.part_id.as_str());
        assert_eq!(yes.origin_reserve, id(RESERVE));
    }
    assert_ne!(yeses[0].reserve_yes, yeses[1].reserve_yes);
    // The real `admit_entry` admits the acceptance on each part's yes, and the start on it.
    assert_admitted(&env);
}

// verifies: REQ-ORD-24
#[test]
fn change_of_owner_is_refused_with_its_class() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));
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
    // The check of the owner changed, then the order's refusal of its class, naming the order.
    let book = env.book.borrow();
    let whats = book.verified();
    let check = whats
        .iter()
        .rposition(|w| {
            matches!(
                w,
                What::DestinationCheck {
                    outcome: rutent_contracts::core_internal::DestinationOutcome::OwnerChanged,
                    ..
                }
            )
        })
        .expect("an owner_changed check");
    assert_eq!(
        whats[check + 1],
        &What::OrderRefusal {
            request: id("client-req-1"),
            order: Some(order.clone()),
            class: OrderRefusalClass::DestinationOwnerChanged,
        }
    );
    drop(book);
    assert!(env.acceptances().is_empty() && env.started().is_empty());
    assert_admitted(&env);
}

// verifies: REQ-ORD-34
#[test]
fn half_accepted_order_is_never_started() {
    // The log takes the acceptance and refuses the start: the order ends, half-accepted, and has no start for the
    // segments to follow, whatever the acceptance says.
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, digest) = env.quoted(&mut sim, &order_request("client-req-1", customer()));
    env.book.borrow_mut().refuse_next = Some("input");
    let told = env.accept(&mut sim, &order, &customer(), &digest).unwrap();
    assert_eq!(
        status_of(&told),
        &Status::CantProceed {
            class: OrderRefusalClass::NoRouteRightNow
        }
    );
    assert_eq!(env.acceptances().len(), 1);
    assert!(env.started().is_empty());
    assert_eq!(env.intake.start(&order), None);
    // Nothing quotes or accepts it again.
    assert_eq!(env.accept(&mut sim, &order, &customer(), &digest), Ok(told));
    assert_eq!(env.quote(&mut sim, &order), Err(IntakeError::NotNow));
    assert_eq!(env.intake.start(&order), None);
    assert_admitted(&env);

    // An order accepted whole has its start, the one the log took.
    let mut env = Env::new();
    let mut sim = env.sim();
    let (order, _) = env.accepted(
        &mut sim,
        &order_request("client-req-1", customer()),
        &customer(),
    );
    let start = env.intake.start(&order).expect("a start");
    let logged = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .find_map(|w| match w {
            What::Input { input } => Some(input.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(start, &logged);
}

// verifies: REQ-ORD-32
#[test]
fn even_split_answered_by_one_yes_is_not_shown() {
    // Two parts of 500 at one reserve: the rounds ask the withdrawal once at 500, so both parts would rest on one
    // statement. The route isn't shown, the order has no route right now, and policy is never asked.
    let mut env = Env::new();
    let mut bounds = serde_json::to_value(&env.bounds).unwrap();
    bounds["max_parts"] = 2.into();
    env.bounds = serde_json::from_value(bounds).unwrap();
    let mut sim = env.sim();
    let order = order_of(
        &env.open(&mut sim, &order_request("client-req-1", customer()))
            .unwrap(),
    );
    SPLIT.with(|s| s.set(["500", "500"]));
    let told = env.with(&mut sim, |intake, desk| {
        intake.quote::<Splitter>(desk, &customer(), &order)
    });
    SPLIT.with(|s| s.set(["600", "400"]));
    let told = told.unwrap();
    // Whatever was shown, the customer accepting it can't reach policy.
    if let Status::Quoted { digest, .. } = status_of(&told).clone() {
        let _ = env.accept(&mut sim, &order, &customer(), &digest);
    }
    assert_eq!(
        status_of(&told),
        &Status::CantProceed {
            class: OrderRefusalClass::NoRouteRightNow
        }
    );
    assert!(
        !env.book
            .borrow()
            .verified()
            .iter()
            .any(|w| matches!(w, What::Shown { .. }))
    );
    assert!(env.policy.asked.is_empty());
    assert!(env.acceptances().is_empty() && env.started().is_empty());
    assert_admitted(&env);
}

/// Only Core recalculates (`T-020-10`): a request whose route claims to follow another routing is refused before an
/// order exists, so no customer can route from where its value isn't evidenced, or past the withdrawal and its yes.
// verifies: REQ-ORD-2, REQ-ORD-32
#[test]
fn request_claiming_to_follow_another_routing_is_refused() {
    let mut env = Env::new();
    let mut sim = env.sim();
    let mut following = order_request("client-req-follows", customer());
    following.route.follows = Some(id("req-choice-of-another"));
    assert_eq!(
        env.open(&mut sim, &following).unwrap(),
        Told::Refused {
            class: OrderRefusalClass::MalformedRequest
        }
    );
    assert!(sim.received.is_empty(), "no provider was asked");
}
