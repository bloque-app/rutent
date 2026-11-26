//! The production mover's hand-on arm (`T-020-17`): a stuck part's value handed on to a linked order, carried out by
//! the holding account's own withdrawal for the linked order over the linked order's accepted route, in place of the
//! tests' `Moves`. The holding account is a stand-in that behaves as the plain wallet does through
//! `Holdings::start_handed_on`: it is shown the operator's signed statement through the mover's rule, and is handed
//! it only when the mover allows. The real wallet's answer to that rule is `components/prov`'s (`T-016-03`); the
//! scenario that runs it with the assembled Core is `T-024-06`'s.
//!
//! Over `T-020-08`'s kit and the real log's rules: every history is replayed through `admit_entry`.

mod segments_support;

use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::calls::StuckOrders;
use rutent_contracts::core_internal::{
    Action, Actor, Decided, Entry, Receipt, Resolution, Role, What,
};
use rutent_contracts::module_interface::PlatformRefusal;
use rutent_contracts::provider_role::{
    Denomination, NodeRef, Operation as ProviderOperation, OuterNode, Statement, Withdrawal,
};
use rutent_contracts::router_api::{Id, PolicyBounds, PositiveAmount, Timestamp};
use rutent_contracts::state_machine::{OrderState, PartPlan, SourceKind};
use rutent_core_orch::hand_on::{Basis, HandOn, HandOnError, HandedOn, Handing, Unproven};
use rutent_core_orch::recalc::Recalculation;
use rutent_core_orch::segments::{NoProxy, Ran, SegmentOutcome, SegmentsError};
use rutent_core_orch::snapshot::Clock;
use rutent_core_orch::stuck::{
    AcceptedRoute, Carrying, Confirming, Deciding, HandingOn, LinkedRoutes, LogRead, Moved, Mover,
    Moving, NoRouteLeft, NotTaken, OperatorSigns, ResolveError, Resolved, Show, Stated, Stuck,
    Withdrawals,
};
use rutent_prov::holding::wallet::{
    HandedOnStart, Operator as WalletOperator, StartError, WalletError, WithdrawalRecord,
};
use segments_support::handing::{
    from_the_ledger, hand_parts, linked_record, linked_route, prove, prove_citing,
    recorded_with_receipts, wallet_answer, wallet_succeeded,
};
use segments_support::rerouting::Rerouting;
use segments_support::resolving::{
    PermitsEveryStep, follow, on_parts, on_platform, on_platform_handing, staff,
};
use segments_support::*;

const LINKED: &str = "ord-2";
const SEGMENT: &str = "p1-s1-of-2";

fn cop_unit() -> Denomination {
    Denomination {
        id: "COP".into(),
        decimals: 2,
    }
}

fn ledger_node() -> String {
    format!("{}-ledger", origin_reserve())
}

// The holding account -----------------------------------------------------------------------------------------

/// What the holding account's stand-in answers once the mover let it be handed the operator's statement.
#[derive(Clone)]
enum Script {
    /// It withdraws and records it, as the plain wallet does.
    Pays,
    /// It withdraws, and its record is changed by this.
    PaysRecorded(fn(&mut WithdrawalRecord)),
    /// It answers with this error.
    Answers(StartError),
    /// It builds a statement of another amount than it was asked for.
    Tampers,
    /// It withdraws and records it, but its signed answer says something else than its record does: this changes
    /// the answer's outcome.
    AnswersOtherwise(fn(&mut serde_json::Value)),
}

/// A node's holding account as `Holdings::start_handed_on` drives it, with what it was shown and handed.
struct Wallet {
    node: String,
    holder: Option<String>,
    /// The provider whose wallet the account is: who it signs as and records its withdrawal by.
    account: String,
    script: Script,
    key: TestKey,
    bounds: PolicyBounds,
    maker: Id,
    made_at: Timestamp,
    sent: u64,
    /// How often the mover was shown a statement.
    shown: usize,
    /// A clock the wallet makes unreadable once it was handed the statement.
    breaks_clock: Option<std::rc::Rc<TestClock>>,
    /// A log that stops attributing statements once the wallet was handed the statement: its answer is recorded
    /// unattributed.
    unattributes: Option<std::rc::Rc<std::cell::RefCell<TestLog>>>,
    /// The statements the wallet was handed.
    handed: Vec<Statement<ProviderOperation>>,
}

impl Wallet {
    fn of(core: &Core, script: Script) -> Self {
        Wallet {
            node: ledger_node(),
            holder: Some(origin_reserve().to_owned()),
            account: origin_reserve().to_owned(),
            script,
            key: TestKey(core.key.0.clone()),
            bounds: core.record.bounds.clone(),
            maker: core.operator.acts.clone(),
            made_at: Timestamp::new(rutent_prov::catalogue::Instant(START).rfc3339().unwrap())
                .unwrap(),
            sent: 0,
            shown: 0,
            breaks_clock: None,
            unattributes: None,
            handed: Vec::new(),
        }
    }
}

impl Wallet {
    /// [`Wallet::of`], the wallet of `account`, another provider than the reserve the linked route starts with: the
    /// holding account of a node that runs no route (`T-020-33`).
    fn held_by(core: &Core, script: Script, account: &str) -> Self {
        Wallet {
            holder: Some(account.to_owned()),
            account: account.to_owned(),
            ..Wallet::of(core, script)
        }
    }
}

impl Withdrawals for Wallet {
    fn holder(&self, node: &str) -> Option<String> {
        (node == self.node).then(|| self.holder.clone()).flatten()
    }

    fn withdraw(
        &mut self,
        route: &[rutent_contracts::state_machine::SegmentPlan],
        order: &str,
        amount: &str,
        asked: &mut Show<'_>,
    ) -> Result<HandedOnStart, StartError> {
        let first = route.first().ok_or(StartError::NotHandedOn)?;
        let NodeRef::Inner(node) = &first.from.node else {
            return Err(StartError::NotHandedOn);
        };
        if *node != self.node {
            return Err(StartError::ValueNotAtStart(node.clone()));
        }
        self.sent += 1;
        let account = self.account.clone();
        let transaction = format!("{account}-withdrawal-{}", self.sent);
        let stated = if matches!(self.script, Script::Tampers) {
            "1"
        } else {
            amount
        };
        let operation = ProviderOperation::Withdrawal(Withdrawal {
            transaction: transaction.clone(),
            provider: account.clone(),
            session: None,
            origin: OuterNode {
                network: origin_reserve().to_owned(),
            },
            destination: node.clone(),
            asset: first.from.asset.id.clone(),
            amount: stated.to_owned(),
            dry_run: false,
            inputs: [("order".to_owned(), order.to_owned())].into(),
        });
        let signer = OperatorSigns::new(
            &mut self.key,
            &self.bounds,
            self.maker.clone(),
            self.made_at.clone(),
        );
        let statement = signer.statement(operation);
        self.shown += 1;
        if !asked(&statement) {
            return Err(StartError::Wallet(WalletError::NotAnswered(
                PlatformRefusal::NotHandled,
            )));
        }
        self.handed.push(statement);
        if let Some(clock) = &self.breaks_clock {
            clock.set(300_000_000_000);
        }
        if let Some(log) = &self.unattributes {
            log.borrow_mut().stop_attributing();
        }
        let mut record = WithdrawalRecord {
            by: account.clone(),
            node: node.clone(),
            asset: first.from.asset.id.clone(),
            order: order.to_owned(),
            amount: amount.parse().unwrap(),
            reference: format!("{account}-withdrawal-{transaction}"),
        };
        let mut outcome = wallet_succeeded(&account, &transaction, amount);
        if let Script::AnswersOtherwise(change) = &self.script {
            change(&mut outcome);
        }
        match &self.script {
            Script::Pays | Script::Tampers | Script::AnswersOtherwise(_) => {}
            Script::PaysRecorded(change) => change(&mut record),
            Script::Answers(error) => return Err(error.clone()),
        }
        // The wallet's own signed answer, as the plain wallet's is: a withdrawal result under its own transaction.
        let answer = wallet_answer(&account, &transaction, self.made_at.as_str(), outcome);
        Ok(HandedOnStart {
            reserve: account.clone(),
            node: node.clone(),
            first_segment: first.segment_id.clone(),
            record,
            transaction,
            answer,
        })
    }
}

/// The linked orders' accepted routes, as the assembly would keep them.
struct Routes(Option<AcceptedRoute>);

impl LinkedRoutes for Routes {
    fn accepted(&self, _linked: &Id) -> Option<AcceptedRoute> {
        self.0.clone()
    }
}

/// The log's entries, as they stand when the move is asked.
struct Entries(Vec<Entry>);

impl LogRead for Entries {
    fn entries(&self) -> Vec<Entry> {
        self.0.clone()
    }
}

fn accepted(core: &Core) -> AcceptedRoute {
    AcceptedRoute {
        order: id(LINKED),
        decision_record: linked_record(LINKED),
        acceptance: id("acc-ord-2"),
        plan: from_the_ledger(core, SEGMENT),
    }
}

// The kit's stuck order ---------------------------------------------------------------------------------------

/// The kit's order with its withdrawal settled and its deposit never begun, then left no route: stuck, its value
/// resting at the reserve's ledger; and the linked order `ord-2` accepted by the original customer.
fn stuck_with_a_linked_order() -> (World, Core, Stuck) {
    stuck_with_a_linked_order_in(World::of(&[RESERVE, OTHER_RESERVE]))
}

/// [`stuck_with_a_linked_order`], in `world`: in the two-party world ([`World::two_parties`], `T-020-39`) the
/// route's deposit is another party's, whose word about the hand-on is a party's word.
fn stuck_with_a_linked_order_in(mut world: World) -> (World, Core, Stuck) {
    let mut core = Core::over(&mut world);
    // The log attributes each statement, as the real one does.
    core.log.borrow_mut().attribute_from_now();
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.refuse_begin = Some(1);
    let ran = core.run_over(&world.catalogue, &mut watched);
    drop(watched);
    let rutent_core_orch::segments::Ran::Segments(done) = &ran else {
        panic!("the part ran: {ran:?}");
    };
    assert!(matches!(done[0].outcome, SegmentOutcome::Settled { .. }));
    world
        .fleet
        .world()
        .script_dry_run(last_reserve(), "deposit", Behaviour::act(Act::Refuse));
    let outcome = Rerouting::of(&core)
        .recalculate(&mut core, &mut world)
        .expect("the recalculation runs");
    assert!(matches!(outcome, Recalculation::NoRoute { .. }));
    let mut stuck = Stuck::new();
    let (order, part, last) = (id(ORDER), id(PART), core.record.record_id().clone());
    let actor = segments_support::core();
    let declared = on_parts(&mut core, |parts| {
        stuck.no_route(
            parts,
            NoRouteLeft {
                order: &order,
                part: &part,
                outcome: &outcome,
                last_record: &last,
                core: &actor,
            },
        )
    })
    .expect("recorded");
    assert_eq!(declared.state, OrderState::Stuck);
    follow(&core, &mut stuck).expect("its ends are Core's");
    seed_linked(
        &mut core,
        &Named::another(2),
        &customer(),
        &linked_record(LINKED),
    );
    (world, core, stuck)
}

fn hand_on_resolution() -> Resolution {
    hand_on_resolution_by(origin_reserve())
}

fn hand_on_resolution_by(by: &str) -> Resolution {
    Resolution::HandOn {
        by: id(by),
        linked_order: id(LINKED),
        asset: cop_unit(),
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    }
}

fn decide(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut dyn StuckOrders,
    decider: &Actor,
) -> Decided {
    decide_by(core, stuck, policy, decider, origin_reserve())
}

/// [`decide`], the decision naming `by` as the holder.
fn decide_by(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut dyn StuckOrders,
    decider: &Actor,
    by: &str,
) -> Decided {
    try_decide_by(core, stuck, policy, decider, by).expect("decided")
}

/// [`decide_by`], where the decide step may refuse.
fn try_decide_by(
    core: &mut Core,
    stuck: &mut Stuck,
    policy: &mut dyn StuckOrders,
    decider: &Actor,
    by: &str,
) -> Result<Decided, ResolveError> {
    let (order, part) = (id(ORDER), id(PART));
    let evidence = own_evidence(core);
    let resolution = hand_on_resolution_by(by);
    let history = core.recorded();
    on_parts(core, |parts| {
        stuck.decide(
            parts,
            policy,
            Deciding {
                order: &order,
                part: &part,
                decider,
                resolution: &resolution,
                evidence: &evidence,
            },
            &history,
        )
    })
    .map(|decision| Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    })
}

/// Asks the move step of the kit's order, by `by`, with the production mover over the world's platform, the holding
/// account `wallet` and the accepted routes `routes`.
#[allow(clippy::too_many_arguments)]
fn move_it(
    core: &mut Core,
    world: &mut World,
    stuck: &mut Stuck,
    mover: &mut Mover,
    policy: &mut PermitsEveryStep,
    carries_out: &Decided,
    by: &Actor,
    wallet: &mut Wallet,
    routes: &Routes,
) -> Result<Moved, ResolveError> {
    let (order, actor) = (id(ORDER), segments_support::core());
    let entries = Entries(
        core.log
            .borrow()
            .entries
            .iter()
            .map(|(e, _)| e.clone())
            .collect(),
    );
    let mut watched = Watched::new(&mut world.fleet.platform);
    on_platform_handing(
        core,
        &world.catalogue,
        &mut watched,
        mover,
        HandingOn {
            withdrawals: wallet,
            routes,
            log: &entries,
        },
        &mut NoProxy,
        |parts, delivering| {
            stuck.carry_out(
                parts,
                policy,
                delivering,
                Moving {
                    order: &order,
                    carries_out,
                    mover: by,
                    core: &actor,
                },
            )
        },
    )
}

fn carried_out(moved: Result<Moved, ResolveError>) -> (Receipt, Resolved) {
    match moved {
        Ok(Moved::CarriedOut {
            effect, resolved, ..
        }) => (effect, resolved),
        other => panic!("carried out: {other:?}"),
    }
}

/// The part is still stuck, its value held: nothing was resolved or handed on.
fn still_held(core: &Core) {
    let part = core
        .segments
        .order(&id(ORDER))
        .unwrap()
        .parts
        .iter()
        .find(|p| p.part_id.as_str() == PART)
        .unwrap()
        .clone();
    assert_eq!(part.state, OrderState::Stuck);
}

/// The statements about `order` under the holding account's own transactions, which the kit's seeding of the linked
/// order doesn't make.
fn stated_about(core: &Core, order: &str) -> Vec<(usize, Option<String>, String)> {
    core.log
        .borrow()
        .entries
        .iter()
        .enumerate()
        .filter_map(|(i, (e, _))| match e {
            Entry::Stated(s)
                if s.about
                    .as_ref()
                    .and_then(|a| a.order.as_ref())
                    .is_some_and(|o| o.as_str() == order) =>
            {
                Some((
                    i,
                    s.about
                        .as_ref()
                        .and_then(|a| a.transaction.as_ref())
                        .map(|t| t.as_str().to_owned()),
                    s.what.statement.as_str().to_owned(),
                ))
            }
            _ => None,
        })
        .filter(|(_, t, _)| t.as_deref().is_some_and(|t| t.contains("-withdrawal-")))
        .collect()
}

fn count(core: &Core, pick: impl Fn(&What) -> bool) -> usize {
    core.log.borrow().count(pick)
}

fn is_hand_on_effect(what: &What) -> bool {
    matches!(
        what,
        What::Effect {
            action: Action::ResolveStuck {
                resolution: Resolution::HandOn { .. },
                ..
            },
            ..
        }
    )
}

/// Hands the kit's stuck order's part on to `ord-2` on the operator's resolution `effect`, over `plan`.
fn operator_hand_on(
    core: &mut Core,
    hand_on: &mut HandOn,
    plan: &[PartPlan],
    effect: &Receipt,
    by: &Actor,
) -> Result<HandedOn, HandOnError> {
    let recorded = recorded_with_receipts(core);
    let (order, part, linked, record, acceptance) = (
        id(ORDER),
        id(PART),
        id(LINKED),
        linked_record(LINKED),
        id("acc-ord-2"),
    );
    let actor = segments_support::core();
    hand_parts(core, &mut Stuck::new(), |parts| {
        hand_on.hand_on(
            parts,
            Handing {
                order: &order,
                part: &part,
                linked: &linked,
                decision_record: &record,
                plan,
                acceptance: &acceptance,
                basis: Basis::Resolution { effect },
                recorded: &recorded,
                by,
                core: &actor,
            },
        )
    })
}

// The tests ---------------------------------------------------------------------------------------------------

/// A stuck part's value is handed on to the linked order by the holding account's own withdrawal, over the linked
/// order's accepted route, on a decision of one party and a move of another. Before the wallet is handed the
/// operator's statement, it is in the log as the operator's question about the linked order and the withdrawal's
/// transaction, attributed. The move answers `Done` from the account's own record, which the hand-on then takes its
/// route from and which proves the linked order's start: the linked order's route runs from where the value sat.
// verifies: REQ-ORD-43, REQ-ORD-40
#[test]
fn operators_hand_on_is_carried_out_by_the_holding_accounts_withdrawal_for_the_linked_order() {
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Done);

    // The wallet was handed one statement: the withdrawal decided, in full, at the node the value rests at.
    assert_eq!(wallet.handed.len(), 1);
    let sent = serde_json::to_value(&wallet.handed[0].statement).unwrap();
    assert_eq!(sent["kind"], "withdrawal");
    assert_eq!(sent["amount"], AMOUNT);
    assert_eq!(sent["destination"], ledger_node());
    assert_eq!(sent["provider"], origin_reserve());

    // The question is on record before the result: the operator's, about the linked order and the wallet's
    // transaction.
    let transaction = sent["transaction"].as_str().unwrap().to_owned();
    let asked = stated_about(&core, LINKED);
    assert_eq!(
        asked.len(),
        2,
        "the question, then the holding account's answer: {asked:?}"
    );
    assert_eq!(asked[0].1.as_deref(), Some(transaction.as_str()));
    assert_eq!(asked[0].2, rutent_contracts::provider_role::OPERATION_SHAPE);
    // The holding account's own signed answer, kept as its stated entry under its own transaction (`T-020-26`).
    assert_eq!(asked[1].1.as_deref(), Some(transaction.as_str()));
    assert_eq!(asked[1].2, rutent_contracts::provider_role::RESULT_SHAPE);
    let result_at = core
        .log
        .borrow()
        .entries
        .iter()
        .position(|(e, _)| {
            matches!(e, Entry::Verified(v) if matches!(&v.what, What::Result { effect: r } if *r == effect))
        })
        .expect("the effect's result");
    assert!(asked[0].0 < asked[1].0, "the question precedes the answer");
    assert!(asked[1].0 < result_at, "the answer precedes the result");

    // The withdrawal done is what the hand-on takes its route and its proof from.
    let withdrawn = mover.withdrawn(&effect).expect("the withdrawal is done");
    assert_eq!(withdrawn.linked.as_str(), LINKED);
    assert_eq!(withdrawn.route, &accepted(&core));
    assert_eq!(
        withdrawn.start.record.amount,
        AMOUNT.parse::<u128>().unwrap()
    );
    assert!(withdrawn.question.is_some());
    assert!(
        withdrawn.statement.is_some(),
        "the holding account's answer is on record"
    );
    let (plan, record) = (withdrawn.route.plan.clone(), withdrawn.start.record.clone());
    let mut hand_on = HandOn::new();
    operator_hand_on(&mut core, &mut hand_on, &plan, &effect, &bob).expect("handed on");
    prove(&mut core, &mut hand_on, &id(LINKED), &record)
        .expect("the holding account's own record proves the start");
    let linked = linked_route(&mut world, &core.clock, LINKED);
    let ran = run_linked(&mut core, &mut world, &linked).expect("the linked route runs");
    assert!(settled(&ran), "{ran:?}");
    assert_eq!(
        core.segments.order(&id(ORDER)).unwrap().parts[0].state,
        OrderState::HandedOn
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// Runs the kit's linked order, `ord-2`, over `record`, its route handed on.
fn run_linked(
    core: &mut Core,
    world: &mut World,
    record: &rutent_contracts::router_api::DecisionRecord,
) -> Result<Ran, SegmentsError> {
    let session = id(&world.context().session);
    let named = Named::another(2);
    let (order, part, auth, origin, end) = (
        id(&named.order),
        id(PART),
        id(&named.authorisation()),
        // A route handed on starts at the kit route's last reserve, which runs its first move (`T-020-39`).
        id(last_reserve()),
        route_end(),
    );
    let segments = [id(SEGMENT)];
    let (caller, core_actor, operator) = (
        segments_support::core(),
        segments_support::core(),
        core.operator.clone(),
    );
    let live = rutent_core_orch::segments::Live {
        order: &order,
        part: &part,
        segments: &segments,
        record,
        at: 0,
        authorisation: &auth,
        origin_reserve: &origin,
        route: &end,
        caller: &caller,
        core: &core_actor,
        operator: &operator,
        session: Some(&session),
        inputs: &core.inputs,
        floor: None,
        record_entry: None,
    };
    let mut general = General(std::rc::Rc::clone(&core.log));
    let mut parts = rutent_core_orch::segments::Parts {
        clock: &*core.clock,
        catalogue: &world.catalogue,
        log: &mut general,
        evidence: &mut core.store,
        decisions: &mut core.policy,
        modules: &mut world.fleet.platform,
        uses: &mut NoProxy,
        grading: &mut Recorded,
        operator_key: &mut core.key,
        fresh: &mut core.ids,
        issued: &core.issued,
        exposures: &mut core.exposures,
        channel: &mut rutent_core_orch::notice::Unbuilt,
    };
    core.segments.run_part(&mut parts, &live)
}

fn settled(ran: &Ran) -> bool {
    matches!(ran, Ran::Segments(done) if matches!(done[0].outcome, SegmentOutcome::Settled { .. }))
}

/// Only the linked order's own accepted route is withdrawn for: another order's route, none, a route that starts
/// elsewhere than where Core's records say the value rests, one with a withdrawal first, one of two parts, a route of
/// an order Core follows already, a holding account of another provider than the decision names, and the lack of any
/// holding account are each refused before the wallet is shown anything, and the part stays held.
// verifies: REQ-ORD-43
#[test]
fn hand_on_of_a_route_other_than_the_linked_orders_accepted_one_is_refused() {
    type Alter = fn(&Core, &mut AcceptedRoute);
    let cases: [(&str, Alter); 8] = [
        ("another order's route", |_, r| r.order = id("ord-3")),
        ("a route starting at another node", |_, r| {
            r.plan[0].segments[0].from.node = NodeRef::Inner("elsewhere".into());
        }),
        ("a route starting in another asset", |_, r| {
            r.plan[0].segments[0].from.asset.id = "USD".into();
        }),
        ("a route that starts with a withdrawal", |_, r| {
            r.plan[0].segments[0].kind = rutent_contracts::state_machine::OperationKind::Withdrawal;
        }),
        ("a route citing no decision record of the log's", |_, r| {
            r.decision_record = id("dr-unrecorded");
        }),
        ("a route naming no acceptance of the log's", |_, r| {
            r.acceptance = id("acc-unrecorded");
        }),
        (
            "a first segment of another provider than the decision names",
            |_, r| {
                r.plan[0].segments[0].provider =
                    rutent_contracts::state_machine::Id::new(second_reserve()).unwrap();
            },
        ),
        ("a route of two parts", |_, r| {
            let again = r.plan[0].clone();
            r.plan.push(again);
        }),
    ];
    for (why, alter) in cases {
        let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
        let mut mover = Mover::new();
        let mut policy = PermitsEveryStep::new(&core.log);
        let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
        let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
        let mut wallet = Wallet::of(&core, Script::Pays);
        let mut route = accepted(&core);
        alter(&core, &mut route);
        let moved = move_it(
            &mut core,
            &mut world,
            &mut stuck,
            &mut mover,
            &mut policy,
            &carries_out,
            &bob,
            &mut wallet,
            &Routes(Some(route)),
        );
        let (effect, resolved) = carried_out(moved);
        assert_eq!(resolved, Resolved::Failed, "{why}");
        assert_eq!(wallet.shown, 0, "{why}: the wallet is shown nothing");
        assert!(wallet.handed.is_empty(), "{why}");
        assert!(mover.withdrawn(&effect).is_none(), "{why}");
        assert!(stated_about(&core, LINKED).is_empty(), "{why}: no question");
        still_held(&core);
    }
    // No accepted route at all; no holding account at the node; another provider's holding account.
    for (why, routes, holder) in [
        (
            "no accepted route",
            false,
            Some(origin_reserve().to_owned()),
        ),
        ("no holding account", true, None),
        (
            "another provider's account",
            true,
            Some(second_reserve().to_owned()),
        ),
    ] {
        let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
        let mut mover = Mover::new();
        let mut policy = PermitsEveryStep::new(&core.log);
        let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
        let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
        let mut wallet = Wallet::of(&core, Script::Pays);
        wallet.holder = holder;
        let routes = Routes(routes.then(|| accepted(&core)));
        let moved = move_it(
            &mut core,
            &mut world,
            &mut stuck,
            &mut mover,
            &mut policy,
            &carries_out,
            &bob,
            &mut wallet,
            &routes,
        );
        let (_, resolved) = carried_out(moved);
        assert_eq!(resolved, Resolved::Failed, "{why}");
        assert_eq!(wallet.shown, 0, "{why}");
        still_held(&core);
    }
}

/// The hand-on keeps the original customer as the linked order's requester and records the operator acting: the
/// move's effect is the mover's, an operator's login under `REQ-AUZ-4`, the hand-on is the operator's word, and the
/// operator's question to the holding account is the operator's, not the mover's or Core's. The linked order's
/// acceptance names the first order's requester, which the log holds (`hand-on-rests-on-the-linked-orders-acceptance`):
/// a linked order accepted for another requester is refused by it at the effect, not by the mover.
// verifies: REQ-ORD-13, REQ-AUZ-4
#[test]
fn hand_on_keeps_the_original_customer_as_requester_and_records_the_operator_acting() {
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Done);
    let withdrawn = mover.withdrawn(&effect).expect("done");
    let plan = withdrawn.route.plan.clone();
    let mut hand_on = HandOn::new();
    let handed =
        operator_hand_on(&mut core, &mut hand_on, &plan, &effect, &bob).expect("handed on");
    // The hand-on is the operator's word, as the mover's login.
    assert_eq!(handed.handed_on.source.kind, SourceKind::Operator);
    assert_eq!(handed.handed_on.source.id.as_str(), bob.acts.as_str());
    // The effect names the operator login that moved it, and the question is the operator's own.
    let log = core.log.borrow();
    let moved_by = log
        .verified()
        .iter()
        .find_map(|v| match &v.what {
            What::StuckMove { request, .. } => Some(request.mover.clone()),
            _ => None,
        })
        .expect("the move step's record");
    assert_eq!(moved_by.role, Role::Operator);
    assert_eq!(moved_by, bob);
    assert!(log.verified().iter().any(|v| is_hand_on_effect(&v.what)));
    let question_by = log.entries.iter().find_map(|(e, _)| match e {
        Entry::Stated(s)
            if s.about
                .as_ref()
                .and_then(|a| a.order.as_ref())
                .is_some_and(|o| o.as_str() == LINKED) =>
        {
            match &s.attribution {
                rutent_contracts::core_internal::Attribution::Unverified { claimed_by } => {
                    Some(claimed_by.clone())
                }
                _ => None,
            }
        }
        _ => None,
    });
    assert_eq!(question_by.as_ref(), Some(&core.operator));
    drop(log);
    // The linked order keeps the original customer as its requester.
    let entries: Vec<Entry> = core
        .log
        .borrow()
        .entries
        .iter()
        .map(|(e, _)| e.clone())
        .collect();
    assert_eq!(
        rutent_core_orch::hand_on::requester_of(&entries, &id(LINKED)),
        rutent_core_orch::hand_on::requester_of(&entries, &id(ORDER)),
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");

    // A linked order accepted for another requester: the mover has no view of the log's acceptances, and the log
    // refuses the effect that hands the value on to it.
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    seed_linked(
        &mut core,
        &Named::another(3),
        &staff("desk-x", "mallory"),
        &linked_record("ord-3"),
    );
    let mut policy = PermitsEveryStep::new(&core.log);
    let mut mover = Mover::new();
    let (order, part) = (id(ORDER), id(PART));
    let resolution = Resolution::HandOn {
        by: id(origin_reserve()),
        linked_order: id("ord-3"),
        asset: cop_unit(),
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    let evidence = own_evidence(&mut core);
    let history = core.recorded();
    let decision = on_parts(&mut core, |parts| {
        stuck.decide(
            parts,
            &mut policy,
            Deciding {
                order: &order,
                part: &part,
                decider: &alice,
                resolution: &resolution,
                evidence: &evidence,
            },
            &history,
        )
    })
    .expect("decided");
    let carries_out = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    let mut wallet = Wallet::of(&core, Script::Pays);
    let mut route = accepted(&core);
    route.order = id("ord-3");
    let _ = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &Routes(Some(route)),
    );
    let refused = core
        .admitted(&world.catalogue)
        .expect_err("the log refuses it");
    assert!(
        refused
            .1
            .contains("keeps the requester of the order handed on"),
        "{refused:?}"
    );
}

/// A wallet's record that proves nothing of this start, a wrong reserve, node, asset, order or amount, leaves the
/// withdrawal unknown, not done: the value may have left the account, so the part is held and the hand-on has nothing
/// to take (`NoDoneResolution`). A route handed on whose start is not proven stays refused
/// (`SegmentsError::StartUnproven`) however it was withdrawn, until the holding account's own record proves it.
// verifies: REQ-ORD-40, INV-5
#[test]
fn hand_on_with_an_unproven_start_is_refused() {
    type Wrong = fn(&mut WithdrawalRecord);
    let wrong: [Wrong; 5] = [
        |r| r.by = second_reserve().into(),
        |r| r.node = format!("{}-ledger", second_reserve()),
        |r| r.asset = "USD".into(),
        |r| r.order = "ord-7".into(),
        |r| r.amount -= 1,
    ];
    for change in wrong {
        let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
        let mut mover = Mover::new();
        let mut policy = PermitsEveryStep::new(&core.log);
        let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
        let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
        let mut wallet = Wallet::of(&core, Script::PaysRecorded(change));
        let routes = Routes(Some(accepted(&core)));
        let moved = move_it(
            &mut core,
            &mut world,
            &mut stuck,
            &mut mover,
            &mut policy,
            &carries_out,
            &bob,
            &mut wallet,
            &routes,
        );
        let (effect, resolved) = carried_out(moved);
        assert_eq!(resolved, Resolved::Unknown);
        assert!(mover.withdrawn(&effect).is_none());
        still_held(&core);
        // Nothing to hand on: the effect's result isn't done.
        let mut hand_on = HandOn::new();
        let plan = accepted(&core).plan;
        assert_eq!(
            operator_hand_on(&mut core, &mut hand_on, &plan, &effect, &bob).map(|_| ()),
            Err(HandOnError::NoDoneResolution)
        );
    }
    // Withdrawn well, handed on, and not proven: the linked order's segments are refused until the account's own
    // record proves the start, and a record of another amount doesn't.
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Done);
    let withdrawn = mover.withdrawn(&effect).expect("done");
    let (plan, record) = (withdrawn.route.plan.clone(), withdrawn.start.record.clone());
    let mut hand_on = HandOn::new();
    operator_hand_on(&mut core, &mut hand_on, &plan, &effect, &bob).expect("handed on");
    let linked = linked_route(&mut world, &core.clock, LINKED);
    let handled = world.fleet.platform.operations_received(origin_reserve());
    assert_eq!(
        run_linked(&mut core, &mut world, &linked).map(|_| ()),
        Err(SegmentsError::StartUnproven(id(LINKED)))
    );
    assert_eq!(
        world.fleet.platform.operations_received(origin_reserve()),
        handled
    );
    let mut short = record.clone();
    short.amount -= 1;
    assert_eq!(
        prove(&mut core, &mut hand_on, &id(LINKED), &short),
        Err(HandOnError::Unproven(Unproven::AnotherAmount))
    );
    prove(&mut core, &mut hand_on, &id(LINKED), &record)
        .expect("the account's own record proves it");
    let ran = run_linked(&mut core, &mut world, &linked).expect("the linked route runs");
    assert!(settled(&ran), "{ran:?}");
}

/// A hand-on is carried out once: the log can't take the question, then the answer's hand-over entries, then the
/// result, each time on every try; the move goes on from where it stopped each time and the wallet is handed one
/// statement in all. A mover with no memory of an effect acted on answers `Unknown` and hands the wallet nothing
/// again; one decision is spent by one move.
// verifies: INV-8
#[test]
fn hand_on_is_carried_out_once() {
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    let routes = Routes(Some(accepted(&core)));
    let failing = |core: &Core, on: On| {
        for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
            core.log
                .borrow_mut()
                .faults
                .push_back((on, Fault::Unavailable));
        }
    };
    macro_rules! attempt {
        () => {
            move_it(
                &mut core,
                &mut world,
                &mut stuck,
                &mut mover,
                &mut policy,
                &carries_out,
                &bob,
                &mut wallet,
                &routes,
            )
        };
    }
    // The question can't be appended: the wallet is handed nothing.
    failing(&core, On::StatedOperation);
    assert!(matches!(attempt!(), Err(ResolveError::Append(_))));
    assert!(
        wallet.handed.is_empty(),
        "nothing before the question is on record"
    );
    // What the platform saw can't be appended: the wallet was handed the statement, once.
    failing(&core, On::HandOver);
    let tried = attempt!();
    assert!(matches!(tried, Err(ResolveError::Append(_))), "{tried:?}");
    assert_eq!(wallet.handed.len(), 1);
    // However often it is tried again, nothing more is handed over.
    failing(&core, On::Effect);
    let (effect, resolved) = loop {
        match attempt!() {
            Ok(Moved::CarriedOut {
                effect, resolved, ..
            }) => break (effect, resolved),
            Err(ResolveError::Append(_)) => {}
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(resolved, Resolved::Done);
    assert_eq!(wallet.handed.len(), 1, "the wallet is handed it once");
    assert_eq!(
        stated_about(&core, LINKED).len(),
        2,
        "the question and the answer are one entry each"
    );
    assert_eq!(count(&core, is_hand_on_effect), 1);
    assert!(mover.withdrawn(&effect).is_some());
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// The same effect taken on by a mover with no memory of it: the wallet may hold the withdrawal, so it isn't handed
/// over again, and the move isn't read as failed; it is unknown and the part stays held.
// verifies: INV-8
#[test]
fn hand_on_taken_on_by_a_mover_with_no_memory_of_it_is_never_handed_over_again() {
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    let routes = Routes(Some(accepted(&core)));
    // The first try: the wallet is handed the statement, and the log can't take what came back.
    for _ in 0..rutent_core_orch::stuck::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::HandOver, Fault::Unavailable));
    }
    let first = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    assert!(matches!(first, Err(ResolveError::Append(_))), "{first:?}");
    assert_eq!(wallet.handed.len(), 1);
    let mut fresh = Mover::new();
    let again = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut fresh,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(again);
    assert_eq!(resolved, Resolved::Unknown);
    assert_eq!(wallet.handed.len(), 1, "never handed over again");
    assert_eq!(wallet.shown, 1);
    assert!(fresh.withdrawn(&effect).is_none());
    still_held(&core);
}

/// Only what Core itself saw proves that nothing passed out. Once the wallet was handed the statement, its own word
/// that the withdrawal failed, a duplicate-transaction refusal, an answer that isn't the withdrawal's, a record it
/// never made and a platform refusal that could follow a hand-over each answer `Unknown`; only a refusal that proves
/// the wallet ran nothing answers `Failed`.
// verifies: INV-2, REQ-ORD-1
#[test]
fn a_wallets_failure_or_a_refusal_that_could_follow_a_hand_over_is_unknown_not_failed() {
    let cases: [(StartError, Resolved); 8] = [
        (
            StartError::Wallet(WalletError::NotSucceeded),
            Resolved::Unknown,
        ),
        (
            StartError::Wallet(WalletError::NotAnswered(
                PlatformRefusal::DuplicateTransaction,
            )),
            Resolved::Unknown,
        ),
        (
            StartError::Wallet(WalletError::NotAnswered(PlatformRefusal::OutcomeUnknown)),
            Resolved::Unknown,
        ),
        (
            StartError::Wallet(WalletError::WrongResult),
            Resolved::Unknown,
        ),
        (StartError::NoRecord, Resolved::Unknown),
        (
            StartError::Wallet(WalletError::Disagrees {
                asked: 1,
                stated: "2".into(),
            }),
            Resolved::Unknown,
        ),
        (
            StartError::Wallet(WalletError::NotAnswered(PlatformRefusal::Unavailable)),
            Resolved::Failed,
        ),
        (
            StartError::Wallet(WalletError::NotAnswered(PlatformRefusal::NotHandled)),
            Resolved::Failed,
        ),
    ];
    for (error, expected) in cases {
        let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
        let mut mover = Mover::new();
        let mut policy = PermitsEveryStep::new(&core.log);
        let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
        let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
        let mut wallet = Wallet::of(&core, Script::Answers(error.clone()));
        let routes = Routes(Some(accepted(&core)));
        let moved = move_it(
            &mut core,
            &mut world,
            &mut stuck,
            &mut mover,
            &mut policy,
            &carries_out,
            &bob,
            &mut wallet,
            &routes,
        );
        let (effect, resolved) = carried_out(moved);
        assert_eq!(resolved, expected, "{error:?}");
        assert!(mover.withdrawn(&effect).is_none(), "{error:?}");
        still_held(&core);
    }
}

/// The operator's question is recorded attributed before the wallet is handed anything: a log that refuses it, one
/// that records it unattributed and a statement that isn't the withdrawal decided (another amount) each leave the
/// wallet handed nothing, and the move fails, which Core itself saw.
// verifies: REQ-AUZ-4, INV-2
#[test]
fn hand_on_question_must_be_recorded_attributed_and_be_the_withdrawal_decided() {
    for why in ["refused", "unattributed", "another amount"] {
        let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
        let mut mover = Mover::new();
        let mut policy = PermitsEveryStep::new(&core.log);
        let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
        let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
        let script = if why == "another amount" {
            Script::Tampers
        } else {
            Script::Pays
        };
        let mut wallet = Wallet::of(&core, script);
        match why {
            "refused" => core
                .log
                .borrow_mut()
                .faults
                .push_back((On::StatedOperation, Fault::Refuse)),
            "unattributed" => core.log.borrow_mut().stop_attributing(),
            _ => {}
        }
        let routes = Routes(Some(accepted(&core)));
        let moved = move_it(
            &mut core,
            &mut world,
            &mut stuck,
            &mut mover,
            &mut policy,
            &carries_out,
            &bob,
            &mut wallet,
            &routes,
        );
        let (_, resolved) = carried_out(moved);
        assert_eq!(resolved, Resolved::Failed, "{why}");
        assert!(wallet.handed.is_empty(), "{why}: nothing handed over");
        still_held(&core);
    }
}

/// A mover given no holding accounts, as `Mover::on` alone gives it, fails a hand-on with nothing handed over: the
/// tests' `Moves` are the only other carrier of one.
// verifies: REQ-ORD-43
#[test]
fn a_mover_without_the_hand_on_ports_fails_a_hand_on() {
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let (order, actor) = (id(ORDER), segments_support::core());
    let mut watched = Watched::new(&mut world.fleet.platform);
    let moved = on_platform(
        &mut core,
        &world.catalogue,
        &mut watched,
        &mut mover,
        |parts, delivering| {
            stuck.carry_out(
                parts,
                &mut policy,
                delivering,
                Moving {
                    order: &order,
                    carries_out: &carries_out,
                    mover: &bob,
                    core: &actor,
                },
            )
        },
    );
    let (_, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Failed);
    still_held(&core);
}

/// The customer's own acceptance of the linked order is what a hand-on rests on, and the operator never chooses where
/// the money goes (`REQ-ORD-43`, the owner's ruling of 2026-11-16): a linked order accepted on an automatic deposit's
/// arrival, not by the customer, is refused before the holding account is shown anything, since a withdrawal done
/// can't be taken back.
// verifies: REQ-ORD-43, REQ-AUZ-4
#[test]
fn hand_on_to_a_linked_order_the_customer_did_not_accept_is_refused_before_the_wallet_is_asked() {
    let mut world = World::of(&[RESERVE, OTHER_RESERVE]);
    let mut core = Core::over(&mut world);
    core.log.borrow_mut().attribute_from_now();
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.refuse_begin = Some(1);
    let _ = core.run_over(&world.catalogue, &mut watched);
    drop(watched);
    world
        .fleet
        .world()
        .script_dry_run(last_reserve(), "deposit", Behaviour::act(Act::Refuse));
    let outcome = Rerouting::of(&core)
        .recalculate(&mut core, &mut world)
        .expect("the recalculation runs");
    let mut stuck = Stuck::new();
    let (order, part, last) = (id(ORDER), id(PART), core.record.record_id().clone());
    let actor = segments_support::core();
    on_parts(&mut core, |parts| {
        stuck.no_route(
            parts,
            NoRouteLeft {
                order: &order,
                part: &part,
                outcome: &outcome,
                last_record: &last,
                core: &actor,
            },
        )
    })
    .expect("recorded");
    follow(&core, &mut stuck).expect("its ends are Core's");
    seed_linked_on_a_deposit(
        &mut core,
        &Named::another(2),
        &customer().party,
        &linked_record(LINKED),
    );
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (_, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Failed);
    assert_eq!(wallet.shown, 0, "the wallet is shown nothing");
    assert!(wallet.handed.is_empty());
    still_held(&core);
}

/// A linked order Core follows already is never withdrawn for, by the mover's own guard: `hand_on`'s check runs only
/// after the withdrawal is done.
// verifies: REQ-ORD-43, INV-4
#[test]
fn hand_on_to_a_linked_order_core_already_follows_is_refused_before_the_wallet_is_asked() {
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let named = Named::another(2);
    let opened = route_chosen_of(&core.record, &named);
    assert!(core.segments.follow(&*core.clock, &opened));
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (_, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Failed);
    assert_eq!(wallet.shown, 0);
    assert!(wallet.handed.is_empty());
}

/// Called directly, as the assembly may, the mover holds the effect's amount to all of what the part holds itself:
/// `Stuck`'s own check isn't on that path.
// verifies: REQ-ORD-43, INV-2
#[test]
fn a_hand_on_resolved_directly_for_less_than_the_part_holds_is_refused() {
    struct Less<'a, 'b>(&'a mut rutent_core_orch::stuck::Delivering<'b>);
    impl rutent_core_orch::stuck::Resolutions for Less<'_, '_> {
        fn resolve(
            &mut self,
            parts: &mut rutent_core_orch::stuck::Parts<'_>,
            carrying: &rutent_core_orch::stuck::Carrying<'_>,
            effect: &rutent_contracts::core_internal::Effect,
        ) -> Result<Resolved, rutent_contracts::core_internal::calls::AppendFailure> {
            let mut changed = effect.clone();
            if let Action::ResolveStuck {
                resolution: Resolution::HandOn { amount, .. },
                ..
            } = &mut changed.action
            {
                *amount = PositiveAmount::new("999999").unwrap();
            }
            self.0.resolve(parts, carrying, &changed)
        }
    }
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    let routes = Routes(Some(accepted(&core)));
    let entries = Entries(
        core.log
            .borrow()
            .entries
            .iter()
            .map(|(e, _)| e.clone())
            .collect(),
    );
    let (order, actor) = (id(ORDER), segments_support::core());
    let mut watched = Watched::new(&mut world.fleet.platform);
    let moved = on_platform_handing(
        &mut core,
        &world.catalogue,
        &mut watched,
        &mut mover,
        HandingOn {
            withdrawals: &mut wallet,
            routes: &routes,
            log: &entries,
        },
        &mut NoProxy,
        |parts, delivering| {
            stuck.carry_out(
                parts,
                &mut policy,
                &mut Less(delivering),
                Moving {
                    order: &order,
                    carries_out: &carries_out,
                    mover: &bob,
                    core: &actor,
                },
            )
        },
    );
    let (_, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Failed);
    assert_eq!(wallet.shown, 0);
}

/// A call that was handed the wallet's statement and stopped before its answer was kept never asks the wallet again:
/// the clock becomes unreadable once the wallet was handed the statement, and however the move is taken on again,
/// the wallet is handed one statement in all, and the outcome isn't read as done.
// verifies: INV-8
#[test]
fn a_call_that_stopped_after_the_wallet_was_asked_never_asks_it_again() {
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    wallet.breaks_clock = Some(std::rc::Rc::clone(&core.clock));
    let routes = Routes(Some(accepted(&core)));
    let first = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    assert_eq!(wallet.handed.len(), 1);
    // However it came out, the clock comes back and the move is taken on again.
    core.clock.set(START);
    let mut resolved = match first {
        Ok(Moved::CarriedOut { resolved, .. }) => Some(resolved),
        _ => None,
    };
    for _ in 0..3 {
        if let Ok(Moved::CarriedOut { resolved: r, .. }) = move_it(
            &mut core,
            &mut world,
            &mut stuck,
            &mut mover,
            &mut policy,
            &carries_out,
            &bob,
            &mut wallet,
            &routes,
        ) {
            resolved = Some(r);
        }
    }
    assert_eq!(
        wallet.handed.len(),
        1,
        "the wallet is handed one statement in all"
    );
    assert_ne!(resolved, Some(Resolved::Failed));
}

// The holding account's stated answer as a known outcome (`T-020-26`) -----------------------------------------

/// The wallet's transaction for the move: its own, `{provider}-withdrawal-{n}`, not the one the decision named.
fn wallets_transaction() -> String {
    format!("{}-withdrawal-1", origin_reserve())
}

/// `maker`'s later signed answer under the wallet's transaction, made now.
fn later_answer(core: &Core, maker: &str, outcome: serde_json::Value) -> Vec<u8> {
    wallet_answer(
        maker,
        &wallets_transaction(),
        core.clock.now().timestamp().unwrap().as_str(),
        outcome,
    )
}

/// Takes `bytes` as evidence of the hand-on `effect`'s outcome.
fn take(
    core: &mut Core,
    world: &mut World,
    mover: &mut Mover,
    effect: &Receipt,
    bytes: &[u8],
) -> Result<Stated, NotTaken> {
    let (order, part, actor) = (id(ORDER), id(PART), segments_support::core());
    let mut watched = Watched::new(&mut world.fleet.platform);
    on_platform(
        core,
        &world.catalogue,
        &mut watched,
        mover,
        |parts, delivering| {
            delivering.statement(
                parts,
                &Carrying {
                    order: &order,
                    part: &part,
                    core: &actor,
                    again: false,
                },
                effect,
                bytes,
            )
        },
    )
}

/// A hand-on the wallet was handed and answered in an error: its outcome is unknown, the part held.
fn left_unknown() -> (
    World,
    Core,
    Stuck,
    Mover,
    PermitsEveryStep,
    Decided,
    Receipt,
    Wallet,
) {
    left_unknown_in(stuck_with_a_linked_order())
}

/// [`left_unknown`] in the two-party world, where another party of the order can confirm the holder's word
/// (`T-020-39`).
fn left_unknown_of_two_parties() -> (
    World,
    Core,
    Stuck,
    Mover,
    PermitsEveryStep,
    Decided,
    Receipt,
    Wallet,
) {
    left_unknown_in(stuck_with_a_linked_order_in(World::two_parties()))
}

/// The hand-on left unknown, from the stuck order `stuck`.
fn left_unknown_in(
    (mut world, mut core, mut stuck): (World, Core, Stuck),
) -> (
    World,
    Core,
    Stuck,
    Mover,
    PermitsEveryStep,
    Decided,
    Receipt,
    Wallet,
) {
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(
        &core,
        Script::Answers(StartError::Wallet(WalletError::NotSucceeded)),
    );
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Unknown);
    assert_eq!(wallet.handed.len(), 1);
    assert!(mover.withdrawn(&effect).is_none());
    still_held(&core);
    (
        world,
        core,
        stuck,
        mover,
        policy,
        carries_out,
        effect,
        wallet,
    )
}

/// A hand-on whose wallet was handed the statement and answered in an error is left unknown, and nothing more is
/// done for it. The holding account's own later signed answer under its own transaction, which Core checks
/// (attributable, a result, naming the wallet's transaction, a withdrawal result of the amount decided with the
/// wallet's own record), is taken as the evidence of its outcome and kept once as the holder's stated entry about the
/// linked order: the mover then gives the withdrawal's start to the hand-on, as it does for one done at once. A
/// statement of another transaction, one a key never published signed, another amount, a record proving nothing
/// of this start, and a wallet that was never asked are each not taken.
// verifies: REQ-ORD-1
#[test]
fn hand_on_left_unknown_takes_its_known_outcome_from_the_holding_accounts_stated_answer() {
    let (mut world, mut core, mut stuck, mut mover, mut policy, carries_out, effect, wallet) =
        left_unknown_of_two_parties();
    let transaction = wallets_transaction();
    let amount = AMOUNT;
    let before = stated_about(&core, LINKED).len();
    // Not taken.
    let other = wallet_answer(
        origin_reserve(),
        "sim-reserve-a-withdrawal-9",
        core.clock.now().timestamp().unwrap().as_str(),
        wallet_succeeded(origin_reserve(), "sim-reserve-a-withdrawal-9", amount),
    );
    assert_eq!(
        take(&mut core, &mut world, &mut mover, &effect, &other),
        Err(NotTaken::OfAnotherTransaction)
    );
    let mut forged: serde_json::Value = serde_json::from_slice(&later_answer(
        &core,
        origin_reserve(),
        wallet_succeeded(origin_reserve(), &transaction, amount),
    ))
    .unwrap();
    forged["signature"]["value"] = rutent_conformance::signing::base64url(&[7u8; 64]).into();
    assert_eq!(
        take(
            &mut core,
            &mut world,
            &mut mover,
            &effect,
            &serde_json::to_vec(&forged).unwrap()
        ),
        Err(NotTaken::Unattributable)
    );
    let short = later_answer(
        &core,
        origin_reserve(),
        wallet_succeeded(origin_reserve(), &transaction, "1"),
    );
    assert_eq!(
        take(&mut core, &mut world, &mut mover, &effect, &short),
        Err(NotTaken::NotCounted),
        "another amount than the part held"
    );
    let no_record = later_answer(
        &core,
        origin_reserve(),
        serde_json::json!({
            "status": "succeeded", "amount_in": amount, "amount_out": amount,
            "fee": { "asset": "COP", "amount": "0" },
            "evidence": [{ "grade": 4, "reference": "elsewhere", "by": origin_reserve() }],
        }),
    );
    assert_eq!(
        stated_about(&core, LINKED).len(),
        before,
        "nothing was kept"
    );
    // A success with no record of the wallet's own is its word and no more: kept, saying nothing the move can rest
    // on, so nothing is started on it.
    let said_nothing = take(&mut core, &mut world, &mut mover, &effect, &no_record)
        .expect("kept as the holder's word");
    assert_eq!(said_nothing.outcome(), Some(Resolved::Unknown));
    let before = stated_about(&core, LINKED).len();
    assert!(mover.withdrawn(&effect).is_none());
    // Taken: the holding account's own answer, once however often it is tried.
    let bytes = later_answer(
        &core,
        origin_reserve(),
        wallet_succeeded(origin_reserve(), &transaction, amount),
    );
    let stated = take(&mut core, &mut world, &mut mover, &effect, &bytes).expect("taken");
    assert_eq!(stated.outcome(), Some(Resolved::Done));
    let again = take(&mut core, &mut world, &mut mover, &effect, &bytes).expect("taken again");
    assert_eq!(again, stated, "a statement tried again is one entry");
    let kept = stated_about(&core, LINKED);
    assert_eq!(kept.len(), before + 1);
    assert_eq!(kept[before].1.as_deref(), Some(transaction.as_str()));
    assert_eq!(
        kept[before].2,
        rutent_contracts::provider_role::RESULT_SHAPE
    );
    // The start the answer proves is the hand-on's, with the answer's receipt for the record to cite.
    let withdrawn = mover.withdrawn(&effect).expect("known done");
    assert_eq!(withdrawn.statement, Some(stated.receipt()));
    assert_eq!(
        withdrawn.start.record.reference,
        format!("{}-withdrawal-{transaction}", origin_reserve())
    );
    assert_eq!(withdrawn.start.transaction, transaction);
    assert_eq!(withdrawn.route, &accepted(&core));
    // The part is held until the outcome is confirmed, and the wallet was handed the statement once.
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    assert_eq!(wallet.handed.len(), 1);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry up to the answer");
    // Reported by the mover on the holder's answer and confirmed by another party on another maker's, through the
    // log: the answer is the evidence a hand-on's outcome rests on, though it names the wallet's own transaction.
    let (order, actor) = (id(ORDER), segments_support::core());
    let (bob, carol) = (staff("desk-b", "bob"), staff("desk-c", "carol"));
    let history = core.recorded();
    let reported = on_parts(&mut core, |parts| {
        stuck.known_outcome(
            parts,
            &mut policy,
            Moving {
                order: &order,
                carries_out: &carries_out,
                mover: &bob,
                core: &actor,
            },
            Resolved::Done,
            &stated,
            &history,
        )
    })
    .expect("reported");
    let Moved::AwaitsConfirmation { report } = reported else {
        panic!("awaits its confirmation: {reported:?}");
    };
    let bytes = later_answer(
        &core,
        second_reserve(),
        wallet_succeeded(second_reserve(), &transaction, amount),
    );
    let others = take(&mut core, &mut world, &mut mover, &effect, &bytes).expect("another maker's");
    let history = core.recorded();
    let confirmed = on_parts(&mut core, |parts| {
        stuck.confirm_outcome(
            parts,
            &mut policy,
            Confirming {
                order: &order,
                carries_out: &carries_out,
                confirmer: &carol,
                outcome: Resolved::Done,
                report,
                reported_with: stated.receipt(),
                evidence: &others,
                core: &actor,
            },
            &history,
        )
    })
    .expect("confirmed");
    assert!(matches!(
        confirmed,
        Moved::CarriedOut {
            resolved: Resolved::Done,
            ..
        }
    ));
    // Done and confirmed, the hand-on takes its route and its record from the answer, and the linked order starts.
    let withdrawn = mover.withdrawn(&effect).expect("known done");
    let (plan, record, statement) = (
        withdrawn.route.plan.clone(),
        withdrawn.start.record.clone(),
        withdrawn.statement.cloned().unwrap(),
    );
    let mut hand_on = HandOn::new();
    operator_hand_on(&mut core, &mut hand_on, &plan, &effect, &bob).expect("handed on");
    segments_support::handing::prove_citing(
        &mut core,
        &mut hand_on,
        &id(LINKED),
        &record,
        &statement,
    )
    .expect("the answer's record proves the start");
    let linked = linked_route(&mut world, &core.clock, LINKED);
    let ran = run_linked(&mut core, &mut world, &linked).expect("the linked route runs");
    assert!(settled(&ran), "{ran:?}");
    core.admitted(&world.catalogue)
        .expect("the log admits the whole history, the outcome known through the holder's answer");
}

/// A wallet that was never handed the statement answered nothing: a statement named for its hand-on is taken for no
/// move, whoever made it.
// verifies: REQ-ORD-1
#[test]
fn a_statement_for_a_hand_on_the_wallet_was_never_asked_is_taken_for_no_move() {
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    // A route the mover refuses before asking the wallet anything.
    let mut route = accepted(&core);
    route.order = id("ord-3");
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &Routes(Some(route)),
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Failed);
    let bytes = later_answer(
        &core,
        origin_reserve(),
        wallet_succeeded(origin_reserve(), &wallets_transaction(), AMOUNT),
    );
    assert_eq!(
        take(&mut core, &mut world, &mut mover, &effect, &bytes),
        Err(NotTaken::NoMove)
    );
    assert!(mover.withdrawn(&effect).is_none());
}

/// A hand-on's failure stated by the holding account alone frees nothing: the mover takes the wallet's own statement
/// that it failed as evidence and says what it states (`Failed`), but the part stays held, its outcome unknown and
/// no new decision possible, until another party, with another maker's statement, confirms it (`THR-70`: a wallet
/// that paid could say it failed). The confirmation, by another actor of another party than the mover, frees it.
// verifies: REQ-ORD-1, REQ-AUZ-4
#[test]
fn hand_on_failure_stated_by_the_holding_account_frees_the_part_only_on_another_partys_confirmation()
 {
    let (mut world, mut core, mut stuck, mut mover, mut policy, carries_out, effect, _wallet) =
        left_unknown_of_two_parties();
    let (alice, bob, carol) = (
        staff("desk-a", "alice"),
        staff("desk-b", "bob"),
        staff("desk-c", "carol"),
    );
    let transaction = wallets_transaction();
    let failed = serde_json::json!({
        "status": "failed",
        "evidence": [{ "grade": 4, "reference": format!("{}-failed-{transaction}", origin_reserve()), "by": origin_reserve() }],
    });
    let bytes = later_answer(&core, origin_reserve(), failed);
    let holders = take(&mut core, &mut world, &mut mover, &effect, &bytes).expect("taken");
    assert_eq!(holders.outcome(), Some(Resolved::Failed));
    assert!(
        mover.withdrawn(&effect).is_none(),
        "a failure starts nothing"
    );
    // Held: the part is stuck and no new decision is taken while the outcome is unknown.
    assert!(stuck.is_stuck(&id(ORDER), &id(PART)));
    let (order, part, actor) = (id(ORDER), id(PART), segments_support::core());
    let evidence = own_evidence(&mut core);
    let resolution = hand_on_resolution();
    let history = core.recorded();
    let again = on_parts(&mut core, |parts| {
        stuck.decide(
            parts,
            &mut policy,
            Deciding {
                order: &order,
                part: &part,
                decider: &alice,
                resolution: &resolution,
                evidence: &evidence,
            },
            &history,
        )
    });
    assert_eq!(again.map(|_| ()), Err(ResolveError::OutcomeUnknown));
    // The mover reports it on the holder's own statement: nothing is freed by that.
    let history = core.recorded();
    let reported = on_parts(&mut core, |parts| {
        stuck.known_outcome(
            parts,
            &mut policy,
            Moving {
                order: &order,
                carries_out: &carries_out,
                mover: &bob,
                core: &actor,
            },
            Resolved::Failed,
            &holders,
            &history,
        )
    })
    .expect("reported");
    let Moved::AwaitsConfirmation { report } = reported else {
        panic!("awaits its confirmation: {reported:?}");
    };
    assert!(
        stuck.is_stuck(&id(ORDER), &id(PART)),
        "a report frees nothing"
    );
    // Another party's confirmation, on another maker's statement, frees it.
    let bytes = later_answer(
        &core,
        second_reserve(),
        serde_json::json!({ "status": "failed" }),
    );
    let others = take(&mut core, &mut world, &mut mover, &effect, &bytes)
        .expect("another maker's statement");
    assert_eq!(
        others.outcome(),
        None,
        "it bears out nothing of the move itself"
    );
    let history = core.recorded();
    let confirmed = on_parts(&mut core, |parts| {
        stuck.confirm_outcome(
            parts,
            &mut policy,
            Confirming {
                order: &order,
                carries_out: &carries_out,
                confirmer: &carol,
                outcome: Resolved::Failed,
                report,
                reported_with: holders.receipt(),
                evidence: &others,
                core: &actor,
            },
            &history,
        )
    })
    .expect("confirmed");
    assert!(matches!(
        confirmed,
        Moved::CarriedOut {
            resolved: Resolved::Failed,
            ..
        }
    ));
    assert!(mover.withdrawn(&effect).is_none());
    assert!(
        stuck.is_stuck(&id(ORDER), &id(PART)),
        "freed to a new decision, still stuck"
    );
    let history = core.recorded();
    let decided_again = on_parts(&mut core, |parts| {
        stuck.decide(
            parts,
            &mut policy,
            Deciding {
                order: &order,
                part: &part,
                decider: &alice,
                resolution: &resolution,
                evidence: &evidence,
            },
            &history,
        )
    });
    assert!(decided_again.is_ok(), "{decided_again:?}");
    core.admitted(&world.catalogue)
        .expect("the log admits the report and the confirmation on the wallet's own answer");
}

/// A wallet whose signed answer doesn't bear out its own record is not taken as having started the route: the record
/// says it paid, and the answer, which is what the log keeps, says another amount, another record, or that it failed.
/// The outcome is unknown and nothing starts on it, whatever the record says.
// verifies: REQ-ORD-40, REQ-ORD-1
#[test]
fn hand_on_whose_answer_does_not_bear_out_its_record_is_left_unknown() {
    type Other = fn(&mut serde_json::Value);
    let cases: [(&str, Other); 4] = [
        ("another amount", |o| {
            o["amount_in"] = "1".into();
            o["amount_out"] = "1".into();
        }),
        ("an unbalanced answer", |o| o["amount_out"] = "1".into()),
        ("another record", |o| {
            o["evidence"][0]["reference"] = "elsewhere".into();
        }),
        ("a failure", |o| {
            *o = serde_json::json!({
                "status": "failed",
                "evidence": [{ "grade": 4, "reference": "x", "by": origin_reserve() }],
            });
        }),
    ];
    for (why, change) in cases {
        let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
        let mut mover = Mover::new();
        let mut policy = PermitsEveryStep::new(&core.log);
        let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
        let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
        let mut wallet = Wallet::of(&core, Script::AnswersOtherwise(change));
        let routes = Routes(Some(accepted(&core)));
        let moved = move_it(
            &mut core,
            &mut world,
            &mut stuck,
            &mut mover,
            &mut policy,
            &carries_out,
            &bob,
            &mut wallet,
            &routes,
        );
        let (effect, resolved) = carried_out(moved);
        assert_eq!(resolved, Resolved::Unknown, "{why}");
        assert!(mover.withdrawn(&effect).is_none(), "{why}");
        assert_eq!(wallet.handed.len(), 1, "{why}: asked once");
        still_held(&core);
    }
}

/// A wallet's answer that the log recorded unattributed, its key unknown to the log, is no record a start can rest on:
/// the log would refuse the `holding_withdrawal` citing it. The move is left unknown, the answer kept, and nothing
/// starts.
// verifies: REQ-ORD-40, REQ-AUD-2
#[test]
fn hand_on_whose_answer_the_log_could_not_attribute_is_left_unknown() {
    let (mut world, mut core, mut stuck) = stuck_with_a_linked_order();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide(&mut core, &mut stuck, &mut policy, &alice);
    let mut wallet = Wallet::of(&core, Script::Pays);
    wallet.unattributes = Some(std::rc::Rc::clone(&core.log));
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Unknown);
    assert!(mover.withdrawn(&effect).is_none());
    assert_eq!(stated_about(&core, LINKED).len(), 2, "the answer is kept");
    still_held(&core);
}

/// A start the holding account's answer once proved is given to no hand-on once the holding account has also stated
/// that the withdrawal failed: the two statements disagree, and nothing starts on either.
// verifies: REQ-ORD-1, REQ-ORD-40
#[test]
fn a_known_start_is_given_to_no_hand_on_once_the_holder_has_stated_a_failure() {
    let (mut world, mut core, _stuck, mut mover, _policy, _carries_out, effect, _wallet) =
        left_unknown();
    let transaction = wallets_transaction();
    let done = later_answer(
        &core,
        origin_reserve(),
        wallet_succeeded(origin_reserve(), &transaction, AMOUNT),
    );
    take(&mut core, &mut world, &mut mover, &effect, &done).expect("taken");
    assert!(mover.withdrawn(&effect).is_some());
    let failed = later_answer(
        &core,
        origin_reserve(),
        serde_json::json!({
            "status": "failed",
            "evidence": [{ "grade": 4, "reference": "x", "by": origin_reserve() }],
        }),
    );
    let stated = take(&mut core, &mut world, &mut mover, &effect, &failed).expect("taken");
    assert_eq!(stated.outcome(), Some(Resolved::Failed));
    assert!(mover.withdrawn(&effect).is_none());
}

// A holding account that runs no route (`T-020-33`) ------------------------------------------------------------

/// The kit's stuck order, its linked order accepted on a route whose first segment is `origin_reserve()`'s, and a wallet that
/// is `second_reserve()`'s: the node's holding account is a provider that runs none of the linked route.
fn stuck_at_another_providers_wallet() -> (World, Core, Stuck, Wallet) {
    let (world, core, stuck) = stuck_with_a_linked_order();
    assert_eq!(
        accepted(&core).plan[0].segments[0].provider.as_str(),
        origin_reserve(),
        "the linked route's first move is the reserve's"
    );
    let wallet = Wallet::held_by(&core, Script::Pays, second_reserve());
    (world, core, stuck, wallet)
}

/// A hand-on names the node's holding account as its holder, and the linked route's first provider, another, runs
/// the move: the decision takes a holder that is no provider of the part's route, the mover asks the holding account
/// for the withdrawal (its record, its transaction and its answer are the holding account's own), the hand-on and the
/// `holding_withdrawal` name the holding account as the linked order's starting reserve, and the route's first
/// segment stays the reserve's.
// verifies: REQ-ORD-43, REQ-ORD-40
#[test]
fn hand_on_names_the_nodes_holding_account_while_the_linked_routes_first_provider_runs_the_move() {
    let (mut world, mut core, mut stuck, mut wallet) = stuck_at_another_providers_wallet();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide_by(&mut core, &mut stuck, &mut policy, &alice, second_reserve());
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Done);

    // The holding account was handed the one withdrawal decided, as its own, at the node the value rests at.
    assert_eq!(wallet.handed.len(), 1);
    let sent = serde_json::to_value(&wallet.handed[0].statement).unwrap();
    assert_eq!(sent["provider"], second_reserve());
    assert_eq!(sent["destination"], ledger_node());
    assert_eq!(sent["amount"], AMOUNT);
    let withdrawn = mover.withdrawn(&effect).expect("the withdrawal is done");
    assert_eq!(withdrawn.start.record.by, second_reserve());
    assert_eq!(withdrawn.start.reserve, second_reserve());
    let (plan, record) = (withdrawn.route.plan.clone(), withdrawn.start.record.clone());
    let statement = withdrawn
        .statement
        .cloned()
        .expect("the account's answer is on record");

    // The hand-on and the start the holding account's own record proves name the holding account, and the linked
    // route's first segment is still the reserve's.
    let mut hand_on = HandOn::new();
    operator_hand_on(&mut core, &mut hand_on, &plan, &effect, &bob).expect("handed on");
    prove_citing(&mut core, &mut hand_on, &id(LINKED), &record, &statement)
        .expect("the holding account's own record proves the start");
    assert!(
        core.log.borrow().records().iter().any(|(_, w)| matches!(w,
            What::HoldingWithdrawal { order, holder, .. }
                if order.as_str() == LINKED && holder.as_str() == second_reserve())),
        "the record names the holding account as the holder"
    );
    assert_eq!(plan[0].segments[0].provider.as_str(), origin_reserve());
    // The linked order's first move is the plan's provider's, resting on the holding account's record: it is in the
    // log, and the log admits it (`T-020-36`'s item 4).
    let linked = linked_route(&mut world, &core.clock, LINKED);
    let ran = run_linked(&mut core, &mut world, &linked).expect("the linked route runs");
    assert!(settled(&ran), "{ran:?}");
    assert!(
        core.log.borrow().entries.iter().any(|(e, _)| matches!(e, Entry::Verified(v)
            if matches!(&v.what, What::Effect { action: Action::MoveValue { order, provider, .. }, .. }
                if order.as_str() == LINKED && provider.as_str() == origin_reserve()))),
        "the first move, by the plan's provider, is in the log"
    );
    assert!(
        core.segments
            .order(&id(ORDER))
            .is_some_and(|o| o.parts[0].state == OrderState::HandedOn),
        "the part is handed on"
    );
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

/// A hand-on whose start no record of the holding account proves stays refused: a record by another provider than
/// the holding account (the linked route's first provider's, say) proves nothing and leaves the withdrawal unknown;
/// a decision naming a provider that isn't the node's holding account, the route's first provider included, is
/// refused before the account is asked anything; a start recovered from the log counts only for the holder the
/// operator's resolution named; and the linked order's segments are refused until the account's own record proves it.
// verifies: REQ-ORD-40
#[test]
fn hand_on_whose_start_no_record_of_the_holding_account_proves_stays_refused() {
    // The account's record is another provider's: the first provider's, as the old rule would have taken.
    let (mut world, mut core, mut stuck, _) = stuck_at_another_providers_wallet();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let carries_out = decide_by(&mut core, &mut stuck, &mut policy, &alice, second_reserve());
    let mut wallet = Wallet::held_by(
        &core,
        Script::PaysRecorded(|r| r.by = origin_reserve().into()),
        second_reserve(),
    );
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Unknown);
    assert!(mover.withdrawn(&effect).is_none());
    still_held(&core);

    // Withdrawn well by the holding account: handed on, and refused as before until its own record proves the start.
    let (mut world, mut core, mut stuck, mut wallet) = stuck_at_another_providers_wallet();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let carries_out = decide_by(&mut core, &mut stuck, &mut policy, &alice, second_reserve());
    let routes = Routes(Some(accepted(&core)));
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Done);
    let withdrawn = mover.withdrawn(&effect).expect("done");
    let (plan, record) = (withdrawn.route.plan.clone(), withdrawn.start.record.clone());
    let statement = withdrawn.statement.cloned().expect("on record");
    let mut hand_on = HandOn::new();
    operator_hand_on(&mut core, &mut hand_on, &plan, &effect, &bob).expect("handed on");
    let linked = linked_route(&mut world, &core.clock, LINKED);
    assert_eq!(
        run_linked(&mut core, &mut world, &linked).map(|_| ()),
        Err(SegmentsError::StartUnproven(id(LINKED)))
    );
    let mut firsts = record.clone();
    firsts.by = origin_reserve().into();
    assert_eq!(
        prove_citing(&mut core, &mut hand_on, &id(LINKED), &firsts, &statement),
        Err(HandOnError::Unproven(Unproven::NotTheStartingReserves)),
        "the first provider's record is not the holding account's"
    );
    assert_eq!(
        run_linked(&mut core, &mut world, &linked).map(|_| ()),
        Err(SegmentsError::StartUnproven(id(LINKED)))
    );

    // A restarted Core proves the start again from the log for the holder the resolution named, and for no other.
    let all: Vec<Entry> = core
        .log
        .borrow()
        .entries
        .iter()
        .map(|(e, _)| e.clone())
        .collect();
    prove_citing(&mut core, &mut hand_on, &id(LINKED), &record, &statement)
        .expect("the holding account's own record proves it");
    let all_proven: Vec<Entry> = core
        .log
        .borrow()
        .entries
        .iter()
        .map(|(e, _)| e.clone())
        .collect();
    assert!(all.len() < all_proven.len());
    let of_the_first_provider: Vec<Entry> = all_proven
        .iter()
        .cloned()
        .map(|mut e| {
            if let Entry::Verified(v) = &mut e
                && let What::HoldingWithdrawal { holder, .. } = &mut v.what
            {
                *holder = id(origin_reserve());
            }
            e
        })
        .collect();
    let mut restarted = rutent_core_orch::segments::Segments::new();
    assert_eq!(
        HandOn::recover_starts(&mut restarted, &of_the_first_provider),
        0,
        "the first provider isn't the holder the resolution named"
    );
    assert_eq!(HandOn::recover_starts(&mut restarted, &all_proven), 1);

    // A decision naming a provider that isn't the node's holding account, the route's first provider included, is
    // refused before the account is shown anything.
    for named in [origin_reserve(), "sim-reserve-z"] {
        let (mut world, mut core, mut stuck, mut wallet) = stuck_at_another_providers_wallet();
        let mut mover = Mover::new();
        let mut policy = PermitsEveryStep::new(&core.log);
        let carries_out = decide_by(&mut core, &mut stuck, &mut policy, &alice, named);
        let routes = Routes(Some(accepted(&core)));
        let moved = move_it(
            &mut core,
            &mut world,
            &mut stuck,
            &mut mover,
            &mut policy,
            &carries_out,
            &bob,
            &mut wallet,
            &routes,
        );
        let (_, resolved) = carried_out(moved);
        assert_eq!(resolved, Resolved::Failed, "{named}");
        assert_eq!(wallet.shown, 0, "{named}: the account is shown nothing");
        still_held(&core);
    }
}

/// A restarted Core proves the start again for the effect that succeeded, not the first that named the linked order
/// (`T-020-33`'s review): the operator first names the route's first provider, which the mover refuses (the effect's
/// result is failed), then the node's holding account, which is done and handed on; the recovery finds the holder the
/// done effect named, and no other.
// verifies: REQ-ORD-43, REQ-ORD-40
#[test]
fn recovered_start_follows_the_hand_on_that_succeeded_not_an_earlier_refused_one() {
    let (mut world, mut core, mut stuck, mut wallet) = stuck_at_another_providers_wallet();
    let mut mover = Mover::new();
    let mut policy = PermitsEveryStep::new(&core.log);
    let (alice, bob) = (staff("desk-a", "alice"), staff("desk-b", "bob"));
    let routes = Routes(Some(accepted(&core)));
    // First naming the route's first provider: refused by the mover, the part still held.
    let wrong = decide_by(&mut core, &mut stuck, &mut policy, &alice, origin_reserve());
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &wrong,
        &bob,
        &mut wallet,
        &routes,
    );
    let (_, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Failed);
    still_held(&core);
    // Then the holding account: done, handed on and proven.
    let carries_out = decide_by(&mut core, &mut stuck, &mut policy, &alice, second_reserve());
    let moved = move_it(
        &mut core,
        &mut world,
        &mut stuck,
        &mut mover,
        &mut policy,
        &carries_out,
        &bob,
        &mut wallet,
        &routes,
    );
    let (effect, resolved) = carried_out(moved);
    assert_eq!(resolved, Resolved::Done);
    let withdrawn = mover.withdrawn(&effect).expect("done");
    let (plan, record) = (withdrawn.route.plan.clone(), withdrawn.start.record.clone());
    let statement = withdrawn.statement.cloned().expect("on record");
    let mut hand_on = HandOn::new();
    operator_hand_on(&mut core, &mut hand_on, &plan, &effect, &bob).expect("handed on");
    prove_citing(&mut core, &mut hand_on, &id(LINKED), &record, &statement).expect("proven");
    let all: Vec<Entry> = core
        .log
        .borrow()
        .entries
        .iter()
        .map(|(e, _)| e.clone())
        .collect();
    let mut restarted = rutent_core_orch::segments::Segments::new();
    assert_eq!(HandOn::recover_starts(&mut restarted, &all), 1);
}
