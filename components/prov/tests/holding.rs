//! The holding-account interface and its check (`T-016-01`).

use rutent_prov::holding::{
    BalanceSource, Difference, HeldForOrder, HoldingAccount, HoldingError, HoldingReading,
    OrderLedger, Reconciliation, reconcile,
};

fn reading(balance: u128, held: &[(&str, u128)], source: BalanceSource) -> HoldingReading {
    HoldingReading {
        node: "node-a".into(),
        asset: "USDC".into(),
        balance,
        source,
        held: held
            .iter()
            .map(|(order, amount)| HeldForOrder {
                order: (*order).into(),
                amount: *amount,
            })
            .collect(),
    }
}

// verifies: REQ-AUD-15
#[test]
fn account_whose_balance_matches_the_sum_held_reconciles() {
    let r = reading(150, &[("o-1", 100), ("o-2", 50)], BalanceSource::Module);
    assert_eq!(reconcile(&r), Ok(Reconciliation::Matches));
}

// verifies: REQ-AUD-15
#[test]
fn account_short_of_the_sum_held_is_raised_as_short() {
    let r = reading(120, &[("o-1", 100), ("o-2", 50)], BalanceSource::Module);
    let Ok(Reconciliation::Differs(Difference::Short { by, .. })) = reconcile(&r) else {
        panic!("expected a short difference");
    };
    assert_eq!(by, 30);
}

// verifies: REQ-AUD-15
#[test]
fn account_over_the_sum_held_is_raised_as_over() {
    let r = reading(180, &[("o-1", 100), ("o-2", 50)], BalanceSource::Module);
    let Ok(Reconciliation::Differs(Difference::Over { by, .. })) = reconcile(&r) else {
        panic!("expected an over difference");
    };
    assert_eq!(by, 30);
}

// verifies: REQ-AUD-15
#[test]
fn private_node_balance_is_labelled_as_its_publishers_word() {
    let source = BalanceSource::Publisher("acme-rails".into());
    let r = reading(10, &[("o-1", 40)], source.clone());
    let Ok(Reconciliation::Differs(d)) = reconcile(&r) else {
        panic!("expected a difference");
    };
    assert_eq!(d.source(), &source);
    assert_eq!(d.node(), "node-a");
}

// verifies: REQ-AUD-15
#[test]
fn reconciling_only_detects_and_leaves_the_reading_unchanged() {
    let r = reading(1, &[("o-1", 40)], BalanceSource::Module);
    let before = r.clone();
    let _ = reconcile(&r);
    assert_eq!(r, before);
}

// verifies: REQ-AUD-15
#[test]
fn sum_that_overflows_is_an_error_not_a_match() {
    let r = reading(0, &[("o-1", u128::MAX), ("o-2", 1)], BalanceSource::Module);
    assert_eq!(reconcile(&r), Err(HoldingError::SumOverflow));
}

// verifies: REQ-AUD-15
#[test]
fn two_entries_for_one_order_in_a_reading_are_refused() {
    let r = reading(2, &[("o-1", 1), ("o-1", 1)], BalanceSource::Module);
    assert_eq!(
        reconcile(&r),
        Err(HoldingError::DuplicateOrder("o-1".into()))
    );
}

// verifies: REQ-CAT-23
#[test]
fn ledger_attributes_value_per_order_and_reads_as_a_holding_account() {
    let mut ledger = OrderLedger::new("node-a", "USDC");
    ledger.hold("o-1", 100).unwrap();
    ledger.hold("o-2", 50).unwrap();
    ledger.hold("o-1", 25).unwrap();

    let r = ledger.reading().unwrap();
    assert_eq!(r.node, "node-a");
    assert_eq!(r.balance, 175);
    assert_eq!(ledger.held_for("o-1"), 125);
    assert_eq!(ledger.held_for("o-3"), 0);
    assert_eq!(r.held.len(), 2);
}

// verifies: REQ-CAT-23
#[test]
fn releasing_more_than_an_order_holds_is_refused_and_changes_nothing() {
    let mut ledger = OrderLedger::new("node-a", "USDC");
    ledger.hold("o-1", 10).unwrap();
    assert_eq!(
        ledger.release("o-1", 11),
        Err(HoldingError::ReleaseExceedsHeld {
            order: "o-1".into(),
            held: 10,
            asked: 11
        })
    );
    assert!(ledger.release("o-2", 1).is_err());
    assert_eq!(ledger.held_for("o-1"), 10);
}

// verifies: REQ-CAT-23
#[test]
fn an_order_released_in_full_leaves_no_entry() {
    let mut ledger = OrderLedger::new("node-a", "USDC");
    ledger.hold("o-1", 10).unwrap();
    ledger.release("o-1", 10).unwrap();
    assert!(ledger.reading().unwrap().held.is_empty());
}

// verifies: REQ-CAT-23
#[test]
fn zero_amounts_are_refused() {
    let mut ledger = OrderLedger::new("node-a", "USDC");
    assert_eq!(ledger.hold("o-1", 0), Err(HoldingError::ZeroAmount));
    assert_eq!(ledger.release("o-1", 0), Err(HoldingError::ZeroAmount));
}

// verifies: REQ-CAT-23
#[test]
fn holding_account_can_be_read_through_a_trait_object() {
    let mut ledger = OrderLedger::new("node-a", "USDC");
    ledger.hold("o-1", 7).unwrap();
    let account: &dyn HoldingAccount = &ledger;
    assert_eq!(account.reading().unwrap().balance, 7);
}
