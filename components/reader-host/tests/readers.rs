//! The reader host's readers (`T-015-04` parts 2, 3, 5 and 6): one process per reader, each answer signed by the host
//! only as asked, the trust rules, and a restart. The stand-in readers are `examples/standin_reader.rs`.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::*;
use rutent_contracts::reader::{Answered, NetworkIdentity, Standing};
use rutent_contracts::router_api::Version;
use rutent_reader_host::{Host, HostConfig, ReaderState, Services};

const READY: Duration = Duration::from_secs(30);

fn is_final_to(answer: &rutent_contracts::reader::ReaderAnswer, destination: &str) -> bool {
    matches!(&answer.answer, Answered::MovementStatus { standing: Standing::Final(m) } if m.destination.as_str() == destination)
}

// verifies: REQ-MDL-11
#[test]
fn reader_that_crashes_or_hangs_is_unreadable_and_the_other_readers_answer_on() {
    let crasher = rpc_reader("crasher", &["crash"]);
    let mut hanger = rpc_reader("hanger", &["hang"]);
    hanger.limits.hang_after_ms = 1_500;
    let steady = rpc_reader("steady", &[]);
    let rig = Rig::start(
        vec![crasher.clone(), hanger.clone(), steady.clone()],
        honest_pool(),
        &[],
    );
    for reader in [&crasher, &hanger, &steady] {
        assert!(
            rig.host.wait_ready(&reader.name, READY),
            "{} passes its checks",
            reader.name
        );
    }

    let crashed = ask(&rig.host, &crasher, status_of("0xmove"));
    assert_eq!(unreadable(&crashed), Some("reader_crashed"));

    // The hung reader holds only its own question: the steady one answers while it hangs.
    let host = Arc::new(rig);
    let hung = {
        let (host, hanger) = (host.clone(), hanger.clone());
        std::thread::spawn(move || {
            let started = Instant::now();
            let answer = ask(&host.host, &hanger, status_of("0xmove"));
            (answer, started.elapsed())
        })
    };
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    let answered = ask(&host.host, &steady, status_of("0xmove"));
    assert!(
        is_final_to(&answered, "dest-1"),
        "the steady reader answers: {answered:?}"
    );
    assert!(
        started.elapsed() < Duration::from_millis(1_200),
        "and doesn't wait for the hung one"
    );
    let (hung, took) = hung.join().unwrap();
    assert_eq!(unreadable(&hung), Some("reader_hung"));
    assert!(
        took >= Duration::from_millis(1_400),
        "the host waited its bound, {took:?}"
    );

    // Each is started again, and answers again once its checks pass.
    for reader in [&crasher, &hanger] {
        assert!(
            host.host.wait_ready(&reader.name, READY),
            "{} is back",
            reader.name
        );
        assert!(
            host.host.starts(&reader.name) >= 2,
            "{} was started again",
            reader.name
        );
    }
    assert_eq!(
        host.host.starts(&steady.name),
        1,
        "the steady reader was never stopped"
    );
}

// verifies: REQ-MDL-11
#[test]
fn reader_with_its_questions_bound_reached_answers_the_next_unreadable_at_once() {
    let mut hanger = rpc_reader("hanger", &["hang"]);
    hanger.limits.hang_after_ms = 3_000;
    hanger.limits.pending = 2;
    let rig = Arc::new(Rig::start(vec![hanger.clone()], honest_pool(), &[]));
    assert!(rig.host.wait_ready(&hanger.name, READY));
    let waiting: Vec<_> = (0..2)
        .map(|_| {
            let (rig, hanger) = (rig.clone(), hanger.clone());
            let handle = std::thread::spawn(move || ask(&rig.host, &hanger, status_of("0xmove")));
            std::thread::sleep(Duration::from_millis(200));
            handle
        })
        .collect();
    let started = Instant::now();
    let refused = ask(&rig.host, &hanger, status_of("0xmove"));
    assert_eq!(unreadable(&refused), Some("too_many_pending_questions"));
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "refused at once"
    );
    let reasons: Vec<String> = waiting
        .into_iter()
        .map(|h| {
            unreadable(&h.join().unwrap())
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(reasons, ["reader_hung", "reader_restarting"]);
}

// verifies: REQ-MDL-6, DEC-28
#[test]
fn host_signs_only_an_answer_that_names_the_question_it_was_asked() {
    let liar = rpc_reader("liar", &["other-request"]);
    let rig = Rig::start(vec![liar.clone()], honest_pool(), &[]);
    assert!(rig.host.wait_ready(&liar.name, READY));
    // `ask` holds the answer to the question asked, as Core does: it names the request asked, and is unreadable.
    let answer = ask(&rig.host, &liar, status_of("0xmove"));
    assert_eq!(unreadable(&answer), Some("answer_to_another_request"));

    // A reader whose rules aren't its configuration's: what Core's start-up check compares with the listing.
    let other = rpc_reader("other", &["other-rules"]);
    let rig = Rig::start(vec![other.clone()], honest_pool(), &[]);
    assert!(rig.host.wait_ready(&other.name, READY));
    assert_eq!(
        unreadable(&ask(&rig.host, &other, network_rules())),
        Some("rules_not_as_configured")
    );
    assert!(unreadable(&ask(&rig.host, &other, status_of("0xmove"))).is_none());
}

// verifies: REQ-ORD-39, DEC-28
#[test]
fn evm_reader_with_fewer_than_two_agreeing_rpc_answers_is_unreadable() {
    let honest = rpc_reader("base-honest", &[]);
    let careless = rpc_reader("base-careless", &["ignore-quorum"]);
    let connector = honest_pool();
    let rig = Rig::start(
        vec![honest.clone(), careless.clone()],
        connector.clone(),
        &[],
    );
    for reader in [&honest, &careless] {
        assert!(rig.host.wait_ready(&reader.name, READY));
    }
    let both = |rig: &Rig| {
        (
            ask(&rig.host, &honest, status_of("0xmove")),
            ask(&rig.host, &careless, status_of("0xmove")),
        )
    };

    // All three agree.
    let (a, b) = both(&rig);
    assert!(
        is_final_to(&a, "dest-1") && is_final_to(&b, "dest-1"),
        "{a:?} {b:?}"
    );

    // One answers: too few.
    connector.set("rpc-b.base.example", failing());
    connector.set("rpc-c.base.example", failing());
    let (a, b) = both(&rig);
    assert_eq!(unreadable(&a), Some("too_few_rpc_answers"));
    // The careless reader answers anyway; the host doesn't sign it as answered.
    assert_eq!(unreadable(&b), Some("trust_rule_not_met"));

    // Two answer, and disagree.
    connector.set("rpc-b.base.example", evm_node(true));
    let (a, b) = both(&rig);
    assert_eq!(unreadable(&a), Some("rpc_answers_disagree"));
    assert_eq!(unreadable(&b), Some("trust_rule_not_met"));

    // Two of three agree, against one that lies: answered as the two say.
    connector.set("rpc-c.base.example", evm_node(false));
    let (a, b) = both(&rig);
    assert!(
        is_final_to(&a, "dest-1") && is_final_to(&b, "dest-1"),
        "{a:?} {b:?}"
    );
}

// verifies: REQ-ORD-39, DEC-28
#[test]
fn substrate_reader_checks_the_networks_genesis_before_answering() {
    let peer = Peer::serve(GENESIS, false);
    let other = Peer::serve(
        "0x1111111111111111111111111111111111111111111111111111111111111111",
        true,
    );
    let reader = light_reader(
        "kreivo",
        &["peer-a.kreivo.example:p2p:30333"],
        kreivo_rule(4),
        &[],
    );
    let wrong = light_reader(
        "kreivo-wrong",
        &["peer-w.kreivo.example:p2p:30333"],
        kreivo_rule(4),
        &[],
    );
    let skipper = light_reader(
        "kreivo-skip",
        &["peer-a.kreivo.example:p2p:30333"],
        kreivo_rule(4),
        &["skip-checks"],
    );
    let rig = Rig::start(
        vec![reader.clone(), wrong.clone(), skipper.clone()],
        Arc::new(StubConnector::default()),
        &[
            ("peer-a.kreivo.example", &peer),
            ("peer-w.kreivo.example", &other),
        ],
    );

    // Its peer has no genesis to give yet: refusing, every question unreadable.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(rig.host.state(&reader.name), Some(ReaderState::Starting));
    for asked in [status_of("0xmove"), network_rules()] {
        assert_eq!(
            unreadable(&ask(&rig.host, &reader, asked)),
            Some("checks_not_passed")
        );
    }
    // A reader that skips its checks and answers anyway is never asked.
    assert_eq!(
        unreadable(&ask(&rig.host, &skipper, status_of("0xmove"))),
        Some("checks_not_passed")
    );

    // A reader that finds another network's genesis never answers.
    let until = Instant::now() + Duration::from_secs(5);
    while !rig
        .host
        .log(&wrong.name)
        .iter()
        .any(|l| l.contains("found another network"))
    {
        assert!(
            Instant::now() < until,
            "the wrong network's checks ran: {:?}",
            rig.host.log(&wrong.name)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(rig.host.state(&wrong.name), Some(ReaderState::Ready));
    assert!(unreadable(&ask(&rig.host, &wrong, status_of("0xmove"))).is_some());

    // The genesis arrives, and holds: it answers.
    peer.open.store(true, Ordering::SeqCst);
    assert!(
        rig.host.wait_ready(&reader.name, READY),
        "{:?}",
        rig.host.log(&reader.name)
    );
    let status = ask(&rig.host, &reader, status_of("0xmove"));
    assert!(
        matches!(
            &status.answer,
            Answered::MovementStatus {
                standing: Standing::Final(_)
            }
        ),
        "{status:?}"
    );
    let Answered::NetworkRules { rules } = ask(&rig.host, &reader, network_rules()).answer else {
        panic!("its network's rules");
    };
    assert_eq!(
        rules.identity,
        NetworkIdentity::GenesisHash {
            hash: rutent_contracts::reader::GenesisHash::new(GENESIS).unwrap()
        }
    );
    assert_eq!(
        unreadable(&ask(&rig.host, &skipper, status_of("0xmove"))),
        Some("checks_not_passed")
    );
    assert_ne!(rig.host.state(&wrong.name), Some(ReaderState::Ready));
}

// verifies: NFR-6
#[test]
fn restarted_reader_host_answers_nothing_until_each_readers_checks_pass_again() {
    let pool_up = Arc::new(AtomicBool::new(true));
    let connector = Arc::new(StubConnector::default());
    for endpoint in pool() {
        let (up, node) = (pool_up.clone(), evm_node(false));
        connector.set(
            endpoint.trim_start_matches("https://"),
            Box::new(move |body| {
                if up.load(Ordering::SeqCst) {
                    node(body)
                } else {
                    failing()(body)
                }
            }),
        );
    }
    let peer = Peer::serve(GENESIS, true);
    let base = rpc_reader("base", &[]);
    let kreivo = light_reader(
        "kreivo",
        &["peer-a.kreivo.example:p2p:30333"],
        kreivo_rule(4),
        &[],
    );
    let dir =
        std::env::temp_dir().join(format!("rutent-reader-host-restart-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("host.json");
    let write = |config: &HostConfig| {
        std::fs::write(&path, serde_json::to_vec_pretty(config).unwrap()).unwrap()
    };
    write(&host_config(vec![base.clone(), kreivo.clone()]));

    let start = |rig_of: &dyn Fn() -> HostConfig| {
        let config = rig_of();
        let records = Arc::new(rutent_reader_host::records::MemoryRecords::new());
        let resolver = Arc::new(StubResolver::default());
        let dialer = Arc::new(MappedDialer::default());
        dialer
            .map
            .lock()
            .unwrap()
            .insert(address_of("peer-a.kreivo.example"), peer.address);
        Host::start(
            config.clone(),
            Services {
                keys: keys_of(&config.readers),
                network: rutent_reader_host::egress::Network {
                    resolver,
                    connector: connector.clone(),
                    dialer,
                },
                records,
            },
        )
        .unwrap()
    };
    let read = || HostConfig::read(&path).unwrap();

    let host = start(&read);
    for reader in [&base, &kreivo] {
        assert!(host.wait_ready(&reader.name, READY));
        assert!(unreadable(&ask(&host, reader, status_of("0xmove"))).is_none());
    }
    drop(host);

    // The configuration changes while the host is down, and its readers' networks are not reachable yet.
    let (mut base, mut kreivo) = (base, kreivo);
    for reader in [&mut base, &mut kreivo] {
        reader.version = Version::new("0.2.0").unwrap();
    }
    write(&host_config(vec![base.clone(), kreivo.clone()]));
    pool_up.store(false, Ordering::SeqCst);
    peer.open.store(false, Ordering::SeqCst);

    let host = start(&read);
    for reader in [&base, &kreivo] {
        for asked in [network_rules(), status_of("0xmove")] {
            let answer = ask(&host, reader, asked);
            assert_eq!(
                unreadable(&answer),
                Some("checks_not_passed"),
                "{}",
                reader.name
            );
            assert_eq!(
                answer.version.as_str(),
                "0.2.0",
                "the configuration was read again"
            );
        }
    }

    // Each answers once its own checks pass, and not before.
    pool_up.store(true, Ordering::SeqCst);
    assert!(host.wait_ready(&base.name, READY));
    assert!(is_final_to(
        &ask(&host, &base, status_of("0xmove")),
        "dest-1"
    ));
    assert_eq!(
        unreadable(&ask(&host, &kreivo, status_of("0xmove"))),
        Some("checks_not_passed")
    );
    peer.open.store(true, Ordering::SeqCst);
    assert!(host.wait_ready(&kreivo.name, READY));
    assert!(unreadable(&ask(&host, &kreivo, status_of("0xmove"))).is_none());
    drop(host);
    let _ = std::fs::remove_dir_all(dir);
}
