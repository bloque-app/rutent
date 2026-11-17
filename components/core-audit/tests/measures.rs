//! The bulk measures and the regulator's read (`T-022-06`): per provider and verifier, quotes against executed
//! prices, dry runs against outcomes, failures against the market's direction, refusals, reversals and rates of no
//! (`REQ-AUD-7`), computed from what the log holds; and a regulator's read of the trail, held to policy's permission
//! and recorded (`REQ-AUD-12`).
//!
//! These tests run on the real log (`AuditLog`, on a memory store), so every entry written is admitted by the
//! citation table. The measures' own test over real runs, `each_measure_is_computed_from_simulated_runs`, and the
//! regulator's permitted read over the real policy engine, `regulators_permitted_read_is_recorded`, are in
//! `conformance/scenarios/tests/bulk_measures.rs`, where Core runs over the simulated providers.

use std::cell::RefCell;
use std::rc::Rc;

use rutent_conformance::signing::{public_key, signing_key};
use rutent_contracts::core_internal::calls::{
    AppendFailure, DecisionEntry, GeneralEntry, Log, Measures, PolicyLog, Reads as Policy,
};
use rutent_contracts::core_internal::{
    Decision, Entry, MeasureRecord, PermissionChange, ReadAnswer, ReadCheck, ReadOutcome, Receipt,
    VerifiedEntry, What,
};
use rutent_contracts::provider_role::Deployment;
use rutent_contracts::router_api::{Amount, Id, Timestamp};
use rutent_core_audit::log::Policy as PolicyHandle;
use rutent_core_audit::log::{AuditLog, Clock, General, KeptInMemory, MemoryStore, Settings};
use rutent_core_audit::measures::{
    COUNTS, Failure, Read, ReadChecks, Record, Report, Window, measure, read_trail,
};
use serde_json::{Value, json};

// --- Fixtures ----------------------------------------------------------------------------------------------------

fn valid(name: &str) -> Value {
    let path = format!(
        "{}/../../contracts/core-internal/fixtures/valid/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

fn id(text: &str) -> Id {
    Id::new(text).unwrap()
}

fn at(text: &str) -> Timestamp {
    Timestamp::new(text).unwrap()
}

const LOG: &str = "log-main";
const OPERATOR: &str = "operator";

/// The test's clock: a fixed instant in November 2026.
#[derive(Debug)]
struct TestClock;

impl Clock for TestClock {
    fn unix_nanos(&self) -> i128 {
        Timestamp::new("2026-11-10T09:00:00Z").unwrap().unix_nanos()
    }
}

fn settings() -> Settings {
    let key = signing_key("the operator of rutent-test", 0);
    let deployment = Deployment {
        name: "rutent-test".to_owned(),
        operator_key: public_key(&key),
    };
    Settings::new(id(LOG), deployment, id(OPERATOR), id(OPERATOR)).on(Rc::new(TestClock))
}

type Shared = Rc<RefCell<AuditLog<MemoryStore, KeptInMemory>>>;

/// A log holding nothing.
fn bare() -> Shared {
    Rc::new(RefCell::new(
        AuditLog::open(settings(), MemoryStore::new(), KeptInMemory::new()).unwrap(),
    ))
}

type Writer = Record<AuditLog<MemoryStore, KeptInMemory>, TestClock>;

fn writer(log: &Shared) -> Writer {
    Record::new(
        Writer::core_audit(id("rutent")),
        General::new(log),
        TestClock,
    )
}

/// Appends an entry a measure record can cite: a holding account's check, which rests on nothing (`T-022-05`). The
/// `n`th is a different entry from the others. Answers its receipt.
fn citable(log: &Shared, n: usize) -> Receipt {
    let entry: VerifiedEntry = serde_json::from_value(json!({
        "kind": "verified",
        "who": {"party": "rutent", "acts": "core-audit", "role": "core", "kind": "component"},
        "what": {
            "call": "holding_check",
            "account": format!("hold-{n}"),
            "provider": "fx-desk",
            "node": "ledger-1",
            "word": "checked",
            "orders": []
        },
        "when": {"at": "2026-11-10T09:00:00Z", "source": "core_clock"},
        "outcome": {"status": "observed"}
    }))
    .unwrap();
    let entry = GeneralEntry::try_from(Entry::Verified(entry)).unwrap();
    log.borrow_mut().append(&entry).unwrap()
}

/// A measure record of `provider`, at `time`, citing `cite`.
fn record(provider: &str, time: &str, measure: Value, cite: &Receipt) -> MeasureRecord {
    serde_json::from_value(json!({
        "provider": provider,
        "order": "ord-1",
        "segment": "seg-1",
        "measure": measure,
        "cites": [cite],
        "at": time
    }))
    .unwrap()
}

fn quote(quoted: &str, executed: &str, asset: &str, decimals: u8) -> Value {
    json!({"kind": "quote", "quoted": quoted, "executed": executed,
           "asset": {"id": asset, "decimals": decimals}})
}

fn dry_run(said: &str, outcome: &str) -> Value {
    json!({"kind": "dry_run", "said": said, "outcome": outcome})
}

fn failure(market: Option<&str>) -> Value {
    match market {
        Some(m) => json!({"kind": "failure", "market": m}),
        None => json!({"kind": "failure"}),
    }
}

fn answer(said: &str) -> Value {
    json!({"kind": "answer", "said": said})
}

fn window(from: &str, until: &str) -> Window {
    Window {
        from: at(from),
        until: at(until),
    }
}

/// The whole of November 2026.
fn november() -> Window {
    window("2026-11-01T00:00:00Z", "2026-12-01T00:00:00Z")
}

fn report(log: &Shared, over: &Window) -> Report {
    measure(log.borrow().entries(), over)
}

// --- REQ-AUD-7 -----------------------------------------------------------------------------------------------------

/// Every measure of `REQ-AUD-7`, per provider and per verifier, read from the log's own records over a window:
/// quotes against executed prices (per asset, below, at and above, and by how much), dry runs against outcomes,
/// failures against the market's direction and with no figure, refusals, reversals and the rate of no. Two providers
/// are kept apart, and a record outside the window, or an entry that is no measure, counts for nothing.
// verifies: REQ-AUD-7
#[test]
fn bulk_measures_are_read_from_the_logs_own_records() {
    let log = bare();
    let cite = citable(&log, 0);
    let mut write = writer(&log);
    let day = |d: u32| format!("2026-11-{d:02}T12:00:00Z");
    let records = [
        // fx-desk's quotes, in two assets: below twice (by 12 500 and 500), at once, above once (by 100).
        record(
            "fx-desk",
            &day(2),
            quote("2500000", "2487500", "COP", 2),
            &cite,
        ),
        record("fx-desk", &day(3), quote("1000", "500", "COP", 2), &cite),
        record("fx-desk", &day(4), quote("700", "700", "COP", 2), &cite),
        record("fx-desk", &day(5), quote("900", "1000", "COP", 2), &cite),
        // The same code in other decimals is another asset.
        record("fx-desk", &day(6), quote("5", "4", "COP", 0), &cite),
        // Dry runs.
        record("fx-desk", &day(7), dry_run("yes", "done"), &cite),
        record("fx-desk", &day(7), dry_run("yes", "failed"), &cite),
        record("fx-desk", &day(8), dry_run("yes", "failed"), &cite),
        record("fx-desk", &day(8), dry_run("no", "done"), &cite),
        record("fx-desk", &day(9), dry_run("no", "failed"), &cite),
        // Failures, by the market's direction.
        record(
            "fx-desk",
            &day(10),
            failure(Some("with_the_provider")),
            &cite,
        ),
        record(
            "fx-desk",
            &day(16),
            failure(Some("with_the_provider")),
            &cite,
        ),
        record(
            "fx-desk",
            &day(11),
            failure(Some("against_the_provider")),
            &cite,
        ),
        record("fx-desk", &day(12), failure(Some("flat")), &cite),
        record("fx-desk", &day(13), failure(None), &cite),
        // Refusals, reversals.
        record("fx-desk", &day(14), json!({"kind": "refusal"}), &cite),
        record("fx-desk", &day(15), json!({"kind": "refusal"}), &cite),
        record("fx-desk", &day(15), json!({"kind": "reversal"}), &cite),
        // Another provider, and a verifier's answers.
        record("us-bank", &day(2), quote("100", "100", "USD", 2), &cite),
        record("us-bank", &day(3), failure(None), &cite),
        record("kyc-check", &day(4), answer("yes"), &cite),
        record("kyc-check", &day(5), answer("no"), &cite),
        record("kyc-check", &day(6), answer("no"), &cite),
        record("kyc-check", &day(7), answer("no"), &cite),
        // Outside the window: before it, and at its end.
        record("fx-desk", "2026-10-31T23:59:59Z", failure(None), &cite),
        record("fx-desk", "2026-12-01T00:00:00Z", failure(None), &cite),
    ];
    for r in &records {
        write.measure(r).expect("recorded");
    }
    let report = report(&log, &november());
    assert_eq!(report.window, november());
    assert_eq!(
        report.counted, 24,
        "the two outside the window aren't counted"
    );
    let providers: Vec<_> = report.providers.keys().map(String::as_str).collect();
    assert_eq!(providers, ["fx-desk", "kyc-check", "us-bank"]);

    let fx = &report.providers["fx-desk"];
    let cop2 = &fx.quotes[&("COP".to_owned(), 2)];
    assert_eq!(
        (
            cop2.quotes,
            cop2.executed_below,
            cop2.executed_at,
            cop2.executed_above
        ),
        (4, 2, 1, 1)
    );
    assert_eq!(
        cop2.below_by,
        Some(Amount::new("13000").unwrap()),
        "12 500 and 500"
    );
    assert_eq!(cop2.above_by, Some(Amount::new("100").unwrap()));
    assert_eq!(cop2.rate_below().per_million(), Some(500_000));
    let cop0 = &fx.quotes[&("COP".to_owned(), 0)];
    assert_eq!(
        (cop0.quotes, cop0.executed_below),
        (1, 1),
        "another unit, another asset"
    );
    assert_eq!(fx.quotes.len(), 2);
    assert_eq!(
        (
            fx.dry_runs.yes_then_done,
            fx.dry_runs.yes_then_failed,
            fx.dry_runs.no_then_done,
            fx.dry_runs.no_then_failed
        ),
        (1, 2, 1, 1)
    );
    assert_eq!(fx.dry_runs.yes_that_failed().per_million(), Some(666_666));
    assert_eq!(
        (
            fx.failures.with_the_provider,
            fx.failures.against_the_provider,
            fx.failures.flat,
            fx.failures.no_figure
        ),
        (2, 1, 1, 1)
    );
    assert_eq!(fx.failures.all(), 5);
    assert_eq!((fx.refusals, fx.reversals), (2, 1));
    assert_eq!((fx.answers.yes, fx.answers.no), (0, 0));

    let kyc = &report.providers["kyc-check"];
    assert_eq!((kyc.answers.yes, kyc.answers.no), (1, 3));
    assert_eq!(kyc.answers.rate_of_no().per_million(), Some(750_000));
    assert!(kyc.quotes.is_empty() && kyc.failures.all() == 0);

    let bank = &report.providers["us-bank"];
    assert_eq!(bank.quotes[&("USD".to_owned(), 2)].executed_at, 1);
    assert_eq!((bank.failures.no_figure, bank.refusals), (1, 0));

    // A narrower window counts fewer, over the same log.
    let narrow = report_of(
        &log,
        &window("2026-11-02T00:00:00Z", "2026-11-04T00:00:00Z"),
    );
    assert_eq!(
        narrow.counted, 4,
        "fx-desk's two and us-bank's two, by the 2nd and 3rd"
    );
    // The measures of an empty window, or one that ends before it starts, are empty.
    for empty in [
        window("2027-01-01T00:00:00Z", "2027-02-01T00:00:00Z"),
        window("2026-11-20T00:00:00Z", "2026-11-10T00:00:00Z"),
    ] {
        let none = report_of(&log, &empty);
        assert_eq!((none.counted, none.providers.len()), (0, 0));
    }
    // Each measure says what it counts.
    assert_eq!(COUNTS.len(), 6);
    assert!(
        COUNTS
            .iter()
            .all(|(name, says)| !name.is_empty() && says.len() > 20)
    );
    // The measure's only input is the log's entries and a window: it coerces to a plain function of the two.
    let _: fn(&[(Receipt, Entry)], &Window) -> Report = measure;
}

fn report_of(log: &Shared, over: &Window) -> Report {
    report(log, over)
}

/// A party that is both a provider and a verifier is one key: its quotes and its answers land in one entry of the
/// report and aren't told apart by role. The record carries one `provider` id and no role, so the limit is visible
/// here (a role field is `T-020-30`'s to list, the next `core-internal` round).
// verifies: REQ-AUD-7
#[test]
fn a_party_in_both_roles_is_one_key_in_the_report() {
    let log = bare();
    let cite = citable(&log, 0);
    let mut write = writer(&log);
    write
        .measure(&record(
            "both",
            "2026-11-02T00:00:00Z",
            quote("10", "9", "COP", 2),
            &cite,
        ))
        .unwrap();
    write
        .measure(&record("both", "2026-11-03T00:00:00Z", answer("no"), &cite))
        .unwrap();
    let report = report(&log, &november());
    assert_eq!(report.providers.len(), 1, "one key for the party");
    let both = &report.providers["both"];
    assert_eq!(
        both.quotes[&("COP".to_owned(), 2)].quotes,
        1,
        "as a provider"
    );
    assert_eq!(both.answers.no, 1, "as a verifier, in the same entry");
}

/// A window reads a time by the instant, its fraction to the nanosecond, not by its text, and is half open.
// verifies: REQ-AUD-7
#[test]
fn a_window_reads_times_by_the_instant_and_is_half_open() {
    let log = bare();
    let cite = citable(&log, 0);
    let mut write = writer(&log);
    for time in [
        "2026-11-10T09:00:00Z",
        "2026-11-10T09:00:00.000000000Z",
        "2026-11-10T09:00:00.5Z",
        "2026-11-10T08:59:59.999999999Z",
        "2026-11-10T09:00:00.000000001Z",
        "2026-11-10T09:00:01Z",
        "2024-02-29T23:59:59Z",
        "1969-12-31T23:59:59Z",
    ] {
        let mut r = record("fx-desk", time, json!({"kind": "refusal"}), &cite);
        r.order = id(&format!("ord-{time}"));
        write.measure(&r).unwrap();
    }
    let count = |from: &str, until: &str| report(&log, &window(from, until)).counted;
    // [09:00:00, 09:00:01): the two ways of writing 09:00:00, the half second and the nanosecond after.
    assert_eq!(count("2026-11-10T09:00:00Z", "2026-11-10T09:00:01Z"), 4);
    // The end isn't in it: up to 09:00:00 holds only the nanosecond before it.
    assert_eq!(count("2026-11-10T00:00:00Z", "2026-11-10T09:00:00Z"), 1);
    // The second after the window's end is in the next one.
    assert_eq!(count("2026-11-10T09:00:01Z", "2026-11-10T09:00:02Z"), 1);
    // Dates before the epoch and on a leap day are read as the dates they are.
    assert_eq!(count("2024-02-29T00:00:00Z", "2024-03-01T00:00:00Z"), 1);
    assert_eq!(count("1969-12-31T00:00:00Z", "1970-01-01T00:00:00Z"), 1);
}

/// A total past what an amount states is said so, never wrapped or understated, and the counts beside it stay right.
// verifies: REQ-AUD-7
#[test]
fn a_total_past_the_bound_is_not_stated_and_the_counts_stay_right() {
    let log = bare();
    let cite = citable(&log, 0);
    let mut write = writer(&log);
    let nines = "9".repeat(78);
    for (n, executed) in [(1, "1"), (2, "1")] {
        let mut r = record(
            "fx-desk",
            "2026-11-02T00:00:00Z",
            quote(&nines, executed, "COP", 2),
            &cite,
        );
        r.order = id(&format!("ord-{n}"));
        write.measure(&r).unwrap();
    }
    let report = report(&log, &november());
    let q = &report.providers["fx-desk"].quotes[&("COP".to_owned(), 2)];
    assert_eq!((q.quotes, q.executed_below), (2, 2));
    assert_eq!(
        q.below_by, None,
        "two shortfalls of 10^78 - 2 pass 78 digits"
    );
    assert_eq!(q.above_by, Some(Amount::zero()));
}

/// A measure record that cites nothing, or an entry the log doesn't hold, is not recorded, and nothing is appended.
// verifies: REQ-AUD-7, REQ-AUD-2
#[test]
fn a_measure_record_with_nothing_behind_it_is_not_recorded() {
    let log = bare();
    let cite = citable(&log, 0);
    let mut write = writer(&log);
    let before = log.borrow().len();
    let mut none = record("fx-desk", "2026-11-02T00:00:00Z", failure(None), &cite);
    none.cites.clear();
    assert!(matches!(
        write.measure(&none),
        Err(AppendFailure::Refused(_))
    ));
    let mut absent = record("fx-desk", "2026-11-02T00:00:00Z", failure(None), &cite);
    absent.cites[0].index = rutent_contracts::provider_role::SafeInteger::try_from(40u64).unwrap();
    assert!(matches!(
        write.measure(&absent),
        Err(AppendFailure::Refused(_))
    ));
    let mut twice = record("fx-desk", "2026-11-02T00:00:00Z", failure(None), &cite);
    twice.cites.push(cite.clone());
    assert!(matches!(
        write.measure(&twice),
        Err(AppendFailure::Refused(_))
    ));
    assert_eq!(log.borrow().len(), before, "nothing was appended");
    assert_eq!(report(&log, &november()).counted, 0);
    // A record written twice is one entry, so it is counted once.
    let fine = record("fx-desk", "2026-11-02T00:00:00Z", failure(None), &cite);
    let first = write.measure(&fine).unwrap();
    assert_eq!(write.measure(&fine), Ok(first));
    assert_eq!(report(&log, &november()).counted, 1);
}

/// What the writer put in the log is a verified entry of Core's own, `measure`, observed, by Core's clock, and holds
/// no secret and no balance (`REQ-AUD-6`): its JSON has only the record's own fields.
// verifies: REQ-AUD-6, REQ-AUD-7
#[test]
fn a_measure_entry_holds_only_the_record() {
    let log = bare();
    let cite = citable(&log, 0);
    let mut write = writer(&log);
    let r = record(
        "fx-desk",
        "2026-11-02T00:00:00Z",
        quote("10", "9", "COP", 2),
        &cite,
    );
    let receipt = write.measure(&r).unwrap();
    let log = log.borrow();
    let (_, entry) = &log.entries()[usize::try_from(receipt.index.get()).unwrap()];
    let Entry::Verified(v) = entry else {
        panic!("a verified entry")
    };
    assert_eq!(v.who.acts.as_str(), "core-audit");
    assert_eq!(
        serde_json::to_value(v.outcome).unwrap(),
        json!({"status": "observed"})
    );
    assert_eq!(serde_json::to_value(v.when.source).unwrap(), "core_clock");
    let json = serde_json::to_value(&v.what).unwrap();
    let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["call", "record"]);
    let mut record_keys: Vec<_> = json["record"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    record_keys.sort();
    assert_eq!(
        record_keys,
        ["at", "cites", "measure", "order", "provider", "segment"]
    );
}

// --- REQ-AUD-12 ----------------------------------------------------------------------------------------------------

/// What the stand-in for policy does with the next read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Does {
    /// Records the read allowed, and answers it so.
    Allow,
    /// Records the read refused, and answers it so.
    Refuse,
    /// Answers allowed, and records nothing: the receipt is of an entry that isn't the read's.
    AnswerAllowedUnrecorded,
    /// Records the read refused and answers it allowed.
    RecordRefusedAnswerAllowed,
    /// Records the read allowed, of another reader, and answers with it.
    RecordAnother,
    /// Answers allowed with a receipt of an entry the log doesn't hold.
    AnswerNoEntry,
    /// Answers another request.
    Astray,
    /// Records the read allowed, and another entry reaches the log before the answer is read.
    AllowThenAnother,
}

/// Policy's read check in a test: it records each read on policy's handle, as the engine does, or misbehaves as
/// asked, and counts what it was asked.
struct Stand {
    log: PolicyHandle<AuditLog<MemoryStore, KeptInMemory>>,
    shared: Shared,
    does: Does,
    asked: Rc<std::cell::Cell<usize>>,
}

impl Stand {
    /// The stand-in, after the operator granted the regulator its permission over the trail (the contract's own
    /// entry, which the log admits for the permitted reads below to rest on).
    fn new(log: &Shared, does: Does) -> Self {
        let mut stand = Stand {
            log: PolicyHandle::new(log),
            shared: Rc::clone(log),
            does,
            asked: Rc::default(),
        };
        let grant: VerifiedEntry =
            serde_json::from_value(valid("verified-entry-of-a-permission-granted")).unwrap();
        stand
            .log
            .append_decision(&DecisionEntry::try_from(grant).unwrap())
            .unwrap();
        stand
    }

    fn recording(&mut self, check: &ReadCheck, outcome: Value) -> Receipt {
        let entry: VerifiedEntry = serde_json::from_value(json!({
            "kind": "verified",
            "who": check.reader,
            "what": {"call": "check_read", "check": check},
            "when": {"at": check.at, "source": "core_clock"},
            "outcome": outcome
        }))
        .unwrap();
        self.log
            .append_decision(&DecisionEntry::try_from(entry).unwrap())
            .unwrap()
    }
}

impl Policy for Stand {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        self.asked.set(self.asked.get() + 1);
        let refused = json!({"status": "refused", "class": "not_permitted"});
        let permitted = json!({"status": "permitted"});
        let (outcome, receipt, request) = match self.does {
            Does::Allow => (
                ReadOutcome::Allowed {},
                self.recording(check, permitted),
                check.request.clone(),
            ),
            Does::Refuse => (
                ReadOutcome::Refused {
                    class: rutent_contracts::core_internal::RefusalClass::NotPermitted,
                },
                self.recording(check, refused),
                check.request.clone(),
            ),
            Does::AnswerAllowedUnrecorded => (
                ReadOutcome::Allowed {},
                self.shared.borrow().entries()[0].0.clone(),
                check.request.clone(),
            ),
            Does::RecordRefusedAnswerAllowed => (
                ReadOutcome::Allowed {},
                self.recording(check, refused),
                check.request.clone(),
            ),
            Does::RecordAnother => {
                let mut other = check.clone();
                other.reader.acts = id("examiner-9");
                (
                    ReadOutcome::Allowed {},
                    self.recording(&other, permitted),
                    check.request.clone(),
                )
            }
            Does::AnswerNoEntry => {
                let mut receipt = self.shared.borrow().entries()[0].0.clone();
                receipt.index =
                    rutent_contracts::provider_role::SafeInteger::try_from(900u64).unwrap();
                (ReadOutcome::Allowed {}, receipt, check.request.clone())
            }
            Does::AllowThenAnother => {
                let receipt = self.recording(check, permitted);
                citable(&self.shared, 1000);
                (ReadOutcome::Allowed {}, receipt, check.request.clone())
            }
            Does::Astray => (
                ReadOutcome::Allowed {},
                self.recording(check, permitted),
                id("another-read"),
            ),
        };
        Ok(ReadAnswer {
            request,
            outcome,
            receipt,
        })
    }

    fn grant_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn revoke_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}

fn trail_check(request: &str) -> ReadCheck {
    let mut check: ReadCheck =
        serde_json::from_value(valid("read-check-by-a-regulator-of-the-whole-trail")).unwrap();
    check.request = id(request);
    check
}

fn history(log: &Shared) -> impl FnOnce() -> Vec<(Receipt, Entry)> + '_ {
    move || log.borrow().entries().to_vec()
}

/// A regulator's read of the trail that policy allowed is served only once the log holds it recorded: the trail up to
/// and including the record of the read, which names who read what and when, and holds nothing `REQ-AUD-6` forbids.
// verifies: REQ-AUD-12, REQ-AUD-6
#[test]
fn a_trail_read_is_served_once_the_log_holds_it_recorded() {
    let log = bare();
    let cite = citable(&log, 0);
    writer(&log)
        .measure(&record(
            "fx-desk",
            "2026-11-02T00:00:00Z",
            failure(None),
            &cite,
        ))
        .unwrap();
    let mut policy = Stand::new(&log, Does::Allow);
    let held = log.borrow().len();
    let check = trail_check("req-trail-1");
    let Ok(Read::Released(trail)) = read_trail(&mut policy, &check, history(&log)) else {
        panic!("the trail is released")
    };
    assert_eq!(
        trail.entries.len(),
        held + 1,
        "what the log held, and the record of the read"
    );
    assert_eq!(trail.entries.last().unwrap().0, trail.read);
    // The record names who read what and when, and is the only entry the read added.
    assert_eq!(log.borrow().len(), held + 1);
    let Entry::Verified(recorded) = &trail.entries.last().unwrap().1 else {
        panic!("verified")
    };
    assert_eq!(recorded.who, check.reader);
    assert_eq!(recorded.when.at, check.at);
    let json = serde_json::to_value(&recorded.what).unwrap();
    assert_eq!(json["call"], "check_read");
    let mut keys: Vec<_> = json["check"].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["at", "permission", "reader", "reads", "request"],
        "no balance, no secret, an identity by reference"
    );
    assert_eq!(json["check"]["reads"], json!({"kind": "trail"}));
    assert_eq!(
        recorded.outcome,
        rutent_contracts::core_internal::Outcome::Permitted {}
    );
    // The entries released are the log's own, each with its receipt.
    assert_eq!(trail.entries, log.borrow().entries().to_vec());
    // A later entry isn't in a trail read before it.
    citable(&log, 1);
    assert_eq!(trail.entries.len(), held + 1);
}

/// A trail ends at the record of its own read: an entry the log took after it isn't in it, whoever wrote it.
// verifies: REQ-AUD-12
#[test]
fn a_trail_ends_at_the_record_of_its_own_read() {
    let log = bare();
    citable(&log, 0);
    let mut policy = Stand::new(&log, Does::AllowThenAnother);
    let held = log.borrow().len();
    let Ok(Read::Released(trail)) =
        read_trail(&mut policy, &trail_check("req-trail-6"), history(&log))
    else {
        panic!("the trail is released")
    };
    assert_eq!(
        log.borrow().len(),
        held + 2,
        "the log took another entry after the read's record"
    );
    assert_eq!(
        trail.entries.len(),
        held + 1,
        "the trail ends at the read's own record"
    );
    assert_eq!(trail.entries.last().unwrap().0, trail.read);
}

/// A trail read policy refused is refused and recorded, and the regulator learns nothing of the trail: the answer is
/// policy's, with no entry in it.
// verifies: REQ-AUD-12
#[test]
fn a_refused_trail_read_is_recorded_and_releases_nothing() {
    let log = bare();
    citable(&log, 0);
    let mut policy = Stand::new(&log, Does::Refuse);
    let held = log.borrow().len();
    let check = trail_check("req-trail-2");
    let Ok(Read::Refused(answer)) = read_trail(&mut policy, &check, history(&log)) else {
        panic!("refused")
    };
    assert!(matches!(answer.outcome, ReadOutcome::Refused { .. }));
    assert_eq!(log.borrow().len(), held + 1, "the refusal is recorded");
    let log = log.borrow();
    let (receipt, Entry::Verified(v)) = &log.entries()[held] else {
        panic!("verified")
    };
    assert_eq!(receipt, &answer.receipt);
    assert!(matches!(&v.what, What::CheckRead { check: c } if c == &check));
    assert!(matches!(
        v.outcome,
        rutent_contracts::core_internal::Outcome::Refused { .. }
    ));
}

/// An answer the log doesn't bear out serves nothing: allowed with no record of the read, with the record of a
/// refusal, of another reader's read, or of an entry the log doesn't hold, and one to another request.
// verifies: REQ-AUD-12
#[test]
fn an_answer_with_no_record_behind_it_releases_nothing() {
    for (does, why) in [
        (Does::AnswerAllowedUnrecorded, Failure::Unrecorded),
        (Does::RecordRefusedAnswerAllowed, Failure::Unrecorded),
        (Does::RecordAnother, Failure::Unrecorded),
        (Does::AnswerNoEntry, Failure::Unrecorded),
        (Does::Astray, Failure::AnswerOfAnother),
    ] {
        let log = bare();
        citable(&log, 0);
        let mut policy = Stand::new(&log, does);
        let read = read_trail(&mut policy, &trail_check("req-trail-3"), history(&log));
        assert_eq!(read, Err(why.clone()));
    }
}

/// The trail's reader serves the trail and nothing else, and doesn't ask policy about anything else.
// verifies: REQ-AUD-12
#[test]
fn only_a_read_of_the_trail_is_served_here() {
    let log = bare();
    citable(&log, 0);
    let mut policy = Stand::new(&log, Does::Allow);
    let held = log.borrow().len();
    let asked = Rc::clone(&policy.asked);
    for reads in [
        json!({"kind": "order", "order": "ord-1"}),
        json!({"kind": "resource", "resource": "checker"}),
    ] {
        let mut check = trail_check("req-trail-4");
        check.reads = serde_json::from_value(reads).unwrap();
        assert_eq!(
            read_trail(&mut policy, &check, history(&log)),
            Err(Failure::NotTheTrail)
        );
    }
    assert_eq!(asked.get(), 0, "policy was asked of nothing");
    assert_eq!(log.borrow().len(), held, "and nothing was recorded");
}

/// Policy can't answer: the log is where it was, nothing is served, and the failure is policy's.
// verifies: REQ-AUD-12
#[test]
fn a_trail_read_policy_cannot_answer_is_not_served() {
    struct Down;
    impl Policy for Down {
        fn check_read(&mut self, _: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
        fn grant_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
        fn revoke_permission(&mut self, _: &PermissionChange) -> Result<Decision, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
    }
    let log = bare();
    let read = read_trail(&mut Down, &trail_check("req-trail-5"), history(&log));
    assert_eq!(read, Err(Failure::Policy(AppendFailure::Unavailable)));
    // What the reader can ask of policy is a read check, and nothing else.
    fn asks<T: ReadChecks>(_: &T) {}
    asks(&Down);
}

// --- Structure: the measures reach nothing that moves value --------------------------------------------------------

/// The pure measure module. It may import the contracts' data types, and nothing else.
const PURE: [&str; 1] = ["compute"];

/// `source` without its comment lines, its whitespace folded to single spaces, so a declaration split across lines
/// reads as one, and none left round a path's `::`, a method's `.` or before a `(`, so `super :: x` and
/// `o . raise (r)` read as `super::x` and `o.raise(r)`.
fn flat_code(source: &str) -> String {
    let mut flat = source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for (spaced, tight) in [
        (" ::", "::"),
        (":: ", "::"),
        (" .", "."),
        (". ", "."),
        (" (", "("),
    ] {
        while flat.contains(spaced) {
            flat = flat.replace(spaced, tight);
        }
    }
    flat
}

/// Whether `c` can be part of an identifier.
fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Every place `word` stands as a whole word in `code`.
fn words<'a>(code: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    code.match_indices(word)
        .map(|(at, _)| at)
        .filter(move |&at| {
            let before = code[..at].chars().next_back();
            let after = code[at + word.len()..].chars().next();
            !before.is_some_and(ident) && !after.is_some_and(ident)
        })
}

/// Each `use` declaration of `code`, whatever its visibility and wherever it stands, whole up to its `;`, without
/// the keyword.
fn use_declarations(code: &str) -> Vec<String> {
    words(code, "use")
        .map(|at| {
            let rest = code[at + 3..].trim_start();
            rest[..rest.find(';').unwrap_or(rest.len())].to_owned()
        })
        .collect()
}

/// The first segment of each path `tree` names after `super::`: its own, or that of each top-level item of a braced
/// group.
fn sibling_heads(tree: &str) -> Vec<String> {
    let head = |item: &str| -> String { item.trim().chars().take_while(|c| ident(*c)).collect() };
    let tree = tree.trim();
    let Some(group) = tree.strip_prefix('{') else {
        return vec![head(tree)];
    };
    let (mut heads, mut item, mut depth) = (Vec::new(), String::new(), 0);
    for c in group.chars() {
        match c {
            '}' if depth == 0 => break,
            ',' if depth == 0 => heads.push(head(&std::mem::take(&mut item))),
            '{' => {
                depth += 1;
                item.push(c);
            }
            '}' => {
                depth -= 1;
                item.push(c);
            }
            _ => item.push(c),
        }
    }
    heads.push(head(&item));
    heads.into_iter().filter(|h| !h.is_empty()).collect()
}

/// Why `code`, a pure module's, could reach something that moves value, if it could: each reason found.
fn reach_of(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    for declaration in use_declarations(code) {
        let path = declaration.trim_start_matches("::");
        if let Some(tree) = path.strip_prefix("super::") {
            for head in sibling_heads(tree) {
                if !PURE.contains(&head.as_str()) {
                    found.push(format!(
                        "imports super::{head}, not a pure sibling: use {declaration}"
                    ));
                }
            }
        } else if !(path.starts_with("rutent_contracts::")
            || path.starts_with("std::collections::")
            || path.starts_with("std::cmp::"))
        {
            found.push(format!(
                "imports outside the contracts and its siblings: use {declaration}"
            ));
        }
    }
    // A capability, imported or named in place.
    if code.contains("calls::") || words(code, "calls").next().is_some() {
        found.push("names the contract's calls".to_owned());
    }
    for capability in [
        "Log",
        "Operator",
        "Clock",
        "Comparisons",
        "Measures",
        "Reads",
    ] {
        if words(code, capability).next().is_some() {
            found.push(format!("names {capability}"));
        }
    }
    // A child module, in a file of its own or inline, which the reader would not see.
    if words(code, "mod").next().is_some() {
        found.push("declares a module".to_owned());
    }
    // A path written in place, outside any `use`.
    for at in words(code, "super") {
        let rest = &code[at..];
        if rest.starts_with("super::super") {
            found.push("names super::super".to_owned());
        } else if let Some(tree) = rest.strip_prefix("super::")
            && !tree.starts_with('{')
        {
            let head: String = tree.chars().take_while(|c| ident(*c)).collect();
            if !PURE.contains(&head.as_str()) {
                found.push(format!("names super::{head}, not a pure sibling"));
            }
        }
    }
    for (at, _) in code.match_indices("rutent_") {
        let name: String = code[at..].chars().take_while(|c| ident(*c)).collect();
        if name != "rutent_contracts" {
            found.push(format!("names the crate {name}"));
        }
    }
    for forbidden in [
        "&mut",
        "unsafe",
        "static ",
        "Cell",
        "Rc<",
        "Arc<",
        "Mutex",
        "dyn ",
        "Box<",
        "thread_local",
        "std::fs",
        "std::net",
        "std::process",
        "std::env",
        "std::time",
        "crate::",
        "extern crate",
        "include!",
        "include_str!",
        "#[path",
        "fn raise",
        ".raise(",
        ".append(",
    ] {
        if code.contains(forbidden) {
            found.push(format!("holds {forbidden:?}"));
        }
    }
    found
}

/// The measure's pure module reaches nothing that moves value, by what it imports and what it can hold: data types of
/// the contracts, shared references, no handle, no channel, no clock and no interior mutability. Every `use` is read
/// whole, `pub` or not, across its lines; a path written in place is read too; no other crate of the workspace is
/// named; the crate takes no dependency on orchestration, so there is no path to the state machine or the provider
/// system from it. The writer of the records and the trail's reader are the module's only parts with a capability,
/// and their types are their reach: a general handle on the log and a clock; a read check of policy's, and no more.
// verifies: REQ-AUD-7
#[test]
fn measure_modules_reach_nothing_that_moves_value() {
    let _: fn(&[(Receipt, Entry)], &Window) -> Report = measure;
    let manifest = env!("CARGO_MANIFEST_DIR");
    for file in PURE {
        let path = format!("{manifest}/src/measures/{file}.rs");
        let code = flat_code(&std::fs::read_to_string(&path).unwrap());
        let reach = reach_of(&code);
        assert!(reach.is_empty(), "{file}: {}", reach.join("; "));
        let children = format!("{manifest}/src/measures/{file}");
        assert!(
            !std::path::Path::new(&children).exists(),
            "{file} has child modules at {children}"
        );
    }
    // The writer and the trail's reader have capabilities by design, but no child module hides in them or beside
    // them, and no file is pulled in by a path.
    for file in ["record", "trail"] {
        let code = flat_code(
            &std::fs::read_to_string(format!("{manifest}/src/measures/{file}.rs")).unwrap(),
        );
        assert!(
            words(&code, "mod").next().is_none(),
            "{file} declares a module"
        );
        for forbidden in ["include!", "include_str!", "#[path", "extern crate"] {
            assert!(!code.contains(forbidden), "{file} holds {forbidden:?}");
        }
        let children = format!("{manifest}/src/measures/{file}");
        assert!(
            !std::path::Path::new(&children).exists(),
            "{file} has child modules at {children}"
        );
    }
    // The reader itself is red on the ways round it, and on the plain ones.
    for (dodge, sneak) in [
        (
            "calls renamed",
            "use rutent_contracts::core_internal::calls as c;",
        ),
        ("a pub use", "pub use super::super::log::General as Sneak;"),
        ("a pub(crate) use", "pub(crate) use crate::log::General;"),
        (
            "a use across lines",
            "use rutent_contracts::core_internal::{\n    calls::Measures as Sneak,\n};",
        ),
        (
            "a path in a body",
            "fn sneak() { let _ = rutent_prov::catalogue::DEFAULT_CLOCK_SKEW_SECONDS; }",
        ),
        ("the router", "use rutent_router::quote;"),
        ("the state machine", "fn s() { rutent_stm::run(); }"),
        ("the writer", "use super::record::Record;"),
        ("the trail's reader", "use super::trail::read_trail;"),
        ("the module's own re-export", "use super::Record;"),
        ("a log", "use rutent_contracts::core_internal::calls::Log;"),
        (
            "spaces round a path's separators",
            "fn s() -> usize { super :: record :: NOT_A_GENERAL_ENTRY.len() }",
        ),
        ("a mutable borrow", "fn s(x: &mut u8) { *x = 0; }"),
        (
            "a child module",
            "mod sneak;\nfn s() -> usize { sneak::reach().len() }",
        ),
        (
            "a capability named in place",
            "fn s<P: rutent_contracts::core_internal::calls::Reads>(p: P) { drop(p); }",
        ),
    ] {
        assert!(
            !reach_of(&flat_code(sneak)).is_empty(),
            "the reader misses {dodge}: {sneak}"
        );
    }
    // No dependency on the parts that move value (`DEC-2`).
    let manifest_text = std::fs::read_to_string(format!("{manifest}/Cargo.toml")).unwrap();
    let dependencies = manifest_text.split("[dev-dependencies]").next().unwrap();
    for part in ["rutent-core-orch", "rutent-core-authz", "rutent-host"] {
        assert!(!dependencies.contains(part), "core-audit depends on {part}");
    }
    // The writer's reach is its type: a general handle on the log, and a clock. It names no operator channel, no
    // handle on policy and no other part.
    let writer = std::fs::read_to_string(format!("{manifest}/src/measures/record.rs")).unwrap();
    let code = flat_code(&writer);
    for forbidden in [
        "PolicyLog",
        "Operator",
        "Decisions",
        "Authorisations",
        "Charges",
        "StuckOrders",
        "rutent_prov",
        "rutent_stm",
        "rutent_router",
        "rutent_evidence",
    ] {
        assert!(
            words(&code, forbidden).next().is_none(),
            "the writer names {forbidden}"
        );
    }
    // The trail's reader asks policy's read check and nothing else of it: it never names the grant or the
    // revocation, nor a handle on the log.
    let reader = std::fs::read_to_string(format!("{manifest}/src/measures/trail.rs")).unwrap();
    let code = flat_code(&reader);
    for forbidden in [
        "grant_permission",
        "revoke_permission",
        "PolicyLog",
        "General",
        "Log",
        "Operator",
        "append",
        "append_decision",
    ] {
        assert!(
            words(&code, forbidden).next().is_none(),
            "the trail's reader names {forbidden}"
        );
    }
}
