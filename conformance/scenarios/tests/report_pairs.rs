//! The pair of reports a segment's end gives, built by orchestration and handed to `core-audit`'s comparison
//! (`REQ-ORD-9`, `REQ-CAT-11`; `T-020-35`), over the assembled Core on the simulated providers of `T-024-04`'s four
//! provider route: a reserve, an exchange, a ramp and the reserve the value is deposited in. Each segment that follows
//! another run by another provider makes a pair with it: what the one reports giving out, what the next reports taking
//! in, both of results Core took and judged, in the log as the providers' own statements about their own segments.

mod execution_support;

use execution_support::*;
use rutent_contracts::core_internal::{
    Difference, Entry, Receipt, ReportPair, ReportedAs, VerifiedEntry, What,
};
use rutent_contracts::router_api::{Amount, Id, PositiveAmount};
use rutent_core_orch::intake::{OrderRequest, Status, Told};
use rutent_core_orch::segments::{Accounted, Ran, SegmentOutcome, Unbuilt};
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{self, AMOUNT};

const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const QUICK: &str = "sim-join-ramp-quick";
const OUT: &str = "sim-join-out";

fn order_across(name: &str, session: &Id) -> OrderRequest {
    request(
        name,
        session,
        AMOUNT,
        IN,
        outer("sim-join-in-bank", "COP"),
        outer("sim-join-out-bank", "USD"),
        OUT,
        OTHER,
        "lowest_total_cost",
    )
}

/// The four-provider order, accepted and run to its end: every segment settled.
fn ran_across(accepts: &'static str) -> (Core, Id, Vec<Accounted>) {
    let (mut core, session) = core_with(accepts, &[IN, EXCHANGE, QUICK, OUT]);
    let (order, told) = quoted(&mut core, order_across("req-pairs", &session));
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted")
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    let ran = core.run(&order).expect("the order ran");
    let [Ran::Segments(segments)] = ran.as_slice() else {
        panic!("one part ran its segments: {ran:?}");
    };
    assert!(
        segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{segments:?}"
    );
    (core, order, segments.clone())
}

/// The pairs the log holds, with the receipt of the entry each is in.
fn pairs_of(core: &Core) -> Vec<(Receipt, ReportPair)> {
    core.log()
        .entries()
        .iter()
        .filter_map(|(receipt, e)| match e {
            Entry::Verified(VerifiedEntry {
                what: What::ReportPair { pair },
                ..
            }) => Some((receipt.clone(), pair.clone())),
            _ => None,
        })
        .collect()
}

/// The differences raised to the operator.
fn raised(core: &Core) -> Vec<Difference> {
    entries(core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Raise { raise, .. } => Some(raise.difference),
            _ => None,
        })
        .collect()
}

fn amount(text: &str) -> Amount {
    Amount::new(text.to_owned()).expect("an amount")
}

// verifies: REQ-ORD-9, REQ-CAT-11
#[test]
fn segment_ending_with_two_reports_hands_core_audit_a_pair_with_its_tuples_terms() {
    let (mut core, order, segments) = ran_across("AC-1");
    // Orchestration holds the pairs the segments' ends gave, and the log none until the caller hands them over.
    assert!(pairs_of(&core).is_empty(), "nothing is compared unasked");
    assert_eq!(core.unpaired_reports(), 3, "one pair for each adjacent two");
    assert_eq!(core.write_pairs(), Ok(3));
    assert_eq!(core.write_pairs(), Ok(0), "each pair is handed over once");

    let pairs = pairs_of(&core);
    assert_eq!(pairs.len(), 3);
    let providers = [IN, EXCHANGE, QUICK, OUT];
    let mut reportings = Vec::new();
    for (n, (_, pair)) in pairs.iter().enumerate() {
        let (before, after) = (&segments[n], &segments[n + 1]);
        assert_eq!(pair.order, order);
        // The pair is of the segment that gives out, and the next one that takes in.
        assert_eq!(pair.segment, before.segment);
        assert_eq!(pair.sent.segment, before.segment);
        assert_eq!(pair.received.segment, after.segment);
        assert_eq!(pair.sent.party.as_str(), providers[n]);
        assert_eq!(pair.received.party.as_str(), providers[n + 1]);
        // The next took in what the one handed on, and the sending tuple's terms say how its figure reads: net, it is
        // what it handed on; gross, that and the charge it holds, which the comparison takes off.
        let SegmentOutcome::Settled { amount_out, .. } = &before.outcome else {
            unreachable!("every segment settled");
        };
        assert_eq!(&pair.received.amount, amount_out, "{pair:?}");
        assert_eq!(pair.sent.asset, pair.received.asset);
        // The first reserve's tuples declare a fill increment of 10: it is carried, in the reports' unit, into the
        // pair; the other tuples declare none.
        let declared = (n == 0).then(|| PositiveAmount::new(amount("10")).unwrap());
        assert_eq!(pair.terms.increment, declared, "{pair:?}");
        match &pair.terms.reporting {
            ReportedAs::Net {} => assert_eq!(&pair.sent.amount, amount_out),
            ReportedAs::Gross { charge } => {
                assert_eq!(
                    pair.sent.amount.checked_sub(charge).as_ref(),
                    Some(amount_out)
                );
            }
        }
        reportings.push(pair.terms.reporting.clone());
        // Each report rests on a stated entry the log holds as its party's own.
        for (report, party) in [
            (&pair.sent, providers[n]),
            (&pair.received, providers[n + 1]),
        ] {
            let at = report.statement.index.get() as usize;
            let log = core.log();
            let Entry::Stated(stated) = &log.entries()[at].1 else {
                panic!("a report rests on a stated entry");
            };
            assert_eq!(stated.attribution.maker().acts.as_str(), party);
            assert_eq!(
                stated.about.as_ref().and_then(|a| a.segment.as_ref()),
                Some(&report.segment)
            );
        }
    }
    // The exchange converts: its gross figure holds no charge in the asset it gives out.
    assert_eq!(
        reportings[1],
        ReportedAs::Gross {
            charge: amount("0")
        }
    );
    assert!(raised(&core).is_empty(), "agreeing reports raise nothing");
}

// verifies: REQ-ORD-9, REQ-CAT-11
#[test]
fn reports_differing_by_less_than_one_increment_on_the_running_platform_raise_nothing() {
    let (mut core, _, _) = ran_across("AC-1");
    // The first reserve's tuples declare a fill increment of 10 (the pair carries it). The simulated providers agree
    // exactly, so the receiving party of the first pair is restated as reporting nine less: within one increment.
    let mut first = true;
    let wrote = core.write_pairs_as(&mut |mut pair| {
        if std::mem::take(&mut first) {
            pair.received.amount = pair.sent.amount.checked_sub(&amount("9")).unwrap();
        }
        pair
    });
    assert_eq!(wrote, Ok(3));
    assert_eq!(pairs_of(&core).len(), 3, "each pair is recorded");
    assert!(
        raised(&core).is_empty(),
        "less than one increment apart raises nothing: {:?}",
        raised(&core)
    );
}

// verifies: REQ-ORD-9, REQ-CAT-11
#[test]
fn reports_differing_by_one_increment_or_more_on_the_running_platform_raise_the_difference() {
    let (mut core, order, segments) = ran_across("AC-1");
    // The same restatement, ten less: one increment apart.
    let mut first = true;
    let wrote = core.write_pairs_as(&mut |mut pair| {
        if std::mem::take(&mut first) {
            pair.received.amount = pair.sent.amount.checked_sub(&amount("10")).unwrap();
        }
        pair
    });
    assert_eq!(wrote, Ok(3));
    let raises = raised(&core);
    let [
        Difference::Reports {
            order: o,
            segment,
            sent,
            received,
            ..
        },
    ] = raises.as_slice()
    else {
        panic!("one difference raised: {raises:?}");
    };
    assert_eq!(o, &order);
    assert_eq!(segment, &segments[0].segment);
    assert_eq!(sent, &amount(AMOUNT));
    assert_eq!(received, &amount("999990"));
}

// verifies: REQ-ORD-9
#[test]
fn pair_that_cant_be_built_leaves_the_segment_settled_and_the_part_running_to_its_end() {
    let (mut core, session) = core_with("AC-1", &[IN, EXCHANGE, QUICK, OUT]);
    // As if a restart had lost what the segments keep for their pairs.
    core.lose_sent_reports();
    let (order, told) = quoted(&mut core, order_across("req-unbuilt", &session));
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted")
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    let ran = core.run(&order).expect("the order ran");
    let [Ran::Segments(segments)] = ran.as_slice() else {
        panic!("one part ran its segments: {ran:?}");
    };
    // A comparison is a detection: every segment settled, and the order is complete.
    assert_eq!(segments.len(), 4);
    assert!(
        segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{segments:?}"
    );
    assert_eq!(
        state(&mut core, &order),
        rutent_contracts::state_machine::OrderState::Complete
    );
    // The pairs that couldn't be built are surfaced, each with why, and none is written.
    let unbuilt = core.unbuilt_reports();
    assert_eq!(unbuilt.len(), 3, "{unbuilt:?}");
    for (n, parked) in unbuilt.iter().enumerate() {
        assert_eq!(parked.order, order);
        assert_eq!(parked.segment, segments[n + 1].segment);
        assert_eq!(
            parked.why,
            Unbuilt::NoSentReport(segments[n].segment.clone())
        );
    }
    assert_eq!(core.unpaired_reports(), 3);
    assert_eq!(core.write_pairs(), Ok(0));
    assert!(pairs_of(&core).is_empty());
}

// verifies: REQ-ORD-9
#[test]
fn adjacent_segments_of_one_provider_make_no_pair_and_leave_nothing_unbuilt() {
    // The kit's reserve runs the withdrawal and the deposit both: one party's word compares nothing.
    let (mut core, session) = core_with("AC-28", &[rutent_scenarios::kit::RESERVE]);
    let order = accepted(
        &mut core,
        within(
            "same",
            &session,
            AMOUNT,
            rutent_scenarios::kit::RESERVE,
            OTHER,
        ),
    );
    let ran = core.run(&order).expect("the order ran");
    let [Ran::Segments(segments)] = ran.as_slice() else {
        panic!("one part ran its segments: {ran:?}");
    };
    assert_eq!(segments.len(), 2);
    assert!(
        segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{segments:?}"
    );
    assert_eq!(core.unpaired_reports(), 0, "no pair owed, none parked");
    assert!(core.unbuilt_reports().is_empty());
    assert_eq!(core.write_pairs(), Ok(0));
    assert!(pairs_of(&core).is_empty());
}
