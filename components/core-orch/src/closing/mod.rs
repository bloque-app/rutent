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
//!   it is: the network for a public record, the receipt's maker for another party's receipt, and otherwise the
//!   provider, whose word or signature it is.
//! - **A part closes at its final deposit** on a final public record of the deposit's movement to the route's one
//!   destination (its account, its to asset, what the deposit gave out), checked by [`public_record_check`], plus the
//!   reserve's signed statement, its result, which `judge` verified (provider-role's README: the result's envelope
//!   is the reserve's "signed statement for its own book"). Two `evidence` entries say whose each is: the network's,
//!   which `segment_settled` cites, and the reserve's. Without the public record the deposit doesn't settle: a
//!   statement alone closes no order (`REQ-ORD-33`). A deposit to a rail declared private closes on the public
//!   movement into the private node instead, plus the reserve's statement, and records no replay (`REQ-ORD-41`).
//! - **Within a private node** (`REQ-ORD-33`). A segment whose two ends are private settles on its provider's
//!   evidence (private nodes rest on grades 3 and 4 by necessity, ADR-0007); where the value leaves the private nodes
//!   on a final public record, each such segment is recorded as closing on the public movements into and out of the
//!   node, an `evidence` entry for each, whose the network's.
//! - **Replay on the way out** (`REQ-ORD-41`, `DEC-24`). Where a segment takes the value from a private node onto a
//!   public one, the private nodes it passed through since it last left a public node are replayed by
//!   [`replay_route`], each publisher's record of the segments inside its node ([`Books`]) in route order, against
//!   the public movement out and the outcome the provider stated, and a `replay` entry records what was found: a
//!   match as consistency with the exit, never proof, each mismatch with its node, or a node that recorded nothing.
//!   On a route through two private nodes with a public node between them, each is replayed in turn, as the value
//!   leaves it; two adjacent ones are replayed together, as one passes the value to the other. A mismatch is
//!   recorded and stops nothing: replay detects, it doesn't prove (`THR-131`).
//! - **One movement, one segment** (`REQ-ORD-38`). [`Closing`] keeps, across orders, which segment each public
//!   movement evidences. A movement already evidencing another order's segment backs nothing for a second: that
//!   segment is graded on its other evidence, so it is recorded as its provider's word unless that grades higher, and
//!   a final deposit on it doesn't close. Each order keeps its own segment.
//! - **Value rests only in a holding account** (`INV-5`) and **no provider below the floor** (`REQ-ORD-39`):
//!   [`admits`], which the segments call before handing a segment over, holds it to the catalogue as it stands.
//! - **A fee record per settled segment** (`REQ-AUD-11`): [`fees`], attributed to its provider and, on a part's final
//!   deposit, to the operator, whose fee that provider keeps back. Nothing here or in the log sums them: no balance.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::{
    Actor, Entry, FeeTo, MismatchKind, Outcome, Receipt, ReplayFound, ReplayMismatch, TimeSource,
    VerifiedEntry, VerifiedKind, What, When,
};
use rutent_contracts::provider_role::{
    ConversionOutcome, Denomination, EvidenceGrade, EvidencePiece, NodeRef, ReserveMovementOutcome,
    ResultBody, Statement, TransferOutcome,
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

use crate::segments::{Graded, Grading, Short, ToGrade};
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

/// The private nodes' publishers' records of the segments inside their nodes (`REQ-ORD-41`). The contracts give the
/// record no shape (`F-015`'s notes), so `CMP-EVIDENCE`'s [`RecordedSegment`] is it, and the assembly reads it from
/// each publisher.
pub trait Books {
    /// What the publisher of private node `node` recorded of the segments inside it that ran under `transactions`,
    /// in the order they ran; `None` where it recorded nothing.
    fn recorded(&self, node: &str, transactions: &[Id]) -> Option<Vec<RecordedSegment>>;
}

/// What grading kept of one settled segment, for the segments after it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    transaction: Id,
    result: Receipt,
    /// Its public movement, where a final public record of it evidences it: the network and the movement.
    movement: Option<(String, MovementId)>,
}

/// Core's record of the evidence it relied on, across orders: which segment each public movement evidences, and what
/// each settled segment's grading found. The production assembly keeps it durably (`F-030`), as `Segments`: a fresh
/// one knows no movement relied on before it.
#[derive(Debug, Clone, Default)]
pub struct Closing {
    relied: BTreeMap<(String, String), (String, String)>,
    seen: BTreeMap<(String, String), Seen>,
}

impl Closing {
    /// Nothing relied on yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The order and segment the public movement `movement` on `network` evidences, if one does.
    pub fn evidences(&self, network: &str, movement: &str) -> Option<(&str, &str)> {
        self.relied
            .get(&(network.to_owned(), movement.to_owned()))
            .map(|(o, s)| (o.as_str(), s.as_str()))
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
}

impl Unadmitted {
    /// The route guard's refusal, for the operation at `place`.
    pub fn at(self, place: usize) -> RouteRefusal {
        match self {
            Unadmitted::BelowEvidenceFloor => RouteRefusal::BelowEvidenceFloor(place),
            Unadmitted::NoHoldingAccount => RouteRefusal::NoHoldingAccount(place),
        }
    }
}

/// Whether `operation` may be routed through now, by the catalogue as it stands: its provider declares evidence at or
/// above the floor the operator set on its tuple (`REQ-ORD-39`), and, where it isn't the part's `last`, the inner
/// node it leaves the value at has a holding account (`INV-5`, `REQ-CAT-23`). The snapshot a route is chosen from
/// leaves such providers and nodes out; this holds a segment to a floor raised, or a catalogue changed, since.
pub fn admits(catalogue: &Catalogue, operation: &Operation, last: bool) -> Result<(), Unadmitted> {
    let floor = catalogue.evidence_floor(&tuple_of(operation));
    let declared = catalogue
        .declared_by(operation.provider.as_str())
        .and_then(|d| d.evidence.as_ref())
        .map(|e| e.grade);
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
/// arrived. A formula may charge zero, and the record says so. Each is the segment's own: none sums another, and
/// nothing keeps a balance. A result that can't be read for its fee gives no provider's record.
pub fn fees(settled: &ToGrade<'_>) -> Vec<Entry> {
    let Some(operation) = operation_of(settled) else {
        return Vec::new();
    };
    let mut fees = Vec::new();
    let entry = |to: FeeTo, party: &Id, amount: Amount, asset: Denomination| {
        verified(
            settled.core,
            What::Fee {
                order: settled.order.clone(),
                segment: settled.segment.clone(),
                to,
                party: party.clone(),
                amount,
                asset,
                billed_separately: false,
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
        ));
    }
    if is_final_deposit(settled)
        && let Some(arrived) = Amount::new(settled.amount_out.to_owned())
        && let Some(amount) = rutent_router::quote::operator_fee(settled.operator_fee, &arrived)
    {
        fees.push(entry(
            FeeTo::Operator,
            &settled.operator.acts,
            amount,
            operation.to.asset.clone(),
        ));
    }
    fees
}

impl Grading for Evidenced<'_> {
    fn grade(&mut self, settled: &ToGrade<'_>) -> Graded {
        match self.graded(settled) {
            Ok(graded) => graded,
            Err(short) => Graded::Waits(short),
        }
    }
}

/// A settled segment's settlement, as its result states it.
struct Settlement {
    fee: String,
    fee_asset: String,
    pieces: Vec<EvidencePiece>,
}

fn settlement_of(bytes: &[u8]) -> Option<Settlement> {
    let result: Statement<ResultBody> = serde_json::from_slice(bytes).ok()?;
    let (fee, pieces) = match result.statement {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match r.outcome {
            ReserveMovementOutcome::Succeeded(s) => (s.fee, s.evidence),
            _ => return None,
        },
        ResultBody::Transfer(r) => match r.outcome {
            TransferOutcome::Succeeded(s) => (s.fee, s.evidence),
            _ => return None,
        },
        ResultBody::Conversion(r) => match r.outcome {
            ConversionOutcome::Succeeded(s) => (s.fee, s.evidence),
            _ => return None,
        },
        _ => return None,
    };
    Some(Settlement {
        fee: fee.amount,
        fee_asset: fee.asset,
        pieces,
    })
}

fn operations_of<'a>(settled: &ToGrade<'a>) -> Option<&'a [Operation]> {
    Some(settled.operations)
}

fn operation_of<'a>(settled: &ToGrade<'a>) -> Option<&'a Operation> {
    operations_of(settled)?.get(settled.place)
}

/// Whether the segment is its part's last, a deposit: where the part closes.
fn is_final_deposit(settled: &ToGrade<'_>) -> bool {
    operations_of(settled).is_some_and(|ops| {
        settled.place + 1 == ops.len() && ops[settled.place].kind == OperationKind::Deposit
    })
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
        let operations = operations_of(settled).ok_or(Short::Unreadable)?;
        let operation = operations.get(settled.place).ok_or(Short::Unreadable)?;
        let settlement = settlement_of(settled.bytes).ok_or(Short::Unreadable)?;
        let last = is_final_deposit(settled);
        let key = (
            settled.order.as_str().to_owned(),
            settled.segment.as_str().to_owned(),
        );

        // The segment's own public movement: at its public end, where it has one, its destination's first.
        let side = if self.public(&operation.to.node) {
            Some(&operation.to)
        } else if self.public(&operation.from.node) {
            Some(&operation.from)
        } else {
            None
        };
        let mut why = None;
        let mut movement: Option<(String, MovementId, Check)> = None;
        if let Some(side) = side {
            let network = node_name(&side.node).to_owned();
            let moved = self.records.movement(&network, settled.transaction);
            match (network_of(self.records, &network), moved) {
                (Some(net), Some(moved)) => {
                    // A final deposit's movement is held to the route's one destination (`DEC-13`).
                    let expected = Expected {
                        network: network.clone(),
                        destination: if last {
                            settled.end.destination.account.as_str().to_owned()
                        } else {
                            moved.destination.clone()
                        },
                        asset: if last {
                            settled.end.to_asset.id.clone()
                        } else {
                            side.asset.id.clone()
                        },
                        amount: settled.amount_out.to_owned(),
                    };
                    why = verify_movement(&net, &moved.id, &expected).err();
                    let check = public_record_check(&net, &moved.id, &expected);
                    if check.movement().is_some() {
                        movement = Some((network, moved.id, check));
                    }
                }
                (None, _) => why = Some(Unmatched::WrongNetwork),
                (Some(_), None) => why = Some(Unmatched::UnknownMovement),
            }
        }
        // One movement evidences one segment: one another order's segment relies on backs nothing here.
        let shared = movement.as_ref().is_some_and(|(network, id, _)| {
            self.closing
                .relied
                .get(&(network.clone(), id.as_str().to_owned()))
                .is_some_and(|by| *by != key)
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
        let whose = graded
            .pieces
            .iter()
            .find(|p| p.effective == graded.grade && p.effective.is_proof())
            .map_or_else(|| Ok(settled.provider.clone()), |p| id(&p.piece.by))?;
        let evidence = |evidence: Id, segment: &Id, grade, whose: Id, rests_on: Receipt| {
            verified(
                settled.core,
                What::Evidence {
                    evidence,
                    order: settled.order.clone(),
                    segment: segment.clone(),
                    grade,
                    whose,
                    rests_on: vec![rests_on],
                },
                settled,
            )
        };

        let mut entries = Vec::new();
        let cites = settled.evidence.clone();
        let into_private_rail = self.private(&operation.to.node);
        if last {
            // The part closes here: on the public record and the reserve's signed statement, or, at a private rail,
            // on the public movement into the private node and the reserve's statement (`REQ-ORD-33`).
            let (network, _) = if into_private_rail {
                self.movement_into(settled, operations)
                    .ok_or(Short::NoPublicRecord(None))?
            } else if shared {
                return Err(Short::MovementShared);
            } else {
                movement
                    .as_ref()
                    .map(|(n, id, _)| (n.clone(), id.clone()))
                    .ok_or(Short::NoPublicRecord(why))?
            };
            entries.push(evidence(
                cites.clone(),
                settled.segment,
                EvidenceGrade::PublicRecord,
                id(&network)?,
                settled.result.clone(),
            ));
            // The reserve's own statement, at the grade its own pieces earn: never the network's record it named.
            let own = graded
                .pieces
                .iter()
                .filter(|p| p.piece.by == settled.provider.as_str())
                .map(|p| p.effective)
                .min()
                .unwrap_or(EvidenceGrade::Statement);
            entries.push(evidence(
                id(&format!("{}.reserve", cites.as_str()))?,
                settled.segment,
                own,
                settled.provider.clone(),
                settled.result.clone(),
            ));
        } else {
            entries.push(evidence(
                cites.clone(),
                settled.segment,
                graded.grade,
                whose,
                settled.result.clone(),
            ));
        }

        // Value leaving private nodes onto a public one: replay them, and close the segments inside them on the
        // public movements in and out (`REQ-ORD-41`, `REQ-ORD-33`).
        let exits = self.private(&operation.from.node) && !into_private_rail && side.is_some();
        if exits {
            entries.extend(self.replayed(settled, operations, movement.as_ref())?);
            if let (Some((out, _, _)), Some((into, _))) =
                (movement.as_ref(), self.movement_into(settled, operations))
            {
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
                    for (end, network) in [("in", &into), ("out", out)] {
                        entries.push(evidence(
                            id(&format!("{}.{end}.{place}", cites.as_str()))?,
                            segment,
                            EvidenceGrade::PublicRecord,
                            id(network)?,
                            result.clone(),
                        ));
                    }
                }
            }
        }

        // What it relied on, kept only now that it settles.
        if let Some((network, movement, _)) = &movement {
            self.closing
                .relied
                .insert((network.clone(), movement.as_str().to_owned()), key.clone());
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
        if !(self.public(&entering.from.node) && self.private(&entering.to.node)) {
            return None;
        }
        self.seen(settled, &settled.segments[start])?
            .movement
            .clone()
    }

    /// The `replay` entry for the private nodes the value leaves at this segment.
    fn replayed(
        &self,
        settled: &ToGrade<'_>,
        operations: &[Operation],
        movement: Option<&(String, MovementId, Check)>,
    ) -> Result<Vec<Entry>, Short> {
        let start = self.run_start(operations, settled.place);
        // The private nodes, in route order, each once, and the segments that ran at each.
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
        let Some((last_node, _)) = nodes.last() else {
            return Ok(Vec::new());
        };
        let last_node = last_node.clone();
        let names = nodes
            .iter()
            .map(|(n, _)| id(n))
            .collect::<Result<Vec<_>, _>>()?;
        let operation = &operations[settled.place];
        let network = node_name(&operation.to.node).to_owned();
        let mut records = Vec::new();
        let mut found = None;
        for (node, transactions) in &nodes {
            match self.books.recorded(node, transactions) {
                Some(segments) if !segments.is_empty() => records.push(NodeRecord {
                    node: node.clone(),
                    segments,
                }),
                _ => {
                    found = Some(ReplayFound::NothingRecorded { node: id(node)? });
                    break;
                }
            }
        }
        let moved = self.records.movement(&network, settled.transaction);
        let found = match (found, network_of(self.records, &network), moved) {
            (Some(found), _, _) => found,
            (None, Some(net), Some(moved)) => {
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
                        amount: settled.amount_out.to_owned(),
                    },
                };
                found_of(
                    replay_route(&records, &RouteEnd::Public(exit), &net),
                    &last_node,
                )?
            }
            // No public movement out to compare with: the exit itself is the mismatch.
            (None, _, _) => ReplayFound::Mismatch {
                mismatches: vec![ReplayMismatch {
                    node: id(&last_node)?,
                    kind: MismatchKind::PublicExit,
                }],
            },
        };
        Ok(vec![verified(
            settled.core,
            What::Replay {
                order: settled.order.clone(),
                segment: settled.segment.clone(),
                nodes: names,
                found,
            },
            settled,
        )])
    }
}

/// What `CMP-EVIDENCE`'s replay found, as the log records it: each mismatch once, against its node, the exit's and
/// the stated outcome's against the last node.
fn found_of(replay: Replay, last: &str) -> Result<ReplayFound, Short> {
    Ok(match replay {
        Replay::Consistent(_) => ReplayFound::ConsistentWithTheExit {},
        Replay::PublishersWord => ReplayFound::ConsistentWithTheExit {},
        Replay::Unreplayable(Unreplayable::NothingRecorded(node)) => {
            ReplayFound::NothingRecorded { node: id(&node)? }
        }
        Replay::Unreplayable(Unreplayable::NoNodes) => {
            ReplayFound::NothingRecorded { node: id(last)? }
        }
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
    })
}
