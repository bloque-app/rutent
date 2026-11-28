//! What the reader host's tests share: the stand-in readers' program, keys, a stand-in network the egress layer
//! reaches (a resolver and a connector of the tests' own, and a dialer mapping public addresses to loopback peers), a
//! stand-in light-client peer, and Core's side of a question: drawing it and checking its answer as Core would.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use rutent_conformance::signing::{base64url, public_key, verify};
use rutent_contracts::module_interface::{Host as HostName, Identifier, Name, SecretValue};
use rutent_contracts::provider_role::{
    Deployment, NoDetail, ProtocolVersions, PublicKey, Statement,
};
use rutent_contracts::reader::{
    Asked, ChainId, Expected, GenesisHash, NetworkIdentity, ReaderAnswer, ReaderQuestion,
    RequestId, RpcEndpoint, TrustRule, check_reader_answer,
};
use rutent_contracts::router_api::{Id, ProtocolVersion, Timestamp, Version};
use rutent_prov::egress::{ConnectError, Connector, Dial, Dialer, NameResolver, Request, Response};
use rutent_prov::keys::{ResolveError, Resolver};
use rutent_reader_host::config::{LightClientRule, PortRule, Protocol, ReaderLimits};
use rutent_reader_host::egress::Network;
use rutent_reader_host::records::MemoryRecords;
use rutent_reader_host::{Host, HostConfig, ReaderConfig, Services};

/// The protocol version the tests' answers are made under.
pub const PROTOCOL: &str = "4.0.0";

/// The stand-in readers' program, `examples/standin_reader.rs`, which `cargo test` builds beside the tests:
/// `RUTENT_STANDIN_READER` if set, else the `examples` directory next to the directory this test runs from, or above
/// it.
pub fn standin() -> PathBuf {
    if let Some(path) = std::env::var_os("RUTENT_STANDIN_READER") {
        return PathBuf::from(path);
    }
    let exe = std::env::current_exe().expect("the test's own path");
    for dir in exe.ancestors().skip(1) {
        let candidate = dir.join("examples").join("standin_reader");
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!(
        "no stand-in reader beside {}: build it with `cargo build -p rutent-reader-host --examples`",
        exe.display()
    );
}

/// A reader's statement key, from its name: the same on every machine.
pub fn key_of(reader: &str) -> SigningKey {
    let mut seed = [0u8; 32];
    for (i, b) in format!("reader-key:{reader}").bytes().enumerate() {
        seed[i % 32] ^= b.rotate_left(i as u32 % 8);
    }
    SigningKey::from_bytes(&seed)
}

/// The private half of a reader's key, as the store holds it: the base64url text of its seed.
pub fn secret_of(reader: &str) -> SecretValue {
    SecretValue::new(base64url(&key_of(reader).to_bytes())).expect("a secret")
}

/// The public half: the reader's listed statement key.
pub fn listed_key(reader: &str) -> PublicKey {
    public_key(&key_of(reader))
}

/// The tests' deployment.
pub fn deployment() -> Deployment {
    Deployment {
        name: "rutent-test".to_owned(),
        operator_key: public_key(&SigningKey::from_bytes(&[7u8; 32])),
    }
}

pub fn id(text: &str) -> Id {
    Id::new(text).expect("an id")
}

/// A request id drawn at random, as Core draws it.
pub fn request_id() -> RequestId {
    loop {
        let mut bytes = [0u8; 16];
        for half in bytes.chunks_mut(8) {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u64(0x5eed);
            half.copy_from_slice(&hasher.finish().to_le_bytes());
        }
        if let Some(id) = RequestId::from_random(bytes) {
            return id;
        }
    }
}

/// A question to `reader` about `network`, with a deadline well after the test.
pub fn question(reader: &str, network: &str, asked: Asked) -> ReaderQuestion {
    ReaderQuestion {
        request: request_id(),
        reader: id(reader),
        network: id(network),
        deadline: Timestamp::new("2099-01-01T00:00:00Z").expect("a time"),
        asked,
    }
}

/// The time `before` now on this machine's clock, to the millisecond: a deadline already past when it is sent.
pub fn timestamp_before(before: Duration) -> Timestamp {
    written(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .saturating_sub(before),
    )
}

/// `at`, time since the epoch, as the contracts write a time.
fn written(at: Duration) -> Timestamp {
    let (seconds, millis) = (at.as_secs(), at.subsec_millis());
    // H. Hinnant's `civil_from_days`.
    let z = (seconds / 86_400) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let rest = seconds % 86_400;
    Timestamp::new(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    ))
    .expect("a time")
}

pub fn network_rules() -> Asked {
    Asked::NetworkRules(NoDetail {})
}

pub fn status_of(movement: &str) -> Asked {
    Asked::MovementStatus {
        movement: id(movement),
    }
}

/// The answer to `question` from the reader `config` names, held as Core holds it: the signature verified against
/// the key it names, then [`check_reader_answer`] against the listed key and version, at a time before the deadline.
pub fn as_core_reads(
    bytes: &[u8],
    question: &ReaderQuestion,
    config: &ReaderConfig,
) -> Statement<ReaderAnswer> {
    verify(bytes).expect("the answer's signature verifies");
    let statement: Statement<ReaderAnswer> = serde_json::from_slice(bytes).expect("an answer");
    let versions = ProtocolVersions::only(PROTOCOL);
    let expected = Expected {
        deployment: &deployment(),
        versions: &versions,
        key: &config.statement_key,
        version: &config.version,
    };
    let now = Timestamp::new("2026-11-27T10:00:00Z").expect("a time");
    check_reader_answer(question, &statement, &expected, &now)
        .expect("the answer holds to the question");
    statement
}

/// Asks `host` and reads the answer as Core does.
pub fn ask(host: &Host, config: &ReaderConfig, asked: Asked) -> ReaderAnswer {
    let question = question(config.name.as_str(), config.network.as_str(), asked);
    let bytes = host
        .answer(&serde_json::to_vec(&question).expect("a question"))
        .expect("an answer");
    as_core_reads(&bytes, &question, config).statement
}

/// Whether an answer is unreadable, and why.
pub fn unreadable(answer: &ReaderAnswer) -> Option<&str> {
    match &answer.answer {
        rutent_contracts::reader::Answered::Unreadable { why } => Some(why.as_str()),
        _ => None,
    }
}

/// Fast limits for the tests.
pub fn limits() -> ReaderLimits {
    ReaderLimits {
        hang_after_ms: 3_000,
        start_within_ms: 20_000,
        restart_after_ms: 100,
        egress_give_up_after_ms: 2_000,
        pending: 4,
        ..ReaderLimits::default()
    }
}

/// The pool every EVM stand-in reads: three endpoints, a quorum of two.
pub fn pool() -> Vec<&'static str> {
    vec![
        "https://rpc-a.base.example",
        "https://rpc-b.base.example",
        "https://rpc-c.base.example",
    ]
}

/// An EVM network's stand-in reader over `pool()`.
pub fn rpc_reader(name: &str, hostile: &[&str]) -> ReaderConfig {
    let mut args = vec!["rpc".to_owned()];
    if !hostile.is_empty() {
        args.extend(["--hostile".to_owned(), hostile.join(",")]);
    }
    ReaderConfig {
        name: id(name),
        network: id("base"),
        version: Version::new("0.1.0").expect("a version"),
        identity: NetworkIdentity::ChainId {
            id: ChainId::new("8453").expect("a chain id"),
        },
        trust: TrustRule::RpcPool {
            endpoints: pool()
                .into_iter()
                .map(|e| RpcEndpoint::new(e).expect("an endpoint"))
                .collect(),
            quorum: 2,
        },
        program: standin(),
        args,
        statement_key: listed_key(name),
        light_client: None,
        limits: limits(),
    }
}

/// The genesis of the stand-in Substrate network.
pub const GENESIS: &str = "0x9f3c2a7be14d4c0e8a51d6b07e2f9c139f3c2a7be14d4c0e8a51d6b07e2f9c13";

/// A Substrate network's stand-in light-client reader, reaching `peers` (`host:protocol:port`) under `rule`.
pub fn light_reader(
    name: &str,
    peers: &[&str],
    rule: LightClientRule,
    hostile: &[&str],
) -> ReaderConfig {
    let mut args = vec!["light-client".to_owned()];
    for peer in peers {
        args.extend(["--peer".to_owned(), (*peer).to_owned()]);
    }
    if !hostile.is_empty() {
        args.extend(["--hostile".to_owned(), hostile.join(",")]);
    }
    ReaderConfig {
        name: id(name),
        network: id("kreivo"),
        version: Version::new("0.1.0").expect("a version"),
        identity: NetworkIdentity::GenesisHash {
            hash: GenesisHash::new(GENESIS).expect("a genesis hash"),
        },
        trust: TrustRule::LightClient(NoDetail {}),
        program: standin(),
        args,
        statement_key: listed_key(name),
        light_client: Some(rule),
        limits: limits(),
    }
}

/// The light client's rule in the tests: p2p on 30333 and wss on 443, at most `cap` connections.
pub fn kreivo_rule(cap: u32) -> LightClientRule {
    LightClientRule {
        ports: vec![
            PortRule {
                protocol: Protocol::P2p,
                port: 30333,
            },
            PortRule {
                protocol: Protocol::Wss,
                port: 443,
            },
        ],
        max_connections: cap,
    }
}

/// A host's configuration of `readers`.
pub fn host_config(readers: Vec<ReaderConfig>) -> HostConfig {
    HostConfig {
        deployment: deployment(),
        protocol_version: ProtocolVersion::new(PROTOCOL).expect("a protocol version"),
        readers,
    }
}

/// A test double of `T-014-01`'s resolver: the keys in memory, keyed by provider (here, the reader) and name (here,
/// the listed public key's text), as a vault bucket's keys are.
#[derive(Default)]
pub struct MemoryKeys {
    pub keys: Mutex<BTreeMap<(String, String), SecretValue>>,
}

impl Resolver for MemoryKeys {
    fn resolve(&self, provider: &Identifier, name: &Name) -> Result<SecretValue, ResolveError> {
        self.keys
            .lock()
            .unwrap()
            .get(&(provider.as_str().to_owned(), name.as_str().to_owned()))
            .cloned()
            .ok_or(ResolveError::NotFound)
    }
}

/// The keys of `readers`, in memory.
pub fn keys_of(readers: &[ReaderConfig]) -> Arc<MemoryKeys> {
    let keys = MemoryKeys::default();
    for reader in readers {
        keys.keys.lock().unwrap().insert(
            (
                reader.name.as_str().to_owned(),
                reader.statement_key.as_str().to_owned(),
            ),
            secret_of(reader.name.as_str()),
        );
    }
    Arc::new(keys)
}

/// A public address standing for a name in the tests: each name its own, none of them special-purpose.
pub fn address_of(name: &str) -> IpAddr {
    let n = name
        .bytes()
        .fold(7u8, |n, b| n.wrapping_mul(31).wrapping_add(b));
    IpAddr::from([93, 184, 216, n.max(1)])
}

/// Resolves every name to [`address_of`], a name with `loopback` in it to `127.0.0.1`, or to what a test set for it.
#[derive(Default)]
pub struct StubResolver {
    pub overrides: Mutex<BTreeMap<String, Vec<IpAddr>>>,
}

impl NameResolver for StubResolver {
    fn resolve(&self, host: &HostName, _port: NonZeroU16) -> Vec<IpAddr> {
        self.overrides
            .lock()
            .unwrap()
            .get(host.as_str())
            .cloned()
            .unwrap_or_else(|| {
                if host.as_str().contains("loopback") {
                    vec![IpAddr::from([127, 0, 0, 1])]
                } else {
                    vec![address_of(host.as_str())]
                }
            })
    }
}

/// How a stand-in RPC endpoint answers a request's body.
pub type Answerer = Box<dyn Fn(&str) -> Result<Response, ConnectError> + Send + Sync>;

/// The stand-in RPC endpoints, by host name: each answers by its own function; a name with none is unreachable.
/// Every exchange's name is kept, so a test sees whom the host reached.
#[derive(Default)]
pub struct StubConnector {
    pub endpoints: Mutex<BTreeMap<String, Arc<Answerer>>>,
    pub dialled: Mutex<Vec<String>>,
}

impl StubConnector {
    pub fn set(&self, host: &str, answerer: Answerer) {
        self.endpoints
            .lock()
            .unwrap()
            .insert(host.to_owned(), Arc::new(answerer));
    }

    pub fn dialled(&self) -> Vec<String> {
        self.dialled.lock().unwrap().clone()
    }
}

impl Connector for StubConnector {
    fn exchange(&self, dial: &Dial<'_>, request: &Request) -> Result<Response, ConnectError> {
        self.dialled
            .lock()
            .unwrap()
            .push(dial.name.as_str().to_owned());
        let answerer = self
            .endpoints
            .lock()
            .unwrap()
            .get(dial.name.as_str())
            .cloned();
        match answerer {
            Some(answer) => answer(&String::from_utf8_lossy(request.body())),
            None => Err(ConnectError::Unreachable),
        }
    }
}

pub fn ok(body: serde_json::Value) -> Result<Response, ConnectError> {
    Ok(Response {
        status: 200,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: body.to_string().into_bytes(),
    })
}

/// An honest stand-in EVM node: chain id 8453 (`0x2105`), the movement `0xmove` final, `100` of `usdc` to `dest-1`,
/// and a large answer to `rutent_large`. `lie` makes it say the movement went to `dest-evil`.
pub fn evm_node(lie: bool) -> Answerer {
    Box::new(move |body: &str| {
        let request: serde_json::Value =
            serde_json::from_str(body).map_err(|_| ConnectError::NoAnswer)?;
        let id = request["id"].clone();
        match request["method"].as_str() {
            Some("eth_chainId") => {
                ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": "0x2105"}))
            }
            Some("rutent_movementStatus") => {
                ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                    "status": "final",
                    "destination": if lie { "dest-evil" } else { "dest-1" },
                    "asset": "usdc",
                    "amount": "100"
                }}))
            }
            // A slow answer, 20 ms, whatever the call's deadline: an endpoint that keeps the host busy.
            Some("rutent_slow") => {
                std::thread::sleep(Duration::from_millis(20));
                ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": 1}))
            }
            // A large answer, 32 KiB: what fills a reader's input when it stops reading.
            Some("rutent_large") => {
                ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": "x".repeat(32 << 10)}))
            }
            _ => ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": null})),
        }
    })
}

/// A stand-in endpoint that is up but gives no usable answer.
pub fn failing() -> Answerer {
    Box::new(|_| {
        Ok(Response {
            status: 503,
            headers: Vec::new(),
            body: Vec::new(),
        })
    })
}

/// Maps each public address a test uses to the loopback peer standing for it.
#[derive(Default)]
pub struct MappedDialer {
    pub map: Mutex<BTreeMap<IpAddr, SocketAddr>>,
}

impl Dialer for MappedDialer {
    fn connect(&self, to: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        let mapped = self.map.lock().unwrap().get(&to.ip()).copied();
        match mapped {
            Some(at) => TcpStream::connect_timeout(&at, timeout),
            None => Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "no peer stands there",
            )),
        }
    }
}

/// A stand-in light-client peer on a loopback port: it answers `genesis` with its genesis once it is open, and
/// `not-yet` before, `status <id>` with the movement final, and `wait` never.
pub struct Peer {
    pub address: SocketAddr,
    pub open: Arc<AtomicBool>,
    pub accepted: Arc<AtomicUsize>,
}

impl Peer {
    pub fn serve(genesis: &'static str, open: bool) -> Peer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("its address");
        let open = Arc::new(AtomicBool::new(open));
        let accepted = Arc::new(AtomicUsize::new(0));
        let (gate, count) = (open.clone(), accepted.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                count.fetch_add(1, Ordering::SeqCst);
                let gate = gate.clone();
                std::thread::spawn(move || {
                    let mut writer = stream.try_clone().expect("a stream");
                    for line in BufReader::new(stream).lines() {
                        let Ok(line) = line else { return };
                        // `wait` is never answered: a peer that keeps a light client waiting.
                        if line == "wait" {
                            continue;
                        }
                        let reply = match line.split_once(' ').map_or((line.as_str(), ""), |(a, b)| (a, b)) {
                            ("genesis", _) if gate.load(Ordering::SeqCst) => genesis.to_owned(),
                            ("genesis", _) => "not-yet".to_owned(),
                            ("status", _) => serde_json::json!({
                                "status": "final", "destination": "dest-1", "asset": "ksm", "amount": "7"
                            })
                            .to_string(),
                            _ => "null".to_owned(),
                        };
                        // One write, so a reply never arrives in two parts.
                        if writer.write_all(format!("{reply}\n").as_bytes()).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Peer {
            address,
            open,
            accepted,
        }
    }
}

/// A host over the stand-in network, with its records and connector for the test to read.
pub struct Rig {
    pub host: Host,
    pub records: Arc<MemoryRecords>,
    pub connector: Arc<StubConnector>,
    pub resolver: Arc<StubResolver>,
    pub dialer: Arc<MappedDialer>,
}

impl Rig {
    /// Starts a host of `readers`, with `connector` standing for the RPC endpoints and `peers` mapped by name.
    pub fn start(
        readers: Vec<ReaderConfig>,
        connector: Arc<StubConnector>,
        peers: &[(&str, &Peer)],
    ) -> Rig {
        let records = Arc::new(MemoryRecords::new());
        let resolver = Arc::new(StubResolver::default());
        let dialer = Arc::new(MappedDialer::default());
        for (name, peer) in peers {
            dialer
                .map
                .lock()
                .unwrap()
                .insert(address_of(name), peer.address);
        }
        let keys = keys_of(&readers);
        let host = Host::start(
            host_config(readers),
            Services {
                keys,
                network: Network {
                    resolver: resolver.clone(),
                    connector: connector.clone(),
                    dialer: dialer.clone(),
                },
                records: records.clone(),
            },
        )
        .expect("a host");
        Rig {
            host,
            records,
            connector,
            resolver,
            dialer,
        }
    }
}

/// A connector with every pool endpoint an honest node.
pub fn honest_pool() -> Arc<StubConnector> {
    let connector = Arc::new(StubConnector::default());
    for endpoint in pool() {
        connector.set(endpoint.trim_start_matches("https://"), evm_node(false));
    }
    connector
}
