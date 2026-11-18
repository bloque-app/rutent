//! The bulk measures of `REQ-AUD-7`: a pure function of what the log holds (`T-022-06`).
//!
//! [`measure`] reads a slice of the log's entries, as the log hands them out, and the window it is asked over, and
//! answers a [`Report`]. It reads nothing else: no figure is passed in beside the log's own records, so a measure
//! can't be made to say what the log doesn't hold. The records it counts are the log's `measure` entries, each
//! citing the entries it rests on (`REQ-AUD-2`), written by [`super::Record`].
//!
//! This module imports data types of the contracts and nothing else: no handle on the log, no clock, no channel to
//! the operator and no module of `core-orch`. A function here takes shared references and returns a value, so it can
//! reach no ledger, store or provider, and nothing that moves value or changes an order (ADR-0009's reasoning, which
//! `REQ-AUD-7` shares: these measures detect only). A test reads this file's source and holds that
//! (`measure_modules_reach_nothing_that_moves_value`).
//!
//! **What each measure counts, and over what window.** Every measure counts the `measure` records of the log whose
//! own time (`at`, by Core's clock) lies in the [`Window`], half open: from its first instant, up to but not
//! including its last. A record is counted once per entry: two records are two measures, and a record written twice
//! is one entry (the log answers an equal entry again with its own receipt). Each measure is kept per provider or
//! verifier, as the record names it, in the role it names (`T-020-30`): a party in both roles has its measures as a
//! provider and as a verifier apart, and nothing is summed across them. [`COUNTS`] says it in words for each.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use rutent_contracts::core_internal::{
    Entry, Market, Measure, MeasuredRole, Receipt, Said, SegmentEnd, What,
};
use rutent_contracts::router_api::{Amount, Timestamp};

/// What each measure counts, in words, by the name the report gives it.
pub const COUNTS: [(&str, &str); 6] = [
    (
        "quotes",
        "per asset, the quotes of the provider in the window, each against the amount executed: how many, how many \
         executed below, at and above the quote, and by how much in all below and above (None once a total passes \
         what an amount states)",
    ),
    (
        "dry_runs",
        "the dry runs of the provider in the window, by what it said and how the segment ended: yes then done, yes \
         then failed, no then done, no then failed",
    ),
    (
        "failures",
        "the failures of the provider in the window, by the market's direction over the segment, and those with no \
         market figure held, counted apart",
    ),
    (
        "refusals",
        "the refusals of the provider in the window, one for each record",
    ),
    (
        "reversals",
        "the reversals of the provider in the window, one for each record",
    ),
    (
        "answers",
        "the yes and no answers of the provider or verifier in the window, so its rate of no is no of yes and no",
    ),
];

/// A stretch of time, half open: `from` is in it, `until` is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// Its first instant.
    pub from: Timestamp,
    /// Its end, which is not in it.
    pub until: Timestamp,
}

/// Nanoseconds since 1970-01-01T00:00:00Z of `time`, an RFC 3339 UTC date-time ending in `Z` (the contract's
/// timestamp), its fraction read to the nanosecond.
fn nanos(time: &Timestamp) -> Option<i128> {
    let b = time.as_str().as_bytes();
    let number = |range: std::ops::Range<usize>| -> Option<i128> {
        b.get(range)?.iter().try_fold(0i128, |n, d| {
            d.is_ascii_digit().then(|| n * 10 + i128::from(d - b'0'))
        })
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    let rest = b.get(19..)?;
    let (digits, rest) = match rest.strip_prefix(b".") {
        Some(after) => after.split_at(after.iter().take_while(|c| c.is_ascii_digit()).count()),
        None => (&rest[..0], rest),
    };
    if rest != b"Z" {
        return None;
    }
    // Nine places of the fraction, short ones padded with zeros, longer ones cut.
    let fraction = digits
        .iter()
        .chain(std::iter::repeat(&b'0'))
        .take(9)
        .fold(0i128, |n, d| n * 10 + i128::from(d - b'0'));
    // Howard Hinnant's days-from-civil, on the proleptic Gregorian calendar.
    let y = year - i128::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    Some(seconds * 1_000_000_000 + fraction)
}

impl Window {
    /// Whether `at` lies in the window. A window whose end isn't after its start holds nothing.
    pub fn holds(&self, at: &Timestamp) -> bool {
        match (nanos(&self.from), nanos(&self.until), nanos(at)) {
            (Some(from), Some(until), Some(at)) => from <= at && at < until,
            _ => false,
        }
    }
}

/// A part of a whole, as counts: never a float, never rounded until it is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    /// How many.
    pub part: u64,
    /// Out of how many.
    pub of: u64,
}

impl Rate {
    /// The rate in parts per million, rounded down, or `None` where there is nothing to take a rate of.
    pub fn per_million(&self) -> Option<u64> {
        (self.of > 0).then(|| {
            u64::try_from(u128::from(self.part) * 1_000_000 / u128::from(self.of))
                .expect("a part of a whole in parts per million fits 64 bits")
        })
    }
}

/// One provider's quotes against what was executed, in one asset (`REQ-AUD-7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteMeasure {
    /// The quotes counted.
    pub quotes: u64,
    /// Those executed below the amount quoted.
    pub executed_below: u64,
    /// Those executed at the amount quoted.
    pub executed_at: u64,
    /// Those executed above it.
    pub executed_above: u64,
    /// The total by which the below ones fell short, or `None` once the total passes what an amount states.
    pub below_by: Option<Amount>,
    /// The total by which the above ones exceeded, or `None` once the total passes what an amount states.
    pub above_by: Option<Amount>,
}

impl Default for QuoteMeasure {
    fn default() -> Self {
        QuoteMeasure {
            quotes: 0,
            executed_below: 0,
            executed_at: 0,
            executed_above: 0,
            below_by: Some(Amount::zero()),
            above_by: Some(Amount::zero()),
        }
    }
}

impl QuoteMeasure {
    /// The rate of quotes executed below the quote.
    pub fn rate_below(&self) -> Rate {
        Rate {
            part: self.executed_below,
            of: self.quotes,
        }
    }

    /// This measure with one more quote, `executed` against `quoted`.
    fn with(self, quoted: &Amount, executed: &Amount) -> Self {
        let order = if quoted == executed {
            Ordering::Equal
        } else if executed.checked_sub(quoted).is_some() {
            Ordering::Greater
        } else {
            Ordering::Less
        };
        let added = |total: Option<Amount>, by: Option<Amount>| match (total, by) {
            (Some(total), Some(by)) => total.checked_add(&by),
            _ => None,
        };
        let (below, above) = (quoted.checked_sub(executed), executed.checked_sub(quoted));
        QuoteMeasure {
            quotes: self.quotes + 1,
            executed_below: self.executed_below + u64::from(order == Ordering::Less),
            executed_at: self.executed_at + u64::from(order == Ordering::Equal),
            executed_above: self.executed_above + u64::from(order == Ordering::Greater),
            below_by: if order == Ordering::Less {
                added(self.below_by, below)
            } else {
                self.below_by
            },
            above_by: if order == Ordering::Greater {
                added(self.above_by, above)
            } else {
                self.above_by
            },
        }
    }
}

/// One provider's dry runs against how its segments ended (`REQ-AUD-7`, `THR-26`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DryRuns {
    /// It said yes, and the segment completed.
    pub yes_then_done: u64,
    /// It said yes, and the segment failed.
    pub yes_then_failed: u64,
    /// It said no, and the segment completed.
    pub no_then_done: u64,
    /// It said no, and the segment failed.
    pub no_then_failed: u64,
}

impl DryRuns {
    /// The dry runs that said yes of those that failed or completed: how often a yes held, as the rate of yes that
    /// then failed.
    pub fn yes_that_failed(&self) -> Rate {
        Rate {
            part: self.yes_then_failed,
            of: self.yes_then_done + self.yes_then_failed,
        }
    }

    /// These dry runs with one more.
    fn with(self, said: Said, outcome: SegmentEnd) -> Self {
        let one = |hit: bool| u64::from(hit);
        let (yes, done) = (said == Said::Yes, outcome == SegmentEnd::Done);
        DryRuns {
            yes_then_done: self.yes_then_done + one(yes && done),
            yes_then_failed: self.yes_then_failed + one(yes && !done),
            no_then_done: self.no_then_done + one(!yes && done),
            no_then_failed: self.no_then_failed + one(!yes && !done),
        }
    }
}

/// One provider's failures against the market's direction (`REQ-AUD-7`, `THR-27`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Failures {
    /// Failures over a segment on which the market moved the provider's way.
    pub with_the_provider: u64,
    /// Failures on which the market moved against it.
    pub against_the_provider: u64,
    /// Failures on which it did not materially move.
    pub flat: u64,
    /// Failures with no market figure held: counted apart, never put in a direction.
    pub no_figure: u64,
}

impl Failures {
    /// All the failures counted.
    pub fn all(&self) -> u64 {
        self.with_the_provider + self.against_the_provider + self.flat + self.no_figure
    }

    /// These failures with one more, over `market`.
    fn with(self, market: Option<Market>) -> Self {
        let one = |hit: bool| u64::from(hit);
        Failures {
            with_the_provider: self.with_the_provider
                + one(market == Some(Market::WithTheProvider)),
            against_the_provider: self.against_the_provider
                + one(market == Some(Market::AgainstTheProvider)),
            flat: self.flat + one(market == Some(Market::Flat)),
            no_figure: self.no_figure + one(market.is_none()),
        }
    }
}

/// One provider's or verifier's answers, for its rate of no (`REQ-AUD-7`, `THR-56`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Answers {
    /// Its yeses.
    pub yes: u64,
    /// Its noes.
    pub no: u64,
}

impl Answers {
    /// The rate of no among its answers.
    pub fn rate_of_no(&self) -> Rate {
        Rate {
            part: self.no,
            of: self.yes + self.no,
        }
    }
}

/// Every measure of one provider or verifier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provider {
    /// Quotes against executed prices, per asset, by the asset's code and decimals.
    pub quotes: BTreeMap<(String, u8), QuoteMeasure>,
    /// Dry runs against outcomes.
    pub dry_runs: DryRuns,
    /// Failures against the market's direction.
    pub failures: Failures,
    /// Refusals.
    pub refusals: u64,
    /// Reversals.
    pub reversals: u64,
    /// Answers, for the rate of no.
    pub answers: Answers,
}

impl Provider {
    /// This provider's measures with one more record.
    fn with(self, measure: &Measure) -> Self {
        match measure {
            Measure::Quote {
                quoted,
                executed,
                asset,
            } => {
                let key = (asset.id.clone(), asset.decimals);
                let Provider {
                    mut quotes,
                    dry_runs,
                    failures,
                    refusals,
                    reversals,
                    answers,
                } = self;
                let counted = quotes
                    .remove(&key)
                    .unwrap_or_default()
                    .with(quoted, executed);
                quotes.insert(key, counted);
                Provider {
                    quotes,
                    dry_runs,
                    failures,
                    refusals,
                    reversals,
                    answers,
                }
            }
            Measure::DryRun { said, outcome } => Provider {
                dry_runs: self.dry_runs.with(*said, *outcome),
                ..self
            },
            Measure::Failure { market } => Provider {
                failures: self.failures.with(*market),
                ..self
            },
            Measure::Refusal {} => Provider {
                refusals: self.refusals + 1,
                ..self
            },
            Measure::Reversal {} => Provider {
                reversals: self.reversals + 1,
                ..self
            },
            Measure::Answer { said } => Provider {
                answers: Answers {
                    yes: self.answers.yes + u64::from(*said == Said::Yes),
                    no: self.answers.no + u64::from(*said == Said::No),
                },
                ..self
            },
        }
    }
}

/// The measures of a window, per provider or verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The window every measure counts over.
    pub window: Window,
    /// The `measure` records counted, in all.
    pub counted: u64,
    /// Each party the window's records measure as a provider, by its identifier, with its measures in that role.
    pub providers: BTreeMap<String, Provider>,
    /// Each party the window's records measure as a verifier, by its identifier, with its measures in that role: a
    /// party in both roles is measured apart in each (`REQ-AUD-7`, `REQ-AUD-8`).
    pub verifiers: BTreeMap<String, Provider>,
}

/// The bulk measures of `history`, the entries of the log with their receipts as the log hands them out, over
/// `window`. Nothing but the log's own `measure` entries is counted.
pub fn measure(history: &[(Receipt, Entry)], window: &Window) -> Report {
    let mut providers: BTreeMap<String, Provider> = BTreeMap::new();
    let mut verifiers: BTreeMap<String, Provider> = BTreeMap::new();
    let mut counted = 0;
    for (_, entry) in history {
        let Entry::Verified(verified) = entry else {
            continue;
        };
        let What::Measure { record } = &verified.what else {
            continue;
        };
        if !window.holds(&record.at) {
            continue;
        }
        // Each role its own key: a party in both is measured apart in each (`REQ-AUD-8`).
        let name = record.provider.as_str().to_owned();
        match record.role {
            MeasuredRole::Provider => {
                let provider = providers.remove(&name).unwrap_or_default();
                providers.insert(name, provider.with(&record.measure));
            }
            MeasuredRole::Verifier => {
                let verifier = verifiers.remove(&name).unwrap_or_default();
                verifiers.insert(name, verifier.with(&record.measure));
            }
        }
        counted += 1;
    }
    Report {
        window: window.clone(),
        counted,
        providers,
        verifiers,
    }
}
