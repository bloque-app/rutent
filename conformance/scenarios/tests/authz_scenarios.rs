//! Authorisation scenarios over the assembled Core (`T-024-07`): a verifier out of reach (`AC-17`), a thousand agents
//! under one accountable party (`AC-18`) and a caller an external authentication layer vouches for (`AC-25`). Each
//! drives Core's own policy through the assembly, whose engine, fixed rules and sources are the production ones.

use rutent_contracts::core_internal::calls::{Charges, Decisions};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, CatalogueChange, Charge, ChargeOutcome, Count,
    DecisionRequest, Entry, NotKnownCause, Outcome, RefusalClass, Role, VerifiedEntry, What, Work,
};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::OrderState;
use rutent_core_authz::parties::Scope;
use rutent_core_orch::intake::{Status, Told};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::policy::Verifier;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};

/// Core put together with no order: the deployment's registers as the assembly lists them, and nothing run.
fn bare(name: &'static str, accepts: &'static str) -> Core {
    Scenario::new(name, accepts, Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference)
        .0
}

/// The last verified entry the log took.
fn last(core: &Core) -> VerifiedEntry {
    core.log()
        .entries()
        .iter()
        .rev()
        .find_map(|(_, e)| match e {
            Entry::Verified(v) => Some(v.clone()),
            Entry::Stated(_) => None,
        })
        .expect("a verified entry")
}

// --- AC-25: an external authentication layer says who is calling, and policy still decides ----------------------------

/// A stand-in for the stand-alone deployment's external authentication layer (single sign-on, a gateway): it turns a
/// credential into the caller it says is calling, and nothing else. It holds no permission and no rule, and Core's
/// calls take the caller it names as an [`Actor`], never the credential, so the layer has no way to allow anything:
/// what it hands over is an input to policy (`REQ-AUZ-1`).
struct Gateway {
    identities: Vec<(&'static str, Actor)>,
}

impl Gateway {
    fn new() -> Self {
        // The layer vouches for each of these, including one in its own group "administrators", whom the deployment
        // never listed, and one that states the customer's party for a login nobody registered.
        let sso_administrator = Actor {
            party: id("operator"),
            acts: id("sso-administrator"),
            role: Role::Operator,
            kind: ActorKind::Person,
        };
        let stranger_of_the_customers_party = Actor {
            party: kit::customer().party,
            acts: id("sso-stranger"),
            role: Role::Customer,
            kind: ActorKind::Person,
        };
        Gateway {
            identities: vec![
                ("token-customer", kit::customer()),
                ("token-operator-login", kit::operator_login()),
                ("token-sso-administrator", sso_administrator),
                ("token-sso-stranger", stranger_of_the_customers_party),
            ],
        }
    }

    /// Who the credential says is calling, where it says anyone.
    fn authenticate(&self, credential: &str) -> Option<Actor> {
        self.identities
            .iter()
            .find(|(token, _)| *token == credential)
            .map(|(_, who)| who.clone())
    }
}

fn promotion(core: &mut Core, caller: &Actor, request: &str) -> DecisionRequest {
    DecisionRequest {
        request: id(request),
        caller: caller.clone(),
        action: Action::ChangeCatalogue {
            change: CatalogueChange::Promote,
            provider: id(kit::RESERVE),
            version: Some(id("1.0.0")),
            tuple: None,
            class: None,
            advertising_keys: Vec::new(),
            statement_key: None,
            setting: None,
        },
        at: core.clock.timestamp(),
    }
}

/// Every caller the layer vouches for is asked of policy as the actor the layer names. The one the deployment listed
/// and gave the permission is permitted by the register; the administrator the layer calls one, and a stranger the
/// layer says acts for the customer's party, are refused, on the record, for what the register doesn't hold; and a
/// listed, permitted caller is refused when the deployment's policy refuses. Core's calls take no credential, so
/// nothing the layer says can stand in for a decision.
// verifies: AC-25, REQ-AUZ-1, REQ-OPS-4
#[test]
fn externally_authenticated_caller_is_still_decided_by_policy() {
    let mut core = bare("an authentication layer in front", "AC-25");
    let gateway = Gateway::new();
    let who = |credential: &str| {
        gateway
            .authenticate(credential)
            .expect("the layer knows it")
    };
    let decide = |core: &mut Core, caller: &Actor, request: &str| {
        let request = promotion(core, caller, request);
        core.policy().decide(&request).expect("the log took it")
    };

    // A credential the layer doesn't know authenticates no one: nothing reaches policy.
    assert!(gateway.authenticate("token-forged").is_none());

    // The login the register lists and gave the permission is permitted, and the entry names it as the caller.
    let operator = who("token-operator-login");
    let permitted = decide(&mut core, &operator, "promote-by-the-listed-login");
    assert_eq!(permitted.outcome, Answer::Permit {});
    assert_eq!(last(&core).outcome, Outcome::Permitted {});

    // The layer says the same of the SSO administrator, in the operator's party and role. Policy holds nothing for
    // it: an input it doesn't have, so a refusal, and nothing may take effect on it.
    let administrator = who("token-sso-administrator");
    let refused = decide(
        &mut core,
        &administrator,
        "promote-by-the-sso-administrator",
    );
    assert_eq!(
        refused.outcome,
        Answer::Refuse {
            class: RefusalClass::MissingInput
        },
        "the layer's word that it is an administrator isn't a rule of policy's"
    );
    assert!(refused.permit().is_none());
    assert_eq!(
        last(&core).outcome,
        Outcome::Refused {
            class: RefusalClass::MissingInput
        }
    );

    // The layer says a login acts for the customer's party. The register doesn't list it: it acts for no one, and a
    // session in the customer's name opened by it is refused.
    let stranger = who("token-sso-stranger");
    let opened = core.open_session(&stranger);
    assert!(opened.is_err(), "the stranger opened a session: {opened:?}");
    // The same login, listed by the deployment for that party, is let in by the register: authentication gave it
    // nothing before, and the register's word is what changes the answer.
    core.registers().list(&stranger);
    assert!(
        core.open_session(&stranger).is_ok(),
        "a listed login of the customer's party opens its session"
    );

    // An agent the layer vouches for and the register lists, with no scope set by its party, may do nothing.
    let agent = Actor {
        party: kit::customer().party,
        acts: id("sso-agent"),
        role: Role::Customer,
        kind: ActorKind::Agent,
    };
    core.registers().list(&agent);
    let charge = Charge {
        request: id("agent-charge"),
        party: agent.party.clone(),
        caller: agent.clone(),
        work: Work::Request,
        count: Count::new(1).expect("a count"),
        order: None,
        at: core.clock.timestamp(),
    };
    assert!(
        matches!(
            core.policy().charge(&charge).expect("answered").outcome,
            ChargeOutcome::Refused { .. }
        ),
        "an authenticated agent with no scope is refused its work"
    );

    // The listed, permitted login is refused when the deployment's own policy refuses: authentication and the
    // register together permit nothing the deployment's policy refuses.
    core.refuse(Some(RefusalClass::NotPermitted));
    let again = decide(&mut core, &operator, "promote-under-a-refusing-policy");
    assert_eq!(
        again.outcome,
        Answer::Refuse {
            class: RefusalClass::NotPermitted
        }
    );
    assert!(again.permit().is_none());

    // Core's calls hold the caller and no credential: every decision recorded names the caller as given.
    let callers: Vec<String> = core
        .log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(VerifiedEntry {
                what: What::Decide { request, .. },
                ..
            }) => Some(request.caller.acts.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert!(
        callers.contains(&"sso-administrator".to_owned()),
        "{callers:?}"
    );
    assert!(
        !core
            .log()
            .entries()
            .iter()
            .any(|(_, e)| serde_json::to_string(e).unwrap().contains("token-")),
        "no credential reached the log"
    );
}

// --- AC-18: a thousand agents under one accountable party share one limit ---------------------------------------------

/// A thousand agents of one party, each listed and scoped by it, each charged far below any bound of its own: the
/// bound is the party's, so the thousand together reach it, and the next charge, by any of them or by a new one, is
/// over it and does no work. The same for questions about people. A second party's agent isn't held by it.
// verifies: AC-18, REQ-AUZ-8, REQ-OPS-1
#[test]
fn thousand_agents_under_one_party_share_one_limit() {
    use rutent_core_authz::parties::{PEOPLE_BOUND, WORK_BOUND};
    let core = bare("a thousand agents", "AC-18");
    let party = kit::customer().party;
    const AGENTS: u64 = 1_000;
    // What each agent charges, so that the thousand together are exactly the bound, and each alone a thousandth of it.
    let each_work = WORK_BOUND / AGENTS;
    let each_person = PEOPLE_BOUND / AGENTS;
    let agent = |n: u64, party: &rutent_contracts::router_api::Id| Actor {
        party: party.clone(),
        acts: id(&format!("agent-{n:04}")),
        role: Role::Customer,
        kind: ActorKind::Agent,
    };
    {
        let mut registers = core.registers();
        for n in 0..=AGENTS {
            let a = agent(n, &party);
            registers.list(&a);
            for scope in [
                Scope::Charge(Work::Request),
                Scope::Charge(Work::PersonQuestion),
            ] {
                registers.scope(&a, scope);
            }
        }
    }
    let charge = |core: &Core, request: String, caller: &Actor, work: Work, count: u64| Charge {
        request: id(&request),
        party: caller.party.clone(),
        caller: caller.clone(),
        work,
        count: Count::new(count).expect("a count"),
        order: None,
        at: core.clock.timestamp(),
    };

    for (work, each, bound) in [
        (Work::Request, each_work, WORK_BOUND),
        (Work::PersonQuestion, each_person, PEOPLE_BOUND),
    ] {
        // Each agent's own charge is within what it alone could do, and within the party's.
        let mut left = bound;
        for n in 0..AGENTS {
            let a = agent(n, &party);
            let c = charge(&core, format!("{work:?}-{n}"), &a, work, each);
            let answer = core.policy().charge(&c).expect("answered");
            left -= each;
            assert_eq!(
                answer.outcome,
                ChargeOutcome::Within {
                    remaining: left.try_into().expect("a safe integer")
                },
                "{work:?}: agent {n} within the party's bound"
            );
        }
        assert_eq!(left, 0, "the thousand used the party's whole bound");
        // The party's bound is reached: the first agent again, which has used a thousandth of any bound of its own,
        // and one that never charged, are both over it, and the work isn't done.
        for (n, name) in [
            (0, "an agent that charged"),
            (AGENTS, "an agent that never did"),
        ] {
            let a = agent(n, &party);
            let c = charge(&core, format!("{work:?}-over-{n}"), &a, work, 1);
            let answer = core.policy().charge(&c).expect("answered");
            assert!(
                matches!(answer.outcome, ChargeOutcome::Over { .. }),
                "{work:?}: {name} is bound by the party's limit: {:?}",
                answer.outcome
            );
            assert!(
                matches!(last(&core).outcome, Outcome::Refused { .. }),
                "the refusal is on the record"
            );
        }
    }

    // Another accountable party's agent isn't bound by this party's count.
    let other_party = id("other-customer");
    let outsider = agent(7, &other_party);
    core.registers().list(&outsider);
    core.registers()
        .scope(&outsider, Scope::Charge(Work::Request));
    let c = charge(&core, "outsider".to_owned(), &outsider, Work::Request, 1);
    assert!(
        matches!(
            core.policy().charge(&c).expect("answered").outcome,
            ChargeOutcome::Within { .. }
        ),
        "a second party's agent has its own limit"
    );
}

// --- AC-17: a verifier out of reach is no permit, and nothing moves --------------------------------------------------

/// An order the customer requests, has quoted and accepts, in a Core whose deployment asks the verifier stand-in for
/// its judgement of the payer before any value moves, the verifier doing `verifier`; and what running it did.
struct Moved {
    core: Core,
    order: Id,
    /// What the reserve's books hold: what moved.
    books: usize,
    /// The verified answers to a decision to move value, in order.
    decisions: Vec<Outcome>,
    /// How many effects that move value the log holds.
    effects: usize,
}

fn order_under_judgement(verifier: Verifier) -> Moved {
    let mut core = bare("a move waits on a verifier's judgement", "AC-17");
    core.deployment.judge_before_moving(true);
    core.verifier_is(verifier);
    let customer = kit::customer();
    let session = core
        .open_session(&customer)
        .expect("the customer's session");
    let told = core
        .request(base_request("req-judged", AMOUNT, &session))
        .expect("the request opens an order");
    let order = Core::order_of(&told).expect("an order");
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = core.quote(&customer, &order).expect("quoted")
    else {
        panic!("a quote is shown");
    };
    core.accept(&customer, &order, &digest).expect("accepted");
    let ran = core.run(&order);
    // Whether Core ran on, or stopped on the answer it was given, nothing may have moved without a permit: what is
    // checked is the record and the reserve's books.
    let _ = ran;
    let books = core.world().journal(RESERVE).len();
    let mut decisions = Vec::new();
    let mut effects = 0;
    for (_, entry) in core.log().entries() {
        let Entry::Verified(v) = entry else { continue };
        match &v.what {
            What::Decide {
                request:
                    DecisionRequest {
                        action: Action::MoveValue { .. },
                        ..
                    },
                ..
            } => decisions.push(v.outcome),
            What::Effect {
                action: Action::MoveValue { .. },
                ..
            } => effects += 1,
            _ => {}
        }
    }
    Moved {
        core,
        order,
        books,
        decisions,
        effects,
    }
}

/// A move of value that needs a verifier's judgement of the payer, the verifier out of reach: the decision is not a
/// permit, so no effect is recorded, the reserve moves nothing and the order doesn't complete. Reached and silent,
/// reached and saying no, are refusals, and nothing moves on them either; reached and saying yes, the same order
/// moves, so the stop above is the verifier's and nothing else's. The engine answers a verifier it can't reach "not
/// known" with its own cause and not "refused" (`REQ-AUZ-5`, core-internal's README), as `AC-17` says since the owner
/// settled `spec-ac17-not-known` on 2026-11-18 (`SPEC.md` 0.4.2).
// verifies: AC-17, REQ-AUZ-5
#[test]
fn unreachable_verifier_gives_not_known_and_nothing_moves() {
    let mut out_of_reach = order_under_judgement(Verifier::Unreachable);
    assert!(
        !out_of_reach.decisions.is_empty(),
        "Core asked policy to move the value"
    );
    for answer in &out_of_reach.decisions {
        assert_eq!(
            *answer,
            Outcome::NotKnown {
                cause: NotKnownCause::VerifierUnreachable
            },
            "the record says why nothing may move"
        );
    }
    assert_eq!(out_of_reach.effects, 0, "no move took effect");
    assert_eq!(out_of_reach.books, 0, "the reserve moved nothing");
    let state = out_of_reach
        .core
        .status(&kit::customer(), &out_of_reach.order)
        .expect("the customer reads its order")
        .state;
    assert!(
        !matches!(state, OrderState::Complete),
        "an order whose move wasn't permitted doesn't complete: {state:?}"
    );

    // A verifier that is reached but holds no judgement, or says no: a refusal on the record, and nothing moves.
    for (verifier, class) in [
        (Verifier::Silent, RefusalClass::MissingInput),
        (Verifier::No, RefusalClass::NotPermitted),
    ] {
        let refused = order_under_judgement(verifier);
        assert!(!refused.decisions.is_empty());
        assert!(
            refused
                .decisions
                .iter()
                .all(|o| *o == Outcome::Refused { class }),
            "{verifier:?}: {:?}",
            refused.decisions
        );
        assert_eq!((refused.effects, refused.books), (0, 0), "{verifier:?}");
    }

    // The control: the verifier reached and satisfied, the same order moves its value and completes.
    let mut judged = order_under_judgement(Verifier::Yes);
    assert!(judged.effects > 0 && judged.books > 0, "the control moved");
    let state = judged
        .core
        .status(&kit::customer(), &judged.order)
        .expect("the customer reads its order")
        .state;
    assert_eq!(state, OrderState::Complete);
}

// --- An agent acting for the operator is held to its party's scope and to the register ---------------------------------

/// An agent of the operator's party asking an erasure is held, as every agent is, to the scope its party set and to
/// the register that lists its login (`REQ-AUZ-8`, `DEC-18`, the review of `T-021-10`): refused when the party never
/// gave it the erase scope, however the register lists it, and refused when its login is unlisted, whatever scope
/// and permission the register holds for its name. With both, listed and scoped, it is permitted, so each refusal
/// is that one thing and not the action. A refusal is on the record, and nothing is carried out.
// verifies: REQ-AUZ-8, DEC-18
#[test]
fn operator_agent_is_refused_an_erasure_without_the_erase_scope_or_when_its_login_is_unlisted() {
    use rutent_contracts::core_internal::Token;
    use rutent_core_authz::roles::Permission;

    let core = bare("an operator's agent asks an erasure", "AC-25");
    let agent = |name: &str| Actor {
        party: kit::operator().party,
        acts: id(name),
        role: Role::Operator,
        kind: ActorKind::Agent,
    };
    let (unscoped, unlisted, whole) = (
        agent("ops-agent-unscoped"),
        agent("ops-agent-unlisted"),
        agent("ops-agent-whole"),
    );
    {
        let mut registers = core.registers();
        // Listed and holding the permission, but its party gave it no erase scope (it may only post routes).
        registers.list(&unscoped);
        registers.permit(&unscoped, Permission::EraseStatement);
        registers.scope(&unscoped, Scope::PostRoute);
        // Scoped and holding the permission by name, but its login isn't listed as acting for anyone.
        registers.permit(&unlisted, Permission::EraseStatement);
        registers.scope(&unlisted, Scope::EraseStatement);
        // Listed, permitted and scoped.
        registers.list(&whole);
        registers.permit(&whole, Permission::EraseStatement);
        registers.scope(&whole, Scope::EraseStatement);
    }
    let erase = |core: &Core, caller: &Actor, n: usize| DecisionRequest {
        request: id(&format!("erase-agent-{n}")),
        caller: caller.clone(),
        action: Action::EraseStatement {
            commitment: serde_json::from_value(serde_json::json!({
                "algorithm": "sha-256",
                "value": format!("{:064x}", 99),
            }))
            .expect("a digest"),
            reason: Token::new("retention").expect("a token"),
            overrides_citations: false,
        },
        at: core.clock.timestamp(),
    };

    for (n, (caller, name)) in [
        (&unscoped, "an agent its party gave no erase scope"),
        (&unlisted, "an agent whose login is unlisted"),
    ]
    .into_iter()
    .enumerate()
    {
        let decision = core
            .policy()
            .decide(&erase(&core, caller, n))
            .expect("policy answers, on the record");
        assert!(
            matches!(
                decision.outcome,
                Answer::Refuse { .. } | Answer::NotKnown { .. }
            ),
            "{name} is refused: {:?}",
            decision.outcome
        );
        assert!(decision.permit().is_none(), "{name}: no permit");
        assert!(
            matches!(
                last(&core).outcome,
                Outcome::Refused { .. } | Outcome::NotKnown { .. }
            ),
            "{name}: the refusal is on the record"
        );
    }
    let decision = core
        .policy()
        .decide(&erase(&core, &whole, 9))
        .expect("policy answers");
    assert!(
        matches!(decision.outcome, Answer::Permit {}),
        "the control, listed and scoped: {:?}",
        decision.outcome
    );
}
