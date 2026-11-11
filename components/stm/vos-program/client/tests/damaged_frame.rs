//! `G-54` on VOS (`T-010-06`): damage the frame that holds the last acknowledged append, on one node and on the
//! leader of three, and record what VOS does with it: refuses it, recovers it, or cuts it silently.
//!
//! `EXP-1` could not say: its damage landed in fjall's newest journal file, most likely on the unsynced batch
//! written after the acknowledged append, which the log replays over. So this test measures the append: it reads the
//! length of the node's journal when the node is quiet before the last submit and again when it is quiet after it,
//! so the bytes that submit wrote are a range, and it finds the last input's record id by content inside that range.
//! It cannot read fjall's frame boundaries (the format was not read), so "the frame holding the append" is the
//! range's first place of the id, and each kind of damage says where it fell in the range.
//!
//! On three nodes every node is killed and only the leader's data is damaged. The leader is then started alone,
//! and its own replica is asked (an applied-consistency query, which a lone node of three answers from what it
//! holds), before the other two start: what the node did with its own store is kept apart from what the cluster
//! did after, since a follower catching a damaged node up is replication (`EXP-1`'s `R-04a`), not damage.
//!
//! The damage is one of three, all in the bytes the last submit wrote: one byte flipped at the first place the id sits
//! (the batch of the append, its length intact, with whatever the submit wrote after it behind), the range's last
//! byte, or the journal cut just after the first place (a torn tail). The outcome is printed as a line beginning
//! `G-54` and appended to the file `G54_OUT` names, if set: `refused` (the node did not start, or started and
//! never answered with its log naming damage), `recovered` (its own replica holds every acknowledged record), `cut
//! and repaired by replication` (three nodes: its own replica lost the record, said nothing, and the cluster gave it
//! back), `cut and logged` (a loss that the node's log names), `cut silently` (a loss nothing names) and `not
//! reproduced` (the journal was cut and its own replica still holds every acknowledged record, so the cut did not
//! take the record and the run says nothing about a torn tail). The test passes on `refused`, `recovered`, `cut and
//! logged` and `undetermined, whole once the others joined` (a lone node of three that could not answer until the
//! others started, then held every record), and fails on the others, `not reproduced` among them: it records the
//! result and need not pass.
//!
//! The last submit is of one of two kinds, and each kind is its own test. One ends the order (the order's seventh
//! input settles its last segment), and `T-010-07` seals the journal on an order's last input, so a cut after it may
//! not reach the record. One does not (the order's sixth input binds its last segment), so the record is the journal's
//! live tail. It needs the VOS toolchain, built by `../build.sh`, and the paths of
//! `../test.sh`; without the `vos` feature this file is empty.
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
            "refused"
                | "recovered"
                | "cut and logged"
                | "undetermined, whole once the others joined"
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

/// The length of each of the node's journal files (`*.jnl`).
fn journal_lengths(data: &Path) -> std::collections::BTreeMap<PathBuf, usize> {
    files_under(data)
        .into_iter()
        .filter(|f| f.extension().is_some_and(|e| e == "jnl"))
        .filter_map(|f| std::fs::metadata(&f).ok().map(|m| (f, m.len() as usize)))
        .collect()
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
            ["corrupt", "checksum", "crc", "invalid journal", "damaged"]
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
    /// One byte flipped in the first place the id sits in the bytes the last submit wrote: inside the batch of the
    /// last acknowledged append, its length intact, with whatever that submit wrote after it still behind.
    InTheRecord,
    /// The last byte of what the last submit wrote: the end of its final frame, whatever that frame holds.
    LastByte,
    /// The journal cut just after the first place the id sits in what the last submit wrote: a torn tail, whatever
    /// frame boundary that is. A torn tail is what a journal's recovery is built to cut.
    TornInTheRecord,
}

impl Where {
    /// Whether the damage removes bytes from the journal, rather than changing them.
    fn is_cut(self) -> bool {
        matches!(self, Where::TornInTheRecord)
    }

    fn name(self) -> &'static str {
        match self {
            Where::InTheRecord => {
                "one byte inside the first place of the id in the last submit's bytes"
            }
            Where::LastByte => "last byte of the last submit's bytes",
            Where::TornInTheRecord => {
                "journal cut just after the id's first place in the last submit's bytes"
            }
        }
    }
}

/// `ends_order`: whether the last submit before the damage ends the order (all seven inputs) or leaves it running
/// (the first six).
fn g54(replicas: usize, name: &str, at: Where, ends_order: bool) -> Finding {
    let base = Base::new(name);
    let mut harness = VosHarness::<Stm>::new(config(replicas), base.0.clone(), "g");
    let store = harness.fresh_store().unwrap();
    let mut host: VosHost = <VosHarness<Stm> as Harness<Stm>>::start(&mut harness, store).unwrap();

    // An order's inputs, all acknowledged; the last one's record id is the one to find on disk.
    let mut inputs: Vec<RecordedInput> = probe::order_inputs("ord-g54", 0);
    if !ends_order {
        // The sixth input only binds the last segment: the order is still running after it.
        inputs.pop();
    }
    let mut acknowledged = 0u64;
    assert_eq!(host.status().unwrap().records, acknowledged);

    // The append itself is measured: the journal's length when the node is quiet before the last submit, and again
    // when it is quiet after it, so what that submit wrote is a range of bytes, not a guess.
    let target = if replicas > 1 {
        host.cluster.leader().expect("a leader")
    } else {
        1
    };
    let data = host.cluster.data(target);
    let (done, last_input) = inputs.split_at(inputs.len() - 1);
    for input in done {
        let delivered = Host::<Stm>::submit(&mut host, input.clone());
        assert!(
            matches!(delivered, Ok(Delivery::Recorded(_))),
            "{delivered:?}"
        );
        acknowledged += 1;
    }
    std::thread::sleep(Duration::from_secs(3));
    let before = journal_lengths(&data);
    let delivered = Host::<Stm>::submit(&mut host, last_input[0].clone());
    assert!(
        matches!(delivered, Ok(Delivery::Recorded(_))),
        "{delivered:?}"
    );
    acknowledged += 1;
    let last = last_input[0].record_id.as_str().to_owned();
    assert_eq!(host.status().unwrap().records, acknowledged);
    // Quiet, so the node has written whatever follows an append, then every node dies as a kill leaves it.
    std::thread::sleep(Duration::from_secs(3));
    let after = journal_lengths(&data);
    host.cluster.kill_all();

    // The newest journal file, and the bytes of it the last submit wrote.
    let (file, end) = after
        .iter()
        .next_back()
        .map(|(f, l)| (f.clone(), *l))
        .expect("a journal file");
    let begin = before.get(&file).copied().unwrap_or(0);
    let found: Vec<Place> = places(&data, last.as_bytes())
        .into_iter()
        .filter(|p| p.file == file && (begin..end).contains(&p.offset))
        .collect();
    assert!(
        !found.is_empty(),
        "the last acknowledged record id is nowhere in the {} bytes its submit wrote to node {target}'s journal",
        end - begin
    );
    let where_it_sat = format!(
        "the submit wrote bytes {begin}..{end} of {} and the id sits {} times in them, first at {}",
        file.strip_prefix(&data).unwrap_or(&file).display(),
        found.len(),
        found[0].offset
    );
    match at {
        Where::InTheRecord => {
            // One byte of the first place: inside the batch the append wrote, its length intact.
            damage(&found[..1]);
        }
        Where::LastByte => {
            damage(&[Place {
                file: file.clone(),
                offset: end - 3,
                after: 0,
            }]);
        }
        Where::TornInTheRecord => {
            let open = std::fs::OpenOptions::new()
                .write(true)
                .open(&file)
                .expect("a journal file");
            open.set_len((found[0].offset + last.len()) as u64)
                .expect("cutting a journal file");
        }
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
                    outcome: silence(&findings),
                    detail: report(format!("the node started and never held every record: {e}")),
                }
            }
        }
        (Ok(()), Ok((first, settled))) if *settled >= acknowledged => Finding {
            // A cut that left every record is not a recovery: the damage did not take the record, so the run
            // shows nothing about a torn tail.
            outcome: if at.is_cut() {
                "not reproduced"
            } else {
                "recovered"
            },
            detail: report(format!(
                "its own replica holds {first} then {settled} of {acknowledged} records{}",
                if at.is_cut() {
                    ", so the cut did not remove the record"
                } else {
                    ""
                }
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
                    outcome: if findings.is_empty() {
                        "cut and repaired by replication"
                    } else {
                        "cut and logged"
                    },
                    detail: report(format!(
                        "its own replica held {first} then {settled} of {acknowledged} records, and caught up from the others"
                    )),
                }
            } else {
                Finding {
                    outcome: silence(&findings),
                    detail: report(format!(
                        "its own replica holds {first} then {settled} of {acknowledged} records"
                    )),
                }
            }
        }
    };
    host.cluster.kill_all();
    finding
}

/// A loss is silent only if the node's log says nothing of damage.
fn silence(findings: &[String]) -> &'static str {
    if findings.is_empty() {
        "cut silently"
    } else {
        "cut and logged"
    }
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

/// Every place of damage on `replicas` nodes, the last submit ending the order or not: every one is recorded, and
/// every one must be refused or recovered, and none may be cut silently or not reproduced.
fn both(replicas: usize, setup: &str, ends_order: bool) {
    let setup = format!(
        "{setup}, last input {} the order",
        if ends_order { "ends" } else { "does not end" }
    );
    let mut unacceptable = Vec::new();
    for at in [Where::InTheRecord, Where::LastByte, Where::TornInTheRecord] {
        let finding = g54(
            replicas,
            &format!("g54-{replicas}-{}-{}", at as u8, u8::from(ends_order)),
            at,
            ends_order,
        );
        record(&setup, &finding);
        if !finding.acceptable() {
            unacceptable.push(format!("VOS: {}; {}", finding.outcome, finding.detail));
        }
    }
    assert!(unacceptable.is_empty(), "{}", unacceptable.join("\n"));
}

// verifies: NFR-6, INV-7
#[test]
fn damaged_last_append_that_ends_an_order_on_one_vos_node_is_refused_or_recovered_and_neither_cut_silently_nor_unreproduced()
 {
    both(1, "one node", true);
}

// verifies: NFR-6, INV-7
#[test]
fn damaged_last_append_that_leaves_an_order_running_on_one_vos_node_is_refused_or_recovered_and_neither_cut_silently_nor_unreproduced()
 {
    both(1, "one node", false);
}

// verifies: NFR-6, INV-7
#[test]
fn damaged_last_append_that_ends_an_order_on_the_vos_leader_of_three_is_refused_or_recovered_and_neither_cut_silently_nor_unreproduced()
 {
    both(3, "leader of three", true);
}

// verifies: NFR-6, INV-7
#[test]
fn damaged_last_append_that_leaves_an_order_running_on_the_vos_leader_of_three_is_refused_or_recovered_and_neither_cut_silently_nor_unreproduced()
 {
    both(3, "leader of three", false);
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
