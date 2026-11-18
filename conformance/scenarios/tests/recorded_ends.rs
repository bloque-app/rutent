//! Two ends that left no record to cite, now recorded (`T-020-25`): an empty shortlist, whose decision record joins
//! the order's chain so a `no_route` input finds it (`REQ-AUD-4`), and an operation that ended with no answer, which
//! `segments/` hands the log as the platform's own record so an outcome unknown cites it (`REQ-MDL-8`, `REQ-ORD-1`).
//! Over `T-020-08`'s kit on the running platform with simulated providers, the log replayed as the log itself would
//! admit it.

mod segments_support;

use rutent_conformance::sim::Behaviour;
use rutent_contracts::core_internal::{Entry, HandedOver, Receipt, What};
use rutent_contracts::state_machine::{Observed, SegmentState};
use rutent_core_orch::recalc::Recalculation;
use rutent_core_orch::segments::{
    APPEND_TRIES, Accounted, Ran, SegmentOutcome, SegmentsError, UnknownBy,
};
use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_core_orch::stuck::{NoRouteLeft, NoRouteRecorded, Stuck, StuckError, seen_by_provider};
use segments_support::rerouting::Rerouting;
use segments_support::resolving::on_parts;
use segments_support::*;

fn segments(ran: &Ran) -> &[Accounted] {
    match ran {
        Ran::Segments(done) => done,
        Ran::Refused { refused, .. } => panic!("the part was refused: {refused:?}"),
    }
}

/// The kit's order with its withdrawal settled and its deposit never begun, then recalculated from the reserve's
/// ledger with every provider excluded: the shortlist keeps nothing. Returns what the recalculation answered, and the
/// places and receipts of the order's decision records before it ran.
fn left_by_an_empty_shortlist() -> (World, Core, Rerouting, Recalculation, Vec<Receipt>) {
    let mut world = World::of(&[RESERVE, OTHER_RESERVE]);
    let mut core = Core::over(&mut world);
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.refuse_begin = Some(1);
    let ran = core.run_over(&world.catalogue, &mut watched);
    drop(watched);
    assert!(matches!(
        segments(&ran)[0].outcome,
        SegmentOutcome::Settled { .. }
    ));
    let before = decision_receipts(&core);
    let mut rerouting = Rerouting::of(&core);
    rerouting.excluded = vec![RESERVE.to_owned(), OTHER_RESERVE.to_owned()];
    let outcome = rerouting
        .recalculate(&mut core, &mut world)
        .expect("the recalculation runs");
    (world, core, rerouting, outcome, before)
}

/// The receipts of the kit's order's decision records, in the log's order.
fn decision_receipts(core: &Core) -> Vec<Receipt> {
    let log = core.log.borrow();
    log.entries
        .iter()
        .enumerate()
        .filter(|(_, (entry, _))| {
            matches!(entry, Entry::Verified(v)
                if matches!(&v.what, What::DecisionRecord { order, .. } if order.as_str() == core.named.order))
        })
        .map(|(at, _)| log.receipt_at(at))
        .collect()
}

/// What an empty shortlist's record names: its shortlisted providers, none, and the receipt it chains to.
fn is_empty_shortlist(what: &What) -> bool {
    matches!(what, What::DecisionRecord { shortlisted: Some(s), .. } if s.is_empty())
}

// verifies: REQ-AUD-4
#[test]
fn empty_shortlist_is_appended_and_the_orders_chain_grows() {
    let (world, core, rerouting, outcome, before) = left_by_an_empty_shortlist();
    assert!(
        matches!(outcome, Recalculation::NoRoute { .. }),
        "{outcome:?}"
    );
    let after = decision_receipts(&core);
    assert_eq!(
        after.len(),
        before.len() + 1,
        "the empty shortlist's record is appended: the order's chain grows"
    );
    let log = core.log.borrow();
    assert_eq!(log.count(is_empty_shortlist), 1);
    let (at, entry) = log
        .entries
        .iter()
        .enumerate()
        .find(|(_, (e, _))| matches!(e, Entry::Verified(v) if is_empty_shortlist(&v.what)))
        .expect("the empty shortlist's entry");
    let Entry::Verified(v) = &entry.0 else {
        unreachable!()
    };
    let What::DecisionRecord { previous, .. } = &v.what else {
        unreachable!()
    };
    assert_eq!(
        previous.as_ref(),
        before.last(),
        "chained to the order's last record"
    );
    assert_eq!(
        rerouting.history.previous(),
        Some(&log.receipt_at(at)),
        "and the chain goes on from it"
    );
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits the empty shortlist");
}

/// Records the no-route outcome of the kit's current order's part.
fn record_no_route(
    core: &mut Core,
    outcome: &Recalculation,
) -> Result<NoRouteRecorded, StuckError> {
    let (order, part, last) = (
        id(&core.named.order),
        id(PART),
        core.record.record_id().clone(),
    );
    let actor = segments_support::core();
    let mut stuck = Stuck::new();
    on_parts(core, |parts| {
        stuck.no_route(
            parts,
            NoRouteLeft {
                order: &order,
                part: &part,
                outcome,
                last_record: &last,
                core: &actor,
            },
        )
    })
}

// verifies: REQ-ORD-28, REQ-AUD-4
#[test]
fn no_route_input_citing_an_empty_shortlist_is_admitted() {
    let (world, mut core, _, outcome, _) = left_by_an_empty_shortlist();
    let Recalculation::NoRoute { record } = &outcome else {
        panic!("{outcome:?}");
    };
    assert!(is_empty_shortlist_record(
        &core,
        record.record.record_id().as_str()
    ));
    record_no_route(&mut core, &outcome).expect("recorded");
    let log = core.log.borrow();
    let cited = log
        .inputs()
        .into_iter()
        .find_map(|i| match &i.observed {
            Observed::NoRoute { decision_record } => Some(decision_record.as_str().to_owned()),
            _ => None,
        })
        .expect("a no_route input");
    assert_eq!(cited, record.record.record_id().as_str());
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits a no_route citing the empty shortlist's record");
}

fn is_empty_shortlist_record(core: &Core, record_id: &str) -> bool {
    core.log.borrow().count(|w| {
        matches!(w, What::DecisionRecord { decision_record, .. }
            if decision_record.as_str() == record_id && is_empty_shortlist(w))
    }) == 1
}

/// The kit's order run with its withdrawal's module passing a call out and answering too late: the operation ended
/// with no answer.
fn ended_with_no_answer() -> (World, Core, Ran) {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour {
            late: true,
            ..Behaviour::default()
        },
    );
    let ran = core.run(&mut world);
    (world, core, ran)
}

fn is_unanswered(what: &What) -> bool {
    matches!(
        what,
        What::HandOver {
            item: HandedOver::OperationUnanswered { .. },
            ..
        }
    )
}

// verifies: REQ-MDL-8
#[test]
fn operation_that_ended_with_no_answer_reaches_the_log_as_a_handed_over_item() {
    let (world, core, ran) = ended_with_no_answer();
    assert!(
        matches!(
            segments(&ran)[0].outcome,
            SegmentOutcome::Unknown(UnknownBy::Platform { .. })
        ),
        "{ran:?}"
    );
    let log = core.log.borrow();
    let items: Vec<&HandedOver> = log
        .records()
        .into_iter()
        .filter_map(|(_, w)| match w {
            What::HandOver { item, .. } if is_unanswered(w) => Some(item),
            _ => None,
        })
        .collect();
    let [
        HandedOver::OperationUnanswered {
            provider,
            transaction,
            ended_by,
        },
    ] = items.as_slice()
    else {
        panic!("one record of the operation that ended with no answer: {items:?}");
    };
    assert_eq!(provider.as_str(), RESERVE);
    assert_eq!(
        ended_by.as_str(),
        "limit_reached",
        "a late answer hit a limit"
    );
    let kept = core.segments.unknown_outcomes(&id(ORDER), &id(WITHDRAWAL));
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].transaction, transaction.as_str());
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

// verifies: REQ-ORD-1
#[test]
fn outcome_unknown_cites_the_platforms_own_record() {
    let (world, mut core, ran) = ended_with_no_answer();
    let SegmentOutcome::Unknown(UnknownBy::Platform { record, .. }) = &segments(&ran)[0].outcome
    else {
        panic!("{ran:?}");
    };
    let cited = record
        .clone()
        .expect("the outcome cites the platform's record");
    // The receipt is of the platform's own record, the entry before the input that says the outcome is unknown.
    let log = core.log.borrow();
    let at = cited.index.get() as usize;
    assert!(
        is_unanswered_entry(&log.entries[at].0),
        "{:?}",
        log.entries[at].0
    );
    let unknown_at = log
        .entries
        .iter()
        .position(|(e, _)| {
            matches!(e, Entry::Verified(v) if matches!(&v.what,
                What::Input { input } if matches!(input.observed, Observed::SegmentOutcomeUnknown {})))
        })
        .expect("the input that says it is unknown");
    assert!(at < unknown_at, "the record is on the log before the input");
    drop(log);
    // What the provider reads of its segment carries the same record.
    let seen = seen_by_provider(
        &core.segments,
        &mut core.policy,
        &*core.clock,
        &mut core.ids,
        &provider_desk(),
        &id(ORDER),
        &id(WITHDRAWAL),
    )
    .expect("the provider reads its segment");
    assert_eq!(seen.segment_state, SegmentState::OutcomeUnknown);
    assert_eq!(seen.platform_record.as_ref(), Some(&cited));
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}

fn is_unanswered_entry(entry: &Entry) -> bool {
    matches!(entry, Entry::Verified(v) if is_unanswered(&v.what))
}

/// How many inputs on the log say a segment's outcome is unknown.
fn count_unknown(core: &Core) -> usize {
    core.log.borrow().count(|w| {
        matches!(w, What::Input { input }
            if matches!(input.observed, Observed::SegmentOutcomeUnknown {}))
    })
}

/// While the log can't take the platform's record of an operation that ended with no answer, the segment's unknown
/// outcome is held back, never recorded without its citation; once the log takes the record, the outcome follows it
/// and cites it (`REQ-MDL-8`, `T-020-25`'s fail-closed default).
// verifies: REQ-MDL-8
#[test]
fn outcome_unknown_waits_while_the_log_cant_take_the_platforms_record_and_is_recorded_after() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world.fleet.world().script_for(
        RESERVE,
        "withdrawal",
        Behaviour {
            late: true,
            ..Behaviour::default()
        },
    );
    // The log can't take a handed-over item for as many tries as one drain makes.
    for _ in 0..APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::HandOver, Fault::Unavailable));
    }
    let session = world.context().session;
    let waiting = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut rutent_core_orch::segments::NoProxy,
        &mut Recorded,
        &session,
    );
    assert!(
        matches!(
            waiting,
            Err(SegmentsError::Append(AppendFailure::Unavailable))
        ),
        "the outcome waits for the record: {waiting:?}"
    );
    assert_eq!(count_unknown(&core), 0, "no outcome unknown yet");
    assert_eq!(
        core.log.borrow().count(is_unanswered),
        0,
        "the platform's record is not on the log yet"
    );

    // The log takes it now: the record first, then the outcome, which cites it.
    let ran = core.run(&mut world);
    let SegmentOutcome::Unknown(UnknownBy::Platform { record, .. }) = &segments(&ran)[0].outcome
    else {
        panic!("{ran:?}");
    };
    let cited = record.clone().expect("the outcome cites the record");
    assert_eq!(core.log.borrow().count(is_unanswered), 1, "one record");
    assert_eq!(count_unknown(&core), 1, "one outcome unknown");
    let log = core.log.borrow();
    assert!(is_unanswered_entry(&log.entries[cited.index.get() as usize].0));
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits every entry");
}
