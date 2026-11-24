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
/// installed at the node and holding the order's value.
fn stuck_at_the_public_node() -> Stuck {
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
    assert_eq!(core.land_settled(), 0, "nothing rests at the node");
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
    // The wallet was asked once, to take the value in, and the account holds it for the order, all of it.
    assert_eq!(
        stuck.core.operations_received(&wallet.provider),
        1,
        "one deposit into the account"
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
    assert_eq!(stuck.core.land_settled(), 0);
    assert_eq!(stuck.core.operations_received(&wallet.provider), 1);
    assert_eq!(stuck.core.holdings().attributions(), held);
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
    let (_, answer) = core
        .ask_wallet(
            &wallet.provider,
            Operation::BalanceVerification(BalanceVerification {
                transaction: asking.to_owned(),
                provider: wallet.provider.clone(),
                session: "s-1".to_owned(),
                account: order.to_owned(),
                asset: "USDC".to_owned(),
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
