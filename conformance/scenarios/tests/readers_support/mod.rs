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
//! - **Core's side** ([`Readers::ask`]): a question under a request id drawn at random, over an mTLS client holding the
//!   host to the test CA and the listed subject name, its answer's signature verified against the key it names and
//!   then held to the question and to the listing's key and version (`check_reader_answer`), as `T-020-60`'s adapter
//!   will.
//! - **The planted faults** ([`Plan`]): a reader that ties a deposit by destination and amount, a hostile reader
//!   (`crash`, `malformed`, `other-movement`, `undeclared`) and a key the listing doesn't name; an endpoint that lies or
//!   is silent is the world's, planted on its endpoints (`Endpoints::plant`).
//!
//! The jail runs here as in production: on Linux the provider's namespaces and in-process layers, which the host holds
//! the reader to before it serves it; on macOS, for development, the provider's `sandbox-exec` profile. No reader runs
//! unjailed.

#![allow(dead_code)]

use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{BuildHasher, Hasher};
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
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use rutent_conformance::signing::{base64url, verify};
use rutent_contracts::core_internal::{NetworkListing, PublicNetwork};
use rutent_contracts::module_interface::{Host as HostName, Identifier, Name, SecretValue};
use rutent_contracts::provider_role::Statement;
use rutent_contracts::reader::{
    Answered, Asked, Expected, ReaderAnswer, ReaderMismatch, ReaderQuestion, RequestId, TrustRule,
    check_reader_answer,
};
use rutent_contracts::router_api::{Id, ProtocolVersion, Timestamp};
use rutent_prov::egress::{ConnectError, Connector, Dial, Dialer, NameResolver, Request, Response};
use rutent_prov::keys::{ResolveError, Resolver};
use rutent_reader_host::config::{LightClientRule, PortRule, Protocol, ReaderLimits};
use rutent_reader_host::egress::Network;
use rutent_reader_host::link::{Link, LinkTls, read_answer, write_question};
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
        // PKCS #8 v1 of an Ed25519 key (RFC 8410): the fixed prefix, then the 32-byte seed.
        let mut pkcs8 = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ];
        pkcs8.extend_from_slice(&CA_SEED);
        let key = KeyPair::from_pkcs8_der_and_sign_algo(
            &PrivatePkcs8KeyDer::from(pkcs8),
            &rcgen::PKCS_ED25519,
        )
        .expect("the test CA's key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("parameters");
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, "rutent scenarios test CA");
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

/// The simulated reader's program, `examples/sim_reader.rs`, as this tree builds it (#353's review, Low 1):
/// `RUTENT_SIM_READER` if set, else built here, once per test process, by `cargo build --example sim_reader` on this
/// package's own manifest into the target directory this test runs from, and found by the path cargo reports for that
/// build. A run of one test binary alone builds it too, and never runs a reader another tree built into a shared target
/// directory. Panics, naming the command, if it can't be built.
pub fn sim_reader() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            if let Some(path) = std::env::var_os("RUTENT_SIM_READER") {
                return PathBuf::from(path);
            }
            let exe = std::env::current_exe().expect("the test's own path");
            // `<target>/<profile>/deps/<test>`: the reader goes into the same target directory and profile.
            let profile_dir = exe
                .parent()
                .and_then(std::path::Path::parent)
                .expect("the test runs from a profile's directory");
            let target = profile_dir
                .parent()
                .expect("the profile's directory is in a target directory");
            let release = profile_dir.file_name().is_some_and(|p| p == "release");
            let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
            let mut build = std::process::Command::new(
                std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
            );
            build
                .args(["build", "--quiet", "--example", "sim_reader"])
                .args(["--message-format", "json"])
                .arg("--manifest-path")
                .arg(&manifest)
                .arg("--target-dir")
                .arg(target);
            if release {
                build.arg("--release");
            }
            let out = build.output().expect("cargo runs");
            let built = String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .filter(|m| {
                    m["reason"] == "compiler-artifact"
                        && m["target"]["name"] == "sim_reader"
                        && m["target"]["kind"]
                            .as_array()
                            .is_some_and(|k| k.iter().any(|k| k == "example"))
                })
                .find_map(|m| m["executable"].as_str().map(PathBuf::from));
            match built {
                Some(path) if out.status.success() && path.is_file() => path,
                _ => panic!(
                    "the simulated reader wasn't built: `cargo build --example sim_reader --manifest-path {}`: {}",
                    manifest.display(),
                    String::from_utf8_lossy(&out.stderr)
                ),
            }
        })
        .clone()
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
/// peer's endpoint does ([`Endpoints::answer`]); a silent peer answers nothing.
pub struct PeerServer {
    pub address: SocketAddr,
}

impl PeerServer {
    pub fn serve(endpoints: Endpoints, host: String) -> PeerServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("its address");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
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
        PeerServer { address }
    }
}

/// Connects a light client's peer, by the public address the world serves it at, to its loopback server; nothing else
/// connects. Every address dialled is kept.
#[derive(Default)]
pub struct SimDialer {
    peers: BTreeMap<IpAddr, SocketAddr>,
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

/// The faults planted in the readers.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// Every reader ties a deposit by its destination and amount (`Core::ties_by_destination`).
    pub ties: bool,
    /// The hostile acts planted in each network's reader, by network.
    pub hostile: BTreeMap<String, Vec<&'static str>>,
    /// The networks whose reader host signs with a key the listing doesn't name.
    pub unlisted_key: BTreeSet<String>,
}

impl Plan {
    /// `act` planted in every listed network's reader.
    pub fn everywhere(listed: &[PublicNetwork], act: &'static str) -> Self {
        let mut plan = Plan::default();
        for network in listed {
            let name = network.network.as_str().to_owned();
            if act == "unlisted-key" {
                plan.unlisted_key.insert(name);
            } else {
                plan.hostile.insert(name, vec![act]);
            }
        }
        plan
    }
}

/// What Core reads of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// An answer that passed every check: its signature, the question it answers, the listed key and version.
    Answered(ReaderAnswer),
    /// An answer refused before anything in it is read: unreadable.
    Refused(Vec<ReaderMismatch>),
    /// No answer at all: unreadable.
    Nothing,
}

impl Read {
    /// The answer, where Core may read it.
    pub fn answered(&self) -> Option<&Answered> {
        match self {
            Read::Answered(answer) => Some(&answer.answer),
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
    client: Arc<ClientConfig>,
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
                peers.insert(
                    served.address,
                    PeerServer::serve(endpoints.clone(), host).address,
                );
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
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(ca.der()))
            .expect("the test CA as Core's one anchor");
        let client = Arc::new(
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("a TLS version")
                .with_root_certificates(roots)
                .with_client_auth_cert(
                    core_leaf.chain.clone(),
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(core_leaf.key_der.clone())),
                )
                .expect("Core's client certificate"),
        );
        Readers {
            link,
            host,
            records,
            connector,
            dialer,
            endpoints,
            listings,
            configs,
            client,
        }
    }

    /// The reader host's configuration of `network`'s reader.
    pub fn config(&self, network: &str) -> &ReaderConfig {
        &self.configs[network]
    }

    /// Every call the readers asked the host for, as the host recorded it.
    pub fn records(&self) -> Vec<CallRecord> {
        self.records.all()
    }

    /// Asks `network`'s reader `asked` over the link, as Core does: the question, and what Core reads of the answer.
    pub fn ask(&self, network: &str, asked: Asked) -> (ReaderQuestion, Read) {
        let listing = &self.listings[network];
        let question = ReaderQuestion {
            request: request_id(),
            reader: listing.endpoint.reader.clone(),
            network: Id::new(network).expect("a network"),
            deadline: Timestamp::new("2099-01-01T00:00:00Z").expect("a time"),
            asked,
        };
        let bytes = self
            .over_link(&serde_json::to_vec(&question).expect("a question"), listing)
            .unwrap_or_default();
        if bytes.is_empty() {
            return (question, Read::Nothing);
        }
        if verify(&bytes).is_err() {
            return (question, Read::Refused(Vec::new()));
        }
        let Ok(statement) = serde_json::from_slice::<Statement<ReaderAnswer>>(&bytes) else {
            return (question, Read::Refused(Vec::new()));
        };
        let context = rutent_scenarios::assembly::context();
        let versions = context.protocol_versions();
        let expected = Expected {
            deployment: &context.deployment,
            versions: &versions,
            key: &listing.reader.statement_key,
            version: &listing.version,
        };
        let now = Timestamp::new("2026-11-27T10:00:00Z").expect("a time");
        let read = match check_reader_answer(&question, &statement, &expected, &now) {
            Ok(()) => Read::Answered(statement.statement),
            Err(mismatches) => Read::Refused(mismatches),
        };
        (question, read)
    }

    /// One question over the mTLS link, the host held to the test CA and the listed subject name.
    fn over_link(&self, question: &[u8], listing: &NetworkListing) -> io::Result<Vec<u8>> {
        let tcp = TcpStream::connect_timeout(&self.link.address(), Duration::from_secs(5))?;
        tcp.set_read_timeout(Some(Duration::from_secs(60)))?;
        let name = ServerName::try_from(listing.reader.subject_name.as_str().to_owned())
            .map_err(io::Error::other)?;
        let connection =
            ClientConnection::new(self.client.clone(), name).map_err(io::Error::other)?;
        let mut stream = StreamOwned::new(connection, tcp);
        write_question(&mut stream, question)?;
        Ok(read_answer(&mut stream)?.unwrap_or_default())
    }
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
