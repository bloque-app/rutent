//! Arrivals and automatic deposits (`T-020-04`): value arriving with no order opens none; an arrival that differs
//! from an open order's amount settles nothing by itself, its reserve's report recorded, and only the operator or
//! that reserve files it anew; a reserve that declared it starts a route on an automatic deposit, recorded as
//! acceptance apart from an explicit one; and each question about an arrival is Core's, asked under a fresh
//! transaction for its order and appended before it is sent, so the reserve's answer is bound to that order. Every
//! log a test writes is replayed as the log itself would admit it.

mod intake_support;
mod rounds_support;
mod snapshot_support;

use std::collections::BTreeMap;

use intake_support::START;
use intake_support::*;
use proptest::prelude::*;
use rounds_support::{Sim, customer, operator};
use rutent_contracts::core_internal::{
    AcceptanceBasis, Actor, ActorKind, ArrivalMatch, Entry, OrderRefusalClass, Parties, Receipt,
    Role, StatedEntry, What,
};
use rutent_contracts::provider_role::{Denomination, OuterNode, RESULT_SHAPE};
use rutent_contracts::router_api::{Id, PositiveAmount};
use rutent_contracts::state_machine::{Observed, SourceKind};
use rutent_core_orch::arrival::{
    ArrivalError, Arrivals, Arrived, Filed, Filing, OpenOrder, OpenOrders, Report,
};
use rutent_core_orch::intake::Told;
use rutent_core_orch::rounds::Providers;
use rutent_core_orch::snapshot::Clock;
use rutent_prov::catalogue::Instant;
use rutent_router::RouteFinder;
use serde_json::{Value, json};
use snapshot_support::{RESERVE, deployment, envelope, id, sign, statements_key};

// The reserve, its notices and its answers -------------------------------------------------------------------------

/// The account the user's session speaks for, at [`RESERVE`].
const ACCOUNT: &str = "acct-1";

fn usdc() -> Denomination {
    Denomination {
        id: "USDC".into(),
        decimals: 6,
    }
}

fn polygon() -> OuterNode {
    OuterNode {
        network: "polygon".into(),
    }
}

fn amount(units: u64) -> PositiveAmount {
    PositiveAmount::new(units.to_string()).unwrap()
}

/// The reserve as it files and refiles, acting for itself: its party is its own id, as policy and the log hold it.
fn reserve_filer() -> Actor {
    Actor {
        party: id(RESERVE),
        acts: id(RESERVE),
        role: Role::Provider,
        kind: ActorKind::Component,
    }
}

/// The providers, and the reserve's answers to Core's balance verifications: sufficient unless told otherwise.
struct Reserve {
    sim: Sim,
    insufficient: bool,
    /// The balance verifications the reserve was asked, as received.
    verifications: Vec<Value>,
}

impl Reserve {
    fn new(env: &Env) -> Self {
        Self {
            sim: env.sim(),
            insufficient: false,
            verifications: Vec::new(),
        }
    }

    /// How many questions any provider was asked.
    fn asked(&self) -> usize {
        self.sim.received.len() + self.verifications.len()
    }
}

impl Providers for Reserve {
    fn ask(&mut self, provider: &Id, question: &[u8]) -> Option<Vec<u8>> {
        let value: Value = serde_json::from_slice(question).unwrap();
        let operation = value["statement"].clone();
        if operation["kind"] != "balance_verification" {
            return self.sim.ask(provider, question);
        }
        self.verifications.push(operation.clone());
        let now = self.sim.clock.now().seconds;
        let body = json!({
            "kind": "balance_verification",
            "transaction": operation["transaction"],
            "outcome": { "status": "answered", "sufficient": !self.insufficient },
        });
        let answer = envelope(&deployment(), RESULT_SHAPE, provider.as_str(), now, body);
        Some(sign(answer, &statements_key(provider.as_str(), 1)))
    }
}

/// The reserve's notice that `units` arrived at [`ACCOUNT`], signed, naming no transaction.
fn notice(units: u64, n: u32) -> Vec<u8> {
    let body = json!({
        "kind": "arrival",
        "node": "polygon",
        "asset": "USDC",
        "amount": units.to_string(),
        "account": ACCOUNT,
        "reference": format!("credit-{n}"),
    });
    let value = envelope(&deployment(), RESULT_SHAPE, RESERVE, START, body);
    sign(value, &statements_key(RESERVE, 1))
}

/// A report of `units` arriving at [`RESERVE`] to the customer's [`ACCOUNT`].
fn report(units: u64, n: u32) -> Report {
    Report {
        reserve: id(RESERVE),
        node: polygon(),
        asset: usdc(),
        amount: amount(units),
        account: id(ACCOUNT),
        holder: customer().party,
        notice: notice(units, n),
    }
}

/// The orders the assembly reads as open, as set by the test.
#[derive(Default)]
struct Open(Vec<OpenOrder>);

impl OpenOrders for Open {
    fn open_at(&mut self, _reserve: &Id, _account: &Id) -> Vec<OpenOrder> {
        self.0.clone()
    }
}

/// `order`, open at [`RESERVE`] for 1 000 USDC units from [`ACCOUNT`].
fn open_order(order: &Id) -> OpenOrder {
    OpenOrder {
        order: order.clone(),
        reserve: id(RESERVE),
        node: polygon(),
        asset: usdc(),
        account: id(ACCOUNT),
        amount: amount(1000),
        session: id(SESSION),
    }
}

/// An environment whose register lists the operator and the reserve as acting for themselves.
fn env() -> Env {
    let env = Env::new();
    env.actors.borrow_mut().list(&operator());
    env.actors.borrow_mut().list(&reserve_filer());
    env
}

fn arrive(
    env: &mut Env,
    providers: &mut Reserve,
    arrivals: &mut Arrivals,
    open: &mut Open,
    report: &Report,
) -> Result<Arrived, ArrivalError> {
    env.with_providers(providers, |_, desk| arrivals.arrive(desk, open, report))
}

fn refile(
    env: &mut Env,
    providers: &mut Reserve,
    arrivals: &mut Arrivals,
    filer: &Actor,
    arrival: &Receipt,
) -> Result<Receipt, ArrivalError> {
    env.with_providers(providers, |_, desk| arrivals.refile(desk, filer, arrival))
}

fn file(
    env: &mut Env,
    providers: &mut Reserve,
    arrivals: &mut Arrivals,
    filing: &Filing,
) -> Result<Filed, ArrivalError> {
    env.with_providers(providers, |_, desk| {
        arrivals.file::<RouteFinder>(desk, filing.clone())
    })
}

/// The filing of 1 000 USDC units on the arrival at `arrival`, by `filer`, to the customer's [`ADDRESS`].
fn filing(filer: Actor, arrival: &Receipt) -> Filing {
    Filing {
        request: id("deposit-1"),
        filer,
        arrival: arrival.clone(),
        parties: Parties {
            requester: customer().party,
            beneficiary: customer().party,
            settlement_account_at: None,
            payer: customer().party,
        },
        session: id(SESSION),
        destination: destination(),
        route: route("1000"),
        inputs: BTreeMap::new(),
        needs: Vec::new(),
    }
}

/// The log replays as the log itself would admit it.
fn assert_admitted(env: &Env) {
    if let Err((place, why)) = env.admitted() {
        panic!("the log refuses entry {place}: {why}");
    }
}

/// Every entry the log holds, in order.
fn entries(env: &Env) -> Vec<Entry> {
    env.book
        .borrow()
        .entries
        .iter()
        .map(|(_, e)| e.clone())
        .collect()
}

/// The entry at `receipt`.
fn at(env: &Env, receipt: &Receipt) -> Entry {
    entries(env)[receipt.index.get() as usize].clone()
}

fn stated(entry: &Entry) -> &StatedEntry {
    match entry {
        Entry::Stated(s) => s,
        Entry::Verified(v) => panic!("not a stated entry: {:?}", v.what),
    }
}

/// Every `arrival` entry: its order, how it matched, and its report's receipt.
fn arrivals_recorded(env: &Env) -> Vec<(Option<Id>, ArrivalMatch, Receipt)> {
    env.book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::Arrival {
                order,
                matched,
                report,
                ..
            } => Some((order.clone(), *matched, report.clone())),
            _ => None,
        })
        .collect()
}

/// Every verified `what` that names an order, or opens, refuses or starts one.
fn order_records(env: &Env) -> Vec<What> {
    env.book
        .borrow()
        .verified()
        .into_iter()
        .filter(|w| {
            matches!(
                w,
                What::Acceptance { .. }
                    | What::Input { .. }
                    | What::OrderRefusal { .. }
                    | What::DestinationCheck { .. }
                    | What::DecisionRecord { .. }
                    | What::Shown { .. }
                    | What::Authorise { .. }
            )
        })
        .cloned()
        .collect()
}

/// An order the customer accepted through intake, for 1 000 USDC units, started.
fn explicit_order(env: &mut Env, providers: &mut Reserve) -> Id {
    let (order, told) = env.accepted(
        &mut providers.sim,
        &order_request("client-req-1", customer()),
        &customer(),
    );
    assert!(matches!(told, Told::Exists { .. }), "{told:?}");
    order
}

/// Holds `arrival` (an `arrival` entry naming `order`) to Core's question about that order: its report is the
/// reserve's statement about the order under a transaction, and the operator's question under that transaction, about
/// that order, was appended before it.
fn assert_bound_to_its_question(env: &Env, order: &Id, report: &Receipt) {
    let all = entries(env);
    let answer = stated(&all[report.index.get() as usize]);
    let about = answer
        .about
        .as_ref()
        .expect("the report is about its order");
    assert_eq!(
        about.order.as_ref(),
        Some(order),
        "the report is about the order"
    );
    let transaction = about
        .transaction
        .as_ref()
        .expect("the report names the transaction Core asked under");
    let question = all[..report.index.get() as usize]
        .iter()
        .filter_map(|e| match e {
            Entry::Stated(s) => Some(s),
            _ => None,
        })
        .find(|s| {
            s.what.statement.as_str() == rutent_contracts::provider_role::OPERATION_SHAPE
                && s.about.as_ref().and_then(|a| a.transaction.as_ref()) == Some(transaction)
        })
        .expect("Core's question under the transaction came first");
    assert_eq!(
        question.about.as_ref().and_then(|a| a.order.as_ref()),
        Some(order),
        "the question was about the order"
    );
    let bytes = &env.store.borrow().kept;
    let body: Value = serde_json::from_slice(
        bytes
            .iter()
            .find(|b| {
                serde_json::from_slice::<Value>(b).is_ok_and(|v| {
                    v["shape"] == rutent_contracts::provider_role::OPERATION_SHAPE
                        && v["statement"]["transaction"] == transaction.as_str()
                })
            })
            .expect("the question's bytes are kept"),
    )
    .unwrap();
    assert_eq!(
        body["maker"],
        operator().acts.as_str(),
        "signed as the operator"
    );
}

// REQ-ORD-14 ---------------------------------------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Value arriving with no order open for it, any amount, any number of times, whatever orders are open elsewhere,
    /// opens no order: each is recorded as an `arrival` with no order, citing the reserve's notice, and nothing else
    /// is recorded or asked, so nothing counts against the operator.
    // verifies: REQ-ORD-14
    #[test]
    fn unattributed_arrival_creates_no_order(
        units in prop::collection::vec(1u64..5_000_000, 1..4),
        elsewhere in prop::sample::select(vec!["none", "reserve", "asset", "account", "node"]),
    ) {
        let mut env = env();
        let mut providers = Reserve::new(&env);
        let mut arrivals = Arrivals::new();
        // An order open for the same amount, but not where the value arrived.
        let mut open = Open::default();
        let mut other = open_order(&id("ord-elsewhere"));
        match elsewhere {
            "reserve" => other.reserve = id("another-reserve"),
            "asset" => other.asset = Denomination { id: "USDT".into(), decimals: 6 },
            "account" => other.account = id("acct-9"),
            "node" => other.node = OuterNode { network: "base".into() },
            _ => {}
        }
        if elsewhere != "none" {
            open.0.push(other);
        }
        let before = env.len();
        for (n, units) in units.iter().enumerate() {
            let mut report = report(*units, n as u32);
            if elsewhere != "none" {
                other_amount(&mut open, *units);
            }
            report.amount = amount(*units);
            let arrived = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &report).unwrap();
            prop_assert!(matches!(arrived, Arrived::NoOrder { .. }), "{arrived:?}");
        }
        let recorded = arrivals_recorded(&env);
        prop_assert_eq!(recorded.len(), units.len());
        for (order, matched, report) in &recorded {
            prop_assert_eq!(order, &None);
            prop_assert_eq!(*matched, ArrivalMatch::NoOrder);
            // The report is the reserve's notice, about nothing.
            let notice = at(&env, report);
            prop_assert!(stated(&notice).about.is_none());
        }
        // A notice and its arrival each, and nothing else: no order, no question to anyone.
        prop_assert_eq!(env.len() - before, 2 * units.len());
        prop_assert!(order_records(&env).is_empty(), "{:?}", order_records(&env));
        prop_assert_eq!(providers.asked(), 0);
        prop_assert_eq!(env.exposures.held().count(), 0);
        prop_assert!(env.exposures.stuck().is_empty());
        assert_admitted(&env);
    }
}

/// Sets the order open elsewhere to the amount that arrives, so only where it is keeps them apart.
fn other_amount(open: &mut Open, units: u64) {
    for o in &mut open.0 {
        o.amount = amount(units);
    }
}

/// The same notice again is the same arrival: told the same, and nothing recorded twice.
// verifies: REQ-ORD-14, INV-8
#[test]
fn same_notice_again_is_recorded_once() {
    let mut env = env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let mut open = Open::default();
    let report = report(1000, 0);
    let first = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &report).unwrap();
    let len = env.len();
    let again = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &report).unwrap();
    assert_eq!(first, again);
    assert_eq!(env.len(), len);
}

// REQ-ORD-23 ---------------------------------------------------------------------------------------------------------

/// Short, excess and split arrivals against an open order of 1 000 are each held against it as differing, on the
/// reserve's own answer about the order, and none of them settles its first segment or moves it at all; one of 1 000
/// matches, and settles nothing by itself either.
// verifies: REQ-ORD-23
#[test]
fn short_excess_and_split_arrivals_settle_no_first_segment_by_themselves() {
    let mut env = env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let inputs_before = env.started();
    assert_eq!(inputs_before, vec![order.as_str().to_owned()]);
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&order)]);
    let cases = [
        ("short", 600),
        ("excess", 1500),
        ("split", 400),
        ("split", 600),
    ];
    for (n, (_, units)) in cases.iter().enumerate() {
        let arrived = arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut open,
            &report(*units, n as u32),
        )
        .unwrap();
        let Arrived::Held {
            order: held,
            matched,
            ..
        } = arrived
        else {
            panic!("{arrived:?}");
        };
        assert_eq!(held, order);
        assert_eq!(matched, ArrivalMatch::Differs, "{:?}", cases[n]);
    }
    let matching = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut open,
        &report(1000, 9),
    )
    .unwrap();
    assert!(
        matches!(
            &matching,
            Arrived::Held {
                matched: ArrivalMatch::Matches,
                ..
            }
        ),
        "{matching:?}"
    );
    let recorded = arrivals_recorded(&env);
    assert_eq!(recorded.len(), 5);
    for (held, _, report) in &recorded {
        assert_eq!(held.as_ref(), Some(&order));
        assert_bound_to_its_question(&env, &order, report);
    }
    // Each arrival asked the reserve once, at the amount that arrived, in the order's session, for its account.
    let asked: Vec<String> = providers
        .verifications
        .iter()
        .map(|v| v["amount"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(asked, ["600", "1500", "400", "600", "1000"]);
    assert!(
        providers
            .verifications
            .iter()
            .all(|v| v["session"] == SESSION && v["account"] == ACCOUNT)
    );
    // Nothing moved: the only input is the order's start, and no effect or settlement was recorded.
    assert_eq!(env.started(), inputs_before);
    assert!(!env.book.borrow().verified().into_iter().any(|w| matches!(
        w,
        What::Effect { .. } | What::Result { .. } | What::Evidence { .. }
    )));
    assert_admitted(&env);
}

/// An arrival held against an order rests on the reserve's answer to Core's own question about that order, under a
/// fresh transaction appended before it was sent, never on the reserve's notice, which is about nothing; where the
/// reserve doesn't confirm it, only its notice is recorded.
// verifies: REQ-ORD-23, REQ-ORD-32, REQ-ORD-38
#[test]
fn arrival_held_against_an_order_rests_on_the_reserves_answer_to_cores_question() {
    let mut env = env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&order)]);
    let short = report(600, 0);
    let arrived = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &short).unwrap();
    let Arrived::Held { arrival, .. } = arrived else {
        panic!("{arrived:?}");
    };
    let (_, _, cited) = arrivals_recorded(&env).pop().unwrap();
    assert_bound_to_its_question(&env, &order, &cited);
    // The notice is recorded too, about nothing, and isn't what the arrival rests on.
    let notice = entries(&env)
        .into_iter()
        .position(|e| {
            matches!(&e, Entry::Stated(s) if s.about.is_none()
                && s.what.statement.as_str() == RESULT_SHAPE)
        })
        .expect("the notice is recorded");
    assert_ne!(cited.index.get() as usize, notice);
    assert!(cited.index.get() < arrival.index.get());
    // The transaction is fresh: one no question of the rounds asked under, naming nothing of the order.
    let transaction = stated(&at(&env, &cited))
        .about
        .as_ref()
        .and_then(|a| a.transaction.clone())
        .unwrap();
    assert!(!transaction.as_str().contains(order.as_str()));
    let rounds_transactions = providers
        .sim
        .received
        .iter()
        .filter_map(|(_, op)| op["transaction"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert!(!rounds_transactions.contains(&transaction.as_str().to_owned()));
    assert_admitted(&env);

    // A reserve that answers it doesn't hold that amount confirms nothing: the arrival isn't held against the order.
    providers.insufficient = true;
    let len = arrivals_recorded(&env).len();
    let unconfirmed = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut open,
        &report(700, 1),
    )
    .unwrap();
    assert!(
        matches!(unconfirmed, Arrived::Unconfirmed { .. }),
        "{unconfirmed:?}"
    );
    assert_eq!(arrivals_recorded(&env).len(), len);
    assert_admitted(&env);
}

/// Only the operator, or the reserve the value arrived at, files a differing arrival anew, against the order it was
/// held against; anyone else, an impostor of either among them, files nothing. An arrival that met no order, or
/// matched its order, isn't filed anew. Filing anew moves nothing.
// verifies: REQ-ORD-23
#[test]
fn differing_arrival_is_filed_anew_only_by_the_operator_or_its_reserve() {
    let mut env = env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&order)]);
    let short = |env: &mut Env, arrivals: &mut Arrivals, providers: &mut Reserve, n| {
        let report = report(600, n);
        match arrive(
            env,
            providers,
            arrivals,
            &mut Open(vec![open_order(&order)]),
            &report,
        )
        .unwrap()
        {
            Arrived::Held { arrival, .. } => arrival,
            other => panic!("{other:?}"),
        }
    };
    let first = short(&mut env, &mut arrivals, &mut providers, 0);
    let second = short(&mut env, &mut arrivals, &mut providers, 1);

    // Anyone else files nothing, and nothing is recorded.
    let impostor_operator = Actor {
        acts: id("someone-else"),
        ..operator()
    };
    let other_reserve = Actor {
        party: id("another-reserve"),
        acts: id("another-reserve"),
        ..reserve_filer()
    };
    env.actors.borrow_mut().list(&other_reserve);
    let len = env.len();
    for stranger in [customer(), impostor_operator, other_reserve] {
        assert_eq!(
            refile(&mut env, &mut providers, &mut arrivals, &stranger, &first),
            Err(ArrivalError::NotTheFiler),
            "{stranger:?}"
        );
    }
    assert_eq!(env.len(), len);

    // The operator files the first anew, the reserve the second, each against the order it arrived for.
    let by_operator = refile(&mut env, &mut providers, &mut arrivals, &operator(), &first).unwrap();
    let by_reserve = refile(
        &mut env,
        &mut providers,
        &mut arrivals,
        &reserve_filer(),
        &second,
    )
    .unwrap();
    for (receipt, filer, arrival) in [
        (&by_operator, operator(), &first),
        (&by_reserve, reserve_filer(), &second),
    ] {
        let Entry::Verified(v) = at(&env, receipt) else {
            panic!("a refiling is a verified entry");
        };
        assert_eq!(v.who, filer);
        assert_eq!(
            v.what,
            What::Refiling {
                arrival: arrival.clone(),
                order: order.clone()
            }
        );
    }
    // Filed anew again, it is the same refiling.
    let len = env.len();
    assert_eq!(
        refile(&mut env, &mut providers, &mut arrivals, &operator(), &first),
        Ok(by_operator)
    );
    assert_eq!(env.len(), len);

    // An arrival that met no order, or matched its order, isn't filed anew.
    let none = match arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &report(600, 2),
    )
    .unwrap()
    {
        Arrived::NoOrder { arrival } => arrival,
        other => panic!("{other:?}"),
    };
    let matched = match arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut open,
        &report(1000, 3),
    )
    .unwrap()
    {
        Arrived::Held { arrival, .. } => arrival,
        other => panic!("{other:?}"),
    };
    for arrival in [&none, &matched] {
        assert_eq!(
            refile(
                &mut env,
                &mut providers,
                &mut arrivals,
                &operator(),
                arrival
            ),
            Err(ArrivalError::NoSuchArrival)
        );
        // To a stranger, how an arrival met its order isn't told: it isn't the filer, whatever the arrival.
        assert_eq!(
            refile(
                &mut env,
                &mut providers,
                &mut arrivals,
                &customer(),
                arrival
            ),
            Err(ArrivalError::NotTheFiler)
        );
    }
    // Nothing moved on any of it.
    assert_eq!(env.started(), vec![order.as_str().to_owned()]);
    assert_admitted(&env);
}

// REQ-ORD-13, REQ-ORD-15 -------------------------------------------------------------------------------------------

/// An order filed on an automatic deposit, by the reserve that declared it or by the operator, is accepted on that
/// deposit: its acceptance says so (`automatic_deposit`), carries no yes and no session, names its filer, rests on the
/// deposit's arrival held against it, and starts the order; an order the customer accepts is recorded as given,
/// with its yes and session. The log admits both.
// verifies: REQ-ORD-15, REQ-ORD-13
#[test]
fn automatic_deposit_is_recorded_as_acceptance_apart_from_an_explicit_one() {
    for filer in [reserve_filer(), operator()] {
        let mut env = env();
        let mut providers = Reserve::new(&env);
        let mut arrivals = Arrivals::new();
        let deposited = match arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut Open::default(),
            &report(1000, 0),
        )
        .unwrap()
        {
            Arrived::NoOrder { arrival } => arrival,
            other => panic!("{other:?}"),
        };
        let filed = file(
            &mut env,
            &mut providers,
            &mut arrivals,
            &filing(filer.clone(), &deposited),
        )
        .unwrap();
        let Filed::Started {
            order: automatic,
            acceptance,
        } = filed
        else {
            panic!("{filed:?}");
        };
        let explicit = explicit_order(&mut env, &mut providers);

        let acceptances = env.acceptances();
        assert_eq!(acceptances.len(), 2);
        let of = |order: &Id| {
            acceptances
                .iter()
                .find(|v| matches!(&v.what, What::Acceptance { order: o, .. } if o == order))
                .cloned()
                .unwrap()
        };
        let auto = of(&automatic);
        let given = of(&explicit);
        let What::Acceptance {
            acceptance: auto_id,
            basis,
            yeses,
            session,
            origin_reserve,
            ..
        } = &auto.what
        else {
            unreachable!()
        };
        assert_eq!(auto_id, &acceptance);
        assert_eq!(*basis, AcceptanceBasis::AutomaticDeposit);
        assert!(yeses.is_empty());
        assert_eq!(session, &None);
        assert_eq!(origin_reserve, &id(RESERVE));
        assert_eq!(auto.who, filer, "the acceptance names its filer");
        let What::Acceptance {
            basis,
            yeses,
            session,
            ..
        } = &given.what
        else {
            unreachable!()
        };
        assert_eq!(*basis, AcceptanceBasis::Given);
        assert_eq!(yeses.len(), 1);
        assert_eq!(session, &Some(id(SESSION)));
        assert_eq!(given.who, customer());

        // The deposit's arrival is held against the order it started, on the reserve's answer about that order.
        let held = arrivals_recorded(&env)
            .into_iter()
            .find(|(o, _, _)| o.as_ref() == Some(&automatic))
            .expect("the deposit's arrival names its order");
        assert_eq!(held.1, ArrivalMatch::Matches);
        assert_bound_to_its_question(&env, &automatic, &held.2);
        // The authorisation is the filer's, and the start is the filer's statement.
        assert_eq!(env.policy.asked[0].given_by, filer);
        assert_eq!(env.policy.asked[0].origin_reserve, id(RESERVE));
        let start = arrivals.start(&automatic).expect("the order started");
        let Observed::RouteChosen { parts, .. } = &start.observed else {
            panic!("a start is a route chosen");
        };
        assert_eq!(parts[0].segments[0].provider.as_str(), RESERVE);
        let kind = if filer.role == Role::Operator {
            SourceKind::Operator
        } else {
            SourceKind::Provider
        };
        assert_eq!(start.source.kind, kind);
        assert_eq!(start.source.id.as_str(), filer.acts.as_str());
        assert!(env.started().contains(&automatic.as_str().to_owned()));
        let record = arrivals.record(&automatic).expect("the order's record");
        assert_eq!(record.parties.requester, customer().party);
        assert_eq!(record.origin_reserve, id(RESERVE));
        assert_admitted(&env);
    }
}

/// A reserve whose declaration says it doesn't start routes on automatic deposits starts nothing on one, whoever
/// files it: no order, no question to anyone, no authorisation asked, and nothing recorded beyond the arrival.
// verifies: REQ-ORD-13
#[test]
fn automatic_deposit_at_a_reserve_that_never_declared_it_starts_nothing() {
    let mut env = env();
    // Its latest declaration, in a version after the one it joined with, says no.
    let mut ad = serde_json::to_value(snapshot_support::reserve(RESERVE, "acme")).unwrap();
    ad["offers"]["reserve"]["declaration"]["starts_routes_on_automatic_deposits"] = false.into();
    ad["offers"]["reserve"]["declaration"]["declared_in_version"] = "2.0.0".into();
    env.catalogue
        .declarations_mut()
        .record(&serde_json::from_value(ad).unwrap(), Instant(START))
        .unwrap();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = match arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &report(1000, 0),
    )
    .unwrap()
    {
        Arrived::NoOrder { arrival } => arrival,
        other => panic!("{other:?}"),
    };
    let len = env.len();
    for filer in [reserve_filer(), operator()] {
        assert_eq!(
            file(
                &mut env,
                &mut providers,
                &mut arrivals,
                &filing(filer, &deposited)
            ),
            Err(ArrivalError::NotDeclared)
        );
    }
    assert_eq!(env.len(), len, "nothing recorded beyond the arrival");
    assert_eq!(providers.asked(), 0, "no one asked");
    assert!(env.policy.asked.is_empty(), "no authorisation asked");
    assert!(env.started().is_empty());
    assert!(order_records(&env).is_empty());
    assert_admitted(&env);
}

/// An automatic deposit is filed only by its reserve or the operator, once, on an arrival that met no order, for what
/// arrived where it arrived, for the account's holder: any other filing starts nothing.
// verifies: REQ-ORD-13, INV-8
#[test]
fn automatic_deposit_is_filed_once_by_its_reserve_or_the_operator_for_what_arrived() {
    let mut env = env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = match arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &report(1000, 0),
    )
    .unwrap()
    {
        Arrived::NoOrder { arrival } => arrival,
        other => panic!("{other:?}"),
    };
    let len = env.len();

    // Not the operator nor its reserve: nothing.
    let other_reserve = Actor {
        party: id("another-reserve"),
        acts: id("another-reserve"),
        ..reserve_filer()
    };
    env.actors.borrow_mut().list(&other_reserve);
    for stranger in [
        customer(),
        other_reserve,
        Actor {
            acts: id("someone-else"),
            ..operator()
        },
    ] {
        assert_eq!(
            file(
                &mut env,
                &mut providers,
                &mut arrivals,
                &filing(stranger, &deposited)
            ),
            Err(ArrivalError::NotTheFiler)
        );
    }
    assert_eq!(env.len(), len);
    assert_eq!(providers.asked(), 0);

    // Not what arrived, or not for its holder: refused before any order exists, with nothing asked.
    let mut other_amount = filing(reserve_filer(), &deposited);
    other_amount.request = id("deposit-2");
    other_amount.route = route("999");
    let mut other_requester = filing(reserve_filer(), &deposited);
    other_requester.request = id("deposit-3");
    other_requester.parties.requester = id("someone-else-party");
    for wrong in [&other_amount, &other_requester] {
        assert_eq!(
            file(&mut env, &mut providers, &mut arrivals, wrong),
            Ok(Filed::CantProceed {
                order: None,
                class: OrderRefusalClass::MalformedRequest
            })
        );
    }
    // Nor in a session that isn't the holder's: refused of policy's class.
    let mut other_session = filing(reserve_filer(), &deposited);
    other_session.request = id("deposit-5");
    // A session policy opened, but for another holder.
    env.sessions.held.insert(
        "session-of-another".into(),
        rutent_core_orch::intake::HeldSession {
            holder: id("another-holder"),
            expires_at: rutent_contracts::router_api::Timestamp::new(
                Instant(SESSION_ENDS).rfc3339().unwrap(),
            )
            .unwrap(),
        },
    );
    other_session.session = id("session-of-another");
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &other_session),
        Ok(Filed::CantProceed {
            order: None,
            class: OrderRefusalClass::RefusedByPolicy
        })
    );
    // Told the same again, recording nothing more.
    let len_refused = env.len();
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &other_session),
        Ok(Filed::CantProceed {
            order: None,
            class: OrderRefusalClass::RefusedByPolicy
        })
    );
    assert_eq!(env.len(), len_refused);
    assert!(env.started().is_empty());
    assert_eq!(providers.asked(), 0);

    // Filed once; the same filing again is told the same and records nothing; another on the same deposit opens
    // nothing.
    let one = filing(reserve_filer(), &deposited);
    let first = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(matches!(first, Filed::Started { .. }), "{first:?}");
    let len = env.len();
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &one),
        Ok(first.clone())
    );
    let mut another = filing(operator(), &deposited);
    another.request = id("deposit-4");
    let Filed::Started { order, .. } = &first else {
        unreachable!()
    };
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &another),
        Err(ArrivalError::Conflicting {
            order: Some(order.clone())
        })
    );
    assert_eq!(env.len(), len);
    assert_eq!(env.started().len(), 1);
    // To anyone but its filers, the order filed on the deposit doesn't exist: no one else is told its id.
    for stranger in [
        customer(),
        Actor {
            party: id("another-reserve"),
            acts: id("another-reserve"),
            ..reserve_filer()
        },
        Actor {
            acts: id("someone-else"),
            ..operator()
        },
    ] {
        let mut theirs = filing(stranger, &deposited);
        theirs.request = id("deposit-6");
        assert_eq!(
            file(&mut env, &mut providers, &mut arrivals, &theirs),
            Err(ArrivalError::NotTheFiler)
        );
    }
    assert_eq!(env.len(), len);
    // And an arrival Core never recorded files nothing.
    let mut unknown = filing(reserve_filer(), &deposited);
    unknown.arrival.index = 999u64.try_into().unwrap();
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &unknown),
        Err(ArrivalError::NoSuchArrival)
    );
    // An arrival held against an open order is the order's: no automatic deposit is filed on it.
    let explicit = explicit_order(&mut env, &mut providers);
    let held = match arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open(vec![open_order(&explicit)]),
        &report(600, 1),
    )
    .unwrap()
    {
        Arrived::Held { arrival, .. } => arrival,
        other => panic!("{other:?}"),
    };
    let mut on_held = filing(reserve_filer(), &held);
    on_held.route = route("600");
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &on_held),
        Err(ArrivalError::NoSuchArrival)
    );
    assert_admitted(&env);
}

/// A filing that stopped where the log couldn't take an entry, or lost its answer, is carried on by filing it again:
/// the reserve is asked once, the order routed once, policy asked once, and each entry recorded as first built.
// verifies: REQ-ORD-15, INV-8
#[test]
fn automatic_deposit_filing_is_carried_on_where_it_stopped_asking_no_one_twice() {
    let mut env = env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = match arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &report(1000, 0),
    )
    .unwrap()
    {
        Arrived::NoOrder { arrival } => arrival,
        other => panic!("{other:?}"),
    };
    let one = filing(reserve_filer(), &deposited);
    // The deposit's arrival, then the acceptance, can't be appended now; then the start is written and its answer lost.
    for (fail, lose) in [
        (Some("other"), None),
        (Some("acceptance"), None),
        (None, Some("input")),
    ] {
        env.book.borrow_mut().fail_next = fail;
        env.book.borrow_mut().lose_next = lose;
        assert_eq!(
            file(&mut env, &mut providers, &mut arrivals, &one),
            Err(ArrivalError::Log(
                rutent_contracts::core_internal::calls::AppendFailure::Unavailable
            ))
        );
    }
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    let Filed::Started { order, .. } = &filed else {
        panic!("{filed:?}");
    };
    assert_eq!(providers.verifications.len(), 1, "the reserve asked once");
    assert_eq!(env.policy.asked.len(), 1, "policy asked once");
    let whats = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let count = |f: &dyn Fn(&What) -> bool| whats.iter().filter(|w| f(w)).count();
    assert_eq!(
        count(&|w| matches!(w, What::Arrival { order: Some(o), .. } if o == order)),
        1
    );
    assert_eq!(
        count(&|w| matches!(w, What::DecisionRecord { order: o, .. } if o == order)),
        2,
        "one routing: a shortlist and a choice"
    );
    assert_eq!(count(&|w| matches!(w, What::Acceptance { .. })), 1);
    // The start whose answer was lost is appended again as first built.
    let starts: Vec<&What> = whats
        .iter()
        .filter(|w| matches!(w, What::Input { .. }))
        .collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0], starts[1]);
    assert_eq!(
        arrivals
            .start(order)
            .map(|i| What::Input { input: i.clone() })
            .as_ref(),
        Some(starts[0])
    );
    assert_admitted(&env);
}

/// A filed order whose destination is refused, whose reserve doesn't confirm the deposit, or whose authorisation
/// policy refuses as outside it ends, of its class; one whose destination isn't resolved yet, or whose filer policy
/// refuses, or that policy can't decide, waits, and filed again goes on.
// verifies: REQ-ORD-13, REQ-ORD-18, REQ-ORD-24
#[test]
fn automatic_deposit_filing_ends_or_waits_as_its_destination_reserve_and_policy_answer() {
    use rutent_contracts::core_internal::{Answer, NotKnownCause, RefusalClass};
    use rutent_core_orch::intake::{Resolution, Wait};

    /// A fresh environment with the deposit arrived, and its filing by the reserve.
    fn deposited() -> (Env, Reserve, Arrivals, Filing) {
        let mut env = env();
        let mut providers = Reserve::new(&env);
        let mut arrivals = Arrivals::new();
        let arrival = match arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut Open::default(),
            &report(1000, 0),
        )
        .unwrap()
        {
            Arrived::NoOrder { arrival } => arrival,
            other => panic!("{other:?}"),
        };
        (env, providers, arrivals, filing(reserve_filer(), &arrival))
    }
    let ended = |filed: &Filed, class| matches!(filed, Filed::CantProceed { order: Some(_), class: c } if *c == class);

    // The destination refused: ended, and never routed.
    let (mut env, mut providers, mut arrivals, one) = deposited();
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::Refused);
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        ended(&filed, OrderRefusalClass::DestinationRefused),
        "{filed:?}"
    );
    assert_eq!(providers.asked(), 0);
    assert!(env.acceptances().is_empty());
    assert_admitted(&env);

    // Not resolved yet: it waits; once it resolves, filed again, it starts, under the same order.
    let (mut env, mut providers, mut arrivals, one) = deposited();
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::NotYet);
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    let Filed::Waiting { order, wait } = &filed else {
        panic!("{filed:?}");
    };
    assert_eq!(*wait, Wait::DestinationNotYetResolved);
    assert_eq!(providers.asked(), 0);
    env.register.answers.clear();
    let again = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        matches!(&again, Filed::Started { order: o, .. } if o == order),
        "{again:?}"
    );
    assert_admitted(&env);

    // The reserve doesn't confirm the deposit: ended, never routed.
    let (mut env, mut providers, mut arrivals, one) = deposited();
    providers.insufficient = true;
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        ended(&filed, OrderRefusalClass::NoRouteRightNow),
        "{filed:?}"
    );
    assert_eq!(providers.sim.received.len(), 0, "never routed");
    assert!(env.policy.asked.is_empty());
    assert_admitted(&env);

    // Policy refuses the authorisation as outside it: ended; filed again, told the same, policy not asked again.
    let (mut env, mut providers, mut arrivals, one) = deposited();
    env.policy.answer = refusal(RefusalClass::OutsideAuthorisation);
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        ended(&filed, OrderRefusalClass::RefusedByPolicy),
        "{filed:?}"
    );
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &one),
        Ok(filed)
    );
    assert_eq!(env.policy.asked.len(), 1);
    assert!(env.acceptances().is_empty());
    assert_admitted(&env);

    // Policy refuses the filer, or can't decide: the order stands; once policy permits, filed again, it starts.
    for answer in [
        refusal(RefusalClass::NotPermitted),
        Answer::NotKnown {
            cause: NotKnownCause::PolicyUnreachable,
        },
    ] {
        let (mut env, mut providers, mut arrivals, one) = deposited();
        env.policy.answer = answer;
        let filed = file(&mut env, &mut providers, &mut arrivals, &one);
        match &filed {
            Err(ArrivalError::Refused { class }) => {
                assert_eq!(*class, OrderRefusalClass::RefusedByPolicy)
            }
            Ok(Filed::Waiting { wait, .. }) => assert_eq!(*wait, Wait::PolicyNotKnown),
            other => panic!("{other:?}"),
        }
        assert!(env.acceptances().is_empty());
        env.policy.answer = Answer::Permit {};
        let again = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
        assert!(matches!(again, Filed::Started { .. }), "{again:?}");
        assert_eq!(env.policy.asked.len(), 2);
        assert_admitted(&env);
    }
}
