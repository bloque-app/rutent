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
    ArrivalError, Arrivals, Arrived, Filed, FiledOrders, FiledWait, Filing, OpenOrder, OpenOrders,
    Report,
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
    /// Whether the reserve answers Core's balance verifications with nothing.
    silent: bool,
    /// The balance verifications the reserve was asked, as received.
    verifications: Vec<Value>,
}

impl Reserve {
    fn new(env: &Env) -> Self {
        Self {
            sim: env.sim(),
            insufficient: false,
            silent: false,
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
        if self.silent {
            return None;
        }
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

/// The shape the tests' reserve gives its notices: provider-role names none for a notice of value arriving.
const NOTICE_SHAPE: &str = "urn:rutent:test:arrival-notice";

/// The transaction the reserve gives the `n`th credit.
fn credit(n: u32) -> String {
    format!("credit-{n}")
}

/// The reserve's notice that `units` arrived at [`ACCOUNT`], signed, naming the transaction it gave the credit.
fn notice(units: u64, n: u32) -> Vec<u8> {
    let body = json!({
        "kind": "arrival",
        "transaction": credit(n),
        "node": "polygon",
        "asset": "USDC",
        "amount": units.to_string(),
        "account": ACCOUNT,
    });
    let value = envelope(&deployment(), NOTICE_SHAPE, RESERVE, START, body);
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
        holder: customer().party,
        amount: amount(1000),
        session: id(SESSION),
        part: None,
    }
}

/// An environment whose register lists the operator and the reserve as acting for themselves.
fn env() -> Env {
    // What the assembly was told is this environment's alone.
    OPENED.with(|o| o.borrow_mut().clear());
    WITHDRAWN.with(|w| w.borrow_mut().clear());
    let mut env = Env::new();
    env.actors.borrow_mut().list(&operator());
    env.actors.borrow_mut().list(&reserve_filer());
    // The customer's address is the customer's own: an automatic deposit goes to its holder.
    env.register.answers.insert(
        ADDRESS.into(),
        rutent_core_orch::intake::Resolution::Owner(customer().party),
    );
    env
}

/// The assembly, told of each order a filing opens: it holds that policy hasn't been asked about the order yet, so
/// policy's sources can answer for it when it is.
struct TellsAssembly(std::rc::Rc<std::cell::RefCell<Book>>);

thread_local! {
    /// Every order record the assembly was told of, in this test's thread.
    static OPENED: std::cell::RefCell<Vec<Id>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Every order the assembly was told never opened, in this test's thread.
    static WITHDRAWN: std::cell::RefCell<Vec<Id>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl FiledOrders for TellsAssembly {
    fn withdrawn(&mut self, order: &Id) {
        assert!(
            OPENED.with(|o| o.borrow().contains(order)),
            "only an order handed over is withdrawn"
        );
        WITHDRAWN.with(|w| w.borrow_mut().push(order.clone()));
    }

    fn opened(&mut self, record: &rutent_core_orch::intake::OrderRecord) {
        OPENED.with(|o| o.borrow_mut().push(record.order.clone()));
        WITHDRAWN.with(|w| {
            assert!(
                !w.borrow().contains(&record.order),
                "an order is opened once"
            )
        });
        let asked = self.0.borrow().verified().into_iter().any(|w| {
            matches!(w, What::Authorise { authorisation, .. } if authorisation.order == record.order)
        });
        assert!(
            !asked,
            "the order's record comes before policy is asked about it"
        );
        assert_eq!(record.origin_reserve, id(RESERVE));
        assert_eq!(record.parties.requester, customer().party);
    }
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
    let mut told = TellsAssembly(env.book.clone());
    env.with_providers(providers, |_, desk| {
        arrivals.file::<RouteFinder>(desk, &mut told, filing.clone())
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
        let mut env = crate::env();
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
            let about = stated(&notice).about.clone().expect("about the deposit's transaction");
            prop_assert!(about.order.is_none());
            prop_assert!(about.transaction.is_some());
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
    let mut env = crate::env();
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
    let mut env = crate::env();
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
    let mut env = crate::env();
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
            matches!(&e, Entry::Stated(s) if s.what.statement.as_str() == NOTICE_SHAPE
                && s.about.as_ref().is_some_and(|a| a.order.is_none()))
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
        matches!(unconfirmed, Arrived::Waiting { .. }),
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
    let mut env = crate::env();
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
    for stranger in [customer(), impostor_operator] {
        assert_eq!(
            refile(&mut env, &mut providers, &mut arrivals, &stranger, &first),
            Err(ArrivalError::NotTheFiler),
            "{stranger:?}"
        );
    }
    // Another reserve, acting for itself, is told only that there is no arrival of its own there.
    assert_eq!(
        refile(
            &mut env,
            &mut providers,
            &mut arrivals,
            &other_reserve,
            &first
        ),
        Err(ArrivalError::NoSuchArrival)
    );
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
        let mut env = crate::env();
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
        assert!(
            OPENED.with(|o| o.borrow().contains(&automatic)),
            "the assembly was told of the order before policy was asked"
        );
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
    let mut env = crate::env();
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
    let mut env = crate::env();
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
    assert_eq!(
        file(
            &mut env,
            &mut providers,
            &mut arrivals,
            &filing(other_reserve.clone(), &deposited)
        ),
        Err(ArrivalError::NoSuchArrival),
        "another reserve has no arrival of its own there"
    );
    for stranger in [
        customer(),
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
            order: order.clone()
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
        // A stranger isn't a filer; another reserve has no arrival of its own there. Neither is told the order.
        let expected = if stranger.role == Role::Provider {
            ArrivalError::NoSuchArrival
        } else {
            ArrivalError::NotTheFiler
        };
        let mut theirs = filing(stranger, &deposited);
        theirs.request = id("deposit-6");
        assert_eq!(
            file(&mut env, &mut providers, &mut arrivals, &theirs),
            Err(expected)
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
    let mut env = crate::env();
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

/// A filing whose destination is refused, or whose authorisation policy refuses as outside it, is refused before an
/// order opens; a filed order whose reserve says the deposit isn't there ends, of its class. One whose destination
/// isn't resolved yet, or whose filer policy refuses, or that policy can't decide, opens nothing, and filed again
/// goes on.
// verifies: REQ-ORD-13, REQ-ORD-18, REQ-ORD-24
#[test]
fn automatic_deposit_filing_ends_or_waits_as_its_destination_reserve_and_policy_answer() {
    use rutent_contracts::core_internal::{Answer, NotKnownCause, RefusalClass};
    use rutent_core_orch::intake::Resolution;

    /// A fresh environment with the deposit arrived, and its filing by the reserve.
    fn deposited() -> (Env, Reserve, Arrivals, Filing) {
        let mut env = crate::env();
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
    let refused = |filed: &Filed, class| matches!(filed, Filed::CantProceed { order: None, class: c } if *c == class);

    // The destination refused: refused before an order opens, and never routed nor put to policy.
    let (mut env, mut providers, mut arrivals, one) = deposited();
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::Refused);
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        refused(&filed, OrderRefusalClass::DestinationRefused),
        "{filed:?}"
    );
    assert_eq!(providers.asked(), 0);
    assert!(env.policy.asked.is_empty());
    assert!(env.acceptances().is_empty());
    assert_admitted(&env);

    // Not resolved yet: it waits, opening nothing; once it resolves, filed again, it starts.
    let (mut env, mut providers, mut arrivals, one) = deposited();
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::NotYet);
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert_eq!(
        filed,
        Filed::Waiting {
            order: None,
            wait: FiledWait::DestinationNotYetResolved
        }
    );
    assert_eq!(providers.asked(), 0);
    assert!(env.policy.asked.is_empty());
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::Owner(customer().party));
    let again = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(matches!(&again, Filed::Started { .. }), "{again:?}");
    assert_admitted(&env);

    // The reserve says the funds aren't there: ended, never routed.
    let (mut env, mut providers, mut arrivals, one) = deposited();
    providers.insufficient = true;
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        ended(&filed, OrderRefusalClass::NoRouteRightNow),
        "{filed:?}"
    );
    assert_eq!(providers.sim.received.len(), 0, "never routed");
    assert_admitted(&env);

    // Policy refuses the authorisation as outside it: refused, no order opened; filed again, told the same, policy not
    // asked again.
    let (mut env, mut providers, mut arrivals, one) = deposited();
    env.policy.answer = refusal(RefusalClass::OutsideAuthorisation);
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        refused(&filed, OrderRefusalClass::RefusedByPolicy),
        "{filed:?}"
    );
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &one),
        Ok(filed)
    );
    assert_eq!(providers.asked(), 0);
    assert_eq!(env.policy.asked.len(), 1);
    assert!(env.acceptances().is_empty());
    assert_admitted(&env);

    // Policy refuses the filer, or can't decide: nothing opens; once policy permits, filed again, it starts.
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
            Ok(Filed::Waiting { order, wait }) => {
                assert_eq!((order, *wait), (&None, FiledWait::PolicyNotKnown))
            }
            other => panic!("{other:?}"),
        }
        assert!(env.acceptances().is_empty());
        assert_eq!(providers.asked(), 0);
        env.policy.answer = Answer::Permit {};
        let again = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
        assert!(matches!(again, Filed::Started { .. }), "{again:?}");
        assert_eq!(env.policy.asked.len(), 2);
        assert_admitted(&env);
    }
}

// The review of b5142fa ---------------------------------------------------------------------------------------------

/// An arrival with no order, at [`RESERVE`], of `units`.
fn deposit(
    env: &mut Env,
    providers: &mut Reserve,
    arrivals: &mut Arrivals,
    units: u64,
    n: u32,
) -> Receipt {
    match arrive(
        env,
        providers,
        arrivals,
        &mut Open::default(),
        &report(units, n),
    )
    .unwrap()
    {
        Arrived::NoOrder { arrival } => arrival,
        other => panic!("{other:?}"),
    }
}

/// The balance verifications the catalogue logged for [`ACCOUNT`].
fn verifications_logged(env: &Env) -> usize {
    env.catalogue
        .check_log(ACCOUNT)
        .iter()
        .filter(|r| r.kind == rutent_prov::catalogue::CheckKind::BalanceVerification)
        .count()
}

/// An actor stating the operator's role is not the operator: only the deployment's operator party is (finding 1).
// verifies: REQ-ORD-23, REQ-ORD-13
#[test]
fn an_actor_stating_the_operators_role_is_not_the_operator() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let short = match arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open(vec![open_order(&order)]),
        &report(600, 0),
    )
    .unwrap()
    {
        Arrived::Held { arrival, .. } => arrival,
        other => panic!("{other:?}"),
    };
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 1);
    // The customer, listed as acting for its own party, stating the operator's role.
    let impostor = Actor {
        role: Role::Operator,
        ..customer()
    };
    let (len, asked) = (env.len(), env.policy.asked.len());
    assert_eq!(
        refile(&mut env, &mut providers, &mut arrivals, &impostor, &short),
        Err(ArrivalError::NotTheFiler)
    );
    assert_eq!(
        file(
            &mut env,
            &mut providers,
            &mut arrivals,
            &filing(impostor, &deposited)
        ),
        Err(ArrivalError::NotTheFiler)
    );
    assert_eq!(env.len(), len);
    assert_eq!(env.policy.asked.len(), asked);
}

/// A filing policy refuses, or one that ends, leaves the deposit free for its reserve to file (finding 2): nothing is
/// opened, held, asked or routed before policy permits.
// verifies: REQ-ORD-13, REQ-ORD-15
#[test]
fn a_filing_refused_or_ended_leaves_the_deposit_to_its_reserve() {
    use rutent_contracts::core_internal::{Answer, RefusalClass};
    // Refused about the filer: a stated operator refused, and an operator without the permission to act for users.
    for class in [RefusalClass::NotPermitted, RefusalClass::MissingInput] {
        let mut env = crate::env();
        let mut providers = Reserve::new(&env);
        let mut arrivals = Arrivals::new();
        let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
        env.policy.answer = refusal(class);
        let refused = file(
            &mut env,
            &mut providers,
            &mut arrivals,
            &filing(operator(), &deposited),
        );
        assert!(
            matches!(refused, Err(ArrivalError::Refused { .. })),
            "{refused:?}"
        );
        assert_eq!(providers.asked(), 0, "nothing asked before policy permits");
        assert!(
            !env.book.borrow().verified().into_iter().any(|w| matches!(
                w,
                What::DestinationCheck { .. }
                    | What::Arrival { order: Some(_), .. }
                    | What::DecisionRecord { .. }
            )),
            "nothing opened, held or routed"
        );
        env.policy.answer = Answer::Permit {};
        let mut theirs = filing(reserve_filer(), &deposited);
        theirs.request = id("deposit-reserve");
        let filed = file(&mut env, &mut providers, &mut arrivals, &theirs).unwrap();
        assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
        assert_admitted(&env);
    }
    // Refused as outside the authorisation, or ended because the reserve says the funds aren't there.
    for case in ["outside", "no"] {
        let mut env = crate::env();
        let mut providers = Reserve::new(&env);
        let mut arrivals = Arrivals::new();
        let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
        match case {
            "outside" => env.policy.answer = refusal(RefusalClass::OutsideAuthorisation),
            _ => providers.insufficient = true,
        }
        let filed = file(
            &mut env,
            &mut providers,
            &mut arrivals,
            &filing(operator(), &deposited),
        )
        .unwrap();
        assert!(matches!(filed, Filed::CantProceed { .. }), "{filed:?}");
        env.policy.answer = Answer::Permit {};
        providers.insufficient = false;
        let mut theirs = filing(reserve_filer(), &deposited);
        theirs.request = id("deposit-reserve");
        let filed = file(&mut env, &mut providers, &mut arrivals, &theirs);
        if case == "no" {
            // The reserve said the funds aren't there: that deposit is filed on no more, and a new notice is.
            assert_eq!(filed, Err(ArrivalError::NotFileable));
            let fresh = deposit(&mut env, &mut providers, &mut arrivals, 1000, 1);
            theirs.arrival = fresh;
            let filed = file(&mut env, &mut providers, &mut arrivals, &theirs).unwrap();
            assert!(matches!(filed, Filed::Started { .. }), "{case}: {filed:?}");
        } else {
            assert!(
                matches!(filed, Ok(Filed::Started { .. })),
                "{case}: {filed:?}"
            );
        }
        assert_admitted(&env);
    }
}

/// A reserve silent on Core's question, or a check the catalogue can't let out, makes the arrival wait, never a final
/// no: the same notice again asks again, and once no order is open it meets none (finding 3).
// verifies: REQ-ORD-23, REQ-ORD-14, REQ-CAT-19
#[test]
fn a_silent_reserve_or_a_refused_check_makes_an_arrival_wait() {
    // P4: silent, then answering.
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&order)]);
    providers.silent = true;
    let short = report(600, 0);
    let first = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &short).unwrap();
    assert!(
        !matches!(first, Arrived::Held { .. } | Arrived::NoOrder { .. }),
        "{first:?}"
    );
    providers.silent = false;
    let again = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &short).unwrap();
    assert!(
        matches!(
            again,
            Arrived::Held {
                matched: ArrivalMatch::Differs,
                ..
            }
        ),
        "{again:?}"
    );
    assert_admitted(&env);

    // P4b: the order's session isn't one the catalogue holds, so the check can't go; once no order is open, it meets
    // none.
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let mut ended_session = open_order(&order);
    ended_session.session = id("session-ended");
    let short = report(600, 0);
    let first = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open(vec![ended_session]),
        &short,
    )
    .unwrap();
    assert!(
        !matches!(first, Arrived::Held { .. } | Arrived::NoOrder { .. }),
        "{first:?}"
    );
    let again = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &short,
    )
    .unwrap();
    assert!(matches!(again, Arrived::NoOrder { .. }), "{again:?}");
    assert_admitted(&env);

    // At a filing: a silent reserve makes it wait, and filed again once it answers, it starts.
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    providers.silent = true;
    let one = filing(reserve_filer(), &deposited);
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        !matches!(filed, Filed::CantProceed { .. } | Filed::Started { .. }),
        "{filed:?}"
    );
    providers.silent = false;
    let again = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(matches!(again, Filed::Started { .. }), "{again:?}");
    assert_admitted(&env);
}

/// One notice is one arrival: the same bytes reported again with other figures are refused, and a notice its reserve
/// didn't make, or one naming a transaction, is refused before anything is kept (finding 4).
// verifies: REQ-ORD-14, REQ-ORD-13, INV-8
#[test]
fn one_notice_is_one_arrival_whatever_figures_it_comes_with() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let reported = report(1000, 0);
    deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let mut other = reported.clone();
    other.amount = amount(900);
    let len = env.len();
    let again = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &other,
    );
    assert!(
        matches!(again, Err(ref e) if !matches!(e, ArrivalError::Log(_))),
        "{again:?}"
    );
    assert_eq!(env.len(), len);
    assert_eq!(arrivals_recorded(&env).len(), 1);
    // Another reserve's notice, one naming no transaction, or a result (which answers a question Core asked), reported
    // as this reserve's.
    let elsewhere = sign(
        envelope(
            &deployment(),
            NOTICE_SHAPE,
            "another-reserve",
            START,
            json!({ "kind": "arrival", "transaction": "credit-elsewhere", "amount": "5" }),
        ),
        &statements_key(RESERVE, 1),
    );
    let unbound = sign(
        envelope(
            &deployment(),
            NOTICE_SHAPE,
            RESERVE,
            START,
            json!({ "kind": "arrival", "amount": "5" }),
        ),
        &statements_key(RESERVE, 1),
    );
    let result = sign(
        envelope(
            &deployment(),
            RESULT_SHAPE,
            RESERVE,
            START,
            json!({ "kind": "arrival", "transaction": "credit-result" }),
        ),
        &statements_key(RESERVE, 1),
    );
    for notice in [elsewhere, unbound, result] {
        let kept = env.store.borrow().kept.len();
        let reported = Report {
            notice,
            ..report(5, 9)
        };
        let refused = arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut Open::default(),
            &reported,
        );
        assert!(
            matches!(refused, Err(ref e) if !matches!(e, ArrivalError::Log(_))),
            "{refused:?}"
        );
        assert_eq!(env.store.borrow().kept.len(), kept, "nothing kept");
        assert_eq!(env.len(), len);
    }
}

/// An arrival is held only against an open order of its account's holder, and never against an order that already
/// has its arrival: two equal orders at one account each get one, and a stranger's deposit meets none (finding 5).
// verifies: REQ-ORD-23, REQ-ORD-14
#[test]
fn an_arrival_is_held_against_its_holders_order_not_yet_arrived() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let a = explicit_order(&mut env, &mut providers);
    let (b, _) = env.accepted(
        &mut providers.sim,
        &order_request("client-req-2", customer()),
        &customer(),
    );
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&a), open_order(&b)]);
    let held = |arrived: Arrived| match arrived {
        Arrived::Held { order, matched, .. } => (order, matched),
        other => panic!("{other:?}"),
    };
    let first = held(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut open,
            &report(1000, 0),
        )
        .unwrap(),
    );
    // A stranger's deposit at the same account, while the second order is still open, meets no order.
    let stranger = Report {
        holder: id("a-stranger"),
        ..report(1000, 2)
    };
    let theirs = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut open,
        &stranger,
    )
    .unwrap();
    assert!(matches!(theirs, Arrived::NoOrder { .. }), "{theirs:?}");
    let second = held(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut open,
            &report(1000, 1),
        )
        .unwrap(),
    );
    assert_eq!(first, (a.clone(), ArrivalMatch::Matches));
    assert_eq!(second, (b.clone(), ArrivalMatch::Matches));
    assert_admitted(&env);

    // An arrival left waiting holds no order: the next takes the first, and the one that waited, answered, the second.
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let a = explicit_order(&mut env, &mut providers);
    let (b, _) = env.accepted(
        &mut providers.sim,
        &order_request("client-req-2", customer()),
        &customer(),
    );
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&a), open_order(&b)]);
    providers.silent = true;
    let waited = report(1000, 0);
    let first = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &waited).unwrap();
    assert!(matches!(first, Arrived::Waiting { .. }), "{first:?}");
    providers.silent = false;
    let next = held(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut open,
            &report(1000, 1),
        )
        .unwrap(),
    );
    let again = held(arrive(&mut env, &mut providers, &mut arrivals, &mut open, &waited).unwrap());
    assert_eq!(next, (a, ArrivalMatch::Matches));
    assert_eq!(again, (b, ArrivalMatch::Matches));
    assert_eq!(arrivals_recorded(&env).len(), 2, "one arrival each");
    assert_admitted(&env);

    // A question under way about the first order, stopped where the log couldn't take it, takes that order: the next
    // arrival is held against the second, and the first carried on is held against the first.
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let a = explicit_order(&mut env, &mut providers);
    let (b, _) = env.accepted(
        &mut providers.sim,
        &order_request("client-req-2", customer()),
        &customer(),
    );
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&a), open_order(&b)]);
    let stopped = report(1000, 0);
    env.store.borrow_mut().fail_after = Some(1);
    assert!(arrive(&mut env, &mut providers, &mut arrivals, &mut open, &stopped).is_err());
    env.book.borrow_mut().fail_stated_next = true;
    assert!(arrive(&mut env, &mut providers, &mut arrivals, &mut open, &stopped).is_err());
    let next = held(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut open,
            &report(1000, 1),
        )
        .unwrap(),
    );
    let resumed =
        held(arrive(&mut env, &mut providers, &mut arrivals, &mut open, &stopped).unwrap());
    assert_eq!(next, (b, ArrivalMatch::Matches));
    assert_eq!(resumed, (a, ArrivalMatch::Matches));
    assert_admitted(&env);
}

/// An automatic deposit goes to its holder: a filing whose destination resolves to anyone but the beneficiary, or
/// whose beneficiary isn't the holder, is refused, and nothing starts (finding 6).
// verifies: REQ-ORD-13, REQ-ORD-24
#[test]
fn an_automatic_deposit_routed_to_anyone_but_its_holder_is_refused() {
    use rutent_core_orch::intake::Resolution;
    let mut env = crate::env();
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::Owner(id("someone-else")));
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let filed = file(
        &mut env,
        &mut providers,
        &mut arrivals,
        &filing(reserve_filer(), &deposited),
    )
    .unwrap();
    assert!(matches!(filed, Filed::CantProceed { .. }), "{filed:?}");
    env.register
        .answers
        .insert(ADDRESS.into(), Resolution::Owner(customer().party));
    let mut elsewhere = filing(reserve_filer(), &deposited);
    elsewhere.request = id("deposit-2");
    elsewhere.parties.beneficiary = id("someone-else");
    let filed = file(&mut env, &mut providers, &mut arrivals, &elsewhere).unwrap();
    assert!(
        matches!(
            filed,
            Filed::CantProceed {
                class: OrderRefusalClass::MalformedRequest,
                ..
            }
        ),
        "{filed:?}"
    );
    assert!(env.started().is_empty());
    assert!(env.policy.asked.is_empty());
}

/// A question that couldn't be kept opens no check: retried, it opens one, and one only (finding 7).
// verifies: REQ-CAT-4, INV-8
#[test]
fn a_question_retried_opens_one_check() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&order)]);
    let before = verifications_logged(&env);
    let short = report(600, 0);
    // The notice is kept; the question isn't.
    env.store.borrow_mut().fail_after = Some(1);
    assert!(arrive(&mut env, &mut providers, &mut arrivals, &mut open, &short).is_err());
    // The question is kept and its check opened, but its stated entry can't be appended now.
    env.book.borrow_mut().fail_stated_next = true;
    assert!(arrive(&mut env, &mut providers, &mut arrivals, &mut open, &short).is_err());
    let held = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &short).unwrap();
    assert!(matches!(held, Arrived::Held { .. }), "{held:?}");
    assert_eq!(verifications_logged(&env) - before, 1, "one check");
    assert_eq!(providers.verifications.len(), 1, "asked once");
    assert_admitted(&env);
}

/// A reserve with no declaration on record never declared it starts routes on automatic deposits (finding 8).
// verifies: REQ-ORD-13
#[test]
fn a_reserve_with_no_declaration_on_record_starts_nothing() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    // A catalogue that holds no declaration of the reserve's.
    env.catalogue = rutent_prov::catalogue::Catalogue::new();
    let len = env.len();
    assert_eq!(
        file(
            &mut env,
            &mut providers,
            &mut arrivals,
            &filing(reserve_filer(), &deposited)
        ),
        Err(ArrivalError::NotDeclared)
    );
    assert_eq!(env.len(), len);
    assert!(env.policy.asked.is_empty());
}

/// A stranger is told nothing of a receipt, whether an arrival is at it or not; a reserve is told only that it can't
/// file on one that isn't its own (finding 9).
// verifies: REQ-ORD-23, REQ-ORD-13
#[test]
fn a_stranger_is_told_nothing_of_a_receipt() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let mut missing = deposited.clone();
    missing.index = 999u64.try_into().unwrap();
    let unlisted = Actor {
        acts: id("someone-unlisted"),
        ..operator()
    };
    let other_reserve = Actor {
        party: id("another-reserve"),
        acts: id("another-reserve"),
        ..reserve_filer()
    };
    env.actors.borrow_mut().list(&other_reserve);
    for receipt in [&deposited, &missing] {
        assert_eq!(
            file(
                &mut env,
                &mut providers,
                &mut arrivals,
                &filing(unlisted.clone(), receipt)
            ),
            Err(ArrivalError::NotTheFiler)
        );
        assert_eq!(
            refile(&mut env, &mut providers, &mut arrivals, &unlisted, receipt),
            Err(ArrivalError::NotTheFiler)
        );
        assert_eq!(
            file(
                &mut env,
                &mut providers,
                &mut arrivals,
                &filing(other_reserve.clone(), receipt)
            ),
            Err(ArrivalError::NoSuchArrival)
        );
    }
}

/// Another login of the filer's party is told the filing as it stands (the review's nit).
// verifies: REQ-OPS-5, INV-8
#[test]
fn another_login_of_the_filers_party_is_told_the_filing_as_it_stands() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let one = filing(operator(), &deposited);
    let first = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(matches!(first, Filed::Started { .. }), "{first:?}");
    let colleague = Actor {
        acts: id("operator-login-2"),
        ..operator()
    };
    env.actors.borrow_mut().list(&colleague);
    let len = env.len();
    let theirs = Filing {
        filer: colleague,
        ..one
    };
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &theirs),
        Ok(first)
    );
    assert_eq!(env.len(), len);

    // A filing still under way is carried on by any listed login of the filer's party, the acceptance naming it.
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 1);
    providers.silent = true;
    let mut one = filing(operator(), &deposited);
    one.request = id("deposit-2");
    let waiting = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        matches!(waiting, Filed::Waiting { order: Some(_), .. }),
        "{waiting:?}"
    );
    providers.silent = false;
    let theirs = Filing {
        filer: Actor {
            acts: id("operator-login-2"),
            ..operator()
        },
        ..one.clone()
    };
    let started = file(&mut env, &mut providers, &mut arrivals, &theirs).unwrap();
    assert!(matches!(started, Filed::Started { .. }), "{started:?}");
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &one),
        Ok(started)
    );
    let named = env
        .acceptances()
        .into_iter()
        .any(|v| v.who.acts == id("operator-login-2"));
    assert!(named, "the acceptance names the login that carried it on");
    assert_admitted(&env);
}

// The re-review ---------------------------------------------------------------------------------------------------

/// A session for [`ACCOUNT`], held by the customer, open until `ends`, in the catalogue and in policy's register.
fn short_session(env: &mut Env, name: &str, ends: u64) -> Id {
    env.catalogue.open_session(name, ACCOUNT, Instant(ends));
    env.sessions.held.insert(
        name.into(),
        rutent_core_orch::intake::HeldSession {
            holder: customer().party,
            expires_at: rutent_contracts::router_api::Timestamp::new(
                Instant(ends).rfc3339().unwrap(),
            )
            .unwrap(),
        },
    );
    id(name)
}

/// Whether the log holds an `order_refusal` naming `order`.
fn ended_in_the_log(env: &Env, order: &Id) -> bool {
    env.book
        .borrow()
        .verified()
        .into_iter()
        .any(|w| matches!(w, What::OrderRefusal { order: Some(o), .. } if o == order))
}

/// A waiting filing whose session ends while its reserve is silent ends, recorded, and frees the deposit (re-review
/// finding 1, probe a).
// verifies: REQ-ORD-13, REQ-CAT-3
#[test]
fn a_waiting_filing_whose_session_ends_ends_and_frees_the_deposit() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let mut one = filing(reserve_filer(), &deposited);
    one.session = short_session(&mut env, "session-short", START + 30);
    providers.silent = true;
    let waiting = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    let Filed::Waiting {
        order: Some(order), ..
    } = waiting
    else {
        panic!("{waiting:?}");
    };
    // The session ends before the reserve's answer is due.
    env.clock.set(START + 40);
    let ended = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        matches!(ended, Filed::CantProceed { order: Some(ref o), .. } if o == &order),
        "{ended:?}"
    );
    assert!(ended_in_the_log(&env, &order));
    providers.silent = false;
    let mut again = filing(reserve_filer(), &deposited);
    again.request = id("deposit-2");
    let filed = file(&mut env, &mut providers, &mut arrivals, &again).unwrap();
    assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
    assert_admitted(&env);
}

/// A waiting filing whose reserve stays silent past its time to answer ends, recorded, and frees the deposit
/// (re-review finding 1, REQ-CAT-19).
// verifies: REQ-ORD-13, REQ-CAT-19
#[test]
fn a_waiting_filing_whose_reserve_stays_silent_past_its_time_ends() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let one = filing(reserve_filer(), &deposited);
    providers.silent = true;
    let waiting = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    let Filed::Waiting {
        order: Some(order), ..
    } = waiting
    else {
        panic!("{waiting:?}");
    };
    let asked = providers.verifications.len();
    env.clock.set(START + 120);
    let ended = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        matches!(ended, Filed::CantProceed { order: Some(ref o), .. } if o == &order),
        "{ended:?}"
    );
    assert_eq!(
        providers.verifications.len(),
        asked,
        "not asked again once past its time"
    );
    assert!(ended_in_the_log(&env, &order));
    // The deposit is free: another filing opens its own order.
    let mut again = filing(operator(), &deposited);
    again.request = id("deposit-2");
    let filed = file(&mut env, &mut providers, &mut arrivals, &again);
    assert!(
        !matches!(filed, Err(ArrivalError::Conflicting { .. })),
        "{filed:?}"
    );
    assert_admitted(&env);
}

/// A waiting filing is carried on by any listed login of its filer's party; once none is listed, it ends, recorded,
/// and frees the deposit (re-review finding 1, probe b).
// verifies: REQ-ORD-13, REQ-OPS-5
#[test]
fn a_waiting_filing_is_carried_on_by_its_partys_logins_and_ends_with_none_left() {
    let colleague = Actor {
        acts: id("operator-login-2"),
        ..operator()
    };
    // Carried on by another listed login of the party.
    let mut env = crate::env();
    env.actors.borrow_mut().list(&colleague);
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let one = filing(operator(), &deposited);
    providers.silent = true;
    assert!(matches!(
        file(&mut env, &mut providers, &mut arrivals, &one),
        Ok(Filed::Waiting { .. })
    ));
    providers.silent = false;
    let theirs = Filing {
        filer: colleague.clone(),
        ..one.clone()
    };
    let filed = file(&mut env, &mut providers, &mut arrivals, &theirs).unwrap();
    assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
    assert_admitted(&env);

    // Its filer struck from the register, with no other login of its party known: the reserve's filing ends it.
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let one = filing(operator(), &deposited);
    providers.silent = true;
    let Ok(Filed::Waiting {
        order: Some(order), ..
    }) = file(&mut env, &mut providers, &mut arrivals, &one)
    else {
        panic!("waits");
    };
    env.actors.borrow_mut().pairs.remove(&(
        operator().acts.as_str().to_owned(),
        operator().party.as_str().to_owned(),
        format!("{:?}", operator().kind),
    ));
    providers.silent = false;
    let mut theirs = filing(reserve_filer(), &deposited);
    theirs.request = id("deposit-reserve");
    let filed = file(&mut env, &mut providers, &mut arrivals, &theirs).unwrap();
    assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
    assert!(ended_in_the_log(&env, &order));
    assert_admitted(&env);
}

/// An order whose record the assembly was given, and which never opened, is withdrawn from it (re-review finding 2).
// verifies: REQ-ORD-13, REQ-AUZ-5
#[test]
fn an_order_that_never_opened_is_withdrawn_from_the_assembly() {
    use rutent_contracts::core_internal::{Answer, NotKnownCause};
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    env.policy.answer = Answer::NotKnown {
        cause: NotKnownCause::PolicyUnreachable,
    };
    let filed = file(
        &mut env,
        &mut providers,
        &mut arrivals,
        &filing(reserve_filer(), &deposited),
    )
    .unwrap();
    assert!(
        matches!(filed, Filed::Waiting { order: None, .. }),
        "{filed:?}"
    );
    let opened = OPENED.with(|o| o.borrow().clone());
    let withdrawn = WITHDRAWN.with(|w| w.borrow().clone());
    assert_eq!(opened.len(), 1);
    assert_eq!(withdrawn, opened, "withdrawn as it was opened");
}

/// Once the reserve said a deposit's funds aren't there, no filing on that deposit asks it again: five filings, one
/// question; a new notice is filed on (re-review finding 3).
// verifies: REQ-ORD-13, REQ-CAT-4
#[test]
fn a_deposit_its_reserve_said_no_to_is_filed_on_no_more() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    providers.insufficient = true;
    let first = file(
        &mut env,
        &mut providers,
        &mut arrivals,
        &filing(reserve_filer(), &deposited),
    )
    .unwrap();
    assert!(
        matches!(first, Filed::CantProceed { order: Some(_), .. }),
        "{first:?}"
    );
    for n in 0..5 {
        let mut again = filing(
            if n % 2 == 0 {
                operator()
            } else {
                reserve_filer()
            },
            &deposited,
        );
        again.request = id(&format!("deposit-again-{n}"));
        assert_eq!(
            file(&mut env, &mut providers, &mut arrivals, &again),
            Err(ArrivalError::NotFileable)
        );
    }
    assert_eq!(providers.verifications.len(), 1, "asked once");
    providers.insufficient = false;
    let fresh = deposit(&mut env, &mut providers, &mut arrivals, 1000, 1);
    let mut on_it = filing(reserve_filer(), &fresh);
    on_it.request = id("deposit-new-notice");
    let filed = file(&mut env, &mut providers, &mut arrivals, &on_it).unwrap();
    assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
    assert_admitted(&env);
}

/// An acceptance on a deposit is the only one: where the log refuses the start after it, the order ends and the
/// deposit is filed on no more (the re-review's nit).
// verifies: REQ-ORD-15, REQ-ORD-34
#[test]
fn a_deposit_with_an_acceptance_recorded_is_filed_on_no_more() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    env.book.borrow_mut().refuse_next = Some("input");
    let filed = file(
        &mut env,
        &mut providers,
        &mut arrivals,
        &filing(reserve_filer(), &deposited),
    )
    .unwrap();
    assert!(
        matches!(filed, Filed::CantProceed { order: Some(_), .. }),
        "{filed:?}"
    );
    assert_eq!(env.acceptances().len(), 1);
    let mut again = filing(operator(), &deposited);
    again.request = id("deposit-2");
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &again),
        Err(ArrivalError::NotFileable)
    );
    assert_eq!(
        env.acceptances().len(),
        1,
        "no second acceptance on the deposit"
    );
    assert_admitted(&env);
}

/// A permit holds no longer than the filing's session: a filing carried on after its session ended ends rather than
/// accept on it (the re-review's nit).
// verifies: REQ-AUZ-3, REQ-CAT-3
#[test]
fn a_filing_carried_on_after_its_session_ended_isnt_accepted() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let mut one = filing(reserve_filer(), &deposited);
    one.session = short_session(&mut env, "session-short", START + 30);
    env.book.borrow_mut().fail_next = Some("shown");
    assert!(file(&mut env, &mut providers, &mut arrivals, &one).is_err());
    env.clock.set(START + 40);
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        matches!(filed, Filed::CantProceed { order: Some(_), .. }),
        "{filed:?}"
    );
    assert!(env.acceptances().is_empty());
    assert_admitted(&env);
}

/// A notice the evidence store finds restates one taken before is the same arrival; with other figures it is refused
/// (the re-review's nit).
// verifies: REQ-ORD-14, INV-8
#[test]
fn a_restated_notice_is_the_same_arrival() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let place = env.store.borrow().kept.len();
    let first = report(1000, 0);
    let arrived = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &first,
    )
    .unwrap();
    // The same statement, in other bytes.
    let value: Value = serde_json::from_slice(&first.notice).unwrap();
    let restated = Report {
        notice: serde_json::to_vec_pretty(&value).unwrap(),
        ..first.clone()
    };
    assert_ne!(restated.notice, first.notice);
    let len = env.len();
    env.store.borrow_mut().restates_next = Some(place);
    let again = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &restated,
    )
    .unwrap();
    assert_eq!(again, arrived);
    assert_eq!(env.len(), len);
    let other = Report {
        notice: serde_json::to_vec_pretty(&value)
            .unwrap()
            .into_iter()
            .chain(*b" ")
            .collect(),
        amount: amount(900),
        ..first
    };
    env.store.borrow_mut().restates_next = Some(place);
    assert_eq!(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut Open::default(),
            &other
        ),
        Err(ArrivalError::OtherFigures)
    );
    assert_eq!(arrivals_recorded(&env).len(), 1);
}

/// A reserve's catalogue statements, its advertisement among them, aren't notices of value arriving (the re-review's
/// records).
// verifies: REQ-ORD-14
#[test]
fn a_reserves_advertisement_is_no_arrival_notice() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let ad = snapshot_support::reserve(RESERVE, "acme");
    let keys = snapshot_support::keys_of(RESERVE, &[&statements_key(RESERVE, 1)]);
    let advertisement = snapshot_support::advertisement_bytes(&ad, keys, START);
    let reported = Report {
        notice: advertisement,
        ..report(1000, 0)
    };
    let len = env.len();
    assert_eq!(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut Open::default(),
            &reported
        ),
        Err(ArrivalError::NotItsNotice)
    );
    assert_eq!(env.len(), len);
    // Nor a result of a kind that answers an operation, though it names no transaction.
    let judged = sign(
        envelope(
            &deployment(),
            RESULT_SHAPE,
            RESERVE,
            START,
            json!({ "kind": "judgement", "outcome": {} }),
        ),
        &statements_key(RESERVE, 1),
    );
    let reported = Report {
        notice: judged,
        ..report(1000, 1)
    };
    assert_eq!(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut Open::default(),
            &reported
        ),
        Err(ArrivalError::NotItsNotice)
    );
}

// Round 3 --------------------------------------------------------------------------------------------------------

/// A login carrying a filing on gives its own authorisation: one policy refuses carries nothing on and names nothing,
/// the acceptance naming the filer policy permitted and its authorisation; one policy permits is named by the
/// acceptance, with the authorisation it gave (round 3, finding 1; `REQ-AUZ-9`).
// verifies: REQ-AUZ-9, REQ-ORD-13
#[test]
fn a_login_carrying_a_filing_on_gives_its_own_authorisation() {
    use rutent_contracts::core_internal::{Answer, RefusalClass};
    let read_only = Actor {
        acts: id("operator-read-only-login"),
        ..operator()
    };
    for permitted in [false, true] {
        let mut env = crate::env();
        env.actors.borrow_mut().list(&read_only);
        let mut providers = Reserve::new(&env);
        let mut arrivals = Arrivals::new();
        let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
        let one = filing(operator(), &deposited);
        providers.silent = true;
        assert!(matches!(
            file(&mut env, &mut providers, &mut arrivals, &one),
            Ok(Filed::Waiting { order: Some(_), .. })
        ));
        providers.silent = false;
        let theirs = Filing {
            filer: read_only.clone(),
            ..one.clone()
        };
        if !permitted {
            // The login holds no permission to act for users: policy refuses it, and nothing moves on its call.
            env.policy.answer = refusal(RefusalClass::MissingInput);
            let len = env.len();
            let told = file(&mut env, &mut providers, &mut arrivals, &theirs);
            assert!(
                matches!(told, Err(ArrivalError::Refused { .. })),
                "{told:?}"
            );
            assert_eq!(env.policy.asked.len(), 2, "asked about the login");
            assert_eq!(env.len(), len + 1, "only policy's own entry");
            env.policy.answer = Answer::Permit {};
            // The filer policy permitted carries it on.
            let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
            assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
        } else {
            let filed = file(&mut env, &mut providers, &mut arrivals, &theirs).unwrap();
            assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
        }
        let (giver, who) = if permitted {
            (read_only.clone(), read_only.clone())
        } else {
            (operator(), operator())
        };
        let acceptance = env.acceptances().pop().unwrap();
        assert_eq!(
            acceptance.who, who,
            "the acceptance names the filer policy permitted"
        );
        let What::Acceptance { authorisation, .. } = &acceptance.what else {
            unreachable!()
        };
        let given = env
            .policy
            .asked
            .iter()
            .find(|a| &a.authorisation == authorisation)
            .expect("the acceptance names an authorisation policy was asked");
        assert_eq!(
            given.given_by, giver,
            "and that authorisation is its giver's"
        );
        assert_admitted(&env);
    }
}

/// A permitted filing that ended in all but name, its session over, is ended when another filer files on the deposit,
/// rather than hold it on `Conflicting` (round 3, finding 2).
// verifies: REQ-ORD-13, REQ-CAT-3
#[test]
fn a_quiet_filing_whose_session_ended_gives_way_to_another_filer() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let mut one = filing(operator(), &deposited);
    one.session = short_session(&mut env, "session-short", START + 30);
    providers.silent = true;
    let Ok(Filed::Waiting {
        order: Some(order), ..
    }) = file(&mut env, &mut providers, &mut arrivals, &one)
    else {
        panic!("waits");
    };
    providers.silent = false;
    env.clock.set(START + 40);
    let mut theirs = filing(reserve_filer(), &deposited);
    theirs.request = id("deposit-reserve");
    theirs.session = short_session(&mut env, "session-fresh", START + 10_000);
    let filed = file(&mut env, &mut providers, &mut arrivals, &theirs).unwrap();
    assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
    assert!(
        ended_in_the_log(&env, &order),
        "the quiet filing's order ended, recorded"
    );
    assert_admitted(&env);

    // Its reserve past its time to answer the question it left unanswered: it gives way too.
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let one = filing(operator(), &deposited);
    providers.silent = true;
    let Ok(Filed::Waiting {
        order: Some(order), ..
    }) = file(&mut env, &mut providers, &mut arrivals, &one)
    else {
        panic!("waits");
    };
    env.clock.set(START + 120);
    let mut theirs = filing(reserve_filer(), &deposited);
    theirs.request = id("deposit-reserve");
    let filed = file(&mut env, &mut providers, &mut arrivals, &theirs);
    assert!(
        !matches!(filed, Err(ArrivalError::Conflicting { .. })),
        "{filed:?}"
    );
    assert!(ended_in_the_log(&env, &order));
    assert_admitted(&env);
}

// Round 4 --------------------------------------------------------------------------------------------------------

/// The distinct orders the log holds a `route_chosen` of.
fn started_orders(env: &Env) -> std::collections::BTreeSet<String> {
    env.started().into_iter().collect()
}

/// A filing whose acceptance and start are in the log is finished, never ended: another filer is told `Conflicting`,
/// and its call finishes the filing's last entries, so the deposit carries one acceptance and one order (round 4).
// verifies: REQ-ORD-15, REQ-ORD-34, INV-8
#[test]
fn a_filing_with_its_acceptance_written_is_finished_not_ended() {
    for case in ["struck", "session"] {
        let mut env = crate::env();
        let mut providers = Reserve::new(&env);
        let mut arrivals = Arrivals::new();
        let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
        let mut one = filing(operator(), &deposited);
        if case == "session" {
            one.session = short_session(&mut env, "session-short", START + 30);
        }
        // The acceptance and the start are written; the start's answer is lost.
        env.book.borrow_mut().lose_next = Some("input");
        assert_eq!(
            file(&mut env, &mut providers, &mut arrivals, &one),
            Err(ArrivalError::Log(
                rutent_contracts::core_internal::calls::AppendFailure::Unavailable
            ))
        );
        let order = env.started().pop().expect("the start is in the log");
        match case {
            "struck" => {
                env.actors.borrow_mut().pairs.remove(&(
                    operator().acts.as_str().to_owned(),
                    operator().party.as_str().to_owned(),
                    format!("{:?}", operator().kind),
                ));
            }
            _ => env.clock.set(START + 40),
        }
        let mut theirs = filing(reserve_filer(), &deposited);
        theirs.request = id("deposit-reserve");
        theirs.session = short_session(&mut env, "session-fresh", START + 10_000);
        for _ in 0..2 {
            assert_eq!(
                file(&mut env, &mut providers, &mut arrivals, &theirs),
                Err(ArrivalError::Conflicting { order: id(&order) }),
                "{case}"
            );
        }
        assert_eq!(
            env.acceptances().len(),
            1,
            "{case}: one acceptance on the deposit"
        );
        assert_eq!(started_orders(&env).len(), 1, "{case}: one order started");
        assert!(
            arrivals.start(&id(&order)).is_some(),
            "{case}: finished by Core"
        );
        assert!(!ended_in_the_log(&env, &id(&order)), "{case}: never ended");
        assert_admitted(&env);
    }
}

/// Every login policy permitted on a filing carries it on without asking policy again (round 4's nit).
// verifies: REQ-AUZ-9, REQ-OPS-5
#[test]
fn logins_policy_permitted_carry_a_filing_on_without_asking_again() {
    let colleague = Actor {
        acts: id("operator-login-2"),
        ..operator()
    };
    let mut env = crate::env();
    env.actors.borrow_mut().list(&colleague);
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let one = filing(operator(), &deposited);
    let theirs = Filing {
        filer: colleague.clone(),
        ..one.clone()
    };
    providers.silent = true;
    for filing in [&one, &theirs, &one, &theirs] {
        assert!(matches!(
            file(&mut env, &mut providers, &mut arrivals, filing),
            Ok(Filed::Waiting { .. })
        ));
    }
    assert_eq!(env.policy.asked.len(), 2, "each login asked about once");
    providers.silent = false;
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
    let acceptance = env.acceptances().pop().unwrap();
    assert_eq!(acceptance.who, operator());
    let What::Acceptance { authorisation, .. } = &acceptance.what else {
        unreachable!()
    };
    let given = env
        .policy
        .asked
        .iter()
        .find(|a| &a.authorisation == authorisation)
        .unwrap();
    assert_eq!(
        given.given_by,
        operator(),
        "the acceptance names its own login's authorisation"
    );
    assert_admitted(&env);
}

// Round 5 --------------------------------------------------------------------------------------------------------

/// One deposit opens one order (`one-order-per-deposit`): once a filing held the deposit against its order, whatever
/// comes of that order, even an acceptance the log refused on its first append, the deposit is filed on no more
/// (integration with the contract head; supersedes round 5's free deposit).
// verifies: REQ-ORD-23, REQ-ORD-14, INV-8
#[test]
fn one_deposit_opens_one_order_whatever_comes_of_it() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    env.book.borrow_mut().refuse_next = Some("acceptance");
    let filed = file(
        &mut env,
        &mut providers,
        &mut arrivals,
        &filing(operator(), &deposited),
    )
    .unwrap();
    assert!(
        matches!(filed, Filed::CantProceed { order: Some(_), .. }),
        "{filed:?}"
    );
    assert!(env.acceptances().is_empty(), "no acceptance written");
    let mut theirs = filing(reserve_filer(), &deposited);
    theirs.request = id("deposit-reserve");
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &theirs),
        Err(ArrivalError::NotFileable)
    );
    let held: Vec<_> = arrivals_recorded(&env)
        .into_iter()
        .filter(|(o, _, _)| o.is_some())
        .collect();
    assert_eq!(held.len(), 1, "one arrival names an order");
    assert_admitted(&env);
}

/// An acceptance written whose answer was lost may be in the log: if its filing then ends, the deposit is filed on
/// no more, so no second acceptance rests on it (round 5).
// verifies: REQ-ORD-15, REQ-ORD-34
#[test]
fn an_acceptance_whose_answer_was_lost_spends_the_deposit_if_its_filing_ends() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let one = filing(operator(), &deposited);
    env.book.borrow_mut().lose_next = Some("acceptance");
    assert!(file(&mut env, &mut providers, &mut arrivals, &one).is_err());
    assert_eq!(env.acceptances().len(), 1, "written, its answer lost");
    env.book.borrow_mut().refuse_next = Some("acceptance");
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert!(
        matches!(filed, Filed::CantProceed { order: Some(_), .. }),
        "{filed:?}"
    );
    let mut theirs = filing(reserve_filer(), &deposited);
    theirs.request = id("deposit-reserve");
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &theirs),
        Err(ArrivalError::NotFileable)
    );
    assert_eq!(env.acceptances().len(), 1);
    assert_admitted(&env);
}

// The contract head's arrival rows --------------------------------------------------------------------------------

/// The notice an arrival names, by its commitment.
fn notice_of(env: &Env, receipt: &Receipt) -> Option<rutent_contracts::module_interface::Digest> {
    match at(env, receipt) {
        Entry::Verified(v) => match v.what {
            What::Arrival { notice, .. } => notice,
            _ => None,
        },
        _ => None,
    }
}

/// A deposit is known by the transaction its reserve's notice signs, whatever bytes carry it, and every arrival of it
/// names that notice, so the log holds one order per deposit (`arrival-names-its-notice`, `one-order-per-deposit`).
// verifies: REQ-ORD-23, REQ-ORD-14, INV-8
#[test]
fn a_deposit_is_known_by_its_transaction_and_every_arrival_names_its_notice() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let reported = report(1000, 0);
    let first = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &reported,
    )
    .unwrap();
    let Arrived::NoOrder { arrival } = first.clone() else {
        panic!("{first:?}");
    };
    // The notice is about the deposit's transaction alone, and the arrival names it by its commitment.
    let stated_notice = entries(&env)
        .into_iter()
        .find_map(|e| match e {
            Entry::Stated(s) if s.what.statement.as_str() == NOTICE_SHAPE => Some(s),
            _ => None,
        })
        .expect("the notice is recorded");
    let about = stated_notice.about.clone().unwrap();
    assert_eq!(about.transaction, Some(id(&credit(0))));
    assert_eq!(about.order, None);
    assert_eq!(
        notice_of(&env, &arrival),
        Some(stated_notice.commitment.clone())
    );
    // The same deposit in other bytes is the same arrival, nothing kept or recorded again; with other figures it is
    // refused.
    let value: Value = serde_json::from_slice(&reported.notice).unwrap();
    let reencoded = Report {
        notice: serde_json::to_vec_pretty(&value).unwrap(),
        ..reported.clone()
    };
    let (kept, len) = (env.store.borrow().kept.len(), env.len());
    assert_eq!(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut Open::default(),
            &reencoded
        ),
        Ok(first)
    );
    assert_eq!((env.store.borrow().kept.len(), env.len()), (kept, len));
    let other = Report {
        amount: amount(999),
        ..reencoded
    };
    assert_eq!(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut Open::default(),
            &other
        ),
        Err(ArrivalError::OtherFigures)
    );
    // The order filed on it names the same notice; so does an arrival held at once against an open order.
    let filed = file(
        &mut env,
        &mut providers,
        &mut arrivals,
        &filing(reserve_filer(), &arrival),
    )
    .unwrap();
    let Filed::Started {
        order: automatic, ..
    } = filed
    else {
        panic!("{filed:?}");
    };
    let (_, _, _) = arrivals_recorded(&env)
        .into_iter()
        .find(|(o, _, _)| o.as_ref() == Some(&automatic))
        .unwrap();
    let named: Vec<_> = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::Arrival { order, notice, .. } => Some((order.clone(), notice.clone())),
            _ => None,
        })
        .collect();
    assert!(
        named
            .iter()
            .all(|(_, n)| n.as_ref() == Some(&stated_notice.commitment))
    );
    let explicit = explicit_order(&mut env, &mut providers);
    let held = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open(vec![open_order(&explicit)]),
        &report(600, 1),
    )
    .unwrap();
    let Arrived::Held { arrival: held, .. } = held else {
        panic!("{held:?}");
    };
    assert!(notice_of(&env, &held).is_some());
    assert_ne!(notice_of(&env, &held), Some(stated_notice.commitment));
    assert_admitted(&env);
}

/// An arrival held against a part of a split order names the part (`split-deposit-arrival-names-its-part`).
// verifies: REQ-ORD-15, REQ-ORD-23
#[test]
fn an_arrival_held_against_a_part_names_the_part() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let mut part = open_order(&order);
    part.part = Some(id("part-2"));
    let held = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open(vec![part]),
        &report(600, 0),
    )
    .unwrap();
    let Arrived::Held { arrival, .. } = held else {
        panic!("{held:?}");
    };
    let Entry::Verified(v) = at(&env, &arrival) else {
        panic!("an arrival is verified");
    };
    let What::Arrival { part, .. } = v.what else {
        unreachable!()
    };
    assert_eq!(part, Some(id("part-2")));
    assert_admitted(&env);
}
