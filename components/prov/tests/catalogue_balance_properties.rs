//! Property tests of balance checks and segment effects (`DEC-18`): seeded, so a failure names the seed that
//! replays it.

mod common;

use common::*;
use rutent_contracts::provider_role::{Operation, ResultBody};
use rutent_prov::catalogue::{
    Catalogue, CheckBound, CheckRefusal, Instant, SegmentEffect, Verdict, segment_effect,
};
use serde_json::{Value, json};

const RESERVE: &str = "polygon-wallet-reserve";

fn op(value: Value) -> Operation {
    serde_json::from_value(value).expect("operation parses")
}

fn result(value: Value) -> ResultBody {
    serde_json::from_value(value).expect("result parses")
}

fn reserve_world(max: u32, window: u64) -> Catalogue {
    let mut catalogue = acme_catalogue(&[reserve_advertisement()]);
    catalogue.set_check_bound(CheckBound {
        max_counted: max,
        window_secs: window,
    });
    catalogue.set_answer_within(10);
    catalogue.open_session("sess", "acct", Instant(u64::MAX));
    catalogue
}

fn balance_op(tx: &str, amount: u64) -> Operation {
    op(json!({
        "kind": "balance_verification", "transaction": tx, "provider": RESERVE,
        "session": "sess", "account": "acct", "asset": "USDC", "amount": amount.to_string(),
    }))
}

fn dry_withdrawal(tx: &str, amount: u64) -> Operation {
    op(json!({
        "kind": "withdrawal", "transaction": tx, "provider": RESERVE, "session": "sess",
        "origin": {"network": "polygon"}, "destination": "acme-ledger",
        "asset": "USDC", "amount": amount.to_string(), "dry_run": true, "inputs": {},
    }))
}

/// A reserve with a balance answers `amount` yes or no, and a dry run of the same amount is answered with figures
/// that differ in every case. Only the yes or no, and for a refusal its class, may reach the asker (`INV-9`).
// verifies: INV-9, AC-15
#[test]
fn every_balance_answer_reveals_only_whether_funds_suffice() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut catalogue = reserve_world(1_000, 1_000_000);
        let amount = 1 + rng.below(1_000_000);

        // Two reserves with different balances, both on the same side of the amount, answer alike.
        let mut verdicts = Vec::new();
        for (i, balance) in [rng.below(2_000_000), rng.below(2_000_000)]
            .into_iter()
            .enumerate()
        {
            let sufficient = balance >= amount;
            let tx = format!("t{i}");
            let ticket = catalogue
                .begin_check(&balance_op(&tx, amount), None, Instant(2_100 + i as u64))
                .unwrap();
            let answer = result(json!({
                "kind": "balance_verification", "transaction": tx,
                "outcome": {"status": "answered", "sufficient": sufficient},
            }));
            let verdict = catalogue.answer_check(&ticket, &answer, Instant(2_101 + i as u64));
            assert_eq!(
                verdict,
                if sufficient {
                    Verdict::Yes
                } else {
                    Verdict::No
                },
                "seed {seed}"
            );
            verdicts.push((sufficient, format!("{verdict:?}")));
        }
        if verdicts[0].0 == verdicts[1].0 {
            assert_eq!(verdicts[0].1, verdicts[1].1, "seed {seed}");
        }

        // A dry run reveals no more: accepted answers with any figures give the same yes; refusals of every
        // class and lift time give no timing and no figure.
        let mut accepted = Vec::new();
        for i in 0..3u64 {
            let tx = format!("d{i}");
            let ticket = catalogue
                .begin_check(&dry_withdrawal(&tx, amount), None, Instant(2_200 + i))
                .unwrap();
            let r = result(json!({
                "kind": "withdrawal", "transaction": tx,
                "outcome": {"status": "dry_run_accepted", "amount_in": rng.below(9_999_999).to_string(),
                    "amount_out": rng.below(9_999_999).to_string(),
                    "fee": {"asset": "USDC", "amount": rng.below(1_000).to_string()},
                    "valid_until": format!("2026-11-06T15:{:02}:05Z", rng.below(60)),
                    "voided_by": ["validity_passes"], "settlement_seconds": rng.below(100_000)},
            }));
            accepted.push(format!(
                "{:?}",
                catalogue.answer_check(&ticket, &r, Instant(2_201 + i))
            ));
        }
        assert!(accepted.iter().all(|v| v == "Yes"), "seed {seed}");

        let mut lifted = Vec::new();
        for i in 0..3u64 {
            let tx = format!("r{i}");
            let ticket = catalogue
                .begin_check(&dry_withdrawal(&tx, amount), None, Instant(2_300 + i))
                .unwrap();
            let r = result(json!({
                "kind": "withdrawal", "transaction": tx,
                "outcome": {"status": "refused", "refusal": {"class": "lifts_at_known_time",
                    "lifts_at": format!("2026-1{}-0{}T10:00:00Z", rng.below(3), 1 + rng.below(9))}},
            }));
            lifted.push(format!(
                "{:?}",
                catalogue.answer_check(&ticket, &r, Instant(2_301 + i))
            ));
        }
        assert!(lifted.iter().all(|v| v == &lifted[0]), "seed {seed}");
        assert!(!lifted[0].contains("2026"), "seed {seed}");
    }
}

/// A model that knows only the rule: a counted check is refused when `max` counted checks fall inside the window
/// that ends at the check's time.
// verifies: REQ-CAT-3, AC-15
#[test]
fn counted_checks_in_a_window_never_exceed_the_bound() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let max = 1 + rng.below(5) as u32;
        let window = 1 + rng.below(30);
        let mut catalogue = reserve_world(max, window);
        let mut admitted: Vec<u64> = Vec::new();
        let mut now = 2_100u64;
        for step in 0..60 {
            now += rng.below(8);
            let own_order = rng.chance(4);
            let outcome = catalogue.begin_check(
                &balance_op(&format!("t{step}"), 1 + rng.below(1_000)),
                own_order.then_some("order-1"),
                Instant(now),
            );
            let in_window = admitted.iter().filter(|t| **t + window > now).count() as u32;
            match outcome {
                Ok(_) => {
                    assert!(own_order || in_window < max, "seed {seed}, step {step}");
                    if !own_order {
                        admitted.push(now);
                    }
                }
                Err(e) => {
                    assert_eq!(e, CheckRefusal::OverBound, "seed {seed}, step {step}");
                    assert!(!own_order && in_window >= max, "seed {seed}, step {step}");
                }
            }
            assert!(
                catalogue.counted_checks("acct", Instant(now)) <= max,
                "seed {seed}, step {step}"
            );
        }
    }
}

/// An origin holds a balance. Whatever way a segment fails, is refused, or is only asked about, nothing is taken
/// from it: the balance after is the balance before, whatever the segment's kind or direction.
// verifies: INV-2
#[test]
fn failed_segments_result_leaves_its_value_at_its_origin_in_full() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let amount = (1 + rng.below(1_000_000)).to_string();
        let direction = if rng.chance(2) {
            "exact_in"
        } else {
            "exact_out"
        };
        let dry = rng.chance(3);
        let (operation, kind, node, asset) = match rng.below(4) {
            0 => (
                json!({"kind": "withdrawal", "transaction": "t", "provider": "p",
                    "origin": {"network": "polygon"}, "destination": "n", "asset": "USDC",
                    "amount": amount, "dry_run": dry, "inputs": {}}),
                "withdrawal",
                rutent_prov::catalogue::Endpoint::Outer("polygon".into()),
                "USDC",
            ),
            1 => (
                json!({"kind": "deposit", "transaction": "t", "provider": "p", "origin": "n",
                    "destination": {"network": "polygon"}, "asset": "USDC", "amount": amount,
                    "dry_run": dry, "inputs": {}}),
                "deposit",
                rutent_prov::catalogue::Endpoint::Inner("n".into()),
                "USDC",
            ),
            2 => (
                json!({"kind": "transfer", "transaction": "t", "provider": "p", "origin": "a",
                    "destination": "b", "asset": "EURC", "direction": direction, "amount": amount,
                    "dry_run": dry, "inputs": {}}),
                "transfer",
                rutent_prov::catalogue::Endpoint::Inner("a".into()),
                "EURC",
            ),
            _ => (
                json!({"kind": "conversion", "transaction": "t", "provider": "p", "node": "n",
                    "from": "COP", "to": "USDC", "direction": direction, "amount": amount,
                    "dry_run": dry, "inputs": {}}),
                "conversion",
                rutent_prov::catalogue::Endpoint::Inner("n".into()),
                "COP",
            ),
        };
        let evidence = json!([{"grade": 1, "reference": "0x1", "by": "polygon"}]);
        let outcome = match rng.below(4) {
            0 => json!({"status": "failed", "evidence": evidence}),
            1 => json!({"status": "refused", "refusal": {"class": "final"}}),
            2 => json!({"status": "refused", "refusal": {"class": "waits_on_person"}}),
            _ => json!({"status": "refused", "refusal": {"class": "lifts_at_known_time",
                    "lifts_at": "2026-11-07T10:00:00Z"}}),
        };
        let op = op_from(operation);
        let res = result(json!({"kind": kind, "transaction": "t", "outcome": outcome}));

        let balance_before = rng.below(10_000_000);
        let effect = segment_effect(&op, &res).unwrap();
        let taken: u64 = match &effect {
            SegmentEffect::NothingTaken { node: n, asset: a } => {
                assert_eq!((n, a.as_str()), (&node, asset), "seed {seed}");
                0
            }
            other => panic!("seed {seed}: {other:?}"),
        };
        assert_eq!(balance_before - taken, balance_before, "seed {seed}");
    }
}

fn op_from(value: Value) -> Operation {
    op(value)
}
