//! The stuck clock's figures, as pure functions and one tuple's settlement history, so each rule can be held to its
//! boundary by a test (`REQ-ORD-29`, `REQ-ORD-6`).

use std::collections::VecDeque;

use rutent_contracts::core_internal::StuckBasis;

/// How many settled segments a tuple needs before its own p99 times the clock: until then, twice its advertised
/// time does (`REQ-ORD-29`, `THR-25`). Twenty, so a handful of fast settlements can't set a p99 the provider then
/// misses. A reading, reversible, on the owner's form.
pub const HISTORY: usize = 20;

/// How many of a tuple's latest settlements its p99 is read over, so the figure follows the provider down as well
/// as up. A reading, reversible, on the owner's form.
pub const WINDOW: usize = 200;

/// How far a tuple's p99 must move from the one last taken as its baseline, in basis points of that baseline, to
/// be a material change raised to the operator (`REQ-ORD-6`, `THR-24`): half of it, either way. The first p99 is
/// compared with the advertised time the same way. A reading, reversible.
pub const MATERIAL_BPS: u64 = 5_000;

/// The p99 of `seconds`, by nearest rank: the smallest sample at or above 99 % of them. `None` with no sample.
/// Below a hundred samples it is the slowest.
pub fn p99<'a>(seconds: impl IntoIterator<Item = &'a u64>) -> Option<u64> {
    let mut sorted: Vec<u64> = seconds.into_iter().copied().collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_unstable();
    // ceil(0.99 * n), at least 1, as an index from 1.
    let rank = (sorted.len() * 99).div_ceil(100).max(1);
    sorted.get(rank - 1).copied()
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

/// A material change in a tuple's settlement time, to raise to the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// The figure before: the baseline p99, or the advertised time when the first p99 is formed.
    pub before: u64,
    /// The p99 now.
    pub now: u64,
    /// Whether this is the first p99 formed, compared with the advertised time.
    pub formed: bool,
}

/// One tuple's settlement history: its latest [`WINDOW`] settlement times, how many it has had, and the baseline its
/// p99's changes are measured from.
#[derive(Debug, Clone, Default)]
pub struct History {
    window: VecDeque<u64>,
    settled: usize,
    baseline: Option<u64>,
}

impl History {
    /// No settlement yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one settlement time; `advertised` is the time the segment declared with its quote. Answers the material
    /// change it makes, if any: the first p99, once the tuple has [`HISTORY`], against the advertised time; each
    /// later one against the baseline. Each p99 raised is the baseline from then.
    pub fn add(&mut self, seconds: u64, advertised: u64) -> Option<Change> {
        self.window.push_back(seconds);
        if self.window.len() > WINDOW {
            self.window.pop_front();
        }
        self.settled = self.settled.saturating_add(1);
        if self.settled < HISTORY {
            return None;
        }
        let now = p99(&self.window)?;
        match self.baseline {
            None => {
                self.baseline = Some(now);
                (advertised != u64::MAX && material(advertised, now)).then_some(Change {
                    before: advertised,
                    now,
                    formed: true,
                })
            }
            Some(before) if material(before, now) => {
                self.baseline = Some(now);
                Some(Change {
                    before,
                    now,
                    formed: false,
                })
            }
            Some(_) => None,
        }
    }

    /// What the stuck clock of a segment at this tuple runs on: twice its p99 once it has [`HISTORY`], twice
    /// `advertised` until then (`REQ-ORD-29`). `None` where neither is known: an advertised time the record couldn't
    /// give (`u64::MAX`, the router's "never settles") times nothing.
    pub fn figure(&self, advertised: u64) -> Option<(StuckBasis, u64)> {
        if self.settled >= HISTORY {
            return p99(&self.window).map(|p| (StuckBasis::TwiceP99, p));
        }
        (advertised != u64::MAX).then_some((StuckBasis::TwiceAdvertised, advertised))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(seconds: u64, count: usize, advertised: u64) -> (History, Vec<Change>) {
        let mut history = History::new();
        let changes = (0..count)
            .filter_map(|_| history.add(seconds, advertised))
            .collect();
        (history, changes)
    }

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
    fn clock_runs_on_the_advertised_time_until_the_tuple_has_twenty_settlements() {
        // Twenty, the default on the owner's form: nineteen settlements aren't a history yet.
        let (history, _) = filled(30, 19, 30);
        assert_eq!(history.figure(60), Some((StuckBasis::TwiceAdvertised, 60)));
        let (history, _) = filled(30, 20, 30);
        assert_eq!(history.figure(60), Some((StuckBasis::TwiceP99, 30)));
        assert_eq!(History::new().figure(u64::MAX), None);
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

    /// Two hundred settlements at 100 s and then fast ones at 10 s: the p99 over the window follows them down, and
    /// the fall is raised once the slow ones have left the window's top rank.
    // verifies: REQ-ORD-6, REQ-ORD-29
    #[test]
    fn p99_falls_after_a_long_slow_history_and_the_fall_is_raised() {
        let (mut history, _) = filled(100, WINDOW, 100);
        assert_eq!(history.figure(100), Some((StuckBasis::TwiceP99, 100)));
        let falls: Vec<Change> = (0..WINDOW).filter_map(|_| history.add(10, 100)).collect();
        assert_eq!(
            falls,
            vec![Change {
                before: 100,
                now: 10,
                formed: false
            }]
        );
        assert_eq!(history.figure(100), Some((StuckBasis::TwiceP99, 10)));
    }

    /// The first p99, formed at twenty settlements, is compared with the time the segment advertised: one five
    /// times slower is raised, one close to it isn't.
    // verifies: REQ-ORD-6
    #[test]
    fn slow_first_settlements_against_the_advertised_time_are_raised() {
        let (_, changes) = filled(300, HISTORY, 60);
        assert_eq!(
            changes,
            vec![Change {
                before: 60,
                now: 300,
                formed: true
            }]
        );
        let (_, changes) = filled(70, HISTORY, 60);
        assert!(changes.is_empty());
    }
}
