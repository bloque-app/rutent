//! Policy in the assembly (`T-024-02`): `CMP-CORE-AUTHZ`'s engine, its fixed rules first, over the deployment's
//! policy, with each input a decision needs answered from a real record ([`Records`]), and the policy calls the
//! engine doesn't build yet answered beside it, on policy's side of the log ([`PolicySide`]).
//!
//! **Where each source is answered from** (`F-021`'s notes):
//!
//! | Source | Record |
//! |---|---|
//! | `ActsFor`, `Permissions` | the deployment's registers ([`Registers`]), which `F-030` keeps in production |
//! | `Account`, `Holder` | the account's reserve's register of accounts, the deployment's in the assembly ([`Registers::accounts`]); no delegation is registered |
//! | `Requester`, `OrderOrigin` | intake's record of the order (`Intake::record`), copied once the order opens |
//! | `OrderParties` | the order's acceptance, as the log holds it |
//! | `SegmentOrder` | the plan that lists the segment: the order's `route_chosen` input, as the log holds it |
//! | `SegmentProvider` | the effect that moved value on the segment, as the log holds it |
//! | `Statement` | the stated entry naming the commitment: its maker's party, as the log attributed it |
//! | `Declarations` | the reserve's advertisement, as the assembly listed it in the log: its reserve declaration |
//! | `Verifier`, `Provider` | nothing yet: a missing input, so a decision that needs a judgement is refused |
//!
//! **Policy's one handle.** The engine holds the log's policy handle; [`PolicySide`] reaches it only through the
//! engine (`Engine::log`), so the calls it answers beside the engine append on the same, one, policy handle, and
//! orchestration holds the general handle alone.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{
    AppendFailure, Authorisations, Charges, DecisionEntry, Decisions, PersonQuestions, PolicyLog,
    Reads, Sessions,
};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, Authorisation, Charge, ChargeAnswer, ChargeOutcome, Decision,
    DecisionRequest, Entry, NotKnownCause, Outcome, PermissionChange, PersonAnswer, PersonOutcome,
    PersonQuestion, ReadAnswer, ReadCheck, Receipt, RefusalClass, SessionAnswer, SessionRequest,
    TimeSource, VerifiedEntry, VerifiedKind, What, When,
};
use rutent_contracts::provider_role::{ReserveDeclaration, SafeInteger};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::Observed;
use rutent_core_authz::engine::{self as authz, Engine, Input, Need, Reply, Ruling, Said, Source};
use rutent_core_authz::roles::{AUTOMATIC_DEPOSIT, NO_USER_WITHDRAWAL, Permission};
use rutent_core_orch::intake::OrderRecord;

use super::CoreLog;
use crate::PolicyHandle;

/// The deployment's registers, which policy reads and `F-030` keeps in production: who each actor acts for, which
/// actor holds which permission, and which party holds each account at its reserve.
#[derive(Debug, Clone, Default)]
pub struct Registers {
    /// Each actor, by its `acts`, and the party it acts for.
    pub actors: BTreeMap<String, String>,
    /// Each permission an actor holds, by its `acts`.
    pub permissions: Vec<(String, Permission)>,
    /// Each account, by reference, and the party that holds it.
    pub accounts: BTreeMap<String, String>,
}

impl Registers {
    /// Lists `actor` as acting for its party.
    pub fn list(&mut self, actor: &Actor) {
        self.actors.insert(
            actor.acts.as_str().to_owned(),
            actor.party.as_str().to_owned(),
        );
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
            .map(|p| p.as_str() == party.as_str())
    }
}

/// What policy's sources read: the log, intake's records of orders, the reserves' declarations and the
/// deployment's registers.
#[derive(Clone)]
pub struct Records {
    /// The log.
    pub(crate) log: Rc<RefCell<CoreLog>>,
    /// Intake's record of each order, copied once the order opens.
    pub(crate) orders: Rc<RefCell<BTreeMap<String, OrderRecord>>>,
    /// Each reserve's declaration, from its advertisement as the assembly listed it in the log.
    pub(crate) declarations: Rc<RefCell<BTreeMap<String, ReserveDeclaration>>>,
    /// The deployment's registers.
    pub(crate) registers: Rc<RefCell<Registers>>,
}

fn said(yes: bool) -> Reply<Said> {
    Reply::Given(if yes { Said::Yes } else { Said::No })
}

impl Records {
    /// Each verified entry's `what`, oldest first.
    fn verified(&self) -> Vec<What> {
        self.log
            .borrow()
            .entries()
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(v) => Some(v.what.clone()),
                Entry::Stated(_) => None,
            })
            .collect()
    }

    fn order_parties(&self, order: &Id, party: &Id) -> Reply<Said> {
        let parties = self.verified().into_iter().find_map(|w| match w {
            What::Acceptance {
                order: o, parties, ..
            } if &o == order => Some(parties),
            _ => None,
        });
        match parties {
            // Its requester, beneficiary or payer, as the log's `customer-read-is-of-its-own-order` holds it: the
            // reserve a settlement account is at is none of them.
            Some(p) => said(&p.requester == party || &p.beneficiary == party || &p.payer == party),
            None => Reply::Missing,
        }
    }

    fn segment_order(&self, segment: &Id, order: &Id) -> Reply<Said> {
        let of = self.verified().into_iter().find_map(|w| match w {
            What::Input { input } => match &input.observed {
                Observed::RouteChosen { parts, .. } => parts
                    .iter()
                    .flat_map(|p| &p.segments)
                    .any(|s| s.segment_id.as_str() == segment.as_str())
                    .then(|| input.subject.order.as_str().to_owned()),
                _ => None,
            },
            _ => None,
        });
        match of {
            Some(o) => said(o == order.as_str()),
            None => Reply::Missing,
        }
    }

    fn segment_provider(&self, order: &Id, segment: &Id, provider: &Id) -> Reply<Said> {
        let moved = self.verified().into_iter().find_map(|w| match w {
            What::Effect {
                action:
                    Action::MoveValue {
                        order: o,
                        segment: s,
                        provider: p,
                        ..
                    },
                ..
            } if &o == order && &s == segment => Some(p),
            _ => None,
        });
        match moved {
            Some(p) => said(&p == provider),
            None => Reply::Missing,
        }
    }

    fn statement(
        &self,
        commitment: &rutent_contracts::module_interface::Digest,
        party: &Id,
    ) -> Reply<Said> {
        let maker = self
            .log
            .borrow()
            .entries()
            .iter()
            .find_map(|(_, e)| match e {
                Entry::Stated(s) if &s.commitment == commitment => {
                    Some(s.attribution.maker().party.clone())
                }
                _ => None,
            });
        match maker {
            Some(m) => said(&m == party),
            None => Reply::Missing,
        }
    }

    fn declared(&self, reserve: &Id, declaration: &Id) -> Reply<Said> {
        let declarations = self.declarations.borrow();
        let Some(d) = declarations.get(reserve.as_str()) else {
            return Reply::Missing;
        };
        match declaration.as_str() {
            NO_USER_WITHDRAWAL => said(!d.can_require_user_initiated_withdrawal),
            AUTOMATIC_DEPOSIT => said(d.starts_routes_on_automatic_deposits),
            // A reserve that published made each declaration either way: one it didn't make is a no.
            _ => said(false),
        }
    }
}

impl authz::Sources for Records {
    fn supply(&mut self, need: &Need) -> Reply<Said> {
        let input = &need.input;
        let registers = self.registers.borrow().clone();
        match &need.from {
            Source::ActsFor { acts, party } => match registers.acts_for(acts, party) {
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
            Source::Requester(order) => match self.orders.borrow().get(order.as_str()) {
                Some(record) => said(&record.parties.requester == input),
                None => Reply::Missing,
            },
            Source::OrderOrigin(order) => match self.orders.borrow().get(order.as_str()) {
                Some(record) => said(&record.origin_reserve == input),
                None => Reply::Missing,
            },
            Source::OrderParties(order) => self.order_parties(order, input),
            Source::SegmentOrder(segment) => self.segment_order(segment, input),
            Source::SegmentProvider { order, segment } => {
                self.segment_provider(order, segment, input)
            }
            Source::Statement(commitment) => self.statement(commitment, input),
            Source::Declarations(reserve) => self.declared(reserve, input),
            // No verifier's judgement or provider's answer is a source in M3's assembly yet.
            Source::Verifier(_) | Source::Provider(_) => Reply::Missing,
        }
    }
}

/// The deployment's own policy: it needs nothing beyond the fixed rules, and permits what they let through, unless
/// a scenario sets it to refuse.
#[derive(Debug, Clone, Default)]
pub struct Deployment {
    /// Refuses every call with this class, where set.
    pub refuse: Rc<std::cell::Cell<Option<RefusalClass>>>,
}

impl authz::Policy for Deployment {
    fn needs(&mut self, _: authz::Asked<'_>) -> Reply<Vec<Need>> {
        Reply::Given(Vec::new())
    }

    fn rule(&mut self, _: authz::Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        Reply::Given(match self.refuse.get() {
            Some(class) => Ruling::Refuse(class),
            None => Ruling::Permit,
        })
    }
}

/// The policy engine as the assembly builds it.
pub type PolicyEngine = Engine<PolicyHandle<CoreLog>, Deployment, Records>;

/// Policy, as the other parts reach it: the engine for the calls it builds (decisions, authorisations, sessions and
/// reads), and,
/// beside it, on policy's handle, the calls whose rows haven't built them on the engine yet. Each of those is decided
/// as the engine decides a call: the engine's own fixed rules first (`Fixed`), over the same records, then the
/// deployment's policy, which a scenario may set to refuse (`PolicySide::consult`). Only the answer is the stand-in's:
///
/// - **Charges (`T-021-05`).** Within a bound no scenario reaches where the consultation permits, over a bound of 0
///   where it doesn't.
/// - **Questions about people (`T-021-07`).** Refused where the consultation refuses; otherwise not known, since no
///   verifier is a source yet.
///
/// Each is replaced, through the same trait, when its row builds it on the engine: drop its `impl` here and forward
/// to the engine, as [`Decisions`] does.
#[derive(Clone)]
pub struct PolicySide {
    engine: Rc<RefCell<PolicyEngine>>,
    records: Records,
    deployment: Deployment,
}

/// One policy's verdict on a call, as the engine reaches its own (`core-authz`'s `verdict`): its needs, each asked of
/// its source in order, the first that fails deciding, then its ruling.
fn verdict(
    policy: &mut impl authz::Policy,
    sources: &mut Records,
    asked: authz::Asked<'_>,
) -> Answer {
    let refused = |class| Answer::Refuse { class };
    let not_known = |cause| Answer::NotKnown { cause };
    let needs = match policy.needs(asked) {
        Reply::Given(needs) => needs,
        Reply::Missing => return refused(RefusalClass::MissingInput),
        Reply::Unreachable => return not_known(NotKnownCause::PolicyUnreachable),
    };
    let mut inputs = Vec::with_capacity(needs.len());
    for need in needs {
        match authz::Sources::supply(sources, &need) {
            Reply::Given(said) => inputs.push(Input { need, said }),
            Reply::Missing => return refused(RefusalClass::MissingInput),
            Reply::Unreachable => return not_known(NotKnownCause::VerifierUnreachable),
        }
    }
    match policy.rule(asked, &inputs) {
        Reply::Given(Ruling::Permit) => Answer::Permit {},
        Reply::Given(Ruling::Refuse(class)) => refused(class),
        Reply::Missing => refused(RefusalClass::MissingInput),
        Reply::Unreachable => not_known(NotKnownCause::PolicyUnreachable),
    }
}

/// A bound on a party's work that no scenario reaches.
pub const CHARGE_BOUND: u64 = 1_000_000;

impl PolicySide {
    /// Policy over `engine`, answering beside it from `records` under `deployment`, the engine's own deployment
    /// policy.
    pub fn new(
        engine: Rc<RefCell<PolicyEngine>>,
        records: Records,
        deployment: Deployment,
    ) -> Self {
        Self {
            engine,
            records,
            deployment,
        }
    }

    /// The consultation of a call answered beside the engine, as the engine's own runs it: its fixed rules first
    /// (`Fixed`, every rule `FIXED` lists, the register of actors and the roles' rules among them), each input
    /// asked of the same records, then the deployment's policy, unless a fixed rule refused. A refusal beats not
    /// known.
    fn consult(&self, asked: authz::Asked<'_>) -> Answer {
        let mut sources = self.records.clone();
        let fixed = verdict(&mut authz::Fixed::default(), &mut sources, asked);
        if matches!(fixed, Answer::Refuse { .. }) {
            return fixed;
        }
        match verdict(&mut self.deployment.clone(), &mut sources, asked) {
            Answer::Permit {} => fixed,
            refused @ Answer::Refuse { .. } => refused,
            not_known @ Answer::NotKnown { .. } => match fixed {
                Answer::NotKnown { .. } => fixed,
                _ => not_known,
            },
        }
    }

    /// The engine.
    pub(crate) fn engine(&self) -> &Rc<RefCell<PolicyEngine>> {
        &self.engine
    }

    /// Appends one answer's verified entry on policy's handle, the engine's.
    fn record(
        &self,
        who: &Actor,
        what: What,
        at: &rutent_contracts::router_api::Timestamp,
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
        let engine = self.engine.borrow();
        engine.log().append_beside(&entry)
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

impl Charges for PolicySide {
    fn charge(&mut self, charge: &Charge) -> Result<ChargeAnswer, AppendFailure> {
        charge.check().map_err(AppendFailure::Refused)?;
        // Over a bound of 0, as the engine's stub answers, where the consultation doesn't permit: no work is done.
        let outcome = match self.consult(authz::Asked::Charge(charge)) {
            Answer::Permit {} => ChargeOutcome::Within {
                remaining: SafeInteger::try_from(CHARGE_BOUND).expect("a safe integer"),
            },
            _ => ChargeOutcome::Over {
                bound: SafeInteger::try_from(0).expect("a safe integer"),
            },
        };
        let what = What::Charge {
            charge: charge.clone(),
        };
        let receipt = self.record(&charge.caller, what, &charge.at, outcome.outcome())?;
        Ok(ChargeAnswer {
            request: charge.request.clone(),
            outcome,
            receipt,
        })
    }
}

impl PersonQuestions for PolicySide {
    fn ask_about_person(
        &mut self,
        question: &PersonQuestion,
    ) -> Result<PersonAnswer, AppendFailure> {
        question.check().map_err(AppendFailure::Refused)?;
        // No verifier is a source yet, so a question the consultation lets through is not known.
        let outcome = match self.consult(authz::Asked::AskAboutPerson(question)) {
            Answer::Refuse { class } => PersonOutcome::Refused { class },
            _ => PersonOutcome::NotKnown {
                cause: NotKnownCause::PolicyUnreachable,
            },
        };
        let what = What::AskAboutPerson {
            question: question.clone(),
        };
        let receipt = self.record(&question.asker, what, &question.at, outcome.outcome())?;
        Ok(PersonAnswer {
            request: question.request.clone(),
            outcome,
            receipt,
        })
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

impl<L: PolicyLog> PolicyHandle<L> {
    /// Appends `entry` through this handle from a shared borrow: for the policy calls answered beside the engine,
    /// which reach the handle only through the engine that holds it.
    pub(crate) fn append_beside(&self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        self.0.borrow_mut().append_decision(entry)
    }
}
