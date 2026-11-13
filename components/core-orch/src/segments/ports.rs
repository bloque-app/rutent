//! What the segments reach outside Core's own records: the module platform, the credential proxy's records of use,
//! and the grading of a settled segment's evidence. The assembly gives each; the tests give fakes, or the platform
//! itself with simulated providers.
//!
//! **The platform is carried out on an effect only.** [`Modules::carry_out`] and [`Modules::promote`] take the
//! [`Effect`] they carry out, whose entry the log admitted only on a permit, so `CMP-CORE-AUTHZ`'s scan of Core's
//! entry points holds them to a decision (`REQ-AUZ-1`). Neither is named after a platform method the scan watches,
//! and the one implementation for [`Platform`] passes its effect on to nothing: holding it is the rule.

use rutent_contracts::core_internal::{
    Actor, Effect, Entry, Outcome, Receipt, RouteEnd, TimeSource, VerifiedEntry, VerifiedKind,
    What, When,
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
    /// What the result says the segment gave out at its destination.
    pub amount_out: &'a str,
    /// Core, which writes the entry.
    pub core: &'a Actor,
    /// The operator, to whom the operator's fee is attributed.
    pub operator: &'a Actor,
    /// When, by Core's clock.
    pub at: &'a Timestamp,
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
    /// Grades the evidence of `settled`.
    fn grade(&mut self, settled: &ToGrade<'_>) -> Graded;

    /// Whether a part ending in `deposit` could close at all: whether its destination has a public record this
    /// grading reads, or is a rail declared private, which closes on the movement into it (`REQ-ORD-33`). A part
    /// that couldn't isn't routed. Every grading can, unless it says otherwise.
    fn can_close(&self, deposit: &RouterOperation) -> bool {
        let _ = deposit;
        true
    }
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
