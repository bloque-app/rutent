//! The stuck clock's figures, as pure functions over a tuple's settlement history, so each rule can be held to its
//! boundary by a test (`REQ-ORD-29`, `REQ-ORD-6`).

use rutent_contracts::core_internal::StuckBasis;

/// How many settled segments a tuple needs before its p99 times the clock: until then, its advertised time does
/// (`REQ-ORD-29`, `THR-25`). One: "until it has history" read as until one segment settled there. A reading,
/// reversible.
pub const HISTORY: usize = 1;

/// How far a tuple's p99 must move from the one last taken as its baseline, in basis points of that baseline, to
/// be a material change raised to the operator (`REQ-ORD-6`, `THR-24`): half of it, either way. A reading,
/// reversible.
pub const MATERIAL_BPS: u64 = 5_000;

/// The p99 of `seconds`, by nearest rank: the smallest sample at or above 99 % of them. `None` with no sample.
/// Below a hundred samples it is the slowest, so a provider's own slowest settlement is the most it can be held
/// to.
pub fn p99(seconds: &[u64]) -> Option<u64> {
    if seconds.is_empty() {
        return None;
    }
    let mut sorted = seconds.to_vec();
    sorted.sort_unstable();
    // ceil(0.99 * n), at least 1, as an index from 1.
    let rank = (sorted.len() * 99).div_ceil(100).max(1);
    sorted.get(rank - 1).copied()
}

/// What the stuck clock of a segment at a tuple runs on: twice its p99 once the tuple has [`HISTORY`], twice its
/// advertised time until then (`REQ-ORD-29`). `None` where neither is known: an advertised time the record
/// couldn't give (`u64::MAX`, the router's "never settles") times nothing.
pub fn clock_figure(history: &[u64], advertised: u64) -> Option<(StuckBasis, u64)> {
    if history.len() >= HISTORY {
        return p99(history).map(|p| (StuckBasis::TwiceP99, p));
    }
    (advertised != u64::MAX).then_some((StuckBasis::TwiceAdvertised, advertised))
}

/// Whether funds held `held` seconds have taken longer than twice `figure`: at exactly twice they haven't.
pub fn over_time(figure: u64, held: u64) -> bool {
    held > figure.saturating_mul(2)
}

/// Whether a p99 of `now` is a material change from a baseline of `before` ([`MATERIAL_BPS`]). A baseline of 0 is
/// counted as 1 second, so a first second of delay after instant settlements is material.
pub fn material(before: u64, now: u64) -> bool {
    let base = u128::from(before.max(1));
    let moved = u128::from(before.abs_diff(now));
    now != before && moved * 10_000 >= u128::from(MATERIAL_BPS) * base
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: REQ-ORD-29
    #[test]
    fn p99_by_nearest_rank_is_the_slowest_below_a_hundred_samples() {
        assert_eq!(p99(&[]), None);
        assert_eq!(p99(&[30]), Some(30));
        assert_eq!(p99(&[10, 50, 30]), Some(50));
        let hundred: Vec<u64> = (1..=100).collect();
        assert_eq!(p99(&hundred), Some(99));
        let two_hundred: Vec<u64> = (1..=200).collect();
        assert_eq!(p99(&two_hundred), Some(198));
    }

    // verifies: REQ-ORD-29
    #[test]
    fn clock_runs_on_the_advertised_time_until_the_tuple_has_history() {
        assert_eq!(
            clock_figure(&[], 60),
            Some((StuckBasis::TwiceAdvertised, 60))
        );
        assert_eq!(clock_figure(&[30], 60), Some((StuckBasis::TwiceP99, 30)));
        assert_eq!(clock_figure(&[], u64::MAX), None);
    }

    // verifies: REQ-ORD-29
    #[test]
    fn exactly_twice_the_figure_is_not_over_time_and_a_second_more_is() {
        assert!(!over_time(30, 60));
        assert!(over_time(30, 61));
        assert!(!over_time(0, 0));
        assert!(over_time(0, 1));
        assert!(!over_time(u64::MAX, u64::MAX));
    }

    // verifies: REQ-ORD-6
    #[test]
    fn a_change_of_half_the_baseline_either_way_is_material() {
        assert!(!material(30, 30));
        assert!(!material(30, 44));
        assert!(material(30, 45));
        assert!(material(30, 15));
        assert!(!material(30, 16));
        assert!(material(0, 1));
    }
}
