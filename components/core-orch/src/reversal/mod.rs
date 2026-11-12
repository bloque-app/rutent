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
//! - **Who carries it** ([`Reversals::show_fault`], [`Reversals::accept_fault`], [`Reversals::dispute`],
//!   `REQ-ORD-11`). The failed provider carries it unless it shows the fault was the customer's or the operator's: its
//!   statement, kept, and the party it names accepting it. A dispute over it, by the party it names, declares the part
//!   stuck through [`Stuck::dispute`], so it is resolved in the stuck order's two steps.
//! - **Ending** ([`Reversals::end`], `REQ-ORD-12`). An order ends without completing only until its first segment
//!   settles, and never while a segment of a part that would end moves value: the `end_requested` input, once the
//!   state machine takes it. After that, a part ends only when its start is proven undone ([`Reversals::confirm`] on
//!   the part's withdrawal, once the part is complete): the origin reserve's own word, labelled so, and the part, and
//!   so the order, is marked failed (`REQ-STM-6`). The loss of an undone start is the origin reserve's.
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
//! **Gaps.** core-internal has no record of who carries a reversal, nor of a provider's showing of fault or its
//! acceptance: Core keeps them ([`Reversals::carrier`]). Core can't ask a failed provider to reverse: core-internal
//! has no action for it to decide. The state machine takes an undone start only of a complete part, so the start of a
//! part still open can't be marked undone. Inside a private node, the publisher's word labelled as such
//! (`REQ-ORD-3`'s third sentence) isn't taken. Each is named in `F-020`'s notes with its owner.

mod journal;
mod judge;

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, Reads};
use rutent_contracts::core_internal::{
    About, Actor, Afterwards, ReadCheck, ReadOutcome, Receipt, Role, What,
};
use rutent_contracts::provider_role::{
    EvidenceGrade, OPERATION_SHAPE, Operation as ProviderOperation, ResultBody,
    ReversalConfirmationRequest, Signature, SignatureScheme, Statement, signing_input,
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

/// Whose fault a failed provider shows a failure was (`REQ-ORD-11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The customer's.
    Customer,
    /// The operator's.
    Operator,
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
    /// The receipt of the statement it rests on.
    pub statement: Receipt,
    /// The receipt of the `reversal_completed` input.
    pub input: Receipt,
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
    disputed: bool,
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
    reports: Vec<(Vec<u8>, Step)>,
    showing: Option<Showing>,
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
    ends: BTreeMap<String, (Step, RecordedInput)>,
    afterwards: BTreeMap<String, Vec<After>>,
    later: BTreeMap<(String, String), Vec<(Vec<u8>, Step)>>,
}

impl Reversals {
    /// Nothing owed yet.
    pub fn new() -> Self {
        Self::default()
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
            question.answer_step = Some(Step::new(journal::stated(
                parts.evidence,
                parts.clock,
                &answer,
                maker,
                about,
            )?));
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

    /// Keeps the failed provider's showing that the failure of `at` was the customer's or the operator's: its
    /// statement, verified as the provider's, appended as its stated entry about the order and segment. Until the
    /// party it names accepts it, the provider carries the reversal (`REQ-ORD-11`).
    pub fn show_fault(
        &mut self,
        parts: &mut Parts<'_>,
        at: At<'_>,
        fault: Fault,
        bytes: &[u8],
    ) -> Result<Receipt, ReversalError> {
        let owing = self.owe(parts, at)?;
        parts
            .catalogue
            .verify_statement(bytes, parts.clock.now().floor())
            .map_err(|_| ReversalError::Unattributable)?;
        if journal::maker_named(bytes).as_ref() != Some(&owing.provider) {
            return Err(ReversalError::NotTheProvider);
        }
        let reported = self.report(parts, at, bytes)?;
        self.reversal(at, &owing.provider).showing = Some(Showing {
            fault,
            disputed: false,
        });
        Ok(reported.statement)
    }

    /// The party the provider's showing names, accepting it: it carries the reversal from then (`REQ-ORD-11`). `by`
    /// must act in that party's role; that it is the order's own customer, or the deployment's operator, is policy's
    /// to hold (`T-021-06`).
    pub fn accept_fault(&mut self, at: At<'_>, by: &Actor) -> Result<Carrier, ReversalError> {
        let reversal = self
            .reversals
            .get_mut(&key(at.order, at.segment))
            .ok_or(ReversalError::NoShowing)?;
        let showing = reversal
            .showing
            .as_ref()
            .filter(|s| !s.disputed)
            .ok_or(ReversalError::NoShowing)?;
        let carrier = party_of(showing.fault, by)?;
        reversal.carrier = carrier.clone();
        reversal.showing = None;
        Ok(carrier)
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
        let showing = reversal.showing.as_mut().ok_or(ReversalError::NoShowing)?;
        party_of(showing.fault, by)?;
        showing.disputed = true;
        let mut stuck_parts = stuck::Parts {
            clock: parts.clock,
            log: &mut *parts.log,
            fresh: &mut *parts.fresh,
            issued: parts.issued,
            segments: &mut *parts.segments,
            exposures: &mut *parts.exposures,
        };
        let declared = stuck
            .dispute(&mut stuck_parts, at.order, at.part, parts.core)
            .map_err(ReversalError::Stuck)?;
        Ok(declared.receipt)
    }

    /// Ends `order` without completing, on its customer's request (see the module's docs): only while a part of it
    /// hasn't passed its first segment, and none of such a part's segments moves value. The `end_requested` input is
    /// built once and appended until the log takes it; then the order Core follows takes it, and no segment of an
    /// ended part is handed over ([`crate::segments::SegmentsError::Ended`]).
    pub fn end(&mut self, parts: &mut Parts<'_>, order: &Id) -> Result<Receipt, ReversalError> {
        let followed = parts.segments.order(order).ok_or(ReversalError::Unknown)?;
        if !self.ends.contains_key(order.as_str()) {
            for part in followed
                .parts
                .iter()
                .filter(|p| p.state == OrderState::NotPastFirstSegment)
            {
                for segment in &part.segments {
                    let segment = Id::new(segment.segment_id.as_str())
                        .ok_or(ReversalError::Id("a segment"))?;
                    if parts.segments.handed_over(order, &segment) {
                        return Err(ReversalError::InMotion(segment));
                    }
                }
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
            start && part.state == OrderState::Complete && segment.state == SegmentState::Settled,
            segment.state == SegmentState::FailedReversalOwed,
        );
        let provider = Id::new(segment.provider.as_str()).ok_or(ReversalError::Id("a provider"))?;
        if start {
            let (transaction, operation) = parts
                .segments
                .sent(at.order, at.segment)
                .ok_or(ReversalError::Unknown)?;
            let (_, asset, amount) = moved(operation).ok_or(ReversalError::NotOwed)?;
            let target = Target {
                provider: provider.clone(),
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
        let owing = match self.owing(at.order, at.segment) {
            Some(owing) => owing.clone(),
            None => self.owe(parts, at)?,
        };
        Ok(Target {
            provider: owing.provider,
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
    origin: Id,
    transaction: Id,
    asset: String,
    amount: String,
}

impl Target {
    fn reversed(&self) -> Reversed<'_> {
        Reversed {
            provider: &self.provider,
            origin: &self.origin,
            transaction: &self.transaction,
            asset: &self.asset,
            amount: &self.amount,
        }
    }
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
