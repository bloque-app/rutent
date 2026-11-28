//! A simulated network's reader (`T-024-25`, `DEC-28`): the program the reader host starts, inside the scenarios' test
//! harness, for each simulated public network. Test code only, never shipped; the real readers are `F-031`'s.
//!
//! ```text
//! sim_reader rpc [--match destination] [--hostile <act>[,<act>...]]
//! sim_reader light-client --peer <host>:p2p:<port> [--match destination] [--hostile <act>[,<act>...]]
//! sim_reader host
//! ```
//!
//! In its `host` mode it is the simulated networks' reader host itself (`T-020-60`), run by the assembly as a process of
//! its own beside Core (`rutent_scenarios::sim::reader_service`): it reads its configuration (the listings, the
//! endpoints the world serves, where the world answers, and the faults planted in the readers) as one JSON line from its
//! standard input, runs `T-024-25`'s harness (`tests/readers_support`) over them, with each reader this same program
//! in its `rpc` or `light-client` mode, tells where its link listens and what Core is to be handed (the test CA's
//! certificate, and Core's client certificate and key) as one JSON line on its standard output, and serves until its
//! standard input closes.
//!
//! It speaks the reader host's wire ([`rutent_reader_host::wire`]) and nothing else: it has no network of its own, and
//! reads its network only through the host, from the endpoints the simulated world serves it
//! (`rutent_scenarios::sim::readers::Endpoints`). An `rpc` reader (the EVM rule) asks its pool, through the host's
//! quorum, and checks the network's chain id first (`eth_chainId`); a `light-client` reader (the Substrate rule) opens
//! a connection to its peer, through the host's rule by protocol and port, and checks the network's genesis first
//! (`rutent_genesis`). Each then answers the network's rules (`rutent_rules`), where a movement stands
//! (`rutent_movementStatus`) and the movement the network ties to a transaction: by default the movement the
//! transaction names, as the simulated providers give each movement its transaction; with `--match destination`, the
//! first movement the network recorded to that movement's destination of its amount (`rutent_movementsTo`), the
//! harness's shared-movement fault (`REQ-ORD-38`).
//!
//! A hostile act makes it misbehave, for the host and Core to hold: `crash` (on a question), `malformed` (answers
//! something that is no answer), `other-movement` (answers about another movement or transaction than the one asked)
//! `other-identity` (answers its rules naming another network's identity than the one it checked, a start-up liar),
//! `unread-movements` (answers its rules, but no movement: every movement question is unreadable),
//! and `undeclared` (also asks an endpoint it doesn't declare: the world's own host for the network, and for a light
//! client a port its rule doesn't name). A key the listing doesn't name is the harness's to plant, in the host's
//! configuration: the reader never holds a key.

use rutent_contracts::provider_role::NoDetail;
use rutent_contracts::reader::{
    Answered, Asked, ChainId, Finality, GenesisHash, NetworkIdentity, NetworkRules, ReaderAnswer,
    Reason, ReorgRule, Standing, Tied, TrustRule,
};
use rutent_contracts::router_api::Id;
use rutent_reader_host::config::Protocol;
use rutent_reader_host::wire::{
    FromReader, OpenResult, ReceiveResult, RpcResult, ToReader, read_frame, write_frame,
};
use serde_json::{Value, json};

#[path = "../tests/readers_support/mod.rs"]
mod readers_support;

/// How many times a start-up check asks again before it gives up, each after a pause.
const CHECK_TRIES: usize = 200;

/// How many times a light client waits on its peer for the rest of a reply.
const RECEIVE_TRIES: usize = 4;

struct Reader {
    hostile: Vec<String>,
    light: bool,
    ties: bool,
    peers: Vec<(String, u16)>,
    network: String,
    endpoints: Vec<String>,
    trust: Option<TrustRule>,
    conn: Option<u64>,
    call: u64,
    /// Whether it is on a question, past its start-up checks: a straying reader strays only then.
    asking: bool,
}

fn send(frame: &FromReader) {
    let bytes = serde_json::to_vec(frame).expect("a frame");
    if write_frame(&mut std::io::stdout().lock(), &bytes).is_err() {
        std::process::exit(0);
    }
}

fn receive() -> ToReader {
    let Ok(Some(bytes)) = read_frame(&mut std::io::stdin().lock(), 1 << 24) else {
        std::process::exit(0);
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|_| std::process::exit(4))
}

/// Waits without a timed sleep, which the provider's `seccomp` allow-list doesn't take: the clock and yielding are
/// allowed.
fn pause(millis: u64) {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(millis);
    while std::time::Instant::now() < until {
        std::thread::yield_now();
    }
}

impl Reader {
    fn hostile(&self, act: &str) -> bool {
        self.hostile.iter().any(|h| h == act)
    }

    fn next_call(&mut self) -> u64 {
        self.call += 1;
        self.call
    }

    fn rpc(&mut self, endpoints: Vec<String>, body: &Value) -> RpcResult {
        let call = self.next_call();
        send(&FromReader::Rpc {
            call,
            endpoints,
            body: body.to_string(),
        });
        match receive() {
            ToReader::Rpc { call: c, result } if c == call => result,
            _ => std::process::exit(4),
        }
    }

    fn open(&mut self, host: String, port: u16) -> OpenResult {
        let call = self.next_call();
        send(&FromReader::Open {
            call,
            protocol: Protocol::P2p,
            host,
            port,
        });
        match receive() {
            ToReader::Opened { call: c, result } if c == call => result,
            _ => std::process::exit(4),
        }
    }

    /// One request on the peer's connection, and the line it answers.
    fn exchange(&mut self, line: &str) -> Option<String> {
        let conn = self.conn?;
        let mut bytes = format!("{line}\n").into_bytes();
        let mut got = Vec::new();
        for _ in 0..RECEIVE_TRIES {
            send(&FromReader::Send {
                conn,
                bytes: std::mem::take(&mut bytes),
            });
            match receive() {
                ToReader::Received {
                    result: ReceiveResult::Bytes { bytes },
                    ..
                } => got.extend(bytes),
                ToReader::Received { .. } => return None,
                _ => std::process::exit(4),
            }
            if got.ends_with(b"\n") {
                return Some(String::from_utf8_lossy(&got).trim().to_owned());
            }
        }
        None
    }

    /// The `result` of one JSON-RPC request to the network: through the pool for an RPC reader, through its peer for a
    /// light client. `Err` names why there is nothing to read.
    fn read(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let body =
            json!({"jsonrpc": "2.0", "id": self.call + 1, "method": method, "params": params});
        if self.hostile("undeclared") && self.asking {
            self.stray(&body);
        }
        let text = if self.light {
            self.exchange(&body.to_string())
                .ok_or_else(|| "peer_gave_no_answer".to_owned())?
        } else {
            let endpoints = self.endpoints.clone();
            match self.rpc(endpoints, &body) {
                RpcResult::Agreed { body } => body,
                RpcResult::Unreadable { why } => return Err(why),
            }
        };
        let value: Value =
            serde_json::from_str(&text).map_err(|_| "network_answer_malformed".to_owned())?;
        Ok(value["result"].clone())
    }

    /// What a straying reader does beside its own calls: asks the world's own host for the network, which it doesn't
    /// declare, and, as a light client, opens a port its rule doesn't name.
    fn stray(&mut self, body: &Value) {
        let host = format!("https://{}.chain.example", self.network);
        let _ = self.rpc(vec![host], body);
        if self.light
            && let Some((peer, _)) = self.peers.first().cloned()
        {
            let _ = self.open(peer, 8080);
        }
    }

    fn status(&mut self, movement: &str) -> Result<Standing, String> {
        let value = self.read("rutent_movementStatus", json!([movement]))?;
        serde_json::from_value(value).map_err(|_| "network_answer_malformed".to_owned())
    }

    /// The start-up checks: the network's identity, found through the host.
    fn check(&mut self) -> Option<NetworkIdentity> {
        if self.light {
            for (host, port) in self.peers.clone() {
                if let OpenResult::Open { conn } = self.open(host, port) {
                    self.conn = Some(conn);
                    break;
                }
            }
            self.conn?;
        }
        for _ in 0..CHECK_TRIES {
            let found = if self.light {
                self.read("rutent_genesis", json!([]))
                    .ok()
                    .and_then(|v| GenesisHash::new(v.as_str()?.to_owned()))
                    .map(|hash| NetworkIdentity::GenesisHash { hash })
            } else {
                self.read("eth_chainId", json!([]))
                    .ok()
                    .and_then(|v| u64::from_str_radix(v.as_str()?.strip_prefix("0x")?, 16).ok())
                    .and_then(|id| ChainId::new(id.to_string()))
                    .map(|id| NetworkIdentity::ChainId { id })
            };
            if found.is_some() {
                return found;
            }
            pause(50);
        }
        None
    }

    fn answer(&mut self, asked: &Asked, found: &NetworkIdentity) -> Answered {
        let unreadable = |why: &str| Answered::Unreadable {
            why: Reason::new(why).unwrap_or_else(|| Reason::new("unreadable").expect("a reason")),
        };
        // A reader planted to lose track of the movements answers its rules but can't stand behind any movement.
        if self.hostile("unread-movements") && !matches!(asked, Asked::NetworkRules(_)) {
            return unreadable("movements_unread");
        }
        let answered = match asked {
            Asked::NetworkRules(_) => self.read("rutent_rules", json!([])).and_then(|value| {
                let finality: Finality = serde_json::from_value(value["finality"].clone())
                    .map_err(|_| "network_answer_malformed".to_owned())?;
                let reorg: ReorgRule = serde_json::from_value(value["reorg"].clone())
                    .map_err(|_| "network_answer_malformed".to_owned())?;
                // A reader planted to lie about its network's identity at start-up names a genesis hash, or a chain id,
                // of another network than the one it checked.
                let identity = if self.hostile("other-identity") {
                    match found {
                        NetworkIdentity::GenesisHash { .. } => NetworkIdentity::GenesisHash {
                            hash: GenesisHash::new(format!("0x{}", "ab".repeat(32)))
                                .expect("a genesis hash"),
                        },
                        NetworkIdentity::ChainId { .. } => NetworkIdentity::ChainId {
                            id: ChainId::new("999999").expect("a chain id"),
                        },
                    }
                } else {
                    found.clone()
                };
                Ok(Answered::NetworkRules {
                    rules: NetworkRules {
                        identity,
                        finality,
                        reorg,
                        trust: self
                            .trust
                            .clone()
                            .unwrap_or(TrustRule::LightClient(NoDetail {})),
                    },
                })
            }),
            Asked::MovementStatus { movement } => self
                .status(movement.as_str())
                .map(|standing| Answered::MovementStatus { standing }),
            Asked::MovementOf { transaction } => self
                .movement_of(transaction.as_str())
                .map(|movement| Answered::MovementOf { movement }),
        };
        answered.unwrap_or_else(|why| unreadable(&why))
    }

    /// The movement the network ties to `transaction`: the one it names, or, matching by destination, the first the
    /// network recorded to that movement's destination of its amount.
    fn movement_of(&mut self, transaction: &str) -> Result<Option<Tied>, String> {
        let (Standing::Pending(moved) | Standing::Final(moved)) = self.status(transaction)? else {
            return Ok(None);
        };
        let mut id = transaction.to_owned();
        if self.ties {
            let first = self.read(
                "rutent_movementsTo",
                json!([moved.destination.as_str(), moved.amount.as_str()]),
            )?;
            if let Some(first) = first.get(0).and_then(Value::as_str) {
                id = first.to_owned();
            }
        }
        let (Standing::Pending(tied) | Standing::Final(tied)) = self.status(&id)? else {
            return Ok(None);
        };
        Ok(Some(Tied {
            id: Id::new(id).ok_or("network_answer_malformed")?,
            destination: tied.destination,
        }))
    }
}

/// `asked`, about another movement or transaction than the one it names: a hostile reader's answer.
fn another(asked: &Asked) -> Asked {
    let other = |id: &Id| Id::new(format!("{id}-other")).unwrap_or_else(|| id.clone());
    match asked {
        Asked::MovementStatus { movement } => Asked::MovementStatus {
            movement: other(movement),
        },
        Asked::MovementOf { transaction } => Asked::MovementOf {
            transaction: other(transaction),
        },
        Asked::NetworkRules(_) => Asked::MovementStatus {
            movement: Id::new("another-movement").expect("an id"),
        },
    }
}

/// The reader host's mode: serves until its standard input closes.
fn host() -> ! {
    use std::io::BufRead as _;
    let tell = |told: Value| {
        println!("{told}");
    };
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        std::process::exit(2);
    }
    let started = std::panic::catch_unwind(|| {
        let config: Value = serde_json::from_str(&line).expect("a configuration");
        let listed: Vec<rutent_contracts::core_internal::PublicNetwork> =
            serde_json::from_value(config["listed"].clone()).expect("the listings");
        let served = config["endpoints"]
            .as_object()
            .expect("the endpoints")
            .iter()
            .map(|(host, served)| {
                (
                    host.clone(),
                    rutent_scenarios::sim::readers::served_of(served).expect("an endpoint"),
                )
            })
            .collect();
        let world = config["world"]
            .as_str()
            .and_then(|a| a.parse().ok())
            .expect("where the world answers");
        let plan = rutent_scenarios::sim::readers::Plan::of_json(&config["plan"]).expect("a plan");
        let endpoints = rutent_scenarios::sim::readers::Endpoints::remote(served, world);
        readers_support::Readers::start(&listed, endpoints, &plan)
    });
    let readers = match started {
        Ok(readers) => readers,
        Err(why) => {
            let why = why
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| why.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            tell(json!({ "error": why }));
            std::process::exit(3);
        }
    };
    tell(json!({
        "address": readers.address().to_string(),
        "ca": readers.ca_pem,
        "chain": readers.core_chain_pem,
        "key": readers.core_key_pem,
    }));
    let mut rest = String::new();
    while std::io::stdin()
        .lock()
        .read_line(&mut rest)
        .is_ok_and(|n| n > 0)
    {
        rest.clear();
    }
    drop(readers);
    std::process::exit(0);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|m| m == "host") {
        host();
    }
    // The provider's jail, from inside, before anything is read: what every reader does first.
    if let Err(e) = rutent_reader_host::jail::confine_reader() {
        eprintln!("sim_reader: not confined: {e}");
        std::process::exit(5);
    }
    let mut reader = Reader {
        hostile: Vec::new(),
        light: args.first().is_some_and(|m| m == "light-client"),
        ties: false,
        peers: Vec::new(),
        network: String::new(),
        endpoints: Vec::new(),
        trust: None,
        conn: None,
        call: 0,
        asking: false,
    };
    let mut i = 1;
    while i < args.len() {
        let value = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--hostile" => reader.hostile = value.split(',').map(str::to_owned).collect(),
            "--match" => reader.ties = value == "destination",
            "--peer" => {
                let mut parts = value.rsplitn(3, ':');
                let port = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
                let _protocol = parts.next();
                let host = parts.next().unwrap_or_default().to_owned();
                reader.peers.push((host, port));
            }
            _ => {
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    let ToReader::Start {
        network,
        version,
        trust,
        ..
    } = receive()
    else {
        std::process::exit(2);
    };
    reader.network = network.as_str().to_owned();
    if let TrustRule::RpcPool { endpoints, .. } = &trust {
        reader.endpoints = endpoints.iter().map(|e| e.as_str().to_owned()).collect();
    }
    reader.trust = Some(trust);
    let Some(found) = reader.check() else {
        send(&FromReader::Log {
            line: "checks: nothing found".to_owned(),
        });
        loop {
            std::thread::park();
        }
    };
    send(&FromReader::Checked {
        identity: found.clone(),
    });
    reader.asking = true;
    loop {
        let ToReader::Question { question } = receive() else {
            std::process::exit(4);
        };
        if reader.hostile("crash") {
            std::process::exit(3);
        }
        if reader.hostile("malformed") {
            send(&FromReader::Answer {
                answer: json!({"answer": "no answer at all", "request": question.request}),
            });
            continue;
        }
        let answer = reader.answer(&question.asked, &found);
        let asked = if reader.hostile("other-movement") {
            another(&question.asked)
        } else {
            question.asked.clone()
        };
        let body = ReaderAnswer {
            request: question.request.clone(),
            network: question.network.clone(),
            version: version.clone(),
            asked,
            answer,
        };
        send(&FromReader::Answer {
            answer: serde_json::to_value(&body).expect("an answer"),
        });
    }
}
