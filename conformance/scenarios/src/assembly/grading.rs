//! What the assembly's grading reads of the simulated world (`T-020-09`): each part runs under
//! `closing::Evidenced`, over the simulated public networks the providers' movements are recorded on, as they stood when
//! Core last looked: at the start of each run and after each operation carried out. The world lives inside the
//! platform, which the segments hold as their modules, so the readings are a copy, refreshed.
//!
//! The private nodes' books are no reading of the world (`T-020-58`): Core asks each publisher's provider for its record
//! through the platform (`closing::publishers`) and hands what it took as a record to [`Readings`], which only keeps it
//! for the closing to read ([`Books`]). The assembly chooses no node and no provider.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_conformance::sim::Spec;
use rutent_contracts::provider_role::{InnerSegments, Statement};
use rutent_contracts::router_api::Id;
use rutent_core_orch::closing::{Books, Moved, PublicRecords, Recorded};
use rutent_evidence::replay::RecordedSegment;
use rutent_evidence::{MovementId, NetworkRules, PublicNetwork, SimulatedNetwork, Status};

use crate::sim::{Installed, SimWorld};

#[derive(Default)]
struct Read {
    networks: BTreeMap<String, SimulatedNetwork>,
    /// What each provider of a private node answered when Core last asked it for its record of the node, taken as one,
    /// by provider and node (`T-020-58`).
    records: BTreeMap<(String, String, String), Statement<InnerSegments>>,
    /// Every record taken, the last for each provider, node and order, dropped never: what a scenario reads of what Core
    /// took, which the books themselves forget once an order's segments have settled.
    taken: BTreeMap<(String, String, String), Statement<InnerSegments>>,
    /// Each question of a node's record Core put, as the platform carried it: the provider, the node and the
    /// transaction.
    asked: Vec<(String, String, String)>,
    /// Every movement recorded on each network, in the order first read, with where it went and how much: what a
    /// network that ties a deposit by its destination and amount reads.
    seen: BTreeMap<String, Vec<(MovementId, String, String)>>,
    /// The planted fault: a deposit tied to the first movement to its destination of its amount.
    ties: bool,
    /// The providers whose answer to the question of their record is altered on its way to Core: a planted fault.
    forged: BTreeMap<String, Tamper>,
}

/// How a planted fault alters a provider's answer to the question of its record, between the platform and Core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tamper {
    /// A figure of the record is changed after the provider signed it, as a forger on the wire would.
    Figure,
    /// The provider signs a record of another publisher's, which its advertisement doesn't declare.
    Publisher,
    /// The provider signs a record of a node its advertisement doesn't declare.
    Node,
    /// The provider signs a record with a key its advertisement doesn't list.
    Key,
    /// The answer is altered after the provider signed it, the record in it untouched.
    Answer,
    /// The provider says, in an answer it signs, that it holds no record of the node, for a transaction Core ran there
    /// (`T-020-67`).
    NotKnown,
}

/// The simulated networks, as Core last read them, and the records of the private nodes it took.
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

    /// From now on, `provider`'s answer to the question of its record is altered as `how` says.
    pub(crate) fn forge(&self, provider: &str, how: Tamper) {
        self.read
            .borrow_mut()
            .forged
            .insert(provider.to_owned(), how);
    }

    /// From now on, `provider`'s answer to the question of its record is as it made it.
    pub(crate) fn unforge(&self, provider: &str) {
        self.read.borrow_mut().forged.remove(provider);
    }

    /// Notes a question of `provider`'s record of `node` that Core put under `transaction`.
    pub(crate) fn note_asked(&self, provider: &str, node: &str, transaction: &str) {
        self.read.borrow_mut().asked.push((
            provider.to_owned(),
            node.to_owned(),
            transaction.to_owned(),
        ));
    }

    /// The records of the private node `node` Core took of its providers when it last read them, in the order the
    /// providers are installed: what closing's books hold.
    pub(crate) fn taken(&self, node: &str) -> Vec<Statement<InnerSegments>> {
        let read = self.read.borrow();
        Self::records_among(&read.taken, &self.movers, node)
    }

    /// What closing's books hold of `node` now: the records of the orders still in flight.
    fn held_of(&self, node: &str) -> Vec<Statement<InnerSegments>> {
        let read = self.read.borrow();
        Self::records_among(&read.records, &self.movers, node)
    }

    fn records_among(
        records: &BTreeMap<(String, String, String), Statement<InnerSegments>>,
        movers: &[Spec],
        node: &str,
    ) -> Vec<Statement<InnerSegments>> {
        movers
            .iter()
            .flat_map(|spec| {
                records
                    .iter()
                    .filter(|((provider, at, _), _)| *provider == spec.provider && at == node)
                    .map(|(_, record)| record.clone())
            })
            .collect()
    }

    /// How many records the books hold now.
    pub(crate) fn held(&self) -> usize {
        self.read.borrow().records.len()
    }

    /// Every question of a node's record Core put: the provider, the node and the transaction, in the order put.
    pub(crate) fn asked(&self) -> Vec<(String, String, String)> {
        self.read.borrow().asked.clone()
    }

    /// `answer`, `provider`'s answer to the question of its record as the platform passed it on, as the planted fault
    /// alters it, if one is planted for `provider`.
    pub(crate) fn tampered(&self, provider: &str, answer: Vec<u8>) -> Vec<u8> {
        let Some(how) = self.read.borrow().forged.get(provider).copied() else {
            return answer;
        };
        let Some(spec) = self.movers.iter().find(|s| s.provider == provider) else {
            return answer;
        };
        tamper(spec, how, &answer).unwrap_or(answer)
    }

    /// `answer`, a provider's answer to the question of its record, as it is when the provider made it `seconds` after it
    /// was asked: its time put later and the answer signed again, as the provider that is slow to answer would sign it
    /// (`T-020-67`). An answer that isn't one of a record is left as it is.
    pub(crate) fn made_later(&self, provider: &str, answer: Vec<u8>, seconds: u64) -> Vec<u8> {
        use rutent_conformance::role::placeholder_key;
        use rutent_conformance::signing::{real, sign_statement};
        let Some(spec) = self.movers.iter().find(|s| s.provider == provider) else {
            return answer;
        };
        let restamped = || -> Option<Vec<u8>> {
            let mut result: Statement<rutent_contracts::provider_role::ResultBody> =
                serde_json::from_slice(&answer).ok()?;
            let asked = rutent_contracts::router_api::Timestamp::new(result.made_at.clone())?;
            let secs = u64::try_from(asked.unix_nanos() / 1_000_000_000).ok()?;
            result.made_at =
                rutent_prov::catalogue::Instant(secs.checked_add(seconds)?).rfc3339()?;
            let key = placeholder_key(spec.seed());
            serde_json::to_vec(&sign_statement(&result, &real(key.as_str()))).ok()
        };
        restamped().unwrap_or(answer)
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

/// Each private node, as its publisher records the segments inside it (`REQ-ORD-41`): the records Core took of every
/// provider that serves in it, for the transactions asked, in the order the providers are installed and the
/// transactions are asked, each segment as its record states it.
impl Books for Readings {
    fn took(&self, provider: &str, node: &str, order: &str, record: Statement<InnerSegments>) {
        let key = (provider.to_owned(), node.to_owned(), order.to_owned());
        let mut read = self.read.borrow_mut();
        read.taken.insert(key.clone(), record.clone());
        read.records.insert(key, record);
    }

    fn forget(&self, order: &str, node: &str) {
        self.read
            .borrow_mut()
            .records
            .retain(|(_, n, o), _| n != node || o != order);
    }

    fn recorded(&self, node: &str, transactions: &[Id]) -> Option<Recorded> {
        // Every order's record of the node, each provider's together, in the order the providers are installed: an order's
        // transactions are listed in its own records alone, so an order reads its own.
        self.read_of(self.held_of(node), transactions)
    }
}

impl Readings {
    /// What every record Core took of `node` gives for `transactions`, whether the books still hold it or not: what a
    /// scenario asks of Core's books after the orders have settled (`T-024-14`).
    pub(crate) fn recorded_ever(&self, node: &str, transactions: &[Id]) -> Option<Recorded> {
        self.read_of(self.taken(node), transactions)
    }

    fn read_of(
        &self,
        records: Vec<Statement<InnerSegments>>,
        transactions: &[Id],
    ) -> Option<Recorded> {
        let mut segments = Vec::new();
        let mut read: Vec<Statement<InnerSegments>> = Vec::new();
        for transaction in transactions {
            let before = segments.len();
            for record in &records {
                let before_record = segments.len();
                segments.extend(
                    record
                        .statement
                        .segments
                        .iter()
                        .filter(|s| s.transaction == transaction.as_str())
                        .map(|s| RecordedSegment {
                            asset_in: s.asset_in.clone(),
                            amount_in: s.amount_in.clone(),
                            asset_out: s.asset_out.clone(),
                            amount_out: s.amount_out.clone(),
                            fee: s.fee.clone(),
                        }),
                );
                if segments.len() > before_record && !read.contains(record) {
                    read.push(record.clone());
                }
            }
            // Every transaction asked ran inside the node, so one with no taken segment is a record refused or never
            // made: nothing recorded for the node, never the rest of it passed off as all of it.
            if segments.len() == before {
                return None;
            }
        }
        (!segments.is_empty()).then_some(Recorded {
            segments,
            records: read,
        })
    }
}

/// `answer`, `spec`'s provider's answer to the question of its record, as the fault `how` alters it: a figure changed
/// after it was signed, or the provider's own signature (it holds the statements key) over a record its advertisement
/// doesn't permit it, the answer re-signed so only the record is in question.
fn tamper(spec: &Spec, how: Tamper, answer: &[u8]) -> Option<Vec<u8>> {
    use rutent_conformance::role::placeholder_key;
    use rutent_conformance::signing::{another_key, real, sign_statement};
    use rutent_contracts::provider_role::ResultBody;
    let mut result: Statement<ResultBody> = serde_json::from_slice(answer).ok()?;
    let ResultBody::InnerSegmentsQuery(inner) = &mut result.statement else {
        return None;
    };
    let key = placeholder_key(spec.seed());
    if how == Tamper::NotKnown {
        inner.outcome = rutent_contracts::provider_role::InnerSegmentsOutcome::NotKnown(
            rutent_contracts::provider_role::NoDetail {},
        );
        return serde_json::to_vec(&sign_statement(&result, &real(key.as_str()))).ok();
    }
    let rutent_contracts::provider_role::InnerSegmentsOutcome::Answered(answered) =
        &mut inner.outcome
    else {
        return None;
    };
    match how {
        Tamper::Figure => {
            answered
                .record
                .statement
                .segments
                .first_mut()?
                .amount_out
                .push('0');
            return serde_json::to_vec(&result).ok();
        }
        Tamper::Publisher => {
            answered.record.statement.publisher = format!("{}-another", spec.publisher());
            *answered.record = sign_statement(&answered.record, &real(key.as_str()));
        }
        Tamper::Node => {
            answered.record.statement.node = format!("{}-another", spec.ledger());
            *answered.record = sign_statement(&answered.record, &real(key.as_str()));
        }
        Tamper::Answer => {
            result.made_at = "2026-11-06T15:04:06Z".to_owned();
            return serde_json::to_vec(&result).ok();
        }
        Tamper::Key => {
            *answered.record = sign_statement(&answered.record, &another_key());
        }
        Tamper::NotKnown => unreachable!("answered above"),
    }
    serde_json::to_vec(&sign_statement(&result, &real(key.as_str()))).ok()
}
