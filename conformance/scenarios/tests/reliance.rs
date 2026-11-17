//! The reserve half of `REQ-CPL-12` (`T-020-23`): where Core hands a provider an input that rests on a reserve's
//! judgement, a `reliance` entry naming the order, the judgement, its judge and the provider relying on it is appended
//! before the operation is sent. The kit's route runs both its segments at [`RESERVE`], whose withdrawal takes
//! `payer.bank_account` and whose deposit takes `beneficiary.bank_account`; the judge is [`OTHER_RESERVE`], whose
//! statement about the order the log holds, verified.

mod segments_support;

use std::cell::RefCell;
use std::rc::Rc;

use rutent_contracts::core_internal::{Entry, Receipt, Through, What};
use rutent_core_orch::reliance::{Judgement, Rests};
use rutent_core_orch::segments::{Ran, Segments, SegmentsError};
use segments_support::*;

fn core_actor() -> rutent_contracts::core_internal::Actor {
    segments_support::core()
}

const PAYER: &str = "payer.bank_account";
const BENEFICIARY: &str = "beneficiary.bank_account";

/// The world with a second reserve, a Core over it, and that second reserve's statement about the order in the log:
/// its receipt, and the judgement it holds.
fn judged_by_the_other_reserve(guarded: bool) -> (World, Core, Judgement) {
    let mut world = World::of(&[RESERVE, OTHER_RESERVE]);
    let mut core = Core::over(&mut world);
    core.log.borrow_mut().guards_reliance = guarded;
    let (_, theirs) = results_after_the_move(&mut core, None, "tx-judged");
    let judgement = Judgement {
        receipt: theirs,
        judge: id(OTHER_RESERVE),
    };
    (world, core, judgement)
}

fn relies(core: &Core) -> Vec<(Receipt, String, String)> {
    core.log
        .borrow()
        .verified()
        .iter()
        .filter_map(|v| match &v.what {
            What::Reliance {
                judgement,
                judge,
                relied_on_by,
                ..
            } => Some((
                judgement.clone(),
                judge.as_str().to_owned(),
                relied_on_by.as_str().to_owned(),
            )),
            _ => None,
        })
        .collect()
}

/// Since `before`, the log holds nothing of the segment but policy's decision of the move: no question to the
/// provider, no effect, no input.
fn only_the_decision_since(core: &Core, before: usize) {
    let log = core.log.borrow();
    for (entry, _) in &log.entries[before..] {
        assert!(
            matches!(entry, Entry::Verified(v) if matches!(v.what, What::Decide { .. })),
            "a segment's record before its reliance: {entry:?}"
        );
    }
}

fn settled(ran: &Ran) -> usize {
    match ran {
        Ran::Segments(done) => done.len(),
        Ran::Refused { refused, .. } => panic!("the part was refused: {refused:?}"),
    }
}

fn rests(core: &mut Core, input: &str, judgement: &Judgement) {
    let order = id(&core.named.order);
    core.segments
        .rests_on(&order, input, judgement.clone())
        .expect("declared before anything is handed over");
}

/// Runs the part over the world's platform, watching what is carried and how many reliances the log held at each
/// carry-out.
fn run_watched(
    world: &mut World,
    core: &mut Core,
) -> (Result<Ran, SegmentsError>, Vec<String>, Vec<usize>) {
    let session = world.context().session;
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (log, spy) = (Rc::clone(&core.log), Rc::clone(&seen));
    let mut watched = Watched::new(&mut world.fleet.platform);
    watched.during = Some(Box::new(move |_, _| {
        let held = log
            .borrow()
            .verified()
            .iter()
            .filter(|v| matches!(v.what, What::Reliance { .. }))
            .count();
        spy.borrow_mut().push(held);
    }));
    let ran = core.try_run_with(
        &world.catalogue,
        &mut watched,
        &mut rutent_core_orch::segments::NoProxy,
        &mut Recorded,
        &session,
    );
    let carried = watched.amounts();
    drop(watched);
    let held = seen.borrow().clone();
    (ran, carried, held)
}

/// A provider handed an input that rests on a reserve's judgement leaves a `reliance` entry naming the order, the
/// judgement, its judge and the provider; the log admits it, and an input resting on nothing leaves none.
// verifies: REQ-CPL-12
#[test]
fn provider_relying_on_a_reserves_judgement_leaves_a_reliance_entry_naming_both() {
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(true);
    rests(&mut core, PAYER, &judgement);
    let (ran, carried, _) = run_watched(&mut world, &mut core);
    assert_eq!(settled(&ran.expect("the part runs")), 2);
    assert_eq!(carried.len(), 2, "both segments were handed over");

    assert_eq!(
        relies(&core),
        vec![(
            judgement.receipt.clone(),
            OTHER_RESERVE.to_owned(),
            RESERVE.to_owned()
        )],
        "one reliance, naming the judgement, its judge and the provider that relied on it"
    );
    let log = core.log.borrow();
    let entry = log
        .verified()
        .into_iter()
        .find_map(|v| match &v.what {
            What::Reliance { order, .. } => Some((order.clone(), v.who.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(entry.0, id(&core.named.order));
    assert_eq!(entry.1, core_actor(), "Core's own record");
    drop(log);
    core.admitted(&world.catalogue)
        .expect("the log admits the history, the reliance among it");

    // An input no tuple of the route takes is handed to no one, so no one relies on what rests on it.
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(true);
    rests(&mut core, "payer.date_of_birth", &judgement);
    let (ran, carried, _) = run_watched(&mut world, &mut core);
    ran.expect("runs");
    assert_eq!(carried.len(), 2);
    assert!(relies(&core).is_empty(), "nothing carried rests on it");
}

/// A judgement that is not the judge's own statement is relied on by no one: the log refuses the entry, so none is
/// written and nothing is sent. The real relation refuses it too, on the history the log would hold.
// verifies: REQ-CPL-12, REQ-AUD-2
#[test]
fn reliance_on_a_judgement_the_log_did_not_verify_as_its_judges_is_not_written() {
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(true);
    // The statement is OTHER_RESERVE's; naming RESERVE as its judge is naming another's.
    let wrong = Judgement {
        judge: id(RESERVE),
        ..judgement.clone()
    };
    rests(&mut core, PAYER, &wrong);
    let before = core.log.borrow().entries.len();
    let (ran, carried, _) = run_watched(&mut world, &mut core);
    assert!(
        matches!(ran, Err(SegmentsError::Append(_))),
        "the log's refusal stops the segment: {ran:?}"
    );
    assert!(carried.is_empty(), "nothing was sent: {carried:?}");
    assert!(relies(&core).is_empty(), "no reliance was written");
    only_the_decision_since(&core, before);

    // Not a statement at all: the receipt of an entry that holds none.
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(true);
    let not_a_statement = core.log.borrow().receipt_at(0);
    rests(
        &mut core,
        PAYER,
        &Judgement {
            receipt: not_a_statement,
            ..judgement.clone()
        },
    );
    let (ran, carried, _) = run_watched(&mut world, &mut core);
    assert!(matches!(ran, Err(SegmentsError::Append(_))), "{ran:?}");
    assert!(carried.is_empty());

    // The log's own relation, on the history it admits: the entry Core writes for the right judge is admitted, and
    // the same entry naming another judge is refused (`reliance-names-its-judges-statement`).
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(false);
    rests(&mut core, PAYER, &judgement);
    let (ran, _, _) = run_watched(&mut world, &mut core);
    ran.expect("an unguarded log takes it");
    let history = core.admitted(&world.catalogue).expect("admitted");
    let written = core
        .log
        .borrow()
        .entries
        .iter()
        .map(|(e, _)| e.clone())
        .find(|e| matches!(e, Entry::Verified(v) if matches!(v.what, What::Reliance { .. })))
        .unwrap();
    let mut forged = serde_json::to_value(&written).unwrap();
    forged["what"]["judge"] = serde_json::json!(RESERVE);
    let forged: Entry = serde_json::from_value(forged).unwrap();
    let at = history
        .iter()
        .position(|e| e == &written)
        .expect("the entry in the admitted history");
    let receipted: Vec<(Receipt, Entry)> = history[..at]
        .iter()
        .enumerate()
        .map(|(i, e)| (core.log.borrow().receipt_at(i), e.clone()))
        .collect();
    let refusal = rutent_contracts::core_internal::admit_entry(
        &receipted,
        &forged,
        Through::General,
        &rutent_contracts::core_internal::DeploymentOperator {
            maker: operator().acts,
            party: operator().party,
        },
    )
    .expect_err("another judge than the statement's maker is refused");
    assert_eq!(refusal.0, "a reliance's judgement is its judge's statement");
}

/// The reliance is in the log before the operation is carried out, and a log that refuses or can't take it leaves
/// the operation unsent; trying again sends it, after the entry.
// verifies: REQ-CPL-12, INV-7
#[test]
fn reliance_is_written_before_the_operation_that_relies_on_it_is_sent() {
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(true);
    rests(&mut core, PAYER, &judgement);
    let (ran, carried, held) = run_watched(&mut world, &mut core);
    ran.expect("runs");
    assert_eq!(carried.len(), 2);
    assert_eq!(
        held[0], 1,
        "the withdrawal, which carries the input, was sent with its reliance in the log"
    );

    // The log refuses the entry: nothing is sent, nothing of the segment is recorded.
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(true);
    rests(&mut core, PAYER, &judgement);
    core.log
        .borrow_mut()
        .faults
        .push_back((On::Reliance, Fault::Refuse));
    let before = core.log.borrow().entries.len();
    let (ran, carried, _) = run_watched(&mut world, &mut core);
    assert!(matches!(ran, Err(SegmentsError::Append(_))), "{ran:?}");
    assert!(carried.is_empty(), "refused, so not sent: {carried:?}");
    only_the_decision_since(&core, before);

    // The log can't take it now, however often it is tried within one run: nothing is sent. Run again, it is sent,
    // after the entry, and the entry is in the log once.
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(true);
    rests(&mut core, PAYER, &judgement);
    for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::Reliance, Fault::Unavailable));
    }
    let (ran, carried, _) = run_watched(&mut world, &mut core);
    assert!(
        matches!(
            ran,
            Err(SegmentsError::Append(
                rutent_contracts::core_internal::calls::AppendFailure::Unavailable
            ))
        ),
        "{ran:?}"
    );
    assert!(carried.is_empty(), "unavailable, so not sent: {carried:?}");
    let (ran, carried, held) = run_watched(&mut world, &mut core);
    ran.expect("runs once the log takes it");
    assert_eq!(carried.len(), 2);
    assert_eq!(held[0], 1, "sent after the entry, on the retry too");
    assert_eq!(relies(&core).len(), 1);
    core.admitted(&world.catalogue).expect("admitted");
}

/// One judgement relied on by one provider is written once, whatever the inputs that rest on it, the segments that
/// carry them and the retries.
// verifies: REQ-CPL-12, INV-8
#[test]
fn one_judgement_relied_on_by_one_provider_is_written_once() {
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(true);
    // Both segments' inputs rest on the one judgement, and both segments are RESERVE's.
    rests(&mut core, PAYER, &judgement);
    rests(&mut core, BENEFICIARY, &judgement);
    let (ran, carried, held) = run_watched(&mut world, &mut core);
    ran.expect("runs");
    assert_eq!(carried.len(), 2);
    assert_eq!(held, vec![1, 1], "in the log before each send");
    assert_eq!(relies(&core).len(), 1, "{:?}", relies(&core));
    assert_eq!(
        core.log.borrow().reliance_appends,
        1,
        "one judgement, one provider: offered to the log once, not once a segment"
    );
    core.admitted(&world.catalogue).expect("admitted");

    // A lost answer: the log took the entry and the writer is told it didn't, so it offers the same entry again.
    let (mut world, mut core, judgement) = judged_by_the_other_reserve(true);
    rests(&mut core, PAYER, &judgement);
    core.log
        .borrow_mut()
        .faults
        .push_back((On::Reliance, Fault::LostAnswer));
    let (ran, _, held) = run_watched(&mut world, &mut core);
    ran.expect("runs");
    assert_eq!(held[0], 1);
    assert_eq!(
        core.log.borrow().reliance_appends,
        2,
        "the same entry, twice"
    );
    assert_eq!(relies(&core).len(), 1, "and the log holds it once");

    // The register itself: the same input resting on another judgement is refused, not replaced, and an input
    // already carried can't be declared.
    let order = id(&core.named.order);
    let other = Judgement {
        judge: id(RESERVE),
        ..judgement.clone()
    };
    let mut fresh = Segments::new();
    fresh.rests_on(&order, PAYER, judgement.clone()).unwrap();
    assert_eq!(fresh.rests_on(&order, PAYER, other), Err(Rests::Another));
    assert_eq!(
        core.segments.rests_on(&order, PAYER, judgement),
        Err(Rests::TooLate),
        "its operation was sent already"
    );
}
