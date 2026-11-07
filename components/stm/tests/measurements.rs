//! Measurements of the purpose-built store for `EXP-1` (`T-007-02`): a journal past 1,024 entries and 64 MiB,
//! the store's size caps, and sequential synced throughput. They write tens of megabytes and sync every append,
//! so they are `#[ignore]`d and run on purpose:
//!
//! ```text
//! cargo test --release -p rutent-stm --features rutent-stm/purpose-built-store --test measurements \
//!     -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Each prints its numbers, which the run note quotes. They are measurements, not targets (`PLAN.md` §6.2,
//! risk 9). Built only with the `purpose-built-store` feature.

#![cfg(feature = "purpose-built-store")]

use std::time::Instant;

use rutent_conformance::host::probe::{self, Probe};
use rutent_contracts::state_machine::host::testing::Harness;
use rutent_contracts::state_machine::host::{Delivery, Host, HostError};
use rutent_contracts::state_machine::{Id, Observed, PartPlan, RecordedInput};
use rutent_stm::purpose_built_store::{FRAME_HEADER, PurposeBuiltHarness, PurposeBuiltHost};

type Pb = PurposeBuiltHost<Probe>;

const MIB: usize = 1 << 20;

/// The largest frame payload the store accepts (`MAX_PAYLOAD` in the module, 64 MiB).
const FRAME_CAP: usize = 64 << 20;

/// A segment settled with evidence of the given length: the one field of the probe scenario whose size is ours.
fn settled(order: &str, record: &str, tick: u32, evidence_len: usize) -> RecordedInput {
    probe::input(
        order,
        record,
        tick,
        Some("p1-s1"),
        Observed::SegmentSettled {
            evidence: Id::new("e".repeat(evidence_len)).unwrap(),
        },
    )
}

fn json_len<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).unwrap().len()
}

/// Every journal of the host is gapless and each recorded state is the probe's fold of the journal's inputs, as
/// `H-17` asks, through the state machine alone.
fn replay_matches(host: &Pb) -> usize {
    let mut entries = 0;
    for order in host.orders().unwrap() {
        let journal = host.journal(&order).unwrap();
        let expected = probe::fold(journal.iter().map(|r| &r.input));
        for (n, (r, state)) in journal.iter().zip(&expected).enumerate() {
            assert_eq!(r.seq, n as u64 + 1, "a gap in {}", order.as_str());
            assert_eq!(&r.state_after, state, "seq {} of {}", r.seq, order.as_str());
        }
        entries += journal.len();
    }
    entries
}

// verifies: REQ-OPS-3, NFR-6, DEC-6, DEC-5
#[test]
#[ignore = "writes about 90 MiB with a sync per append; run on purpose"]
fn g01_a_journal_past_1024_entries_and_64_mib_replays_whole() {
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let store = harness.fresh_store().unwrap();
    let mut host = Pb::open(store.clone()).unwrap();
    let order = "ord-big";
    let route = probe::order_inputs(order, 0).remove(0);
    let mut inputs = vec![route];
    // 1,200 settled inputs of about 80 KB each, under distinct record ids: past 1,024 entries and 64 MiB.
    for n in 0..1_200u32 {
        inputs.push(settled(order, &format!("rec-big-{n}"), 1 + n, 80_000));
    }
    let started = Instant::now();
    for input in &inputs {
        assert!(matches!(
            host.submit(input.clone()).unwrap(),
            Delivery::Recorded(_)
        ));
    }
    let wrote = started.elapsed();
    let journal = host.journal(&Id::new(order).unwrap()).unwrap();
    let journal_bytes: usize = journal.iter().map(json_len).sum();
    let log_bytes = std::fs::metadata(store.log_path()).unwrap().len();
    assert!(journal.len() > 1_024, "{} entries", journal.len());
    assert!(journal_bytes > 64 * MIB, "{journal_bytes} bytes");
    let before = replay_matches(&host);

    // Kill, reopen from the log alone, and replay again.
    let store = harness.kill(host).unwrap();
    let reopened_at = Instant::now();
    let host = Pb::open(store).unwrap();
    let reopen = reopened_at.elapsed();
    let after = replay_matches(&host);
    let again = host.journal(&Id::new(order).unwrap()).unwrap();
    assert_eq!(again, journal, "the journal changed across the reopen");
    assert_eq!(host.orders().unwrap(), vec![Id::new(order).unwrap()]);
    println!(
        "G-01: one order, {} journal entries, {journal_bytes} bytes of journal JSON ({} MiB), log file \
         {log_bytes} bytes; replay through the state machine alone matched every recorded state, {before} entries \
         before the kill and {after} after the reopen; written in {wrote:.1?}, reopened in {reopen:.1?}",
        journal.len(),
        journal_bytes / MIB
    );
}

/// A route chosen with `parts` parts, each the probe's three-segment route: an input and, once opened, a state
/// that both grow with the number of parts. Part ids are of one width, so the size is linear in `parts`.
fn route_with(parts: usize) -> RecordedInput {
    let mut input = probe::order_inputs("ord-cap", 0).remove(0);
    input.observed = Observed::RouteChosen {
        decision_record: Id::new("dr-cap").unwrap(),
        parts: (0..parts)
            .map(|n| PartPlan {
                part_id: Id::new(format!("p{n:08}")).unwrap(),
                segments: probe::route(),
            })
            .collect(),
        handed_on_from: None,
    };
    input
}

/// What a submit of `input` to a fresh store (after `first`, if any) gives: the frame's payload size, the input's
/// and the state's JSON size, or the error; and whether a small input is recorded right after.
fn try_submit(
    first: Option<RecordedInput>,
    input: RecordedInput,
) -> (Result<(usize, usize, usize), HostError>, bool) {
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let store = harness.fresh_store().unwrap();
    let mut host = Pb::open(store.clone()).unwrap();
    if let Some(first) = first {
        host.submit(first).unwrap();
    }
    let before = std::fs::metadata(store.log_path()).unwrap().len() as usize;
    let out = host.submit(input).map(|d| match d {
        Delivery::Recorded(r) => {
            let after = std::fs::metadata(store.log_path()).unwrap().len() as usize;
            (
                after - before - FRAME_HEADER,
                json_len(&r.input),
                json_len(&r.state_after),
            )
        }
        Delivery::AlreadyRecorded(_) => unreachable!("a fresh record id"),
    });
    let goes_on = host.submit(settled("ord-small", "rec-small", 3, 8)).is_ok();
    (out, goes_on)
}

/// The largest `n` whose input is accepted, from two measured sizes, then checked at the edge: `n` accepted,
/// `n + 1` refused. Returns `n`, the payload, input and state sizes at `n`, the per-unit slope, the fixed part and
/// what the refusal said and whether the host went on after each.
fn edge(
    make: impl Fn(usize) -> (Option<RecordedInput>, RecordedInput),
    small: usize,
    big: usize,
) -> (
    usize,
    (usize, usize, usize),
    usize,
    usize,
    String,
    bool,
    bool,
) {
    let measure = |n: usize| {
        let (first, input) = make(n);
        try_submit(first, input).0.expect("a small record fits").0
    };
    let (a, b) = (measure(small), measure(big));
    let slope = (b - a) / (big - small);
    let fixed = a - small * slope;
    let n = (FRAME_CAP - fixed) / slope;
    let (first, input) = make(n);
    let (fit, goes_on_fit) = try_submit(first, input);
    let sizes = fit.expect("the largest n that fits is accepted");
    let (first, input) = make(n + 1);
    let (over, goes_on_over) = try_submit(first, input);
    let refused = over.expect_err("one more than a frame holds is refused");
    assert!(matches!(refused, HostError::Unavailable { .. }));
    assert!(
        sizes.0 <= FRAME_CAP && sizes.0 + slope > FRAME_CAP,
        "{sizes:?}"
    );
    (
        n,
        sizes,
        slope,
        fixed,
        format!("{refused:?}"),
        goes_on_fit,
        goes_on_over,
    )
}

// verifies: REQ-OPS-3, NFR-6, DEC-6, DEC-5
#[test]
#[ignore = "writes a 64 MiB frame several times; run on purpose"]
fn g32_the_frame_cap_and_the_largest_record_it_accepts() {
    // Evidence id: grows the input only (the state keeps no evidence text).
    let first = || probe::order_inputs("ord-cap", 0).remove(0);
    let (n, (payload, input, state), slope, fixed, refused, fit_ok, over_ok) = edge(
        |n| (Some(first()), settled("ord-cap", "rec-cap-1", 1, n)),
        10,
        1_010,
    );
    println!(
        "G-32 evidence: frame cap {FRAME_CAP} bytes ({} MiB) of payload, behind an 8-byte header (4-byte length, 4-byte CRC-32); a \
         settled input's frame payload is {fixed} bytes plus {slope} per byte of evidence id; the \
         longest accepted has evidence of {n} bytes: payload {payload} bytes, input JSON {input}, state JSON \
         {state}; one byte more is refused with {refused}; the host goes on after the accepted record: {fit_ok}, \
         after the refused one: {over_ok}",
        FRAME_CAP / MIB
    );

    // Parts: grow the input and the state together.
    let (n, (payload, input, state), slope, fixed, refused, fit_ok, over_ok) =
        edge(|n| (None, route_with(n)), 10, 110);
    println!(
        "G-32 parts: a route chosen's frame payload is {fixed} bytes plus {slope} per part of three segments; the \
         most parts accepted is {n}: payload {payload} bytes, input JSON {input}, state JSON {state}; one part \
         more is refused with {refused}; the host goes on after the accepted record: {fit_ok}, after the refused \
         one: {over_ok}"
    );
}
// verifies: REQ-OPS-3, NFR-6, DEC-6, DEC-5
#[test]
#[ignore = "syncs every append for several seconds; run on purpose"]
fn g42_sequential_synced_submits_per_second() {
    // Distinct orders of seven inputs each (the probe scenario), one host, one disk, one thread; every submit is
    // written and synced before it returns. Three rounds, each on a fresh store.
    const ORDERS: u32 = 400;
    for round in 1..=3 {
        let mut harness = PurposeBuiltHarness::<Probe>::new();
        let store = harness.fresh_store().unwrap();
        let mut host = Pb::open(store.clone()).unwrap();
        let mut inputs = Vec::new();
        for n in 0..ORDERS {
            inputs.extend(probe::order_inputs(&format!("ord-{n}"), 0));
        }
        let mut each = Vec::with_capacity(inputs.len());
        let started = Instant::now();
        for input in inputs.iter().cloned() {
            let one = Instant::now();
            assert!(matches!(host.submit(input).unwrap(), Delivery::Recorded(_)));
            each.push(one.elapsed());
        }
        let total = started.elapsed();
        each.sort();
        let at = |p: f64| each[((each.len() as f64 * p) as usize).min(each.len() - 1)];
        let log = std::fs::metadata(store.log_path()).unwrap().len();
        println!(
            "G-42 round {round}: {} sequential synced submits in {total:.2?} = {:.0} submits/s; per submit \
             p50 {:.2?}, p99 {:.2?}, max {:.2?}; log {log} bytes",
            inputs.len(),
            inputs.len() as f64 / total.as_secs_f64(),
            at(0.50),
            at(0.99),
            each.last().unwrap()
        );
    }
}

// verifies: REQ-OPS-3, NFR-6, DEC-6, DEC-5
#[test]
#[ignore = "a measurement for the scoring sheet; run on purpose"]
fn g32_the_largest_input_fixture_and_its_record() {
    // The same fixture `G-32` measures VOS with: three parts, nine segments.
    let fixture = include_str!(
        "../../../contracts/state-machine/fixtures/valid/recorded-input-route-chosen-three-parts.json"
    );
    let input: RecordedInput = serde_json::from_str(fixture).unwrap();
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let store = harness.fresh_store().unwrap();
    let mut host = Pb::open(store.clone()).unwrap();
    // The format entry is the log's first frame; take it out of the count.
    host.submit(probe::order_inputs("ord-first", 0).remove(0))
        .unwrap();
    let before = std::fs::metadata(store.log_path()).unwrap().len() as usize;
    let Delivery::Recorded(record) = host.submit(input.clone()).unwrap() else {
        unreachable!("a fresh record id")
    };
    let after = std::fs::metadata(store.log_path()).unwrap().len() as usize;
    println!(
        "G-32 fixture: recorded-input-route-chosen-three-parts.json ({} parts), input JSON {} bytes, state JSON {} \
         bytes, record JSON {} bytes, the log's frame {} bytes (payload {} of the {FRAME_CAP}-byte cap, behind an \
         {FRAME_HEADER}-byte header)",
        match &input.observed {
            Observed::RouteChosen { parts, .. } => parts.len(),
            _ => 0,
        },
        json_len(&record.input),
        json_len(&record.state_after),
        json_len(&record),
        after - before,
        after - before - FRAME_HEADER
    );
}
