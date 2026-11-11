//! Statements tied to their makers' keys before a record is formed (`T-020-05`; `REQ-AUD-4`, `REQ-PRT-1`,
//! `THR-3`), and refused ones recorded as unverified.

mod snapshot_support;

use ed25519_dalek::SigningKey;
use rutent_contracts::core_internal::{Attribution, Entry};
use rutent_contracts::provider_role::{Deployment, JUDGEMENT_SHAPE, RESULT_SHAPE};
use rutent_contracts::router_api::{
    DECISION_RECORD_SHAPE, FigureValue, PositiveAmount, Request, Timestamp,
};
use rutent_core_orch::snapshot::{
    Asked, Exposures, Received, Refusal, RefusedStatement, Releases, bind_operator_statement,
    bind_statement, record_refused,
};
use rutent_prov::catalogue::{Instant, StatementRefusal};
use serde_json::{Value, json};
use snapshot_support::*;

/// A reserve's accepted dry-run withdrawal of 1 000, made at `made_at` in `deployment`, signed with `key`.
fn dry_run(deployment: &Deployment, made_at: u64, key: &SigningKey) -> Vec<u8> {
    sign(dry_run_value(deployment, made_at), key)
}

fn dry_run_value(deployment: &Deployment, made_at: u64) -> Value {
    envelope(
        deployment,
        RESULT_SHAPE,
        RESERVE,
        made_at,
        json!({
            "kind": "withdrawal",
            "transaction": format!("tx-{made_at}"),
            "outcome": {
                "status": "dry_run_accepted",
                "amount_in": "1000",
                "amount_out": "1000",
                "fee": { "asset": "USDC", "amount": "0" },
                "valid_until": Instant(made_at + 3_600).rfc3339().unwrap(),
                "voided_by": ["validity_passes", "amount_changes"],
                "settlement_seconds": 60,
            },
        }),
    )
}

/// A judgement result of the reserve's, at 3 500 and signed with `outer_key`, nesting a judgement made by the
/// reserve at `nested_at` in `nested_deployment`, signed with `nested_key`.
fn nesting(
    outer_key: &SigningKey,
    nested_deployment: &Deployment,
    nested_at: u64,
    nested_key: &SigningKey,
) -> Vec<u8> {
    let mut nested = envelope(
        nested_deployment,
        JUDGEMENT_SHAPE,
        RESERVE,
        nested_at,
        json!({
            "subject": "subj-1",
            "class": "kyc.natural_person",
            "answer": "yes",
            "valid_until": Instant(nested_at + 86_400).rfc3339().unwrap(),
        }),
    );
    sign_value(&mut nested, nested_key);
    let outer = envelope(
        &deployment(),
        RESULT_SHAPE,
        RESERVE,
        3_500,
        json!({ "kind": "judgement", "outcome": { "status": "judged", "judgement": nested } }),
    );
    sign(outer, outer_key)
}

fn asked_dry_run(bytes: Vec<u8>) -> Received {
    Received {
        tuple: withdrawal_tuple(RESERVE),
        asked: Asked::DryRun {
            amount_in: PositiveAmount::new("1000").unwrap(),
        },
        bytes,
    }
}

/// The dry runs a snapshot carries, by their signing key and `made_at`.
fn dry_runs(snapshot: &rutent_contracts::router_api::CatalogueSnapshot) -> Vec<(String, String)> {
    snapshot
        .figures
        .iter()
        .filter_map(|f| match &f.value {
            FigureValue::DryRun { result, .. } => Some((
                result.signature.key.as_str().to_owned(),
                result.made_at.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// A choice round refining a shortlist taken at 3 000.
fn choice_request() -> Request {
    serde_json::from_value(json!({
        "request_id": "req-2",
        "round": { "kind": "choice", "refines": {
            "request_id": "req-1", "taken_at": Instant(3_000).rfc3339().unwrap()
        } },
        "from": outer(),
        "to": outer(),
        "amount": "1000",
        "objective": { "kind": "lowest_total_cost" },
    }))
    .unwrap()
}

/// Records each refused statement and checks it went to the log as its claimed maker's word, unverified, with its
/// bytes in the evidence store.
fn record_each(refused: &[RefusedStatement]) {
    let mut log = KeptLog::default();
    let mut evidence = KeptEvidence::default();
    for statement in refused {
        record_refused(
            &mut evidence,
            &mut log,
            statement,
            provider_actor(&statement.claimed_maker().unwrap()),
            Timestamp::new(Instant(4_000).rfc3339().unwrap()).unwrap(),
            None,
        )
        .unwrap();
    }
    assert_eq!(log.entries.len(), refused.len());
    for (entry, statement) in log.entries.iter().zip(refused) {
        let Entry::Stated(stated) = entry else {
            panic!("a refused statement is a stated entry");
        };
        assert!(matches!(stated.attribution, Attribution::Unverified { .. }));
        assert!(evidence.kept.contains(&statement.bytes));
    }
}

// verifies: REQ-AUD-4, THR-3
#[test]
fn statement_signed_by_a_key_not_current_at_its_making_is_refused_before_the_record() {
    let mut catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
    let (first, second) = (statements_key(RESERVE, 1), statements_key(RESERVE, 2));
    // At 3 000 the reserve's second advertisement retires its first statements key for a second one.
    let successor = advertisement_bytes(
        &reserve(RESERVE, "acme"),
        keys_of(RESERVE, &[&second]),
        3_000,
    );
    catalogue.join(&successor, Instant(3_000)).unwrap();
    let clock = TestClock::at(4_000);
    let bounds = no_ceilings();

    let good = dry_run(&deployment(), 3_500, &second);
    let before_its_key = dry_run(&deployment(), 2_500, &second);
    let retired_key = dry_run(&deployment(), 3_600, &first);
    let statements = [
        asked_dry_run(good.clone()),
        asked_dry_run(before_its_key.clone()),
        asked_dry_run(retired_key.clone()),
    ];
    let formed = form(
        &catalogue,
        &clock,
        &Exposures::new(),
        &Releases::new(),
        &bounds,
        &prices(&[RESERVE]),
        &statements,
    );

    let carried = dry_runs(&formed.snapshot);
    assert_eq!(
        carried,
        vec![(
            public_key(&second).as_str().to_owned(),
            Instant(3_500).rfc3339().unwrap()
        )]
    );
    let refused: Vec<(&[u8], &Refusal)> = formed
        .refused
        .iter()
        .map(|r| (r.bytes.as_slice(), &r.reason))
        .collect();
    let not_in_force = Refusal::Key {
        path: vec![],
        refusal: StatementRefusal::KeyNotInForce,
    };
    assert_eq!(
        refused,
        vec![
            (before_its_key.as_slice(), &not_in_force),
            (retired_key.as_slice(), &not_in_force),
        ]
    );
    // The record formed from the snapshot carries neither.
    let record = route(&choice_request(), &formed.snapshot, &bounds);
    assert_eq!(dry_runs(&record.snapshot), carried);
    record_each(&formed.refused);

    // A statement nested in another is tied to its own maker's key at its own making.
    assert_eq!(
        bind_statement(
            &catalogue,
            &nesting(&second, &deployment(), 3_400, &second),
            Instant(4_000)
        ),
        Ok(())
    );
    assert_eq!(
        bind_statement(
            &catalogue,
            &nesting(&second, &deployment(), 2_500, &second),
            Instant(4_000)
        ),
        Err(Refusal::Key {
            path: vec!["statement".into(), "outcome".into(), "judgement".into()],
            refusal: StatementRefusal::KeyNotInForce,
        })
    );
    // One nested where member names can't reach it isn't taken.
    let mut listed: Value =
        serde_json::from_slice(&nesting(&second, &deployment(), 3_400, &second)).unwrap();
    let nested = listed["statement"]["outcome"]["judgement"].take();
    listed["statement"]["outcome"]["judgement"] = json!([nested]);
    assert_eq!(
        bind_statement(&catalogue, &sign(listed, &second), Instant(4_000)),
        Err(Refusal::NestedOutOfReach)
    );
}

// verifies: REQ-PRT-1
#[test]
fn statement_of_another_deployment_is_refused_before_the_record() {
    let catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
    let key = statements_key(RESERVE, 1);
    let clock = TestClock::at(4_000);
    let bounds = no_ceilings();

    let ours = dry_run(&deployment(), 3_500, &key);
    let theirs = dry_run(&other_deployment(), 3_600, &key);
    let formed = form(
        &catalogue,
        &clock,
        &Exposures::new(),
        &Releases::new(),
        &bounds,
        &prices(&[RESERVE]),
        &[asked_dry_run(ours), asked_dry_run(theirs.clone())],
    );

    assert_eq!(
        dry_runs(&formed.snapshot),
        vec![(
            public_key(&key).as_str().to_owned(),
            Instant(3_500).rfc3339().unwrap()
        )]
    );
    assert_eq!(formed.refused.len(), 1);
    assert_eq!(formed.refused[0].bytes, theirs);
    assert_eq!(
        formed.refused[0].reason,
        Refusal::Key {
            path: vec![],
            refusal: StatementRefusal::WrongContext,
        }
    );
    let record = route(&choice_request(), &formed.snapshot, &bounds);
    assert!(record.snapshot.figures.iter().all(|f| match &f.value {
        FigureValue::DryRun { result, .. } => result.deployment == deployment(),
        _ => true,
    }));
    record_each(&formed.refused);

    // Nested in a statement of this deployment, another deployment's statement is refused all the same.
    assert_eq!(
        bind_statement(
            &catalogue,
            &nesting(&key, &other_deployment(), 3_400, &key),
            Instant(4_000)
        ),
        Err(Refusal::Key {
            path: vec!["statement".into(), "outcome".into(), "judgement".into()],
            refusal: StatementRefusal::WrongContext,
        })
    );
}

// verifies: REQ-AUD-4
#[test]
fn operators_statement_is_tied_to_the_deployments_operator_key() {
    let catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
    let record = |key: &SigningKey| {
        sign(
            envelope(
                &deployment(),
                DECISION_RECORD_SHAPE,
                "rutent-core",
                3_500,
                json!({}),
            ),
            key,
        )
    };
    assert_eq!(
        bind_operator_statement(&catalogue, &record(&operator_key()), Instant(4_000)),
        Ok(())
    );
    assert_eq!(
        bind_operator_statement(
            &catalogue,
            &record(&statements_key(RESERVE, 1)),
            Instant(4_000)
        ),
        Err(Refusal::Key {
            path: vec![],
            refusal: StatementRefusal::NotSignedByOperator,
        })
    );
    // A module the operator publishes speaks with the operator's key, never one an advertisement lists.
    let holding = |key: &SigningKey| {
        sign(
            envelope(
                &deployment(),
                RESULT_SHAPE,
                "wallet-module",
                3_500,
                json!({}),
            ),
            key,
        )
    };
    assert_eq!(
        bind_statement(&catalogue, &holding(&operator_key()), Instant(4_000)),
        Ok(())
    );
    assert_eq!(
        bind_statement(
            &catalogue,
            &holding(&statements_key(RESERVE, 1)),
            Instant(4_000)
        ),
        Err(Refusal::Key {
            path: vec![],
            refusal: StatementRefusal::NotSignedByOperator,
        })
    );
}
