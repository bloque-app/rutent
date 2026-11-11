//! A route handed on (`T-016-03`; `REQ-ORD-40`, `INV-5`): it starts at an inner node where value rests, the node's
//! holding account counts as its starting reserve, and the account's own record proves the withdrawal.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;
#[path = "holding_support/mod.rs"]
mod support;

use rutent_contracts::provider_role::{Denomination, NodeRef, OuterNode};
use rutent_contracts::provider_role::{EvidenceGrade, EvidencePiece};
use rutent_contracts::state_machine::{Endpoint, Id, OperationKind, SegmentPlan};
use rutent_prov::holding::wallet::{Holdings, StartError, proof_of_withdrawal};
use rutent_prov::holding::{Reconciliation, reconcile};
use signed::Signed;
use support::{POLYGON_WALLET, TestOperator, covers, run_wallet};

use signed::catalogue_support::{CASES, Rng};

fn endpoint(node: NodeRef, asset: &str) -> Endpoint {
    Endpoint {
        node,
        asset: Denomination {
            id: asset.into(),
            decimals: 6,
        },
    }
}

fn inner(name: &str) -> NodeRef {
    NodeRef::Inner(name.into())
}

fn outer(network: &str) -> NodeRef {
    NodeRef::Outer(OuterNode {
        network: network.into(),
    })
}

fn segment(id: &str, kind: OperationKind, from: NodeRef, to: NodeRef) -> SegmentPlan {
    SegmentPlan {
        segment_id: Id::new(id).unwrap(),
        provider: Id::new("some-router").unwrap(),
        kind,
        from: endpoint(from, "USDC"),
        to: endpoint(to, "USDC"),
    }
}

/// A route handed on from `node`: a route to another inner node, then a deposit out.
fn handed_on_from(node: &str) -> Vec<SegmentPlan> {
    vec![
        segment("s1", OperationKind::Route, inner(node), inner("hub")),
        segment("s2", OperationKind::Deposit, inner("hub"), outer("bank")),
    ]
}

/// A wallet registered at `polygon`, with `100` of `USDC` landed for `o-1`.
fn polygon_with_value() -> (Signed, Holdings) {
    let mut s = Signed::new();
    run_wallet(&mut s, POLYGON_WALLET, "1.0.0");
    let mut holdings = Holdings::default();
    holdings
        .register(
            &mut s.catalogue,
            &s.platform,
            "polygon",
            POLYGON_WALLET,
            "USDC",
        )
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
        .expect("the wallet takes the value");
    (s, holdings)
}

// verifies: REQ-ORD-40, INV-5
#[test]
fn handed_on_routes_withdrawal_is_proven_by_the_holding_accounts_record() {
    let (mut s, mut holdings) = polygon_with_value();
    let start = holdings
        .start_handed_on(
            &mut s.platform,
            &TestOperator,
            &handed_on_from("polygon"),
            "o-1",
            "60",
        )
        .expect("the route starts");

    // The holding account is the route's starting reserve, and its own record is the proof of the withdrawal.
    assert_eq!(start.reserve, POLYGON_WALLET);
    assert_eq!(start.node, "polygon");
    assert_eq!(start.first_segment, Id::new("s1").unwrap());
    assert_eq!(start.record.amount, 60);
    assert_eq!(start.record.by, POLYGON_WALLET);
    assert!(
        start.record.reference.starts_with(POLYGON_WALLET)
            && start.record.reference.contains("withdrawal"),
        "the proof is the wallet's own record of a withdrawal: {}",
        start.record.reference
    );

    // The value left the account for the route: the order holds what is left, by the wallet's own word too.
    assert!(covers(&mut s, POLYGON_WALLET, "USDC", "o-1", 40));
    assert!(!covers(&mut s, POLYGON_WALLET, "USDC", "o-1", 41));
    let readings = holdings.readings(&mut s.platform, &TestOperator);
    let reading = readings[0].1.as_ref().expect("a reading");
    assert_eq!(reading.balance, 40);
    assert_eq!(reconcile(reading), Ok(Reconciliation::Matches));
}

// verifies: REQ-ORD-40
#[test]
fn route_that_is_not_handed_on_or_not_at_a_holding_account_does_not_start() {
    let (mut s, mut holdings) = polygon_with_value();
    let before = holdings.attributions();

    let from_outer = vec![
        segment("s1", OperationKind::Withdrawal, outer("bank"), inner("hub")),
        segment("s2", OperationKind::Deposit, inner("hub"), outer("bank")),
    ];
    let refused = [
        (from_outer, "o-1", "10", StartError::NotHandedOn),
        (Vec::new(), "o-1", "10", StartError::NotHandedOn),
        (
            handed_on_from("ethereum"),
            "o-1",
            "10",
            StartError::ValueNotAtStart("ethereum".to_owned()),
        ),
    ];
    for (route, order, amount, expected) in refused {
        let outcome =
            holdings.start_handed_on(&mut s.platform, &TestOperator, &route, order, amount);
        assert_eq!(outcome.err(), Some(expected));
    }

    // More than the order holds, an order that holds nothing, another asset: refused, and nothing moved.
    for (order, amount) in [("o-1", "101"), ("o-2", "1"), ("o-1", "0")] {
        let outcome = holdings.start_handed_on(
            &mut s.platform,
            &TestOperator,
            &handed_on_from("polygon"),
            order,
            amount,
        );
        assert!(matches!(outcome, Err(StartError::Wallet(_))), "{outcome:?}");
    }
    let mut other_asset = handed_on_from("polygon");
    other_asset[0].from = endpoint(inner("polygon"), "EURC");
    let outcome =
        holdings.start_handed_on(&mut s.platform, &TestOperator, &other_asset, "o-1", "10");
    assert_eq!(
        outcome.err(),
        Some(StartError::ValueNotAtStart("polygon".to_owned()))
    );

    assert_eq!(holdings.attributions(), before);
    assert!(covers(&mut s, POLYGON_WALLET, "USDC", "o-1", 100));
}

// verifies: INV-5
#[test]
fn value_handed_on_leaves_only_by_the_holding_account_and_rutent_keeps_none() {
    let (mut s, mut holdings) = polygon_with_value();
    let orders = ["o-1", "o-2", "o-3"];
    let (mut landed, mut released, mut started) = (100u128, 0u128, 0u64);
    let mut held: std::collections::BTreeMap<&str, u128> = [("o-1", 100)].into();

    for case in 0..CASES {
        let mut rng = Rng::new(case);
        let order = orders[rng.below(orders.len() as u64) as usize];
        let amount = 1 + rng.below(500) as u128;
        let have = held.get(order).copied().unwrap_or(0);
        if rng.chance(2) {
            holdings
                .land(
                    &mut s.platform,
                    &TestOperator,
                    "polygon",
                    "USDC",
                    order,
                    &amount.to_string(),
                )
                .expect("a landing");
            *held.entry(order).or_default() += amount;
            landed += amount;
        } else {
            let outcome = holdings.start_handed_on(
                &mut s.platform,
                &TestOperator,
                &handed_on_from("polygon"),
                order,
                &amount.to_string(),
            );
            if amount <= have {
                let start = outcome.unwrap_or_else(|e| panic!("case {case}: {e:?}"));
                assert_eq!(start.record.amount, amount, "case {case}");
                held.insert(order, have - amount);
                released += amount;
                started += 1;
            } else {
                assert!(outcome.is_err(), "case {case}: {amount} left {have}");
            }
        }
        // After each step the account's balance, by the wallet's own word, is everything that landed and hasn't
        // been handed on: rutent's own position is empty.
        let readings = holdings.readings(&mut s.platform, &TestOperator);
        let reading = readings[0].1.as_ref().expect("a reading");
        assert_eq!(
            reconcile(reading),
            Ok(Reconciliation::Matches),
            "case {case}"
        );
        assert_eq!(landed - released - reading.balance, 0, "case {case}");
    }
    assert!(started > 50, "only {started} routes started");
}

fn piece(by: &str, reference: &str) -> EvidencePiece {
    EvidencePiece {
        grade: EvidenceGrade::SignedByProvider,
        reference: reference.to_owned(),
        by: by.to_owned(),
    }
}

// verifies: REQ-ORD-40, INV-5
#[test]
fn withdrawal_without_the_wallets_own_record_of_that_transaction_is_not_proven() {
    let ours = "w-withdrawal-7";
    let good = format!("w-withdrawal-{ours}");
    // The wallet's own piece for this very withdrawal proves it.
    assert_eq!(
        proof_of_withdrawal("w", ours, &[piece("w", &good)]),
        Ok(good.clone())
    );
    // No piece at all, a piece by another party, another transaction's (stale) reference of the same wallet, and
    // one of the same wallet's withdrawals that merely shares the prefix: none is a record of this withdrawal.
    let cases: [Vec<EvidencePiece>; 4] = [
        vec![],
        vec![piece("other", &good)],
        vec![piece("w", "w-withdrawal-w-withdrawal-6")],
        vec![piece("w", &format!("{good}0"))],
    ];
    for evidence in cases {
        assert_eq!(
            proof_of_withdrawal("w", ours, &evidence),
            Err(StartError::NoRecord),
            "{evidence:?}"
        );
    }
}
