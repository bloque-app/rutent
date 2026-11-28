//! The bulk measures over simulated runs, and a regulator's read of the trail over the real policy engine
//! (`T-022-06`): Core is put together over a simulated reserve, orders are run through its own `request`, `quote`,
//! `accept` and `run`, each leaving what the simulated provider did in the log, and `core-audit`'s measures are read
//! back from that log (`REQ-AUD-7`); a regulator reads the trail by the permission the operator grants through
//! policy, and each read is recorded (`REQ-AUD-12`).
//!
//! **`each_measure_is_computed_from_simulated_runs` is a counting test over records written as orchestration will
//! write them, not an end-to-end one.** Orchestration has no caller of `Measures` yet (`T-020-31` adds it, and its
//! end-to-end scenario replaces the hand-written feed below), so the test plays its part, and only that: for each segment Core's
//! own run accounted for, it writes the record that segment's outcome gives (the quote the route's operation priced
//! against the amount the segment settled with, a failure, a refusal), citing the receipt of that segment's own
//! effect, through the one door the assembly gives (`Core::measure_writer`). Every figure it writes is the run's;
//! none is made up. What the measures count is then read from the log, not from the test.

mod execution_support;

use execution_support::*;
use rutent_conformance::sim::{Act, Behaviour, Delivery};
use rutent_contracts::core_internal::calls::{Measures, Reads};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Entry, Measure, MeasureRecord, Outcome, PermissionChange, ReadCheck,
    ReadOutcome, Receipt, Role, Said, What,
};
use rutent_contracts::router_api::{Amount, Id, Timestamp};
use rutent_contracts::state_machine::Observed;
use rutent_core_audit::measures::{Read, Window, measure, read_trail};
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::segments::{Accounted, Ran, SegmentOutcome};
use rutent_prov::catalogue::Verdict;
use rutent_scenarios::assembly::Core;
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};
use rutent_scenarios::on_tick::OnTick;
use serde_json::json;

/// One order run: its id, the providers' quotes by segment and what each segment came to.
struct Run {
    order: Id,
    /// The amount each operation of the route was priced to deliver, and its provider and asset, in order.
    priced: Vec<(Id, Amount, rutent_contracts::provider_role::Denomination)>,
    segments: Vec<Accounted>,
}

/// An order requested, quoted, accepted and run, with `script` queued at the reserve before it runs.
fn ran(core: &mut Core, session: &Id, name: &str, script: impl FnOnce(&mut Core)) -> Run {
    let request = within(name, session, AMOUNT, RESERVE, OTHER);
    let (order, told) = quoted(core, request);
    let record = record_of(core, &told);
    let priced = record.chosen().expect("a route was chosen").parts[0]
        .operations
        .iter()
        .map(|op| {
            (
                op.provider.clone(),
                op.amount_out.clone(),
                op.to.asset.clone(),
            )
        })
        .collect();
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted")
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    script(core);
    let ran = core.ran_on_tick(&order).expect("the order ran");
    let [Ran::Segments(segments)] = ran.as_slice() else {
        panic!("one part ran its segments: {ran:?}");
    };
    Run {
        order,
        priced,
        segments: segments.clone(),
    }
}

/// What a dry run at the reserve said, as the verdict the catalogue read from it gives.
fn dry_run(core: &mut Core, session: &Id) -> Option<Said> {
    match core
        .check_by_dry_run(RESERVE, session, AMOUNT)
        .expect("a check")
    {
        Verdict::Yes => Some(Said::Yes),
        Verdict::No | Verdict::Refused(_) => Some(Said::No),
        Verdict::NotKnown => None,
    }
}

/// When the log entry at `receipt` was made, by Core's clock.
fn made(core: &Core, receipt: &Receipt) -> Timestamp {
    let log = core.log();
    let (_, entry) = &log.entries()[usize::try_from(receipt.index.get()).unwrap()];
    match entry {
        Entry::Verified(v) => v.when.at.clone(),
        Entry::Stated(_) => panic!("an effect is verified"),
    }
}

/// Plays orchestration's part for one run: the records its segments' outcomes give, written to the log.
fn record(core: &Core, run: &Run, dry: Option<Said>) {
    use rutent_contracts::core_internal::SegmentEnd;
    let mut writer = core.measure_writer();
    let write = |segment: &Accounted, what: Measure, provider: &Id| {
        let cite = segment
            .effect
            .clone()
            .expect("the segment's effect is recorded");
        let record: MeasureRecord = serde_json::from_value(json!({
            "provider": provider,
            "role": "provider",
            "order": run.order,
            "segment": segment.segment,
            "measure": what,
            "cites": [cite],
            "at": made(core, &cite),
        }))
        .unwrap();
        record
    };
    for (n, segment) in run.segments.iter().enumerate() {
        let (provider, quoted, asset) = &run.priced[n];
        let measures = match &segment.outcome {
            SegmentOutcome::Settled { amount_out, .. } => vec![Measure::Quote {
                quoted: quoted.clone(),
                executed: amount_out.clone(),
                asset: asset.clone(),
            }],
            SegmentOutcome::Failed | SegmentOutcome::WithdrawalCancelled => {
                vec![Measure::Failure { market: None }]
            }
            SegmentOutcome::Refused(_) => vec![Measure::Refusal {}],
            other => panic!("a run this test doesn't plan: {other:?}"),
        };
        // The dry run said before the order ran is measured against how its first segment ended.
        let first = n == 0;
        let ended = match segment.outcome {
            SegmentOutcome::Settled { .. } => SegmentEnd::Done,
            _ => SegmentEnd::Failed,
        };
        let mut all = measures;
        if let (true, Some(said)) = (first, dry) {
            all.push(Measure::DryRun {
                said,
                outcome: ended,
            });
            all.push(Measure::Answer { said });
        }
        for what in all {
            writer
                .measure(&write(segment, what, provider))
                .expect("recorded");
        }
    }
}

/// A window wide enough to hold the run.
fn whole() -> Window {
    Window {
        from: Timestamp::new("2000-01-01T00:00:00Z").unwrap(),
        until: Timestamp::new("2100-01-01T00:00:00Z").unwrap(),
    }
}

/// What a run's segments are, counted from the run's own accounting, for the test to hold the log's measures to.
#[derive(Default, Debug, PartialEq, Eq)]
struct Expected {
    quotes: u64,
    below: u64,
    at: u64,
    above: u64,
    failures: u64,
    refusals: u64,
}

fn expected(runs: &[&Run]) -> Expected {
    let mut e = Expected::default();
    for run in runs {
        for (n, segment) in run.segments.iter().enumerate() {
            match &segment.outcome {
                SegmentOutcome::Settled { amount_out, .. } => {
                    e.quotes += 1;
                    let quoted = &run.priced[n].1;
                    if amount_out == quoted {
                        e.at += 1;
                    } else if quoted.checked_sub(amount_out).is_some() {
                        e.below += 1;
                    } else {
                        e.above += 1;
                    }
                }
                SegmentOutcome::Refused(_) => e.refusals += 1,
                _ => e.failures += 1,
            }
        }
    }
    e
}

// verifies: REQ-AUD-7
#[test]
fn each_measure_is_computed_from_simulated_runs() {
    let (mut core, session) = core_with("AC-28", &[RESERVE]);
    // The records here are this test's own, written by hand through the log's writer: Core's own writer, which its
    // ticks run, is held off after its first tick, so the measures are of these records alone.
    core.tick();
    core.set_cadence(rutent_core_orch::schedule::Job::WriteMeasures, u64::MAX);

    // Four orders over the one simulated reserve. Each is dry-run first, so a dry run is measured against how its
    // order ended.
    let mut drys = Vec::new();
    let dry = dry_run(&mut core, &session);
    drys.push(dry);
    let full = ran(&mut core, &session, "bulk-full", |_| {});
    record(&core, &full, dry);

    let dry = dry_run(&mut core, &session);
    drys.push(dry);
    let short = ran(&mut core, &session, "bulk-short", |core| {
        core.world().script_for(
            RESERVE,
            "withdrawal",
            Behaviour::delivering(Delivery::Short { bps: 100 }),
        );
    });
    record(&core, &short, dry);

    let dry = dry_run(&mut core, &session);
    drys.push(dry);
    let failed = ran(&mut core, &session, "bulk-failed", |core| {
        core.world()
            .script_for(RESERVE, "deposit", Behaviour::act(Act::Fail));
    });
    record(&core, &failed, dry);
    // Left with no route, so Core's run recalculates nothing of it on the ticks the next orders run on: the measures are
    // of the four runs alone.
    core.leave_no_route(&failed.order)
        .expect("left with no route");

    // The third order's final deposit fails at the reserve; the reserve says no to the fourth's dry run, then
    // refuses its final deposit.
    core.world()
        .script_dry_run(RESERVE, "withdrawal", Behaviour::act(Act::Refuse));
    let dry = dry_run(&mut core, &session);
    drys.push(dry);
    let refused = ran(&mut core, &session, "bulk-refused", |core| {
        core.world()
            .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    });
    record(&core, &refused, dry);
    core.leave_no_route(&refused.order)
        .expect("left with no route");

    // What the runs came to, as Core accounted for them: the test's expectation, from the run and not the log.
    let runs = [&full, &short, &failed, &refused];
    let want = expected(&runs);
    let outcomes: Vec<_> = runs
        .iter()
        .map(|r| {
            r.segments
                .iter()
                .map(|s| format!("{:?}", s.outcome))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        want.quotes >= 3 && want.below >= 1,
        "the runs settled, one of them short: {want:?} {outcomes:?}"
    );
    // The failed and the refused orders really ended so, asserted on the runs themselves: a run with no failure
    // can't pass for one that had it.
    assert!(
        matches!(
            failed.segments.last().unwrap().outcome,
            SegmentOutcome::Failed
        ),
        "the third order's deposit failed: {outcomes:?}"
    );
    assert!(
        matches!(
            refused.segments.last().unwrap().outcome,
            SegmentOutcome::Refused(_)
        ),
        "the fourth order's deposit was refused: {outcomes:?}"
    );
    assert_eq!(
        (want.failures, want.refusals),
        (1, 1),
        "{want:?} {outcomes:?}"
    );

    // The measures are read from the log Core's own run left.
    let report = {
        let log = core.log();
        measure(log.entries(), &whole())
    };
    let providers: Vec<_> = report.providers.keys().map(String::as_str).collect();
    assert_eq!(providers, [RESERVE], "the one provider the runs used");
    let reserve = &report.providers[RESERVE];
    let (mut quotes, mut below, mut at, mut above) = (0, 0, 0, 0);
    for q in reserve.quotes.values() {
        quotes += q.quotes;
        below += q.executed_below;
        at += q.executed_at;
        above += q.executed_above;
    }
    assert_eq!(
        Expected {
            quotes,
            below,
            at,
            above,
            failures: reserve.failures.all(),
            refusals: reserve.refusals,
        },
        want,
        "quotes against what was executed, failures and refusals, as the runs came to"
    );
    assert_eq!(
        reserve.failures.no_figure,
        reserve.failures.all(),
        "no market figure is held in M3"
    );
    // The short delivery is among the quotes below the quote, and its shortfall is stated.
    let (cop, shortfall) = reserve
        .quotes
        .iter()
        .find(|(_, q)| q.executed_below > 0)
        .map(|(key, q)| (key.clone(), q.below_by.clone()))
        .unwrap();
    assert!(shortfall.is_some_and(|s| !s.is_zero()), "{cop:?}");

    // Dry runs against how the segment they were run for ended, as each run's accounting gives it.
    let mut dry_want = (0, 0, 0, 0);
    for (said, run) in drys.iter().zip(runs) {
        let done = matches!(run.segments[0].outcome, SegmentOutcome::Settled { .. });
        match (said, done) {
            (Some(Said::Yes), true) => dry_want.0 += 1,
            (Some(Said::Yes), false) => dry_want.1 += 1,
            (Some(Said::No), true) => dry_want.2 += 1,
            (Some(Said::No), false) => dry_want.3 += 1,
            (None, _) => {}
        }
    }
    let d = &reserve.dry_runs;
    assert_eq!(
        (
            d.yes_then_done,
            d.yes_then_failed,
            d.no_then_done,
            d.no_then_failed
        ),
        dry_want
    );
    assert!(
        d.no_then_done == 1,
        "the reserve's no to a dry run whose operation then settled: {d:?}"
    );
    // Rates of no: the one refused dry run among four answers.
    assert_eq!((reserve.answers.yes, reserve.answers.no), (3, 1));
    assert_eq!(reserve.answers.rate_of_no().per_million(), Some(250_000));
    // Reversals: no run of the assembled Core reverses anything in M3 (T-020-31 records them), so the count is
    // asserted to be 0 explicitly, and it is the log's own.
    let reversals = core
        .inputs()
        .iter()
        .filter(|i| matches!(i.observed, Observed::ReversalCompleted { .. }))
        .count() as u64;
    assert_eq!(reversals, 0, "no run reverses anything yet");
    assert_eq!(reserve.reversals, reversals);

    // Over a window that holds none of it, nothing is counted; and what was counted came from the log: every
    // measure record of the log is counted once.
    let before = Window {
        from: Timestamp::new("2000-01-01T00:00:00Z").unwrap(),
        until: Timestamp::new("2001-01-01T00:00:00Z").unwrap(),
    };
    let log = core.log();
    assert_eq!(measure(log.entries(), &before).counted, 0);
    let records = log
        .entries()
        .iter()
        .filter(|(_, e)| matches!(e, Entry::Verified(v) if matches!(v.what, What::Measure { .. })))
        .count() as u64;
    assert_eq!(report.counted, records);
}

/// The regulator whose reads these are: a person acting for a regulating party.
fn regulator() -> Actor {
    Actor {
        party: id("fin-regulator"),
        acts: id("examiner-3"),
        role: Role::Regulator,
        kind: ActorKind::Person,
    }
}

/// The operator's login, which grants the permission.
fn grantor() -> Actor {
    kit::operator_login()
}

fn trail_read(request: &str, permission: Option<&str>, at: &Timestamp) -> ReadCheck {
    serde_json::from_value(json!({
        "request": request,
        "reader": regulator(),
        "reads": {"kind": "trail"},
        "at": at,
    }))
    .map(|mut c: ReadCheck| {
        c.permission = permission.map(id);
        c
    })
    .unwrap()
}

fn permission(request: &str, at: &Timestamp) -> PermissionChange {
    serde_json::from_value(json!({
        "request": request,
        "permission": "perm-reg-1",
        "grantee": regulator(),
        "scope": {"kind": "trail"},
        "by": grantor(),
        "at": at,
    }))
    .unwrap()
}

// verifies: REQ-AUD-12
#[test]
fn regulators_permitted_read_is_recorded() {
    let (mut core, session) = core_with("AC-28", &[RESERVE]);
    let run = ran(&mut core, &session, "regulated", |_| {});
    record(&core, &run, None);
    core.registers().list(&regulator());
    core.registers().list(&grantor());
    // Granting and revoking a read are each a permission of the operator's login's own (`T-021-14`).
    core.registers()
        .permit(&grantor(), rutent_core_authz::roles::Permission::GrantRead);
    core.registers()
        .permit(&grantor(), rutent_core_authz::roles::Permission::RevokeRead);
    let now = || {
        let log = core.log();
        match &log.entries().last().unwrap().1 {
            Entry::Verified(v) => v.when.at.clone(),
            Entry::Stated(_) => panic!("verified"),
        }
    };
    let mut policy = core.policy();
    let history = || core.log().entries().to_vec();

    // Without its permission the read is refused, and the refusal is recorded: nothing of the trail comes with it.
    let unnamed = trail_read("req-trail-a", None, &now());
    let Ok(Read::Refused(answer)) = read_trail(&mut policy, &unnamed, history) else {
        panic!("a regulator's read with no permission is refused")
    };
    assert!(matches!(answer.outcome, ReadOutcome::Refused { .. }));
    let recorded = |receipt: &Receipt| -> Entry {
        core.log().entries()[usize::try_from(receipt.index.get()).unwrap()]
            .1
            .clone()
    };
    assert!(matches!(
        recorded(&answer.receipt),
        Entry::Verified(v) if matches!(&v.what, What::CheckRead { check } if check.request.as_str() == "req-trail-a")
            && matches!(v.outcome, Outcome::Refused { .. })
    ));
    // Naming a permission not yet granted is refused too, and recorded.
    let named = trail_read("req-trail-b", Some("perm-reg-1"), &now());
    let Ok(Read::Refused(_)) = read_trail(&mut policy, &named, history) else {
        panic!("a permission not granted is refused")
    };

    // The operator grants the permission through policy, and the same read is allowed and recorded.
    let granted = policy
        .grant_permission(&permission("req-grant-1", &now()))
        .expect("asked");
    assert!(
        granted.permit().is_some(),
        "the operator's grant is permitted: {granted:?}"
    );
    let before = core.log().len();
    let permitted = trail_read("req-trail-c", Some("perm-reg-1"), &now());
    let Ok(Read::Released(trail)) = read_trail(&mut policy, &permitted, history) else {
        panic!("a regulator holding the permission reads the trail")
    };
    assert_eq!(
        core.log().len(),
        before + 1,
        "the read added one entry: its record"
    );
    let (receipt, last) = trail.entries.last().unwrap();
    assert_eq!(receipt, &trail.read);
    let Entry::Verified(v) = last else {
        panic!("verified")
    };
    assert_eq!(v.who, regulator(), "who read");
    assert_eq!(v.when.at, permitted.at, "when");
    assert_eq!(v.outcome, Outcome::Permitted {});
    let What::CheckRead { check } = &v.what else {
        panic!("a read is recorded as a read check")
    };
    assert_eq!(
        check, &permitted,
        "what: the trail, by the permission named"
    );
    // The trail released is the log's, entry for entry, up to the record of the read: the run's own entries and the
    // measure records among them.
    assert_eq!(trail.entries, core.log().entries().to_vec());
    assert!(
        trail.entries.iter().any(
            |(_, e)| matches!(e, Entry::Verified(v) if matches!(v.what, What::Measure { .. }))
        )
    );
    // The record holds nothing `REQ-AUD-6` keeps out: only the check's own fields.
    let json = serde_json::to_value(&v.what).unwrap();
    let mut keys: Vec<_> = json["check"].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["at", "permission", "reader", "reads", "request"]);

    // Another regulating party, holding no permission of its own, can't borrow it; the refusal is recorded.
    let mut other = permitted.clone();
    other.request = id("req-trail-d");
    other.reader.party = id("other-regulator");
    other.reader.acts = id("examiner-9");
    core.registers().list(&other.reader);
    let Ok(Read::Refused(_)) = read_trail(&mut policy, &other, history) else {
        panic!("another regulator can't use the permission")
    };

    // Revoked, the permission reads no more, and the refusal is recorded.
    let mut revoke = permission("req-revoke-1", &now());
    revoke.permission = id("perm-reg-1");
    assert!(
        policy
            .revoke_permission(&revoke)
            .expect("asked")
            .permit()
            .is_some()
    );
    let after = trail_read("req-trail-e", Some("perm-reg-1"), &now());
    let Ok(Read::Refused(refused)) = read_trail(&mut policy, &after, history) else {
        panic!("a revoked permission reads no more")
    };
    assert!(matches!(
        recorded(&refused.receipt),
        Entry::Verified(v) if matches!(v.outcome, Outcome::Refused { .. })
    ));
}
