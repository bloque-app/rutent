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
use rutent_contracts::core_internal::{Count, WaitEnd};
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
    /// The maker its answers to them name, where another provider's than the reserve's: a relay's copy.
    speaks_as: Option<&'static str>,
    /// What the reserve's book holds for the holder's account, where a test keeps one: its answers to balance
    /// verifications follow it, and a question that moves value (a withdrawal or a transfer that isn't a dry run)
    /// debits it, so a Core that moved the holder's funds would be read here. `None` keeps no book, and every verification
    /// is answered by `insufficient` alone.
    book: Option<u64>,
}

impl Reserve {
    fn new(env: &Env) -> Self {
        Self {
            sim: env.sim(),
            insufficient: false,
            silent: false,
            verifications: Vec::new(),
            speaks_as: None,
            book: None,
        }
    }

    /// The reserve's book of the holder's account: what it holds for it, which Core can read only by asking.
    fn balance(&self) -> u64 {
        self.book.expect("the test keeps a book")
    }

    /// What the reserve answers a verification that the holder's account holds `units`, as Core is answered: its
    /// signed answer read for its `sufficient`.
    fn confirms(&mut self, units: u64) -> bool {
        let question = json!({ "statement": {
            "kind": "balance_verification",
            "transaction": format!("read-the-book-of-{units}"),
            "amount": units.to_string(),
            "session": SESSION,
            "account": ACCOUNT,
        } });
        let bytes = self
            .ask(&id(RESERVE), &serde_json::to_vec(&question).unwrap())
            .expect("the reserve answers");
        let answer: Value = serde_json::from_slice(&bytes).unwrap();
        answer["statement"]["outcome"]["sufficient"]
            .as_bool()
            .unwrap()
    }

    /// Whether the reserve's book covers `amount`, as it answers a balance verification (without a book, always).
    fn covers(&self, amount: u64) -> bool {
        self.book.is_none_or(|held| amount <= held)
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
            // A question that moves value (a withdrawal or a transfer not asked as a dry run) takes it out of the
            // reserve's book, if the test keeps one: the double is what a move out of the holder's account reaches.
            if matches!(operation["kind"].as_str(), Some("withdrawal" | "transfer"))
                && operation["dry_run"] == false
                && let (Some(held), Some(amount)) = (
                    self.book.as_mut(),
                    operation["amount"]
                        .as_str()
                        .and_then(|a| a.parse::<u64>().ok()),
                )
            {
                *held = held.saturating_sub(amount);
            }
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
            "outcome": {
                "status": "answered",
                "sufficient": !self.insufficient
                    && self.covers(operation["amount"].as_str().unwrap().parse().unwrap()),
            },
        });
        let maker = self.speaks_as.unwrap_or(provider.as_str());
        let answer = envelope(&deployment(), RESULT_SHAPE, maker, now, body);
        Some(sign(answer, &statements_key(maker, 1)))
    }
}

/// The shape the tests' reserve gives its notices: provider-role's arrival notice, the only shape the log takes as a
/// deposit's notice (`arrival-names-its-notice`), and the one Core requires (`T-020-19`).
const NOTICE_SHAPE: &str = rutent_contracts::provider_role::ARRIVAL_NOTICE_SHAPE;

/// The transaction the reserve gives the `n`th credit: its per-credit reference.
fn credit(n: u32) -> String {
    format!("credit-{n}")
}

/// What the reserve's notice states of one credit: provider-role's `ArrivalNotice`, the contract's own body.
#[derive(Clone)]
struct Credit {
    n: u32,
    units: u64,
    node: OuterNode,
    asset: Denomination,
    account: String,
    holder: Id,
}

impl Credit {
    /// `units` of USDC arriving at polygon, at [`ACCOUNT`] of the customer, under the reference of the `n`th credit.
    fn new(units: u64, n: u32) -> Self {
        Self {
            n,
            units,
            node: polygon(),
            asset: usdc(),
            account: ACCOUNT.into(),
            holder: customer().party,
        }
    }

    /// The body the contract names for a notice of this credit.
    fn body(&self) -> Value {
        json!({
            "transaction": credit(self.n),
            "node": self.node,
            "asset": self.asset,
            "amount": self.units.to_string(),
            "account": self.account,
            "holder": self.holder,
        })
    }

    /// The notice, signed by `maker`'s statements key, under `shape`, made at `made_at`.
    fn notice_as(&self, shape: &str, maker: &str, made_at: u64) -> Vec<u8> {
        let value = envelope(&deployment(), shape, maker, made_at, self.body());
        sign(value, &statements_key(RESERVE, 1))
    }

    /// The reserve's notice of the credit, signed.
    fn notice(&self) -> Vec<u8> {
        self.notice_as(NOTICE_SHAPE, RESERVE, START)
    }

    /// The report of the notice.
    fn report(&self) -> Report {
        reported(self.notice())
    }
}

/// A report of `units` arriving at [`RESERVE`] to the customer's [`ACCOUNT`].
fn report(units: u64, n: u32) -> Report {
    Credit::new(units, n).report()
}

/// The report of the reserve's `notice`, as received.
fn reported(notice: Vec<u8>) -> Report {
    Report {
        reserve: id(RESERVE),
        notice,
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
            let report = report(*units, n as u32);
            if elsewhere != "none" {
                other_amount(&mut open, *units);
            }
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
        let What::Refiling {
            arrival: filed,
            order: against,
            decision,
        } = &v.what
        else {
            panic!("a refiling");
        };
        assert_eq!((filed, against), (arrival, &order));
        // It rests on policy's permit of this refiling, a decision the log holds (`T-020-52`).
        assert!(
            matches!(at(&env, decision), Entry::Verified(d) if matches!(&d.what, What::Decide { .. }))
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

/// A login listed for the operator's party, or for a reserve, in another role than the one it states, files nothing
/// anew: the register of actors is asked for the role the filer states, as policy binds it (`T-021-14`), so a customer's
/// login of the operator's party can't state itself the operator, nor a customer's login of a reserve state itself the
/// reserve (`T-020-52`). The refusal is the same as a stranger's, and nothing is recorded.
// verifies: REQ-AUZ-4
#[test]
fn arrival_refiled_by_a_login_stating_a_role_its_party_does_not_hold_is_refused() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let first = match arrive(
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
    // Each of the two parties has a login the register lists in the customer's role alone; it states the role the
    // party's own filer holds.
    let operators_customer = Actor {
        acts: id("operators-customer-login"),
        role: Role::Customer,
        kind: ActorKind::Person,
        ..operator()
    };
    let reserves_customer = Actor {
        acts: id("reserves-customer-login"),
        role: Role::Customer,
        kind: ActorKind::Person,
        ..reserve_filer()
    };
    env.actors.borrow_mut().list(&operators_customer);
    env.actors.borrow_mut().list(&reserves_customer);
    let stating_operator = Actor {
        role: Role::Operator,
        ..operators_customer
    };
    let stating_provider = Actor {
        role: Role::Provider,
        ..reserves_customer
    };
    let len = env.len();
    for liar in [stating_operator, stating_provider] {
        assert_eq!(
            refile(&mut env, &mut providers, &mut arrivals, &liar, &first),
            Err(ArrivalError::NotTheFiler),
            "{liar:?}"
        );
    }
    assert_eq!(env.len(), len, "nothing is recorded for a refused filer");
    assert!(
        !env.book
            .borrow()
            .verified()
            .iter()
            .any(|w| matches!(w, What::Refiling { .. })),
        "no refiling is written"
    );
    // The operator's own login and the reserve's, in the roles they are listed in, file.
    assert!(refile(&mut env, &mut providers, &mut arrivals, &operator(), &first).is_ok());
    assert_admitted(&env);
}

/// A refiling is decided by policy before it is recorded, and the entry cites the permit (`T-020-52`): a login the
/// register lists for its role, but whose party policy doesn't let file, is refused with no refiling appended, and
/// told of its class; policy's refusal is its own record. Filed once permitted, the entry names the permitting
/// decision, which asks for this arrival, order and filer, and the log admits it.
// verifies: REQ-ORD-23, REQ-AUZ-1
#[test]
fn arrival_refiled_by_a_login_policy_does_not_let_file_is_refused_and_nothing_is_appended() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let first = match arrive(
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
    env.deciding.refuse = true;
    let refiled_before = |env: &Env| {
        env.book
            .borrow()
            .verified()
            .iter()
            .filter(|w| matches!(w, What::Refiling { .. }))
            .count()
    };
    for filer in [operator(), reserve_filer()] {
        assert_eq!(
            refile(&mut env, &mut providers, &mut arrivals, &filer, &first),
            Err(ArrivalError::Refused {
                class: OrderRefusalClass::RefusedByPolicy
            }),
            "{filer:?}"
        );
    }
    assert_eq!(refiled_before(&env), 0, "no refiling is appended");
    assert_eq!(env.deciding.asked.len(), 2, "policy was asked each time");
    assert_admitted(&env);

    // Permitted, the refiling cites the permit that decided it.
    env.deciding.refuse = false;
    let receipt = refile(&mut env, &mut providers, &mut arrivals, &operator(), &first).unwrap();
    let Entry::Verified(v) = at(&env, &receipt) else {
        panic!("a refiling is a verified entry");
    };
    let What::Refiling { decision, .. } = &v.what else {
        panic!("a refiling");
    };
    let Entry::Verified(decided) = at(&env, decision) else {
        panic!("the permit is a verified entry");
    };
    assert!(matches!(
        &decided.what,
        What::Decide { request, .. }
            if matches!(&request.action, rutent_contracts::core_internal::Action::RefileArrival { arrival, filer, .. }
                if arrival == &first && filer == &operator())
    ));
    assert_eq!(
        decided.outcome,
        rutent_contracts::core_internal::Outcome::Permitted {}
    );
    assert_eq!(refiled_before(&env), 1);
    assert_admitted(&env);
}

/// A refiling the log holds is rebuilt when Core reopens: the same filer filing the same arrival anew after a restart is
/// told the refiling it made, and appends neither a second `Refiling` nor a second decision; another filer's refiling
/// is its own, asked of policy and appended once (`T-020-62`; the milestone reviewer's check of `T-020-52`).
// verifies: NFR-6, REQ-AUZ-1
#[test]
fn refiling_after_a_restart_is_not_filed_twice() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let first = match arrive(
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
    let refiled = refile(&mut env, &mut providers, &mut arrivals, &operator(), &first).unwrap();
    let count = |env: &Env| {
        let book = env.book.borrow();
        let whats = book.verified();
        (
            whats
                .iter()
                .filter(|w| matches!(w, What::Refiling { .. }))
                .count(),
            whats
                .iter()
                .filter(|w| matches!(w, What::Decide { request, .. }
                    if matches!(request.action, rutent_contracts::core_internal::Action::RefileArrival { .. })))
                .count(),
        )
    };
    assert_eq!(count(&env), (1, 1));

    // Core stops and opens again over its log.
    let history = env.history();
    let mut reopened = reopened_over(&mut env, &mut providers, &history);
    let len = env.len();
    assert_eq!(
        refile(&mut env, &mut providers, &mut reopened, &operator(), &first),
        Ok(refiled),
        "the same filer is told the refiling it made"
    );
    assert_eq!(env.len(), len, "nothing is appended");
    assert_eq!(
        count(&env),
        (1, 1),
        "no second refiling, no second decision"
    );

    // The reserve, which never refiled it, has its own refiling decided and appended once.
    let by_reserve = refile(
        &mut env,
        &mut providers,
        &mut reopened,
        &reserve_filer(),
        &first,
    )
    .unwrap();
    assert_eq!(count(&env), (2, 2));
    assert_eq!(
        refile(
            &mut env,
            &mut providers,
            &mut reopened,
            &reserve_filer(),
            &first
        ),
        Ok(by_reserve)
    );
    assert_eq!(count(&env), (2, 2));
    assert_admitted(&env);
}

/// The arrival filing draws its order's identifier from the same source, and never one a refused request's record could
/// take (`request-` and a caller's suffix): a source whose every other draw begins so still opens the order under another
/// name, and one that only does opens none (`T-020-64`).
// verifies: REQ-AUZ-8, NFR-6
#[test]
fn a_filed_order_is_never_drawn_a_name_a_refused_requests_record_could_take() {
    for always in [false, true] {
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
        env.ids.alternate = !always;
        env.ids.always = always;
        let filed = file(
            &mut env,
            &mut providers,
            &mut arrivals,
            &filing(reserve_filer(), &deposited),
        );
        if always {
            assert_eq!(filed, Err(ArrivalError::Fresh));
        } else {
            let Ok(Filed::Started { order, .. }) = filed else {
                panic!("{filed:?}");
            };
            assert!(!order.as_str().starts_with("request-"), "{order:?}");
        }
    }
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
    // Policy's register of sessions unread: the filing is refused for its missing input, not as a session that isn't
    // held (`T-021-17`, `T-020-52`), and nothing is asked.
    env.sessions.unread = true;
    let mut unread = filing(reserve_filer(), &deposited);
    unread.request = id("deposit-6");
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &unread),
        Ok(Filed::CantProceed {
            order: None,
            class: OrderRefusalClass::MissingInput
        })
    );
    env.sessions.unread = false;
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
    deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let other = report(900, 0);
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
    let stranger = Credit {
        holder: id("a-stranger"),
        ..Credit::new(1000, 2)
    }
    .report();
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

/// The entry Core builds for a deposit states what the reserve's kept notice states (`T-020-50`, gap 10 of `T-020-45`):
/// a filing on a deposit whose notice the store no longer keeps is refused before an order opens, and goes through once
/// the notice is kept, since the log can't read a notice's body to hold the entry to it.
// verifies: REQ-ORD-13, REQ-AUD-2
#[test]
fn filing_on_a_deposit_whose_kept_notice_is_erased_or_replaced_is_refused_before_an_order_opens() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    env.kept.forgets = true;
    let filed = file(
        &mut env,
        &mut providers,
        &mut arrivals,
        &filing(reserve_filer(), &deposited),
    )
    .unwrap();
    assert!(
        matches!(
            filed,
            Filed::CantProceed {
                order: None,
                class: OrderRefusalClass::MalformedRequest
            }
        ),
        "{filed:?}"
    );
    assert!(env.started().is_empty());
    assert!(env.policy.asked.is_empty(), "no order opened");
    env.kept.forgets = false;
    // A notice replaced in the store: the store answers another statement (another holder) under the commitment of the
    // one kept, and it agrees with nothing either.
    let kept_notice = env
        .store
        .borrow()
        .kept
        .iter()
        .find(|b| String::from_utf8_lossy(b).contains("arrival-notice"))
        .cloned()
        .expect("the notice is kept");
    let mut other: serde_json::Value = serde_json::from_slice(&kept_notice).unwrap();
    other["statement"]["holder"] = "someone-else".into();
    env.kept.replaces = Some(serde_json::to_vec(&other).unwrap());
    let mut replaced = filing(reserve_filer(), &deposited);
    replaced.request = id("deposit-replaced");
    let filed = file(&mut env, &mut providers, &mut arrivals, &replaced).unwrap();
    assert!(
        matches!(
            filed,
            Filed::CantProceed {
                order: None,
                class: OrderRefusalClass::MalformedRequest
            }
        ),
        "{filed:?}"
    );
    assert!(env.started().is_empty());
    assert!(env.policy.asked.is_empty(), "no order opened");
    env.kept.replaces = None;
    let mut again = filing(reserve_filer(), &deposited);
    again.request = id("deposit-2");
    let filed = file(&mut env, &mut providers, &mut arrivals, &again).unwrap();
    assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
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

/// [`short_session`], opened through policy too: the log holds its `open_session`, which an acceptance given in it rests
/// on.
fn opened_session(env: &mut Env, name: &str, ends: u64) -> Id {
    let session = short_session(env, name, ends);
    env.session_opened_as(&customer().party, name, ends);
    session
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
    env.actors.borrow_mut().unlist(&operator());
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
    let nine_hundred: Value = serde_json::from_slice(&report(900, 0).notice).unwrap();
    let other = Report {
        notice: serde_json::to_vec_pretty(&nine_hundred)
            .unwrap()
            .into_iter()
            .chain(*b" ")
            .collect(),
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
                env.actors.borrow_mut().unlist(&operator());
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
    let other = report(999, 0);
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

// REQ-QTE-10, AC-13 (T-020-41: the deposit waits for its customer) -----------------------------------------------------

/// Prices the reserve's deposit at `seconds` to settle: over three days it leaves its route without a firm rate.
fn slow(env: &mut Env, seconds: u64) {
    for price in env.prices.iter_mut() {
        if price.tuple == snapshot_support::deposit_tuple(RESERVE) {
            price.settlement_seconds = seconds.try_into().unwrap();
        }
    }
}

/// The customer accepts the slow route of the filed order `order`, the quote of `digest`, in the session the deposit was
/// filed in.
fn accept_slow(
    env: &mut Env,
    providers: &mut Reserve,
    arrivals: &mut Arrivals,
    order: &Id,
    by: &Actor,
    digest: &rutent_contracts::module_interface::Digest,
) -> Result<Filed, ArrivalError> {
    accept_in(env, providers, arrivals, order, by, &id(SESSION), digest)
}

/// The customer accepts the slow route of the filed order `order`, the quote of `digest`, in `session`.
fn accept_in(
    env: &mut Env,
    providers: &mut Reserve,
    arrivals: &mut Arrivals,
    order: &Id,
    by: &Actor,
    session: &Id,
    digest: &rutent_contracts::module_interface::Digest,
) -> Result<Filed, ArrivalError> {
    env.with_providers(providers, |_, desk| {
        arrivals.accept::<RouteFinder>(desk, order, by, session, digest)
    })
}

/// A slow deposit of 1 000 filed by the reserve: the deposit, its filing, the order it waits as, and its quote's digest.
fn waiting_slow(
    env: &mut Env,
    providers: &mut Reserve,
    arrivals: &mut Arrivals,
) -> (Filing, Id, rutent_contracts::module_interface::Digest) {
    slow(env, 4 * 86_400);
    let deposited = deposit(env, providers, arrivals, 1000, 0);
    let one = filing(reserve_filer(), &deposited);
    let filed = file(env, providers, arrivals, &one).unwrap();
    let Filed::Waiting {
        order: Some(order),
        wait: FiledWait::CustomerAcceptance,
    } = filed
    else {
        panic!("{filed:?}");
    };
    let (_, digest, _, _) = awaited(env, arrivals, &order).expect("its quote is shown");
    (one, order, digest)
}

/// The quote `order` waits on its customer to accept, as of the environment's clock: the quote, its digest, until when
/// it stands and whether it stopped standing.
fn awaited(
    env: &Env,
    arrivals: &Arrivals,
    order: &Id,
) -> Option<(
    rutent_core_orch::intake::Quote,
    rutent_contracts::module_interface::Digest,
    Option<rutent_contracts::router_api::Timestamp>,
    bool,
)> {
    let now = env.clock.now().timestamp().unwrap();
    arrivals
        .awaiting(order, &now)
        .map(|a| (a.quote.clone(), a.digest.clone(), a.valid_until, a.expired))
}

/// A deposit filed automatically whose only route has a segment over three days has no firm rate, which only the
/// customer's acceptance may start (`REQ-QTE-10`), and an automatic deposit isn't the customer's: the filing shows the
/// quote with its slow segments named, records the order as waiting for its customer, and starts nothing. Filing again
/// is told the same and records nothing.
// verifies: AC-13, REQ-QTE-10
#[test]
fn automatic_deposit_whose_only_route_has_no_firm_rate_waits_for_its_customer() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let (one, order, digest) = waiting_slow(&mut env, &mut providers, &mut arrivals);

    let (quote, shown_digest, valid_until, expired) =
        awaited(&env, &arrivals, &order).expect("its quote");
    assert_eq!(shown_digest, digest);
    // The customer is shown until when the quote stands, and it stands.
    assert!(valid_until.is_some() && !expired);
    assert_eq!(
        quote.shown_as,
        rutent_contracts::core_internal::ShownAs::QuoteWithoutFirmRate
    );
    assert!(
        !quote.price.no_firm_rate.is_empty(),
        "the slow segments are named"
    );
    assert!(
        quote
            .price
            .no_firm_rate
            .iter()
            .all(|s| s.provider == id(RESERVE) && s.declared_seconds == Some(4 * 86_400)),
        "{:?}",
        quote.price.no_firm_rate
    );
    let log = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    assert!(log.iter().any(|w| matches!(
        w,
        What::Shown { order: o, shown, shown_as: rutent_contracts::core_internal::ShownAs::QuoteWithoutFirmRate, awaiting: Some(_) }
            if o == &order && shown == &digest
    )));
    // The log says whom it waits for, until when and which segments are slow, for the customer's notice.
    let told = log
        .iter()
        .find_map(|w| match w {
            What::Shown {
                awaiting: Some(a), ..
            } => Some(a.clone()),
            _ => None,
        })
        .expect("the shown quote awaits its customer");
    assert_eq!(told.requester, customer().party);
    assert_eq!(told.valid_until, valid_until);
    let places: Vec<u64> = told.slow.iter().map(|s| u64::from(s.position)).collect();
    // Each slow segment by its place in the route shown, from 1.
    let expected: Vec<u64> = quote
        .price
        .no_firm_rate
        .iter()
        .map(|slow| {
            quote
                .price
                .segments
                .iter()
                .position(|s| s.part == slow.part && s.operation == slow.operation)
                .expect("a slow segment of the route") as u64
                + 1
        })
        .collect();
    assert_eq!(places, expected);
    assert!(env.acceptances().is_empty(), "no acceptance on the deposit");
    assert!(env.started().is_empty(), "nothing started");
    assert!(
        !ended_in_the_log(&env, &order),
        "the order waits, it doesn't end"
    );
    assert_admitted(&env);

    let before = env.len();
    let again = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    assert_eq!(
        again,
        Filed::Waiting {
            order: Some(order),
            wait: FiledWait::CustomerAcceptance
        }
    );
    assert_eq!(env.len(), before, "nothing recorded");
    // The deposit is held against the order: another filing of it opens nothing.
    let mut other = filing(reserve_filer(), &one.arrival);
    other.request = id("deposit-2");
    assert!(matches!(
        file(&mut env, &mut providers, &mut arrivals, &other),
        Err(ArrivalError::Conflicting { .. })
    ));
    assert_admitted(&env);
}

/// The customer's acceptance of the slow route, given in the session the deposit was filed in, starts it: recorded on
/// `given_on_deposit`, its `who` the customer, naming the quote shown and the session, with no yes; only the order's
/// customer, only for the quote shown.
// verifies: REQ-QTE-10, REQ-ORD-15
#[test]
fn customers_acceptance_of_the_slow_route_starts_it() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let (one, order, digest) = waiting_slow(&mut env, &mut providers, &mut arrivals);

    // Not the quote shown: nothing is recorded.
    let before = env.len();
    let other: rutent_contracts::module_interface::Digest =
        serde_json::from_value(json!({ "algorithm": "sha-256", "value": "77".repeat(32) }))
            .unwrap();
    assert_eq!(
        accept_slow(
            &mut env,
            &mut providers,
            &mut arrivals,
            &order,
            &customer(),
            &other
        ),
        Err(ArrivalError::NotTheQuoteShown)
    );
    // Not the order's customer: the reserve filing it, an unlisted login of its party, or a listed customer of
    // another party.
    let other_party = Actor {
        party: id("another-customer"),
        acts: id("another-customer"),
        role: Role::Customer,
        kind: ActorKind::Person,
    };
    env.actors.borrow_mut().list(&other_party);
    for who in [reserve_filer(), impostor(), other_party] {
        assert_eq!(
            accept_slow(
                &mut env,
                &mut providers,
                &mut arrivals,
                &order,
                &who,
                &digest
            ),
            Err(ArrivalError::NotTheFiler),
            "{who:?}"
        );
    }
    assert_eq!(env.len(), before, "nothing recorded");
    assert!(env.started().is_empty());

    let accepted = accept_slow(
        &mut env,
        &mut providers,
        &mut arrivals,
        &order,
        &customer(),
        &digest,
    )
    .unwrap();
    let Filed::Started {
        order: started,
        acceptance,
    } = accepted
    else {
        panic!("{accepted:?}");
    };
    assert_eq!(started, order);
    let acceptances = env.acceptances();
    assert_eq!(acceptances.len(), 1);
    let What::Acceptance {
        acceptance: id_of,
        order: accepted_order,
        quote,
        basis,
        yeses,
        session,
        held,
        ..
    } = &acceptances[0].what
    else {
        unreachable!()
    };
    assert_eq!(
        (
            id_of,
            accepted_order,
            quote,
            basis,
            yeses.len(),
            session,
            held
        ),
        (
            &acceptance,
            &order,
            &digest,
            &AcceptanceBasis::GivenOnDeposit,
            0,
            &Some(id(SESSION)),
            &None
        )
    );
    assert_eq!(acceptances[0].who, customer());
    assert_eq!(env.started(), [order.as_str()], "the route started");
    // The same acceptance again is told the same and records nothing.
    let before = env.len();
    assert_eq!(
        accept_slow(
            &mut env,
            &mut providers,
            &mut arrivals,
            &order,
            &customer(),
            &digest
        ),
        Ok(Filed::Started {
            order: order.clone(),
            acceptance
        })
    );
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &one).unwrap(),
        Filed::Started {
            order,
            acceptance: match &acceptances[0].what {
                What::Acceptance { acceptance, .. } => acceptance.clone(),
                _ => unreachable!(),
            }
        }
    );
    assert_eq!(env.len(), before);
    assert_admitted(&env);
}

/// While the filed order waits nothing is authorised to move: no effect, no start, no exposure, and the reserve was
/// asked nothing that moves value. The funds stay in the holder's account at the reserve, whatever comes of the wait:
/// the filing's session ending doesn't end it, and its lifetime ending does, moving nothing either (`GUA-1`,
/// `T-020-51`). The reserve's book is read at each step, by its balance and by the answers it gives Core's
/// verifications of the amount and of one unit more.
// verifies: GUA-1
#[test]
fn nothing_moves_while_the_filed_order_waits_and_the_funds_stay_with_the_holder() {
    // The wait ends two ways once its lifetime is over: the filing is filed again, or the customer accepts.
    for customer_accepts in [false, true] {
        let mut env = crate::env();
        let mut providers = Reserve::new(&env);
        // The deposit credits the holder's account at the reserve; the book is the reserve's own.
        providers.book = Some(1000);
        let mut arrivals = Arrivals::new().with_deposit_wait(Count::new(1_800).unwrap());
        slow(&mut env, 4 * 86_400);
        let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
        let mut one = filing(reserve_filer(), &deposited);
        one.session = short_session(&mut env, "session-short", START + 600);
        let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
        let Filed::Waiting {
            order: Some(order), ..
        } = filed
        else {
            panic!("{filed:?}");
        };
        let (_, digest, _, _) = awaited(&env, &arrivals, &order).expect("its quote");
        let moved = |env: &Env| {
            env.book
                .borrow()
                .verified()
                .into_iter()
                .filter(|w| {
                    matches!(
                        w,
                        What::Effect { .. } | What::Input { .. } | What::Acceptance { .. }
                    )
                })
                .count()
        };
        // The reserve's book: the holder's 1 000 are there, all of them and no more, and what Core asked of it
        // took none (the questions it was asked are verifications, which a book answers and does not change).
        let book = |providers: &mut Reserve, when: &str| {
            assert_eq!(
                providers.balance(),
                1000,
                "{when}: the funds stay with the holder"
            );
            assert!(providers.confirms(1000), "{when}: all of it is there");
            assert!(!providers.confirms(1001), "{when}: and no more than it");
        };
        assert_eq!(moved(&env), 0);
        assert_eq!(env.exposures.held().count(), 0, "nothing held in flight");
        book(&mut providers, "filed, waiting");

        // Waiting, the filing is told it waits, and still does once the session it was filed in is over.
        for when in [300, 700] {
            env.clock.set(START + when);
            assert!(matches!(
                file(&mut env, &mut providers, &mut arrivals, &one).unwrap(),
                Filed::Waiting {
                    wait: FiledWait::CustomerAcceptance,
                    ..
                }
            ));
            assert!(!ended_in_the_log(&env, &order), "{when}");
            assert_eq!(moved(&env), 0);
            book(&mut providers, "waiting still");
        }

        // Its lifetime over, the wait ends the order, recorded, and still nothing moved: a customer accepting then
        // is told it ended, and no acceptance is recorded.
        env.clock.set(seconds(&arrivals.wait_ends(&order).unwrap()));
        let ended = if customer_accepts {
            accept_slow(
                &mut env,
                &mut providers,
                &mut arrivals,
                &order,
                &customer(),
                &digest,
            )
            .unwrap()
        } else {
            file(&mut env, &mut providers, &mut arrivals, &one).unwrap()
        };
        assert!(
            matches!(ended, Filed::CantProceed { order: Some(ref o), .. } if o == &order),
            "{customer_accepts}: {ended:?}"
        );
        assert!(ended_in_the_log(&env, &order));
        assert!(env.acceptances().is_empty());
        assert_eq!(moved(&env), 0);
        book(&mut providers, "ended");
        assert_eq!(env.exposures.held().count(), 0);
        assert_admitted(&env);
    }
}

/// A slow route's quote is as valid as the figures it stands on (`REQ-QTE-15`): the customer's acceptance past it is
/// asked again, the route shown anew, and starts nothing; the old quote is no longer the one shown, the new one is
/// accepted.
// verifies: REQ-QTE-15
#[test]
fn slow_routes_quote_past_its_validity_is_asked_again_before_it_is_accepted() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let (_, order, first) = waiting_slow(&mut env, &mut providers, &mut arrivals);

    // Past what its figures say, the customer is told the quote stopped standing, and is shown when it did.
    let (_, _, standing_until, expired) = awaited(&env, &arrivals, &order).expect("its quote");
    assert!(!expired);
    env.clock.set(START + 3 * 3_600);
    let (_, _, until, expired) = awaited(&env, &arrivals, &order).expect("its quote, expired");
    assert!(expired, "a dead quote is said to be");
    assert_eq!(until, standing_until);
    // Nothing is accepted on it: the route is asked again and shown anew.
    let asked = accept_slow(
        &mut env,
        &mut providers,
        &mut arrivals,
        &order,
        &customer(),
        &first,
    )
    .unwrap();
    assert_eq!(
        asked,
        Filed::Waiting {
            order: Some(order.clone()),
            wait: FiledWait::CustomerAcceptance
        }
    );
    assert!(env.acceptances().is_empty(), "nothing accepted");
    assert!(env.started().is_empty());
    let (_, second, renewed, expired) =
        awaited(&env, &arrivals, &order).expect("a quote shown anew");
    assert!(!expired && renewed.is_some());
    assert_ne!(second, first, "a quote of its own");
    let shown: Vec<_> = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::Shown { shown, .. } => Some(shown.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(shown, [first.clone(), second.clone()]);
    // Each quote says it waits on the customer, for the customer's notice.
    let awaiting: Vec<bool> = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::Shown { awaiting, .. } => Some(awaiting.is_some()),
            _ => None,
        })
        .collect();
    assert_eq!(awaiting, [true, true]);
    assert_admitted(&env);

    // The old quote isn't the one shown any more; the new one is accepted.
    assert_eq!(
        accept_slow(
            &mut env,
            &mut providers,
            &mut arrivals,
            &order,
            &customer(),
            &first
        ),
        Err(ArrivalError::NotTheQuoteShown)
    );
    let accepted = accept_slow(
        &mut env,
        &mut providers,
        &mut arrivals,
        &order,
        &customer(),
        &second,
    )
    .unwrap();
    assert!(matches!(accepted, Filed::Started { .. }), "{accepted:?}");
    assert_eq!(env.acceptances().len(), 1);
    assert_admitted(&env);
}

/// An order its customer accepted on a deposit (`given_on_deposit`) is split no more than one accepted by the deposit
/// itself: a start of more than one part rests on an arrival naming each part (`split-deposit-arrival-names-its-part`),
/// which the one arrival of the deposit's order, naming none, isn't. The filing never splits (one deposit funds one part),
/// so the case is the log's own: the order's start as the log holds it, with a second part, offered to the log.
// verifies: REQ-ORD-15, REQ-ORD-42
#[test]
fn a_split_start_of_an_order_accepted_on_a_deposit_rests_on_an_arrival_naming_each_part() {
    use rutent_contracts::core_internal::{
        DeploymentOperator, Through, VerifiedEntry, admit_entry,
    };
    use rutent_contracts::state_machine::{Id as StmId, RecordId};

    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let (_, order, digest) = waiting_slow(&mut env, &mut providers, &mut arrivals);
    accept_slow(
        &mut env,
        &mut providers,
        &mut arrivals,
        &order,
        &customer(),
        &digest,
    )
    .unwrap();

    let history = env.history();
    let (start, input) = history
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Input { input } if matches!(input.observed, Observed::RouteChosen { .. }) => {
                    Some((v.clone(), input.clone()))
                }
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .expect("the order started");
    let mut split = input;
    split.record_id = RecordId::new("split-start").unwrap();
    let Observed::RouteChosen { parts, .. } = &mut split.observed else {
        unreachable!()
    };
    let mut second = parts[0].clone();
    second.part_id = StmId::new("part-two").unwrap();
    for (n, segment) in second.segments.iter_mut().enumerate() {
        segment.segment_id = StmId::new(format!("part-two-segment-{n}")).unwrap();
    }
    parts.push(second);
    let entry = Entry::Verified(VerifiedEntry {
        what: What::Input { input: split },
        ..start
    });
    let refused = admit_entry(
        &history,
        &entry,
        Through::General,
        &DeploymentOperator {
            maker: operator().acts,
            party: operator().party,
        },
    )
    .expect_err("a split start with no arrival naming its parts");
    assert_eq!(
        refused.0,
        "a split route of an order accepted on an automatic deposit rests on an arrival naming each part"
    );
}

/// A slow route's quote asked again past its validity may come back firm: it is shown as a plain quote, with no slow
/// segment named, which the order still waits on its customer to accept, since the customer was asked; the old digest
/// is refused, and accepting the new one starts the order (`REQ-QTE-15`, `REQ-QTE-10`).
// verifies: REQ-QTE-15, REQ-QTE-10
#[test]
fn a_requote_that_comes_back_firm_is_shown_and_accepted_by_the_customer() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let (_, order, first) = waiting_slow(&mut env, &mut providers, &mut arrivals);

    // The validity passes and the route is firm now.
    env.clock.set(START + 3 * 3_600);
    slow(&mut env, 60);
    let asked = accept_slow(
        &mut env,
        &mut providers,
        &mut arrivals,
        &order,
        &customer(),
        &first,
    )
    .expect("the log takes the quote shown anew");
    assert_eq!(
        asked,
        Filed::Waiting {
            order: Some(order.clone()),
            wait: FiledWait::CustomerAcceptance
        }
    );
    assert!(env.acceptances().is_empty() && env.started().is_empty());
    let (quote, second, valid_until, expired) =
        awaited(&env, &arrivals, &order).expect("a quote shown anew");
    assert_ne!(second, first);
    assert!(quote.price.no_firm_rate.is_empty(), "no slow segment named");
    assert_eq!(
        quote.shown_as,
        rutent_contracts::core_internal::ShownAs::Quote
    );
    assert!(valid_until.is_some() && !expired);
    // The log says it waits on the customer still, naming no slow segment.
    let last = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::Shown { awaiting, .. } => Some(awaiting.clone()),
            _ => None,
        })
        .next_back()
        .expect("a quote shown");
    assert!(last.is_some_and(|a| a.slow.is_empty() && a.requester == customer().party));
    assert_admitted(&env);

    assert_eq!(
        accept_slow(
            &mut env,
            &mut providers,
            &mut arrivals,
            &order,
            &customer(),
            &first
        ),
        Err(ArrivalError::NotTheQuoteShown)
    );
    let started = accept_slow(
        &mut env,
        &mut providers,
        &mut arrivals,
        &order,
        &customer(),
        &second,
    )
    .unwrap();
    assert!(matches!(started, Filed::Started { .. }), "{started:?}");
    assert_eq!(env.acceptances().len(), 1);
    assert_admitted(&env);
}

/// A deposit filed automatically whose route the router offers but can't be delivered as composed is never shown: the
/// reserve reports net, its withdrawal says it gives out 500 more than it takes in, which the router confirms as
/// stated, and its deposit takes that in at a fee of 500, so the route is no rate cycle the router drops. `show`
/// counts the withdrawal as `segments::judge` would and refuses it. The filing ends its order with the class the quote
/// module gives it, `no_route_right_now`, records no `shown`, and accepts and starts nothing.
// verifies: REQ-QTE-2, REQ-ORD-18
#[test]
fn automatic_deposit_whose_route_cant_be_delivered_ends_unshown() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    providers.sim.overclaim.insert(RESERVE.into(), 500);
    providers.sim.deposit_fees.insert(RESERVE.into(), 500);
    // The deposit is priced at that fee too, so the route as first planned comes back to the origin with no more than
    // it left with, and the router asks the deposit again at what the withdrawal claims.
    for price in env.prices.iter_mut() {
        if price.tuple == snapshot_support::deposit_tuple(RESERVE) {
            price.fee.flat = rutent_contracts::router_api::Amount::new("500").unwrap();
        }
    }
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
        &filing(reserve_filer(), &deposited),
    )
    .unwrap();
    let Filed::CantProceed {
        order: Some(order),
        class: OrderRefusalClass::NoRouteRightNow,
    } = filed
    else {
        panic!("{filed:?}");
    };
    // The router offered the route: the deposit was asked at what the withdrawal claimed.
    let asked: Vec<String> = providers
        .sim
        .received
        .iter()
        .filter_map(|(_, op)| op["amount"].as_str().map(str::to_owned))
        .collect();
    assert!(
        asked.iter().any(|a| a == "1500"),
        "the deposit was asked at what the withdrawal claimed: {asked:?}"
    );
    let shown = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .filter(|w| matches!(w, What::Shown { .. }))
        .count();
    assert_eq!(shown, 0, "nothing shown");
    assert!(order_records(&env).iter().any(|w| matches!(
        w,
        What::OrderRefusal { order: Some(o), class: OrderRefusalClass::NoRouteRightNow, .. } if o == &order
    )));
    assert!(env.acceptances().is_empty());
    assert!(env.started().is_empty());
    assert_admitted(&env);
}

// T-020-19: the arrival rests on the reserve's signed notice -------------------------------------------------------

/// The receipt of the entry at `index`, as the tests' log gives one.
fn receipt_at(index: usize) -> Receipt {
    serde_json::from_value(json!({
        "log": "log-1",
        "index": index,
        "entry": { "algorithm": "sha-256", "value": "72".repeat(32) },
    }))
    .unwrap()
}

/// Every `arrival` entry the log holds, with its receipt, as recorded.
fn arrival_entries(env: &Env) -> Vec<(Receipt, What)> {
    env.book
        .borrow()
        .entries
        .iter()
        .enumerate()
        .filter_map(|(at, (_, e))| match e {
            Entry::Verified(v) if matches!(v.what, What::Arrival { .. }) => {
                Some((receipt_at(at), v.what.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Whether any stated entry of the notice shape, or any arrival, was recorded: what a refused notice must not leave.
fn leaves_nothing(env: &Env, len: usize, kept: usize) {
    assert_eq!(env.len(), len, "nothing appended");
    assert_eq!(env.store.borrow().kept.len(), kept, "nothing kept");
    assert!(arrival_entries(env).is_empty(), "no arrival");
    assert!(order_records(env).is_empty(), "no order");
}

/// What an arrival states of what arrived, its node, asset and amount, is what the reserve's signed notice states, not
/// what a port said; and the account's holder, who decides which order it is held against, is the notice's too.
// verifies: REQ-ORD-23
#[test]
fn arrivals_figures_are_read_from_the_reserves_signed_notice() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    // The notice states 750 USDT at base, which is no open order's: the open order is for 1 000 USDC at polygon.
    let stated = Credit {
        node: OuterNode {
            network: "base".into(),
        },
        asset: Denomination {
            id: "USDT".into(),
            decimals: 6,
        },
        ..Credit::new(750, 0)
    };
    let mut open = Open(vec![open_order(&id("ord-open"))]);
    let arrived = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut open,
        &stated.report(),
    )
    .unwrap();
    assert!(matches!(arrived, Arrived::NoOrder { .. }), "{arrived:?}");
    let recorded = arrival_entries(&env);
    let [
        (
            _,
            What::Arrival {
                node,
                asset,
                amount: arrived_amount,
                order,
                ..
            },
        ),
    ] = &recorded[..]
    else {
        panic!("{recorded:?}");
    };
    assert_eq!(node, &stated.node);
    assert_eq!(asset, &stated.asset);
    assert_eq!(arrived_amount.as_str(), "750");
    assert_eq!(order, &None);
    assert_eq!(providers.asked(), 0, "the notice meets no open order");

    // The notice states another holder than the order's: the arrival is not held against the customer's order.
    let stranger = Credit {
        holder: id("a-stranger"),
        ..Credit::new(1000, 1)
    };
    let arrived = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut open,
        &stranger.report(),
    )
    .unwrap();
    assert!(matches!(arrived, Arrived::NoOrder { .. }), "{arrived:?}");
    assert_eq!(providers.asked(), 0);
    // Nor against it when the notice states another account than the order's, or the order's amount in another unit.
    for (n, other) in [
        Credit {
            account: "acct-9".into(),
            ..Credit::new(1000, 2)
        },
        Credit {
            asset: Denomination {
                id: "USDC".into(),
                decimals: 18,
            },
            ..Credit::new(1000, 3)
        },
    ]
    .into_iter()
    .enumerate()
    {
        let arrived = arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut open,
            &other.report(),
        )
        .unwrap();
        assert!(
            matches!(arrived, Arrived::NoOrder { .. }),
            "{n}: {arrived:?}"
        );
    }
    assert_eq!(providers.asked(), 0);
    // The notice that does state the open order's figures is held against it.
    let held = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut open,
        &Credit::new(1000, 4).report(),
    )
    .unwrap();
    assert!(matches!(held, Arrived::Held { .. }), "{held:?}");
    assert_admitted(&env);
}

/// Two notices of one credit, by its per-credit reference, are one arrival, whenever and however they were signed; two
/// references are two deposits, and one reference with other figures is refused.
// verifies: REQ-ORD-23
#[test]
fn two_notices_of_one_credit_are_one_arrival() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let first = Credit::new(1000, 0);
    let arrived = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &first.report(),
    )
    .unwrap();
    // The same credit, signed again later: other bytes, other time.
    let later = reported(first.notice_as(NOTICE_SHAPE, RESERVE, START + 5));
    assert_ne!(later.notice, first.notice());
    let (len, kept) = (env.len(), env.store.borrow().kept.len());
    let again = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &later,
    )
    .unwrap();
    assert_eq!(again, arrived);
    assert_eq!((env.len(), env.store.borrow().kept.len()), (len, kept));
    assert_eq!(arrival_entries(&env).len(), 1);
    // Another reference, the same figures: another credit, its own arrival and notice.
    let other = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &Credit::new(1000, 1).report(),
    )
    .unwrap();
    assert_ne!(other, arrived);
    let all = arrival_entries(&env);
    assert_eq!(all.len(), 2);
    let notices: Vec<_> = all
        .iter()
        .map(|(_, w)| match w {
            What::Arrival { notice, .. } => notice.clone(),
            _ => unreachable!(),
        })
        .collect();
    assert_ne!(notices[0], notices[1], "each deposit cites its own notice");
    // One reference, other figures, even where signed as a second notice: refused, nothing kept.
    let (len, kept) = (env.len(), env.store.borrow().kept.len());
    let restated = Credit {
        units: 999,
        ..first.clone()
    };
    assert_eq!(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut Open::default(),
            &restated.report()
        ),
        Err(ArrivalError::OtherFigures)
    );
    for credit in [
        Credit {
            account: "acct-9".into(),
            ..first.clone()
        },
        Credit {
            holder: id("a-stranger"),
            ..first.clone()
        },
        Credit {
            node: OuterNode {
                network: "base".into(),
            },
            ..first.clone()
        },
        Credit {
            asset: Denomination {
                id: "USDC".into(),
                decimals: 18,
            },
            ..first
        },
    ] {
        assert_eq!(
            arrive(
                &mut env,
                &mut providers,
                &mut arrivals,
                &mut Open::default(),
                &credit.report()
            ),
            Err(ArrivalError::OtherFigures)
        );
    }
    assert_eq!((env.len(), env.store.borrow().kept.len()), (len, kept));
    assert_eq!(arrival_entries(&env).len(), 2);
    assert_admitted(&env);
}

/// A deposit filed on after its arrival met no order is held against the order the filing opened by a second `arrival`,
/// which names the first, so one deposit reads as one and not two (`arrival-names-its-earlier-arrival`).
// verifies: REQ-ORD-23
#[test]
fn deposit_filed_again_against_another_order_names_its_earlier_arrival() {
    for filer in [reserve_filer(), operator()] {
        let mut env = crate::env();
        let mut providers = Reserve::new(&env);
        let mut arrivals = Arrivals::new();
        let first = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
        // Another deposit meets no order too, and is not the one filed.
        let unrelated = deposit(&mut env, &mut providers, &mut arrivals, 1000, 1);
        let filed = file(
            &mut env,
            &mut providers,
            &mut arrivals,
            &filing(filer, &first),
        )
        .unwrap();
        let Filed::Started { order, .. } = filed else {
            panic!("{filed:?}");
        };
        let all = arrival_entries(&env);
        let held: Vec<_> = all
            .iter()
            .filter(|(_, w)| matches!(w, What::Arrival { order: Some(o), .. } if o == &order))
            .collect();
        let [
            (
                _,
                What::Arrival {
                    earlier, notice, ..
                },
            ),
        ] = &held[..]
        else {
            panic!("{held:?}");
        };
        assert_eq!(earlier.as_ref(), Some(&first), "the arrival it holds");
        let (
            _,
            What::Arrival {
                notice: of_first, ..
            },
        ) = all.iter().find(|(r, _)| r == &first).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(notice, of_first, "one deposit, one notice");
        // The arrivals that met no order name no earlier one, and the other deposit's names nothing of this one.
        for (receipt, what) in &all {
            if let What::Arrival {
                order: None,
                earlier,
                ..
            } = what
            {
                assert_eq!(earlier, &None, "{receipt:?}");
            }
        }
        assert_ne!(Some(&unrelated), earlier.as_ref());
        assert_admitted(&env);
    }
}

/// A notice the log could not attribute to its reserve (signed with a key the reserve never listed) creates no arrival,
/// and so no order a filing could rest on.
// verifies: REQ-ORD-14, REQ-AUD-2
#[test]
fn notice_the_log_could_not_attribute_to_its_reserve_creates_no_arrival() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let credit = Credit::new(1000, 0);
    let forged = reported(sign(
        envelope(&deployment(), NOTICE_SHAPE, RESERVE, START, credit.body()),
        &statements_key(RESERVE, 7),
    ));
    let refused = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &forged,
    );
    assert!(matches!(refused, Err(ArrivalError::Log(_))), "{refused:?}");
    assert!(arrival_entries(&env).is_empty(), "no arrival");
    // Nor an arrival against an open order. Core can't read the log's attribution of a notice (a gap, in the notes), so
    // it still asks the reserve its balance question for the order before the log refuses the arrival; nothing the
    // reserve is sent moves value, and none of it is an operation.
    let order = explicit_order(&mut env, &mut providers);
    let operations = providers.sim.received.len();
    let mut open = Open(vec![open_order(&order)]);
    let refused = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &forged);
    assert!(matches!(refused, Err(ArrivalError::Log(_))), "{refused:?}");
    assert!(arrival_entries(&env).is_empty(), "no arrival");
    assert_eq!(
        providers.sim.received.len(),
        operations,
        "no operation sent"
    );
    assert!(
        order_records(&env)
            .iter()
            .all(|w| !matches!(w, What::Acceptance { order: o, .. } if o != &order))
    );
    // The same reserve's real notice of the same credit, later: taken, as the forged one holds no deposit.
    let real = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &credit.report(),
    );
    assert!(real.is_ok(), "{real:?}");
}

/// Value arriving on a notice no one could attribute opens no order, whatever follows it: the arrival that would be
/// filed on is never recorded, so there is none to file.
// verifies: REQ-ORD-14
#[test]
fn unattributed_arrival_still_creates_no_order() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let credit = Credit::new(1000, 0);
    let forged = reported(sign(
        envelope(&deployment(), NOTICE_SHAPE, RESERVE, START, credit.body()),
        &statements_key("an-impostor", 1),
    ));
    let refused = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &forged,
    );
    assert!(matches!(refused, Err(ArrivalError::Log(_))), "{refused:?}");
    // A filer holds no receipt of it to file on: any receipt it names is no arrival Core recorded.
    let no_arrival = receipt_at(0);
    let filed = file(
        &mut env,
        &mut providers,
        &mut arrivals,
        &filing(reserve_filer(), &no_arrival),
    );
    assert_eq!(filed, Err(ArrivalError::NoSuchArrival));
    assert!(order_records(&env).is_empty());
    assert!(arrival_entries(&env).is_empty());
    assert_admitted(&env);
}

/// What the log would take for a notice only by its relation: a signed hold, or an arrival answer, each the reserve's
/// and each with a body `transaction`, is not a notice, and, with every other body or shape that is not the
/// contract's, is refused before anything is kept or appended.
// verifies: REQ-ORD-14, REQ-AUD-2
#[test]
fn signed_hold_or_arrival_answer_is_not_taken_for_an_arrival_notice() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let stated = Credit::new(1000, 0);
    let signed = |shape: &str, maker: &str, body: Value| {
        sign(
            envelope(&deployment(), shape, maker, START, body),
            &statements_key(RESERVE, 1),
        )
    };
    let with = |key: &str, value: Value| {
        let mut body = stated.body();
        body[key] = value;
        body
    };
    let hold = signed(
        rutent_contracts::provider_role::HOLD_SHAPE,
        RESERVE,
        json!({ "transaction": credit(0), "reserve": RESERVE }),
    );
    let answer = signed(
        rutent_contracts::provider_role::ARRIVAL_ANSWER_SHAPE,
        RESERVE,
        json!({ "transaction": credit(0), "outcome": { "status": "nothing_arrived" } }),
    );
    let question = signed(
        rutent_contracts::provider_role::ARRIVAL_QUESTION_SHAPE,
        RESERVE,
        json!({ "transaction": credit(0), "provider": RESERVE, "session": "s", "account": ACCOUNT,
                "node": polygon(), "asset": usdc() }),
    );
    // The notice's shape over another statement's body, and the notice's body under another statement's shape.
    let hold_as_notice = signed(
        NOTICE_SHAPE,
        RESERVE,
        json!({ "transaction": credit(0), "reserve": RESERVE }),
    );
    let answer_as_notice = signed(
        NOTICE_SHAPE,
        RESERVE,
        json!({ "transaction": credit(0), "outcome": { "status": "nothing_arrived" } }),
    );
    let as_result = signed(RESULT_SHAPE, RESERVE, stated.body());
    let as_hold = signed(
        rutent_contracts::provider_role::HOLD_SHAPE,
        RESERVE,
        stated.body(),
    );
    // A body that lacks a field, carries an extra one, or states what no amount is.
    let mut short = stated.body();
    short.as_object_mut().unwrap().remove("holder");
    let short = signed(NOTICE_SHAPE, RESERVE, short);
    let extra = signed(NOTICE_SHAPE, RESERVE, with("kind", json!("arrival")));
    let zero = signed(NOTICE_SHAPE, RESERVE, with("amount", json!("0")));
    let negative = signed(NOTICE_SHAPE, RESERVE, with("amount", json!("-5")));
    let fraction = signed(NOTICE_SHAPE, RESERVE, with("amount", json!("1.5")));
    let numeric = signed(NOTICE_SHAPE, RESERVE, with("amount", json!(1000)));
    let unnamed = signed(NOTICE_SHAPE, RESERVE, with("transaction", json!("")));
    let elsewhere = signed(NOTICE_SHAPE, "another-reserve", stated.body());
    let (len, kept) = (env.len(), env.store.borrow().kept.len());
    for (what, bytes) in [
        ("a hold", hold),
        ("an arrival answer", answer),
        ("an arrival question", question),
        ("a hold's body under the notice's shape", hold_as_notice),
        (
            "an answer's body under the notice's shape",
            answer_as_notice,
        ),
        ("a notice's body as a result", as_result),
        ("a notice's body as a hold", as_hold),
        ("a notice without its holder", short),
        ("a notice with a field the contract doesn't name", extra),
        ("a notice of no amount", zero),
        ("a notice of a negative amount", negative),
        ("a notice of a fraction", fraction),
        ("a notice whose amount is a number", numeric),
        ("a notice naming no transaction", unnamed),
        ("another reserve's notice", elsewhere),
        ("bytes that are no statement", b"not json".to_vec()),
    ] {
        for held in [Open::default(), Open(vec![open_order(&id("ord-open"))])] {
            let mut held = held;
            let refused = arrive(
                &mut env,
                &mut providers,
                &mut arrivals,
                &mut held,
                &reported(bytes.clone()),
            );
            assert_eq!(refused, Err(ArrivalError::NotItsNotice), "{what}");
        }
    }
    leaves_nothing(&env, len, kept);
    assert_eq!(providers.asked(), 0);
    // The contract's own notice, beside them, is taken.
    let taken = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &stated.report(),
    );
    assert!(matches!(taken, Ok(Arrived::NoOrder { .. })), "{taken:?}");
    assert_admitted(&env);
}

/// Figures are read from the notice the log's own read gives back, and only if it is the statement Core read them
/// from: a store that keeps nothing behind the commitment leaves the notice unrecorded and the deposit unknown.
// verifies: REQ-AUD-2, REQ-ORD-23
#[test]
fn notice_the_store_keeps_nothing_behind_is_refused() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let (len, asked) = (env.len(), env.kept.asked);
    env.kept.forgets = true;
    let refused = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &report(1000, 0),
    );
    assert_eq!(refused, Err(ArrivalError::NotItsNotice));
    assert!(env.kept.asked > asked, "the read was made");
    assert_eq!(env.len(), len, "nothing appended");
    env.kept.forgets = false;
    // The deposit was never taken: the same notice, read back, is taken now.
    let taken = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut Open::default(),
        &report(1000, 0),
    );
    assert!(matches!(taken, Ok(Arrived::NoOrder { .. })), "{taken:?}");
    assert_admitted(&env);
}

/// A forged notice parked by a silent reserve, under a reference with other figures, doesn't wedge the reserve's genuine
/// notice of that credit: the later notice replaces it, and once the reserve says yes the genuine one is held, with one
/// arrival recorded. An arrival already recorded under the reference stands against other figures.
// verifies: REQ-ORD-14, REQ-AUD-2
#[test]
fn a_parked_forged_notice_does_not_wedge_the_genuine_one() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&order)]);
    providers.silent = true;
    let forged = reported(sign(
        envelope(
            &deployment(),
            NOTICE_SHAPE,
            RESERVE,
            START,
            Credit::new(1000, 0).body(),
        ),
        &statements_key(RESERVE, 7),
    ));
    let parked = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &forged).unwrap();
    assert!(matches!(parked, Arrived::Waiting { .. }), "{parked:?}");
    // The genuine notice of the credit states other figures than the forgery did.
    let genuine = Credit::new(999, 0).report();
    let waiting = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &genuine);
    assert!(
        matches!(waiting, Ok(Arrived::Waiting { .. })),
        "{waiting:?}"
    );
    providers.silent = false;
    let held = arrive(&mut env, &mut providers, &mut arrivals, &mut open, &genuine).unwrap();
    assert!(
        matches!(held, Arrived::Held { order: ref o, .. } if o == &order),
        "{held:?}"
    );
    assert_eq!(arrival_entries(&env).len(), 1);
    // Recorded, it stands: other figures under the reference are refused.
    assert_eq!(
        arrive(
            &mut env,
            &mut providers,
            &mut arrivals,
            &mut open,
            &Credit::new(1000, 0).report()
        ),
        Err(ArrivalError::OtherFigures)
    );
    assert_eq!(arrival_entries(&env).len(), 1);
    assert_admitted(&env);
}

/// The stated entries the log holds claimed by `maker`, each with who it says relayed it.
fn relays_of(env: &Env, maker: &str) -> Vec<Option<Actor>> {
    entries(env)
        .iter()
        .filter_map(|entry| match entry {
            Entry::Stated(stated) if stated.attribution.maker().acts == id(maker) => {
                Some(stated.relayed_by.clone())
            }
            _ => None,
        })
        .collect()
}

/// The reserve is asked about an arrival held against an order and answers with a statement another provider signed:
/// the entry is that maker's, unverified, and names the reserve that handed it over as its relay. The reserve's own
/// answer, and its own notice, name none.
// verifies: REQ-AUD-2
#[test]
fn arrival_notice_restated_by_a_relay_is_recorded_naming_the_relay() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&order)]);
    providers.speaks_as = Some("other-reserve");
    let _ = arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut open,
        &report(600, 0),
    );
    let [relayed] = relays_of(&env, "other-reserve").try_into().unwrap();
    assert_eq!(relayed.expect("the relay is named").acts, id(RESERVE));

    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let mut open = Open(vec![open_order(&order)]);
    arrive(
        &mut env,
        &mut providers,
        &mut arrivals,
        &mut open,
        &report(600, 0),
    )
    .unwrap();
    let own = relays_of(&env, RESERVE);
    assert!(own.len() >= 2 && own.iter().all(Option::is_none), "{own:?}");
    assert_admitted(&env);
}

// REQ-QTE-10, GUA-1, REQ-ORD-15 (T-020-51: a deposit waiting for its customer outlives the filer's session) -------------

/// The wait settings the T-020-51 tests use: a lifetime short enough to pass within the filing's session.
const WAIT: u64 = 1_800;

/// A slow deposit filed by the reserve in the short session `session-short`, its wait lasting [`WAIT`] seconds: the
/// arrival, the filing, the order it waits as and its quote's digest.
fn waiting_in_a_short_session(
    env: &mut Env,
    providers: &mut Reserve,
    arrivals: &mut Arrivals,
) -> (Filing, Id, rutent_contracts::module_interface::Digest) {
    slow(env, 4 * 86_400);
    let deposited = deposit(env, providers, arrivals, 1000, 0);
    let mut one = filing(reserve_filer(), &deposited);
    one.session = short_session(env, "session-short", START + 600);
    let filed = file(env, providers, arrivals, &one).unwrap();
    let Filed::Waiting {
        order: Some(order),
        wait: FiledWait::CustomerAcceptance,
    } = filed
    else {
        panic!("{filed:?}");
    };
    let (_, digest, _, _) = awaited(env, arrivals, &order).expect("its quote is shown");
    (one, order, digest)
}

/// `at`, in whole seconds since the Unix epoch.
fn seconds(at: &rutent_contracts::router_api::Timestamp) -> u64 {
    u64::try_from(at.unix_nanos() / 1_000_000_000).unwrap()
}

/// When the first quote the log shows was shown, in whole seconds.
fn first_shown_at(env: &Env) -> u64 {
    env.book
        .borrow()
        .entries
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Verified(v) if matches!(v.what, What::Shown { .. }) => Some(seconds(&v.when.at)),
            _ => None,
        })
        .expect("a quote was shown")
}

/// Arrivals whose waits last [`WAIT`] seconds.
fn short_waits() -> Arrivals {
    Arrivals::new().with_deposit_wait(Count::new(WAIT).unwrap())
}

/// The ends of waits the log holds, with why.
fn wait_ends_in_the_log(env: &Env) -> Vec<(Id, WaitEnd)> {
    env.book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::WaitEnded { order, why } => Some((order.clone(), *why)),
            _ => None,
        })
        .collect()
}

/// A customer whose filing session is over accepts the waiting deposit in a session of its own, opened later: the wait
/// outlives the filer's session, the acceptance is recorded on `given_on_deposit` in the customer's session, and only a
/// session open for the customer takes it (`REQ-QTE-10`, `AC-13`).
// verifies: REQ-QTE-10, AC-13
#[test]
fn customer_accepts_a_waiting_deposit_in_a_later_session_of_its_own() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = short_waits();
    let (one, order, digest) = waiting_in_a_short_session(&mut env, &mut providers, &mut arrivals);

    // The filer's session is over: the wait is held, and its filer told so.
    env.clock.set(START + 700);
    assert!(matches!(
        file(&mut env, &mut providers, &mut arrivals, &one).unwrap(),
        Filed::Waiting {
            wait: FiledWait::CustomerAcceptance,
            ..
        }
    ));
    assert!(!ended_in_the_log(&env, &order));
    // The customer's own session, opened later; and one that isn't the customer's, one that is over, one unknown.
    let later = opened_session(&mut env, "session-later", START + 10_000);
    env.sessions.held.insert(
        "session-another".into(),
        rutent_core_orch::intake::HeldSession {
            holder: id("another-customer"),
            expires_at: rutent_contracts::router_api::Timestamp::new(
                Instant(START + 10_000).rfc3339().unwrap(),
            )
            .unwrap(),
        },
    );
    let before = env.len();
    for session in ["session-short", "session-another", "session-nobody-opened"] {
        assert_eq!(
            accept_in(
                &mut env,
                &mut providers,
                &mut arrivals,
                &order,
                &customer(),
                &id(session),
                &digest
            ),
            Err(ArrivalError::NoSession),
            "{session}"
        );
    }
    assert_eq!(env.len(), before, "nothing recorded");
    assert!(env.started().is_empty());

    let accepted = accept_in(
        &mut env,
        &mut providers,
        &mut arrivals,
        &order,
        &customer(),
        &later,
        &digest,
    )
    .unwrap();
    assert!(matches!(accepted, Filed::Started { .. }), "{accepted:?}");
    let acceptances = env.acceptances();
    assert_eq!(acceptances.len(), 1);
    let What::Acceptance {
        basis,
        session,
        yeses,
        ..
    } = &acceptances[0].what
    else {
        unreachable!()
    };
    assert_eq!(
        (basis, session, yeses.len()),
        (&AcceptanceBasis::GivenOnDeposit, &Some(later), 0)
    );
    assert_eq!(acceptances[0].who, customer());
    assert_eq!(env.started(), [order.as_str()]);
    assert!(wait_ends_in_the_log(&env).is_empty());
    assert_admitted(&env);
}

/// A wait past its lifetime ends with the log's `wait_ended`, whichever call reads it first: Core's schedule
/// (`Arrivals::end_expired`), the filer filing again, or the customer accepting. Nothing moves, the order takes no
/// acceptance after it, and the deposit is free (`GUA-1`).
// verifies: GUA-1
#[test]
fn wait_past_its_lifetime_ends_and_frees_its_deposit() {
    for reader in ["schedule", "filing", "customer"] {
        let mut env = crate::env();
        let mut providers = Reserve::new(&env);
        let mut arrivals = short_waits();
        let (one, order, digest) =
            waiting_in_a_short_session(&mut env, &mut providers, &mut arrivals);
        let lasts = arrivals.wait_ends(&order).expect("a wait has its end");
        assert_eq!(
            seconds(&lasts),
            first_shown_at(&env) + WAIT,
            "counted from the quote shown"
        );

        // Before its lifetime, nothing ends.
        env.clock.set(seconds(&lasts) - 1);
        let ended = env
            .with_providers(&mut providers, |_, desk| arrivals.end_expired(desk))
            .unwrap();
        assert!(ended.is_empty(), "{reader}: still waiting");
        assert!(wait_ends_in_the_log(&env).is_empty());

        env.clock.set(seconds(&lasts));
        let (_, _, _, expired) = awaited(&env, &arrivals, &order).expect("its quote");
        assert!(expired, "{reader}: the quote can't stand past the wait");
        match reader {
            "schedule" => {
                let ended = env
                    .with_providers(&mut providers, |_, desk| arrivals.end_expired(desk))
                    .unwrap();
                assert_eq!(ended, std::slice::from_ref(&order));
            }
            "filing" => {
                let told = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
                assert!(
                    matches!(told, Filed::CantProceed { order: Some(ref o), .. } if o == &order),
                    "{told:?}"
                );
            }
            _ => {
                let told = accept_slow(
                    &mut env,
                    &mut providers,
                    &mut arrivals,
                    &order,
                    &customer(),
                    &digest,
                )
                .unwrap();
                assert!(
                    matches!(told, Filed::CantProceed { order: Some(ref o), .. } if o == &order),
                    "{told:?}"
                );
            }
        }
        // Recorded once: the end of the wait, then the order's refusal, which is what tells the customer.
        assert_eq!(
            wait_ends_in_the_log(&env),
            [(order.clone(), WaitEnd::PastItsLifetime)],
            "{reader}"
        );
        assert!(ended_in_the_log(&env, &order), "{reader}");
        assert!(env.acceptances().is_empty() && env.started().is_empty());
        assert_eq!(arrivals.wait_ends(&order), None, "{reader}: no wait left");
        // Told again, it records nothing more.
        let before = env.len();
        let again = env
            .with_providers(&mut providers, |_, desk| arrivals.end_expired(desk))
            .unwrap();
        assert!(again.is_empty());
        assert_eq!(
            accept_slow(
                &mut env,
                &mut providers,
                &mut arrivals,
                &order,
                &customer(),
                &digest
            )
            .map(|f| matches!(f, Filed::CantProceed { .. })),
            Ok(true),
            "{reader}: the customer is told it ended"
        );
        assert_eq!(env.len(), before, "{reader}");
        assert_admitted(&env);
    }
}

/// The deposit a wait's end freed is filed again on the reserve's next filing, and routed once: a second order opens on
/// it (the log admits it, `one-order-per-deposit`), starts on the deposit once, and the deposit is held against that
/// order, not filed on a third time (`REQ-ORD-15`).
// verifies: REQ-ORD-15
#[test]
fn freed_deposit_is_filed_again_and_routed_once() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = short_waits();
    let (one, first, _) = waiting_in_a_short_session(&mut env, &mut providers, &mut arrivals);
    // While it waits the deposit is held: another filing opens nothing.
    let mut other = filing(reserve_filer(), &one.arrival);
    other.request = id("deposit-2");
    other.session = id(SESSION);
    assert!(matches!(
        file(&mut env, &mut providers, &mut arrivals, &other),
        Err(ArrivalError::Conflicting { .. })
    ));
    env.clock.set(seconds(&arrivals.wait_ends(&first).unwrap()));
    env.with_providers(&mut providers, |_, desk| arrivals.end_expired(desk))
        .unwrap();
    assert_eq!(
        wait_ends_in_the_log(&env),
        [(first.clone(), WaitEnd::PastItsLifetime)]
    );

    // Its next filing, the route firm this time, takes the deposit.
    slow(&mut env, 3_600);
    other.session = short_session(&mut env, "session-next", START + 20_000);
    let filed = file(&mut env, &mut providers, &mut arrivals, &other).unwrap();
    let Filed::Started {
        order: ref second, ..
    } = filed
    else {
        panic!("{filed:?}");
    };
    assert_ne!(second, &first);
    let acceptances = env.acceptances();
    assert_eq!(acceptances.len(), 1, "routed once");
    let What::Acceptance { order, basis, .. } = &acceptances[0].what else {
        unreachable!()
    };
    assert_eq!((order, basis), (second, &AcceptanceBasis::AutomaticDeposit));
    assert_eq!(env.started(), [second.as_str()]);
    // The same filing again is told the same and records nothing; a third filing finds the deposit held.
    let before = env.len();
    assert_eq!(
        file(&mut env, &mut providers, &mut arrivals, &other).unwrap(),
        filed
    );
    let mut third = other.clone();
    third.request = id("deposit-3");
    assert!(matches!(
        file(&mut env, &mut providers, &mut arrivals, &third),
        Err(ArrivalError::Conflicting { .. })
    ));
    assert_eq!(env.len(), before);
    assert_admitted(&env);
}

/// A Core that reopens rebuilds the waits from its log: the order still waits, its lifetime counted from the quote the
/// log shows, its deposit held; the customer's acceptance of the quote it was shown asks the route again (the quote
/// itself isn't in the log), and the new quote is accepted; and a wait past its lifetime ends on the reopened Core as on
/// the one that ran on (`REQ-QTE-10`).
// verifies: REQ-QTE-10
#[test]
fn filed_order_waiting_for_its_customer_still_waits_after_core_reopens() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = short_waits();
    let (one, order, digest) = waiting_in_a_short_session(&mut env, &mut providers, &mut arrivals);
    let lasts = arrivals.wait_ends(&order).unwrap();

    let history = env.history();
    let mut reopened = env.with_providers(&mut providers, |_, desk| {
        Arrivals::reopened(&history, desk.kept).with_deposit_wait(Count::new(WAIT).unwrap())
    });
    assert_eq!(
        reopened.wait_ends(&order),
        Some(lasts.clone()),
        "the wait goes on"
    );
    // Its filer, filing the deposit again under the request the log names, is told the wait goes on, and records
    // nothing. Another filing of it opens nothing.
    env.clock.set(START + 700);
    let before = env.len();
    assert!(matches!(
        file(&mut env, &mut providers, &mut reopened, &one).unwrap(),
        Filed::Waiting {
            order: Some(ref o),
            wait: FiledWait::CustomerAcceptance
        } if o == &order
    ));
    assert_eq!(env.len(), before);
    let mut other = filing(reserve_filer(), &one.arrival);
    other.request = id("deposit-2");
    assert!(matches!(
        file(&mut env, &mut providers, &mut reopened, &other),
        Err(ArrivalError::Conflicting { .. })
    ));
    // A quote the log never showed isn't the one shown.
    let other_digest: rutent_contracts::module_interface::Digest =
        serde_json::from_value(json!({ "algorithm": "sha-256", "value": "77".repeat(32) }))
            .unwrap();
    let later = opened_session(&mut env, "session-later", START + 10_000);
    assert_eq!(
        accept_in(
            &mut env,
            &mut providers,
            &mut reopened,
            &order,
            &customer(),
            &later,
            &other_digest
        ),
        Err(ArrivalError::NotTheQuoteShown)
    );
    // The quote it was shown is asked again, the route shown anew; the customer accepts that one.
    let asked = accept_in(
        &mut env,
        &mut providers,
        &mut reopened,
        &order,
        &customer(),
        &later,
        &digest,
    )
    .unwrap();
    assert_eq!(
        asked,
        Filed::Waiting {
            order: Some(order.clone()),
            wait: FiledWait::CustomerAcceptance
        }
    );
    assert!(env.acceptances().is_empty());
    let (_, second, until, _) = awaited(&env, &reopened, &order).expect("a quote shown anew");
    assert_ne!(second, digest);
    assert!(
        until.is_some_and(|u| u.unix_nanos() <= lasts.unix_nanos()),
        "the wait is no longer for the new quote"
    );
    let accepted = accept_in(
        &mut env,
        &mut providers,
        &mut reopened,
        &order,
        &customer(),
        &later,
        &second,
    )
    .unwrap();
    assert!(matches!(accepted, Filed::Started { .. }), "{accepted:?}");
    assert_admitted(&env);

    // A reopened Core's wait ends at its lifetime too.
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = short_waits();
    let (one, order, _) = waiting_in_a_short_session(&mut env, &mut providers, &mut arrivals);
    let history = env.history();
    let mut reopened = env.with_providers(&mut providers, |_, desk| {
        Arrivals::reopened(&history, desk.kept).with_deposit_wait(Count::new(WAIT).unwrap())
    });
    env.clock.set(seconds(&reopened.wait_ends(&order).unwrap()));
    let ended = env
        .with_providers(&mut providers, |_, desk| reopened.end_expired(desk))
        .unwrap();
    assert_eq!(ended, std::slice::from_ref(&order));
    assert_eq!(
        wait_ends_in_the_log(&env),
        [(order, WaitEnd::PastItsLifetime)]
    );
    assert_admitted(&env);

    // Reopened again after the end, the deposit is still free: its next filing takes it.
    let history = env.history();
    let mut reopened = env.with_providers(&mut providers, |_, desk| {
        Arrivals::reopened(&history, desk.kept)
    });
    slow(&mut env, 3_600);
    let mut next = filing(reserve_filer(), &one.arrival);
    next.request = id("deposit-2");
    next.session = short_session(&mut env, "session-next", START + 20_000);
    let filed = file(&mut env, &mut providers, &mut reopened, &next).unwrap();
    assert!(matches!(filed, Filed::Started { .. }), "{filed:?}");
    assert_admitted(&env);
}

/// A waiting quote past its validity is asked again on the customer's acceptance, as the figures it stood on were:
/// nothing is accepted, the route is shown anew, and the wait goes on from the first quote shown, not from this one;
/// a quote's validity never outlasts its wait (`REQ-QTE-15`).
// verifies: REQ-QTE-15
#[test]
fn acceptance_on_a_waiting_quote_past_its_validity_asks_again() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = Arrivals::new();
    let (_, order, first) = waiting_slow(&mut env, &mut providers, &mut arrivals);
    let lasts = arrivals.wait_ends(&order).expect("its wait");
    let (_, _, standing_until, _) = awaited(&env, &arrivals, &order).expect("its quote");
    assert!(
        standing_until.is_some_and(|u| u.unix_nanos() <= lasts.unix_nanos()),
        "the quote doesn't outlast the wait"
    );
    env.clock.set(START + 3 * 3_600);
    let asked = accept_slow(
        &mut env,
        &mut providers,
        &mut arrivals,
        &order,
        &customer(),
        &first,
    )
    .unwrap();
    assert_eq!(
        asked,
        Filed::Waiting {
            order: Some(order.clone()),
            wait: FiledWait::CustomerAcceptance
        }
    );
    assert!(env.acceptances().is_empty() && env.started().is_empty());
    let (_, second, renewed, expired) = awaited(&env, &arrivals, &order).expect("a quote anew");
    assert_ne!(second, first);
    assert!(!expired);
    assert!(
        renewed
            .as_ref()
            .is_some_and(|u| u.unix_nanos() <= lasts.unix_nanos()),
        "{renewed:?} past {lasts:?}"
    );
    assert_eq!(
        arrivals.wait_ends(&order),
        Some(lasts),
        "the wait isn't renewed"
    );
    assert!(wait_ends_in_the_log(&env).is_empty());
    assert_admitted(&env);
}

/// The unique value the tests' filings carry as an input, which no entry of the log may hold (`REQ-AUD-6`).
const PAYER_ACCOUNT: &str = "acct-payer-secret-77";

/// A slow deposit of 1 000 filed by the reserve with an input of its own, in the short session: the arrival, the
/// filing, the order and its quote's digest.
fn waiting_with_inputs(
    env: &mut Env,
    providers: &mut Reserve,
    arrivals: &mut Arrivals,
    n: u32,
    request: &str,
) -> (Filing, Id, rutent_contracts::module_interface::Digest) {
    slow(env, 4 * 86_400);
    let deposited = deposit(env, providers, arrivals, 1000, n);
    let mut one = filing(reserve_filer(), &deposited);
    one.request = id(request);
    one.session = short_session(env, "session-short", START + 600);
    one.inputs
        .insert("payer.bank_account".into(), PAYER_ACCOUNT.into());
    let filed = file(env, providers, arrivals, &one).unwrap();
    let Filed::Waiting {
        order: Some(order), ..
    } = filed
    else {
        panic!("{filed:?}");
    };
    let (_, digest, _, _) = awaited(env, arrivals, &order).expect("its quote is shown");
    (one, order, digest)
}

/// Reopens `arrivals` over `history`, as a Core that stopped and opened again reads its own log.
fn reopened_over(env: &mut Env, providers: &mut Reserve, history: &[(Receipt, Entry)]) -> Arrivals {
    env.with_providers(providers, |_, desk| {
        Arrivals::reopened(history, desk.kept).with_deposit_wait(Count::new(WAIT).unwrap())
    })
}

/// The hold arrivals of `history`, with the digest each names its filing's statement by, set by `change`.
fn with_statements(
    history: &[(Receipt, Entry)],
    change: &dyn Fn(
        &Id,
        Option<rutent_contracts::module_interface::Digest>,
    ) -> Option<rutent_contracts::module_interface::Digest>,
) -> Vec<(Receipt, Entry)> {
    history
        .iter()
        .cloned()
        .map(|(receipt, mut entry)| {
            if let Entry::Verified(v) = &mut entry
                && let What::Arrival {
                    order: Some(order),
                    inputs,
                    earlier: Some(_),
                    ..
                } = &mut v.what
            {
                *inputs = change(order, inputs.take());
            }
            (receipt, entry)
        })
        .collect()
}

/// A waiting deposit whose filing carried an input of its own is accepted after Core reopens with no new filing: the
/// log holds the operator's statement of the filing's inputs and objective behind a commitment alone (no account in
/// it), the reopened wait reads them back, asks the route again with them, and the customer accepts the quote shown
/// anew, the filer never filing again (`REQ-QTE-10`, `REQ-AUD-6`).
// verifies: REQ-QTE-10, REQ-AUD-6
#[test]
fn waiting_deposit_is_accepted_after_core_reopens_with_no_new_filing() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = short_waits();
    let (one, order, digest) =
        waiting_with_inputs(&mut env, &mut providers, &mut arrivals, 0, "deposit-1");

    // The log names the filing's request and the commitment of its statement, and holds no account of it.
    let held: Vec<_> = env
        .book
        .borrow()
        .verified()
        .into_iter()
        .filter_map(|w| match w {
            What::Arrival {
                order: Some(o),
                request,
                inputs,
                ..
            } if o == &order => Some((request.clone(), inputs.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].0, Some(one.request.clone()));
    assert!(held[0].1.is_some(), "the statement's commitment");
    let log: String = entries(&env)
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    assert!(!log.contains(PAYER_ACCOUNT), "no account in the log");
    assert_admitted(&env);

    let history = env.history();
    let mut reopened = reopened_over(&mut env, &mut providers, &history);
    let filing = reopened.filing_of(&order).expect("the wait's filing");
    assert_eq!(filing.request, one.request);
    assert_eq!(
        filing.inputs.get("payer.bank_account").map(String::as_str),
        Some(PAYER_ACCOUNT),
        "its inputs read back"
    );
    assert_eq!(filing.route.objective, one.route.objective);

    // No new filing: the customer asks for the quote it was shown, is shown the route anew, and accepts that.
    env.clock.set(START + 700);
    let later = opened_session(&mut env, "session-later", START + 10_000);
    let asked = accept_in(
        &mut env,
        &mut providers,
        &mut reopened,
        &order,
        &customer(),
        &later,
        &digest,
    )
    .unwrap();
    assert_eq!(
        asked,
        Filed::Waiting {
            order: Some(order.clone()),
            wait: FiledWait::CustomerAcceptance
        }
    );
    let (_, second, _, _) = awaited(&env, &reopened, &order).expect("a quote shown anew");
    assert_ne!(second, digest);
    let accepted = accept_in(
        &mut env,
        &mut providers,
        &mut reopened,
        &order,
        &customer(),
        &later,
        &second,
    )
    .unwrap();
    assert!(matches!(accepted, Filed::Started { .. }), "{accepted:?}");
    assert_admitted(&env);
}

/// A reopened wait whose statement can't be read back (the store keeps none under the commitment, the arrival names
/// none, or it is another filing's) stays held: it is told it waits, never accepted, never asked the route again, and
/// ends at its lifetime like any wait (`REQ-QTE-10`).
// verifies: REQ-QTE-10, REQ-AUD-6
#[test]
fn reopened_waiting_deposit_whose_statement_cannot_be_read_stays_held() {
    for why in [
        "the store keeps nothing behind it",
        "the arrival names no statement",
        "the statement is another order's",
    ] {
        let mut env = crate::env();
        let mut providers = Reserve::new(&env);
        let mut arrivals = short_waits();
        let (_, first, first_digest) =
            waiting_with_inputs(&mut env, &mut providers, &mut arrivals, 0, "deposit-1");
        let (_, second, _) =
            waiting_with_inputs(&mut env, &mut providers, &mut arrivals, 1, "deposit-2");
        let history = env.history();
        let statement_of = |order: &Id| {
            history.iter().find_map(|(_, e)| match e {
                Entry::Verified(v) => match &v.what {
                    What::Arrival {
                        order: Some(o),
                        inputs: Some(i),
                        earlier: Some(_),
                        ..
                    } if o == order => Some(i.clone()),
                    _ => None,
                },
                _ => None,
            })
        };
        let nothing: rutent_contracts::module_interface::Digest =
            serde_json::from_value(json!({ "algorithm": "sha-256", "value": "99".repeat(32) }))
                .unwrap();
        let others = statement_of(&second).expect("the other filing's statement");
        let altered = with_statements(&history, &|order, own| {
            if order != &first {
                return own;
            }
            match why {
                "the store keeps nothing behind it" => Some(nothing.clone()),
                "the arrival names no statement" => None,
                _ => Some(others.clone()),
            }
        });
        let mut reopened = reopened_over(&mut env, &mut providers, &altered);
        let later = opened_session(&mut env, "session-later", START + 10_000);
        env.clock.set(START + 700);
        let before = env.len();
        for _ in 0..2 {
            let told = accept_in(
                &mut env,
                &mut providers,
                &mut reopened,
                &first,
                &customer(),
                &later,
                &first_digest,
            )
            .unwrap();
            assert_eq!(
                told,
                Filed::Waiting {
                    order: Some(first.clone()),
                    wait: FiledWait::CustomerAcceptance
                },
                "{why}"
            );
        }
        assert_eq!(env.len(), before, "{why}: nothing asked, shown or recorded");
        assert!(
            env.acceptances().is_empty() && env.started().is_empty(),
            "{why}"
        );
        // It still waits, and ends at its lifetime, as every wait does.
        let lasts = reopened.wait_ends(&first).expect("{why}: it still waits");
        env.clock.set(seconds(&lasts));
        let ended = env
            .with_providers(&mut providers, |_, desk| reopened.end_expired(desk))
            .unwrap();
        assert!(ended.contains(&first), "{why}");
    }
}

/// A filing restated after a reopen changes nothing the log, the operator's kept statement and the authorisation fixed:
/// another destination, another payer account and another objective are told the wait as it stands, the order starts on
/// the destination policy authorised, with the inputs and objective the operator stated, and no entry holds what the
/// restatement named (`REQ-AUD-6`, `REQ-AUZ-3`).
// verifies: REQ-AUZ-3, REQ-AUD-6
#[test]
fn filing_restated_after_a_reopen_changes_nothing_the_statement_fixed() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = short_waits();
    let (one, order, digest) =
        waiting_with_inputs(&mut env, &mut providers, &mut arrivals, 0, "deposit-1");
    let history = env.history();
    let mut reopened = reopened_over(&mut env, &mut providers, &history);
    let fixed = reopened
        .filing_of(&order)
        .expect("the wait's filing")
        .clone();

    let mut restated = one.clone();
    restated.destination.account = id("acct-attacker-dest");
    restated
        .inputs
        .insert("payer.bank_account".into(), "acct-attacker-payer".into());
    restated.route.objective = rutent_contracts::router_api::Objective::FastestSettlement {};
    env.clock.set(START + 700);
    let told = file(&mut env, &mut providers, &mut reopened, &restated).unwrap();
    assert!(matches!(told, Filed::Waiting { .. }), "{told:?}");
    let now = reopened.filing_of(&order).expect("the wait's filing");
    assert_eq!(now.destination, fixed.destination);
    assert_eq!(now.inputs, fixed.inputs);
    assert_eq!(now.route.objective, fixed.route.objective);

    let later = opened_session(&mut env, "session-later", START + 10_000);
    accept_in(
        &mut env,
        &mut providers,
        &mut reopened,
        &order,
        &customer(),
        &later,
        &digest,
    )
    .unwrap();
    let (_, second, _, _) = awaited(&env, &reopened, &order).expect("a quote shown anew");
    let started = accept_in(
        &mut env,
        &mut providers,
        &mut reopened,
        &order,
        &customer(),
        &later,
        &second,
    )
    .unwrap();
    assert!(matches!(started, Filed::Started { .. }), "{started:?}");
    let log: String = entries(&env)
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    assert!(!log.contains("acct-attacker"), "nothing of the restatement");
    for w in env.book.borrow().verified() {
        if let What::Authorise { authorisation, .. } = w
            && authorisation.order == order
        {
            assert_eq!(authorisation.destination, fixed.destination);
        }
    }
    assert_admitted(&env);
}

/// A reopened wait reads the filing's judgements (`needs`) back from the operator's kept statement, as it reads the
/// inputs and objective: a need on a tuple that isn't on the route, so routing asks no person question, comes back
/// whole (`REQ-AUD-6`, `REQ-QTE-10`).
// verifies: REQ-AUD-6, REQ-QTE-10
#[test]
fn reopened_wait_reads_the_filings_needs_back_from_the_statement() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let mut arrivals = short_waits();
    slow(&mut env, 4 * 86_400);
    let deposited = deposit(&mut env, &mut providers, &mut arrivals, 1000, 0);
    let mut one = filing(reserve_filer(), &deposited);
    one.session = short_session(&mut env, "session-short", START + 600);
    one.needs = vec![rutent_core_orch::rounds::Need {
        tuple: snapshot_support::deposit_tuple("provider-off-the-route"),
        person: id("person-ref-8"),
        verifier: id("kyc-check"),
        subject: "subject-8".into(),
        class: "kyc".into(),
    }];
    let filed = file(&mut env, &mut providers, &mut arrivals, &one).unwrap();
    let Filed::Waiting {
        order: Some(order), ..
    } = filed
    else {
        panic!("{filed:?}");
    };
    let history = env.history();
    let reopened = reopened_over(&mut env, &mut providers, &history);
    let read = reopened.filing_of(&order).expect("the wait's filing");
    assert_eq!(read.needs, one.needs);
}

/// A refiling built for one filer, whose append failed, is appended again only on that filer's own call: another filer
/// is asked of policy on its own, and one policy refuses is refused, never told the arrival was filed anew on a permit
/// that wasn't its own (`T-020-52`, the review of `96d161a0`).
// verifies: REQ-ORD-23, REQ-AUZ-1
#[test]
fn a_built_refiling_is_reused_only_by_the_filer_it_was_built_for() {
    let mut env = crate::env();
    let mut providers = Reserve::new(&env);
    let order = explicit_order(&mut env, &mut providers);
    let mut arrivals = Arrivals::new();
    let first = match arrive(
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
    // The operator's refiling is permitted and built, and the log can't take it now.
    env.book.borrow_mut().fail_next = Some("other");
    assert!(matches!(
        refile(&mut env, &mut providers, &mut arrivals, &operator(), &first),
        Err(ArrivalError::Log(_))
    ));
    // Policy refuses the reserve: it is told so, asked on its own, and nothing is appended on the operator's permit.
    env.deciding.refuse = true;
    let asked = env.deciding.asked.len();
    assert_eq!(
        refile(
            &mut env,
            &mut providers,
            &mut arrivals,
            &reserve_filer(),
            &first
        ),
        Err(ArrivalError::Refused {
            class: OrderRefusalClass::RefusedByPolicy
        })
    );
    assert_eq!(env.deciding.asked.len(), asked + 1, "policy was asked");
    assert!(
        !env.book
            .borrow()
            .verified()
            .iter()
            .any(|w| matches!(w, What::Refiling { .. })),
        "no refiling is appended"
    );
    // The operator's own call carries its built refiling on, with no second question.
    env.deciding.refuse = false;
    let asked = env.deciding.asked.len();
    let receipt = refile(&mut env, &mut providers, &mut arrivals, &operator(), &first).unwrap();
    assert_eq!(
        env.deciding.asked.len(),
        asked,
        "its permit is the one it built on"
    );
    let Entry::Verified(v) = at(&env, &receipt) else {
        panic!("a refiling is a verified entry");
    };
    assert_eq!(v.who, operator());
    assert_admitted(&env);
}
