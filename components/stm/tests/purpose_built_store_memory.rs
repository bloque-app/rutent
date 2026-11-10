//! What the purpose-built store holds in memory (`T-010-06`, the `M1` retrospective): an index, not the log. The
//! store used to keep every record it had ever written; a journal of 90 MiB cost 90 MiB of memory and more. This
//! file measures live heap bytes with a counting allocator, so it holds one test and nothing else runs beside it.
//! Built only with the `purpose-built-store` feature.

#![cfg(feature = "purpose-built-store")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use rutent_conformance::host::probe::{self, Probe};
use rutent_contracts::state_machine::host::testing::Harness;
use rutent_contracts::state_machine::host::{Delivery, Host};
use rutent_contracts::state_machine::{Id, Observed, RecordedInput};
use rutent_stm::purpose_built_store::{PurposeBuiltHarness, PurposeBuiltHost};

type Pb = PurposeBuiltHost<Probe>;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// The system allocator, counting the bytes live and the most that were live at once.
struct Counting;

// SAFETY: every call is the system allocator's; the counters only observe.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract is passed on unchanged.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(live, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        // SAFETY: the caller's contract is passed on unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller's contract is passed on unchanged.
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            if new_size >= layout.size() {
                let live = LIVE.fetch_add(new_size - layout.size(), Ordering::SeqCst)
                    + (new_size - layout.size());
                PEAK.fetch_max(live, Ordering::SeqCst);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::SeqCst);
            }
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn live() -> usize {
    LIVE.load(Ordering::SeqCst)
}

/// A segment settled with evidence of the given length: the one field of the probe scenario whose size is ours.
fn settled(record: &str, tick: u32, evidence_len: usize) -> RecordedInput {
    probe::input(
        "ord-big",
        record,
        tick,
        Some("p1-s1"),
        Observed::SegmentSettled {
            evidence: Id::new("e".repeat(evidence_len)).unwrap(),
        },
    )
}

/// What one history cost: the log's bytes, the heap an open host retains over it, and the most the heap grew by
/// while that log was opened again.
struct Cost {
    log: usize,
    retained: usize,
    reopen_peak: usize,
}

/// `entries` settled inputs of `evidence_len` bytes each, written to a fresh store, and the store opened again.
fn history(entries: u32, evidence_len: usize) -> Cost {
    let mut harness = PurposeBuiltHarness::<Probe>::new();
    let store = harness.fresh_store().unwrap();
    let mut inputs = vec![probe::order_inputs("ord-big", 0).remove(0)];
    inputs.extend((0..entries).map(|n| settled(&format!("rec-{n}"), 1 + n, evidence_len)));
    let mut host = Pb::open(store.clone()).unwrap();
    for input in inputs {
        assert!(matches!(host.submit(input), Ok(Delivery::Recorded(_))));
    }
    let log = std::fs::metadata(store.log_path()).unwrap().len() as usize;
    let store = harness.kill(host).unwrap();

    let before = live();
    PEAK.store(before, Ordering::SeqCst);
    let host = Pb::open(store).unwrap();
    let retained = live().saturating_sub(before);
    let reopen_peak = PEAK.load(Ordering::SeqCst).saturating_sub(before);
    // The host still answers from what it kept on disk.
    let journal = host.journal(&Id::new("ord-big").unwrap()).unwrap();
    assert_eq!(journal.len(), entries as usize + 1);
    drop(host);
    Cost {
        log,
        retained,
        reopen_peak,
    }
}

// verifies: REQ-OPS-3
#[test]
fn purpose_built_store_memory_stays_bounded_as_its_history_grows() {
    let small = history(200, 2 * 1024);
    let large = history(200, 64 * 1024);
    let longer = history(400, 64 * 1024);
    println!(
        "small: log {} B, retained {} B, reopen peak {} B; large: log {} B, retained {} B, reopen peak {} B; \
         longer: log {} B, retained {} B, reopen peak {} B",
        small.log,
        small.retained,
        small.reopen_peak,
        large.log,
        large.retained,
        large.reopen_peak,
        longer.log,
        longer.retained,
        longer.reopen_peak
    );
    assert!(large.log > 12 << 20, "the large history is {} B", large.log);
    // What the host retains does not follow the size of what it recorded: thirty times the bytes per record cost
    // no more than a few kilobytes, and the heap is a small fraction of the log however long the log is.
    assert!(
        large.retained <= small.retained + 16 * 1024,
        "retained {} B for small records and {} B for large ones",
        small.retained,
        large.retained
    );
    for cost in [&large, &longer] {
        assert!(
            cost.retained * 50 < cost.log,
            "an open host retains {} B over a log of {} B",
            cost.retained,
            cost.log
        );
    }
    for cost in [&small, &large, &longer] {
        // Opening reads the log a frame at a time: the heap never holds the log, only a frame or two beside the
        // index.
        assert!(
            cost.reopen_peak <= cost.retained + (1 << 20),
            "opening a log of {} B peaked at {} B over the heap before it",
            cost.log,
            cost.reopen_peak
        );
    }
}
