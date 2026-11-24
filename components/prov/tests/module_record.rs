//! The platform's carried-out record survives a restart (`T-013-10`; `INV-8`, `NFR-6`, `REQ-CAT-10`,
//! `REQ-MDL-14`): what the platform carried out under each transaction, the segments in flight, the transactions
//! that ended and the runs it counted are written to a record before the platform acts on them, and read back when
//! a platform opens on it. A restarted platform answers a repeat from the record as `T-013-09`'s does in one run.
//! The providers are the catalogue's reserve (`safe_retry` false, evidence unbounded) and ramp (`safe_retry` true,
//! evidence for 30 days), both running the boundary suite's stand-in.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;
#[path = "holding_support/mod.rs"]
mod support;

use std::io;
use std::path::PathBuf;

use common::{TestWorld, WORLD_AT, withdrawal};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_prov::catalogue::Instant;
use rutent_prov::module::lifecycle::{DecisionKind, LifecycleRefusal, Mark};
use rutent_prov::module::record::{FileRecord, MemoryRecord, Record};
use rutent_prov::module::types::{Admitted, Handled};
use signed::{AT, RAMP, RESERVE, Signed, release};

/// How long the ramp's advertisement says evidence of an outcome stays available: 30 days.
const RAMP_EVIDENCE: u64 = 2_592_000;

/// Admits `provider`'s first release, handling withdrawals, and gives it its acceptance: it runs. A restarted
/// platform holds no release until one is admitted again, as a deployment's start does.
fn admit(s: &mut Signed, provider: &str) {
    admit_version(s, provider, "1.0.0");
    assert_eq!(
        s.platform.active_version(provider).as_deref(),
        Some("1.0.0")
    );
}

/// Admits `provider`'s release of `version`, handling withdrawals, and gives it its acceptance.
fn admit_version(s: &mut Signed, provider: &str, version: &str) {
    let key = signed::catalogue_support::advertising_key(provider);
    let (declaration, binaries) = release(provider, version, &["withdrawal"]);
    let Admitted::Artefact(artefact) =
        s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
    else {
        panic!("the release wasn't admitted");
    };
    if artefact.acceptance.accepted.is_none() {
        s.accept(provider, &artefact.assertion_digest, "test-reviewer")
            .expect("the release is accepted");
    }
}

/// A platform on `record` over `world`, with each of `providers`' first release running.
fn opened(record: Box<dyn Record>, world: TestWorld, providers: &[&str]) -> Signed {
    let mut s = Signed::on(record, world);
    for provider in providers {
        admit(&mut s, provider);
    }
    s
}

/// The platform stops: every value it held in memory is dropped, its providers' processes with it. The world it
/// reached beyond them stays, to be handed to the platform that opens next.
fn stop(mut s: Signed) -> TestWorld {
    let world = std::mem::take(&mut s.platform.world);
    drop(s);
    world
}

fn tx(provider: &str, n: usize) -> Identifier {
    Identifier::new(format!("tx-{provider}-{n}")).expect("an identifier")
}

/// `withdrawal(provider, n)` with `change` made to its JSON.
fn changed(
    provider: &str,
    n: usize,
    change: impl FnOnce(&mut serde_json::Value),
) -> Statement<Operation> {
    let mut v = serde_json::to_value(withdrawal(provider, n)).expect("JSON");
    change(&mut v);
    serde_json::from_value(v).expect("an operation")
}

/// A dry run of `withdrawal(provider, n)`: it moves nothing.
fn dry_run(provider: &str, n: usize) -> Statement<Operation> {
    changed(provider, n, |v| v["statement"]["dry_run"] = true.into())
}

/// A fresh file for a record, in a directory of its own.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rutent-module-record-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir.join("carried")
}

/// A record that fails every append from the first whose entry is of `kind` on, as a platform stopped at that
/// point leaves its record: what came before it is written, nothing after.
struct StopsAt {
    inner: MemoryRecord,
    kind: &'static str,
    stopped: bool,
}

impl StopsAt {
    fn new(inner: &MemoryRecord, kind: &'static str) -> Self {
        Self {
            inner: inner.clone(),
            kind,
            stopped: false,
        }
    }
}

impl Record for StopsAt {
    fn load(&mut self) -> io::Result<Vec<String>> {
        self.inner.load()
    }

    fn append(&mut self, line: &str) -> io::Result<()> {
        self.stopped |= line.contains(&format!("\"entry\":\"{}\"", self.kind));
        if self.stopped {
            return Err(io::Error::other("stopped"));
        }
        self.inner.append(line)
    }

    fn replace(&mut self, lines: &[String]) -> io::Result<()> {
        self.inner.replace(lines)
    }
}

// verifies: INV-8
#[test]
fn a_repeat_after_the_platform_restarts_moves_nothing_twice() {
    let path = scratch("restart");
    // The deployment's first start makes its record; every start after opens it.
    drop(FileRecord::create(&path).expect("the record is created"));
    let file = || Box::new(FileRecord::open(&path).expect("the record opens"));
    let mut s = opened(file(), TestWorld::default(), &[RESERVE, RAMP]);
    // The reserve's transaction is held in a segment, as Core holds it; the ramp's isn't.
    s.platform
        .begin_segment(RESERVE, &tx(RESERVE, 1))
        .expect("a segment");
    let reserve = s.platform.handle(RESERVE, &withdrawal(RESERVE, 1));
    assert!(matches!(reserve, Handled::Answered(_)), "{reserve:?}");
    let ramp = s.platform.handle(RAMP, &withdrawal(RAMP, 1));
    assert!(matches!(ramp, Handled::Answered(_)), "{ramp:?}");

    // The platform restarts mid-segment, on its record and nothing else it held.
    let world = stop(s);
    let mut s = opened(file(), world, &[RESERVE, RAMP]);
    assert_eq!(
        s.platform
            .lifecycle()
            .segment(tx(RESERVE, 1).as_str())
            .map(|segment| segment.runs_on.as_str()),
        Some("1.0.0"),
        "the segment in flight is held again"
    );
    // A provider without safe retry: refused, the module not run.
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(
        s.platform.operations_run(RESERVE),
        1,
        "the run before the restart is counted, and the repeat never ran"
    );
    // A provider with safe retry: the first answer, byte for byte, from the record.
    assert_eq!(s.platform.handle(RAMP, &withdrawal(RAMP, 1)), ramp);
    assert_eq!(s.platform.operations_run(RAMP), 1);
    // The segment ends on the restarted platform as it would have on the first.
    s.platform
        .end_segment(tx(RESERVE, 1).as_str())
        .expect("the segment's record");
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(s.platform.operations_run(RESERVE), 1);

    // Stopped while the module ran: the operation was written before it was handed over, and its answer never was.
    let memory = MemoryRecord::new();
    let mut s = opened(
        Box::new(StopsAt::new(&memory, "ran")),
        TestWorld::default(),
        &[RESERVE],
    );
    let handled = s.platform.handle(RESERVE, &withdrawal(RESERVE, 2));
    assert!(matches!(handled, Handled::Answered(_)), "{handled:?}");
    let world = stop(s);
    let mut s = opened(Box::new(memory), world, &[RESERVE]);
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 2)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(s.platform.operations_run(RESERVE), 0);
    let _ = std::fs::remove_dir_all(path.parent().expect("a directory"));
}

/// An operation the platform can't write to its record before the hand-over is not handed over: a restart could
/// lose it, and a repeat after it move value twice.
// verifies: INV-8
#[test]
fn an_operation_the_platform_cannot_record_is_not_handed_to_the_module() {
    let memory = MemoryRecord::new();
    let mut s = opened(
        Box::new(StopsAt::new(&memory, "carrying")),
        TestWorld::default(),
        &[RESERVE],
    );
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::Unavailable)
    );
    assert_eq!(s.platform.operations_run(RESERVE), 0);
    // What moves nothing needs no record.
    let dry = s.platform.handle(RESERVE, &dry_run(RESERVE, 2));
    assert!(matches!(dry, Handled::Answered(_)), "{dry:?}");
}

/// A second platform can't open a record a running one holds: two would each carry out what the other did.
// verifies: INV-8
#[test]
fn a_record_another_platform_holds_is_not_opened() {
    let path = scratch("held");
    let held = FileRecord::create(&path).expect("the record is created");
    assert_eq!(
        FileRecord::open(&path).expect_err("held").kind(),
        io::ErrorKind::WouldBlock
    );
    drop(held);
    let _ = std::fs::remove_dir_all(path.parent().expect("a directory"));
}

// verifies: INV-8
#[test]
fn ended_transaction_stays_ended_after_the_platform_restarts() {
    let memory = MemoryRecord::new();
    let mut s = opened(Box::new(memory.clone()), TestWorld::default(), &[RESERVE]);
    s.platform
        .begin_segment(RESERVE, &tx(RESERVE, 1))
        .expect("a segment");
    let first = s.platform.handle(RESERVE, &withdrawal(RESERVE, 1));
    assert!(matches!(first, Handled::Answered(_)), "{first:?}");
    s.platform
        .end_segment(tx(RESERVE, 1).as_str())
        .expect("the segment's record");
    assert!(!s.platform.carries(tx(RESERVE, 1).as_str()));

    let world = stop(s);
    let mut s = opened(Box::new(memory), world, &[RESERVE]);
    assert!(s.platform.lifecycle().has_ended(tx(RESERVE, 1).as_str()));
    assert_eq!(
        s.platform.begin_segment(RESERVE, &tx(RESERVE, 1)),
        Err(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    // Ended is ended: not even a dry run of it runs.
    assert_eq!(
        s.platform.handle(RESERVE, &dry_run(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(s.platform.operations_run(RESERVE), 1);
}

// verifies: INV-8, REQ-MDL-14
#[test]
fn segment_begun_on_a_transaction_already_carried_out_is_refused() {
    let memory = MemoryRecord::new();
    let mut s = opened(
        Box::new(memory.clone()),
        TestWorld::default(),
        &[RESERVE, RAMP],
    );
    let first = s.platform.handle(RESERVE, &withdrawal(RESERVE, 1));
    assert!(matches!(first, Handled::Answered(_)), "{first:?}");
    // Carried out with no segment: no segment begins on it, at its provider or another.
    for provider in [RESERVE, RAMP] {
        assert_eq!(
            s.platform.begin_segment(provider, &tx(RESERVE, 1)),
            Err(PlatformRefusal::DuplicateTransaction),
            "{provider}"
        );
    }
    // A dry run carries nothing out, so a segment may begin on its transaction.
    let dry = s.platform.handle(RESERVE, &dry_run(RESERVE, 2));
    assert!(matches!(dry, Handled::Answered(_)), "{dry:?}");
    s.platform
        .begin_segment(RESERVE, &tx(RESERVE, 2))
        .expect("a segment");

    // Nor after a restart.
    let world = stop(s);
    let mut s = opened(Box::new(memory), world, &[RESERVE]);
    assert_eq!(
        s.platform.begin_segment(RESERVE, &tx(RESERVE, 1)),
        Err(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(s.platform.operations_run(RESERVE), 2);
}

// verifies: REQ-CAT-10
#[test]
fn unsegmented_operations_record_is_freed_after_its_evidence_window() {
    let memory = MemoryRecord::new();
    let mut s = opened(
        Box::new(memory.clone()),
        TestWorld::default(),
        &[RESERVE, RAMP],
    );
    let first = s.platform.handle(RAMP, &withdrawal(RAMP, 1));
    assert!(matches!(first, Handled::Answered(_)), "{first:?}");
    // One held in a segment, and one at a provider whose evidence stays for as long as its record exists.
    s.platform
        .begin_segment(RAMP, &tx(RAMP, 2))
        .expect("a segment");
    let segmented = s.platform.handle(RAMP, &withdrawal(RAMP, 2));
    assert!(matches!(segmented, Handled::Answered(_)), "{segmented:?}");
    let unbounded = s.platform.handle(RESERVE, &withdrawal(RESERVE, 1));
    assert!(matches!(unbounded, Handled::Answered(_)), "{unbounded:?}");

    // A second before its window closes the record is kept, and a repeat is answered from it.
    s.platform
        .world
        .set_clock(Instant(WORLD_AT.0 + RAMP_EVIDENCE - 1));
    assert_eq!(s.platform.handle(RAMP, &withdrawal(RAMP, 1)), first);
    assert!(s.platform.carries(tx(RAMP, 1).as_str()));

    // Once it has closed the record is freed, and the transaction stays spent: a repeat is refused, not run.
    s.platform
        .world
        .set_clock(Instant(WORLD_AT.0 + RAMP_EVIDENCE));
    assert_eq!(
        s.platform.handle(RAMP, &withdrawal(RAMP, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert!(!s.platform.carries(tx(RAMP, 1).as_str()));
    assert_eq!(s.platform.operations_run(RAMP), 2);
    // A segment's record is freed when the segment ends, not by the window; unbounded evidence keeps its record.
    assert!(s.platform.carries(tx(RAMP, 2).as_str()));
    assert!(s.platform.carries(tx(RESERVE, 1).as_str()));

    // Freed stays freed after a restart, and so does spent.
    let world = stop(s);
    let mut s = opened(Box::new(memory.clone()), world, &[RAMP]);
    assert!(!s.platform.carries(tx(RAMP, 1).as_str()));
    assert!(s.platform.carries(tx(RAMP, 2).as_str()));
    assert_eq!(
        s.platform.handle(RAMP, &withdrawal(RAMP, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    // The record read back holds what is live, not every entry written.
    assert!(
        !memory.lines().iter().any(
            |line| line.contains(tx(RAMP, 1).as_str()) && !line.contains("\"entry\":\"ended\"")
        ),
        "{:#?}",
        memory.lines()
    );
}

/// The operator's marks are in the record: a segment restored after a restart on a version declared exploited is
/// refused, not run, once that version's release is admitted again, and a marked version is never put back in
/// force by starting (`REQ-MDL-20`, `T-013-10`).
// verifies: INV-8, REQ-MDL-20
#[test]
fn segment_restored_after_a_restart_refuses_a_version_declared_exploited() {
    let memory = MemoryRecord::new();
    let mut s = opened(Box::new(memory.clone()), TestWorld::default(), &[RESERVE]);
    s.platform
        .begin_segment(RESERVE, &tx(RESERVE, 1))
        .expect("a segment");
    admit_version(&mut s, RESERVE, "1.0.1");
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.1"));
    s.platform
        .declare_exploited(RESERVE, "1.0.0", "test-operator", AT)
        .expect("declared");
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::VersionExploited)
    );

    let world = stop(s);
    let mut s = Signed::on(Box::new(memory), world);
    assert_eq!(
        s.platform.lifecycle().mark(RESERVE, "1.0.0"),
        Some(Mark::Exploited)
    );
    assert!(
        s.platform
            .lifecycle()
            .decisions()
            .iter()
            .any(|d| d.kind == DecisionKind::DeclaredExploited && d.version == "1.0.0")
    );
    // The exploited release admitted again runs nothing: it isn't put in force, and no process runs it.
    admit_version(&mut s, RESERVE, "1.0.0");
    assert_eq!(s.platform.active_version(RESERVE), None);
    assert_eq!(s.platform.version_process(RESERVE, "1.0.0"), None);
    admit_version(&mut s, RESERVE, "1.0.1");
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.1"));
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::VersionExploited)
    );
    assert_eq!(s.platform.operations_run(RESERVE), 0);
}

/// A record that fails its first `carrying` write and every write after, as a full disk leaves a `FileRecord`:
/// handing `provider` an operation under transaction 9 breaks it.
fn break_record(s: &mut Signed, provider: &str) {
    assert_eq!(
        s.platform.handle(provider, &withdrawal(provider, 9)),
        Handled::Refused(PlatformRefusal::Unavailable)
    );
}

/// A decision that only restricts what runs is taken even where the record can't hold it: a failing disk never
/// keeps an exploited version running (`REQ-MDL-20`). It is answered `TakenUnrecorded`, since a restart loses it.
/// The reviewer's probe of fix round 1: a segment in flight on `1.0.0`, `1.0.1` in force, the record broken.
// verifies: REQ-MDL-20, INV-8
#[test]
fn an_exploit_declaration_the_record_cannot_hold_still_stops_the_version() {
    let memory = MemoryRecord::new();
    let mut s = opened(
        Box::new(StopsAt::new(&memory, "carrying")),
        TestWorld::default(),
        &[RESERVE],
    );
    s.platform
        .begin_segment(RESERVE, &tx(RESERVE, 1))
        .expect("a segment");
    admit_version(&mut s, RESERVE, "1.0.1");
    break_record(&mut s, RESERVE);
    assert!(s.platform.version_process(RESERVE, "1.0.0").is_some());

    assert_eq!(
        s.platform
            .declare_exploited(RESERVE, "1.0.0", "test-operator", AT),
        Err(LifecycleRefusal::TakenUnrecorded)
    );
    assert_eq!(
        s.platform.lifecycle().mark(RESERVE, "1.0.0"),
        Some(Mark::Exploited)
    );
    assert_eq!(s.platform.version_process(RESERVE, "1.0.0"), None);
    // Not even a dry run reaches it.
    assert_eq!(
        s.platform.handle(RESERVE, &dry_run(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::VersionExploited)
    );
    assert_eq!(s.platform.operations_run(RESERVE), 0);
}

/// Taken where the record can't hold it, a revocation of the version in force puts no other back in force:
/// putting one in force widens what runs, which an unrecorded decision never does.
// verifies: REQ-MDL-20
#[test]
fn a_revocation_the_record_cannot_hold_puts_no_version_back_in_force() {
    let memory = MemoryRecord::new();
    let mut s = opened(
        Box::new(StopsAt::new(&memory, "carrying")),
        TestWorld::default(),
        &[RESERVE],
    );
    admit_version(&mut s, RESERVE, "1.0.1");
    break_record(&mut s, RESERVE);
    assert_eq!(
        s.platform.revoke(RESERVE, "1.0.1", "test-operator", AT),
        Err(LifecycleRefusal::TakenUnrecorded)
    );
    assert_eq!(
        s.platform.lifecycle().mark(RESERVE, "1.0.1"),
        Some(Mark::Revoked)
    );
    assert_eq!(s.platform.lifecycle().in_force(RESERVE), None);
    assert_eq!(s.platform.active_version(RESERVE), None);
}

/// A decision that widens what runs the record can't hold isn't taken: a restart would lose it.
// verifies: REQ-MDL-20
#[test]
fn a_widening_decision_the_platform_cannot_record_is_not_taken() {
    let memory = MemoryRecord::new();
    let mut s = opened(
        Box::new(StopsAt::new(&memory, "carrying")),
        TestWorld::default(),
        &[RESERVE],
    );
    s.platform.set_staging(Some(
        rutent_prov::module::lifecycle::Staging::new(50, 0, 0).expect("a policy"),
    ));
    admit_version(&mut s, RESERVE, "1.0.1");
    assert_eq!(s.platform.lifecycle().staged(RESERVE), Some("1.0.1"));
    break_record(&mut s, RESERVE);
    for decided in [
        s.platform
            .roll_back(RESERVE, "test-operator", AT)
            .map(|_| ()),
        s.platform.promote(RESERVE, "test-operator", AT).map(|_| ()),
    ] {
        assert_eq!(decided, Err(LifecycleRefusal::Unrecorded));
    }
    assert_eq!(s.platform.lifecycle().staged(RESERVE), Some("1.0.1"));
    assert_eq!(s.platform.lifecycle().in_force(RESERVE), Some("1.0.0"));
    assert_eq!(s.platform.lifecycle().mark(RESERVE, "1.0.1"), None);
    assert!(s.platform.lifecycle().decisions().is_empty());
}

/// A record that fails one append, its first of `kind`, and takes every other: one that recovers in place, against
/// `Record`'s contract.
struct FailsOnce {
    inner: MemoryRecord,
    kind: &'static str,
    failed: bool,
}

impl Record for FailsOnce {
    fn load(&mut self) -> io::Result<Vec<String>> {
        self.inner.load()
    }

    fn append(&mut self, line: &str) -> io::Result<()> {
        if !self.failed && line.contains(&format!("\"entry\":\"{}\"", self.kind)) {
            self.failed = true;
            return Err(io::Error::other("failed once"));
        }
        self.inner.append(line)
    }

    fn replace(&mut self, lines: &[String]) -> io::Result<()> {
        self.inner.replace(lines)
    }
}

/// The platform holds itself to `Record`'s contract: after one failed write it writes nothing more, even to a
/// record that would take it, so a decision taken unrecorded is never followed by entries that leave it out and
/// read back as a state without it.
// verifies: REQ-MDL-20, INV-8
#[test]
fn after_a_failed_write_the_platform_writes_nothing_more_even_to_a_record_that_recovered() {
    let memory = MemoryRecord::new();
    let mut s = opened(
        Box::new(FailsOnce {
            inner: memory.clone(),
            kind: "decided",
            failed: false,
        }),
        TestWorld::default(),
        &[RESERVE],
    );
    admit_version(&mut s, RESERVE, "1.0.1");
    assert_eq!(
        s.platform
            .declare_exploited(RESERVE, "1.0.0", "test-operator", AT),
        Err(LifecycleRefusal::TakenUnrecorded)
    );
    let written = memory.lines().len();
    // The record would take these; the platform sends it none of them.
    assert_eq!(
        s.platform.revoke(RESERVE, "1.0.1", "test-operator", AT),
        Err(LifecycleRefusal::TakenUnrecorded)
    );
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::Unavailable)
    );
    assert_eq!(memory.lines().len(), written, "{:#?}", memory.lines());
}
