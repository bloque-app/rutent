//! The reader contract's tests: its schemas against its fixtures, its Rust types against its schemas, and an answer
//! held to the question it answers (`DEC-4`, `DEC-28`, `T-020-59`).

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::provider_role::{Deployment, ProtocolVersions, PublicKey, Statement};
use rutent_contracts::reader::{
    ANSWER_SHAPE, Answered, Asked, Expected, LEAST_QUORUM, NetworkIdentity, ReaderAnswer,
    ReaderMismatch, ReaderQuestion, RequestId, RpcEndpoint, TrustRule, VERSION, check_pairing,
    check_reader_answer, is_host_name,
};
use rutent_contracts::router_api::{Timestamp, Version};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The contract's shapes; each has `schemas/<shape>.schema.json` and the `$id` `urn:rutent:reader:<shape>`.
const SHAPES: [&str; 3] = ["common", "question", "answer"];

/// The other contracts whose shapes these schemas refer to by `$id`.
const REFERRED: [&str; 1] = ["provider-role"];

/// Invalid fixtures the schemas accept and the Rust types refuse, each with the refusal it must give. Every other
/// invalid fixture is refused by both.
const RUST_RULES: [(&str, &str); 5] = [
    (
        "answer-of-an-rpc-pool-naming-one-host-with-and-without-a-trailing-slash",
        "an rpc pool names each host once",
    ),
    (
        "answer-of-an-rpc-pool-naming-one-host-with-its-default-port",
        "an rpc pool names each host once",
    ),
    (
        "answer-of-an-rpc-pool-naming-one-host-by-two-schemes",
        "an rpc pool names each host once",
    ),
    (
        "answer-of-an-rpc-pool-with-fewer-endpoints-than-its-quorum",
        "an rpc pool has at least as many endpoints as its quorum",
    ),
    (
        "answer-of-a-movements-status-to-a-network-rules-question",
        "an answer answers the kind of question it names, or is unreadable",
    ),
];

fn contracts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn contract_dir() -> PathBuf {
    contracts_dir().join("reader")
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn schema(shape: &str) -> Value {
    read_json(
        &contract_dir()
            .join("schemas")
            .join(format!("{shape}.schema.json")),
    )
}

fn urn(shape: &str) -> String {
    format!("urn:rutent:reader:{shape}")
}

fn schemas_of(contract: &str) -> Vec<(String, Value)> {
    let dir = contracts_dir().join(contract).join("schemas");
    fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| read_json(&entry.unwrap().path()))
        .map(|s| (s["$id"].as_str().expect("an $id").to_owned(), s))
        .collect()
}

fn validator(shape: &str) -> Validator {
    let mut registry = Registry::new();
    let others = REFERRED.iter().flat_map(|c| schemas_of(c));
    for (id, s) in others.chain(SHAPES.iter().map(|s| (urn(s), schema(s)))) {
        registry = registry
            .add(id.clone(), s)
            .unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let registry = registry.prepare().expect("registry");
    jsonschema::options()
        .offline()
        .with_registry(&registry)
        .build(&schema(shape))
        .unwrap_or_else(|e| panic!("{shape}: {e}"))
}

fn shape_of(name: &str) -> &'static str {
    SHAPES
        .iter()
        .filter(|s| name.starts_with(&format!("{s}-")))
        .max_by_key(|s| s.len())
        .unwrap_or_else(|| panic!("{name} names no shape"))
}

fn fixtures(kind: &str) -> Vec<(String, Value)> {
    let dir = contract_dir().join("fixtures").join(kind);
    let mut out: Vec<(String, Value)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            (name, read_json(&p))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn fixture(kind: &str, name: &str) -> Value {
    read_json(
        &contract_dir()
            .join("fixtures")
            .join(kind)
            .join(format!("{name}.json")),
    )
}

fn round_trip<T: Serialize + DeserializeOwned>(value: &Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&typed).map_err(|e| e.to_string())
}

/// The fixture through its Rust type and back. An answer's envelope is held to its shape too, as a reader of it does
/// before trusting it.
fn through_rust(shape: &str, value: &Value) -> Result<Value, String> {
    match shape {
        "question" => round_trip::<ReaderQuestion>(value),
        "answer" => {
            let answer: Statement<ReaderAnswer> =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            answer.check_shape().map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(&answer).map_err(|e| e.to_string())
        }
        other => Err(format!("no Rust type for {other}")),
    }
}

// verifies: DEC-4, DEC-28
#[test]
fn every_valid_reader_fixture_validates_and_every_invalid_one_is_rejected() {
    let valid = fixtures("valid");
    let invalid = fixtures("invalid");
    assert!(!valid.is_empty() && !invalid.is_empty(), "fixtures missing");
    let mut failures = Vec::new();
    for (name, value) in &valid {
        let shape = shape_of(name);
        if let Err(e) = validator(shape).validate(value) {
            failures.push(format!("{name} should validate as {shape}: {e}"));
        }
        match through_rust(shape, value) {
            Ok(back) if &back == value => {}
            Ok(back) => failures.push(format!("{name} changes through Rust: {back}")),
            Err(e) => failures.push(format!("{name} doesn't deserialise: {e}")),
        }
    }
    for (name, value) in &invalid {
        let shape = shape_of(name);
        let schema_accepts = validator(shape).is_valid(value);
        let refused = through_rust(shape, value);
        match RUST_RULES.iter().find(|(n, _)| n == name) {
            Some((_, rule)) => {
                if !schema_accepts {
                    failures.push(format!(
                        "{name} breaks a rule only Rust states, so the schema should accept it"
                    ));
                }
                if !refused.as_ref().is_err_and(|e| e.contains(rule)) {
                    failures.push(format!("{name} not refused by its rule: {refused:?}"));
                }
            }
            None => {
                if schema_accepts {
                    failures.push(format!("{name} should be rejected by the {shape} schema"));
                }
                if refused.is_ok() {
                    failures.push(format!("{name} should be refused by the Rust types"));
                }
            }
        }
    }
    for (name, _) in RUST_RULES {
        if !invalid.iter().any(|(n, _)| n == name) {
            failures.push(format!("missing invalid fixture {name}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// verifies: DEC-4
#[test]
fn every_reader_shape_but_the_shared_atoms_has_a_valid_fixture() {
    let valid = fixtures("valid");
    for shape in SHAPES.iter().filter(|s| **s != "common") {
        assert!(
            valid.iter().any(|(n, _)| shape_of(n) == *shape),
            "{shape} has no valid fixture"
        );
    }
    let on_disk = fs::read_dir(contract_dir().join("schemas"))
        .unwrap()
        .count();
    assert_eq!(on_disk, SHAPES.len(), "a schema file is not a listed shape");
}

// verifies: DEC-4
#[test]
fn reader_version_matches_the_changelog() {
    let changelog = fs::read_to_string(contract_dir().join("CHANGELOG.md")).unwrap();
    let heading = changelog
        .lines()
        .find_map(|l| l.strip_prefix("## "))
        .expect("the changelog has no version heading");
    assert_eq!(heading.trim(), VERSION);
    assert_eq!(VERSION, "1.0.0");
}

// verifies: DEC-4
#[test]
fn every_reader_schema_names_draft_2020_12_and_its_urn() {
    for shape in SHAPES {
        let s = schema(shape);
        assert_eq!(
            s["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "{shape}"
        );
        assert_eq!(s["$id"], urn(shape).as_str(), "{shape}");
    }
    assert_eq!(
        schema("answer")["properties"]["shape"]["const"],
        ANSWER_SHAPE
    );
}

fn question(name: &str) -> ReaderQuestion {
    serde_json::from_value(fixture("valid", name)).unwrap()
}

fn answer(name: &str) -> Statement<ReaderAnswer> {
    serde_json::from_value(fixture("valid", name)).unwrap()
}

/// What Core expects of the fixtures' answers: their deployment, the one protocol version in force, the reader's listed
/// statement key and version; and Core's clock as the answer reaches it, before the question's deadline.
struct Listed {
    deployment: Deployment,
    versions: ProtocolVersions,
    key: PublicKey,
    version: Version,
    now: Timestamp,
}

impl Listed {
    fn expected(&self) -> Expected<'_> {
        Expected {
            deployment: &self.deployment,
            versions: &self.versions,
            key: &self.key,
            version: &self.version,
        }
    }
}

fn listed() -> Listed {
    Listed {
        deployment: serde_json::from_value(
            fixture("valid", "answer-of-a-final-movement")["deployment"].clone(),
        )
        .unwrap(),
        versions: ProtocolVersions::only("4.0.0"),
        key: PublicKey::new("oKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr8").unwrap(),
        version: Version::new("0.1.0").unwrap(),
        now: Timestamp::new("2026-11-27T10:00:02Z").unwrap(),
    }
}

fn held(
    question: &ReaderQuestion,
    answer: &Statement<ReaderAnswer>,
) -> Result<(), Vec<ReaderMismatch>> {
    let listed = listed();
    check_reader_answer(question, answer, &listed.expected(), &listed.now)
}

/// An answer is held to the question Core sent: the same request, the same network, the same kind of question, and
/// the movement or transaction asked about. An answer to another request, or about another movement, transaction or
/// network, or to another kind of question, is refused, whatever it says (`REQ-ORD-39`, `DEC-28`).
// verifies: REQ-ORD-39
#[test]
fn reader_answer_to_another_request_or_movement_than_the_one_asked_is_refused() {
    let status = question("question-for-a-movements-status");
    let final_movement = answer("answer-of-a-final-movement");
    assert_eq!(held(&status, &final_movement), Ok(()));
    let of = question("question-for-the-movement-of-a-transaction");
    assert_eq!(
        held(&of, &answer("answer-of-the-movement-tied-to-a-transaction")),
        Ok(())
    );
    assert_eq!(
        held(&status, &answer("answer-unreadable-to-a-movements-status")),
        Ok(()),
        "an unreadable answer to the question asked holds, and settles nothing"
    );

    // Another request.
    let mut other = final_movement.clone();
    other.statement.request = RequestId::new("1d2e3f405162738495a6b7c8d9eaf0b1").unwrap();
    assert_eq!(
        held(&status, &other),
        Err(vec![ReaderMismatch::AnswersAnotherRequest])
    );
    // Another movement.
    let mut other = final_movement.clone();
    other.statement.asked = Asked::MovementStatus {
        movement: "0x00aa".parse_id(),
    };
    assert_eq!(
        held(&status, &other),
        Err(vec![ReaderMismatch::AboutAnotherMovement])
    );
    // Another transaction.
    let mut other = answer("answer-of-the-movement-tied-to-a-transaction");
    other.statement.asked = Asked::MovementOf {
        transaction: "tx-seg-3".parse_id(),
    };
    assert_eq!(
        held(&of, &other),
        Err(vec![ReaderMismatch::AboutAnotherTransaction])
    );
    // Another network.
    let mut other = final_movement.clone();
    other.statement.network = "kusama".parse_id();
    assert_eq!(
        held(&status, &other),
        Err(vec![ReaderMismatch::AboutAnotherNetwork])
    );
    // Another kind of question: the movement's answer to a question for the network's rules, and the rules to a
    // movement's question.
    let rules = question("question-for-a-networks-rules");
    assert_eq!(
        held(&rules, &final_movement),
        Err(vec![ReaderMismatch::AnswersAnotherQuestion])
    );
    assert_eq!(
        held(&status, &answer("answer-of-a-light-client-networks-rules")),
        Err(vec![ReaderMismatch::AnswersAnotherQuestion])
    );
    // And the answer's own kind is held to the question it names: a movement's standing under a rules question is
    // refused as it is read.
    assert!(
        serde_json::from_value::<Statement<ReaderAnswer>>(fixture(
            "invalid",
            "answer-of-a-movements-status-to-a-network-rules-question"
        ))
        .is_err()
    );
}

/// An answer is held to the reader the listing names: its maker, its statement key and its version; and to the
/// question's deadline on Core's own clock, never the host's: the answer's `made_at` is not read; to the envelope's
/// deployment and protocol version; and to its own rules, built in memory as read (`DEC-28`, `REQ-AUD-4`,
/// `REQ-PRT-1`).
// verifies: DEC-28, REQ-AUD-4, REQ-PRT-1
#[test]
fn reader_answer_from_another_reader_key_or_version_or_past_its_deadline_is_refused() {
    let status = question("question-for-a-movements-status");
    let good = answer("answer-of-a-final-movement");
    let listed = listed();
    let at_time = |expected: &Expected<'_>, answer: &Statement<ReaderAnswer>, now: &str| {
        check_reader_answer(&status, answer, expected, &Timestamp::new(now).unwrap())
    };
    let now = listed.now.as_str();
    let mut other = good.clone();
    other.maker = "kusama-reader".to_owned();
    assert_eq!(
        held(&status, &other),
        Err(vec![ReaderMismatch::AnsweredByAnotherReader])
    );
    let another_key = PublicKey::new("AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA").unwrap();
    assert_eq!(
        at_time(
            &Expected {
                key: &another_key,
                ..listed.expected()
            },
            &good,
            now
        ),
        Err(vec![ReaderMismatch::SignedWithAnotherKey])
    );
    let another_version = Version::new("0.2.0").unwrap();
    assert_eq!(
        at_time(
            &Expected {
                version: &another_version,
                ..listed.expected()
            },
            &good,
            now
        ),
        Err(vec![ReaderMismatch::OfAnotherVersion])
    );
    let mut shaped = good.clone();
    shaped.shape = "urn:rutent:provider-role:result".to_owned();
    assert_eq!(
        held(&status, &shaped),
        Err(vec![ReaderMismatch::NotAnAnswer])
    );
    // The envelope's deployment and protocol version (`REQ-PRT-1`, the review's L4).
    let mut elsewhere = good.clone();
    elsewhere.deployment.name = "another-deployment".to_owned();
    assert_eq!(
        held(&status, &elsewhere),
        Err(vec![ReaderMismatch::OfAnotherDeployment])
    );
    let mut later = good.clone();
    later.protocol_version = "9.9.9".to_owned();
    assert_eq!(
        held(&status, &later),
        Err(vec![ReaderMismatch::UnderAProtocolVersionNotInForce])
    );
    // A value built in memory is held to the answer's own rules, as one read from the wire is: a network's rules
    // answering a movement's question it names.
    let mut built = good.clone();
    built.statement.answer = answer("answer-of-a-light-client-networks-rules")
        .statement
        .answer;
    assert!(built.statement.check().is_err());
    assert_eq!(
        held(&status, &built),
        Err(vec![ReaderMismatch::BreaksItsShape])
    );
    // The deadline, on Core's clock: at it the answer is taken, after it refused.
    let at = "2026-11-27T10:00:05Z";
    let late = "2026-11-27T10:00:05.000000001Z";
    assert_eq!(at_time(&listed.expected(), &good, at), Ok(()));
    assert_eq!(
        at_time(&listed.expected(), &good, late),
        Err(vec![ReaderMismatch::PastItsDeadline])
    );
    // The host's clock decides nothing: an answer it says it made after the deadline, reaching Core before it, is
    // taken, and one it says it made long before, reaching Core after it, is refused.
    let mut host_late = good.clone();
    host_late.made_at = "2026-11-27T11:00:00Z".to_owned();
    assert_eq!(at_time(&listed.expected(), &host_late, now), Ok(()));
    let mut host_early = good.clone();
    host_early.made_at = "2026-11-27T09:00:00Z".to_owned();
    assert_eq!(
        at_time(&listed.expected(), &host_early, late),
        Err(vec![ReaderMismatch::PastItsDeadline])
    );
}

/// A pool's quorum is at least two, and a pool names at least as many endpoints as its quorum, each once (item
/// `reader-answer-trust`, `DEC-28`): a quorum below two, or fewer endpoints than the quorum, is refused, by the schema
/// and the Rust types where the schema can state it, and by the Rust types alone where it can't.
// verifies: DEC-28
#[test]
fn rpc_pool_trust_rule_with_a_quorum_below_two_is_refused() {
    assert_eq!(LEAST_QUORUM, 2);
    let endpoints = |n: usize| -> Vec<RpcEndpoint> {
        (0..n)
            .map(|i| RpcEndpoint::new(format!("https://rpc-{i}.base.example")).unwrap())
            .collect()
    };
    for quorum in [0, 1] {
        let rule = TrustRule::RpcPool {
            endpoints: endpoints(3),
            quorum,
        };
        assert!(rule.check().is_err(), "a quorum of {quorum} is refused");
        let wire =
            serde_json::json!({"rule": "rpc_pool", "endpoints": endpoints(3), "quorum": quorum});
        assert!(serde_json::from_value::<TrustRule>(wire).is_err());
    }
    let two_of_three = TrustRule::RpcPool {
        endpoints: endpoints(3),
        quorum: 2,
    };
    assert_eq!(two_of_three.check(), Ok(()));
    let three_of_two = TrustRule::RpcPool {
        endpoints: endpoints(2),
        quorum: 3,
    };
    assert!(
        three_of_two.check().is_err(),
        "fewer endpoints than its quorum"
    );
    let twice = TrustRule::RpcPool {
        endpoints: vec![
            endpoints(1)[0].clone(),
            endpoints(1)[0].clone(),
            endpoints(2)[1].clone(),
        ],
        quorum: 2,
    };
    assert!(twice.check().is_err(), "an endpoint named twice");
    assert_eq!(
        TrustRule::LightClient(rutent_contracts::provider_role::NoDetail {}).check(),
        Ok(())
    );
    // The fixtures, by both.
    for name in [
        "answer-of-an-rpc-pool-with-a-quorum-of-one",
        "answer-of-an-rpc-pool-of-one-endpoint",
        "answer-of-an-rpc-pool-naming-an-endpoint-twice",
    ] {
        let value = fixture("invalid", name);
        assert!(!validator("answer").is_valid(&value), "{name}: the schema");
        assert!(through_rust("answer", &value).is_err(), "{name}: Rust");
    }
    let value = fixture(
        "invalid",
        "answer-of-an-rpc-pool-with-fewer-endpoints-than-its-quorum",
    );
    assert!(through_rust("answer", &value).is_err());
}

/// Core draws each question's request id at random, never from a counter, so a host can't compute an answer before it
/// is asked; and each question carries the deadline its answer must meet on Core's clock (`DEC-28`'s conditions (b)
/// and (c)). An id from a counter, one of fewer bits, and a question with no id or no deadline are refused.
// verifies: DEC-28
#[test]
fn reader_question_carries_a_request_id_drawn_at_random_and_a_deadline() {
    let asked = question("question-for-a-movements-status");
    assert_eq!(asked.request.as_str().len(), 32);
    assert_eq!(asked.deadline.as_str(), "2026-11-27T10:00:05Z");
    // 128 bits from a random source make an id; the top 64 bits all zero, as a counter's, make none.
    let drawn = RequestId::from_random([
        0x9f, 0x3c, 0x2a, 0x7b, 0xe1, 0x4d, 0x4c, 0x0e, 0x8a, 0x51, 0xd6, 0xb0, 0x7e, 0x2f, 0x9c,
        0x13,
    ])
    .unwrap();
    assert_eq!(drawn, asked.request);
    let mut counter = [0u8; 16];
    counter[15] = 7;
    assert_eq!(RequestId::from_random(counter), None);
    for text in [
        "00000000000000000000000000000007",
        "0000000000000000ffffffffffffffff",
        "req-0001",
        "9f3c2a7be14d4c0e",
        "9F3C2A7BE14D4C0E8A51D6B07E2F9C13",
    ] {
        assert_eq!(RequestId::new(text), None, "{text}");
    }
    for name in [
        "question-under-a-counter-request-id",
        "question-under-a-request-id-that-is-not-hex",
        "question-under-a-request-id-of-64-bits",
        "question-without-a-request-id",
        "question-without-a-deadline",
    ] {
        let value = fixture("invalid", name);
        assert!(
            !validator("question").is_valid(&value),
            "{name}: the schema"
        );
        assert!(through_rust("question", &value).is_err(), "{name}: Rust");
    }
}

/// A pool's quorum counts hosts, not spellings (the review of `T-020-59`, M2): one host named with and without a
/// trailing `/`, with its default port, by two schemes, or by two paths, is one node agreeing with itself, and is
/// refused; a pool of distinct hosts, on any ports, is taken.
// verifies: DEC-28
#[test]
fn rpc_pool_quorum_counts_distinct_hosts_not_spellings() {
    let pool = |endpoints: &[&str]| -> Result<TrustRule, String> {
        let wire = serde_json::json!({"rule": "rpc_pool", "endpoints": endpoints, "quorum": 2});
        serde_json::from_value::<TrustRule>(wire).map_err(|e| e.to_string())
    };
    for pair in [
        ["https://rpc-a.base.example", "https://rpc-a.base.example/"],
        [
            "https://rpc-a.base.example",
            "https://rpc-a.base.example:443",
        ],
        ["https://rpc-a.base.example", "wss://rpc-a.base.example"],
        [
            "https://rpc-a.base.example",
            "https://rpc-a.base.example:8545",
        ],
        [
            "https://rpc-a.base.example/a",
            "https://rpc-a.base.example/b",
        ],
        [
            "https://rpc-a.base.example/",
            "https://rpc-a.base.example//",
        ],
    ] {
        assert!(pool(&pair).is_err(), "{pair:?} is one host");
    }
    for pair in [
        ["https://rpc-a.base.example", "https://rpc-b.base.example"],
        [
            "https://rpc-a.base.example:8545",
            "wss://rpc-b.base.example/",
        ],
    ] {
        assert!(pool(&pair).is_ok(), "{pair:?} are two hosts");
    }
    assert_eq!(
        RpcEndpoint::new("wss://rpc-a.base.example:8545/")
            .unwrap()
            .host(),
        "rpc-a.base.example"
    );
    for name in [
        "answer-of-an-rpc-pool-naming-one-host-with-and-without-a-trailing-slash",
        "answer-of-an-rpc-pool-naming-one-host-with-its-default-port",
        "answer-of-an-rpc-pool-naming-one-host-by-two-schemes",
    ] {
        let refused = through_rust("answer", &fixture("invalid", name));
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.contains("an rpc pool names each host once")),
            "{name}: {refused:?}"
        );
    }
}

/// A network's identity and its trust rule are paired (item `reader-answer-trust`, the review of `T-020-59`, M1): an
/// EVM network, by its chain id, is read from an RPC pool with a quorum, and a Substrate network, by its genesis hash,
/// by a light client; the two other pairs are refused, so no EVM chain skips its quorum.
// verifies: DEC-28, REQ-ORD-39
#[test]
fn network_identity_is_paired_with_its_trust_rule() {
    let rules = |name: &str| match answer(name).statement.answer {
        Answered::NetworkRules { rules } => rules,
        other => panic!("{name}: {other:?}"),
    };
    let light = rules("answer-of-a-light-client-networks-rules");
    let pool = rules("answer-of-an-rpc-pool-networks-rules");
    assert!(matches!(
        light.identity,
        NetworkIdentity::GenesisHash { .. }
    ));
    assert!(matches!(pool.identity, NetworkIdentity::ChainId { .. }));
    assert_eq!(check_pairing(&light.identity, &light.trust), Ok(()));
    assert_eq!(check_pairing(&pool.identity, &pool.trust), Ok(()));
    assert!(
        check_pairing(&pool.identity, &light.trust).is_err(),
        "a chain id read by a light client"
    );
    assert!(
        check_pairing(&light.identity, &pool.trust).is_err(),
        "a genesis hash read from a pool"
    );
    for name in [
        "answer-of-a-chain-id-read-by-a-light-client",
        "answer-of-a-genesis-hash-read-from-an-rpc-pool",
    ] {
        let value = fixture("invalid", name);
        assert!(!validator("answer").is_valid(&value), "{name}: the schema");
        let refused = through_rust("answer", &value);
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.contains("a chain id is read from an rpc pool")),
            "{name}: {refused:?}"
        );
    }
}

/// An RPC endpoint names a host by its DNS name and nothing that could be a key (the review of `T-020-59`, L1 and L2):
/// no IP literal, no single-label host, no user, path, query or fragment; a key or a path a node needs is the reader
/// host's to resolve by name through the egress layer's secret lookup.
// verifies: DEC-28, REQ-MDL-6
#[test]
fn rpc_endpoint_names_a_host_and_nothing_that_could_be_a_key() {
    for text in [
        "https://rpc-a.base.example",
        "https://rpc-a.base.example/",
        "wss://rpc-a.base.example:8546",
        "https://1rpc.io",
    ] {
        assert!(RpcEndpoint::new(text).is_some(), "{text}");
    }
    for text in [
        "https://169.254.169.254",
        "https://127.0.0.1:8545",
        "https://1.2",
        "https://localhost",
        "https://key@rpc-a.base.example",
        "https://rpc-a.base.example/v3/0123456789abcdef0123456789abcdef",
        "https://rpc-a.base.example/a",
        "https://rpc-a.base.example/?apikey=k",
        "https://rpc-a.base.example/#k",
        "https://rpc-a.base.example//",
    ] {
        assert_eq!(RpcEndpoint::new(text), None, "{text}");
    }
    assert!(is_host_name("readers.rutent.internal") && !is_host_name("10.0.0.1"));
    for name in [
        "answer-of-an-rpc-pool-endpoint-at-an-ip-literal",
        "answer-of-an-rpc-pool-endpoint-at-a-single-label-host",
        "answer-of-an-rpc-pool-endpoint-with-a-path",
        "answer-of-an-rpc-pool-endpoint-naming-a-key-in-its-path",
        "answer-of-an-rpc-pool-endpoint-with-a-fragment",
        "answer-of-an-rpc-pool-endpoint-with-a-query",
        "answer-of-an-rpc-pool-endpoint-naming-a-user",
    ] {
        let value = fixture("invalid", name);
        assert!(!validator("answer").is_valid(&value), "{name}: the schema");
        assert!(through_rust("answer", &value).is_err(), "{name}: Rust");
    }
}

/// Builds an identifier from text in a test.
trait ParseId {
    fn parse_id(&self) -> rutent_contracts::router_api::Id;
}

impl ParseId for str {
    fn parse_id(&self) -> rutent_contracts::router_api::Id {
        rutent_contracts::router_api::Id::new(self).unwrap()
    }
}
