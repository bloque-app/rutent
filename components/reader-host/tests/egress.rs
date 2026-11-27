//! A reader's egress, enforced by the host (`T-015-04` part 4): only what the reader declares, every redirect checked,
//! a light client by protocol and port with a cap, and every call recorded with destination, reader, version and
//! request.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::*;
use rutent_contracts::module_interface::{CallOutcome, RefusalReason, Refused};
use rutent_contracts::reader::{Answered, Standing};
use rutent_prov::egress::Response;
use rutent_reader_host::config::Protocol;
use rutent_reader_host::records::{CallRecord, ConnectionOutcome, START_UP};

const READY: Duration = Duration::from_secs(30);

fn redirecting_for_movements(to: &'static str) -> Answerer {
    let node = evm_node(false);
    Box::new(move |body: &str| {
        if body.contains("rutent_movementStatus") {
            Ok(Response {
                status: 302,
                headers: vec![("location".to_owned(), to.to_owned())],
                body: Vec::new(),
            })
        } else {
            node(body)
        }
    })
}

// verifies: REQ-MDL-7, REQ-MDL-8, THR-79
#[test]
fn readers_call_to_an_endpoint_it_did_not_declare_is_refused_by_the_reader_host_and_recorded() {
    // It asks an endpoint it doesn't declare too, and answers whatever the pool said: the host holds it.
    let straying = rpc_reader("base-straying", &["undeclared", "ignore-quorum"]);
    let keeping = rpc_reader("base-keeping", &[]);
    let connector = honest_pool();
    // The undeclared endpoint would answer as an honest node does, if the host ever reached it.
    connector.set("evil.example", evm_node(false));
    let rig = Rig::start(
        vec![straying.clone(), keeping.clone()],
        connector.clone(),
        &[],
    );
    for reader in [&straying, &keeping] {
        assert!(rig.host.wait_ready(&reader.name, READY));
    }

    let question = question("base-straying", "base", status_of("0xmove"));
    let bytes = rig
        .host
        .answer(&serde_json::to_vec(&question).unwrap())
        .unwrap();
    let answer = as_core_reads(&bytes, &question, &straying).statement;
    assert_eq!(unreadable(&answer), Some("trust_rule_not_met"));
    assert!(
        !connector.dialled().iter().any(|d| d == "evil.example"),
        "nothing reached it"
    );

    let refused: Vec<_> = rig
        .records
        .all()
        .into_iter()
        .filter_map(|r| match r {
            CallRecord::Request(r) if r.hops.iter().any(|h| h.host.as_str() == "evil.example") => {
                Some(r)
            }
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 1, "one record of the refused call");
    let record = &refused[0];
    assert_eq!(record.provider.as_str(), "base-straying", "the reader");
    assert_eq!(record.module_version.as_str(), "0.1.0", "its version");
    assert_eq!(
        record.transaction.as_str(),
        question.request.as_str(),
        "the request"
    );
    assert_eq!(record.hops[0].destination, None, "no declared destination");
    assert_eq!(
        record.outcome,
        CallOutcome::Refused(Refused {
            reason: RefusalReason::UndeclaredDestination
        })
    );

    // The reader that keeps to its list answers, and its calls are recorded too.
    let kept = ask(&rig.host, &keeping, status_of("0xmove"));
    assert!(
        matches!(
            kept.answer,
            Answered::MovementStatus {
                standing: Standing::Final(_)
            }
        ),
        "{kept:?}"
    );
    let records = rig.records.all();
    assert!(
        records
            .iter()
            .any(|r| r.reader() == "base-keeping" && r.request() == START_UP)
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.reader() == "base-keeping" && r.request() == kept.request.as_str())
            .count(),
        3,
        "one record per pool endpoint asked"
    );
}

// verifies: REQ-MDL-7
#[test]
fn reader_redirected_to_an_undeclared_endpoint_is_refused_by_the_reader_host() {
    let reader = rpc_reader("base", &[]);
    let connector = honest_pool();
    connector.set(
        "rpc-a.base.example",
        redirecting_for_movements("https://evil.example/"),
    );
    connector.set("evil.example", evm_node(true));
    let rig = Rig::start(vec![reader.clone()], connector.clone(), &[]);
    assert!(rig.host.wait_ready(&reader.name, READY));

    // One endpoint redirects away: refused, and the two others still agree.
    let answer = ask(&rig.host, &reader, status_of("0xmove"));
    assert!(
        matches!(&answer.answer, Answered::MovementStatus { standing: Standing::Final(m) } if m.destination.as_str() == "dest-1"),
        "{answer:?}"
    );
    assert!(
        !connector.dialled().iter().any(|d| d == "evil.example"),
        "the redirect was never followed"
    );
    let redirected: Vec<_> = rig
        .records
        .all()
        .into_iter()
        .filter_map(|r| match r {
            CallRecord::Request(r)
                if r.transaction.as_str() == answer.request.as_str() && r.hops.len() == 2 =>
            {
                Some(r)
            }
            _ => None,
        })
        .collect();
    assert_eq!(redirected.len(), 1);
    let record = &redirected[0];
    assert_eq!(record.hops[0].host.as_str(), "rpc-a.base.example");
    assert_eq!(record.hops[1].host.as_str(), "evil.example");
    assert_eq!(record.hops[1].address, None, "nothing connected to it");
    assert_eq!(
        record.outcome,
        CallOutcome::RefusedAfterDelivery(Refused {
            reason: RefusalReason::RedirectToUndeclaredDestination
        })
    );

    // Two redirect away: too few answers, unreadable.
    connector.set(
        "rpc-b.base.example",
        redirecting_for_movements("https://evil.example/"),
    );
    let answer = ask(&rig.host, &reader, status_of("0xmove"));
    assert_eq!(unreadable(&answer), Some("too_few_rpc_answers"));
    assert!(!connector.dialled().iter().any(|d| d == "evil.example"));
}

// verifies: REQ-MDL-7, REQ-MDL-11
#[test]
fn light_client_reader_reaches_only_its_declared_protocols_and_ports_and_no_more_connections_than_its_cap()
 {
    let (a, b, c) = (
        Peer::serve(GENESIS, true),
        Peer::serve(GENESIS, true),
        Peer::serve(GENESIS, true),
    );
    let reader = light_reader(
        "kreivo",
        &[
            "peer-a.kreivo.example:p2p:30333",
            "loopback.kreivo.example:p2p:30333",
            "peer-b.kreivo.example:wss:443",
            "peer-c.kreivo.example:p2p:30333",
            "peer-d.kreivo.example:p2p:8080",
            "peer-f.kreivo.example:wss:30333",
        ],
        kreivo_rule(2),
        &[],
    );
    let rig = Rig::start(
        vec![reader.clone()],
        Arc::new(StubConnector::default()),
        &[
            ("peer-a.kreivo.example", &a),
            ("peer-b.kreivo.example", &b),
            ("peer-c.kreivo.example", &c),
            ("peer-d.kreivo.example", &c),
            ("peer-f.kreivo.example", &c),
        ],
    );
    // A discovered peer whose name resolves to the host's own loopback (`StubResolver`'s `loopback` names) is no peer.
    assert!(
        rig.host.wait_ready(&reader.name, READY),
        "{:?}",
        rig.host.log(&reader.name)
    );
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

    let opened: Vec<(String, Protocol, u16, ConnectionOutcome)> = rig
        .records
        .all()
        .into_iter()
        .filter_map(|r| match r {
            CallRecord::Connection(c) => {
                assert_eq!(
                    (c.reader.as_str(), c.version.as_str(), c.request.as_str()),
                    ("kreivo", "0.1.0", START_UP)
                );
                Some((c.host, c.protocol, c.port, c.outcome))
            }
            CallRecord::Request(_) => None,
        })
        .collect();
    let peer = |name: &str| format!("peer-{name}.kreivo.example");
    assert_eq!(
        opened,
        vec![
            (
                peer("a"),
                Protocol::P2p,
                30333,
                ConnectionOutcome::Connected
            ),
            (
                "loopback.kreivo.example".to_owned(),
                Protocol::P2p,
                30333,
                ConnectionOutcome::AddressNotAllowed
            ),
            (peer("b"), Protocol::Wss, 443, ConnectionOutcome::Connected),
            (
                peer("c"),
                Protocol::P2p,
                30333,
                ConnectionOutcome::LimitReached
            ),
            (
                peer("d"),
                Protocol::P2p,
                8080,
                ConnectionOutcome::UndeclaredDestination
            ),
            (
                peer("f"),
                Protocol::Wss,
                30333,
                ConnectionOutcome::UndeclaredDestination
            ),
        ]
    );
    assert_eq!(a.accepted.load(Ordering::SeqCst), 1);
    assert_eq!(b.accepted.load(Ordering::SeqCst), 1);
    assert_eq!(
        c.accepted.load(Ordering::SeqCst),
        0,
        "no connection past the cap, or to an undeclared port"
    );
}
