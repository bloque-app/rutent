//! Runs rutent's host-contract suite on the VOS host for `EXP-1` and prints its report as Markdown.
//!
//! ```text
//! exp1-vos <run-id> <pass> <base>
//! ```
//!
//! `<pass>` is one of:
//!
//! - `vos-to-ref`: VOS is the candidate, with every cell and the fault seam; swaps go from VOS to the reference
//!   host;
//! - `ref-to-vos`: the reference host is the candidate; swaps go from it to VOS, so VOS's import is scored;
//! - `compacted`: as `vos-to-ref`, with VOS's log compacted every 16 entries to a tail of 4 (`compact 16 4`);
//! - `control-no-hold`: as `vos-to-ref`, but VOS does not hold commits at the after-computed stop. A control:
//!   `stop-after-computed` must then fail, showing that the append reached VOS before the kill;
//! - `compaction`: VOS's log compacted every 16 entries to a tail of 4; 40 orders' inputs recorded, then the
//!   replica's log start read, the node killed and reopened, and every journal checked whole and replayed;
//! - `session-expiry`: one order recorded, the node's clock moved past VOS's session expiry with its `clock`
//!   fault, another order recorded, the node killed and reopened, and every input's source read back;
//! - `sizes`: the largest input and state the suite's probe writes, as canonical JSON, against VOS's caps;
//! - `export-file`: for the re-score's `R-07a` and `R-07b`, three orders part-way and a refusal recorded on this
//!   build's VOS, the host halted, its snapshot stream written to `<base>/stream.jsonl`, one entry a line, and the
//!   node killed, leaving its data directory at `<base>/x1` for the tip's node to open.
//!
//! The paths come from the environment: `VOS_BIN` (a `vos` built with `--features faults`), `VOS_AGENT_ELF`
//! and `JOURNAL_ELF`. Each pass makes its stores under `<base>`, which it must not hold yet.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rutent_conformance::host::probe::{self, Probe};
use rutent_conformance::host::{RunConfig, run_with_fault_seam};
use rutent_contracts::state_machine::Id;
use rutent_contracts::state_machine::host::Host;
use rutent_contracts::state_machine::host::testing::Harness;
use rutent_stm::reference_host::ReferenceHarness;
use rutent_vos_adapter::{VosConfig, VosHarness, canonical};

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set")))
}

fn config(base: PathBuf, settings: &str) -> VosConfig {
    VosConfig {
        vos: env_path("VOS_BIN"),
        agent_elf: env_path("VOS_AGENT_ELF"),
        journal_elf: env_path("JOURNAL_ELF"),
        base,
        settings: settings.into(),
        wait: Duration::from_secs(60),
        computed_kill_after: Duration::from_millis(1_000),
        hold_at_computed: true,
    }
}

fn run(run_id: &str, candidate: &str, target: &str) -> RunConfig {
    RunConfig {
        run_id: run_id.into(),
        candidate: candidate.into(),
        target: target.into(),
        seed: 20_261_106,
    }
}

/// The largest input fixture of the state-machine contract, submitted to VOS, and the record VOS holds for it.
fn largest_fixture(base: PathBuf) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../contracts/state-machine/fixtures/valid/recorded-input-route-chosen-three-parts.json"
    );
    let text = std::fs::read_to_string(path).expect("the fixture");
    let input: rutent_contracts::state_machine::RecordedInput =
        serde_json::from_str(&text).expect("a recorded input");
    let mut harness = VosHarness::<Probe>::new(config(base, ""), "z");
    let store = harness.fresh_store().expect("a store");
    let mut host = harness.start(store).expect("a host");
    host.submit(input.clone()).expect("a submit");
    let journal = host.journal(&input.subject.order).expect("a journal");
    let record = &journal[0];
    let input_bytes = canonical(&record.input).expect("an input").len();
    let state_bytes = record
        .state_after
        .as_ref()
        .map_or(0, |s| canonical(s).expect("a state").len());
    let parts = record.state_after.as_ref().map_or(0, |s| s.parts.len());
    let segments: usize = record
        .state_after
        .as_ref()
        .map_or(0, |s| s.parts.iter().map(|p| p.segments.len()).sum());
    let key = record
        .input
        .record_id
        .as_str()
        .len()
        .max(record.order_id.as_str().len());
    let value = input_bytes + state_bytes + 16;
    let request = input_bytes + state_bytes + key * 2 + 32;
    println!(
        "\nThe largest input fixture, `recorded-input-route-chosen-three-parts.json`, recorded by VOS ({parts} parts, \
         {segments} segments), against each of VOS's caps (`agent/wire/src/abi/cap.rs:9-18`):\n"
    );
    println!("| Cap | VOS's limit, bytes | This record, bytes | What it is |");
    println!("|---|---|---|---|");
    println!(
        "| A key | 1024 | {key} | The longest key the journal writes: a record id or an order id; its other keys are fixed-width integers |"
    );
    println!(
        "| A value | 65536 | about {value} | The record's row: its input ({input_bytes}) and its state ({state_bytes}) as canonical JSON, framed |"
    );
    println!(
        "| A request | 65536 | about {request} | The append's arguments: order id, sequence, record id, input, state |"
    );
    println!(
        "| A reply | 65536 | at most 40960 plus framing | A page of a journal read, which the journal caps at 40 KiB (`PAGE_BYTES`), and at least one record; one record is about {value} |"
    );
    println!(
        "| Writes in one step | 262144 | about {} | An append's rows: the record, its record id's place, its order's row and place |",
        value + 3 * key + 64
    );
    let _ = harness.kill(host);
}

fn sizes() {
    let mut inputs = probe::order_inputs("ord-1", 0);
    inputs.push(probe::orphan(50));
    let states = probe::fold(&inputs);
    let input = inputs
        .iter()
        .map(|i| canonical(i).expect("an input serialises").len())
        .max()
        .unwrap_or(0);
    let state = states
        .iter()
        .flatten()
        .map(|s| canonical(s).expect("a state serialises").len())
        .max()
        .unwrap_or(0);
    println!("| What | Largest, bytes | VOS cap, bytes |");
    println!("|---|---|---|");
    println!("| An input, canonical JSON | {input} | 65536 (a value) |");
    println!("| A state, canonical JSON | {state} | 65536 (a value) |");
    println!(
        "| An append's arguments, about | {} | 65536 (a request) |",
        input + state + 64
    );
    println!("| Inputs in one order | {} | |", inputs.len() - 1);
}

/// Records 40 orders' inputs on a VOS whose log compacts every 16 entries, then checks each journal is whole after
/// a kill and reopen.
fn compaction(base: PathBuf) {
    let mut harness = VosHarness::<Probe>::new(config(base, "compact 16 4"), "k");
    let store = harness.fresh_store().expect("a store");
    let mut host = harness.start(store).expect("a host");
    let mut sent = Vec::new();
    for n in 0..40u32 {
        let inputs = probe::order_inputs(&format!("ord-k{n}"), 1_000 + 10 * n);
        for input in &inputs {
            host.submit(input.clone()).expect("a submit");
        }
        sent.push(inputs);
    }
    let (log_start, last_index, applied, _) = host.log_status().expect("the replica's status");
    let stages = host.stage_means().expect("the node's stats");
    let store = harness.kill(host).expect("a kill");
    let host = harness.reopen(store).expect("a reopen");
    let (after_start, after_last, after_applied, _) =
        host.log_status().expect("the replica's status");
    let mut whole = 0;
    let mut replayed = 0;
    for inputs in &sent {
        let order = Id::new(inputs[0].subject.order.as_str()).expect("an id");
        let journal = host.journal(&order).expect("a journal");
        let read: Vec<_> = journal.iter().map(|r| r.input.clone()).collect();
        if &read == inputs {
            whole += 1;
        }
        let states: Vec<_> = journal.iter().map(|r| r.state_after.clone()).collect();
        if states == probe::fold(&read) {
            replayed += 1;
        }
    }
    let records: usize = sent.iter().map(Vec::len).sum();
    println!("| What | Value |");
    println!("|---|---|");
    println!(
        "| Settings | `compact 16 4`, standing in for VOS's defaults of 1,024 entries or 64 MiB to a tail of 256, which 280 records would not reach |"
    );
    println!("| Orders, records submitted | {}, {records} |", sent.len());
    println!(
        "| Before the kill: log start, last index, applied | {log_start}, {last_index}, {applied} |"
    );
    println!(
        "| After the reopen: log start, last index, applied | {after_start}, {after_last}, {after_applied} |"
    );
    println!(
        "| Journals whole after the reopen, inputs equal to those submitted | {whole} of {} |",
        sent.len()
    );
    println!(
        "| Journals whose states the probe's fold reaches | {replayed} of {} |",
        sent.len()
    );
    println!(
        "\nThe node's stages over those submits, as its operator's stats read them before the kill (a measurement, not a target):\n"
    );
    println!("| Stage | Values | Mean, µs where timed |");
    println!("|---|---|---|");
    for (name, count, mean) in stages {
        if count > 0 {
            println!("| `{name}` | {count} | {mean} |");
        }
    }
    let _ = harness.kill(host);
}

/// Records one order from a first session; moves the node's clock just past VOS's session expiry and records half
/// of another order, which needs a second session; moves it past the first session's collection bucket and records
/// the rest; then kills and reopens the node and reads back every input's source. The count of sessions the
/// replica holds shows the first one expired (a second one appears) and then collected (the count falls back).
fn session_expiry(base: PathBuf) {
    // VOS's SESSION_EXPIRY_MS (agent/wire/src/defaults.rs:6) is an hour; an expired session is collected at most
    // SESSION_BUCKET_MS, five minutes (:13), after it expires.
    const EXPIRED_MS: i64 = 61 * 60 * 1000;
    const COLLECTED_MS: i64 = 75 * 60 * 1000;
    let mut harness = VosHarness::<Probe>::new(config(base, ""), "e");
    let store = harness.fresh_store().expect("a store");
    let mut host = harness.start(store).expect("a host");
    let first = probe::order_inputs("ord-e1", 2_000);
    for input in &first {
        host.submit(input.clone()).expect("a submit");
    }
    let sessions = |host: &rutent_vos_adapter::VosHost<Probe>| {
        host.log_status().expect("the replica's status").3
    };
    let at_start = sessions(&host);
    let second = probe::order_inputs("ord-e2", 2_100);
    let (early, rest) = second.split_at(second.len() / 2);
    let mut late = 0;
    host.move_clock(EXPIRED_MS).expect("the clock fault");
    for input in early {
        match host.submit(input.clone()) {
            Ok(_) => late += 1,
            Err(e) => println!("A submit after the clock moved: {e:?}"),
        }
    }
    let expired = sessions(&host);
    host.move_clock(COLLECTED_MS).expect("the clock fault");
    for input in rest {
        match host.submit(input.clone()) {
            Ok(_) => late += 1,
            Err(e) => println!("A submit after the clock moved: {e:?}"),
        }
    }
    // The leader sweeps due sessions in batches of its own: poll up to 30 s for the count to fall.
    let mut collected = sessions(&host);
    let mut waited = 0;
    while collected >= expired && waited < 30 {
        std::thread::sleep(Duration::from_secs(1));
        waited += 1;
        collected = sessions(&host);
    }
    let store = harness.kill(host).expect("a kill");
    let host = harness.reopen(store).expect("a reopen");
    let reopened = sessions(&host);
    let mut intact = 0;
    let mut total = 0;
    for inputs in [&first, &second] {
        let order = Id::new(inputs[0].subject.order.as_str()).expect("an id");
        let journal = host.journal(&order).expect("a journal");
        for (sent, record) in inputs.iter().zip(&journal) {
            total += 1;
            if sent.source == record.input.source && sent == &record.input {
                intact += 1;
            }
        }
    }
    println!("| What | Value |");
    println!("|---|---|");
    println!(
        "| Clock moved ahead, through VOS's `clock` fault | {EXPIRED_MS} ms, then {COLLECTED_MS} ms |"
    );
    println!(
        "| Inputs recorded: before the moves, after them | {}, {late} of {} |",
        first.len(),
        second.len()
    );
    println!(
        "| Sessions the replica holds: after the first order, after the first move and its submits, after the second move and its submits ({waited} s of polling), after a kill and reopen | {at_start}, {expired}, {collected}, {reopened} |"
    );
    println!(
        "| Inputs read back after the reopen, source and all equal to those submitted | {intact} of {total} (expected {}) |",
        first.len() + second.len()
    );
    let _ = harness.kill(host);
}

/// Records three orders part-way and one refusal on this build's VOS, halts, writes the snapshot stream to
/// `<base>/stream.jsonl` and kills the node, leaving its data directory at `<base>/x1`.
fn export_file(base: PathBuf) {
    let mut harness = VosHarness::<Probe>::new(config(base.clone(), ""), "x");
    let store = harness.fresh_store().expect("a store");
    let mut host = harness.start(store).expect("a host");
    let mut first = None;
    for (n, keep) in [(0u32, 4usize), (1, 6), (2, 2)] {
        let inputs = probe::order_inputs(&format!("ord-pin{n}"), 100 * n);
        for input in &inputs[..keep] {
            host.submit(input.clone()).expect("a submit");
        }
        first.get_or_insert(inputs[1].clone());
    }
    let reused = probe::reusing(first.as_ref().expect("an input"));
    let refused = host.submit(reused);
    host.halt().expect("a halt");
    let entries: Vec<_> = host
        .export()
        .expect("an export")
        .collect::<Result<_, _>>()
        .expect("the stream");
    let lines: String = entries
        .iter()
        .map(|e| serde_json::to_string(e).expect("an entry serialises") + "\n")
        .collect();
    std::fs::write(base.join("stream.jsonl"), lines).expect("the stream file");
    let _ = harness.kill(host);
    println!("| What | Value |");
    println!("|---|---|");
    println!("| VOS | vos-core at 1aadf44, the first run's adapter and journal |");
    println!("| Entries written to `stream.jsonl` | {} |", entries.len());
    println!("| The reused record id | refused: {} |", refused.is_err());
    println!("| The data directory left for `R-07b` | `x1` |");
}

/// The report, its verdict line saying it is a pre-freeze development run when the host contract is still 0.x,
/// before the `C1` freeze. From 1.0.0 the report is a scored run's and is left as the suite printed it.
fn pre_freeze(markdown: String) -> String {
    if !rutent_contracts::state_machine::host::VERSION.starts_with("0.") {
        return markdown;
    }
    markdown.replacen(
        ". Verdict: ",
        ". Pre-freeze development run (host contract 0.x, before `C1`). Verdict: ",
        1,
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, run_id, pass, base] = &args[..] else {
        eprintln!("usage: exp1-vos <run-id> <pass> <base>");
        std::process::exit(2);
    };
    let base = PathBuf::from(base);
    std::fs::create_dir_all(&base).expect("the base directory");
    let started = Instant::now();
    let report = match pass.as_str() {
        "vos-to-ref" => run_with_fault_seam(
            run(run_id, "vos", "reference-host"),
            &mut VosHarness::<Probe>::new(config(base, ""), "v"),
            &mut ReferenceHarness::<Probe>::new(),
        ),
        "ref-to-vos" => run_with_fault_seam(
            run(run_id, "reference-host", "vos"),
            &mut ReferenceHarness::<Probe>::new(),
            &mut VosHarness::<Probe>::new(config(base, ""), "t"),
        ),
        "compacted" => run_with_fault_seam(
            run(run_id, "vos (compact 16 4)", "reference-host"),
            &mut VosHarness::<Probe>::new(config(base, "compact 16 4"), "c"),
            &mut ReferenceHarness::<Probe>::new(),
        ),
        "control-no-hold" => {
            let mut config = config(base, "");
            config.hold_at_computed = false;
            run_with_fault_seam(
                run(run_id, "vos (control: no hold)", "reference-host"),
                &mut VosHarness::<Probe>::new(config, "n"),
                &mut ReferenceHarness::<Probe>::new(),
            )
        }
        "sizes" => {
            sizes();
            largest_fixture(base);
            return;
        }
        "compaction" => {
            compaction(base);
            return;
        }
        "session-expiry" => {
            session_expiry(base);
            return;
        }
        "export-file" => {
            export_file(base);
            return;
        }
        other => {
            eprintln!("no pass {other}");
            std::process::exit(2);
        }
    };
    println!("{}", pre_freeze(report.to_markdown()));
    println!(
        "\nElapsed: {} s (a measurement, not a target).",
        started.elapsed().as_secs()
    );
}
