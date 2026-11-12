//! The policy engine: every action that moves value, posts a route or changes the catalogue reaches one decision, with
//! no path around it, and each decision is appended to the log before it answers.
//!
//! Declared by `T-020-01`; `T-021-01` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! # How a decision is reached
//!
//! [`Engine`] holds the log it appends to, the [`Fixed`] rules `SPEC.md` sets, the deployment's [`Policy`] service
//! and the [`Sources`] of the inputs a decision needs. Every call reaches one consultation, `Engine::consult`,
//! through what it was [`Asked`]. The fixed rules are asked first, then the deployment's service, each in three
//! steps:
//!
//! 1. The policy service names the inputs the decision needs. If it can't be reached, the answer is not known; if it
//!    answers without naming them, an input of the policy service's is missing, and the answer is a refusal
//!    (`REQ-AUZ-5`).
//! 2. Each input is asked of its source, a verifier or a provider, in the order the service named them. The first
//!    that fails decides: a source that answered without the input gives a refusal, one that couldn't be reached
//!    gives not known, and the inputs after it are not asked.
//! 3. The policy service rules over the request and every input given. A judgement's or a provider's yes is one of
//!    those inputs and never the decision: the service may refuse on it (`REQ-CPL-3`), and nothing is permitted
//!    without its ruling.
//!
//! Only a permitting verdict mints a permitting answer, and only in `consult`: no other code of Core's three crates
//! writes one, which a scan of their source watches for as a tripwire. An open session and an allowed read are taken
//! from that verdict, each in one function here that writes it only where the verdict permits. The guard is the
//! log's split append surface (`T-020-01`): a decision's entry reaches the log only through policy's handle,
//! [`PolicyLog`], which the assembly hands to this engine alone, and orchestration's handle takes no decision kind.
//! The engine appends through that handle and nothing else: each answer's entry is built, checked, made a
//! [`DecisionEntry`], which refuses any kind but a decision's, and appended, in that order, before the answer is
//! given; an append that fails answers nothing (`INV-7`). External authentication said who the caller is
//! (`Actor::acts`) and nothing more (`REQ-AUZ-1`): the engine reads nothing of the caller to permit, and hands it to
//! the policy service as it came.
//!
//! # Readings
//!
//! - **Not known is not a refusal.** `AC-17` says an unreachable verifier gives a refusal; `REQ-AUZ-5` and
//!   core-internal's README make it not known, never a refusal and never a permit. The engine answers not known, and
//!   nothing moves on it, which is what `AC-17` asks of the value.
//! - **A decision's id is its request's**, as the stubs had it, so a permit names one request and one decision.
//! - **The rules `SPEC.md` sets are the engine's.** [`Fixed`] holds them, one entry per later row of `F-021`, and
//!   [`Engine::with_policy`] asks them before the deployment's policy, so no assembly can leave one out. A refusal from either
//!   beats not known from the other.
//! - **An unreachable provider** gives `verifier_unreachable`: core-internal has no cause of its own for it.
//! - **`AC-17`'s reading is pending** on the owner's form as `spec-ac17-not-known`, with "not known" the default.

#[cfg(test)]
mod entry_points;

use rutent_contracts::core_internal::calls::{AppendFailure, DecisionEntry, Decisions, PolicyLog};
use rutent_contracts::core_internal::{
    Actor, Answer, Authorisation, Charge, Decision, DecisionRequest, NotKnownCause, Opened,
    Outcome, PermissionChange, PersonQuestion, ReadCheck, ReadOutcome, Receipt, RefusalClass,
    SessionOutcome, SessionRequest, StuckDecide, StuckMove, TimeSource, VerifiedEntry,
    VerifiedKind, What, When,
};
use rutent_contracts::router_api::{Id, Timestamp};

/// What policy is asked: one call's request, as it was handed over. Every policy call reaches the engine's one
/// consultation through one of these. Which row answers which call is `PLAN.md` §4.4's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked<'a> {
    /// The one decision call (`REQ-AUZ-1`).
    Decide(&'a DecisionRequest),
    /// An authorisation to move funds (`REQ-AUZ-3`).
    Authorise(&'a Authorisation),
    /// A stuck order's decide step (`REQ-AUZ-4`).
    DecideStuck(&'a StuckDecide),
    /// A stuck order's move step (`REQ-AUZ-4`).
    MoveStuck(&'a StuckMove),
    /// A session (`REQ-CAT-3`).
    OpenSession(&'a SessionRequest),
    /// A read (`REQ-AUZ-7`).
    CheckRead(&'a ReadCheck),
    /// A read permission granted (`REQ-AUZ-7`).
    GrantPermission(&'a PermissionChange),
    /// A read permission revoked (`REQ-AUZ-7`).
    RevokePermission(&'a PermissionChange),
    /// Work charged to a party (`REQ-AUZ-8`).
    Charge(&'a Charge),
    /// A question about a person (`REQ-CPL-2`).
    AskAboutPerson(&'a PersonQuestion),
}

impl Asked<'_> {
    /// Who asks, exactly as the request names it: the party it acts for, the actor external authentication
    /// established, its role and its kind. Policy decides what that caller may do (`REQ-AUZ-1`).
    pub fn caller(&self) -> &Actor {
        match self {
            Asked::Decide(r) => &r.caller,
            Asked::Authorise(a) => &a.given_by,
            Asked::DecideStuck(r) => &r.decider,
            Asked::MoveStuck(r) => &r.mover,
            Asked::OpenSession(r) => &r.requester,
            Asked::CheckRead(c) => &c.reader,
            Asked::GrantPermission(c) | Asked::RevokePermission(c) => &c.by,
            Asked::Charge(c) => &c.caller,
            Asked::AskAboutPerson(q) => &q.asker,
        }
    }
}

/// What the policy service or a source answered: what was asked for, an answer without it, or no answer at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply<T> {
    /// What was asked for.
    Given(T),
    /// The service or source answered, but without it: the decision misses an input, and is a refusal
    /// (`REQ-AUZ-5`).
    Missing,
    /// It couldn't be reached: the decision is not known, and nothing moves on it (`REQ-AUZ-5`).
    Unreachable,
}

/// Who gives an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A verifier: its judgement (`REQ-CPL-1`, ADR-0012).
    Verifier(Id),
    /// A provider: its yes or no, to eligibility or to a dry run, or its vouching (`REQ-CPL-6`, `REQ-CPL-7`).
    Provider(Id),
    /// An account's record, where the account is kept: whether the party the input names holds it (`REQ-CAT-3`).
    Account(Id),
    /// An account's holder: whether the party the input names is its delegate on that account (`REQ-CAT-3`).
    Holder {
        /// The account, by reference.
        account: Id,
        /// The party that holds it.
        holder: Id,
    },
    /// The record of an order, as the log holds its acceptance: whether the party the input names is its
    /// requester, beneficiary or payer (`REQ-AUZ-7`).
    OrderParties(Id),
    /// The evidence store's record of a statement it keeps, by its commitment: whether the party the input names may
    /// hold it as its own, as the party that made it or the one it was told to about its own part of an order
    /// (`REQ-AUD-5`).
    Statement(rutent_contracts::module_interface::Digest),
    /// The record of a segment, as the log holds the plan that lists it: whether the order the input names is the
    /// order it belongs to (`REQ-AUZ-7`, `REQ-ORD-38`).
    SegmentOrder(Id),
    /// The record of a segment, as the log holds the effect that moved value on it: whether the party the input
    /// names is the provider it moved through (`REQ-AUZ-7`).
    SegmentProvider {
        /// The order.
        order: Id,
        /// The segment.
        segment: Id,
    },
    /// The deployment's register of permissions: whether the actor the input names (its `acts`), acting for `party`,
    /// holds `permission`. It answers by the actor, never by the party alone, so each of a party's logins or
    /// components holds only what it is listed for (`REQ-AUZ-2`, `REQ-AUZ-6`, `REQ-AUZ-9`). Who is listed, and each
    /// change to that, is the deployment's to keep and record (`F-030`); the engine only asks.
    Permissions {
        /// The accountable party the actor acts for.
        party: Id,
        /// The one permission asked about.
        permission: crate::roles::Permission,
    },
    /// A reserve's declarations at publishing (`REQ-CAT-6`): whether the reserve made the declaration the input
    /// names, such as that it can't require the user's own withdrawal, under which the operator may act for the
    /// user there (`REQ-AUZ-9`). A reserve makes each declaration either way at publishing, so the assembly answers
    /// no, not nothing, for one a reserve that published didn't make; nothing is left for a reserve that never
    /// published, which refuses as a missing input.
    Declarations(Id),
    /// The record of an order: whether the party the input names is its requester, whose agents act in its party
    /// (`REQ-AUZ-9`, `REQ-ORD-13`).
    Requester(Id),
    /// The record of an order: whether the reserve the input names is its origin reserve, the provider of its first
    /// withdrawal or the holding account's on a route handed on (`REQ-AUZ-9`).
    OrderOrigin(Id),
    /// The deployment's register of actors: whether the actor `acts`, as external authentication established it,
    /// acts for the accountable party `party` its request states. External authentication says who acts and nothing
    /// more (`REQ-AUZ-1`), so a party is only the request's word until this says yes (`REQ-AUZ-8`). It answers by
    /// `acts`.
    ActsFor {
        /// The actor.
        acts: Id,
        /// The party its request says it acts for.
        party: Id,
    },
}

/// One input a decision needs, as the policy service names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    /// The input, by the name the policy service gives it, such as a judgement's class.
    pub input: Id,
    /// Who gives it.
    pub from: Source,
}

/// What an input says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Said {
    /// Yes: still only an input, which policy may refuse on (`REQ-CPL-3`).
    Yes,
    /// No.
    No,
}

/// One input as its source gave it, which the policy service rules over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    /// The input asked for.
    pub need: Need,
    /// What its source said.
    pub said: Said,
}

/// The policy service's ruling over a request and its inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ruling {
    /// The action may take effect.
    Permit,
    /// It may not, for this class of reason.
    Refuse(RefusalClass),
}

/// The deployment's policy service: what each call needs, and its ruling once those inputs are in.
///
/// The engine asks [`Fixed`], the rules `SPEC.md` sets, before it, and an assembly can't leave those out.
pub trait Policy {
    /// The inputs the decision on `asked` needs, beside its request; `Given` with none where it needs none.
    fn needs(&mut self, asked: Asked<'_>) -> Reply<Vec<Need>>;

    /// The ruling on `asked`, given every input [`needs`](Policy::needs) named, in that order.
    fn rule(&mut self, asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling>;
}

/// Where the inputs come from: the verifiers and the providers.
///
/// A source hands over what is already there to be had: a judgement its holder presented (ADR-0012), or a provider's
/// answer to a question policy allowed. The engine asks no one about a person itself; a question about a person goes
/// through [`PersonQuestions`](rutent_contracts::core_internal::calls::PersonQuestions) first, bounded and recorded
/// (`REQ-CPL-2`, `REQ-CPL-5`, `T-021-07`).
pub trait Sources {
    /// What `need`'s source says, if it can be asked and has it.
    fn supply(&mut self, need: &Need) -> Reply<Said>;
}

/// The needs of two policies, asked in order: both lists where both name theirs. A missing answer from either is the
/// answer, even where the other can't be reached, since it refuses whatever the other would say.
fn needs_of_both(
    first: Reply<Vec<Need>>,
    second: impl FnOnce() -> Reply<Vec<Need>>,
) -> Reply<Vec<Need>> {
    match first {
        Reply::Missing => Reply::Missing,
        Reply::Given(mut needs) => match second() {
            Reply::Given(more) => {
                needs.extend(more);
                Reply::Given(needs)
            }
            other => other,
        },
        Reply::Unreachable => match second() {
            Reply::Missing => Reply::Missing,
            _ => Reply::Unreachable,
        },
    }
}

/// The ruling of two policies, asked in order: a permit only where both permit. A refusal or a missing ruling from
/// either is the answer, even where the other can't be reached; the second isn't asked once the first refused.
fn ruling_of_both(first: Reply<Ruling>, second: impl FnOnce() -> Reply<Ruling>) -> Reply<Ruling> {
    match first {
        Reply::Given(Ruling::Permit) => second(),
        Reply::Unreachable => match second() {
            refused @ (Reply::Missing | Reply::Given(Ruling::Refuse(_))) => refused,
            _ => Reply::Unreachable,
        },
        refused => refused,
    }
}

/// Two policies, both of which must permit, asked in order. A refusal or a missing answer beats not known, so one
/// policy refusing is the answer even where the other can't be reached, and the first refusal asked is the one given.
/// Each ruling sees every input both named.
impl<A: Policy, B: Policy> Policy for (A, B) {
    fn needs(&mut self, asked: Asked<'_>) -> Reply<Vec<Need>> {
        let first = self.0.needs(asked);
        needs_of_both(first, || self.1.needs(asked))
    }

    fn rule(&mut self, asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
        let first = self.0.rule(asked, inputs);
        ruling_of_both(first, || self.1.rule(asked, inputs))
    }
}

/// One rule `SPEC.md` sets, which the engine asks of every call before the deployment's policy: the inputs it needs,
/// and its ruling.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rule {
    /// The rule, by the row that adds it and what it holds to. Only tests read it, until a rule's refusal is recorded
    /// with its name.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) name: &'static str,
    /// The inputs it needs.
    pub(crate) needs: fn(Asked<'_>) -> Reply<Vec<Need>>,
    /// Its ruling.
    pub(crate) rule: fn(Asked<'_>, &[Input]) -> Reply<Ruling>,
}

/// The fixed rules, in the order the engine asks them. Each later row of `F-021` adds its own entry here, and the lead
/// sequences those edits (`PLAN.md` §4.4). `T-021-01` adds none: its rules are the consultation itself.
pub(crate) const FIXED: &[Rule] = &[
    Rule {
        name: "T-021-03: every actor a request names acts only for the party the register says it acts for",
        needs: crate::roles::acts_for_needs,
        rule: crate::roles::acts_for_rule,
    },
    Rule {
        name: "T-021-02: a session is its holder's or its delegate's, bounded in time; a read is its reader's",
        needs: crate::session::needs,
        rule: crate::session::rule,
    },
    Rule {
        name: "T-021-03: only the operator posts a route; each operator permission alone allows its own action; \
               the operator acts for a user only at a reserve that declared it",
        needs: crate::roles::needs,
        rule: crate::roles::rule,
    },
];

/// The rules `SPEC.md` sets, which every engine asks first: [`Engine::with_policy`] puts them there, so no assembly can leave
/// one out. Among them, as between two policies, a refusal beats not known.
#[derive(Debug, Clone, Copy)]
pub struct Fixed {
    rules: &'static [Rule],
}

impl Default for Fixed {
    fn default() -> Self {
        Fixed { rules: FIXED }
    }
}

impl Policy for Fixed {
    fn needs(&mut self, asked: Asked<'_>) -> Reply<Vec<Need>> {
        self.rules
            .iter()
            .fold(Reply::Given(Vec::new()), |so_far, r| {
                needs_of_both(so_far, || (r.needs)(asked))
            })
    }

    fn rule(&mut self, asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
        self.rules
            .iter()
            .fold(Reply::Given(Ruling::Permit), |so_far, r| {
                ruling_of_both(so_far, || (r.rule)(asked, inputs))
            })
    }
}

/// No policy service and no sources: each can't be reached, so every decision is not known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Absent;

impl Policy for Absent {
    fn needs(&mut self, _: Asked<'_>) -> Reply<Vec<Need>> {
        Reply::Unreachable
    }

    fn rule(&mut self, _: Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        Reply::Unreachable
    }
}

impl Sources for Absent {
    fn supply(&mut self, _: &Need) -> Reply<Said> {
        Reply::Unreachable
    }
}

/// `CMP-CORE-AUTHZ`'s policy engine: the log it appends to, the fixed rules and the deployment's policy service it
/// consults, and the sources of the inputs its decisions need. It implements [`Decisions`]; each other policy trait
/// is its own row's, implemented in that row's module (`PLAN.md` §4.4).
#[derive(Debug)]
pub struct Engine<L, P, S> {
    pub(crate) log: L,
    pub(crate) policy: (Fixed, P),
    pub(crate) sources: S,
    /// The sessions opened and the read permissions granted and revoked (`T-021-02`).
    pub(crate) register: crate::session::Register,
    /// The authorisations to move funds permitted (`T-021-04`).
    pub(crate) authorised: crate::funds::Authorised,
}

/// The engine with no policy service and no sources. Its decisions are not known, and each policy trait whose row
/// hasn't built it yet is a stub on this type, in that row's module, that permits nothing: every call is appended to
/// the log first, and answered not known where its answer can say so, refused or over its bound where it can't
/// (`REQ-AUZ-5`). Where the log takes no entry it answers nothing at all.
pub type Unbuilt<L> = Engine<L, Absent, Absent>;

impl<L> Unbuilt<L> {
    /// The engine with no policy service and no sources, appending to `log`, as the assembly builds it until a
    /// deployment's policy is handed in.
    pub fn new(log: L) -> Self {
        Engine::with_policy(log, Absent, Absent)
    }
}

/// Why the unbuilt stubs know nothing.
pub(crate) const UNREACHABLE: NotKnownCause = NotKnownCause::PolicyUnreachable;

impl<L, P, S> Engine<L, P, S> {
    /// The engine that appends to `log`, consults the fixed rules and then `policy`, and asks `sources` for each
    /// input.
    pub fn with_policy(log: L, policy: P, sources: S) -> Self {
        Engine {
            log,
            policy: (Fixed::default(), policy),
            sources,
            register: crate::session::Register::default(),
            authorised: crate::funds::Authorised::default(),
        }
    }

    /// The engine with `fixed` in place of the rules [`FIXED`] lists, for a test to plant a fixed rule.
    #[cfg(test)]
    pub(crate) fn with_fixed(log: L, fixed: &'static [Rule], policy: P, sources: S) -> Self {
        Engine {
            log,
            policy: (Fixed { rules: fixed }, policy),
            sources,
            register: crate::session::Register::default(),
            authorised: crate::funds::Authorised::default(),
        }
    }

    /// The log it appends to.
    pub fn log(&self) -> &L {
        &self.log
    }
}

impl<L: PolicyLog, P, S> Engine<L, P, S> {
    /// Appends the verified entry of one answer, timed by Core's clock, through policy's handle: built, checked, made
    /// a [`DecisionEntry`] and appended, in that order. An entry that fails its check, or isn't a decision's, is
    /// refused before the log sees it.
    pub(crate) fn record(
        &mut self,
        who: &Actor,
        what: What,
        at: &Timestamp,
        outcome: Outcome,
    ) -> Result<Receipt, AppendFailure> {
        let entry = VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: who.clone(),
            what,
            when: When {
                at: at.clone(),
                source: TimeSource::CoreClock,
            },
            outcome,
        };
        entry.check().map_err(AppendFailure::Refused)?;
        let entry = DecisionEntry::try_from(entry).map_err(AppendFailure::Refused)?;
        self.log.append_decision(&entry)
    }

    /// Records `answer` to a decision-shaped call and then gives it, its decision id its request's. Where the log
    /// takes no entry, nothing is answered.
    pub(crate) fn answer(
        &mut self,
        request: &Id,
        who: &Actor,
        what: impl FnOnce(Id) -> What,
        at: &Timestamp,
        answer: Answer,
    ) -> Result<Decision, AppendFailure> {
        let receipt = self.record(who, what(request.clone()), at, answer.outcome())?;
        Ok(Decision {
            request: request.clone(),
            decision: request.clone(),
            outcome: answer,
            receipt,
        })
    }

    /// Answers not known to a decision-shaped call, as the unbuilt stubs do.
    pub(crate) fn not_known(
        &mut self,
        request: &Id,
        who: &Actor,
        what: impl FnOnce(Id) -> What,
        at: &Timestamp,
    ) -> Result<Decision, AppendFailure> {
        self.answer(
            request,
            who,
            what,
            at,
            Answer::NotKnown { cause: UNREACHABLE },
        )
    }
}

/// What one policy, fixed or the deployment's, answered once its inputs were asked.
enum Verdict {
    Permit,
    Refuse(RefusalClass),
    NotKnown(NotKnownCause),
}

/// Asks `policy` what `asked` needs, asks each input of its source, and has `policy` rule over them.
///
/// A provider that can't be reached gives `verifier_unreachable`: core-internal's `NotKnownCause` has no cause of
/// its own for a provider, and a provider answering eligibility stands where a verifier would (`REQ-CPL-6`).
fn verdict(policy: &mut impl Policy, sources: &mut impl Sources, asked: Asked<'_>) -> Verdict {
    let needs = match policy.needs(asked) {
        Reply::Given(needs) => needs,
        Reply::Missing => return Verdict::Refuse(RefusalClass::MissingInput),
        Reply::Unreachable => return Verdict::NotKnown(NotKnownCause::PolicyUnreachable),
    };
    let mut inputs = Vec::with_capacity(needs.len());
    for need in needs {
        match sources.supply(&need) {
            Reply::Given(said) => inputs.push(Input { need, said }),
            Reply::Missing => return Verdict::Refuse(RefusalClass::MissingInput),
            Reply::Unreachable => return Verdict::NotKnown(NotKnownCause::VerifierUnreachable),
        }
    }
    match policy.rule(asked, &inputs) {
        Reply::Given(Ruling::Permit) => Verdict::Permit,
        Reply::Given(Ruling::Refuse(class)) => Verdict::Refuse(class),
        Reply::Missing => Verdict::Refuse(RefusalClass::MissingInput),
        Reply::Unreachable => Verdict::NotKnown(NotKnownCause::PolicyUnreachable),
    }
}

impl<L, P: Policy, S: Sources> Engine<L, P, S> {
    /// The one consultation every call reaches: the fixed rules first, each with its inputs, then the deployment's
    /// policy service with its own. A refusal from either is the answer, even where the other can't be reached; the
    /// service isn't asked once a fixed rule refused. This is the only place in Core that mints a permitting answer.
    pub(crate) fn consult(&mut self, asked: Asked<'_>) -> Answer {
        let fixed = verdict(&mut self.policy.0, &mut self.sources, asked);
        let verdict = match fixed {
            Verdict::Refuse(_) => fixed,
            _ => match verdict(&mut self.policy.1, &mut self.sources, asked) {
                Verdict::Permit => fixed,
                deployment @ Verdict::Refuse(_) => deployment,
                deployment @ Verdict::NotKnown(_) => match fixed {
                    Verdict::NotKnown(_) => fixed,
                    _ => deployment,
                },
            },
        };
        match verdict {
            Verdict::Permit => Answer::Permit {},
            Verdict::Refuse(class) => Answer::Refuse { class },
            Verdict::NotKnown(cause) => Answer::NotKnown { cause },
        }
    }
}

/// A session answered on the engine's verdict: opened, as `opened`, only where it permits. The only place in Core that
/// writes an open session; the session it opens is `opened`, which the session's row names.
pub(crate) fn session_outcome(answer: Answer, opened: Opened) -> SessionOutcome {
    match answer {
        Answer::Permit {} => SessionOutcome::Opened {
            session: opened.session,
            expires_at: opened.expires_at,
        },
        Answer::Refuse { class } => SessionOutcome::Refused { class },
        Answer::NotKnown { cause } => SessionOutcome::NotKnown { cause },
    }
}

/// A read answered on the engine's verdict: allowed only where it permits. A read is allowed or refused and nothing
/// else (core-internal's `ReadOutcome`), so a read policy can't decide is refused: not permitted, since nothing showed
/// it was. The only place in Core that writes an allowed read.
pub(crate) fn read_outcome(answer: Answer) -> ReadOutcome {
    match answer {
        Answer::Permit {} => ReadOutcome::Allowed {},
        Answer::Refuse { class } => ReadOutcome::Refused { class },
        Answer::NotKnown { .. } => ReadOutcome::Refused {
            class: RefusalClass::NotPermitted,
        },
    }
}

/// Every action that moves value, posts a route or changes the catalogue is decided here, and only on the policy
/// service's ruling. A stuck order's value never comes this way: a request carrying `resolve_stuck` fails its check
/// and is refused unrecorded, since it travels only as a stuck move (`REQ-AUZ-4`).
impl<L: PolicyLog + crate::funds::Recorded, P: Policy, S: Sources> Decisions for Engine<L, P, S> {
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure> {
        request.check().map_err(AppendFailure::Refused)?;
        // A move acts only inside the authorisation in force for its order, as the log policy appends to holds it
        // (`T-021-04`), held after the consultation so its fixed rules' refusals stand.
        let consulted = self.consult(Asked::Decide(request));
        let answer = crate::funds::held(
            consulted,
            self.authorised.refuses_move(&mut self.log, &request.action),
        );
        let what = |decision| What::Decide {
            request: request.clone(),
            decision,
        };
        self.answer(&request.request, &request.caller, what, &request.at, answer)
    }
}

#[cfg(test)]
mod tests {
    /// A well-formed Ed25519 key for the catalogue changes that name one.
    const A_KEY: &str = "PT4_QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1w";

    /// A valid fixture of the contract, read as whatever type the call site wants.
    macro_rules! fixture {
        ($name:expr $(,)?) => {{
            let path = format!(
                "{}/../../contracts/core-internal/fixtures/valid/{}.json",
                env!("CARGO_MANIFEST_DIR"),
                $name
            );
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
        }};
    }

    use super::*;
    use crate::funds::{ReadFailure, Recorded};
    use rutent_contracts::core_internal::calls::{
        Authorisations, Charges, GeneralEntry, Log, PersonQuestions, Reads, Sessions, StuckOrders,
    };
    use rutent_contracts::core_internal::{
        Action, ActorKind, CatalogueChange, ChargeOutcome, Entry, Permit, PersonOutcome, RELATIONS,
        ReadOutcome, Role, SessionOutcome, Through, admit_entry,
    };
    use serde_json::{Value, json};

    /// A log that takes every entry, so the stub's answers can be seen.
    #[derive(Default)]
    struct Taking(Vec<Entry>);

    impl PolicyLog for Taking {
        fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
            self.0.push(Entry::Verified(entry.entry().clone()));
            Ok(fixture!("receipt-of-an-appended-entry"))
        }
    }

    impl Recorded for Taking {
        fn permitted(&mut self) -> Result<Vec<Authorisation>, ReadFailure> {
            Ok(held_and(&self.0))
        }
    }

    /// A log that takes nothing.
    struct Refusing;

    impl PolicyLog for Refusing {
        fn append_decision(&mut self, _: &DecisionEntry) -> Result<Receipt, AppendFailure> {
            Err(AppendFailure::Unavailable)
        }
    }

    impl Recorded for Refusing {
        fn permitted(&mut self) -> Result<Vec<Authorisation>, ReadFailure> {
            Err(ReadFailure::Unavailable)
        }
    }

    /// A log that holds every entry to the contract's admission, as `CMP-CORE-AUDIT`'s log must, on each of its two
    /// handles: an effect only on a permitting decision of this log for the same action, every other entry on what it
    /// cites, and a decision only through policy's handle.
    #[derive(Default)]
    struct Guarded(Vec<(Receipt, Entry)>);

    impl PolicyLog for Guarded {
        fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
            self.admit(Entry::Verified(entry.entry().clone()), Through::Policy)
        }
    }

    impl Recorded for Guarded {
        fn permitted(&mut self) -> Result<Vec<Authorisation>, ReadFailure> {
            Ok(held_and(self.0.iter().map(|(_, e)| e)))
        }
    }

    impl Log for Guarded {
        fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
            self.admit(entry.entry().clone(), Through::General)
        }
    }

    impl Guarded {
        fn admit(&mut self, entry: Entry, through: Through) -> Result<Receipt, AppendFailure> {
            let entry = &entry;
            entry.check().map_err(AppendFailure::Refused)?;
            if let Some((receipt, _)) = self.0.iter().find(|(_, e)| e == entry) {
                return Ok(receipt.clone());
            }
            // The operator party Core's questions are verified to, as the log's `StatementKeys::operator` answers.
            let operator = Id::new("operator").unwrap();
            admit_entry(&self.0, entry, through, &operator).map_err(AppendFailure::Refused)?;
            let index = self.0.len();
            let receipt: Receipt = serde_json::from_value(json!({
                "log": "log-test",
                "index": index,
                "entry": {"algorithm": "sha-256", "value": format!("{:064x}", index + 1)},
            }))
            .unwrap();
            self.0.push((receipt.clone(), entry.clone()));
            Ok(receipt)
        }
    }

    impl Guarded {
        /// A log holding the decision record the fixtures' posted route cites, so a `post_route` decision is
        /// admitted.
        fn with_a_decision_record() -> Self {
            let mut record: Value = fixture!("verified-entry-of-a-first-decision-record");
            record["what"]["order"] = json!("ord-18");
            record["what"]["record"]["value"] = json!("2b".repeat(32));
            let mut log = Guarded::default();
            log.append(&serde_json::from_value(record).unwrap())
                .unwrap();
            log
        }

        /// The outcome of the last entry, which must be the decision `decision` answered with.
        fn last_decided(&self, decision: &Decision) -> Outcome {
            let (receipt, Entry::Verified(entry)) = self.0.last().unwrap() else {
                panic!("a verified entry");
            };
            assert_eq!(
                receipt, &decision.receipt,
                "the answer carries its entry's receipt"
            );
            assert_eq!(entry.outcome, decision.outcome.outcome());
            entry.outcome
        }

        /// Appends the effect of `action` on `permit`, as orchestration would before it acts.
        fn effect(&mut self, action: &Action, permit: Permit) -> Result<Receipt, AppendFailure> {
            let entry = Entry::Verified(VerifiedEntry {
                kind: VerifiedKind::Verified,
                who: actor("rutent", "core-orch", Role::Core, ActorKind::Component),
                what: What::Effect {
                    action: action.clone(),
                    decision: permit,
                },
                when: When {
                    at: Timestamp::new("2026-11-10T09:00:01Z").unwrap(),
                    source: TimeSource::CoreClock,
                },
                outcome: Outcome::ToTakeEffect {},
            });
            self.append(&GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?)
        }
    }

    /// What a test's log reads as permitted (`T-021-04`): the fixtures' authorisation, `auth-17` for `ord-17`, which
    /// the log held before the test began, so the fixtures' move, which acts inside it, is decided on the rules these
    /// tests drive; then each authorisation the log took as permitted.
    fn held_and<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Vec<Authorisation> {
        let mut held = vec![fixture!("authorisation-of-one-order-with-a-floor")];
        held.extend(crate::funds::permitted_in(entries));
        held
    }

    /// A permit for `decision`, built by hand whatever it answered: what a path around policy would hold.
    fn forged(decision: &Decision) -> Permit {
        serde_json::from_value(json!({
            "request": decision.request,
            "decision": decision.decision,
            "answer": "permit",
            "receipt": decision.receipt,
        }))
        .unwrap()
    }

    /// The log's refusal of an effect whose permit names no permitting decision of it.
    fn no_permitting_decision() -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Refused(
            rutent_contracts::core_internal::Violation(RELATIONS[0].refusal),
        ))
    }

    fn id(text: &str) -> Id {
        Id::new(text).unwrap()
    }

    fn actor(party: &str, acts: &str, role: Role, kind: ActorKind) -> Actor {
        Actor {
            party: id(party),
            acts: id(acts),
            role,
            kind,
        }
    }

    /// How a test's policy service rules.
    type Ruler = Box<dyn FnMut(Asked<'_>, &[Input]) -> Reply<Ruling>>;

    /// A policy service whose answers the test sets, and which keeps what it ruled on.
    struct Service {
        needs: Reply<Vec<Need>>,
        rule: Ruler,
        asked: usize,
        ruled: Vec<(Actor, Vec<Input>)>,
    }

    impl Service {
        /// A service that needs `needs` and rules with `rule`.
        fn new(
            needs: Reply<Vec<Need>>,
            rule: impl FnMut(Asked<'_>, &[Input]) -> Reply<Ruling> + 'static,
        ) -> Self {
            Service {
                needs,
                rule: Box::new(rule),
                asked: 0,
                ruled: Vec::new(),
            }
        }

        /// A service that needs nothing and always rules `ruling`.
        fn ruling(ruling: Reply<Ruling>) -> Self {
            Service::new(Reply::Given(Vec::new()), move |_, _| ruling.clone())
        }
    }

    impl Policy for Service {
        fn needs(&mut self, _: Asked<'_>) -> Reply<Vec<Need>> {
            self.asked += 1;
            self.needs.clone()
        }

        fn rule(&mut self, asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
            self.ruled.push((asked.caller().clone(), inputs.to_vec()));
            (self.rule)(asked, inputs)
        }
    }

    /// Verifiers and providers whose answers the test sets, input by input, and which keep what they were asked.
    ///
    /// The deployment's register lists Core's own actor, `core-orch`, as carrying the user's authorisation onward,
    /// and says every actor the fixtures name acts for the party it states (`T-021-03`), so the fixtures' requests pass
    /// the fixed rules on every engine here. Those words are the register's, answered here and kept out of `asked`,
    /// which counts the inputs the tests set.
    #[derive(Default)]
    struct Given {
        says: Vec<(Need, Reply<Said>)>,
        asked: Vec<Need>,
    }

    impl Sources for Given {
        fn supply(&mut self, need: &Need) -> Reply<Said> {
            if need == &core_carries_onward() || matches!(need.from, Source::ActsFor { .. }) {
                return Reply::Given(Said::Yes);
            }
            self.asked.push(need.clone());
            self.says
                .iter()
                .find(|(n, _)| n == need)
                .map_or(Reply::Missing, |(_, said)| said.clone())
        }
    }

    /// The register's word that Core's `core-orch` carries the user's authorisation onward.
    fn core_carries_onward() -> Need {
        Need {
            input: id("core-orch"),
            from: Source::Permissions {
                party: id("rutent"),
                permission: crate::roles::Permission::CarryOnward,
            },
        }
    }

    fn a_verifiers_judgement() -> Need {
        Need {
            input: id("kyc-basic"),
            from: Source::Verifier(id("kyc-check")),
        }
    }

    fn a_providers_eligibility() -> Need {
        Need {
            input: id("eligibility"),
            from: Source::Provider(id("bank-co")),
        }
    }

    /// The request to move value, with `action` in its place: asked by the operator's login where only the operator
    /// may ask for it (`T-021-03`), by Core otherwise.
    fn request(action: Action) -> DecisionRequest {
        let mut request: DecisionRequest = fixture!("decision-request-to-move-value");
        if matches!(
            action,
            Action::PostRoute { .. } | Action::ChangeCatalogue { .. }
        ) {
            request.caller = actor("operator", "op-login-2", Role::Operator, ActorKind::Person);
        }
        request.action = action;
        request
    }

    /// Sources that say yes to every input the fixed rules need for each of `requests`, so that what is decided is
    /// the deployment's policy's ruling.
    fn yes_to_the_fixed_rules<'a>(
        requests: impl IntoIterator<Item = &'a DecisionRequest>,
    ) -> Given {
        let mut given = Given::default();
        for request in requests {
            let Reply::Given(needs) = Fixed::default().needs(Asked::Decide(request)) else {
                panic!("the fixed rules name their inputs");
            };
            given
                .says
                .extend(needs.into_iter().map(|n| (n, Reply::Given(Said::Yes))));
        }
        given
    }

    /// Every action a decision request carries, each catalogue change once and a posted route with and without its
    /// suggester. The matches have no catch-all, so an action or a change a contract bump adds fails this build until
    /// it is listed.
    fn every_decided_action() -> Vec<Action> {
        let moved: DecisionRequest = fixture!("decision-request-to-move-value");
        let move_value = moved.action;
        let suggested: DecisionRequest = fixture!("decision-request-to-post-a-suggested-route");
        let mut posted = suggested.action.clone();
        if let Action::PostRoute { suggested_by, .. } = &mut posted {
            *suggested_by = None;
        }
        let mut actions = vec![move_value, suggested.action, posted];
        for change in [
            CatalogueChange::Grant,
            CatalogueChange::RevokeGrant,
            CatalogueChange::AcceptModule,
            CatalogueChange::RevokeRelease,
            CatalogueChange::MarkStale,
            CatalogueChange::DeclareExploited,
            CatalogueChange::Reinstate,
            CatalogueChange::WithdrawRelease,
            CatalogueChange::SetSlippage,
            CatalogueChange::Promote,
        ] {
            match change {
                CatalogueChange::Grant
                | CatalogueChange::RevokeGrant
                | CatalogueChange::AcceptModule
                | CatalogueChange::RevokeRelease
                | CatalogueChange::MarkStale
                | CatalogueChange::DeclareExploited
                | CatalogueChange::Reinstate
                | CatalogueChange::WithdrawRelease
                | CatalogueChange::SetSlippage
                | CatalogueChange::Promote => {}
            }
            actions.push(Action::ChangeCatalogue {
                change,
                provider: id("bridge-pay"),
                version: change.is_about_a_release().then(|| id("2.1.0")),
                tuple: change.is_about_a_tuple().then(|| id("usd-cop")),
                advertising_keys: if change == CatalogueChange::Grant {
                    vec![rutent_contracts::provider_role::PublicKey::new(A_KEY).unwrap()]
                } else {
                    vec![]
                },
                statement_key: (change == CatalogueChange::AcceptModule)
                    .then(|| rutent_contracts::provider_role::PublicKey::new(A_KEY).unwrap()),
            });
        }
        for action in &actions {
            match action {
                Action::MoveValue { .. }
                | Action::PostRoute { .. }
                | Action::ChangeCatalogue { .. } => {}
                // A decided action too (`REQ-AUD-13`), which policy's role table answers no rule for yet: it joins
                // this list when policy's rows add the operator's permission to erase (core-internal's README).
                Action::EraseStatement { .. } => unreachable!("not yet in policy's tables"),
                Action::ResolveStuck { .. } => unreachable!("travels only as a stuck move"),
            }
        }
        actions
    }

    /// The outcome of the one entry the log took since the last look, which must pass its checks.
    fn last(policy: &mut Unbuilt<Taking>) -> Outcome {
        let entries: [Entry; 1] = std::mem::take(&mut policy.log.0).try_into().unwrap();
        let [Entry::Verified(entry)] = entries else {
            panic!("one verified entry");
        };
        assert_eq!(entry.check(), Ok(()));
        entry.outcome
    }

    /// The unbuilt engine permits nothing on any call, records each answer first, and answers nothing when the log
    /// takes no entry.
    // verifies: REQ-AUZ-1, REQ-AUZ-5, INV-7
    #[test]
    fn the_unbuilt_engine_permits_nothing_on_any_call_and_records_first() {
        let not_known = Outcome::NotKnown { cause: UNREACHABLE };
        let refused = |class| Outcome::Refused { class };
        let mut policy = Unbuilt::new(Taking::default());

        // Core's move waits on the register's word that its actor carries the authorisation onward (`T-021-03`),
        // which no source can give; the move is inside the authorisation the log holds (`T-021-04`).
        let decision = policy
            .decide(&fixture!("decision-request-to-move-value"))
            .unwrap();
        assert!(decision.permit().is_none());
        let unreachable = Outcome::NotKnown {
            cause: NotKnownCause::VerifierUnreachable,
        };
        assert_eq!(last(&mut policy), unreachable);
        // The authorisation, the one the log holds given again, waits on the register's word that its giver acts for
        // the party it states (`T-021-03`).
        let decision = policy
            .authorise(&fixture!("authorisation-of-one-order-with-a-floor"))
            .unwrap();
        assert!(decision.permit().is_none());
        assert_eq!(last(&mut policy), unreachable);
        let decision = policy
            .decide_stuck(&fixture!("stuck-decide-naming-its-evidence"))
            .unwrap();
        assert!(decision.permit().is_none());
        assert_eq!(last(&mut policy), not_known);
        let decision = policy
            .move_stuck(&fixture!("stuck-move-carrying-out-a-decision"))
            .unwrap();
        assert!(decision.permit().is_none());
        assert_eq!(last(&mut policy), not_known);

        // The delegate's session waits on its holder's word, which no source can give.
        policy.set_clock(crate::session::Stopped::at(
            &Timestamp::new("2026-11-10T09:00:00Z").unwrap(),
        ));
        let session = policy
            .open_session(&fixture!("session-request-of-a-delegate"))
            .unwrap();
        assert!(matches!(session.outcome, SessionOutcome::NotKnown { .. }));
        assert_eq!(
            last(&mut policy),
            Outcome::NotKnown {
                cause: NotKnownCause::VerifierUnreachable
            }
        );
        let read = policy
            .check_read(&fixture!("read-check-by-a-provider-of-its-segment"))
            .unwrap();
        assert!(matches!(read.outcome, ReadOutcome::Refused { .. }));
        assert_eq!(last(&mut policy), refused(RefusalClass::NotPermitted));
        let grant: PermissionChange = fixture!("permission-change-granting-a-regulator-the-trail");
        // The grant waits on the register's word that its actors act for the parties they state (`T-021-03`).
        let granted = policy.grant_permission(&grant).unwrap();
        assert!(granted.permit().is_none());
        assert_eq!(
            last(&mut policy),
            Outcome::NotKnown {
                cause: NotKnownCause::VerifierUnreachable
            }
        );
        // Nothing was granted, so there is nothing to revoke.
        let revoked = policy.revoke_permission(&grant).unwrap();
        assert!(revoked.permit().is_none());
        assert_eq!(last(&mut policy), refused(RefusalClass::NotPermitted));
        let charge = policy
            .charge(&fixture!("charge-of-fanned-out-work"))
            .unwrap();
        assert!(matches!(charge.outcome, ChargeOutcome::Over { .. }));
        assert_eq!(last(&mut policy), refused(RefusalClass::OverBound));
        let answer = policy
            .ask_about_person(&fixture!(
                "person-question-on-the-persons-authority-first-asked",
            ))
            .unwrap();
        assert!(matches!(answer.outcome, PersonOutcome::NotKnown { .. }));
        assert_eq!(last(&mut policy), not_known);

        let mut policy = Unbuilt::new(Refusing);
        assert_eq!(
            policy.decide(&fixture!("decision-request-to-move-value")),
            Err(AppendFailure::Unavailable)
        );
        assert_eq!(
            policy.charge(&fixture!("charge-of-fanned-out-work")),
            Err(AppendFailure::Unavailable)
        );
    }

    /// Every action a decision request carries, value moved, route posted with or without its suggester and each
    /// catalogue change, is refused and recorded when policy denies it, and the log admits no effect on it. The same
    /// actions permitted are admitted, so the refusal is policy's. A stuck order's value doesn't come this way at
    /// all: its request fails its check and nothing is recorded.
    // verifies: REQ-AUZ-1
    #[test]
    fn every_value_moving_action_is_refused_when_policy_denies() {
        let denies = || Service::ruling(Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted)));
        let actions = every_decided_action();
        assert_eq!(actions.len(), 13);
        let requests: Vec<DecisionRequest> = actions.iter().cloned().map(request).collect();
        let mut policy = Engine::with_policy(
            Guarded::with_a_decision_record(),
            denies(),
            yes_to_the_fixed_rules(&requests),
        );
        for action in &actions {
            let decision = policy.decide(&request(action.clone())).unwrap();
            assert_eq!(
                decision.outcome,
                Answer::Refuse {
                    class: RefusalClass::NotPermitted
                },
                "{action:?}"
            );
            assert!(decision.permit().is_none());
            assert_eq!(
                policy.log.last_decided(&decision),
                Outcome::Refused {
                    class: RefusalClass::NotPermitted
                }
            );
            assert_eq!(
                policy.log.effect(action, forged(&decision)),
                no_permitting_decision(),
                "{action:?}"
            );
        }
        assert_eq!(policy.policy.1.ruled.len(), actions.len());

        let permits = Service::ruling(Reply::Given(Ruling::Permit));
        let mut policy = Engine::with_policy(
            Guarded::with_a_decision_record(),
            permits,
            yes_to_the_fixed_rules(&requests),
        );
        for action in &actions {
            let decision = policy.decide(&request(action.clone())).unwrap();
            let permit = decision.permit().expect("a permit");
            let effect = policy.log.effect(action, permit);
            match action {
                // Its permit holds, and the log goes on to want the authorisation and acceptance it rests on.
                Action::MoveValue { .. } => assert!(
                    matches!(effect, Err(AppendFailure::Refused(v)) if v.0 != RELATIONS[0].refusal)
                ),
                _ => assert!(effect.is_ok(), "{action:?}"),
            }
        }

        let resolve: rutent_contracts::core_internal::Effect =
            fixture!("effect-resolving-a-stuck-order");
        let entries = policy.log.0.len();
        assert!(matches!(
            policy.decide(&request(resolve.action)),
            Err(AppendFailure::Refused(_))
        ));
        assert_eq!(policy.log.0.len(), entries, "nothing recorded");
        assert_eq!(
            policy.policy.1.ruled.len(),
            actions.len(),
            "policy not asked"
        );
    }

    /// External authentication says who the caller is and nothing more: whatever role, kind or party a caller
    /// comes with, operator and Core's own parts included, the answer is policy's, and policy sees the caller as it
    /// came. A caller in the operator's role that policy doesn't know as an operator is refused.
    // verifies: AC-25, REQ-AUZ-1
    #[test]
    fn authenticated_caller_is_still_decided_by_policy() {
        let roles = [
            Role::Customer,
            Role::Operator,
            Role::Provider,
            Role::Verifier,
            Role::Regulator,
            Role::Checker,
            Role::Core,
        ];
        let kinds = [ActorKind::Person, ActorKind::Agent, ActorKind::Component];
        let mut requests = Vec::new();
        for role in roles {
            for kind in kinds {
                for action in every_decided_action() {
                    let mut asked = request(action);
                    asked.caller = actor("operator", "op-login-2", role, kind);
                    requests.push(asked);
                }
            }
        }
        // Every input the fixed rules name is given yes, so each caller they let through is the deployment's to decide.
        let denies = Service::ruling(Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted)));
        let mut policy = Engine::with_policy(
            Guarded::with_a_decision_record(),
            denies,
            yes_to_the_fixed_rules(&requests),
        );
        let mut callers = Vec::new();
        for asked in &requests {
            let caller = &asked.caller;
            let decision = policy.decide(asked).unwrap();
            assert!(decision.permit().is_none(), "{caller:?}");
            // `T-021-03`'s fixed rule lets only the operator post a route or change the catalogue, and only the user,
            // Core or the operator ask to move value; it refuses the rest before the deployment's policy is asked.
            let reaches = match asked.action {
                Action::MoveValue { .. } => {
                    matches!(caller.role, Role::Customer | Role::Core | Role::Operator)
                }
                _ => caller.role == Role::Operator,
            };
            if reaches {
                callers.push(caller.clone());
            } else {
                assert_eq!(
                    decision.outcome,
                    Answer::Refuse {
                        class: RefusalClass::NotPermitted
                    },
                    "{caller:?}"
                );
            }
        }
        assert_eq!(
            callers.len(),
            3 * 13 + 2 * 3,
            "the operator on every action, the user and Core on a move"
        );
        let seen: Vec<Actor> = policy
            .policy
            .1
            .ruled
            .iter()
            .map(|(a, _)| a.clone())
            .collect();
        assert_eq!(seen, callers, "policy decided every caller, as it came");

        // The deployment's policy knows one operator login; another claiming the operator's role is still refused.
        let knows_one_operator = Service::new(Reply::Given(Vec::new()), |asked, _| {
            let caller = asked.caller();
            Reply::Given(
                if caller.role == Role::Operator && caller.acts.as_str() == "op-login-2" {
                    Ruling::Permit
                } else {
                    Ruling::Refuse(RefusalClass::NotPermitted)
                },
            )
        });
        let mut stale: DecisionRequest = fixture!("decision-request-to-mark-a-release-stale");
        let mut other = stale.clone();
        other.request = id("req-103");
        other.caller.acts = id("op-login-9");
        // Both logins hold the permission the fixed rule asks for, so the refusal is the deployment's policy's.
        let mut policy = Engine::with_policy(
            Guarded::default(),
            knows_one_operator,
            yes_to_the_fixed_rules([&stale, &other]),
        );
        assert!(policy.decide(&stale).unwrap().permit().is_some());
        stale = other;
        let decision = policy.decide(&stale).unwrap();
        assert_eq!(
            decision.outcome,
            Answer::Refuse {
                class: RefusalClass::NotPermitted
            }
        );
        assert_eq!(
            policy.log.effect(&stale.action, forged(&decision)),
            no_permitting_decision()
        );
    }

    /// Each input taken away in turn, the policy service's own and its sources', gives a refusal for the missing
    /// input, recorded, with no input after it asked and nothing permitted. With every input in, policy permits.
    // verifies: REQ-AUZ-5
    #[test]
    fn decision_missing_an_input_is_a_refusal() {
        let yes = Reply::Given(Said::Yes);
        let both = || vec![a_verifiers_judgement(), a_providers_eligibility()];
        let sources = |judgement: Reply<Said>, eligibility: Reply<Said>| Given {
            says: vec![
                (a_verifiers_judgement(), judgement),
                (a_providers_eligibility(), eligibility),
            ],
            asked: Vec::new(),
        };
        let permits = |needs| Service::new(needs, |_, _| Reply::Given(Ruling::Permit));
        let missing = Answer::Refuse {
            class: RefusalClass::MissingInput,
        };
        let cases = [
            // The policy service names no inputs for the call.
            (
                permits(Reply::Missing),
                sources(yes.clone(), yes.clone()),
                0,
                false,
            ),
            // The verifier has no judgement.
            (
                permits(Reply::Given(both())),
                sources(Reply::Missing, yes.clone()),
                1,
                false,
            ),
            // The provider gives no yes or no.
            (
                permits(Reply::Given(both())),
                sources(yes.clone(), Reply::Missing),
                2,
                false,
            ),
            // The policy service has no ruling.
            (
                Service::new(Reply::Given(both()), |_, _| Reply::Missing),
                sources(yes.clone(), yes.clone()),
                2,
                true,
            ),
        ];
        for (n, (service, given, asked, ruled)) in cases.into_iter().enumerate() {
            let mut policy = Engine::with_policy(Guarded::default(), service, given);
            let request: DecisionRequest = fixture!("decision-request-to-move-value");
            let decision = policy.decide(&request).unwrap();
            assert_eq!(decision.outcome, missing, "case {n}");
            assert_eq!(policy.log.last_decided(&decision), missing.outcome());
            assert_eq!(policy.sources.asked.len(), asked, "case {n}: inputs asked");
            assert_eq!(!policy.policy.1.ruled.is_empty(), ruled, "case {n}: ruled");
            assert_eq!(
                policy.log.effect(&request.action, forged(&decision)),
                no_permitting_decision()
            );
        }

        let mut policy = Engine::with_policy(
            Guarded::default(),
            permits(Reply::Given(both())),
            sources(yes.clone(), yes),
        );
        let decision = policy
            .decide(&fixture!("decision-request-to-move-value"))
            .unwrap();
        assert_eq!(decision.outcome, Answer::Permit {});
    }

    /// The policy service unreachable, before it names its inputs or when it rules, and a verifier or a provider
    /// unreachable, each give not known, recorded with its cause, and nothing moves: the log admits no effect on it.
    /// The answer is appended first: where the log can't take it, no answer is given at all.
    // verifies: AC-17, REQ-AUZ-5
    #[test]
    fn unreachable_policy_service_gives_not_known_and_nothing_moves() {
        let yes = Reply::Given(Said::Yes);
        let both = || Reply::Given(vec![a_verifiers_judgement(), a_providers_eligibility()]);
        let sources = |judgement: Reply<Said>, eligibility: Reply<Said>| Given {
            says: vec![
                (a_verifiers_judgement(), judgement),
                (a_providers_eligibility(), eligibility),
            ],
            asked: Vec::new(),
        };
        let permits = |needs| Service::new(needs, |_, _| Reply::Given(Ruling::Permit));
        let policy_down = NotKnownCause::PolicyUnreachable;
        let verifier_down = NotKnownCause::VerifierUnreachable;
        let cases = [
            (
                permits(Reply::Unreachable),
                sources(yes.clone(), yes.clone()),
                policy_down,
            ),
            (
                permits(both()),
                sources(Reply::Unreachable, yes.clone()),
                verifier_down,
            ),
            (
                permits(both()),
                sources(yes.clone(), Reply::Unreachable),
                verifier_down,
            ),
            (
                Service::new(both(), |_, _| Reply::Unreachable),
                sources(yes.clone(), yes.clone()),
                policy_down,
            ),
        ];
        for (n, (service, given, cause)) in cases.into_iter().enumerate() {
            let mut policy = Engine::with_policy(Guarded::default(), service, given);
            let request: DecisionRequest = fixture!("decision-request-to-move-value");
            let decision = policy.decide(&request).unwrap();
            assert_eq!(decision.outcome, Answer::NotKnown { cause }, "case {n}");
            assert!(decision.permit().is_none());
            assert_eq!(
                policy.log.last_decided(&decision),
                Outcome::NotKnown { cause }
            );
            assert_eq!(
                policy.log.effect(&request.action, forged(&decision)),
                no_permitting_decision()
            );
        }

        let mut policy = Engine::with_policy(Refusing, permits(both()), sources(yes.clone(), yes));
        assert_eq!(
            policy.decide(&fixture!("decision-request-to-move-value")),
            Err(AppendFailure::Unavailable)
        );
        assert_eq!(
            policy.policy.1.ruled.len(),
            1,
            "policy ruled, and nothing was answered"
        );
    }

    /// A provider's yes and a verifier's yes are inputs to policy, not the decision: policy sees both, and refuses
    /// on them. Composed with a policy that would permit, a rule that refuses still refuses.
    // verifies: REQ-CPL-3
    #[test]
    fn policy_refuses_on_a_providers_yes() {
        let needs = || Reply::Given(vec![a_providers_eligibility(), a_verifiers_judgement()]);
        let yes = || Given {
            says: vec![
                (a_providers_eligibility(), Reply::Given(Said::Yes)),
                (a_verifiers_judgement(), Reply::Given(Said::Yes)),
            ],
            asked: Vec::new(),
        };
        let refuses = || {
            Service::new(needs(), |_, _| {
                Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted))
            })
        };
        let refused = Answer::Refuse {
            class: RefusalClass::NotPermitted,
        };
        let request: DecisionRequest = fixture!("decision-request-to-move-value");

        let mut policy = Engine::with_policy(Guarded::default(), refuses(), yes());
        let decision = policy.decide(&request).unwrap();
        assert_eq!(decision.outcome, refused);
        let [(_, inputs)] = &policy.policy.1.ruled[..] else {
            panic!("policy ruled once");
        };
        assert_eq!(
            inputs,
            &[
                Input {
                    need: a_providers_eligibility(),
                    said: Said::Yes,
                },
                Input {
                    need: a_verifiers_judgement(),
                    said: Said::Yes,
                },
            ]
        );
        assert_eq!(
            policy.log.effect(&request.action, forged(&decision)),
            no_permitting_decision()
        );

        let permits = Service::new(needs(), |_, _| Reply::Given(Ruling::Permit));
        let mut policy = Engine::with_policy(Guarded::default(), (permits, refuses()), yes());
        assert_eq!(policy.decide(&request).unwrap().outcome, refused);
        let permits = Service::new(needs(), |_, _| Reply::Given(Ruling::Permit));
        let mut policy = Engine::with_policy(Guarded::default(), (refuses(), permits), yes());
        assert_eq!(policy.decide(&request).unwrap().outcome, refused);

        let permits = Service::new(needs(), |_, _| Reply::Given(Ruling::Permit));
        let mut policy = Engine::with_policy(Guarded::default(), permits, yes());
        assert_eq!(policy.decide(&request).unwrap().outcome, Answer::Permit {});
    }

    /// No public entry point of Core's three crates reaches an effect without a decision. Over their source, test
    /// code aside: a permitting answer is minted in one place, the engine's consultation, and no decision is built
    /// elsewhere; an effect is recorded, and an action carried out through the outer libraries, only by
    /// orchestration, and only where a permit or the effect it rests on is in hand. Each rule is shown to catch a
    /// planted bypass. At run time the log is the guard: driven through every policy call, the unbuilt engine gives
    /// no permit the log admits an effect on.
    // verifies: REQ-AUZ-1
    #[test]
    fn no_public_entry_point_of_core_reaches_an_effect_without_a_decision() {
        entry_points::carry_out_is_pinned();
        entry_points::every_entry_point_of_core_holds();
        entry_points::each_rule_catches_its_planted_bypass();

        // Each decision-shaped call of the unbuilt engine: either the log took no entry and nothing was answered, or
        // the answer permits nothing and an effect on a permit forged from it is refused.
        let mut policy = Unbuilt::new(Guarded::with_a_decision_record());
        let grant: PermissionChange = fixture!("permission-change-granting-a-regulator-the-trail");
        let decisions = [
            policy.decide(&fixture!("decision-request-to-move-value")),
            policy.authorise(&fixture!("authorisation-of-one-order-with-a-floor")),
            policy.decide_stuck(&fixture!("stuck-decide-naming-its-evidence")),
            policy.move_stuck(&fixture!("stuck-move-carrying-out-a-decision")),
            policy.grant_permission(&grant),
            policy.revoke_permission(&grant),
        ];
        let moved: DecisionRequest = fixture!("decision-request-to-move-value");
        let mut answered = 0;
        for decision in decisions {
            let Ok(decision) = decision else { continue };
            answered += 1;
            assert!(decision.permit().is_none());
            assert_eq!(
                policy.log.effect(&moved.action, forged(&decision)),
                no_permitting_decision()
            );
        }
        assert!(
            answered >= 3,
            "the decide, authorise and grant calls answer"
        );
    }

    fn needs_nothing(_: Asked<'_>) -> Reply<Vec<Need>> {
        Reply::Given(Vec::new())
    }

    fn refuses(_: Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        Reply::Given(Ruling::Refuse(RefusalClass::NoAccountableParty))
    }

    fn cannot_be_reached(_: Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        Reply::Unreachable
    }

    /// A fixed rule that refuses every call, as a later row's might.
    const REFUSING: &[Rule] = &[Rule {
        name: "test: refuses",
        needs: needs_nothing,
        rule: refuses,
    }];

    /// A fixed rule whose ruling can't be reached, then one that refuses.
    const UNREACHABLE_THEN_REFUSING: &[Rule] = &[
        Rule {
            name: "test: can't be reached",
            needs: needs_nothing,
            rule: cannot_be_reached,
        },
        Rule {
            name: "test: refuses",
            needs: needs_nothing,
            rule: refuses,
        },
    ];

    /// A fixed rule whose ruling can't be reached.
    const CANT_BE_REACHED: &[Rule] = &[Rule {
        name: "test: can't be reached",
        needs: needs_nothing,
        rule: cannot_be_reached,
    }];

    /// The rules `SPEC.md` sets are asked before the deployment's policy, which every engine has from `Engine::with_policy`
    /// (`FIXED`, each row's entry; the planted rules below show how they combine):
    /// a fixed rule's refusal is the answer, the deployment's policy not even asked, and it beats the deployment's
    /// service being unreachable. A deployment's refusal likewise beats a fixed rule that can't be reached.
    // verifies: REQ-AUZ-1, REQ-AUZ-5
    #[test]
    fn fixed_rules_are_asked_first_and_their_refusal_beats_an_unreachable_service() {
        let engine = Engine::with_policy(Taking::default(), Absent, Absent);
        let names = |rules: &[Rule]| rules.iter().map(|r| r.name).collect::<Vec<_>>();
        assert_eq!(
            names(engine.policy.0.rules),
            names(FIXED),
            "every engine asks the fixed rules"
        );

        let request: DecisionRequest = fixture!("decision-request-to-move-value");
        let refused = |class| Answer::Refuse { class };
        let decide = |fixed: &'static [Rule], service: Service| {
            let mut engine =
                Engine::with_fixed(Guarded::default(), fixed, service, Given::default());
            let answer = engine.decide(&request).unwrap().outcome;
            (answer, engine.policy.1.asked)
        };
        let unreachable = || Service::new(Reply::Unreachable, |_, _| Reply::Unreachable);
        let permits = || Service::ruling(Reply::Given(Ruling::Permit));
        let denies = || Service::ruling(Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted)));

        assert_eq!(
            decide(REFUSING, unreachable()),
            (refused(RefusalClass::NoAccountableParty), 0)
        );
        assert_eq!(
            decide(REFUSING, permits()),
            (refused(RefusalClass::NoAccountableParty), 0)
        );
        assert_eq!(
            decide(UNREACHABLE_THEN_REFUSING, unreachable()),
            (refused(RefusalClass::NoAccountableParty), 0),
            "among the fixed rules too, a refusal beats not known"
        );
        assert_eq!(
            decide(CANT_BE_REACHED, denies()),
            (refused(RefusalClass::NotPermitted), 1)
        );
        assert_eq!(
            decide(CANT_BE_REACHED, permits()),
            (
                Answer::NotKnown {
                    cause: NotKnownCause::PolicyUnreachable
                },
                1
            )
        );
        assert_eq!(decide(&[], permits()), (Answer::Permit {}, 1));
    }

    /// Two policies composed: both are asked what they need, and either one's missing answer refuses, even where the
    /// other can't be reached; either one's unreachable answer, with nothing refusing, gives not known. A refusal in
    /// either ruling beats the other's being unreachable, and both must permit.
    // verifies: REQ-AUZ-5
    #[test]
    fn a_pair_of_policies_permits_only_where_both_do_and_a_refusal_beats_not_known() {
        let request: DecisionRequest = fixture!("decision-request-to-move-value");
        let decide = |first: Service, second: Service, sources: Given| {
            let mut engine = Engine::with_policy(Guarded::default(), (first, second), sources);
            let answer = engine.decide(&request).unwrap().outcome;
            (answer, engine.sources.asked)
        };
        let permits = |needs| Service::new(needs, |_, _| Reply::Given(Ruling::Permit));
        let nothing = || Reply::Given(Vec::new());
        let missing = Answer::Refuse {
            class: RefusalClass::MissingInput,
        };
        let not_known = Answer::NotKnown {
            cause: NotKnownCause::PolicyUnreachable,
        };
        let judged = || Given {
            says: vec![(a_verifiers_judgement(), Reply::Given(Said::Yes))],
            asked: Vec::new(),
        };

        // The second policy's needs are asked: one with no source to give it refuses.
        let (answer, asked) = decide(
            permits(nothing()),
            permits(Reply::Given(vec![a_providers_eligibility()])),
            Given::default(),
        );
        assert_eq!((answer, asked), (missing, vec![a_providers_eligibility()]));
        let (answer, asked) = decide(
            permits(Reply::Given(vec![a_verifiers_judgement()])),
            permits(nothing()),
            judged(),
        );
        assert_eq!(
            (answer, asked),
            (Answer::Permit {}, vec![a_verifiers_judgement()])
        );

        // Either one's needs missing or unreachable answers.
        assert_eq!(
            decide(permits(nothing()), permits(Reply::Missing), judged()).0,
            missing
        );
        assert_eq!(
            decide(permits(nothing()), permits(Reply::Unreachable), judged()).0,
            not_known
        );
        assert_eq!(
            decide(permits(Reply::Missing), permits(nothing()), judged()).0,
            missing
        );
        assert_eq!(
            decide(
                permits(Reply::Unreachable),
                permits(Reply::Missing),
                judged()
            )
            .0,
            missing
        );
        assert_eq!(
            decide(
                permits(Reply::Missing),
                permits(Reply::Unreachable),
                judged()
            )
            .0,
            missing
        );

        // Rulings: a refusal beats not known, in either order; both must permit.
        let refusing = || {
            Service::new(nothing(), |_, _| {
                Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted))
            })
        };
        let unreachable = || Service::new(nothing(), |_, _| Reply::Unreachable);
        let refused = Answer::Refuse {
            class: RefusalClass::NotPermitted,
        };
        assert_eq!(decide(unreachable(), refusing(), judged()).0, refused);
        assert_eq!(decide(refusing(), unreachable(), judged()).0, refused);
        assert_eq!(
            decide(unreachable(), permits(nothing()), judged()).0,
            not_known
        );
        assert_eq!(decide(permits(nothing()), refusing(), judged()).0, refused);
        assert_eq!(
            decide(permits(nothing()), permits(nothing()), judged()).0,
            Answer::Permit {}
        );
    }

    /// The guarded log, keeping which policy call each entry policy appended records, so a row of the table is tied
    /// to the call its closure makes.
    struct Seen {
        log: Guarded,
        called: Vec<&'static str>,
    }

    impl Default for Seen {
        fn default() -> Self {
            Seen {
                log: Guarded::with_a_decision_record(),
                called: Vec::new(),
            }
        }
    }

    impl PolicyLog for Seen {
        fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
            self.called.push(call_of(&entry.entry().what));
            self.log.append_decision(entry)
        }
    }

    impl Recorded for Seen {
        fn permitted(&mut self) -> Result<Vec<Authorisation>, ReadFailure> {
            self.log.permitted()
        }
    }

    /// The policy call a decision entry records.
    fn call_of(what: &What) -> &'static str {
        match what {
            What::Decide { .. } => "decide",
            What::Authorise { .. } => "authorise",
            What::StuckDecide { .. } => "decide_stuck",
            What::StuckMove { .. } => "move_stuck",
            What::ConfirmOutcome { .. } => "confirm_outcome",
            What::OpenSession { .. } => "open_session",
            What::CheckRead { .. } => "check_read",
            What::GrantPermission { .. } => "grant_permission",
            What::RevokePermission { .. } => "revoke_permission",
            What::Charge { .. } => "charge",
            What::AskAboutPerson { .. } => "ask_about_person",
            _ => "not a policy call",
        }
    }

    /// How a policy call came out: permitted, or not (refused, not known, over its bound, or no answer at all).
    type Permitted = bool;

    /// One policy call, driven on an engine that consults a policy that records being asked, or, where its row hasn't
    /// built it yet, on the unbuilt engine, whose stub must permit nothing.
    enum Call {
        Built(fn(&mut Engine<Seen, Service, Given>) -> Permitted),
        Unbuilt {
            row: &'static str,
            call: fn(&mut Unbuilt<Seen>) -> Permitted,
        },
    }

    /// Every method of core-internal's policy traits. A row that builds its call moves it from `Unbuilt` to `Built`.
    fn calls() -> Vec<(&'static str, Call)> {
        vec![
            (
                "decide",
                Call::Built(|e| {
                    e.decide(&fixture!("decision-request-to-move-value"))
                        .is_ok_and(|d| d.permit().is_some())
                }),
            ),
            (
                // The customer's own, which needs only the order's record to say its party requested the order.
                "authorise",
                Call::Built(|e| {
                    let authorisation: Authorisation =
                        fixture!("authorisation-of-one-order-with-a-floor");
                    let requested = Need {
                        input: authorisation.given_by.party.clone(),
                        from: Source::Requester(authorisation.order.clone()),
                    };
                    e.sources.says.push((requested, Reply::Given(Said::Yes)));
                    e.authorise(&authorisation)
                        .is_ok_and(|d| d.permit().is_some())
                }),
            ),
            (
                "decide_stuck",
                Call::Unbuilt {
                    row: "T-021-06",
                    call: |e| {
                        e.decide_stuck(&fixture!("stuck-decide-naming-its-evidence"))
                            .is_ok_and(|d| d.permit().is_some())
                    },
                },
            ),
            (
                "move_stuck",
                Call::Unbuilt {
                    row: "T-021-06",
                    call: |e| {
                        e.move_stuck(&fixture!("stuck-move-carrying-out-a-decision"))
                            .is_ok_and(|d| d.permit().is_some())
                    },
                },
            ),
            (
                "confirm_outcome",
                Call::Unbuilt {
                    row: "T-021-06",
                    call: |e| {
                        let entry: serde_json::Value =
                            fixture!("verified-entry-of-an-outcome-confirmed");
                        let request =
                            serde_json::from_value(entry["what"]["request"].clone()).unwrap();
                        e.confirm_outcome(&request)
                            .is_ok_and(|d| d.permit().is_some())
                    },
                },
            ),
            (
                // The holder's own session, which needs only the account's record to say it holds the account.
                "open_session",
                Call::Built(|e| {
                    let mut request: SessionRequest = fixture!("session-request-of-a-delegate");
                    request.requester.party = request.holder.clone();
                    let holds = Need {
                        input: request.holder.clone(),
                        from: Source::Account(request.account.clone()),
                    };
                    e.sources.says.push((holds, Reply::Given(Said::Yes)));
                    e.set_clock(crate::session::Stopped::at(&request.at));
                    e.open_session(&request)
                        .is_ok_and(|a| matches!(a.outcome, SessionOutcome::Opened { .. }))
                }),
            ),
            (
                // A read of a resource, which neither the log nor the fixed rule holds to a party.
                "check_read",
                Call::Built(|e| {
                    e.check_read(&fixture!("read-check-of-who-the-checker-is"))
                        .is_ok_and(|a| matches!(a.outcome, ReadOutcome::Allowed { .. }))
                }),
            ),
            (
                "grant_permission",
                Call::Built(|e| {
                    e.grant_permission(&fixture!(
                        "permission-change-granting-a-regulator-the-trail"
                    ))
                    .is_ok_and(|d| d.permit().is_some())
                }),
            ),
            (
                // A permission granted before, held by the log and the register without a call of the table's.
                "revoke_permission",
                Call::Built(|e| {
                    let change: PermissionChange =
                        fixture!("permission-change-granting-a-regulator-the-trail");
                    let granted: VerifiedEntry = fixture!("verified-entry-of-a-permission-granted");
                    e.log
                        .log
                        .append_decision(&DecisionEntry::try_from(granted).unwrap())
                        .unwrap();
                    e.register.grants.push(change.clone());
                    e.revoke_permission(&change)
                        .is_ok_and(|d| d.permit().is_some())
                }),
            ),
            (
                "charge",
                Call::Unbuilt {
                    row: "T-021-05",
                    call: |e| {
                        e.charge(&fixture!("charge-of-fanned-out-work"))
                            .is_ok_and(|a| matches!(a.outcome, ChargeOutcome::Within { .. }))
                    },
                },
            ),
            (
                "ask_about_person",
                Call::Unbuilt {
                    row: "T-021-07",
                    call: |e| {
                        e.ask_about_person(&fixture!(
                            "person-question-on-the-persons-authority-first-asked"
                        ))
                        .is_ok_and(|a| matches!(a.outcome, PersonOutcome::Allowed { .. }))
                    },
                },
            ),
        ]
    }

    /// Every trait of core-internal's calls, as policy's or not. A trait added to the contract fails
    /// [`policy_methods`] until it is classified here.
    const TRAITS: &[(&str, bool)] = &[
        ("Log", false),
        ("PolicyLog", false),
        ("Evidence", false),
        ("Decisions", true),
        ("Authorisations", true),
        ("Sessions", true),
        ("Reads", true),
        ("Charges", true),
        ("StuckOrders", true),
        ("PersonQuestions", true),
        ("Comparisons", false),
        ("Measures", false),
        ("Operator", false),
        ("KeptStatements", false),
        ("StatementKeys", false),
        ("Signatures", false),
    ];

    /// The methods of the policy traits in `source`, a version of core-internal's `calls.rs`, or the traits it
    /// declares that [`TRAITS`] doesn't classify, and those classified that it no longer declares.
    fn methods_of_policy(source: &str) -> Result<Vec<String>, (Vec<String>, Vec<String>)> {
        let parsed = syn::parse_file(source).unwrap();
        let mut methods = Vec::new();
        let mut declared = Vec::new();
        let mut unclassified = Vec::new();
        for item in &parsed.items {
            let syn::Item::Trait(t) = item else { continue };
            let name = t.ident.to_string();
            match TRAITS.iter().find(|(n, _)| *n == name) {
                None => unclassified.push(name.clone()),
                Some((_, true)) => {
                    for item in &t.items {
                        if let syn::TraitItem::Fn(f) = item {
                            methods.push(f.sig.ident.to_string());
                        }
                    }
                }
                Some((_, false)) => {}
            }
            declared.push(name);
        }
        let gone: Vec<String> = TRAITS
            .iter()
            .filter(|(n, _)| !declared.iter().any(|d| d == n))
            .map(|(n, _)| n.to_string())
            .collect();
        if !unclassified.is_empty() || !gone.is_empty() {
            return Err((unclassified, gone));
        }
        methods.sort();
        Ok(methods)
    }

    /// The methods of core-internal's policy traits, read from the contract's source, every trait in it classified.
    fn policy_methods() -> Vec<String> {
        let path = format!(
            "{}/../../contracts/src/core_internal/calls.rs",
            env!("CARGO_MANIFEST_DIR")
        );
        methods_of_policy(&std::fs::read_to_string(path).unwrap()).unwrap_or_else(
            |(unclassified, gone)| {
                panic!("traits unclassified: {unclassified:?}; classified but gone: {gone:?}")
            },
        )
    }

    /// Every policy call reaches the one consultation: driven against a policy that records being asked, each built
    /// call is asked of it, once, and answers as it rules. A call whose row hasn't built it yet is marked so, and its
    /// stub permits nothing. The table holds every method of core-internal's policy traits.
    // verifies: REQ-AUZ-1
    #[test]
    fn every_policy_call_reaches_the_one_consultation_or_is_marked_unbuilt() {
        let calls = calls();
        let mut named: Vec<String> = calls.iter().map(|(n, _)| n.to_string()).collect();
        named.sort();
        assert_eq!(named, policy_methods(), "the table holds every policy call");

        // A trait the contract adds, policy's or not, fails until it is classified.
        let path = format!(
            "{}/../../contracts/src/core_internal/calls.rs",
            env!("CARGO_MANIFEST_DIR")
        );
        let calls_rs = std::fs::read_to_string(path).unwrap();
        let waivers = format!("{calls_rs}\npub trait Waivers {{ fn waive(&mut self); }}\n");
        assert_eq!(
            methods_of_policy(&waivers),
            Err((vec!["Waivers".to_string()], Vec::new()))
        );
        let without_operator = calls_rs.replace("pub trait Operator", "pub trait Operators");
        assert_eq!(
            methods_of_policy(&without_operator),
            Err((vec!["Operators".to_string()], vec!["Operator".to_string()]))
        );
        let mut unbuilt = Vec::new();
        for (name, call) in calls {
            match call {
                Call::Built(call) => {
                    for (ruling, permitted) in [
                        (Ruling::Permit, true),
                        (Ruling::Refuse(RefusalClass::NotPermitted), false),
                    ] {
                        let service = Service::ruling(Reply::Given(ruling));
                        let mut engine =
                            Engine::with_policy(Seen::default(), service, Given::default());
                        assert_eq!(
                            call(&mut engine),
                            permitted,
                            "{name} answers as policy rules"
                        );
                        assert_eq!(engine.log.called, [name], "the row is the call it makes");
                        assert_eq!(engine.policy.1.asked, 1, "{name} consulted once");
                        assert_eq!(engine.policy.1.ruled.len(), 1, "{name} ruled once");
                    }
                }
                Call::Unbuilt { row, call } => {
                    let mut engine = Unbuilt::new(Seen::default());
                    assert!(
                        !call(&mut engine),
                        "{name}, {row}'s, permits nothing until it is built"
                    );
                    assert_eq!(engine.log.called, [name], "the row is the call it makes");
                    unbuilt.push(row);
                }
            }
        }
        unbuilt.dedup();
        assert_eq!(unbuilt, ["T-021-06", "T-021-05", "T-021-07"]);
    }

    /// A log reachable only through policy's handle, which keeps what it took.
    #[derive(Default)]
    struct PolicysHandleOnly(Vec<DecisionEntry>);

    impl PolicyLog for PolicysHandleOnly {
        fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
            self.0.push(entry.clone());
            Ok(fixture!("receipt-of-an-appended-entry"))
        }
    }

    impl Recorded for PolicysHandleOnly {
        fn permitted(&mut self) -> Result<Vec<Authorisation>, ReadFailure> {
            let entries: Vec<Entry> = self
                .0
                .iter()
                .map(|d| Entry::Verified(d.entry().clone()))
                .collect();
            Ok(held_and(&entries))
        }
    }

    /// The engine writes the log only through policy's handle: holding nothing but it, each of its calls appends a
    /// decision's entry and nothing else. An entry it would write that fails its check, or isn't a decision's, is
    /// refused before it reaches the handle, and nothing is appended.
    // verifies: REQ-AUZ-1, INV-7
    #[test]
    fn the_engine_appends_only_through_policys_handle() {
        let mut engine = Unbuilt::new(PolicysHandleOnly::default());
        let grant: PermissionChange = fixture!("permission-change-granting-a-regulator-the-trail");
        let _ = engine.decide(&fixture!("decision-request-to-move-value"));
        let _ = engine.authorise(&fixture!("authorisation-of-one-order-with-a-floor"));
        let _ = engine.decide_stuck(&fixture!("stuck-decide-naming-its-evidence"));
        let _ = engine.move_stuck(&fixture!("stuck-move-carrying-out-a-decision"));
        let _ = engine.open_session(&fixture!("session-request-of-a-delegate"));
        let _ = engine.check_read(&fixture!("read-check-by-a-provider-of-its-segment"));
        let _ = engine.grant_permission(&grant);
        let _ = engine.revoke_permission(&grant);
        let _ = engine.charge(&fixture!("charge-of-fanned-out-work"));
        let _ = engine.ask_about_person(&fixture!(
            "person-question-on-the-persons-authority-first-asked"
        ));
        assert_eq!(engine.log.0.len(), 10, "every call appended its entry");
        for entry in &engine.log.0 {
            assert!(rutent_contracts::core_internal::calls::is_decision(
                &entry.entry().what
            ));
        }

        let core = actor("rutent", "core-authz", Role::Core, ActorKind::Component);
        let at = Timestamp::new("2026-11-10T09:00:00Z").unwrap();
        let moved: DecisionRequest = fixture!("decision-request-to-move-value");
        let effect: rutent_contracts::core_internal::Effect =
            fixture!("effect-moving-value-on-a-permit");
        let not_a_decision = What::Effect {
            action: effect.action,
            decision: effect.decision,
        };
        assert_eq!(
            engine.record(&core, not_a_decision, &at, Outcome::ToTakeEffect {}),
            Err(AppendFailure::Refused(
                rutent_contracts::core_internal::Violation(
                    rutent_contracts::core_internal::calls::RECORD_ON_THE_POLICY_HANDLE
                )
            ))
        );
        let unchecked = What::Decide {
            request: moved.clone(),
            decision: moved.request.clone(),
        };
        assert!(matches!(
            engine.record(&core, unchecked, &at, Outcome::ToTakeEffect {}),
            Err(AppendFailure::Refused(v))
                if v.0 != rutent_contracts::core_internal::calls::RECORD_ON_THE_POLICY_HANDLE
        ));
        assert_eq!(engine.log.0.len(), 10, "nothing more reached the handle");
    }
}
