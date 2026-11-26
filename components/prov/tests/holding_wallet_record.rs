//! The plain wallet's record, run natively (`T-016-05`; `INV-1`, `REQ-CAT-23`, `REQ-ORD-43`): the guest's own
//! `wallet.rs`, compiled into the test, so a landing the platform's record never refuses first (it holds one per
//! transaction, and answers a repeat itself) is still the wallet's to judge. The Wasm build of the same file runs in
//! `holding_wallet.rs`.

#[path = "../guests/plain-wallet/src/wallet.rs"]
#[allow(dead_code)]
mod wallet;

use rutent_contracts::provider_role::{
    BalanceOutcome, HoldingBalanceOutcome, Operation, ReserveMovementOutcome, ResultBody,
};
use serde_json::json;
use wallet::Wallet;

const ME: &str = "plain-wallet-polygon";

fn operation(value: serde_json::Value) -> Operation {
    serde_json::from_value(value).expect("a valid operation")
}

fn outcome(body: ResultBody) -> ReserveMovementOutcome {
    match body {
        ResultBody::Deposit(r) | ResultBody::Withdrawal(r) => r.outcome,
        other => panic!("not a movement's result: {other:?}"),
    }
}

fn land(w: &mut Wallet, transaction: &str, order: &str, amount: &str) -> ReserveMovementOutcome {
    let Operation::Deposit(d) = operation(json!({
        "kind": "deposit", "transaction": transaction, "provider": ME, "origin": "polygon",
        "destination": { "network": ME }, "asset": "USDC", "amount": amount, "dry_run": false,
        "inputs": { "order": order },
    })) else {
        unreachable!()
    };
    outcome(w.deposit(&d))
}

fn release(w: &mut Wallet, transaction: &str, order: &str, amount: &str) -> ReserveMovementOutcome {
    let Operation::Withdrawal(d) = operation(json!({
        "kind": "withdrawal", "transaction": transaction, "provider": ME,
        "origin": { "network": ME }, "destination": "polygon", "asset": "USDC", "amount": amount,
        "dry_run": false, "inputs": { "order": order },
    })) else {
        unreachable!()
    };
    outcome(w.withdrawal(&d))
}

/// Whether the wallet says the order holds at least `amount`.
fn holds(w: &Wallet, order: &str, amount: u128) -> bool {
    let Operation::BalanceVerification(v) = operation(json!({
        "kind": "balance_verification", "transaction": "check", "provider": ME, "session": "s-1",
        "account": order, "asset": "USDC", "amount": amount.to_string(),
    })) else {
        unreachable!()
    };
    let ResultBody::BalanceVerification(r) = w.verification(&v) else {
        panic!("not a verification's result");
    };
    let BalanceOutcome::Answered(a) = r.outcome else {
        panic!("refused");
    };
    a.sufficient
}

/// What the wallet says its account holds.
fn balance(w: &Wallet) -> String {
    let Operation::HoldingBalance(h) = operation(json!({
        "kind": "holding_balance", "transaction": "balance", "provider": ME, "node": "polygon",
        "asset": "USDC",
    })) else {
        unreachable!()
    };
    let ResultBody::HoldingBalance(r) = w.balance(&h) else {
        panic!("not a balance's result");
    };
    let HoldingBalanceOutcome::Answered(a) = r.outcome else {
        panic!("refused");
    };
    a.balance
}

fn succeeded(o: &ReserveMovementOutcome) -> bool {
    matches!(o, ReserveMovementOutcome::Succeeded(_))
}

// verifies: INV-1, REQ-CAT-23
#[test]
fn landing_an_order_the_wallet_already_holds_adds_nothing() {
    let mut w = Wallet::default();
    let first = land(&mut w, "land-1", "o-1", "100");
    assert!(succeeded(&first));

    // The same landing again, as a re-landing from the log may send it: answered done, as before, and nothing is
    // added.
    assert_eq!(land(&mut w, "land-1", "o-1", "100"), first);
    assert_eq!(
        balance(&w),
        "100",
        "a repeat of a landing doubled the value"
    );
    assert!(holds(&w, "o-1", 100) && !holds(&w, "o-1", 101));

    // The repeat holds after the order is released in full: the landing is still the one it was.
    assert!(succeeded(&release(&mut w, "wd-1", "o-1", "100")));
    assert_eq!(land(&mut w, "land-1", "o-1", "100"), first);
    assert_eq!(
        balance(&w),
        "0",
        "a repeat of a spent landing put the value back"
    );

    // A landing under another transaction is another landing, and adds. The same transaction for another order or
    // amount is no repeat: it is refused, and adds nothing.
    assert!(succeeded(&land(&mut w, "land-2", "o-1", "30")));
    assert_eq!(balance(&w), "30");
    for (order, amount) in [("o-2", "30"), ("o-1", "31")] {
        assert!(
            matches!(
                land(&mut w, "land-2", order, amount),
                ReserveMovementOutcome::Refused(_)
            ),
            "{order} {amount}"
        );
    }
    assert_eq!(balance(&w), "30");
}

// verifies: REQ-ORD-43, INV-1
#[test]
fn a_refused_landing_leaves_no_record_that_a_repeat_could_answer() {
    let mut w = Wallet::default();
    // A landing the wallet refuses (a zero amount) is no landing: the transaction is free for a good one.
    assert!(matches!(
        land(&mut w, "land-1", "o-1", "0"),
        ReserveMovementOutcome::Refused(_)
    ));
    assert!(succeeded(&land(&mut w, "land-1", "o-1", "5")));
    assert_eq!(balance(&w), "5");
}

/// Core's question whether a reversed segment's credit came back into the account, in the operator's envelope.
fn reversal_asked(
    segment: &str,
    amount: &str,
) -> rutent_contracts::provider_role::Statement<Operation> {
    serde_json::from_value(json!({
        "deployment": { "name": "rutent-test", "operator_key": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA" },
        "protocol_version": "4.0.0",
        "shape": "urn:rutent:provider-role:operation",
        "maker": "rutent-operator",
        "made_at": "2026-11-25T00:00:00Z",
        "signature": { "scheme": "ed25519", "key": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA",
            "value": "BwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyAhIiMkJSYnKCkqKywtLi8wMTIzNDU2Nzg5Ojs8PT4_QEFCQ0RFRg" },
        "statement": {
            "kind": "reversal_confirmation", "transaction": "ask-1", "provider": ME,
            "segment": segment, "asset": "USDC", "amount": amount,
        },
    }))
    .expect("a reversal confirmation question")
}

/// What the wallet answers about a reversed segment's credit, given the account's statement of it.
fn confirms(w: &Wallet, segment: &str, amount: &str, credit: Option<(&str, &str)>) -> &'static str {
    use rutent_contracts::provider_role::ReversalConfirmationOutcome;
    let asked = reversal_asked(segment, amount);
    let Operation::ReversalConfirmation(c) = &asked.statement else {
        unreachable!()
    };
    let credit = credit.map(|(asset, amount)| wallet::Credit {
        asset: asset.to_owned(),
        amount: amount.to_owned(),
    });
    let ResultBody::ReversalConfirmation(r) = w.confirmation(&asked, c, credit.as_ref()) else {
        panic!("not a confirmation's result");
    };
    assert_eq!(
        r.transaction, "ask-1",
        "it answers the question's own transaction"
    );
    match r.outcome {
        ReversalConfirmationOutcome::Confirmed(confirmed) => {
            let inner = &confirmed.confirmation;
            assert_eq!(inner.maker, ME, "in the account's own name");
            assert_eq!(
                inner.deployment, asked.deployment,
                "in the question's deployment"
            );
            assert_eq!(
                (
                    inner.statement.segment.as_str(),
                    inner.statement.amount.as_str()
                ),
                (segment, amount),
                "of the segment and amount asked"
            );
            "confirmed"
        }
        ReversalConfirmationOutcome::NotConfirmed(_) => "not confirmed",
        ReversalConfirmationOutcome::Refused(_) => "refused",
    }
}

/// The account confirms a reversed segment's credit only where its statement holds that segment's credit in full and
/// in the asset asked (`T-020-57`, `REQ-ORD-3`): a short credit, another asset, no credit or a malformed amount
/// confirms nothing; an account fixed to another asset refuses.
// verifies: REQ-ORD-43, INV-1
#[test]
fn wallet_confirms_a_reversed_segments_credit_only_in_full_from_its_statement() {
    let mut w = Wallet::default();
    assert_eq!(
        confirms(&w, "tx-move", "1000", Some(("USDC", "1000"))),
        "confirmed"
    );
    assert_eq!(
        confirms(&w, "tx-move", "1000", Some(("USDC", "990"))),
        "not confirmed",
        "less came back"
    );
    assert_eq!(
        confirms(&w, "tx-move", "1000", Some(("USDT", "1000"))),
        "not confirmed",
        "another asset"
    );
    assert_eq!(confirms(&w, "tx-move", "1000", None), "not confirmed");
    assert_eq!(confirms(&w, "tx-move", "01000", None), "refused");
    assert_eq!(confirms(&w, "tx-move", "0", None), "refused");
    // Fixed to USDC by a landing, an account asked about another asset refuses.
    assert!(matches!(
        land(&mut w, "land-1", "ord-1", "5"),
        ReserveMovementOutcome::Succeeded(_)
    ));
    let mut asked = reversal_asked("tx-move", "1000");
    if let Operation::ReversalConfirmation(c) = &mut asked.statement {
        c.asset = "USDT".to_owned();
    }
    let Operation::ReversalConfirmation(c) = &asked.statement else {
        unreachable!()
    };
    let ResultBody::ReversalConfirmation(r) = w.confirmation(&asked, c, None) else {
        panic!("not a confirmation's result");
    };
    assert!(matches!(
        r.outcome,
        rutent_contracts::provider_role::ReversalConfirmationOutcome::Refused(_)
    ));
}
