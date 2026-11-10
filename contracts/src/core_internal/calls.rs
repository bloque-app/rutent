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

/// `CMP-CORE-AUDIT`'s log (ADR-0005), which orchestration and policy append to.
pub trait Log {
    /// Appends `entry` and answers with its receipt once it is durable, or with why nothing was appended.
    ///
    /// - It runs [`Entry::check`] and refuses an entry that fails it.
    /// - It refuses any entry that [`admit_entry`](super::admit_entry) doesn't admit over the log's history: an
    ///   `effect` only on an unused permit for a permitting entry of this log that decided the same action (a stuck
    ///   order's only on its stuck move, with its decide step's resolution), a `result` only of an effect of this
    ///   log and only once, and every other entry only when the earlier entries it cites are in the log. Permits,
    ///   receipts and ids can be built anywhere, so this is the guard.
    /// - An entry the log took is never dropped. Appending an equal entry again after a failure appends it once;
    ///   entries that differ in anything, a handed-over item's drain or position included, are two entries.
    fn append(&mut self, entry: &Entry) -> Result<Receipt, AppendFailure>;
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
