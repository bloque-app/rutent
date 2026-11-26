//! Orchestration's calls of `Measures::measure` over the assembled Core (`T-020-31`, `T-022-06`'s gap 1): a segment
//! that ends, and a reversal that completes, each hand the measure writer the record that ending gives, citing the
//! receipts of the entries it rests on; `core-audit`'s bulk measures are then read back from the log Core's own runs
//! left, with no record written by a test (`REQ-AUD-7`).
//!
//! The writer is the one handle the assembly adds ([`Core::tick`]'s measures job, over `Core::measure_writer`): a run
//! queues each record where the segment ends, and the writer's call hands the queue to the log. A run whose writer is
//! not called leaves the queue unwritten, and the first test below holds that.

mod execution_support;

use execution_support::*;
use rutent_conformance::sim::{Act, Behaviour, Delivery};
use rutent_contracts::core_internal::{Entry, Measure, MeasureRecord, MeasuredRole, Receipt, What};
use rutent_contracts::router_api::Timestamp;
use rutent_core_audit::measures::{Window, measure};
use rutent_core_orch::reversal::Confirmation;
use rutent_core_orch::segments::{Accounted, Ran, SegmentOutcome};
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{AMOUNT, RESERVE};

/// A window wide enough to hold every run.
fn whole() -> Window {
    Window {
        from: Timestamp::new("2000-01-01T00:00:00Z").unwrap(),
        until: Timestamp::new("2100-01-01T00:00:00Z").unwrap(),
    }
}

/// The segments one run accounted for.
fn accounted(ran: &[Ran]) -> Vec<Accounted> {
    let [Ran::Segments(segments)] = ran else {
        panic!("one part ran its segments: {ran:?}");
    };
    segments.clone()
}

/// Every measure record of the log, with the receipt of the entry it sits in.
fn records(core: &Core) -> Vec<(Receipt, MeasureRecord)> {
    core.log()
        .entries()
        .iter()
        .filter_map(|(receipt, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Measure { record } => Some((receipt.clone(), record.clone())),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect()
}

/// An order run over the one simulated reserve, `script` queued before it runs.
fn ran(
    core: &mut Core,
    session: &rutent_contracts::router_api::Id,
    name: &str,
    script: impl FnOnce(&mut Core),
) -> (rutent_contracts::router_api::Id, Vec<Accounted>) {
    let order = accepted(core, within(name, session, AMOUNT, RESERVE, OTHER));
    script(core);
    let ran = core.run(&order).expect("the order ran");
    (order, accounted(&ran))
}

// verifies: REQ-AUD-7
#[test]
fn segment_ending_leaves_no_measure_record_when_its_writer_is_not_called() {
    let (mut core, session) = core_with("AC-28", &[RESERVE]);
    let (order, segments) = ran(&mut core, &session, "ended", |_| {});
    assert!(
        segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "the order's segments settled: {segments:?}"
    );

    // Orchestration holds what each ending gave, and the log holds none of it until the writer is called.
    assert!(
        records(&core).is_empty(),
        "no measure record is in the log before the writer is called"
    );
    let written = core.tick().measures;
    assert!(written > 0, "the writer was handed the segments' records");
    let written = records(&core);
    assert!(!written.is_empty(), "the log holds what the writer took");

    // Each record is about this order and one of its segments, and cites the entry of that segment's own effect.
    for segment in &segments {
        let effect = segment.effect.clone().expect("the segment's effect");
        let own: Vec<_> = written
            .iter()
            .filter(|(_, r)| r.segment.as_ref() == Some(&segment.segment))
            .collect();
        assert!(
            own.iter()
                .any(|(_, r)| matches!(r.measure, Measure::Quote { .. })
                    && r.cites.contains(&effect)),
            "a quote against the executed price, citing the effect: {own:?}"
        );
        assert!(own.iter().all(|(_, r)| r.order == order));
    }
    assert!(
        written
            .iter()
            .all(|(_, r)| r.role == MeasuredRole::Provider && r.provider.as_str() == RESERVE),
        "the reserve's own records, in its role as a provider"
    );

    // Called again, it writes nothing more: each ending is handed over once.
    assert_eq!(core.tick().measures, 0);
    assert_eq!(records(&core).len(), written.len());
}

/// What a run's segments came to, counted from the run's own accounting.
#[derive(Default, Debug, PartialEq, Eq)]
struct Expected {
    quotes: u64,
    below: u64,
    at: u64,
    above: u64,
    failures: u64,
    refusals: u64,
}

fn expected(runs: &[(Vec<Accounted>, Vec<rutent_contracts::router_api::Amount>)]) -> Expected {
    let mut e = Expected::default();
    for (segments, priced) in runs {
        for (n, segment) in segments.iter().enumerate() {
            match &segment.outcome {
                SegmentOutcome::Settled { amount_out, .. } => {
                    e.quotes += 1;
                    let quoted = &priced[n];
                    if amount_out == quoted {
                        e.at += 1;
                    } else if quoted.checked_sub(amount_out).is_some() {
                        e.below += 1;
                    } else {
                        e.above += 1;
                    }
                }
                SegmentOutcome::Refused(_) => e.refusals += 1,
                SegmentOutcome::Failed | SegmentOutcome::WithdrawalCancelled => e.failures += 1,
                other => panic!("a run this test doesn't plan: {other:?}"),
            }
        }
    }
    e
}

// verifies: REQ-AUD-7
#[test]
fn bulk_measures_over_assembled_runs_with_no_hand_written_record_count_a_failed_segment_a_refusal_and_a_quote_against_its_executed_price()
 {
    let (mut core, session) = core_with("AC-28", &[RESERVE]);
    let mut runs = Vec::new();
    let mut each = |core: &mut Core, name: &str, script: &dyn Fn(&mut Core)| {
        let request = within(name, &session, AMOUNT, RESERVE, OTHER);
        let (order, told) = quoted(core, request);
        let priced = record_of(core, &told).chosen().expect("a route").parts[0]
            .operations
            .iter()
            .map(|op| op.amount_out.clone())
            .collect::<Vec<_>>();
        let rutent_core_orch::intake::Told::Exists {
            status: rutent_core_orch::intake::Status::Quoted { digest, .. },
            ..
        } = told
        else {
            panic!("quoted")
        };
        core.accept(&rutent_scenarios::kit::customer(), &order, &digest)
            .expect("accepted");
        script(core);
        let ran = core.run(&order).expect("ran");
        runs.push((accounted(&ran), priced));
    };
    each(&mut core, "full", &|_| {});
    each(&mut core, "short", &|core| {
        core.world().script_for(
            RESERVE,
            "withdrawal",
            Behaviour::delivering(Delivery::Short { bps: 100 }),
        );
    });
    each(&mut core, "failing", &|core| {
        core.world()
            .script_for(RESERVE, "deposit", Behaviour::act(Act::Fail));
    });
    each(&mut core, "refusing", &|core| {
        core.world()
            .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    });

    // The runs are what they were scripted to be, asserted on the runs themselves.
    let last = |n: usize| runs[n].0.last().unwrap().outcome.clone();
    assert!(matches!(last(2), SegmentOutcome::Failed), "{:?}", last(2));
    assert!(
        matches!(last(3), SegmentOutcome::Refused(_)),
        "{:?}",
        last(3)
    );
    let want = expected(&runs);
    assert!(want.below >= 1, "one run settled short: {want:?}");
    assert_eq!((want.failures, want.refusals), (1, 1), "{want:?}");

    // No record was written by this test: the writer Core's own assembly gives takes what the runs queued.
    assert!(records(&core).is_empty());
    core.tick();

    let report = {
        let log = core.log();
        measure(log.entries(), &whole())
    };
    let reserve = &report.providers[RESERVE];
    let (mut quotes, mut below, mut at, mut above) = (0, 0, 0, 0);
    for q in reserve.quotes.values() {
        quotes += q.quotes;
        below += q.executed_below;
        at += q.executed_at;
        above += q.executed_above;
    }
    assert_eq!(
        Expected {
            quotes,
            below,
            at,
            above,
            failures: reserve.failures.all(),
            refusals: reserve.refusals,
        },
        want,
        "quotes against what was executed, failures and refusals, as the runs came to"
    );
    assert!(reserve.failures.all() > 0, "the failed segment is counted");
    assert!(reserve.refusals > 0, "the refusal is counted");
    assert_eq!(
        reserve.failures.no_figure,
        reserve.failures.all(),
        "no market figure is held in M3"
    );
    assert!(report.verifiers.is_empty(), "no verifier was asked");
    // The answers a route was evaluated with rest on the entry that carries them: the order's decision record.
    let heard: Vec<_> = records(&core)
        .into_iter()
        .filter(|(_, r)| matches!(r.measure, Measure::DryRun { .. } | Measure::Answer { .. }))
        .collect();
    assert!(!heard.is_empty(), "the runs' dry runs were measured");
    for (_, r) in &heard {
        let carried = r.cites.iter().any(|c| {
            matches!(
                &core.log().entries()[usize::try_from(c.index.get()).unwrap()].1,
                Entry::Verified(v) if matches!(&v.what, What::DecisionRecord { .. })
            )
        });
        assert!(
            carried,
            "{r:?} cites the decision record that carries the answer"
        );
    }
    // What was counted is the log's own: every measure record of the log, once.
    assert_eq!(report.counted, records(&core).len() as u64);
}

/// `text` with each digest's value left out.
fn undigested(text: &str) -> String {
    const OPEN: &str = "DigestValue(\"";
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at + OPEN.len()]);
        let after = &rest[at + OPEN.len()..];
        rest = after.find('"').map_or("", |end| &after[end..]);
    }
    out.push_str(rest);
    out
}

// verifies: REQ-AUD-7
#[test]
fn a_measure_that_cannot_be_written_leaves_the_segments_outcome_unchanged() {
    let (mut with, session) = core_with("AC-28", &[RESERVE]);
    let (mut without, other) = core_with("AC-28", &[RESERVE]);
    // The log's store turns away every measure record, as one that can't take them now does.
    without.refuse_log_appends("measure", 10_000);
    let script = |core: &mut Core| {
        core.world()
            .script_for(RESERVE, "deposit", Behaviour::act(Act::Fail));
    };
    let (_, a) = ran(&mut with, &session, "same", script);
    let (_, b) = ran(&mut without, &other, "same", script);
    assert_eq!(
        format!("{a:?}"),
        format!("{b:?}"),
        "the segment's outcome is as without it"
    );
    // Everything else the run left is the same entry for entry: the measure changed nothing. (Compared before the ticks:
    // a tick's reads of a private node's record are decided at the time each Core's clock then reads, `T-020-58`.)
    // (Two Cores are two logs: the digests of their entries are left out, the entries' contents compared.)
    let others = |core: &Core| -> Vec<String> {
        core.verified_whats()
            .into_iter()
            .filter(|w| !matches!(w, What::Measure { .. }))
            .map(|w| undigested(&format!("{w:?}")))
            .collect()
    };
    assert_eq!(others(&with), others(&without));

    assert!(with.tick().measures > 0);
    assert_eq!(without.tick().measures, 0, "nothing could be written");
    assert!(records(&without).is_empty());

    // The log able to take them again, the caller's next call writes what waited, each once.
    without.refuse_log_appends("measure", 0);
    let written = without.tick().measures;
    assert!(written > 0, "retried by the caller as any append");
    assert_eq!(records(&without).len(), records(&with).len());
    assert_eq!(without.tick().measures, 0);
}

// verifies: REQ-AUD-7
#[test]
fn completed_reversal_is_measured() {
    let (mut core, session) = core_with("AC-8", &[RESERVE]);
    let (order, ran) = run(
        &mut core,
        within("req-undone", &session, AMOUNT, RESERVE, OTHER),
    );
    let withdrawal = accounted(&ran)[0].segment.clone();
    core.tick();
    let before = records(&core);
    assert!(
        !before
            .iter()
            .any(|(_, r)| matches!(r.measure, Measure::Reversal {})),
        "nothing reversed yet"
    );

    // The origin reserve says the credit arrived: the start is proven undone, by its own word.
    core.world()
        .script(RESERVE, Behaviour::crediting_a_reversal());
    let done = core.confirm_reversal(&order, &withdrawal).expect("asked");
    let Confirmation::Completed(completed) = done else {
        panic!("the start is proven undone: {done:?}");
    };
    assert_eq!(core.tick().measures, 1, "one record for the reversal");
    let after = records(&core);
    let reversals: Vec<_> = after
        .iter()
        .filter(|(_, r)| matches!(r.measure, Measure::Reversal {}))
        .collect();
    let [(_, record)] = reversals.as_slice() else {
        panic!("one reversal record: {reversals:?}");
    };
    assert_eq!(record.provider.as_str(), RESERVE, "the provider reversed");
    assert_eq!(record.role, MeasuredRole::Provider);
    assert_eq!(record.order, order);
    assert_eq!(record.segment.as_ref(), Some(&withdrawal));
    assert!(
        record.cites.contains(&completed.statement) && record.cites.contains(&completed.input),
        "it cites the confirmation it rests on and the input that recorded it: {:?}",
        record.cites
    );

    // The measure reads it back.
    let report = {
        let log = core.log();
        measure(log.entries(), &whole())
    };
    assert_eq!(report.providers[RESERVE].reversals, 1);
    // Asked again, the completed reversal is not measured twice.
    let _ = core.confirm_reversal(&order, &withdrawal);
    assert_eq!(core.tick().measures, 0);
}

/// A reversal completed before Core stopped, its record not yet written: the reopened Core keeps the record again from
/// the log (`T-020-46`), so the writer's next call writes it, once, on the confirmation it rests on and the input that
/// recorded it; the segments' records written before the stop aren't written again (`REQ-AUD-7`, `NFR-6`).
// verifies: REQ-AUD-7, NFR-6
#[test]
fn reversal_completed_before_core_reopens_is_measured_once() {
    let (mut core, session) = core_with("AC-8", &[RESERVE]);
    let (order, ran) = run(
        &mut core,
        within("req-undone", &session, AMOUNT, RESERVE, OTHER),
    );
    let withdrawal = accounted(&ran)[0].segment.clone();
    core.tick();
    core.world()
        .script(RESERVE, Behaviour::crediting_a_reversal());
    let done = core.confirm_reversal(&order, &withdrawal).expect("asked");
    let Confirmation::Completed(completed) = done else {
        panic!("the start is proven undone: {done:?}");
    };
    let before = records(&core).len();
    let mut core =
        rutent_scenarios::harness::Scenario::new("execution", "AC-8", given(&[RESERVE, OTHER]))
            .reopen(core, 1, &mut rutent_scenarios::assembly::machine::Reference);
    assert_eq!(
        core.tick().measures,
        1,
        "the reversal's record, which the restart dropped"
    );
    assert_eq!(core.tick().measures, 0, "and nothing twice");
    let after = records(&core);
    assert_eq!(after.len(), before + 1);
    let reversals: Vec<_> = after
        .iter()
        .filter(|(_, r)| matches!(r.measure, Measure::Reversal {}))
        .collect();
    let [(_, record)] = reversals.as_slice() else {
        panic!("one reversal record: {reversals:?}");
    };
    assert_eq!(record.provider.as_str(), RESERVE);
    assert_eq!(record.segment.as_ref(), Some(&withdrawal));
    assert!(
        record.cites.contains(&completed.statement) && record.cites.contains(&completed.input),
        "{:?}",
        record.cites
    );
}
