//! What the segments reach outside Core's own records: the module platform, the credential proxy's records of use,
//! and the grading of a settled segment's evidence. The assembly gives each; the tests give fakes, or the platform
//! itself with simulated providers.
//!
//! **The platform is carried out on an effect only.** [`Modules::carry_out`] and [`Modules::promote`] take the
//! [`Effect`] they carry out, whose entry the log admitted only on a permit, so `CMP-CORE-AUTHZ`'s scan of Core's
//! entry points holds them to a decision (`REQ-AUZ-1`). Neither is named after a platform method the scan watches,
//! and the one implementation for [`Platform`] passes its effect on to nothing: holding it is the rule.

use rutent_contracts::core_internal::calls::{AppendFailure, Log, Operator};
use rutent_contracts::core_internal::{
    About, Actor, Effect, Entry, Outcome, Receipt, RouteEnd, TimeSource, VerifiedEntry,
    VerifiedKind, What, When,
};
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_contracts::provider_role::{EvidenceGrade, Operation, Statement};
use rutent_contracts::router_api::{
    DecisionRecord, FeeFormula, Id, Operation as RouterOperation, Timestamp,
};
use rutent_prov::catalogue::Instant;
use rutent_prov::module::lifecycle::{LifecycleRefusal, SegmentRecord};
use rutent_prov::module::platform::{Platform, World};
use rutent_prov::module::types::{Events, Handled};
use rutent_prov::proxy::{CredentialProxy, Resolver, RouteNotNamed, Use};

use rutent_evidence::{Refusal as EvidenceRefusal, Unmatched};

use crate::rounds::Fresh;
use crate::snapshot::Clock;

/// What a provider's module gave back for one operation Core carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered {
    /// The result, as the platform passed it on: the bytes Core judges, keeps and logs.
    Bytes(Vec<u8>),
    /// The platform refused it. `OutcomeUnknown` once the module passed something out; any other refusal, before.
    Refused(PlatformRefusal),
}

/// The module platform, as the segments use it (`T-013-06`, `T-014-04`).
pub trait Modules {
    /// Begins the segment of `transaction` at `provider`, answering the version it runs on
    /// (`Platform::begin_segment`).
    fn begin_segment(
        &mut self,
        provider: &Id,
        transaction: &Identifier,
    ) -> Result<String, PlatformRefusal>;

    /// Names, in `transaction`, the calls the operation at `at` of `record`'s chosen candidate makes, against
    /// `segment` (`RouteCalls::name_segment`, `REQ-MDL-21`).
    fn name_calls(
        &mut self,
        transaction: &Identifier,
        record: &DecisionRecord,
        at: (usize, usize),
        segment: &Identifier,
    ) -> Result<usize, RouteNotNamed>;

    /// Hands `operation`, the operation `effect` permits, to `provider`'s module (`Platform::handle`).
    fn carry_out(
        &mut self,
        effect: &Effect,
        provider: &Id,
        operation: &Statement<Operation>,
    ) -> Answered;

    /// Asks `provider`'s module where the segment of `about` stands, as `effect` (the permitted `ask_status`) lets,
    /// under `asking`, the fresh transaction of Core's own question (`REQ-ORD-1`, `T-020-27`). The answer is the
    /// provider's own statement about `about`'s transaction, kept as such and moving nothing (`INV-4`). The
    /// default answers that the module handles no such query, so nothing comes back and a late status is never
    /// taken from a platform that can't ask for one; the production platform answers it ([`Platform::status`],
    /// `T-013-11`), whose question Core holds the answer to with `check_status_answer`, since the platform checks the
    /// answer's shape, transaction and signer and not its `about` (`T-020-47`).
    fn status(
        &mut self,
        effect: &Effect,
        provider: &Id,
        asking: &Id,
        about: &Statement<Operation>,
    ) -> Answered {
        let _ = (effect, provider, asking, about);
        Answered::Refused(PlatformRefusal::NotHandled)
    }

    /// Hands the module of `provider` `question`, the operator's question of its record of a private node, in its
    /// envelope signed with the operator key, as `effect` (the permitted `read_node_record`) lets (`REQ-ORD-41`,
    /// `REQ-AUZ-1`, `T-020-58`). The question moves nothing; Core holds the answer to it with
    /// `closing::publishers::taken`. The default answers that the module handles no such question, so no record comes
    /// back and the node's books say nothing; the production platform answers it ([`Platform::handle`]).
    fn ask_for_record(
        &mut self,
        effect: &Effect,
        provider: &Id,
        question: &Statement<Operation>,
    ) -> Answered {
        let _ = (effect, provider, question);
        Answered::Refused(PlatformRefusal::NotHandled)
    }

    /// What the platform saw since it was last asked (`Platform::take_events`).
    fn take_events(&mut self) -> Events;

    /// Ends the segment of `transaction`, handing over its record (`Platform::end_segment`).
    fn end_segment(&mut self, transaction: &Identifier) -> Option<SegmentRecord>;

    /// Ends `transaction`'s route: no call is named in it any more (`RouteCalls::end`).
    fn end_calls(&mut self, transaction: &Identifier);

    /// The provider's staged version, if one is (`Lifecycle::staged`).
    fn staged(&self, provider: &Id) -> Option<String>;

    /// Puts the provider's staged release in force, by `by` at `at`, as `effect` permits (`Platform::promote`).
    fn promote(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<String, LifecycleRefusal>;
}

impl<W: World> Modules for Platform<W> {
    fn begin_segment(
        &mut self,
        provider: &Id,
        transaction: &Identifier,
    ) -> Result<String, PlatformRefusal> {
        Platform::begin_segment(self, provider.as_str(), transaction)
    }

    fn name_calls(
        &mut self,
        transaction: &Identifier,
        record: &DecisionRecord,
        at: (usize, usize),
        segment: &Identifier,
    ) -> Result<usize, RouteNotNamed> {
        self.routes.name_segment(transaction, record, at, segment)
    }

    fn carry_out(
        &mut self,
        effect: &Effect,
        provider: &Id,
        operation: &Statement<Operation>,
    ) -> Answered {
        let _permitted = effect;
        match Platform::handle(self, provider.as_str(), operation) {
            // The module answered, so it may have moved value: an answer that can't be kept is no answer, and its
            // outcome is unknown, never a refusal that would say nothing moved.
            Handled::Answered(result) => match serde_json::to_vec(&*result) {
                Ok(bytes) => Answered::Bytes(bytes),
                Err(_) => Answered::Refused(PlatformRefusal::OutcomeUnknown),
            },
            Handled::Refused(refusal) => Answered::Refused(refusal),
        }
    }

    fn status(
        &mut self,
        effect: &Effect,
        provider: &Id,
        asking: &Id,
        about: &Statement<Operation>,
    ) -> Answered {
        let _permitted = effect;
        let Some(asking) = Identifier::new(asking.as_str()) else {
            return Answered::Refused(PlatformRefusal::NotHandled);
        };
        match Platform::status(self, provider.as_str(), &asking, about) {
            Handled::Answered(result) => serde_json::to_vec(&*result).map_or(
                Answered::Refused(PlatformRefusal::OutcomeUnknown),
                Answered::Bytes,
            ),
            Handled::Refused(refusal) => Answered::Refused(refusal),
        }
    }

    fn ask_for_record(
        &mut self,
        effect: &Effect,
        provider: &Id,
        question: &Statement<Operation>,
    ) -> Answered {
        let _permitted = effect;
        match Platform::handle(self, provider.as_str(), question) {
            Handled::Answered(result) => serde_json::to_vec(&*result).map_or(
                Answered::Refused(PlatformRefusal::OutcomeUnknown),
                Answered::Bytes,
            ),
            Handled::Refused(refusal) => Answered::Refused(refusal),
        }
    }

    fn take_events(&mut self) -> Events {
        Platform::take_events(self)
    }

    fn end_segment(&mut self, transaction: &Identifier) -> Option<SegmentRecord> {
        Platform::end_segment(self, transaction.as_str())
    }

    fn end_calls(&mut self, transaction: &Identifier) {
        self.routes.end(transaction);
    }

    fn staged(&self, provider: &Id) -> Option<String> {
        self.lifecycle()
            .staged(provider.as_str())
            .map(str::to_owned)
    }

    fn promote(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<String, LifecycleRefusal> {
        let _permitted = effect;
        Platform::promote(self, provider.as_str(), by.as_str(), at)
    }
}

/// The credential proxy's records of use, each with its transaction (`REQ-MDL-6`).
pub trait Uses {
    /// The records of every use since they were last taken, oldest first (`CredentialProxy::take_uses`).
    fn take_uses(&mut self) -> Vec<Use>;
}

impl<R: Resolver> Uses for CredentialProxy<R> {
    fn take_uses(&mut self) -> Vec<Use> {
        CredentialProxy::take_uses(self)
    }
}

/// No proxy: a world that resolves nothing through one, as the simulated providers' does, records no use.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProxy;

impl Uses for NoProxy {
    fn take_uses(&mut self) -> Vec<Use> {
        Vec::new()
    }
}

/// A settled segment whose evidence is to be graded: what `segment_settled` cites, and everything the grading reads
/// of the segment and its part.
#[derive(Debug, Clone, Copy)]
pub struct ToGrade<'a> {
    /// The id the evidence takes, which `segment_settled` cites.
    pub evidence: &'a Id,
    /// The order.
    pub order: &'a Id,
    /// The part.
    pub part: &'a Id,
    /// The segment.
    pub segment: &'a Id,
    /// The segment's provider, whose result it is.
    pub provider: &'a Id,
    /// The transaction the segment ran under.
    pub transaction: &'a Id,
    /// The receipt of the result's stated entry.
    pub result: &'a Receipt,
    /// The result's bytes, as received: a result that counted (`judge`).
    pub bytes: &'a [u8],
    /// The part's operations, as the router chose them, in the route's order.
    pub operations: &'a [RouterOperation],
    /// The operator's fee formula the route was chosen under (`REQ-QTE-2`).
    pub operator_fee: &'a FeeFormula,
    /// The segment's place in the part.
    pub place: usize,
    /// The id of each of the part's segments, in the route's order.
    pub segments: &'a [Id],
    /// What the route ends at: its one destination (`DEC-13`).
    pub end: &'a RouteEnd,
    /// What the result says the segment gave out at its destination, its charge within it where its tuple reports
    /// gross.
    pub amount_out: &'a str,
    /// What arrives by the segment, net of its charge: what the network moves out of a final deposit, and what the
    /// operator's fee is figured on, as the quote figures it. The `amount_out` under `net`; under `gross`, that less the
    /// charge, as `judge` counts it.
    pub arrives: &'a str,
    /// Core, which writes the entry.
    pub core: &'a Actor,
    /// The operator, to whom the operator's fee is attributed.
    pub operator: &'a Actor,
    /// When, by Core's clock.
    pub at: &'a Timestamp,
    /// The statements the grading asked to have kept ([`Grading::statements`]), appended before it grades, with the
    /// receipt of each.
    pub kept: &'a [Kept],
    /// The transactions of the reversals Core carried out for the order: a private node's record that lists one is
    /// the order's own (`REQ-ORD-41`).
    pub reversals: &'a [Id],
}

/// A statement a grading read and cites: kept and appended, about its order and, where it names one, its node, before
/// the evidence that cites it is built (`T-020-32`). A receipt isn't known before the log gives it, so the grading
/// asks, the segments append, and the grading cites what came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToKeep {
    /// What the grading finds the statement by in [`ToGrade::kept`].
    pub key: String,
    /// The statement's bytes, as its maker signed them.
    pub bytes: Vec<u8>,
    /// What it is about: its order, and a private node's record its node alone.
    pub about: About,
}

/// A statement the grading asked to have kept, and the receipt of its entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    /// The key it was asked under.
    pub key: String,
    /// Its entry's receipt.
    pub receipt: Receipt,
}

/// Why a segment's evidence doesn't settle it yet. The segment stays in flight, held, and is graded again on the
/// next run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Short {
    /// A part's final deposit has no final public record of its movement to the route's destination, or, at a
    /// private rail, none of the movement into the private node: a statement alone closes no order (`REQ-ORD-33`).
    NoPublicRecord(Option<Unmatched>),
    /// The final deposit's public movement already evidences another order's segment: one movement evidences one
    /// segment (`REQ-ORD-38`), so it closes neither a second time.
    MovementShared,
    /// The result can't be read for its settlement and evidence.
    Unreadable,
    /// `CMP-EVIDENCE` refused the outcome's evidence: a claim above the provider's declared grade, or one movement or
    /// receipt backing two pieces.
    Refused(EvidenceRefusal),
}

/// What grading answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Graded {
    /// The segment settles on `cites`, the id of the first `evidence` entry among `entries`, which are appended in
    /// order, each once, before `segment_settled` (graded evidence, saying whose each is; any replay).
    Settles {
        /// The evidence `segment_settled` cites.
        cites: Id,
        /// The entries to record.
        entries: Vec<Entry>,
    },
    /// It doesn't settle yet.
    Waits(Short),
}

/// The grading of a settled segment's evidence through `CMP-EVIDENCE` (`T-020-09`'s `closing::Evidenced`): it
/// answers the entries the segment's `segment_settled` rests on, or why it doesn't settle yet. It appends nothing
/// itself: the segments append what it answers, each entry once, and grade a segment again only while it waits.
pub trait Grading {
    /// The private nodes' records the grading of `settled` reads, each of a provider of the node's publisher: what the
    /// segments ask for, decided and taken as an effect each (`read_node_record`, `REQ-ORD-41`, `T-020-58`), each time
    /// the segment is graded, so what the grading reads is what the providers state now. None unless a grading says
    /// otherwise.
    fn records_wanted(&mut self, settled: &ToGrade<'_>) -> Vec<RecordWanted> {
        let _ = settled;
        Vec::new()
    }

    /// What came back of a record `wanted`: the question as put and the answer as received, or `None` where nothing was
    /// asked (policy refused) or nothing came back.
    fn record_read(
        &mut self,
        wanted: &RecordWanted,
        answer: Option<(&Statement<Operation>, &[u8])>,
    ) {
        let _ = (wanted, answer);
    }

    /// The statements the grading reads and will cite, for the segments to keep and append before they grade
    /// `settled`, each once however often it is tried; their receipts come back in [`ToGrade::kept`]. None unless a
    /// grading says otherwise.
    fn statements(&mut self, settled: &ToGrade<'_>) -> Vec<ToKeep> {
        let _ = settled;
        Vec::new()
    }

    /// Grades the evidence of `settled`.
    fn grade(&mut self, settled: &ToGrade<'_>) -> Graded;

    /// Whether a part ending in `deposit` could close at all: whether its destination has a public record this
    /// grading reads, or is a rail declared private, which closes on the movement into it (`REQ-ORD-33`). A part
    /// that couldn't isn't routed. Every grading can, unless it says otherwise.
    fn can_close(&self, deposit: &RouterOperation) -> bool {
        let _ = deposit;
        true
    }

    /// The receipts of the entries `Graded::Settles` answered for `settled`, in the order it answered them, once the
    /// segments appended them: what a grading that raises a difference to cite one of its entries needs (`T-020-22`).
    /// Called again, with the same receipts, where the segment's settling is run again.
    fn recorded(&mut self, settled: &ToGrade<'_>, receipts: &[Receipt]) {
        let _ = (settled, receipts);
    }

    /// Raises to the operator what the grading found that it raises, and records each raise: a difference and nothing
    /// that can move value (`REQ-ORD-9`, ADR-0009). Called on every run of a part, so a raise whose record the log
    /// couldn't take is recorded when it can, and none is raised twice.
    ///
    /// # Errors
    ///
    /// If the log refused or couldn't take a raise's entry; the raise made is kept and its entry appended as built on
    /// the next call.
    fn raise(&mut self, desk: RaiseDesk<'_>) -> Result<(), AppendFailure> {
        let _ = desk;
        Ok(())
    }
}

/// A record of a private node the grading reads: asked of `provider`, one of its publisher's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordWanted {
    /// The private node.
    pub node: String,
    /// The provider asked.
    pub provider: String,
}

/// What a grading reaches to raise a difference: the log, Core's clock, a source of fresh identifiers, the
/// operator's channel and Core's actor, who records the raise.
pub struct RaiseDesk<'a> {
    /// The log, through orchestration's handle.
    pub log: &'a mut dyn Log,
    /// Core's clock.
    pub clock: &'a dyn Clock,
    /// Fresh identifiers, for each raise.
    pub fresh: &'a mut dyn Fresh,
    /// The operator's channel.
    pub operator: &'a mut dyn Operator,
    /// Core's actor, who records the raise.
    pub core: &'a Actor,
}

/// A stand-in that settles a segment on the provider's result alone, graded as its statement (grade 4), resting on
/// the result's stated entry, and never a part's final deposit, which waits: a statement alone closes no order
/// (`REQ-ORD-33`), and the log refuses one that does (`final-deposit-closes-on-a-public-record`). An order closes
/// only under `closing::Evidenced`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProvidersWord;

impl Grading for ProvidersWord {
    fn grade(&mut self, settled: &ToGrade<'_>) -> Graded {
        // A part's final deposit closes only on a public record and its reserve's statement: never on a word.
        if crate::closing::is_final_deposit(settled) {
            return Graded::Waits(Short::NoPublicRecord(None));
        }
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: settled.core.clone(),
            what: What::Evidence {
                evidence: settled.evidence.clone(),
                order: settled.order.clone(),
                segment: settled.segment.clone(),
                grade: EvidenceGrade::Statement,
                whose: settled.provider.clone(),
                rests_on: vec![settled.result.clone()],
                // A segment's settlement on its provider's word: the label of a private node's publisher counts only for a
                // reversal's evidence (`reversal-rests-on-its-evidence`), so a settlement carries none.
                publisher: None,
                movement: None,
            },
            when: When {
                at: settled.at.clone(),
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::Observed {},
        });
        Graded::Settles {
            cites: settled.evidence.clone(),
            entries: vec![entry],
        }
    }
}
