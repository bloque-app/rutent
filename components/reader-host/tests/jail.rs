//! A reader's process has no network route but the reader host (`T-015-04` part 4, `REQ-MDL-7`). The jail is Linux's,
//! so off Linux the test is ignored with its reason; `.github/workflows/jail-probes.yml` runs it on Linux beside the
//! provider jail's probes, and fails if it is skipped. It prints the run id of the CI run it ran in.

mod common;

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
    use std::time::Duration;

    use super::common::*;
    use rutent_contracts::reader::{Answered, Standing};

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
