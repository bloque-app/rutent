//! The simulated world the providers' modules run in: the platform's shared state, scripts and journals among it, the
//! simulated public networks and the rails the providers write their books to.

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr};

use rutent_conformance::signing::{Ed25519Key, base64url};
use rutent_conformance::sim::{
    BOOK_KEY, BOOK_LENGTH_KEY, Behaviour, Entry, Movement, SCRIPT_PREFIX, chain_host, rail_host,
};
use rutent_contracts::module_interface::Identifier;
use rutent_contracts::module_interface::{SecretRefusalReason, SecretValue, Time};
use rutent_evidence::{MovementId, NetworkRules, SimulatedNetwork};
use rutent_prov::catalogue::Instant;
use rutent_prov::module::platform::{ClockError, World};
use rutent_prov::module::types::Endpoint;
use rutent_prov::proxy::Caller;

/// The world's clock until a scenario sets it: 2026-11-07T00:00:00Z.
pub const WORLD_AT: Instant = Instant(1_794_009_600);

/// One behaviour waiting in a script.
#[derive(Debug, Clone)]
struct Queued {
    /// The kind of operation it is for, or any real one.
    on: Option<String>,
    /// Whether it is for a dry run.
    dry_run: bool,
    behaviour: Behaviour,
}

/// What a provider's script holds: behaviours in the order they are taken, and what answers once they are used up.
#[derive(Debug, Clone, Default)]
struct Script {
    queue: Vec<Queued>,
    otherwise: Behaviour,
}

/// An outbound request a rail or a network received, as the platform's egress layer delivered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    /// The host it was sent to.
    pub host: String,
    /// The body.
    pub body: String,
}

/// The world a platform runs simulated providers in (`World`).
///
/// It holds, per provider, a [`script`](SimWorld::script) the module reads its next behaviour from and a journal
/// the module writes its books to, through its rail, and reads them back from; and a simulated public network each
/// movement is recorded on. Every one of these is the platform's shared state, which a module reaches only through
/// the interfaces the platform serves, so what a module does is recorded by the platform and nothing of it escapes
/// the record (`NFR-5`).
#[derive(Default)]
pub struct SimWorld {
    scripts: BTreeMap<String, Script>,
    journals: BTreeMap<String, Vec<Entry>>,
    networks: BTreeMap<String, SimulatedNetwork>,
    addresses: BTreeMap<String, IpAddr>,
    rails: BTreeMap<String, String>,
    chains: BTreeMap<String, String>,
    statement_keys: BTreeMap<String, String>,
    delivered: Vec<Delivered>,
    /// Movements the networks refused to record, and why.
    pub refused_movements: Vec<(Movement, String)>,
    /// The state keys the platform asked this world for, in order.
    pub state_asked: Vec<String>,
    seeded: HashMap<(String, String), String>,
    clock: Option<Instant>,
    keys_down: Vec<String>,
}

impl SimWorld {
    /// A world with nothing in it.
    pub fn new() -> Self {
        Self::default()
    }

    /// Serves `host` at the next address, and gives it.
    fn serve(&mut self, host: &str) -> IpAddr {
        let next = u8::try_from(self.addresses.len() + 1).expect("fewer than 250 hosts");
        *self
            .addresses
            .entry(host.to_owned())
            .or_insert(IpAddr::V4(Ipv4Addr::new(185, 12, 70, next)))
    }

    /// Serves `provider`'s rail: the backend its module writes its books to.
    pub fn serve_rail(&mut self, provider: &str) {
        let host = rail_host(provider);
        self.serve(&host);
        self.rails.insert(host, provider.to_owned());
        self.journals.entry(provider.to_owned()).or_default();
    }

    /// Serves a simulated public network, `network`, with `rules` for its finality and its reorgs (`T-015-02`).
    pub fn serve_network(&mut self, network: &str, rules: NetworkRules) {
        let host = chain_host(network);
        self.serve(&host);
        self.chains.insert(host, network.to_owned());
        self.networks
            .insert(network.to_owned(), SimulatedNetwork::new(network, rules));
    }

    /// A simulated public network.
    pub fn network(&self, network: &str) -> Option<&SimulatedNetwork> {
        self.networks.get(network)
    }

    /// A simulated public network, to drive: blocks, reorgs and checkpoints are the scenario's to cause.
    pub fn network_mut(&mut self, network: &str) -> Option<&mut SimulatedNetwork> {
        self.networks.get_mut(network)
    }

    /// Queues `behaviour` for `provider`'s next operation that isn't a dry run and names a transaction, of whatever
    /// kind: a withdrawal, deposit, transfer, conversion or reversal, and also a balance verification or a
    /// reversal confirmation, which move nothing but ask the script all the same. A judgement names no
    /// transaction and never does. To keep a behaviour for one kind, queue it with
    /// [`SimWorld::script_for`].
    pub fn script(&mut self, provider: &str, behaviour: Behaviour) {
        self.queue(provider, None, false, behaviour);
    }

    /// Queues `behaviour` for `provider`'s next real operation of `kind`.
    pub fn script_for(&mut self, provider: &str, kind: &str, behaviour: Behaviour) {
        self.queue(provider, Some(kind), false, behaviour);
    }

    /// Queues `behaviour` for `provider`'s next dry run of `kind`.
    pub fn script_dry_run(&mut self, provider: &str, kind: &str, behaviour: Behaviour) {
        self.queue(provider, Some(kind), true, behaviour);
    }

    fn queue(&mut self, provider: &str, on: Option<&str>, dry_run: bool, behaviour: Behaviour) {
        self.scripts
            .entry(provider.to_owned())
            .or_default()
            .queue
            .push(Queued {
                on: on.map(str::to_owned),
                dry_run,
                behaviour,
            });
    }

    /// What `provider` does once its queued behaviours are used up. Without it, it answers as its role asks.
    pub fn otherwise(&mut self, provider: &str, behaviour: Behaviour) {
        self.scripts
            .entry(provider.to_owned())
            .or_default()
            .otherwise = behaviour;
    }

    /// How many behaviours are still queued for `provider`.
    pub fn queued(&self, provider: &str) -> usize {
        self.scripts.get(provider).map_or(0, |s| s.queue.len())
    }

    /// `provider`'s journal: its books, as its rail holds them.
    pub fn journal(&self, provider: &str) -> &[Entry] {
        self.journals.get(provider).map_or(&[], Vec::as_slice)
    }

    /// Every request the rails and the networks received, in order.
    pub fn delivered(&self) -> &[Delivered] {
        &self.delivered
    }

    /// Seeds one entry of `provider`'s own state, which its outside part may read.
    pub fn seed_state(&mut self, provider: &str, key: &str, value: &str) {
        self.seeded
            .insert((provider.to_owned(), key.to_owned()), value.to_owned());
    }

    /// Sets the clock the platform's records take their times from.
    pub fn set_clock(&mut self, at: Instant) {
        self.clock = Some(at);
    }

    /// Holds `key` as `provider`'s statement key.
    pub fn hold_statement_key(&mut self, provider: &str, key: &Ed25519Key) {
        self.statement_keys
            .insert(provider.to_owned(), base64url(key.as_bytes()));
    }

    /// Takes down the store holding `provider`'s statement key.
    pub fn make_key_store_unavailable(&mut self, provider: &str) {
        self.keys_down.push(provider.to_owned());
    }

    fn next_behaviour(&mut self, provider: &str, kind: &str, dry_run: bool) -> Option<Behaviour> {
        let script = self.scripts.get_mut(provider)?;
        let at = script
            .queue
            .iter()
            .position(|q| q.dry_run == dry_run && q.on.as_deref().is_none_or(|on| on == kind));
        match at {
            Some(at) => Some(script.queue.remove(at).behaviour),
            None if script.otherwise != Behaviour::default() && !dry_run => {
                Some(script.otherwise.clone())
            }
            None => None,
        }
    }
}

impl World for SimWorld {
    fn secret(&mut self, _: &str, _: &str) -> Option<String> {
        None
    }

    fn state(&mut self, provider: &str, key: &str) -> Option<String> {
        self.state_asked.push(key.to_owned());
        if key == BOOK_KEY {
            return Some(serde_json::to_string(self.journal(provider)).expect("a journal"));
        }
        if key == BOOK_LENGTH_KEY {
            return Some(self.journal(provider).len().to_string());
        }
        if let Some(rest) = key
            .strip_prefix(SCRIPT_PREFIX)
            .and_then(|r| r.strip_prefix('/'))
        {
            let (kind, mode) = rest.split_once('/')?;
            let behaviour = self.next_behaviour(provider, kind, mode == "dry")?;
            return Some(serde_json::to_string(&behaviour).expect("a behaviour"));
        }
        self.seeded
            .get(&(provider.to_owned(), key.to_owned()))
            .cloned()
    }

    fn route(&mut self, endpoint: &Endpoint) -> Option<(IpAddr, Option<Endpoint>)> {
        self.addresses
            .get(&endpoint.host)
            .map(|address| (*address, None))
    }

    fn deliver(&mut self, _: &Endpoint, _: &str) {}

    fn receive(&mut self, endpoint: &Endpoint, request: &rutent_prov::egress::Request) {
        let body = String::from_utf8_lossy(request.body()).into_owned();
        self.delivered.push(Delivered {
            host: endpoint.host.clone(),
            body: body.clone(),
        });
        if let Some(provider) = self.rails.get(&endpoint.host)
            && let Ok(entry) = serde_json::from_str::<Entry>(&body)
        {
            self.journals
                .entry(provider.clone())
                .or_default()
                .push(entry);
        } else if let Some(network) = self.chains.get(&endpoint.host)
            && let Ok(movement) = serde_json::from_str::<Movement>(&body)
        {
            let recorded = MovementId::new(movement.id.clone()).map(|id| {
                self.networks.get_mut(network).map(|net| {
                    net.record(
                        id,
                        rutent_evidence::Movement {
                            destination: movement.destination.clone(),
                            asset: movement.asset.clone(),
                            amount: movement.amount.clone(),
                        },
                    )
                })
            });
            if let Some(Some(Err(why))) = recorded {
                self.refused_movements.push((movement, format!("{why:?}")));
            }
        }
    }

    fn now(&mut self) -> Result<Time, ClockError> {
        self.clock
            .unwrap_or(WORLD_AT)
            .rfc3339()
            .and_then(Time::new)
            .ok_or(ClockError::PastYear9999)
    }

    fn statement_key(
        &mut self,
        caller: &Caller,
        _: &Identifier,
    ) -> Result<SecretValue, SecretRefusalReason> {
        let provider = caller.provider().as_str();
        if self.keys_down.iter().any(|p| p == provider) {
            return Err(SecretRefusalReason::StoreUnavailable);
        }
        self.statement_keys
            .get(provider)
            .cloned()
            .and_then(SecretValue::new)
            .ok_or(SecretRefusalReason::NotFound)
    }
}
