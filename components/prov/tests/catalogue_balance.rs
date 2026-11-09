//! Balance checks and eligibility (`T-012-04`): a yes or no inside the holder's session, bounded and logged per
//! account, anything revealing sufficiency counted, refusal classes, dry runs that change nothing, a segment
//! without a true undo declined, and a silent reserve marked ineligible with an alert.

mod common;

use common::*;
use rutent_contracts::provider_role::{Operation, ResultBody};
use rutent_prov::catalogue::{
    Catalogue, CheckBound, CheckRefusal, DryRunAssessment, DryRunError, EffectError, Eligibility,
    Instant, RefusalClass, SegmentDecline, SegmentEffect, TupleKey, UndoVerdict, Unusable, Verdict,
    judge_reversal, reveals_sufficiency, segment_effect, tuples_of,
};
use serde_json::{Value, json};

const T0: Instant = Instant(2_000);
const RESERVE: &str = "polygon-wallet-reserve";

fn op(value: Value) -> Operation {
    serde_json::from_value(value).expect("operation parses")
}

fn result(value: Value) -> ResultBody {
    serde_json::from_value(value).expect("result parses")
}

fn balance_op(tx: &str, session: &str, account: &str, amount: &str) -> Operation {
    op(json!({
        "kind": "balance_verification", "transaction": tx, "provider": RESERVE,
        "session": session, "account": account, "asset": "USDC", "amount": amount,
    }))
}

fn dry_withdrawal(tx: &str, session: Option<&str>) -> Operation {
    let mut value = json!({
        "kind": "withdrawal", "transaction": tx, "provider": RESERVE,
        "origin": {"network": "polygon"}, "destination": "acme-ledger",
        "asset": "USDC", "amount": "100", "dry_run": true, "inputs": {},
    });
    if let Some(session) = session {
        value["session"] = json!(session);
    }
    op(value)
}

fn answer(tx: &str, sufficient: bool) -> ResultBody {
    result(json!({
        "kind": "balance_verification", "transaction": tx,
        "outcome": {"status": "answered", "sufficient": sufficient},
    }))
}

/// A catalogue with the reserve, a session for `acct-1`, a bound of three checks a minute and an answer time of
/// ten seconds.
fn world() -> Catalogue {
    let mut catalogue = acme_catalogue(&[reserve_advertisement()]);
    catalogue.set_check_bound(CheckBound {
        max_counted: 3,
        window_secs: 60,
    });
    catalogue.set_answer_within(10);
    catalogue.open_session("sess-1", "acct-1", Instant(10_000));
    catalogue
}

// verifies: REQ-CAT-3
#[test]
fn balance_check_outside_the_holders_session_is_refused() {
    let mut catalogue = world();
    catalogue.open_session("sess-2", "acct-2", Instant(10_000));
    catalogue.open_session("sess-old", "acct-1", Instant(1_000));

    let unknown = catalogue.begin_check(&balance_op("t1", "nobody", "acct-1", "50"), None, T0);
    let other_account =
        catalogue.begin_check(&balance_op("t2", "sess-2", "acct-1", "50"), None, T0);
    let expired = catalogue.begin_check(&balance_op("t3", "sess-old", "acct-1", "50"), None, T0);
    let no_session_on_dry_run = catalogue.begin_check(&dry_withdrawal("t4", None), None, T0);

    assert_eq!(unknown.unwrap_err(), CheckRefusal::NoSession);
    assert_eq!(
        other_account.unwrap_err(),
        CheckRefusal::SessionNotForAccount
    );
    assert_eq!(expired.unwrap_err(), CheckRefusal::NoSession);
    assert_eq!(no_session_on_dry_run.unwrap_err(), CheckRefusal::NoSession);
    // A refused check is logged but never counted, and nothing was asked of the reserve.
    assert_eq!(catalogue.counted_checks("acct-1", T0), 0);
    assert!(!catalogue.check_log("acct-1").is_empty());
    assert_eq!(catalogue.pending_checks(), 0);
}

// verifies: REQ-CAT-3, REQ-CAT-19
#[test]
fn balance_check_is_refused_until_the_operator_sets_a_bound_and_an_answer_time() {
    let mut no_bound = acme_catalogue(&[reserve_advertisement()]);
    no_bound.set_answer_within(10);
    no_bound.open_session("sess-1", "acct-1", Instant(10_000));
    let mut no_time = acme_catalogue(&[reserve_advertisement()]);
    no_time.set_check_bound(CheckBound {
        max_counted: 3,
        window_secs: 60,
    });
    no_time.open_session("sess-1", "acct-1", Instant(10_000));

    let o = balance_op("t1", "sess-1", "acct-1", "50");
    assert_eq!(
        no_bound.begin_check(&o, None, T0).unwrap_err(),
        CheckRefusal::NoBoundSet
    );
    assert_eq!(
        no_time.begin_check(&o, None, T0).unwrap_err(),
        CheckRefusal::NoAnswerTimeSet
    );
}

// verifies: REQ-CAT-3, AC-15
#[test]
fn checks_are_bounded_logged_and_counted_per_account() {
    let mut catalogue = world();
    catalogue.open_session("sess-2", "acct-2", Instant(10_000));
    for i in 0..3 {
        let o = balance_op(&format!("t{i}"), "sess-1", "acct-1", "50");
        catalogue.begin_check(&o, None, Instant(2_000 + i)).unwrap();
    }

    let over = catalogue.begin_check(
        &balance_op("t9", "sess-1", "acct-1", "50"),
        None,
        Instant(2_010),
    );
    assert_eq!(over.unwrap_err(), CheckRefusal::OverBound);
    // Another account has its own count.
    catalogue
        .begin_check(
            &balance_op("u1", "sess-2", "acct-2", "50"),
            None,
            Instant(2_010),
        )
        .unwrap();
    // The window moves on: the first check is 60 seconds old.
    catalogue
        .begin_check(
            &balance_op("t10", "sess-1", "acct-1", "50"),
            None,
            Instant(2_060),
        )
        .unwrap();

    assert_eq!(catalogue.counted_checks("acct-1", Instant(2_060)), 3);
    // Three counted before the window moved, the refused one, and the new one: all logged.
    assert_eq!(catalogue.check_log("acct-1").len(), 5);
    assert_eq!(catalogue.counted_checks("acct-2", Instant(2_060)), 1);
}

// verifies: REQ-CAT-3
#[test]
fn checks_for_the_holders_own_order_are_logged_but_do_not_count_against_the_bound() {
    let mut catalogue = world();
    for i in 0..10 {
        let o = balance_op(&format!("t{i}"), "sess-1", "acct-1", "50");
        catalogue.begin_check(&o, Some("order-7"), T0).unwrap();
    }
    assert_eq!(catalogue.counted_checks("acct-1", T0), 0);
    assert_eq!(catalogue.check_log("acct-1").len(), 10);
    // Any stated amount may be asked, since thresholds are free.
    catalogue
        .begin_check(&balance_op("t-big", "sess-1", "acct-1", "1"), None, T0)
        .unwrap();
}

// verifies: REQ-CAT-4, AC-15
#[test]
fn dry_run_revealing_sufficiency_counts_as_a_balance_check() {
    let mut catalogue = world();
    let dry = dry_withdrawal("t1", Some("sess-1"));
    assert!(reveals_sufficiency(&dry));
    assert!(reveals_sufficiency(&balance_op(
        "t0", "sess-1", "acct-1", "5"
    )));

    catalogue.begin_check(&dry, None, T0).unwrap();

    assert_eq!(catalogue.counted_checks("acct-1", T0), 1);
    // The reserve's refusal of the dry run is still the one check, not a free one.
    let refused = result(json!({
        "kind": "withdrawal", "transaction": "t1",
        "outcome": {"status": "refused", "refusal": {"class": "final"}},
    }));
    let ticket = catalogue
        .begin_check(&dry_withdrawal("t2", Some("sess-1")), None, T0)
        .unwrap();
    catalogue.answer_check(&ticket, &refused, T0);
    assert_eq!(catalogue.counted_checks("acct-1", T0), 2);
}

// verifies: REQ-CAT-4
#[test]
fn what_does_not_reveal_sufficiency_is_not_a_check() {
    let real_withdrawal = op(json!({
        "kind": "withdrawal", "transaction": "t1", "provider": RESERVE, "session": "sess-1",
        "origin": {"network": "polygon"}, "destination": "acme-ledger",
        "asset": "USDC", "amount": "100", "dry_run": false, "inputs": {},
    }));
    let deposit_dry_run = op(json!({
        "kind": "deposit", "transaction": "t2", "provider": RESERVE, "origin": "acme-ledger",
        "destination": {"network": "polygon"}, "asset": "USDC", "amount": "100",
        "dry_run": true, "inputs": {},
    }));
    assert!(!reveals_sufficiency(&real_withdrawal));
    assert!(!reveals_sufficiency(&deposit_dry_run));
    let mut catalogue = world();
    assert_eq!(
        catalogue
            .begin_check(&real_withdrawal, None, T0)
            .unwrap_err(),
        CheckRefusal::NotACheck
    );
}

// verifies: REQ-CAT-9
#[test]
fn balance_check_goes_only_to_a_reserve_with_a_grant_in_force() {
    let mut catalogue = acme_catalogue(&[exchange_advertisement()]);
    catalogue.set_check_bound(CheckBound {
        max_counted: 3,
        window_secs: 60,
    });
    catalogue.set_answer_within(10);
    catalogue.open_session("sess-1", "acct-1", Instant(10_000));
    let o = op(json!({
        "kind": "balance_verification", "transaction": "t1", "provider": "dex-pool",
        "session": "sess-1", "account": "acct-1", "asset": "USDC", "amount": "5",
    }));
    assert_eq!(
        catalogue.begin_check(&o, None, T0).unwrap_err(),
        CheckRefusal::Unusable(Unusable::OutsideGrantScope)
    );
}

// verifies: INV-9
#[test]
fn answer_carries_yes_or_no_for_a_matching_transaction_and_nothing_otherwise() {
    let mut catalogue = world();
    let ticket = catalogue
        .begin_check(&balance_op("t1", "sess-1", "acct-1", "50"), None, T0)
        .unwrap();

    // An answer for another transaction counts for nothing and leaves the check pending.
    assert_eq!(
        catalogue.answer_check(&ticket, &answer("t-other", true), Instant(2_001)),
        Verdict::NotKnown
    );
    assert_eq!(catalogue.pending_checks(), 1);
    assert_eq!(
        catalogue.answer_check(&ticket, &answer("t1", false), Instant(2_002)),
        Verdict::No
    );
    assert_eq!(catalogue.pending_checks(), 0);
}

// verifies: REQ-CAT-19, NFR-8
#[test]
fn silent_reserve_is_marked_ineligible_and_an_alert_is_raised() {
    let mut catalogue = world();
    let ad = reserve_advertisement();
    let key = tuples_of(&ad).remove(0);
    catalogue
        .begin_check(&balance_op("t1", "sess-1", "acct-1", "50"), None, T0)
        .unwrap();

    // One second before the stated time, nothing happens.
    assert!(catalogue.sweep_silent(Instant(2_009)).is_empty());
    assert_eq!(
        catalogue.reserve_eligibility(RESERVE, Instant(2_009)),
        Eligibility::Eligible
    );
    assert_eq!(catalogue.usable(RESERVE, &key, Instant(2_009)), Ok(()));

    // At the stated time, it is marked and one alert is raised.
    let alerts = catalogue.sweep_silent(Instant(2_010));
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].provider, RESERVE);
    assert_eq!(alerts[0].asked_at, T0);
    assert_eq!(
        catalogue.reserve_eligibility(RESERVE, Instant(2_010)),
        Eligibility::TemporarilyUnavailable {
            since: Instant(2_010)
        }
    );
    assert_eq!(
        catalogue.usable(RESERVE, &key, Instant(2_011)),
        Err(Unusable::TemporarilyUnavailable {
            since: Instant(2_010)
        })
    );
    // A second sweep raises nothing new, and the alert stays on the record.
    assert!(catalogue.sweep_silent(Instant(2_500)).is_empty());
    assert_eq!(catalogue.alerts().len(), 1);
}

// verifies: REQ-CAT-19, REQ-CAT-3
#[test]
fn reserve_answering_a_later_check_in_time_is_eligible_again() {
    let mut catalogue = world();
    let first = catalogue
        .begin_check(&balance_op("t1", "sess-1", "acct-1", "50"), None, T0)
        .unwrap();
    catalogue.sweep_silent(Instant(2_010));
    // A late answer is still delivered, but doesn't lift the mark.
    assert_eq!(
        catalogue.answer_check(&first, &answer("t1", true), Instant(2_050)),
        Verdict::Yes
    );
    let ticket = catalogue_ticket(&mut catalogue, "t1x");
    let late = catalogue.answer_check(&ticket, &answer("t1x", true), Instant(2_100));
    assert_eq!(late, Verdict::Yes);
    assert!(matches!(
        catalogue.reserve_eligibility(RESERVE, Instant(2_100)),
        Eligibility::TemporarilyUnavailable { .. }
    ));
    // A check sent now and answered in time lifts it.
    let ticket = catalogue
        .begin_check(
            &balance_op("t2", "sess-1", "acct-1", "50"),
            None,
            Instant(2_200),
        )
        .unwrap();
    assert_eq!(
        catalogue.answer_check(&ticket, &answer("t2", true), Instant(2_203)),
        Verdict::Yes
    );
    assert_eq!(
        catalogue.reserve_eligibility(RESERVE, Instant(2_203)),
        Eligibility::Eligible
    );
}

/// A ticket for a check made at 2 000 whose answer is read late.
fn catalogue_ticket(catalogue: &mut Catalogue, tx: &str) -> rutent_prov::catalogue::CheckTicket {
    catalogue
        .begin_check(
            &balance_op(tx, "sess-1", "acct-1", "50"),
            Some("order-1"),
            Instant(2_020),
        )
        .unwrap()
}

// verifies: REQ-CAT-18
#[test]
fn each_kind_of_refusal_is_told_apart() {
    let lifts = result(json!({
        "kind": "transfer", "transaction": "t",
        "outcome": {"status": "refused", "refusal": {"class": "lifts_at_known_time", "lifts_at": "2026-11-07T10:00:00Z"}},
    }));
    let waits = result(json!({
        "kind": "transfer", "transaction": "t",
        "outcome": {"status": "refused", "refusal": {"class": "waits_on_person"}},
    }));
    let end = result(json!({
        "kind": "transfer", "transaction": "t",
        "outcome": {"status": "refused", "refusal": {"class": "final"}},
    }));
    let transfer = op(json!({
        "kind": "transfer", "transaction": "t", "provider": "bridge-pay", "origin": "polygon",
        "destination": "acme-ledger", "asset": "USDC", "direction": "exact_in", "amount": "10",
        "dry_run": true, "inputs": {},
    }));
    let catalogue = Catalogue::new();

    let class = |r: &ResultBody| match catalogue.assess_dry_run(&transfer, r).unwrap() {
        DryRunAssessment::Rejected(class) => class,
        other => panic!("expected a rejection, got {other:?}"),
    };

    assert_eq!(
        class(&lifts),
        RefusalClass::LiftsAtKnownTime {
            lifts_at: "2026-11-07T10:00:00Z".into()
        }
    );
    assert_eq!(class(&waits), RefusalClass::WaitsOnPerson);
    assert_eq!(class(&end), RefusalClass::Final);
    assert!(!class(&lifts).is_final() && !class(&waits).is_final() && class(&end).is_final());
}

// verifies: REQ-CAT-14
#[test]
fn rejected_dry_run_removes_that_option_and_an_accepted_one_keeps_it() {
    let catalogue = Catalogue::new();
    let dry = dry_deposit("tx-012");
    let accepted = result(json!({
        "kind": "deposit", "transaction": "tx-012",
        "outcome": {"status": "dry_run_accepted", "amount_in": "100", "amount_out": "99",
            "fee": {"asset": "USDC", "amount": "1"}, "valid_until": "2026-11-06T15:05:05Z",
            "voided_by": ["validity_passes"], "settlement_seconds": 60},
    }));
    let refused = result(json!({
        "kind": "deposit", "transaction": "tx-012",
        "outcome": {"status": "refused", "refusal": {"class": "final"}},
    }));

    let kept = catalogue.assess_dry_run(&dry, &accepted).unwrap();
    let removed = catalogue.assess_dry_run(&dry, &refused).unwrap();

    assert_eq!(kept, DryRunAssessment::Serves);
    assert!(!kept.removes_option());
    assert!(removed.removes_option());
}

// verifies: REQ-CAT-14
#[test]
fn dry_run_changes_nothing_in_the_catalogue_or_at_the_origin() {
    let catalogue = world();
    let before = format!("{catalogue:?}");
    let not_a_dry_run = op(json!({
        "kind": "conversion", "transaction": "tx-1", "provider": "dex-pool", "node": "polygon",
        "from": "USDC", "to": "COPM", "direction": "exact_in", "amount": "100", "dry_run": false, "inputs": {},
    }));
    let accepted_withdrawal = result(json!({
        "kind": "deposit", "transaction": "tx-2",
        "outcome": {"status": "dry_run_accepted", "amount_in": "100", "amount_out": "99",
            "fee": {"asset": "USDC", "amount": "1"}, "valid_until": "2026-11-06T15:05:05Z",
            "voided_by": ["validity_passes"], "settlement_seconds": 60},
    }));
    let withdrawal = dry_deposit("tx-2");

    let assessed = catalogue.assess_dry_run(&withdrawal, &accepted_withdrawal);
    let effect = segment_effect(&withdrawal, &accepted_withdrawal).unwrap();
    // A result that isn't a dry run's answer is not taken as one.
    let refused_as_dry = catalogue.assess_dry_run(&not_a_dry_run, &accepted_withdrawal);

    assert_eq!(assessed.unwrap(), DryRunAssessment::Serves);
    assert!(matches!(effect, SegmentEffect::NothingTaken { .. }));
    assert!(refused_as_dry.is_err());
    assert_eq!(format!("{catalogue:?}"), before);
}

fn reversal_op(amount: &str) -> Operation {
    op(json!({
        "kind": "reversal", "transaction": "rev-1", "provider": "bridge-pay", "reverses": "tx-003",
        "node": "polygon", "asset": "USDC", "amount": amount,
    }))
}

fn reversal_result(status: Value) -> ResultBody {
    result(json!({"kind": "reversal", "transaction": "rev-1", "outcome": status}))
}

// verifies: REQ-CAT-15
#[test]
fn only_a_true_undo_counts_as_a_reversal() {
    let evidence = json!([{"grade": 1, "reference": "0x1", "by": "polygon"}]);
    let whole = reversal_result(json!({
        "status": "succeeded", "node": "polygon", "asset": "USDC", "amount": "100", "evidence": evidence,
    }));
    let less = reversal_result(json!({
        "status": "succeeded", "node": "polygon", "asset": "USDC", "amount": "99", "evidence": evidence,
    }));
    let other_asset = reversal_result(json!({
        "status": "succeeded", "node": "polygon", "asset": "EURC", "amount": "100", "evidence": evidence,
    }));
    let other_node = reversal_result(json!({
        "status": "succeeded", "node": "elsewhere", "asset": "USDC", "amount": "100", "evidence": evidence,
    }));
    let refused = reversal_result(json!({"status": "refused", "refusal": {"class": "final"}}));
    let unknown = reversal_result(json!({"status": "unknown"}));
    let o = reversal_op("100");

    assert_eq!(judge_reversal(&o, &whole), UndoVerdict::TrueUndo);
    for not_undone in [&less, &other_asset, &other_node, &refused, &unknown] {
        assert_eq!(judge_reversal(&o, not_undone), UndoVerdict::NotUndone);
    }
}

fn exchange_catalogue() -> (Catalogue, TupleKey) {
    let ad = exchange_advertisement();
    let mut key = tuples_of(&ad).remove(0);
    key.asset = "USDC".into();
    (acme_catalogue(&[ad]), key)
}

// verifies: REQ-CAT-15
#[test]
fn segment_of_an_exchange_without_a_true_undo_is_declined() {
    let (mut catalogue, key) = exchange_catalogue();
    // The common grant's ceiling is 500 USDC.
    assert_eq!(catalogue.admit_segment("dex-pool", &key, 500, T0), Ok(()));
    assert_eq!(
        catalogue.admit_segment("dex-pool", &key, 501, T0),
        Err(SegmentDecline::OverReversalCeiling { ceiling: 500 })
    );
    catalogue.set_ceiling("g-0", 0).unwrap();
    assert_eq!(
        catalogue.admit_segment("dex-pool", &key, 1, T0),
        Err(SegmentDecline::NoReversalCapacity)
    );
    assert_eq!(
        catalogue.admit_segment("nobody", &key, 1, T0),
        Err(SegmentDecline::NoReversalCapacity)
    );
}

// verifies: REQ-CAT-15
#[test]
fn reserve_is_not_declined_for_lacking_a_true_undo() {
    let ad = reserve_advertisement();
    let key = tuples_of(&ad).remove(0);
    let mut catalogue = acme_catalogue(&[ad]);
    catalogue.set_ceiling("g-0", 0).unwrap();
    assert_eq!(
        catalogue.admit_segment(RESERVE, &key, 1_000_000, T0),
        Ok(())
    );
}

// verifies: INV-2
#[test]
fn failed_segment_leaves_its_value_at_its_origin_in_full_for_each_kind_of_segment() {
    let withdrawal = dry_withdrawal("t", None);
    let failed = result(json!({
        "kind": "withdrawal", "transaction": "t",
        "outcome": {"status": "failed", "evidence": [{"grade": 1, "reference": "0x1", "by": "polygon"}]},
    }));
    let unknown =
        result(json!({"kind": "withdrawal", "transaction": "t", "outcome": {"status": "unknown"}}));

    let effect = segment_effect(&withdrawal, &failed).unwrap();
    assert_eq!(
        effect,
        SegmentEffect::NothingTaken {
            node: rutent_prov::catalogue::Endpoint::Outer("polygon".into()),
            asset: "USDC".into()
        }
    );
    // Unknown is neither: the value is not claimed to be anywhere.
    assert_eq!(
        segment_effect(&withdrawal, &unknown).unwrap(),
        SegmentEffect::Unresolved
    );
}

fn dry_deposit(tx: &str) -> Operation {
    op(json!({
        "kind": "deposit", "transaction": tx, "provider": RESERVE, "origin": "acme-ledger",
        "destination": {"network": "polygon"}, "asset": "USDC", "amount": "100",
        "dry_run": true, "inputs": {},
    }))
}

// verifies: REQ-CAT-4, AC-15
#[test]
fn withdrawal_dry_run_is_never_read_outside_the_check_path() {
    let catalogue = world();
    let dry = dry_withdrawal("t1", Some("sess-1"));
    for outcome in [
        json!({"status": "refused", "refusal": {"class": "lifts_at_known_time", "lifts_at": "2026-11-07T10:00:00Z"}}),
        json!({"status": "refused", "refusal": {"class": "final"}}),
    ] {
        let r = result(json!({"kind": "withdrawal", "transaction": "t1", "outcome": outcome}));
        assert_eq!(
            catalogue.assess_dry_run(&dry, &r).unwrap_err(),
            DryRunError::RevealsSufficiency
        );
    }
    assert_eq!(catalogue.counted_checks("acct-1", T0), 0);
}

// verifies: REQ-CAT-19, NFR-8
#[test]
fn reserve_that_stays_silent_is_out_at_its_due_time_even_with_no_sweep() {
    let mut catalogue = world();
    let key = tuples_of(&reserve_advertisement()).remove(0);
    catalogue
        .begin_check(&balance_op("t1", "sess-1", "acct-1", "50"), None, T0)
        .unwrap();

    assert_eq!(catalogue.usable(RESERVE, &key, Instant(2_009)), Ok(()));
    assert_eq!(
        catalogue.usable(RESERVE, &key, Instant(2_010)),
        Err(Unusable::TemporarilyUnavailable {
            since: Instant(2_010)
        })
    );
    assert_eq!(
        catalogue.reserve_eligibility(RESERVE, Instant(2_500)),
        Eligibility::TemporarilyUnavailable {
            since: Instant(2_010)
        }
    );
}

// verifies: REQ-CAT-19
#[test]
fn answer_in_time_to_one_check_does_not_lift_the_mark_while_another_is_silent() {
    let mut catalogue = world();
    catalogue
        .begin_check(&balance_op("a", "sess-1", "acct-1", "50"), Some("o"), T0)
        .unwrap();
    let b = catalogue
        .begin_check(
            &balance_op("b", "sess-1", "acct-1", "50"),
            Some("o"),
            Instant(2_015),
        )
        .unwrap();
    catalogue.sweep_silent(Instant(2_012));

    assert_eq!(
        catalogue.answer_check(&b, &answer("b", true), Instant(2_016)),
        Verdict::Yes
    );
    assert!(matches!(
        catalogue.reserve_eligibility(RESERVE, Instant(2_016)),
        Eligibility::TemporarilyUnavailable { .. }
    ));
}

// verifies: INV-2, REQ-CAT-14
#[test]
fn dry_run_answered_as_if_carried_out_is_not_read_as_value_taken() {
    let dry = dry_deposit("t");
    let done = result(json!({
        "kind": "deposit", "transaction": "t",
        "outcome": {"status": "succeeded", "amount_in": "100", "amount_out": "99",
            "fee": {"asset": "USDC", "amount": "1"},
            "evidence": [{"grade": 1, "reference": "0x1", "by": "polygon"}]},
    }));
    assert_eq!(
        segment_effect(&dry, &done).unwrap_err(),
        EffectError::NotADryRunAnswer
    );
}
