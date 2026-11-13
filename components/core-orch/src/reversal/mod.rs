//! Reversals and ending.
//!
//! Declared by `T-020-01`; `T-020-12` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! [`Reversals`] is Core's record of the reversals segments owe and how each ended, which the production assembly
//! keeps durably (`F-030`), as it does [`Segments`]. Every entry is built once and appended as built until the log
//! takes it, so a call that stopped carries on from where it stopped, asking no one twice and recording nothing
//! twice (`INV-8`); a receipt is taken only where it is its own entry's.
//!
//! - **Owed, fed to the catalogue's list** ([`Reversals::owe`], `REQ-CAT-7`, `F-012`'s notes). A segment that failed
//!   with its reversal owed, by the order Core follows (no deposit: a failed deposit leaves its value where it was),
//!   is owed by its provider, for what it took in and in its asset, as Core signed it ([`Segments::sent`]). The
//!   catalogue's list keeps it; silence, lost standing and expiry never discharge it, and only a completed reversal
//!   does ([`Reversals::confirm`]).
//! - **Complete only on another party's statement** ([`Reversals::confirm`], `REQ-ORD-3`, ADR-0008). Core asks the
//!   origin reserve, the provider of the part's withdrawal, whether the credit arrived: a `reversal_confirmation`
//!   question naming the reversed segment's transaction, the asset it left in and what it took in, under a fresh
//!   transaction, signed with the operator key and appended about the order and segment before it is sent, its answer
//!   appended under that transaction. The answer counts as `judge`'s docs say: not the failed provider's own word,
//!   nor its publisher's, unless the failed provider is the reserve itself, whose own statement then counts, labelled
//!   its word; in full and in its asset. What counts is an `evidence` entry saying whose it is, and then the
//!   `reversal_completed` input citing it. Once the log took it, the order Core follows takes it, the segment's hold
//!   is released, the owed reversal discharged, and the part's value is evidenced at the segment's start again, so it
//!   can be recalculated (`T-020-10`). An answer that doesn't count is kept and completes nothing, and the next call
//!   asks again.
//! - **Kept as it arrives** ([`Reversals::report`], `REQ-ORD-30`). A statement about a reversal or about an undone
//!   starting segment that answers no question of Core's (the failed provider's own, a piece of an undone start) is
//!   kept and appended as its maker's, about the order and segment, each once, and says why it completes nothing. The
//!   log verifies only an answer under the transaction Core asked it under, so nothing completes on one.
//! - **Asked of the failed provider** ([`Reversals::ask`], `REQ-ORD-3`, `REQ-ORD-11`). Policy decides `ask_reversal`;
//!   on its permit Core records the effect and carries out provider-role's `reversal` under a fresh transaction. The
//!   answer is the failed provider's own word, which completes nothing, except inside a private node its own
//!   publisher holds and the log binds as that publisher's ([`Reversals::follow`]): there it is the publisher's word,
//!   recorded with the `evidence`'s `publisher` label, graded a statement (`REQ-ORD-3`'s third sentence).
//! - **Who carries it** ([`Reversals::show_fault`], [`Reversals::accept_fault`], [`Reversals::dispute`],
//!   `REQ-ORD-11`). The failed provider carries it unless it shows the fault was the customer's or the operator's: its
//!   failed result naming the fault (provider-role's `Failure.fault`), kept as its result statement, and the party it
//!   names accepting it by its act: the customer's signed fault acceptance in its open session, or the operator's
//!   `accept_fault` command. The `reversal_carrier` records it, resting on both. A dispute over it, by the party it
//!   names, declares the part stuck through [`Stuck::dispute`], so it is resolved in the stuck order's two steps.
//! - **Ending** ([`Reversals::end`], `REQ-ORD-12`). An order ends without completing only until its first segment
//!   settles, and never while a segment of a part that would end moves value: the `end_requested` input, once the
//!   state machine takes it. After that, a part ends only when its start is proven undone ([`Reversals::confirm`] on
//!   the part's withdrawal): the origin reserve's own word, labelled so, and the part, and so the order, is marked
//!   failed (`REQ-STM-6`). A part still open is undone only once none of its value is in motion: no later segment
//!   handed over without a final outcome, nor failed owing its reversal (`part-waits-for-value-in-motion`). The loss
//!   of an undone start is the origin reserve's.
//! - **After the end** ([`Reversals::returned`], [`Reversals::late_approval`], `REQ-ORD-31`, `REQ-ORD-25`). A
//!   complete order's destination reporting the payment returned, and an origin approval arriving after its order
//!   ended, are each kept as their maker's statement and recorded as `afterwards`, moving nothing: the order stays
//!   as it ended, and nothing starts. They are what [`Reversals::seen`] shows its reader, on a read policy allows,
//!   and what `F-023` notifies.
//! - **A late report** ([`Reversals::later`], `REQ-STM-6`, `AC-5`). A provider's result about a segment Core handed
//!   over, arriving after the segment's answer was taken, is kept and appended as its maker's statement about the
//!   segment and its transaction, and moves nothing: the segments took the segment's one answer, and a stuck part's
//!   value moves only on its resolution, which may cite it.
//!
//! **Residuals.** The log never checks that the node a publisher's word is about is the one its provider's publisher
//! holds: Core does, from the catalogue's holding accounts. A reserve's signed confirmation of its own publisher's
//! provider's reversal can't carry the label the log takes only on a statement, so it completes nothing. Each is named
//! in `F-020`'s notes.

mod journal;
mod judge;

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, Reads};
use rutent_contracts::core_internal::{
    About, Action, Actor, Afterwards, Answer, Carrier as Carries, DecisionRequest, Effect, Entry,
    FAULT_ACCEPTANCE_SHAPE, FaultAcceptance, OperatorCommand, Outcome, Permit, ReadCheck,
    ReadOutcome, Receipt, Role, VerifiedKind, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    ConversionOutcome, EvidenceGrade, OPERATION_SHAPE, Operation as ProviderOperation,
    RESULT_SHAPE, ReserveMovementOutcome, ResultBody, Reversal as ReversalOperation,
    ReversalConfirmationRequest, ReversalOutcome, Signature, SignatureScheme, Statement,
    TransferOutcome, signing_input,
};
use rutent_contracts::router_api::{Id, PolicyBounds};
use rutent_contracts::state_machine::{
    self as stm, Observed, OperationKind, OrderState, RecordedInput, SegmentState, Source,
    SourceKind, Standing, StateMachine, Subject,
};
use rutent_prov::catalogue::{Catalogue, Observation, OwedError};
use rutent_stm::Stm;

use crate::rounds::{Fresh, Issued, OperatorKey, Providers};
use crate::segments::{Segments, StatusError};
use crate::snapshot::{Clock, Exposures, OrchestrationLog};
use crate::stuck::{self, Stuck, StuckError};

use journal::Step;
pub use judge::NotCounted;
use judge::{Reversed, Whose};

/// What the reversals reach. The assembly builds it; Core's clock, segments and exposures are those the rest of Core
/// uses, and the catalogue the one whose list of owed reversals it feeds.
pub struct Parts<'a> {
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// The catalogue: statements are attributed by its keys and publishers, and it keeps the reversals owed.
    pub catalogue: &'a mut Catalogue,
    /// Orchestration's handle on the log.
    pub log: &'a mut dyn OrchestrationLog,
    /// The evidence store.
    pub evidence: &'a mut dyn Evidence,
    /// Fresh identifiers: records, evidence and questions' transactions.
    pub fresh: &'a mut dyn Fresh,
    /// The transactions the routing rounds asked under: no question's is one of them.
    pub issued: &'a Issued,
    /// The segments Core handed over, and the orders it follows.
    pub segments: &'a mut Segments,
    /// Core's record of value in flight: a completed reversal releases its segment's hold.
    pub exposures: &'a mut Exposures,
    /// Core, which writes its own entries.
    pub core: &'a Actor,
    /// The operator, in whose name Core signs its questions: its party is the one a module the operator publishes
    /// states for.
    pub operator: &'a Actor,
}

/// A segment of a part of an order.
#[derive(Debug, Clone, Copy)]
pub struct At<'a> {
    /// The order.
    pub order: &'a Id,
    /// The part.
    pub part: &'a Id,
    /// The segment.
    pub segment: &'a Id,
}

/// What Core asks with: the order's bounds, whose deployment and protocol version a question is made for, the
/// operator key that signs it, and the providers' modules it is sent to.
pub struct Asking<'a> {
    /// The bounds of the order's chosen record.
    pub bounds: &'a PolicyBounds,
    /// The operator key.
    pub key: &'a mut dyn OperatorKey,
    /// The providers' modules.
    pub providers: &'a mut dyn Providers,
}

/// What a reversal Core asks of a failed provider moves through: policy, which decides it, and the platform, which
/// carries it out.
pub struct Moving<'a> {
    /// Policy's one decision call.
    pub decisions: &'a mut dyn rutent_contracts::core_internal::calls::Decisions,
    /// The module platform.
    pub modules: &'a mut dyn crate::segments::Modules,
}

/// A failed provider asked to reverse its segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The receipt of the `ask_reversal` effect.
    pub effect: Receipt,
    /// The reversal's operation, as Core signed it.
    pub operation: Statement<ProviderOperation>,
    /// The receipt of the provider's answer's stated entry, where it answered.
    pub answer: Option<Receipt>,
    /// The reversal, where the answer completed it.
    pub completed: Option<Completed>,
    /// Why the answer completed nothing, where it didn't.
    pub why: Option<NotCounted>,
}

/// Whose fault a failed provider shows a failure was (`REQ-ORD-11`): provider-role's, read from its failed result.
pub use rutent_contracts::provider_role::Fault;

/// The act of the party at fault accepting a provider's showing (`reversal-carrier-rests-on-its-acceptance`).
#[derive(Debug, Clone, Copy)]
pub enum Accepting<'a> {
    /// The customer: its own signed fault acceptance (`FAULT_ACCEPTANCE_SHAPE`), given in its open session.
    Customer {
        /// The customer, in the customer's role.
        by: &'a Actor,
        /// Its signed fault acceptance, as received.
        statement: &'a [u8],
        /// The session it was given in.
        session: &'a Id,
    },
    /// The operator, by its `accept_fault` command, which Core records in its name.
    Operator {
        /// The operator, in the operator's role.
        by: &'a Actor,
    },
}
/// Who carries a reversal (`REQ-ORD-11`, `REQ-ORD-30`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Carrier {
    /// The failed provider: the default.
    Provider(Id),
    /// The customer, on the provider's showing, which the customer accepted.
    Customer,
    /// The operator, on the provider's showing, which the operator accepted.
    Operator,
    /// The origin reserve, which carries the loss of an undone start (`REQ-ORD-30`).
    OriginReserve(Id),
}

/// A reversal owed, as the catalogue's list keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owing {
    /// Its identifier in the catalogue's list.
    pub id: String,
    /// The provider that owes it.
    pub provider: Id,
    /// The transaction the reversed segment ran under.
    pub transaction: Id,
    /// The asset it is owed in: the one the segment took in.
    pub asset: String,
    /// What is owed: what the segment took in, in full.
    pub amount: String,
}

/// A reversal completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completed {
    /// The evidence its `reversal_completed` cites.
    pub evidence: Id,
    /// Whose word it is.
    pub whose: Id,
    /// Its grade.
    pub grade: EvidenceGrade,
    /// Whose word it is labelled: the reserve's, or its publisher's inside the publisher's own private node.
    pub word: Word,
    /// The receipt of the statement it rests on.
    pub statement: Receipt,
    /// The receipt of the `reversal_completed` input.
    pub input: Receipt,
}

/// Whose word a completed reversal rests on (`REQ-ORD-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Word {
    /// The origin reserve's own.
    Reserves,
    /// The publisher's, inside its own private node: the origin reserve and the failed provider are both of it.
    Publishers(Id),
}

/// How Core's question about a reversal came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmation {
    /// The reversal is complete.
    Completed(Completed),
    /// The answer completes nothing: kept, with why. The next call asks again.
    NotCompleted {
        /// The receipt of the answer's stated entry, where something came back.
        statement: Option<Receipt>,
        /// Why it completes nothing.
        why: NotCounted,
    },
}

/// A statement kept as it arrived, and why it completes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    /// The receipt of its stated entry.
    pub statement: Receipt,
    /// Why it completes nothing.
    pub why: NotCounted,
}

/// Something recorded after an order ended (`REQ-ORD-25`, `REQ-ORD-31`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Afterward {
    /// The order.
    pub order: Id,
    /// What came.
    pub event: Afterwards,
    /// Who reported it.
    pub maker: Id,
    /// The receipt of its stated entry.
    pub statement: Receipt,
    /// The receipt of the `afterwards` entry.
    pub receipt: Receipt,
}

/// Why a reversal or an ending wasn't recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReversalError {
    /// Core follows no such order, part or segment, or handed no such segment over.
    Unknown,
    /// The segment owes no reversal: it didn't fail with its reversal owed, or it is a deposit, or it isn't the
    /// complete part's settled withdrawal.
    NotOwed,
    /// The part has no withdrawal whose reserve is its origin reserve.
    NoOriginReserve,
    /// The segment doesn't start at the node the part's withdrawal took the value to, where its origin reserve
    /// holds it: no reserve Core can ask shows the value back at the segment's origin node (`REQ-ORD-3`). Nothing is
    /// asked or recorded; the reversal stays owed.
    NoReserveAtNode,
    /// The state machine wouldn't take the input from the order as it stands: nothing is recorded.
    NotApplicable(stm::Outcome),
    /// A segment of a part that would end moves value: it is handed over with no final outcome.
    InMotion(Id),
    /// The order isn't complete.
    NotComplete,
    /// The order hasn't ended.
    NotEnded,
    /// The statement isn't the failed provider's.
    NotTheProvider,
    /// The statement isn't the destination reserve's.
    NotTheDestination,
    /// The statement isn't the origin reserve's.
    NotTheOriginReserve,
    /// The actor isn't of the party the provider's showing names.
    NotTheParty,
    /// No showing of fault waits for an answer.
    NoShowing,
    /// The statement shows no fault: it isn't a failed result of the segment's transaction naming the customer's or
    /// the operator's.
    NoFaultShown,
    /// A showing of fault was accepted already: the carrier stands.
    FaultSettled,
    /// The statement isn't the customer's fault acceptance of this showing: its shape, its maker, its order,
    /// segment, transaction, showing or fault.
    NotAnAcceptance,
    /// Policy didn't permit the reversal to be asked: nothing was recorded as an effect.
    NotPermitted(Answer),
    /// The effect was acted on already (`INV-8`).
    AlreadyActed(Receipt),
    /// A dispute of the showing waits on the log, kept by the stuck orders to append: the showing is answered by it
    /// once it is retried to completion, and accepted by no one before.
    DisputePending,
    /// The segment's answer wasn't taken yet: nothing it gives back is late.
    NotAnswered,
    /// The statement's signature doesn't verify under a key its maker published for statements.
    Unattributable,
    /// The statement can't be kept: it names no shape, maker or time a stated entry can carry.
    Unrecordable,
    /// No question could be written or signed.
    Unsigned,
    /// The source of fresh identifiers repeats ones Core used.
    Fresh,
    /// The catalogue's list refused.
    Owed(OwedError),
    /// The log or the evidence store refused or failed an append. One the log can't take now is kept, and the next
    /// call appends it as built.
    Append(AppendFailure),
    /// The stuck orders couldn't declare the part.
    Stuck(StuckError),
    /// An identifier Core holds isn't one the state machine's input can carry.
    Id(&'static str),
}

impl From<AppendFailure> for ReversalError {
    fn from(failure: AppendFailure) -> Self {
        ReversalError::Append(failure)
    }
}

/// Core's question to the origin reserve, kept from when it is built until its answer is judged.
#[derive(Debug, Clone)]
struct Question {
    transaction: Id,
    origin: Id,
    bytes: Vec<u8>,
    question: Step,
    sent: bool,
    answer: Option<Vec<u8>>,
    answer_step: Option<Step>,
}

/// A completion built, kept until the log took each of its entries.
#[derive(Debug, Clone)]
struct Completion {
    evidence_id: Id,
    whose: Whose,
    statement: Receipt,
    evidence: Step,
    input: Step,
    recorded: RecordedInput,
}

/// A failed provider's showing of fault, and how it was answered.
#[derive(Debug, Clone)]
struct Showing {
    fault: Fault,
    /// The receipt of its stated entry.
    receipt: Receipt,
    /// The commitment its stated entry names it by.
    commitment: Digest,
    /// The transaction it is about: the failed segment's own.
    transaction: Id,
    disputed: bool,
    /// A dispute of it the log couldn't take now, kept by the stuck orders to append: until it is retried to
    /// completion, no one accepts the showing, or the kept declaration would make the part stuck over a fault
    /// accepted.
    pending: bool,
}

/// One reversal: owed, or of an undone start.
#[derive(Debug, Clone)]
struct Reversal {
    owing: Option<Owing>,
    carrier: Carrier,
    question: Option<Question>,
    completion: Option<Completion>,
    completed: Option<Completed>,
    /// The statements kept as they arrived, each once, by their bytes.
    reports: Statements,
    /// The failed provider's showings of fault, each once, by their bytes.
    showings: Statements,
    /// An acceptance of the showing, built: its act and its carrier entry, kept until the log takes each.
    accepting: Option<Accepted>,
    /// Core's asking the failed provider to reverse, kept from its permit until its answer is judged.
    asking: Option<AskState>,
    showing: Option<Showing>,
}

/// Statements kept as they arrived, each once, by their bytes.
type Statements = Vec<(Vec<u8>, Step)>;

/// An acceptance of a showing, built: its act (the customer's statement or the operator's command), its carrier
/// entry and the carrier it records, each appended once.
#[derive(Debug, Clone)]
struct Accepted {
    by: Actor,
    /// The customer's statement, as received.
    statement: Option<Vec<u8>>,
    act: Step,
    session: Option<Id>,
    carrier: Option<Step>,
    carries: Carrier,
}

/// Core's asking the failed provider to reverse its segment, from the decision on.
#[derive(Debug, Clone)]
struct AskState {
    permit: Permit,
    action: Action,
    transaction: Id,
    statement: Statement<ProviderOperation>,
    question: Step,
    effect: Step,
    carried: bool,
    answer: Option<Vec<u8>>,
    answer_step: Option<Step>,
    refused: bool,
    result: Option<Step>,
}

/// Something after an order ended, built once.
#[derive(Debug, Clone)]
struct After {
    bytes: Vec<u8>,
    event: Afterwards,
    maker: Id,
    statement: Step,
    entry: Option<Step>,
}

/// Core's record of reversals and endings, kept across orders by the assembly, which the production assembly keeps
/// durably (`F-030`), as it does [`Segments`].
#[derive(Debug, Clone, Default)]
pub struct Reversals {
    reversals: BTreeMap<(String, String), Reversal>,
    /// Each order's requester's party, from its acceptance in the log ([`Reversals::follow`]).
    requesters: BTreeMap<String, Id>,
    /// Each order's private nodes the log binds to their publisher ([`Reversals::follow`]), by node.
    bound: BTreeMap<String, BTreeMap<String, Id>>,
    ends: BTreeMap<String, (Step, RecordedInput)>,
    afterwards: BTreeMap<String, Vec<After>>,
    later: BTreeMap<(String, String), Statements>,
}

impl Reversals {
    /// Nothing owed yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Follows `order` from the log's entries, as the log holds them: the party its acceptance names as requester,
    /// whose acceptance of a fault is the customer's (`reversal-carrier-is-accepted-by-the-party-at-fault`). False
    /// where they hold no acceptance of it.
    ///
    /// It also reads the private nodes the log binds as their publisher's for the order (`reversal-rests-on-its-evidence`,
    /// field `private`): evidence of the order whose movement names the node, or a replay of the order naming it,
    /// resting on a verified statement about the order stating for the publisher. Called again as the log grows.
    pub fn follow(&mut self, order: &Id, history: &[(Receipt, Entry)]) -> bool {
        fn verified(e: &Entry) -> Option<&rutent_contracts::core_internal::VerifiedEntry> {
            match e {
                Entry::Verified(v) => Some(v),
                Entry::Stated(_) => None,
            }
        }
        let party_of = |receipt: &Receipt| {
            history
                .iter()
                .find(|(r, _)| r == receipt)
                .and_then(|(_, e)| match e {
                    Entry::Stated(s)
                        if matches!(
                            s.attribution,
                            rutent_contracts::core_internal::Attribution::Verified { .. }
                        ) && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order) =>
                    {
                        Some(s.attribution.maker().party.clone())
                    }
                    _ => None,
                })
        };
        let mut bound = BTreeMap::new();
        let mut requester = None;
        for (_, entry) in history {
            let Some(v) = verified(entry) else { continue };
            match &v.what {
                What::Acceptance {
                    order: o, parties, ..
                } if o == order => requester = Some(parties.requester.clone()),
                What::Evidence {
                    order: o,
                    movement: Some(movement),
                    rests_on,
                    ..
                } if o == order => {
                    if let Some(node) = &movement.node {
                        for party in rests_on.iter().filter_map(party_of) {
                            bound.insert(node.as_str().to_owned(), party);
                        }
                    }
                }
                What::Replay {
                    order: o,
                    nodes,
                    rests_on,
                    ..
                } if o == order => {
                    if let Some(party) = party_of(rests_on) {
                        for node in nodes {
                            bound.insert(node.as_str().to_owned(), party.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        self.bound.insert(order.as_str().to_owned(), bound);
        match requester {
            Some(requester) => {
                self.requesters.insert(order.as_str().to_owned(), requester);
                true
            }
            None => false,
        }
    }

    /// Asks the failed provider of `at` to reverse it (`ask_reversal`, `REQ-ORD-3`, `REQ-ORD-11`): policy decides
    /// the action, and only its permit goes on; the reversal's operation, under a fresh transaction, names the
    /// segment's transaction, the node it took the value from, the asset and what it took in, signed and appended
    /// about the order, segment and transaction, then the effect on the permit, acted on once in the segments' one
    /// register, carried out on the platform; the answer is kept under that transaction and the effect's result
    /// recorded. The answer is the failed provider's own word, which completes nothing, unless the segment starts
    /// in a private node the provider's own publisher holds and the log binds as that publisher's: there it is the
    /// publisher's word and completes the reversal, labelled (`REQ-ORD-3`'s third sentence). A call that stopped
    /// carries on from where it stopped; one whose answer was judged asks again.
    pub fn ask(
        &mut self,
        parts: &mut Parts<'_>,
        asking: &mut Asking<'_>,
        moving: &mut Moving<'_>,
        at: At<'_>,
    ) -> Result<Asked, ReversalError> {
        let target = self.target(parts, at)?;
        let owing = self
            .owing(at.order, at.segment)
            .cloned()
            .ok_or(ReversalError::NotOwed)?;
        let node = target.node.clone().ok_or(ReversalError::NoReserveAtNode)?;
        let at_key = key(at.order, at.segment);
        if self
            .reversals
            .get(&at_key)
            .is_none_or(|r| r.asking.is_none())
        {
            let action = Action::AskReversal {
                order: at.order.clone(),
                segment: at.segment.clone(),
                provider: owing.provider.clone(),
            };
            let request = DecisionRequest {
                request: parts.fresh.id(),
                caller: parts.core.clone(),
                action: action.clone(),
                at: journal::now(parts.clock)?,
            };
            let decision = moving.decisions.decide(&request)?;
            let Some(permit) = decision.permit() else {
                return Err(ReversalError::NotPermitted(decision.outcome));
            };
            let transaction = parts
                .segments
                .claim_transaction(parts.fresh, parts.issued)
                .map_err(|_| ReversalError::Fresh)?;
            let operation = ProviderOperation::Reversal(ReversalOperation {
                transaction: transaction.as_str().to_owned(),
                provider: owing.provider.as_str().to_owned(),
                reverses: owing.transaction.as_str().to_owned(),
                node,
                asset: owing.asset.clone(),
                amount: owing.amount.clone(),
            });
            let bytes = sign(parts, asking, operation)?;
            let statement: Statement<ProviderOperation> =
                serde_json::from_slice(&bytes).map_err(|_| ReversalError::Unsigned)?;
            let about = About {
                order: Some(at.order.clone()),
                segment: Some(at.segment.clone()),
                transaction: Some(transaction.clone()),
            };
            let question = Step::new(journal::stated(
                parts.evidence,
                parts.clock,
                &bytes,
                parts.operator.clone(),
                about,
            )?);
            let effect = Step::new(journal::recorded(
                parts.clock,
                parts.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                Outcome::ToTakeEffect {},
            )?);
            self.reversal(at, &owing.provider).asking = Some(AskState {
                permit,
                action,
                transaction,
                statement,
                question,
                effect,
                carried: false,
                answer: None,
                answer_step: None,
                refused: false,
                result: None,
            });
        }
        let bound = self
            .bound
            .get(at.order.as_str())
            .and_then(|b| target.node.as_ref().and_then(|n| b.get(n)))
            .map(|p| p.as_str().to_owned());
        let reversal = self
            .reversals
            .get_mut(&at_key)
            .ok_or(ReversalError::Unknown)?;
        let state = reversal.asking.as_mut().ok_or(ReversalError::Unknown)?;
        // The operation, then the effect, before anything takes effect (`INV-7`).
        state.question.append(parts.log)?;
        let effect_receipt = state.effect.append(parts.log)?;
        let effect = Effect {
            action: state.action.clone(),
            decision: state.permit.clone(),
            receipt: effect_receipt.clone(),
        };
        if !state.carried {
            if !parts.segments.act_on(&effect_receipt) {
                return Err(ReversalError::AlreadyActed(effect_receipt));
            }
            match moving
                .modules
                .carry_out(&effect, &owing.provider, &state.statement)
            {
                crate::segments::Answered::Bytes(bytes) => state.answer = Some(bytes),
                crate::segments::Answered::Refused(refusal) => {
                    state.refused = refusal
                        != rutent_contracts::module_interface::PlatformRefusal::OutcomeUnknown;
                }
            }
            state.carried = true;
        }
        if let (Some(answer), None) = (&state.answer, &state.answer_step) {
            let about = About {
                order: Some(at.order.clone()),
                segment: Some(at.segment.clone()),
                transaction: Some(state.transaction.clone()),
            };
            let maker = journal::provider_maker(parts.catalogue, parts.operator, &owing.provider);
            match journal::stated(parts.evidence, parts.clock, answer, maker, about) {
                Ok(entry) => state.answer_step = Some(Step::new(entry)),
                Err(ReversalError::Unrecordable) => state.answer = None,
                Err(other) => return Err(other),
            }
        }
        let answer_receipt = match state.answer_step.as_mut() {
            Some(step) => Some(step.append(parts.log)?),
            None => None,
        };
        let judged = match &state.answer {
            Some(answer) => judge::judge_reversal(
                parts.catalogue,
                parts.clock.now().floor(),
                &target.reversed(),
                &state.transaction,
                bound.as_deref(),
                answer,
            ),
            None => Err(NotCounted::Unanswered),
        };
        // The effect's one result: done where the provider says it reversed, failed where it refused or the platform
        // refused before its module passed anything out, unanswered otherwise.
        if state.result.is_none() {
            let said = state
                .answer
                .as_ref()
                .and_then(|a| serde_json::from_slice::<Statement<ResultBody>>(a).ok())
                .and_then(|r| match r.statement {
                    ResultBody::Reversal(r) => Some(r.outcome),
                    _ => None,
                });
            let outcome = match said {
                Some(ReversalOutcome::Succeeded(_)) => Outcome::Done {},
                Some(ReversalOutcome::Refused(_)) => Outcome::Failed {},
                _ if state.refused => Outcome::Failed {},
                _ => Outcome::Unanswered {},
            };
            state.result = Some(Step::new(journal::recorded(
                parts.clock,
                parts.core,
                What::Result {
                    effect: effect_receipt.clone(),
                },
                outcome,
            )?));
        }
        state
            .result
            .as_mut()
            .ok_or(ReversalError::Unknown)?
            .append(parts.log)?;
        let operation = state.statement.clone();
        reversal.asking = None;
        let (completed, why) = match judged {
            Ok(whose) => {
                let statement = answer_receipt.clone().ok_or(ReversalError::Unknown)?;
                let completion = Self::completion(parts, at, whose, statement)?;
                self.reversals
                    .get_mut(&at_key)
                    .ok_or(ReversalError::Unknown)?
                    .completion = Some(completion);
                (Some(self.complete(parts, at)?), None)
            }
            Err(why) => (None, Some(why)),
        };
        Ok(Asked {
            effect: effect_receipt,
            operation,
            answer: answer_receipt,
            completed,
            why,
        })
    }

    /// The reversal `segment` of `order` owes, once Core recorded it owed.
    pub fn owing(&self, order: &Id, segment: &Id) -> Option<&Owing> {
        self.reversals
            .get(&key(order, segment))
            .and_then(|r| r.owing.as_ref())
    }

    /// Who carries the reversal of `segment` of `order` (`REQ-ORD-11`, `REQ-ORD-30`).
    pub fn carrier(&self, order: &Id, segment: &Id) -> Option<&Carrier> {
        self.reversals.get(&key(order, segment)).map(|r| &r.carrier)
    }

    /// The reversal of `segment` of `order`, once complete.
    pub fn completed(&self, order: &Id, segment: &Id) -> Option<&Completed> {
        self.reversals
            .get(&key(order, segment))
            .and_then(|r| r.completed.as_ref())
    }

    /// Records the reversal `at` owes in the catalogue's list (see the module's docs): once, by its provider, for what
    /// it took in and in its asset. The same segment again answers what was recorded.
    pub fn owe(&mut self, parts: &mut Parts<'_>, at: At<'_>) -> Result<Owing, ReversalError> {
        if let Some(owing) = self.owing(at.order, at.segment) {
            return Ok(owing.clone());
        }
        let followed = parts
            .segments
            .order(at.order)
            .ok_or(ReversalError::Unknown)?;
        let (_, segment) = find(followed, at)?;
        if segment.state != SegmentState::FailedReversalOwed
            || segment.kind == OperationKind::Deposit
        {
            return Err(ReversalError::NotOwed);
        }
        let (transaction, operation) = parts
            .segments
            .sent(at.order, at.segment)
            .ok_or(ReversalError::Unknown)?;
        let (provider, asset, amount) = moved(operation).ok_or(ReversalError::NotOwed)?;
        let owing = Owing {
            id: format!("{}/{}", at.order.as_str(), at.segment.as_str()),
            provider: Id::new(provider).ok_or(ReversalError::Id("a provider"))?,
            transaction: transaction.clone(),
            asset: asset.to_owned(),
            amount: amount.to_owned(),
        };
        match parts.catalogue.owed_mut().owe(
            &owing.id,
            owing.provider.as_str(),
            &owing.asset,
            amount.parse().map_err(|_| ReversalError::Id("an amount"))?,
        ) {
            Ok(()) | Err(OwedError::DuplicateOwed(_)) => {}
            Err(other) => return Err(ReversalError::Owed(other)),
        }
        let reversal = self.reversal(at, &owing.provider);
        reversal.owing = Some(owing.clone());
        Ok(owing)
    }

    /// Asks the origin reserve whether the credit of `at`'s reversal arrived, and completes the reversal on its answer
    /// where it counts (see the module's docs). `at` is a segment that owes its reversal, recorded owed first if it
    /// isn't yet, or the settled withdrawal of a complete part, whose undoing marks the part failed.
    pub fn confirm(
        &mut self,
        parts: &mut Parts<'_>,
        asking: &mut Asking<'_>,
        at: At<'_>,
    ) -> Result<Confirmation, ReversalError> {
        if let Some(done) = self.completed(at.order, at.segment) {
            return Ok(Confirmation::Completed(done.clone()));
        }
        let target = self.target(parts, at)?;
        let at_key = key(at.order, at.segment);
        // A completion built already is appended as built, its question spent.
        if self
            .reversals
            .get(&at_key)
            .is_some_and(|r| r.completion.is_some())
        {
            return self.complete(parts, at).map(Confirmation::Completed);
        }
        if self
            .reversals
            .get(&at_key)
            .is_none_or(|r| r.question.is_none())
        {
            let question = self.question(parts, asking, at, &target)?;
            self.reversal(at, &target.provider).question = Some(question);
        }
        let reversal = self
            .reversals
            .get_mut(&at_key)
            .ok_or(ReversalError::Unknown)?;
        let question = reversal.question.as_mut().ok_or(ReversalError::Unknown)?;
        // Appended before it is sent, so the log binds its answer to the order and segment before the answer exists.
        question.question.append(parts.log)?;
        if !question.sent {
            question.answer = asking.providers.ask(&question.origin, &question.bytes);
            question.sent = true;
        }
        let Some(answer) = question.answer.clone() else {
            reversal.question = None;
            return Ok(Confirmation::NotCompleted {
                statement: None,
                why: NotCounted::Unanswered,
            });
        };
        if question.answer_step.is_none() {
            let maker = journal::provider_maker(parts.catalogue, parts.operator, &question.origin);
            let about = About {
                order: Some(at.order.clone()),
                segment: Some(at.segment.clone()),
                transaction: Some(question.transaction.clone()),
            };
            match journal::stated(parts.evidence, parts.clock, &answer, maker, about) {
                Ok(entry) => question.answer_step = Some(Step::new(entry)),
                Err(ReversalError::Unrecordable) => {
                    // Nothing can be kept of it: the question is spent, and the next call asks again.
                    reversal.question = None;
                    return Ok(Confirmation::NotCompleted {
                        statement: None,
                        why: NotCounted::Unreadable,
                    });
                }
                Err(other) => return Err(other),
            }
        }
        let statement = question
            .answer_step
            .as_mut()
            .ok_or(ReversalError::Unknown)?
            .append(parts.log)?;
        let transaction = question.transaction.clone();
        let reversed = target.reversed();
        let judged = judge::judge(
            parts.catalogue,
            parts.clock.now().floor(),
            &reversed,
            Some(&transaction),
            &answer,
        );
        let whose = match judged {
            Ok(whose) => whose,
            Err(why) => {
                // Kept, and spent: the next call asks again, under a fresh transaction.
                reversal.question = None;
                return Ok(Confirmation::NotCompleted {
                    statement: Some(statement),
                    why,
                });
            }
        };
        reversal.question = None;
        let completion = Self::completion(parts, at, whose, statement)?;
        self.reversals
            .get_mut(&at_key)
            .ok_or(ReversalError::Unknown)?
            .completion = Some(completion);
        self.complete(parts, at).map(Confirmation::Completed)
    }

    /// Keeps `bytes`, a statement about `at`'s reversal or its undone start that answers no question of Core's, as
    /// its maker's stated entry about the order and segment, once, and says why it completes nothing (see the
    /// module's docs).
    pub fn report(
        &mut self,
        parts: &mut Parts<'_>,
        at: At<'_>,
        bytes: &[u8],
    ) -> Result<Reported, ReversalError> {
        let target = self.target(parts, at)?;
        let maker = journal::maker_named(bytes).ok_or(ReversalError::Unrecordable)?;
        let reversal = self.reversal(at, &target.provider);
        if !reversal.reports.iter().any(|(kept, _)| kept == bytes) {
            let claimed = journal::provider_maker(parts.catalogue, parts.operator, &maker);
            let about = About {
                order: Some(at.order.clone()),
                segment: Some(at.segment.clone()),
                transaction: None,
            };
            let step = Step::new(journal::stated(
                parts.evidence,
                parts.clock,
                bytes,
                claimed,
                about,
            )?);
            reversal.reports.push((bytes.to_vec(), step));
        }
        let (_, step) = reversal
            .reports
            .iter_mut()
            .find(|(kept, _)| kept == bytes)
            .ok_or(ReversalError::Unknown)?;
        let statement = step.append(parts.log)?;
        let why = match judge::judge(
            parts.catalogue,
            parts.clock.now().floor(),
            &target.reversed(),
            None,
            bytes,
        ) {
            Err(why) => why,
            Ok(_) => NotCounted::Unasked,
        };
        Ok(Reported { statement, why })
    }

    /// Keeps the failed provider's showing that the failure of `at` was the customer's or the operator's: its signed
    /// result for the segment's own transaction, verified as the provider's, whose `Failure.fault` names the fault,
    /// read from those bytes and never from the caller, appended as its result statement about the
    /// order, the segment and that transaction. Until the party it names accepts it, the provider carries the
    /// reversal (`REQ-ORD-11`). A later showing replaces one not yet answered; once one is accepted or disputed, the
    /// carrier stands and no other is taken (`FaultSettled`).
    pub fn show_fault(
        &mut self,
        parts: &mut Parts<'_>,
        at: At<'_>,
        bytes: &[u8],
    ) -> Result<Receipt, ReversalError> {
        let owing = self.owe(parts, at)?;
        let reversal = self.reversal(at, &owing.provider);
        if !matches!(reversal.carrier, Carrier::Provider(_))
            || reversal.showing.as_ref().is_some_and(|s| s.disputed)
        {
            return Err(ReversalError::FaultSettled);
        }
        if reversal.showing.as_ref().is_some_and(|s| s.pending) {
            return Err(ReversalError::DisputePending);
        }
        parts
            .catalogue
            .verify_statement(bytes, parts.clock.now().floor())
            .map_err(|_| ReversalError::Unattributable)?;
        if journal::maker_named(bytes).as_ref() != Some(&owing.provider) {
            return Err(ReversalError::NotTheProvider);
        }
        let fault = fault_shown(bytes, &owing.transaction).ok_or(ReversalError::NoFaultShown)?;
        let about = About {
            order: Some(at.order.clone()),
            segment: Some(at.segment.clone()),
            transaction: Some(owing.transaction.clone()),
        };
        let claimed = journal::provider_maker(parts.catalogue, parts.operator, &owing.provider);
        let reversal = self.reversal(at, &owing.provider);
        if !reversal.showings.iter().any(|(kept, _)| kept == bytes) {
            let step = Step::new(journal::stated(
                parts.evidence,
                parts.clock,
                bytes,
                claimed,
                about,
            )?);
            reversal.showings.push((bytes.to_vec(), step));
        }
        let (_, step) = reversal
            .showings
            .iter_mut()
            .find(|(kept, _)| kept == bytes)
            .ok_or(ReversalError::Unknown)?;
        let receipt = step.append(parts.log)?;
        let Entry::Stated(kept) = step.entry.entry() else {
            return Err(ReversalError::Unrecordable);
        };
        let commitment = kept.commitment.clone();
        reversal.showing = Some(Showing {
            fault,
            receipt: receipt.clone(),
            commitment,
            transaction: owing.transaction.clone(),
            disputed: false,
            pending: false,
        });
        reversal.accepting = None;
        Ok(receipt)
    }

    /// The party the provider's showing names, accepting it by its act (see [`Accepting`]): it carries the reversal
    /// from then (`REQ-ORD-11`). The act and a `reversal_carrier` resting on it and on the showing are built once and
    /// appended until the log takes each; Core's carrier changes only once the log took the carrier. The customer is
    /// the order's requester's party, as the order's acceptance in the log names it ([`Reversals::follow`]), and its
    /// statement is its fault acceptance of this showing; the operator is the deployment operator's party. That the
    /// customer's session is open is policy's register's, and the log holds it (`reversal-carrier-rests-on-its-acceptance`).
    pub fn accept_fault(
        &mut self,
        parts: &mut Parts<'_>,
        at: At<'_>,
        accepting: Accepting<'_>,
    ) -> Result<Carrier, ReversalError> {
        let requester = self.requesters.get(at.order.as_str()).cloned();
        let owed = parts
            .segments
            .order(at.order)
            .and_then(|o| find(o, at).ok())
            .is_some_and(|(_, s)| s.state == SegmentState::FailedReversalOwed);
        let reversal = self
            .reversals
            .get_mut(&key(at.order, at.segment))
            .ok_or(ReversalError::NoShowing)?;
        let showing = reversal
            .showing
            .as_ref()
            .filter(|s| !s.disputed)
            .ok_or(ReversalError::NoShowing)?
            .clone();
        if showing.pending {
            return Err(ReversalError::DisputePending);
        }
        if !owed {
            return Err(ReversalError::NotOwed);
        }
        let by = match accepting {
            Accepting::Customer { by, .. } | Accepting::Operator { by } => by,
        };
        let carries = party_of(showing.fault, by)?;
        let party_ok = match carries {
            Carrier::Customer => requester.as_ref() == Some(&by.party),
            Carrier::Operator => by.party == parts.operator.party,
            _ => false,
        };
        if !party_ok {
            return Err(ReversalError::NotTheParty);
        }
        // A kept acceptance by another actor, or of another statement, nothing of it taken yet, gives way to this one.
        let statement = match accepting {
            Accepting::Customer { statement, .. } => Some(statement),
            Accepting::Operator { .. } => None,
        };
        if reversal.accepting.as_ref().is_some_and(|a| {
            a.act.receipt.is_none() && (a.by != *by || a.statement.as_deref() != statement)
        }) {
            reversal.accepting = None;
        }
        if reversal.accepting.is_none() {
            let (act, session) = match accepting {
                Accepting::Customer {
                    by,
                    statement,
                    session,
                } => {
                    let accepted: Statement<FaultAcceptance> = serde_json::from_slice(statement)
                        .map_err(|_| ReversalError::NotAnAcceptance)?;
                    let a = &accepted.statement;
                    if accepted.shape != FAULT_ACCEPTANCE_SHAPE
                        || accepted.maker != by.acts.as_str()
                        || a.order != *at.order
                        || a.segment != *at.segment
                        || a.transaction != showing.transaction
                        || a.showing != showing.commitment
                        || a.fault != Fault::Customer
                    {
                        return Err(ReversalError::NotAnAcceptance);
                    }
                    let about = About {
                        order: Some(at.order.clone()),
                        segment: Some(at.segment.clone()),
                        transaction: Some(showing.transaction.clone()),
                    };
                    let act =
                        journal::stated(parts.evidence, parts.clock, statement, by.clone(), about)?;
                    (act, Some(session.clone()))
                }
                Accepting::Operator { by } => {
                    let act = journal::recorded(
                        parts.clock,
                        by,
                        What::Command {
                            command: OperatorCommand::AcceptFault,
                            order: Some(at.order.clone()),
                            subject: Some(at.segment.clone()),
                        },
                        Outcome::Done {},
                    )?;
                    (act, None)
                }
            };
            reversal.accepting = Some(Accepted {
                by: by.clone(),
                statement: statement.map(<[u8]>::to_vec),
                act: Step::new(act),
                session,
                carrier: None,
                carries: carries.clone(),
            });
        }
        let accepted = reversal.accepting.as_mut().ok_or(ReversalError::Unknown)?;
        let acted = match accepted.act.append(parts.log) {
            Ok(receipt) => receipt,
            Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
            Err(refused) => {
                reversal.accepting = None;
                return Err(refused.into());
            }
        };
        if accepted.carrier.is_none() {
            let carrier = match accepted.carries {
                Carrier::Customer => Carries::Customer,
                _ => Carries::Operator,
            };
            accepted.carrier = Some(Step::new(journal::verified(
                parts.clock,
                parts.core,
                What::ReversalCarrier {
                    order: at.order.clone(),
                    segment: at.segment.clone(),
                    carrier,
                    showing: Some(showing.receipt.clone()),
                    accepted_by: Some(accepted.by.clone()),
                    accepted_with: Some(acted),
                    session: accepted.session.clone(),
                },
            )?));
        }
        let step = accepted.carrier.as_mut().ok_or(ReversalError::Unknown)?;
        match step.append(parts.log) {
            Ok(_) => {}
            Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
            Err(refused) => {
                reversal.accepting = None;
                return Err(refused.into());
            }
        }
        let carries = accepted.carries.clone();
        reversal.carrier = carries.clone();
        reversal.showing = None;
        reversal.accepting = None;
        Ok(carries)
    }

    /// The party the provider's showing names, disputing it: the part is declared stuck, `dispute`, through the stuck
    /// orders, and resolved in their two steps (`REQ-ORD-11`, `REQ-AUZ-4`). The provider carries the reversal until
    /// then.
    pub fn dispute(
        &mut self,
        parts: &mut Parts<'_>,
        stuck: &mut Stuck,
        at: At<'_>,
        by: &Actor,
    ) -> Result<Receipt, ReversalError> {
        let reversal = self
            .reversals
            .get_mut(&key(at.order, at.segment))
            .ok_or(ReversalError::NoShowing)?;
        let showing = reversal.showing.as_ref().ok_or(ReversalError::NoShowing)?;
        if showing.disputed {
            return Err(ReversalError::FaultSettled);
        }
        party_of(showing.fault, by)?;
        let mut stuck_parts = stuck::Parts {
            clock: parts.clock,
            log: &mut *parts.log,
            fresh: &mut *parts.fresh,
            issued: parts.issued,
            segments: &mut *parts.segments,
            exposures: &mut *parts.exposures,
        };
        let declared = stuck.dispute(&mut stuck_parts, at.order, at.part, parts.core);
        if let Some(showing) = self
            .reversals
            .get_mut(&key(at.order, at.segment))
            .and_then(|r| r.showing.as_mut())
        {
            match &declared {
                Ok(_) => {
                    showing.disputed = true;
                    showing.pending = false;
                }
                // Kept by the stuck orders, to append as built: the dispute stands, waiting on the log.
                Err(StuckError::Append(AppendFailure::Unavailable)) => showing.pending = true,
                // Refused, nothing kept: the showing stays open.
                Err(_) => showing.pending = false,
            }
        }
        Ok(declared.map_err(ReversalError::Stuck)?.receipt)
    }

    /// Ends `order` without completing, on its customer's request (see the module's docs): only while a part of it
    /// hasn't passed its first segment, and none of such a part's segments moves value. The `end_requested` input is
    /// built once and appended until the log takes it; then the order Core follows takes it, and no segment of an
    /// ended part is handed over ([`crate::segments::SegmentsError::Ended`]).
    pub fn end(&mut self, parts: &mut Parts<'_>, order: &Id) -> Result<Receipt, ReversalError> {
        if !self.ends.contains_key(order.as_str()) {
            if let Some(moving) = in_motion(parts.segments, order)? {
                return Err(ReversalError::InMotion(moving));
            }
            let input = input(parts, order, None, None, Observed::EndRequested {})?;
            let entry = journal::verified(
                parts.clock,
                parts.core,
                What::Input {
                    input: input.clone(),
                },
            )?;
            self.ends
                .insert(order.as_str().to_owned(), (Step::new(entry), input));
        }
        self.record_end(parts, order)
    }

    /// Records that `order`, complete, was returned: `bytes`, its destination reserve's report, kept as its stated
    /// entry about the order, and an `afterwards` of `returned` resting on it. The order stays complete
    /// (`REQ-ORD-31`).
    pub fn returned(
        &mut self,
        parts: &mut Parts<'_>,
        order: &Id,
        bytes: &[u8],
    ) -> Result<Afterward, ReversalError> {
        let followed = parts.segments.order(order).ok_or(ReversalError::Unknown)?;
        if followed.state != OrderState::Complete {
            return Err(ReversalError::NotComplete);
        }
        let destinations: Vec<Id> = followed
            .parts
            .iter()
            .filter_map(|p| p.segments.last())
            .filter(|s| s.kind == OperationKind::Deposit)
            .filter_map(|s| Id::new(s.provider.as_str()))
            .collect();
        self.after(
            parts,
            order,
            bytes,
            Afterwards::Returned,
            &destinations,
            ReversalError::NotTheDestination,
        )
    }

    /// Records an origin approval that arrived after `order` ended: `bytes`, its origin reserve's statement, kept as
    /// its stated entry about the order, and an `afterwards` of `late_approval` resting on it. It starts nothing
    /// (`REQ-ORD-25`).
    pub fn late_approval(
        &mut self,
        parts: &mut Parts<'_>,
        order: &Id,
        bytes: &[u8],
    ) -> Result<Afterward, ReversalError> {
        let followed = parts.segments.order(order).ok_or(ReversalError::Unknown)?;
        if !stm::rules::is_ended(followed.state) {
            return Err(ReversalError::NotEnded);
        }
        let origins: Vec<Id> = followed
            .parts
            .iter()
            .filter_map(|p| origin_of(p).ok())
            .collect();
        self.after(
            parts,
            order,
            bytes,
            Afterwards::LateApproval,
            &origins,
            ReversalError::NotTheOriginReserve,
        )
    }

    /// What came after `order` ended, for `reader`, once policy allows its read of the order; the check is recorded
    /// either way (`REQ-ORD-25`, `REQ-AUZ-7`).
    pub fn seen(
        &self,
        reads: &mut dyn Reads,
        clock: &dyn Clock,
        fresh: &mut dyn Fresh,
        reader: &Actor,
        permission: Option<&Id>,
        order: &Id,
    ) -> Result<Vec<Afterward>, StatusError> {
        let at = journal::now(clock).map_err(StatusError::Append)?;
        let answer = reads
            .check_read(&ReadCheck {
                request: fresh.id(),
                reader: reader.clone(),
                reads: rutent_contracts::core_internal::Reads::Order {
                    order: order.clone(),
                },
                permission: permission.cloned(),
                at,
            })
            .map_err(StatusError::Append)?;
        if !matches!(answer.outcome, ReadOutcome::Allowed {}) {
            return Err(StatusError::Refused(answer));
        }
        Ok(self
            .afterwards
            .get(order.as_str())
            .into_iter()
            .flatten()
            .filter_map(|a| {
                Some(Afterward {
                    order: order.clone(),
                    event: a.event,
                    maker: a.maker.clone(),
                    statement: a.statement.receipt.clone()?,
                    receipt: a.entry.as_ref()?.receipt.clone()?,
                })
            })
            .collect())
    }

    /// Keeps `bytes`, a provider's result about `at` arriving after the segment's answer was taken, as its maker's
    /// stated entry about the segment and, where it names it, the transaction the segment ran under; once, moving
    /// nothing (`REQ-STM-6`, `AC-5`).
    pub fn later(
        &mut self,
        parts: &mut Parts<'_>,
        at: At<'_>,
        bytes: &[u8],
    ) -> Result<Receipt, ReversalError> {
        let (transaction, _) = parts
            .segments
            .sent(at.order, at.segment)
            .ok_or(ReversalError::Unknown)?;
        let transaction = transaction.clone();
        if !parts.segments.answered(at.order, at.segment) {
            return Err(ReversalError::NotAnswered);
        }
        let kept = self.later.entry(key(at.order, at.segment)).or_default();
        if !kept.iter().any(|(b, _)| b == bytes) {
            let maker = journal::maker_named(bytes).ok_or(ReversalError::Unrecordable)?;
            let names = serde_json::from_slice::<Statement<ResultBody>>(bytes)
                .ok()
                .and_then(|s| serde_json::to_value(&s.statement).ok())
                .is_some_and(|body| body["transaction"] == transaction.as_str());
            let about = About {
                order: Some(at.order.clone()),
                segment: Some(at.segment.clone()),
                transaction: names.then_some(transaction),
            };
            let claimed = journal::provider_maker(parts.catalogue, parts.operator, &maker);
            let step = Step::new(journal::stated(
                parts.evidence,
                parts.clock,
                bytes,
                claimed,
                about,
            )?);
            kept.push((bytes.to_vec(), step));
        }
        let (_, step) = kept
            .iter_mut()
            .find(|(b, _)| b == bytes)
            .ok_or(ReversalError::Unknown)?;
        Ok(step.append(parts.log)?)
    }

    // ---------------------------------------------------------------------------------------------------------------

    /// The reversal of `at`, made if it isn't kept yet, its provider carrying it.
    fn reversal(&mut self, at: At<'_>, provider: &Id) -> &mut Reversal {
        self.reversals
            .entry(key(at.order, at.segment))
            .or_insert_with(|| Reversal {
                owing: None,
                carrier: Carrier::Provider(provider.clone()),
                question: None,
                completion: None,
                completed: None,
                reports: Vec::new(),
                showings: Vec::new(),
                accepting: None,
                asking: None,
                showing: None,
            })
    }

    /// What `at` reverses, from Core's records: a segment that owes its reversal, recorded owed first, or the settled
    /// withdrawal of a complete part.
    fn target(&mut self, parts: &mut Parts<'_>, at: At<'_>) -> Result<Target, ReversalError> {
        let followed = parts
            .segments
            .order(at.order)
            .ok_or(ReversalError::Unknown)?;
        let (part, segment) = find(followed, at)?;
        let origin = origin_of(part)?;
        let start = part.segments.first().is_some_and(|s| {
            s.segment_id.as_str() == at.segment.as_str() && s.kind == OperationKind::Withdrawal
        });
        let (start, failed) = (
            start && segment.state == SegmentState::Settled && !stm::rules::is_ended(part.state),
            segment.state == SegmentState::FailedReversalOwed,
        );
        let start = start || {
            // A complete part's start too: complete is an ending the state machine lets an undone start leave.
            part.state == OrderState::Complete
                && segment.state == SegmentState::Settled
                && part.segments.first().is_some_and(|s| {
                    s.segment_id.as_str() == at.segment.as_str()
                        && s.kind == OperationKind::Withdrawal
                })
        };
        let provider = Id::new(segment.provider.as_str()).ok_or(ReversalError::Id("a provider"))?;
        let node = inner_node(segment);
        let at_node = part
            .segments
            .first()
            .is_some_and(|withdrawal| starts_where_withdrawn(withdrawal, segment));
        if start {
            // An open part's start is undone only once no value of the part is in motion
            // (`part-waits-for-value-in-motion`): a later segment handed over, or failed owing its reversal.
            if part.state != OrderState::Complete
                && let Some(moving) = part_in_motion(parts.segments, at.order, part)?
            {
                return Err(ReversalError::InMotion(moving));
            }
            let (transaction, operation) = parts
                .segments
                .sent(at.order, at.segment)
                .ok_or(ReversalError::Unknown)?;
            let (_, asset, amount) = moved(operation).ok_or(ReversalError::NotOwed)?;
            let target = Target {
                provider: provider.clone(),
                node: inner_node(segment),
                origin,
                transaction: transaction.clone(),
                asset: asset.to_owned(),
                amount: amount.to_owned(),
            };
            let reversal = self.reversal(at, &provider);
            if reversal.completion.is_none() && reversal.completed.is_none() {
                reversal.carrier = Carrier::OriginReserve(target.origin.clone());
            }
            return Ok(target);
        }
        let done = self.completed(at.order, at.segment).is_some()
            || self
                .reversals
                .get(&key(at.order, at.segment))
                .is_some_and(|r| r.completion.is_some());
        if !failed && !done {
            return Err(ReversalError::NotOwed);
        }
        // The origin reserve can show the value back only at its own node: the one the withdrawal took it to.
        if !at_node {
            return Err(ReversalError::NoReserveAtNode);
        }
        let owing = match self.owing(at.order, at.segment) {
            Some(owing) => owing.clone(),
            None => self.owe(parts, at)?,
        };
        Ok(Target {
            provider: owing.provider,
            node,
            origin,
            transaction: owing.transaction,
            asset: owing.asset,
            amount: owing.amount,
        })
    }

    /// Core's question to the origin reserve about `target`'s credit, built and kept: signed, its bytes kept, its
    /// stated entry built about the order, the segment and the question's own fresh transaction.
    fn question(
        &mut self,
        parts: &mut Parts<'_>,
        asking: &mut Asking<'_>,
        at: At<'_>,
        target: &Target,
    ) -> Result<Question, ReversalError> {
        let transaction = parts
            .segments
            .claim_transaction(parts.fresh, parts.issued)
            .map_err(|_| ReversalError::Fresh)?;
        let operation = ProviderOperation::ReversalConfirmation(ReversalConfirmationRequest {
            transaction: transaction.as_str().to_owned(),
            provider: target.origin.as_str().to_owned(),
            segment: target.transaction.as_str().to_owned(),
            asset: target.asset.clone(),
            amount: target.amount.clone(),
        });
        let bytes = sign(parts, asking, operation)?;
        let about = About {
            order: Some(at.order.clone()),
            segment: Some(at.segment.clone()),
            transaction: Some(transaction.clone()),
        };
        let question = Step::new(journal::stated(
            parts.evidence,
            parts.clock,
            &bytes,
            parts.operator.clone(),
            about,
        )?);
        Ok(Question {
            transaction,
            origin: target.origin.clone(),
            bytes,
            question,
            sent: false,
            answer: None,
            answer_step: None,
        })
    }

    /// The completion of `at`'s reversal on `whose` word, at `statement`: its `evidence` and its `reversal_completed`,
    /// built once, the input checked against the order as it stands.
    fn completion(
        parts: &mut Parts<'_>,
        at: At<'_>,
        whose: Whose,
        statement: Receipt,
    ) -> Result<Completion, ReversalError> {
        let evidence_id = parts.fresh.id();
        let evidence = journal::verified(
            parts.clock,
            parts.core,
            What::Evidence {
                evidence: evidence_id.clone(),
                order: at.order.clone(),
                segment: at.segment.clone(),
                grade: whose.grade,
                whose: whose.maker.clone(),
                rests_on: vec![statement.clone()],
                movement: None,
                publisher: match &whose.word {
                    Word::Publishers(publisher) => Some(publisher.clone()),
                    Word::Reserves => None,
                },
            },
        )?;
        let observed = Observed::ReversalCompleted {
            evidence: stm::Id::new(evidence_id.as_str()).ok_or(ReversalError::Id("an evidence"))?,
        };
        let recorded = input(parts, at.order, Some(at.part), Some(at.segment), observed)?;
        let input = journal::verified(
            parts.clock,
            parts.core,
            What::Input {
                input: recorded.clone(),
            },
        )?;
        Ok(Completion {
            evidence_id,
            whose,
            statement,
            evidence: Step::new(evidence),
            input: Step::new(input),
            recorded,
        })
    }

    /// Appends a completion built already, each entry once; before its input is appended the order is read again,
    /// and one the state machine no longer takes is dropped. Once the log took it: the order Core follows takes it,
    /// the segment's hold is released and the owed reversal discharged.
    fn complete(&mut self, parts: &mut Parts<'_>, at: At<'_>) -> Result<Completed, ReversalError> {
        let at_key = key(at.order, at.segment);
        let reversal = self
            .reversals
            .get_mut(&at_key)
            .ok_or(ReversalError::Unknown)?;
        let completion = reversal.completion.as_mut().ok_or(ReversalError::Unknown)?;
        if completion.input.receipt.is_none() {
            let followed = parts
                .segments
                .order(at.order)
                .ok_or(ReversalError::Unknown)?;
            let after = Stm::transition(followed, &completion.recorded);
            if after.last_input.outcome != (stm::Outcome::Applied {}) {
                reversal.completion = None;
                return Err(ReversalError::NotApplicable(after.last_input.outcome));
            }
        }
        completion.evidence.append(parts.log)?;
        let input = completion.input.append(parts.log)?;
        let done = Completed {
            evidence: completion.evidence_id.clone(),
            whose: completion.whose.maker.clone(),
            grade: completion.whose.grade,
            word: completion.whose.word.clone(),
            statement: completion.statement.clone(),
            input,
        };
        let recorded = completion.recorded.clone();
        reversal.completion = None;
        reversal.completed = Some(done.clone());
        parts.segments.observe(parts.clock, &recorded);
        parts.exposures.release(at.order, at.segment);
        if let Some(owing) = &reversal.owing {
            parts
                .catalogue
                .owed_mut()
                .observe(&owing.id, Observation::Confirmed)
                .map_err(ReversalError::Owed)?;
        }
        Ok(done)
    }

    /// Appends `order`'s `end_requested` as built: read again first, and dropped where the state machine no longer
    /// takes it.
    fn record_end(&mut self, parts: &mut Parts<'_>, order: &Id) -> Result<Receipt, ReversalError> {
        let (step, recorded) = self
            .ends
            .get_mut(order.as_str())
            .ok_or(ReversalError::Unknown)?;
        if step.receipt.is_none() {
            // Read again before every append, a retry's included: a withdrawal handed over since the request was
            // kept is in motion now, and the request is dropped.
            if let Some(moving) = in_motion(parts.segments, order)? {
                self.ends.remove(order.as_str());
                return Err(ReversalError::InMotion(moving));
            }
            let followed = parts.segments.order(order).ok_or(ReversalError::Unknown)?;
            let after = Stm::transition(followed, recorded);
            if after.last_input.outcome != (stm::Outcome::Applied {}) {
                self.ends.remove(order.as_str());
                return Err(ReversalError::NotApplicable(after.last_input.outcome));
            }
        }
        let receipt = match step.append(parts.log) {
            Ok(receipt) => receipt,
            Err(AppendFailure::Unavailable) => return Err(AppendFailure::Unavailable.into()),
            Err(refused) => {
                self.ends.remove(order.as_str());
                return Err(refused.into());
            }
        };
        let recorded = recorded.clone();
        parts.segments.observe(parts.clock, &recorded);
        Ok(receipt)
    }

    /// Keeps `bytes` as its maker's statement about `order`, a maker among `makers` (`refused` otherwise), and
    /// records an `afterwards` of `event` resting on it, each once.
    fn after(
        &mut self,
        parts: &mut Parts<'_>,
        order: &Id,
        bytes: &[u8],
        event: Afterwards,
        makers: &[Id],
        refused: ReversalError,
    ) -> Result<Afterward, ReversalError> {
        let kept = self
            .afterwards
            .entry(order.as_str().to_owned())
            .or_default();
        if !kept.iter().any(|a| a.bytes == bytes && a.event == event) {
            parts
                .catalogue
                .verify_statement(bytes, parts.clock.now().floor())
                .map_err(|_| ReversalError::Unattributable)?;
            let maker = journal::maker_named(bytes).ok_or(ReversalError::Unrecordable)?;
            if !makers.contains(&maker) {
                return Err(refused);
            }
            let claimed = journal::provider_maker(parts.catalogue, parts.operator, &maker);
            let about = About {
                order: Some(order.clone()),
                segment: None,
                transaction: None,
            };
            let statement = Step::new(journal::stated(
                parts.evidence,
                parts.clock,
                bytes,
                claimed,
                about,
            )?);
            kept.push(After {
                bytes: bytes.to_vec(),
                event,
                maker,
                statement,
                entry: None,
            });
        }
        let after = kept
            .iter_mut()
            .find(|a| a.bytes == bytes && a.event == event)
            .ok_or(ReversalError::Unknown)?;
        let statement = after.statement.append(parts.log)?;
        if after.entry.is_none() {
            after.entry = Some(Step::new(journal::verified(
                parts.clock,
                parts.core,
                What::Afterwards {
                    order: order.clone(),
                    event,
                    statement: statement.clone(),
                },
            )?));
        }
        let receipt = after
            .entry
            .as_mut()
            .ok_or(ReversalError::Unknown)?
            .append(parts.log)?;
        Ok(Afterward {
            order: order.clone(),
            event,
            maker: after.maker.clone(),
            statement,
            receipt,
        })
    }
}

/// What a reversal returns, from Core's records.
#[derive(Debug, Clone)]
struct Target {
    provider: Id,
    /// The inner node the segment starts at, where the value is to be back.
    node: Option<String>,
    origin: Id,
    transaction: Id,
    asset: String,
    amount: String,
}

impl Target {
    fn reversed(&self) -> Reversed<'_> {
        Reversed {
            provider: &self.provider,
            node: self.node.as_deref(),
            origin: &self.origin,
            transaction: &self.transaction,
            asset: &self.asset,
            amount: &self.amount,
        }
    }
}

/// The first segment of `order` that moves value, of a part that hasn't passed its first segment: handed over with no
/// final outcome. Such a part doesn't end while it does.
fn in_motion(segments: &Segments, order: &Id) -> Result<Option<Id>, ReversalError> {
    let followed = segments.order(order).ok_or(ReversalError::Unknown)?;
    for part in followed
        .parts
        .iter()
        .filter(|p| p.state == OrderState::NotPastFirstSegment)
    {
        for segment in &part.segments {
            let segment =
                Id::new(segment.segment_id.as_str()).ok_or(ReversalError::Id("a segment"))?;
            if segments.handed_over(order, &segment) {
                return Ok(Some(segment));
            }
        }
    }
    Ok(None)
}

/// The first segment of `part` of `order` whose value is in motion: handed over with no final outcome, or failed
/// owing its reversal, its value disputed with its provider until the reversal completes.
fn part_in_motion(
    segments: &Segments,
    order: &Id,
    part: &stm::Part,
) -> Result<Option<Id>, ReversalError> {
    for segment in &part.segments {
        let id = Id::new(segment.segment_id.as_str()).ok_or(ReversalError::Id("a segment"))?;
        let owes = segment.state == SegmentState::FailedReversalOwed
            && segment.kind != OperationKind::Deposit;
        if owes || segments.handed_over(order, &id) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// The part and segment `at` names in the order Core follows.
fn find<'o>(
    followed: &'o stm::Order,
    at: At<'_>,
) -> Result<(&'o stm::Part, &'o stm::Segment), ReversalError> {
    let part = followed
        .parts
        .iter()
        .find(|p| p.part_id.as_str() == at.part.as_str())
        .ok_or(ReversalError::Unknown)?;
    let segment = part
        .segments
        .iter()
        .find(|s| s.segment_id.as_str() == at.segment.as_str())
        .ok_or(ReversalError::Unknown)?;
    Ok((part, segment))
}

/// Whether `segment` starts where `withdrawal`, a part's first segment, took the value: at the inner node the origin
/// reserve holds it at, in the same asset, so the origin reserve's word is about the segment's origin node.
fn starts_where_withdrawn(withdrawal: &stm::Segment, segment: &stm::Segment) -> bool {
    withdrawal.kind == OperationKind::Withdrawal && withdrawal.to == segment.from
}

/// The inner node `segment` starts at, if it starts at one.
fn inner_node(segment: &stm::Segment) -> Option<String> {
    match &segment.from.node {
        rutent_contracts::provider_role::NodeRef::Inner(node) => Some(node.as_str().to_owned()),
        rutent_contracts::provider_role::NodeRef::Outer(_) => None,
    }
}

/// A part's origin reserve: the provider of its withdrawal, its first segment.
fn origin_of(part: &stm::Part) -> Result<Id, ReversalError> {
    part.segments
        .first()
        .filter(|s| s.kind == OperationKind::Withdrawal)
        .and_then(|s| Id::new(s.provider.as_str()))
        .ok_or(ReversalError::NoOriginReserve)
}

/// What an operation Core signed moved: its provider, the asset it took in and how much. Core writes every operation
/// `exact_in`, so its amount is what it takes in.
fn moved(operation: &ProviderOperation) -> Option<(&str, &str, &str)> {
    match operation {
        ProviderOperation::Withdrawal(o) => Some((&o.provider, &o.asset, &o.amount)),
        ProviderOperation::Deposit(o) => Some((&o.provider, &o.asset, &o.amount)),
        ProviderOperation::Transfer(o) => Some((&o.provider, &o.asset, &o.amount)),
        ProviderOperation::Conversion(o) => Some((&o.provider, &o.from, &o.amount)),
        _ => None,
    }
}

/// The fault `bytes`, the provider's failed result for `transaction`, show (provider-role's `Failure.fault`): none
/// where it isn't a result, is of another transaction, didn't fail, or names no fault.
fn fault_shown(bytes: &[u8], transaction: &Id) -> Option<Fault> {
    let result: Statement<ResultBody> = serde_json::from_slice(bytes).ok()?;
    if result.shape != RESULT_SHAPE {
        return None;
    }
    let (named, failure) = match &result.statement {
        ResultBody::Withdrawal(r) | ResultBody::Deposit(r) => match &r.outcome {
            ReserveMovementOutcome::Failed(f) => (&r.transaction, f),
            _ => return None,
        },
        ResultBody::Transfer(r) => match &r.outcome {
            TransferOutcome::Failed(f) => (&r.transaction, f),
            _ => return None,
        },
        ResultBody::Conversion(r) => match &r.outcome {
            ConversionOutcome::Failed(f) => (&r.transaction, f),
            _ => return None,
        },
        _ => return None,
    };
    if named != transaction.as_str() {
        return None;
    }
    failure.fault
}

/// The carrier a showing of `fault`, answered by `by`, names: `by` must act in that party's role.
fn party_of(fault: Fault, by: &Actor) -> Result<Carrier, ReversalError> {
    match (fault, by.role) {
        (Fault::Customer, Role::Customer) => Ok(Carrier::Customer),
        (Fault::Operator, Role::Operator) => Ok(Carrier::Operator),
        _ => Err(ReversalError::NotTheParty),
    }
}

/// A state-machine input about `order`, and its `part` and `segment` where named, written by Core at its clock,
/// checked against the order as Core follows it: the state machine must take it.
fn input(
    parts: &mut Parts<'_>,
    order: &Id,
    part: Option<&Id>,
    segment: Option<&Id>,
    observed: Observed,
) -> Result<RecordedInput, ReversalError> {
    let state_id = |text: &str, what| stm::Id::new(text).ok_or(ReversalError::Id(what));
    let when = journal::now(parts.clock)?;
    let input = RecordedInput {
        record_id: stm::RecordId::new(parts.fresh.id().as_str())
            .ok_or(ReversalError::Id("a record id"))?,
        recorded_at: stm::Timestamp::new(when.as_str()).ok_or(ReversalError::Id("a time"))?,
        time_source: state_id("core_clock", "a time source")?,
        source: Source {
            kind: SourceKind::Orchestrator,
            id: state_id(parts.core.acts.as_str(), "Core's id")?,
        },
        standing: Standing::Stated,
        subject: Subject {
            order: state_id(order.as_str(), "an order")?,
            part: part.map(|p| state_id(p.as_str(), "a part")).transpose()?,
            segment: segment
                .map(|s| state_id(s.as_str(), "a segment"))
                .transpose()?,
        },
        observed,
    };
    let followed = parts.segments.order(order).ok_or(ReversalError::Unknown)?;
    let after = Stm::transition(followed, &input);
    if after.last_input.outcome != (stm::Outcome::Applied {}) {
        return Err(ReversalError::NotApplicable(after.last_input.outcome));
    }
    Ok(input)
}

/// The question in its envelope, made by the operator at Core's clock for the bounds' deployment and protocol
/// version, signed with the operator key: the bytes as signed.
fn sign(
    parts: &Parts<'_>,
    asking: &mut Asking<'_>,
    operation: ProviderOperation,
) -> Result<Vec<u8>, ReversalError> {
    let made_at = journal::now(parts.clock)?;
    let mut statement = Statement {
        deployment: asking.bounds.deployment.clone(),
        protocol_version: asking.bounds.protocol_version.as_str().to_owned(),
        shape: OPERATION_SHAPE.to_owned(),
        maker: parts.operator.acts.as_str().to_owned(),
        made_at: made_at.as_str().to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: asking.key.key(),
            value: base64url(&[0u8; 64]),
        },
        statement: operation,
    };
    let unsigned = serde_json::to_vec(&statement).map_err(|_| ReversalError::Unsigned)?;
    let input = signing_input(&unsigned).map_err(|_| ReversalError::Unsigned)?;
    let signature = asking.key.sign(&input).ok_or(ReversalError::Unsigned)?;
    statement.signature.value = base64url(&signature);
    serde_json::to_vec(&statement).map_err(|_| ReversalError::Unsigned)
}

/// Bytes as base64url without padding.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

fn key(order: &Id, segment: &Id) -> (String, String) {
    (order.as_str().to_owned(), segment.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use rutent_contracts::state_machine::{Segment, SegmentPlan, SegmentState};

    use super::starts_where_withdrawn;

    fn segment(
        kind: &str,
        provider: &str,
        from: serde_json::Value,
        to: serde_json::Value,
    ) -> Segment {
        let plan: SegmentPlan = serde_json::from_value(serde_json::json!({
            "segment_id": format!("{provider}-s"), "provider": provider, "kind": kind, "from": from, "to": to,
        }))
        .unwrap();
        Segment::new(plan, SegmentState::FailedReversalOwed)
    }

    fn at(node: serde_json::Value, asset: &str) -> serde_json::Value {
        serde_json::json!({ "node": node, "asset": { "id": asset, "decimals": 2 } })
    }

    /// The origin reserve's word is about the node its withdrawal took the value to: a segment starting there is
    /// one whose reversal it can confirm, and one starting at a later inner node isn't.
    // verifies: REQ-ORD-3
    #[test]
    fn origin_reserve_confirms_only_a_segment_starting_at_its_node() {
        let withdrawal = segment(
            "withdrawal",
            "bank",
            at(serde_json::json!({ "network": "co-ach" }), "COP"),
            at("ledger-in".into(), "COP"),
        );
        let first = segment(
            "route",
            "ramp",
            at("ledger-in".into(), "COP"),
            at("ledger-out".into(), "COP"),
        );
        let later = segment(
            "route",
            "ramp-2",
            at("ledger-out".into(), "COP"),
            at("ledger-far".into(), "COP"),
        );
        assert!(starts_where_withdrawn(&withdrawal, &first));
        assert!(!starts_where_withdrawn(&withdrawal, &later));
        assert!(
            !starts_where_withdrawn(&first, &later),
            "only from a withdrawal"
        );
    }
}
