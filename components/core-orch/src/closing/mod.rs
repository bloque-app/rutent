//! Evidence and closing: each segment's evidence graded through `CMP-EVIDENCE` and recorded.
//!
//! Declared by `T-020-01`; `T-020-09` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! [`Evidenced`] is the grading the segments call once a result counts ([`Grading`]). It answers the entries a
//! segment's `segment_settled` rests on, or why it doesn't settle yet; the segments append them, each once.
//!
//! - **Graded through `CMP-EVIDENCE`** (`REQ-ORD-39`, `DEC-13`). The result's pieces are graded by
//!   [`grade_outcome`] against the grade its provider declared. A claim of grade 1 or 2 is backed only by the check
//!   of the segment's own public movement, the one its network ties to the segment's transaction
//!   ([`PublicRecords::movement`]); an unbacked claim is a statement. The `evidence` entry says the grade and whose
//!   it is: for a public record, the network Core checked, whatever the piece names, and the `movement`; otherwise
//!   the provider, whose word or signature it is. A segment's movement is held to the route's one destination only
//!   at a final deposit (`DEC-13`); elsewhere the destination the network's matching gives is taken as the
//!   segment's, so that tie rests on each network's matching (`F-031`). The amount compared is what the provider
//!   says the segment gave out; whether a network shows a movement gross or net of a fee is that network's
//!   matching too (`F-031`).
//! - **A part closes at its final deposit** on a final public record of the deposit's movement to the route's one
//!   destination (its account, its to asset, what the deposit gave out), checked by [`public_record_check`], plus the
//!   reserve's signed statement, its result, which `judge` verified (provider-role's README: the result's envelope
//!   is the reserve's "signed statement for its own book"). Two `evidence` entries say whose each is: the network's,
//!   which `segment_settled` cites and which names the movement, and the reserve's, at the grade its own pieces earn.
//!   Without the public record the deposit doesn't settle: a statement alone closes no order (`REQ-ORD-33`), and the
//!   log refuses one that does (`final-deposit-closes-on-a-public-record`). A deposit to a rail declared private
//!   closes on the public movement into the private node instead, the movement naming the node, plus the reserve's
//!   statement, and records no replay (`REQ-ORD-41`). A final deposit to a reserve whose destination Core can read
//!   no public record of, and which no publisher declares private, isn't routed to ([`Grading::can_close`]).
//! - **Within a private node** (`REQ-ORD-33`). A segment whose two ends are private settles on its provider's
//!   evidence (private nodes rest on grades 3 and 4 by necessity, ADR-0007); where the value leaves the private nodes
//!   on a final public record and the node's replay is consistent with it, each such segment is recorded as closing
//!   on the public movements into and out of the node, an `evidence` entry for each, whose the network's, naming the
//!   movement and the node. A node the replay caught disagreeing gets none.
//! - **Replay on the way out** (`REQ-ORD-41`, `DEC-24`). Where a segment takes the value from a private node onto a
//!   public one, the private nodes it passed through since it last left a public node are replayed by
//!   [`replay_route`], each publisher's record of the segments inside its node ([`Books`]) in route order, against
//!   the public movement out and the outcome the provider stated, and a `replay` entry records what was found: a
//!   match as consistency with the exit, never proof, each mismatch with its node, or every node that recorded
//!   nothing; it rests on the exit's result and names the movement out.
//!   On a route through two private nodes with a public node between them, each is replayed in turn, as the value
//!   leaves it; two adjacent ones are replayed together, as one passes the value to the other. A mismatch is
//!   recorded and stops nothing: replay detects, it doesn't prove (`THR-131`).
//! - **One movement, one segment** (`REQ-ORD-38`). Each public record names its movement, so the log refuses a second
//!   segment relying on one (`movement-evidences-one-segment`), and [`Closing`], which keeps across orders which
//!   segment each movement evidences, is rebuilt from the log after a restart ([`Closing::recorded`]). A movement
//!   already evidencing another order's segment backs nothing for a second: that segment is graded on its other
//!   evidence, so it is recorded as its provider's word unless that grades higher, and a final deposit on it doesn't
//!   close. Each order keeps its own segment.
//! - **Value rests only in a holding account** (`INV-5`) and **no provider below the floor** (`REQ-ORD-39`):
//!   [`admits`], which the segments call before handing a segment over, holds it to the catalogue as it stands.
//! - **A fee record per settled segment** (`REQ-AUD-11`): [`fees`], attributed to its provider and, on a part's final
//!   deposit, to the operator, whose fee that provider keeps back, carrying what the provider stated of billing it
//!   separately, and nothing where it said nothing. Nothing here or in the log sums them: no balance.

use std::collections::BTreeMap;

mod price;
mod public_end;
pub mod publishers;

pub use price::{PriceDesk, Reported, RouteRun, Unreported, report_logged_price, report_price};

use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::core_internal::{
    About, Actor, Entry, FeeTo, MismatchKind, Outcome, PublicMovement, Receipt, ReplayFound,
    ReplayMismatch, TimeSource, VerifiedEntry, VerifiedKind, What, When,
};
use rutent_contracts::provider_role::{
    ConversionOutcome, Denomination, EvidenceGrade, EvidencePiece, InnerSegments, NodeRef, Payouts,
    ReserveMovementOutcome, ResultBody, Statement, TransferOutcome,
};
use rutent_contracts::router_api::{Amount, Id, Operation, OperationKind};
use rutent_evidence::replay::{
    Mismatch, NodeRecord, PublicExit, RecordedSegment, Replay, RouteEnd, Stated, Unreplayable,
    replay_route,
};
use rutent_evidence::{
    Check, Claim, Expected, GradeExt, MovementId, NetworkRules, PublicNetwork, Status, Unmatched,
    grade_outcome, public_record_check, verify_movement,
};
use rutent_prov::catalogue::{Catalogue, Category, Endpoint, TupleKey};

use crate::segments::{AskDesk, Graded, Grading, RaiseDesk, Short, ToGrade, ToKeep};
use crate::snapshot::RouteRefusal;

/// A segment's movement on a public network, as that network ties it to the segment's transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// The movement's identifier on its network.
    pub id: MovementId,
    /// Where it went, as the network records it.
    pub destination: String,
}

/// The public networks Core reads, and how each ties a movement to a segment: each network's finality, reorg rule and
/// movement-to-segment matching are its own task when it joins the catalogue (`DEC-13`, `F-031`). Core holds a
/// final deposit's movement to the route's one destination whatever the matching says.
///
/// What Core records of a movement, and what "one movement, one segment" compares, is the network's label and the
/// movement's id as given here, so an implementation gives one label per chain, whatever node names it, and one
/// canonical spelling of each movement's id (a transaction hash in one case, with or without its prefix, never both):
/// two spellings of one movement would be two movements to the log (`movement-evidences-one-segment`).
pub trait PublicRecords {
    /// The finality and reorg rules of the public network named `network`; `None` for one Core reads no record of.
    fn rules(&self, network: &str) -> Option<NetworkRules>;

    /// What `network` knows of the movement `id` now. Read-only.
    fn status(&self, network: &str, id: &MovementId) -> Status;

    /// The movement on `network` that the segment run under `transaction` made, by the network's own matching;
    /// `None` where it ties none to it.
    fn movement(&self, network: &str, transaction: &Id) -> Option<Moved>;
}

/// One public network, as `CMP-EVIDENCE` reads it, through [`PublicRecords`].
struct Network<'r> {
    records: &'r dyn PublicRecords,
    name: String,
    rules: NetworkRules,
}

impl PublicNetwork for Network<'_> {
    fn network(&self) -> &str {
        &self.name
    }

    fn rules(&self) -> NetworkRules {
        self.rules
    }

    fn status(&self, id: &MovementId) -> Status {
        self.records.status(&self.name, id)
    }
}

fn network_of<'r>(records: &'r dyn PublicRecords, name: &str) -> Option<Network<'r>> {
    Some(Network {
        records,
        name: name.to_owned(),
        rules: records.rules(name)?,
    })
}

/// What a private node's publisher recorded of the segments inside it, as the assembly reads it from the publishers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The segments, as `CMP-EVIDENCE`'s [`RecordedSegment`] states each, in the order asked.
    pub segments: Vec<RecordedSegment>,
    /// The publisher's signed `inner-segments` records those segments were read from, whole, one for each provider of
    /// the node that recorded any of them (`T-020-32`): what closing appends as a stated entry and cites.
    pub records: Vec<Statement<InnerSegments>>,
}

/// The private nodes' publishers' records of the segments inside their nodes (`REQ-ORD-41`): provider-role's
/// `inner-segments` statement, whole, so closing can append it (`T-020-32`) and `CMP-EVIDENCE`'s [`RecordedSegment`]
/// is what the replay reads of it. The assembly reads it from each publisher.
pub trait Books {
    /// What the publisher of private node `node` recorded of the segments inside it that ran under `transactions`,
    /// in the order they ran; `None` where it recorded nothing.
    fn recorded(&self, node: &str, transactions: &[Id]) -> Option<Recorded>;

    /// Hands the books what `provider`, a provider of `node`'s publisher, answered when Core asked it for its record of
    /// the node, taken as one ([`publishers::taken`]): the record, or `None` where the answer wasn't a record of the
    /// node, which leaves the books with nothing of that provider's. Core asks again each time it grades a segment, so
    /// the books keep only the last answer for each provider and node and rebuild from the next asking after a restart
    /// (`T-020-58`). Books that are given nothing (a fixed listing) ignore it.
    fn took(&self, provider: &str, node: &str, record: Option<Statement<InnerSegments>>) {
        let _ = (provider, node, record);
    }
}

/// What grading kept of one settled segment, for the segments after it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    transaction: Id,
    result: Receipt,
    /// Its own public movement, where a final public record of it evidences it: the network and the movement.
    movement: Option<(String, MovementId)>,
}

/// A segment that took a public movement: its order and segment, and whether it took it as a private node's
/// movement in or out, which each segment inside the node takes alike, of its one order (`REQ-ORD-33`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Taker {
    order: String,
    segment: String,
    node: bool,
}

/// Core's record of the evidence it relied on, across orders: every public movement Core checked and which segments
/// took it, and what each settled segment's grading found. It takes a movement exactly as the log does
/// (`movement-evidences-one-segment`): a segment of another order never takes one taken, and one of the same order
/// only as a node's movement in or out. The production assembly keeps it as durably as `Segments` (`F-030`); after a
/// restart [`Closing::recorded`] rebuilds it from the log, which names every movement Core checked.
#[derive(Debug, Clone, Default)]
pub struct Closing {
    relied: BTreeMap<(String, String), Vec<Taker>>,
    seen: BTreeMap<(String, String), Seen>,
    /// The mismatches at non-final public ends found and not raised and recorded yet (`T-020-22`).
    unraised: public_end::Unraised,
}

impl Closing {
    /// Nothing relied on yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Core's record rebuilt from the log's `history`, as after a restart: every movement an `evidence` entry names,
    /// with the segment that took it, and each settled segment's transaction, result and own movement, read from the
    /// evidence `segment_settled` cites and the result's stated entry it rests on.
    pub fn recorded(history: &[(Receipt, Entry)]) -> Self {
        let mut closing = Self::new();
        closing.unraised = public_end::Unraised::rebuilt(history);
        let verified = |e: &Entry| match e {
            Entry::Verified(v) => Some(v.what.clone()),
            Entry::Stated(_) => None,
        };
        let cited: Vec<String> = history
            .iter()
            .filter_map(|(_, e)| match verified(e)? {
                What::Input { input } => match input.observed {
                    rutent_contracts::state_machine::Observed::SegmentSettled { evidence } => {
                        Some(evidence.as_str().to_owned())
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        for (_, entry) in history {
            let Some(What::Evidence {
                evidence,
                order,
                segment,
                rests_on,
                movement,
                ..
            }) = verified(entry)
            else {
                continue;
            };
            let key = (order.as_str().to_owned(), segment.as_str().to_owned());
            if let Some(m) = &movement {
                closing.take(m.network.as_str(), m.id.as_str(), &key, m.node.is_some());
            }
            let own = movement.filter(|m| m.node.is_none());
            if !cited.contains(&evidence.as_str().to_owned()) {
                continue;
            }
            let transaction = rests_on.iter().find_map(|r| {
                history.iter().find_map(|(receipt, e)| match e {
                    Entry::Stated(s) if receipt == r => s.about.as_ref()?.transaction.clone(),
                    _ => None,
                })
            });
            if let (Some(transaction), Some(result)) = (transaction, rests_on.first()) {
                closing.seen.insert(
                    key,
                    Seen {
                        transaction,
                        result: result.clone(),
                        movement: own.and_then(|m| {
                            Some((
                                m.network.as_str().to_owned(),
                                MovementId::new(m.id.as_str())?,
                            ))
                        }),
                    },
                );
            }
        }
        closing
    }

    /// The raises of a non-final public end's mismatch the log refused, each with its order, segment and why: a refusal
    /// is deterministic and the raise advisory, so the part runs on and the refusal stays readable here
    /// (`T-020-22`).
    pub fn refused_raises(&self) -> &[(Id, Id, rutent_contracts::core_internal::Violation)] {
        self.unraised.refused()
    }

    /// The order and segment that first took the public movement `movement` on `network`, if one did.
    pub fn evidences(&self, network: &str, movement: &str) -> Option<(&str, &str)> {
        self.relied
            .get(&(network.to_owned(), movement.to_owned()))
            .and_then(|takers| takers.first())
            .map(|t| (t.order.as_str(), t.segment.as_str()))
    }

    /// Whether the segment `key` may take the movement, as its own (`node` false) or as a node's movement in or out:
    /// no other order took it, and no other segment of its order took it as its own where this one would too.
    fn free_for(&self, network: &str, movement: &str, key: &(String, String), node: bool) -> bool {
        self.relied
            .get(&(network.to_owned(), movement.to_owned()))
            .is_none_or(|takers| {
                takers
                    .iter()
                    .all(|t| t.order == key.0 && (t.segment == key.1 || t.node || node))
            })
    }

    /// Whether the segment `key` took the movement already, as a node's movement in or out.
    fn took_in_node(&self, network: &str, movement: &str, key: &(String, String)) -> bool {
        self.relied
            .get(&(network.to_owned(), movement.to_owned()))
            .is_some_and(|takers| {
                takers
                    .iter()
                    .any(|t| t.node && t.order == key.0 && t.segment == key.1)
            })
    }

    fn take(&mut self, network: &str, movement: &str, key: &(String, String), node: bool) {
        let takers = self
            .relied
            .entry((network.to_owned(), movement.to_owned()))
            .or_default();
        let taker = Taker {
            order: key.0.clone(),
            segment: key.1.clone(),
            node,
        };
        if !takers.contains(&taker) {
            takers.push(taker);
        }
    }
}

/// The grading through `CMP-EVIDENCE`, for the segments to call ([`Grading`]): the catalogue the providers'
/// declarations and the nodes are read from, the public networks, the private nodes' records, and Core's record
/// across orders.
pub struct Evidenced<'a> {
    /// The catalogue: each provider's declared grade, which nodes are private and which public.
    pub catalogue: &'a Catalogue,
    /// The public networks.
    pub records: &'a dyn PublicRecords,
    /// The private nodes' records.
    pub books: &'a dyn Books,
    /// Core's record across orders.
    pub closing: &'a mut Closing,
}

/// Why an operation isn't routed through, before its segment is handed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unadmitted {
    /// Its provider declares evidence below the floor on its tuple, or declares none, or its tuple has no floor
    /// (`REQ-ORD-39`).
    BelowEvidenceFloor,
    /// It leaves the value at an inner node with no holding account (`INV-5`).
    NoHoldingAccount,
    /// The catalogue lists no such provider.
    NotListed,
    /// It is a part's final deposit to a destination with no public record Core reads, which no publisher declares a
    /// private rail: nothing could close the part (`REQ-ORD-33`).
    NoPublicRecord,
    /// It is a part's final deposit at a reserve tuple that declares `batched` payouts, which no order's close can
    /// rest on alone (`REQ-ORD-33`, `T-020-43`).
    BatchedPayout,
}

impl Unadmitted {
    /// The route guard's refusal, for the operation at `place`.
    pub fn at(self, place: usize) -> RouteRefusal {
        match self {
            Unadmitted::BelowEvidenceFloor => RouteRefusal::BelowEvidenceFloor(place),
            Unadmitted::NoHoldingAccount => RouteRefusal::NoHoldingAccount(place),
            Unadmitted::NotListed => RouteRefusal::ProviderNotListed(place),
            Unadmitted::NoPublicRecord => RouteRefusal::NoPublicRecord(place),
            Unadmitted::BatchedPayout => RouteRefusal::BatchedPayout(place),
        }
    }
}

/// Whether `deposit` is at a reserve tuple that declares `batched` payouts in its provider's accepted advertisement:
/// one movement may pay several orders' deposits, and a movement evidences one order's close, so a part ending
/// here could never close (`REQ-ORD-33`, `T-020-43`). A reserve that declares nothing pays per order, and so does
/// a deposit whose provider or tuple the catalogue doesn't hold, which is refused on other grounds (`admits`).
pub fn pays_in_batches(catalogue: &Catalogue, deposit: &Operation) -> bool {
    deposit.kind == OperationKind::Deposit
        && catalogue
            .accepted_advertisement(deposit.provider.as_str())
            .and_then(|ad| ad.statement.offers.reserve.as_ref())
            .is_some_and(|reserve| {
                reserve.tuples.iter().any(|t| {
                    t.origin == deposit.from.node
                        && t.destination == deposit.to.node
                        && t.asset == deposit.from.asset.id
                        && t.payouts() == Payouts::Batched
                })
            })
}

/// Whether `operation` may be routed through now, by the catalogue as it stands: the catalogue lists its provider,
/// which declares evidence at or above the floor the operator set on its tuple (`REQ-ORD-39`), and, where it isn't the
/// part's `last`, the inner node it leaves the value at has a holding account (`INV-5`, `REQ-CAT-23`). The snapshot a
/// route is chosen from leaves such providers and nodes out; this holds a segment to a floor raised, or a catalogue
/// changed, since.
pub fn admits(catalogue: &Catalogue, operation: &Operation, last: bool) -> Result<(), Unadmitted> {
    let Some(declarations) = catalogue.declared_by(operation.provider.as_str()) else {
        return Err(Unadmitted::NotListed);
    };
    let floor = catalogue.evidence_floor(&tuple_of(operation));
    let declared = declarations.evidence.as_ref().map(|e| e.grade);
    match (floor, declared) {
        (Some(floor), Some(declared)) if !floor.is_stronger_than(declared) => {}
        _ => return Err(Unadmitted::BelowEvidenceFloor),
    }
    if !last
        && let NodeRef::Inner(node) = &operation.to.node
        && catalogue.nodes().holding(node).is_none()
    {
        return Err(Unadmitted::NoHoldingAccount);
    }
    Ok(())
}

/// The catalogue's key for the tuple `operation` runs through.
fn tuple_of(operation: &Operation) -> TupleKey {
    let category = match operation.kind {
        OperationKind::Withdrawal | OperationKind::Deposit => Category::Reserve,
        OperationKind::Exchange => Category::Exchange,
        OperationKind::Route => Category::Ramp,
    };
    TupleKey {
        category,
        origin: Endpoint::from(&operation.from.node),
        destination: Endpoint::from(&operation.to.node),
        asset: operation.from.asset.id.clone(),
        to_asset: (category == Category::Exchange).then(|| operation.to.asset.id.clone()),
    }
}

/// The fee records of a settled segment (`REQ-AUD-11`): the fee its result states, attributed to its provider, and,
/// on a part's final deposit, the operator's fee, which that provider keeps back, by the operator's formula over what
/// arrived. A formula may charge zero, and the record says so. Each carries what its provider stated of billing it
/// separately, and nothing where it said nothing; the operator's is the provider's to keep back, never billed apart.
/// Each is the segment's own: none sums another, and nothing keeps a balance. A result that can't be read for its fee
/// gives no provider's record.
pub fn fees(settled: &ToGrade<'_>) -> Vec<Entry> {
    let Some(operation) = operation_of(settled) else {
        return Vec::new();
    };
    let mut fees = Vec::new();
    let entry = |to: FeeTo, party: &Id, amount: Amount, asset: Denomination, billed| {
        verified(
            settled.core,
            What::Fee {
                order: settled.order.clone(),
                segment: settled.segment.clone(),
                to,
                party: party.clone(),
                amount,
                asset,
                billed_separately: billed,
            },
            settled,
        )
    };
    if let Some(settlement) = settlement_of(settled.bytes)
        && settlement.fee_asset == operation.from.asset.id
        && let Some(amount) = Amount::new(settlement.fee)
    {
        fees.push(entry(
            FeeTo::Provider,
            settled.provider,
            amount,
            operation.from.asset.clone(),
            settlement.billed_separately,
        ));
    }
    if is_final_deposit(settled)
        && let Some(arrived) = Amount::new(settled.arrives.to_owned())
        && let Some(amount) = rutent_router::quote::operator_fee(settled.operator_fee, &arrived)
    {
        fees.push(entry(
            FeeTo::Operator,
            &settled.operator.acts,
            amount,
            operation.to.asset.clone(),
            None,
        ));
    }
    fees
}

impl Grading for Evidenced<'_> {
    fn ask(&mut self, settled: &ToGrade<'_>, desk: &mut AskDesk<'_>) {
        self.ask_publishers(settled, desk);
    }

    fn statements(&mut self, settled: &ToGrade<'_>) -> Vec<ToKeep> {
        self.to_keep(settled)
    }

    fn grade(&mut self, settled: &ToGrade<'_>) -> Graded {
        match self.graded(settled) {
            Ok(graded) => graded,
            Err(short) => Graded::Waits(short),
        }
    }

    fn can_close(&self, deposit: &Operation) -> bool {
        match &deposit.to.node {
            // A private rail closes on the public movement into it, which only a deposit from a public node makes.
            NodeRef::Outer(_) if self.private(&deposit.to.node) => self.public(&deposit.from.node),
            NodeRef::Outer(outer) => self.records.rules(&outer.network).is_some(),
            NodeRef::Inner(_) => false,
        }
    }

    fn recorded(&mut self, settled: &ToGrade<'_>, receipts: &[Receipt]) {
        self.closing
            .unraised
            .bind(settled.order, settled.segment, receipts);
    }

    fn raise(&mut self, desk: RaiseDesk<'_>) -> Result<(), AppendFailure> {
        self.closing.unraised.raise(desk)
    }
}

/// A settled segment's settlement, as its result states it.
struct Settlement {
    /// What the segment took in, its charge included: what entered a reserve from a public network.
    amount_in: String,
    fee: String,
    fee_asset: String,
    billed_separately: Option<bool>,
    pieces: Vec<EvidencePiece>,
}

fn settlement_of(bytes: &[u8]) -> Option<Settlement> {
    let result: Statement<ResultBody> = serde_json::from_slice(bytes).ok()?;
    let (amount_in, fee, pieces) = match result.statement {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match r.outcome {
            ReserveMovementOutcome::Succeeded(s) => (s.amount_in, s.fee, s.evidence),
            _ => return None,
        },
        ResultBody::Transfer(r) => match r.outcome {
            TransferOutcome::Succeeded(s) => (s.amount_in, s.fee, s.evidence),
            _ => return None,
        },
        ResultBody::Conversion(r) => match r.outcome {
            ConversionOutcome::Succeeded(s) => (s.amount_in, s.fee, s.evidence),
            _ => return None,
        },
        _ => return None,
    };
    Some(Settlement {
        amount_in,
        fee: fee.amount,
        fee_asset: fee.asset,
        billed_separately: fee.billed_separately,
        pieces,
    })
}

fn operation_of<'a>(settled: &ToGrade<'a>) -> Option<&'a Operation> {
    settled.operations.get(settled.place)
}

/// Whether the segment is its part's last, a deposit: where the part closes.
pub fn is_final_deposit(settled: &ToGrade<'_>) -> bool {
    let ops = settled.operations;
    settled.place + 1 == ops.len()
        && ops
            .get(settled.place)
            .is_some_and(|op| op.kind == OperationKind::Deposit)
}

fn verified(core: &Actor, what: What, settled: &ToGrade<'_>) -> Entry {
    Entry::Verified(VerifiedEntry {
        kind: VerifiedKind::Verified,
        who: core.clone(),
        what,
        when: When {
            at: settled.at.clone(),
            source: TimeSource::CoreClock,
        },
        outcome: Outcome::Observed {},
    })
}

fn node_name(node: &NodeRef) -> &str {
    match node {
        NodeRef::Inner(id) => id,
        NodeRef::Outer(outer) => &outer.network,
    }
}

fn id(text: &str) -> Result<Id, Short> {
    Id::new(text).ok_or(Short::Unreadable)
}

fn public_movement(
    network: &str,
    movement: &MovementId,
    node: Option<&str>,
) -> Result<PublicMovement, Short> {
    Ok(PublicMovement {
        network: id(network)?,
        id: id(movement.as_str())?,
        node: node.map(id).transpose()?,
    })
}

/// The segment's own public movement, checked: its network, its id, and the check `CMP-EVIDENCE` made.
type Checked = (String, MovementId, Check);

/// A non-final segment's public record that contradicts what its provider said, as far as it could be read: what is
/// recorded of it, each part `None` where it can't be written as the log's shape.
struct Contradicted {
    found: rutent_contracts::core_internal::PublicEndMismatch,
    /// The segment's planned public end on the movement's network.
    node: Option<rutent_contracts::provider_role::OuterNode>,
    network: String,
    movement: MovementId,
    /// What the network moved.
    moved: Option<rutent_contracts::core_internal::PublicFigures>,
    /// What the provider said it moved.
    stated: Option<rutent_contracts::core_internal::PublicFigures>,
}

impl Evidenced<'_> {
    /// Whether `node` is declared private by its publisher: an inner node, or a rail with no public record of its
    /// own (`REQ-ORD-33`, `REQ-CAT-22`).
    fn private(&self, node: &NodeRef) -> bool {
        self.catalogue
            .publishers()
            .publisher_of_node(node_name(node))
            .is_some()
    }

    /// Whether `node` is public: an outer node no publisher declares private, or an inner node on the operator's
    /// public list.
    fn public(&self, node: &NodeRef) -> bool {
        match node {
            NodeRef::Outer(outer) => !self.private(node) && !outer.network.is_empty(),
            NodeRef::Inner(id) => self.catalogue.nodes().is_public(id),
        }
    }

    fn graded(&mut self, settled: &ToGrade<'_>) -> Result<Graded, Short> {
        let operations = settled.operations;
        let operation = operations.get(settled.place).ok_or(Short::Unreadable)?;
        let settlement = settlement_of(settled.bytes).ok_or(Short::Unreadable)?;
        let last = is_final_deposit(settled);
        // A part's final deposit to a rail declared private (`REQ-ORD-33`).
        let into_private_rail = last && self.private(&operation.to.node);
        let key = (
            settled.order.as_str().to_owned(),
            settled.segment.as_str().to_owned(),
        );

        // The segment's own public movement: at its public end, where it has one, its destination's first. Only a
        // final deposit out to its destination is held to the route's one destination (`DEC-13`); elsewhere the
        // destination the network's matching gives is the segment's (`F-031`).
        let leaves = self.public(&operation.to.node);
        let side = if leaves {
            Some(&operation.to)
        } else if self.public(&operation.from.node) {
            Some(&operation.from)
        } else {
            None
        };
        let to_destination = last && !into_private_rail;
        let mut why = None;
        let mut movement: Option<Checked> = None;
        // What a non-final segment's public record contradicts of what its provider said (`T-020-22`).
        let mut contradicted: Option<Contradicted> = None;
        if let Some(side) = side {
            let network = node_name(&side.node).to_owned();
            let moved = self.records.movement(&network, settled.transaction);
            match (network_of(self.records, &network), moved) {
                (Some(net), Some(moved)) => {
                    let expected = Expected {
                        network: network.clone(),
                        destination: if to_destination {
                            settled.end.destination.account.as_str().to_owned()
                        } else {
                            moved.destination.clone()
                        },
                        asset: if to_destination {
                            settled.end.to_asset.id.clone()
                        } else {
                            side.asset.id.clone()
                        },
                        // What the network moved at this end: onto a public node, what arrives by the segment, its
                        // charge staying with the reserve; from one, what entered, the charge included.
                        amount: if leaves {
                            settled.arrives.to_owned()
                        } else {
                            settlement.amount_in.clone()
                        },
                    };
                    why = verify_movement(&net, &moved.id, &expected).err();
                    // A part's final deposit is held to a public record, so its contradiction settles nothing and
                    // is no mismatch; any other segment settles on its provider's word, and the contradiction is
                    // recorded and raised.
                    if !last
                        && let Some(found) = public_end::found(why)
                        && let Status::Final(record) = net.status(&moved.id)
                    {
                        contradicted = Some(Contradicted {
                            found,
                            node: public_end::planned_end(operation, &network),
                            network: network.clone(),
                            movement: moved.id.clone(),
                            moved: public_end::network_figures(&record),
                            stated: public_end::stated_figures(&expected),
                        });
                    }
                    let check = public_record_check(&net, &moved.id, &expected);
                    if check.movement().is_some() {
                        movement = Some((network, moved.id, check));
                    }
                }
                (None, _) => why = Some(Unmatched::WrongNetwork),
                (Some(_), None) => why = Some(Unmatched::UnknownMovement),
            }
        }
        // One movement evidences one segment: one another segment took backs nothing here.
        let shared = movement.as_ref().is_some_and(|(network, id, _)| {
            !self
                .closing
                .free_for(network, id.as_str(), &key, into_private_rail)
        });
        if shared {
            movement = None;
        }

        // The pieces, graded: a claim of grade 1 or 2 is backed only by the check of this movement, on the piece
        // that names it.
        let mut backing = movement
            .as_ref()
            .map(|(_, id, check)| (id.clone(), check.clone()));
        let claims: Vec<Claim> = settlement
            .pieces
            .iter()
            .map(|piece| {
                let check = match &backing {
                    Some((id, _)) if piece.grade.is_proof() && piece.reference == id.as_str() => {
                        backing.take().map(|(_, check)| check)
                    }
                    _ => None,
                };
                Claim {
                    piece: piece.clone(),
                    check: check.unwrap_or_else(Check::unchecked),
                }
            })
            .collect();
        let declared = self
            .catalogue
            .declared_by(settled.provider.as_str())
            .and_then(|d| d.evidence.as_ref())
            .map_or(EvidenceGrade::Statement, |e| e.grade);
        let graded =
            grade_outcome(settled.provider.as_str(), declared, claims).map_err(Short::Refused)?;
        // A public record is the network's that Core checked, never whoever the piece names; anything else is the
        // provider's own word or signature.
        let relies = graded.grade == EvidenceGrade::PublicRecord && movement.is_some();
        let evidence = |evidence: Id,
                        segment: &Id,
                        grade,
                        whose: Id,
                        rests_on: Receipt,
                        movement: Option<PublicMovement>,
                        publisher: Option<Id>| {
            verified(
                settled.core,
                What::Evidence {
                    evidence,
                    order: settled.order.clone(),
                    segment: segment.clone(),
                    grade,
                    whose,
                    rests_on: vec![rests_on],
                    movement,
                    publisher,
                },
                settled,
            )
        };

        // Whose word a segment inside a private node rests on: the node's publisher's, where the statement relied
        // on is by that publisher, through a provider of its (`REQ-ORD-3`). Labelled only where the log binds the
        // segment's from-node as that publisher's private node: inside the node the run's public movement entered,
        // the segment's own record of that movement, naming the node and resting on its statement, written first. A
        // part's final deposit into a rail declared private starts at a public node, which nothing binds so: it,
        // and anything else, is labelled with no publisher.
        let publishers = self.catalogue.publishers();
        let theirs = publishers.publisher_of_provider(settled.provider.as_str());
        let declares = |node: &NodeRef| {
            self.private(node) && publishers.publisher_of_node(node_name(node)) == theirs
        };
        let entered = &operations[self.run_start(operations, settled.place)]
            .to
            .node;
        let moved_in = (!last
            && self.private(&operation.from.node)
            && self.private(&operation.to.node)
            && node_name(entered) == node_name(&operation.from.node))
        .then(|| self.movement_into(settled, operations))
        .flatten();
        let bound = moved_in.is_some() && declares(&operation.from.node);
        let label = match theirs {
            Some(publisher) if bound => Some(id(publisher)?),
            _ => None,
        };
        let mut node_takes = Vec::new();

        let mut entries = Vec::new();
        let cites = settled.evidence.clone();
        if last {
            // The part closes here: on the public record and the reserve's signed statement, or, at a private rail,
            // on the public movement into the private node and the reserve's statement (`REQ-ORD-33`).
            let closing = if into_private_rail {
                // Only the deposit's own movement, from a public node, is the movement into the rail: a payment into
                // an earlier private node is no movement into this one.
                if shared {
                    return Err(Short::MovementShared);
                }
                let own = movement
                    .as_ref()
                    .filter(|_| self.public(&operation.from.node))
                    .ok_or(Short::NoPublicRecord(why))?;
                public_movement(&own.0, &own.1, Some(node_name(&operation.to.node)))?
            } else if shared {
                return Err(Short::MovementShared);
            } else {
                let (network, id_, _) = movement.as_ref().ok_or(Short::NoPublicRecord(why))?;
                public_movement(network, id_, None)?
            };
            entries.push(evidence(
                cites.clone(),
                settled.segment,
                EvidenceGrade::PublicRecord,
                closing.network.clone(),
                settled.result.clone(),
                Some(closing),
                None,
            ));
            // The reserve's own statement, at the grade its own pieces earn: never the network's record it named.
            let own = graded
                .pieces
                .iter()
                .filter(|p| p.piece.by == settled.provider.as_str())
                .map(|p| p.effective)
                .min()
                .unwrap_or(EvidenceGrade::Statement)
                .max(EvidenceGrade::SignedByProvider);
            entries.push(evidence(
                id(&format!("{}.reserve", cites.as_str()))?,
                settled.segment,
                own,
                settled.provider.clone(),
                settled.result.clone(),
                None,
                None,
            ));
        } else {
            // Every movement Core checked is named, whatever the grade, so the log holds what Core took; a public
            // record is the network's, anything else its provider's.
            let named = movement
                .as_ref()
                .map(|(network, id_, _)| public_movement(network, id_, None))
                .transpose()?;
            let whose = match (&movement, relies) {
                (Some((network, _, _)), true) => id(network)?,
                _ => settled.provider.clone(),
            };
            // Labelled, the movement into the node comes first, naming it: the log's binding of the node the label
            // relies on. The segment takes it now as the node's movement in, which its exit doesn't record again.
            if let (Some(_), Some((network, moved))) = (&label, &moved_in) {
                entries.push(evidence(
                    id(&format!("{}.in.{}", cites.as_str(), settled.place))?,
                    settled.segment,
                    EvidenceGrade::PublicRecord,
                    id(network)?,
                    settled.result.clone(),
                    Some(public_movement(
                        network,
                        moved,
                        Some(node_name(&operation.from.node)),
                    )?),
                    None,
                ));
                node_takes.push((network.clone(), moved.as_str().to_owned(), key.clone()));
            }
            // Inside a private node, a statement its publisher made, through it or a provider of its, is that
            // publisher's word (`REQ-ORD-3`): labelled so, graded a statement.
            let grade = if label.is_some() {
                EvidenceGrade::Statement
            } else {
                graded.grade
            };
            entries.push(evidence(
                cites.clone(),
                settled.segment,
                grade,
                whose,
                settled.result.clone(),
                named,
                label.clone(),
            ));
        }

        // Value entering a private node from a public one: its publisher's record of the node, appended by the
        // segments before this grading, binds the node in the log, in an evidence naming the node whose movement is
        // the one into it (`REQ-ORD-3`, `REQ-ORD-41`, `T-020-32`). The failed segment inside the node never reaches
        // here, so the node is bound when the value enters it. Without a record the log can verify, or without that
        // movement, the node stays unbound.
        if !last && self.public(&operation.from.node) && self.private(&operation.to.node) {
            let node = node_name(&operation.to.node);
            if let (Some((network, moved, _)), Some(record)) = (
                movement.as_ref(),
                self.kept(settled, &format!("enter:{node}")),
            ) {
                entries.push(verified(
                    settled.core,
                    What::Evidence {
                        evidence: id(&format!("{}.node", cites.as_str()))?,
                        order: settled.order.clone(),
                        segment: settled.segment.clone(),
                        grade: EvidenceGrade::PublicRecord,
                        whose: id(network)?,
                        rests_on: vec![settled.result.clone(), record.clone()],
                        movement: Some(public_movement(network, moved, Some(node))?),
                        publisher: None,
                    },
                    settled,
                ));
                node_takes.push((network.clone(), moved.as_str().to_owned(), key.clone()));
            }
        }

        // Value leaving private nodes onto a public one: replay them, and, where the replay is consistent with the
        // movement out, close the segments inside them on the public movements in and out (`REQ-ORD-41`,
        // `REQ-ORD-33`).
        let exits = self.private(&operation.from.node)
            && !self.private(&operation.to.node)
            && side.is_some();
        if exits {
            let (replay, consistent) = self.replayed(settled, operations, movement.as_ref())?;
            entries.extend(replay);
            if let (true, Some((out, out_id, _)), Some((into, into_id))) = (
                consistent,
                movement.as_ref(),
                self.movement_into(settled, operations),
            ) {
                let start = self.run_start(operations, settled.place);
                for (place, op) in operations
                    .iter()
                    .enumerate()
                    .take(settled.place)
                    .skip(start)
                {
                    if !(self.private(&op.from.node) && self.private(&op.to.node)) {
                        continue;
                    }
                    let segment = &settled.segments[place];
                    let Some(seen) = self.seen(settled, segment) else {
                        continue;
                    };
                    let result = seen.result.clone();
                    let node = node_name(&op.from.node);
                    let inner = (
                        settled.order.as_str().to_owned(),
                        segment.as_str().to_owned(),
                    );
                    for (end, network, moved) in [("in", &into, &into_id), ("out", out, out_id)] {
                        // Recorded once: a segment whose label it bound took the movement in when it settled.
                        if end == "in" && self.closing.took_in_node(network, moved.as_str(), &inner)
                        {
                            continue;
                        }
                        node_takes.push((
                            network.clone(),
                            moved.as_str().to_owned(),
                            inner.clone(),
                        ));
                        entries.push(evidence(
                            id(&format!("{}.{end}.{place}", cites.as_str()))?,
                            segment,
                            EvidenceGrade::PublicRecord,
                            id(network)?,
                            result.clone(),
                            Some(public_movement(network, moved, Some(node))?),
                            None,
                        ));
                    }
                }
            }
        }

        // A non-final segment's public record that contradicts its provider: recorded as a mismatch, resting on the
        // provider's result, and raised once the segments have appended it (`REQ-ORD-33`). The segment settles on
        // the grade it earned above, as it would had the network said nothing.
        let mismatch = match contradicted {
            Some(c) => {
                let (Some(node), Some(moved), Some(stated)) = (c.node, c.moved, c.stated) else {
                    return Err(Short::Unreadable);
                };
                entries.push(verified(
                    settled.core,
                    What::PublicMismatch {
                        order: settled.order.clone(),
                        segment: settled.segment.clone(),
                        node,
                        movement: public_movement(&c.network, &c.movement, None)?,
                        found: c.found,
                        moved: moved.clone(),
                        stated: stated.clone(),
                        rests_on: settled.result.clone(),
                    },
                    settled,
                ));
                Some(((c.found, moved, stated), entries.len() - 1))
            }
            None => None,
        };

        // What it took, kept only now that it settles: every movement its records name, as the log takes them.
        if let Some((network, movement, _)) = &movement {
            self.closing
                .take(network, movement.as_str(), &key, into_private_rail);
        }
        for (network, movement, inner) in &node_takes {
            self.closing.take(network, movement, inner, true);
        }
        if let Some((figures, index)) = mismatch {
            self.closing.unraised.expect(
                (settled.order.clone(), settled.segment.clone()),
                figures,
                index,
            );
        }
        self.closing.seen.insert(
            key,
            Seen {
                transaction: settled.transaction.clone(),
                result: settled.result.clone(),
                movement: movement.map(|(n, id, _)| (n, id)),
            },
        );
        Ok(Graded::Settles { cites, entries })
    }

    fn seen(&self, settled: &ToGrade<'_>, segment: &Id) -> Option<&Seen> {
        self.closing.seen.get(&(
            settled.order.as_str().to_owned(),
            segment.as_str().to_owned(),
        ))
    }

    /// Where the run of private nodes the segment at `place` is in began: the place of the first segment that took
    /// the value from a node no publisher declares private, walking back while each segment starts at a private one.
    fn run_start(&self, operations: &[Operation], place: usize) -> usize {
        let mut start = place;
        while start > 0 && self.private(&operations[start].from.node) {
            start -= 1;
        }
        start
    }

    /// The public movement that brought the value into the private nodes the segment leaves: that of the segment
    /// which took it from a public node into a private one, where a final public record evidences it.
    fn movement_into(
        &self,
        settled: &ToGrade<'_>,
        operations: &[Operation],
    ) -> Option<(String, MovementId)> {
        let start = self.run_start(operations, settled.place);
        let entering = &operations[start];
        if start == settled.place
            || !(self.public(&entering.from.node) && self.private(&entering.to.node))
        {
            return None;
        }
        self.seen(settled, &settled.segments[start])?
            .movement
            .clone()
    }

    /// The receipt of the statement kept under `key` for the grading of `settled`, if it was kept.
    fn kept<'s>(&self, settled: &'s ToGrade<'_>, key: &str) -> Option<&'s Receipt> {
        settled
            .kept
            .iter()
            .find(|k| k.key == key)
            .map(|k| &k.receipt)
    }

    /// The private nodes the value is in at `settled`'s segment, in route order, each once, and the transactions that
    /// ran at each.
    fn run_nodes(&self, settled: &ToGrade<'_>, operations: &[Operation]) -> Vec<(String, Vec<Id>)> {
        let start = self.run_start(operations, settled.place);
        let mut nodes: Vec<(String, Vec<Id>)> = Vec::new();
        for (place, op) in operations
            .iter()
            .enumerate()
            .take(settled.place + 1)
            .skip(start)
        {
            let transaction = if place == settled.place {
                Some(settled.transaction.clone())
            } else {
                self.seen(settled, &settled.segments[place])
                    .map(|s| s.transaction.clone())
            };
            for end in [&op.from.node, &op.to.node] {
                if !self.private(end) {
                    continue;
                }
                let name = node_name(end).to_owned();
                if nodes.last().is_none_or(|(n, _)| *n != name) {
                    nodes.push((name, Vec::new()));
                }
                if let (Some((_, txs)), Some(tx)) = (nodes.last_mut(), &transaction)
                    && !txs.contains(tx)
                {
                    txs.push(tx.clone());
                }
            }
        }
        nodes
    }

    /// The private nodes this segment's closing reads the books of: the node a public segment enters, and the nodes
    /// the value leaves for a public one, which the route Core runs names (`REQ-ORD-41`).
    fn nodes_read(&self, settled: &ToGrade<'_>) -> Vec<String> {
        let operations = settled.operations;
        let Some(operation) = operations.get(settled.place) else {
            return Vec::new();
        };
        let mut nodes = Vec::new();
        if !is_final_deposit(settled)
            && self.public(&operation.from.node)
            && self.private(&operation.to.node)
        {
            nodes.push(node_name(&operation.to.node).to_owned());
        }
        if self.private(&operation.from.node)
            && !self.private(&operation.to.node)
            && self.public(&operation.to.node)
        {
            for (node, _) in self.run_nodes(settled, operations) {
                if !nodes.contains(&node) {
                    nodes.push(node);
                }
            }
        }
        nodes
    }

    /// Asks each provider of each private node this segment's closing reads for its record of the node, through the
    /// platform, and hands the books what each answered taken as a record (`REQ-ORD-41`, `T-020-58`). The nodes are
    /// Core's: the ones the route it runs passes. The deployment carries the question and chooses nothing.
    fn ask_publishers(&mut self, settled: &ToGrade<'_>, desk: &mut AskDesk<'_>) {
        for node in self.nodes_read(settled) {
            for provider in publishers::serving(self.catalogue, &node) {
                let record = desk.ask(&provider, &node).and_then(|(question, answer)| {
                    publishers::taken(self.catalogue, desk.now(), &question.statement, &answer).ok()
                });
                self.books.took(&provider, &node, record);
            }
        }
    }

    /// The statements `settled`'s grading will cite, for the segments to append first (`T-020-32`): at a segment that
    /// takes the value into a private node, its publisher's record of the node, which binds it; at one that takes it
    /// out onto a public node, the record of each node replayed. Each is the whole signed `inner-segments` record of
    /// the provider that ran the segment (the exit's own where it served the node, else the first), made of the
    /// operations of this order alone: a record that lists a segment of another order, or one this order didn't run
    /// (a book that spans orders; a reversal Core carried out for the order is the order's own, `T-020-47`), the log would not verify about this order, so it is not appended, and what it
    /// would have bound stays unbound (fail closed). It is about its order and node only.
    fn to_keep(&self, settled: &ToGrade<'_>) -> Vec<ToKeep> {
        let operations = settled.operations;
        let Some(operation) = operations.get(settled.place) else {
            return Vec::new();
        };
        let ours: Vec<&Id> = self
            .closing
            .seen
            .iter()
            .filter(|((order, _), _)| order == settled.order.as_str())
            .map(|(_, seen)| &seen.transaction)
            .chain(std::iter::once(settled.transaction))
            .chain(settled.reversals)
            .collect();
        let choose = |node: &str, transactions: &[Id], must: Option<&Id>| -> Option<ToKeep> {
            let read = self.books.recorded(node, transactions)?;
            let ofs = |r: &&Statement<InnerSegments>| {
                r.statement.node == node
                    && r.statement
                        .segments
                        .iter()
                        .all(|s| ours.iter().any(|t| t.as_str() == s.transaction))
                    && must.is_none_or(|t| {
                        r.statement
                            .segments
                            .iter()
                            .any(|s| s.transaction == t.as_str())
                    })
            };
            let ok: Vec<&Statement<InnerSegments>> = read.records.iter().filter(ofs).collect();
            let record = ok
                .iter()
                .find(|r| r.maker == settled.provider.as_str())
                .or(ok.first())?;
            Some(ToKeep {
                key: String::new(),
                bytes: serde_json::to_vec(record).ok()?,
                about: About {
                    order: Some(settled.order.clone()),
                    segment: None,
                    transaction: None,
                    node: Id::new(node),
                },
            })
        };
        let mut keep = Vec::new();
        if !is_final_deposit(settled)
            && self.public(&operation.from.node)
            && self.private(&operation.to.node)
        {
            let node = node_name(&operation.to.node);
            let tx = [settled.transaction.clone()];
            if let Some(mut one) = choose(node, &tx, Some(settled.transaction)) {
                one.key = format!("enter:{node}");
                keep.push(one);
            }
        }
        if self.private(&operation.from.node)
            && !self.private(&operation.to.node)
            && self.public(&operation.to.node)
        {
            for (node, transactions) in self.run_nodes(settled, operations) {
                if let Some(mut one) = choose(&node, &transactions, None) {
                    one.key = format!("replay:{node}");
                    keep.push(one);
                }
            }
        }
        keep
    }

    /// The `replay` entry for the private nodes the value leaves at this segment, if it records one, and whether it
    /// found the records consistent with the movement out.
    fn replayed(
        &self,
        settled: &ToGrade<'_>,
        operations: &[Operation],
        movement: Option<&Checked>,
    ) -> Result<(Option<Entry>, bool), Short> {
        let nodes = self.run_nodes(settled, operations);
        let Some((last_node, _)) = nodes.last() else {
            return Ok((None, false));
        };
        let last_node = last_node.clone();
        let names = nodes
            .iter()
            .map(|(n, _)| id(n))
            .collect::<Result<Vec<_>, _>>()?;
        let operation = &operations[settled.place];
        let network = node_name(&operation.to.node).to_owned();
        let mut records = Vec::new();
        let mut silent = Vec::new();
        let mut cited: Vec<Receipt> = Vec::new();
        for (node, transactions) in &nodes {
            match self.books.recorded(node, transactions) {
                Some(read) if !read.segments.is_empty() => {
                    // Each node's record that the replay read, cited where it was kept (`REQ-AUD-9`, `T-020-32`).
                    cited.extend(self.kept(settled, &format!("replay:{node}")).cloned());
                    records.push(NodeRecord {
                        node: node.clone(),
                        segments: read.segments,
                    });
                }
                _ => silent.push(id(node)?),
            }
        }
        let moved = self.records.movement(&network, settled.transaction);
        let exit_movement = match (movement, &moved) {
            (Some((n, id_, _)), _) => Some(public_movement(n, id_, None)?),
            (None, Some(moved)) => Some(public_movement(&network, &moved.id, None)?),
            (None, None) => None,
        };
        let found = if !silent.is_empty() {
            Some(ReplayFound::NothingRecorded { nodes: silent })
        } else {
            match (network_of(self.records, &network), moved) {
                (Some(net), Some(moved)) => {
                    let exit = PublicExit {
                        movement: movement.map_or(moved.id, |(_, id, _)| id.clone()),
                        network: network.clone(),
                        destination: if is_final_deposit(settled) {
                            settled.end.destination.account.as_str().to_owned()
                        } else {
                            moved.destination
                        },
                        stated: Stated {
                            asset: operation.to.asset.id.clone(),
                            // Value leaving onto a public node: what arrives, the charge staying with the reserve.
                            amount: settled.arrives.to_owned(),
                        },
                    };
                    found_of(
                        replay_route(&records, &RouteEnd::Public(exit), &net),
                        &last_node,
                    )?
                }
                // No public movement out to compare with: the exit itself is the mismatch.
                _ => Some(ReplayFound::Mismatch {
                    mismatches: vec![ReplayMismatch {
                        node: id(&last_node)?,
                        kind: MismatchKind::PublicExit,
                    }],
                }),
            }
        };
        // A replay that ran no check (the route ends inside the node) records none.
        let Some(found) = found else {
            return Ok((None, false));
        };
        let consistent = found == ReplayFound::ConsistentWithTheExit {};
        Ok((
            Some(verified(
                settled.core,
                What::Replay {
                    order: settled.order.clone(),
                    segment: settled.segment.clone(),
                    nodes: names,
                    found,
                    // The exit's result, then the record of each node it read that the log can verify, one per node.
                    rests_on: std::iter::once(settled.result.clone())
                        .chain(cited)
                        .collect(),
                    movement: exit_movement,
                },
                settled,
            )),
            consistent,
        ))
    }
}

/// What `CMP-EVIDENCE`'s replay found, as the log records it: each mismatch once, against its node, the exit's and
/// the stated outcome's against the last node; `None` where it ran no check, which records none.
fn found_of(replay: Replay, last: &str) -> Result<Option<ReplayFound>, Short> {
    Ok(Some(match replay {
        Replay::Consistent(_) => ReplayFound::ConsistentWithTheExit {},
        Replay::PublishersWord => return Ok(None),
        Replay::Unreplayable(Unreplayable::NothingRecorded(node)) => ReplayFound::NothingRecorded {
            nodes: vec![id(&node)?],
        },
        Replay::Unreplayable(Unreplayable::NoNodes) => ReplayFound::NothingRecorded {
            nodes: vec![id(last)?],
        },
        Replay::Mismatch(found) => {
            let mut mismatches: Vec<ReplayMismatch> = Vec::new();
            for m in found {
                let (node, kind) = match m {
                    Mismatch::Malformed { node, .. } | Mismatch::NoSegments { node } => {
                        (node, MismatchKind::Malformed)
                    }
                    Mismatch::BrokenChain { node, .. } => (node, MismatchKind::BrokenChain),
                    Mismatch::FeeMismatch { node, .. } => (node, MismatchKind::FeeMismatch),
                    Mismatch::HandoffBroken { from, .. } => (from, MismatchKind::HandoffBroken),
                    Mismatch::PublicExit(_) => (last.to_owned(), MismatchKind::PublicExit),
                    Mismatch::StatedOutcome { .. } => {
                        (last.to_owned(), MismatchKind::StatedOutcome)
                    }
                };
                let mismatch = ReplayMismatch {
                    node: id(&node)?,
                    kind,
                };
                if !mismatches.contains(&mismatch) {
                    mismatches.push(mismatch);
                }
            }
            ReplayFound::Mismatch { mismatches }
        }
    }))
}
