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
use rutent_contracts::module_interface::{
    Acceptance, AdmissionRefusal, Identifier, PlatformRefusal,
};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_prov::catalogue::Instant;
use rutent_prov::module::lifecycle::{Counts, DecisionKind, LifecycleRefusal, Mark, Staging};
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
    // A platform that stopped with its record cut short ended no way the record can vouch for: the operator
    // confirms what it holds before anything runs (`T-013-11`).
    let mut s = Signed::on(Box::new(memory), world);
    admit_version(&mut s, RESERVE, "1.0.0");
    assert_eq!(s.platform.active_version(RESERVE), None);
    s.platform
        .confirm(RESERVE, "test-operator", AT)
        .expect("confirmed");
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

/// Admits `provider`'s release of `version`, handling withdrawals, as [`admit_version`] does, and answers what
/// acceptance it was given, as the artefact says it.
fn admitted_as(s: &mut Signed, provider: &str, version: &str) -> Acceptance {
    let key = signed::catalogue_support::advertising_key(provider);
    let (declaration, binaries) = release(provider, version, &["withdrawal"]);
    let Admitted::Artefact(artefact) =
        s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
    else {
        panic!("the release wasn't admitted");
    };
    if artefact.acceptance.accepted.is_some() {
        return artefact.acceptance;
    }
    s.accept(provider, &artefact.assertion_digest, "test-reviewer")
        .expect("the release is accepted")
        .acceptance
}

/// What a release admitted again is given, with no acceptance of the platform's own to give.
fn readmitted(s: &mut Signed, provider: &str, version: &str) -> Admitted {
    let key = signed::catalogue_support::advertising_key(provider);
    let (declaration, binaries) = release(provider, version, &["withdrawal"]);
    s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
}

// verifies: REQ-MDL-4, INV-8
#[test]
fn release_in_force_and_staged_survives_a_platform_restart() {
    let policy = Staging::new(100, 60, 2).expect("a policy");
    let record = MemoryRecord::new();
    let mut s = Signed::on(Box::new(record.clone()), TestWorld::default());
    // The reserve's first release waits for a fresh acceptance, which a person gives; its next, asking for nothing
    // more, is accepted by itself and staged beside it.
    let first = admitted_as(&mut s, RESERVE, "1.0.0");
    s.platform.set_staging(Some(policy));
    let second = admitted_as(&mut s, RESERVE, "1.1.0");
    assert_ne!(first, second);
    assert_eq!(s.platform.lifecycle().in_force(RESERVE), Some("1.0.0"));
    assert_eq!(s.platform.lifecycle().staged(RESERVE), Some("1.1.0"));
    // Three segments begin on the staged release, which answers two operations; the third stays in flight.
    for n in 1..=3 {
        assert_eq!(
            s.platform
                .begin_segment(RESERVE, &tx(RESERVE, n))
                .as_deref(),
            Ok("1.1.0")
        );
    }
    for n in 1..=2 {
        assert!(s.answers_as(RESERVE, "1.1.0", n));
    }
    let counts = s
        .platform
        .lifecycle()
        .observation(RESERVE)
        .expect("staged")
        .staged_counts;
    assert_eq!(
        counts,
        Counts {
            segments: 3,
            answered: 2,
            refused: 0
        }
    );
    // The ramp's release was withdrawn while it waited.
    let key = signed::catalogue_support::advertising_key(RAMP);
    let (declaration, binaries) = release(RAMP, "1.0.0", &["withdrawal"]);
    let Admitted::Artefact(waiting) =
        s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
    else {
        panic!("the release wasn't admitted");
    };
    assert!(
        s.platform
            .withdraw(RAMP, &waiting.assertion_digest, "test-reviewer", AT)
            .is_some()
    );

    // The platform restarts, on its record and nothing else it held. It holds each release as it was: accepted,
    // in force, staged, counted, the third segment in flight, the ramp's version withdrawn; and no process.
    let world = stop(s);
    let mut s = Signed::on(Box::new(record.clone()), world);
    s.platform.set_staging(Some(policy));
    let lifecycle = s.platform.lifecycle();
    assert_eq!(lifecycle.in_force(RESERVE), Some("1.0.0"));
    assert_eq!(lifecycle.staged(RESERVE), Some("1.1.0"));
    assert!(lifecycle.is_accepted(RESERVE, "1.0.0") && lifecycle.is_accepted(RESERVE, "1.1.0"));
    assert_eq!(
        lifecycle
            .observation(RESERVE)
            .expect("staged")
            .staged_counts,
        counts
    );
    assert_eq!(
        lifecycle
            .segment(tx(RESERVE, 3).as_str())
            .map(|segment| segment.runs_on.as_str()),
        Some("1.1.0")
    );
    assert_eq!(s.platform.active_version(RESERVE), None);
    assert_eq!(
        s.platform
            .promote(RESERVE, "test-operator", Instant(AT.0 + 60)),
        Err(LifecycleRefusal::StagedNotRunning)
    );

    // Admitted again, in the order accepted, each release is accepted by whom it was and when, and goes where it
    // stood: neither is given an acceptance again, and the second isn't staged as a new upgrade.
    let Admitted::Artefact(again) = readmitted(&mut s, RESERVE, "1.0.0") else {
        panic!("the release wasn't admitted again");
    };
    assert_eq!(again.acceptance, first);
    assert!(s.platform.pending(RESERVE).is_none());
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    let Admitted::Artefact(again) = readmitted(&mut s, RESERVE, "1.1.0") else {
        panic!("the release wasn't admitted again");
    };
    assert_eq!(again.acceptance, second);
    assert_eq!(s.platform.lifecycle().in_force(RESERVE), Some("1.0.0"));
    assert_eq!(s.platform.lifecycle().staged(RESERVE), Some("1.1.0"));
    assert!(s.platform.version_process(RESERVE, "1.1.0").is_some());
    assert_eq!(
        s.platform
            .lifecycle()
            .observation(RESERVE)
            .expect("staged")
            .staged_counts,
        counts,
        "admitting again counts nothing"
    );
    // The withdrawn release stays withdrawn.
    assert_eq!(
        readmitted(&mut s, RAMP, "1.0.0"),
        Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted)
    );
    // The segment in flight runs on the staged release, and the staged release, whose two answers were counted
    // before the restart, is promoted on them once its window has passed.
    assert!(s.answers_as(RESERVE, "1.1.0", 3));
    assert_eq!(
        s.platform
            .promote(RESERVE, "test-operator", Instant(AT.0 + 60))
            .as_deref(),
        Ok("1.1.0")
    );
    // And the promotion, taken after the restart, is held by the next.
    let world = stop(s);
    let s = Signed::on(Box::new(record), world);
    assert_eq!(s.platform.lifecycle().in_force(RESERVE), Some("1.1.0"));
    assert_eq!(s.platform.lifecycle().staged(RESERVE), None);
}

/// A record that takes every write until the test breaks it, as a disk that fills leaves a [`FileRecord`].
struct Breakable {
    inner: MemoryRecord,
    broken: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Record for Breakable {
    fn load(&mut self) -> io::Result<Vec<String>> {
        self.inner.load()
    }

    fn append(&mut self, line: &str) -> io::Result<()> {
        if self.broken.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(io::Error::other("broken"));
        }
        self.inner.append(line)
    }

    fn replace(&mut self, lines: &[String]) -> io::Result<()> {
        self.inner.replace(lines)
    }
}

// verifies: REQ-MDL-4, INV-8
#[test]
fn a_release_the_record_cannot_hold_is_not_accepted() {
    let broken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let record = Breakable {
        inner: MemoryRecord::new(),
        broken: broken.clone(),
    };
    let mut s = Signed::on(Box::new(record), TestWorld::default());
    admit_version(&mut s, RESERVE, "1.0.0");
    assert_eq!(s.platform.lifecycle().in_force(RESERVE), Some("1.0.0"));

    // An automatic acceptance the record can't hold isn't given: the release waits, nothing runs it, and the
    // version in force stays.
    broken.store(true, std::sync::atomic::Ordering::SeqCst);
    let key = signed::catalogue_support::advertising_key(RESERVE);
    let (declaration, binaries) = release(RESERVE, "1.1.0", &["withdrawal"]);
    let Admitted::Artefact(artefact) =
        s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
    else {
        panic!("the release wasn't admitted");
    };
    assert_eq!(artefact.acceptance.accepted, None);
    assert!(s.platform.pending(RESERVE).is_some());
    assert!(!s.platform.lifecycle().is_accepted(RESERVE, "1.1.0"));
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    assert_eq!(s.platform.version_process(RESERVE, "1.1.0"), None);
    // A person's acceptance the record can't hold isn't given either.
    assert!(
        s.accept(RESERVE, &artefact.assertion_digest, "test-reviewer")
            .is_none()
    );
    assert!(s.platform.pending(RESERVE).is_some());
    assert!(!s.platform.lifecycle().is_accepted(RESERVE, "1.1.0"));
    assert_eq!(s.platform.version_process(RESERVE, "1.1.0"), None);
}

// verifies: REQ-MDL-4, INV-8
#[test]
fn accepted_releases_are_held_by_a_platform_restarted_at_once() {
    let policy = Staging::new(100, 60, 2).expect("a policy");
    let record = MemoryRecord::new();
    let mut s = Signed::on(Box::new(record.clone()), TestWorld::default());
    let first = admitted_as(&mut s, RESERVE, "1.0.0");
    s.platform.set_staging(Some(policy));
    let second = admitted_as(&mut s, RESERVE, "1.1.0");
    // Nothing else has been written since: the acceptances and where they went are all the record holds.
    let world = stop(s);
    let mut s = Signed::on(Box::new(record), world);
    s.platform.set_staging(Some(policy));
    let lifecycle = s.platform.lifecycle();
    assert_eq!(lifecycle.in_force(RESERVE), Some("1.0.0"));
    assert_eq!(lifecycle.staged(RESERVE), Some("1.1.0"));
    assert!(lifecycle.is_accepted(RESERVE, "1.0.0") && lifecycle.is_accepted(RESERVE, "1.1.0"));
    let Admitted::Artefact(again) = readmitted(&mut s, RESERVE, "1.0.0") else {
        panic!("the release wasn't admitted again");
    };
    assert_eq!(again.acceptance, first);
    let Admitted::Artefact(again) = readmitted(&mut s, RESERVE, "1.1.0") else {
        panic!("the release wasn't admitted again");
    };
    assert_eq!(again.acceptance, second);
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    assert_eq!(s.platform.lifecycle().staged(RESERVE), Some("1.1.0"));
}

// verifies: REQ-MDL-1, REQ-MDL-3, REQ-AUD-1
#[test]
fn a_different_release_under_an_accepted_version_is_refused_after_a_restart() {
    let record = MemoryRecord::new();
    let mut s = Signed::on(Box::new(record.clone()), TestWorld::default());
    admit_version(&mut s, RESERVE, "1.0.0");
    let world = stop(s);
    let mut s = Signed::on(Box::new(record), world);

    // Another assertion under the version a person accepted: it handles more, and is no release they saw.
    let key = signed::catalogue_support::advertising_key(RESERVE);
    let (declaration, binaries) = release(RESERVE, "1.0.0", &["withdrawal", "status_query"]);
    let other = s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries);
    assert_eq!(
        other,
        Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted)
    );
    assert!(s.platform.pending(RESERVE).is_none());
    assert_eq!(s.platform.active_version(RESERVE), None);
    // The release that was accepted is the one that is accepted again.
    let Admitted::Artefact(again) = readmitted(&mut s, RESERVE, "1.0.0") else {
        panic!("the release wasn't admitted again");
    };
    assert_eq!(
        again
            .acceptance
            .accepted
            .and_then(|a| a.by)
            .map(|by| by.as_str().to_owned()),
        Some("test-reviewer".to_owned())
    );
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
}

// verifies: REQ-MDL-20, INV-8
#[test]
fn an_exploit_declaration_the_record_could_not_hold_is_not_undone_by_a_restart() {
    let broken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let inner = MemoryRecord::new();
    let mut s = Signed::on(
        Box::new(Breakable {
            inner: inner.clone(),
            broken: broken.clone(),
        }),
        TestWorld::default(),
    );
    admit_version(&mut s, RESERVE, "1.0.0");
    broken.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        s.platform
            .declare_exploited(RESERVE, "1.0.0", "test-operator", AT),
        Err(LifecycleRefusal::TakenUnrecorded)
    );
    let world = stop(s);

    // The record doesn't know the version was declared exploited, and ended no way it can vouch for: nothing of the
    // provider runs, whatever is admitted again, until the operator takes the decision again and confirms.
    let mut s = Signed::on(Box::new(inner.clone()), world);
    assert_eq!(s.platform.lifecycle().mark(RESERVE, "1.0.0"), None);
    assert!(matches!(
        readmitted(&mut s, RESERVE, "1.0.0"),
        Admitted::Artefact(_)
    ));
    assert_eq!(s.platform.active_version(RESERVE), None);
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 5)),
        Handled::Refused(PlatformRefusal::Unavailable)
    );
    assert_eq!(
        s.platform.begin_segment(RESERVE, &tx(RESERVE, 5)),
        Err(PlatformRefusal::Unavailable)
    );
    s.platform
        .declare_exploited(RESERVE, "1.0.0", "test-operator", AT)
        .expect("declared again");
    s.platform
        .confirm(RESERVE, "test-operator", AT)
        .expect("confirmed");
    assert_eq!(s.platform.version_process(RESERVE, "1.0.0"), None);
    assert!(!matches!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 5)),
        Handled::Answered(_)
    ));
    let world = stop(s);
    // Confirmed and ended whole, it is held: the next restart asks nothing more.
    let s = Signed::on(Box::new(inner), world);
    assert_eq!(
        s.platform.lifecycle().mark(RESERVE, "1.0.0"),
        Some(Mark::Exploited)
    );
}

// verifies: REQ-MDL-20, INV-8
#[test]
fn releases_of_a_platform_that_ended_unclean_run_once_the_operator_confirms_them() {
    let broken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let inner = MemoryRecord::new();
    let mut s = Signed::on(
        Box::new(Breakable {
            inner: inner.clone(),
            broken: broken.clone(),
        }),
        TestWorld::default(),
    );
    admit_version(&mut s, RESERVE, "1.0.0");
    // A platform whose record failed ends with nothing of the closing written.
    broken.store(true, std::sync::atomic::Ordering::SeqCst);
    let world = stop(s);
    let mut s = Signed::on(Box::new(inner.clone()), world);
    assert!(matches!(
        readmitted(&mut s, RESERVE, "1.0.0"),
        Admitted::Artefact(_)
    ));
    assert_eq!(s.platform.active_version(RESERVE), None);
    // Nothing waits on a provider the record holds nothing of.
    assert_eq!(
        s.platform.confirm(RAMP, "test-operator", AT),
        Err(LifecycleRefusal::NoSuchVersion)
    );
    s.platform
        .confirm(RESERVE, "test-operator", AT)
        .expect("confirmed");
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    assert!(s.answers_as(RESERVE, "1.0.0", 7));
    assert!(
        s.platform
            .lifecycle()
            .decisions()
            .iter()
            .any(|d| d.kind == DecisionKind::Confirmed && d.by.as_str() == "test-operator")
    );
    // Confirmed and ended whole, the next restart asks nothing more of the operator.
    let world = stop(s);
    let mut s = Signed::on(Box::new(inner), world);
    assert!(matches!(
        readmitted(&mut s, RESERVE, "1.0.0"),
        Admitted::Artefact(_)
    ));
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
}

// verifies: REQ-MDL-3, REQ-MDL-20
#[test]
fn a_release_admitted_while_its_provider_waits_for_confirmation_starts_nothing() {
    let broken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let inner = MemoryRecord::new();
    let mut s = Signed::on(
        Box::new(Breakable {
            inner: inner.clone(),
            broken: broken.clone(),
        }),
        TestWorld::default(),
    );
    admit_version(&mut s, RESERVE, "1.0.0");
    broken.store(true, std::sync::atomic::Ordering::SeqCst);
    let world = stop(s);
    let mut s = Signed::on(Box::new(inner), world);
    assert!(matches!(
        readmitted(&mut s, RESERVE, "1.0.0"),
        Admitted::Artefact(_)
    ));
    // A new release is accepted and placed, and nothing runs it: no process, no operation, until the operator confirms.
    admit_version(&mut s, RESERVE, "1.1.0");
    assert!(s.platform.lifecycle().is_accepted(RESERVE, "1.1.0"));
    assert_eq!(s.platform.active_version(RESERVE), None);
    assert_eq!(s.platform.version_process(RESERVE, "1.1.0"), None);
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 3)),
        Handled::Refused(PlatformRefusal::Unavailable)
    );
    s.platform
        .confirm(RESERVE, "test-operator", AT)
        .expect("confirmed");
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.1.0"));
}

/// Confirming some of the held providers records each one confirmed (`T-020-49`, the reviewer's Low on `T-013-11`): a
/// platform that ends unclean again before the others are confirmed holds only those, and the ones confirmed run again
/// once admitted, with no second word from the operator. A provider confirmed only before an earlier opening, whose run
/// since then ended unclean too, is held again.
// verifies: NFR-6
#[test]
fn providers_confirmed_before_a_restart_are_not_held_again_after_it() {
    let broken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let inner = MemoryRecord::new();
    let mut s = Signed::on(
        Box::new(Breakable {
            inner: inner.clone(),
            broken: broken.clone(),
        }),
        TestWorld::default(),
    );
    admit_version(&mut s, RESERVE, "1.0.0");
    admit_version(&mut s, RAMP, "1.0.0");
    // The record fails: the platform ends with nothing of its closing written.
    broken.store(true, std::sync::atomic::Ordering::SeqCst);
    let world = stop(s);

    // Both are held; the operator confirms the reserve alone, and the platform stops again, the ramp still held, so
    // its record doesn't end whole.
    let mut s = Signed::on(Box::new(inner.clone()), world);
    for provider in [RESERVE, RAMP] {
        assert!(matches!(
            readmitted(&mut s, provider, "1.0.0"),
            Admitted::Artefact(_)
        ));
        assert_eq!(s.platform.active_version(provider), None, "{provider}");
        assert!(s.platform.awaiting_confirmation(provider).is_some());
    }
    assert_eq!(
        s.platform.awaiting_confirmation(RESERVE),
        Some(std::collections::BTreeSet::from(["1.0.0".to_owned()]))
    );
    s.platform
        .confirm(RESERVE, "test-operator", AT)
        .expect("confirmed");
    assert_eq!(s.platform.awaiting_confirmation(RESERVE), None);
    let world = stop(s);

    // After the restart, the reserve runs once admitted again; only the ramp waits.
    let mut s = Signed::on(Box::new(inner.clone()), world);
    for provider in [RESERVE, RAMP] {
        assert!(matches!(
            readmitted(&mut s, provider, "1.0.0"),
            Admitted::Artefact(_)
        ));
    }
    assert_eq!(s.platform.awaiting_confirmation(RESERVE), None);
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    assert!(s.answers_as(RESERVE, "1.0.0", 7));
    assert!(s.platform.awaiting_confirmation(RAMP).is_some());
    assert_eq!(s.platform.active_version(RAMP), None);
    // This run's record fails before it ends: it can't vouch for anything it took since, so the reserve, which ran in
    // it, is held again with the ramp.
    let world = stop(s);
    let s = Signed::on(
        Box::new(Breakable {
            inner: inner.clone(),
            broken: broken.clone(),
        }),
        world,
    );
    broken.store(true, std::sync::atomic::Ordering::SeqCst);
    let world = stop(s);
    let s = Signed::on(Box::new(inner), world);
    assert!(s.platform.awaiting_confirmation(RESERVE).is_some());
    assert!(s.platform.awaiting_confirmation(RAMP).is_some());
}

/// A restriction taken unrecorded after a confirmation holds after a second unclean end (`T-020-49`, H1 of #337's
/// review, its reproduction): the provider, held after an unclean end, is confirmed and runs; its record then fails and
/// its version is declared exploited in memory only (`TakenUnrecorded`); the platform ends. The next opening holds the
/// provider again, so nothing of the exploited version is back in force without the operator's word, which Core's
/// confirmation gives only after taking its restrictions again from its log.
// verifies: REQ-MDL-20, NFR-6
#[test]
fn unrecorded_restriction_after_a_confirmation_is_held_after_a_second_unclean_end() {
    let broken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let inner = MemoryRecord::new();
    let mut s = Signed::on(
        Box::new(Breakable {
            inner: inner.clone(),
            broken: broken.clone(),
        }),
        TestWorld::default(),
    );
    admit_version(&mut s, RESERVE, "1.0.0");
    broken.store(true, std::sync::atomic::Ordering::SeqCst);
    let world = stop(s);

    // Held, confirmed and running; then the record breaks and 1.0.0 is declared exploited unrecorded.
    broken.store(false, std::sync::atomic::Ordering::SeqCst);
    let mut s = Signed::on(
        Box::new(Breakable {
            inner: inner.clone(),
            broken: broken.clone(),
        }),
        world,
    );
    assert!(matches!(
        readmitted(&mut s, RESERVE, "1.0.0"),
        Admitted::Artefact(_)
    ));
    assert!(s.platform.awaiting_confirmation(RESERVE).is_some());
    s.platform
        .confirm(RESERVE, "test-operator", AT)
        .expect("confirmed");
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    broken.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        s.platform
            .declare_exploited(RESERVE, "1.0.0", "test-operator", AT),
        Err(LifecycleRefusal::TakenUnrecorded)
    );
    assert_eq!(s.platform.active_version(RESERVE), None);
    let world = stop(s);

    // The second unclean end: the provider is held, and 1.0.0 isn't in force.
    let mut s = Signed::on(Box::new(inner), world);
    let _ = readmitted(&mut s, RESERVE, "1.0.0");
    assert!(s.platform.awaiting_confirmation(RESERVE).is_some(), "held");
    assert_eq!(s.platform.active_version(RESERVE), None, "nothing exploited is back in force");
}
