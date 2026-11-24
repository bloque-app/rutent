//! `T-025-03` (`EXP-1`'s `G-26`, `THR-79`): after full simulated runs on three VOS replicas, the state machine's journal
//! and the nodes' own files hold no provider credential, no secret's plaintext and no module state, and the scan
//! shows it can see: each kind, let through the state machine's boundary once, is found in the journal and in the
//! nodes' files. The scan is `rutent_scenarios::secrets`, which the reference host's run uses too.
//! `T-024-20` adds `REQ-AUD-6`'s other half, "no balance": the same scan, seeking the balances the simulated reserves'
//! banks and the holding account held, over a full run on three replicas, with the evidence store's shelf the one
//! surface that may hold one.
//! These need `../build.sh`'s binaries and `../test.sh`'s paths; without the `vos` feature this file is empty.

#![cfg(feature = "vos")]

use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::harness::seeds;
use rutent_scenarios::secrets::{
    Kind, Planted, Scan, Scanned, Surface, run_balanced, run_leaking, run_planted,
};
use rutent_scenarios_vos::{Base, Vos};

/// What the nodes' files and the host's answers show of a run, for a scan of `scanned`'s values.
struct Looked {
    host: Result<rutent_scenarios::secrets::Report, Vec<String>>,
    nodes: rutent_scenarios::secrets::DirScan,
}

fn look(scan: &Scan, scanned: &Scanned, base: &Base) -> Looked {
    Looked {
        host: scan.check(&scanned.surfaces),
        nodes: scan
            .find_in_dir(&base.0)
            .unwrap_or_else(|e| panic!("the nodes' files couldn't be read: {e}")),
    }
}

// verifies: THR-79, REQ-AUD-6
#[test]
fn vos_journal_after_a_full_run_holds_no_credential_or_module_state() {
    for seed in seeds(&[1, 2, 3]) {
        // The run as the assembly makes it: the values planted, the boundary as it is.
        let base = Base::new(&format!("secrets-{seed}"));
        let scanned = run_planted(&mut Vos::three(&base, "n"), seed);
        let scan = Scan::new(scanned.canaries.clone()).expect("a scan");
        assert_eq!(
            scan.kinds().len(),
            3,
            "seed {seed}: a credential, a secret and module state"
        );
        let looked = look(&scan, &scanned, &base);
        // Where module state sat: the platform's world, in Core's process, and none of the VOS space's.
        println!(
            "seed {seed}: module state sat in the platform's world, in Core's process: {:?}; the scan looked at {} surfaces on the host and {} files, {} bytes, of the nodes'",
            scanned.module_state,
            scanned.surfaces.len(),
            looked.nodes.files,
            looked.nodes.bytes
        );
        let report = looked.host.unwrap_or_else(|hits| {
            panic!(
                "seed {seed}: the journal or the log holds: {}",
                hits.join("; ")
            )
        });
        assert!(
            report
                .looked_at
                .iter()
                .any(|(n, _)| n.contains("its snapshot")),
            "seed {seed}: the scan didn't read the host's snapshot"
        );
        assert!(
            report
                .looked_at
                .iter()
                .any(|(n, _)| n.contains("evidence shelf")),
            "seed {seed}: the scan didn't read the evidence shelf"
        );
        assert!(
            looked.nodes.files > 0 && looked.nodes.bytes > 0,
            "seed {seed}: the nodes left no file to read, so the scan saw nothing of their state"
        );
        assert!(
            looked.nodes.hits.is_empty(),
            "seed {seed}: the nodes' files hold a value: {:?}",
            looked.nodes.hits
        );
    }
}

// verifies: THR-79, REQ-AUD-6
#[test]
fn scan_finds_each_kind_of_value_let_into_three_vos_replicas_journal_and_files() {
    for (seed, kind) in [
        (1, Kind::Credential),
        (2, Kind::Secret),
        (3, Kind::ModuleState),
    ] {
        let base = Base::new(&format!("secrets-leak-{seed}"));
        let scanned = run_leaking(&mut Vos::three(&base, "l"), seed, kind);
        let scan = Scan::new(scanned.canaries.clone()).expect("a scan");
        let looked = look(&scan, &scanned, &base);
        let hits = scan.find(&scanned.surfaces);
        assert!(
            hits.iter()
                .any(|h| h.kind == kind && h.surface.contains("journal of")),
            "{kind:?}: the scan didn't find the value in the journal the host answered: {hits:?}"
        );
        // The same value, in the nodes' own files: if it isn't there, the scan of them can't see the state
        // (it is compressed, or held somewhere the scan doesn't reach), and a clean scan of them means nothing.
        assert!(
            looked.nodes.hits.iter().any(|h| h.kind == kind),
            "{kind:?}: the value let in isn't found in the nodes' files ({} files, {} bytes): the scan can't see their state",
            looked.nodes.files,
            looked.nodes.bytes
        );
        assert!(
            looked.host.is_err(),
            "{kind:?}: the host's surfaces read clean"
        );
    }
}

// verifies: REQ-AUD-6
#[test]
fn vos_journal_and_files_after_a_full_run_hold_no_simulated_balance_outside_the_evidence_store() {
    for seed in seeds(&[1, 2, 3]) {
        let base = Base::new(&format!("balances-{seed}"));
        let run = run_balanced(&mut Vos::three(&base, "b"), seed, None);
        let scan = Scan::new(run.scanned.canaries.clone()).expect("a scan");
        assert_eq!(
            scan.kinds(),
            std::collections::BTreeSet::from([Kind::Balance]),
            "seed {seed}: the scan seeks balances"
        );
        let nodes = scan
            .find_in_dir(&base.0)
            .unwrap_or_else(|e| panic!("the nodes' files couldn't be read: {e}"));
        let (shelf, outside): (Vec<Surface>, Vec<Surface>) = run
            .scanned
            .surfaces
            .iter()
            .cloned()
            .partition(|s| s.name.contains("evidence shelf"));
        println!(
            "seed {seed}: the scan looked at {} surfaces on the host, {} of them the shelf's, and {} files, {} bytes, of the nodes'",
            run.scanned.surfaces.len(),
            shelf.len(),
            nodes.files,
            nodes.bytes
        );
        // The reading is kept, in the shelf, so a clean scan of the rest is not a scan of a run that kept nothing.
        assert!(
            !scan.find(&shelf).is_empty(),
            "seed {seed}: the holding account's reading isn't in the evidence store"
        );
        let report = scan.check(&outside).unwrap_or_else(|hits| {
            panic!(
                "seed {seed}: the journal or the log holds: {}",
                hits.join("; ")
            )
        });
        let named = |part: &str| report.looked_at.iter().any(|(n, _)| n.contains(part));
        assert!(
            named("its snapshot") && named("journal of") && named("Core's log, record"),
            "seed {seed}: the scan didn't read the host's answers and the log"
        );
        assert!(
            named("platform's journal of") && named("requests the platform delivered"),
            "seed {seed}: the scan didn't read the platform's records"
        );
        assert!(
            nodes.files > 0 && nodes.bytes > 0,
            "seed {seed}: the nodes left no file to read, so the scan saw nothing of their state"
        );
        assert!(
            nodes.hits.is_empty(),
            "seed {seed}: the nodes' files hold a balance: {:?}",
            nodes.hits
        );
    }
}

// verifies: REQ-AUD-6
#[test]
fn scan_finds_a_balance_let_into_three_vos_replicas_journal_and_files() {
    // The figures the accounts held, read from a run that has none planted, on the reference host: the accounts are
    // the platform's and the same on any host. (A balance in Core's log, which no host touches, is the reference
    // host's control: `secrets_scan.rs`.)
    let clean = run_balanced(&mut Reference, 1, None);
    let mut figures: Vec<(String, String)> = clean
        .scanned
        .canaries
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                String::from_utf8(c.bytes.clone()).expect("a figure"),
            )
        })
        .collect();
    figures.sort_by(|a, b| a.1.cmp(&b.1));
    figures.dedup_by(|a, b| a.1 == b.1);
    assert!(
        figures.len() >= 2,
        "the holding account and a reserve hold figures of their own: {figures:?}"
    );
    for (n, (name, figure)) in figures.iter().enumerate() {
        // Into the state machine's journal, as an input the boundary let through: on the replicas' files too.
        let base = Base::new(&format!("balances-control-{n}"));
        let run = run_balanced(
            &mut Vos::three(&base, "j"),
            1,
            Some(&Planted::InJournal(figure.clone())),
        );
        let scan = Scan::new(run.scanned.canaries.clone()).expect("a scan");
        let looked = scan
            .find_in_dir(&base.0)
            .unwrap_or_else(|e| panic!("the nodes' files couldn't be read: {e}"));
        let hits = scan.find(&run.scanned.surfaces);
        assert!(
            hits.iter()
                .any(|h| h.kind == Kind::Balance && h.surface.contains("journal of")),
            "`{name}`: the scan didn't find the balance in the journal the host answered: {hits:?}"
        );
        assert!(
            looked.hits.iter().any(|h| h.kind == Kind::Balance),
            "`{name}`: the balance let in isn't found in the nodes' files ({} files, {} bytes): the scan can't see their state",
            looked.files,
            looked.bytes
        );
    }
}
