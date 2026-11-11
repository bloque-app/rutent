//! A guest's CPU, time and memory limits (`REQ-MDL-11`, `B-03`, `B-04`): a guest past any is stopped, its operation
//! is refused, and its provider keeps answering. CPU is bounded per operation: the outside part's calls for one
//! operation share one budget of fuel, and the platform kills a process that hasn't answered by the deadline.

#[path = "module_support/mod.rs"]
mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use common::Probe;
use common::{
    OPERATION, WasmBinding, answers_as, binding, declaration, install, languages, spec, withdrawal,
};
use rutent_conformance::boundary::{
    BoundaryHarness, Endpoint, Handled, ModuleBinding, ModuleSpec, Output, PlatformRefusal, Step,
};
use rutent_contracts::module_interface::{
    CallOutcome, CallRefusalReason, CrossModuleOutcome, Declaration, OperationKind, PartName,
    RefusalReason, validate_call, validate_call_against, validate_record,
};
use rutent_prov::module::engine::{FUEL_PER_PART, MEMORY_PER_PART};
use rutent_prov::module::limits::{Bounds, LimitHit, LimitKind, MAX_KEY, Metered, Scope};
use rutent_prov::module::platform::OPERATION_DEADLINE;
use rutent_prov::module::sandbox::read_frame_up_to;
use serde_json::json;

// verifies: REQ-MDL-11
#[test]
fn a_guest_that_loops_is_stopped_when_its_fuel_runs_out() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (looping, steady) = (format!("looping-{language}"), format!("steady-{language}"));
        install(
            &mut binding,
            &spec(&steady, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        install(
            &mut binding,
            &spec(&looping, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Spin],
        );
        let pid = binding.process(&looping);
        let started = Instant::now();
        let handled = binding.handle(&looping, &common::withdrawal(&looping, 1));
        let took = started.elapsed();
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::LimitReached),
            "a looping guest answered"
        );
        assert_eq!(
            binding.process(&looping),
            pid,
            "running out of fuel ended the process"
        );
        assert!(
            answers_as(&mut binding, &steady, "1.0.0", 2),
            "another provider stopped answering"
        );
        let why = binding.measurements.last_trap.clone().unwrap_or_default();
        assert!(why.contains("fuel"), "it stopped for another reason: {why}");
        assert!(
            took < OPERATION_DEADLINE / 4,
            "running out of fuel took {took:?}, too near the deadline"
        );
        eprintln!(
            "B-03 [{language}]: {FUEL_PER_PART} units of fuel; stopped after {took:?}: {why}"
        );
    }
}

// verifies: REQ-MDL-11
#[test]
fn a_guest_growing_its_memory_past_the_bound_is_refused_the_memory() {
    let bound_pages = (MEMORY_PER_PART / 65536) as u32;
    for language in languages() {
        let mut binding = binding("reserve");
        let (small, large) = (format!("small-{language}"), format!("large-{language}"));
        install(
            &mut binding,
            &spec(&small, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Grow(16)],
        );
        install(
            &mut binding,
            &spec(&large, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Grow(bound_pages)],
        );
        assert!(
            answers_as(&mut binding, &small, "1.0.0", 1),
            "growing within the bound was refused"
        );
        let pid = binding.process(&large);
        let handled = binding.handle(&large, &common::withdrawal(&large, 2));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::LimitReached),
            "growing past the bound gave an answer, or was reported as something else"
        );
        assert_eq!(
            binding.process(&large),
            pid,
            "the refusal ended the process"
        );
        assert!(
            answers_as(&mut binding, &small, "1.0.0", 3),
            "another provider stopped answering"
        );
        let why = binding.measurements.last_trap.clone().unwrap_or_default();
        eprintln!(
            "B-04 [{language}]: bound {MEMORY_PER_PART} bytes; growing by {bound_pages} pages refused: {why}"
        );
    }
}

/// How many `burned` values the provider's answer to a withdrawal carries, or `None` if it gave no answer.
fn burned(binding: &mut common::WasmBinding, provider: &str, n: usize) -> Option<usize> {
    let Handled::Answered(result) = binding.handle(provider, &common::withdrawal(provider, n))
    else {
        return None;
    };
    let value = serde_json::to_value(&result.statement).ok()?;
    let reference = value["outcome"]["evidence"][0]["reference"]
        .as_str()?
        .to_owned();
    Some(reference.matches("-burned").count())
}

// verifies: REQ-MDL-11
#[test]
fn the_outside_parts_calls_for_one_operation_share_one_budget_of_fuel() {
    // Each outside request costs the outside part well under its fuel: 2,000,000 iterations, about 14 million
    // units in `c` and 32 million in `rs` on arm64 macOS, the parsing of its request included. Thirty of them cost
    // more than one budget, so a fresh budget per call would let all thirty finish, and one per operation can't.
    const TIMES: usize = 30;
    for language in languages() {
        let mut binding = binding("reserve");
        let provider = format!("burning-{language}");
        install(
            &mut binding,
            &spec(&provider, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Burn {
                iterations: 2_000_000,
                times: TIMES,
            }],
        );
        let pid = binding.process(&provider);
        let first = burned(&mut binding, &provider, 1).expect("the operation was answered");
        assert!(
            first >= 1,
            "not one outside request finished within the budget"
        );
        assert!(
            first < TIMES,
            "all {TIMES} outside requests finished: the outside part's fuel was refilled per call"
        );
        // The next operation has a budget of its own, and fuel is deterministic.
        let second = burned(&mut binding, &provider, 2).expect("the next operation was answered");
        assert_eq!(second, first, "the next operation had another budget");
        assert_eq!(
            binding.process(&provider),
            pid,
            "running out of fuel ended the process"
        );
        eprintln!(
            "B-03 [{language}]: {first} of {TIMES} outside requests finished on one operation's {FUEL_PER_PART} units"
        );
    }
}

// verifies: REQ-MDL-11, REQ-MDL-13
#[test]
fn a_provider_process_that_misses_the_operation_deadline_is_killed_and_started_again() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (late, steady) = (format!("late-{language}"), format!("steady-{language}"));
        install(
            &mut binding,
            &spec(&steady, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        install(
            &mut binding,
            &spec(&late, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Spin],
        );
        let (late_pid, steady_pid) = (binding.process(&late), binding.process(&steady));
        // Far below what the spinning guest takes to run out of fuel, so the deadline, not fuel, stops it.
        let deadline = Duration::from_millis(1);
        binding.set_deadline(deadline);
        let started = Instant::now();
        let handled = binding.handle(&late, &common::withdrawal(&late, 1));
        let took = started.elapsed();
        binding.set_deadline(OPERATION_DEADLINE);
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::LimitReached),
            "an operation past its deadline was answered"
        );
        let why = binding.measurements.last_trap.clone().unwrap_or_default();
        assert!(
            why.starts_with("deadline"),
            "it stopped for another reason: {why}"
        );
        assert_eq!(
            binding.measurements.overdue, 1,
            "the deadline wasn't counted"
        );
        assert!(
            binding.process(&late).is_some() && binding.process(&late) != late_pid,
            "the late process wasn't replaced"
        );
        if let Some(old) = late_pid {
            assert!(!common::alive(old), "the late process is still alive");
        }
        assert_eq!(
            binding.process(&steady),
            steady_pid,
            "another provider's process changed"
        );
        assert!(
            answers_as(&mut binding, &steady, "1.0.0", 2),
            "another provider stopped answering"
        );
        eprintln!("deadline [{language}]: {deadline:?}; refused after {took:?}: {why}");
    }
}

fn serve_api(binding: &mut WasmBinding, provider: &str) -> Endpoint {
    let endpoint = Endpoint::https(&format!("api.{provider}.example"));
    binding.serve(&endpoint, IpAddr::V4(Ipv4Addr::new(185, 12, 64, 40)), None);
    endpoint
}

fn api_call(provider: &str) -> Step {
    Step::Call {
        operation: OPERATION.to_owned(),
        endpoint: Endpoint::https(&format!("api.{provider}.example")),
    }
}

fn read_entry() -> Step {
    Step::ReadState {
        part: PartName::Outside,
        key: "entry".to_owned(),
    }
}

/// The default bounds with `change` made.
fn tight(change: impl FnOnce(&mut Bounds)) -> Bounds {
    let mut bounds = Bounds::DEFAULT;
    change(&mut bounds);
    bounds
}

/// A module of `provider` that may call `callee` for withdrawals.
fn calling(provider: &str, callee: Option<&str>) -> Declaration {
    let mut value =
        serde_json::to_value(declaration(provider, "1.0.0", &["withdrawal"])).expect("JSON");
    if let Some(callee) = callee {
        value["outside"]["calls"] = json!([{ "callee": callee, "kinds": ["withdrawal"] }]);
    }
    serde_json::from_value(value).expect("a valid declaration")
}

fn module(declaration: Declaration, steps: Vec<Step>) -> ModuleSpec {
    ModuleSpec {
        declaration,
        steps,
        output: Output::Honest,
        starts: true,
    }
}

/// One limit, and a module that hits it.
struct Case {
    label: &'static str,
    bounds: Bounds,
    steps: fn(&str) -> Vec<Step>,
    /// The length of the value seeded under the key `entry`, if any.
    state: Option<usize>,
    probes: Vec<Probe>,
    hit: LimitHit,
    /// How many outbound call records, and how many of them completed, the operation leaves.
    records: (usize, usize),
}

fn operation(what: Metered) -> LimitHit {
    LimitHit {
        kind: LimitKind::Metered(what),
        scope: Some(Scope::Operation),
    }
}

// verifies: REQ-MDL-11
#[test]
fn module_hitting_each_limit_leaves_its_transaction_in_a_known_state() {
    let cases = [
        Case {
            label: "calls",
            bounds: tight(|b| b.per_operation.outbound_calls = 2),
            steps: |p| vec![api_call(p), api_call(p), api_call(p), api_call(p)],
            state: None,
            probes: Vec::new(),
            hit: operation(Metered::OutboundCalls),
            records: (3, 2),
        },
        Case {
            label: "bytes",
            bounds: tight(|b| b.per_operation.outbound_bytes = 500),
            // Each call carries the 300 bytes the read obtained.
            steps: |p| vec![read_entry(), api_call(p), api_call(p), api_call(p)],
            state: Some(300),
            probes: Vec::new(),
            hit: operation(Metered::OutboundBytes),
            records: (2, 1),
        },
        Case {
            label: "reads",
            bounds: tight(|b| b.per_operation.state_reads = 2),
            steps: |_| vec![read_entry(), read_entry(), read_entry(), read_entry()],
            state: Some(10),
            probes: Vec::new(),
            hit: operation(Metered::StateReads),
            records: (0, 0),
        },
        Case {
            label: "state",
            bounds: tight(|b| b.per_operation.state_bytes = 500),
            steps: |_| vec![read_entry(), read_entry(), read_entry()],
            state: Some(300),
            probes: Vec::new(),
            hit: operation(Metered::StateBytes),
            records: (0, 0),
        },
        Case {
            label: "pending",
            bounds: tight(|b| b.per_operation.outside_requests = 3),
            steps: |_| Vec::new(),
            state: None,
            probes: vec![Probe::Burn {
                iterations: 1,
                times: 6,
            }],
            hit: operation(Metered::OutsideRequests),
            records: (0, 0),
        },
    ];
    for language in languages() {
        for case in &cases {
            let mut binding = binding("reserve");
            let provider = format!("{}-{language}", case.label);
            let steady = format!("steady-{}-{language}", case.label);
            let endpoint = serve_api(&mut binding, &provider);
            install(
                &mut binding,
                &spec(&steady, "1.0.0", Vec::new(), true),
                &language,
                "reserve",
                &[],
            );
            if let Some(length) = case.state {
                binding.seed_state(&provider, "entry", &"x".repeat(length));
            }
            install(
                &mut binding,
                &module(
                    declaration(&provider, "1.0.0", &["withdrawal"]),
                    (case.steps)(&provider),
                ),
                &language,
                "reserve",
                &case.probes,
            );
            binding.set_bounds(case.bounds);
            let pid = binding.process(&provider);
            for n in 1..=2 {
                binding.take_events();
                binding.take_limits();
                let handled = binding.handle(&provider, &withdrawal(&provider, n));
                let label = format!("{} [{language}], operation {n}", case.label);
                // A limit hit after a call reached the endpoint leaves the outcome unknown, never a refusal: the
                // endpoint may have acted on it (`REQ-ORD-1`). With nothing passed out, it is the limit.
                let passed_out = case.records.1 > 0;
                assert_eq!(
                    handled,
                    Handled::Refused(if passed_out {
                        PlatformRefusal::OutcomeUnknown
                    } else {
                        PlatformRefusal::LimitReached
                    }),
                    "{label}: the operation wasn't refused as it should be"
                );
                // Known state: one limit event, naming the bound, the provider and the transaction.
                let events = binding.take_events();
                let unknown = binding.take_unknown();
                if passed_out {
                    assert_eq!(unknown.len(), 1, "{label}: {unknown:?}");
                    assert_eq!(
                        unknown[0].ended_by,
                        PlatformRefusal::LimitReached,
                        "{label}"
                    );
                } else {
                    assert!(unknown.is_empty(), "{label}: {unknown:?}");
                }
                let limits = binding.take_limits();
                assert_eq!(limits.len(), 1, "{label}: {limits:?}");
                assert_eq!(limits[0].provider, provider, "{label}");
                assert_eq!(
                    limits[0].transaction,
                    format!("tx-{provider}-{n}"),
                    "{label}"
                );
                assert_eq!(limits[0].hit, case.hit, "{label}");
                // Every call it made is on record, and the endpoint received what completed and nothing more.
                assert_eq!(events.records.len(), case.records.0, "{label}: records");
                for record in &events.records {
                    assert_eq!(validate_record(record), Ok(()), "{label}");
                }
                if case.records.0 > case.records.1 {
                    assert_eq!(
                        events.records.last().map(|r| &r.outcome),
                        Some(&CallOutcome::Refused(
                            rutent_contracts::module_interface::Refused {
                                reason: RefusalReason::LimitReached
                            }
                        )),
                        "{label}: the call over the limit wasn't recorded as refused for it"
                    );
                }
                assert_eq!(
                    binding.received(&endpoint).len(),
                    case.records.1 * n,
                    "{label}: the endpoint received a call the platform refused"
                );
                assert_eq!(
                    binding.process(&provider),
                    pid,
                    "{label}: the process ended"
                );
                assert!(
                    answers_as(&mut binding, &steady, "1.0.0", n),
                    "{label}: another provider stopped answering"
                );
            }
        }
    }
}

// verifies: REQ-MDL-11, REQ-MDL-21
#[test]
fn module_hitting_a_cross_module_limit_leaves_its_caller_and_callee_in_a_known_state() {
    for language in languages() {
        // The caller's own count of calls.
        {
            let mut binding = binding("reserve");
            let (caller, callee) = (format!("caller-{language}"), format!("callee-{language}"));
            install(
                &mut binding,
                &spec(&callee, "1.0.0", Vec::new(), true),
                &language,
                "reserve",
                &[],
            );
            let steps = (0..4)
                .map(|_| Step::CallModule {
                    callee: callee.clone(),
                    kind: OperationKind::Withdrawal,
                })
                .collect();
            let caller_module = module(calling(&caller, Some(&callee)), steps);
            install(&mut binding, &caller_module, &language, "reserve", &[]);
            binding.set_bounds(tight(|b| b.per_operation.module_calls = 2));
            binding.take_events();
            let handled = binding.handle(&caller, &withdrawal(&caller, 1));
            // Two calls were delivered before the limit, so the caller's outcome is unknown, never a refusal, and
            // the limit that ended it is recorded beside it (`REQ-ORD-1`).
            assert_eq!(handled, Handled::Refused(PlatformRefusal::OutcomeUnknown));
            let events = binding.take_events();
            let unknown = binding.take_unknown();
            assert_eq!(unknown.len(), 1, "{unknown:?}");
            assert_eq!(unknown[0].ended_by, PlatformRefusal::LimitReached);
            let limits = binding.take_limits();
            assert_eq!(limits.len(), 1, "{limits:?}");
            assert_eq!(limits[0].provider, caller);
            assert_eq!(limits[0].hit, operation(Metered::ModuleCalls));
            let outcomes: Vec<_> = events.calls.iter().map(|c| &c.outcome).collect();
            assert!(
                matches!(
                    outcomes.as_slice(),
                    [
                        CrossModuleOutcome::Delivered(_),
                        CrossModuleOutcome::Delivered(_),
                        CrossModuleOutcome::Refused(_)
                    ]
                ),
                "the calls weren't two delivered and one refused: {outcomes:?}"
            );
            for call in &events.calls {
                assert_eq!(validate_call(call), Ok(()));
                assert_eq!(
                    validate_call_against(&caller_module.declaration, call),
                    Ok(())
                );
            }
            assert_eq!(binding.operations_received(&callee), 2);
            assert!(answers_as(&mut binding, &callee, "1.0.0", 9));
        }
        // A provider's budget across the operations it handles inside one top-level operation.
        {
            let mut binding = binding("reserve");
            let (caller, callee) = (format!("outer-{language}"), format!("inner-{language}"));
            let endpoint = serve_api(&mut binding, &callee);
            install(
                &mut binding,
                &module(
                    declaration(&callee, "1.0.0", &["withdrawal"]),
                    vec![api_call(&callee), api_call(&callee)],
                ),
                &language,
                "reserve",
                &[],
            );
            let steps = (0..3)
                .map(|_| Step::CallModule {
                    callee: callee.clone(),
                    kind: OperationKind::Withdrawal,
                })
                .collect();
            install(
                &mut binding,
                &module(calling(&caller, Some(&callee)), steps),
                &language,
                "reserve",
                &[],
            );
            // Each of the callee's operations makes two calls, within its own budget of two; the third passes the
            // provider's five.
            binding.set_bounds(tight(|b| {
                b.per_operation.outbound_calls = 2;
                b.per_provider.outbound_calls = 5;
            }));
            binding.take_events();
            let handled = binding.handle(&caller, &withdrawal(&caller, 1));
            assert!(
                matches!(handled, Handled::Answered(_)),
                "the caller hit no limit of its own, and was refused: {handled:?}"
            );
            let events = binding.take_events();
            let limits = binding.take_limits();
            assert_eq!(limits.len(), 1, "{limits:?}");
            assert_eq!(limits[0].provider, callee);
            assert_eq!(
                limits[0].hit,
                LimitHit {
                    kind: LimitKind::Metered(Metered::OutboundCalls),
                    scope: Some(Scope::Provider)
                }
            );
            let outcomes: Vec<_> = events.calls.iter().map(|c| &c.outcome).collect();
            assert!(
                matches!(
                    outcomes.as_slice(),
                    [
                        CrossModuleOutcome::Delivered(_),
                        CrossModuleOutcome::Delivered(_),
                        CrossModuleOutcome::Unanswered(_)
                    ]
                ),
                "the third call wasn't unanswered: {outcomes:?}"
            );
            assert_eq!(
                binding.received(&endpoint).len(),
                5,
                "calls past the budget were delivered"
            );
            // The budget ended with the top-level operation: the next one has its own.
            binding.set_bounds(Bounds::DEFAULT);
            assert!(answers_as(&mut binding, &callee, "1.0.0", 9));
        }
        // The depth of a chain of calls.
        {
            let mut binding = binding("reserve");
            let names = ["first", "second", "third"].map(|n| format!("{n}-{language}"));
            for (i, name) in names.iter().enumerate() {
                let next = names.get(i + 1);
                let steps = next
                    .map(|callee| {
                        vec![Step::CallModule {
                            callee: callee.clone(),
                            kind: OperationKind::Withdrawal,
                        }]
                    })
                    .unwrap_or_default();
                install(
                    &mut binding,
                    &module(calling(name, next.map(String::as_str)), steps),
                    &language,
                    "reserve",
                    &[],
                );
            }
            binding.set_bounds(tight(|b| b.depth = 2));
            binding.take_events();
            let handled = binding.handle(&names[0], &withdrawal(&names[0], 1));
            assert!(
                matches!(handled, Handled::Answered(_)),
                "the first module hit no limit of its own: {handled:?}"
            );
            let events = binding.take_events();
            let limits = binding.take_limits();
            assert_eq!(limits.len(), 1, "{limits:?}");
            assert_eq!(limits[0].provider, names[1]);
            assert_eq!(limits[0].hit.kind, LimitKind::CallDepth);
            assert_eq!(
                events.calls.len(),
                2,
                "the second's refused call and the first's"
            );
            assert!(matches!(
                events.calls[0].outcome,
                CrossModuleOutcome::Refused(ref r) if r.reason == CallRefusalReason::LimitReached
            ));
            assert!(matches!(
                events.calls[1].outcome,
                CrossModuleOutcome::Unanswered(_)
            ));
            assert_eq!(binding.operations_received(&names[2]), 0);
        }
    }
}

/// A call over the caller's own count of calls is refused for the limit, not as an unavailable callee: the callee
/// runs, and the record says the caller's bound refused it.
// verifies: DEC-4, REQ-MDL-9
#[test]
fn call_refused_by_a_limit_names_its_own_reason() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (caller, callee) = (format!("counted-{language}"), format!("running-{language}"));
        install(
            &mut binding,
            &spec(&callee, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        let steps = (0..2)
            .map(|_| Step::CallModule {
                callee: callee.clone(),
                kind: OperationKind::Withdrawal,
            })
            .collect();
        let caller_module = module(calling(&caller, Some(&callee)), steps);
        install(&mut binding, &caller_module, &language, "reserve", &[]);
        binding.set_bounds(tight(|b| b.per_operation.module_calls = 1));
        binding.take_events();
        let handled = binding.handle(&caller, &withdrawal(&caller, 1));
        // The first call was delivered before the limit: the caller's outcome is unknown (`REQ-ORD-1`).
        assert_eq!(handled, Handled::Refused(PlatformRefusal::OutcomeUnknown));
        let events = binding.take_events();
        let refused = events.calls.last().expect("a record of the refused call");
        let reason = serde_json::to_value(&refused.outcome).expect("JSON")["reason"].clone();
        assert_ne!(reason, "callee_unavailable", "{language}");
        assert_eq!(reason, "limit_reached", "{language}");
        assert_eq!(validate_call(refused), Ok(()));
        assert_eq!(
            validate_call_against(&caller_module.declaration, refused),
            Ok(())
        );
        assert_eq!(binding.operations_received(&callee), 1, "{language}");
    }
}

// verifies: REQ-MDL-11, THR-57
#[test]
fn oversized_state_read_key_is_refused_before_it_is_copied() {
    for language in languages() {
        for part in [PartName::Outside, PartName::Sandboxed] {
            let mut binding = binding("reserve");
            let provider = format!("longkey-{language}-{part:?}").to_lowercase();
            let steady = format!("steady-{language}-{part:?}").to_lowercase();
            let key = "k".repeat(MAX_KEY + 1);
            binding.seed_state(&provider, &key, "found");
            install(
                &mut binding,
                &spec(&steady, "1.0.0", Vec::new(), true),
                &language,
                "reserve",
                &[],
            );
            install(
                &mut binding,
                &spec(
                    &provider,
                    "1.0.0",
                    vec![Step::ReadState {
                        part,
                        key: key.clone(),
                    }],
                    true,
                ),
                &language,
                "reserve",
                &[],
            );
            let pid = binding.process(&provider);
            binding.take_events();
            let handled = binding.handle(&provider, &withdrawal(&provider, 1));
            assert_eq!(
                handled,
                Handled::Refused(PlatformRefusal::LimitReached),
                "{language} {part:?}: a key of {} bytes was taken",
                key.len()
            );
            let events = binding.take_events();
            let limits = binding.take_limits();
            assert_eq!(limits.len(), 1, "{limits:?}");
            assert_eq!(limits[0].hit.kind, LimitKind::KeyLength);
            // Not copied: the world was never asked, and no event holds it.
            assert!(
                binding.world.state_asked.iter().all(|k| k.len() <= MAX_KEY),
                "the platform handed the oversized key to the world"
            );
            assert!(
                events.state_reads.iter().all(|e| e.key.len() <= MAX_KEY),
                "an event holds the oversized key"
            );
            assert_eq!(binding.process(&provider), pid, "the process ended");
            assert!(answers_as(&mut binding, &steady, "1.0.0", 2));
            // The longest key that is allowed is read.
            let longest = "k".repeat(MAX_KEY);
            binding.seed_state(&provider, &longest, "found");
            install(
                &mut binding,
                &spec(
                    &format!("{provider}-ok"),
                    "1.0.0",
                    vec![Step::ReadState { part, key: longest }],
                    true,
                ),
                &language,
                "reserve",
                &[],
            );
            assert!(answers_as(
                &mut binding,
                &format!("{provider}-ok"),
                "1.0.0",
                3
            ));
        }
    }
}

// verifies: REQ-MDL-11, THR-57
#[test]
fn a_frame_longer_than_the_bound_is_refused_from_its_length_prefix_without_reading_it() {
    let bound = 1024;
    // Only the prefix is there: a reader that went on to read the announced bytes would find none.
    let mut input = std::io::Cursor::new(((bound + 1) as u32).to_le_bytes().to_vec());
    let refused = read_frame_up_to(&mut input, bound);
    assert_eq!(
        refused.expect_err("a frame past the bound").kind(),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(input.position(), 4, "it read past the prefix");
    // At the bound it is read whole.
    let mut frame = (bound as u32).to_le_bytes().to_vec();
    frame.extend(std::iter::repeat_n(7u8, bound));
    let read = read_frame_up_to(&mut std::io::Cursor::new(frame), bound).expect("a frame");
    assert_eq!(read.map(|b| b.len()), Some(bound));
}

// verifies: REQ-MDL-11, THR-57
#[test]
fn module_repeating_secret_lookups_or_refused_calls_hits_the_limit() {
    let lookup = |part| Step::LookUpSecret {
        part,
        operation: OPERATION.to_owned(),
        name: "own.key".to_owned(),
    };
    for language in languages() {
        // Lookups, of the outside part and of the sandboxed part that has no route to the proxy.
        for part in [PartName::Outside, PartName::Sandboxed] {
            let mut binding = binding("reserve");
            let provider = format!("looker-{language}-{part:?}").to_lowercase();
            install(
                &mut binding,
                &module(
                    declaration(&provider, "1.0.0", &["withdrawal"]),
                    (0..4).map(|_| lookup(part)).collect(),
                ),
                &language,
                "reserve",
                &[],
            );
            binding.set_bounds(tight(|b| b.per_operation.secret_lookups = 2));
            let pid = binding.process(&provider);
            binding.take_events();
            let handled = binding.handle(&provider, &withdrawal(&provider, 1));
            assert_eq!(
                handled,
                Handled::Refused(PlatformRefusal::LimitReached),
                "{language} {part:?}: lookups went uncounted"
            );
            binding.take_events();
            let limits = binding.take_limits();
            assert_eq!(limits.len(), 1, "{limits:?}");
            assert_eq!(limits[0].hit, operation(Metered::SecretLookups));
            assert_eq!(binding.process(&provider), pid);
        }
        // Calls the platform refuses for what the caller declares count like any other.
        let mut binding = binding("reserve");
        let caller = format!("refused-{language}");
        let steps = (0..4)
            .map(|_| Step::CallModule {
                callee: format!("nobody-{language}"),
                kind: OperationKind::Withdrawal,
            })
            .collect();
        install(
            &mut binding,
            &module(calling(&caller, None), steps),
            &language,
            "reserve",
            &[],
        );
        binding.set_bounds(tight(|b| b.per_operation.module_calls = 2));
        binding.take_events();
        let handled = binding.handle(&caller, &withdrawal(&caller, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::LimitReached),
            "{language}: refused calls went uncounted"
        );
        let events = binding.take_events();
        let limits = binding.take_limits();
        assert_eq!(limits.len(), 1, "{limits:?}");
        assert_eq!(limits[0].hit, operation(Metered::ModuleCalls));
        assert_eq!(
            events.calls.len(),
            3,
            "two refused and the one over the limit"
        );
        assert!(events.calls.iter().all(|c| matches!(
            &c.outcome,
            CrossModuleOutcome::Refused(r) if r.reason == CallRefusalReason::UndeclaredCallee
        )));
    }
}

// verifies: REQ-MDL-11, REQ-MDL-12
#[test]
fn an_output_too_large_for_the_frame_is_refused_as_oversized_and_the_process_keeps_running() {
    // The C stand-in builds a padded output by reallocating in 512-byte steps, never freeing, so it runs out of
    // its memory long before 2 MB; only the Rust stand-in can make an output this size.
    for language in languages().into_iter().filter(|l| l == "rs") {
        let mut binding = binding("reserve");
        let (big, steady) = (format!("big-{language}"), format!("steady-{language}"));
        install(
            &mut binding,
            &spec(&steady, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        let module = ModuleSpec {
            declaration: declaration(&big, "1.0.0", &["withdrawal"]),
            steps: Vec::new(),
            output: Output::PaddedTo(2_200_000),
            starts: true,
        };
        install(&mut binding, &module, &language, "reserve", &[]);
        // The platform's own limit is above the output: the frame, not the limit, is what it doesn't fit.
        binding.set_output_limit(8 * 1024 * 1024);
        let pid = binding.process(&big);
        let handled = binding.handle(&big, &withdrawal(&big, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::OversizedOutput),
            "{language}: an output of 2.2 MB"
        );
        assert_eq!(binding.process(&big), pid, "{language}: the process ended");
        assert!(answers_as(&mut binding, &steady, "1.0.0", 2));
    }
}

// verifies: REQ-MDL-11, THR-57
#[test]
fn module_repeating_a_call_to_a_callee_that_is_no_identifier_hits_the_limit() {
    for language in languages() {
        let mut binding = binding("reserve");
        let caller = format!("badcall-{language}");
        let steps = (0..4)
            .map(|_| Step::CallModule {
                callee: "BAD_CALLEE!".to_owned(),
                kind: OperationKind::Withdrawal,
            })
            .collect();
        install(
            &mut binding,
            &module(calling(&caller, None), steps),
            &language,
            "reserve",
            &[],
        );
        binding.set_bounds(tight(|b| b.per_operation.module_calls = 2));
        let pid = binding.process(&caller);
        binding.take_events();
        let handled = binding.handle(&caller, &withdrawal(&caller, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::LimitReached),
            "{language}: calls to an invalid callee went uncounted"
        );
        binding.take_events();
        let limits = binding.take_limits();
        assert_eq!(limits.len(), 1, "{limits:?}");
        assert_eq!(limits[0].hit, operation(Metered::ModuleCalls));
        assert_eq!(binding.process(&caller), pid);
    }
}

// verifies: REQ-MDL-11, THR-57
#[test]
fn module_repeating_lookups_through_the_credential_proxy_hits_the_limit_and_resolves_no_more() {
    for language in languages() {
        // Resolved lookups, and lookups of a name the declaration doesn't have: both are counted before the proxy
        // answers or refuses.
        for name in ["own.key", "not.declared"] {
            let mut binding = binding("reserve");
            let provider = format!("proxied-{language}-{}", name.replace('.', "-"));
            binding.seed_secret(&provider, "own.key", "canary-secret");
            let steps = (0..5)
                .map(|_| Step::LookUpSecret {
                    part: PartName::Outside,
                    operation: OPERATION.to_owned(),
                    name: name.to_owned(),
                })
                .collect();
            install(
                &mut binding,
                &module(declaration(&provider, "1.0.0", &["withdrawal"]), steps),
                &language,
                "reserve",
                &[],
            );
            binding.set_bounds(tight(|b| b.per_operation.secret_lookups = 3));
            binding.take_events();
            let handled = binding.handle(&provider, &withdrawal(&provider, 1));
            assert_eq!(
                handled,
                Handled::Refused(PlatformRefusal::LimitReached),
                "{language} {name}"
            );
            let events = binding.take_events();
            let limits = binding.take_limits();
            assert_eq!(limits.len(), 1, "{limits:?}");
            assert_eq!(limits[0].hit, operation(Metered::SecretLookups));
            assert_eq!(
                events.lookups.len(),
                3,
                "{language} {name}: lookups past the bound were answered"
            );
            let resolved = binding.delivered_secrets(&provider).len();
            assert_eq!(
                resolved,
                if name == "own.key" { 3 } else { 0 },
                "{language} {name}"
            );
        }
    }
}

// verifies: REQ-MDL-11, THR-57
#[test]
fn module_repeating_lookups_the_proxy_refuses_hits_the_limit() {
    for language in languages() {
        let mut binding = binding("reserve");
        let provider = format!("prober-{language}");
        let steps = (0..4)
            .map(|_| Step::LookUpSecret {
                part: PartName::Outside,
                operation: OPERATION.to_owned(),
                name: "someone.elses.key".to_owned(),
            })
            .collect();
        install(
            &mut binding,
            &module(declaration(&provider, "1.0.0", &["withdrawal"]), steps),
            &language,
            "reserve",
            &[],
        );
        binding.set_bounds(tight(|b| b.per_operation.secret_lookups = 2));
        binding.take_events();
        let handled = binding.handle(&provider, &withdrawal(&provider, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::LimitReached),
            "{language}: lookups the proxy refuses went uncounted"
        );
        let events = binding.take_events();
        let limits = binding.take_limits();
        assert_eq!(limits.len(), 1, "{limits:?}");
        assert_eq!(limits[0].hit, operation(Metered::SecretLookups));
        // The two within the bound were answered by the proxy, refused; none resolved.
        assert_eq!(events.lookups.len(), 2);
        assert!(binding.delivered_secrets(&provider).is_empty());
    }
}
