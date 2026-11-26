//! What Core lands in a holding account, and what it doesn't (`T-020-33`'s review; `REQ-AUD-15`, `INV-5`): only value
//! that still rests at the node, once for its segment, and asked again only where the wallet took nothing. Over `T-020-08`'s
//! kit, with a fake holding account at the reserve's ledger, the node the value rests at when the deposit out of it is
//! refused.

mod segments_support;

use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::router_api::Amount;
use rutent_core_orch::segments::{Landed, Landings, SegmentOutcome};
use segments_support::*;

/// A holding account of the reserve's ledger that answers as it is told and keeps what it was asked.
struct Fake {
    node: String,
    answers: Vec<Landed>,
    landed: Vec<(String, String)>,
    released: Vec<(String, String)>,
    /// Whether the account's module restarted since it last confirmed a movement, as the platform reports it.
    restarted: bool,
    /// Every call, in order: what Core asked the account.
    calls: Vec<String>,
}

impl Fake {
    fn at_the_ledger(answers: &[Landed]) -> Self {
        Fake {
            node: format!("{RESERVE}-ledger"),
            answers: answers.to_vec(),
            landed: Vec::new(),
            released: Vec::new(),
            restarted: false,
            calls: Vec::new(),
        }
    }

    fn answer(&mut self) -> Landed {
        if self.answers.is_empty() {
            Landed::Placed
        } else {
            self.answers.remove(0)
        }
    }
}

impl Landings for Fake {
    fn holds(&self, node: &str) -> bool {
        node == self.node
    }

    fn land(&mut self, _: &str, _: &str, order: &str, amount: &Amount) -> Landed {
        self.landed
            .push((order.to_owned(), amount.as_str().to_owned()));
        self.calls.push(format!("land {order} {}", amount.as_str()));
        self.answer()
    }

    fn release(&mut self, _: &str, _: &str, order: &str, amount: &Amount) -> Landed {
        self.released
            .push((order.to_owned(), amount.as_str().to_owned()));
        self.calls
            .push(format!("release {order} {}", amount.as_str()));
        self.answer()
    }

    fn restarted(&self, node: &str) -> bool {
        node == self.node && self.restarted
    }

    fn restore(&mut self, _: &str, _: &str, order: &str, amount: &Amount) -> Landed {
        self.calls
            .push(format!("restore {order} {}", amount.as_str()));
        self.restarted = false;
        Landed::Placed
    }
}

/// The kit's order run with its deposit refused: the withdrawal settled at the ledger and the value rests there.
fn resting_at_the_ledger() -> (World, Core) {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    world
        .fleet
        .world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    core.run(&mut world);
    (world, core)
}

/// Value that rests at the node lands, once for its segment, the whole of what arrived.
// verifies: REQ-AUD-15, INV-5
#[test]
fn value_resting_at_a_held_node_is_landed_once() {
    let (_world, mut core) = resting_at_the_ledger();
    let mut fake = Fake::at_the_ledger(&[]);
    assert_eq!(core.segments.land_settled(&mut fake), 1);
    assert_eq!(fake.landed, [(ORDER.to_owned(), AMOUNT.to_owned())]);
    assert_eq!(core.segments.land_settled(&mut fake), 0);
    assert_eq!(fake.landed.len(), 1, "asked once");
    assert!(fake.released.is_empty());
}

/// Value that passed through the node, the segment out of it settled, lands nothing: the account never holds what
/// the order no longer has there.
// verifies: REQ-AUD-15, INV-5
#[test]
fn value_that_passed_through_a_held_node_is_never_landed() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    let ran = core.run(&mut world);
    let rutent_core_orch::segments::Ran::Segments(done) = &ran else {
        panic!("the part ran: {ran:?}")
    };
    assert!(
        done.iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{done:?}"
    );
    let mut fake = Fake::at_the_ledger(&[]);
    assert_eq!(core.segments.land_settled(&mut fake), 0);
    assert!(fake.landed.is_empty(), "nothing landed: {:?}", fake.landed);
    assert!(fake.released.is_empty());
}

/// A landing the wallet couldn't take now is asked again, until it is placed; one it didn't confirm is never asked
/// again and is counted.
// verifies: REQ-AUD-15, INV-5
#[test]
fn a_landing_is_asked_again_only_where_nothing_took_effect() {
    let (_world, mut core) = resting_at_the_ledger();
    let mut fake = Fake::at_the_ledger(&[Landed::NotNow, Landed::NotNow, Landed::Placed]);
    assert_eq!(core.segments.land_settled(&mut fake), 0);
    assert_eq!(core.segments.land_settled(&mut fake), 0);
    assert_eq!(core.segments.land_settled(&mut fake), 1);
    assert_eq!(fake.landed.len(), 3);
    assert_eq!(core.segments.land_settled(&mut fake), 0);
    assert_eq!(fake.landed.len(), 3, "placed once, not asked again");

    let (_world, mut core) = resting_at_the_ledger();
    let mut fake = Fake::at_the_ledger(&[Landed::Unconfirmed]);
    assert_eq!(core.segments.land_settled(&mut fake), 0);
    assert_eq!(core.segments.land_settled(&mut fake), 0);
    assert_eq!(fake.landed.len(), 1, "never asked again");
    assert_eq!(core.segments.landings_unconfirmed(), 1);
}

/// The withdrawal settles at the ledger and its value lands in the account; then the account's module restarts, and
/// the wallet's record of the order with it, while Core runs on. When the deposit out of the ledger settles, the
/// account is asked to restore what the log holds for the order there before the release, so the release finds it:
/// restore first, then release, each once (`REQ-ORD-43`, `INV-1`, `T-020-46`).
// verifies: REQ-ORD-43, INV-1
#[test]
fn release_after_a_holding_accounts_module_restarts_restores_first() {
    let mut world = World::of(&[RESERVE]);
    let mut core = Core::over(&mut world);
    for _ in 0..rutent_core_orch::segments::APPEND_TRIES {
        core.log
            .borrow_mut()
            .faults
            .push_back((On::EffectOf(DEPOSIT), Fault::Unavailable));
    }
    let session = world.context().session;
    let stopped = core.try_run_with(
        &world.catalogue,
        &mut world.fleet.platform,
        &mut rutent_core_orch::segments::NoProxy,
        &mut Recorded,
        &session,
    );
    assert!(
        stopped.is_err(),
        "the deposit wasn't handed over: {stopped:?}"
    );
    let mut fake = Fake::at_the_ledger(&[]);
    assert_eq!(
        core.segments.land_settled(&mut fake),
        1,
        "the withdrawal's value landed"
    );
    fake.restarted = true;
    let ran = core.run(&mut world);
    let rutent_core_orch::segments::Ran::Segments(done) = &ran else {
        panic!("the part ran: {ran:?}")
    };
    assert!(
        done.iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{done:?}"
    );
    core.segments.land_settled(&mut fake);
    assert_eq!(
        fake.calls,
        [
            format!("land {ORDER} {AMOUNT}"),
            format!("restore {ORDER} {AMOUNT}"),
            format!("release {ORDER} {AMOUNT}"),
        ]
    );
}

/// The withdrawal's value landed at the ledger, then the withdrawal proven undone (its `reversal_completed`, the deposit
/// refused): the Core that ran on releases the landing and counts nothing of it as resting at the ledger, and a Core
/// reopened on the same log restores nothing of it nor counts it, the value having come back to where it came from
/// (`INV-1`, `INV-8`, `T-020-46`).
// verifies: INV-1, INV-8
#[test]
fn reversed_segments_landing_is_not_restored_after_core_reopens() {
    use rutent_contracts::core_internal::{
        Entry, Outcome, TimeSource, VerifiedEntry, VerifiedKind, What, When,
    };
    use rutent_contracts::state_machine::{
        self as stm, Observed, RecordedInput, Source, SourceKind, Standing, Subject,
    };
    let (world, mut core) = resting_at_the_ledger();
    let mut fake = Fake::at_the_ledger(&[]);
    assert_eq!(core.segments.land_settled(&mut fake), 1, "landed");
    let at = rutent_core_orch::snapshot::Clock::now(&*core.clock)
        .timestamp()
        .expect("a time");
    let input = RecordedInput {
        record_id: stm::RecordId::new("rec-reversed").unwrap(),
        recorded_at: stm::Timestamp::new(at.as_str()).unwrap(),
        time_source: stm::Id::new("core_clock").unwrap(),
        source: Source {
            kind: SourceKind::Orchestrator,
            id: stm::Id::new("core-orch").unwrap(),
        },
        standing: Standing::Stated,
        subject: Subject {
            order: stm::Id::new(ORDER).unwrap(),
            part: Some(stm::Id::new(PART).unwrap()),
            segment: Some(stm::Id::new(WITHDRAWAL).unwrap()),
        },
        observed: Observed::ReversalCompleted {
            evidence: stm::Id::new("ev-undone").unwrap(),
        },
    };
    append_entry(
        &core,
        &Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: segments_support::core(),
            what: What::Input {
                input: input.clone(),
            },
            when: When {
                at,
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::Observed {},
        }),
        rutent_contracts::core_internal::Through::General,
    );
    assert!(
        core.segments.observe(&*core.clock, &input),
        "the reversal applies"
    );
    let ledger = (format!("{RESERVE}-ledger"), ORDER.to_owned());
    assert!(
        !core.segments.resting_totals().contains_key(&ledger),
        "the Core that ran on counts no reversed value as resting"
    );
    core.segments.land_settled(&mut fake);
    assert_eq!(
        fake.released,
        [(ORDER.to_owned(), AMOUNT.to_owned())],
        "the Core that ran on releases it"
    );
    segments_support::reopen_segments(&mut core, &world.catalogue, &[]);
    assert!(
        !core.segments.resting_totals().contains_key(&ledger),
        "nor the reopened one: a hand-on's restore restores none of it"
    );
    let mut reopened = Fake::at_the_ledger(&[]);
    reopened.restarted = true;
    core.segments.reconcile_landings(&mut reopened);
    assert!(
        reopened.calls.is_empty(),
        "nothing of the reversed value is landed or restored: {:?}",
        reopened.calls
    );
}
