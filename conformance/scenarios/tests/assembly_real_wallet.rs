//! The assembled Core hands a stuck order on through the real plain wallet (`T-024-17`; `REQ-ORD-43`, `REQ-ORD-40`,
//! `REQ-CAT-23`, `REQ-AUD-2`, `REQ-ORD-34`), where `T-020-26`'s assembled-Core test is the fail-closed one.
//!
//! The order runs over simulated providers to a public node, where its value rests in the operator's plain wallet: a
//! reserve into its private node, a ramp from there to the public node `sim-chain-b`, and the deposit out of it,
//! which the platform refuses to begin, so the part is stuck at the node. The wallet is the real one, built from
//! `components/prov/guests/plain-wallet/` the way the simulated providers are, admitted to the platform on the
//! operator's own signed release, and its release accepted in the log by a decided catalogue change.

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
use rutent_core_orch::stuck::{Moved, Resolved};
use rutent_scenarios::assembly::linked::{AcceptedLinked, LinkedOrder};
use rutent_scenarios::assembly::{Core, StepError};
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
/// it: stuck for want of a route from there.
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
    core.land(NODE, "USDC", order.as_str(), AMOUNT)
        .expect("the wallet takes the order's value");
    Stuck {
        core,
        order,
        part,
        session,
        wallet,
    }
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

/// The commitment of a statement the log holds about `order`: the evidence a decision names.
fn evidence_about(core: &Core, order: &Id) -> Vec<rutent_contracts::module_interface::Digest> {
    let log = core.log();
    let found = log
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s)
                if s.about
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

/// The hand-on of a stuck part's value to a linked order through the holding account of a public node, the real plain
/// wallet, is blocked by three of Core's rules that one provider can't meet at once, and the wallet is asked nothing
/// (`T-024-17`'s gap, in `F-024`'s notes): the decide step takes `by` among the providers of the part's route
/// (`stuck::within`), the mover takes it as the provider of the node's holding account and the provider of the linked
/// route's first segment (`Mover::plan_hand`), and the wallet is no provider of any route (it advertises no tuple, and no
/// segment runs from a node to itself). So this refuses, fail closed, each way a decision can name it.
// verifies: REQ-ORD-43, REQ-AUZ-4
#[test]
fn hand_on_through_a_public_nodes_wallet_is_refused_whichever_provider_the_decision_names() {
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

    // Naming the wallet, the holder of the node: the decide step refuses it, as no provider of the part's route.
    let (_, decision) = operator_decides(&mut stuck, &linked, &wallet.provider);
    assert!(
        matches!(
            decision,
            Err(StepError::Resolve(
                rutent_core_orch::stuck::ResolveError::NotItsValue
            ))
        ),
        "{decision:?}"
    );

    // Naming the provider that put the value at the node, which the decide step takes: the mover refuses it before the
    // wallet is asked, since it is neither the node's holding account nor the linked route's first provider.
    let (bob, decision) = operator_decides(&mut stuck, &linked, RAMP_B);
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
    assert!(
        matches!(
            moved,
            Moved::CarriedOut {
                resolved: Resolved::Failed,
                ..
            }
        ),
        "{moved:?}"
    );
    assert_eq!(
        stuck.core.operations_received(&wallet.provider),
        handled,
        "the wallet is asked nothing"
    );
    assert!(
        stuck.core.is_stuck(&stuck.order, &stuck.part),
        "the part stays held"
    );
    let whats = stuck.core.verified_whats();
    assert!(
        !whats
            .iter()
            .any(|w| matches!(w, What::HoldingWithdrawal { .. })),
        "no holding account's record starts a route"
    );
}

/// The stuck order's value is handed on through the real plain wallet: the operator decides and another desk moves it,
/// the operator's question and the wallet's own answer are the log's stated entries about the linked order, the
/// answer verified under the wallet's accepted release, the effect's result is done, and the hand-on, the linked
/// order's `route_chosen` at the node and the holding account's withdrawal record follow, each admitted by the log.
///
/// Blocked by Core (see `hand_on_through_a_public_nodes_wallet_is_refused_whichever_provider_the_decision_names`):
/// ignored until a hand-on's holder may be a wallet that is no provider of the route.
// verifies: REQ-ORD-43, REQ-ORD-40
#[test]
#[ignore = "T-024-17's gap: a plain wallet can't be a hand-on's holder (stuck::within, Mover::plan_hand); see F-024's notes"]
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
    assert!(
        whats
            .iter()
            .any(|w| matches!(w, What::HoldingWithdrawal { order, .. } if *order == linked.order))
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
