//! The VOS host server (`T-024-02`): one host of the host contract, `Stm` on a cluster of VOS nodes, served over
//! the wire (`rutent-stm-vos-wire`) on standard input and output, for a caller that can't link VOS.
//!
//! Usage: `rutent-stm-vos-serve DIR REPLICAS`, with `../test.sh`'s variables naming the node, the agent and the
//! program (`VOS_BIN`, `VOS_AGENT_ELF`, `STM_PROGRAM_ELF`). The cluster lives under `DIR`, and its nodes stop when the
//! server does. The first call opens a fresh host or imports one; each later one is a host call. The nodes run the
//! JIT on a Linux x86_64 machine that gives it, and the server fails closed elsewhere; `VOS_DEV=1` runs the
//! interpreter, a development run.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use rutent_contracts::state_machine::host::{Host, HostError};
use rutent_stm::Stm;
use rutent_stm_vos::cluster::{ClusterConfig, NodeBuild};
use rutent_stm_vos::host::{VosHarness, VosHost};
use rutent_stm_vos::platform::{Backend, Facts};
use rutent_stm_vos_wire::{Answer, Request};

fn var(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var(name).unwrap_or_else(|_| panic!("{name} names a binary: see test.sh")),
    )
}

fn config(replicas: usize) -> ClusterConfig {
    let facts = Facts::read();
    let backend = if std::env::var("VOS_DEV").is_ok_and(|v| v == "1") {
        Backend::Interpreter
    } else {
        facts.assert_jit().unwrap_or_else(|e| {
            panic!("the platform assertion failed, so the nodes did not run: {e}")
        });
        Backend::Jit
    };
    ClusterConfig {
        build: NodeBuild {
            vos: var("VOS_BIN"),
            faults: true,
        },
        agent_elf: var("VOS_AGENT_ELF"),
        program_elf: var("STM_PROGRAM_ELF"),
        replicas,
        spares: 0,
        executors: if replicas > 1 {
            (facts.vcpus / 3).max(1)
        } else {
            facts.vcpus.saturating_sub(1).max(1)
        },
        backend,
        settings: String::new(),
        wait: Duration::from_secs(60),
        strace: None,
    }
}

fn answer(host: &mut VosHost, request: Request) -> Answer {
    let failed = |e: HostError| Answer::Failed { error: e.into() };
    match request {
        Request::Open | Request::Import { .. } => failed(HostError::Unavailable {
            reason: "the host is open already".to_owned(),
        }),
        Request::Submit { input } => Host::<Stm>::submit(host, *input).into(),
        Request::Orders => {
            Host::<Stm>::orders(host).map_or_else(failed, |orders| Answer::Orders { orders })
        }
        Request::State { order } => {
            Host::<Stm>::state(host, &order).map_or_else(failed, |state| Answer::State { state })
        }
        Request::Journal { order } => Host::<Stm>::journal(host, &order)
            .map_or_else(failed, |records| Answer::Journal { records }),
        Request::Halt => Host::<Stm>::halt(host).map_or_else(failed, |()| Answer::Done),
        Request::Resume => Host::<Stm>::resume(host).map_or_else(failed, |()| Answer::Done),
        Request::Export => {
            match Host::<Stm>::export(host).and_then(|s| s.collect::<Result<Vec<_>, _>>()) {
                Ok(entries) => Answer::Entries { entries },
                Err(e) => failed(e),
            }
        }
        Request::Stop => Answer::Done,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .expect("usage: rutent-stm-vos-serve DIR REPLICAS"),
    );
    let replicas: usize = args
        .next()
        .and_then(|n| n.parse().ok())
        .expect("usage: rutent-stm-vos-serve DIR REPLICAS");
    let mut harness = VosHarness::<Stm>::new(config(replicas), dir, "h");
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut lines = stdin.lock().lines();
    let mut send = |answer: &Answer| {
        let line = serde_json::to_string(answer).expect("an answer is JSON");
        writeln!(stdout, "{line}").expect("the caller reads the answer");
        stdout.flush().expect("the caller reads the answer");
    };
    let first: Request = match lines.next() {
        Some(Ok(line)) => serde_json::from_str(&line).expect("a request"),
        _ => return,
    };
    let store = match harness.fresh() {
        Ok(store) => store,
        Err(e) => {
            send(&Answer::Failed {
                error: HostError::Unavailable {
                    reason: format!("{e:?}"),
                }
                .into(),
            });
            return;
        }
    };
    let opened = match first {
        Request::Open => <VosHost as Host<Stm>>::open(store),
        Request::Import { entries } => {
            <VosHost as Host<Stm>>::import(store, Box::new(entries.into_iter().map(Ok)))
        }
        _ => Err(HostError::Unavailable {
            reason: "the first call opens or imports a host".to_owned(),
        }),
    };
    let mut host = match opened {
        Ok(host) => {
            send(&Answer::Done);
            host
        }
        Err(e) => {
            send(&Answer::Failed { error: e.into() });
            return;
        }
    };
    for line in lines {
        let Ok(line) = line else { break };
        let request: Request = serde_json::from_str(&line).expect("a request");
        let stop = request == Request::Stop;
        send(&answer(&mut host, request));
        if stop {
            break;
        }
    }
    drop(host.kill());
}
