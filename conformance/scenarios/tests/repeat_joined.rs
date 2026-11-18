//! A repeated delivery on the joined route (`T-013-09`; `INV-8`): every operation reaches its provider twice, beneath
//! Core, and the simulated exchange, which declares no safe retry and moves again on a repeat (`T-024-01`), moves
//! nothing twice, because the platform refuses the repeat. The runs are `T-024-08`'s (`invariants_support`).

mod execution_support;
mod invariants_support;

use invariants_support::*;

// verifies: INV-8
#[test]
fn repeated_delivery_on_the_joined_route_moves_nothing_twice() {
    let mut moved = 0;
    for seed in [110, 210, 310, 410, 510] {
        let sim = play(Plan {
            seed,
            shape: Shape::Joined,
            kind: Kind::Repeated,
        });
        let name = sim.name();
        let books = sim.run.movements();
        let twice: Vec<_> = books.iter().filter(|(_, n)| **n > 1).collect();
        assert!(
            twice.is_empty(),
            "{name}: moved more than once in a provider's book: {twice:?}"
        );
        // The repeat was planted on this route: providers that declare a safe retry answered it, where the
        // platform let it through, and the exchange, which doesn't, was refused it.
        assert!(
            sim.run.delivered_twice > 0,
            "{name}: no operation reached its provider twice, so the duplicate was not planted"
        );
        moved += books.len();
    }
    assert!(
        moved > 0,
        "no run moved anything, so none could move it twice"
    );
}
