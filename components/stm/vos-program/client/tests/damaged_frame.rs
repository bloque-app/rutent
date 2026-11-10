//! `G-54` on VOS (`T-010-06`): damage the frame that holds the last acknowledged append, on one node and on the
//! leader of three, and record what VOS does with it: refuses it, recovers it, or cuts it silently.
//!
//! `EXP-1` could not say: its damage landed in fjall's newest journal file, most likely on the unsynced batch
//! written after the acknowledged append, which the log replays over. So this test finds the acknowledged frame by
//! its content. The last acknowledged input has a record id of its own; once the nodes are quiet and killed, the
//! test searches the node's data directory for those bytes and flips one byte inside every place they sit in a
//! journal file. A batch's checksum covers every byte of it, so that batch is damaged; the test records the file
//! and offset of each place and how many bytes follow it, so a reader sees whether the damage hit a batch with
//! later batches behind it.
//!
//! On three nodes every node is killed and only the leader's data is damaged. The leader is then started alone,
//! and its own replica is asked (an applied-consistency query, which a lone node of three answers from what it
//! holds), before the other two start: what the node did with its own store is kept apart from what the cluster
//! did after, since a follower catching a damaged node up is replication (`EXP-1`'s `R-04a`), not damage.
//!
//! The damage is one of three: a byte inside every place the id sits, the journal's last byte, or the journal cut just after
//! the id (the acknowledged frame last and torn). The outcome is one of four, printed as a line beginning `G-54` and appended to the file `G54_OUT` names, if set:
//! `refused` (the node did not start, or its log says it found damage), `recovered` (its own replica holds every
//! acknowledged record), `cut and repaired by replication` (three nodes: its own replica lost the record and the
//! cluster gave it back) and `cut silently` (the record is gone and nothing said so). The test passes on the first
//! two, and on `undetermined, whole once the others joined` (a lone node of three that could not answer until the
//! others started, then held every record), and fails on the others: it records the result and need not pass. It needs the VOS toolchain, built by
//! `../build.sh`, and the paths of `../test.sh`; without the `vos` feature this file is empty.
//!
//! The nodes run the JIT on a Linux x86_64 machine that gives it, and the test fails closed elsewhere. `VOS_DEV=1`
//! runs the interpreter instead, a development run whose result is not the task's.

#![cfg(feature = "vos")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use rutent_conformance::host::probe;
use rutent_contracts::state_machine::RecordedInput;
use rutent_contracts::state_machine::host::testing::Harness;
use rutent_contracts::state_machine::host::{Delivery, Host};
use rutent_stm::Stm;
use rutent_stm_vos::cluster::{ClusterConfig, INSTANCE, NodeBuild};
use rutent_stm_vos::host::{self, VosHarness, VosHost};
use rutent_stm_vos::platform::{Backend, Facts};
use rutent_stm_vos_program::Status;
use vos_wire::ingress::Consistency;

fn var(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var(name).unwrap_or_else(|_| panic!("{name} names a binary: see test.sh")),
    )
}

fn development() -> bool {
    std::env::var("VOS_DEV").is_ok_and(|v| v == "1")
}

/// The configuration of a cluster of `replicas` nodes, after the platform assertion (fail closed).
fn config(replicas: usize) -> ClusterConfig {
    let facts = Facts::read();
    let backend = if development() {
        Backend::Interpreter
    } else {
        facts.assert_jit().unwrap_or_else(|e| {
            panic!("the platform assertion failed, so the nodes did not run: {e}")
        });
        Backend::Jit
    };
    ClusterConfig {
        build: NodeBuild {
            vos: var("VOS_BIN"),
            faults: true,
        },
        agent_elf: var("VOS_AGENT_ELF"),
        program_elf: var("STM_PROGRAM_ELF"),
        replicas,
        spares: 0,
        executors: if replicas > 1 {
            (facts.vcpus / 3).max(1)
        } else {
            facts.vcpus.saturating_sub(1).max(1)
        },
        backend,
        settings: String::new(),
        wait: Duration::from_secs(60),
        strace: None,
    }
}

/// A directory for the test's cluster, removed with it.
struct Base(PathBuf);

impl Base {
    fn new(name: &str) -> Base {
        let dir = std::env::temp_dir().join(format!("rsv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Base(dir)
    }
}

impl Drop for Base {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the test saw, in one line.
struct Finding {
    /// `refused`, `recovered`, `cut and repaired by replication` or `cut silently`.
    outcome: &'static str,
    detail: String,
}

impl Finding {
    fn acceptable(&self) -> bool {
        matches!(
            self.outcome,
            "refused" | "recovered" | "undetermined, whole once the others joined"
        )
    }
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Where `needle` sits in `haystack`: each offset.
fn occurrences(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at)
        .collect()
}

/// One place the acknowledged record's id sits.
struct Place {
    file: PathBuf,
    offset: usize,
    /// How many bytes of the file lie after the id.
    after: usize,
}

impl Place {
    fn describe(&self, data: &Path) -> String {
        format!(
            "{}@{} ({} bytes after)",
            self.file.strip_prefix(data).unwrap_or(&self.file).display(),
            self.offset,
            self.after
        )
    }
}

/// Where the id's bytes sit in the node's journal files: every file under its data directory whose path says
/// `journal`, or, if none does, every file that holds them.
fn places(data: &Path, needle: &[u8]) -> Vec<Place> {
    let all: Vec<(PathBuf, Vec<u8>)> = files_under(data)
        .into_iter()
        .filter_map(|f| std::fs::read(&f).ok().map(|bytes| (f, bytes)))
        .collect();
    let held: Vec<Place> = all
        .iter()
        .flat_map(|(file, bytes)| {
            occurrences(bytes, needle).into_iter().map(|offset| Place {
                file: file.clone(),
                offset,
                after: bytes.len() - offset - needle.len(),
            })
        })
        .collect();
    let in_journals: Vec<Place> = held
        .iter()
        .filter(|p| p.file.to_string_lossy().contains("journal"))
        .map(|p| Place {
            file: p.file.clone(),
            offset: p.offset,
            after: p.after,
        })
        .collect();
    if in_journals.is_empty() {
        held
    } else {
        in_journals
    }
}

/// Flips one byte inside each place's id.
fn damage(places: &[Place]) {
    for place in places {
        let mut bytes = std::fs::read(&place.file).expect("a journal file");
        bytes[place.offset + 2] ^= 0x55;
        std::fs::write(&place.file, bytes).expect("damaging a journal file");
    }
}

/// Node `n`'s own replica's status, asked of that node alone and answered from what it has applied.
fn own_status(host: &VosHost, n: usize) -> Result<Status, String> {
    let client =
        vos_node::client::Client::new(&host.cluster.target(n), SigningKey::from_bytes(&[3; 32]));
    let not_after = vos_node::wall_clock() + 60_000;
    host::answer::<Status>(client.query_with(
        host.agent,
        INSTANCE,
        host.methods.status,
        &[],
        not_after,
        Consistency::Applied { at: 0 },
    ))
    .map_err(|e| format!("{e:?}"))
}

/// Node `n`'s own record count: the first answer, then the one a few seconds on, or the error that stopped it.
fn own_records(host: &VosHost, n: usize, wait: Duration) -> Result<(u64, u64), String> {
    let until = Instant::now() + wait;
    let first = loop {
        match own_status(host, n) {
            Ok(status) => break status.records,
            Err(e) if Instant::now() > until => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    };
    std::thread::sleep(Duration::from_secs(5));
    let settled = own_status(host, n).map_or(first, |s| s.records);
    Ok((first, settled))
}

/// The words in a node's log that say it found damage, if any.
fn log_findings(host: &VosHost, n: usize) -> Vec<String> {
    let log = std::fs::read_to_string(host.cluster.log(n)).unwrap_or_default();
    log.lines()
        .filter(|l| {
            let l = l.to_lowercase();
            [
                "corrupt",
                "checksum",
                "crc",
                "invalid journal",
                "damaged",
                "recovery",
            ]
            .iter()
            .any(|w| l.contains(w))
        })
        .map(|l| l.chars().take(200).collect())
        .collect()
}

/// Where the damage goes.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Where {
    /// A byte inside every place the last acknowledged record id sits in the journal.
    InTheRecord,
    /// The last byte of the journal file that holds it: the final frame, whatever it holds, with nothing after it
    /// to show the damage is in the middle.
    LastByte,
    /// The journal file cut short just after the first place the record id sits: the frame that holds it is the
    /// last, and torn, with nothing after it. A torn tail is what a journal's recovery is built to cut.
    TornInTheRecord,
}

impl Where {
    fn name(self) -> &'static str {
        match self {
            Where::InTheRecord => "inside the record",
            Where::LastByte => "last byte of the journal",
            Where::TornInTheRecord => "journal cut just after the id's first place",
        }
    }
}

fn g54(replicas: usize, name: &str, at: Where) -> Finding {
    let base = Base::new(name);
    let mut harness = VosHarness::<Stm>::new(config(replicas), base.0.clone(), "g");
    let store = harness.fresh_store().unwrap();
    let mut host: VosHost = <VosHarness<Stm> as Harness<Stm>>::start(&mut harness, store).unwrap();

    // An order's inputs, all acknowledged; the last one's record id is the one to find on disk.
    let inputs: Vec<RecordedInput> = probe::order_inputs("ord-g54", 0);
    let mut acknowledged = 0u64;
    for input in &inputs {
        let delivered = Host::<Stm>::submit(&mut host, input.clone());
        assert!(
            matches!(delivered, Ok(Delivery::Recorded(_))),
            "{delivered:?}"
        );
        acknowledged += 1;
    }
    let last = inputs.last().unwrap().record_id.as_str().to_owned();
    assert_eq!(host.status().unwrap().records, acknowledged);

    // Quiet, so the node has written whatever follows an append, then every node dies as a kill leaves it.
    std::thread::sleep(Duration::from_secs(3));
    let target = if replicas > 1 {
        host.cluster.leader().expect("a leader")
    } else {
        1
    };
    host.cluster.kill_all();

    let data = host.cluster.data(target);
    let found = places(&data, last.as_bytes());
    let where_it_sat = format!(
        "{} places, the last at {}",
        found.len(),
        found.last().map_or(String::new(), |p| p.describe(&data))
    );
    assert!(
        !found.is_empty(),
        "the last acknowledged record id is nowhere in node {target}'s data"
    );
    let targets: Vec<Place> = match at {
        Where::InTheRecord => found
            .iter()
            .map(|p| Place {
                file: p.file.clone(),
                offset: p.offset,
                after: p.after,
            })
            .collect(),
        Where::LastByte | Where::TornInTheRecord => {
            let file = found.last().expect("a place").file.clone();
            let size = std::fs::metadata(&file).expect("a journal file").len() as usize;
            vec![Place {
                file,
                offset: size - 3,
                after: 0,
            }]
        }
    };
    if let Where::TornInTheRecord = at {
        // The first place: the id sits in many frames, and the first is the one the append itself wrote.
        let place = found.first().expect("a place");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&place.file)
            .expect("a journal file");
        file.set_len((place.offset + last.len()) as u64)
            .expect("cutting a journal file");
    } else {
        damage(&targets);
    }

    // The damaged node starts alone, so its own store is what answers.
    let started = host.cluster.start(target);
    let own = match &started {
        Ok(()) => own_records(&host, target, Duration::from_secs(30)),
        Err(e) => Err(e.clone()),
    };
    let findings = log_findings(&host, target);
    let report = |what: String| {
        format!(
            "damage {}; node {target} of {replicas}, record {last} sat at {where_it_sat}; {what}; log lines naming damage: {findings:?}",
            at.name()
        )
    };

    let finding = match (&started, &own) {
        (Err(e), _) => Finding {
            outcome: "refused",
            detail: report(format!("the node did not start: {e}")),
        },
        (Ok(()), Err(e)) if !findings.is_empty() => Finding {
            outcome: "refused",
            detail: report(format!("the node started but never answered: {e}")),
        },
        (Ok(()), Err(e)) => {
            // A lone node of three answers "behind" until the others are there; so start them, and ask again.
            let mut whole = false;
            if replicas > 1 {
                for n in (1..=3).filter(|n| *n != target) {
                    let _ = host.cluster.start(n);
                }
                let until = Instant::now() + Duration::from_secs(60);
                while Instant::now() < until && !whole {
                    whole = own_status(&host, target).is_ok_and(|s| s.records >= acknowledged);
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            if whole {
                Finding {
                    outcome: "undetermined, whole once the others joined",
                    detail: report(format!(
                        "the lone node could not answer ({e}); with the others up its replica holds all {acknowledged} records, so a loss here, if there was one, was repaired by replication"
                    )),
                }
            } else {
                Finding {
                    outcome: "cut silently",
                    detail: report(format!(
                        "the node started and never held every record, with nothing in its log: {e}"
                    )),
                }
            }
        }
        (Ok(()), Ok((first, settled))) if *settled >= acknowledged => Finding {
            outcome: "recovered",
            detail: report(format!(
                "its own replica holds {first} then {settled} of {acknowledged} records"
            )),
        },
        (Ok(()), Ok((first, settled))) => {
            // Lost locally. On three, the others start and may give it back.
            let mut repaired = false;
            if replicas > 1 {
                for n in (1..=3).filter(|n| *n != target) {
                    let _ = host.cluster.start(n);
                }
                host.client = host.cluster.client();
                let until = Instant::now() + Duration::from_secs(60);
                while Instant::now() < until && !repaired {
                    repaired = own_status(&host, target).is_ok_and(|s| s.records >= acknowledged);
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            if repaired {
                Finding {
                    outcome: "cut and repaired by replication",
                    detail: report(format!(
                        "its own replica held {first} then {settled} of {acknowledged} records, and caught up from the others"
                    )),
                }
            } else {
                Finding {
                    outcome: "cut silently",
                    detail: report(format!(
                        "its own replica holds {first} then {settled} of {acknowledged} records, and nothing said so"
                    )),
                }
            }
        }
    };
    host.cluster.kill_all();
    finding
}

fn record(setup: &str, finding: &Finding) {
    let line = format!(
        "G-54 {setup}: {} ({}); {}",
        finding.outcome,
        if development() {
            "development run, interpreter"
        } else {
            "JIT"
        },
        finding.detail
    );
    println!("{line}");
    if let Ok(path) = std::env::var("G54_OUT")
        && let Ok(mut out) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(out, "{line}");
    }
}

/// Both places of damage on `replicas` nodes: every one is recorded, and every one must be refused or recovered.
fn both(replicas: usize, setup: &str) {
    let mut unacceptable = Vec::new();
    for at in [Where::InTheRecord, Where::LastByte, Where::TornInTheRecord] {
        let finding = g54(replicas, &format!("g54-{replicas}-{}", at as u8), at);
        record(setup, &finding);
        if !finding.acceptable() {
            unacceptable.push(format!("VOS: {}; {}", finding.outcome, finding.detail));
        }
    }
    assert!(unacceptable.is_empty(), "{}", unacceptable.join("\n"));
}

// verifies: NFR-6, INV-7
#[test]
fn damaged_last_acknowledged_append_on_one_vos_node_is_refused_or_recovered_and_never_cut_silently()
{
    both(1, "one node");
}

// verifies: NFR-6, INV-7
#[test]
fn damaged_last_acknowledged_append_on_the_vos_leader_of_three_is_refused_or_recovered_and_never_cut_silently()
 {
    both(3, "leader of three");
}

/// The harness can find an id's bytes: the check the test's damage rests on.
// verifies: NFR-6, INV-7
#[test]
fn the_places_of_an_id_are_found_by_its_bytes() {
    let dir = std::env::temp_dir().join(format!("rsv-places-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("journals")).unwrap();
    std::fs::write(dir.join("journals/1"), b"xx rec-7 yy rec-7 zz").unwrap();
    std::fs::write(dir.join("other"), b"rec-7").unwrap();
    let found = places(&dir, b"rec-7");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(found.len(), 2, "only the journal's two places");
    assert_eq!(
        (
            found[0].offset,
            found[0].after,
            found[1].offset,
            found[1].after
        ),
        (3, 12, 12, 3)
    );
}
