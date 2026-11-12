//! Shared helpers for intake's tests (`T-020-03`): one log that both orchestration's handle and policy's write to,
//! as the assembly's log does, attributing each stated entry and admitting each entry over the ones before it as
//! it is appended (`attribute`, `admit_entry`), and able to fail an append or lose its answer on demand; policy's
//! authorisations, holding each actor to the register of actors (`ActsFor`), and the user's session, recorded on
//! policy's path; the register of actors and policy's session register as intake reads them; a destination
//! register; and one environment that opens, quotes and accepts orders. Built on `snapshot_support` (`T-020-05`)
//! and `rounds_support` (`T-020-06`), used unchanged.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{
    AppendFailure, Authorisations, Evidence, Fetched, Kept, KeptStatements, Log, Signatures,
    StatementKeys, is_decision,
};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Answer, Authorisation, Decision, Destination, Entry, KeptStatement, Opened,
    Outcome, Parties, ReadCheck, Receipt, RefusalClass, Role, SessionRequest, SignedWith, Through,
    TimeSource, VerifiedEntry, VerifiedKind, What, When, admit_entry, attribute,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{Deployment, OuterNode, SafeInteger};
use rutent_contracts::router_api::{Id, PolicyBounds, Request, Timestamp};
use rutent_core_orch::intake::{
    Actors, Desk, Destinations, HeldSession, Intake, IntakeError, OrderRequest, Resolution,
    SessionRegister, Status, Told,
};
use rutent_core_orch::rounds::{Fresh, Issued, Parts, Providers};
use rutent_core_orch::snapshot::{Clock, Exposures, Price, Releases};
use rutent_prov::catalogue::{Catalogue, CheckBound, Instant, Publisher};
use rutent_router::RouteFinder;
use serde_json::{Value, json};

use crate::rounds_support::{Bound, Ids, People, Sim, TestKey, customer, operator};
use crate::snapshot_support::*;

/// The user's session, for the account `acct-1`, open until [`SESSION_ENDS`].
pub const SESSION: &str = "session-1";
/// When the user's session ends.
pub const SESSION_ENDS: u64 = 50_000;
/// When the tests' clock starts.
pub const START: u64 = 2_000;
/// The address the requester gives for the deposit.
pub const ADDRESS: &str = "0xbeef";
/// The destination's owner, as the register first resolves it.
pub const OWNER: &str = "owner-1";

// One log, two handles -----------------------------------------------------------------------------------------------

/// The log: each entry as handed in, with the path it came by, in order, and each as the log recorded it, its
/// statement attributed. An entry the log wouldn't admit is refused, with nothing appended.
pub struct Book {
    pub entries: Vec<(Through, Entry)>,
    history: Vec<(Receipt, Entry)>,
    /// What the log reads to attribute a statement: the catalogue's keys as the test set them, the store, the clock.
    catalogue: Catalogue,
    store: Rc<RefCell<Store>>,
    clock: Rc<TestClock>,
    /// The call whose next append on orchestration's handle fails, once, with nothing appended.
    pub fail_next: Option<&'static str>,
    /// The call whose next append on orchestration's handle is written, but its answer lost, once.
    pub lose_next: Option<&'static str>,
    /// The call whose next append on orchestration's handle the log refuses, once, with nothing appended.
    pub refuse_next: Option<&'static str>,
    /// Whether the next stated entry appended on orchestration's handle fails, once, with nothing appended
    /// (`T-020-04`). Off unless a test sets it.
    pub fail_stated_next: bool,
}

fn log_receipt(index: usize) -> Receipt {
    Receipt {
        log: id("log-1"),
        index: SafeInteger::try_from(index as u64).unwrap(),
        entry: serde_json::from_value(json!({ "algorithm": "sha-256", "value": "72".repeat(32) }))
            .unwrap(),
    }
}

impl Book {
    fn push(&mut self, through: Through, entry: Entry) -> Result<Receipt, AppendFailure> {
        entry.check().map_err(AppendFailure::Refused)?;
        let recorded = match &entry {
            Entry::Stated(claim) => {
                let store = self.store.borrow();
                let mut sources = Sources {
                    catalogue: &self.catalogue,
                    store: &store,
                    at: self.clock.now().floor(),
                };
                let mut keys = Sources { ..sources };
                let signatures = Sources { ..sources };
                Entry::Stated(attribute(
                    claim,
                    &self.history,
                    &mut sources,
                    &mut keys,
                    &signatures,
                )?)
            }
            verified => verified.clone(),
        };
        admit_entry(&self.history, &recorded, through, &operator().acts)
            .map_err(AppendFailure::Refused)?;
        let receipt = log_receipt(self.entries.len());
        self.entries.push((through, entry));
        self.history.push((receipt.clone(), recorded));
        Ok(receipt)
    }

    /// Every entry's `what`, verified entries only, in order.
    pub fn verified(&self) -> Vec<&What> {
        self.entries
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(v) => Some(&v.what),
                _ => None,
            })
            .collect()
    }

    /// Every verified entry, in order.
    pub fn verified_entries(&self) -> Vec<&VerifiedEntry> {
        self.entries
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(v) => Some(v),
                _ => None,
            })
            .collect()
    }
}

/// Orchestration's handle on the shared log.
pub struct General(pub Rc<RefCell<Book>>);

impl Log for General {
    fn append(
        &mut self,
        entry: &rutent_contracts::core_internal::calls::GeneralEntry,
    ) -> Result<Receipt, AppendFailure> {
        let mut book = self.0.borrow_mut();
        let call = match entry.entry() {
            Entry::Verified(v) => Some(call_of(&v.what)),
            Entry::Stated(_) => None,
        };
        if call.is_none() && book.fail_stated_next {
            book.fail_stated_next = false;
            return Err(AppendFailure::Unavailable);
        }
        if call.is_some() && book.fail_next == call {
            book.fail_next = None;
            return Err(AppendFailure::Unavailable);
        }
        if call.is_some() && book.refuse_next == call {
            book.refuse_next = None;
            return Err(AppendFailure::Refused(
                rutent_contracts::core_internal::Violation("refused on demand"),
            ));
        }
        let receipt = book.push(Through::General, entry.entry().clone())?;
        if call.is_some() && book.lose_next == call {
            book.lose_next = None;
            return Err(AppendFailure::Unavailable);
        }
        Ok(receipt)
    }
}

/// Policy, as far as intake reaches it: each authorisation answered as set, and recorded on policy's path before
/// it is answered.
pub struct Policy {
    pub book: Rc<RefCell<Book>>,
    pub clock: Rc<TestClock>,
    pub actors: Rc<RefCell<ActorRegister>>,
    pub answer: Answer,
    pub asked: Vec<Authorisation>,
}

/// Policy itself, as the actor its entries name.
pub fn policy_actor() -> Actor {
    Actor {
        party: id("operator"),
        acts: id("rutent-policy"),
        role: Role::Core,
        kind: ActorKind::Component,
    }
}

fn now(clock: &TestClock) -> Timestamp {
    clock.now().timestamp().unwrap()
}

impl Authorisations for Policy {
    fn authorise(&mut self, authorisation: &Authorisation) -> Result<Decision, AppendFailure> {
        self.asked.push(authorisation.clone());
        // `ActsFor`, the first fixed rule: the giver acts for the party it states, or the answer is a refusal.
        let giver = &authorisation.given_by;
        let answer = if self.actors.borrow().acts_for(&giver.acts, &giver.party) {
            self.answer
        } else {
            refusal(RefusalClass::NoAccountableParty)
        };
        let decision = id(&format!("dec-{}", authorisation.authorisation.as_str()));
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: policy_actor(),
            what: What::Authorise {
                authorisation: authorisation.clone(),
                decision: decision.clone(),
            },
            when: When {
                at: now(&self.clock),
                source: TimeSource::CoreClock,
            },
            outcome: answer.outcome(),
        });
        let receipt = self.book.borrow_mut().push(Through::Policy, entry)?;
        Ok(Decision {
            request: authorisation.authorisation.clone(),
            decision,
            outcome: answer,
            receipt,
        })
    }
}

/// The destination register: each destination's answer by account, set by the test; an account it holds nothing
/// for resolves to [`OWNER`].
#[derive(Debug, Default)]
pub struct Register {
    pub answers: BTreeMap<String, Resolution>,
    pub asked: usize,
}

impl Destinations for Register {
    fn resolve(&mut self, destination: &Destination) -> Resolution {
        self.asked += 1;
        self.answers
            .get(destination.account.as_str())
            .cloned()
            .unwrap_or_else(|| Resolution::Owner(id(OWNER)))
    }
}

/// The register of actors: who each actor, as authenticated, acts for.
#[derive(Debug, Default)]
pub struct ActorRegister {
    pub pairs: BTreeSet<(String, String)>,
}

impl ActorRegister {
    pub fn acts_for(&self, acts: &Id, party: &Id) -> bool {
        self.pairs
            .contains(&(acts.as_str().to_owned(), party.as_str().to_owned()))
    }

    pub fn list(&mut self, actor: &Actor) {
        self.pairs.insert((
            actor.acts.as_str().to_owned(),
            actor.party.as_str().to_owned(),
        ));
    }
}

/// The register of actors, as intake reads it.
pub struct Acting(pub Rc<RefCell<ActorRegister>>);

impl Actors for Acting {
    fn acts_for(&mut self, acts: &Id, party: &Id) -> bool {
        self.0.borrow().acts_for(acts, party)
    }
}

/// Policy's session register, as intake reads it.
#[derive(Debug, Default)]
pub struct Sessions {
    pub held: BTreeMap<String, HeldSession>,
}

impl SessionRegister for Sessions {
    fn held(&mut self, session: &Id) -> Option<HeldSession> {
        self.held.get(session.as_str()).cloned()
    }
}

/// The store, shared with the log, which reads what it kept.
pub struct SharedStore(pub Rc<RefCell<Store>>);

impl Evidence for SharedStore {
    fn keep(&mut self, received: &[u8]) -> Result<Kept, AppendFailure> {
        self.0.borrow_mut().keep(received)
    }

    fn fetch(&mut self, check: &ReadCheck) -> Result<Fetched, AppendFailure> {
        self.0.borrow_mut().fetch(check)
    }
}

/// An evidence store that keeps every statement it is handed.
#[derive(Debug, Default)]
pub struct Store {
    pub kept: Vec<Vec<u8>>,
    /// How many more statements it keeps before it fails to keep one, once, keeping nothing (`T-020-04`). Off unless
    /// a test sets it.
    pub fail_after: Option<usize>,
}

fn commitment(place: usize) -> Digest {
    serde_json::from_value(json!({ "algorithm": "sha-256", "value": format!("{place:064x}") }))
        .unwrap()
}

impl Evidence for Store {
    fn keep(&mut self, received: &[u8]) -> Result<Kept, AppendFailure> {
        match self.fail_after {
            Some(0) => {
                self.fail_after = None;
                return Err(AppendFailure::Unavailable);
            }
            Some(n) => self.fail_after = Some(n - 1),
            None => {}
        }
        self.kept.push(received.to_vec());
        Ok(Kept {
            commitment: commitment(self.kept.len() - 1),
            restates: None,
        })
    }

    fn fetch(&mut self, _check: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Ok(Fetched::Erased)
    }
}

// The environment ----------------------------------------------------------------------------------------------------

/// Everything intake reaches but the providers.
pub struct Env {
    pub catalogue: Catalogue,
    pub clock: Rc<TestClock>,
    pub exposures: Exposures,
    pub releases: Releases,
    pub book: Rc<RefCell<Book>>,
    pub log: General,
    pub evidence: SharedStore,
    pub store: Rc<RefCell<Store>>,
    pub actors: Rc<RefCell<ActorRegister>>,
    pub acting: Acting,
    pub sessions: Sessions,
    pub bound: Bound,
    pub people: People,
    pub key: TestKey,
    pub ids: Ids,
    pub issued: Issued,
    pub policy: Policy,
    pub register: Register,
    pub bounds: PolicyBounds,
    pub prices: Vec<Price>,
    pub intake: Intake,
}

impl Env {
    /// An environment over the one reserve [`RESERVE`], on `polygon`, with the user's session open on `acct-1`
    /// until [`SESSION_ENDS`], in the catalogue and in the log, and every authorisation permitted.
    pub fn new() -> Self {
        let mut catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
        catalogue.set_check_bound(CheckBound {
            max_counted: 10,
            window_secs: 86_400,
        });
        catalogue.set_answer_within(60);
        catalogue.open_session(SESSION, "acct-1", Instant(SESSION_ENDS));
        let clock = Rc::new(TestClock::at(START));
        let store = Rc::new(RefCell::new(Store::default()));
        let book = Rc::new(RefCell::new(Book {
            entries: Vec::new(),
            history: Vec::new(),
            catalogue: catalogue.clone(),
            store: store.clone(),
            clock: clock.clone(),
            fail_next: None,
            lose_next: None,
            refuse_next: None,
            fail_stated_next: false,
        }));
        let actors = Rc::new(RefCell::new(ActorRegister::default()));
        actors.borrow_mut().list(&customer());
        actors.borrow_mut().list(&agent());
        let mut sessions = Sessions::default();
        sessions.held.insert(
            SESSION.into(),
            HeldSession {
                holder: customer().party,
                expires_at: Timestamp::new(Instant(SESSION_ENDS).rfc3339().unwrap()).unwrap(),
            },
        );
        let env = Self {
            catalogue,
            clock: clock.clone(),
            exposures: Exposures::new(),
            releases: Releases::new(),
            book: book.clone(),
            log: General(book.clone()),
            evidence: SharedStore(store.clone()),
            store,
            actors: actors.clone(),
            acting: Acting(actors.clone()),
            sessions,
            bound: Bound::of(100),
            people: People::default(),
            key: TestKey(operator_key()),
            ids: Ids::default(),
            issued: Issued::new(),
            policy: Policy {
                book: book.clone(),
                clock,
                actors,
                answer: Answer::Permit {},
                asked: Vec::new(),
            },
            register: Register::default(),
            bounds: no_ceilings(),
            prices: prices(&[RESERVE]),
            intake: Intake::new(),
        };
        env.session_opened(&customer().party);
        env
    }

    /// Policy's entry of the user's session, opened for `holder`.
    fn session_opened(&self, holder: &Id) {
        let at = now(&self.clock);
        let request = SessionRequest {
            request: id("req-session-1"),
            account: id("acct-1"),
            holder: holder.clone(),
            requester: customer(),
            until: Timestamp::new(Instant(SESSION_ENDS).rfc3339().unwrap()).unwrap(),
            at: at.clone(),
        };
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: policy_actor(),
            what: What::OpenSession {
                request,
                opened: Some(Opened {
                    session: id(SESSION),
                    expires_at: Timestamp::new(Instant(SESSION_ENDS).rfc3339().unwrap()).unwrap(),
                }),
            },
            when: When {
                at,
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::Permitted {},
        });
        self.book.borrow_mut().push(Through::Policy, entry).unwrap();
    }

    /// Simulated providers on this environment's clock.
    pub fn sim(&self) -> Sim {
        Sim::new(self.clock.clone())
    }

    /// Runs `call` on intake with the desk it reaches.
    pub fn with<T>(
        &mut self,
        sim: &mut Sim,
        call: impl FnOnce(&mut Intake, &mut Desk<'_, '_>) -> T,
    ) -> T {
        self.with_providers(sim, call)
    }

    /// Runs `call` on intake with the desk it reaches, asking `providers`.
    pub fn with_providers<T>(
        &mut self,
        providers: &mut dyn Providers,
        call: impl FnOnce(&mut Intake, &mut Desk<'_, '_>) -> T,
    ) -> T {
        let core = core_actor();
        let operator = operator();
        let mut parts = Parts {
            catalogue: &mut self.catalogue,
            clock: &*self.clock,
            exposures: &self.exposures,
            releases: &self.releases,
            log: &mut self.log,
            evidence: &mut self.evidence,
            charges: &mut self.bound,
            persons: &mut self.people,
            providers,
            operator_key: &mut self.key,
            fresh: &mut self.ids,
            issued: &mut self.issued,
        };
        let mut desk = Desk {
            parts: &mut parts,
            destinations: &mut self.register,
            authorisations: &mut self.policy,
            actors: &mut self.acting,
            sessions: &mut self.sessions,
            bounds: &self.bounds,
            prices: &self.prices,
            core: &core,
            operator: &operator,
        };
        call(&mut self.intake, &mut desk)
    }

    /// Opens `request`.
    pub fn open(&mut self, sim: &mut Sim, request: &OrderRequest) -> Result<Told, IntakeError> {
        self.with(sim, |intake, desk| intake.open(desk, request.clone()))
    }

    /// The customer quotes `order` with `RouteFinder`.
    pub fn quote(&mut self, sim: &mut Sim, order: &Id) -> Result<Told, IntakeError> {
        self.quote_by(sim, &customer(), order)
    }

    /// `caller` quotes `order` with `RouteFinder`.
    pub fn quote_by(
        &mut self,
        sim: &mut Sim,
        caller: &Actor,
        order: &Id,
    ) -> Result<Told, IntakeError> {
        self.with(sim, |intake, desk| {
            intake.quote::<RouteFinder>(desk, caller, order)
        })
    }

    /// `by` accepts `order`'s quote of `digest`.
    pub fn accept(
        &mut self,
        sim: &mut Sim,
        order: &Id,
        by: &Actor,
        digest: &Digest,
    ) -> Result<Told, IntakeError> {
        self.with(sim, |intake, desk| {
            intake.accept(desk, order, by, digest, None)
        })
    }

    /// `by` accepts `order`'s quote of `digest`, confirming its destination's owner is `owner`.
    pub fn confirm(
        &mut self,
        sim: &mut Sim,
        order: &Id,
        by: &Actor,
        digest: &Digest,
        owner: &Id,
    ) -> Result<Told, IntakeError> {
        self.with(sim, |intake, desk| {
            intake.accept(desk, order, by, digest, Some(owner))
        })
    }

    /// What `caller` is told about its request `request`.
    pub fn told(&mut self, caller: &Actor, request: &str) -> Option<Told> {
        self.intake.told(&mut self.acting, caller, &id(request))
    }

    /// What the customer is told about its request `request`.
    pub fn told_customer(&mut self, request: &str) -> Option<Told> {
        self.told(&customer(), request)
    }

    /// Opens `request` and quotes it: its order and the quote's digest.
    pub fn quoted(&mut self, sim: &mut Sim, request: &OrderRequest) -> (Id, Digest) {
        let order = order_of(&self.open(sim, request).expect("the request opens"));
        match self.quote(sim, &order).expect("the routing runs") {
            Told::Exists {
                status: Status::Quoted { digest, .. },
                ..
            } => (order, digest),
            other => panic!("no quote: {other:?}"),
        }
    }

    /// Opens, quotes and accepts `request`, by `by`.
    pub fn accepted(&mut self, sim: &mut Sim, request: &OrderRequest, by: &Actor) -> (Id, Told) {
        let (order, digest) = self.quoted(sim, request);
        let told = self
            .accept(sim, &order, by, &digest)
            .expect("the acceptance runs");
        (order, told)
    }

    /// Every acceptance recorded.
    pub fn acceptances(&self) -> Vec<VerifiedEntry> {
        self.book
            .borrow()
            .verified_entries()
            .into_iter()
            .filter(|v| matches!(v.what, What::Acceptance { .. }))
            .cloned()
            .collect()
    }

    /// Every `route_chosen` input recorded, by order.
    pub fn started(&self) -> Vec<String> {
        self.book
            .borrow()
            .verified()
            .into_iter()
            .filter_map(|w| match w {
                What::Input { input } => Some(input.subject.order.as_str().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// Every `order_refusal` recorded: its request, its order and its class.
    pub fn refusals(&self) -> Vec<What> {
        self.book
            .borrow()
            .verified()
            .into_iter()
            .filter(|w| matches!(w, What::OrderRefusal { .. }))
            .cloned()
            .collect()
    }

    /// How many entries the log holds.
    pub fn len(&self) -> usize {
        self.book.borrow().entries.len()
    }
}

/// The order a told answer names.
pub fn order_of(told: &Told) -> Id {
    match told {
        Told::Exists { order, .. } => order.clone(),
        Told::Refused { class } => panic!("refused: {class:?}"),
    }
}

/// A refusal policy answers.
pub fn refusal(class: RefusalClass) -> Answer {
    Answer::Refuse { class }
}

// The log's own reading ---------------------------------------------------------------------------------------------

struct Sources<'a> {
    catalogue: &'a Catalogue,
    store: &'a Store,
    at: Instant,
}

impl KeptStatements for Sources<'_> {
    fn kept(&mut self, wanted: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        let place = (0..self.store.kept.len()).find(|i| &commitment(*i) == wanted);
        Ok(place.map(|i| {
            serde_json::from_value(json!({
                "commitment": wanted,
                "salt": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "bytes": base64url(&self.store.kept[i]),
            }))
            .unwrap()
        }))
    }
}

impl StatementKeys for Sources<'_> {
    fn deployment(&mut self) -> Result<Deployment, AppendFailure> {
        Ok(deployment())
    }

    fn operator(&mut self) -> Result<Id, AppendFailure> {
        Ok(operator().acts)
    }

    fn publisher(&mut self, maker: &Id) -> Result<Option<Id>, AppendFailure> {
        if maker == &operator().acts {
            return Ok(Some(operator().party));
        }
        Ok(
            match self.catalogue.publishers().publisher(maker.as_str()) {
                Some(Publisher::Operator) => Some(operator().party),
                Some(Publisher::Party(party)) => Id::new(party),
                None => None,
            },
        )
    }

    fn signs_for(&mut self, _: &Id, _: &SignedWith, _: &Timestamp) -> Result<bool, AppendFailure> {
        Ok(true)
    }
}

impl Signatures for Sources<'_> {
    fn verifies(&self, received: &[u8]) -> bool {
        let maker = serde_json::from_slice::<Value>(received)
            .ok()
            .and_then(|v| v["maker"].as_str().map(str::to_owned));
        if maker.as_deref() == Some(operator().acts.as_str()) {
            self.catalogue
                .verify_operator_statement(received, self.at)
                .is_ok()
        } else {
            self.catalogue.verify_statement(received, self.at).is_ok()
        }
    }
}

impl Env {
    /// The log as the log itself would keep it: each stated entry attributed from its kept bytes (core-internal's
    /// `attribute`), and every entry admitted over the ones before it (`admit_entry`), on the path it came by, so
    /// every row over what intake and the rounds write holds: the acceptance's quote shown, reserve's yes and
    /// session, `route_chosen` resting on the acceptance and opened once, and each answer after its question.
    /// Refused at the first entry the log wouldn't admit, with its place and why.
    ///
    /// As `rounds_support`'s `Env::admitted`, it leaves out policy's charge and person-question entries (they go to
    /// fakes), every receipt shares one digest, and `signs_for` always answers true.
    pub fn admitted(&self) -> Result<Vec<Entry>, (usize, String)> {
        let book = self.book.borrow();
        let mut history: Vec<(Receipt, Entry)> = Vec::new();
        for (i, (through, entry)) in book.entries.iter().enumerate() {
            let entry = match entry {
                Entry::Stated(claim) => {
                    let store = self.store.borrow();
                    let mut sources = Sources {
                        catalogue: &self.catalogue,
                        store: &store,
                        at: self.clock.now().floor(),
                    };
                    let mut keys = Sources { ..sources };
                    let signatures = Sources { ..sources };
                    Entry::Stated(
                        attribute(claim, &history, &mut sources, &mut keys, &signatures)
                            .map_err(|e| (i, format!("{e:?}")))?,
                    )
                }
                verified => verified.clone(),
            };
            let decision = matches!(&entry, Entry::Verified(v) if is_decision(&v.what));
            assert_eq!(
                decision,
                *through == Through::Policy,
                "entry {i} came by the wrong handle"
            );
            admit_entry(&history, &entry, *through, &operator().acts)
                .map_err(|v| (i, v.0.to_owned()))?;
            history.push((log_receipt(i), entry));
        }
        Ok(history.into_iter().map(|(_, e)| e).collect())
    }
}

// Requests ----------------------------------------------------------------------------------------------------------

/// The destination: the deposit at [`RESERVE`] to `polygon`, to the requester's [`ADDRESS`].
pub fn destination() -> Destination {
    Destination {
        reserve: id(RESERVE),
        node: OuterNode {
            network: "polygon".into(),
        },
        account: id(ADDRESS),
    }
}

/// Three different parties: the customer requests, a beneficiary receives, a third party pays.
pub fn three_parties() -> Parties {
    Parties {
        requester: customer().party,
        beneficiary: id("beneficiary-ref-2"),
        settlement_account_at: None,
        payer: id("payer-ref-3"),
    }
}

/// The route asked for: 1 000 USDC units from `polygon`'s outer node to its outer node.
pub fn route(amount: &str) -> Request {
    shortlist_request(amount)
}

/// A request under `request`, by `caller`, of 1 000 USDC units through [`RESERVE`].
pub fn order_request(request: &str, caller: Actor) -> OrderRequest {
    OrderRequest {
        request: id(request),
        caller,
        parties: three_parties(),
        origin_reserve: id(RESERVE),
        session: id(SESSION),
        destination: destination(),
        route: route("1000"),
        inputs: BTreeMap::new(),
        needs: Vec::new(),
    }
}

/// An actor that states the customer's party, in the customer's role, but doesn't act for it: the register of
/// actors lists it for no one.
pub fn impostor() -> Actor {
    Actor {
        party: customer().party,
        acts: id("someone-else"),
        role: Role::Customer,
        kind: ActorKind::Person,
    }
}

/// The customer's agent: the same party, in the customer's role, acting as an agent.
pub fn agent() -> Actor {
    Actor {
        party: customer().party,
        acts: id("agent-7"),
        role: Role::Customer,
        kind: ActorKind::Agent,
    }
}

/// Whether `entry` is the verified `what` of a call.
pub fn call_of(what: &What) -> &'static str {
    match what {
        What::Acceptance { .. } => "acceptance",
        What::OrderRefusal { .. } => "order_refusal",
        What::DestinationCheck { .. } => "destination_check",
        What::Shown { .. } => "shown",
        What::Input { .. } => "input",
        What::DecisionRecord { .. } => "decision_record",
        What::Authorise { .. } => "authorise",
        What::OpenSession { .. } => "open_session",
        _ => "other",
    }
}

/// Fresh identifiers drawn by `ids` so far are never an order's; a helper for tests that need one.
pub fn fresh_id(ids: &mut Ids) -> Id {
    ids.id()
}
