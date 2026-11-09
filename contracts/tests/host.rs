//! The host contract's tests: its guarantees against its README and against the suite PLAN names (`T-005-03`),
//! its schemas against its fixtures and streams, and its Rust types against its schemas (`DEC-4`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use rutent_contracts::state_machine::host::{
    EFFECT_DRIVER, GUARANTEES, RefusalRecord, SnapshotEntry, StreamCheck, TransitionRecord, VERSION,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The host contract's shapes, `urn:rutent:host:<shape>`.
const SHAPES: [&str; 3] = ["transition-record", "refusal-record", "snapshot-entry"];

/// The state-machine shapes the host's schemas refer to by `$id`.
const STATE_MACHINE_SHAPES: [&str; 5] = [
    "order-state",
    "segment-state",
    "segment-plan",
    "recorded-input",
    "order",
];

/// The tests `PLAN.md` names for the host-contract suite (`T-005-03`), each with the guarantee it scores.
const SUITE_TESTS: [(&str, &str); 4] = [
    (
        "crash between record and effect leaves no effect",
        "no-effect-without-record",
    ),
    ("repeated input moves nothing twice", "exactly-once"),
    ("swap mid-order loses no order", "swap"),
    ("replay reaches the same states", "replay"),
];

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn contract_dir() -> PathBuf {
    manifest_dir().join("state-machine/host")
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&read_text(path)).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every schema of another contract the state machine's shapes refer to, by its own `$id`.
fn schemas_of(contract: &str) -> Vec<(String, Value)> {
    let dir = manifest_dir().join(contract).join("schemas");
    let mut out: Vec<(String, Value)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| read_json(&entry.unwrap().path()))
        .map(|s| {
            (
                s["$id"]
                    .as_str()
                    .expect("a schema names its $id")
                    .to_owned(),
                s,
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn validator(shape: &str) -> Validator {
    let mut registry = Registry::new();
    for (id, s) in schemas_of("provider-role")
        .into_iter()
        .chain(schemas_of("router-api"))
    {
        registry = registry
            .add(id.clone(), s)
            .unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    for s in STATE_MACHINE_SHAPES {
        let path = manifest_dir().join(format!("state-machine/schemas/{s}.schema.json"));
        registry = registry
            .add(format!("urn:rutent:state-machine:{s}"), read_json(&path))
            .unwrap_or_else(|e| panic!("{s}: {e}"));
    }
    for s in SHAPES {
        let path = contract_dir().join(format!("schemas/{s}.schema.json"));
        registry = registry
            .add(format!("urn:rutent:host:{s}"), read_json(&path))
            .unwrap_or_else(|e| panic!("{s}: {e}"));
    }
    let registry = registry.prepare().expect("registry");
    let path = contract_dir().join(format!("schemas/{shape}.schema.json"));
    jsonschema::options()
        .with_registry(&registry)
        .should_validate_formats(true)
        .build(&read_json(&path))
        .unwrap_or_else(|e| panic!("{shape}: {e}"))
}

fn shape_of(fixture: &Path) -> &'static str {
    let name = fixture.file_stem().unwrap().to_str().unwrap();
    SHAPES
        .iter()
        .filter(|s| name.starts_with(&format!("{s}-")))
        .max_by_key(|s| s.len())
        .unwrap_or_else(|| panic!("{name} names no shape"))
}

fn fixtures(kind: &str, extension: &str) -> Vec<PathBuf> {
    let dir = contract_dir().join("fixtures").join(kind);
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == extension))
        .collect();
    paths.sort();
    paths
}

fn round_trip<T: Serialize + DeserializeOwned>(value: &Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&typed).map_err(|e| e.to_string())
}

fn through_rust(shape: &str, value: &Value) -> Result<Value, String> {
    match shape {
        "transition-record" => round_trip::<TransitionRecord>(value),
        "refusal-record" => round_trip::<RefusalRecord>(value),
        "snapshot-entry" => round_trip::<SnapshotEntry>(value),
        other => Err(format!("no Rust type for {other}")),
    }
}

/// A snapshot stream file, one JSON value per line.
fn stream_lines(path: &Path) -> Vec<Value> {
    read_text(path)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
        .collect()
}

/// Whether a stream passes: every entry validates and deserialises, and the whole passes [`StreamCheck`].
fn stream_passes(lines: &[Value], schema: &Validator) -> Result<Vec<SnapshotEntry>, String> {
    let mut check = StreamCheck::new();
    let mut entries = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        if let Err(e) = schema.validate(line) {
            return Err(format!("entry {n}: {e}"));
        }
        let entry: SnapshotEntry =
            serde_json::from_value(line.clone()).map_err(|e| format!("entry {n}: {e}"))?;
        check.push(&entry).map_err(|e| format!("entry {n}: {e}"))?;
        entries.push(entry);
    }
    check.finish().map_err(|e| e.to_string())?;
    Ok(entries)
}

/// The ids in a table cell such as "`INV-7`, `NFR-5`".
fn ids(cell: &str) -> Vec<String> {
    cell.split(',')
        .map(|id| id.trim().trim_matches('`').to_owned())
        .filter(|id| !id.is_empty())
        .collect()
}

/// The guarantee rows of the README's table: name and the ids it serves.
fn readme_guarantees() -> Vec<(String, Vec<String>)> {
    let readme = read_text(&contract_dir().join("README.md"));
    readme
        .lines()
        .skip_while(|l| !l.starts_with("| Guarantee |"))
        .skip(2)
        .take_while(|l| l.starts_with('|'))
        .map(|l| {
            let cells: Vec<&str> = l.split('|').collect();
            (cells[1].trim().trim_matches('`').to_owned(), ids(cells[2]))
        })
        .collect()
}

/// The test sentences of `PLAN.md`'s `T-005-03` row.
fn plan_suite_tests() -> Vec<String> {
    let plan = read_text(&manifest_dir().join("../PLAN.md"));
    let row = plan
        .lines()
        .find(|l| l.starts_with("| `T-005-03` |"))
        .expect("PLAN has no T-005-03 row");
    let tests = row.split('|').nth(5).expect("the row has a Tests column");
    let sentences = tests.split('[').next().unwrap();
    sentences
        .split(';')
        .map(|s| s.trim().trim_matches('`').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn guarantee(id: &str) -> &'static rutent_contracts::state_machine::host::Guarantee {
    GUARANTEES
        .iter()
        .find(|g| g.id == id)
        .unwrap_or_else(|| panic!("no `{id}` guarantee"))
}

// verifies: REQ-OPS-3, NFR-6, REQ-STM-4, INV-7
#[test]
fn host_contract_states_delivery_and_swap_guarantees() {
    let published: Vec<(String, Vec<String>)> = GUARANTEES
        .iter()
        .map(|g| {
            (
                g.id.to_owned(),
                g.serves.iter().map(|s| s.to_string()).collect(),
            )
        })
        .collect();
    assert_eq!(
        readme_guarantees(),
        published,
        "the README's table and GUARANTEES differ"
    );

    let cited: Vec<String> = SUITE_TESTS.iter().map(|(t, _)| t.to_string()).collect();
    assert_eq!(
        plan_suite_tests(),
        cited,
        "the cited suite tests have drifted from PLAN"
    );
    for (_, id) in SUITE_TESTS {
        guarantee(id);
    }
    for required in ["durable", "exactly-once", "per-order-sequence", "swap"] {
        guarantee(required);
    }

    let served: BTreeSet<&str> = GUARANTEES
        .iter()
        .flat_map(|g| g.serves.iter().copied())
        .collect();
    for id in ["REQ-OPS-3", "NFR-6", "REQ-STM-4"] {
        assert!(served.contains(id), "no guarantee serves {id}");
    }
    for g in GUARANTEES {
        assert!(
            !g.statement.is_empty() && !g.suite.is_empty(),
            "{} needs a statement and a suite check",
            g.id
        );
    }
}

// verifies: INV-7, NFR-5
#[test]
fn every_crash_point_is_scored_by_the_suite() {
    let crash = guarantee("no-effect-without-record").suite;
    for point in ["after received", "after computed", "after durable"] {
        assert!(
            crash.contains(point),
            "the crash between record and effect doesn't score a stop {point}"
        );
    }
    let durable = guarantee("durable").statement;
    assert!(
        durable.contains("durable but not yet acknowledged"),
        "`durable` doesn't say what happens to the record in flight"
    );
    assert!(
        durable.contains("halted host reopens halted"),
        "`durable` doesn't say that halting survives a reopen"
    );
    assert!(
        durable.contains("atomic"),
        "`durable` doesn't say a write is atomic"
    );
}

// verifies: REQ-STM-4, INV-8
#[test]
fn a_repeat_makes_no_second_record() {
    let once = guarantee("exactly-once").statement;
    assert!(once.contains("at most one record per record id"));
    assert!(once.contains("RFC 8785 (JCS)"));
    assert!(once.contains("recorded_at included"));
    assert!(once.contains("refusal is itself recorded"));
}

// verifies: DEC-4
#[test]
fn the_effect_driver_s_obligation_is_stated_apart_from_the_host_s_guarantees() {
    let readme = read_text(&contract_dir().join("README.md"));
    let quoted: Vec<&str> = readme
        .lines()
        .filter_map(|l| l.strip_prefix("> "))
        .collect();
    assert_eq!(
        quoted.join(" "),
        EFFECT_DRIVER,
        "the README's obligation and EFFECT_DRIVER differ"
    );
    for words in [
        "only from released records",
        "at least one attempt per record id",
        "never repeats an attempt that was confirmed",
        "at most one attempt per record id",
        "never repeated automatically",
        "outcome unknown",
    ] {
        assert!(
            EFFECT_DRIVER.contains(words),
            "the obligation lacks \"{words}\""
        );
    }
    for g in GUARANTEES {
        assert!(
            !g.serves.contains(&"REQ-CAT-10"),
            "`{}` claims REQ-CAT-10, which the host suite doesn't score",
            g.id
        );
    }
}

// verifies: REQ-OPS-3, NFR-6, NFR-4, DEC-4
#[test]
fn every_valid_fixture_validates_and_every_invalid_one_is_rejected() {
    let mut failures = Vec::new();
    for path in fixtures("valid", "json") {
        let shape = shape_of(&path);
        let value = read_json(&path);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if let Err(e) = validator(shape).validate(&value) {
            failures.push(format!("{name} should validate as {shape}: {e}"));
        }
        match through_rust(shape, &value) {
            Ok(back) if back == value => {}
            Ok(back) => failures.push(format!("{name} changes through Rust: {back}")),
            Err(e) => failures.push(format!("{name} doesn't deserialise: {e}")),
        }
    }
    for path in fixtures("invalid", "json") {
        let shape = shape_of(&path);
        if validator(shape).is_valid(&read_json(&path)) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            failures.push(format!("{name} should be rejected as {shape}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// verifies: REQ-OPS-3, NFR-6, REQ-STM-4
#[test]
fn every_valid_stream_passes_and_every_invalid_one_fails() {
    let schema = validator("snapshot-entry");
    let mut failures = Vec::new();
    for path in fixtures("valid", "jsonl") {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if let Err(e) = stream_passes(&stream_lines(&path), &schema) {
            failures.push(format!("{name} should pass: {e}"));
        }
    }
    for path in fixtures("invalid", "jsonl") {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if stream_passes(&stream_lines(&path), &schema).is_ok() {
            failures.push(format!("{name} should fail"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Invalid fixtures the Rust types accept on purpose, each with its reason. Empty: every rule the schemas state is
/// also checked in Rust. A fixture added here needs its reason beside it.
const ACCEPTED_BY_RUST_ON_PURPOSE: [&str; 0] = [];

// verifies: DEC-4
#[test]
fn every_invalid_fixture_is_rejected_by_the_rust_types_too() {
    let mut accepted = Vec::new();
    for path in fixtures("invalid", "json") {
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        if through_rust(shape_of(&path), &read_json(&path)).is_ok()
            && !ACCEPTED_BY_RUST_ON_PURPOSE.contains(&name.as_str())
        {
            accepted.push(name);
        }
    }
    assert!(accepted.is_empty(), "Rust accepts: {}", accepted.join(", "));
}

fn stream_fixture() -> Vec<Value> {
    stream_lines(&contract_dir().join("fixtures/valid/snapshot-stream-of-orders-in-flight.jsonl"))
}

// verifies: REQ-STM-4, INV-8, NFR-4, DEC-4
#[test]
fn rules_the_schemas_cannot_state_are_checked_in_rust() {
    type Break = fn(&mut Vec<Value>);
    let cases: [(&str, Break); 6] = [
        ("a record with a state, then one without", |s| {
            let mut later = s[3].clone();
            later["body"]["seq"] = 3.into();
            later["body"]["input"]["record_id"] = "rec-0003".into();
            later["body"].as_object_mut().unwrap().remove("state_after");
            s.insert(4, later);
            s.last_mut().unwrap()["body"]["records"] = 5.into();
        }),
        ("a record filed under another order than its input's", |s| {
            s[3]["body"]["order_id"] = "ord-3".into();
            s[3]["body"]["state_after"]["order_id"] = "ord-3".into();
        }),
        ("a record whose state is another order's", |s| {
            s[3]["body"]["state_after"]["order_id"] = "ord-3".into();
        }),
        (
            "a record whose state names another input as its last",
            |s| {
                s[3]["body"]["state_after"]["last_input"]["record_id"] = "rec-0001".into();
                s[3]["body"]["state_after"]["last_record"] = "rec-0001".into();
            },
        ),
        ("a record id recorded under two orders", |s| {
            s[5]["body"]["input"]["record_id"] = "rec-0001".into();
            s[5]["body"]["state_after"]["last_record"] = "rec-0001".into();
            s[5]["body"]["state_after"]["last_input"]["record_id"] = "rec-0001".into();
        }),
        ("a gap in the refusal log", |s| {
            s[4]["body"]["seq"] = 2.into();
        }),
    ];
    let schema = validator("snapshot-entry");
    for (case, breaks) in cases {
        let mut stream = stream_fixture();
        breaks(&mut stream);
        for (n, line) in stream.iter().enumerate() {
            assert!(
                schema.is_valid(line),
                "{case}: the schema already rejects entry {n}"
            );
        }
        assert!(
            stream_passes(&stream, &schema).is_err(),
            "{case}: Rust accepts it"
        );
    }
}

// verifies: REQ-OPS-3, NFR-6
#[test]
fn a_snapshot_stream_carries_every_order_refusal_and_record_id_across_a_swap() {
    let schema = validator("snapshot-entry");
    let exported = stream_passes(&stream_fixture(), &schema).unwrap();
    let wire: Vec<String> = exported
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    let imported: Vec<SnapshotEntry> = wire
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(imported, exported);

    let mut orders = BTreeSet::new();
    let mut records = Vec::new();
    let mut refusals = 0;
    for entry in &imported {
        match entry {
            SnapshotEntry::Record(r) => {
                orders.insert(r.order_id.as_str());
                records.push(r.input.record_id.as_str());
            }
            SnapshotEntry::Refusal(_) => refusals += 1,
            _ => {}
        }
    }
    assert_eq!(orders, BTreeSet::from(["ord-1", "ord-3", "ord-9"]));
    assert_eq!(records, ["rec-0001", "rec-0090", "rec-0002", "rec-0031"]);
    assert_eq!(refusals, 1);
}

// verifies: DEC-4
#[test]
fn version_matches_the_changelog() {
    let changelog = read_text(&contract_dir().join("CHANGELOG.md"));
    let heading = changelog
        .lines()
        .find_map(|l| l.strip_prefix("## "))
        .expect("the changelog has no version heading");
    assert_eq!(heading.trim(), VERSION);
}

// verifies: DEC-4
#[test]
fn every_schema_names_draft_2020_12_and_its_urn() {
    for shape in SHAPES {
        let s = read_json(&contract_dir().join(format!("schemas/{shape}.schema.json")));
        assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
        assert_eq!(s["$id"], format!("urn:rutent:host:{shape}").as_str());
    }
}
