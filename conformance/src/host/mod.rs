//! The host-contract suite (`T-005-03`, `DEC-5`): the shared conformance suite every host candidate is scored by,
//! the `EXP-1` candidates included (`DEC-6`). It drives a candidate through the host contract's test harness
//! ([`Harness`]), and through [`FaultSeam`] when the candidate runs in-process, and checks each guarantee of
//! [`GUARANTEES`] cell by cell.
//!
//! Candidates run the suite's own state machine, [`probe::Probe`], so a host is scored as `Harness<Probe>`: a real
//! host is generic over its state machine and is instantiated with the probe here. Expected states always come
//! from folding the probe over a journal, never from the host's own reads, and every cell checks the host's
//! `state()` and journals against that fold.
//!
//! A run reports per cell, per fault and per guarantee ([`Report`]), and renders as Markdown tables an `EXP-1`
//! scoring sheet can paste ([`Report::to_markdown`]). Every row carries the run id, the candidate, the swap target,
//! both contract versions and the seed, and a swap cell's row names its direction. The caller names the run; the
//! suite reads no clock.
//!
//! # Not covered
//!
//! - **Power loss before an fsync**, and **torn writes**. A process kill keeps the operating system's page cache,
//!   and no crash point falls inside a write, so neither can be produced here. `EXP-1` checks them by inspecting
//!   each candidate's durability code.
//! - **Kills inside a submit, for a black-box candidate.** Driven through [`Harness`] alone, a candidate is killed
//!   only between submits; the named stops need [`FaultSeam`], and a black-box run reports them not run.
//! - **Concurrent submits**: [`Host::submit`] takes `&mut self`.
//! - **The effect driver's obligation** and `REQ-CAT-10`, which the host contract states apart from its guarantees.

pub mod probe;

use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use rutent_contracts::state_machine::host::testing::{
    CrashPoint, FaultSeam, Harness, HarnessError,
};
use rutent_contracts::state_machine::host::{
    self as contract, Delivery, GUARANTEES, Host, HostError, RefusalRecord, SnapshotEntry,
    TransitionRecord, Version,
};
use rutent_contracts::state_machine::{self as machine, Id, OrderState, RecordedInput};

use probe::Probe;

/// A fault, or a kind of fault, the suite injects or exercises. The first five are the faults `PLAN.md` names for
/// `T-005-03`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fault {
    /// A crash between a record and its effect: a stop after received, after computed or after durable, or a
    /// kill after records were released.
    CrashBetweenRecordAndEffect,
    /// The same input delivered again.
    RepeatedDelivery,
    /// Interleaved orders' inputs delivered out of their recorded order.
    ReorderedDelivery,
    /// The execution environment replaced mid-order.
    SwapMidOrder,
    /// A recorded run replayed through the state machine alone.
    ReplayOfARecordedRun,
    /// The host killed and its store reopened.
    KillAndReopen,
    /// A different input under a record id already used.
    ReusedRecordId,
    /// The host halted.
    Halt,
    /// The new host stopped part-way through an import.
    StopDuringImport,
    /// An import given a stream it must refuse.
    UnacceptableStream,
}

impl Fault {
    /// Every fault, in report order.
    pub const ALL: [Fault; 10] = [
        Fault::CrashBetweenRecordAndEffect,
        Fault::RepeatedDelivery,
        Fault::ReorderedDelivery,
        Fault::SwapMidOrder,
        Fault::ReplayOfARecordedRun,
        Fault::KillAndReopen,
        Fault::ReusedRecordId,
        Fault::Halt,
        Fault::StopDuringImport,
        Fault::UnacceptableStream,
    ];

    /// The fault's name in a report; the first five are `PLAN.md`'s words.
    pub fn name(self) -> &'static str {
        match self {
            Fault::CrashBetweenRecordAndEffect => "crash between record and effect",
            Fault::RepeatedDelivery => "repeated delivery",
            Fault::ReorderedDelivery => "reordered delivery",
            Fault::SwapMidOrder => "swap mid-order",
            Fault::ReplayOfARecordedRun => "replay of a recorded run",
            Fault::KillAndReopen => "kill and reopen",
            Fault::ReusedRecordId => "reused record id",
            Fault::Halt => "halt",
            Fault::StopDuringImport => "stop during import",
            Fault::UnacceptableStream => "unacceptable stream",
        }
    }
}

/// One cell of the suite: a check of one or more guarantees under one fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// A stable kebab-case id, the same on every run and candidate.
    pub id: &'static str,
    /// The host-contract guarantees it checks, by their ids in [`GUARANTEES`].
    pub guarantees: &'static [&'static str],
    /// The fault it injects.
    pub fault: Fault,
    /// Whether it needs [`FaultSeam`]; a black-box run reports it not run.
    pub needs_fault_seam: bool,
    /// Whether it moves a host from the candidate to the swap target; its report row names the direction.
    pub uses_target: bool,
}

const SEAM: bool = true;
const TARGET: bool = true;

const fn cell(
    id: &'static str,
    guarantees: &'static [&'static str],
    fault: Fault,
    needs_fault_seam: bool,
    uses_target: bool,
) -> Cell {
    Cell {
        id,
        guarantees,
        fault,
        needs_fault_seam,
        uses_target,
    }
}

/// The stop cells: each also repeats the stopped input after the reopen.
const BOTH: &[&str] = &["durable", "no-effect-without-record", "exactly-once"];

/// Every cell of the suite, in report order.
pub const CELLS: [Cell; 16] = [
    cell(
        "acknowledged-records-survive-a-kill",
        &["durable"],
        Fault::KillAndReopen,
        !SEAM,
        !TARGET,
    ),
    cell(
        "halt-survives-a-kill",
        &["durable"],
        Fault::KillAndReopen,
        !SEAM,
        !TARGET,
    ),
    cell(
        "stop-after-received",
        BOTH,
        Fault::CrashBetweenRecordAndEffect,
        SEAM,
        !TARGET,
    ),
    cell(
        "stop-after-computed",
        BOTH,
        Fault::CrashBetweenRecordAndEffect,
        SEAM,
        !TARGET,
    ),
    cell(
        "stop-after-durable",
        BOTH,
        Fault::CrashBetweenRecordAndEffect,
        SEAM,
        !TARGET,
    ),
    cell(
        "released-records-survive-a-kill",
        &["no-effect-without-record"],
        Fault::CrashBetweenRecordAndEffect,
        !SEAM,
        !TARGET,
    ),
    cell(
        "repeat-returns-the-recorded-record",
        &["exactly-once"],
        Fault::RepeatedDelivery,
        !SEAM,
        !TARGET,
    ),
    cell(
        "repeat-after-a-kill",
        &["exactly-once"],
        Fault::RepeatedDelivery,
        !SEAM,
        !TARGET,
    ),
    cell(
        "reused-record-id-is-refused-and-logged",
        &["exactly-once"],
        Fault::ReusedRecordId,
        !SEAM,
        !TARGET,
    ),
    cell(
        "reordered-delivery-is-applied-in-submit-order",
        &["per-order-sequence"],
        Fault::ReorderedDelivery,
        !SEAM,
        !TARGET,
    ),
    cell(
        "swap-mid-order-loses-no-order",
        &["swap"],
        Fault::SwapMidOrder,
        !SEAM,
        TARGET,
    ),
    cell(
        "repeat-after-a-swap-is-recognised",
        &["swap", "exactly-once"],
        Fault::SwapMidOrder,
        !SEAM,
        TARGET,
    ),
    cell(
        "halted-host-refuses-and-only-a-halted-host-exports",
        &["swap"],
        Fault::Halt,
        !SEAM,
        !TARGET,
    ),
    cell(
        "stop-during-import-is-refused-at-open",
        &["swap"],
        Fault::StopDuringImport,
        !SEAM,
        TARGET,
    ),
    cell(
        "import-refuses-a-stream-it-cannot-take",
        &["swap"],
        Fault::UnacceptableStream,
        !SEAM,
        TARGET,
    ),
    cell(
        "replay-reaches-the-same-states",
        &["replay"],
        Fault::ReplayOfARecordedRun,
        !SEAM,
        !TARGET,
    ),
];

/// What one cell found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellOutcome {
    /// The candidate met every check of the cell.
    Pass,
    /// It didn't; the detail says which check and why.
    Fail(String),
    /// The cell couldn't run on this candidate; the detail says why. Never counts as a pass.
    NotRun(String),
}

/// A verdict over several cells: fail if any failed, else incomplete if any didn't run, else pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Every cell passed.
    Pass,
    /// At least one cell failed.
    Fail,
    /// None failed, but at least one didn't run.
    Incomplete,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::Incomplete => "incomplete",
        })
    }
}

/// How a run is named and seeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    /// The run's id, given by the caller, for example `2026-11-06-reference-1`.
    pub run_id: String,
    /// The candidate's name.
    pub candidate: String,
    /// The swap target's name: the implementation swap cells import into.
    pub target: String,
    /// The seed of the reordered delivery.
    pub seed: u64,
}

/// The result of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// How the run was named and seeded.
    pub config: RunConfig,
    /// The host contract version the suite checks.
    pub host_version: &'static str,
    /// The state-machine contract version the suite checks.
    pub state_machine_version: &'static str,
    /// Every cell of [`CELLS`] with its outcome, in order.
    pub cells: Vec<(Cell, CellOutcome)>,
}

fn verdict<'a>(outcomes: impl Iterator<Item = &'a CellOutcome>) -> Verdict {
    let mut result = Verdict::Pass;
    for outcome in outcomes {
        match outcome {
            CellOutcome::Fail(_) => return Verdict::Fail,
            CellOutcome::NotRun(_) => result = Verdict::Incomplete,
            CellOutcome::Pass => {}
        }
    }
    result
}

impl Report {
    /// The outcome of one cell.
    pub fn outcome(&self, id: &str) -> Option<&CellOutcome> {
        self.cells.iter().find(|(c, _)| c.id == id).map(|(_, o)| o)
    }

    /// The verdict on one guarantee, over every cell that checks it.
    pub fn guarantee(&self, id: &str) -> Verdict {
        verdict(
            self.cells
                .iter()
                .filter(|(c, _)| c.guarantees.contains(&id))
                .map(|(_, o)| o),
        )
    }

    /// The verdict on one fault, over every cell that injects it.
    pub fn fault(&self, fault: Fault) -> Verdict {
        verdict(
            self.cells
                .iter()
                .filter(|(c, _)| c.fault == fault)
                .map(|(_, o)| o),
        )
    }

    /// The verdict on the whole run.
    pub fn verdict(&self) -> Verdict {
        verdict(self.cells.iter().map(|(_, o)| o))
    }

    /// The report as two Markdown tables, per fault and per guarantee, each row carrying the run's provenance.
    pub fn to_markdown(&self) -> String {
        let c = &self.config;
        let provenance = format!(
            "{} | {} | {} | {} | {} | {}",
            c.run_id, c.candidate, c.target, self.host_version, self.state_machine_version, c.seed
        );
        let header = "| Run | Candidate | Target | Host | State machine | Seed";
        let mut out = format!(
            "## Host-contract suite, run {}\n\nCandidate `{}`, swap target `{}`; host contract {}; state machine {}; \
             seed {}. Verdict: {}.\n\n### Per fault\n\n{header} | Fault | Cell | Guarantees | Direction | Result | \
             Detail |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n",
            c.run_id,
            c.candidate,
            c.target,
            self.host_version,
            self.state_machine_version,
            c.seed,
            self.verdict()
        );
        for fault in Fault::ALL {
            for (cell, outcome) in self.cells.iter().filter(|(cell, _)| cell.fault == fault) {
                let (result, detail) = match outcome {
                    CellOutcome::Pass => ("pass", String::new()),
                    CellOutcome::Fail(d) => ("fail", d.replace('|', "\\|")),
                    CellOutcome::NotRun(d) => ("not run", d.replace('|', "\\|")),
                };
                let direction = if cell.uses_target {
                    format!("{} to {}", c.candidate, c.target)
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "| {provenance} | {} | `{}` | {} | {direction} | {result} | {detail} |\n",
                    fault.name(),
                    cell.id,
                    cell.guarantees.join(", ")
                ));
            }
        }
        out.push_str(&format!(
            "\n### Per guarantee\n\n{header} | Guarantee | Cells | Result |\n|---|---|---|---|---|---|---|---|---|\n"
        ));
        for g in GUARANTEES {
            let count = self
                .cells
                .iter()
                .filter(|(cell, _)| cell.guarantees.contains(&g.id))
                .count();
            out.push_str(&format!(
                "| {provenance} | `{}` | {count} | {} |\n",
                g.id,
                self.guarantee(g.id)
            ));
        }
        out
    }
}

type Checked<T> = Result<T, String>;

fn host_err<T>(result: Result<T, HostError>, what: &str) -> Checked<T> {
    result.map_err(|e| format!("{what}: the host returned {e:?}"))
}

fn harness_err<T>(result: Result<T, HarnessError>, what: &str) -> Checked<T> {
    result.map_err(|e| format!("{what}: {e}"))
}

fn started<H: Harness<Probe>>(harness: &mut H) -> Checked<H::Host> {
    let store = harness_err(harness.fresh_store(), "fresh store")?;
    harness_err(harness.start(store), "start")
}

fn reopened<H: Harness<Probe>>(harness: &mut H, host: H::Host) -> Checked<H::Host> {
    let store = harness_err(harness.kill(host), "kill")?;
    harness_err(harness.reopen(store), "reopen after a kill")
}

fn record<H: Host<Probe>>(host: &mut H, input: &RecordedInput) -> Checked<TransitionRecord> {
    match host.submit(input.clone()) {
        Ok(Delivery::Recorded(record)) => Ok(record),
        Ok(Delivery::AlreadyRecorded(_)) => Err(format!(
            "{} is new but came back as already recorded",
            input.record_id.as_str()
        )),
        Err(e) => Err(format!(
            "submitting {}: the host returned {e:?}",
            input.record_id.as_str()
        )),
    }
}

fn record_all<H: Host<Probe>>(
    host: &mut H,
    inputs: &[RecordedInput],
) -> Checked<Vec<TransitionRecord>> {
    inputs.iter().map(|input| record(host, input)).collect()
}

type Journals = BTreeMap<String, Vec<TransitionRecord>>;

fn journals<H: Host<Probe>>(host: &H) -> Checked<Journals> {
    let mut all = Journals::new();
    for order in host_err(host.orders(), "orders")? {
        let journal = host_err(host.journal(&order), "journal")?;
        all.insert(order.as_str().to_owned(), journal);
    }
    Ok(all)
}

/// Every journal's states, and every order's `state()`, are what folding the probe over the journal reaches.
fn states_follow_the_probe<H: Host<Probe>>(host: &H, after: &str) -> Checked<()> {
    for order in host_err(host.orders(), "orders")? {
        let journal = host_err(host.journal(&order), "journal")?;
        let expected = probe::fold(journal.iter().map(|r| &r.input));
        for (r, state) in journal.iter().zip(&expected) {
            if &r.state_after != state {
                return Err(format!(
                    "seq {} of {}: its recorded state differs from the probe's {after}",
                    r.seq,
                    order.as_str()
                ));
            }
        }
        let latest = expected.into_iter().rev().find_map(|s| s);
        if host_err(host.state(&order), "state")? != latest {
            return Err(format!(
                "{}'s state() differs from the probe's fold of its journal {after}",
                order.as_str()
            ));
        }
    }
    Ok(())
}

fn grouped(records: &[TransitionRecord]) -> Journals {
    let mut all = Journals::new();
    for r in records {
        all.entry(r.order_id.as_str().to_owned())
            .or_default()
            .push(r.clone());
    }
    all
}

/// Every record of `expected` is in `found`, whole; the first that isn't is named.
fn contains_all(found: &Journals, expected: &[TransitionRecord], after: &str) -> Checked<()> {
    for r in expected {
        let present = found
            .get(r.order_id.as_str())
            .and_then(|j| j.get(r.seq as usize - 1))
            .is_some_and(|f| f == r);
        if !present {
            return Err(format!(
                "{} (seq {} of {}) is gone or changed {after}",
                r.input.record_id.as_str(),
                r.seq,
                r.order_id.as_str()
            ));
        }
    }
    Ok(())
}

fn same_journals(found: &Journals, expected: &Journals, after: &str) -> Checked<()> {
    if found == expected {
        return Ok(());
    }
    for (order, journal) in expected {
        match found.get(order) {
            None => return Err(format!("order {order} is missing {after}")),
            Some(j) if j.len() != journal.len() => {
                return Err(format!(
                    "order {order} has {} records {after}, not {}",
                    j.len(),
                    journal.len()
                ));
            }
            Some(j) if j != journal => {
                return Err(format!("order {order}'s records changed {after}"));
            }
            _ => {}
        }
    }
    let extra: Vec<&String> = found
        .keys()
        .filter(|k| !expected.contains_key(*k))
        .collect();
    Err(format!("orders {extra:?} appeared {after}"))
}

fn interleaved(orders: &[Vec<RecordedInput>]) -> Vec<RecordedInput> {
    let longest = orders.iter().map(Vec::len).max().unwrap_or(0);
    (0..longest)
        .flat_map(|i| orders.iter().filter_map(move |o| o.get(i).cloned()))
        .collect()
}

/// The scenario most cells run: two orders interleaved, with an input that opens nothing between them.
fn scenario() -> Vec<RecordedInput> {
    let mut all = interleaved(&[
        probe::order_inputs("ord-1", 0),
        probe::order_inputs("ord-2", 100),
    ]);
    all.insert(7, probe::orphan(50));
    all
}

fn acknowledged_records_survive_a_kill<H: Harness<Probe>>(harness: &mut H) -> Checked<()> {
    let mut host = started(harness)?;
    let acks = record_all(&mut host, &scenario())?;
    let host = reopened(harness, host)?;
    same_journals(
        &journals(&host)?,
        &grouped(&acks),
        "after a kill and reopen",
    )?;
    states_follow_the_probe(&host, "after a kill and reopen")
}

fn halt_survives_a_kill<H: Harness<Probe>>(harness: &mut H) -> Checked<()> {
    let inputs = scenario();
    let mut host = started(harness)?;
    record_all(&mut host, &inputs[..2])?;
    host_err(host.halt(), "halt")?;
    let mut host = reopened(harness, host)?;
    match host.submit(inputs[2].clone()) {
        Err(HostError::Halted) => {
            states_follow_the_probe(&host, "after a halted host was reopened")
        }
        other => Err(format!(
            "a halted host reopened and answered a submit with {other:?}, not Halted"
        )),
    }
}

fn stop_at<H: FaultSeam<Probe>>(harness: &mut H, point: CrashPoint) -> Checked<()> {
    let inputs = scenario();
    let (before, in_flight) = (&inputs[..5], &inputs[5]);
    let mut host = started(harness)?;
    let acks = record_all(&mut host, before)?;
    harness.crash_at(&mut host, point);
    match host.submit(in_flight.clone()) {
        Err(HostError::Unavailable { .. }) => {}
        other => {
            return Err(format!(
                "the submit didn't stop at {point:?}: it returned {other:?}"
            ));
        }
    }
    // A stopped host is fit only for a kill: nothing is read from it first.
    let mut host = reopened(harness, host)?;
    let found = journals(&host)?;
    contains_all(&found, &acks, "after the stop")?;
    states_follow_the_probe(&host, &format!("after a stop at {point:?}"))?;
    let under_its_id: Vec<&TransitionRecord> = found
        .values()
        .flatten()
        .filter(|r| r.input.record_id == in_flight.record_id)
        .collect();
    if let Some(other) = under_its_id.iter().find(|r| r.input != *in_flight) {
        return Err(format!(
            "the record under {} after a stop at {point:?} holds another input: {:?}",
            in_flight.record_id.as_str(),
            other.input
        ));
    }
    let in_flight_records = under_its_id.len();
    let durable = point == CrashPoint::AfterDurable;
    match (durable, in_flight_records) {
        (false, 0) | (true, 1) => {}
        (false, _) => {
            return Err(format!(
                "a record of {} is present though the host stopped at {point:?}, before durability",
                in_flight.record_id.as_str()
            ));
        }
        (true, n) => {
            return Err(format!(
                "{n} records of {} after a stop after durable, not 1",
                in_flight.record_id.as_str()
            ));
        }
    }
    let records_before: usize = found.values().map(Vec::len).sum();
    let delivery = host_err(
        host.submit(in_flight.clone()),
        "submitting the stopped input again",
    )?;
    let released = match (durable, delivery) {
        (false, Delivery::Recorded(r)) | (true, Delivery::AlreadyRecorded(r)) => r,
        (_, other) => {
            return Err(format!(
                "submitting the stopped input again after {point:?} gave {other:?}"
            ));
        }
    };
    let after = journals(&host)?;
    let records_after: usize = after.values().map(Vec::len).sum();
    let expected = records_before + usize::from(!durable);
    if records_after != expected {
        return Err(format!(
            "submitting the stopped input again left {records_after} records, not {expected}"
        ));
    }
    if released.input != *in_flight {
        return Err(format!(
            "submitting {} again released a record of another input",
            in_flight.record_id.as_str()
        ));
    }
    contains_all(&after, &[released], "after it was submitted again")?;
    states_follow_the_probe(&host, "after the stopped input was submitted again")
}

fn released_records_survive_a_kill<H: Harness<Probe>>(harness: &mut H) -> Checked<()> {
    let mut host = started(harness)?;
    let mut released = record_all(&mut host, &scenario())?;
    // Reads release records too: an effect driver may act on anything read.
    released.extend(journals(&host)?.into_values().flatten());
    let host = reopened(harness, host)?;
    contains_all(
        &journals(&host)?,
        &released,
        "after a kill, though it was released: an effect could act on it with no record",
    )?;
    states_follow_the_probe(&host, "after a kill")
}

fn expect_repeat<H: Host<Probe>>(host: &mut H, record: &TransitionRecord) -> Checked<()> {
    let id = record.input.record_id.as_str();
    match host.submit(record.input.clone()) {
        Ok(Delivery::AlreadyRecorded(r)) if &r == record => Ok(()),
        Ok(Delivery::AlreadyRecorded(r)) => {
            Err(format!("a repeat of {id} returned another record: {r:?}"))
        }
        Ok(Delivery::Recorded(r)) => Err(format!(
            "a repeat of {id} made a second record (seq {} of {})",
            r.seq,
            r.order_id.as_str()
        )),
        Err(e) => Err(format!("a repeat of {id}: the host returned {e:?}")),
    }
}

fn repeat_returns_the_recorded_record<H: Harness<Probe>>(harness: &mut H) -> Checked<()> {
    let mut host = started(harness)?;
    let acks = record_all(&mut host, &scenario())?;
    for r in &acks {
        expect_repeat(&mut host, r)?;
    }
    same_journals(
        &journals(&host)?,
        &grouped(&acks),
        "after every input was repeated",
    )?;
    states_follow_the_probe(&host, "after every input was repeated")
}

fn repeat_after_a_kill<H: Harness<Probe>>(harness: &mut H) -> Checked<()> {
    let mut host = started(harness)?;
    let acks = record_all(&mut host, &scenario())?;
    let mut host = reopened(harness, host)?;
    for r in &acks {
        expect_repeat(&mut host, r)?;
    }
    same_journals(
        &journals(&host)?,
        &grouped(&acks),
        "after a kill and every input repeated",
    )?;
    states_follow_the_probe(&host, "after a kill and every input repeated")
}

fn reused_record_id_is_refused_and_logged<H: Harness<Probe>>(harness: &mut H) -> Checked<()> {
    let inputs = scenario();
    let mut host = started(harness)?;
    let acks = record_all(&mut host, &inputs)?;
    let taken = &acks[2];
    let variants = [
        ("differs in what was observed", probe::reusing(&taken.input)),
        (
            "differs only in recorded_at",
            probe::restamped(&taken.input),
        ),
        (
            "differs only in its order",
            probe::for_another_order(&taken.input),
        ),
    ];
    let mut expected = Vec::new();
    for (n, (what, other)) in variants.into_iter().enumerate() {
        let refusal = match host.submit(other.clone()) {
            Err(HostError::RecordIdReused { refusal }) => *refusal,
            result => {
                return Err(format!(
                    "an input under a used record id that {what} gave {result:?}, not a refusal"
                ));
            }
        };
        let wanted = RefusalRecord {
            seq: n as u64 + 1,
            input: other,
            reason: contract::RefusalReason::RecordIdReused,
            existing: contract::RecordRef {
                order_id: taken.order_id.clone(),
                seq: taken.seq,
            },
        };
        if refusal != wanted {
            return Err(format!(
                "the refusal of an input that {what} is {refusal:?}, not {wanted:?}"
            ));
        }
        expected.push(wanted);
    }
    let log = host_err(host.refusals(), "refusals")?;
    if log != expected {
        return Err(format!(
            "the refusal log holds {log:?}, not the {} refusals",
            expected.len()
        ));
    }
    same_journals(&journals(&host)?, &grouped(&acks), "after refused inputs")?;
    let host = reopened(harness, host)?;
    let log = host_err(host.refusals(), "refusals after a kill")?;
    if log != expected {
        return Err(format!(
            "the refusal log holds {} entries after a kill, not {}: it isn't durable",
            log.len(),
            expected.len()
        ));
    }
    states_follow_the_probe(&host, "after refused inputs and a kill")
}

fn reordered_delivery_is_applied_in_submit_order<H: Harness<Probe>>(
    harness: &mut H,
    seed: u64,
) -> Checked<()> {
    let orders = ["ord-1", "ord-2"];
    let recorded = interleaved(&[
        probe::order_inputs(orders[0], 0),
        probe::order_inputs(orders[1], 100),
    ]);
    let mut submitted = recorded.clone();
    probe::shuffle(&mut submitted, seed);
    if submitted == recorded {
        return Err(format!(
            "seed {seed} leaves the delivery in its recorded order; choose another"
        ));
    }
    let mut host = started(harness)?;
    record_all(&mut host, &submitted)?;
    for order in orders {
        let mine: Vec<RecordedInput> = submitted
            .iter()
            .filter(|i| i.subject.order.as_str() == order)
            .cloned()
            .collect();
        let journal = host_err(host.journal(&Id::new(order).expect("an id")), "journal")?;
        if journal.len() != mine.len() {
            return Err(format!(
                "{order}'s journal holds {} records, not {}",
                journal.len(),
                mine.len()
            ));
        }
        let expected = probe::fold(&mine);
        for (n, (r, (input, state))) in journal.iter().zip(mine.iter().zip(&expected)).enumerate() {
            if r.seq != n as u64 + 1 {
                return Err(format!(
                    "record {} of {order} has seq {}: the order's sequence has a gap or is out of order",
                    n + 1,
                    r.seq
                ));
            }
            if &r.input != input {
                return Err(format!(
                    "seq {} of {order} holds {} but its submit {} was {}: the host reordered the inputs",
                    r.seq,
                    r.input.record_id.as_str(),
                    n + 1,
                    input.record_id.as_str()
                ));
            }
            if &r.state_after != state {
                return Err(format!(
                    "seq {} of {order}: its state doesn't follow from the one before",
                    r.seq
                ));
            }
        }
    }
    states_follow_the_probe(&host, "after a reordered delivery")
}

/// A host after a swap, with what the old host held.
struct Swapped<H> {
    new: H,
    rest: Vec<RecordedInput>,
    held: Journals,
    refusals: Vec<RefusalRecord>,
}

/// Runs half the scenario and a refused input on a fresh `harness` host, halts it, and swaps it onto `target`.
fn swapped<H: Harness<Probe>, S: Harness<Probe>>(
    harness: &mut H,
    target: &mut S,
) -> Checked<Swapped<S::Host>> {
    let inputs = scenario();
    // The split falls after the input that opens nothing, so a record with no state crosses the swap.
    let orphan = inputs
        .iter()
        .position(|i| i.subject.order.as_str() == probe::ORPHAN_ORDER)
        .expect("the scenario has an input that opens nothing");
    let (before, after) = inputs.split_at(orphan + 1);
    let mut old = started(harness)?;
    let acks = record_all(&mut old, before)?;
    match old.submit(probe::reusing(&acks[0].input)) {
        Err(HostError::RecordIdReused { .. }) => {}
        other => {
            return Err(format!(
                "a reused record id before the swap gave {other:?}, not a refusal"
            ));
        }
    }
    host_err(old.halt(), "halt")?;
    let held = journals(&old)?;
    contains_all(&held, &acks, "on the old host before the swap")?;
    if !held
        .get(probe::ORPHAN_ORDER)
        .is_some_and(|j| !j.is_empty() && j.iter().all(|r| r.state_after.is_none()))
    {
        return Err("the old host holds no record without a state to carry across the swap".into());
    }
    let refusals = host_err(old.refusals(), "refusals")?;
    let store = harness_err(target.fresh_store(), "fresh store for the new host")?;
    let stream = host_err(old.export(), "export")?;
    let new = harness_err(target.import(store, stream), "import")?;
    Ok(Swapped {
        new,
        rest: after.to_vec(),
        held,
        refusals,
    })
}

fn swap_mid_order_loses_no_order<H: Harness<Probe>, S: Harness<Probe>>(
    harness: &mut H,
    target: &mut S,
) -> Checked<()> {
    let Swapped {
        mut new,
        rest,
        held,
        refusals,
    } = swapped(harness, target)?;
    same_journals(&journals(&new)?, &held, "after the swap")?;
    states_follow_the_probe(&new, "after the swap")?;
    let moved = host_err(new.refusals(), "refusals on the new host")?;
    if moved != refusals {
        return Err(format!("the refusal log changed in the swap: {moved:?}"));
    }
    host_err(new.resume(), "resume")?;
    record_all(&mut new, &rest)?;
    for order in ["ord-1", "ord-2"] {
        let state = host_err(new.state(&Id::new(order).expect("an id")), "state")?;
        if state.as_ref().map(|o| o.state) != Some(OrderState::Complete) {
            return Err(format!(
                "{order} didn't run to its end on the new host: {:?}",
                state.map(|o| o.state)
            ));
        }
    }
    states_follow_the_probe(&new, "after the order ran to its end on the new host")
}

fn repeat_after_a_swap_is_recognised<H: Harness<Probe>, S: Harness<Probe>>(
    harness: &mut H,
    target: &mut S,
) -> Checked<()> {
    let Swapped { mut new, held, .. } = swapped(harness, target)?;
    host_err(new.resume(), "resume")?;
    for r in held.values().flatten() {
        expect_repeat(&mut new, r)?;
    }
    same_journals(
        &journals(&new)?,
        &held,
        "after inputs from before the swap were repeated",
    )?;
    states_follow_the_probe(&new, "after inputs from before the swap were repeated")
}

fn halted_host_refuses_and_only_a_halted_host_exports<H: Harness<Probe>>(
    harness: &mut H,
) -> Checked<()> {
    let inputs = scenario();
    let mut host = started(harness)?;
    let acks = record_all(&mut host, &inputs[..3])?;
    match host.export().map(|_| ()) {
        Err(HostError::NotHalted) => {}
        other => {
            return Err(format!(
                "a host that isn't halted answered an export with {other:?}, not NotHalted"
            ));
        }
    }
    host_err(host.halt(), "halt")?;
    match host.submit(inputs[3].clone()) {
        Err(HostError::Halted) => {}
        other => {
            return Err(format!(
                "a halted host answered a submit with {other:?}, not Halted"
            ));
        }
    }
    same_journals(
        &journals(&host)?,
        &grouped(&acks),
        "after a submit to a halted host",
    )?;
    states_follow_the_probe(&host, "after a submit to a halted host")?;
    host_err(host.export().map(|_| ()), "export of a halted host")?;
    host_err(host.resume(), "resume")?;
    same_journals(
        &journals(&host)?,
        &grouped(&acks),
        "after resume, holding an input the host refused while halted",
    )?;
    // The refused input is new to the host: submitted again after resume, it is recorded once, now.
    let refused = record(&mut host, &inputs[3])?;
    let mut expected = acks;
    expected.push(refused);
    same_journals(
        &journals(&host)?,
        &grouped(&expected),
        "after the input refused while halted was submitted again",
    )?;
    states_follow_the_probe(&host, "after resume")
}

/// The scenario's export from a fresh, halted `harness` host, as entries.
fn exported<H: Harness<Probe>>(harness: &mut H) -> Checked<Vec<SnapshotEntry>> {
    let mut old = started(harness)?;
    record_all(&mut old, &scenario())?;
    match old.submit(probe::reusing(&scenario()[0])) {
        Err(HostError::RecordIdReused { .. }) => {}
        other => return Err(format!("a reused record id gave {other:?}, not a refusal")),
    }
    host_err(old.halt(), "halt")?;
    let stream = host_err(old.export(), "export")?;
    stream
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("reading the export: the host returned {e:?}"))
}

fn stream(entries: Vec<SnapshotEntry>) -> contract::SnapshotStream<'static> {
    Box::new(entries.into_iter().map(Ok))
}

fn stop_during_import_is_refused_at_open<H: Harness<Probe>, S: Harness<Probe>>(
    harness: &mut H,
    target: &mut S,
) -> Checked<()> {
    let entries = exported(harness)?;
    let last = entries.len() as u64 - 1;
    for stop in [1, last / 2, last] {
        let store = harness_err(target.fresh_store(), "fresh store for the new host")?;
        let left = harness_err(
            target.import_crashing_after(store, stream(entries.clone()), stop),
            "stopped import",
        )?;
        match target.reopen(left) {
            Err(HarnessError::Host(HostError::ImportIncomplete)) => {}
            Ok(_) => {
                return Err(format!(
                    "a store left by an import stopped after {stop} of {} entries opened as a host",
                    entries.len()
                ));
            }
            Err(e) => {
                return Err(format!(
                    "a store left by an import stopped after {stop} entries gave {e}, not ImportIncomplete"
                ));
            }
        }
        let fresh = harness_err(target.fresh_store(), "fresh store to redo the import")?;
        let redone = harness_err(
            target.import(fresh, stream(entries.clone())),
            &format!("the import redone on a fresh store after a stop at {stop}"),
        )?;
        let records: Vec<TransitionRecord> = entries
            .iter()
            .filter_map(|e| match e {
                SnapshotEntry::Record(r) => Some(r.clone()),
                _ => None,
            })
            .collect();
        let refusals: Vec<RefusalRecord> = entries
            .iter()
            .filter_map(|e| match e {
                SnapshotEntry::Refusal(r) => Some(r.clone()),
                _ => None,
            })
            .collect();
        same_journals(
            &journals(&redone)?,
            &grouped(&records),
            &format!("after the import was redone on a fresh store, following a stop at {stop}"),
        )?;
        if host_err(redone.refusals(), "refusals after the redone import")? != refusals {
            return Err(format!(
                "the refusal log differs after the import was redone, following a stop at {stop}"
            ));
        }
        states_follow_the_probe(&redone, "after the import was redone")?;
    }
    Ok(())
}

fn import_refuses_a_stream_it_cannot_take<H: Harness<Probe>, S: Harness<Probe>>(
    harness: &mut H,
    target: &mut S,
) -> Checked<()> {
    let entries = exported(harness)?;

    let mut unreadable = entries.clone();
    unreadable[0] = SnapshotEntry::Header {
        host_version: Version::new("99.0.0").expect("a version"),
        state_machine_version: Version::new(machine::VERSION).expect("a version"),
    };

    let mut duplicated = entries.clone();
    let Some(SnapshotEntry::Record(first)) = entries.get(1).cloned() else {
        return Err("the export's first entry after its header isn't a record".into());
    };
    let copy_order = Id::new("ord-copy").expect("an id");
    let mut copy = first;
    copy.order_id = copy_order.clone();
    copy.input.subject.order = copy_order.clone();
    if let Some(state) = copy.state_after.as_mut() {
        state.order_id = copy_order;
    }
    let end = duplicated.len() - 1;
    duplicated.insert(end, SnapshotEntry::Record(copy));
    if let Some(SnapshotEntry::End { records, .. }) = duplicated.last_mut() {
        *records += 1;
    }

    let cases: [(&str, Vec<SnapshotEntry>, bool); 3] = [
        ("a version it can't read", unreadable, false),
        ("a record id recorded twice", duplicated, false),
        ("a store that isn't empty", entries, true),
    ];
    for (what, entries, occupied) in cases {
        let store = if occupied {
            let mut other = started(target)?;
            record(&mut other, &probe::order_inputs("ord-already", 300)[0])?;
            harness_err(target.kill(other), "kill")?
        } else {
            harness_err(target.fresh_store(), "fresh store for the new host")?
        };
        match target.import(store, stream(entries)) {
            Err(HarnessError::Host(HostError::SnapshotRefused { .. })) => {}
            Ok(_) => return Err(format!("a stream with {what} was imported")),
            Err(e) => {
                return Err(format!(
                    "a stream with {what} gave {e}, not SnapshotRefused"
                ));
            }
        }
    }
    Ok(())
}

fn replay_reaches_the_same_states<H: Harness<Probe>>(harness: &mut H, seed: u64) -> Checked<()> {
    let mut inputs = scenario();
    let mut reordered = probe::order_inputs("ord-3", 200);
    probe::shuffle(&mut reordered, seed);
    inputs.extend(reordered);
    let mut host = started(harness)?;
    let acks = record_all(&mut host, &inputs)?;
    if host.submit(probe::reusing(&acks[0].input)).is_ok() {
        return Err("a reused record id was accepted".into());
    }
    let listed = journals(&host)?;
    for (order, journal) in &listed {
        let expected = probe::fold(journal.iter().map(|r| &r.input));
        for (r, state) in journal.iter().zip(&expected) {
            if &r.state_after != state {
                return Err(format!(
                    "seq {} of {order} ({}): the recorded state differs from the replay",
                    r.seq,
                    r.input.record_id.as_str()
                ));
            }
        }
        let latest = expected.into_iter().rev().find_map(|s| s);
        let id = Id::new(order.as_str()).expect("a listed order has an id");
        if host_err(host.state(&id), "state")? != latest {
            return Err(format!(
                "{order}'s state() differs from the replay of its journal"
            ));
        }
    }
    let recorded: std::collections::BTreeSet<&str> =
        acks.iter().map(|r| r.order_id.as_str()).collect();
    let orders: std::collections::BTreeSet<&str> = listed.keys().map(String::as_str).collect();
    if orders != recorded {
        return Err(format!(
            "orders lists {orders:?}, but the orders with a record are {recorded:?}"
        ));
    }
    Ok(())
}

fn guarded(check: impl FnOnce() -> Checked<()>) -> CellOutcome {
    match catch_unwind(AssertUnwindSafe(check)) {
        Ok(Ok(())) => CellOutcome::Pass,
        Ok(Err(detail)) => CellOutcome::Fail(detail),
        Err(_) => CellOutcome::Fail("the candidate panicked".into()),
    }
}

fn black_box_outcome<H: Harness<Probe>, S: Harness<Probe>>(
    id: &str,
    seed: u64,
    harness: &mut H,
    target: &mut S,
) -> CellOutcome {
    guarded(|| match id {
        "acknowledged-records-survive-a-kill" => acknowledged_records_survive_a_kill(harness),
        "halt-survives-a-kill" => halt_survives_a_kill(harness),
        "released-records-survive-a-kill" => released_records_survive_a_kill(harness),
        "repeat-returns-the-recorded-record" => repeat_returns_the_recorded_record(harness),
        "repeat-after-a-kill" => repeat_after_a_kill(harness),
        "reused-record-id-is-refused-and-logged" => reused_record_id_is_refused_and_logged(harness),
        "reordered-delivery-is-applied-in-submit-order" => {
            reordered_delivery_is_applied_in_submit_order(harness, seed)
        }
        "swap-mid-order-loses-no-order" => swap_mid_order_loses_no_order(harness, target),
        "repeat-after-a-swap-is-recognised" => repeat_after_a_swap_is_recognised(harness, target),
        "halted-host-refuses-and-only-a-halted-host-exports" => {
            halted_host_refuses_and_only_a_halted_host_exports(harness)
        }
        "stop-during-import-is-refused-at-open" => {
            stop_during_import_is_refused_at_open(harness, target)
        }
        "import-refuses-a-stream-it-cannot-take" => {
            import_refuses_a_stream_it_cannot_take(harness, target)
        }
        "replay-reaches-the-same-states" => replay_reaches_the_same_states(harness, seed),
        other => Err(format!("the suite has no check for {other}")),
    })
}

fn crash_point(id: &str) -> Option<CrashPoint> {
    match id {
        "stop-after-received" => Some(CrashPoint::AfterReceived),
        "stop-after-computed" => Some(CrashPoint::AfterComputed),
        "stop-after-durable" => Some(CrashPoint::AfterDurable),
        _ => None,
    }
}

fn report(config: RunConfig, mut outcome: impl FnMut(&Cell) -> CellOutcome) -> Report {
    Report {
        host_version: contract::VERSION,
        state_machine_version: machine::VERSION,
        cells: CELLS.iter().map(|c| (*c, outcome(c))).collect(),
        config,
    }
}

/// Runs the suite on a candidate driven only through [`Harness`], such as a runtime in another process, which the
/// suite stops by killing it between submits. The cells that need [`FaultSeam`] are reported not run. A swap goes
/// from a `harness` host to a `target` host; `EXP-1` pairs two different implementations.
pub fn run_black_box<H: Harness<Probe>, S: Harness<Probe>>(
    config: RunConfig,
    harness: &mut H,
    target: &mut S,
) -> Report {
    let seed = config.seed;
    report(config, |c| {
        if c.needs_fault_seam {
            CellOutcome::NotRun(
                "needs FaultSeam: a candidate driven only through Harness is killed between submits, not stopped at \
                 a named point"
                    .into(),
            )
        } else {
            black_box_outcome(c.id, seed, harness, target)
        }
    })
}

/// Runs every cell on an in-process candidate whose harness implements [`FaultSeam`].
pub fn run_with_fault_seam<H: FaultSeam<Probe>, S: Harness<Probe>>(
    config: RunConfig,
    harness: &mut H,
    target: &mut S,
) -> Report {
    let seed = config.seed;
    report(config, |c| match crash_point(c.id) {
        Some(point) => guarded(|| stop_at(harness, point)),
        None => black_box_outcome(c.id, seed, harness, target),
    })
}
