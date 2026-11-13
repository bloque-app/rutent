//! Statements tied to their makers' keys before a record is formed (`T-020-05`; `REQ-AUD-4`, `REQ-PRT-1`,
//! `THR-3`), and refused ones recorded as unverified.

mod snapshot_support;

use ed25519_dalek::SigningKey;
use rutent_contracts::core_internal::calls::{
    AppendFailure, KeptStatements, Signatures, StatementKeys,
};
use rutent_contracts::core_internal::{
    Attribution, Entry, KeptStatement, SignedWith, StatedEntry, attribute,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{Deployment, JUDGEMENT_SHAPE, RESULT_SHAPE};
use rutent_contracts::router_api::{DECISION_RECORD_SHAPE, FigureValue, PositiveAmount, Request};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_orch::snapshot::{
    Asked, Clock, Exposures, PathStep, Received, Refusal, RefusedStatement, Releases,
    bind_operator_statement, bind_statement, record_refused,
};
use rutent_prov::catalogue::Publisher;
use rutent_prov::catalogue::{Instant, StatementRefusal};
use serde_json::{Value, json};
use snapshot_support::*;

fn m(name: &str) -> PathStep {
    PathStep::Member(name.into())
}

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

/// Records each refused statement and checks it went to the log unverified, claimed by the maker its envelope names
/// and about the transaction its bytes sign, for the log to attribute; received at Core's clock, with its bytes in
/// the evidence store.
fn record_each(
    catalogue: &rutent_prov::catalogue::Catalogue,
    clock: &TestClock,
    refused: &[RefusedStatement],
) {
    let mut log = KeptLog::default();
    let mut evidence = KeptEvidence::default();
    for statement in refused {
        record_refused(&mut evidence, &mut log, clock, catalogue, statement, None).unwrap();
    }
    assert_eq!(log.entries.len(), refused.len());
    for (entry, statement) in log.entries.iter().zip(refused) {
        let Entry::Stated(stated) = entry else {
            panic!("a refused statement is a stated entry");
        };
        let Attribution::Unverified { claimed_by } = &stated.attribution else {
            panic!("orchestration appends a statement unverified; the log attributes it");
        };
        assert_eq!(claimed_by.acts.as_str(), RESERVE);
        assert_eq!(claimed_by.party.as_str(), "acme");
        assert_eq!(
            stated
                .about
                .as_ref()
                .and_then(|a| a.transaction.as_ref())
                .map(|t| t.as_str()),
            signed_transaction(&statement.bytes).as_deref()
        );
        assert_eq!(
            stated.received_at.as_str(),
            clock.now().timestamp().unwrap().as_str()
        );
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
    record_each(&catalogue, &clock, &formed.refused);

    // A statement nested in another is tied to its own maker's key at its own making.
    assert_eq!(
        bind_statement(
            &catalogue,
            &clock,
            &nesting(&second, &deployment(), 3_400, &second)
        ),
        Ok(())
    );
    assert_eq!(
        bind_statement(
            &catalogue,
            &clock,
            &nesting(&second, &deployment(), 2_500, &second)
        ),
        Err(Refusal::Key {
            path: vec![m("statement"), m("outcome"), m("judgement")],
            refusal: StatementRefusal::KeyNotInForce,
        })
    );
    // Nested in a list, a statement is found by its place and tied all the same.
    let listed = |nested_at: u64| {
        let mut listed: Value =
            serde_json::from_slice(&nesting(&second, &deployment(), nested_at, &second)).unwrap();
        let nested = listed["statement"]["outcome"]["judgement"].take();
        listed["statement"]["outcome"]["judgement"] = json!([{ "other": 1 }, nested]);
        sign(listed, &second)
    };
    assert_eq!(bind_statement(&catalogue, &clock, &listed(3_400)), Ok(()));
    assert_eq!(
        bind_statement(&catalogue, &clock, &listed(2_500)),
        Err(Refusal::Key {
            path: vec![
                m("statement"),
                m("outcome"),
                m("judgement"),
                PathStep::Item(1)
            ],
            refusal: StatementRefusal::KeyNotInForce,
        })
    );

    // A statement refused for something else than its key is still its maker's: one dated after the snapshot,
    // within the clock's skew.
    let ahead = dry_run(&deployment(), 4_100, &second);
    let formed = form(
        &catalogue,
        &clock,
        &Exposures::new(),
        &Releases::new(),
        &bounds,
        &prices(&[RESERVE]),
        &[asked_dry_run(ahead)],
    );
    assert_eq!(formed.refused.len(), 1);
    assert_eq!(formed.refused[0].reason, Refusal::DatedAfterSnapshot);
    assert!(formed.refused[0].verified);
    record_each(&catalogue, &clock, &formed.refused);
}

// verifies: REQ-AUD-4, THR-3
#[test]
fn decision_record_signed_by_the_operator_is_tied_with_every_statement_it_carries() {
    let catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
    let key = statements_key(RESERVE, 1);
    let clock = TestClock::at(4_000);
    let bounds = no_ceilings();
    let formed = form(
        &catalogue,
        &clock,
        &Exposures::new(),
        &Releases::new(),
        &bounds,
        &prices(&[RESERVE]),
        &[asked_dry_run(dry_run(&deployment(), 3_500, &key))],
    );
    let record = route(&choice_request(), &formed.snapshot, &bounds);
    assert_eq!(dry_runs(&record.snapshot).len(), 1);
    let recorded = |record: Value, signer: &SigningKey| {
        sign(
            envelope(
                &deployment(),
                DECISION_RECORD_SHAPE,
                "rutent-core",
                3_900,
                record,
            ),
            signer,
        )
    };
    let as_made = serde_json::to_value(&record).unwrap();

    // The router's own record, signed with the operator key: it and the dry run in its snapshot are tied.
    assert_eq!(
        bind_operator_statement(
            &catalogue,
            &clock,
            &recorded(as_made.clone(), &operator_key())
        ),
        Ok(())
    );
    // The same record carrying a dry run signed with a key the reserve never listed is refused, at its place.
    let mut forged = as_made.clone();
    let figures = forged["snapshot"]["figures"].as_array_mut().unwrap();
    let place = figures
        .iter()
        .position(|f| f["value"]["result"].is_object())
        .expect("the dry run's figure");
    let stray = signing_key("stray", 9);
    figures[place]["value"]["result"] =
        serde_json::from_slice(&dry_run(&deployment(), 3_500, &stray)).unwrap();
    assert_eq!(
        bind_operator_statement(&catalogue, &clock, &recorded(forged, &operator_key())),
        Err(Refusal::Key {
            path: vec![
                m("statement"),
                m("snapshot"),
                m("figures"),
                PathStep::Item(place),
                m("value"),
                m("result")
            ],
            refusal: StatementRefusal::KeyNotInForce,
        })
    );
    // And the record itself, signed with anything but the operator key, is refused.
    assert_eq!(
        bind_operator_statement(&catalogue, &clock, &recorded(as_made, &key)),
        Err(Refusal::Key {
            path: vec![],
            refusal: StatementRefusal::NotSignedByOperator,
        })
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
    record_each(&catalogue, &clock, &formed.refused);

    // Nested in a statement of this deployment, another deployment's statement is refused all the same.
    assert_eq!(
        bind_statement(
            &catalogue,
            &clock,
            &nesting(&key, &other_deployment(), 3_400, &key)
        ),
        Err(Refusal::Key {
            path: vec![m("statement"), m("outcome"), m("judgement")],
            refusal: StatementRefusal::WrongContext,
        })
    );
}

// verifies: REQ-AUD-4
#[test]
fn operators_statement_is_tied_to_the_deployments_operator_key() {
    let catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
    let clock = TestClock::at(4_000);
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
        bind_operator_statement(&catalogue, &clock, &record(&operator_key())),
        Ok(())
    );
    assert_eq!(
        bind_operator_statement(&catalogue, &clock, &record(&statements_key(RESERVE, 1))),
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
        bind_statement(&catalogue, &clock, &holding(&operator_key())),
        Ok(())
    );
    assert_eq!(
        bind_statement(&catalogue, &clock, &holding(&statements_key(RESERVE, 1))),
        Err(Refusal::Key {
            path: vec![],
            refusal: StatementRefusal::NotSignedByOperator,
        })
    );
}

/// The log's sources over the real catalogue, for core-internal's `attribute`: the bytes the evidence store kept by
/// their commitment, the catalogue's publishers and deployment, and its verification of a statement's key and
/// signature at Core's clock.
struct LogSources<'a> {
    catalogue: &'a rutent_prov::catalogue::Catalogue,
    evidence: &'a KeptEvidence,
    log: &'a KeptLog,
    at: Instant,
}

impl KeptStatements for LogSources<'_> {
    fn kept(&mut self, commitment: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        // The stated entries name their bytes in the order the store kept them.
        let place = self
            .log
            .entries
            .iter()
            .position(|e| matches!(e, Entry::Stated(s) if &s.commitment == commitment));
        Ok(place.map(|i| {
            serde_json::from_value(json!({
                "commitment": commitment,
                "salt": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "bytes": base64url(&self.evidence.kept[i]),
            }))
            .unwrap()
        }))
    }
}

impl StatementKeys for LogSources<'_> {
    fn deployment(&mut self) -> Result<Deployment, AppendFailure> {
        Ok(deployment())
    }

    fn operator(&mut self) -> Result<Id, AppendFailure> {
        Ok(Id::new("operator").unwrap())
    }

    fn publisher(&mut self, maker: &Id) -> Result<Option<Id>, AppendFailure> {
        Ok(
            match self.catalogue.publishers().publisher(maker.as_str()) {
                Some(Publisher::Operator) => Id::new("operator"),
                Some(Publisher::Party(party)) => Id::new(party),
                None => None,
            },
        )
    }

    fn signs_for(&mut self, _: &Id, _: &SignedWith, _: &Timestamp) -> Result<bool, AppendFailure> {
        // The catalogue's verification below ties the key to its maker at its `made_at`.
        Ok(true)
    }
}

impl Signatures for LogSources<'_> {
    fn verifies(&self, received: &[u8]) -> bool {
        self.catalogue.verify_statement(received, self.at).is_ok()
    }
}

fn base64url(raw: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in raw.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// The transaction a statement's bytes sign, where its body names one.
fn signed_transaction(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    Some(value["statement"]["transaction"].as_str()?.to_string())
}

// verifies: REQ-AUD-2
#[test]
fn refused_statement_claiming_its_signature_verified_is_recorded_by_its_own_signature() {
    let catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
    let clock = TestClock::at(4_000);
    let stray = signing_key("stray", 9);
    // Built by hand, claiming a verification its signature doesn't bear out.
    let claimed = RefusedStatement {
        tuple: withdrawal_tuple(RESERVE),
        bytes: dry_run(&deployment(), 3_500, &stray),
        reason: Refusal::NoTuple,
        verified: true,
    };
    let mut log = KeptLog::default();
    let mut evidence = KeptEvidence::default();
    record_refused(&mut evidence, &mut log, &clock, &catalogue, &claimed, None).unwrap();
    // Orchestration never attributes a statement, whatever it is told: it appends it unverified, claimed by the
    // maker its envelope names and about the transaction its bytes sign, and the log, which alone attributes, reads
    // the kept bytes' own signature (core-internal's `attribute`).
    let honest = RefusedStatement {
        bytes: dry_run(&deployment(), 3_500, &statements_key(RESERVE, 1)),
        verified: false,
        ..claimed.clone()
    };
    record_refused(&mut evidence, &mut log, &clock, &catalogue, &honest, None).unwrap();
    let [Entry::Stated(stray), Entry::Stated(signed)] = log.entries.as_slice() else {
        panic!("two stated entries");
    };
    for (stated, statement) in [(stray, &claimed), (signed, &honest)] {
        assert!(matches!(stated.attribution, Attribution::Unverified { .. }));
        assert_eq!(
            stated
                .about
                .as_ref()
                .and_then(|a| a.transaction.as_ref())
                .map(|t| t.as_str()),
            signed_transaction(&statement.bytes).as_deref()
        );
    }

    // The log attributes each by its own signature, over the catalogue: the stray one stays unverified, and the
    // honest one is its maker's, the provider whose key signs, stating for its publisher.
    let (stray, signed) = (stray.clone(), signed.clone());
    let attributed = |claim: &StatedEntry| {
        let mut sources = LogSources {
            catalogue: &catalogue,
            evidence: &evidence,
            log: &log,
            at: clock.now().floor(),
        };
        let mut keys = LogSources { ..sources };
        let signatures = LogSources { ..sources };
        attribute(claim, &[], &mut sources, &mut keys, &signatures).unwrap()
    };
    assert!(matches!(
        attributed(&stray).attribution,
        Attribution::Unverified { .. }
    ));
    let Attribution::Verified { who } = attributed(&signed).attribution else {
        panic!("the honest statement is its maker's");
    };
    assert_eq!(who.acts.as_str(), RESERVE);
    assert_eq!(who.party.as_str(), "acme");
}

// verifies: THR-3, REQ-AUD-4
#[test]
fn statement_naming_a_member_twice_is_refused_before_the_record() {
    let catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
    let key = statements_key(RESERVE, 1);
    let clock = TestClock::at(4_000);
    let good = nesting(&key, &deployment(), 3_400, &key);
    assert_eq!(bind_statement(&catalogue, &clock, &good), Ok(()));
    // The nested judgement names its class twice: a reader could take either.
    let text = String::from_utf8(good).unwrap();
    let once = r#""class":"kyc.natural_person""#;
    assert_eq!(text.matches(once).count(), 1);
    let twice = text.replace(
        once,
        &format!(r#"{once},"class":"aml.sanctions_screening""#),
    );
    let at = twice.find(once).unwrap() + once.len() + 1;
    assert_eq!(
        bind_statement(&catalogue, &clock, twice.as_bytes()),
        Err(Refusal::DuplicateMember { at })
    );
}
