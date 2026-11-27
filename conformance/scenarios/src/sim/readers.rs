//! The simulated networks' readers (`T-024-25`, `DEC-28`, ADR-0007 as amended): what each simulated public network
//! lists for its reader service, and the endpoints the simulated world serves that reader.
//!
//! Every network a provider's tuples are on (`sim::fleet::networks_of`) is listed by a decided
//! `list_public_network` ([`listing_of`]), naming its reader's endpoint (the host setting [`HOST_SETTING`] and the
//! reader's name), its identity (the harness's test CA by digest, [`CA_SHA256`], the reader host's subject name
//! [`HOST_NAME`] and the reader's statement key), its version and its trust rule. Each network follows one of the two
//! rules the owner set (item `reader-answer-trust`):
//!
//! - **The EVM rule** ([`Rule::RpcPool`]): `polygon` and `bsc`, by their chain ids, read from a pool of three RPC
//!   endpoints ([`pool_of`]) with a quorum of two.
//! - **The Substrate rule** ([`Rule::LightClient`]): every other simulated network, by a genesis hash drawn from its
//!   name ([`identity_of`]), read by a light-client-shaped reader through one peer ([`peer_of`]).
//!
//! The world serves each reader's endpoints ([`Endpoints`]): the pool's endpoints and the peer, each at an address of
//! its own, answering one JSON-RPC request per exchange (a line, for the peer) from the network as the world last
//! published it ([`crate::sim::SimWorld::publish_to_readers`]). The world's faults planted at an endpoint
//! ([`EndpointFault`]): one that lies, each in a lie of its own, and one that is silent. The readers themselves, and
//! the faults planted in them, are the test harness's (`conformance/scenarios/examples/sim_reader.rs`,
//! `conformance/scenarios/tests/readers_support/`): the reader host is a dev-dependency of this crate, for its tests
//! only (`DEC-28`).
//!
//! `Readings`' `impl PublicRecords` stays Core's path until `T-020-60` puts Core on the adapter: these readers answer
//! as it does over the same world, which `simulated reader answers as its simulated network does` holds.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use rutent_contracts::core_internal::{
    NetworkListing, PublicNetwork, ReaderEndpoint, ReaderIdentity, SubjectName,
};
use rutent_contracts::module_interface::{Digest, DigestAlgorithm, DigestValue};
use rutent_contracts::provider_role::{NoDetail, PublicKey};
use rutent_contracts::reader::{
    ChainId, Depth, Finality, GenesisHash, Movement, NetworkIdentity, ReorgRule, RpcEndpoint,
    Standing, TrustRule,
};
use rutent_contracts::router_api::{Amount, Id, Version};
use rutent_evidence::{MovementId, PublicNetwork as _, SimulatedNetwork, Status};
use serde_json::{Value, json};

/// The simulated readers' version, which every listing names and every answer carries.
pub const READER_VERSION: &str = "0.1.0";

/// The deployment's host setting that holds the simulated reader host's address, which every listing names.
pub const HOST_SETTING: &str = "sim-reader-host";

/// The subject name the simulated reader host's server certificate carries, which every listing names.
pub const HOST_NAME: &str = "readers.rutent-scenarios.example";

/// The subject name the test CA issues Core's client certificate under, which the reader host takes connections from.
pub const CORE_NAME: &str = "core.rutent-scenarios.example";

/// The SHA-256 digest of the test CA's certificate (DER) the harness makes (`readers_support::TestCa`): deterministic,
/// from a fixed key and fixed fields, so the assembly lists it without making it. The harness's test
/// `harness ca is the one the listings name` holds the CA it makes to this digest.
pub const CA_SHA256: &str = "8d33b2efb51f7f1b85d8e0137ddb896971c60afb8edb295df3216baef3abc339";

/// How many endpoints of an EVM network's pool must agree.
pub const QUORUM: u32 = 2;

/// The labels of an EVM network's three pool endpoints.
pub const POOL: [&str; 3] = ["rpc-a", "rpc-b", "rpc-c"];

/// The port a light client's peer is reached on, over the network's own p2p transport.
pub const PEER_PORT: u16 = 30333;

/// How a simulated network's reader stands behind what it answers (item `reader-answer-trust`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// A Substrate network's: a light client, by the network's genesis hash.
    LightClient,
    /// An EVM network's: a pool of three RPC endpoints, a quorum of two, by the network's chain id.
    RpcPool,
}

/// The EVM chains among the simulated networks, with their chain ids.
const EVM: [(&str, u64); 2] = [("polygon", 137), ("bsc", 56)];

/// The rule `network` follows: the EVM rule for the EVM chains among the simulated networks, the Substrate rule for
/// every other.
pub fn rule_of(network: &str) -> Rule {
    if chain_id_of(network).is_some() {
        Rule::RpcPool
    } else {
        Rule::LightClient
    }
}

/// An EVM network's chain id.
pub fn chain_id_of(network: &str) -> Option<u64> {
    EVM.iter().find(|(n, _)| *n == network).map(|(_, id)| *id)
}

/// The reader's name on its host: the maker of its answers.
pub fn reader_name(network: &str) -> String {
    format!("{network}-reader")
}

/// A Substrate network's genesis hash: drawn from its name, so each simulated network has its own.
pub fn genesis_of(network: &str) -> String {
    format!(
        "0x{}",
        rutent_prov::module::wire::sha256(format!("rutent-sim-genesis/{network}").as_bytes())
    )
}

/// The network's identity, which its reader checks before it answers anything.
///
/// # Panics
///
/// Never for a simulated network: a chain id and a drawn genesis hash are always well formed.
pub fn identity_of(network: &str) -> NetworkIdentity {
    match chain_id_of(network) {
        Some(id) => NetworkIdentity::ChainId {
            id: ChainId::new(id.to_string()).expect("a chain id"),
        },
        None => NetworkIdentity::GenesisHash {
            hash: GenesisHash::new(genesis_of(network)).expect("a genesis hash"),
        },
    }
}

/// The host of one endpoint of `network`'s pool.
pub fn pool_host(network: &str, label: &str) -> String {
    format!("{label}.{network}.chain.example")
}

/// An EVM network's pool, as its trust rule names it: three `https` endpoints, by name.
pub fn pool_of(network: &str) -> Vec<String> {
    POOL.iter()
        .map(|label| format!("https://{}", pool_host(network, label)))
        .collect()
}

/// The host of a Substrate network's peer, which its light client opens a connection to.
pub fn peer_of(network: &str) -> String {
    format!("peer.{network}.chain.example")
}

/// The reader's trust rule.
///
/// # Panics
///
/// Never for a simulated network, whose pool's endpoints are host names.
pub fn trust_of(network: &str) -> TrustRule {
    match rule_of(network) {
        Rule::LightClient => TrustRule::LightClient(NoDetail {}),
        Rule::RpcPool => TrustRule::RpcPool {
            endpoints: pool_of(network)
                .into_iter()
                .map(|e| RpcEndpoint::new(e).expect("an endpoint"))
                .collect(),
            quorum: QUORUM,
        },
    }
}

/// The seed of a reader's statement key, from its name: the same on every machine, as every simulated key is. The
/// harness's key store holds its private half; the reader's process never does.
pub fn statement_seed(reader: &str) -> [u8; 32] {
    let digest =
        rutent_prov::module::wire::sha256(format!("rutent-sim-reader-key/{reader}").as_bytes());
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&digest[2 * i..2 * i + 2], 16).unwrap_or(0);
    }
    seed
}

/// The reader's listed statement key: the public half of [`statement_seed`].
pub fn statement_key(reader: &str) -> PublicKey {
    rutent_conformance::signing::public_key(&ed25519_dalek::SigningKey::from_bytes(
        &statement_seed(reader),
    ))
}

/// What lists `network` (`DEC-28`): its identity, its reader's endpoint, identity, version and trust rule.
///
/// # Panics
///
/// Never for a simulated network, whose names are identifiers.
pub fn listing_of(network: &str) -> PublicNetwork {
    let reader = reader_name(network);
    PublicNetwork {
        network: Id::new(network).expect("a network"),
        listing: Some(NetworkListing {
            identity: identity_of(network),
            endpoint: ReaderEndpoint {
                host_setting: Id::new(HOST_SETTING).expect("a setting"),
                reader: Id::new(reader.as_str()).expect("a reader"),
            },
            reader: ReaderIdentity {
                ca: Digest {
                    algorithm: DigestAlgorithm::Sha256,
                    value: DigestValue::new(CA_SHA256).expect("a digest"),
                },
                subject_name: SubjectName::new(HOST_NAME).expect("a subject name"),
                statement_key: statement_key(&reader),
            },
            version: Version::new(READER_VERSION).expect("a version"),
            trust: trust_of(network),
        }),
    }
}

/// A network's finality and reorg rules as the reader contract states them. A count of zero is the rule it amounts
/// to: final at once, never undone.
pub fn rules_of(rules: rutent_evidence::NetworkRules) -> (Finality, ReorgRule) {
    let finality = match rules.finality {
        rutent_evidence::Finality::Instant => Finality::Instant(NoDetail {}),
        rutent_evidence::Finality::Confirmations(n) => match Depth::new(n) {
            Some(count) => Finality::Confirmations { count },
            None => Finality::Instant(NoDetail {}),
        },
        rutent_evidence::Finality::Checkpoint => Finality::Checkpoint(NoDetail {}),
    };
    let reorg = match rules.reorg {
        rutent_evidence::ReorgRule::Never => ReorgRule::Never(NoDetail {}),
        rutent_evidence::ReorgRule::DepthBound(d) => match Depth::new(d) {
            Some(depth) => ReorgRule::DepthBound { depth },
            None => ReorgRule::Never(NoDetail {}),
        },
        rutent_evidence::ReorgRule::UntilFinal => ReorgRule::UntilFinal(NoDetail {}),
    };
    (finality, reorg)
}

/// Where a movement stands, as the reader contract states it: `None` where the network's record of it can't be
/// stated (an identifier or an amount the contract refuses), which a reader answers as unreadable.
pub fn standing_of(status: &Status) -> Option<Standing> {
    let movement = |m: &rutent_evidence::Movement| {
        Some(Movement {
            destination: Id::new(m.destination.as_str())?,
            asset: Id::new(m.asset.as_str())?,
            amount: Amount::new(m.amount.as_str())?,
        })
    };
    Some(match status {
        Status::Unknown => Standing::Unknown(NoDetail {}),
        Status::Pending(m) => Standing::Pending(movement(m)?),
        Status::Final(m) => Standing::Final(movement(m)?),
        Status::ReorgedOut => Standing::ReorgedOut(NoDetail {}),
    })
}

/// A fault the world plants at one of a reader's endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointFault {
    /// It answers every movement wrongly, each lying endpoint in a lie of its own (its host in the destination), and
    /// tells the truth about the network's identity and rules: what an RPC node that lies about the movements a pool
    /// asks of it does.
    Lies,
    /// It never answers.
    Silent,
}

/// What one endpoint the world serves is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    /// The network it serves.
    pub network: String,
    /// Its address.
    pub address: IpAddr,
    /// Whether it is a light client's peer, rather than an endpoint of a pool.
    pub peer: bool,
}

#[derive(Debug, Default)]
struct Snapshot {
    /// Each endpoint the world serves a reader, by host.
    endpoints: BTreeMap<String, Served>,
    /// Each network as the world last published it.
    networks: BTreeMap<String, SimulatedNetwork>,
    /// Every movement recorded on each network, in the order first delivered, with where it went and how much: what a
    /// reader that ties a deposit by its destination and amount reads.
    seen: BTreeMap<String, Vec<(MovementId, String, String)>>,
    /// The faults planted at endpoints, by host.
    faults: BTreeMap<String, EndpointFault>,
}

/// The endpoints the simulated world serves the readers, and the networks they answer from, as the world last
/// published them: shared, so the reader host's network, on threads of its own, reads what the world holds.
#[derive(Debug, Clone, Default)]
pub struct Endpoints {
    inner: Arc<Mutex<Snapshot>>,
}

impl Endpoints {
    fn with<R>(&self, f: impl FnOnce(&mut Snapshot) -> R) -> R {
        let mut snapshot = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut snapshot)
    }

    /// Serves `network`'s reader its endpoints: its pool's three, or its peer, each at an address of its own, in a
    /// range of the readers' own (`185.12.71.0/24`, public), apart from the providers' hosts.
    pub(crate) fn serve(&self, network: &str) {
        self.with(|s| {
            let hosts: Vec<(String, bool)> = match rule_of(network) {
                Rule::RpcPool => POOL
                    .iter()
                    .map(|label| (pool_host(network, label), false))
                    .collect(),
                Rule::LightClient => vec![(peer_of(network), true)],
            };
            for (host, peer) in hosts {
                let next = u8::try_from(s.endpoints.len() + 1).expect("fewer than 250 endpoints");
                s.endpoints.entry(host).or_insert(Served {
                    network: network.to_owned(),
                    address: IpAddr::V4(Ipv4Addr::new(185, 12, 71, next)),
                    peer,
                });
            }
        });
    }

    /// Takes in `networks` and the movements delivered to them, in order: what the readers answer from from now on.
    pub(crate) fn publish(
        &self,
        networks: &BTreeMap<String, SimulatedNetwork>,
        delivered: &[rutent_conformance::sim::Movement],
    ) {
        let mut seen: BTreeMap<String, Vec<(MovementId, String, String)>> = BTreeMap::new();
        for m in delivered {
            let Some(id) = MovementId::new(m.id.clone()) else {
                continue;
            };
            let list = seen.entry(m.network.clone()).or_default();
            if !list.iter().any(|(i, ..)| *i == id) {
                list.push((id, m.destination.clone(), m.amount.clone()));
            }
        }
        self.with(|s| {
            s.networks = networks.clone();
            s.seen = seen;
        });
    }

    /// Plants `fault` at the endpoint `host`, or clears it.
    pub fn plant(&self, host: &str, fault: Option<EndpointFault>) {
        self.with(|s| match fault {
            Some(fault) => {
                s.faults.insert(host.to_owned(), fault);
            }
            None => {
                s.faults.remove(host);
            }
        });
    }

    /// The endpoint the world serves at `host`, if it serves one there.
    pub fn endpoint(&self, host: &str) -> Option<Served> {
        self.with(|s| s.endpoints.get(host).cloned())
    }

    /// The host the world serves an endpoint at `address` under, if any.
    pub fn host_at(&self, address: IpAddr) -> Option<String> {
        self.with(|s| {
            s.endpoints
                .iter()
                .find(|(_, e)| e.address == address)
                .map(|(host, _)| host.clone())
        })
    }

    /// Every endpoint the world serves, by host.
    pub fn all(&self) -> BTreeMap<String, Served> {
        self.with(|s| s.endpoints.clone())
    }

    /// What the endpoint `host` answers to one JSON-RPC request, `body`: the response's text, or `None` where it gives
    /// none (a silent endpoint, or no endpoint at all). Its methods: `eth_chainId` (an EVM network's) or
    /// `rutent_genesis` (a Substrate network's), `rutent_rules` (the finality and reorg rules), `rutent_movementStatus`
    /// (`[id]`: where a movement stands) and `rutent_movementsTo` (`[destination, amount]`: the movements to that
    /// destination of that amount, in the order the network first recorded them).
    pub fn answer(&self, host: &str, body: &str) -> Option<String> {
        self.with(|s| {
            let served = s.endpoints.get(host)?;
            let fault = s.faults.get(host).copied();
            if fault == Some(EndpointFault::Silent) {
                return None;
            }
            let lies = fault == Some(EndpointFault::Lies);
            let request: Value = serde_json::from_str(body).ok()?;
            let id = request["id"].clone();
            let network = s.networks.get(&served.network);
            let param = |n: usize| request["params"][n].as_str().map(str::to_owned);
            let result = match request["method"].as_str()? {
                "eth_chainId" if !served.peer => chain_id_of(&served.network)
                    .map_or(Value::Null, |id| Value::String(format!("0x{id:x}"))),
                "rutent_genesis" if served.peer => Value::String(genesis_of(&served.network)),
                "rutent_rules" => match network {
                    Some(net) => {
                        let (finality, reorg) = rules_of(net.rules());
                        json!({"finality": finality, "reorg": reorg})
                    }
                    None => Value::Null,
                },
                "rutent_movementStatus" => {
                    let status = match (network, param(0).and_then(MovementId::new)) {
                        (Some(net), Some(movement)) => net.status(&movement),
                        _ => Status::Unknown,
                    };
                    let status = match status {
                        Status::Pending(mut m) if lies => {
                            m.destination = format!("{}-lie-{}", m.destination, label_of(host));
                            Status::Pending(m)
                        }
                        Status::Final(mut m) if lies => {
                            m.destination = format!("{}-lie-{}", m.destination, label_of(host));
                            Status::Final(m)
                        }
                        status => status,
                    };
                    standing_of(&status)
                        .and_then(|standing| serde_json::to_value(standing).ok())
                        .unwrap_or(Value::Null)
                }
                "rutent_movementsTo" => {
                    let (to, amount) = (param(0)?, param(1)?);
                    let ids: Vec<String> = if lies {
                        vec![format!("{}-lie", label_of(host))]
                    } else {
                        s.seen
                            .get(&served.network)
                            .map(|seen| {
                                seen.iter()
                                    .filter(|(_, d, a)| *d == to && *a == amount)
                                    .map(|(id, ..)| id.as_str().to_owned())
                                    .collect()
                            })
                            .unwrap_or_default()
                    };
                    json!(ids)
                }
                _ => Value::Null,
            };
            Some(json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string())
        })
    }
}

/// The first label of a host: what tells one endpoint of a pool from another.
fn label_of(host: &str) -> &str {
    host.split('.').next().unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: DEC-28
    #[test]
    fn each_simulated_network_follows_one_rule_and_its_listing_holds_to_the_contract() {
        for network in ["polygon", "bsc", "sim-reserve-a-bank", "sim-join-chain"] {
            let listed = listing_of(network);
            let listing = listed.listing.as_ref().expect("a listing");
            rutent_contracts::reader::check_pairing(&listing.identity, &listing.trust)
                .expect("its identity and its trust rule are paired");
            listing.trust.check().expect("its trust rule holds");
            assert_eq!(
                matches!(listing.trust, TrustRule::RpcPool { .. }),
                rule_of(network) == Rule::RpcPool
            );
        }
        assert_eq!(rule_of("polygon"), Rule::RpcPool);
        assert_eq!(rule_of("sim-reserve-a-bank"), Rule::LightClient);
        assert_ne!(genesis_of("a-bank"), genesis_of("b-bank"));
    }
}
