//! The plain wallet (`T-016-02`; `REQ-CAT-23`, `INV-5`, `REQ-MDL-1`): a Wasm component admitted like any module and
//! registered by the operator as a public node's holding account, which keeps the value resting at its node for
//! each order.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;
#[path = "holding_support/mod.rs"]
mod support;

use rutent_contracts::provider_role::{Operation, ReserveMovementOutcome, ResultBody};
use rutent_prov::catalogue::{HoldingSource, NodeError};
use rutent_prov::holding::wallet::{Holdings, RegisterError, WalletError};
use rutent_prov::holding::{
    BalanceSource, HoldingAccount, HoldingError, Reconciliation, reconcile,
};
use rutent_prov::module::admission::Unverified;
use rutent_prov::module::types::{Admitted, Handled};
use serde_json::json;
use signed::{RAMP, RESERVE, Signed};
use support::{TestOperator, admit_wallet, covers, run_wallet};

use signed::catalogue_support::{CASES, Rng};

fn operation(value: serde_json::Value) -> Operation {
    serde_json::from_value(value).expect("a valid operation")
}

/// What the wallet of `provider` answers to a deposit of `amount` of `asset` for `order` at `node`.
fn deposit_answer(
    s: &mut Signed,
    provider: &str,
    node: &str,
    asset: &str,
    order: &str,
    amount: &str,
) -> ResultBody {
    use rutent_prov::holding::wallet::Operator as _;
    let statement = TestOperator.statement(operation(json!({
        "kind": "deposit",
        "transaction": format!("tx-{order}-{amount}"),
        "provider": provider,
        "origin": node,
        "destination": { "network": provider },
        "asset": asset,
        "amount": amount,
        "dry_run": false,
        "inputs": { "order": order },
    })));
    match s.platform.handle(provider, &statement) {
        Handled::Answered(result) => result.statement,
        Handled::Refused(why) => panic!("the platform refused the deposit: {why:?}"),
    }
}

// verifies: REQ-CAT-23, REQ-MDL-1
#[test]
fn operator_registers_a_plain_wallet_as_a_nodes_holding_account() {
    let mut s = Signed::new();
    run_wallet(&mut s, RESERVE, "1.0.0");
    assert!(s.catalogue.nodes().holding("polygon").is_none());

    let mut holdings = Holdings::default();
    holdings
        .register(&mut s.catalogue, &s.platform, "polygon", RESERVE, "USDC")
        .expect("the operator registers the wallet");

    let account = s.catalogue.nodes().holding("polygon").expect("registered");
    assert_eq!(account.reference, RESERVE);
    assert_eq!(account.source, HoldingSource::Operator);

    // The wallet is the account: value landing at the node is recorded by it, and read back as its reading.
    holdings
        .land(
            &mut s.platform,
            &TestOperator,
            "polygon",
            "USDC",
            "o-1",
            "100",
        )
        .expect("the wallet takes the value");
    holdings
        .land(
            &mut s.platform,
            &TestOperator,
            "polygon",
            "USDC",
            "o-2",
            "50",
        )
        .expect("the wallet takes the value");
    let reading = holdings
        .account("polygon")
        .expect("the node's account")
        .reading()
        .expect("a reading");
    assert_eq!(reading.node, "polygon");
    assert_eq!(reading.asset, "USDC");
    assert_eq!(reading.balance, 150);
    assert_eq!(reading.source, BalanceSource::Module);
    assert_eq!(reconcile(&reading), Ok(Reconciliation::Matches));
    let held: Vec<(&str, u128)> = reading
        .held
        .iter()
        .map(|h| (h.order.as_str(), h.amount))
        .collect();
    assert_eq!(held, [("o-1", 100), ("o-2", 50)]);
}

// verifies: REQ-CAT-23, REQ-MDL-1
#[test]
fn wallet_that_does_not_run_is_not_registered_and_neither_is_a_node_the_operator_does_not_list() {
    let mut s = Signed::new();
    let mut holdings = Holdings::default();

    // Never admitted.
    assert_eq!(
        holdings.register(&mut s.catalogue, &s.platform, "polygon", RESERVE, "USDC"),
        Err(RegisterError::NotRunning(RESERVE.to_owned()))
    );

    // Admitted, but waiting for its fresh acceptance: it doesn't run.
    let admitted = admit_wallet(&mut s, RESERVE, "1.0.0");
    assert!(matches!(&admitted, Admitted::Artefact(a) if a.acceptance.accepted.is_none()));
    assert_eq!(
        holdings.register(&mut s.catalogue, &s.platform, "polygon", RESERVE, "USDC"),
        Err(RegisterError::NotRunning(RESERVE.to_owned()))
    );

    // Its assertion doesn't verify, so it was never admitted (signed with the statements key).
    let (declaration, binaries) = support::wallet_release(RAMP, "1.0.0");
    let wrongly_signed = signed::assertion(
        &declaration,
        &binaries,
        &signed::catalogue_support::statements_key(RAMP),
    );
    assert!(matches!(
        s.admit(&wrongly_signed, &binaries),
        Admitted::Unverified(Unverified::Signature(_))
    ));
    assert_eq!(
        holdings.register(&mut s.catalogue, &s.platform, "polygon", RAMP, "USDC"),
        Err(RegisterError::NotRunning(RAMP.to_owned()))
    );

    // A running wallet at a node the operator doesn't list as public.
    run_wallet(&mut s, RAMP, "1.0.0");
    assert_eq!(
        holdings.register(&mut s.catalogue, &s.platform, "unlisted", RAMP, "USDC"),
        Err(RegisterError::Node(NodeError::NotPublic("unlisted".into())))
    );
    for node in ["polygon", "unlisted"] {
        assert!(s.catalogue.nodes().holding(node).is_none(), "{node}");
        assert!(holdings.account(node).is_none(), "{node}");
    }
}

// verifies: REQ-CAT-23
#[test]
fn wallet_answers_in_the_contracts_types_and_checks_a_balance_for_an_order() {
    let mut s = Signed::new();
    run_wallet(&mut s, RESERVE, "1.0.0");

    let ResultBody::Deposit(result) =
        deposit_answer(&mut s, RESERVE, "polygon", "USDC", "o-1", "100")
    else {
        panic!("not a deposit's result");
    };
    let ReserveMovementOutcome::Succeeded(settled) = result.outcome else {
        panic!("the deposit didn't succeed");
    };
    assert_eq!(
        (settled.amount_in.as_str(), settled.amount_out.as_str()),
        ("100", "100")
    );
    assert_eq!(settled.fee.amount, "0");

    assert!(covers(&mut s, RESERVE, "USDC", "o-1", 100));
    assert!(!covers(&mut s, RESERVE, "USDC", "o-1", 101));
    assert!(
        !covers(&mut s, RESERVE, "USDC", "o-2", 1),
        "an order that landed nothing holds nothing"
    );
}

// verifies: REQ-CAT-23, INV-1
#[test]
fn wallet_refuses_another_asset_or_node_and_a_bad_amount_and_holds_nothing_for_them() {
    let mut s = Signed::new();
    run_wallet(&mut s, RESERVE, "1.0.0");
    let mut holdings = Holdings::default();
    holdings
        .register(&mut s.catalogue, &s.platform, "polygon", RESERVE, "USDC")
        .expect("registered");
    holdings
        .land(
            &mut s.platform,
            &TestOperator,
            "polygon",
            "USDC",
            "o-1",
            "100",
        )
        .expect("lands");

    // The host binds the account to its asset and node before it asks the wallet anything.
    let sent = s.platform.operations_received(RESERVE);
    assert_eq!(
        holdings.land(
            &mut s.platform,
            &TestOperator,
            "polygon",
            "EURC",
            "o-1",
            "5"
        ),
        Err(WalletError::Holding(HoldingError::WrongAsset {
            expected: "USDC".into(),
            found: "EURC".into()
        }))
    );
    for bad in [
        "0",
        "-5",
        "05",
        "1.5",
        "",
        "abc",
        "340282366920938463463374607431768211456",
    ] {
        assert!(
            matches!(
                holdings.land(
                    &mut s.platform,
                    &TestOperator,
                    "polygon",
                    "USDC",
                    "o-1",
                    bad
                ),
                Err(WalletError::Holding(
                    HoldingError::BadAmount(_) | HoldingError::ZeroAmount
                ))
            ),
            "{bad:?} was taken"
        );
    }
    assert_eq!(
        s.platform.operations_received(RESERVE),
        sent,
        "nothing was asked of the wallet"
    );

    // The wallet binds itself too: asked directly, it refuses another asset and another node.
    for (node, asset) in [("polygon", "EURC"), ("ethereum", "USDC")] {
        let ResultBody::Deposit(result) = deposit_answer(&mut s, RESERVE, node, asset, "o-9", "5")
        else {
            panic!("not a deposit's result");
        };
        assert!(
            matches!(result.outcome, ReserveMovementOutcome::Refused(_)),
            "{node} {asset}"
        );
    }
    let reading = holdings
        .account("polygon")
        .expect("account")
        .reading()
        .expect("reading");
    assert_eq!((reading.balance, reading.held.len()), (100, 1));

    // Nor does it release what an order doesn't hold, asked directly: the order holds 100, and 101 is refused, as
    // is any amount for an order that holds nothing.
    use rutent_prov::holding::wallet::Operator as _;
    for (order, amount) in [("o-1", "101"), ("o-9", "1")] {
        let statement = TestOperator.statement(operation(json!({
            "kind": "withdrawal",
            "transaction": format!("tx-out-{order}"),
            "provider": RESERVE,
            "origin": { "network": RESERVE },
            "destination": "polygon",
            "asset": "USDC",
            "amount": amount,
            "dry_run": false,
            "inputs": { "order": order },
        })));
        let Handled::Answered(result) = s.platform.handle(RESERVE, &statement) else {
            panic!("not answered");
        };
        let ResultBody::Withdrawal(result) = result.statement else {
            panic!("not a withdrawal's result");
        };
        assert!(
            matches!(result.outcome, ReserveMovementOutcome::Refused(_)),
            "{order}"
        );
    }
    // The refusals changed nothing in the wallet: what o-1 landed can still all be released.
    holdings
        .release(
            &mut s.platform,
            &TestOperator,
            "polygon",
            "USDC",
            "o-1",
            "100",
        )
        .expect("o-1 releases all it landed, so the wallet's own record still holds it");
}

// verifies: REQ-CAT-23, INV-1
#[test]
fn release_above_what_an_order_holds_is_refused_and_changes_nothing() {
    let mut s = Signed::new();
    run_wallet(&mut s, RESERVE, "1.0.0");
    let mut holdings = Holdings::default();
    holdings
        .register(&mut s.catalogue, &s.platform, "polygon", RESERVE, "USDC")
        .expect("registered");
    holdings
        .land(
            &mut s.platform,
            &TestOperator,
            "polygon",
            "USDC",
            "o-1",
            "100",
        )
        .expect("lands");

    assert!(matches!(
        holdings.release(
            &mut s.platform,
            &TestOperator,
            "polygon",
            "USDC",
            "o-1",
            "101"
        ),
        Err(WalletError::Holding(HoldingError::ReleaseExceedsHeld {
            held: 100,
            asked: 101,
            ..
        }))
    ));
    assert!(
        holdings
            .release(
                &mut s.platform,
                &TestOperator,
                "polygon",
                "USDC",
                "o-2",
                "1"
            )
            .is_err()
    );
    let reading = holdings
        .account("polygon")
        .expect("account")
        .reading()
        .expect("reading");
    assert_eq!(reading.balance, 100);

    // A release within what the order holds goes through, and leaves the rest.
    holdings
        .release(
            &mut s.platform,
            &TestOperator,
            "polygon",
            "USDC",
            "o-1",
            "40",
        )
        .expect("releases part");
    let reading = holdings
        .account("polygon")
        .expect("account")
        .reading()
        .expect("reading");
    assert_eq!((reading.balance, reading.held[0].amount), (60, 60));
}

// verifies: INV-5
#[test]
fn value_between_segments_rests_only_in_a_nodes_holding_account() {
    // Two public nodes with a wallet each, a third public node with none, and a fourth the operator doesn't list.
    let mut s = Signed::new();
    s.catalogue
        .add_public_node("ethereum")
        .expect("a public node");
    s.catalogue
        .add_public_node("stellar")
        .expect("a public node");
    run_wallet(&mut s, RESERVE, "1.0.0");
    run_wallet(&mut s, RAMP, "1.0.0");
    let mut holdings = Holdings::default();
    holdings
        .register(&mut s.catalogue, &s.platform, "polygon", RESERVE, "USDC")
        .expect("registered");
    holdings
        .register(&mut s.catalogue, &s.platform, "ethereum", RAMP, "USDC")
        .expect("registered");

    let nodes = ["polygon", "ethereum", "stellar", "unlisted"];
    let orders = ["o-1", "o-2", "o-3", "o-4", "o-5"];
    // The model: what each registered node's wallet holds for each order, and what the cases moved in and out. The
    // run is one evolving history, as value at a node is: each case is one step of it, drawn from its seed.
    let mut model: std::collections::BTreeMap<(&str, &str), u128> = Default::default();
    let (mut landed, mut released) = (0u128, 0u128);
    let (mut refused_landings, mut accepted_landings) = (0u64, 0u64);

    for case in 0..CASES {
        let mut rng = Rng::new(case);
        let node = nodes[rng.below(nodes.len() as u64) as usize];
        let order = orders[rng.below(orders.len() as u64) as usize];
        let amount = 1 + rng.below(1_000) as u128;
        let registered = matches!(node, "polygon" | "ethereum");
        let before = holdings.readings();

        match rng.below(4) {
            // Value lands at a node.
            0 | 1 => {
                let outcome = holdings.land(
                    &mut s.platform,
                    &TestOperator,
                    node,
                    "USDC",
                    order,
                    &amount.to_string(),
                );
                if registered {
                    outcome.unwrap_or_else(|e| panic!("case {case}: a landing was refused: {e:?}"));
                    *model.entry((node, order)).or_default() += amount;
                    landed += amount;
                    accepted_landings += 1;
                } else {
                    // A node with no holding account takes no value (`REQ-CAT-23`): there is nowhere for it to rest.
                    assert_eq!(
                        outcome,
                        Err(WalletError::NoHoldingAccount(node.to_owned())),
                        "case {case}: value landed at {node}"
                    );
                    assert_eq!(
                        holdings.readings(),
                        before,
                        "case {case}: a refusal changed a record"
                    );
                    refused_landings += 1;
                }
            }
            // Value leaves for an order, up to what it holds there (or a little more).
            2 => {
                let held = model.get(&(node, order)).copied().unwrap_or(0);
                let ask = if rng.chance(3) {
                    held + 1
                } else {
                    held.min(amount)
                };
                let outcome = holdings.release(
                    &mut s.platform,
                    &TestOperator,
                    node,
                    "USDC",
                    order,
                    &ask.to_string(),
                );
                if ask > 0 && ask <= held {
                    outcome.unwrap_or_else(|e| panic!("case {case}: a release was refused: {e:?}"));
                    let left = held - ask;
                    if left == 0 {
                        model.remove(&(node, order));
                    } else {
                        model.insert((node, order), left);
                    }
                    released += ask;
                } else {
                    assert!(
                        outcome.is_err(),
                        "case {case}: {ask} left an order holding {held}"
                    );
                    assert_eq!(
                        holdings.readings(),
                        before,
                        "case {case}: a refusal changed a record"
                    );
                }
            }
            // Another asset lands, or a landing names a node it doesn't belong to: refused, nothing changes.
            _ => {
                let outcome = holdings.land(
                    &mut s.platform,
                    &TestOperator,
                    node,
                    "EURC",
                    order,
                    &amount.to_string(),
                );
                assert!(
                    outcome.is_err(),
                    "case {case}: another asset landed at {node}"
                );
                assert_eq!(
                    holdings.readings(),
                    before,
                    "case {case}: a refusal changed a record"
                );
            }
        }

        // The wallet's own word, asked of the wallet and not read from the host's record: the order this step
        // touched at a registered node holds what the model says, and not one unit more.
        if registered {
            let provider = if node == "polygon" { RESERVE } else { RAMP };
            let model_held = model.get(&(node, order)).copied().unwrap_or(0);
            assert!(
                covers(&mut s, provider, "USDC", order, model_held),
                "case {case}: the wallet at {node} doesn't cover {model_held} for {order}"
            );
            assert!(
                !covers(&mut s, provider, "USDC", order, model_held + 1),
                "case {case}: the wallet at {node} covers more than {model_held} for {order}"
            );
        }

        // After every step: each registered account holds exactly what the model says, per order, its balance is
        // the sum, and so every unit that landed and hasn't left rests in a holding account. rutent's own position,
        // what came in less what left less what rests in the accounts, is empty.
        let readings = holdings.readings();
        let mut resting = 0u128;
        for reading in &readings {
            assert_eq!(
                reconcile(reading),
                Ok(Reconciliation::Matches),
                "case {case}"
            );
            for held in &reading.held {
                assert_eq!(
                    model.get(&(reading.node.as_str(), held.order.as_str())),
                    Some(&held.amount),
                    "case {case}: {} holds a different amount for {}",
                    reading.node,
                    held.order
                );
            }
            let expected: u128 = model
                .iter()
                .filter(|((n, _), _)| *n == reading.node)
                .map(|(_, a)| a)
                .sum();
            assert_eq!(reading.balance, expected, "case {case}: {}", reading.node);
            resting += reading.balance;
        }
        assert!(
            readings.len() == 2,
            "case {case}: only the registered nodes have accounts"
        );
        assert_eq!(
            landed - released - resting,
            0,
            "case {case}: rutent's own position isn't empty"
        );
    }
    // The refusal branch and the landing branch were both drawn.
    assert!(accepted_landings > 50, "only {accepted_landings} landings");
    assert!(
        refused_landings > 20,
        "only {refused_landings} refused landings"
    );
}
