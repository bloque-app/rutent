//! `T-025-03`'s scan, on the reference host: what it seeks and how, and that it can see. The same scan runs on three
//! VOS replicas in `rutent-scenarios-vos`, with the nodes' own files added to what it looks in.

use rutent_contracts::module_interface::SecretValue;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::secrets::{
    Canary, DECISION_FIELDS, Kind, MIN_LEN, Planted, Scan, Surface, opening_for, run_balanced,
    run_leaking, run_planted,
};

fn text(kind: Kind, value: &str) -> Canary {
    Canary::text(format!("{kind:?} value"), kind, value)
}

fn surface(bytes: &[u8]) -> Vec<Surface> {
    vec![Surface {
        name: "somewhere".to_owned(),
        bytes: bytes.to_vec(),
    }]
}

// verifies: THR-79, REQ-AUD-6
#[test]
fn scan_finds_a_value_in_each_form_a_record_writes_it() {
    let value = "rutent-canary: a \"quoted\" secret\\path";
    let bytes = value.as_bytes();
    let scan = Scan::new(vec![text(Kind::Credential, value)]).expect("a scan");
    // Each form written here, not by the scan's own `forms`, so a form the scan drops is a test that fails.
    let escaped = serde_json::to_string(value).expect("JSON");
    let lower: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let percent: String = bytes.iter().map(|b| format!("%{b:02X}")).collect();
    let mut written: Vec<(String, Vec<u8>)> = vec![
        ("as it is".to_owned(), bytes.to_vec()),
        ("escaped as JSON".to_owned(), escaped.into_bytes()),
        ("in lower-case hex".to_owned(), lower.clone().into_bytes()),
        (
            "in upper-case hex".to_owned(),
            lower.to_uppercase().into_bytes(),
        ),
        (
            "as a JSON array of bytes".to_owned(),
            serde_json::to_vec(&bytes.to_vec()).expect("JSON"),
        ),
        ("percent-encoded".to_owned(), percent.into_bytes()),
    ];
    // Inside a longer string, at each alignment base64 can fall on.
    for lead in 0..3 {
        for alphabet in [base64_of as fn(&[u8]) -> String, base64url_of] {
            let mut padded = vec![b'x'; lead];
            padded.extend_from_slice(bytes);
            padded.extend_from_slice(b"yz");
            written.push((
                format!("base64, after {lead} bytes"),
                alphabet(&padded).into_bytes(),
            ));
        }
    }
    assert!(written.len() >= 12, "forms: {}", written.len());
    for (name, haystack) in written {
        let mut around = b"{\"before\":\"".to_vec();
        around.extend_from_slice(&haystack);
        around.extend_from_slice(b"\",\"after\":1}");
        assert!(
            !scan.find(&surface(&around)).is_empty(),
            "the scan didn't find the value {name}"
        );
    }
    assert!(
        scan.find(&surface(b"nothing of the sort is here"))
            .is_empty(),
        "the scan found a value in a record that holds none"
    );
}

fn base64_of(bytes: &[u8]) -> String {
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    encode(bytes, table)
}

fn base64url_of(bytes: &[u8]) -> String {
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    encode(bytes, table)
}

fn encode(bytes: &[u8], table: &[u8; 64]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(table[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

// verifies: THR-79
#[test]
fn scan_with_nothing_to_seek_or_nothing_to_look_in_is_refused() {
    assert!(Scan::new(Vec::new()).is_err(), "no value");
    assert!(
        Scan::new(vec![text(Kind::Secret, &"x".repeat(MIN_LEN - 1))]).is_err(),
        "a value under {MIN_LEN} bytes"
    );
    let twice = vec![
        text(Kind::Secret, "rutent-canary-one"),
        text(Kind::Secret, "rutent-canary-two"),
    ];
    assert!(Scan::new(twice).is_err(), "two values of one name");
    let scan = Scan::new(vec![text(Kind::Secret, "rutent-canary-one")]).expect("a scan");
    assert!(scan.check(&[]).is_err(), "no surface");
    assert!(scan.check(&surface(b"")).is_err(), "an empty surface");
    assert!(scan.check(&surface(b"something")).is_ok());
}

// verifies: THR-79, REQ-AUD-6
#[test]
fn reference_run_after_planting_holds_no_credential_secret_or_module_state() {
    for seed in [1, 2] {
        let scanned = run_planted(&mut Reference, seed);
        let scan = Scan::new(scanned.canaries.clone()).expect("a scan");
        assert_eq!(
            scan.kinds().len(),
            3,
            "seed {seed}: the scan seeks a credential, a secret and module state"
        );
        let report = scan
            .check(&scanned.surfaces)
            .unwrap_or_else(|hits| panic!("seed {seed}: {}", hits.join("; ")));
        // It looked at the host's answers and at the log, not at nothing.
        let named = |part: &str| report.looked_at.iter().any(|(n, _)| n.contains(part));
        assert!(named("its snapshot") && named("journal of") && named("Core's log"));
        assert!(named("the log read back") && named("the inputs fed"));
        assert!(named("evidence shelf"), "the evidence shelf wasn't read");
    }
}

// verifies: THR-79, REQ-AUD-6
#[test]
fn scan_finds_each_kind_of_value_let_through_the_state_machines_boundary() {
    for kind in [Kind::Credential, Kind::Secret, Kind::ModuleState] {
        let scanned = run_leaking(&mut Reference, 3, kind);
        let scan = Scan::new(scanned.canaries.clone()).expect("a scan");
        let hits = scan.find(&scanned.surfaces);
        let of_kind: Vec<_> = hits.iter().filter(|h| h.kind == kind).collect();
        assert!(
            of_kind.iter().any(|h| h.surface.contains("journal of"))
                && of_kind.iter().any(|h| h.surface.contains("its snapshot")),
            "{kind:?}: the scan didn't find the value in the host's journal and snapshot: {hits:?}"
        );
        // And nothing of another kind: what was let through is the one value.
        assert!(
            hits.iter().all(|h| h.kind == kind),
            "{kind:?}: a value of another kind was found too: {hits:?}"
        );
    }
}

// verifies: THR-79, REQ-AUD-6
#[test]
fn scan_finds_a_secret_written_with_its_redaction_off() {
    let plaintext = "rutent-canary-credential-redaction";
    let value = SecretValue::new(plaintext).expect("a secret");
    let scan = Scan::new(vec![text(Kind::Credential, plaintext)]).expect("a scan");
    // As a record would carry it by `Debug`: redacted, and the scan finds nothing.
    let redacted = format!("an answer: {value:?}");
    assert!(scan.check(&surface(redacted.as_bytes())).is_ok());
    // With the redaction off, as a log line that wrote the text itself would.
    let unredacted = format!("an answer: {}", value.as_str());
    assert_eq!(scan.find(&surface(unredacted.as_bytes())).len(), 1);
}

// verifies: THR-79
#[test]
fn scan_of_a_directory_finds_a_value_in_a_file_and_across_a_chunk_boundary() {
    let dir = std::env::temp_dir().join(format!("rsv-secrets-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("node-1/data")).expect("a directory");
    let value = "rutent-canary-credential-on-a-disk";
    let scan = Scan::new(vec![text(Kind::Credential, value)]).expect("a scan");
    // Straddling the 1 MiB chunk the scan reads in.
    let mut file = vec![b'.'; (1 << 20) - 10];
    file.extend_from_slice(value.as_bytes());
    file.extend_from_slice(&[b'.'; 100]);
    std::fs::write(dir.join("node-1/data/frames"), &file).expect("a file");
    std::fs::write(dir.join("node-1/other"), b"nothing here at all").expect("a file");
    let found = scan.find_in_dir(&dir).expect("readable");
    assert_eq!((found.files, found.bytes), (2, file.len() as u64 + 19));
    assert_eq!(found.hits.len(), 1, "{:?}", found.hits);
    assert!(found.hits[0].surface.ends_with("frames"));
    // The same tree with the value gone: nothing found, and it still says what it read.
    std::fs::write(dir.join("node-1/data/frames"), vec![b'.'; 1 << 21]).expect("a file");
    let clean = scan.find_in_dir(&dir).expect("readable");
    assert!(clean.hits.is_empty() && clean.files == 2 && clean.bytes > 1 << 21);
    let _ = std::fs::remove_dir_all(&dir);
}

// verifies: THR-79
#[test]
fn run_records_where_module_state_sat() {
    let scanned = run_planted(&mut Reference, 1);
    assert!(
        scanned
            .module_state
            .iter()
            .any(|(provider, key)| provider == rutent_scenarios::kit::RESERVE
                && key == "private/notes"),
        "the planted entry of module state isn't in the platform's world: {:?}",
        scanned.module_state
    );
}

/// Every object of `value` that holds both of `keys`.
fn objects_with<'a>(
    value: &'a serde_json::Value,
    keys: [&str; 2],
    found: &mut Vec<&'a serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(map) => {
            if keys.iter().all(|k| map.contains_key(*k)) {
                found.push(value);
            }
            map.values().for_each(|v| objects_with(v, keys, found));
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| objects_with(v, keys, found)),
        _ => {}
    }
}

// verifies: THR-79, REQ-AUD-6
#[test]
fn run_looks_the_planted_credential_up_and_reads_the_private_state_it_holds() {
    let scanned = run_planted(&mut Reference, 1);
    let log = serde_json::to_value(&scanned.run.log).expect("the log is JSON");
    // The credential: each lookup of `api-key` the log records was resolved, and there is at least one.
    let mut lookups = Vec::new();
    objects_with(&log, ["lookup", "outcome"], &mut lookups);
    let lookups: Vec<_> = lookups
        .into_iter()
        .filter(|l| l.to_string().contains("api-key"))
        .collect();
    assert!(
        !lookups.is_empty(),
        "the log holds no record of a lookup of api-key"
    );
    for lookup in &lookups {
        assert_eq!(
            lookup["outcome"]["status"], "resolved",
            "a lookup of api-key was refused, so the value never travelled: {lookup}"
        );
    }
    // The private state: a read of its key, which the module found.
    let mut reads = Vec::new();
    objects_with(&log, ["key", "found"], &mut reads);
    let reads: Vec<_> = reads
        .into_iter()
        .filter(|r| r["key"] == "private/notes")
        .collect();
    assert!(
        !reads.is_empty(),
        "the log holds no record of a read of private/notes"
    );
    assert!(
        reads.iter().all(|r| r["found"] == true),
        "a read of private/notes found nothing, so the value never travelled: {reads:?}"
    );
}

// verifies: THR-79
#[test]
fn leaked_input_carries_the_value_in_its_payload_and_not_only_in_its_ids() {
    let scanned = run_planted(&mut Reference, 1);
    let leaked =
        rutent_scenarios::secrets::leaked_input(&scanned.run.inputs, "rutent-canary-payload");
    let payload = serde_json::to_string(&leaked.observed).expect("JSON");
    assert!(
        payload.contains("rutent-canary-payload"),
        "the value isn't in the input's payload: {payload}"
    );
}

/// The surfaces that are the evidence store's shelf: the one place a holding account's balance reading may be kept.
fn is_shelf(surface: &Surface) -> bool {
    surface.name.contains("evidence shelf")
}

// verifies: REQ-AUD-6
#[test]
fn full_runs_records_hold_no_simulated_balance_outside_the_evidence_store() {
    for seed in [1, 2] {
        let run = run_balanced(&mut Reference, seed, None);
        let scan = Scan::new(run.scanned.canaries.clone()).expect("a scan");
        assert_eq!(
            scan.kinds(),
            std::collections::BTreeSet::from([Kind::Balance]),
            "seed {seed}: the scan seeks balances"
        );
        // The values are the ones the accounts held, read from them, and no figure the run states is one of them.
        let names = scan.names();
        assert!(
            names.iter().any(|n| n.contains("holding account"))
                && names
                    .iter()
                    .any(|n| n.contains("reserve") && n.contains("at the start"))
                && names
                    .iter()
                    .any(|n| n.contains("reserve") && n.contains("at the end")),
            "seed {seed}: {names:?}"
        );
        assert_eq!(
            run.holding,
            run.landed
                .iter()
                .map(|(_, a)| a.parse::<u128>().expect("a figure"))
                .sum::<u128>(),
            "seed {seed}: the holding account's balance is what the wallet was given"
        );
        for canary in &run.scanned.canaries {
            let text = String::from_utf8(canary.bytes.clone()).expect("a figure");
            assert!(
                text.len() >= MIN_LEN && text.bytes().all(|b| b.is_ascii_digit()),
                "seed {seed}: `{}` is not a distinctive figure",
                canary.name
            );
            assert!(
                !run.stated.contains(&text),
                "seed {seed}: `{}` is a figure the run states",
                canary.name
            );
        }
        // Every surface but the evidence store's shelf is clean, and the scan says what it looked at.
        let (shelf, outside): (Vec<Surface>, Vec<Surface>) =
            run.scanned.surfaces.iter().cloned().partition(is_shelf);
        assert!(!shelf.is_empty(), "seed {seed}: the shelf wasn't read");
        // The reading is kept there, so a clean scan of the rest is not a scan of a run that kept nothing.
        assert!(
            !scan.find(&shelf).is_empty(),
            "seed {seed}: the holding account's reading isn't in the evidence store"
        );
        let report = scan
            .check(&outside)
            .unwrap_or_else(|hits| panic!("seed {seed}: {}", hits.join("; ")));
        let named = |part: &str| report.looked_at.iter().any(|(n, _)| n.contains(part));
        assert!(named("its snapshot") && named("journal of") && named("Core's log, record"));
        assert!(named("the log read back") && named("the inputs fed"));
        assert!(
            named("platform's journal of") && named("requests the platform delivered"),
            "seed {seed}: the platform's records weren't read"
        );
        assert!(
            named(&format!(
                "platform's journal of {}",
                rutent_scenarios::kit::OPERATOR_WALLET
            )),
            "seed {seed}: the wallet's journal wasn't read"
        );
    }
}

// verifies: REQ-AUD-6
#[test]
fn balance_planted_in_the_log_is_found_by_the_scan() {
    let clean = run_balanced(&mut Reference, 1, None);
    let scan = Scan::new(clean.scanned.canaries.clone()).expect("a scan");
    // Each figure once: those the accounts held at several moments are one figure to plant.
    let mut once: Vec<&Canary> = Vec::new();
    for canary in &clean.scanned.canaries {
        if !once.iter().any(|c| c.bytes == canary.bytes) {
            once.push(canary);
        }
    }
    for canary in once {
        let value = String::from_utf8(canary.bytes.clone()).expect("a figure");
        for (planted, surface) in [
            (Planted::InLog(value.clone()), "Core's log, record"),
            (Planted::InLog(value.clone()), "the log read back"),
            (Planted::InJournal(value.clone()), "journal of"),
        ] {
            let run = run_balanced(&mut Reference, 1, Some(&planted));
            let hits = scan.find(&run.scanned.surfaces);
            let outside: Vec<_> = hits
                .iter()
                .filter(|h| !h.surface.contains("evidence shelf"))
                .collect();
            assert!(
                outside
                    .iter()
                    .any(|h| h.canary == canary.name && h.surface.contains(surface)),
                "`{}` planted ({planted:?}) wasn't found in a surface named `{surface}`: {outside:?}",
                canary.name
            );
            // And it is the planted figure that is found, not another, outside the shelf: a reserve's balance at
            // the start and at the end are one figure where its round trip left it as it was.
            let figure = |name: &str| {
                clean
                    .scanned
                    .canaries
                    .iter()
                    .find(|c| c.name == name)
                    .map(|c| c.bytes.clone())
            };
            assert!(
                outside
                    .iter()
                    .all(|h| figure(&h.canary) == Some(canary.bytes.clone())),
                "another figure was found too: {outside:?}"
            );
        }
    }
}

// verifies: REQ-AUD-6
#[test]
fn holding_accounts_balance_reading_is_found_only_in_the_evidence_store() {
    let run = run_balanced(&mut Reference, 1, None);
    let scan = Scan::new(run.scanned.canaries.clone()).expect("a scan");
    let hits = scan.find(&run.scanned.surfaces);
    let holding = |h: &&rutent_scenarios::secrets::Hit| h.canary.contains("holding account");
    let of_holding: Vec<_> = hits.iter().filter(holding).collect();
    assert!(
        !of_holding.is_empty()
            && of_holding
                .iter()
                .all(|h| h.surface.contains("evidence shelf")),
        "the reading is kept in the evidence store and nowhere else: {hits:?}"
    );
    assert_eq!(
        hits.len(),
        of_holding.len(),
        "nothing else is found anywhere: {hits:?}"
    );
    // The log holds the check the reading was compared by, with the account it was of, and not the figure.
    let log = serde_json::to_string(&run.scanned.run.log).expect("the log is JSON");
    assert!(
        log.contains("holding_check") && log.contains(&format!("holding-{}", run.node)),
        "the log holds no record of the check"
    );
    // Not the figure, but for the difference a raise states where Core's own segments rest nothing at the node, which is
    // a figure of the raise and not a reading (`T-020-63`).
    let mut whole = serde_json::to_value(&run.scanned.run.log).expect("the log is JSON");
    whole
        .as_array_mut()
        .expect("a list")
        .iter_mut()
        .for_each(rutent_scenarios::secrets::without_the_difference);
    let without_differences = serde_json::to_string(&whole).expect("JSON");
    assert!(!without_differences.contains(&run.holding.to_string()));
}

/// What a surface is, by the part of its name that says so.
fn named<'a>(surfaces: &'a [Surface], part: &str) -> Vec<&'a Surface> {
    surfaces.iter().filter(|s| s.name.contains(part)).collect()
}

/// The figures a scan seeks, each once, with the first name it goes by.
fn distinct(canaries: &[Canary]) -> Vec<(String, String)> {
    let mut figures: Vec<(String, String)> = Vec::new();
    for canary in canaries {
        let value = String::from_utf8(canary.bytes.clone()).expect("a figure");
        if !figures.iter().any(|(_, v)| *v == value) {
            figures.push((canary.name.clone(), value));
        }
    }
    figures
}

// verifies: REQ-AUD-6
#[test]
fn decision_records_hold_no_simulated_balance() {
    for seed in [1, 2] {
        let run = run_balanced(&mut Reference, seed, None);
        let scan = Scan::new(run.scanned.canaries.clone()).expect("a scan");
        // The records Core signs, each as signed: the log holds only their digests, so this is the one place to read them.
        let records: Vec<Surface> = named(&run.scanned.surfaces, "Core's decision record")
            .into_iter()
            .cloned()
            .collect();
        assert!(
            records.len() >= run.scanned.run.orders.len(),
            "seed {seed}: the scan read {} decision records for {} orders",
            records.len(),
            run.scanned.run.orders.len()
        );
        for record in &records {
            let text = String::from_utf8_lossy(&record.bytes);
            assert!(
                text.contains("router_version")
                    && text.contains("candidates")
                    && text.contains("signature"),
                "seed {seed}: `{}` is no decision record, as signed",
                record.name
            );
        }
        let report = scan
            .check(&records)
            .unwrap_or_else(|hits| panic!("seed {seed}: {}", hits.join("; ")));
        assert_eq!(report.looked_at.len(), records.len());
    }
}

// verifies: REQ-AUD-6
#[test]
fn balance_held_after_any_step_is_found_nowhere_outside_the_evidence_store() {
    let mut openings = Vec::new();
    for seed in [1, 2] {
        let run = run_balanced(&mut Reference, seed, None);
        let scan = Scan::new(run.scanned.canaries.clone()).expect("a scan");
        let figures = distinct(&run.scanned.canaries);
        let names = scan.names();
        // A reserve's balance after each of its movements, and the holding account's after each step.
        let after = |part: &str| names.iter().filter(|n| n.contains(part)).count();
        assert!(
            after("holding account, after step") >= 3,
            "seed {seed}: the scan seeks no balance the holding account held after a step: {names:?}"
        );
        // The holding account's balance after a step is not one figure for the run: it moves between steps.
        let after_steps: std::collections::BTreeSet<&[u8]> = run
            .scanned
            .canaries
            .iter()
            .filter(|c| c.name.contains("holding account, after step"))
            .map(|c| c.bytes.as_slice())
            .collect();
        assert!(
            after_steps.len() >= 2,
            "seed {seed}: the holding account's balance after each step is one figure"
        );
        // The reserve's opening balance is no round constant, and the run sets it.
        let opening = run
            .scanned
            .canaries
            .iter()
            .find(|c| c.name.contains("reserve") && c.name.contains("at the start"))
            .map(|c| String::from_utf8(c.bytes.clone()).expect("a figure"))
            .expect("an opening balance");
        assert_ne!(
            opening, "50000000",
            "seed {seed}: the opening is the stand-in's constant"
        );
        assert!(
            opening.len() >= MIN_LEN && !opening.ends_with("000000"),
            "seed {seed}: the opening {opening} isn't distinctive"
        );
        // A balance mid-route: the opening less what the first order took out.
        let mid = opening.parse::<u128>().expect("a figure") - 12_345_678;
        assert!(
            figures.iter().any(|(_, v)| *v == mid.to_string()),
            "seed {seed}: the balance after the first withdrawal ({mid}) isn't sought: {figures:?}"
        );
        // A balance held between two movements: a figure of a reserve after a movement that is neither its opening nor
        // its close.
        let closing = run
            .scanned
            .canaries
            .iter()
            .find(|c| c.name.contains("reserve") && c.name.contains("at the end"))
            .map(|c| String::from_utf8(c.bytes.clone()).expect("a figure"))
            .expect("a closing balance");
        let mid_route = run
            .scanned
            .canaries
            .iter()
            .filter(|c| c.name.contains("after movement"))
            .map(|c| String::from_utf8(c.bytes.clone()).expect("a figure"))
            .any(|v| v != opening && v != closing);
        assert!(
            mid_route,
            "seed {seed}: no balance held between two movements is sought: {names:?}"
        );
        for (name, value) in &figures {
            assert!(
                value.len() >= MIN_LEN && !run.stated.contains(value),
                "seed {seed}: `{name}` is not a distinctive figure"
            );
        }
        let (shelf, outside): (Vec<Surface>, Vec<Surface>) =
            run.scanned.surfaces.iter().cloned().partition(is_shelf);
        scan.check(&outside)
            .unwrap_or_else(|hits| panic!("seed {seed}: {}", hits.join("; ")));
        assert!(
            scan.find(&shelf)
                .iter()
                .all(|h| h.canary.contains("holding account")),
            "seed {seed}: a reserve's balance is on the shelf"
        );
        openings.push(opening);
        // And the text Core produces about the run, which no other surface holds.
        let looked: Vec<&str> = run
            .scanned
            .surfaces
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        for part in [
            "why the routes stopped",
            "what Core told of each order",
            "why a swap failed",
            "whether the log read back",
        ] {
            assert!(
                looked.contains(&part),
                "seed {seed}: the scan didn't read `{part}`"
            );
        }
    }
    // The opening spans the range the notes give, and no seed leaves it.
    let drawn: Vec<u128> = (0..2_000).map(opening_for).collect();
    assert!(
        drawn
            .iter()
            .all(|o| (61_234_567..=89_234_566).contains(o) && o % 1_000_000 != 0)
            && drawn.iter().any(|o| *o > 79_000_000),
        "the opening isn't spread over its range"
    );
    assert_ne!(
        openings[0], openings[1],
        "the run doesn't set the opening from its seed"
    );
}

// verifies: REQ-AUD-6
#[test]
fn balance_planted_in_a_decision_record_is_found_by_the_scan() {
    let clean = run_balanced(&mut Reference, 1, None);
    let scan = Scan::new(clean.scanned.canaries.clone()).expect("a scan");
    // Each figure in one field of the record, the fields taking turns: the request's id, `round.refines.request_id`,
    // `bounds.policy_version` and `router_version` (`DECISION_FIELDS`).
    for (n, (name, value)) in distinct(&clean.scanned.canaries).into_iter().enumerate() {
        let at = DECISION_FIELDS[n % DECISION_FIELDS.len()];
        let run = run_balanced(
            &mut Reference,
            1,
            Some(&Planted::InDecisionRecord(value.clone(), at)),
        );
        let hits = scan.find(&run.scanned.surfaces);
        assert!(
            hits.iter()
                .any(|h| h.surface.contains("Core's decision record")
                    && String::from_utf8(scan_bytes(&clean.scanned.canaries, &h.canary))
                        .is_ok_and(|v| v == value)),
            "`{name}` planted in a decision record wasn't found in a decision record (at {at}): {hits:?}"
        );
    }
}

fn scan_bytes(canaries: &[Canary], name: &str) -> Vec<u8> {
    canaries
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.bytes.clone())
        .unwrap_or_default()
}

// verifies: REQ-AUD-6
#[test]
#[should_panic(expected = "doesn't read back as one")]
fn decision_record_that_does_not_read_back_is_refused_and_not_dropped_from_the_scan() {
    // A record Core signed whose candidates are no list reads back as no record; the run is refused, so it can't
    // vanish from the scan with the rest staying green.
    let _ = run_balanced(&mut Reference, 1, Some(&Planted::CorruptDecisionRecord));
}

/// The one value a holding raise is left without is its difference's `by`: a figure planted in any other field of a
/// holding raise is found by the scan, and the difference itself is not sought there (`REQ-AUD-6`, `T-020-63`; review M3).
// verifies: REQ-AUD-6
#[test]
fn secret_planted_elsewhere_in_a_holding_raise_is_still_found() {
    use rutent_scenarios::secrets::{states_a_holding_difference, without_the_difference};
    let raise = |asset: &str| {
        serde_json::json!({
            "kind": "verified",
            "what": { "call": "raise", "raise": {
                "raise": "raise-1", "cites": [],
                "difference": {
                    "kind": "holding", "account": "holding-sim-wallet-node", "node": "sim-wallet-node",
                    "asset": { "id": asset, "decimals": 2 }, "direction": "over",
                    "by": { "asset": asset, "amount": "424242424" },
                    "word": "checked",
                },
            } },
        })
    };
    let balance = Canary::text("balance of the holding account", Kind::Balance, "424242424");
    let secret = Canary::text("a planted secret", Kind::Balance, "909090909");
    let scan = Scan::new(vec![balance, secret]).expect("a scan");
    let surface = |record: &serde_json::Value| {
        vec![Surface::of(
            "Core's log, record 1",
            serde_json::to_vec(record),
        )]
    };
    // The record as the scan reads it: the figure by which the account is over is not there.
    let mut clean = raise("USDC");
    assert!(states_a_holding_difference(&clean));
    without_the_difference(&mut clean);
    assert!(
        scan.find(&surface(&clean)).is_empty(),
        "the difference is left out"
    );
    // A secret put in the asset's name, anywhere but `by`, is found in the same record.
    let mut planted = raise("909090909");
    planted["what"]["raise"]["difference"]["by"]["asset"] = "USDC".into();
    without_the_difference(&mut planted);
    let hits = scan.find(&surface(&planted));
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].canary, "a planted secret");
    // And the balance itself put in another field is found too: only `by` is left out.
    let mut elsewhere = raise("USDC");
    elsewhere["what"]["raise"]["difference"]["account"] = "424242424".into();
    without_the_difference(&mut elsewhere);
    assert_eq!(scan.find(&surface(&elsewhere)).len(), 1);
}
