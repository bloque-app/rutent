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
use rutent_core_orch::stuck::{
    AcceptedRoute, Deciding, HandingOn, LinkedRoutes, Moved, Mover, Moving, NoRouteLeft,
    OperatorSigns, ResolveError, Resolved, Show, Stuck, Withdrawals,
};
use rutent_prov::holding::wallet::{
    HandedOnStart, Operator as WalletOperator, StartError, WalletError, WithdrawalRecord,
};
use segments_support::handing::{
    from_the_ledger, hand_parts, linked_record, linked_route, prove, recorded_with_receipts,
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
    format!("{RESERVE}-ledger")
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
}

/// A node's holding account as `Holdings::start_handed_on` drives it, with what it was shown and handed.
struct Wallet {
    node: String,
    holder: Option<String>,
    script: Script,
    key: TestKey,
    bounds: PolicyBounds,
    maker: Id,
    made_at: Timestamp,
    sent: u64,
    /// How often the mover was shown a statement.
    shown: usize,
    /// The statements the wallet was handed.
    handed: Vec<Statement<ProviderOperation>>,
}

impl Wallet {
    fn of(core: &Core, script: Script) -> Self {
        Wallet {
            node: ledger_node(),
            holder: Some(RESERVE.to_owned()),
            script,
            key: TestKey(core.key.0.clone()),
            bounds: core.record.bounds.clone(),
            maker: core.operator.acts.clone(),
            made_at: Timestamp::new(rutent_prov::catalogue::Instant(START).rfc3339().unwrap())
                .unwrap(),
            sent: 0,
            shown: 0,
            handed: Vec::new(),
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
        let transaction = format!("{RESERVE}-withdrawal-{}", self.sent);
        let stated = if matches!(self.script, Script::Tampers) {
            "1"
        } else {
            amount
        };
        let operation = ProviderOperation::Withdrawal(Withdrawal {
            transaction: transaction.clone(),
            provider: RESERVE.to_owned(),
            session: None,
            origin: OuterNode {
                network: RESERVE.to_owned(),
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
        let mut record = WithdrawalRecord {
            by: RESERVE.to_owned(),
            node: node.clone(),
            asset: first.from.asset.id.clone(),
            order: order.to_owned(),
            amount: amount.parse().unwrap(),
            reference: format!("{RESERVE}-withdrawal-{transaction}"),
        };
        match &self.script {
            Script::Pays | Script::Tampers => {}
            Script::PaysRecorded(change) => change(&mut record),
            Script::Answers(error) => return Err(error.clone()),
        }
        Ok(HandedOnStart {
            reserve: RESERVE.to_owned(),
            node: node.clone(),
            first_segment: first.segment_id.clone(),
            record,
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

fn accepted(core: &Core) -> AcceptedRoute {
    AcceptedRoute {
        order: id(LINKED),
        decision_record: linked_record(LINKED),
        plan: from_the_ledger(core, SEGMENT),
    }
}

// The kit's stuck order ---------------------------------------------------------------------------------------

/// The kit's order with its withdrawal settled and its deposit never begun, then left no route: stuck, its value
/// resting at the reserve's ledger; and the linked order `ord-2` accepted by the original customer.
fn stuck_with_a_linked_order() -> (World, Core, Stuck) {
    let mut world = World::of(&[RESERVE, OTHER_RESERVE]);
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
        .script_dry_run(RESERVE, "deposit", Behaviour::act(Act::Refuse));
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
    Resolution::HandOn {
        by: id(RESERVE),
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
    let (order, part) = (id(ORDER), id(PART));
    let evidence = vec![digest(&"ab".repeat(32))];
    let resolution = hand_on_resolution();
    let decision = on_parts(core, |parts| {
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
        )
    })
    .expect("decided");
    Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    }
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
    let mut watched = Watched::new(&mut world.fleet.platform);
    on_platform_handing(
        core,
        &world.catalogue,
        &mut watched,
        mover,
        HandingOn {
            withdrawals: wallet,
            routes,
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
    assert_eq!(sent["provider"], RESERVE);

    // The question is on record before the result: the operator's, about the linked order and the wallet's
    // transaction.
    let transaction = sent["transaction"].as_str().unwrap().to_owned();
    let asked = stated_about(&core, LINKED);
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].1.as_deref(), Some(transaction.as_str()));
    assert_eq!(asked[0].2, rutent_contracts::provider_role::OPERATION_SHAPE);
    let result_at = core
        .log
        .borrow()
        .entries
        .iter()
        .position(|(e, _)| {
            matches!(e, Entry::Verified(v) if matches!(&v.what, What::Result { effect: r } if *r == effect))
        })
        .expect("the effect's result");
    assert!(asked[0].0 < result_at, "the question precedes the result");

    // The withdrawal done is what the hand-on takes its route and its proof from.
    let withdrawn = mover.withdrawn(&effect).expect("the withdrawal is done");
    assert_eq!(withdrawn.linked.as_str(), LINKED);
    assert_eq!(withdrawn.route, &accepted(&core));
    assert_eq!(
        withdrawn.start.record.amount,
        AMOUNT.parse::<u128>().unwrap()
    );
    assert!(withdrawn.question.is_some());
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
        id(RESERVE),
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
    let cases: [(&str, Alter); 5] = [
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
        ("no accepted route", false, Some(RESERVE.to_owned())),
        ("no holding account", true, None),
        (
            "another provider's account",
            true,
            Some(OTHER_RESERVE.to_owned()),
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
        by: id(RESERVE),
        linked_order: id("ord-3"),
        asset: cop_unit(),
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    let evidence = vec![digest(&"ab".repeat(32))];
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
        |r| r.by = OTHER_RESERVE.into(),
        |r| r.node = format!("{OTHER_RESERVE}-ledger"),
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
    let handled = world.fleet.platform.operations_received(RESERVE);
    assert_eq!(
        run_linked(&mut core, &mut world, &linked).map(|_| ()),
        Err(SegmentsError::StartUnproven(id(LINKED)))
    );
    assert_eq!(world.fleet.platform.operations_received(RESERVE), handled);
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
        1,
        "the question is one entry"
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
