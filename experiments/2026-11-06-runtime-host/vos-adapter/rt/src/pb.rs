//! The re-score's new rows on the purpose-built store (`pb`, `T-007-02`), under the same criteria as VOS's, wherever
//! they apply. `pb` runs in-process, through its public API alone; a real process kill uses a child process of this
//! binary (`pb-submit-child`), and the fsync count runs this binary under strace (`pb-load`).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rutent_conformance::host::probe::{self, Probe};
use rutent_contracts::state_machine::host::{Delivery, Host, HostError, SnapshotEntry};
use rutent_contracts::state_machine::{RecordedInput, StateMachine};
use rutent_stm::purpose_built_store::{
    DiskStore, FRAME_HEADER, PurposeBuiltHost, STORE_FORMAT, crc32,
};

use crate::passes::{Env, short};
use crate::rows::{orders, percentiles, strace_calls};

type Pb = PurposeBuiltHost<Probe>;

fn header(env: &Env, pass: &str) {
    println!("#### Pass `{pass}`, run {}\n", env.run_id);
    println!("| What | Value |\n|---|---|");
    print!("{}", env.facts.markdown());
    println!(
        "| Candidate | `pb`, `rutent-stm`'s `purpose-built-store`, in-process, one store on the runner's disk |\n"
    );
}

fn store(base: &Path, name: &str) -> DiskStore {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).expect("a store directory");
    DiskStore::at(dir)
}

fn verdict(pass: bool) -> &'static str {
    if pass { "pass" } else { "fail" }
}

/// How many acknowledged records, by record id, a journal set is missing.
fn lost(host: &Pb, acknowledged: &BTreeMap<String, (String, u64)>) -> usize {
    acknowledged
        .iter()
        .filter(|(id, (order, seq))| {
            let journal = rutent_contracts::state_machine::Id::new(order.clone())
                .and_then(|o| Host::<Probe>::journal(host, &o).ok())
                .unwrap_or_default();
            !journal
                .iter()
                .any(|r| r.seq == *seq && r.input.record_id.as_str() == *id)
        })
        .count()
}

fn acknowledge(
    acknowledged: &mut BTreeMap<String, (String, u64)>,
    delivered: &Result<Delivery, HostError>,
) {
    if let Ok(Delivery::Recorded(r) | Delivery::AlreadyRecorded(r)) = delivered {
        acknowledged.insert(
            r.input.record_id.as_str().to_string(),
            (r.order_id.as_str().to_string(), r.seq),
        );
    }
}

/// A child of this binary: opens the store at `dir`, submits the inputs of `PB_ORDERS` orders, and prints each
/// acknowledged record id on its own line. The parent kills it with SIGKILL.
pub fn submit_child(dir: &Path) -> bool {
    let count: usize = std::env::var("PB_ORDERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(50);
    let prefix = std::env::var("PB_PREFIX").unwrap_or_else(|_| "ord-k".into());
    let Ok(mut host) = Pb::open(DiskStore::at(dir)) else {
        return false;
    };
    for input in orders(&prefix, count).into_iter().flatten() {
        if let Ok(Delivery::Recorded(r) | Delivery::AlreadyRecorded(r)) = host.submit(input) {
            println!(
                "ack {} {} {}",
                r.input.record_id.as_str(),
                r.order_id.as_str(),
                r.seq
            );
        }
    }
    true
}

/// Runs `submit_child` on `dir`, kills it with SIGKILL after `after`, and gives back what it acknowledged.
fn killed_child(
    env: &Env,
    dir: &Path,
    prefix: &str,
    after: Duration,
) -> BTreeMap<String, (String, u64)> {
    let exe = std::env::current_exe().expect("this binary");
    let mut child = Command::new(exe)
        .args([env.run_id.as_str(), "pb-submit-child"])
        .arg(dir)
        .env("PB_PREFIX", prefix)
        .env("EXP1_DEV", if env.dev { "1" } else { "0" })
        .stdout(Stdio::piped())
        .spawn()
        .expect("the child starts");
    let stdout = child.stdout.take().unwrap();
    let acks = Arc::new(Mutex::new(BTreeMap::new()));
    let a = acks.clone();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let words: Vec<&str> = line.split_whitespace().collect();
            if let ["ack", id, order, seq] = words[..] {
                a.lock().unwrap().insert(
                    id.to_string(),
                    (order.to_string(), seq.parse().unwrap_or(0)),
                );
            }
        }
    });
    std::thread::sleep(after);
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    acks.lock().unwrap().clone()
}

/// The suite on `pb` against host contract 2.0.0, swaps into the reference host, with the fault seam: its first
/// scored run (`EXP-1-pb-20261107-1`) ran host contract 1.0.0.
pub fn suite(env: &Env) -> bool {
    header(env, "pb-suite");
    let report = rutent_conformance::host::run_with_fault_seam(
        rutent_conformance::host::RunConfig {
            run_id: env.run_id.clone(),
            candidate: "pb".into(),
            target: "reference-host".into(),
            seed: 20_261_106,
        },
        &mut rutent_stm::purpose_built_store::PurposeBuiltHarness::<Probe>::new(),
        &mut rutent_stm::reference_host::ReferenceHarness::<Probe>::new(),
    );
    println!("{}", report.to_markdown());
    let into = rutent_conformance::host::run_with_fault_seam(
        rutent_conformance::host::RunConfig {
            run_id: env.run_id.clone(),
            candidate: "reference-host".into(),
            target: "pb".into(),
            seed: 20_261_106,
        },
        &mut rutent_stm::reference_host::ReferenceHarness::<Probe>::new(),
        &mut rutent_stm::purpose_built_store::PurposeBuiltHarness::<Probe>::new(),
    );
    println!("\n{}", into.to_markdown());
    true
}

pub fn r01(env: &Env, base: PathBuf) -> bool {
    header(env, "pb-r01");
    println!(
        "`pb` has one node, so `R-01` is **fail (by construction)**. Its recovery path, a restart on the same disk after a SIGKILL mid-submit, is run for the note:\n"
    );
    println!(
        "| Round | Killed after, ms | Acknowledged before the kill | Restart (open), ms | Acknowledged lost |"
    );
    println!("|---|---|---|---|---|");
    let mut ok = true;
    for round in 1..=5u64 {
        let store = store(&base, &format!("k{round}"));
        let after = Duration::from_millis(150 + 70 * round);
        let acks = killed_child(env, store.dir(), &format!("ord-k{round}-"), after);
        let started = Instant::now();
        let host = Pb::open(store);
        let open = started.elapsed();
        let lost_now = host.as_ref().map_or(acks.len(), |h| lost(h, &acks));
        println!(
            "| {round} | {} | {} | {:.1} | {lost_now} |",
            after.as_millis(),
            acks.len(),
            open.as_secs_f64() * 1000.0
        );
        ok &= host.is_ok() && lost_now == 0;
    }
    println!(
        "\nVerdict: fail (by construction). The restarts lost {} acknowledged records.",
        if ok { "no" } else { "**some**" }
    );
    true
}

pub fn r03(env: &Env, base: PathBuf) -> bool {
    header(env, "pb-r03");
    let live = store(&base, "live");
    let mut host = Pb::open(live.clone()).expect("a store");
    let mut acknowledged = BTreeMap::new();
    for input in orders("ord-t", 10).into_iter().flatten() {
        let delivered = host.submit(input);
        acknowledge(&mut acknowledged, &delivered);
    }
    drop(host);
    // The copy at time T, taken of a closed store.
    let copy = base.join("copy");
    let copied = Command::new("cp")
        .arg("-R")
        .arg(live.dir())
        .arg(&copy)
        .status()
        .is_ok_and(|s| s.success());
    let mut host = Pb::open(live.clone()).expect("the store again");
    let mut after_t = BTreeMap::new();
    for input in orders("ord-u", 4).into_iter().flatten() {
        let delivered = host.submit(input);
        acknowledge(&mut after_t, &delivered);
    }
    drop(host);
    // The store is lost; the copy is restored in its place.
    let _ = std::fs::remove_dir_all(live.dir());
    let restored = Command::new("cp")
        .arg("-R")
        .arg(&copy)
        .arg(live.dir())
        .status()
        .is_ok_and(|s| s.success());
    let host = Pb::open(live);
    let lost_before = host
        .as_ref()
        .map_or(acknowledged.len(), |h| lost(h, &acknowledged));
    let lost_after = host.as_ref().map_or(after_t.len(), |h| lost(h, &after_t));
    println!("| What | Value |\n|---|---|");
    println!("| The copy at T, then restored | {} |", copied && restored);
    println!(
        "| The restored store opens | {} |",
        short(&host.as_ref().map(|_| ()))
    );
    println!(
        "| Acknowledged before T, lost | {lost_before} of {} |",
        acknowledged.len()
    );
    println!(
        "| Acknowledged after T, lost | {lost_after} of {} |",
        after_t.len()
    );
    println!(
        "| Were their effects released | yes: each was acknowledged, so released to its submitter, and an effect driver may have acted on it |"
    );
    let ok = host.is_ok() && lost_before == 0 && lost_after == 0;
    println!("\nVerdict: {}.", verdict(ok));
    true
}

pub fn r04a(env: &Env, base: PathBuf) -> bool {
    header(env, "pb-r04a");
    println!(
        "| Damage | Log bytes before | Opened | Acknowledged | Lost after the open | Result |"
    );
    println!("|---|---|---|---|---|---|");
    let mut ok = true;
    for how in ["truncated", "flipped"] {
        let s = store(&base, how);
        let mut host = Pb::open(s.clone()).expect("a store");
        let mut acknowledged = BTreeMap::new();
        for input in orders(&format!("ord-{how}-"), 10).into_iter().flatten() {
            let delivered = host.submit(input);
            acknowledge(&mut acknowledged, &delivered);
        }
        drop(host);
        let log = s.log_path();
        let len = std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0);
        if how == "truncated" {
            let f = std::fs::OpenOptions::new().write(true).open(&log).unwrap();
            f.set_len(len - 8).unwrap();
        } else {
            let mut bytes = std::fs::read(&log).unwrap();
            *bytes.last_mut().unwrap() ^= 0xff;
            std::fs::write(&log, bytes).unwrap();
        }
        let host = Pb::open(s);
        let lost_now = host.as_ref().map_or(0, |h| lost(h, &acknowledged));
        let pass = host.is_err() || lost_now == 0;
        ok &= pass;
        println!(
            "| last frame {how} | {len} | {} | {} | {lost_now} | {} |",
            match &host {
                Ok(_) => "yes".to_string(),
                Err(e) => format!("no: {}", short::<(), _>(&Err(e.clone()))),
            },
            acknowledged.len(),
            if pass {
                "pass"
            } else {
                "**fail: opened silently short**"
            }
        );
    }
    println!(
        "\nVerdict: {}. `R-04b` is n/a for `pb`: one node, so it is this row.",
        verdict(ok)
    );
    true
}

/// The load `pb-r05` measures: `submitters` threads sharing one host behind a mutex (the store holds an exclusive
/// lock, so one host is all a process can have), each submitting `per` orders.
fn pb_load(
    host: Pb,
    submitters: usize,
    per: usize,
) -> (
    Arc<Mutex<Pb>>,
    usize,
    Duration,
    Vec<u64>,
    Vec<Vec<RecordedInput>>,
    usize,
) {
    let host = Arc::new(Mutex::new(host));
    let started = Instant::now();
    let threads: Vec<_> = (0..submitters)
        .map(|t| {
            let host = host.clone();
            let inputs = orders(&format!("ord-c{t}-"), per);
            std::thread::spawn(move || {
                let mut latencies = Vec::new();
                let mut errors = 0;
                for input in inputs.iter().flatten() {
                    let at = Instant::now();
                    let delivered = host.lock().unwrap().submit(input.clone());
                    match delivered {
                        Ok(_) => latencies.push(at.elapsed().as_micros() as u64),
                        Err(_) => errors += 1,
                    }
                }
                (latencies, errors, inputs)
            })
        })
        .collect();
    let mut latencies = Vec::new();
    let mut submitted = Vec::new();
    let mut errors = 0;
    for thread in threads {
        let (l, e, i) = thread.join().unwrap();
        latencies.extend(l);
        errors += e;
        submitted.extend(i);
    }
    let elapsed = started.elapsed();
    (host, latencies.len(), elapsed, latencies, submitted, errors)
}

/// The child the fsync count runs under strace: the load alone, on the store at `dir`.
pub fn load_child(dir: &Path) -> bool {
    let submitters: usize = std::env::var("PB_SUBMITTERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);
    let per: usize = std::env::var("PB_PER")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(10);
    let Ok(host) = Pb::open(DiskStore::at(dir)) else {
        return false;
    };
    let (_, records, ..) = pb_load(host, submitters, per);
    println!("records {records}");
    true
}

pub fn r05(env: &Env, base: PathBuf, fsyncs: bool) -> bool {
    header(env, if fsyncs { "pb-r05-fsyncs" } else { "pb-r05" });
    println!(
        "| Candidate | Submitters | Records | Records/s | p50, µs | p99, µs | fsyncs/record | Guarantees under load |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    let mut ok = true;
    for submitters in [1usize, 8, 64] {
        let per = (800 / submitters).max(14) / 7;
        let s = store(&base, &format!("c{submitters}"));
        if fsyncs {
            let out = base.join(format!("c{submitters}.strace"));
            let exe = std::env::current_exe().unwrap();
            let status = Command::new("strace")
                .args([
                    "-f",
                    "-c",
                    "-e",
                    "trace=fsync,fdatasync,sync_file_range",
                    "-o",
                ])
                .arg(&out)
                .arg(exe)
                .args([env.run_id.as_str(), "pb-load"])
                .arg(s.dir())
                .env("PB_SUBMITTERS", submitters.to_string())
                .env("PB_PER", per.to_string())
                .env("EXP1_DEV", if env.dev { "1" } else { "0" })
                .stdout(Stdio::null())
                .status();
            let dir = base.join(format!("c{submitters}-strace"));
            std::fs::create_dir_all(&dir).unwrap();
            let _ = std::fs::rename(&out, dir.join("pb.strace"));
            let (calls, by) = strace_calls(&dir);
            let records = per * 7 * submitters;
            println!(
                "| pb | {submitters} | {records} | counted in `pb-r05` | | | {:.2} ({calls} calls: {by}) | strace ran: {} |",
                calls as f64 / records as f64,
                status.is_ok_and(|s| s.success())
            );
            continue;
        }
        let host = Pb::open(s).expect("a store");
        let (host, records, elapsed, mut latencies, submitted, errors) =
            pb_load(host, submitters, per);
        let host = host.lock().unwrap();
        let holds = errors == 0
            && submitted.iter().all(|inputs| {
                let journal =
                    Host::<Probe>::journal(&*host, &inputs[0].subject.order).unwrap_or_default();
                let read: Vec<RecordedInput> = journal.iter().map(|r| r.input.clone()).collect();
                let states: Vec<_> = journal.iter().map(|r| r.state_after.clone()).collect();
                &read == inputs && states == probe::fold(&read)
            });
        let (p50, p99) = percentiles(&mut latencies);
        println!(
            "| pb | {submitters} | {records} | {:.0} | {p50} | {p99} | counted in `pb-r05-fsyncs` | {} |",
            records as f64 / elapsed.as_secs_f64(),
            if holds { "hold" } else { "**broken**" }
        );
        ok &= holds;
    }
    println!(
        "\nOne host behind a mutex: `pb`'s store takes an exclusive lock, so its submits are serialised and each pays its own sync."
    );
    ok
}

pub fn r06a(env: &Env, base: PathBuf) -> bool {
    header(env, "pb-r06a");
    let s = store(&base, "redeploy");
    let mut host = Pb::open(s.clone()).expect("a store");
    let inputs = orders("ord-v", 2);
    for input in &inputs[1][..4] {
        let _ = host.submit(input.clone());
    }
    let before = Host::<Probe>::journal(&host, &inputs[1][0].subject.order).unwrap_or_default();
    drop(host);
    // The redeploy: a process of this binary submitting, killed mid-way, then this one opening the same disk.
    let acks = killed_child(env, s.dir(), "ord-w", Duration::from_millis(300));
    let started = Instant::now();
    let mut host = Pb::open(s).expect("the store after the redeploy");
    let reopened = started.elapsed();
    let after = Host::<Probe>::journal(&host, &inputs[1][0].subject.order).unwrap_or_default();
    let mut twice = 0;
    let mut in_flight_seen = 0;
    for n in 0..50 {
        let order = rutent_contracts::state_machine::Id::new(format!("ord-w{n}")).unwrap();
        let journal = Host::<Probe>::journal(&host, &order).unwrap_or_default();
        let mut ids: Vec<_> = journal.iter().map(|r| r.input.record_id.clone()).collect();
        let len = ids.len();
        ids.sort();
        ids.dedup();
        twice += len - ids.len();
        in_flight_seen += len;
    }
    let lost_now = lost(&host, &acks);
    let expected = before
        .last()
        .and_then(|r| r.state_after.clone())
        .map(|state| Probe::transition(&state, &inputs[1][4]));
    let next = host.submit(inputs[1][4].clone());
    let applied = matches!(&next, Ok(Delivery::Recorded(r)) if r.state_after == expected);
    println!("| What | Value |\n|---|---|");
    println!(
        "| The redeploy | a child process of this binary killed with SIGKILL mid-submit, then this process opened the same disk, in {:.1} ms |",
        reopened.as_secs_f64() * 1000.0
    );
    println!(
        "| Journals survived | {} |",
        if before == after { "yes" } else { "**no**" }
    );
    println!(
        "| Acknowledged by the killed process, lost | {lost_now} of {} |",
        acks.len()
    );
    println!(
        "| Records of the killed process's orders, any record id twice | {in_flight_seen}, {twice} |"
    );
    println!(
        "| The next input applied to the state from before the redeploy | {} |",
        if applied { "yes" } else { "**no**" }
    );
    let ok = before == after && lost_now == 0 && twice == 0 && applied;
    println!("\nVerdict: {}.", verdict(ok));
    true
}

pub fn r06b(env: &Env, base: PathBuf) -> bool {
    header(env, "pb-r06b");
    let s = store(&base, "layout");
    let mut host = Pb::open(s.clone()).expect("a store");
    for input in orders("ord-y", 2).into_iter().flatten() {
        let _ = host.submit(input);
    }
    drop(host);
    // The store's first frame, its format entry, rewritten to name the next layout, its checksum made good, as a
    // build with a changed layout would have written it. This build, which reads layout 1 alone, opens it.
    let log = s.log_path();
    let mut bytes = std::fs::read(&log).unwrap();
    let len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let payload = String::from_utf8(bytes[FRAME_HEADER..FRAME_HEADER + len].to_vec()).unwrap();
    let rewritten = payload.replacen(
        &format!("\"store_format\":{STORE_FORMAT}"),
        &format!("\"store_format\":{}", STORE_FORMAT + 1),
        1,
    );
    let changed = rewritten != payload && rewritten.len() == payload.len();
    bytes[FRAME_HEADER..FRAME_HEADER + len].copy_from_slice(rewritten.as_bytes());
    let crc = crc32(&[&bytes[..4], rewritten.as_bytes()]);
    bytes[4..8].copy_from_slice(&crc.to_le_bytes());
    std::fs::write(&log, &bytes).unwrap();
    let opened = Pb::open(s);
    println!("| What | Value |\n|---|---|");
    println!(
        "| The store's format entry, rewritten from layout {STORE_FORMAT} to {}, its checksum good | {changed} |",
        STORE_FORMAT + 1
    );
    println!(
        "| This build, which reads layout {STORE_FORMAT} alone, opening it | {} |",
        short(&opened.as_ref().map(|_| ()))
    );
    let ok = changed && opened.is_err();
    println!(
        "\nThe check is symmetric: a build of another layout refuses a layout-{STORE_FORMAT} store the same way. Verdict: {}.",
        verdict(ok)
    );
    true
}

pub fn r08(env: &Env, base: PathBuf) -> bool {
    header(env, "pb-r08");
    println!(
        "| Records | Filled by | Fill, s | Halt to export done, ms | Import start to ready, ms | RTO, ms (their sum) | Imported equal | RPO |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    let mut ok = true;
    for records in env.r08_sizes() {
        let host = Pb::open(store(&base, &format!("x{records}"))).expect("a store");
        let fill_started = Instant::now();
        let (host, filled, ..) = pb_load(host, 64, records / 7 / 64 + 1);
        let fill = fill_started.elapsed();
        let mut host = Arc::try_unwrap(host).ok().unwrap().into_inner().unwrap();
        let halt_started = Instant::now();
        let exported: Result<Vec<SnapshotEntry>, HostError> = Host::<Probe>::halt(&mut host)
            .and_then(|()| Host::<Probe>::export(&host))
            .and_then(|s| s.collect());
        let export = halt_started.elapsed();
        let Ok(entries) = exported else {
            println!("| {records} | the export failed |||||||");
            ok = false;
            continue;
        };
        let import_started = Instant::now();
        let imported = Pb::import(
            store(&base, &format!("y{records}")),
            Box::new(entries.clone().into_iter().map(Ok)),
        );
        let import = import_started.elapsed();
        let equal = imported
            .as_ref()
            .ok()
            .and_then(|t| Host::<Probe>::export(t).ok())
            .map(|s| s.collect::<Result<Vec<_>, _>>())
            .is_some_and(|again| again.is_ok_and(|again| again == entries));
        println!(
            "| {filled} | 64 concurrent submitters behind one mutex, 7 inputs an order | {:.1} | {} | {} | {} | {} | the export interval |",
            fill.as_secs_f64(),
            export.as_millis(),
            import.as_millis(),
            (export + import).as_millis(),
            if equal { "yes" } else { "**no**" }
        );
        ok &= equal;
    }
    ok
}

pub fn r09(env: &Env, base: PathBuf) -> bool {
    header(env, "pb-r09");
    let mut host = Pb::open(store(&base, "steps")).expect("a store");
    let inputs: Vec<RecordedInput> = orders("ord-z", 30).into_iter().flatten().collect();
    let mut latencies = Vec::new();
    for input in &inputs {
        let at = Instant::now();
        let _ = host.submit(input.clone());
        latencies.push(at.elapsed().as_micros() as u64);
    }
    let submit_mean = latencies.iter().sum::<u64>() / latencies.len() as u64;
    // The state machine's step alone, natively: the probe folded over the same inputs, many times.
    let rounds = 200;
    let started = Instant::now();
    for _ in 0..rounds {
        for order in inputs.chunks(7) {
            let mut state = Probe::open(&order[0]);
            for input in &order[1..] {
                state = state.map(|s| Probe::transition(&s, input));
            }
            std::hint::black_box(&state);
        }
    }
    let step = started.elapsed().as_secs_f64() * 1e6 / (rounds * inputs.len()) as f64;
    println!("| What | Value |\n|---|---|");
    println!("| Submits, one after another | {} |", inputs.len());
    println!("| A submit's mean, µs, its sync included | {submit_mean} |");
    println!("| The state machine's step alone, natively, mean, µs | {step:.2} |");
    println!("| Gas | n/a: `pb` runs native code, unmetered |");
    true
}
