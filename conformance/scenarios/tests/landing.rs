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
}

impl Fake {
    fn at_the_ledger(answers: &[Landed]) -> Self {
        Fake {
            node: format!("{RESERVE}-ledger"),
            answers: answers.to_vec(),
            landed: Vec::new(),
            released: Vec::new(),
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
        self.answer()
    }

    fn release(&mut self, _: &str, _: &str, order: &str, amount: &Amount) -> Landed {
        self.released
            .push((order.to_owned(), amount.as_str().to_owned()));
        self.answer()
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
