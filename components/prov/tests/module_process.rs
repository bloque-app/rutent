//! One OS process per provider (ADR-0003, `REQ-MDL-13`, `B-07`), and what a guest's failure reaches (`B-05`).

#[path = "module_support/mod.rs"]
mod common;

use common::Probe;
use common::{alive, answers_as, binding, install, languages, spec};
use rutent_conformance::boundary::{
    Admitted, BoundaryHarness, Endpoint, Handled, ModuleBinding, PlatformRefusal, Step,
};
use rutent_contracts::module_interface::Time;
use rutent_prov::catalogue::Instant;
use rutent_prov::module::children::{ProviderProcess, Stderrs};
use rutent_prov::module::platform::OPERATION_DEADLINE;
use rutent_prov::module::wire::sha256;

// verifies: REQ-MDL-13, AC-19
#[test]
fn restarting_one_providers_process_leaves_the_others_running() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (steady, moving) = (format!("steady-{language}"), format!("moving-{language}"));
        for provider in [&steady, &moving] {
            let admitted = install(
                &mut binding,
                &spec(provider, "1.0.0", Vec::new(), true),
                &language,
                "reserve",
                &[],
            );
            assert!(
                matches!(admitted, Admitted::Artefact(_)),
                "{provider} wasn't admitted"
            );
        }
        let steady_pid = binding.process(&steady).expect("the steady provider runs");
        let moving_pid = binding.process(&moving).expect("the moving provider runs");
        assert_ne!(steady_pid, moving_pid, "two providers share a process");
        assert!(
            alive(steady_pid) && alive(moving_pid),
            "a provider's process isn't a live OS process"
        );
        assert_ne!(
            steady_pid,
            u64::from(std::process::id()),
            "a provider runs in the platform's process"
        );

        assert!(binding.restart(&moving), "the restart was refused");
        let restarted = binding
            .process(&moving)
            .expect("the restarted provider runs");
        assert_ne!(restarted, moving_pid, "the restart kept the old process");
        assert!(!alive(moving_pid), "the old process is still running");
        assert!(alive(restarted), "the new process isn't running");
        assert!(
            answers_as(&mut binding, &moving, "1.0.0", 1),
            "the restarted provider doesn't answer"
        );

        assert_eq!(
            binding.process(&steady),
            Some(steady_pid),
            "the other provider's process changed"
        );
        assert!(alive(steady_pid), "the other provider's process stopped");
        assert!(
            answers_as(&mut binding, &steady, "1.0.0", 2),
            "the other provider stopped answering"
        );
    }
}

// verifies: REQ-MDL-13, REQ-MDL-11
#[test]
fn a_trapping_guest_ends_only_its_own_operation() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (steady, trapping) = (format!("steady-{language}"), format!("trapping-{language}"));
        install(
            &mut binding,
            &spec(&steady, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        install(
            &mut binding,
            &spec(&trapping, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Trap],
        );
        let (steady_pid, trapping_pid) = (binding.process(&steady), binding.process(&trapping));
        let handled = binding.handle(&trapping, &common::withdrawal(&trapping, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::Unavailable),
            "a trap gave an answer"
        );
        assert_eq!(
            binding.process(&trapping),
            trapping_pid,
            "a trap ended the provider's process"
        );
        assert_eq!(
            binding.process(&steady),
            steady_pid,
            "a trap disturbed another provider's process"
        );
        assert!(
            answers_as(&mut binding, &steady, "1.0.0", 2),
            "another provider stopped answering"
        );
        // Its parts are set up again from the same machine code, and it can be asked again.
        let again = binding.handle(&trapping, &common::withdrawal(&trapping, 3));
        assert_eq!(
            again,
            Handled::Refused(PlatformRefusal::Unavailable),
            "the trap didn't happen again"
        );
    }
}

// verifies: REQ-MDL-13
#[test]
fn a_killed_provider_process_is_started_again_and_the_others_keep_answering() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (steady, crashing) = (format!("steady-{language}"), format!("crashing-{language}"));
        install(
            &mut binding,
            &spec(&steady, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        install(
            &mut binding,
            &spec(&crashing, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        let (steady_pid, crashed_pid) = (binding.process(&steady), binding.process(&crashing));
        assert!(binding.kill(&crashing), "nothing to kill");
        let handled = binding.handle(&crashing, &common::withdrawal(&crashing, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::Unavailable),
            "a dead process answered"
        );
        let respawned = binding.process(&crashing);
        assert!(
            respawned.is_some() && respawned != crashed_pid,
            "the platform didn't start it again"
        );
        assert_eq!(
            binding.process(&steady),
            steady_pid,
            "another provider's process changed"
        );
        assert!(
            answers_as(&mut binding, &steady, "1.0.0", 2),
            "another provider stopped answering"
        );
        assert!(
            answers_as(&mut binding, &crashing, "1.0.0", 3),
            "the started-again provider doesn't answer"
        );
    }
}

// verifies: REQ-MDL-2, REQ-MDL-1
#[test]
fn a_provider_process_refuses_machine_code_that_isnt_the_admitted_one_before_loading_it() {
    let language = languages().remove(0);
    let mut binding = binding("reserve");
    let provider = format!("admitted-{language}");
    install(
        &mut binding,
        &spec(&provider, "1.0.0", Vec::new(), true),
        &language,
        "reserve",
        &[],
    );
    let machine = binding
        .machine(&provider, "1.0.0")
        .expect("its machine code");
    let dir = std::env::temp_dir().join(format!("rutent-wasm-test-digests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a work directory");
    let (sandboxed, outside) = (sha256(&machine.sandboxed), sha256(&machine.outside));
    let wrong = "0".repeat(64);
    let stderrs = Stderrs::default();
    let started = ProviderProcess::spawn(
        &common::paths(),
        &dir,
        &provider,
        &machine,
        [&sandboxed, &outside],
        &stderrs,
        OPERATION_DEADLINE,
    );
    assert!(started.is_ok(), "the admitted machine code didn't start");
    for admitted in [
        [wrong.as_str(), outside.as_str()],
        [sandboxed.as_str(), wrong.as_str()],
    ] {
        let refused = ProviderProcess::spawn(
            &common::paths(),
            &dir,
            &provider,
            &machine,
            admitted,
            &stderrs,
            OPERATION_DEADLINE,
        );
        let Err(why) = refused else {
            panic!("machine code other than the admitted one was started")
        };
        assert!(
            why.contains("isn't the one its admission artefact records"),
            "refused for another reason: {why}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

// verifies: AC-21, REQ-MDL-3, REQ-MDL-13, AC-19
#[test]
fn a_pending_release_leaves_the_running_version_in_force_across_a_restart() {
    for language in languages() {
        let mut binding = binding("reserve");
        let provider = format!("pending-{language}");
        let running = spec(&provider, "1.0.0", Vec::new(), true);
        install(&mut binding, &running, &language, "reserve", &[]);
        // 1.1.0 widens by a destination, so it waits for a fresh acceptance.
        let mut declaration = serde_json::to_value(&running.declaration).expect("JSON");
        declaration["version"] = serde_json::Value::from("1.1.0");
        declaration["outside"]["destinations"]
            .as_array_mut()
            .expect("destinations")
            .push(serde_json::json!({
                "name": "extra", "scheme": "https", "host": format!("extra.{provider}.example"), "port": 443,
                "addresses": "public",
            }));
        declaration["outside"]["operations"][0]["destinations"]
            .as_array_mut()
            .expect("an operation's destinations")
            .push(serde_json::Value::from("extra"));
        let widened = rutent_conformance::boundary::ModuleSpec {
            declaration: serde_json::from_value(declaration).expect("a valid declaration"),
            ..running.clone()
        };
        let binaries = binding
            .build_with(&widened, &language, "reserve", &[])
            .expect("a stand-in");
        let admitted = binding.admit(
            &common::assertion(&widened.declaration, &binaries),
            &binaries,
        );
        assert!(
            matches!(&admitted, Admitted::Artefact(a) if a.acceptance.accepted.is_none()),
            "the widening release didn't wait"
        );
        // The running version can't be admitted again, under any declaration, while 1.1.0 waits.
        let again = binding
            .build_with(&running, &language, "reserve", &[Probe::Trap])
            .expect("a stand-in");
        let readmitted = binding.admit(&common::assertion(&running.declaration, &again), &again);
        assert!(
            matches!(readmitted, Admitted::Refused(_)),
            "a version already admitted was admitted again"
        );
        assert!(binding.restart(&provider), "the restart was refused");
        assert_eq!(binding.active_version(&provider).as_deref(), Some("1.0.0"));
        assert!(
            answers_as(&mut binding, &provider, "1.0.0", 1),
            "the running version stopped answering"
        );
        binding
            .accept(&provider, "1.1.0", "test-reviewer")
            .expect("the pending release");
        assert!(
            answers_as(&mut binding, &provider, "1.1.0", 2),
            "the accepted release didn't run"
        );
        assert!(binding.restart(&provider), "the second restart was refused");
        assert!(
            answers_as(&mut binding, &provider, "1.1.0", 3),
            "the accepted release didn't survive a restart"
        );
    }
}

// verifies: REQ-MDL-11, NFR-3
#[test]
fn restarts_do_not_grow_platform_memory() {
    const CAP: usize = 3;
    for language in languages().into_iter().take(1) {
        let mut binding = binding("reserve");
        binding.set_measurement_cap(CAP);
        binding.set_stderr_capacity(CAP);
        let providers: Vec<String> = (0..5)
            .map(|n| format!("restarted-{language}-{n}"))
            .collect();
        for provider in &providers {
            install(
                &mut binding,
                &spec(provider, "1.0.0", Vec::new(), true),
                &language,
                "reserve",
                &[],
            );
        }
        // Many more restarts than anything platform-side may keep a trace of.
        for round in 0..6 {
            for provider in &providers {
                assert!(binding.restart(provider), "{provider} didn't restart");
            }
            assert!(
                answers_as(&mut binding, &providers[0], "1.0.0", round + 1),
                "a restarted provider stopped answering"
            );
        }
        let measurements = &binding.measurements;
        assert_eq!(
            measurements.starts.len(),
            CAP,
            "the starts it keeps aren't the newest {CAP} of {}",
            providers.len() * 7
        );
        assert_eq!(measurements.compiles.len(), CAP);
        assert!(binding.stderrs().len() <= CAP);
        // Each restart's process left nothing behind: one working directory per process still running.
        let directories = std::fs::read_dir(binding.workdir())
            .expect("the platform's work directory")
            .count();
        assert_eq!(
            directories,
            binding.processes().len(),
            "a restart left a working directory, or a process, behind"
        );
        assert_eq!(binding.processes().len(), providers.len());
    }
}

/// A guest that makes an outbound call and then traps may have acted: its operation is refused as
/// `OutcomeUnknown`, never `Unavailable`, which says nothing of it took effect. The call's record says what became
/// of it, and is dated by the world's clock. A guest that traps before passing anything out is still
/// `Unavailable`.
// verifies: REQ-ORD-1, REQ-MDL-8, REQ-AUD-1
#[test]
fn a_guest_that_traps_after_an_outbound_call_has_an_unknown_outcome() {
    for language in languages() {
        let mut binding = binding("reserve");
        let provider = format!("calling-{language}");
        let endpoint = Endpoint::https(&format!("api.{provider}.example"));
        binding.serve(
            &endpoint,
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(185, 12, 64, 40)),
            None,
        );
        install(
            &mut binding,
            &spec(
                &provider,
                "1.0.0",
                vec![Step::Call {
                    operation: common::OPERATION.to_owned(),
                    endpoint: endpoint.clone(),
                }],
                true,
            ),
            &language,
            "reserve",
            &[Probe::Trap],
        );
        // 2026-12-01T00:00:00Z.
        binding.world.set_clock(Instant(1_796_083_200));
        binding.take_events();
        let handled = binding.handle(&provider, &common::withdrawal(&provider, 1));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::OutcomeUnknown),
            "{language}: a trap after an outbound call was given another reason"
        );
        let events = binding.take_events();
        assert_eq!(events.records.len(), 1, "{language}: {:?}", events.records);
        assert_eq!(
            events.records[0].started_at,
            Time::new("2026-12-01T00:00:00Z").expect("a time"),
            "{language}: the record isn't dated by the world's clock"
        );
        assert_eq!(binding.received(&endpoint).len(), 1, "{language}");

        // The control: a trap with nothing passed out is still `Unavailable`.
        let quiet = format!("quiet-{language}");
        install(
            &mut binding,
            &spec(&quiet, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Trap],
        );
        assert_eq!(
            binding.handle(&quiet, &common::withdrawal(&quiet, 1)),
            Handled::Refused(PlatformRefusal::Unavailable),
            "{language}"
        );
    }
}
