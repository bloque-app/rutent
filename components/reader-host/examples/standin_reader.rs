//! The stand-in readers the reader host's tests run (`T-015-04`): test code only, never shipped. The real readers are
//! `F-031`'s (`T-031-02` for a Substrate network's light client, `T-031-03` for an EVM network's RPC pool).
//!
//! ```text
//! standin_reader rpc [--hostile <act>[,<act>...]]
//! standin_reader light-client --peer <host>:<p2p|wss>:<port> [--peer ...] [--hostile <act>[,<act>...]]
//! ```
//!
//! An `rpc` reader finds its chain id through the host's pool, `eth_chainId`, and answers a movement's status and the
//! movement of a transaction from the pool's agreed answer to `rutent_movementStatus` and `rutent_movementOf`, the
//! stand-in network's own methods. A `light-client` reader opens a connection to each peer named, asks the first for
//! the network's genesis (`genesis\n`) until it has one, reports it, and answers from the first peer's `status <id>\n`
//! and `movement_of <id>\n`. Each answers unreadable where its trust rule isn't met.
//!
//! A hostile act makes it misbehave, for the host to hold: `ignore-quorum` (answers a final movement whatever the
//! pool agreed), `undeclared` (asks an endpoint it doesn't declare too), `other-request` (answers under another
//! request id), `other-rules` (names a stricter quorum than its listing in its network's rules), `skip-checks` (never
//! reports its checks, and answers anyway), `crash` and `hang` (on a question), `unconfined` (skips the jail's in-process half),
//! `inspect` (logs its arguments, its environment and every frame it reads) and `probe-network=<address:port>` (tries
//! to connect to that address, and to a public one, at start, and logs what happened).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use rutent_contracts::provider_role::NoDetail;
use rutent_contracts::reader::{
    Answered, Asked, ChainId, Depth, Finality, GenesisHash, Movement, NetworkIdentity,
    NetworkRules, ReaderAnswer, ReaderQuestion, Reason, ReorgRule, RequestId, Standing, Tied,
    TrustRule,
};
use rutent_contracts::router_api::{Amount, Id};
use rutent_reader_host::config::Protocol;
use rutent_reader_host::wire::{
    FromReader, OpenResult, ReceiveResult, RpcResult, ToReader, read_frame, write_frame,
};
use serde_json::{Value, json};

struct Io {
    input: std::io::Stdin,
    output: std::io::Stdout,
    inspect: bool,
}

impl Io {
    fn send(&mut self, frame: &FromReader) {
        let bytes = serde_json::to_vec(frame).expect("a frame");
        if write_frame(&mut self.output.lock(), &bytes).is_err() {
            std::process::exit(0);
        }
    }

    fn receive(&mut self) -> ToReader {
        let Ok(Some(bytes)) = read_frame(&mut self.input.lock(), 1 << 24) else {
            std::process::exit(0);
        };
        if self.inspect {
            let line = format!("frame {}", String::from_utf8_lossy(&bytes));
            self.send(&FromReader::Log { line });
        }
        serde_json::from_slice(&bytes).expect("a frame from the host")
    }

    fn log(&mut self, line: impl Into<String>) {
        self.send(&FromReader::Log { line: line.into() });
    }
}

struct Peer {
    host: String,
    protocol: Protocol,
    port: u16,
}

struct Reader {
    io: Io,
    hostile: Vec<String>,
    peers: Vec<Peer>,
    light: bool,
    trust: Option<TrustRule>,
    endpoints: Vec<String>,
    conns: Vec<u64>,
    call: u64,
}

impl Reader {
    fn hostile(&self, act: &str) -> bool {
        self.hostile.iter().any(|h| h == act)
    }

    fn next_call(&mut self) -> u64 {
        self.call += 1;
        self.call
    }

    fn rpc(&mut self, endpoints: Vec<String>, body: Value) -> RpcResult {
        let call = self.next_call();
        self.io.send(&FromReader::Rpc {
            call,
            endpoints,
            body: body.to_string(),
        });
        match self.io.receive() {
            ToReader::Rpc { call: c, result } if c == call => result,
            _ => std::process::exit(4),
        }
    }

    fn open(&mut self, protocol: Protocol, host: String, port: u16) -> OpenResult {
        let call = self.next_call();
        self.io.send(&FromReader::Open {
            call,
            protocol,
            host,
            port,
        });
        match self.io.receive() {
            ToReader::Opened { call: c, result } if c == call => result,
            _ => std::process::exit(4),
        }
    }

    fn exchange(&mut self, conn: u64, text: &str) -> Option<String> {
        self.io.send(&FromReader::Send {
            conn,
            bytes: text.as_bytes().to_vec(),
        });
        match self.io.receive() {
            ToReader::Received {
                result: ReceiveResult::Bytes { bytes },
                ..
            } => Some(String::from_utf8_lossy(&bytes).trim().to_owned()),
            ToReader::Received { .. } => None,
            _ => std::process::exit(4),
        }
    }

    /// The start-up checks: the network's identity, found through the host.
    fn check(&mut self) -> Option<NetworkIdentity> {
        if self.light {
            let peers: Vec<(Protocol, String, u16)> = self
                .peers
                .iter()
                .map(|p| (p.protocol, p.host.clone(), p.port))
                .collect();
            for (protocol, host, port) in peers {
                let result = self.open(protocol, host.clone(), port);
                self.io.log(format!(
                    "open {host}:{}:{port}: {result:?}",
                    protocol.as_str()
                ));
                if let OpenResult::Open { conn } = result {
                    self.conns.push(conn);
                }
            }
            let conn = *self.conns.first()?;
            loop {
                let answer = self.exchange(conn, "genesis\n")?;
                if let Some(hash) = GenesisHash::new(answer.clone()) {
                    return Some(NetworkIdentity::GenesisHash { hash });
                }
                pause(Duration::from_millis(50));
            }
        }
        loop {
            let endpoints = self.endpoints.clone();
            let result = self.rpc(
                endpoints,
                json!({"jsonrpc": "2.0", "id": 1, "method": "eth_chainId"}),
            );
            if let RpcResult::Agreed { body } = result {
                let value: Value = serde_json::from_str(&body).ok()?;
                let hex = value["result"].as_str()?.strip_prefix("0x")?;
                let id = u64::from_str_radix(hex, 16).ok()?;
                return Some(NetworkIdentity::ChainId {
                    id: ChainId::new(id.to_string())?,
                });
            }
            pause(Duration::from_millis(50));
        }
    }

    /// What the network says, as a JSON value: through the pool for an RPC reader, from the first peer for a light
    /// client. `Err` names why there is nothing to read.
    fn read(&mut self, method: &str, id: &str) -> Result<Value, String> {
        if self.light {
            let conn = *self.conns.first().ok_or("no_peer")?;
            let text = self
                .exchange(conn, &format!("{method} {id}\n"))
                .ok_or("peer_closed")?;
            return serde_json::from_str(&text).map_err(|_| "peer_answer_malformed".to_owned());
        }
        let mut endpoints = self.endpoints.clone();
        if self.hostile("undeclared") {
            endpoints.push("https://evil.example".to_owned());
        }
        let rpc_method = match method {
            "status" => "rutent_movementStatus",
            _ => "rutent_movementOf",
        };
        match self.rpc(
            endpoints,
            json!({"jsonrpc": "2.0", "id": 2, "method": rpc_method, "params": [id]}),
        ) {
            RpcResult::Agreed { body } => {
                let value: Value =
                    serde_json::from_str(&body).map_err(|_| "rpc_answer_malformed")?;
                Ok(value["result"].clone())
            }
            RpcResult::Unreadable { why } if self.hostile("ignore-quorum") => {
                self.io.log(format!("ignoring the pool: {why}"));
                Ok(
                    json!({"status": "final", "destination": "dest-1", "asset": "usdc", "amount": "100"}),
                )
            }
            RpcResult::Unreadable { why } => Err(why),
        }
    }

    fn answer(&mut self, question: &ReaderQuestion, found: &NetworkIdentity) -> Answered {
        let unreadable = |why: &str| Answered::Unreadable {
            why: Reason::new(why).unwrap_or_else(|| Reason::new("unreadable").expect("a reason")),
        };
        match &question.asked {
            Asked::NetworkRules(_) => Answered::NetworkRules {
                rules: NetworkRules {
                    identity: found.clone(),
                    finality: if self.light {
                        Finality::Checkpoint(NoDetail {})
                    } else {
                        Finality::Confirmations {
                            count: Depth::new(12).expect("a depth"),
                        }
                    },
                    reorg: if self.light {
                        ReorgRule::UntilFinal(NoDetail {})
                    } else {
                        ReorgRule::DepthBound {
                            depth: Depth::new(12).expect("a depth"),
                        }
                    },
                    trust: match self.trust.clone().expect("started") {
                        TrustRule::RpcPool { endpoints, .. } if self.hostile("other-rules") => {
                            TrustRule::RpcPool {
                                quorum: endpoints.len() as u32,
                                endpoints,
                            }
                        }
                        trust => trust,
                    },
                },
            },
            Asked::MovementStatus { movement } => match self.read("status", movement.as_str()) {
                Ok(value) => standing(&value).map_or_else(
                    || unreadable("answer_malformed"),
                    |standing| Answered::MovementStatus { standing },
                ),
                Err(why) => unreadable(&why),
            },
            Asked::MovementOf { transaction } => {
                match self.read("movement_of", transaction.as_str()) {
                    Ok(Value::Null) => Answered::MovementOf { movement: None },
                    Ok(value) => match (
                        value["id"].as_str().and_then(Id::new),
                        value["destination"].as_str().and_then(Id::new),
                    ) {
                        (Some(id), Some(destination)) => Answered::MovementOf {
                            movement: Some(Tied { id, destination }),
                        },
                        _ => unreadable("answer_malformed"),
                    },
                    Err(why) => unreadable(&why),
                }
            }
        }
    }
}

/// Waits `for_` without a timed sleep, which the provider's `seccomp` allow-list doesn't take: the clock and yielding
/// are allowed.
fn pause(for_: Duration) {
    let until = std::time::Instant::now() + for_;
    while std::time::Instant::now() < until {
        std::thread::yield_now();
    }
}

fn standing(value: &Value) -> Option<Standing> {
    let movement = || -> Option<Movement> {
        Some(Movement {
            destination: Id::new(value["destination"].as_str()?)?,
            asset: Id::new(value["asset"].as_str()?)?,
            amount: Amount::new(value["amount"].as_str()?)?,
        })
    };
    Some(match value["status"].as_str()? {
        "unknown" => Standing::Unknown(NoDetail {}),
        "pending" => Standing::Pending(movement()?),
        "final" => Standing::Final(movement()?),
        "reorged_out" => Standing::ReorgedOut(NoDetail {}),
        _ => return None,
    })
}

fn probe(io: &mut Io, address: &str) {
    for target in [address, "93.184.216.34:443", "[2606:2800:220:1::]:443"] {
        let outcome = match target.parse::<SocketAddr>() {
            Ok(to) => match TcpStream::connect_timeout(&to, Duration::from_secs(2)) {
                Ok(_) => "connected".to_owned(),
                Err(e) => format!("refused: {:?} {e}", e.kind()),
            },
            Err(_) => "not an address".to_owned(),
        };
        io.log(format!("probe {target}: {outcome}"));
    }
    let mut table = String::new();
    match std::fs::File::open("/proc/net/route").and_then(|mut f| f.read_to_string(&mut table)) {
        Ok(_) => {
            let routes = table
                .lines()
                .skip(1)
                .filter(|l| !l.trim().is_empty())
                .count();
            io.log(format!("probe routes: {routes}"));
        }
        Err(e) => io.log(format!("probe routes: unreadable: {e}")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut hostile = Vec::new();
    let mut peers = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--hostile" => {
                hostile.extend(
                    args.get(i + 1)
                        .map(|a| a.split(',').map(str::to_owned).collect::<Vec<_>>())
                        .unwrap_or_default(),
                );
                i += 2;
            }
            "--peer" => {
                let spec = args.get(i + 1).cloned().unwrap_or_default();
                let mut parts = spec.rsplitn(3, ':');
                let port = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
                let protocol = match parts.next() {
                    Some("wss") => Protocol::Wss,
                    _ => Protocol::P2p,
                };
                let host = parts.next().unwrap_or_default().to_owned();
                peers.push(Peer {
                    host,
                    protocol,
                    port,
                });
                i += 2;
            }
            _ => i += 1,
        }
    }
    // The provider's jail, from inside, before anything is read: what every reader does first. `unconfined` skips it,
    // for the host to refuse.
    if !hostile.iter().any(|h| h == "unconfined")
        && let Err(e) = rutent_reader_host::jail::confine_reader()
    {
        eprintln!("standin_reader: not confined: {e}");
        std::process::exit(5);
    }
    let inspect = hostile.iter().any(|h| h == "inspect");
    let mut reader = Reader {
        io: Io {
            input: std::io::stdin(),
            output: std::io::stdout(),
            inspect,
        },
        hostile,
        peers,
        light: args.first().is_some_and(|m| m == "light-client"),
        trust: None,
        endpoints: Vec::new(),
        conns: Vec::new(),
        call: 0,
    };
    if inspect {
        reader.io.log(format!("args {args:?}"));
        for (name, value) in std::env::vars() {
            reader.io.log(format!("env {name}={value}"));
        }
    }
    let ToReader::Start {
        identity,
        trust,
        version,
        ..
    } = reader.io.receive()
    else {
        std::process::exit(2);
    };
    if let TrustRule::RpcPool { endpoints, .. } = &trust {
        reader.endpoints = endpoints.iter().map(|e| e.as_str().to_owned()).collect();
    }
    reader.trust = Some(trust);
    if let Some(address) = reader
        .hostile
        .iter()
        .find_map(|h| h.strip_prefix("probe-network=").map(str::to_owned))
    {
        probe(&mut reader.io, &address);
    }
    let found = if reader.hostile("skip-checks") {
        identity
    } else {
        let Some(found) = reader.check() else {
            reader.io.log("checks: nothing found");
            loop {
                std::thread::park();
            }
        };
        reader.io.send(&FromReader::Checked {
            identity: found.clone(),
        });
        found
    };
    loop {
        let ToReader::Question { question } = reader.io.receive() else {
            std::process::exit(4);
        };
        if reader.hostile("crash") {
            std::process::exit(3);
        }
        if reader.hostile("hang") {
            loop {
                std::thread::park();
            }
        }
        let answer = reader.answer(&question, &found);
        let request = if reader.hostile("other-request") {
            RequestId::from_random([0x5a; 16]).expect("a request id")
        } else {
            question.request.clone()
        };
        let body = ReaderAnswer {
            request,
            network: question.network.clone(),
            version: version.clone(),
            asked: question.asked.clone(),
            answer,
        };
        let answer = serde_json::to_value(&body).expect("an answer");
        reader.io.send(&FromReader::Answer { answer });
        let _ = reader.io.output.flush();
    }
}
