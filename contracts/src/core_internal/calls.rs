//! The calls between Core's three parts, as traits: policy's (`CMP-CORE-AUTHZ`), the log's, its evidence store's
//! and its checks' (`CMP-CORE-AUDIT`), and the operator's channel, which the checks raise to.
//!
//! Each trait is implemented by the part that owns it and called by the others, which hold it as `Box<dyn ...>` or
//! as a type parameter: every method takes `&mut self` and plain references, so every trait is object-safe. One
//! assembly package builds the parts and hands each the others' traits; no part imports another (`DEC-2`).
//!
//! **The callee checks what it is handed.** Every implementer runs the `check` of each value it is handed, which
//! recurses into everything that value holds, and refuses one that fails it: a policy call answers
//! `Err(AppendFailure::Refused)` and records nothing, as the log does. Values built in memory are held to the same
//! rules as values read from the wire.
//!
//! **Two handles on one log.** The log takes entries through two traits. [`Log`] takes a [`GeneralEntry`], which
//! can't hold a decision kind: that is orchestration's handle, and the checks'. [`PolicyLog`] takes a
//! [`DecisionEntry`], which holds nothing but a decision kind: that is policy's. Neither type can be built, nor read
//! from the wire, holding the other's kinds, so no part that holds only a [`Log`] can write a decision, however it
//! builds or deserialises the entry. Only the assembly holds the log itself, and it hands [`PolicyLog`] to the policy
//! engine alone; the crate graph keeps every other part from reaching the log's type (`tools/boundary-check`).
//!
//! **The log, not the writer, attributes a statement.** A stated entry reaches the general handle unverified: it
//! names only the maker it claims and the commitment of the bytes the evidence store keeps. A [`GeneralEntry`]
//! can't hold one attributed `verified`. On append the log fetches the kept bytes by their commitment
//! ([`KeptStatements`]), checks them against the maker's keys at their `made_at` and this deployment
//! ([`StatementKeys`], read from the catalogue, never from orchestration) and their signature ([`Signatures`]), and
//! records the entry [`attribute`](super::attribute) answers. A relation that trusts a statement's attribution
//! trusts the log's.
//!
//! **The log answers first.** Every policy answer is appended to the log before it is given, and an answer comes
//! with the receipt of its entry; where the append fails, policy answers nothing, and nothing moves on it
//! (`T-021-01`, `INV-7`). Orchestration appends an [`Effect`]'s entry and holds its receipt before the action takes
//! effect, so a failure between the record and the effect leaves no effect.
//!
//! **Detection only reaches the operator.** The checks answer orchestration with a receipt and nothing else; a
//! difference goes to [`Operator::raise`], whose [`Raise`] carries nothing that can move value (ADR-0009).

use super::Violation;
use super::types::*;
use crate::module_interface::Digest;
use crate::provider_role::Deployment;
use crate::router_api::{Id, Timestamp};

/// Why the log, or the evidence store, took nothing. Nothing was appended or kept, so the action it would have
/// recorded must not take effect (`INV-7`). A library API result, with no wire form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendFailure {
    /// The store can't be written now; the caller may retry with the same entry or bytes.
    Unavailable,
    /// What was handed over breaks a rule of this contract, or one the log holds to, such as an effect whose
    /// action isn't the one its permit's decision decided, or an entry carrying a secret or a balance
    /// (`REQ-AUD-6`).
    Refused(Violation),
}

/// The kinds of verified entry only policy writes: each policy call's record of its answer (`decide`, `authorise`,
/// `stuck_decide`, `stuck_move`, `open_session`, `check_read`, `grant_permission`, `revoke_permission`, `charge`,
/// `ask_about_person`).
pub fn is_decision(what: &What) -> bool {
    matches!(
        what,
        What::Decide { .. }
            | What::Authorise { .. }
            | What::StuckDecide { .. }
            | What::StuckMove { .. }
            | What::OpenSession { .. }
            | What::CheckRead { .. }
            | What::GrantPermission { .. }
            | What::RevokePermission { .. }
            | What::Charge { .. }
            | What::AskAboutPerson { .. }
    )
}

/// The refusal of a decision kind on the general handle.
pub const DECISION_ON_THE_GENERAL_HANDLE: &str =
    "a decision entry reaches the log only through policy's handle";

/// The refusal of anything but a decision kind on policy's handle.
pub const RECORD_ON_THE_POLICY_HANDLE: &str =
    "policy's handle takes only the record of a policy decision";

/// The refusal of a stated entry its writer attributed, on the general handle.
pub const ATTRIBUTED_BY_ITS_WRITER: &str =
    "a stated entry reaches the log unverified: the log, not its writer, attributes it";

/// An entry that may reach the log through the general handle, [`Log`]: a stated entry its writer leaves
/// unverified, naming only the maker it claims, or a verified entry of any kind but a decision kind
/// ([`is_decision`]). Its field is private, and it is built or deserialised only through [`TryFrom<Entry>`], which
/// refuses a decision kind and a stated entry attributed `verified`, so no value of this type holds either: the log
/// sets a statement's attribution on append ([`attribute`](super::attribute)).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "Entry", into = "Entry")]
pub struct GeneralEntry(Entry);

impl GeneralEntry {
    /// The entry.
    pub fn entry(&self) -> &Entry {
        &self.0
    }
}

impl TryFrom<Entry> for GeneralEntry {
    type Error = Violation;

    fn try_from(entry: Entry) -> Result<Self, Violation> {
        match &entry {
            Entry::Verified(v) if is_decision(&v.what) => {
                Err(Violation(DECISION_ON_THE_GENERAL_HANDLE))
            }
            Entry::Stated(s) if matches!(s.attribution, Attribution::Verified { .. }) => {
                Err(Violation(ATTRIBUTED_BY_ITS_WRITER))
            }
            _ => Ok(GeneralEntry(entry)),
        }
    }
}

impl From<GeneralEntry> for Entry {
    fn from(entry: GeneralEntry) -> Self {
        entry.0
    }
}

/// An entry that reaches the log through policy's handle, [`PolicyLog`]: a verified entry of a decision kind, and
/// nothing else. Its field is private, and it is built or deserialised only through [`TryFrom<VerifiedEntry>`],
/// which refuses any other kind.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "VerifiedEntry", into = "VerifiedEntry")]
pub struct DecisionEntry(VerifiedEntry);

impl DecisionEntry {
    /// The entry.
    pub fn entry(&self) -> &VerifiedEntry {
        &self.0
    }
}

impl TryFrom<VerifiedEntry> for DecisionEntry {
    type Error = Violation;

    fn try_from(entry: VerifiedEntry) -> Result<Self, Violation> {
        if is_decision(&entry.what) {
            Ok(DecisionEntry(entry))
        } else {
            Err(Violation(RECORD_ON_THE_POLICY_HANDLE))
        }
    }
}

impl From<DecisionEntry> for VerifiedEntry {
    fn from(entry: DecisionEntry) -> Self {
        entry.0
    }
}

/// `CMP-CORE-AUDIT`'s log (ADR-0005) through its general handle: orchestration's, and the checks'. It takes no
/// decision kind: those come only through [`PolicyLog`].
pub trait Log {
    /// Appends `entry` and answers with its receipt once it is durable, or with why nothing was appended.
    ///
    /// - It runs [`Entry::check`] and refuses an entry that fails it.
    /// - It attributes a stated entry itself: it records the entry [`attribute`](super::attribute) answers over the
    ///   log's [`KeptStatements`], [`StatementKeys`] and [`Signatures`], `verified` only where the kept bytes bear
    ///   the claim out, and refuses one whose bytes the evidence store doesn't keep. The receipt is of the entry
    ///   recorded, not the one handed in.
    /// - It refuses any entry that [`admit_entry`](super::admit_entry) doesn't admit over the log's history, on the
    ///   general path ([`Through::General`](super::Through::General)), which refuses a decision kind as well: an
    ///   `effect` only on an unused permit for a permitting entry of this log that decided the same action (a stuck
    ///   order's only on its stuck move, with its decide step's resolution), a `result` only of an effect of this
    ///   log and only once, and every other entry only when the earlier entries it cites are in the log. Permits,
    ///   receipts and ids can be built anywhere, so this is the guard.
    /// - An entry the log took is never dropped. Appending an equal entry again after a failure appends it once;
    ///   entries that differ in anything, a handed-over item's drain or position included, are two entries.
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure>;
}

/// `CMP-CORE-AUDIT`'s log through policy's handle: the one way a decision entry reaches the log. The assembly hands
/// it to the policy engine and to no other part.
pub trait PolicyLog {
    /// Appends a policy decision's entry, as [`Log::append`] does, on the policy path
    /// ([`Through::Policy`](super::Through::Policy)).
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure>;
}

/// The evidence store's own read for the log: the statement kept under a commitment, with no policy read, since
/// nothing leaves Core's audit part (`T-022-03` implements it beside [`Evidence`]; `T-022-01`'s log holds it).
pub trait KeptStatements {
    /// The statement kept under `commitment`, or `None` where none is.
    fn kept(&mut self, commitment: &Digest) -> Result<Option<KeptStatement>, AppendFailure>;
}

/// Whose keys sign statements here, as the log reads them to attribute a statement: a provider's `statements` keys
/// from its advertisements, and the operator's key, which signs Core's questions, from the deployment. The catalogue
/// the state machine holds is state orchestration drives, so `T-022-01`'s log takes a provider's keys only from
/// advertisements it recorded itself under permitted `change_catalogue` effects; never from orchestration, which
/// could otherwise name its own key a provider's and sign that provider's word. A key is current at the statement's
/// own `made_at`, so a key revoked since still verifies a statement dated before (`THR-3`, a residual).
pub trait StatementKeys {
    /// This deployment, which every statement made for it names (`REQ-PRT-1`).
    fn deployment(&mut self) -> Result<Deployment, AppendFailure>;

    /// The maker this deployment's operator signs its own statements as, Core's questions among them, with the
    /// deployment's `operator_key`: a statement's `acts`, not the party a module the operator publishes states for.
    /// The log hands it to [`admit_entry`](super::admit_entry).
    fn operator(&mut self) -> Result<Id, AppendFailure>;

    /// The party `maker` states for: its publisher, by the advertisements the log recorded, or the operator's
    /// party for the operator and a module the operator publishes, by the releases the log accepted; `None` where it
    /// knows no such maker. A verified
    /// statement's `party` is this, never the writer's word.
    fn publisher(&mut self, maker: &Id) -> Result<Option<Id>, AppendFailure>;

    /// Whether `key` signs `maker`'s statements at `at`: a `statements` key of the provider's advertisement
    /// current at `at`, or, for the operator, the deployment's `operator_key` (`REQ-AUD-2`, `THR-3`). The log's
    /// first trust in a provider's advertisements is the advertising keys its latest grant in the log names
    /// ([`granted_advertising_keys`](super::granted_advertising_keys)), never a deployment setting such as
    /// `first_trust`, so a checker outside the operator recomputes every attribution from the history alone. A
    /// module the operator publishes isn't asked here: [`attribute`](super::attribute) holds it to the statement
    /// key of a release of it the log accepted ([`release_statement_keys`](super::release_statement_keys)).
    fn signs_for(
        &mut self,
        maker: &Id,
        key: &SignedWith,
        at: &Timestamp,
    ) -> Result<bool, AppendFailure>;
}

/// The check of a statement's signature over provider-role's `signing_input` of its bytes as received, against the
/// key the envelope names, strictly (RFC 8032 §5.1.7). `T-022-01` gives the log one; the contract leaves the
/// arithmetic out, so the contracts crate stays `no_std` with no curve in it.
pub trait Signatures {
    /// True where `received`'s signature is its named key's over its signing input.
    fn verifies(&self, received: &[u8]) -> bool;
}

/// What the evidence store answers when it keeps a statement's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    /// The salted commitment a [`StatedEntry`] names the bytes by.
    pub commitment: Digest,
    /// The commitment of a statement kept before whose signed content is the same: the one this restates, where it
    /// restates one (`REQ-AUD-2`, `T-022-02`).
    pub restates: Option<Digest>,
}

/// What the evidence store answers a fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// The statement as kept, for the reader to check (see [`KeptStatement`]).
    Kept(KeptStatement),
    /// Policy refused the read, and recorded it.
    Refused(ReadAnswer),
    /// It was erased outside the log; its commitment stays (`REQ-AUD-13`).
    Erased,
}

/// The store of evidence apart from the log (ADR-0005, `T-022-03`): each statement's bytes as received, with its
/// signature in them, named from the log by a salted commitment.
///
/// **Restatements are found here, not in the log.** The salt that keeps the log from confirming a guess also keeps
/// it from showing that two entries hold one statement. The store keeps an unsalted index, the SHA-256 digest of
/// each statement's signed content (provider-role's `signing_input`), which never enters the log, and answers each
/// [`keep`](Evidence::keep) with the commitment of the statement it restates, so a relay's copy of another party's
/// statement is detected and stays that party's (`REQ-AUD-2`).
pub trait Evidence {
    /// Keeps `received`, a party's statement exactly as it arrived, beside a fresh random salt of 32 bytes, and
    /// answers with the commitment: the SHA-256 digest of the salt followed by the bytes, and the statement it
    /// restates, if any. Neither the salt nor the unsalted index enters the log, so the log lets no one confirm a
    /// guess of what a statement says (`REQ-AUD-6`). Keep the bytes before appending the entry that names them.
    fn keep(&mut self, received: &[u8]) -> Result<Kept, AppendFailure>;

    /// Answers a read of one kept statement (`check.reads` an
    /// [`Reads::Evidence`](super::Reads::Evidence)), for the party it concerns or a checker by its permission (`REQ-AUD-5`, `REQ-AUD-10`). The store asks policy through [`Reads`] and hands the
    /// statement over only on an allowed read. A check that reads anything else is refused.
    fn fetch(&mut self, check: &ReadCheck) -> Result<Fetched, AppendFailure>;
}

/// The one decision call (`REQ-AUZ-1`), `CMP-CORE-AUTHZ`'s policy engine.
pub trait Decisions {
    /// Decides `request`: permit, refuse with its class, or not known (`REQ-AUZ-5`). A decision missing an input is
    /// a refusal, an unreachable policy service or verifier gives not known, and nothing moves on either. The
    /// `decide` entry names the decision's id, and a permit is minted from the answer by [`Decision::permit`].
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure>;
}

/// The authorisations to move funds (`REQ-AUZ-3`).
pub trait Authorisations {
    /// Registers `authorisation`, so value-moving actions that name it are decided against it. The decision's
    /// `request` is the authorisation's own id. A new destination after a failed final deposit is a new one
    /// (`REQ-ORD-43`).
    fn authorise(&mut self, authorisation: &Authorisation) -> Result<Decision, AppendFailure>;
}

/// The session register the catalogue reads (`REQ-CAT-3`).
pub trait Sessions {
    /// Opens a session for an account's holder or its delegate, bounded in time, or refuses one. Its entry names
    /// the session opened.
    fn open_session(&mut self, request: &SessionRequest) -> Result<SessionAnswer, AppendFailure>;
}

/// Read checks (`REQ-AUZ-7`, `REQ-AUD-12`).
pub trait Reads {
    /// Allows or refuses a read, by any actor; either way it is recorded.
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure>;

    /// Grants a read permission, or refuses to. Policy learns of it here, and a read naming it rests on it.
    fn grant_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure>;

    /// Revokes a read permission granted before, or refuses to.
    fn revoke_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure>;
}

/// The work a caller causes, charged to its accountable party (`REQ-OPS-1`, `REQ-AUZ-8`).
pub trait Charges {
    /// Charges `charge` to its party, answering within or over the party's bound. Over the bound, the work is not
    /// done.
    fn charge(&mut self, charge: &Charge) -> Result<ChargeAnswer, AppendFailure>;
}

/// Resolving a stuck order in two steps (`REQ-AUZ-4`, ADR-0011).
pub trait StuckOrders {
    /// The decide step: the outcome and the evidence it relies on.
    fn decide_stuck(&mut self, request: &StuckDecide) -> Result<Decision, AppendFailure>;

    /// The move step: permitted only to an actor whose accountable party is not the decider's, and only for the
    /// outcome the decision named. Its permit is what an [`Action::ResolveStuck`] effect cites.
    fn move_stuck(&mut self, request: &StuckMove) -> Result<Decision, AppendFailure>;
}

/// Questions about people (`REQ-CPL-2`, `REQ-CPL-5`).
pub trait PersonQuestions {
    /// Allows, refuses or bounds a question about a person before any provider or verifier is asked it.
    fn ask_about_person(
        &mut self,
        question: &PersonQuestion,
    ) -> Result<PersonAnswer, AppendFailure>;
}

/// The comparisons that only detect (`REQ-ORD-9`, `REQ-AUD-15`, ADR-0009).
pub trait Comparisons {
    /// Records a segment's pair of reports and compares them; a difference raises to the operator. The answer is
    /// the receipt of the pair's entry, and says nothing of the comparison.
    fn report_pair(&mut self, pair: &ReportPair) -> Result<Receipt, AppendFailure>;

    /// Checks a holding account's balance against the value held there for each order; a difference raises to the
    /// operator. The log keeps the check without the balance (`REQ-AUD-6`). The answer is the receipt of the
    /// check's entry, and says nothing of the comparison.
    fn holding_reading(&mut self, reading: &HoldingReading) -> Result<Receipt, AppendFailure>;
}

/// The records the bulk measures read (`REQ-AUD-7`).
pub trait Measures {
    /// Records one measure record.
    fn measure(&mut self, record: &MeasureRecord) -> Result<Receipt, AppendFailure>;
}

/// Whether a raise reached the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// It reached the operator.
    Delivered,
    /// It didn't, yet: the channel retries it within its bound and records it (`T-023-02`).
    Undelivered,
}

/// The operator's channel, which the checks raise to.
pub trait Operator {
    /// Raises a difference to the operator, and says whether it reached the operator, which the raise's entry
    /// records. It says nothing else, so nothing that raises can act on the answer.
    fn raise(&mut self, raise: &Raise) -> Delivery;
}
