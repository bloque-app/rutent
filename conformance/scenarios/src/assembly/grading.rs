//! What the assembly's grading reads of the simulated world (`T-020-09`): each part runs under
//! `closing::Evidenced`, over the simulated public networks the providers' movements are recorded on and the reserves'
//! books, as they stood when Core last looked: at the start of each run and after each operation carried out. The
//! world lives inside the platform, which the segments hold as their modules, so the readings are a copy, refreshed.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::sim::{Entry as BookEntry, Role, Spec, recorded_segments, transaction_of};
use rutent_contracts::router_api::Id;
use rutent_core_orch::closing::{Books, Moved, PublicRecords};
use rutent_evidence::replay::RecordedSegment;
use rutent_evidence::{MovementId, NetworkRules, PublicNetwork, SimulatedNetwork, Status};

use crate::sim::{Installed, SimWorld};

#[derive(Default)]
struct Read {
    networks: BTreeMap<String, SimulatedNetwork>,
    books: BTreeMap<String, Vec<BookEntry>>,
}

/// The simulated networks and the reserves' books, as Core last read them.
#[derive(Clone, Default)]
pub(crate) struct Readings {
    read: Rc<RefCell<Read>>,
    /// Each network a provider's tuples are on.
    networks: Rc<Vec<String>>,
    /// Each reserve, whose private ledger is its book.
    reserves: Rc<Vec<String>>,
}

impl Readings {
    /// Readings of the networks and books of `installed`.
    pub(crate) fn of(installed: &[Installed]) -> Self {
        let mut networks = Vec::new();
        let mut reserves = Vec::new();
        for one in installed {
            match one.spec.role {
                Role::Reserve => {
                    networks.push(one.spec.bank());
                    reserves.push(one.spec.provider.clone());
                }
                Role::Exchange => networks.push("polygon".to_owned()),
                Role::Ramp => networks.push("bsc".to_owned()),
                Role::Verifier => {}
            }
        }
        networks.sort();
        networks.dedup();
        Readings {
            read: Rc::default(),
            networks: Rc::new(networks),
            reserves: Rc::new(reserves),
        }
    }

    /// Reads `world` again: every network and book as it stands now.
    pub(crate) fn refresh(&self, world: &SimWorld) {
        let mut read = self.read.borrow_mut();
        for network in self.networks.iter() {
            if let Some(net) = world.network(network) {
                read.networks.insert(network.clone(), net.clone());
            }
        }
        for reserve in self.reserves.iter() {
            read.books
                .insert(reserve.clone(), world.journal(reserve).to_vec());
        }
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
        self.read
            .borrow()
            .networks
            .get(network)
            .map_or(Status::Unknown, |net| net.status(id))
    }

    fn movement(&self, network: &str, transaction: &Id) -> Option<Moved> {
        let id = MovementId::new(transaction.as_str())?;
        match self.status(network, &id) {
            Status::Pending(m) | Status::Final(m) => Some(Moved {
                id,
                destination: m.destination,
            }),
            Status::Unknown | Status::ReorgedOut => None,
        }
    }
}

/// Each reserve's ledger, as its publisher records the segments inside it: its book's entries for the transactions
/// asked, read as `rutent_conformance::sim::recorded_segments` reads a book.
impl Books for Readings {
    fn recorded(&self, node: &str, transactions: &[Id]) -> Option<Vec<RecordedSegment>> {
        let reserve = node.strip_suffix("-ledger")?;
        let read = self.read.borrow();
        let book: Vec<BookEntry> = read
            .books
            .get(reserve)?
            .iter()
            .filter(|e| {
                transaction_of(&e.operation.statement)
                    .is_some_and(|tx| transactions.iter().any(|t| t.as_str() == tx))
            })
            .cloned()
            .collect();
        let spec = Spec::of(Role::Reserve, reserve);
        let segments: Vec<RecordedSegment> =
            recorded_segments(&spec, &SuiteContext::default(), &book)
                .into_iter()
                .map(|s| RecordedSegment {
                    asset_in: s.asset_in,
                    amount_in: s.amount_in,
                    asset_out: s.asset_out,
                    amount_out: s.amount_out,
                    fee: s.fee,
                })
                .collect();
        (!segments.is_empty()).then_some(segments)
    }
}
