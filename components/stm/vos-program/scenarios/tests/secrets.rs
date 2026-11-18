//! `T-025-03` (`EXP-1`'s `G-26`, `THR-79`): after full simulated runs on three VOS replicas, the state machine's journal
//! and the nodes' own files hold no provider credential, no secret's plaintext and no module state, and the scan
//! shows it can see: each kind, let through the state machine's boundary once, is found in the journal and in the
//! nodes' files. The scan is `rutent_scenarios::secrets`, which the reference host's run uses too.
//! These need `../build.sh`'s binaries and `../test.sh`'s paths; without the `vos` feature this file is empty.

#![cfg(feature = "vos")]

use rutent_scenarios::harness::seeds;
use rutent_scenarios::secrets::{Kind, Scan, Scanned, run_leaking, run_planted};
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
