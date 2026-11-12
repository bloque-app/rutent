//! The scenario harness (`T-024-02`, `DEC-18`): scenarios written as given, when and then, each tagged with the
//! `AC-*` it accepts, run on the assembled Core ([`crate::assembly::Core`]) against the simulated providers, with
//! faults injected, from a seed, so a run replays.
//!
//! - **Given**: the providers, the staging, the platform's deadline and how the log's inputs reach the state machine
//!   ([`Given`]). Staging has no default: every scenario sets it.
//! - **When**: the steps ([`Step`]): an order opened, quoted and accepted by its customer; its route run; a fault
//!   planted; the clock moved.
//! - **Then**: checks over the run ([`Run`]), each named, every one of them run, a failure naming the scenario, its
//!   seed and how to replay it.
//!
//! **Faults** ([`Fault`]): a crash between a record and its effect, at the log's store or at the platform's seam; a
//! repeated delivery and a reordered one, to the state machine; a provider's silence; and the execution environment
//! swapped mid-order. A planted crash stops Core where it falls: what survives it is the disk (the log and the
//! evidence store), the providers and the state machine's host, and the run is read from those.
//!
//! **Seeds.** A run's seed draws every identifier Core draws and every choice a seeded scenario makes, so two runs
//! from one seed are one run. CI sets the seeds through [`SEEDS`]; a failing run prints its seed. The evidence store
//! salts each statement from the operating system (`T-022-03`), which no seed reaches, so a run is compared by its
//! [`Run::projection`]: every digest and signature replaced by the order it first appeared in.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use rutent_conformance::sim::{Behaviour, Role, Spec};
use rutent_contracts::core_internal::{Destination, Entry, Parties, What};
use rutent_contracts::provider_role::OuterNode;
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::{Observed, RecordedInput, StateMachine};
use rutent_core_orch::intake::{OrderRequest, Status as IntakeStatus, Told};
use rutent_stm::Stm;
use serde_json::{Value, json};

use crate::assembly::durable::{CRASHED, Crash};
use crate::assembly::machine::{Delivery, Environment, Reference};
use crate::assembly::{Config, Core, DESTINATION_ACCOUNT, Staged, StepError, read_back};
use crate::kit::{self, RESERVE, Seeded, id};

/// The environment variable CI sets the seeds in: a comma-separated list of numbers.
pub const SEEDS: &str = "RUTENT_SCENARIO_SEEDS";

/// The seeds a run is made under: [`SEEDS`]'s, or `fallback` where it sets none.
///
/// # Panics
///
/// If [`SEEDS`] names something that isn't a number.
pub fn seeds(fallback: &[u64]) -> Vec<u64> {
    match std::env::var(SEEDS) {
        Ok(text) if !text.trim().is_empty() => text
            .split(',')
            .map(|s| s.trim().parse().expect("each seed is a number"))
            .collect(),
        _ => fallback.to_vec(),
    }
}

/// What a scenario is given.
#[derive(Debug, Clone)]
pub struct Given {
    /// The simulated providers.
    pub providers: Vec<Spec>,
    /// How the platform stages an upgrade: set by every scenario, with no default.
    pub staging: Staged,
    /// How long the platform waits on a module before it counts it silent.
    pub deadline: Duration,
    /// How the log's inputs reach the state machine.
    pub delivery: Delivery,
}

impl Given {
    /// The one simulated reserve [`RESERVE`], staging as `staging` says, a two-second deadline, each input delivered
    /// once.
    pub fn one_reserve(staging: Staged) -> Self {
        Self {
            providers: vec![Spec::of(Role::Reserve, RESERVE)],
            staging,
            deadline: Duration::from_secs(2),
            delivery: Delivery::Once,
        }
    }
}

/// A fault a scenario plants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The process crashes right after the log's store took the `nth` record of the verified kind `call`, before
    /// the log answers: the record is durable, and its writer never hears so.
    CrashAfterRecord {
        /// The verified entry's `call`, such as `effect`.
        call: &'static str,
        /// Which one, from 0.
        nth: usize,
    },
    /// The process crashes right before the platform carries out the `nth` effect, from 0: its record is in the
    /// log, answered, and nothing is carried out.
    CrashBeforeEffect(usize),
    /// The state machine moves to the swap environment right before the platform carries out the `nth` effect.
    SwapBeforeEffect(usize),
    /// The log's inputs reach the state machine as this says from now on.
    Deliver(Delivery),
    /// `provider` stays silent on its next operation of `kind` that isn't a dry run.
    Silence {
        /// The provider.
        provider: String,
        /// The operation's kind, such as `deposit`.
        kind: String,
    },
}

/// One step of a scenario's when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The customer opens an order of `amount` from [`RESERVE`]'s bank back to it, under its request `request`,
    /// has it quoted and accepts the quote: its session first, the first time.
    Order {
        /// The customer's request identifier.
        request: String,
        /// The amount, in the asset's unit.
        amount: String,
    },
    /// Every order accepted and not yet run runs its route.
    Run,
    /// A fault is planted.
    Plant(Fault),
    /// Core's clock moves on.
    Advance(u64),
    /// The state machine moves to the swap environment, now.
    Swap,
}

/// One named check of a scenario's then.
pub type Then = Box<dyn Fn(&Run) -> Result<(), String>>;

/// A scenario: what it is given, its steps, and its checks, and the `AC-*` it accepts.
pub struct Scenario {
    /// What it is called.
    pub name: &'static str,
    /// The acceptance criterion it accepts, `AC-*`.
    pub accepts: &'static str,
    /// What it is given.
    pub given: Given,
    /// Its steps.
    pub when: Vec<Step>,
    then: Vec<(&'static str, Then)>,
}

impl Scenario {
    /// A scenario named `name`, accepting `accepts`, given `given`, with no step and no check yet.
    ///
    /// # Panics
    ///
    /// If `accepts` names no acceptance criterion: every scenario accepts one (`DEC-18`).
    pub fn new(name: &'static str, accepts: &'static str, given: Given) -> Self {
        assert!(
            accepts.starts_with("AC-") && accepts[3..].parse::<u32>().is_ok(),
            "a scenario accepts an AC-*: {accepts}"
        );
        Self {
            name,
            accepts,
            given,
            when: Vec::new(),
            then: Vec::new(),
        }
    }

    /// Adds a step.
    #[must_use]
    pub fn when(mut self, step: Step) -> Self {
        self.when.push(step);
        self
    }

    /// Adds a check, named.
    #[must_use]
    pub fn then(
        mut self,
        name: &'static str,
        check: impl Fn(&Run) -> Result<(), String> + 'static,
    ) -> Self {
        self.then.push((name, Box::new(check)));
        self
    }

    /// Runs it from `seed`, its state machine in `environment` and a planted swap moving it to `swap_to`, and runs
    /// every check over the run.
    ///
    /// # Panics
    ///
    /// If Core can't be put together, a step fails other than by a planted crash, or a check fails: each names the
    /// scenario, its seed and how to replay it.
    pub fn run(
        &self,
        seed: u64,
        environment: &mut dyn Environment,
        swap_to: Box<dyn Environment>,
    ) -> Run {
        let run = self.play(seed, environment, swap_to);
        let failed: Vec<String> = self
            .then
            .iter()
            .filter_map(|(name, check)| check(&run).err().map(|why| format!("then {name}: {why}")))
            .collect();
        assert!(
            failed.is_empty(),
            "scenario `{}` ({}), seed {seed}, failed; replay it with {SEEDS}={seed}:\n{}",
            self.name,
            self.accepts,
            failed.join("\n")
        );
        run
    }

    /// Plays the steps from `seed` and hands back Core as they left it, and the orders accepted, for a test that
    /// reads Core itself.
    ///
    /// # Panics
    ///
    /// If Core can't be put together, or a step fails, a planted crash included.
    pub fn core(&self, seed: u64, environment: &mut dyn Environment) -> (Core, Vec<Id>) {
        let config = self.config(seed);
        let mut core = Core::assemble(&config, environment).unwrap_or_else(|e| {
            panic!(
                "scenario `{}`, seed {seed}: Core wasn't put together: {e:?}",
                self.name
            )
        });
        let (mut orders, mut ran, mut session, mut stopped) =
            (Vec::new(), Vec::new(), None, Vec::new());
        for step in &self.when {
            take(
                &mut core,
                step,
                &mut session,
                &mut orders,
                &mut ran,
                &mut stopped,
            )
            .and_then(|()| core.flush())
            .unwrap_or_else(|e| panic!("scenario `{}`: step {step:?}: {e:?}", self.name));
        }
        (core, orders)
    }

    fn config(&self, seed: u64) -> Config {
        Config {
            seed,
            staging: self.given.staging,
            deadline: self.given.deadline,
            providers: self.given.providers.clone(),
            delivery: self.given.delivery,
        }
    }

    /// Plays the steps from `seed` and reads the run, checking nothing.
    ///
    /// # Panics
    ///
    /// If Core can't be put together, or a step fails other than by a planted crash.
    pub fn play(
        &self,
        seed: u64,
        environment: &mut dyn Environment,
        swap_to: Box<dyn Environment>,
    ) -> Run {
        let config = self.config(seed);
        let mut core = Core::assemble(&config, environment).unwrap_or_else(|e| {
            panic!(
                "scenario `{}`, seed {seed}: Core wasn't put together: {e:?}",
                self.name
            )
        });
        core.swap_to = Some(swap_to);
        let mut orders: Vec<Id> = Vec::new();
        let mut ran: Vec<Id> = Vec::new();
        let mut session: Option<Id> = None;
        let mut crashed = false;
        let mut stopped = Vec::new();
        for step in &self.when {
            let done = catch_unwind(AssertUnwindSafe(|| {
                take(
                    &mut core,
                    step,
                    &mut session,
                    &mut orders,
                    &mut ran,
                    &mut stopped,
                )
            }));
            match done {
                Ok(Ok(())) => {
                    // A step's end: whatever a delivery held back reaches the state machine now.
                    if let Err(e) = core.flush() {
                        panic!(
                            "scenario `{}`, seed {seed}: the state machine: {e:?}",
                            self.name
                        );
                    }
                }
                Ok(Err(e)) => panic!(
                    "scenario `{}`, seed {seed}: step {step:?} failed: {e:?}",
                    self.name
                ),
                Err(panic) if is_crash(&*panic) => {
                    crashed = true;
                    break;
                }
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
        let mut run = Run::read(self, seed, core, orders, crashed);
        run.stopped = stopped;
        run
    }
}

fn is_crash(panic: &(dyn std::any::Any + Send)) -> bool {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .is_some_and(|m| m.contains(CRASHED))
}

/// The base order's request, under `request`, for `amount`: from [`RESERVE`]'s bank back to it, the customer its
/// requester, beneficiary and payer, delivered to [`DESTINATION_ACCOUNT`].
pub fn base_request(request: &str, amount: &str, session: &Id) -> OrderRequest {
    let customer = kit::customer();
    OrderRequest {
        request: id(request),
        caller: customer.clone(),
        parties: Parties {
            requester: customer.party.clone(),
            beneficiary: customer.party.clone(),
            settlement_account_at: None,
            payer: customer.party.clone(),
        },
        origin_reserve: id(RESERVE),
        session: session.clone(),
        destination: Destination {
            reserve: id(RESERVE),
            node: OuterNode {
                network: format!("{RESERVE}-bank"),
            },
            account: id(DESTINATION_ACCOUNT),
        },
        route: serde_json::from_value(json!({
            "request_id": format!("{request}-route"),
            "round": {"kind": "shortlist"},
            "from": kit::bank(RESERVE),
            "to": kit::bank(RESERVE),
            "amount": amount,
            "objective": {"kind": "lowest_total_cost"},
        }))
        .expect("a route request"),
        inputs: BTreeMap::from([
            ("payer.bank_account".to_owned(), "acct-1".to_owned()),
            (
                "beneficiary.bank_account".to_owned(),
                DESTINATION_ACCOUNT.to_owned(),
            ),
        ]),
        needs: Vec::new(),
    }
}

/// Takes one step.
fn take(
    core: &mut Core,
    step: &Step,
    session: &mut Option<Id>,
    orders: &mut Vec<Id>,
    ran: &mut Vec<Id>,
    stopped: &mut Vec<(String, String)>,
) -> Result<(), StepError> {
    let customer = kit::customer();
    match step {
        Step::Order { request, amount } => {
            let session = match session {
                Some(s) => s.clone(),
                None => {
                    let s = core.open_session(&customer)?;
                    *session = Some(s.clone());
                    s
                }
            };
            let told = core.request(base_request(request, amount, &session))?;
            let order = Core::order_of(&told).ok_or(StepError::NotNow("the order opens"))?;
            let told = core.quote(&customer, &order)?;
            let Told::Exists {
                status: IntakeStatus::Quoted { digest, .. },
                ..
            } = told
            else {
                return Err(StepError::NotNow("the order is quoted"));
            };
            core.accept(&customer, &order, &digest)?;
            orders.push(order);
        }
        Step::Run => {
            for order in orders.iter() {
                if !ran.contains(order) {
                    ran.push(order.clone());
                    // A part that stops is what the run tells, not a failed step: its segment waits.
                    if let Err(e) = core.run(order) {
                        stopped.push((order.as_str().to_owned(), format!("{e:?}")));
                    }
                }
            }
        }
        Step::Plant(fault) => match fault {
            Fault::CrashAfterRecord { call, nth } => {
                core.disk.plant(Crash::AfterRecord { call, nth: *nth })
            }
            Fault::CrashBeforeEffect(n) => core.faults.crash_before_effect = Some(*n),
            Fault::SwapBeforeEffect(n) => core.faults.swap_before_effect = Some(*n),
            Fault::Deliver(delivery) => core.feed.deliver(*delivery),
            Fault::Silence { provider, kind } => {
                core.fleet
                    .world()
                    .script_for(provider, kind, Behaviour::silent());
            }
        },
        Step::Advance(seconds) => core.clock.advance(*seconds),
        Step::Swap => {
            core.flush()?;
            if let Some(to) = core.swap_to.as_deref_mut() {
                core.feed.swap(to)?;
            }
        }
    }
    Ok(())
}

/// What a run left, as read once its steps ended or Core crashed: the log as it reads back from the disk, the
/// providers' books, the state machine's journals and what Core told of each order.
#[derive(Debug, Clone)]
pub struct Run {
    /// The scenario.
    pub scenario: &'static str,
    /// The `AC-*` it accepts.
    pub accepts: &'static str,
    /// Its seed.
    pub seed: u64,
    /// Whether a planted crash stopped Core.
    pub crashed: bool,
    /// The orders accepted, in order.
    pub orders: Vec<Id>,
    /// The log, as read back from the disk: each entry.
    pub log: Vec<Entry>,
    /// Whether the log and the evidence store read back and bound, and what the store found missing.
    pub reopened: Result<usize, String>,
    /// Every input the log took, in its order.
    pub inputs: Vec<RecordedInput>,
    /// Each order's journal on the state machine's host, as `(sequence, record id, state after)`.
    pub journals: BTreeMap<String, Vec<(u64, String, Option<String>)>>,
    /// Each order's state on the host.
    pub states: BTreeMap<String, Option<String>>,
    /// The orders the host holds.
    pub hosted: Vec<String>,
    /// Each order's state as Core follows it, read by its customer through policy; none after a crash.
    pub followed: BTreeMap<String, String>,
    /// Each provider's book: the journal the simulated world keeps, as JSON.
    pub books: BTreeMap<String, Vec<Value>>,
    /// Every submit the host answered: each input's record id, and whether it was new.
    pub deliveries: Vec<(String, bool)>,
    /// The environments the state machine ran in, in order.
    pub environments: Vec<String>,
    /// Why a planted swap failed, if it did.
    pub swap_failed: Option<String>,
    /// Each order whose route stopped before its end, and why.
    pub stopped: Vec<(String, String)>,
}

fn state_name(order: &rutent_contracts::state_machine::Order) -> String {
    serde_json::to_value(order.state)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{:?}", order.state))
}

impl Run {
    fn read(
        scenario: &Scenario,
        seed: u64,
        mut core: Core,
        orders: Vec<Id>,
        crashed: bool,
    ) -> Self {
        if !crashed {
            let _ = core.flush();
        }
        let machine = core.feed.machine();
        let hosted: Vec<String> = machine
            .orders()
            .unwrap_or_default()
            .iter()
            .map(|o| o.as_str().to_owned())
            .collect();
        let mut journals = BTreeMap::new();
        let mut states = BTreeMap::new();
        for order in &hosted {
            let at = rutent_contracts::state_machine::Id::new(order.as_str()).expect("an id");
            journals.insert(
                order.clone(),
                machine
                    .journal(&at)
                    .unwrap_or_default()
                    .iter()
                    .map(|r| {
                        (
                            r.seq,
                            r.input.record_id.as_str().to_owned(),
                            r.state_after.as_ref().map(state_name),
                        )
                    })
                    .collect(),
            );
            states.insert(
                order.clone(),
                machine.state(&at).ok().flatten().as_ref().map(state_name),
            );
        }
        let mut followed = BTreeMap::new();
        if !crashed {
            for order in &orders {
                if let Ok(status) = core.status(&kit::customer(), order) {
                    followed.insert(
                        order.as_str().to_owned(),
                        serde_json::to_value(status.state)
                            .ok()
                            .and_then(|v| v.as_str().map(str::to_owned))
                            .unwrap_or_default(),
                    );
                }
            }
        }
        let mut books = BTreeMap::new();
        for spec in &scenario.given.providers {
            books.insert(
                spec.provider.clone(),
                core.fleet
                    .world()
                    .journal(&spec.provider)
                    .iter()
                    .map(|e| serde_json::to_value(e).expect("a journal entry is JSON"))
                    .collect(),
            );
        }
        let deliveries = core
            .feed
            .deliveries
            .iter()
            .map(|(i, new)| (i.record_id.as_str().to_owned(), *new))
            .collect();
        let environments = core.feed.environments.clone();
        let swap_failed = core.faults.swap_failed.as_ref().map(|e| format!("{e:?}"));
        // Core goes before its disk is read back: what reads it then is a process of its own, as after a crash.
        let disk = core.disk.clone();
        let clock = core.clock.clone();
        drop(core);
        let reopened = read_back(&disk, &clock).and_then(|(store, log)| {
            let bound = store.bind(std::rc::Rc::clone(&log))?;
            Ok((bound.missing().len(), log))
        });
        let (log, reopened) = match reopened {
            Ok((missing, log)) => (
                log.borrow()
                    .entries()
                    .iter()
                    .map(|(_, e)| e.clone())
                    .collect(),
                Ok(missing),
            ),
            Err(e) => (Vec::new(), Err(format!("{e:?}"))),
        };
        let inputs: Vec<RecordedInput> = log
            .iter()
            .filter_map(|e| match e {
                Entry::Verified(v) => match &v.what {
                    What::Input { input } => Some(input.clone()),
                    _ => None,
                },
                Entry::Stated(_) => None,
            })
            .collect();
        Run {
            scenario: scenario.name,
            accepts: scenario.accepts,
            seed,
            crashed,
            orders,
            log,
            reopened,
            inputs,
            journals,
            states,
            hosted,
            followed,
            books,
            deliveries,
            environments,
            swap_failed,
            stopped: Vec::new(),
        }
    }

    /// The run with what no seed reaches taken out: every digest and signature value replaced by the order it
    /// first appeared in, so two runs from one seed project to one value though the evidence store salted their
    /// statements apart (`T-022-03`).
    pub fn projection(&self) -> Value {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        let mut value = json!({
            "scenario": self.scenario,
            "accepts": self.accepts,
            "seed": self.seed,
            "crashed": self.crashed,
            "orders": self.orders,
            "log": self.log,
            "reopened": format!("{:?}", self.reopened),
            "journals": self.journals,
            "states": self.states,
            "hosted": self.hosted,
            "followed": self.followed,
            "books": self.books,
            "deliveries": self.deliveries,
            "environments": self.environments,
            "stopped": self.stopped,
        });
        normalise(&mut value, &mut seen);
        value
    }

    /// Every movement a provider's book holds that has no `move_value` effect of the log behind it: an effect
    /// without its record (`NFR-5`, `INV-7`). A movement is a book entry of an operation that moves value for
    /// real; its record is the operator's statement of that operation in the log, about its segment and
    /// transaction, and an effect of `move_value` on that segment.
    pub fn effects_without_record(&self) -> Vec<String> {
        let mut missing = Vec::new();
        for (provider, book) in &self.books {
            for entry in book {
                let operation = &entry["operation"]["statement"];
                let real = matches!(
                    operation["kind"].as_str(),
                    Some("withdrawal" | "deposit" | "transfer" | "conversion")
                ) && operation["dry_run"].as_bool() != Some(true)
                    || operation["kind"] == "reversal";
                if !real {
                    continue;
                }
                let transaction = operation["transaction"].as_str().unwrap_or_default();
                let segment = self.log.iter().find_map(|e| match e {
                    Entry::Stated(s) => s.about.as_ref().and_then(|a| {
                        (a.transaction.as_ref().map(Id::as_str) == Some(transaction))
                            .then(|| a.segment.clone())
                            .flatten()
                    }),
                    Entry::Verified(_) => None,
                });
                let recorded = segment.is_some_and(|segment| {
                    self.log.iter().any(|e| {
                        matches!(e, Entry::Verified(v) if matches!(&v.what, What::Effect {
                            action: rutent_contracts::core_internal::Action::MoveValue { segment: s, provider: p, .. },
                            ..
                        } if *s == segment && p.as_str() == provider))
                    })
                });
                if !recorded {
                    missing.push(format!("{provider}: {transaction}"));
                }
            }
        }
        missing
    }

    /// Each order's state, folded from the log's inputs through `Stm` alone, with no host: what replaying the
    /// recorded inputs reaches (`NFR-4`).
    pub fn replayed(&self) -> BTreeMap<String, Option<String>> {
        let mut orders: BTreeMap<String, rutent_contracts::state_machine::Order> = BTreeMap::new();
        for input in &self.inputs {
            let key = input.subject.order.as_str().to_owned();
            let next = match orders.get(&key) {
                None => Stm::open(input),
                Some(order) => Some(Stm::transition(order, input)),
            };
            if let Some(next) = next {
                orders.insert(key, next);
            }
        }
        orders
            .into_iter()
            .map(|(k, o)| (k, Some(state_name(&o))))
            .collect()
    }

    /// How many inputs the log took for `order`, and how many records the host holds for it.
    pub fn recorded(&self, order: &Id) -> (usize, usize) {
        let inputs = self
            .inputs
            .iter()
            .filter(|i| i.subject.order.as_str() == order.as_str())
            .count();
        let journal = self.journals.get(order.as_str()).map_or(0, Vec::len);
        (inputs, journal)
    }

    /// Whether the log holds a route chosen for `order`.
    pub fn started(&self, order: &Id) -> bool {
        self.inputs.iter().any(|i| {
            i.subject.order.as_str() == order.as_str()
                && matches!(i.observed, Observed::RouteChosen { .. })
        })
    }
}

/// Replaces each 64-hex digest and each 86-character signature value in `value` by the order it first appeared in.
fn normalise(value: &mut Value, seen: &mut BTreeMap<String, usize>) {
    match value {
        Value::String(s) => {
            let hex = s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
            let signature = s.len() == 86
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if hex || signature {
                let next = seen.len();
                let n = *seen.entry(s.clone()).or_insert(next);
                *s = format!("#{n}");
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| normalise(v, seen)),
        Value::Object(map) => map.values_mut().for_each(|v| normalise(v, seen)),
        _ => {}
    }
}

/// A seeded scenario's steps: two orders, each of an amount the seed picks, a delivery fault and, where the seed
/// says so, the state machine swapped before an effect and the reserve silent on a deposit. Two runs from one seed
/// take the same steps.
pub fn seeded_steps(seed: u64) -> Vec<Step> {
    let mut draw = Seeded::new(seed ^ 0x5ce4_a410);
    // Two orders of at most 20 100 000 each fit the simulated reserve's opening balance of 50 000 000.
    let amount = |draw: &mut Seeded| (100 * (1_000 + draw.below(200_000))).to_string();
    let delivery = match draw.below(3) {
        0 => Delivery::Once,
        1 => Delivery::Repeated,
        _ => Delivery::Reordered,
    };
    let mut steps = vec![
        Step::Plant(Fault::Deliver(delivery)),
        Step::Order {
            request: "req-seeded-1".to_owned(),
            amount: amount(&mut draw),
        },
        Step::Order {
            request: "req-seeded-2".to_owned(),
            amount: amount(&mut draw),
        },
    ];
    if draw.below(2) == 1 {
        steps.push(Step::Plant(Fault::SwapBeforeEffect(
            usize::try_from(draw.below(4)).unwrap_or(0),
        )));
    }
    if draw.below(4) == 0 {
        steps.push(Step::Plant(Fault::Silence {
            provider: RESERVE.to_owned(),
            kind: "deposit".to_owned(),
        }));
    }
    steps.push(Step::Run);
    steps
}

/// The reference host, as a swap's target.
pub fn reference() -> Box<dyn Environment> {
    Box::new(Reference)
}
