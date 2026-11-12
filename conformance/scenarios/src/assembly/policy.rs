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
    pub log: Rc<RefCell<CoreLog>>,
    /// Intake's record of each order, copied once the order opens.
    pub orders: Rc<RefCell<BTreeMap<String, OrderRecord>>>,
    /// Each reserve's declaration, from its advertisement as the assembly listed it in the log.
    pub declarations: Rc<RefCell<BTreeMap<String, ReserveDeclaration>>>,
    /// The deployment's registers.
    pub registers: Rc<RefCell<Registers>>,
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
            Some(p) => said(
                &p.requester == party
                    || &p.beneficiary == party
                    || &p.payer == party
                    || p.settlement_account_at.as_ref() == Some(party),
            ),
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

/// Policy, as the other parts reach it: the engine for the calls it builds (decisions, sessions and reads), and,
/// beside it, on policy's handle, the calls whose rows haven't built them yet.
///
/// - **Authorisations (`T-021-04`).** An authorisation is permitted where its giver acts for the party it states
///   (`ActsFor`) and that party requested the order (`Requester`), the rule `T-021-03` holds a customer's
///   authorisation to, and refused otherwise; it is recorded on policy's handle as the `authorise` entry the log
///   holds a move to (`move-rests-on-its-authorisation`).
/// - **Charges (`T-021-05`).** Within a bound no scenario reaches, recorded.
/// - **Questions about people (`T-021-07`).** Not known, as the engine's stub answers them, recorded.
///
/// Each is replaced, through the same trait, when its row builds it on the engine.
#[derive(Clone)]
pub struct PolicySide {
    engine: Rc<RefCell<PolicyEngine>>,
    records: Records,
}

/// A bound on a party's work that no scenario reaches.
pub const CHARGE_BOUND: u64 = 1_000_000;

impl PolicySide {
    /// Policy over `engine`, answering beside it from `records`.
    pub fn new(engine: Rc<RefCell<PolicyEngine>>, records: Records) -> Self {
        Self { engine, records }
    }

    /// The engine.
    pub fn engine(&self) -> &Rc<RefCell<PolicyEngine>> {
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

impl Authorisations for PolicySide {
    fn authorise(&mut self, a: &Authorisation) -> Result<Decision, AppendFailure> {
        let giver = &a.given_by;
        let registers = self.records.registers.borrow().clone();
        let acts = registers.acts_for(&giver.acts, &giver.party);
        let requester = self
            .records
            .orders
            .borrow()
            .get(a.order.as_str())
            .map(|r| r.parties.requester.clone());
        let answer = match (acts, requester) {
            (Some(true), Some(requester)) if requester == giver.party => Answer::Permit {},
            (Some(false), _) => Answer::Refuse {
                class: RefusalClass::NoAccountableParty,
            },
            (None, _) | (_, None) => Answer::Refuse {
                class: RefusalClass::MissingInput,
            },
            _ => Answer::Refuse {
                class: RefusalClass::NotPermitted,
            },
        };
        let what = What::Authorise {
            authorisation: a.clone(),
            decision: a.authorisation.clone(),
        };
        let receipt = self.record(giver, what, &a.at, answer.outcome())?;
        Ok(Decision {
            request: a.authorisation.clone(),
            decision: a.authorisation.clone(),
            outcome: answer,
            receipt,
        })
    }
}

impl Charges for PolicySide {
    fn charge(&mut self, charge: &Charge) -> Result<ChargeAnswer, AppendFailure> {
        charge.check().map_err(AppendFailure::Refused)?;
        let outcome = ChargeOutcome::Within {
            remaining: SafeInteger::try_from(CHARGE_BOUND).expect("a safe integer"),
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
        let outcome = PersonOutcome::NotKnown {
            cause: NotKnownCause::PolicyUnreachable,
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
