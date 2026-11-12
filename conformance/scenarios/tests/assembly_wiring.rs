//! The assembly's wiring, each piece held by a test that fails without it (`T-024-02`): one clock, policy's sources
//! answered from the records, the evidence store bound to Core's own log, staging set explicitly, and the rounds'
//! transactions kept across routings, and the grading through `CMP-EVIDENCE` (`T-020-09`).

use rutent_contracts::core_internal::{Entry, What};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::Id;
use rutent_core_audit::receipts::store::EVIDENCE_RESTED_ON;
use rutent_core_authz::engine::{Need, Reply, Said, Source, Sources};
use rutent_core_authz::roles::{AUTOMATIC_DEPOSIT, NO_USER_WITHDRAWAL, Permission};
use rutent_prov::module::lifecycle::Staging;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged, context};
use rutent_scenarios::harness::{Given, Scenario, Step};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};

fn one_order(given: Given) -> (Core, Id) {
    let (core, orders) = Scenario::new("one order, run", "AC-28", given)
        .when(Step::Order {
            request: "req-1".to_owned(),
            amount: AMOUNT.to_owned(),
        })
        .when(Step::Run)
        .core(1, &mut Reference);
    (core, orders[0].clone())
}

fn need(input: &str, from: Source) -> Need {
    Need {
        input: id(input),
        from,
    }
}

fn yes() -> Reply<Said> {
    Reply::Given(Said::Yes)
}

fn no() -> Reply<Said> {
    Reply::Given(Said::No)
}

/// The commitment of the stated entry at `receipt`'s index.
fn commitment_at(core: &Core, index: usize) -> Digest {
    match &core.log().entries()[index].1 {
        Entry::Stated(s) => s.commitment.clone(),
        Entry::Verified(_) => panic!("a stated entry"),
    }
}

// verifies: DEC-18
#[test]
fn policy_the_log_core_and_the_world_read_one_clock() {
    let (mut core, _) = Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    // Moved days on, and still inside the simulated providers' validity: a part reading its host's clock, not
    // Core's, would be days adrift and refuse what this asks.
    core.clock.advance(3 * 86_400 + 7_777);
    let later = core.clock.peek();
    // Policy bounds a session from Core's clock: the session asked for now opens only on it.
    let session = core
        .open_session(&kit::customer())
        .expect("policy opens the session on Core's clock");
    assert_eq!(session.as_str(), context().session);
    // The log holds a catalogue change to its own clock's skew: it takes one dated now.
    let installed = core.installed[0].clone();
    core.grant(&installed)
        .expect("the log takes a grant dated by Core's clock");
    // Core's snapshots and the simulated world: an order quoted and run now is stamped now.
    let customer = kit::customer();
    let told = core
        .request(rutent_scenarios::harness::base_request(
            "req-later",
            AMOUNT,
            &session,
        ))
        .unwrap();
    let order = Core::order_of(&told).unwrap();
    let told = core.quote(&customer, &order).unwrap();
    let rutent_core_orch::intake::Told::Exists {
        status: rutent_core_orch::intake::Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("{told:?}")
    };
    core.accept(&customer, &order, &digest).unwrap();
    core.run(&order).unwrap();
    let hour = |at: &str| at[..13].to_owned();
    let expected = hour(
        rutent_prov::catalogue::Instant(u64::try_from(later / 1_000_000_000).unwrap())
            .rfc3339()
            .unwrap()
            .as_str(),
    );
    let log = core.log();
    let results: Vec<String> = log
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
                    && s.about.as_ref().is_some_and(|a| a.segment.is_some()) =>
            {
                Some(s.when.at.as_str().to_owned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        results.iter().all(|at| hour(at) == expected),
        "the world stamps its results by Core's clock: {results:?}, {expected}"
    );
}

// verifies: DEC-18
#[test]
fn policys_sources_answer_from_the_records() {
    let (core, order) = one_order(Given::one_reserve(Staged::AtOnce));
    let mut sources = core.sources();
    let customer = kit::customer();
    let party = customer.party.as_str();
    // The deployment's registers.
    let acts_as = |acts: &str, party: &str, kind| Source::ActsFor {
        acts: id(acts),
        party: id(party),
        kind,
    };
    let acts = |acts: &str, party: &str| acts_as(acts, party, customer.kind);
    assert_eq!(sources.supply(&need("x", acts("user-1", party))), yes());
    assert_eq!(sources.supply(&need("x", acts("user-1", "operator"))), no());
    assert_eq!(
        sources.supply(&need("x", acts("someone", party))),
        Reply::Missing
    );
    // The register binds the kind too (`T-021-05`): the customer's login stating itself an agent is no one's.
    assert_eq!(
        sources.supply(&need(
            "x",
            acts_as(
                "user-1",
                party,
                rutent_contracts::core_internal::ActorKind::Agent
            )
        )),
        no()
    );
    // An agent's scope, as its party set it (`T-021-05`).
    let agent = rutent_contracts::core_internal::Actor {
        acts: id("agent-1"),
        kind: rutent_contracts::core_internal::ActorKind::Agent,
        ..customer.clone()
    };
    core.registers()
        .scope(&agent, rutent_core_authz::parties::Scope::Authorise);
    let scope = |scope| Source::AgentScope {
        party: customer.party.clone(),
        scope,
    };
    use rutent_core_authz::parties::Scope;
    assert_eq!(
        sources.supply(&need("agent-1", scope(Scope::Authorise))),
        yes()
    );
    assert_eq!(
        sources.supply(&need("agent-1", scope(Scope::MoveValue))),
        no()
    );
    assert_eq!(
        sources.supply(&need("agent-2", scope(Scope::Authorise))),
        Reply::Missing
    );
    let holds = |permission| Source::Permissions {
        party: id("operator"),
        permission,
    };
    assert_eq!(
        sources.supply(&need("rutent-core", holds(Permission::CarryOnward))),
        yes()
    );
    assert_eq!(
        sources.supply(&need("op-login-1", holds(Permission::CarryOnward))),
        no()
    );
    // The account's register.
    let account = Source::Account(id(&context().account));
    assert_eq!(sources.supply(&need(party, account.clone())), yes());
    assert_eq!(sources.supply(&need("someone", account)), no());
    assert_eq!(
        sources.supply(&need(party, Source::Account(id("no-such-account")))),
        Reply::Missing
    );
    // Intake's record of the order.
    assert_eq!(
        sources.supply(&need(party, Source::Requester(order.clone()))),
        yes()
    );
    assert_eq!(
        sources.supply(&need("someone", Source::Requester(order.clone()))),
        no()
    );
    assert_eq!(
        sources.supply(&need(RESERVE, Source::OrderOrigin(order.clone()))),
        yes()
    );
    assert_eq!(
        sources.supply(&need(party, Source::Requester(id("no-such-order")))),
        Reply::Missing
    );
    // The log: the order's acceptance, its plan, the effects on its segments and its statements.
    assert_eq!(
        sources.supply(&need(party, Source::OrderParties(order.clone()))),
        yes()
    );
    assert_eq!(
        sources.supply(&need("someone", Source::OrderParties(order.clone()))),
        no()
    );
    let whats = core.verified_whats();
    let segments: Vec<Id> = whats
        .iter()
        .filter_map(|w| match w {
            What::Effect {
                action: rutent_contracts::core_internal::Action::MoveValue { segment, .. },
                ..
            } => Some(segment.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(segments.len(), 2);
    for segment in &segments {
        let order_of = Source::SegmentOrder(segment.clone());
        assert_eq!(
            sources.supply(&need(order.as_str(), order_of.clone())),
            yes()
        );
        assert_eq!(sources.supply(&need("another-order", order_of)), no());
        let moved = Source::SegmentProvider {
            order: order.clone(),
            segment: segment.clone(),
        };
        assert_eq!(sources.supply(&need(RESERVE, moved.clone())), yes());
        assert_eq!(sources.supply(&need("sim-reserve-b", moved)), no());
    }
    let result = core
        .log()
        .entries()
        .iter()
        .position(|(_, e)| matches!(e, Entry::Stated(s) if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE))
        .expect("a result in the log");
    let statement = Source::Statement(commitment_at(&core, result));
    let publisher = format!("{RESERVE}-publisher");
    assert_eq!(sources.supply(&need(&publisher, statement.clone())), yes());
    assert_eq!(sources.supply(&need(party, statement)), no());
    // The evidence store's statements, by the commitment the log names each by, about their order (`T-021-06`).
    let kept = Source::Kept(commitment_at(&core, result));
    assert_eq!(sources.supply(&need(order.as_str(), kept.clone())), yes());
    assert_eq!(sources.supply(&need("another-order", kept)), no());
    let unkept: Digest = serde_json::from_value(
        serde_json::json!({"algorithm": "sha-256", "value": "ab".repeat(32)}),
    )
    .unwrap();
    assert_eq!(
        sources.supply(&need(order.as_str(), Source::Kept(unkept))),
        no()
    );
    // The stated entry's own order, as the decision's evidence about the order itself must be.
    let about = Source::KeptAbout(commitment_at(&core, result));
    assert_eq!(sources.supply(&need(order.as_str(), about.clone())), yes());
    assert_eq!(sources.supply(&need("another-order", about)), no());
    // A statement about no order in particular is kept, but not about the order itself.
    let unbound = core
        .log()
        .entries()
        .iter()
        .position(|(_, e)| {
            matches!(e, Entry::Stated(s) if s.about.as_ref().and_then(|a| a.order.as_ref()).is_none())
        })
        .expect("a statement about no order");
    let unbound = commitment_at(&core, unbound);
    assert_eq!(
        sources.supply(&need(order.as_str(), Source::Kept(unbound.clone()))),
        yes()
    );
    assert_eq!(
        sources.supply(&need(order.as_str(), Source::KeptAbout(unbound))),
        no()
    );
    // The register's `ActsFor` chains (`T-021-06`): two desks acting for the operator, one's person, and the
    // operator's own. A chain reaches what lies up it; two desks' chains meet at the operator and reach neither desk.
    let listed = |acts: &str, party: &str| rutent_contracts::core_internal::Actor {
        acts: id(acts),
        party: id(party),
        ..customer.clone()
    };
    for (acts, party) in [
        ("desk-b", "operator"),
        ("desk-c", "operator"),
        ("bob", "desk-b"),
        ("carol", "desk-c"),
    ] {
        core.registers().list(&listed(acts, party));
    }
    let reaches = |from: &str, to: &str| Source::Reaches {
        from: id(from),
        to: id(to),
    };
    assert_eq!(
        sources.supply(&need("bob", reaches("bob", "desk-b"))),
        yes()
    );
    assert_eq!(
        sources.supply(&need("bob", reaches("bob", "operator"))),
        yes()
    );
    assert_eq!(sources.supply(&need("bob", reaches("bob", "desk-c"))), no());
    assert_eq!(
        sources.supply(&need("carol", reaches("carol", "desk-b"))),
        no()
    );
    assert_eq!(sources.supply(&need("bob", reaches("bob", party))), no());
    // The deployment's operator: its people along their chains, and no one else.
    assert_eq!(sources.supply(&need("bob", Source::OfOperator)), yes());
    assert_eq!(sources.supply(&need("user-1", Source::OfOperator)), no());
    // The reserve's declarations, from its advertisement as listed in the log.
    let declaration = core.installed[0]
        .advertisement
        .statement()
        .statement
        .offers
        .reserve
        .clone()
        .expect("a reserve's offer")
        .declaration;
    let said = |b: bool| if b { yes() } else { no() };
    assert_eq!(
        sources.supply(&need(NO_USER_WITHDRAWAL, Source::Declarations(id(RESERVE)))),
        said(!declaration.can_require_user_initiated_withdrawal)
    );
    assert_eq!(
        sources.supply(&need(AUTOMATIC_DEPOSIT, Source::Declarations(id(RESERVE)))),
        said(declaration.starts_routes_on_automatic_deposits)
    );
    assert_eq!(
        sources.supply(&need(
            AUTOMATIC_DEPOSIT,
            Source::Declarations(id("sim-reserve-b"))
        )),
        Reply::Missing
    );
    // No judgement is a source yet.
    assert_eq!(
        sources.supply(&need("kyc", Source::Verifier(id("sim-verifier")))),
        Reply::Missing
    );
}

// verifies: DEC-18
#[test]
fn evidence_store_is_bound_to_cores_own_log() {
    let (core, order) = one_order(Given::one_reserve(Staged::AtOnce));
    // The acceptance rests on the reserve's yes: the store bound to Core's log refuses to erase it.
    let yes = core
        .verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::Acceptance {
                order: o, yeses, ..
            } if o == order => Some(yeses[0].reserve_yes.clone()),
            _ => None,
        })
        .expect("the acceptance names its yes");
    let index = usize::try_from(u64::from(yes.index)).unwrap();
    let commitment = commitment_at(&core, index);
    let refused = core.bound().erasable(&commitment).unwrap_err();
    assert_eq!(
        refused,
        rutent_contracts::core_internal::calls::AppendFailure::Refused(
            rutent_contracts::core_internal::Violation(EVIDENCE_RESTED_ON)
        )
    );
    assert!(core.bound().missing().is_empty());
}

// verifies: DEC-18
#[test]
fn staging_is_the_one_the_scenario_sets() {
    let staging = Staging::new(10, 86_400, 100).expect("a staging policy");
    for staged in [Staged::AtOnce, Staged::Under(staging)] {
        let core = Core::assemble(
            &rutent_scenarios::assembly::Config {
                seed: 1,
                staging: staged,
                deadline: std::time::Duration::from_secs(2),
                providers: Given::one_reserve(staged).providers,
                delivery: rutent_scenarios::assembly::machine::Delivery::Once,
            },
            &mut Reference,
        )
        .expect("assembled");
        let expected = match staged {
            Staged::AtOnce => None,
            Staged::Under(s) => Some(s),
        };
        assert_eq!(core.fleet().platform.lifecycle().staging(), expected);
    }
}

// verifies: DEC-18
#[test]
fn rounds_transactions_are_kept_across_routings_and_no_segment_runs_under_one() {
    let (core, _) = Scenario::new(
        "two orders, run",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .when(Step::Order {
        request: "req-2".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .when(Step::Run)
    .core(1, &mut Reference);
    let operations: Vec<(String, bool)> = core
        .log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Stated(s)
                if s.what.statement.as_str()
                    == rutent_contracts::provider_role::OPERATION_SHAPE =>
            {
                let about = s.about.as_ref()?;
                Some((
                    about.transaction.as_ref()?.as_str().to_owned(),
                    about.segment.is_some(),
                ))
            }
            _ => None,
        })
        .collect();
    let questions: Vec<&String> = operations
        .iter()
        .filter(|(_, s)| !s)
        .map(|(t, _)| t)
        .collect();
    let segments: Vec<&String> = operations
        .iter()
        .filter(|(_, s)| *s)
        .map(|(t, _)| t)
        .collect();
    // Both orders' routings asked, and the segments of both ran.
    assert!(questions.len() >= 4, "{questions:?}");
    assert_eq!(segments.len(), 4, "{segments:?}");
    for t in &questions {
        assert!(core.issued().contains(t), "the rounds' {t} is kept");
    }
    for t in &segments {
        assert!(
            !core.issued().contains(t),
            "a segment ran under the rounds' {t}"
        );
    }
}

// verifies: DEC-18
#[test]
fn catalogues_grant_is_the_one_the_log_records() {
    use rutent_contracts::core_internal::{Action, CatalogueChange};
    let (core, _) = Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    let log = core.log();
    let entries: Vec<_> = log.entries().iter().map(|(_, e)| e.clone()).collect();
    drop(log);
    // Each grant the log records: its tuple, its keys, who decided it and when it took effect.
    let mut tuples = Vec::new();
    let mut keys = Vec::new();
    let mut deciders = Vec::new();
    let mut times = Vec::new();
    for entry in &entries {
        let Entry::Verified(v) = entry else { continue };
        if let What::Effect {
            action:
                Action::ChangeCatalogue {
                    change: CatalogueChange::Grant,
                    provider,
                    tuple,
                    advertising_keys,
                    ..
                },
            decision,
        } = &v.what
        {
            assert_eq!(provider.as_str(), RESERVE);
            tuples.push(tuple.clone().expect("a grant names its tuple"));
            keys = advertising_keys.clone();
            times.push(rutent_core_authz::session::instant(&v.when.at));
            let decider = entries.iter().find_map(|e| match e {
                Entry::Verified(d) => match &d.what {
                    What::Decide {
                        request,
                        decision: id,
                    } if id == decision.decision() => Some(request.caller.acts.clone()),
                    _ => None,
                },
                Entry::Stated(_) => None,
            });
            deciders.push(decider.expect("the effect's decision is in the log"));
        }
    }
    // One grant in Core's catalogue per tuple the log granted, each on that effect's terms: its tuple, its
    // decider, its time, its key.
    let grants = core.catalogue().grants_of_provider(RESERVE);
    assert_eq!(
        grants.len(),
        tuples.len(),
        "one catalogue grant per recorded grant"
    );
    for ((tuple, decider), at) in tuples.iter().zip(&deciders).zip(&times) {
        let grant = grants
            .iter()
            .find(|g| {
                g.terms()
                    .tuples
                    .iter()
                    .map(|t| kit::tuple_id(RESERVE, t))
                    .collect::<Vec<_>>()
                    == vec![tuple.clone()]
            })
            .unwrap_or_else(|| panic!("the catalogue grants {tuple} alone"));
        let terms = grant.terms();
        assert_eq!(terms.granted_by, decider.as_str());
        assert_eq!(&terms.granted_at, at, "granted at its own effect's time");
        assert_eq!(grant.advertising_keys(), keys.as_slice());
    }
}

// verifies: DEC-18
#[test]
fn reserve_a_settlement_account_is_at_is_not_one_of_the_orders_parties() {
    use rutent_core_orch::intake::{Status, Told};
    let (mut core, _) = Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    let customer = kit::customer();
    let session = core.open_session(&customer).unwrap();
    let mut request = rutent_scenarios::harness::base_request("req-settled", AMOUNT, &session);
    request.parties.settlement_account_at = Some(id(RESERVE));
    let order = Core::order_of(&core.request(request).unwrap()).unwrap();
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = core.quote(&customer, &order).unwrap()
    else {
        panic!("no quote")
    };
    core.accept(&customer, &order, &digest).unwrap();
    let mut sources = core.sources();
    assert_eq!(
        sources.supply(&need(
            customer.party.as_str(),
            Source::OrderParties(order.clone())
        )),
        yes()
    );
    assert_eq!(
        sources.supply(&need(RESERVE, Source::OrderParties(order))),
        no()
    );
}

// verifies: DEC-18
#[test]
fn silent_provider_records_its_deposit_failed_and_moves_nothing_for_it() {
    use rutent_contracts::core_internal::{Action, Outcome};
    use rutent_scenarios::harness::Fault;
    let run = Scenario::new(
        "a silent deposit",
        "AC-28",
        Given::one_reserve(Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .when(Step::Plant(Fault::Silence {
        provider: RESERVE.to_owned(),
        kind: "deposit".to_owned(),
    }))
    .when(Step::Run)
    .play(1, &mut Reference, rutent_scenarios::harness::reference());
    // Two effects, the withdrawal's and the deposit's, and one movement: the withdrawal's.
    let effects: Vec<usize> = run
        .log
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            matches!(e, Entry::Verified(v) if matches!(v.what, What::Effect { action: Action::MoveValue { .. }, .. }))
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(effects.len(), 2);
    assert_eq!(run.movements().values().sum::<usize>(), 1);
    // The platform refused the deposit before its module passed anything out, so its effect failed: its result is
    // `failed`, not `unanswered`, and the order stays past its first segment, not complete.
    let deposit = effects[1];
    let failed = run.log.iter().any(|e| {
        matches!(e, Entry::Verified(v) if matches!(&v.what, What::Result { effect } if u64::from(effect.index) == deposit as u64)
            && v.outcome == Outcome::Failed {})
    });
    assert!(failed, "the deposit's result is failed");
    assert_eq!(
        run.state_names().values().cloned().collect::<Vec<_>>(),
        vec![Some("first_segment_settled".to_owned())]
    );
}

// verifies: DEC-18
#[test]
fn a_tuples_id_is_the_digest_of_its_canonical_json() {
    let (core, _) = Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    let ad = core.installed[0]
        .advertisement
        .statement()
        .statement
        .clone();
    for tuple in rutent_prov::catalogue::tuples_of(&ad) {
        // As an outside checker recomputes it: provider-role's own names for each part.
        let node = |end: &rutent_prov::catalogue::Endpoint| match end {
            rutent_prov::catalogue::Endpoint::Inner(n) => serde_json::json!(n),
            rutent_prov::catalogue::Endpoint::Outer(n) => serde_json::json!({ "network": n }),
        };
        let mut value = serde_json::json!({
            "provider": RESERVE,
            "category": tuple.category,
            "origin": node(&tuple.origin),
            "destination": node(&tuple.destination),
            "asset": tuple.asset,
        });
        if let Some(to) = &tuple.to_asset {
            value["to_asset"] = to.clone().into();
        }
        let canonical = rutent_contracts::provider_role::to_canonical(&value).unwrap();
        let digest = rutent_prov::module::wire::sha256(&canonical);
        assert_eq!(
            kit::tuple_id(RESERVE, &tuple).as_str(),
            format!("tuple-{}", &digest[..32])
        );
    }
}

/// The assembled Core grades through `CMP-EVIDENCE` (`T-020-09`): the order's final deposit closes on a public record
/// of its movement on the reserve's bank network, the record naming the movement, and on the reserve's own statement,
/// and the order completes; a statement alone would close nothing.
// verifies: REQ-ORD-33, REQ-ORD-39
#[test]
fn assembled_order_closes_on_the_public_record_and_the_reserves_statement() {
    use rutent_contracts::provider_role::EvidenceGrade;
    let (core, order) = one_order(Given::one_reserve(Staged::AtOnce));
    let evidence: Vec<(EvidenceGrade, String, bool)> = core
        .log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Evidence {
                    order: o,
                    grade,
                    whose,
                    movement,
                    ..
                } if o == &order => Some((*grade, whose.as_str().to_owned(), movement.is_some())),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect();
    let bank = format!("{RESERVE}-bank");
    assert!(
        evidence.contains(&(EvidenceGrade::PublicRecord, bank, true)),
        "the deposit's public record, naming its movement: {evidence:?}"
    );
    assert!(
        evidence.contains(&(EvidenceGrade::SignedByProvider, RESERVE.to_owned(), false)),
        "the reserve's statement: {evidence:?}"
    );
    let settled = core
        .log()
        .entries()
        .iter()
        .filter(|(_, e)| {
            matches!(e, Entry::Verified(v) if matches!(&v.what, What::Input { input }
                if input.subject.order.as_str() == order.as_str()
                    && matches!(input.observed, rutent_contracts::state_machine::Observed::SegmentSettled { .. })))
        })
        .count();
    assert_eq!(settled, 2, "the withdrawal and the deposit settled");
}
