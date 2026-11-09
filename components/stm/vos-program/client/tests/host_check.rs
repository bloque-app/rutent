//! The production-host check (`T-010-08`, `G-37`), `../../host-check.sh`: it fails unless the kernel is 6.7 or later,
//! userfaultfd write-protect is allowed and the JIT's perf map appears. The script's three facts are read from the
//! host; each has a hook (`HOST_CHECK_KERNEL`, `HOST_CHECK_OS`, `HOST_CHECK_UFFD`, `HOST_CHECK_PERF`) so these tests
//! can stand in for a host that lacks one. They need no VOS and no node, and run on any machine with a POSIX shell.
//! The check against the real host is CI's step in `stm-vos.yml`, and `F-030`'s on the production host.

use std::path::PathBuf;
use std::process::{Command, Output};

/// A host that gives all three, as the hooks say it.
const GOOD: [(&str, &str); 4] = [
    ("HOST_CHECK_OS", "Linux x86_64"),
    ("HOST_CHECK_KERNEL", "6.8.0-1-azure"),
    ("HOST_CHECK_UFFD", "true"),
    ("HOST_CHECK_PERF", "true"),
];

/// The check on a host that is `GOOD` but for `changes`.
fn check(changes: &[(&str, &str)]) -> Output {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../host-check.sh");
    let mut command = Command::new("sh");
    command.arg(script);
    for (name, value) in GOOD {
        command.env(name, value);
    }
    for (name, value) in changes {
        command.env(name, value);
    }
    command.output().expect("sh runs the script")
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

// verifies: REQ-OPS-3
#[test]
fn host_check_fails_on_a_host_without_userfaultfd_write_protect() {
    let output = check(&[("HOST_CHECK_UFFD", "false")]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(said(&output).contains("userfaultfd"), "{}", said(&output));
}

// verifies: REQ-OPS-3
#[test]
fn host_check_fails_on_a_kernel_older_than_6_7() {
    for kernel in [
        "6.6.30-generic",
        "5.15.0-1",
        "4.19",
        "6.7-rc1",
        "6.7.0-rc3-generic",
    ] {
        let output = check(&[("HOST_CHECK_KERNEL", kernel)]);
        assert!(!output.status.success(), "{kernel}: {}", said(&output));
        assert!(said(&output).contains("kernel"), "{}", said(&output));
    }
}

// verifies: REQ-OPS-3
#[test]
fn host_check_fails_when_the_jit_perf_map_does_not_appear() {
    let output = check(&[("HOST_CHECK_PERF", "false")]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(said(&output).contains("perf map"), "{}", said(&output));
}

// verifies: REQ-OPS-3
#[test]
fn host_check_fails_off_linux_x86_64() {
    let output = check(&[("HOST_CHECK_OS", "Darwin arm64")]);
    assert!(!output.status.success(), "{}", said(&output));
}

// verifies: REQ-OPS-3
#[test]
fn host_check_reports_every_failing_fact_and_not_only_the_first() {
    let output = check(&[
        ("HOST_CHECK_KERNEL", "6.1.0"),
        ("HOST_CHECK_UFFD", "false"),
        ("HOST_CHECK_PERF", "false"),
    ]);
    let text = said(&output);
    assert!(!output.status.success(), "{text}");
    for fact in ["kernel", "userfaultfd", "perf map"] {
        assert!(text.contains(fact), "{fact} is not reported: {text}");
    }
}

// verifies: REQ-OPS-3
#[test]
fn host_check_passes_on_a_host_that_gives_all_three() {
    for kernel in [
        "6.7.0",
        "6.7",
        "6.8.0-1-azure",
        "7.0.0-1019-azure",
        "10.1.2",
    ] {
        let output = check(&[("HOST_CHECK_KERNEL", kernel)]);
        assert!(output.status.success(), "{kernel}: {}", said(&output));
    }
}

// verifies: REQ-OPS-3
#[test]
fn host_check_without_a_build_fails_for_want_of_a_perf_map() {
    // No hook and no build directory: the perf map cannot appear, so the check fails rather than skipping it.
    let output = check(&[("HOST_CHECK_PERF", "")]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(said(&output).contains("perf map"), "{}", said(&output));
}

/// The userfaultfd probe against a fake kernel that checks the size of every ioctl's struct (`uffd-probe.py`).
fn probe(mode: &str) -> Output {
    let probe = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../uffd-probe.py");
    Command::new("python3")
        .arg(probe)
        .env("UFFD_PROBE_FAKE", mode)
        .output()
        .expect("python3 runs the probe")
}

// verifies: REQ-OPS-3
#[test]
fn uffd_probe_passes_on_a_kernel_that_answers_every_ioctl_in_its_exact_struct() {
    let output = probe("ok");
    assert_eq!(output.status.code(), Some(0), "{}", said(&output));
}

// verifies: REQ-OPS-3
#[test]
fn uffd_probe_reports_not_allowed_for_a_refusal_and_for_a_missing_feature_bit() {
    for mode in ["eperm", "nofeature"] {
        let output = probe(mode);
        assert_eq!(output.status.code(), Some(1), "{mode}: {}", said(&output));
    }
}

// verifies: REQ-OPS-3
#[test]
fn uffd_probe_reports_its_own_error_apart_from_a_host_that_does_not_allow_it() {
    let output = probe("errno");
    assert_eq!(output.status.code(), Some(2), "{}", said(&output));
    assert!(said(&output).contains("SCRIPT ERROR"), "{}", said(&output));
}

// verifies: REQ-OPS-3
#[test]
fn host_check_says_a_failed_probe_is_a_script_error_and_still_fails() {
    let output = check(&[("HOST_CHECK_UFFD", "exit 2")]);
    assert!(!output.status.success(), "{}", said(&output));
    assert!(said(&output).contains("script error"), "{}", said(&output));
}
