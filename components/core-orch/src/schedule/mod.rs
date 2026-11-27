//! Core's own schedule (`T-020-48`, finding 4 of `M3`'s milestone review of 2026-11-24): one entry point, [`Schedule::tick`],
//! which the deployment calls on a timer, the one thing left to it, and the scenarios call on Core's clock.
//!
//! Each tick runs what is due, in this order, so that one job's result is there for the next:
//!
//! 1. the late answers the platform holds for Core's segments ([`Job::LateAnswers`], `AC-5`): each is kept, as a
//!    statement that moves nothing;
//! 2. the value of each segment that settled at a public node a holding account holds, landed in it
//!    ([`Job::LandSettled`], `REQ-ORD-40`, `INV-5`), the measures the segments' endings gave written
//!    ([`Job::WriteMeasures`], `REQ-AUD-7`), the pairs of adjacent reports compared ([`Job::WritePairs`], `REQ-ORD-9`,
//!    `THR-14`), and the orders that completed closed, each reporting its executed price once ([`Job::Close`],
//!    `REQ-QTE-7`);
//! 3. the check of each holding account ([`Job::HoldingCheck`], `REQ-AUD-15`, `AC-39`, `INV-1`'s second half): its
//!    balance is read by Core itself, a decided read recorded as an effect before the module is asked
//!    (`read_holding_balance`, `T-020-63`: the account's module's `holding_balance` at a public node, its publisher's
//!    signed word at a private node), never handed in. It is set beside what Core's own segments rest at the node
//!    (`INV-1`'s second half), a public node's beside the wallet host's own record too, and goes to the log's checks. An
//!    account whose module gives no balance, one policy didn't let Core read, one Core's clock couldn't time, and a holding
//!    account the catalogue lists that the register doesn't name, is raised to the operator (`holding_not_read`);
//! 4. the publishers' records of their private nodes' segments ([`Job::PublishersRecords`], `REQ-ORD-41`): each private
//!    node the routes Core runs pass is asked of the providers of its publisher through the platform
//!    (`inner_segments_query`, `T-020-58`), and an answer is taken only as a record of that node;
//! 5. a status question again for each segment still unknown, at its provider's cadence ([`Job::StatusAsks`],
//!    `REQ-ORD-1`); each followed order's part timed, a material change of a p99 raised and a part over its time
//!    declared stuck ([`Job::Watch`], `REQ-ORD-29`, `REQ-ORD-6`, `AC-5`); and what Core kept of an answer handed to the
//!    stuck mover ([`Job::StuckReports`], `REQ-ORD-1`, `AC-6`): a segment its provider says failed is reported, not
//!    left unknown until its window;
//! 6. the filed waits past their lifetime ended ([`Job::EndExpired`], `T-020-51`);
//! 7. the pairs a restart dropped, raised to the operator and recorded once ([`Job::UnbuiltPairs`], `REQ-ORD-9`);
//! 8. the notices the log's changes call for, sent ([`Job::Notify`], `REQ-ORD-20`), and the checkpoints due on the
//!    schedule fixed in advance, issued and published ([`Job::Checkpoints`], `REQ-AUD-14`), so they hold what the jobs
//!    before them wrote;
//! 9. Core's order run ([`Job::Run`], `T-020-61`, the [`run`] module), last: what each started order runs on held, each
//!    part run on the route it runs on now, a segment begun before a restart carried on, a failed part recalculated and a
//!    hand-on done. What it does is the next tick's jobs' to land, close, check, time, notify and checkpoint, as a
//!    deployment's run between two ticks was.
//!
//! **The schedule decides when; each job's body is Core's** ([`jobs`]): the schedule adds a cadence for each job, an
//! operator's setting with a default ([`Cadences`]; `T-020-49` makes it a decided one), and the time each job last ran,
//! stamped only once the job succeeded, so a job that failed is tried again on the next tick and not after its whole
//! cadence. A restart forgets when each job last ran, so a reopened Core runs every job on its first tick (it fails
//! closed: a job done twice is done once, as the jobs' own rules say); it doesn't forget when each segment was last asked
//! its status, which [`Schedule::recorded`] rebuilds from the log's `ask_status` and `repeat_operation` effects, so the first tick presses no
//! provider again (`T-020-63`). What each job does is [`jobs::Jobs`]'s, over [`jobs::Ports`], which the
//! deployment answers with what only it has (the platform, its register of holding accounts, the log, the evidence
//! store, the notices channel, the clock, policy) and which decide nothing. [`Duties`] is the seam between the two, for
//! the schedule's own tests.

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::{Action, Entry, HoldingReading, Receipt, What};
use rutent_contracts::provider_role::{InnerSegments, Statement};
use rutent_contracts::router_api::Id;

use crate::closing::{Reported as PriceReported, Unreported};
use crate::reversal::StatusAsked;
use crate::segments::UnbuiltPair;
use crate::stuck::Watch;

pub mod jobs;
pub mod run;

pub use jobs::{
    Actors, Jobs, MoverLent, NotConfirmed, Placed, Ports, PublicAccount, PublisherAccount,
    RecalculationsLent, Started, StuckLent, confirm_reported_failure,
};
pub use run::{HandedOnRan, PartRan, Runs, resume};

/// How often a segment left unknown is asked its status again until the operator or its provider says otherwise: a
/// provider is not pressed (an hour).
pub const DEFAULT_STATUS_SECONDS: u64 = 3600;

/// How often an account the holding check couldn't read, or couldn't check, is read again until it is (five minutes): its
/// own cadence, so one unreadable account neither presses its module every tick nor makes the accounts that were read
/// wait again (`T-020-63`). A reading, reversible.
pub const HOLDING_RETRY_SECONDS: u64 = 300;

/// One of Core's scheduled jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Job {
    /// Core's order run (`T-020-61`): the hand-ons done, what each started order runs on held, each part run on the
    /// route it runs on now, a segment begun before a restart carried on, and a failed part recalculated
    /// (`REQ-ORD-40`, `REQ-ORD-22`, `REQ-ORD-43`, `NFR-6`).
    Run,
    /// The late answers the platform holds, taken (`AC-5`).
    LateAnswers,
    /// What settled at a public node a holding account holds, landed in the account (`REQ-ORD-40`).
    LandSettled,
    /// The measure records the segments' endings gave, written (`REQ-AUD-7`).
    WriteMeasures,
    /// The pairs of adjacent reports the segments' endings gave, compared (`REQ-ORD-9`, `THR-14`).
    WritePairs,
    /// The orders that completed closed: each reports its executed price, once (`REQ-QTE-7`).
    Close,
    /// Each holding account read and checked (`REQ-AUD-15`).
    HoldingCheck,
    /// The publishers' records of their private nodes, read (`REQ-ORD-41`).
    PublishersRecords,
    /// A status question again for each segment still unknown (`REQ-ORD-1`).
    StatusAsks,
    /// Each followed order's part timed: a material change of a p99 raised, a part over its time declared stuck
    /// (`REQ-ORD-29`, `REQ-ORD-6`).
    Watch,
    /// What a provider's kept answer says of a stuck move, reported to its mover (`AC-6`).
    StuckReports,
    /// The waits for a customer past their lifetime, ended (`T-020-51`).
    EndExpired,
    /// The pairs of reports a restart dropped, surfaced (`REQ-ORD-9`).
    UnbuiltPairs,
    /// The notices the log's changes call for, sent (`REQ-ORD-20`).
    Notify,
    /// The checkpoints due on the schedule fixed in advance, issued and published (`REQ-AUD-14`).
    Checkpoints,
}

impl Job {
    /// Every job, in the order a tick runs them.
    pub const ALL: [Job; 15] = [
        Job::LateAnswers,
        Job::LandSettled,
        Job::WriteMeasures,
        Job::WritePairs,
        Job::Close,
        Job::HoldingCheck,
        Job::PublishersRecords,
        Job::StatusAsks,
        Job::Watch,
        Job::StuckReports,
        Job::EndExpired,
        Job::UnbuiltPairs,
        Job::Notify,
        Job::Checkpoints,
        Job::Run,
    ];

    /// The job's name, as the operator's `cadence` setting names it (`core-internal`'s `OperatorSetting::Cadence`).
    pub fn name(self) -> &'static str {
        match self {
            Job::Run => "run",
            Job::LateAnswers => "late_answers",
            Job::LandSettled => "land_settled",
            Job::WriteMeasures => "write_measures",
            Job::WritePairs => "write_pairs",
            Job::Close => "close",
            Job::HoldingCheck => "holding_check",
            Job::PublishersRecords => "publishers_records",
            Job::StatusAsks => "status_asks",
            Job::Watch => "watch",
            Job::StuckReports => "stuck_reports",
            Job::EndExpired => "end_expired",
            Job::UnbuiltPairs => "unbuilt_pairs",
            Job::Notify => "notify",
            Job::Checkpoints => "checkpoints",
        }
    }

    /// The job named `name`, if one is.
    pub fn named(name: &str) -> Option<Job> {
        Job::ALL.into_iter().find(|job| job.name() == name)
    }

    /// The cadence a job has until the operator sets another, in seconds. A job whose work is only what has happened
    /// since it last ran runs every tick (zero); a check of a balance, or of a private node's record, which ask a module, runs hourly. The status job runs every
    /// tick and holds each segment to its provider's cadence ([`Cadences::status_of`]).
    pub const fn default_seconds(self) -> u64 {
        match self {
            Job::HoldingCheck | Job::PublishersRecords => 3600,
            _ => 0,
        }
    }
}

/// How often each job runs: an operator's setting with a default, and, for a status question, each provider's own.
#[derive(Debug, Clone, Default)]
pub struct Cadences {
    jobs: BTreeMap<Job, u64>,
    providers: BTreeMap<String, u64>,
}

impl Cadences {
    /// The defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `job` to run every `seconds` (the operator's `cadence` setting, `T-020-49` decides it).
    pub fn set(&mut self, job: Job, seconds: u64) {
        self.jobs.insert(job, seconds);
    }

    /// Sets how often a segment unknown at `provider` is asked its status again: the provider's own cadence, where it
    /// has one. No contract shape of a provider's declarations carries one yet, so the operator holds it for each
    /// provider (`F-020`'s notes, gap of `T-020-48`).
    pub fn set_status_of(&mut self, provider: &str, seconds: u64) {
        self.providers.insert(provider.to_owned(), seconds);
    }

    /// How often `job` runs.
    pub fn of(&self, job: Job) -> u64 {
        self.jobs
            .get(&job)
            .copied()
            .unwrap_or_else(|| job.default_seconds())
    }

    /// How often a segment unknown at `provider` is asked again: its own cadence, else the one the operator set for
    /// the job, else [`DEFAULT_STATUS_SECONDS`]. The job itself runs every tick, so that each segment is held to its
    /// own provider's cadence.
    pub fn status_of(&self, provider: &str) -> u64 {
        self.providers
            .get(provider)
            .or_else(|| self.jobs.get(&Job::StatusAsks))
            .copied()
            .unwrap_or(DEFAULT_STATUS_SECONDS)
    }
}

/// A segment whose outcome Core holds unknown, which a status question may be put about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown {
    /// The order.
    pub order: Id,
    /// The segment.
    pub segment: Id,
    /// The provider that was handed it.
    pub provider: String,
}

/// A late answer the platform held for a segment, taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateTaken {
    /// The order.
    pub order: Id,
    /// The segment.
    pub segment: Id,
    /// The receipt of the statement Core kept, or why it was not kept.
    pub kept: Result<Receipt, String>,
}

/// What the late-answers job did: the answers taken, and the segments whose own operation it carried out again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Late {
    /// The late answers taken.
    pub taken: Vec<LateTaken>,
    /// The unknown segments whose operation was carried out again (or refused by policy), which count as asked for their
    /// provider's cadence.
    pub repeated: Vec<Unknown>,
}

/// One holding account, read and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldingChecked {
    /// The node the account is at.
    pub node: String,
    /// What the account's module (public node) or its publisher (private node) stated, with what Core's segments rest
    /// there for each order (`INV-1`'s second half), or why it could not be read. It goes to the log's check and is not
    /// itself logged (`REQ-AUD-6`).
    pub reading: Result<HoldingReading, String>,
    /// The receipt of the check's record, where there was a reading and the log took the check, or why it didn't.
    pub check: Result<Receipt, String>,
    /// Where the module gave no balance: the receipt of the raise to the operator (`holding_not_read`), or why none
    /// was raised now (one was, earlier in this spell of failure). `None` where the account was read.
    pub not_read: Option<Result<Receipt, String>>,
    /// At a public node, the same balance beside the wallet host's own record of what each order holds there, a second
    /// comparison, where that record's total isn't what Core's segments rest at the node (`T-020-63`). Where the two
    /// totals are the same, the second comparison could only say what the first did, and none is made. `None` at a private
    /// node, and where the account was not read.
    pub against_wallet: Option<WalletCompared>,
}

/// A public node's balance read against the wallet host's own record (`T-020-63`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletCompared {
    /// The balance with what the wallet host's record says each order holds there. It goes to the log's check and is not
    /// itself logged (`REQ-AUD-6`).
    pub reading: HoldingReading,
    /// The receipt of the check's record, or why the log didn't take it.
    pub check: Result<Receipt, String>,
}

/// A publisher's signed record of the segments inside one of its private nodes, as Core read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    /// The order whose segments the record is about.
    pub order: Id,
    /// The private node.
    pub node: String,
    /// The record, held to its maker's advertisement and its signature.
    pub record: Statement<InnerSegments>,
}

/// A status question put about an unknown segment (`REQ-ORD-1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusTried {
    /// The segment.
    pub unknown: Unknown,
    /// The receipt of the question's effect, or why none was put.
    pub asked: Result<Receipt, String>,
    /// The receipt of the provider's answer as Core kept it, where it answered.
    pub kept: Option<Receipt>,
    /// The question as Core put it, where it did.
    pub status: Option<StatusAsked>,
}

/// An order Core closed, or tried to: its price reported, or why it wasn't (`REQ-QTE-7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Priced {
    /// The order.
    pub order: Id,
    /// The price as Core reported it, or why it didn't.
    pub reported: Result<PriceReported, Unreported>,
    /// Whether this round reported it.
    pub now: bool,
}

/// What timing an order's part found, where it was more than running: declared stuck now, or why it couldn't be timed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    /// The order.
    pub order: Id,
    /// What the watch found.
    pub found: Result<Watch, String>,
}

/// What Core's report of a stuck move's outcome to its mover came to (`AC-6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    /// The segment.
    pub unknown: Unknown,
    /// The receipt of the statement the report rests on.
    pub statement: Receipt,
    /// The version of the report, where it was taken: what a confirmation names ([`Moved::AwaitsConfirmation`](crate::stuck::Moved)); or why it wasn't.
    pub taken: Result<u64, String>,
}

/// What a tick did: each job that ran, and what it came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Round {
    /// The jobs that ran, in the order they ran.
    pub ran: Vec<Job>,
    /// What Core's order run did (`T-020-61`).
    pub runs: run::Runs,
    /// The late answers taken.
    pub late: Vec<LateTaken>,
    /// How many segments' value was landed in a holding account.
    pub landed: usize,
    /// How many measure records the log took.
    pub measures: usize,
    /// How many pairs of reports the comparison took, or why it stopped.
    pub pairs: Option<Result<usize, String>>,
    /// The orders closed: their executed price reported by this round.
    pub closed: Vec<Id>,
    /// Each order this round closed or failed to, with its price or why it has none.
    pub priced: Vec<Priced>,
    /// Each holding account read and checked.
    pub holding: Vec<HoldingChecked>,
    /// The records the publishers keep of their private nodes' segments.
    pub published: Vec<Published>,
    /// The status questions put again.
    pub statuses: Vec<StatusTried>,
    /// The parts the watch declared stuck, or couldn't time.
    pub watched: Vec<Watched>,
    /// The stuck moves a provider's answer was reported for.
    pub reported: Vec<Reported>,
    /// The orders whose wait for a customer ended.
    pub ended: Vec<Id>,
    /// The pairs a restart dropped, that no earlier round told the operator of.
    pub unbuilt_pairs: Vec<UnbuiltPair>,
    /// How many pairs this round raised to the operator and had the log record (each pair once).
    pub raised: usize,
    /// How many notices were sent, or why they stopped.
    pub notified: Option<Result<usize, String>>,
    /// How many checkpoints were published, or why they stopped.
    pub checkpoints: Option<Result<usize, String>>,
}

impl Round {
    /// Whether `job` ran in this round.
    pub fn did(&self, job: Job) -> bool {
        self.ran.contains(&job)
    }

    /// The reading of the holding account at `node`, if the round read one.
    pub fn holding_of(&self, node: &str) -> Option<&HoldingChecked> {
        self.holding.iter().find(|h| h.node == node)
    }

    /// The records published for the private node `node`.
    pub fn published_in(&self, node: &str) -> Vec<&Statement<InnerSegments>> {
        self.published
            .iter()
            .filter(|p| p.node == node)
            .map(|p| &p.record)
            .collect()
    }
}

/// What a tick calls: each job, its body [`jobs::Jobs`]'s over the deployment's [`jobs::Ports`]. The seam is a trait so
/// that the schedule's own rules (cadences, order, what a round says) are tested apart from the jobs.
pub trait Duties {
    /// Core's order run: the hand-ons done, what each started order runs on held, each part run, a segment begun
    /// before a restart carried on, and a failed part recalculated (`T-020-61`).
    fn run_orders(&mut self) -> run::Runs;
    /// Carries the own operation of each segment left unknown out again that `due` says is due, where its provider
    /// declares that safe and no status query, as a decided effect, and takes the late answers the platform holds for
    /// Core's segments, each kept once as a statement.
    fn late_answers(&mut self, due: &dyn Fn(&Unknown) -> bool) -> Late;
    /// Lands what settled at a public node a holding account holds; how many segments were landed.
    fn land_settled(&mut self) -> usize;
    /// Writes the measure records the segments' endings gave; how many the log took.
    fn write_measures(&mut self) -> usize;
    /// Compares the pairs of adjacent reports the segments' endings gave; how many the log took.
    ///
    /// # Errors
    ///
    /// Why the comparison stopped.
    fn write_pairs(&mut self) -> Result<usize, String>;
    /// Closes the orders that completed and reports each one's executed price, once.
    fn close_completed(&mut self) -> Vec<Priced>;
    /// Reads each holding account through the platform and checks it: only the accounts at `only`, where it names them.
    fn check_holdings(&mut self, only: Option<&BTreeSet<String>>) -> Vec<HoldingChecked>;
    /// Reads the publishers' records of their private nodes.
    fn read_publishers(&mut self) -> Vec<Published>;
    /// The segments whose outcome Core holds unknown.
    fn unknown_segments(&self) -> Vec<Unknown>;
    /// Puts a status question about `unknown` again. Nothing is moved by it.
    fn ask_status_again(&mut self, unknown: &Unknown) -> StatusTried;
    /// Times each followed order's part; the parts declared stuck now, or that couldn't be timed.
    fn watch(&mut self) -> Vec<Watched>;
    /// Hands what Core kept of a provider's answer about a stuck move to its mover's report.
    fn report_kept_answers(&mut self) -> Vec<Reported>;
    /// Ends the waits for a customer past their lifetime; the orders ended.
    fn end_expired(&mut self) -> Vec<Id>;
    /// The pairs a restart dropped or that couldn't be built, with why.
    fn unbuilt_pairs(&self) -> Vec<UnbuiltPair>;
    /// Raises each such pair to the operator and has the log record it, once; how many it raised now.
    fn raise_unbuilt_pairs(&mut self) -> usize;
    /// Sends the notices the log's changes call for; how many.
    ///
    /// # Errors
    ///
    /// Why the notices stopped.
    fn notify(&mut self) -> Result<usize, String>;
    /// Issues and publishes the checkpoints due; how many were published.
    ///
    /// # Errors
    ///
    /// Why the checkpointer stopped.
    fn checkpoints(&mut self) -> Result<usize, String>;
}

/// Core's schedule: the cadences and what ran when.
#[derive(Debug, Clone, Default)]
pub struct Schedule {
    cadences: Cadences,
    /// When each job last ran, in whole seconds of Core's clock.
    last: BTreeMap<Job, u64>,
    /// When each unknown segment was last asked its status by Core, in whole seconds, by order and segment.
    asked: BTreeMap<(String, String), u64>,
    /// The pairs already told to the operator, by order and later segment.
    told: BTreeSet<(String, String)>,
    /// The holding accounts the last check couldn't read or check, by node, and when each was last tried: each is read
    /// again at [`HOLDING_RETRY_SECONDS`], alone.
    unread: BTreeMap<String, u64>,
}

impl Schedule {
    /// A schedule with the default cadences: every job due on its first tick.
    pub fn new() -> Self {
        Self::default()
    }

    /// A schedule rebuilt from the log a Core reopens on (`NFR-6`, `T-020-63`): when each segment was last asked its
    /// status, from the log's `ask_status` effects, so the first tick after a restart keeps each provider's cadence and
    /// presses none. The time of an ask is its effect's entry's, which Core's clock gave it, in whole seconds. Every job's
    /// own last run is not recorded anywhere, so each runs on the first tick, as before (the jobs are fail-closed: one done
    /// twice is done once). The cadences themselves are the operator's settings, rebuilt by `Settings::reopen`
    /// (`T-020-49`).
    pub fn recorded(history: &[(Receipt, Entry)]) -> Self {
        let mut schedule = Self::default();
        for (_, entry) in history {
            let Entry::Verified(verified) = entry else {
                continue;
            };
            // A status question put, or a segment's operation carried out again, is the provider asked: both count from
            // their effect's time, so a reopened Core runs neither again before the provider's cadence (`T-020-63`).
            let What::Effect {
                action:
                    Action::AskStatus { order, segment, .. }
                    | Action::RepeatOperation { order, segment, .. },
                ..
            } = &verified.what
            else {
                continue;
            };
            let at = u64::try_from(verified.when.at.unix_nanos() / 1_000_000_000).unwrap_or(0);
            let key = (order.as_str().to_owned(), segment.as_str().to_owned());
            let last = schedule.asked.entry(key).or_insert(at);
            *last = (*last).max(at);
        }
        schedule
    }

    /// The cadences, to read.
    pub fn cadences(&self) -> &Cadences {
        &self.cadences
    }

    /// The cadences, to set (the operator's setting; `T-020-49` decides it).
    pub fn cadences_mut(&mut self) -> &mut Cadences {
        &mut self.cadences
    }

    /// Whether `job` is due at `now`: it never ran, or its cadence has passed since it did. A clock that went back
    /// makes it due (it fails closed: a job is run more, never less).
    pub fn due(&self, job: Job, now: u64) -> bool {
        self.last
            .get(&job)
            .is_none_or(|last| now < *last || now - *last >= self.cadences.of(job))
    }

    /// Whether the unknown segment is due its status question at `now`: Core never asked it, or its provider's cadence
    /// has passed since it did.
    pub fn status_due(&self, unknown: &Unknown, now: u64) -> bool {
        let at = (
            unknown.order.as_str().to_owned(),
            unknown.segment.as_str().to_owned(),
        );
        self.asked.get(&at).is_none_or(|last| {
            now < *last || now - *last >= self.cadences.status_of(&unknown.provider)
        })
    }

    /// That Core put a status question about `unknown` at `now`: by this schedule, or by the step that left it
    /// unknown (`T-020-47`), which the assembly tells through [`Schedule::note_asked`].
    pub fn note_asked(&mut self, unknown: &Unknown, now: u64) {
        self.asked.insert(
            (
                unknown.order.as_str().to_owned(),
                unknown.segment.as_str().to_owned(),
            ),
            now,
        );
    }

    /// Runs what is due at `now`, in whole seconds of Core's clock, over `duties`, and answers what ran and what it
    /// came to. A job not yet due is left for a later tick; one due runs once, whatever happened between ticks.
    pub fn tick(&mut self, now: u64, duties: &mut dyn Duties) -> Round {
        let mut round = Round::default();
        for job in Job::ALL {
            // The holding check not due may still have accounts to read again, alone.
            let again: Option<BTreeSet<String>> = if self.due(job, now) {
                None
            } else if job == Job::HoldingCheck {
                let due: BTreeSet<String> = self
                    .unread
                    .iter()
                    .filter(|(_, at)| now < **at || now - **at >= HOLDING_RETRY_SECONDS)
                    .map(|(node, _)| node.clone())
                    .collect();
                if due.is_empty() {
                    continue;
                }
                Some(due)
            } else {
                continue;
            };
            round.ran.push(job);
            // A job counts as run, and waits its cadence, only once it succeeded: one that failed (a holding account not
            // read or not checked, a comparison, a notice or a checkpoint the log or the store didn't take) is due again
            // on the next tick, never held off for a whole cadence (`T-020-63`).
            let succeeded = match job {
                Job::Run => {
                    round.runs = duties.run_orders();
                    // A status question the run put after a part counts as its provider asked, for its cadence.
                    for tried in round.runs.parts.iter().flat_map(|p| p.asked.iter()) {
                        if tried.asked.is_ok() {
                            self.note_asked(&tried.unknown, now);
                        }
                    }
                    true
                }
                Job::LateAnswers => {
                    let late = duties.late_answers(&|unknown| self.status_due(unknown, now));
                    // A segment's operation carried out again counts as its provider asked, for its cadence.
                    for unknown in &late.repeated {
                        self.note_asked(unknown, now);
                    }
                    round.late = late.taken;
                    round.late.iter().all(|late| late.kept.is_ok())
                }
                Job::LandSettled => {
                    round.landed = duties.land_settled();
                    true
                }
                Job::WriteMeasures => {
                    round.measures = duties.write_measures();
                    true
                }
                Job::WritePairs => {
                    let pairs = duties.write_pairs();
                    let succeeded = pairs.is_ok();
                    round.pairs = Some(pairs);
                    succeeded
                }
                Job::Close => {
                    round.priced = duties.close_completed();
                    round.closed = round
                        .priced
                        .iter()
                        .filter(|p| p.now)
                        .map(|p| p.order.clone())
                        .collect();
                    true
                }
                Job::HoldingCheck => {
                    round.holding = duties.check_holdings(again.as_ref());
                    // An account that was read and checked waits its cadence; one that wasn't is read again alone, at its
                    // own, never making the others wait again nor pressing its module every tick (`T-020-63`).
                    if let Some(nodes) = &again {
                        self.unread.retain(|node, _| {
                            !nodes.contains(node) || round.holding.iter().any(|h| &h.node == node)
                        });
                    }
                    for held in &round.holding {
                        let failed = held.reading.is_err()
                            || held.check.is_err()
                            || held
                                .against_wallet
                                .as_ref()
                                .is_some_and(|w| w.check.is_err());
                        if failed {
                            self.unread.insert(held.node.clone(), now);
                        } else {
                            self.unread.remove(&held.node);
                        }
                    }
                    true
                }
                Job::PublishersRecords => {
                    round.published = duties.read_publishers();
                    true
                }
                Job::StatusAsks => {
                    let mut succeeded = true;
                    for unknown in duties.unknown_segments() {
                        if !self.status_due(&unknown, now) {
                            continue;
                        }
                        let tried = duties.ask_status_again(&unknown);
                        // The segment's own cadence counts from an ask that went out; one that didn't is tried again
                        // on the next tick.
                        if tried.asked.is_ok() {
                            self.note_asked(&unknown, now);
                        } else {
                            succeeded = false;
                        }
                        round.statuses.push(tried);
                    }
                    succeeded
                }
                Job::Watch => {
                    round.watched = duties.watch();
                    true
                }
                Job::StuckReports => {
                    round.reported = duties.report_kept_answers();
                    true
                }
                Job::EndExpired => {
                    round.ended = duties.end_expired();
                    true
                }
                Job::UnbuiltPairs => {
                    round.raised = duties.raise_unbuilt_pairs();
                    for pair in duties.unbuilt_pairs() {
                        let at = (
                            pair.order.as_str().to_owned(),
                            pair.segment.as_str().to_owned(),
                        );
                        if self.told.insert(at) {
                            round.unbuilt_pairs.push(pair);
                        }
                    }
                    true
                }
                Job::Notify => {
                    let notified = duties.notify();
                    let succeeded = notified.is_ok();
                    round.notified = Some(notified);
                    succeeded
                }
                Job::Checkpoints => {
                    let checkpoints = duties.checkpoints();
                    let succeeded = checkpoints.is_ok();
                    round.checkpoints = Some(checkpoints);
                    succeeded
                }
            };
            if succeeded && again.is_none() {
                self.last.insert(job, now);
            }
        }
        round
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Counting {
        landed: usize,
        asked: Vec<String>,
        unknown: Vec<Unknown>,
        /// Whether a status question put again goes out (a receipt) or doesn't (a stand-in's refusal).
        asks_go_out: bool,
        /// The accounts the holding check was asked about, in order, and the ones it can't read.
        checked: Vec<String>,
        unread: Vec<&'static str>,
    }

    fn reading() -> HoldingReading {
        serde_json::from_value(serde_json::json!({
            "account": "holding-a", "provider": "p", "node": "a",
            "asset": { "id": "USDC", "decimals": 2 }, "balance": "0", "word": "checked",
            "held_for": [], "at": "2026-11-10T09:00:00Z",
        }))
        .expect("a reading")
    }

    fn receipt() -> Receipt {
        serde_json::from_value(serde_json::json!({
            "log": "log-1",
            "index": 1,
            "entry": { "algorithm": "sha-256", "value": "ab".repeat(32) },
        }))
        .expect("a receipt")
    }

    impl Duties for Counting {
        fn run_orders(&mut self) -> run::Runs {
            run::Runs::default()
        }
        fn late_answers(&mut self, _: &dyn Fn(&Unknown) -> bool) -> Late {
            Late::default()
        }
        fn land_settled(&mut self) -> usize {
            self.landed += 1;
            0
        }
        fn write_measures(&mut self) -> usize {
            0
        }
        fn write_pairs(&mut self) -> Result<usize, String> {
            Ok(0)
        }
        fn close_completed(&mut self) -> Vec<Priced> {
            Vec::new()
        }
        fn check_holdings(&mut self, only: Option<&BTreeSet<String>>) -> Vec<HoldingChecked> {
            ["a", "b"]
                .into_iter()
                .filter(|node| only.is_none_or(|nodes| nodes.contains(*node)))
                .map(|node| {
                    self.checked.push(node.to_owned());
                    let failed = self.unread.contains(&node);
                    HoldingChecked {
                        node: node.to_owned(),
                        reading: if failed {
                            Err("the module gave no balance".to_owned())
                        } else {
                            Ok(reading())
                        },
                        check: if failed {
                            Err("the module gave no balance".to_owned())
                        } else {
                            Ok(receipt())
                        },
                        not_read: None,
                        against_wallet: None,
                    }
                })
                .collect()
        }
        fn read_publishers(&mut self) -> Vec<Published> {
            Vec::new()
        }
        fn unknown_segments(&self) -> Vec<Unknown> {
            self.unknown.clone()
        }
        fn ask_status_again(&mut self, unknown: &Unknown) -> StatusTried {
            self.asked.push(unknown.segment.as_str().to_owned());
            StatusTried {
                unknown: unknown.clone(),
                asked: if self.asks_go_out {
                    Ok(receipt())
                } else {
                    Err("a stand-in".to_owned())
                },
                kept: None,
                status: None,
            }
        }
        fn watch(&mut self) -> Vec<Watched> {
            Vec::new()
        }
        fn report_kept_answers(&mut self) -> Vec<Reported> {
            Vec::new()
        }
        fn end_expired(&mut self) -> Vec<Id> {
            Vec::new()
        }
        fn unbuilt_pairs(&self) -> Vec<UnbuiltPair> {
            Vec::new()
        }
        fn raise_unbuilt_pairs(&mut self) -> usize {
            0
        }
        fn notify(&mut self) -> Result<usize, String> {
            Ok(0)
        }
        fn checkpoints(&mut self) -> Result<usize, String> {
            Ok(0)
        }
    }

    fn unknown(provider: &str) -> Unknown {
        Unknown {
            order: Id::new("order").unwrap(),
            segment: Id::new("segment").unwrap(),
            provider: provider.to_owned(),
        }
    }

    // verifies: REQ-ORD-1
    #[test]
    fn a_job_runs_when_its_cadence_has_passed_and_not_before() {
        let mut schedule = Schedule::new();
        schedule.cadences_mut().set(Job::LandSettled, 60);
        let mut duties = Counting::default();
        assert!(schedule.tick(1_000, &mut duties).did(Job::LandSettled));
        assert!(!schedule.tick(1_059, &mut duties).did(Job::LandSettled));
        assert!(schedule.tick(1_060, &mut duties).did(Job::LandSettled));
        assert_eq!(duties.landed, 2);
        // A clock that went back runs it again: more, never less.
        assert!(schedule.tick(10, &mut duties).did(Job::LandSettled));
    }

    // verifies: REQ-ORD-1
    #[test]
    fn an_unknown_segment_is_asked_at_its_providers_cadence_not_the_jobs() {
        let mut schedule = Schedule::new();
        schedule.cadences_mut().set(Job::StatusAsks, 0);
        schedule.cadences_mut().set_status_of("slow", 600);
        let mut duties = Counting {
            unknown: vec![unknown("slow")],
            asks_go_out: true,
            ..Counting::default()
        };
        schedule.note_asked(&unknown("slow"), 1_000);
        schedule.tick(1_100, &mut duties);
        assert!(
            duties.asked.is_empty(),
            "its provider's cadence hasn't passed"
        );
        schedule.tick(1_600, &mut duties);
        assert_eq!(duties.asked, ["segment"]);
        schedule.tick(1_700, &mut duties);
        assert_eq!(duties.asked.len(), 1, "and not again before it passes");
    }

    /// One account the holding check can't read doesn't make the check run, and every other account be read again, on every
    /// tick: it is read again alone, at its own cadence, and the accounts that were read wait theirs (`NFR-6`, `REQ-AUD-15`,
    /// `T-020-63`; review M1).
    // verifies: NFR-6, REQ-AUD-15
    #[test]
    fn an_unreadable_account_is_read_again_alone_at_its_own_cadence() {
        let mut schedule = Schedule::new();
        let mut duties = Counting {
            unread: vec!["a"],
            ..Counting::default()
        };
        let asked = |duties: &Counting| duties.checked.join("");
        assert!(schedule.tick(1_000, &mut duties).did(Job::HoldingCheck));
        assert_eq!(asked(&duties), "ab", "every account, the first time");
        // Ticks every ten seconds, well inside the retry's cadence and the hour: nothing is asked of anyone.
        for at in (1_010..1_300).step_by(10) {
            assert!(!schedule.tick(at, &mut duties).did(Job::HoldingCheck));
        }
        assert_eq!(asked(&duties), "ab");
        // The unreadable account alone, once its own cadence has passed, and again after the next.
        assert!(schedule.tick(1_300, &mut duties).did(Job::HoldingCheck));
        assert_eq!(asked(&duties), "aba");
        assert!(!schedule.tick(1_301, &mut duties).did(Job::HoldingCheck));
        assert!(schedule.tick(1_600, &mut duties).did(Job::HoldingCheck));
        assert_eq!(asked(&duties), "abaa");
        // Read at last: it waits no more, and the hour comes round for both.
        duties.unread.clear();
        assert!(schedule.tick(1_900, &mut duties).did(Job::HoldingCheck));
        assert!(!schedule.tick(2_200, &mut duties).did(Job::HoldingCheck));
        assert_eq!(asked(&duties), "abaaa");
        assert!(
            schedule
                .tick(1_000 + 3_600, &mut duties)
                .did(Job::HoldingCheck)
        );
        assert_eq!(asked(&duties), "abaaaab");
    }

    // verifies: NFR-6, REQ-ORD-1
    #[test]
    fn a_status_question_that_did_not_go_out_is_asked_again_on_the_next_tick() {
        let mut schedule = Schedule::new();
        let mut duties = Counting {
            unknown: vec![unknown("slow")],
            ..Counting::default()
        };
        // The question wasn't put: nothing counts as asked, so the next tick, well inside the provider's hour, asks again.
        schedule.tick(1_000, &mut duties);
        schedule.tick(1_001, &mut duties);
        assert_eq!(duties.asked.len(), 2);
        // Put at last: the provider's cadence counts from it.
        duties.asks_go_out = true;
        schedule.tick(1_002, &mut duties);
        schedule.tick(1_003, &mut duties);
        assert_eq!(duties.asked.len(), 3);
    }

    /// The schedule rebuilt from a log holds each segment's last status question from the log's `ask_status` effect, and the
    /// latest of two (`NFR-6`, `T-020-63`).
    // verifies: NFR-6
    #[test]
    fn schedule_rebuilt_from_the_log_keeps_each_segments_last_status_question() {
        let effect_of = |kind: &str, order: &str, segment: &str, at: &str| -> (Receipt, Entry) {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                "../../contracts/core-internal/fixtures/valid/verified-entry-of-an-effect.json",
            );
            let mut entry: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).expect("the fixture")).expect("JSON");
            entry["what"]["action"] = serde_json::json!({
                "kind": kind, "order": order, "segment": segment,
                "provider": "slow", "transaction": format!("tx-{order}-{segment}-{at}"),
            });
            entry["when"]["at"] = at.into();
            (receipt(), serde_json::from_value(entry).expect("an entry"))
        };
        let effect =
            |order: &str, segment: &str, at: &str| effect_of("ask_status", order, segment, at);
        let history = vec![
            effect("order", "segment", "1970-01-01T00:16:40Z"),
            effect("order", "segment", "1970-01-01T00:20:00Z"),
            effect("order", "other", "1970-01-01T00:10:00Z"),
            // A segment's operation carried out again is the provider asked as well: at 1 500 s, the later of the two.
            effect_of(
                "repeat_operation",
                "order",
                "repeated",
                "1970-01-01T00:25:00Z",
            ),
        ];
        let mut schedule = Schedule::recorded(&history);
        schedule.cadences_mut().set_status_of("slow", 600);
        let due = |segment: &str, at: u64| {
            let mut one = unknown("slow");
            one.segment = Id::new(segment).unwrap();
            schedule.status_due(&one, at)
        };
        // The segment was last asked at 1 200 s (the later of two): not due before 1 800, and due at it.
        assert!(!due("segment", 1_799) && due("segment", 1_800));
        // Another, asked at 600 s, is due at 1 200; a segment never asked is due at once.
        assert!(!due("other", 1_199) && due("other", 1_200));
        assert!(due("never", 0));
        // A repeat counts from its effect: not due before 2 100 s, and due at it, as a status question is.
        assert!(!due("repeated", 2_099) && due("repeated", 2_100));
    }
}
