//! One OS process per provider (ADR-0003, `REQ-MDL-13`, `B-07`), and what a guest's failure reaches (`B-05`).

mod common;

use common::{alive, answers_as, binding, install, languages, spec};
use rutent_conformance::boundary::{Admitted, Handled, ModuleBinding, PlatformRefusal};
use rutent_wasm_binding::children::ProviderProcess;
use rutent_wasm_binding::program::Probe;
use rutent_wasm_binding::wire::sha256;

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
    let started = ProviderProcess::spawn(&common::paths(), &dir, &machine, [&sandboxed, &outside]);
    assert!(started.is_ok(), "the admitted machine code didn't start");
    for admitted in [
        [wrong.as_str(), outside.as_str()],
        [sandboxed.as_str(), wrong.as_str()],
    ] {
        let refused = ProviderProcess::spawn(&common::paths(), &dir, &machine, admitted);
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
