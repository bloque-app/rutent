//! `T-020-55`'s differential: Core's own answers to policy's questions beside the assembly's records, answer by
//! answer, over the assembly's seeded scenarios, each with a restart among its steps.

use rutent_scenarios::assembly::Staged;
use rutent_scenarios::harness::{self, Given, Scenario};

/// Every answer Core's own sources gave is the records' over the seeded scenarios: nothing disagrees, and each of
/// Core's sources the scenarios ask is compared.
// verifies: REQ-AUZ-5
#[test]
fn cores_own_answers_are_the_assemblys_records_over_its_seeded_scenarios() {
    let mut compared = std::collections::BTreeMap::<String, usize>::new();
    let mut disagreed = Vec::new();
    for seed in 11..16 {
        let mut scenario = Scenario::new(
            "the seeded steps, compared",
            "AC-28",
            Given::one_reserve(Staged::AtOnce),
        );
        for step in harness::seeded_steps(seed) {
            scenario = scenario.when(step);
        }
        let mut environment = harness::reference();
        let _ = scenario.play_probed(
            seed,
            environment.as_mut(),
            harness::reference(),
            &mut |_| {},
            &mut |core| {
                let seen = core.comparison();
                for (kind, n) in seen.compared {
                    *compared.entry(kind).or_default() += n;
                }
                disagreed.extend(seen.disagreed);
            },
        );
    }
    assert!(disagreed.is_empty(), "{disagreed:#?}");
    assert!(!compared.is_empty(), "nothing was compared");
    eprintln!("compared: {compared:?}");
}
