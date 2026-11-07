//! The role conformance suite (`T-002-03`, `DEC-5`, `REQ-CAT-8`, `REQ-CAT-21`, `REQ-MOD-4`).
//!
//! Any provider implementation meets the platform through [`RoleProvider`]: it publishes its advertisement and
//! answers the contract's operations with the contract's results. [`run_role_suite`] derives its probes from the
//! advertisement alone. That covers:
//!
//! - every tuple at its bands, with a dry run, an execution, and a repeat where a repeat is declared safe; for an
//!   exchange or ramp, a failed segment and its reversal, and the reversal of a settled one;
//! - the bands' edges, a real operation outside them, a tuple with an unadvertised node or asset, and missing
//!   required inputs;
//! - the operations outside the provider's role, and operations replayed from another deployment or signed with
//!   a key other than its operator's;
//! - balance checks in and out of the holder's session, and reversal confirmations, for a reserve;
//! - judgements, for a verifier.
//!
//! The suite checks each answer against what the provider-role contract promises, and reports every broken rule
//! as a [`Finding`] under one [`Check`]. It also derives from the advertisement the answers each tuple, asset,
//! class and the provider itself must give, and reports any it didn't see as [`Uncovered`]. A report passes only
//! with neither, and with no check deferred, so a provider that refuses everything can't pass by saying nothing.
//!
//! **The harness is apart from the provider.** Some answers can't be provoked from outside: outcome unknown, a
//! failure, a reversed credit arriving at a reserve. The suite drives them through [`RoleHarness`], a test-only
//! trait over the provider. A production adapter implements [`RoleProvider`] alone, so it has no way to fabricate a
//! reserve's word. [`WithoutHooks`] runs such a provider with no hooks, and the answers they provoke are then
//! uncovered. A certification run of a production adapter must go through [`WithoutHooks`]; the report records
//! whether any hook took effect ([`Report::hooks_used`]), so a hooked run can't be passed off as one.
//!
//! The suite sees only segments. A provider may take internal steps to serve one (`REQ-CAT-21`), and that
//! changes nothing here.
//!
//! **Signatures are not verified yet.** The suite checks that every envelope is well formed, and hands each
//! signature to a [`SignatureVerifier`]. The default, [`DeferredVerifier`], verifies nothing and says so in the
//! report, where [`Check::SignatureVerifies`] stays deferred and the report doesn't pass. Verification arrives in
//! `M2`. It will need the statement's bytes as received, so this hook's signature will change then.

use std::collections::BTreeSet;

use rutent_contracts::provider_role::{
    Advertisement, Deployment, Operation, ResultBody, Signature, Statement, VERSION,
};

mod encoding;
pub mod stand_in;
mod suite;

pub use encoding::{
    is_canonical_public_key, is_canonical_signature, is_protocol_version, is_utc_timestamp,
};
pub use suite::run_role_suite;

/// A provider implementation, as the role suite sees it.
pub trait RoleProvider {
    /// The provider's advertisement, as its own signed statement.
    fn advertisement(&self) -> Statement<Advertisement>;

    /// Answers one operation, an operator's statement, with the provider's result statement.
    fn handle(&mut self, operation: &Statement<Operation>) -> Statement<ResultBody>;

    /// An opaque fingerprint of the value the provider has moved and holds on its books. The suite compares it
    /// around each probe: a dry run, balance check, refusal, failure or repeat changes nothing, a success changes
    /// it, and a reversal puts it back. It trusts the fingerprint to be honest.
    fn state(&self) -> String;

    /// The provider's answer to a status query for a transaction, if it offers status queries (`REQ-CAT-16`).
    fn status(&mut self, _transaction: &str) -> Option<Statement<ResultBody>> {
        None
    }
}

/// The test hooks the suite drives a provider through, for answers it can't provoke from outside. It is a harness
/// over the provider, never part of a production adapter: a reserve's adapter that could credit a reversal could
/// fabricate the reserve's word (ADR-0008). Each hook returns false if the provider can't be driven that way; the
/// answers it provokes are then uncovered, and the provider doesn't pass.
pub trait RoleHarness: RoleProvider {
    /// Makes the provider's next real operation end with outcome unknown.
    fn force_outcome_unknown(&mut self) -> bool {
        false
    }

    /// Makes the provider's next real operation fail, with evidence, having moved nothing on, so the suite can ask
    /// for its reversal (`AC-4`, `INV-3`).
    fn force_failure(&mut self) -> bool {
        false
    }

    /// For a reserve: records that a reversed segment's value, `amount` of `asset`, arrived at its inner node
    /// `node`, so the suite can ask for the reserve's confirmation (ADR-0008).
    fn credit_reversal(
        &mut self,
        _segment: &str,
        _node: &str,
        _asset: &str,
        _amount: &str,
    ) -> bool {
        false
    }
}

/// A provider run with no test hooks, such as a production adapter.
#[derive(Debug, Clone)]
pub struct WithoutHooks<P>(pub P);

impl<P: RoleProvider> RoleProvider for WithoutHooks<P> {
    fn advertisement(&self) -> Statement<Advertisement> {
        self.0.advertisement()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Statement<ResultBody> {
        self.0.handle(operation)
    }

    fn state(&self) -> String {
        self.0.state()
    }

    fn status(&mut self, transaction: &str) -> Option<Statement<ResultBody>> {
        self.0.status(transaction)
    }
}

impl<P: RoleProvider> RoleHarness for WithoutHooks<P> {}

/// Where the suite runs: the deployment and protocol version every statement must name, and the operator, session,
/// account and subject its probes use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteContext {
    /// The deployment every statement must name.
    pub deployment: Deployment,
    /// The protocol version every statement must name. By default, the provider-role contract's `VERSION`.
    pub protocol_version: String,
    /// The operator's party identifier, the maker of every operation.
    pub operator: String,
    /// The account holder's session, for balance checks and withdrawal dry runs (`REQ-CAT-3`). The suite also
    /// probes with another session, this one with `-another` appended, which a reserve must refuse.
    pub session: String,
    /// The account that balance checks ask about.
    pub account: String,
    /// The subject reference that judgement questions carry.
    pub subject: String,
}

impl Default for SuiteContext {
    fn default() -> Self {
        Self {
            deployment: Deployment {
                name: "conformance".to_owned(),
                operator_key: placeholder_key(0),
            },
            protocol_version: VERSION.to_owned(),
            operator: "conformance-operator".to_owned(),
            session: "conformance-session".to_owned(),
            account: "conformance-account".to_owned(),
            subject: "conformance-subject".to_owned(),
        }
    }
}

/// One rule of the provider-role contract the suite checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Check {
    /// The advertisement is a valid statement of its provider: its shape, its context, and every rule of
    /// `validate_statement`.
    AdvertisementValid,
    /// Every statement's envelope is well formed, a nested judgement's or confirmation's included: its shape, its
    /// deployment and protocol version, a nested statement's context and maker, and canonical keys, signature
    /// values and times (`REQ-PRT-1`).
    EnvelopeWellFormed,
    /// Every result, and every statement nested in one, is made by the advertised provider and names one of its
    /// published keys.
    MakerAndKey,
    /// Every signature verifies, once a verifier is plugged in.
    SignatureVerifies,
    /// A result answers its operation: the same kind and transaction, and the stated amount (`REQ-ORD-1`).
    AnswersItsOperation,
    /// A dry run is answered by `dry_run_accepted` or a refusal (`REQ-CAT-14`).
    DryRunAnswered,
    /// A dry run changes nothing at the provider (`REQ-CAT-14`).
    DryRunHasNoEffect,
    /// A real operation is answered by an outcome, never by `dry_run_accepted`; a success changes the provider's
    /// state and a failure doesn't.
    ExecutionReported,
    /// An execution keeps to its quote: the direction asked, no higher fee, and no less out than quoted beyond
    /// the quote's slippage regime (`REQ-QTE-1`, `REQ-CAT-9`).
    QuoteHonoured,
    /// A repeat of a transaction, where the provider declares a repeat safe, changes nothing and is answered the
    /// same (`REQ-CAT-10`).
    SafeRetry,
    /// The provider refuses what it doesn't advertise and moves nothing for it: another node or asset, an amount
    /// outside its bands, or a class it doesn't judge (`REQ-CAT-13`, `REQ-CAT-14`).
    StaysWithinTuplesAndBands,
    /// The provider refuses a segment that lacks the inputs its tuple declares (`REQ-CAT-12`).
    RequiresDeclaredInputs,
    /// The provider refuses, finally and with no effect, an operation replayed from another deployment or protocol
    /// version, or signed with a key other than its deployment's operator key (`REQ-PRT-1`).
    RefusesForeignOperations,
    /// The provider refuses every operation outside its role, and moves nothing for it (`REQ-CAT-9`,
    /// `REQ-CAT-21`).
    StaysWithinRole,
    /// A refusal states the right class: `final` for what the provider never serves, and `waits_on_person` for
    /// missing inputs (`REQ-CAT-18`).
    RefusalClass,
    /// Outcome unknown is reported as itself, and a status query answers for the same transaction with an
    /// outcome that resolves it (`REQ-ORD-1`).
    OutcomeUnknownDistinct,
    /// A success or failure carries evidence at least as strong as the grade the provider declares
    /// (`REQ-ORD-39`).
    EvidenceAtDeclaredGrade,
    /// A balance check, or a withdrawal dry run, which reveals the same, is answered only in the holder's
    /// session; a balance check is answered yes or no, or refused, and changes nothing (`INV-9`, `REQ-CAT-3`,
    /// `REQ-CAT-4`).
    BalanceRevealsOnlySufficiency,
    /// A judgement is about the subject and class asked (`REQ-CPL-1`).
    JudgementMatchesQuestion,
    /// A reversal is a true undo: the value back at the segment's origin, in full and in the asset it left, and
    /// the provider's state as before the segment. A failed segment it doesn't refuse finally to undo, and one it
    /// never served it refuses (`REQ-CAT-15`, `REQ-ORD-3`, `AC-4`).
    ReversalIsTrueUndo,
    /// A reserve confirms a reversed credit only when that credit arrived, whole: the segment, asset and amount
    /// asked, and the question changes nothing (ADR-0008, `REQ-ORD-3`).
    ReversalConfirmationMatches,
}

/// A kind of answer the suite saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Exercised {
    /// A dry run answered by `dry_run_accepted`.
    DryRunAccepted,
    /// A real operation that succeeded, with evidence.
    Execution,
    /// A real operation driven to fail, with evidence.
    Failure,
    /// A real operation reported as outcome unknown.
    OutcomeUnknown,
    /// A status query resolving outcome unknown.
    StatusQuery,
    /// A refusal of something outside the provider's tuples or bands.
    RefusalOutsideTerms,
    /// A refusal of a segment missing the inputs its tuple declares.
    RefusalForMissingInputs,
    /// A refusal of an operation outside the provider's role.
    RefusalOutsideRole,
    /// A refusal that lifts at a known time, where one is allowed (`REQ-CAT-18`).
    RefusalLiftingAtKnownTime,
    /// A balance check answered yes or no.
    BalanceAnswered,
    /// A judgement given.
    Judged,
    /// A reversal that truly undid a failed segment.
    Reversal,
    /// A reversed credit confirmed.
    ReversalConfirmed,
    /// A reversal confirmation answered `not_confirmed` for a credit that didn't arrive whole.
    ReversalNotConfirmed,
}

impl Exercised {
    /// Whether only a [`RoleHarness`] hook can provoke this answer: outcome unknown and the status query that
    /// resolves it, a failure and its owed reversal, and a confirmed reversed credit. A run through
    /// [`WithoutHooks`] leaves these, and only these, uncovered.
    pub fn needs_a_hook(self) -> bool {
        matches!(
            self,
            Self::OutcomeUnknown
                | Self::StatusQuery
                | Self::Failure
                | Self::Reversal
                | Self::ReversalConfirmed
        )
    }
}

/// An answer the advertisement requires that the suite didn't see: for a tuple, an asset, a class or the
/// provider itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uncovered {
    /// What the answer is required of, as the suite's probes name it.
    pub subject: String,
    /// The answer.
    pub answer: Exercised,
}

/// One broken rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The rule.
    pub check: Check,
    /// What the suite saw.
    pub detail: String,
}

/// What a run of the suite found and exercised.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Every broken rule.
    pub findings: Vec<Finding>,
    /// The kinds of answer the suite saw.
    pub exercised: BTreeSet<Exercised>,
    /// The answers the advertisement requires that the suite didn't see.
    pub uncovered: BTreeSet<Uncovered>,
    /// Checks the suite couldn't conclude, such as signatures with no verifier plugged in.
    pub deferred: BTreeSet<Check>,
    /// How many signatures were handed to the verifier and deferred, not verified.
    pub signatures_deferred: usize,
    /// Whether any test hook of [`RoleHarness`] took effect in the run. A certification run of a production
    /// adapter goes through [`WithoutHooks`], so this is false there; a report with it true speaks for a test
    /// harness, not for the adapter.
    pub hooks_used: bool,
}

impl Report {
    /// Whether no rule was broken, every required answer was seen, and no check was deferred.
    pub fn passed(&self) -> bool {
        self.passed_but_for_deferred() && self.deferred.is_empty()
    }

    /// Whether no rule was broken and every required answer was seen, leaving aside the deferred checks.
    pub fn passed_but_for_deferred(&self) -> bool {
        self.findings.is_empty() && self.uncovered.is_empty()
    }

    /// The first of a certification's two reports (ruled by the orchestrator for `F-031`): a run through
    /// [`WithoutHooks`] in which no hook took effect, no rule was broken, no check was deferred, and every
    /// answer left uncovered is one only a hook can provoke ([`Exercised::needs_a_hook`]). The second report is a
    /// hooked run of the same adapter build against the provider's test mode or sandbox, which must
    /// [`passed`](Self::passed).
    pub fn passed_without_hooks(&self) -> bool {
        !self.hooks_used
            && self.findings.is_empty()
            && self.deferred.is_empty()
            && self.uncovered.iter().all(|u| u.answer.needs_a_hook())
    }

    /// The rules broken, each once.
    pub fn flagged(&self) -> BTreeSet<Check> {
        self.findings.iter().map(|f| f.check).collect()
    }
}

/// What a signature verifier concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// The signature verifies.
    Verified,
    /// The signature doesn't verify.
    Failed(String),
    /// Verification is not implemented yet (`M2`).
    Deferred,
}

/// The hook where signature verification plugs in (`M2`). It sees the maker and the signature. It will need the
/// statement's bytes as received, canonicalised under the contract's signing rule, so its signature changes when
/// verification lands.
pub trait SignatureVerifier {
    /// Verifies the signature a maker put on a statement.
    fn verify(&self, maker: &str, signature: &Signature) -> Verification;
}

/// A verifier that verifies nothing and says so: every signature is [`Verification::Deferred`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DeferredVerifier;

impl SignatureVerifier for DeferredVerifier {
    fn verify(&self, _maker: &str, _signature: &Signature) -> Verification {
        Verification::Deferred
    }
}

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// A well-formed, canonical placeholder for a 32-byte public key, distinct per seed. It is not a real key.
pub fn placeholder_key(seed: u8) -> String {
    let c = BASE64URL[usize::from(seed) % 64] as char;
    format!("{}A", c.to_string().repeat(42))
}

/// A well-formed, canonical placeholder for a 64-byte signature value. It is not a real signature.
pub fn placeholder_signature() -> String {
    format!("{}A", "s".repeat(85))
}
