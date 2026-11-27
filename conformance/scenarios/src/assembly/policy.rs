//! Policy in the assembly (`T-024-02`): `CMP-CORE-AUTHZ`'s engine, its fixed rules first, over the deployment's
//! policy, with each input a decision needs answered by Core's own sources (`rutent_core_authz::core_sources`, `T-020-55`)
//! from Core's log and evidence store, beside the deployment's registers ([`Deployed`]), every policy call reaching it
//! through [`PolicySide`].
//!
//! **Where each source is answered from** (`F-021`'s notes; `T-020-55` moved every log-backed answer into Core, so the
//! assembly supplies only what is the deployment's):
//!
//! | Source | Answered by | Record |
//! |---|---|---|
//! | `Requester`, `OrderOrigin`, `OrderParties`, `SegmentOrder`, `SegmentProvider`, `Statement`, `Showing`, `Declarations`, `Kept`, `KeptAbout` | Core (`CoreSources`) | Core's log and its key register ([`LogRead`]), and the records a filing hands over before the log holds its order (`OpenedOrders`) |
//! | `ActsFor`, `Permissions` | the deployment ([`Deployed`]) | its registers ([`Registers`]), which `F-030` keeps in production; `ActsFor` binds each login's party, kind and role (`T-021-14`) |
//! | `AgentScope`, `Reaches`, `MeetsBelowOperator`, `OfOperator` | the deployment | the same registers: agents' scopes, and the chains of `ActsFor` links |
//! | `Account`, `Holder` | the deployment | the account's reserve's register of accounts, the deployment's in the assembly ([`Registers::accounts`]); no delegation is registered |
//! | `HoldingAccount` | the deployment | its register of holding accounts ([`Registers::holding_accounts`]), set where the operator installs a node's wallet and kept across a reopen as the deployment's own configuration, never read from the host's holdings Core carries across it |
//! | `Verifier` | the deployment | the verifier stand-in ([`Verifier`]): by default reached and silent, a missing input, so a decision that needs a judgement is refused; a scenario sets it out of reach, or to yes or no (`T-024-07`) |
//! | `Provider` | the deployment | nothing yet: a missing input, so a decision that needs a provider's answer is refused |
//! | `Consent` | the deployment | nothing yet: no register of a person's consents, so a question on the person's own authority is refused as missing its input (`T-021-07`) |
//!
//! **Policy's one handle.** The engine holds the log's policy handle and appends every decision there; [`PolicySide`]
//! only forwards to the engine, and orchestration holds the general handle alone. Core's sources read the log through
//! a shared borrow, and append nothing.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{
    AppendFailure, Authorisations, Charges, Decisions, PersonQuestions, Reads, Sessions,
    StuckOrders,
};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Authorisation, Charge, ChargeAnswer, Decision, DecisionRequest,
    Entry, PermissionChange, PersonAnswer, PersonQuestion, ReadAnswer, ReadCheck, RefusalClass,
    Role, SessionAnswer, SessionRequest,
};
use rutent_contracts::router_api::Id;
use rutent_core_authz::core_sources::{DeploymentRegisters, Registered};
use rutent_core_authz::engine::{self as authz, Engine, Input, Need, Reply, Ruling, Said, Source};
use rutent_core_authz::parties::Scope;
use rutent_core_authz::roles::Permission;

use super::CoreLog;
use crate::PolicyHandle;

/// The deployment's registers, which policy reads and `F-030` keeps in production: who each actor acts for, which
/// actor holds which permission, which party holds each account at its reserve, and whose wallet holds each public
/// node's holding account.
#[derive(Debug, Clone, Default)]
pub struct Registers {
    /// Each actor, by its `acts`, the party it acts for and the kind of actor it is (`T-021-05`).
    pub actors: BTreeMap<String, (String, ActorKind)>,
    /// Each actor's roles, by its `acts`: the roles it is listed in for its party, as that kind (`T-021-14`).
    pub roles: BTreeMap<String, Vec<Role>>,
    /// Each agent's scope, by its party and its `acts`: the calls its party lets it make (`T-021-05`).
    pub scopes: Vec<(String, String, Scope)>,
    /// Each permission an actor holds, by its `acts`.
    pub permissions: Vec<(String, Permission)>,
    /// Each account, by reference, and the party that holds it.
    pub accounts: BTreeMap<String, String>,
    /// The register of holding accounts (`REQ-CAT-23`, `T-020-55`): each public inner node, by its identifier, and the
    /// provider whose wallet holds its account, as the operator installed it. The deployment's own configuration, kept
    /// across a reopen as the deployment keeps it, never read from the host's holdings Core carries across one.
    pub holding_accounts: BTreeMap<String, String>,
    /// The decimals the register gives the asset of each public node's holding account, set beside it (`T-020-63`): what
    /// the schedule's readings of the account are stated in. A node the register names with none is not read, and says so.
    pub holding_decimals: BTreeMap<String, u8>,
    /// The same register's private nodes' accounts (`REQ-CAT-23`, `T-020-48`): each private node, by its identifier,
    /// the publisher whose word its balance is, the module that answers for it and its asset, as the publisher gave the
    /// operator the way to it. Kept across a reopen with the rest of the register, so a reopened Core reads them again
    /// (the review's S1).
    pub publisher_accounts: BTreeMap<String, rutent_core_orch::schedule::PublisherAccount>,
}

impl Registers {
    /// Lists `actor` as acting for its party, as the kind of actor it is, in its role. Listed again for the same party
    /// and kind, it acts in that role too; listed for another party or kind, it acts in that role alone.
    pub fn list(&mut self, actor: &Actor) {
        let acts = actor.acts.as_str().to_owned();
        let listed = (actor.party.as_str().to_owned(), actor.kind);
        if self.actors.get(&acts) != Some(&listed) {
            self.roles.remove(&acts);
        }
        self.actors.insert(acts.clone(), listed);
        let roles = self.roles.entry(acts).or_default();
        if !roles.contains(&actor.role) {
            roles.push(actor.role);
        }
    }

    /// Lets the agent `agent` make the call `scope` for its party.
    pub fn scope(&mut self, agent: &Actor, scope: Scope) {
        self.scopes.push((
            agent.party.as_str().to_owned(),
            agent.acts.as_str().to_owned(),
            scope,
        ));
    }

    /// Lets `actor` hold `permission`.
    pub fn permit(&mut self, actor: &Actor, permission: Permission) {
        self.permissions
            .push((actor.acts.as_str().to_owned(), permission));
    }

    /// Whether `acts` acts for `party`, where the register lists it at all.
    pub fn acts_for(&self, acts: &Id, party: &Id) -> Option<bool> {
        self.actors
            .get(acts.as_str())
            .map(|(p, _)| p.as_str() == party.as_str())
    }

    /// Whether `acts` acts for `party` as the kind of actor `kind`, where the register lists it at all: policy's
    /// word (`Source::ActsFor`), which binds the kind too, so an agent's login can't state itself a person.
    /// Nothing but Core's own components acts for Core's party, or for a party that reaches it (`T-021-06`): any other
    /// listed there acts for no one.
    pub fn acts_as(&self, acts: &Id, party: &Id, kind: ActorKind) -> Option<bool> {
        let core = crate::kit::CORE_PARTY;
        self.actors.get(acts.as_str()).map(|(p, k)| {
            let reaches_core = self
                .chain(&Id::new(p).expect("a listed party"))
                .iter()
                .any(|c| c == core);
            let cores_own = p.as_str() == core && *k == ActorKind::Component;
            p.as_str() == party.as_str() && *k == kind && (!reaches_core || cores_own)
        })
    }

    /// Whether `acts` acts for `party` as the kind of actor `kind`, in the role `role`, where the register lists it at
    /// all: policy's word (`Source::ActsFor`), which binds the role too, so a customer's login can't state itself the
    /// operator, a regulator or a checker (`T-021-14`).
    pub fn acts_in(&self, acts: &Id, party: &Id, kind: ActorKind, role: Role) -> Option<bool> {
        self.acts_as(acts, party, kind).map(|yes| {
            yes && self
                .roles
                .get(acts.as_str())
                .is_some_and(|roles| roles.contains(&role))
        })
    }

    /// Everything `start`, a login or a party, reaches along the register's `ActsFor` links: itself, the party it is
    /// listed as acting for, and that party's own in turn, each once (`T-021-06`).
    pub fn chain(&self, start: &Id) -> Vec<String> {
        let mut reached = vec![start.as_str().to_owned()];
        while let Some((party, _)) = reached.last().and_then(|at| self.actors.get(at)) {
            if reached.contains(party) {
                break;
            }
            reached.push(party.clone());
        }
        reached
    }

    /// Whether the chains of `a` and `b`, each a login, meet below the party `root`: each cut where it reaches `root`,
    /// which isn't compared, nor anything above it. Policy's word (`Source::MeetsBelowOperator`), `root` the
    /// deployment operator's party.
    pub fn meet_below(&self, a: &Id, b: &Id, root: &Id) -> bool {
        let below = |start: &Id| -> Vec<String> {
            self.chain(start)
                .into_iter()
                .take_while(|p| p.as_str() != root.as_str())
                .collect()
        };
        let theirs = below(b);
        below(a).iter().any(|p| theirs.contains(p))
    }

    /// Whether `from`, a login or a party, reaches the party `to` along its chain: policy's word (`Source::Reaches`) on
    /// two actors, an actor and the order's customer, or an actor and the provider holding the funds (`T-021-06`).
    pub fn reaches(&self, from: &Id, to: &Id) -> bool {
        self.chain(from).iter().any(|p| p.as_str() == to.as_str())
    }

    /// Whether the agent `agent` may make the call `scope` for `party`, where its party set it a scope at all.
    pub fn in_scope(&self, party: &Id, agent: &Id, scope: Scope) -> Option<bool> {
        let set: Vec<Scope> = self
            .scopes
            .iter()
            .filter(|(p, a, _)| p.as_str() == party.as_str() && a.as_str() == agent.as_str())
            .map(|(_, _, s)| *s)
            .collect();
        (!set.is_empty()).then(|| set.contains(&scope))
    }
}

/// What the verifier stand-in does when policy asks it for a judgement: it is reached and says nothing (the default,
/// as the assembly had it: a missing input), can't be reached, or says yes or no (`T-024-07`, `AC-17`). The production
/// verifier is `F-053`'s; this stands in for the role only (`DEC-16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Verifier {
    /// Reached, and holds no judgement of the party asked about.
    #[default]
    Silent,
    /// Out of reach.
    Unreachable,
    /// Reached, and judges yes.
    Yes,
    /// Reached, and judges no.
    No,
}

/// The name the deployment's judgement rule asks the verifier stand-in under.
pub const VERIFIER: &str = "sim-verifier";

/// The input the deployment's judgement rule asks the verifier for.
pub const JUDGEMENT: &str = "payer-eligible";

fn said(yes: bool) -> Reply<Said> {
    Reply::Given(if yes { Said::Yes } else { Said::No })
}

/// The deployment's registers, as Core's sources ask them (`T-020-55`): only the sources that are the deployment's own
/// reach here, through `rutent_core_authz::core_sources::Registered`, which only Core's sources build.
#[derive(Clone, Default)]
pub struct Deployed {
    /// The deployment's registers.
    pub(crate) registers: Rc<RefCell<Registers>>,
    /// What the verifier stand-in does when policy asks it for a judgement (`T-024-07`).
    pub(crate) verifier: Rc<std::cell::Cell<Verifier>>,
}

impl Deployed {
    /// Sets what the verifier stand-in does from now on (`T-024-07`).
    pub fn verifier_is(&self, verifier: Verifier) {
        self.verifier.set(verifier);
    }
}

impl DeploymentRegisters for Deployed {
    fn registered(&mut self, asked: &Registered<'_>) -> Reply<Said> {
        let input = asked.input();
        // Out of reach where the registers are held mutably elsewhere at the moment, so policy is never stopped on a
        // borrow.
        let Ok(registers) = self.registers.try_borrow() else {
            return Reply::Unreachable;
        };
        match asked.source() {
            Source::ActsFor {
                acts,
                party,
                kind,
                role,
            } => match registers.acts_in(acts, party, *kind, *role) {
                Some(yes) => said(yes),
                None => Reply::Missing,
            },
            Source::AgentScope { party, scope } => match registers.in_scope(party, input, *scope) {
                Some(yes) => said(yes),
                None => Reply::Missing,
            },
            Source::Permissions { permission, .. } => said(
                registers
                    .permissions
                    .iter()
                    .any(|(a, p)| a == input.as_str() && p == permission),
            ),
            Source::Account(account) => match registers.accounts.get(account.as_str()) {
                Some(holder) => said(holder == input.as_str()),
                None => Reply::Missing,
            },
            // No delegation is registered: a holder names no delegate.
            Source::Holder { .. } => said(false),
            Source::Reaches { from, to } => said(registers.reaches(from, to)),
            Source::MeetsBelowOperator { from, with } => {
                said(registers.meet_below(from, with, &crate::kit::operator().party))
            }
            // The deployment's operator is the kit's: Core signs in its name.
            Source::OfOperator => said(registers.reaches(input, &crate::kit::operator().party)),
            Source::Verifier(_) => match self.verifier.get() {
                Verifier::Silent => Reply::Missing,
                Verifier::Unreachable => Reply::Unreachable,
                Verifier::Yes => said(true),
                Verifier::No => said(false),
            },
            // No provider's answer is a source in M3's assembly yet, and no register of a person's consents
            // (`T-021-07`): a question on the person's own authority is refused.
            Source::Provider(_) | Source::Consent { .. } => Reply::Missing,
            // The wallet holding the node's account, by the deployment's register of holding accounts (`T-020-55`): yes
            // for it, no for another provider, nothing for a node with no account.
            Source::HoldingAccount { node } => match registers.holding_accounts.get(node) {
                Some(provider) => said(provider == input.as_str()),
                None => Reply::Missing,
            },
            // Core's own: Core's sources answer them, and never ask here.
            _ => Reply::Missing,
        }
    }
}

/// Core's log, as Core's own sources read it (`T-020-55`): not known where it is held mutably elsewhere at the moment.
#[derive(Clone)]
pub struct LogRead(pub(crate) Rc<RefCell<CoreLog>>);

impl rutent_core_authz::core_sources::LogEntries for LogRead {
    fn read<T>(
        &mut self,
        read: impl FnOnce(&[(rutent_contracts::core_internal::Receipt, Entry)]) -> T,
    ) -> Result<T, rutent_core_authz::funds::ReadFailure> {
        let log = self
            .0
            .try_borrow()
            .map_err(|_| rutent_core_authz::funds::ReadFailure::Unavailable)?;
        Ok(read(log.entries()))
    }
}

/// Core's log's key register, as Core's own sources read it (`T-020-55`, the review of `8f7ddefd`): the advertisement
/// it accepted last for a provider, not known where the log is held mutably elsewhere at the moment.
impl rutent_core_authz::core_sources::AcceptedAdvertisements for LogRead {
    fn accepted(
        &mut self,
        provider: &Id,
    ) -> Result<
        Option<rutent_core_authz::core_sources::AcceptedAdvertisement>,
        rutent_core_authz::funds::ReadFailure,
    > {
        let log = self
            .0
            .try_borrow()
            .map_err(|_| rutent_core_authz::funds::ReadFailure::Unavailable)?;
        Ok(log.accepted_advertisement(provider.as_str()).map(|a| {
            rutent_core_authz::core_sources::AcceptedAdvertisement {
                commitment: a.commitment,
                declaration: a.declaration,
            }
        }))
    }
}

/// Core's own sources, as the assembly puts them together (`T-020-55`).
pub type CoreSources = rutent_core_authz::core_sources::CoreSources<LogRead, LogRead, Deployed>;

/// The deployment's own policy: it needs nothing beyond the fixed rules, and permits what they let through, unless
/// a scenario sets it to refuse.
#[derive(Debug, Clone, Default)]
pub struct Deployment {
    /// Refuses every call with this class, where set.
    pub refuse: Rc<std::cell::Cell<Option<RefusalClass>>>,
    /// Whether a move of value needs the verifier's judgement of the payer first (`T-024-07`, `AC-17`).
    judges_moves: Rc<std::cell::Cell<bool>>,
}

impl Deployment {
    /// Has the deployment's policy ask the verifier stand-in for its judgement of the payer before any value moves,
    /// and refuse where it says no.
    pub fn judge_before_moving(&self, on: bool) {
        self.judges_moves.set(on);
    }
}

impl authz::Policy for Deployment {
    fn needs(&mut self, asked: authz::Asked<'_>) -> Reply<Vec<Need>> {
        let moves = matches!(
            asked,
            authz::Asked::Decide(DecisionRequest {
                action: Action::MoveValue { .. },
                ..
            })
        );
        Reply::Given(if moves && self.judges_moves.get() {
            vec![Need {
                input: Id::new(JUDGEMENT).expect("an identifier"),
                from: Source::Verifier(Id::new(VERIFIER).expect("an identifier")),
            }]
        } else {
            Vec::new()
        })
    }

    fn rule(&mut self, _: authz::Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
        Reply::Given(match self.refuse.get() {
            Some(class) => Ruling::Refuse(class),
            None if inputs.iter().any(|i| i.said == Said::No) => {
                Ruling::Refuse(RefusalClass::NotPermitted)
            }
            None => Ruling::Permit,
        })
    }
}

/// The policy engine as the assembly builds it.
pub type PolicyEngine = Engine<PolicyHandle<CoreLog>, Deployment, CoreSources>;

/// Policy, as the other parts reach it: the engine, for every policy call (decisions, authorisations, sessions, reads,
/// charges, a stuck order's steps and a move's outcome, and questions about people, `T-021-07`'s, the last). Each call
/// forwards to the engine, which decides it through its one consultation, its fixed rules first and then the
/// deployment's policy ([`Deployment`], which a scenario may set to refuse), over Core's own sources and the
/// deployment's registers ([`CoreSources`], [`Deployed`]), and
/// records it on policy's handle before it answers. Nothing is answered beside the engine any more.
#[derive(Clone)]
pub struct PolicySide {
    engine: Rc<RefCell<PolicyEngine>>,
}

impl PolicySide {
    /// Policy over `engine`. The engine holds the sources it reads and the deployment's policy itself; `sources` and
    /// `deployment` are the ones it was built with, taken so the assembly's wiring names them, and nothing else
    /// answers from them.
    pub fn new(
        engine: Rc<RefCell<PolicyEngine>>,
        sources: CoreSources,
        deployment: Deployment,
    ) -> Self {
        let _ = (sources, deployment);
        Self { engine }
    }

    /// The engine.
    pub(crate) fn engine(&self) -> &Rc<RefCell<PolicyEngine>> {
        &self.engine
    }
}

impl Decisions for PolicySide {
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure> {
        self.engine.borrow_mut().decide(request)
    }
}

impl Sessions for PolicySide {
    fn open_session(&mut self, request: &SessionRequest) -> Result<SessionAnswer, AppendFailure> {
        self.engine.borrow_mut().open_session(request)
    }
}

impl Reads for PolicySide {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        self.engine.borrow_mut().check_read(check)
    }

    fn grant_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        self.engine.borrow_mut().grant_permission(change)
    }

    fn revoke_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        self.engine.borrow_mut().revoke_permission(change)
    }
}

/// The engine's own, since `T-021-04` built it: decided through its one consultation, held to the register it reads
/// from the log on its policy handle, and recorded there before it answers.
impl Authorisations for PolicySide {
    fn authorise(&mut self, a: &Authorisation) -> Result<Decision, AppendFailure> {
        self.engine.borrow_mut().authorise(a)
    }
}

/// The engine's own, since `T-021-05` built it: decided through its one consultation, held to the party's bound in
/// the counter it rebuilds from the log on its policy handle, and recorded there before it answers.
impl Charges for PolicySide {
    fn charge(&mut self, charge: &Charge) -> Result<ChargeAnswer, AppendFailure> {
        self.engine.borrow_mut().charge(charge)
    }
}

/// The engine's own: a stuck order's two steps, and a move's outcome reported and confirmed, each recorded through
/// policy's handle and answered as the engine answers it, on `T-021-06`'s rules.
impl StuckOrders for PolicySide {
    fn decide_stuck(
        &mut self,
        request: &rutent_contracts::core_internal::StuckDecide,
    ) -> Result<Decision, AppendFailure> {
        self.engine.borrow_mut().decide_stuck(request)
    }

    fn move_stuck(
        &mut self,
        request: &rutent_contracts::core_internal::StuckMove,
    ) -> Result<Decision, AppendFailure> {
        self.engine.borrow_mut().move_stuck(request)
    }

    fn report_outcome(
        &mut self,
        request: &rutent_contracts::core_internal::OutcomeReport,
    ) -> Result<Decision, AppendFailure> {
        self.engine.borrow_mut().report_outcome(request)
    }

    fn confirm_outcome(
        &mut self,
        request: &rutent_contracts::core_internal::OutcomeConfirmation,
    ) -> Result<Decision, AppendFailure> {
        self.engine.borrow_mut().confirm_outcome(request)
    }
}

/// A decline of a move's outcome report (`REQ-AUZ-4`): the engine decides it since `T-021-09`, as it decides a
/// confirmation, so the call goes to it as the others do (`T-024-19`; it answered unavailable before).
impl rutent_contracts::core_internal::calls::OutcomeDeclines for PolicySide {
    fn decline_outcome(
        &mut self,
        request: &rutent_contracts::core_internal::OutcomeDecline,
    ) -> Result<Decision, AppendFailure> {
        self.engine.borrow_mut().decline_outcome(request)
    }
}

/// The engine's own, since `T-021-07` built it: decided through its one consultation, held to the shortlist, the
/// party's charges and the set asked that it reads from the log on its policy handle, and recorded there before it
/// answers.
impl PersonQuestions for PolicySide {
    fn ask_about_person(
        &mut self,
        question: &PersonQuestion,
    ) -> Result<PersonAnswer, AppendFailure> {
        self.engine.borrow_mut().ask_about_person(question)
    }
}

/// The receipts policy's sources and stand-ins read are of the log they share: a check that nothing but the
/// engine's handle writes a decision, kept for the tests.
pub fn decisions_in(log: &CoreLog) -> usize {
    log.entries()
        .iter()
        .filter(|(_, e)| matches!(e, Entry::Verified(v) if rutent_contracts::core_internal::calls::is_decision(&v.what)))
        .count()
}
