//! A reader's process has no network route but the reader host (`T-015-04` part 4, `REQ-MDL-7`), and its address space
//! is bounded by the host, whatever the reader does itself (`T-015-05`, `REQ-MDL-11`). The jail is Linux's, so off
//! Linux the tests are ignored with their reason; `.github/workflows/jail-probes.yml` runs them on Linux beside the
//! provider jail's probes, and fails if either is skipped. Each prints the run id of the CI run it ran in.

mod common;

// verifies: REQ-MDL-11
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the reader's jail is Linux's: the jail-probe job runs this"
)]
fn reader_that_skips_its_own_memory_limit_is_refused_by_the_hosts_check() {
    #[cfg(target_os = "linux")]
    linux::memory_bounded_by_the_host();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: REQ-MDL-7
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the reader's jail is Linux's: the jail-probe job runs this"
)]
fn reader_process_has_no_network_route_but_the_reader_host() {
    #[cfg(target_os = "linux")]
    linux::no_route_but_the_host();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::ErrorKind;
    use std::net::{TcpListener, TcpStream};
    use std::process::Child;
    use std::time::{Duration, Instant};

    use super::common::*;
    use rutent_contracts::reader::{Answered, Standing};
    use rutent_prov::confine::{Limits, PROVIDER_ADDRESS_SPACE, isolated};
    use rutent_reader_host::jail::{check_confined, command, workdir};

    /// The one child of `launched`, `unshare`: the reader's process.
    fn reader_of(launched: u32) -> Option<u32> {
        let wanted = format!("PPid:\t{launched}");
        let children: Vec<u32> = std::fs::read_dir("/proc")
            .ok()?
            .flatten()
            .filter_map(|entry| {
                let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
                let status = std::fs::read_to_string(entry.path().join("status")).ok()?;
                status.lines().any(|l| l == wanted).then_some(pid)
            })
            .collect();
        match children.as_slice() {
            [reader] => Some(*reader),
            _ => None,
        }
    }

    /// The host's check of the reader `child` launched, once the reader's process is there.
    fn verdict(child: &Child) -> Option<&'static str> {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let verdict = check_confined(child.id());
            if verdict != Some("reader_process_not_found") || Instant::now() >= until {
                return verdict;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The reader's address space limit, soft and hard, as the kernel reports it.
    fn address_space(child: &Child) -> Option<String> {
        let reader = reader_of(child.id())?;
        let limits = std::fs::read_to_string(format!("/proc/{reader}/limits")).ok()?;
        limits
            .lines()
            .find_map(|l| l.strip_prefix("Max address space"))
            .map(|rest| {
                rest.split_whitespace()
                    .take(2)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
    }

    fn stop(mut child: Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    pub fn memory_bounded_by_the_host() {
        let run = std::env::var("GITHUB_RUN_ID").unwrap_or_else(|_| "local".to_owned());
        // A reader that skips its own limit, and with it the rest of the jail's in-process half.
        let reader = rpc_reader("loose", &["unconfined"]);
        let args: Vec<&str> = reader.args.iter().map(String::as_str).collect();
        let bound = format!("{PROVIDER_ADDRESS_SPACE} {PROVIDER_ADDRESS_SPACE}");
        // Every expectation is read before any fails, so a run shows each that doesn't hold.
        let mut failed = Vec::new();

        // Started without the host's own bound (the provider's launcher alone), its address space is unbounded, and
        // the host's check refuses it for that, first.
        let dir = workdir(&std::env::temp_dir(), "loose", 1).expect("a working directory");
        let bare = isolated(&reader.program, &args, &dir, Limits::PROVIDER)
            .expect("the launcher")
            .spawn()
            .expect("started");
        let (refused, unbounded) = (verdict(&bare), address_space(&bare));
        eprintln!(
            "jail-probe run {run}: reader-host: unbounded reader: {refused:?}, {unbounded:?}"
        );
        if refused != Some("reader_memory_unbounded") {
            failed.push(format!(
                "a reader with no address-space bound is refused for it: {refused:?}"
            ));
        }
        if unbounded.as_deref() == Some(bound.as_str()) {
            failed.push(format!(
                "the provider's launcher alone set no bound: {unbounded:?}"
            ));
        }
        stop(bare);

        // Started by the host's launcher, it is bounded at a provider's process's bound, soft and hard, though it set
        // nothing itself: the check holds the bound, and refuses it only for the rest of its jail.
        let dir_hosted = workdir(&std::env::temp_dir(), "loose", 2).expect("a working directory");
        let hosted = command(&reader, &dir_hosted)
            .expect("the host's launcher")
            .spawn()
            .expect("started");
        let (refused, bounded) = (verdict(&hosted), address_space(&hosted));
        eprintln!("jail-probe run {run}: reader-host: hosted reader: {refused:?}, {bounded:?}");
        if bounded.as_deref() != Some(bound.as_str()) {
            failed.push(format!(
                "the host bounds the reader's address space at {bound}: {bounded:?}"
            ));
        }
        if refused != Some("reader_not_confined") {
            failed.push(format!(
                "the host's bound holds, and the reader is refused for the rest of its jail: {refused:?}"
            ));
        }
        stop(hosted);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir_hosted);

        assert!(failed.is_empty(), "{failed:#?}");
        eprintln!(
            "jail-probe run {run}: reader-host: reader_that_skips_its_own_memory_limit_is_refused_by_the_hosts_check: held"
        );
    }

    pub fn no_route_but_the_host() {
        let run = std::env::var("GITHUB_RUN_ID").unwrap_or_else(|_| "local".to_owned());
        // A port on the host's own loopback, which the host's own process reaches.
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().unwrap();
        TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .expect("the host reaches its own port");
        let _ = listener.accept().expect("and it is accepted");
        listener.set_nonblocking(true).unwrap();

        let probe = format!("probe-network={address}");
        let reader = rpc_reader("base", &[probe.as_str()]);
        let rig = Rig::start(vec![reader.clone()], honest_pool(), &[]);
        let ready = rig.host.wait_ready(&reader.name, Duration::from_secs(30));
        let log = rig.host.log(&reader.name);
        eprintln!("jail-probe run {run}: reader-host: {log:?}");
        assert!(
            ready,
            "the jailed reader started and passed its checks through the host: {log:?}"
        );

        // Its one channel is the host: its checks went through the host's egress, and so does a question.
        let answer = ask(&rig.host, &reader, status_of("0xmove"));
        assert!(
            matches!(
                answer.answer,
                Answered::MovementStatus {
                    standing: Standing::Final(_)
                }
            ),
            "{answer:?}"
        );

        // Nothing else: not the host's own loopback, not a public address over IPv4 or IPv6, and no route at all.
        for target in [
            address.to_string(),
            "93.184.216.34:443".to_owned(),
            "[2606:2800:220:1::]:443".to_owned(),
        ] {
            let line = log
                .iter()
                .find(|l| l.starts_with(&format!("probe {target}: ")))
                .unwrap_or_else(|| panic!("the reader probed {target}: {log:?}"));
            assert!(
                line.contains("refused"),
                "the reader reached {target}: {line}"
            );
        }
        // No route to read, or none to read it with: the jail's root is empty.
        assert!(
            log.iter()
                .any(|l| l == "probe routes: 0" || l.starts_with("probe routes: unreadable")),
            "the reader has no route: {log:?}"
        );
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == ErrorKind::WouldBlock),
            "nothing connected to the host's port from the reader"
        );

        // A reader that skips the jail's in-process half (the empty root, capabilities, Landlock, seccomp) is never
        // taken: the host reads its status and refuses it before it serves it anything. It probes the network first,
        // with only the namespaces around it and no seccomp: what refuses it then is the network namespace alone (no
        // route: `NetworkUnreachable`, `AddrNotAvailable` or `HostUnreachable`, never seccomp's `PermissionDenied`).
        let loose = rpc_reader("loose", &["unconfined", probe.as_str()]);
        let loose_rig = Rig::start(vec![loose.clone()], honest_pool(), &[]);
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !loose_rig
            .host
            .log(&loose.name)
            .iter()
            .any(|l| l.contains("not confined: reader_not_confined"))
        {
            assert!(
                std::time::Instant::now() < until,
                "the unconfined reader was refused: {:?}",
                loose_rig.host.log(&loose.name)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !loose_rig
                .host
                .wait_ready(&loose.name, Duration::from_secs(1))
        );
        assert!(unreadable(&ask(&loose_rig.host, &loose, status_of("0xmove"))).is_some());
        let loose_log = loose_rig.host.log(&loose.name);
        eprintln!("jail-probe run {run}: reader-host: unconfined: {loose_log:?}");
        for target in [
            address.to_string(),
            "93.184.216.34:443".to_owned(),
            "[2606:2800:220:1::]:443".to_owned(),
        ] {
            let line = loose_log
                .iter()
                .find(|l| l.starts_with(&format!("probe {target}: ")))
                .unwrap_or_else(|| panic!("the unconfined reader probed {target}: {loose_log:?}"));
            // No route in the namespace: no interface is up for IPv4, and no IPv6 source address at all.
            let kind = line
                .split("refused: ")
                .nth(1)
                .and_then(|rest| rest.split(' ').next());
            let no_route = matches!(
                kind,
                Some("NetworkUnreachable" | "AddrNotAvailable" | "HostUnreachable")
            );
            assert!(
                no_route && !line.contains("PermissionDenied") && !line.contains("connected"),
                "the network namespace alone refuses {target}: {line}"
            );
        }
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == ErrorKind::WouldBlock),
            "nothing connected to the host's port from the unconfined reader"
        );
        eprintln!(
            "jail-probe run {run}: reader-host: reader_process_has_no_network_route_but_the_reader_host: held"
        );
    }
}
