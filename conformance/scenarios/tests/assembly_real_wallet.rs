//! The assembled Core hands a stuck order on through the real plain wallet (`T-024-17`; `REQ-ORD-43`, `REQ-ORD-40`,
//! `REQ-CAT-23`, `REQ-AUD-2`, `REQ-ORD-34`), where `T-020-26`'s assembled-Core test is the fail-closed one.
//!
//! The order runs over simulated providers to a public node, where its value rests in the operator's plain wallet: a
//! reserve into its private node, a ramp from there to the public node `sim-chain-b`, and the deposit out of it,
//! which the platform refuses to begin, so the part is stuck at the node. The wallet is the real one, built from
//! `components/prov/guests/plain-wallet/` the way the simulated providers are, admitted to the platform on the
//! operator's own signed release, and its release accepted in the log by a decided catalogue change.
//!
//! The wallet is the node's holding account, and a provider of no route: a hand-on names it as the holder, and the
//! linked route's first provider runs the first move (`T-020-33`). The value rests in the wallet because Core landed it
//! there when the segment that brought it settled (`Segments::land_settled`), not because a scenario said so.

mod execution_support;

use std::collections::BTreeMap;

use execution_support::*;
use rutent_contracts::core_internal::{
    About, Action, Actor, ActorKind, Answer, Attribution, Decided, Entry, Receipt, Resolution,
    Role, What,
};
use rutent_contracts::provider_role::{BalanceVerification, Denomination, Operation};
use rutent_contracts::router_api::{Id, PositiveAmount, Request};
use rutent_core_authz::roles::Permission;
use rutent_core_orch::segments::{Ran, SegmentOutcome};
use rutent_core_orch::stuck::{Moved, Resolved};
use rutent_scenarios::assembly::linked::{AcceptedLinked, LinkedOrder};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, StepError};
use rutent_scenarios::harness::Scenario;
use rutent_scenarios::kit::{self, AMOUNT, id};
use rutent_scenarios::sim::InstalledWallet;
use serde_json::json;

const CHAIN_IN: &str = "sim-chain-in";
const RAMP_B: &str = "sim-chain-ramp-b";
const OUT_B: &str = "sim-chain-out-b";

/// The public node the order's value rests at, which the wallet holds.
const NODE: &str = "sim-chain-b";

/// The address the requester provides for the deposit on chain, which the destination reserve's register holds as the
/// customer's.
const ADDRESS: &str = "0x5e11a9d0c0ffee";

/// An order across the three providers, a wallet installed at the node it rests at and the order's value landed in
/// it by Core, where the segment that brought it settled: stuck for want of a route from there.
struct Stuck {
    core: Core,
    order: Id,
    part: Id,
    session: Id,
    wallet: InstalledWallet,
}

fn staff(core: &Core, desk: &str, login: &str) -> Actor {
    let actor = Actor {
        party: id(desk),
        acts: id(login),
        role: Role::Operator,
        kind: ActorKind::Person,
    };
    let mut registers = core.registers();
    registers.list(&Actor {
        party: kit::operator().party,
        acts: id(desk),
        role: Role::Operator,
        kind: ActorKind::Component,
    });
    registers.list(&actor);
    actor
}

fn usdc() -> Denomination {
    Denomination {
        id: "USDC".into(),
        decimals: 2,
    }
}

/// The order across the three providers, run until the deposit is refused its start, and left no route; the wallet
/// installed at the node and holding the order's value, which Core's schedule has landed there.
fn stuck_at_the_public_node() -> Stuck {
    let mut stuck = stuck_before_the_schedule_runs();
    assert_eq!(stuck.core.tick().landed, 1, "the schedule lands the value");
    stuck
}

/// [`stuck_at_the_public_node`] before Core's schedule has run: the value settled at the node and is not landed yet.
fn stuck_before_the_schedule_runs() -> Stuck {
    let (mut core, session) = core_with("AC-6", &[CHAIN_IN, RAMP_B, OUT_B]);
    core.registers().accounts.insert(
        ADDRESS.to_owned(),
        kit::customer().party.as_str().to_owned(),
    );
    let wallet = core
        .install_wallet(NODE, "USDC")
        .expect("the wallet is installed");
    let asked = request(
        "req-stuck",
        &session,
        AMOUNT,
        CHAIN_IN,
        outer("sim-chain-a", "USDC"),
        outer("sim-chain-e", "USDC"),
        OUT_B,
        ADDRESS,
        "lowest_total_cost",
    );
    let order = accepted(&mut core, asked);
    let part = core.first_part(&order).expect("a started order");
    core.faults.refuse_begin = Some(2);
    core.run(&order).expect("the order runs");
    core.leave_no_route(&order).expect("the part is stuck");
    assert!(core.is_stuck(&order, &part));
    Stuck {
        core,
        order,
        part,
        session,
        wallet,
    }
}

/// What settled at a public node a holding account holds is landed by Core's schedule, and the measures the segments'
/// endings gave are written by it: a run does neither, a tick does each once (`REQ-AUD-7`, `INV-1`, `T-020-48`).
// verifies: REQ-AUD-7, INV-1
#[test]
fn settled_segments_land_and_measures_are_written_on_cores_schedule() {
    let mut stuck = stuck_before_the_schedule_runs();
    let measures = |core: &Core| {
        core.log()
            .entries()
            .iter()
            .filter(
                |(_, e)| matches!(e, Entry::Verified(v) if matches!(v.what, What::Measure { .. })),
            )
            .count()
    };
    assert_eq!(
        stuck.core.holdings().attributions(),
        [(NODE.to_owned(), Vec::new())],
        "the run landed nothing"
    );
    assert_eq!(measures(&stuck.core), 0, "and wrote no measure");
    let round = stuck.core.tick();
    assert_eq!(round.landed, 1, "the schedule landed the segment's value");
    assert!(round.measures > 0, "and wrote the endings' measures");
    assert_eq!(measures(&stuck.core), round.measures);
    assert_eq!(
        stuck.core.holdings().attributions(),
        [(
            NODE.to_owned(),
            vec![rutent_prov::holding::HeldForOrder {
                order: stuck.order.as_str().to_owned(),
                amount: AMOUNT.parse().unwrap(),
            }]
        )]
    );
    // Each once: a later tick lands and writes nothing more.
    let again = stuck.core.tick();
    assert_eq!((again.landed, again.measures), (0, 0));
    assert_eq!(measures(&stuck.core), round.measures);
}

/// An order whose value only passes through the node, its deposit out of it settling, leaves the wallet's account
/// holding nothing for it: the account's attribution is what the node holds (`REQ-AUD-15`, `INV-5`).
// verifies: REQ-AUD-15, INV-5
#[test]
fn value_that_passes_through_the_wallets_node_is_not_held_for_the_order() {
    let (mut core, session) = core_with("AC-6", &[CHAIN_IN, RAMP_B, OUT_B]);
    core.registers().accounts.insert(
        ADDRESS.to_owned(),
        kit::customer().party.as_str().to_owned(),
    );
    core.install_wallet(NODE, "USDC")
        .expect("the wallet is installed");
    let asked = request(
        "req-through",
        &session,
        AMOUNT,
        CHAIN_IN,
        outer("sim-chain-a", "USDC"),
        outer("sim-chain-e", "USDC"),
        OUT_B,
        ADDRESS,
        "lowest_total_cost",
    );
    let order = accepted(&mut core, asked);
    core.run(&order).expect("the order runs");
    assert_eq!(core.tick().landed, 0, "nothing rests at the node");
    assert_eq!(
        core.holdings().attributions(),
        [(NODE.to_owned(), Vec::new())],
        "the account holds nothing for the order"
    );
}

/// The customer's ask of the linked order: from the node to the address, the whole of what rests.
fn linked_order(stuck: &Stuck) -> LinkedOrder {
    let route: Request = serde_json::from_value(json!({
        "request_id": "req-linked",
        "round": {"kind": "shortlist"},
        "from": inner(NODE, "USDC"),
        "to": outer("sim-chain-e", "USDC"),
        "amount": AMOUNT,
        "objective": {"kind": "lowest_total_cost"},
    }))
    .expect("a route request");
    LinkedOrder {
        from: stuck.order.clone(),
        holder: id(&stuck.wallet.provider),
        customer: kit::customer(),
        session: stuck.session.clone(),
        route,
        inputs: BTreeMap::from([
            ("payer.bank_account".to_owned(), "acct-1".to_owned()),
            ("beneficiary.address".to_owned(), ADDRESS.to_owned()),
        ]),
    }
}

/// The entries of the log, with their receipts.
fn recorded(core: &Core) -> Vec<(Receipt, Entry)> {
    core.log()
        .entries()
        .iter()
        .map(|(r, e)| (r.clone(), e.clone()))
        .collect()
}

/// The attribution the log gave the stated entry at `receipt`.
fn attribution_at(core: &Core, receipt: &Receipt) -> Attribution {
    match recorded(core).into_iter().find(|(r, _)| r == receipt) {
        Some((_, Entry::Stated(s))) => s.attribution,
        other => panic!("a stated entry at {receipt:?}: {other:?}"),
    }
}

fn is_verified(attribution: &Attribution) -> bool {
    matches!(attribution, Attribution::Verified { .. })
}

/// The wallet asked whether it holds the order's value, under `transaction`: the operator's question and the wallet's
/// answer, each recorded as a stated entry about the transaction, as a move records them. The receipts of the two
/// entries and the answer's bytes.
fn balance_asked(
    core: &mut Core,
    wallet: &InstalledWallet,
    transaction: &str,
    account: &str,
) -> (Receipt, Receipt, Vec<u8>) {
    core.ask_wallet_stating(
        &wallet.provider,
        Operation::BalanceVerification(BalanceVerification {
            transaction: transaction.to_owned(),
            provider: wallet.provider.clone(),
            session: "s-1".to_owned(),
            account: account.to_owned(),
            asset: "USDC".to_owned(),
            amount: AMOUNT.to_owned(),
        }),
        &About {
            order: Some(id(account)),
            segment: None,
            transaction: Some(id(transaction)),
            node: None,
        },
    )
    .expect("the wallet is asked and its answer recorded")
}

/// The wallet is a module the operator publishes, whose release the log accepts by a decided catalogue change: until
/// then none of its answers is verified (the log knows no key that signs for it), and once it is, the same answer is
/// verified to the wallet, for the operator's party, by the key the release names; a key the release doesn't name
/// verifies nothing.
// verifies: REQ-CAT-23, REQ-AUD-2
#[test]
fn release_of_an_operator_published_wallet_is_accepted_in_the_log_and_its_answers_are_verified() {
    let mut stuck = stuck_at_the_public_node();
    let (wallet, order) = (stuck.wallet.clone(), stuck.order.clone());
    let core = &mut stuck.core;

    // Before the release is accepted the log holds no key for the wallet: the answer is recorded, attributed to no one.
    let (_, before, _) = balance_asked(core, &wallet, "tx-before", order.as_str());
    assert!(
        !is_verified(&attribution_at(core, &before)),
        "no accepted release, no verified statement"
    );

    // The release is accepted through the log's own catalogue change: decided, its effect and its done result.
    let effect = core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let whats = core.verified_whats();
    assert!(
        whats.iter().any(|w| matches!(w,
            What::Effect { action: Action::ChangeCatalogue { provider, statement_key: Some(key), .. }, .. }
                if provider.as_str() == wallet.provider && *key == wallet.statement_key())),
        "the effect names the release's statements key"
    );
    assert!(
        whats
            .iter()
            .any(|w| matches!(w, What::Result { effect: e } if *e == effect)),
        "and its result is recorded"
    );

    // The same wallet's answer, under another transaction, is now the wallet's, for the operator's party.
    let (_, after, answer) = balance_asked(core, &wallet, "tx-after", order.as_str());
    match attribution_at(core, &after) {
        Attribution::Verified { who } => {
            assert_eq!(who.acts.as_str(), wallet.provider);
            assert_eq!(who.party, kit::operator().party);
            assert_eq!(who.role, Role::Provider);
        }
        other => panic!("the wallet's answer is verified: {other:?}"),
    }
    // The answer is the wallet's own, signed with the key its release declares.
    let said: serde_json::Value = serde_json::from_slice(&answer).expect("a signed result");
    assert_eq!(said["maker"], wallet.provider.as_str());
    assert_eq!(
        said["signature"]["key"],
        wallet.statement_key().as_str(),
        "the key the release names"
    );
    assert!(rutent_conformance::signing::verify(&answer).is_ok());
}

/// The commitment of a provider's result the log holds about `order`: the evidence a decision names, a party's word
/// (Core's own questions are no evidence, `T-020-40`).
fn evidence_about(core: &Core, order: &Id) -> Vec<rutent_contracts::module_interface::Digest> {
    let log = core.log();
    let found = log
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
                    && s.about
                        .as_ref()
                        .is_some_and(|a| a.order.as_ref() == Some(order)) =>
            {
                Some(s.commitment.clone())
            }
            _ => None,
        })
        .expect("a statement about the order");
    vec![found]
}

/// The operator's decision to hand the stuck part's value on to `linked`, by `by`, on the evidence the log holds,
/// with the desks that decide it and move it.
fn operator_decides(
    stuck: &mut Stuck,
    linked: &AcceptedLinked,
    by: &str,
) -> (
    Actor,
    Result<rutent_contracts::core_internal::Decision, StepError>,
) {
    let (alice, bob) = (
        staff(&stuck.core, "desk-a", "alice"),
        staff(&stuck.core, "desk-b", "bob"),
    );
    {
        let mut registers = stuck.core.registers();
        registers.permit(&alice, Permission::DecideStuck);
        registers.permit(&bob, Permission::MoveStuck);
    }
    let resolution = Resolution::HandOn {
        by: id(by),
        linked_order: linked.order.clone(),
        asset: usdc(),
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    let evidence = evidence_about(&stuck.core, &stuck.order);
    let decision = stuck
        .core
        .decide_stuck(&stuck.order, &alice, &resolution, &evidence);
    (bob, decision)
}

/// A linked order the customer accepts is routed from the node by the router, shown as a quote, and accepted on the
/// yes of the provider its route starts with, in the customer's own session, which the log holds. The order isn't
/// opened by its acceptance: it starts, at the inner node where the value rests, when the value is handed on, and
/// then its route is the one it was accepted on.
// verifies: REQ-ORD-43, REQ-ORD-34
#[test]
fn linked_order_is_accepted_through_the_assembled_core_and_starts_at_an_inner_node() {
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let core = &stuck.core;

    // The route runs from the node where the value rests: the router's route is the deposit out of it, by the
    // reserve whose public node it is.
    let [part] = linked.route.plan.as_slice() else {
        panic!("one part: {:?}", linked.route.plan)
    };
    let [segment] = part.segments.as_slice() else {
        panic!("one segment: {part:?}")
    };
    assert_eq!(segment.provider.as_str(), OUT_B);
    assert_eq!(
        segment.kind,
        rutent_contracts::state_machine::OperationKind::Deposit
    );
    assert!(matches!(
        segment.from.node,
        rutent_contracts::provider_role::NodeRef::Inner(_)
    ));
    assert_eq!(node_name(&segment.from.node), NODE);
    assert_eq!(segment.from.asset.id, "USDC");

    // The log holds the router's record of the route chosen for the linked order, the quote shown, the starting
    // reserve's yes (its verified answer, about the linked order) and the customer's own acceptance on them.
    let whats = core.verified_whats();
    assert!(
        whats.iter().any(
            |w| matches!(w, What::DecisionRecord { decision_record, order, .. }
            if *decision_record == linked.route.decision_record && *order == linked.order)
        ),
        "the choice round's record is the log's"
    );
    assert!(
        whats
            .iter()
            .any(|w| matches!(w, What::Shown { order, .. } if *order == linked.order))
    );
    let (acceptance, who) = recorded(core)
        .into_iter()
        .find_map(|(_, e)| match e {
            Entry::Verified(v) => match v.what {
                What::Acceptance { ref order, .. } if *order == linked.order => {
                    Some((v.what, v.who))
                }
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .expect("the customer's acceptance");
    assert_eq!(who, kit::customer());
    let What::Acceptance {
        order,
        basis,
        yeses,
        session,
        parties,
        origin_reserve,
        ..
    } = acceptance
    else {
        unreachable!("an acceptance")
    };
    assert_eq!(order, linked.order);
    assert_eq!(
        basis,
        rutent_contracts::core_internal::AcceptanceBasis::Given
    );
    assert_eq!(session.as_ref(), Some(&stuck.session));
    assert_eq!(
        parties.requester,
        kit::customer().party,
        "the original customer is the requester"
    );
    assert_eq!(origin_reserve.as_str(), OUT_B);
    assert_eq!(yeses.len(), 1);
    assert_eq!(yeses[0].origin_reserve.as_str(), OUT_B);
    assert_eq!(yeses[0].reserve_yes, linked.yes);
    match attribution_at(core, &linked.yes) {
        Attribution::Verified { who } => assert_eq!(who.acts.as_str(), OUT_B),
        other => panic!("the starting reserve's yes is verified: {other:?}"),
    }

    // Accepted, the order isn't open: nothing of it reached the state machine, and Core holds the route it was accepted
    // on, which starts at the node.
    assert!(
        !core
            .inputs()
            .iter()
            .any(|i| i.subject.order.as_str() == linked.order.as_str()),
        "no input of the linked order yet"
    );
    assert_eq!(linked.route.order, linked.order);
    assert_eq!(linked.route.acceptance, linked.acceptance);
}

/// The hand-on of a stuck part's value through the holding account of a public node, the real plain wallet, names the
/// node's holding account and no other (`T-020-33`, which fixed `T-024-17`'s gap): the decide step takes the wallet as
/// the holder though it is a provider of no route, and a decision naming any other provider, the one that put the value
/// at the node or the linked route's first provider, is refused by the mover before the wallet is asked anything. The
/// part stays held and no record of a holding account starts a route.
// verifies: REQ-ORD-43, REQ-AUZ-4
#[test]
fn hand_on_through_a_public_nodes_wallet_names_the_wallet_and_refuses_any_other_holder() {
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let handled = stuck.core.operations_received(&wallet.provider);

    // Neither the provider that put the value at the node nor the linked route's first provider is the node's holding
    // account: each decision is taken, and each move is refused with the wallet asked nothing.
    for other in [RAMP_B, OUT_B] {
        let (bob, decision) = operator_decides(&mut stuck, &linked, other);
        let decision = decision.expect("asked");
        assert_eq!(decision.outcome, Answer::Permit {}, "{other}: {decision:?}");
        let carries_out = Decided {
            request: decision.request.clone(),
            decision: decision.decision.clone(),
        };
        let moved = stuck
            .core
            .move_stuck(&stuck.order, &carries_out, &bob)
            .expect("moved");
        assert!(
            matches!(
                moved,
                Moved::CarriedOut {
                    resolved: Resolved::Failed,
                    ..
                }
            ),
            "{other}: {moved:?}"
        );
        assert_eq!(
            stuck.core.operations_received(&wallet.provider),
            handled,
            "{other}: the wallet is asked nothing"
        );
        assert!(
            stuck.core.is_stuck(&stuck.order, &stuck.part),
            "{other}: the part stays held"
        );
    }
    let whats = stuck.core.verified_whats();
    assert!(
        !whats
            .iter()
            .any(|w| matches!(w, What::HoldingWithdrawal { .. })),
        "no holding account's record starts a route"
    );
}

/// Value that settles at a public inner node a holding account holds lands in it, by Core (`T-020-33`;
/// `Holdings::land`): the wallet is asked, in the operator's signed statement, to place the part's value in the
/// account, once for the segment that brought it, and the host attributes it to the order; a node with no holding
/// account takes none. Nothing in the scenario lands it.
// verifies: REQ-ORD-40, INV-5
#[test]
fn value_settled_at_a_public_node_held_by_a_holding_account_lands_in_it() {
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    // The wallet was asked once, to take the value in, and once for its balance by the tick's check, and the account
    // holds it for the order, all of it.
    assert_eq!(
        stuck.core.operations_received(&wallet.provider),
        2,
        "one deposit into the account, and the balance question of the tick's check: {:?}",
        stuck
            .core
            .world()
            .journal(&wallet.provider)
            .iter()
            .map(|e| serde_json::to_value(&e.operation.statement).unwrap()["kind"].clone())
            .collect::<Vec<_>>()
    );
    let held = stuck.core.holdings().attributions();
    assert_eq!(
        held,
        [(
            NODE.to_owned(),
            vec![rutent_prov::holding::HeldForOrder {
                order: stuck.order.as_str().to_owned(),
                amount: AMOUNT.parse().unwrap(),
            }]
        )]
    );
    // Landed once for the segment, however often Core is asked to land what settled.
    assert_eq!(stuck.core.tick().landed, 0);
    assert_eq!(stuck.core.operations_received(&wallet.provider), 2);
    assert_eq!(stuck.core.holdings().attributions(), held);
}

/// The holding check at a public node sets the module's balance beside what Core's own segments rest there, as it does at
/// a private node, and the wallet host's record is a second comparison (`INV-1`'s second half, `REQ-AUD-15`, `T-020-63`;
/// the milestone reviewer's check of `T-020-48`): a difference between what Core attributes and what the wallet holds is
/// raised even where the wallet's own record agrees with its balance, which the first build never compared.
// verifies: INV-1, REQ-AUD-15
#[test]
fn public_nodes_balance_is_checked_against_what_cores_segments_rest_there() {
    use rutent_contracts::core_internal::{Difference, Direction, Word};
    use rutent_core_orch::schedule::Job;
    let mut stuck = stuck_at_the_public_node();
    let (order, wallet) = (stuck.order.clone(), stuck.wallet.clone());
    let core = &mut stuck.core;
    core.set_cadence(Job::HoldingCheck, 0);

    // What Core rests there is what the wallet holds, and its host's record says the same: a match, one comparison.
    let round = core.tick();
    let held = round.holding_of(NODE).expect("the account is read");
    let reading = held.reading.as_ref().expect("read");
    assert_eq!(reading.balance.as_str(), AMOUNT);
    assert_eq!(reading.word, Word::Checked);
    assert_eq!(
        reading
            .held_for
            .iter()
            .map(|h| (h.order.as_str(), h.amount.amount().as_str()))
            .collect::<Vec<_>>(),
        [(order.as_str(), AMOUNT)],
        "what Core's segments rest at the node"
    );
    assert!(
        held.against_wallet.is_none(),
        "the host's record is Core's: nothing more to compare"
    );
    assert!(
        core.raised().is_empty(),
        "a match raises nothing: {:?}",
        core.raised()
    );

    // The wallet takes a hundred more for an order Core's segments never brought: the host's record and the wallet's
    // balance agree with each other, and not with what Core rests. The wallet's record alone would call it a match.
    core.land(NODE, "USDC", "ord-elsewhere", "100")
        .expect("the wallet takes the value");
    let round = core.tick();
    let held = round.holding_of(NODE).expect("the account is read");
    let reading = held.reading.as_ref().expect("read");
    assert_eq!(
        reading.balance.as_str(),
        "1100".replace("1100", &(AMOUNT.parse::<u128>().unwrap() + 100).to_string())
    );
    assert_eq!(
        reading.held_for.len(),
        1,
        "Core attributes the one order's value: {reading:?}"
    );
    held.check.as_ref().expect("the check is recorded");
    let second = held
        .against_wallet
        .as_ref()
        .expect("the host's record, whose total isn't Core's, is a second comparison");
    assert_eq!(
        second.reading.held_for.len(),
        2,
        "the host's record names both orders"
    );
    second.check.as_ref().expect("recorded too");
    let raised = core.raised();
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert!(
        matches!(
            &raised[0].difference,
            Difference::Holding { direction: Direction::Over, by, word: Word::Checked, .. } if by.amount().as_str() == "100"
        ),
        "the wallet holds more than Core attributes: {:?}",
        raised[0].difference
    );

    // The wallet then gives five hundred out that neither record knows of: short of both, each comparison says so, by
    // what it is short of.
    let statement = serde_json::json!({
        "kind": "withdrawal", "transaction": "direct-withdrawal-1", "provider": wallet.provider,
        "origin": { "network": wallet.provider }, "destination": NODE, "asset": "USDC",
        "amount": "500", "dry_run": false, "inputs": { "order": order.as_str() },
    });
    let operation: Operation = serde_json::from_value(statement).expect("an operation");
    core.ask_wallet(&wallet.provider, operation)
        .expect("the wallet answers the operator");
    core.tick();
    let shorts: Vec<String> = core
        .raised()
        .iter()
        .filter_map(|r| match &r.difference {
            Difference::Holding {
                direction: Direction::Short,
                by,
                ..
            } => Some(by.amount().as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(
        shorts,
        ["400", "500"],
        "short of what Core attributes by 400, of what the host's record says by 500"
    );
}

/// A holding account the catalogue lists at a node where Core's segments rest value, and the deployment's register of holding
/// accounts doesn't name, is raised to the operator as not read, once, and nothing is asked of its module: the register is the
/// definition of what is checked, and what it leaves out is said, not skipped (`REQ-AUD-15`, `INV-1`, `T-020-63`).
// verifies: REQ-AUD-15, INV-1
#[test]
fn holder_the_register_does_not_name_is_raised_as_not_read() {
    use rutent_contracts::core_internal::{Difference, HoldingNotRead};
    use rutent_core_orch::schedule::Job;
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    let core = &mut stuck.core;
    core.set_cadence(Job::HoldingCheck, 0);
    // The deployment's configuration lost the wallet's entry; the catalogue still lists it, and the order's value rests there.
    core.registers().holding_accounts.remove(NODE);
    let asked = core.operations_received(&wallet.provider);
    let round = core.tick();
    let held = round
        .holding_of(NODE)
        .expect("the node's account is in the round, not left out");
    assert!(held.reading.is_err() && held.check.is_err(), "{held:?}");
    assert!(matches!(held.not_read, Some(Ok(_))), "{held:?}");
    assert_eq!(
        core.operations_received(&wallet.provider),
        asked,
        "its module is asked nothing"
    );
    let unread: Vec<_> = core
        .raised()
        .into_iter()
        .filter_map(|r| match r.difference {
            Difference::HoldingNotRead { node, why, .. } => Some((node.as_str().to_owned(), why)),
            _ => None,
        })
        .collect();
    assert_eq!(
        unread,
        [(NODE.to_owned(), HoldingNotRead::NotRegistered {})]
    );
    // Once in a spell: the next tick raises no more.
    core.clock.advance(60);
    core.tick();
    assert_eq!(core.raised().len(), 1);
}

/// A public node's reading is stated in the decimals the deployment's register of holding accounts gives its asset, not in
/// a figure of the schedule's own (`REQ-AUD-15`, `T-020-63`; the milestone reviewer's check of `T-020-48`, its Low on
/// `DECIMALS`).
// verifies: REQ-AUD-15
#[test]
fn public_nodes_reading_is_stated_in_the_decimals_the_register_gives_its_asset() {
    use rutent_core_orch::schedule::Job;
    let mut stuck = stuck_at_the_public_node();
    let core = &mut stuck.core;
    core.set_cadence(Job::HoldingCheck, 0);
    let round = core.tick();
    let reading = round
        .holding_of(NODE)
        .and_then(|h| h.reading.as_ref().ok())
        .expect("the account is read");
    assert_eq!(reading.asset.decimals, 2, "as the register was given them");
    core.registers().holding_decimals.insert(NODE.to_owned(), 6);
    let round = core.tick();
    let reading = round
        .holding_of(NODE)
        .and_then(|h| h.reading.as_ref().ok())
        .expect("the account is read");
    assert_eq!(reading.asset.decimals, 6, "as the register now gives them");
}

/// One account that can't be read doesn't make the check read every account again on every tick: it is read again alone,
/// at its own cadence, and the accounts read keep their hour, so neither the modules are pressed nor the log and the
/// operator filled with what was said already (`NFR-6`, `REQ-AUD-15`, `T-020-63`; review M1).
// verifies: NFR-6, REQ-AUD-15
#[test]
fn one_unreadable_account_does_not_make_the_check_read_every_account_every_tick() {
    use rutent_contracts::core_internal::Difference;
    use rutent_core_orch::schedule::{HOLDING_RETRY_SECONDS, Job};
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    let core = &mut stuck.core;
    core.set_cadence(Job::HoldingCheck, 3600);
    // A private node's account whose module can't sign an answer: never read; and a difference at the wallet's node.
    let account = core
        .install_publisher_account("sim-chain-ledger", "sim-chain-publisher", "USDC")
        .expect("the publisher's account");
    core.world().make_key_store_unavailable(&account.provider);
    core.land(NODE, "USDC", "ord-elsewhere", "100")
        .expect("the wallet takes the value");
    // The hour has come round: the check reads both, raises the difference once, and the unread account once.
    core.clock.advance(3600);
    let round = core.tick();
    assert!(round.did(Job::HoldingCheck));
    let (log_after_hour, asked_wallet, asked_account) = (
        core.log().entries().len(),
        core.operations_received(&wallet.provider),
        core.operations_received(&account.provider),
    );
    let raised = core.raised().len();
    // Ticks every ten seconds inside the retry's cadence: nothing is asked of any module, nothing recorded or raised.
    for _ in 0..(HOLDING_RETRY_SECONDS / 10 - 1) {
        core.clock.advance(10);
        let round = core.tick();
        assert!(!round.did(Job::HoldingCheck));
    }
    assert_eq!(core.operations_received(&wallet.provider), asked_wallet);
    assert_eq!(core.operations_received(&account.provider), asked_account);
    assert_eq!(core.raised().len(), raised);
    // At its own cadence the unreadable account alone is read again: the wallet keeps its hour, and nothing is raised
    // again for what is raised already.
    core.clock.advance(10);
    let round = core.tick();
    assert!(round.did(Job::HoldingCheck));
    assert_eq!(round.holding.len(), 1, "{:?}", round.holding);
    assert_eq!(round.holding[0].node, "sim-chain-ledger");
    assert_eq!(core.operations_received(&wallet.provider), asked_wallet);
    assert_eq!(core.raised().len(), raised, "once in a spell");
    assert!(
        core.log().entries().len() - log_after_hour < 10,
        "a decision, an effect and a result"
    );
    assert_eq!(
        core.raised()
            .iter()
            .filter(|r| matches!(r.difference, Difference::Holding { .. }))
            .count(),
        1,
        "the difference at the wallet's node was raised once"
    );
}

/// The stuck order's value is handed on through the real plain wallet: the operator decides and another desk moves it,
/// the operator's question and the wallet's own answer are the log's stated entries about the linked order, the
/// answer verified under the wallet's accepted release, the effect's result is done, and the hand-on, the linked
/// order's `route_chosen` at the node and the holding account's withdrawal record follow, each admitted by the log.
///
/// The decision names the wallet, the node's holding account, which is a provider of no route (`T-020-33`).
// verifies: REQ-ORD-43, REQ-ORD-40
#[test]
fn stuck_order_of_the_assembled_core_is_handed_on_through_the_real_plain_wallet() {
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let (bob, decision) = operator_decides(&mut stuck, &linked, &wallet.provider);
    let decision = decision.expect("asked");
    assert_eq!(decision.outcome, Answer::Permit {}, "{decision:?}");
    let carries_out = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    let moved = stuck
        .core
        .move_stuck(&stuck.order, &carries_out, &bob)
        .expect("moved");
    let Moved::CarriedOut {
        effect,
        resolved: Resolved::Done,
        ..
    } = moved
    else {
        panic!("the move is done: {moved:?}")
    };
    // The question and the wallet's answer are the log's, about the linked order, before the effect's result.
    let stated: Vec<_> = recorded(&stuck.core)
        .into_iter()
        .filter_map(|(r, e)| match e {
            Entry::Stated(s)
                if s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(&linked.order) =>
            {
                Some((r, s))
            }
            _ => None,
        })
        .collect();
    let answered = stated
        .iter()
        .find(|(_, s)| s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE)
        .expect("the wallet's answer is the log's");
    assert!(is_verified(&answered.1.attribution));
    let handed = stuck
        .core
        .hand_on(&stuck.order, &effect, &bob)
        .expect("handed on");
    assert_eq!(node_name(&handed.at.node), NODE);
    let whats = stuck.core.verified_whats();
    let holding = whats
        .iter()
        .find_map(|w| match w {
            What::HoldingWithdrawal {
                order,
                holder,
                node,
                ..
            } if *order == linked.order => Some((holder.clone(), node.clone())),
            _ => None,
        })
        .expect("the holding account's record starts the linked order");
    assert_eq!(
        holding.0.as_str(),
        wallet.provider,
        "the wallet is the holder"
    );
    assert_eq!(holding.1.as_str(), NODE, "at the plan's first node");

    // The linked order's first move is the plan's provider's, not the wallet's, and the log admits all of it.
    let moves_before = stuck.core.operations_received(&wallet.provider);
    let ran = stuck
        .core
        .run_linked(&stuck.order, &linked_order(&stuck), &linked)
        .expect("the linked order's route runs");
    let Some(Ran::Segments(done)) = ran.first() else {
        panic!("the linked order ran: {ran:?}")
    };
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
    assert_eq!(
        done[0].segment.as_str(),
        linked.route.plan[0].segments[0].segment_id.as_str()
    );
    assert_eq!(
        stuck.core.operations_received(&wallet.provider),
        moves_before,
        "the wallet is asked nothing by the first move"
    );
    assert!(
        stuck.core.verified_whats().iter().any(|w| matches!(w,
            What::Effect { action: Action::MoveValue { order, provider, .. }, .. }
                if *order == linked.order && provider.as_str() == OUT_B)),
        "the first move, by the plan's provider, is in the log"
    );
    // The assembled Core's log is the real one, which admits each entry as it is appended: a run that returned
    // settled has every entry of the move admitted.
}

/// Whether the wallet itself answers that it holds the base amount for `order`: its own word, asked outside any
/// move, never the host's attribution.
fn wallet_holds(core: &mut Core, wallet: &InstalledWallet, order: &str, asking: &str) -> bool {
    wallet_holds_at_least(core, wallet, order, asking, AMOUNT)
}

/// Whether the wallet itself answers that it holds at least `amount` for `order`.
fn wallet_holds_at_least(
    core: &mut Core,
    wallet: &InstalledWallet,
    order: &str,
    asking: &str,
    amount: &str,
) -> bool {
    wallet_holds_in(core, wallet, order, asking, amount, "USDC")
}

/// Whether the wallet itself answers that it holds at least `amount` of `asset` for `order`.
fn wallet_holds_in(
    core: &mut Core,
    wallet: &InstalledWallet,
    order: &str,
    asking: &str,
    amount: &str,
    asset: &str,
) -> bool {
    let (_, answer) = core
        .ask_wallet(
            &wallet.provider,
            Operation::BalanceVerification(BalanceVerification {
                transaction: asking.to_owned(),
                provider: wallet.provider.clone(),
                session: "s-1".to_owned(),
                account: order.to_owned(),
                asset: asset.to_owned(),
                amount: amount.to_owned(),
            }),
        )
        .expect("the wallet answers");
    let answer: serde_json::Value = serde_json::from_slice(&answer).unwrap();
    answer["statement"]["outcome"]["sufficient"] == true
}

/// The stuck order's value handed on once, as the log and the wallet both say: the move done, the hand-on recorded,
/// and the host's attribution of the order gone with the withdrawal.
fn handed_on_once(stuck: &mut Stuck, linked: &AcceptedLinked, bob: &Actor, decision: &Decided) {
    let moved = stuck
        .core
        .move_stuck(&stuck.order, decision, bob)
        .expect("moved");
    let Moved::CarriedOut {
        effect,
        resolved: Resolved::Done,
        ..
    } = moved
    else {
        panic!("the move is done: {moved:?}")
    };
    stuck
        .core
        .hand_on(&stuck.order, &effect, bob)
        .expect("handed on");
    let withdrawals = stuck
        .core
        .verified_whats()
        .iter()
        .filter(|w| matches!(w, What::HoldingWithdrawal { order, .. } if *order == linked.order))
        .count();
    assert_eq!(withdrawals, 1, "one withdrawal starts the linked order");
    assert_eq!(
        stuck.core.holdings().attributions(),
        [(NODE.to_owned(), Vec::new())],
        "the account holds nothing for the order once it left"
    );
}

/// The wallet's module restarts, and the wallet's record of the order with it; the host still attributes the value,
/// which the wallet now says it doesn't hold. Core, opened again, asks the wallet what it holds for each order the
/// log settled at its node and lands again only what it lacks, under a transaction of its own: the wallet holds the
/// value again, the host attributes it once, and a second reopening lands nothing more. Then the stuck order's value
/// is handed on, its release finding the value in the account (`REQ-ORD-43`, `INV-1`).
// verifies: REQ-ORD-43, INV-1
#[test]
fn holding_account_whose_module_restarted_is_re_landed_from_the_log_before_a_release() {
    let names = [CHAIN_IN, RAMP_B, OUT_B];
    let scenario = || Scenario::new("execution", "AC-6", given(&names));
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    let order = stuck.order.as_str().to_owned();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let held = stuck.core.holdings().attributions();
    assert!(stuck.core.restart_module(&wallet.provider));
    assert!(
        !wallet_holds(&mut stuck.core, &wallet, &order, "ask-1"),
        "the wallet's record is gone"
    );
    stuck.core = scenario().reopen(stuck.core, 1, &mut Reference);
    assert!(
        wallet_holds(&mut stuck.core, &wallet, &order, "ask-2"),
        "re-landed from the log"
    );
    assert_eq!(
        stuck.core.holdings().attributions(),
        held,
        "attributed once"
    );
    // Opened again: the wallet is asked, holds it, and nothing more is landed.
    let asked = stuck.core.operations_received(&wallet.provider);
    stuck.core = scenario().reopen(stuck.core, 1, &mut Reference);
    assert_eq!(
        stuck.core.operations_received(&wallet.provider),
        asked + 1,
        "one question, no landing"
    );
    assert_eq!(stuck.core.holdings().attributions(), held);
    let twice = (2 * AMOUNT.parse::<u128>().unwrap()).to_string();
    assert!(
        !wallet_holds_at_least(&mut stuck.core, &wallet, &order, "ask-3", &twice),
        "the wallet holds the log's amount once, never twice"
    );
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let (bob, decision) = operator_decides(&mut stuck, &linked, &wallet.provider);
    let decision = decision.expect("asked");
    assert_eq!(decision.outcome, Answer::Permit {}, "{decision:?}");
    let decided = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    handed_on_once(&mut stuck, &linked, &bob, &decided);
}

/// The wallet's module restarts after the operator decided to hand the stuck order's value on, and the wallet's record
/// of the order goes with it. Core, which ran on, reads the platform's report of the restart and restores in the account
/// what the log holds for the order before the withdrawal, under a transaction of its own: the move is done, the order
/// handed on once, and the host's attribution of the order gone with the withdrawal (`REQ-ORD-43`).
// verifies: REQ-ORD-43
#[test]
fn stuck_order_is_handed_on_after_the_holding_accounts_module_restarts() {
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    let order = stuck.order.as_str().to_owned();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let (bob, decision) = operator_decides(&mut stuck, &linked, &wallet.provider);
    let decision = decision.expect("asked");
    assert_eq!(decision.outcome, Answer::Permit {}, "{decision:?}");
    let decided = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    assert!(stuck.core.restart_module(&wallet.provider));
    assert!(
        !wallet_holds(&mut stuck.core, &wallet, &order, "ask-1"),
        "the wallet's record is gone"
    );
    handed_on_once(&mut stuck, &linked, &bob, &decided);
}

/// How many inputs of `kind` the log holds about `order`.
fn inputs_of(
    core: &Core,
    order: &Id,
    kind: fn(&rutent_contracts::state_machine::Observed) -> bool,
) -> usize {
    core.inputs()
        .iter()
        .filter(|i| i.subject.order.as_str() == order.as_str() && kind(&i.observed))
        .count()
}

/// The hand-on of a stuck order's value, cut short by a crash right after the first order's `handed_on` reached the
/// log, before the linked order's `route_chosen`. The assembled Core opened again rebuilds from the log the hand-on it
/// holds half of, and the holding account's withdrawal its mover kept (the effect, its `done` result, the linked
/// order's acceptance and decision record, the wallet's own answer): asked again, it appends the linked order's
/// `route_chosen` and the holding account's record of its start, each once, and the `handed_on` stays the one the log
/// took; the route it logged is the one the linked order was accepted on, so its first move runs and settles
/// (`REQ-ORD-43`, `INV-4`).
// verifies: REQ-ORD-43, INV-4
#[test]
fn pending_hand_on_is_rebuilt_from_the_log_through_the_assembly() {
    let names = [CHAIN_IN, RAMP_B, OUT_B];
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let (bob, decision) = operator_decides(&mut stuck, &linked, &wallet.provider);
    let decision = decision.expect("asked");
    let decided = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    let moved = stuck
        .core
        .move_stuck(&stuck.order, &decided, &bob)
        .expect("moved");
    let Moved::CarriedOut {
        effect,
        resolved: Resolved::Done,
        ..
    } = moved
    else {
        panic!("the move is done: {moved:?}")
    };
    rutent_scenarios::harness::plant(
        &mut stuck.core,
        &rutent_scenarios::harness::Fault::CrashAfterRecord {
            call: "input",
            nth: 0,
        },
    );
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stuck.core.hand_on(&stuck.order, &effect, &bob)
    }));
    assert!(crashed.is_err(), "the crash stopped Core");
    stuck.core =
        Scenario::new("execution", "AC-6", given(&names)).reopen(stuck.core, 1, &mut Reference);
    let bob = staff(&stuck.core, "desk-b", "bob");
    let handed_on = |o: &rutent_contracts::state_machine::Observed| {
        matches!(
            o,
            rutent_contracts::state_machine::Observed::HandedOn { .. }
        )
    };
    let chosen = |o: &rutent_contracts::state_machine::Observed| {
        matches!(
            o,
            rutent_contracts::state_machine::Observed::RouteChosen { .. }
        )
    };
    assert_eq!(
        inputs_of(&stuck.core, &stuck.order, handed_on),
        1,
        "the handed_on is in the log"
    );
    assert_eq!(
        inputs_of(&stuck.core, &linked.order, chosen),
        0,
        "the linked route_chosen is not"
    );
    stuck
        .core
        .hand_on(&stuck.order, &effect, &bob)
        .expect("the hand-on, rebuilt from the log, completes");
    assert_eq!(
        inputs_of(&stuck.core, &stuck.order, handed_on),
        1,
        "the handed_on once"
    );
    assert_eq!(
        inputs_of(&stuck.core, &linked.order, chosen),
        1,
        "the linked route_chosen once"
    );
    let starts = stuck
        .core
        .verified_whats()
        .iter()
        .filter(|w| matches!(w, What::HoldingWithdrawal { order, .. } if *order == linked.order))
        .count();
    assert_eq!(
        starts, 1,
        "the holding account's record starts the linked order once"
    );
    // The route the reopened Core logged is the one the linked order was accepted on, segment by segment: its first
    // move runs on it and settles.
    let ran = stuck
        .core
        .run_linked(&stuck.order, &linked_order(&stuck), &linked)
        .expect("the linked order's route runs");
    let Some(Ran::Segments(done)) = ran.first() else {
        panic!("the linked order ran: {ran:?}")
    };
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
}

/// A stuck order whose value rests in the wallet's account, its linked order accepted, when Core reopens: the
/// reopened Core holds the account's attribution of the order as the log has it, and the linked order's accepted
/// route rebuilt from the log (its acceptance and the record its route was chosen by), so the operator's hand-on goes
/// ahead on it. Then the linked order, stopped right after policy registered its authorisation, is started again from
/// the log by a second reopening and runs its first move (`NFR-6`, `REQ-ORD-43`).
// verifies: NFR-6
#[test]
fn holdings_and_accepted_routes_of_a_reopened_order_are_rebuilt_from_the_log() {
    let names = [CHAIN_IN, RAMP_B, OUT_B];
    let scenario = || Scenario::new("execution", "AC-6", given(&names));
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let held = stuck.core.holdings().attributions();
    stuck.core = scenario().reopen(stuck.core, 1, &mut Reference);
    assert_eq!(
        stuck.core.holdings().attributions(),
        held,
        "the account holds the order's value as the log has it"
    );
    let (bob, decision) = operator_decides(&mut stuck, &linked, &wallet.provider);
    let decision = decision.expect("asked");
    let decided = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    handed_on_once(&mut stuck, &linked, &bob, &decided);
    rutent_scenarios::harness::plant(
        &mut stuck.core,
        &rutent_scenarios::harness::Fault::CrashAfterRecord {
            call: "authorise",
            nth: 0,
        },
    );
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stuck
            .core
            .run_linked(&stuck.order, &linked_order(&stuck), &linked)
    }));
    assert!(crashed.is_err(), "the crash stopped Core");
    stuck.core = scenario().reopen(stuck.core, 1, &mut Reference);
    let ran = stuck
        .core
        .run(&linked.order)
        .expect("the linked order runs on from the log");
    let Some(Ran::Segments(done)) = ran.first() else {
        panic!("the linked order ran: {ran:?}")
    };
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Settled { .. }),
        "{done:?}"
    );
}

/// What the real wallet answers a withdrawal is its own signed statement, which the platform passes on and the host
/// keeps re-serialised as the holder's stated entry (`T-020-26`): the bytes kept still verify under the key the
/// wallet's release publishes, are the wallet's by that key for the operator's party once the log accepted the
/// release, and a changed figure no longer verifies.
// verifies: REQ-AUD-2, REQ-ORD-40
#[test]
fn real_plain_wallets_re_serialised_answer_verifies_under_its_published_key() {
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let order = stuck.order.clone();
    let core = &mut stuck.core;

    // The holding account's withdrawal for the linked order's route, from where the original order's value rests.
    let (start, question) = core
        .withdraw_handed_on(&linked.route.plan[0].segments, order.as_str(), AMOUNT)
        .expect("the wallet withdraws");
    assert_eq!(start.reserve, wallet.provider);
    assert_eq!(start.node, NODE);
    assert_eq!(start.record.amount, AMOUNT.parse::<u128>().unwrap());

    // The answer, as kept: a signed result under the key the release publishes, which the wallet made.
    let signed: serde_json::Value = serde_json::from_slice(&start.answer).expect("a signed result");
    assert_eq!(signed["maker"], wallet.provider.as_str());
    assert_eq!(
        signed["shape"],
        rutent_contracts::provider_role::RESULT_SHAPE
    );
    assert_eq!(
        signed["statement"]["transaction"],
        start.transaction.as_str()
    );
    assert_eq!(signed["signature"]["key"], wallet.statement_key().as_str());
    assert!(
        rutent_conformance::signing::verify(&start.answer).is_ok(),
        "the re-serialised answer verifies"
    );
    let mut changed = signed.clone();
    changed["statement"]["outcome"]["amount_out"] = "1".into();
    assert!(
        rutent_conformance::signing::verify(&serde_json::to_vec(&changed).unwrap()).is_err(),
        "a changed figure doesn't"
    );

    // The log attributes it to the wallet: the operator's question first, under the wallet's own transaction, about
    // the linked order, then the answer.
    let about = About {
        order: Some(linked.order.clone()),
        segment: None,
        transaction: Some(id(&start.transaction)),
        node: None,
    };
    let asked = core
        .state_by(&question, &kit::operator(), &about)
        .expect("the question is recorded");
    assert!(
        is_verified(&attribution_at(core, &asked)),
        "the operator's question is the operator's"
    );
    let answered = core
        .state_answer(&start.answer, &about)
        .expect("the answer is recorded");
    match attribution_at(core, &answered) {
        Attribution::Verified { who } => {
            assert_eq!(who.acts.as_str(), wallet.provider);
            assert_eq!(who.party, kit::operator().party);
        }
        other => panic!("the wallet's answer is verified: {other:?}"),
    }
}

/// Two values of one order rest at the wallet's node as the log holds them, the wallet's module restarts, and one
/// part is handed on: Core's withdrawal restores the order's whole value at the node first, so after the part leaves
/// the wallet still holds the other value for the order and the host attributes it there (`INV-1`, `REQ-ORD-43`,
/// `T-020-46`). Restoring the part alone would leave the order's other value unheld.
// verifies: INV-1, REQ-ORD-43
#[test]
fn hand_on_after_a_module_restart_restores_the_orders_whole_value_at_the_node() {
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    let order = stuck.order.as_str().to_owned();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let route = linked.route.plan[0].segments.clone();
    assert!(stuck.core.restart_module(&wallet.provider));
    let part: u128 = AMOUNT.parse().expect("an amount");
    let whole = (2 * part).to_string();
    stuck
        .core
        .withdraw_for_a_hand_on(&route, &order, AMOUNT, &whole)
        .expect("withdrawn");
    assert!(
        wallet_holds_at_least(&mut stuck.core, &wallet, &order, "ask-other", AMOUNT),
        "the wallet still holds the order's other value"
    );
    let attributed: u128 = stuck
        .core
        .holdings()
        .attributions()
        .into_iter()
        .filter(|(node, _)| node == NODE)
        .flat_map(|(_, held)| held)
        .filter(|h| h.order == order)
        .map(|h| h.amount)
        .sum();
    assert_eq!(
        attributed, part,
        "the host attributes the other value at the node"
    );
}

/// What the host attributes to each order at `node`.
fn attributed_at(core: &mut Core, node: &str) -> Vec<(String, u128)> {
    core.holdings()
        .attributions()
        .into_iter()
        .filter(|(n, _)| n == node)
        .flat_map(|(_, held)| held)
        .map(|h| (h.order, h.amount))
        .collect()
}

/// A stuck order's value handed on, then Core reopened: the reopened Core reads the value as gone from the node with
/// the hand-on, so the host attributes nothing to the order there and the wallet holds nothing for it; nothing is
/// landed again (`INV-1`, `INV-8`, `T-020-46`).
// verifies: INV-1, INV-8
#[test]
fn handed_on_value_is_not_landed_again_after_core_reopens() {
    let names = [CHAIN_IN, RAMP_B, OUT_B];
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    let order = stuck.order.as_str().to_owned();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let (bob, decision) = operator_decides(&mut stuck, &linked, &wallet.provider);
    let decision = decision.expect("asked");
    let decided = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    handed_on_once(&mut stuck, &linked, &bob, &decided);
    stuck.core =
        Scenario::new("execution", "AC-6", given(&names)).reopen(stuck.core, 1, &mut Reference);
    assert_eq!(
        attributed_at(&mut stuck.core, NODE),
        Vec::new(),
        "nothing attributed again"
    );
    assert!(
        !wallet_holds_at_least(&mut stuck.core, &wallet, &order, "ask-after", "1"),
        "the wallet holds nothing for the handed-on order"
    );
}

/// Core reopened between a hand-on's withdrawal and the hand-on itself: the reopened Core reads the value as gone
/// with the done withdrawal, lands nothing again, and the hand-on then goes ahead, the account still holding nothing
/// for the order (`INV-1`, `INV-8`, `T-020-46`).
// verifies: INV-1, INV-8
#[test]
fn value_withdrawn_for_a_hand_on_is_not_landed_again_when_core_reopens_before_the_hand_on() {
    let names = [CHAIN_IN, RAMP_B, OUT_B];
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.clone();
    let order = stuck.order.as_str().to_owned();
    stuck
        .core
        .accept_release(&wallet)
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let (bob, decision) = operator_decides(&mut stuck, &linked, &wallet.provider);
    let decision = decision.expect("asked");
    let decided = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    let moved = stuck
        .core
        .move_stuck(&stuck.order, &decided, &bob)
        .expect("moved");
    let Moved::CarriedOut {
        effect,
        resolved: Resolved::Done,
        ..
    } = moved
    else {
        panic!("the move is done: {moved:?}")
    };
    stuck.core =
        Scenario::new("execution", "AC-6", given(&names)).reopen(stuck.core, 1, &mut Reference);
    assert_eq!(
        attributed_at(&mut stuck.core, NODE),
        Vec::new(),
        "nothing attributed again"
    );
    assert!(
        !wallet_holds_at_least(&mut stuck.core, &wallet, &order, "ask-after", "1"),
        "the wallet holds nothing for the order"
    );
    stuck
        .core
        .hand_on(&stuck.order, &effect, &bob)
        .expect("handed on");
    assert_eq!(attributed_at(&mut stuck.core, NODE), Vec::new());
}

/// Policy's `HoldingAccount` reads the deployment's register of holding accounts (`T-020-55` (1)): the operator
/// installed the wallet as the node's holding account, and the register names it, before Core reopens and after,
/// the register going on as the deployment keeps it. Once the register names another holder for the node, or none,
/// the answer follows the register, though the host's holdings Core carried across the reopen still name the wallet;
/// and the hand-on through the wallet, which policy would permit by the register, is refused once the register no
/// longer names it.
// verifies: REQ-AUZ-4, NFR-6
#[test]
fn holding_account_answer_reads_the_deployments_register_not_holdings_carried_across_a_reopen() {
    use rutent_core_authz::engine::{Need, Reply, Said, Source, Sources};

    let names = [CHAIN_IN, RAMP_B, OUT_B];
    let scenario = || Scenario::new("execution", "AC-6", given(&names));
    let mut stuck = stuck_at_the_public_node();
    let wallet = stuck.wallet.provider.clone();
    let ask = |core: &Core, by: &str| {
        core.sources().supply(&Need {
            input: id(by),
            from: Source::HoldingAccount {
                node: NODE.to_owned(),
            },
        })
    };
    assert_eq!(ask(&stuck.core, &wallet), Reply::Given(Said::Yes));
    assert_eq!(ask(&stuck.core, "another-wallet"), Reply::Given(Said::No));

    stuck.core = scenario().reopen(stuck.core, 1, &mut Reference);
    assert_eq!(
        ask(&stuck.core, &wallet),
        Reply::Given(Said::Yes),
        "the register goes on across the reopen"
    );
    let held_by = |core: &mut Core| {
        core.holdings()
            .account(NODE)
            .map(|a| a.provider().to_owned())
    };
    assert_eq!(held_by(&mut stuck.core), Some(wallet.clone()));

    stuck
        .core
        .registers()
        .holding_accounts
        .insert(NODE.to_owned(), "another-wallet".to_owned());
    assert_eq!(ask(&stuck.core, &wallet), Reply::Given(Said::No));
    stuck.core.registers().holding_accounts.remove(NODE);
    assert_eq!(ask(&stuck.core, &wallet), Reply::Missing);
    assert_eq!(
        held_by(&mut stuck.core),
        Some(wallet.clone()),
        "the host's holdings still name the wallet"
    );

    // The hand-on the register no longer backs is refused.
    stuck
        .core
        .accept_release(&stuck.wallet.clone())
        .expect("the release is accepted");
    let linked = stuck
        .core
        .accept_linked(&linked_order(&stuck))
        .expect("the linked order is accepted");
    let (_, decision) = operator_decides(&mut stuck, &linked, &wallet);
    assert!(
        !matches!(&decision, Ok(d) if d.outcome == Answer::Permit {}),
        "{decision:?}"
    );
}

// A linked order's reversal back into the held node (`T-020-57`) ------------------------------------------------

const RAMP_C: &str = "sim-chain-ramp-c";
const OUT_C: &str = "sim-chain-out-c";

/// An order across a reserve, two ramps and a reserve out, to the network `sim-chain-d`: its move out of the public
/// node, the second ramp's, is refused its start, so its value rests in the wallet there, stuck for want of a route;
/// the wallet's release accepted in the log.
fn stuck_before_the_second_ramp() -> Stuck {
    let (mut core, session) = core_with("AC-6", &[CHAIN_IN, RAMP_B, RAMP_C, OUT_C]);
    core.registers().accounts.insert(
        ADDRESS.to_owned(),
        kit::customer().party.as_str().to_owned(),
    );
    let wallet = core
        .install_wallet(NODE, "USDC")
        .expect("the wallet is installed");
    core.accept_release(&wallet)
        .expect("the release is accepted");
    let asked = request(
        "req-stuck",
        &session,
        AMOUNT,
        CHAIN_IN,
        outer("sim-chain-a", "USDC"),
        outer("sim-chain-d", "USDC"),
        OUT_C,
        ADDRESS,
        "lowest_total_cost",
    );
    let order = accepted(&mut core, asked);
    let part = core.first_part(&order).expect("a started order");
    core.faults.refuse_begin = Some(2);
    core.run(&order).expect("the order runs");
    assert_eq!(core.tick().landed, 1, "the schedule lands the value");
    core.leave_no_route(&order).expect("the part is stuck");
    assert!(core.is_stuck(&order, &part));
    Stuck {
        core,
        order,
        part,
        session,
        wallet,
    }
}

/// The customer's ask of the linked order: from the node, by the second ramp, to the address on `sim-chain-d`.
fn linked_on_the_second_ramp(stuck: &Stuck) -> LinkedOrder {
    let mut linked = linked_order(stuck);
    linked.route = serde_json::from_value(json!({
        "request_id": "req-linked",
        "round": {"kind": "shortlist"},
        "from": inner(NODE, "USDC"),
        "to": outer("sim-chain-d", "USDC"),
        "amount": AMOUNT,
        "objective": {"kind": "lowest_total_cost"},
    }))
    .expect("a route request");
    linked
}

/// The stuck order's value handed on to a linked order whose first move, the second ramp's transfer out of the node,
/// fails owing its reversal: the linked order, its accepted route, and the failed move's segment.
fn linked_first_move_failed() -> (Stuck, LinkedOrder, AcceptedLinked, Id) {
    let mut stuck = stuck_before_the_second_ramp();
    let wallet = stuck.wallet.clone();
    let asked = linked_on_the_second_ramp(&stuck);
    let linked = stuck
        .core
        .accept_linked(&asked)
        .expect("the linked order is accepted");
    let (bob, decision) = operator_decides(&mut stuck, &linked, &wallet.provider);
    let decision = decision.expect("asked");
    let carries_out = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    handed_on_once(&mut stuck, &linked, &bob, &carries_out);
    stuck.core.world().script_for(
        RAMP_C,
        "transfer",
        rutent_conformance::sim::Behaviour::act(rutent_conformance::sim::Act::Fail),
    );
    let ran = stuck
        .core
        .run_linked(&stuck.order, &asked, &linked)
        .expect("the linked order's route runs");
    let Some(Ran::Segments(done)) = ran.first() else {
        panic!("the linked order ran: {ran:?}")
    };
    assert!(
        matches!(done[0].outcome, SegmentOutcome::Failed),
        "{done:?}"
    );
    let segment = done[0].segment.clone();
    (stuck, asked, linked, segment)
}

/// A hand-on's linked order's first move out of the public node fails, owing its reversal. A route handed on has no
/// withdrawal: it starts from the node's holding account, which counts as its origin reserve, so Core asks the wallet
/// whether the credit came back. Until the account's statement holds it, the wallet doesn't confirm it and nothing is
/// landed; once the account took the credit back, in full, the wallet's own signed confirmation completes the reversal,
/// the log takes its evidence as the wallet's, and the value is landed again in the wallet for the linked order, whose
/// value it is now, as the wallet itself says (`REQ-ORD-43`, `INV-1`, `T-020-57`).
// verifies: REQ-ORD-43, INV-1
#[test]
fn linked_orders_reversal_back_into_a_held_node_is_confirmed_by_the_nodes_wallet_and_completes() {
    use rutent_core_orch::reversal::{Confirmation, NotCounted};
    let (mut stuck, _, linked, segment) = linked_first_move_failed();
    let wallet = stuck.wallet.clone();
    let owing = stuck
        .core
        .owe_reversal(&linked.order, &segment)
        .expect("the reversal is owed");
    assert_eq!(owing.provider.as_str(), RAMP_C, "owed by the failed ramp");

    // Nothing came back yet, then less than the move took in: neither is confirmed, and nothing is landed again.
    for credit in [None, Some("999999")] {
        if let Some(amount) = credit {
            // The world's credit back into the account: it stands in for the network reader's confirmation (`T-020-60`).
            stuck
                .core
                .credit_wallet(&wallet.provider, owing.transaction.as_str(), "USDC", amount);
        }
        let answer = stuck
            .core
            .confirm_reversal(&linked.order, &segment)
            .expect("the wallet is asked");
        assert!(
            matches!(
                answer,
                Confirmation::NotCompleted {
                    why: NotCounted::NotConfirmed,
                    ..
                }
            ),
            "{credit:?}: {answer:?}"
        );
        stuck.core.tick();
        assert!(
            !wallet_holds_at_least(
                &mut stuck.core,
                &wallet,
                linked.order.as_str(),
                "ask-short",
                "1"
            ),
            "{credit:?}: nothing landed again"
        );
    }

    // The account took the credit back, in full: the wallet confirms it, and the reversal completes on its word.
    // The world's credit back into the account: it stands in for the network reader's confirmation (`T-020-60`).
    stuck
        .core
        .credit_wallet(&wallet.provider, owing.transaction.as_str(), "USDC", AMOUNT);
    let answer = stuck
        .core
        .confirm_reversal(&linked.order, &segment)
        .expect("the wallet is asked");
    let Confirmation::Completed(done) = answer else {
        panic!("completed: {answer:?}")
    };
    assert_eq!(done.whose.as_str(), wallet.provider, "on the wallet's word");
    assert!(
        stuck.core.verified_whats().iter().any(|w| matches!(w,
            What::Evidence { order, segment: s, whose, .. }
                if *order == linked.order && *s == segment && whose.as_str() == wallet.provider)),
        "the log takes the evidence as the wallet's"
    );
    stuck.core.tick();
    assert!(
        wallet_holds(&mut stuck.core, &wallet, linked.order.as_str(), "ask-back"),
        "landed again in the wallet for the linked order, as the wallet says"
    );
    assert!(
        !wallet_holds(&mut stuck.core, &wallet, stuck.order.as_str(), "ask-first"),
        "nothing for the order it was handed on from"
    );
    stuck.core.tick();
    assert_eq!(
        stuck.core.holdings().attributions(),
        [(
            NODE.to_owned(),
            vec![rutent_prov::holding::HeldForOrder {
                order: linked.order.as_str().to_owned(),
                amount: AMOUNT.parse().unwrap(),
            }]
        )],
        "attributed once, to the linked order"
    );
}

/// The linked order's first move fails and its reversal completes on the wallet's word: the value is landed again in
/// the wallet for the linked order. The part is recalculated from the node, which retires the reversed move, and Core
/// stops before the new route runs. Reopened, Core rebuilds the returned value from the log's `reversal_completed` and
/// the wallet's confirmation it rests on, so when the new route's first move takes the value out of the node, the
/// wallet's landing of it is released, once, as a Core that ran on would release it: the wallet holds nothing more
/// for the linked order (`NFR-6`, `INV-1`, `T-020-57`).
// verifies: NFR-6, INV-1
#[test]
fn returned_value_retired_by_a_recalculation_is_rebuilt_after_core_reopens() {
    use rutent_core_orch::recalc::Recalculation;
    use rutent_core_orch::reversal::Confirmation;
    let (mut stuck, _, linked, segment) = linked_first_move_failed();
    let wallet = stuck.wallet.clone();
    let owing = stuck
        .core
        .owe_reversal(&linked.order, &segment)
        .expect("the reversal is owed");
    // The world's credit back into the account: it stands in for the network reader's confirmation (`T-020-60`).
    stuck
        .core
        .credit_wallet(&wallet.provider, owing.transaction.as_str(), "USDC", AMOUNT);
    let answer = stuck
        .core
        .confirm_reversal(&linked.order, &segment)
        .expect("the wallet is asked");
    assert!(matches!(answer, Confirmation::Completed(_)), "{answer:?}");
    stuck.core.tick();
    assert!(
        wallet_holds(&mut stuck.core, &wallet, linked.order.as_str(), "ask-back"),
        "landed again for the linked order"
    );

    // Recalculated from the node: the reversed move is retired, and the new route waits to run.
    let recalculated = match stuck
        .core
        .recalculate(&linked.order)
        .expect("the part is recalculated")
    {
        Recalculation::Recalculated(recalculated) => recalculated,
        other => panic!("recalculated: {other:?}"),
    };
    assert!(
        stuck.core.verified_whats().iter().any(|w| matches!(w,
            What::Input { input } if input.subject.order.as_str() == linked.order.as_str()
                && matches!(&input.observed,
                    rutent_contracts::state_machine::Observed::RouteRecalculated { segments, .. }
                        if segments.iter().all(|s| s.segment_id.as_str() != segment.as_str())))),
        "the log recalculated the part, the reversed move left out"
    );

    // Core stops and is opened again on its disk.
    let names = [CHAIN_IN, RAMP_B, RAMP_C, OUT_C];
    stuck.core =
        Scenario::new("execution", "AC-6", given(&names)).reopen(stuck.core, 1, &mut Reference);
    assert!(
        wallet_holds(
            &mut stuck.core,
            &wallet,
            linked.order.as_str(),
            "ask-reopened"
        ),
        "the wallet still holds the returned value"
    );

    // The new route's first move takes the value out of the node: its landing is released, once.
    stuck
        .core
        .run_recalculated(&linked.order, &recalculated)
        .expect("the recalculated route runs");
    stuck.core.tick();
    stuck.core.tick();
    assert!(
        !wallet_holds_at_least(
            &mut stuck.core,
            &wallet,
            linked.order.as_str(),
            "ask-gone",
            "1"
        ),
        "released once the value left the node"
    );
    assert_eq!(
        stuck.core.holdings().attributions(),
        [(NODE.to_owned(), Vec::new())],
        "the account holds nothing for any order"
    );
}

/// The operator registers another module as the node's holding account after the hand-on. Core still asks the holder
/// the log names, the one whose `holding_withdrawal` started the linked order and the one party the log takes as the
/// reserve there, so Core and the log never name different confirmers: the reversal completes on the first wallet's
/// word, not the new module's (`REQ-ORD-43`, `REQ-ORD-3`, `T-020-57`'s review, L1).
// verifies: REQ-ORD-43, REQ-ORD-3
#[test]
fn reversal_is_asked_of_the_holder_the_log_names_after_the_node_is_registered_again() {
    use rutent_core_orch::reversal::Confirmation;
    let (mut stuck, _, linked, segment) = linked_first_move_failed();
    let wallet = stuck.wallet.clone();
    stuck
        .core
        .register_node_holding(NODE, "operator-wallet-2")
        .expect("the node is registered to another module");
    let owing = stuck
        .core
        .owe_reversal(&linked.order, &segment)
        .expect("the reversal is owed");
    // The world's credit back into the account: it stands in for the network reader's confirmation (`T-020-60`).
    stuck
        .core
        .credit_wallet(&wallet.provider, owing.transaction.as_str(), "USDC", AMOUNT);
    let answer = stuck
        .core
        .confirm_reversal(&linked.order, &segment)
        .expect("the holder the log names is asked");
    let Confirmation::Completed(done) = answer else {
        panic!("completed: {answer:?}")
    };
    assert_eq!(
        done.whose.as_str(),
        wallet.provider,
        "on the word of the holder that started the order"
    );
}

/// The holder the log names is a module the operator publishes whose release the log revoked after the hand-on: none
/// of its statements would be attributed, so Core doesn't ask it, says why, and the reversal stays owed (`REQ-ORD-3`,
/// `T-020-57`'s review, L1).
// verifies: REQ-ORD-3
#[test]
fn holder_with_no_release_in_force_is_not_asked_and_the_reversal_stays_owed() {
    let (mut stuck, _, linked, segment) = linked_first_move_failed();
    let wallet = stuck.wallet.clone();
    stuck
        .core
        .revoke_release(&wallet)
        .expect("the release is revoked");
    let owing = stuck
        .core
        .owe_reversal(&linked.order, &segment)
        .expect("the reversal is owed");
    let asked = stuck.core.operations_received(&wallet.provider);
    let refused = stuck.core.confirm_reversal(&linked.order, &segment);
    assert!(
        matches!(
            &refused,
            Err(StepError::Reversal(rutent_core_orch::reversal::ReversalError::HolderNotInForce(h)))
                if h.as_str() == wallet.provider
        ),
        "{refused:?}"
    );
    assert_eq!(
        stuck.core.operations_received(&wallet.provider),
        asked,
        "the wallet is asked nothing"
    );
    assert_eq!(
        stuck
            .core
            .owe_reversal(&linked.order, &segment)
            .expect("still owed")
            .transaction,
        owing.transaction
    );
}

/// The operator's catalogue settings stand when the deployment's own listing touches the catalogue (`T-020-49`, L2 of
/// #337's review): the operator unlists the node, then a wallet is installed there, which lists the node for its
/// registration; the node is off the public list again after it, as the operator's setting says. An advertisement's
/// join re-applies the settings the same way (`Core::advertise`).
// verifies: REQ-AUZ-6
#[test]
fn operators_catalogue_settings_stand_after_the_deployments_own_listing() {
    use rutent_contracts::core_internal::OperatorSetting;
    let (mut core, _) = core_with("AC-6", &[CHAIN_IN, RAMP_B, OUT_B]);
    assert!(
        core.catalogue().nodes().is_public(NODE),
        "the join listed it"
    );
    let unlisted: OperatorSetting = serde_json::from_value(serde_json::json!({
        "setting": "public_node", "node": { "network": NODE }, "listed": false,
    }))
    .expect("a setting");
    core.set_setting(&unlisted)
        .expect("the operator unlists it");
    assert!(!core.catalogue().nodes().is_public(NODE));
    core.install_wallet(NODE, "USDC")
        .expect("the wallet is installed");
    assert!(
        !core.catalogue().nodes().is_public(NODE),
        "the operator's setting stands after the wallet's install"
    );
}

/// The operator's `public_node` setting at `node`, listed or not.
fn public_node(node: &str, listed: bool) -> rutent_contracts::core_internal::OperatorSetting {
    serde_json::from_value(json!({
        "setting": "public_node", "node": { "network": node }, "listed": listed,
    }))
    .expect("a setting")
}

/// How many `set_setting` effects Core's log holds.
fn settings_set(core: &Core) -> usize {
    core.verified_whats()
        .iter()
        .filter(|w| {
            matches!(
                w,
                What::Effect {
                    action: rutent_contracts::core_internal::Action::ChangeCatalogue {
                        change: rutent_contracts::core_internal::CatalogueChange::SetSetting,
                        ..
                    },
                    ..
                }
            )
        })
        .count()
}

/// A public node can't be unlisted while a segment or a holding rests there (`T-020-65`, `REQ-CAT-22`, the rule in
/// `F-020`'s notes): with the order's value landed in the node's holding account, the operator's unlist is refused after
/// policy is asked, no effect is recorded and the node stays on the public list. Once nothing of Core's rests at the
/// node, an unlist is permitted, and the snapshot a route is chosen from leaves the node out: the same order is quoted
/// no route through it.
// verifies: REQ-CAT-22
#[test]
fn public_node_cannot_be_unlisted_while_a_segment_or_holding_rests_there() {
    use rutent_core_orch::intake::{Status, Told};
    use rutent_core_orch::settings::SettingRefused;
    let mut stuck = stuck_at_the_public_node();
    let before = settings_set(&stuck.core);
    let refused = stuck.core.set_setting(&public_node(NODE, false));
    assert!(
        matches!(&refused, Err(SettingRefused::Rests { node }) if node == NODE),
        "{refused:?}"
    );
    assert_eq!(settings_set(&stuck.core), before, "no effect recorded");
    assert!(
        stuck.core.catalogue().nodes().is_public(NODE),
        "still public"
    );

    // Nothing of Core's rests at the node: the unlist takes effect, and the snapshot leaves the node out.
    let (mut core, session) = core_with("AC-6", &[CHAIN_IN, RAMP_B, OUT_B]);
    core.registers().accounts.insert(
        ADDRESS.to_owned(),
        kit::customer().party.as_str().to_owned(),
    );
    core.install_wallet(NODE, "USDC")
        .expect("the wallet is installed");
    core.set_setting(&public_node(NODE, false))
        .expect("nothing rests there: unlisted");
    assert!(!core.catalogue().nodes().is_public(NODE));
    let (_, told) = quoted(
        &mut core,
        request(
            "req-after-unlist",
            &session,
            AMOUNT,
            CHAIN_IN,
            outer("sim-chain-a", "USDC"),
            outer("sim-chain-e", "USDC"),
            OUT_B,
            ADDRESS,
            "lowest_total_cost",
        ),
    );
    assert!(
        !matches!(
            told,
            Told::Exists {
                status: Status::Quoted { .. },
                ..
            }
        ),
        "no route through the unlisted node: {told:?}"
    );
}
