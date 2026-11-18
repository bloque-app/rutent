//! Eligibility on the shortlisted route (`T-021-07`, `REQ-CPL-6`, `DEC-16`): in v0 each shortlisted provider checks
//! eligibility itself, for the parties its rules name, as its own answer to Core's question; a no drops its candidate,
//! and the route is worked out again without it. Over `T-020-06`'s rounds kit, unchanged.

mod rounds_support;
mod snapshot_support;

use rounds_support::*;
use rutent_contracts::router_api::DropReason;
use rutent_core_orch::rounds::{History, Offer, Routed};
use snapshot_support::*;

fn run(env: &mut Env, sim: &mut Sim, inputs: &Inputs) -> Routed {
    env.route(sim, inputs.routing(), &mut History::new())
        .expect("the routing runs")
}

/// Whether the chosen route of `routed` passes through `provider`.
fn chosen_through(routed: &Routed, provider: &str) -> bool {
    let record = &routed.choices.last().expect("a choice round").record;
    record.chosen().is_some_and(|c| {
        c.parts
            .iter()
            .flat_map(|p| p.operations.iter())
            .any(|o| o.provider == id(provider))
    })
}

/// The cheaper ramp, shortlisted and asked, says no for the order's parties: its candidate is dropped, and the route
/// is worked out again, in a further choice round, through the other ramp; nothing in Core takes the no for a yes.
/// Without the no the cheaper ramp is chosen.
// verifies: REQ-CPL-6
#[test]
fn eligibility_no_brings_a_recalculation() {
    let inputs = polygon_to_base_inputs();

    // Without a no: the cheaper ramp carries the order.
    let mut env = Env::new(ramp_catalogue(), true, 100);
    let mut sim = env.sim();
    sim.fees.insert(RAMP_B.into(), 10);
    let routed = run(&mut env, &mut sim, &inputs);
    assert!(
        matches!(routed.offer, Offer::Offered { .. }),
        "{:?}",
        routed.offer
    );
    assert!(chosen_through(&routed, RAMP_A));

    // The cheaper ramp says no: it was shortlisted and asked, its candidate is dropped, and the order goes through
    // the other ramp.
    let mut env = Env::new(ramp_catalogue(), true, 100);
    let mut sim = ramp_sim(&env);
    let routed = run(&mut env, &mut sim, &inputs);
    assert!(
        routed
            .shortlist
            .record
            .shortlist()
            .iter()
            .flat_map(|c| c.parts.iter().flat_map(|p| p.operations.iter()))
            .any(|o| o.provider == id(RAMP_A)),
        "the ramp that says no was shortlisted"
    );
    assert!(!sim.asked(RAMP_A).is_empty(), "and asked");
    let first = &routed.choices[0].record;
    assert!(
        first.chosen.is_none(),
        "nothing is chosen on the first answers"
    );
    assert!(
        first.candidates.iter().any(|c| {
            c.dropped.is_some()
                && !matches!(c.dropped, Some(DropReason::QuoteVoided { .. }))
                && c.parts
                    .iter()
                    .flat_map(|p| p.operations.iter())
                    .any(|o| o.provider == id(RAMP_A))
        }),
        "the ramp's candidate is dropped on its no: {:?}",
        first
            .candidates
            .iter()
            .map(|c| &c.dropped)
            .collect::<Vec<_>>()
    );
    assert!(routed.choices.len() > 1, "the route is worked out again");
    assert!(
        matches!(routed.offer, Offer::Offered { .. }),
        "{:?}",
        routed.offer
    );
    assert!(!chosen_through(&routed, RAMP_A));
    assert!(chosen_through(&routed, RAMP_B));
    env.admitted().expect("the log admits every entry");
}
