//! Signed holds redeemed in intake (`T-020-15`), over the assembled Core against the simulated providers: the acceptance
//! the assembled Core records names the receipt of the hold it redeems, a hold is redeemed by one order only also
//! after a restart, and the log admits everything it takes.

mod routing_support;

use routing_support::*;
use rutent_contracts::core_internal::{Receipt, What};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::HOLD_SHAPE;
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_core_orch::intake::{IntakeError, Status, Told};
use rutent_core_orch::quote::HoldError;
use rutent_scenarios::assembly::Core;
use rutent_scenarios::assembly::StepError;
use rutent_scenarios::kit;
use serde_json::{Value, json};

/// The reserve's signed hold of the quote `record` chose, until `until`, for `held_for`.
fn signed_hold(
    core: &Core,
    record: &DecisionRecord,
    recorded: &Digest,
    transaction: &str,
    held_for: Value,
    until: &str,
) -> Vec<u8> {
    let context = rutent_scenarios::assembly::context();
    let chosen = record.chosen().expect("the record chose a candidate");
    let mut statement = json!({
        "deployment": context.deployment,
        "protocol_version": context.protocol_version,
        "shape": HOLD_SHAPE,
        "maker": A,
        "made_at": core.clock.timestamp(),
        "signature": { "scheme": "ed25519", "key": "", "value": "" },
        "statement": {
            "transaction": transaction,
            "record": { "id": record.record_id(), "digest": recorded },
            "candidate": chosen.candidate_id,
            "for": held_for,
            "valid_until": until,
        },
    });
    let key = core
        .statements_signing_key(A)
        .expect("the reserve is installed");
    rutent_conformance::signing::sign_value(&mut statement, &key);
    serde_json::to_vec(&statement).expect("JSON")
}

/// The id and digest the log names `order`'s last decision record by.
fn recorded_as(core: &Core, order: &Id) -> (Id, Digest) {
    core.verified_whats()
        .into_iter()
        .rev()
        .find_map(|w| match w {
            What::DecisionRecord {
                decision_record,
                order: o,
                record,
                ..
            } if &o == order => Some((decision_record, record)),
            _ => None,
        })
        .expect("the log names the decision record")
}

/// The acceptances the log holds: their order and the hold each cites.
fn acceptances(core: &Core) -> Vec<(Id, Option<Receipt>)> {
    core.verified_whats()
        .into_iter()
        .filter_map(|w| match w {
            What::Acceptance { order, held, .. } => Some((order, held)),
            _ => None,
        })
        .collect()
}

/// An order for the whole amount at `a`, quoted: its id and its quote's digest.
fn offered(core: &mut Core, session: &Id, name: &str) -> (Id, Digest) {
    let (order, told) = quoted(core, request(name, AMOUNT, session, "lowest_total_cost"));
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted");
    };
    (order, digest)
}

/// The reserve holds `order`'s quote for `held_for` and Core takes its hold: the hold's transaction and the receipt of
/// its entry.
fn held(core: &mut Core, order: &Id, transaction: &str, held_for: Value) -> (Id, Receipt) {
    let (record_id, digest) = recorded_as(core, order);
    let record = record_named(core, &record_id);
    let chosen = record.chosen().expect("chosen");
    // As long as the figures the route stands on allow, to the second.
    let now = core.clock.timestamp().unix_nanos();
    let expiry = record
        .earliest_expiry(chosen)
        .unwrap_or(now + 60_000_000_000);
    let until = rutent_contracts::router_api::Timestamp::new(
        rutent_prov::catalogue::Instant(u64::try_from(expiry / 1_000_000_000).expect("a time"))
            .rfc3339()
            .expect("a time"),
    )
    .expect("a timestamp");
    let bytes = signed_hold(
        core,
        &record,
        &digest,
        transaction,
        held_for,
        until.as_str(),
    );
    let receipt = core
        .state_hold(&bytes, &record_id, &digest, record, until)
        .expect("Core takes the reserve's hold");
    (kit::id(transaction), receipt)
}

// verifies: REQ-QTE-15
#[test]
fn the_acceptance_an_assembled_core_records_names_the_redeemed_holds_receipt() {
    let (mut core, session) = core_with("AC-23", &[]);
    let (order, digest) = offered(&mut core, &session, "req-held");
    let (hold, receipt) = held(
        &mut core,
        &order,
        "hold-1",
        json!({ "kind": "order", "order": order }),
    );

    core.accept_holding(&kit::customer(), &order, &digest, Some(&hold))
        .expect("the acceptance redeems the hold");
    assert_eq!(
        acceptances(&core),
        vec![(order.clone(), Some(receipt))],
        "the acceptance names the receipt of the hold's entry"
    );
    assert_eq!(core.holds().redeemed_by(&hold), Some(&order));
}

// verifies: REQ-QTE-15
#[test]
fn a_hold_is_redeemed_by_one_order_only_also_after_a_restart() {
    let (mut core, session) = core_with("AC-23", &[]);
    let (first, first_digest) = offered(&mut core, &session, "req-first");
    let (second, second_digest) = offered(&mut core, &session, "req-second");
    let (hold, _) = held(
        &mut core,
        &first,
        "hold-1",
        json!({ "kind": "session", "session": session }),
    );
    core.accept_holding(&kit::customer(), &first, &first_digest, Some(&hold))
        .expect("the first order redeems it");

    // Core's holds are lost and rebuilt from the log; the reserve's hold is kept again.
    core.holds_rebuilt_from_log();
    assert_eq!(core.holds().redeemed_by(&hold), Some(&first));
    let (again, _) = held(
        &mut core,
        &first,
        "hold-1",
        json!({ "kind": "session", "session": session }),
    );
    assert_eq!(again, hold);

    let recorded = acceptances(&core).len();
    let refused = core.accept_holding(&kit::customer(), &second, &second_digest, Some(&hold));
    assert!(
        matches!(
            refused,
            Err(StepError::Intake(IntakeError::Hold(
                HoldError::RedeemedByAnother
            )))
        ),
        "{refused:?}"
    );
    assert_eq!(acceptances(&core).len(), recorded, "nothing was recorded");
}
