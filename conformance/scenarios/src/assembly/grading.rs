//! What the assembly's grading reads of the simulated world (`T-020-09`): each part runs under
//! `closing::Evidenced`, over the simulated public networks the providers' movements are recorded on and the reserves'
//! books, as they stood when Core last looked: at the start of each run and after each operation carried out. The
//! world lives inside the platform, which the segments hold as their modules, so the readings are a copy, refreshed.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_conformance::sim::{Entry as BookEntry, Spec, recorded_segments, transaction_of};
use rutent_contracts::router_api::Id;
use rutent_core_orch::closing::{Books, Moved, PublicRecords};
use rutent_evidence::replay::RecordedSegment;
use rutent_evidence::{MovementId, NetworkRules, PublicNetwork, SimulatedNetwork, Status};

use crate::sim::{Installed, SimWorld};

#[derive(Default)]
struct Read {
    networks: BTreeMap<String, SimulatedNetwork>,
    books: BTreeMap<String, Vec<BookEntry>>,
    /// Every movement recorded on each network, in the order first read, with where it went and how much: what a
    /// network that ties a deposit by its destination and amount reads.
    seen: BTreeMap<String, Vec<(MovementId, String, String)>>,
    /// The planted fault: a deposit tied to the first movement to its destination of its amount.
    ties: bool,
}

/// The simulated networks and the reserves' books, as Core last read them.
#[derive(Clone, Default)]
pub(crate) struct Readings {
    read: Rc<RefCell<Read>>,
    /// Each network a provider's tuples are on.
    networks: Rc<Vec<String>>,
    /// Each provider that moves value, whose journal is its book and whose private node, where it has one, its
    /// publisher records the segments it serves in.
    movers: Rc<Vec<Spec>>,
}

impl Readings {
    /// Readings of the networks and books of `installed`.
    pub(crate) fn of(installed: &[Installed]) -> Self {
        let mut networks = Vec::new();
        let mut movers = Vec::new();
        for one in installed {
            // The networks the fleet serves for the provider's tuples, as it serves them.
            networks.extend(crate::sim::fleet::networks_of(&one.spec));
            if one.spec.role.moves_value() {
                movers.push(one.spec.clone());
            }
        }
        networks.sort();
        networks.dedup();
        Readings {
            read: Rc::default(),
            networks: Rc::new(networks),
            movers: Rc::new(movers),
        }
    }

    /// From now on, a movement is tied to a segment by its destination and amount, the first such movement on the
    /// network, rather than by its transaction: as a reserve paying two orders with one payment would have it.
    pub(crate) fn tie_by_destination(&self) {
        self.read.borrow_mut().ties = true;
    }

    /// Whether the planted fault ties movements by destination and amount.
    pub(crate) fn ties(&self) -> bool {
        self.read.borrow().ties
    }

    /// Reads `world` again: every network and book as it stands now.
    pub(crate) fn refresh(&self, world: &SimWorld) {
        let mut read = self.read.borrow_mut();
        for network in self.networks.iter() {
            if let Some(net) = world.network(network) {
                read.networks.insert(network.clone(), net.clone());
            }
        }
        let recorded: Vec<rutent_conformance::sim::Movement> = world
            .delivered()
            .iter()
            .filter_map(|d| serde_json::from_str(&d.body).ok())
            .collect();
        for m in recorded {
            let Some(id) = MovementId::new(m.id.clone()) else {
                continue;
            };
            let seen = read.seen.entry(m.network.clone()).or_default();
            if !seen.iter().any(|(i, ..)| *i == id) {
                seen.push((id, m.destination, m.amount));
            }
        }
        for mover in self.movers.iter() {
            read.books.insert(
                mover.provider.clone(),
                world.journal(&mover.provider).to_vec(),
            );
        }
    }
}

impl Readings {
    fn status_of(&self, read: &Read, network: &str, id: &MovementId) -> Status {
        read.networks
            .get(network)
            .map_or(Status::Unknown, |net| net.status(id))
    }
}

/// Each simulated network's movements; a movement is tied to a segment by its transaction, which the simulated
/// providers give it as its identifier (`rutent_conformance::sim::movements_of`), and goes where the network records.
impl PublicRecords for Readings {
    fn rules(&self, network: &str) -> Option<NetworkRules> {
        self.read
            .borrow()
            .networks
            .get(network)
            .map(PublicNetwork::rules)
    }

    fn status(&self, network: &str, id: &MovementId) -> Status {
        self.status_of(&self.read.borrow(), network, id)
    }

    fn movement(&self, network: &str, transaction: &Id) -> Option<Moved> {
        let id = MovementId::new(transaction.as_str())?;
        let tied = {
            let read = self.read.borrow();
            match (read.ties, self.status_of(&read, network, &id)) {
                (true, Status::Pending(m) | Status::Final(m)) => read
                    .seen
                    .get(network)
                    .and_then(|seen| {
                        seen.iter()
                            .find(|(_, to, amount)| *to == m.destination && *amount == m.amount)
                    })
                    .map(|(first, ..)| first.clone()),
                _ => None,
            }
        };
        let id = tied.unwrap_or(id);
        match self.status(network, &id) {
            Status::Pending(m) | Status::Final(m) => Some(Moved {
                id,
                destination: m.destination,
            }),
            Status::Unknown | Status::ReorgedOut => None,
        }
    }
}

/// Each private node, as its publisher records the segments inside it (`REQ-ORD-41`): the entries of the books of every
/// provider that serves in it, for the transactions asked, in the order the transactions are asked, each read as
/// `rutent_conformance::sim::recorded_segments` reads a book.
impl Books for Readings {
    fn recorded(&self, node: &str, transactions: &[Id]) -> Option<Vec<RecordedSegment>> {
        let read = self.read.borrow();
        let context = crate::assembly::context();
        let mut segments = Vec::new();
        for transaction in transactions {
            for spec in self.movers.iter() {
                if !spec.private_nodes().iter().any(|n| n == node) {
                    continue;
                }
                let Some(book) = read.books.get(&spec.provider) else {
                    continue;
                };
                let entries: Vec<BookEntry> = book
                    .iter()
                    .filter(|e| {
                        transaction_of(&e.operation.statement)
                            .is_some_and(|tx| tx == transaction.as_str())
                    })
                    .cloned()
                    .collect();
                segments.extend(
                    recorded_segments(spec, &context, &entries)
                        .into_iter()
                        .map(|s| RecordedSegment {
                            asset_in: s.asset_in,
                            amount_in: s.amount_in,
                            asset_out: s.asset_out,
                            amount_out: s.amount_out,
                            fee: s.fee,
                        }),
                );
            }
        }
        (!segments.is_empty()).then_some(segments)
    }
}
