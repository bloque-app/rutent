//! The simulated networks' readers as reader services, inside the test harness (`T-024-25`, `DEC-28`, ADR-0007 as
//! amended): each listed network's reader run by the reader host (`rutent_reader_host`, `T-015-04`), the same `reader`
//! protocol over the same mTLS handshake, over TLS inside the test process, with certificates from a test CA the
//! harness makes ([`TestCa`]).
//!
//! - **The readers.** One per network the log lists ([`rutent_scenarios::assembly::Core::public_networks`]): the
//!   simulated reader, `examples/sim_reader.rs`, started by the reader host in its jail from the listing (its name,
//!   network, version, identity and trust rule), its statement key resolved by the host from a key store of the
//!   harness's ([`KeyStore`]) and never handed to the reader.
//! - **Their network.** The reader host's network reaches only the endpoints the simulated world serves
//!   ([`rutent_scenarios::sim::readers::Endpoints`]): a resolver that resolves their names ([`SimResolver`]), a
//!   connector that answers a pool's requests from the world ([`SimConnector`]), and a dialer that connects a light
//!   client's peer, by its public address, to a loopback server answering from the world ([`SimDialer`], [`PeerServer`]).
//!   The host's own egress layer holds every call to the reader's declared destinations, as in production.
//! - **Core's side** ([`Readers::ask`]): Core's own adapter (`rutent_core_orch::closing::Readers`, `T-020-60`), handed
//!   the test CA, Core's client certificate and the host's address as a deployment hands them: a question under a
//!   request id drawn from Core's random source, its deadline and its arrival on Core's clock, over Core's mTLS client
//!   holding the host to the test CA and the listed subject name, its answer's signature verified against the listed
//!   key and then held to the question and to the listing's key and version (`check_reader_answer`).
//! - **As a process of its own** (`examples/sim_reader.rs host`): the assembly runs this harness in the simulated reader
//!   host's process beside Core (`rutent_scenarios::sim::reader_service`), over the world's endpoints read through a
//!   loopback connection, so every scenario reads through Core's adapter.
//! - **The planted faults** ([`Plan`]): a reader that ties a deposit by destination and amount, a hostile reader
//!   (`crash`, `malformed`, `other-movement`, `undeclared`) and a key the listing doesn't name; an endpoint that lies or
//!   is silent is the world's, planted on its endpoints (`Endpoints::plant`).
//!
//! The jail runs here as in production: on Linux the provider's namespaces and in-process layers, which the host holds
//! the reader to before it serves it; on macOS, for development, the provider's `sandbox-exec` profile. No reader runs
//! unjailed.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SerialNumber,
};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rutent_conformance::signing::base64url;
use rutent_contracts::core_internal::{NetworkListing, PublicNetwork};
use rutent_contracts::module_interface::{Host as HostName, Identifier, Name, SecretValue};
use rutent_contracts::reader::{Answered, Asked, ReaderMismatch, TrustRule};
use rutent_contracts::router_api::ProtocolVersion;
use rutent_core_orch::closing::Unreadable as CoreUnreadable;
use rutent_core_orch::closing::readers::Put;
use rutent_prov::egress::{ConnectError, Connector, Dial, Dialer, NameResolver, Request, Response};
use rutent_prov::keys::{ResolveError, Resolver};
use rutent_reader_host::config::{LightClientRule, PortRule, Protocol, ReaderLimits};
use rutent_reader_host::egress::Network;
use rutent_reader_host::link::{Link, LinkTls};
use rutent_reader_host::records::{CallRecord, MemoryRecords};
use rutent_reader_host::{Host, HostConfig, ReaderConfig, Services};
use rutent_scenarios::sim::readers::{
    self, CORE_NAME, Endpoints, PEER_PORT, Rule, peer_of, pool_host, rule_of,
};

/// How long a reader is given to pass its start-up checks.
pub const READY: Duration = Duration::from_secs(60);

/// The seed of the test CA's key: fixed, so the CA's certificate is the same on every machine and every run, and the
/// assembly lists it by digest without making it ([`readers::CA_SHA256`]).
const CA_SEED: [u8; 32] = *b"rutent-scenarios-test-ca-seed-01";

/// The test CA the harness makes: the operator's CA, in the scenarios, which issues the reader host's server
/// certificate and Core's client certificate.
pub struct TestCa {
    certificate: Certificate,
    key: KeyPair,
}

/// A certificate the test CA issued, with its key.
pub struct Leaf {
    pub chain: Vec<CertificateDer<'static>>,
    pub chain_pem: String,
    pub key_der: Vec<u8>,
    pub key_pem: String,
}

impl TestCa {
    /// The test CA: an Ed25519 key from [`CA_SEED`], and fixed fields (its name, serial and validity), so its
    /// certificate is deterministic (an Ed25519 signature is).
    pub fn new() -> Self {
        Self::of(CA_SEED, "rutent scenarios test CA")
    }

    /// Another CA than the operator's, of its own key and name: what no listing names.
    pub fn other() -> Self {
        Self::of(*b"rutent-scenarios-other-ca-seed-1", "another CA")
    }

    fn of(seed: [u8; 32], common_name: &str) -> Self {
        // PKCS #8 v1 of an Ed25519 key (RFC 8410): the fixed prefix, then the 32-byte seed.
        let mut pkcs8 = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ];
        pkcs8.extend_from_slice(&seed);
        let key = KeyPair::from_pkcs8_der_and_sign_algo(
            &PrivatePkcs8KeyDer::from(pkcs8),
            &rcgen::PKCS_ED25519,
        )
        .expect("the test CA's key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("parameters");
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, common_name);
        params.distinguished_name = name;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.serial_number = Some(SerialNumber::from(vec![0x01]));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let certificate = params.self_signed(&key).expect("the test CA");
        TestCa { certificate, key }
    }

    /// Its certificate, DER.
    pub fn der(&self) -> Vec<u8> {
        self.certificate.der().to_vec()
    }

    /// Its certificate, PEM.
    pub fn pem(&self) -> String {
        self.certificate.pem()
    }

    /// A leaf for `name`, for `purpose`, under a fresh key.
    pub fn issue(&self, name: &str, purpose: ExtendedKeyUsagePurpose) -> Leaf {
        let key = KeyPair::generate().expect("a leaf's key");
        let mut params = CertificateParams::new(vec![name.to_owned()]).expect("parameters");
        params.extended_key_usages = vec![purpose];
        let leaf = params
            .signed_by(&key, &self.certificate, &self.key)
            .expect("a leaf");
        Leaf {
            chain: vec![leaf.der().clone()],
            chain_pem: leaf.pem(),
            key_der: key.serialize_der(),
            key_pem: key.serialize_pem(),
        }
    }
}

/// The simulated reader's program, `examples/sim_reader.rs`, as this tree builds it (#353's review, Low 1): the
/// library's [`rutent_scenarios::sim::reader_service::program`], which finds the example `cargo test` built beside the
/// test binary and builds it only for a run of one test binary that built none.
pub fn sim_reader() -> PathBuf {
    rutent_scenarios::sim::reader_service::program()
}

/// The harness's key store, as `T-014-01`'s resolver: each reader's statement key, by the reader and the public key's
/// text, as a vault bucket holds them. The reader host resolves from it per answer; no reader ever sees it.
#[derive(Default)]
pub struct KeyStore {
    keys: Mutex<BTreeMap<(String, String), SecretValue>>,
}

impl KeyStore {
    fn hold(&self, reader: &str, seed: [u8; 32]) {
        let key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let public = rutent_conformance::signing::public_key(&key);
        self.keys.lock().unwrap().insert(
            (reader.to_owned(), public.as_str().to_owned()),
            SecretValue::new(base64url(&seed)).expect("a secret"),
        );
    }
}

impl Resolver for KeyStore {
    fn resolve(&self, provider: &Identifier, name: &Name) -> Result<SecretValue, ResolveError> {
        self.keys
            .lock()
            .unwrap()
            .get(&(provider.as_str().to_owned(), name.as_str().to_owned()))
            .cloned()
            .ok_or(ResolveError::NotFound)
    }
}

/// Resolves the names of the endpoints the world serves the readers, and of the world's own host for each network
/// (`{network}.chain.example`, which no reader declares), each to a public address; nothing else resolves.
pub struct SimResolver(pub Endpoints);

/// The address the world's own host for a network resolves to, in a range of its own.
fn world_host_address(host: &str) -> IpAddr {
    let n = host
        .bytes()
        .fold(7u8, |n, b| n.wrapping_mul(31).wrapping_add(b));
    IpAddr::V4(Ipv4Addr::new(185, 12, 72, n.max(1)))
}

/// The network a host is the world's own host for, `{network}.chain.example`, if it is one.
fn world_host_of(host: &str) -> Option<&str> {
    host.strip_suffix(".chain.example")
        .filter(|network| !network.contains('.'))
}

impl NameResolver for SimResolver {
    fn resolve(&self, host: &HostName, _port: NonZeroU16) -> Vec<IpAddr> {
        match self.0.endpoint(host.as_str()) {
            Some(served) => vec![served.address],
            None if world_host_of(host.as_str()).is_some() => {
                vec![world_host_address(host.as_str())]
            }
            None => Vec::new(),
        }
    }
}

/// A pool's endpoints, as the world serves them: each request answered by the endpoint its declared name names
/// ([`Endpoints::answer`]), as the world's network stands; a silent endpoint gives no answer. The world's own host
/// for a network would answer as the network's endpoint does, if a reader ever reached it. Every exchange's name is
/// kept, so a test sees whom the host reached.
pub struct SimConnector {
    pub endpoints: Endpoints,
    dialled: Mutex<Vec<String>>,
}

impl SimConnector {
    pub fn dialled(&self) -> Vec<String> {
        self.dialled.lock().unwrap().clone()
    }
}

impl Connector for SimConnector {
    fn exchange(&self, dial: &Dial<'_>, request: &Request) -> Result<Response, ConnectError> {
        let host = dial.name.as_str();
        self.dialled.lock().unwrap().push(host.to_owned());
        let target = if self.endpoints.endpoint(host).is_some() {
            host.to_owned()
        } else if let Some(network) = world_host_of(host) {
            match rule_of(network) {
                Rule::RpcPool => pool_host(network, readers::POOL[0]),
                Rule::LightClient => peer_of(network),
            }
        } else {
            return Err(ConnectError::Unreachable);
        };
        match self
            .endpoints
            .answer(&target, &String::from_utf8_lossy(request.body()))
        {
            Some(text) => Ok(Response {
                status: 200,
                headers: vec![("content-type".to_owned(), "application/json".to_owned())],
                body: text.into_bytes(),
            }),
            None => Err(ConnectError::NoAnswer),
        }
    }
}

/// A light client's peer, as the world serves it: a loopback server answering one JSON-RPC request per line, as the
/// peer's endpoint does ([`Endpoints::answer`]); a silent peer answers nothing. Stopped, and its accepting thread
/// joined, when dropped (the review of `T-024-25` at `64309ac3`, Low 5); a connection's thread ends with its
/// connection, which the reader's process holds.
pub struct PeerServer {
    pub address: SocketAddr,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for PeerServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_secs(1));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl PeerServer {
    pub fn serve(endpoints: Endpoints, host: String) -> PeerServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("its address");
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                let (endpoints, host) = (endpoints.clone(), host.clone());
                std::thread::spawn(move || {
                    let Ok(mut writer) = stream.try_clone() else {
                        return;
                    };
                    for line in BufReader::new(stream).lines() {
                        let Ok(line) = line else { return };
                        let Some(reply) = endpoints.answer(&host, &line) else {
                            continue;
                        };
                        // One write, so a reply arrives whole.
                        if writer.write_all(format!("{reply}\n").as_bytes()).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        PeerServer {
            address,
            stop,
            thread: Some(thread),
        }
    }
}

/// Connects a light client's peer, by the public address the world serves it at, to its loopback server; nothing else
/// connects. Every address dialled is kept.
#[derive(Default)]
pub struct SimDialer {
    peers: BTreeMap<IpAddr, SocketAddr>,
    servers: Vec<PeerServer>,
    dialled: Mutex<Vec<SocketAddr>>,
}

impl SimDialer {
    pub fn dialled(&self) -> Vec<SocketAddr> {
        self.dialled.lock().unwrap().clone()
    }
}

impl Dialer for SimDialer {
    fn connect(&self, to: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        self.dialled.lock().unwrap().push(to);
        match self.peers.get(&to.ip()) {
            Some(at) if to.port() == PEER_PORT => TcpStream::connect_timeout(at, timeout),
            _ => Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "no peer stands there",
            )),
        }
    }
}

pub use rutent_scenarios::sim::readers::Plan;

/// What Core reads of an answer, through its own adapter (`rutent_core_orch::closing::Readers`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// An answer that passed every check: its signature under the listed key, the question it answers, the listed key
    /// and version, and its deadline on Core's clock. A reader's own unreadable answer is one too, which settles nothing.
    Answered(Answered),
    /// An answer refused by `check_reader_answer` before anything in it is read: unreadable.
    Refused(Vec<ReaderMismatch>),
    /// Anything else Core's adapter reads as unreadable: no answer, one that isn't a signed answer, or one whose
    /// signature doesn't verify under the listed key.
    Unreadable(CoreUnreadable),
}

impl Read {
    /// The answer, where Core may read it.
    pub fn answered(&self) -> Option<&Answered> {
        match self {
            Read::Answered(answer) => Some(answer),
            _ => None,
        }
    }

    /// Whether Core reads nothing it can settle on: an unreadable answer, a refused one or none.
    pub fn is_unreadable(&self) -> bool {
        !matches!(self.answered(), Some(a) if !matches!(a, Answered::Unreadable { .. }))
    }

    /// Why an answered unreadable answer is so.
    pub fn why(&self) -> Option<&str> {
        match self.answered() {
            Some(Answered::Unreadable { why }) => Some(why.as_str()),
            _ => None,
        }
    }
}

/// Every listed network's reader, run by one reader host behind its mTLS link, and Core's side of the link.
pub struct Readers {
    link: Link,
    host: Arc<Host>,
    records: Arc<MemoryRecords>,
    pub connector: Arc<SimConnector>,
    pub dialer: Arc<SimDialer>,
    pub endpoints: Endpoints,
    listings: BTreeMap<String, NetworkListing>,
    configs: BTreeMap<String, ReaderConfig>,
    /// Core's side of the link: Core's own adapter, as the deployment hands it the test CA, its client certificate
    /// and the host's address (`T-020-60`), asking at Core's clock.
    core: rutent_core_orch::closing::Readers,
    /// The test CA's certificate, PEM: what the deployment hands Core as the operator's CA.
    pub ca_pem: String,
    /// Further links to the same host, each under a leaf of its own ([`Readers::serve_leaf`]).
    others: Vec<Link>,
    /// The client certificate the test CA issued Core, and its key, PEM.
    pub core_chain_pem: String,
    pub core_key_pem: String,
}

/// The light client's rule: its peer's p2p port and `wss`, at most two connections.
fn light_rule() -> LightClientRule {
    LightClientRule {
        ports: vec![
            PortRule {
                protocol: Protocol::P2p,
                port: PEER_PORT,
            },
            PortRule {
                protocol: Protocol::Wss,
                port: 443,
            },
        ],
        max_connections: 2,
    }
}

impl Readers {
    /// Starts a reader for each of `listed` under `plan`, over the endpoints the world serves, and waits for each to
    /// pass its start-up checks.
    ///
    /// # Panics
    ///
    /// If a listing names another CA than the test CA, or a reader doesn't pass its checks within [`READY`].
    pub fn start(listed: &[PublicNetwork], endpoints: Endpoints, plan: &Plan) -> Readers {
        let ca = TestCa::new();
        let context = rutent_scenarios::assembly::context();
        let keys = Arc::new(KeyStore::default());
        let mut configs = BTreeMap::new();
        let mut listings = BTreeMap::new();
        let mut peers = BTreeMap::new();
        let mut servers = Vec::new();
        for public in listed {
            let network = public.network.as_str().to_owned();
            let listing = public.listing.clone().expect("a listing");
            assert_eq!(
                listing.reader.ca.value.as_str(),
                rutent_prov::module::wire::sha256(&ca.der()),
                "{network} is listed under the harness's test CA"
            );
            let name = listing.endpoint.reader.as_str().to_owned();
            let light = matches!(listing.trust, TrustRule::LightClient(_));
            let mut args = vec![if light { "light-client" } else { "rpc" }.to_owned()];
            if light {
                args.extend([
                    "--peer".to_owned(),
                    format!("{}:p2p:{PEER_PORT}", peer_of(&network)),
                ]);
                let host = peer_of(&network);
                let served = endpoints
                    .endpoint(&host)
                    .expect("the world serves its peer");
                let server = PeerServer::serve(endpoints.clone(), host);
                peers.insert(served.address, server.address);
                servers.push(server);
            }
            if plan.ties {
                args.extend(["--match".to_owned(), "destination".to_owned()]);
            }
            if let Some(acts) = plan.hostile.get(&network) {
                args.extend(["--hostile".to_owned(), acts.join(",")]);
            }
            // The key the host signs with: the listed one, or, planted, one the listing doesn't name.
            let seed = if plan.unlisted_key.contains(&network) {
                readers::statement_seed(&format!("{name}-unlisted"))
            } else {
                readers::statement_seed(&name)
            };
            keys.hold(&name, seed);
            let config = ReaderConfig {
                name: listing.endpoint.reader.clone(),
                network: public.network.clone(),
                version: listing.version.clone(),
                identity: listing.identity.clone(),
                trust: listing.trust.clone(),
                program: sim_reader(),
                args,
                statement_key: rutent_conformance::signing::public_key(
                    &ed25519_dalek::SigningKey::from_bytes(&seed),
                ),
                light_client: light.then(light_rule),
                limits: ReaderLimits {
                    hang_after_ms: 10_000,
                    start_within_ms: 30_000,
                    restart_after_ms: 100,
                    egress_give_up_after_ms: 2_000,
                    pending: 8,
                    ..ReaderLimits::default()
                },
            };
            configs.insert(network.clone(), config);
            listings.insert(network, listing);
        }
        let records = Arc::new(MemoryRecords::new());
        let connector = Arc::new(SimConnector {
            endpoints: endpoints.clone(),
            dialled: Mutex::default(),
        });
        let dialer = Arc::new(SimDialer {
            peers,
            servers,
            dialled: Mutex::default(),
        });
        let host = Host::start(
            HostConfig {
                deployment: context.deployment.clone(),
                protocol_version: ProtocolVersion::new(context.protocol_version.as_str())
                    .expect("a protocol version"),
                readers: configs.values().cloned().collect(),
            },
            Services {
                keys,
                network: Network {
                    resolver: Arc::new(SimResolver(endpoints.clone())),
                    connector: connector.clone(),
                    dialer: dialer.clone(),
                },
                records: records.clone(),
            },
        )
        .expect("a reader host");
        for config in configs.values() {
            assert!(
                host.wait_ready(&config.name, READY),
                "{} passes its start-up checks: {:?}",
                config.name,
                host.log(&config.name)
            );
        }
        let host = Arc::new(host);
        let host_leaf = ca.issue(readers::HOST_NAME, ExtendedKeyUsagePurpose::ServerAuth);
        let tls = LinkTls::new(
            ca.pem().as_bytes(),
            host_leaf.chain_pem.as_bytes(),
            host_leaf.key_pem.as_bytes(),
            CORE_NAME,
        )
        .expect("the link's TLS");
        let link = Link::serve(
            TcpListener::bind("127.0.0.1:0").expect("a loopback port"),
            tls,
            host.clone(),
        )
        .expect("the link");
        let core_leaf = ca.issue(CORE_NAME, ExtendedKeyUsagePurpose::ClientAuth);
        let tls = rutent_core_orch::closing::readers::ReaderTls::new(
            ca.pem().as_bytes(),
            core_leaf.chain_pem.as_bytes(),
            core_leaf.key_pem.as_bytes(),
        )
        .expect("Core's side of the link");
        let mut core = rutent_core_orch::closing::Readers::new(
            tls,
            context.deployment.clone(),
            context.protocol_versions(),
        );
        core.set_host(readers::HOST_SETTING, link.address());
        for public in listed {
            core.list(public);
        }
        Readers {
            link,
            host,
            records,
            connector,
            dialer,
            endpoints,
            listings,
            configs,
            core,
            others: Vec::new(),
            ca_pem: ca.pem(),
            core_chain_pem: core_leaf.chain_pem.clone(),
            core_key_pem: core_leaf.key_pem.clone(),
        }
    }

    /// Where the link listens.
    pub fn address(&self) -> SocketAddr {
        self.link.address()
    }

    /// How many times the reader host started `network`'s reader's process.
    pub fn starts(&self, network: &str) -> usize {
        self.host.starts(&self.configs[network].name)
    }

    /// The reader host's configuration of `network`'s reader.
    pub fn config(&self, network: &str) -> &ReaderConfig {
        &self.configs[network]
    }

    /// Every call the readers asked the host for, as the host recorded it.
    pub fn records(&self) -> Vec<CallRecord> {
        self.records.all()
    }

    /// Core's adapter over the link.
    pub fn core(&self) -> &rutent_core_orch::closing::Readers {
        &self.core
    }

    /// Core's adapter over the link, to point at another of the host's links.
    pub fn core_mut(&mut self) -> &mut rutent_core_orch::closing::Readers {
        &mut self.core
    }

    /// A fresh adapter of Core's over the link, as the deployment would hand Core one: the test CA, Core's client
    /// certificate and the host's address, the listings taken in and not yet checked.
    pub fn core_adapter(&self) -> rutent_core_orch::closing::Readers {
        let tls = rutent_core_orch::closing::readers::ReaderTls::new(
            self.ca_pem.as_bytes(),
            self.core_chain_pem.as_bytes(),
            self.core_key_pem.as_bytes(),
        )
        .expect("Core's side of the link");
        let context = rutent_scenarios::assembly::context();
        let mut core = rutent_core_orch::closing::Readers::new(
            tls,
            context.deployment.clone(),
            context.protocol_versions(),
        );
        core.set_host(readers::HOST_SETTING, self.link.address());
        for (network, listing) in &self.listings {
            core.list(&PublicNetwork {
                network: rutent_contracts::router_api::Id::new(network.as_str()).expect("a network"),
                listing: Some(listing.clone()),
            });
        }
        core
    }

    /// The same host served on another link, under a leaf `ca` issues for `name` with a fresh key: a rotated
    /// certificate, where `ca` is the operator's and `name` the listed one. Where it listens.
    pub fn serve_leaf(&mut self, ca: &TestCa, name: &str) -> &Link {
        let leaf = ca.issue(name, ExtendedKeyUsagePurpose::ServerAuth);
        let tls = LinkTls::new(
            self.ca_pem.as_bytes(),
            leaf.chain_pem.as_bytes(),
            leaf.key_pem.as_bytes(),
            CORE_NAME,
        )
        .expect("the link's TLS");
        let link = Link::serve(
            TcpListener::bind("127.0.0.1:0").expect("a loopback port"),
            tls,
            self.host.clone(),
        )
        .expect("the link");
        self.others.push(link);
        self.others.last().expect("just served")
    }

    /// Asks `network`'s reader `asked` over the link, through Core's own adapter at Core's clock (the host's system
    /// clock, which a deployment hands Core: `rutent_core_orch::snapshot::SystemClock`), whatever its standing: the
    /// question as Core put it, and what Core reads of the answer.
    pub fn ask(&self, network: &str, asked: Asked) -> (Put, Read) {
        self.ask_at(network, asked, &rutent_core_orch::snapshot::SystemClock)
    }

    /// The same, at `clock`.
    pub fn ask_at(
        &self,
        network: &str,
        asked: Asked,
        clock: &dyn rutent_core_orch::snapshot::Clock,
    ) -> (Put, Read) {
        let answered = self.core.question(network, asked, clock);
        let put = self.core.put().pop().expect("a question put");
        let read = match answered {
            Ok(answered) => Read::Answered(answered),
            Err(CoreUnreadable::Reader(why)) => Read::Answered(Answered::Unreadable { why }),
            Err(CoreUnreadable::Refused(mismatches)) => Read::Refused(mismatches),
            Err(other) => Read::Unreadable(other),
        };
        (put, read)
    }
}
