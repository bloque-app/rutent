//! `T-025-03`'s scan, on the reference host: what it seeks and how, and that it can see. The same scan runs on three
//! VOS replicas in `rutent-scenarios-vos`, with the nodes' own files added to what it looks in.

use rutent_contracts::module_interface::SecretValue;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::secrets::{
    Canary, Kind, MIN_LEN, Scan, Surface, forms, run_leaking, run_planted,
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
    let mut written: Vec<(String, Vec<u8>)> = forms(bytes)
        .into_iter()
        .map(|(name, form)| (name.to_owned(), form))
        .collect();
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
    assert!(written.len() >= 10, "forms: {}", written.len());
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
