//! The assembly's wiring, each piece held by a test that fails without it (`T-024-02`): one clock, policy's sources
//! answered from the records, the evidence store bound to Core's own log, staging set explicitly, and the rounds'
//! transactions kept across routings.

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

// verifies: DEC-2, DEC-18
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

// verifies: DEC-2, DEC-18
#[test]
fn policys_sources_answer_from_the_records() {
    let (core, order) = one_order(Given::one_reserve(Staged::AtOnce));
    let mut sources = core.sources();
    let customer = kit::customer();
    let party = customer.party.as_str();
    // The deployment's registers.
    let acts = |acts: &str, party: &str| Source::ActsFor {
        acts: id(acts),
        party: id(party),
    };
    assert_eq!(sources.supply(&need("x", acts("user-1", party))), yes());
    assert_eq!(sources.supply(&need("x", acts("user-1", "operator"))), no());
    assert_eq!(
        sources.supply(&need("x", acts("someone", party))),
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

// verifies: DEC-2, INV-7
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
        assert_eq!(core.fleet.platform.lifecycle().staging(), expected);
    }
}

// verifies: DEC-2, DEC-18
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
    let grants = core.catalogue.grants_of_provider(RESERVE);
    assert_eq!(grants.len(), 1, "one grant in the catalogue");
    let terms = grants[0].terms();
    let mut granted: Vec<String> = terms
        .tuples
        .iter()
        .map(|t| kit::tuple_id(RESERVE, t).as_str().to_owned())
        .collect();
    let mut tuples: Vec<String> = tuples.iter().map(|t| t.as_str().to_owned()).collect();
    granted.sort();
    tuples.sort();
    assert_eq!(
        granted, tuples,
        "the catalogue grants the tuples the log records"
    );
    assert!(deciders.iter().all(|d| d.as_str() == terms.granted_by));
    assert_eq!(Some(&terms.granted_at), times.iter().min());
    assert_eq!(grants[0].advertising_keys(), keys.as_slice());
}
