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
fn reader_that_keeps_talking_or_stops_reading_is_stopped_at_its_bound_and_the_others_answer_on() {
    // One floods the host with pool requests and never reads its replies; one keeps the host busy, a request after each
    // reply, and never answers. The call bound is set out of the way, so what stops them is the bound on what waits for
    // a reader, and the time bound read on every turn.
    let mut flooder = rpc_reader("flooder", &["flood"]);
    let mut chatter = rpc_reader("busy", &["busy"]);
    for reader in [&mut flooder, &mut chatter] {
        reader.limits.hang_after_ms = 1_500;
        reader.limits.calls_per_question = 1_000_000;
    }
    // And a light client that keeps a send queued ahead of each reply, to a peer that never answers: there is always an
    // event for its worker, so only the bound read on every turn stops it.
    let mut streamer = light_reader(
        "streamer",
        &["peer-a.kreivo.example:p2p:30333"],
        kreivo_rule(4),
        &["stream"],
    );
    streamer.limits.hang_after_ms = 1_500;
    streamer.limits.calls_per_question = 1_000_000;
    // And one that keeps two slow pool requests queued ahead of each reply: its worker always has a frame waiting, so
    // only the bound read on every turn, not a quiet moment, stops it.
    let mut pipeliner = rpc_reader("pipeliner", &["pipeline"]);
    pipeliner.limits.hang_after_ms = 1_500;
    pipeliner.limits.calls_per_question = 1_000_000;
    let peer = Peer::serve(GENESIS, true);
    let steady = rpc_reader("steady", &[]);
    let rig = Arc::new(Rig::start(
        vec![
            flooder.clone(),
            chatter.clone(),
            streamer.clone(),
            pipeliner.clone(),
            steady.clone(),
        ],
        honest_pool(),
        &[("peer-a.kreivo.example", &peer)],
    ));
    for reader in [&flooder, &chatter, &streamer, &pipeliner, &steady] {
        assert!(
            rig.host.wait_ready(&reader.name, READY),
            "{} passes its checks",
            reader.name
        );
    }
    let asked: Vec<_> = [
        flooder.clone(),
        chatter.clone(),
        streamer.clone(),
        pipeliner.clone(),
    ]
    .into_iter()
    .map(|reader| {
        let rig = rig.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let answer = ask(&rig.host, &reader, status_of("0xmove"));
            (answer, started.elapsed())
        })
    })
    .collect();
    std::thread::sleep(Duration::from_millis(200));
    for _ in 0..3 {
        let started = Instant::now();
        let answered = ask(&rig.host, &steady, status_of("0xmove"));
        assert!(
            is_final_to(&answered, "dest-1"),
            "the steady reader answers: {answered:?}"
        );
        assert!(
            started.elapsed() < Duration::from_millis(1_000),
            "and doesn't wait for the others"
        );
    }
    let mut results = asked.into_iter().map(|h| h.join().unwrap());
    let (flooded, took) = results.next().unwrap();
    // Each large reply waits for a reader that doesn't read: stopped by the bound on what waits to be written to it,
    // well before its own frames reach the bound on what waits for the host.
    assert_eq!(
        unreadable(&flooded),
        Some("reader_stopped_reading"),
        "{:?}",
        rig.host.log(&flooder.name)
    );
    assert!(
        took < Duration::from_millis(2_500),
        "stopped within its bound: {took:?}"
    );
    let (chattered, took) = results.next().unwrap();
    assert_eq!(unreadable(&chattered), Some("reader_hung"));
    assert!(
        took < Duration::from_millis(2_500),
        "stopped at its bound though it kept talking: {took:?}"
    );
    let (streamed, took) = results.next().unwrap();
    assert_eq!(
        unreadable(&streamed),
        Some("reader_hung"),
        "{:?}",
        rig.host.log(&streamer.name)
    );
    assert!(
        took < Duration::from_millis(2_500),
        "stopped at its bound though it always had a frame waiting: {took:?}"
    );
    let (pipelined, took) = results.next().unwrap();
    assert_eq!(
        unreadable(&pipelined),
        Some("reader_hung"),
        "{:?}",
        rig.host.log(&pipeliner.name)
    );
    assert!(
        took < Duration::from_millis(2_500),
        "stopped at its bound though its worker always had a frame waiting: {took:?}"
    );
    for reader in [&flooder, &chatter, &streamer, &pipeliner] {
        assert!(
            rig.host.wait_ready(&reader.name, READY),
            "{} is back",
            reader.name
        );
        assert!(
            rig.host.starts(&reader.name) >= 2,
            "{} was started again",
            reader.name
        );
    }
    assert_eq!(
        rig.host.starts(&steady.name),
        1,
        "the steady reader was never stopped"
    );
}

// verifies: REQ-ORD-39, DEC-28
#[test]
fn rpc_readers_chain_id_is_held_to_the_pools_own_answer_not_the_readers_report() {
    // A reader that reports the identity it was handed, without asking: taken only where the pool itself agrees.
    let echo = rpc_reader("base-echo", &["echo"]);
    let rig = Rig::start(vec![echo.clone()], honest_pool(), &[]);
    assert!(
        rig.host.wait_ready(&echo.name, READY),
        "{:?}",
        rig.host.log(&echo.name)
    );

    let other_chain = Arc::new(StubConnector::default());
    for endpoint in pool() {
        other_chain.set(
            endpoint.trim_start_matches("https://"),
            Box::new(|body: &str| {
                let id = serde_json::from_str::<serde_json::Value>(body).unwrap()["id"].clone();
                ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": "0x1"}))
            }),
        );
    }
    let rig = Rig::start(vec![echo.clone()], other_chain, &[]);
    let until = Instant::now() + Duration::from_secs(10);
    while !rig
        .host
        .log(&echo.name)
        .iter()
        .any(|l| l.contains("the pool found another network"))
    {
        assert!(Instant::now() < until, "{:?}", rig.host.log(&echo.name));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!rig.host.wait_ready(&echo.name, Duration::from_millis(500)));
    assert!(unreadable(&ask(&rig.host, &echo, status_of("0xmove"))).is_some());
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
    let lazy = rpc_reader("base-lazy", &["no-call"]);
    let connector = honest_pool();
    let rig = Rig::start(
        vec![honest.clone(), careless.clone(), lazy.clone()],
        connector.clone(),
        &[],
    );
    for reader in [&honest, &careless, &lazy] {
        assert!(rig.host.wait_ready(&reader.name, READY));
    }
    // An answer that rests on no answer of the pool at all: none agreeing.
    assert_eq!(
        unreadable(&ask(&rig.host, &lazy, status_of("0xmove"))),
        Some("trust_rule_not_met")
    );
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

// verifies: DEC-28
#[test]
fn question_past_its_deadline_is_dropped_by_the_reader_host() {
    // A pool whose movement answers take 400 ms from each endpoint, so a question waits behind another.
    let connector = Arc::new(StubConnector::default());
    for endpoint in pool() {
        let honest = evm_node(false);
        connector.set(
            endpoint.trim_start_matches("https://"),
            Box::new(move |body: &str| {
                if body.contains("rutent_movementStatus") {
                    std::thread::sleep(Duration::from_millis(400));
                }
                honest(body)
            }),
        );
    }
    let reader = rpc_reader("base", &["inspect"]);
    let rig = Rig::start(vec![reader.clone()], connector, &[]);
    assert!(rig.host.wait_ready(&reader.name, READY));
    let asked = |question: &rutent_contracts::reader::ReaderQuestion| {
        rig.host
            .answer(&serde_json::to_vec(question).expect("a question"))
    };
    let seen = |question: &rutent_contracts::reader::ReaderQuestion| {
        rig.host
            .log(&reader.name)
            .iter()
            .any(|line| line.starts_with("frame ") && line.contains(question.request.as_str()))
    };

    // Already past on arrival: dropped, answered with nothing, and never handed to the reader.
    let mut dead = question("base", "base", status_of("0xmove"));
    dead.deadline = timestamp_after(Duration::ZERO);
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(
        asked(&dead),
        None,
        "a dead question is answered with nothing"
    );

    // Alive on arrival, but past its deadline by the time its turn comes: dropped when its turn comes.
    let first = question("base", "base", status_of("0xmove"));
    let mut late = question("base", "base", status_of("0xmove"));
    late.deadline = timestamp_after(Duration::from_millis(400));
    let mut dead_while_busy = question("base", "base", status_of("0xmove"));
    dead_while_busy.deadline = timestamp_after(Duration::ZERO);
    let (first_answer, late_answer, dead_answer, dead_took) = std::thread::scope(|scope| {
        let first_answer = scope.spawn(|| asked(&first));
        std::thread::sleep(Duration::from_millis(100));
        let late_answer = scope.spawn(|| asked(&late));
        std::thread::sleep(Duration::from_millis(50));
        // Dead on arrival while the reader is busy: not queued behind the first, so answered with nothing at once.
        let started = Instant::now();
        let dead_answer = asked(&dead_while_busy);
        let dead_took = started.elapsed();
        (
            first_answer.join().unwrap(),
            late_answer.join().unwrap(),
            dead_answer,
            dead_took,
        )
    });
    assert_eq!(dead_answer, None);
    assert!(
        dead_took < Duration::from_millis(300),
        "a dead question is dropped on arrival, not queued: {dead_took:?}"
    );
    let first_answer = as_core_reads(
        &first_answer.expect("the first is answered"),
        &first,
        &reader,
    );
    assert!(
        is_final_to(&first_answer.statement, "dest-1"),
        "{first_answer:?}"
    );
    assert_eq!(
        late_answer, None,
        "a question that waited past its deadline is dropped"
    );

    assert!(seen(&first), "the reader was handed the first question");
    assert!(!seen(&dead), "the dead question never reached the reader");
    assert!(!seen(&late), "the late question never reached the reader");
    assert!(
        !seen(&dead_while_busy),
        "the question dead on arrival never reached the reader"
    );
    // And the reader answers on.
    assert!(is_final_to(
        &ask(&rig.host, &reader, status_of("0xmove")),
        "dest-1"
    ));
}

// verifies: REQ-MDL-11
#[test]
fn reader_host_refuses_a_restart_delay_of_zero() {
    let mut reader = rpc_reader("base", &[]);
    assert!(host_config(vec![reader.clone()]).check().is_ok());
    reader.limits.restart_after_ms = 0;
    let refused = host_config(vec![reader]).check().unwrap_err();
    assert!(refused.0.contains("every bound is above zero"), "{refused}");
}

// verifies: REQ-MDL-11
#[test]
fn reader_host_refuses_more_questions_waiting_than_its_cap() {
    use rutent_reader_host::config::MAX_PENDING;
    let mut reader = rpc_reader("base", &[]);
    reader.limits.pending = MAX_PENDING;
    assert!(host_config(vec![reader.clone()]).check().is_ok());
    reader.limits.pending = MAX_PENDING + 1;
    let refused = host_config(vec![reader]).check().unwrap_err();
    assert!(refused.0.contains("MAX_PENDING"), "{refused}");
}
