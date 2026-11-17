//! What the assembly's grading reads of the simulated world (`T-020-09`): each part runs under
//! `closing::Evidenced`, over the simulated public networks the providers' movements are recorded on and the reserves'
//! books, as they stood when Core last looked: at the start of each run and after each operation carried out. The
//! world lives inside the platform, which the segments hold as their modules, so the readings are a copy, refreshed.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_conformance::signing::verify;
use rutent_conformance::sim::{Entry as BookEntry, Spec, publishers_record};
use rutent_contracts::provider_role::{
    Advertisement, InnerSegments, KeyPurpose, Statement, check_inner_segments,
};
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
    /// Each mover's advertisement, the one its catalogue holds: what a record of its is held to.
    advertisements: Rc<BTreeMap<String, Statement<Advertisement>>>,
}

impl Readings {
    /// Readings of the networks and books of `installed`.
    pub(crate) fn of(installed: &[Installed]) -> Self {
        let mut networks = Vec::new();
        let mut movers = Vec::new();
        let mut advertisements = BTreeMap::new();
        for one in installed {
            advertisements.insert(
                one.spec.provider.clone(),
                one.advertisement.statement().clone(),
            );
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
            advertisements: Rc::new(advertisements),
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

impl Readings {
    /// The record `provider`'s publisher keeps of the segments `provider` ran inside its private node, as the
    /// publisher's own signed statement (`rutent_conformance::sim::publishers_record`), as Core reads it: held to its
    /// maker's advertisement and to its signature, and refused, as no record at all, where it fails either. That its
    /// segments add up, or hand on to the public exit, is not asked: it is the replay's (`REQ-ORD-41`).
    pub(crate) fn published_by(&self, provider: &str) -> Option<Statement<InnerSegments>> {
        let spec = self.movers.iter().find(|s| s.provider == provider)?;
        let record = {
            let read = self.read.borrow();
            publishers_record(spec, &crate::assembly::context(), read.books.get(provider)?)?
        };
        accepted(&record, self.advertisements.get(provider)?).then_some(record)
    }

    /// Every record published for the private node `node`, one for each provider that serves in it, in the order the
    /// providers are installed.
    pub(crate) fn published_in(&self, node: &str) -> Vec<Statement<InnerSegments>> {
        self.movers
            .iter()
            .filter_map(|spec| self.published_by(&spec.provider))
            .filter(|record| record.statement.node == node)
            .collect()
    }
}

/// Each private node, as its publisher records the segments inside it (`REQ-ORD-41`): the records of every provider
/// that serves in it ([`Readings::published_in`]), for the transactions asked, in the order the transactions are asked,
/// each segment as its record states it.
impl Books for Readings {
    fn recorded(&self, node: &str, transactions: &[Id]) -> Option<Vec<RecordedSegment>> {
        let records = self.published_in(node);
        let mut segments = Vec::new();
        for transaction in transactions {
            for record in &records {
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
            }
        }
        (!segments.is_empty()).then_some(segments)
    }
}

/// Whether Core reads `record` as its maker's: signed with a key `advertisement`, its maker's, lists for statements and
/// verifying under it, and held to that advertisement as the contract asks (`check_inner_segments`: its maker's own
/// advertisement, its publisher the one the record names, its node among the maker's private nodes).
fn accepted(record: &Statement<InnerSegments>, advertisement: &Statement<Advertisement>) -> bool {
    let Ok(bytes) = serde_json::to_vec(record) else {
        return false;
    };
    let listed = advertisement
        .statement
        .keys
        .iter()
        .any(|k| k.purpose == KeyPurpose::Statements && k.public_key == record.signature.key);
    listed && verify(&bytes).is_ok() && check_inner_segments(record, advertisement).is_ok()
}

#[cfg(test)]
mod tests {
    use rutent_conformance::role::{RoleProvider, SuiteContext};
    use rutent_conformance::signing::{Signing, another_key, sign_statement};
    use rutent_conformance::sim::{Role, Spec};
    use rutent_contracts::provider_role::{
        INNER_SEGMENTS_SHAPE, InnerSegment, Signature, SignatureScheme, Standing,
    };

    use super::*;
    use crate::sim::fleet::InProcess;

    /// The reserve `spec`, as its advertisement lists it, and a record it signs of one segment inside its node.
    fn record_of(
        spec: &Spec,
    ) -> (
        Statement<Advertisement>,
        Statement<InnerSegments>,
        rutent_conformance::signing::Ed25519Key,
    ) {
        let context = SuiteContext::default();
        let advertisement = Signing(InProcess(spec.make(&context)))
            .advertisement()
            .into_statement();
        let key = rutent_conformance::role::placeholder_key(spec.seed());
        let record = Statement {
            deployment: context.deployment.clone(),
            protocol_version: context.protocol_version.clone(),
            shape: INNER_SEGMENTS_SHAPE.to_owned(),
            maker: spec.provider.clone(),
            made_at: advertisement.made_at.clone(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: key.clone(),
                value: rutent_conformance::role::placeholder_signature(),
            },
            statement: InnerSegments {
                node: spec.ledger(),
                publisher: spec.publisher(),
                standing: Standing::PublishersWord,
                segments: vec![InnerSegment {
                    transaction: "tx-1".to_owned(),
                    asset_in: "COP".to_owned(),
                    amount_in: "1000".to_owned(),
                    asset_out: "COP".to_owned(),
                    amount_out: "990".to_owned(),
                    fee: "10".to_owned(),
                }],
            },
        };
        let signing = rutent_conformance::signing::real(key.as_str());
        let signed = sign_statement(&record, &signing);
        (advertisement, signed, signing)
    }

    // verifies: REQ-ORD-41
    #[test]
    fn a_record_its_maker_signed_and_declared_is_read() {
        let (advertisement, record, _) = record_of(&Spec::of(Role::Reserve, "sim-reserve-b"));
        assert!(accepted(&record, &advertisement));
    }

    // verifies: REQ-ORD-41
    #[test]
    fn a_record_changed_after_it_was_signed_is_not_read() {
        let (advertisement, mut record, _) = record_of(&Spec::of(Role::Reserve, "sim-reserve-b"));
        record.statement.segments[0].amount_out = "999".to_owned();
        assert!(!accepted(&record, &advertisement));
    }

    // verifies: REQ-ORD-41
    #[test]
    fn a_record_signed_with_a_key_its_maker_does_not_list_is_not_read() {
        let (advertisement, record, _) = record_of(&Spec::of(Role::Reserve, "sim-reserve-b"));
        let by_another = sign_statement(&record, &another_key());
        assert!(!accepted(&by_another, &advertisement));
    }

    // verifies: REQ-ORD-41
    #[test]
    fn a_record_of_another_publisher_or_another_node_is_not_read() {
        let (advertisement, record, key) = record_of(&Spec::of(Role::Reserve, "sim-reserve-b"));
        let mut other_publisher = record.clone();
        other_publisher.statement.publisher = "sim-reserve-a-publisher".to_owned();
        assert!(!accepted(
            &sign_statement(&other_publisher, &key),
            &advertisement
        ));
        let mut other_node = record;
        other_node.statement.node = "sim-reserve-a-ledger".to_owned();
        assert!(!accepted(
            &sign_statement(&other_node, &key),
            &advertisement
        ));
    }
}
