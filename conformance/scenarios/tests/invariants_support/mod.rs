//! The seeded runs `T-024-08`'s invariant tests read (`INV-1` to `INV-10`, `NFR-5`): the assembled Core put to work on
//! orders over the simulated providers with `T-024-02`'s faults planted and `T-024-06`'s failure steps taken, each run
//! drawn from a seed and read back from the disk the log survives on.
//!
//! **Runs are the same for every invariant.** [`runs`] plays them once for the whole test binary: [`PINNED`] first, one
//! for each kind of fault, so no invariant is tested over runs that never reached what it guards against, then the
//! seeds `RUTENT_SCENARIO_SEEDS` sets (CI's seeded step), each drawing its own kind of fault. A run is reproduced by its
//! seed and kind alone ([`play`]); a failing check prints both.
//!
//! **Nothing here counts for a check.** A run's [`Sim`] holds the run as read back: the log's entries, the statements
//! the evidence store kept for the log's commitments, the state machine's journals and the providers' own books.
//! The invariants are counted from those in `invariants.rs`, never from what a driver here did.

#![allow(dead_code)]

use rutent_scenarios::on_tick::OnTick;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::OnceLock;

use rutent_conformance::sim::{Act, Behaviour, Delivery as Paid};
use rutent_contracts::router_api::Id;
use rutent_core_orch::intake::OrderRequest;
use rutent_core_orch::recalc::Recalculation;
use rutent_scenarios::assembly::machine::{Delivery, Reference};
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Fault, Given, Run, Scenario, plant, reference, seeds};
use rutent_scenarios::kit::{self, RESERVE, Seeded};

use crate::execution_support::{OTHER, outer, request, within};

pub const IN: &str = "sim-join-in";
pub const EXCHANGE: &str = "sim-join-exchange";
pub const RAMP: &str = "sim-join-ramp-quick";
pub const OUT: &str = "sim-join-out";

/// The acceptance criterion every run accepts: `AC-28`'s base order, `AC-1`'s three providers.
const BASE_AC: &str = "AC-28";
const JOINED_AC: &str = "AC-1";

/// The routes a run puts orders on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// One reserve: its withdrawal and its deposit.
    Base,
    /// A reserve, an exchange, a ramp and the reserve the value is deposited in.
    Joined,
}

/// The fault a run is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// No fault.
    Clean,
    /// The last provider of the route refuses its deposit, having moved nothing.
    FailsAtTheEnd,
    /// A provider in the middle of the route fails.
    FailsInTheMiddle,
    /// The provider of the final deposit takes it and never answers.
    SilentAtTheEnd,
    /// The process crashes right after the log's store took the `n`th effect, before it answered.
    CrashAfterRecord(usize),
    /// The process crashes right before the platform carries out the `n`th effect.
    CrashBeforeEffect(usize),
    /// Every operation reaches its provider twice, the state machine takes each input twice or out of order, and the
    /// state machine is swapped mid-order.
    Repeated,
    /// The whole route is run again after it ended.
    Rerun,
    /// The process crashes right after the log's store took the `n`th effect, and Core and its platform are restarted
    /// from their disks and carry on from the log (`T-024-21`).
    RestartAfterRecord(usize),
    /// The process crashes right after the platform carried out the `n`th effect, its provider having moved, and before
    /// Core logged its answer, and Core and its platform are restarted and carry on (`T-020-61`, item 9: the one point a
    /// restarted Core could move value twice; `RestartBeforeEffect` fell at `RestartAfterRecord`'s point).
    RestartAfterEffect(usize),
    /// The process crashes right after the log's store took the `n`th provider's answer, its result statement, the
    /// provider having moved, before Core acted on it: Core and its platform are restarted, and the segment goes on
    /// from its logged answer (`T-024-21`).
    RestartAfterAnswer(usize),
    /// The final deposit is taken and never answered, the part is declared stuck, and then Core and its platform crash
    /// and are restarted, and the stuck order is followed on from the log (`T-024-21`).
    RestartWhileStuck,
}

/// The restart kinds whose caught order ends stuck, not complete, for the operator (`T-020-61`, item 8 as `T-020-63` designed
/// it): a crash after the log took a segment's effect and before the platform carried it out leaves a segment whose provider
/// knows no result of it; the restarted Core asks its status where it answers status queries, repeats only at a provider
/// that declares a safe repeat and no status query, and otherwise reads back the answer the platform recorded, which it
/// recorded none of, so it leaves the segment for the stuck check, never handing it again. A crash after the platform
/// carried the operation out (`RestartAfterEffect`) completes at every provider, the exchange's included: its answer is read
/// back from the platform's record (item 10).
pub fn stuck_for_the_operator(plan: &Plan) -> bool {
    matches!(plan.kind, Kind::RestartAfterRecord(_))
}

impl Kind {
    /// Whether a run of this kind restarts Core and its platform mid-order (`T-024-21`).
    pub fn restarts(self) -> bool {
        matches!(
            self,
            Kind::RestartAfterRecord(_)
                | Kind::RestartAfterEffect(_)
                | Kind::RestartAfterAnswer(_)
                | Kind::RestartWhileStuck
        )
    }
}

impl Kind {
    /// The kind of fault seed `seed` draws, when it isn't pinned. A crash is followed by a restart as often as not
    /// (`T-024-21`): the seeds CI sets plant restarts of Core and its platform at points they draw.
    fn drawn(draw: &mut Seeded) -> Kind {
        match draw.below(8) {
            0 => Kind::Clean,
            1 => Kind::FailsAtTheEnd,
            2 => Kind::FailsInTheMiddle,
            3 => Kind::SilentAtTheEnd,
            4 => {
                let nth = draw.below(3) as usize;
                match draw.below(3) {
                    0 => Kind::CrashAfterRecord(nth),
                    1 => Kind::RestartAfterRecord(nth),
                    _ => Kind::RestartAfterAnswer(nth.min(1)),
                }
            }
            5 => {
                let nth = draw.below(3) as usize;
                if draw.below(2) == 0 {
                    Kind::CrashBeforeEffect(nth)
                } else {
                    Kind::RestartAfterEffect(nth)
                }
            }
            6 => Kind::Repeated,
            _ => {
                if draw.below(2) == 0 {
                    Kind::Rerun
                } else {
                    Kind::RestartWhileStuck
                }
            }
        }
    }
}

/// One run's plan: what it is reproduced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Its seed: Core's identifiers, the amounts and every draw below.
    pub seed: u64,
    /// The routes it puts orders on.
    pub shape: Shape,
    /// The fault it is about.
    pub kind: Kind,
}

/// The runs every test binary plays, whatever the seeds say: one of each kind of fault, on both routes where the
/// kind has a meaning on both, restarts of Core and its platform among them (`T-024-21`).
pub const PINNED: [Plan; 19] = [
    Plan {
        seed: 101,
        shape: Shape::Base,
        kind: Kind::Clean,
    },
    Plan {
        seed: 102,
        shape: Shape::Joined,
        kind: Kind::Clean,
    },
    Plan {
        seed: 103,
        shape: Shape::Base,
        kind: Kind::FailsAtTheEnd,
    },
    Plan {
        seed: 104,
        shape: Shape::Joined,
        kind: Kind::FailsInTheMiddle,
    },
    Plan {
        seed: 105,
        shape: Shape::Base,
        kind: Kind::SilentAtTheEnd,
    },
    Plan {
        seed: 106,
        shape: Shape::Base,
        kind: Kind::CrashAfterRecord(0),
    },
    Plan {
        seed: 107,
        shape: Shape::Base,
        kind: Kind::CrashBeforeEffect(1),
    },
    Plan {
        seed: 108,
        shape: Shape::Joined,
        kind: Kind::CrashAfterRecord(2),
    },
    Plan {
        seed: 109,
        shape: Shape::Base,
        kind: Kind::Repeated,
    },
    Plan {
        seed: 110,
        shape: Shape::Joined,
        kind: Kind::Repeated,
    },
    Plan {
        seed: 111,
        shape: Shape::Joined,
        kind: Kind::Rerun,
    },
    Plan {
        seed: 112,
        shape: Shape::Joined,
        kind: Kind::FailsAtTheEnd,
    },
    Plan {
        seed: 113,
        shape: Shape::Base,
        kind: Kind::RestartAfterRecord(0),
    },
    Plan {
        seed: 114,
        shape: Shape::Base,
        kind: Kind::RestartAfterEffect(1),
    },
    Plan {
        seed: 115,
        shape: Shape::Joined,
        kind: Kind::RestartAfterRecord(2),
    },
    Plan {
        seed: 116,
        shape: Shape::Joined,
        kind: Kind::RestartAfterEffect(1),
    },
    Plan {
        seed: 117,
        shape: Shape::Base,
        kind: Kind::RestartWhileStuck,
    },
    Plan {
        seed: 118,
        shape: Shape::Base,
        kind: Kind::RestartAfterAnswer(0),
    },
    Plan {
        seed: 119,
        shape: Shape::Joined,
        kind: Kind::RestartAfterAnswer(1),
    },
];

/// The seeds the seeded runs are made under where `RUTENT_SCENARIO_SEEDS` sets none.
pub const DEFAULT_SEEDS: [u64; 4] = [1, 2, 3, 4];

/// A run, read back.
pub struct Sim {
    /// What it was reproduced from.
    pub plan: Plan,
    /// The run as read back from its disk.
    pub run: Run,
    /// What each failure step answered, for the record of what the run did.
    pub notes: Vec<String>,
    /// The orders a planted crash caught mid-run, Core and its platform restarted under them (`T-024-21`).
    pub caught: Vec<String>,
    /// What Core knew of its orders right before each restart and right after it (`T-020-61`, item 9's Low).
    pub snapshots: Vec<(Snapshot, Snapshot)>,
}

impl Sim {
    /// The state a restarted run's `order` must end in (`T-024-21`): `stuck` where it was stuck when the run restarted
    /// (`RestartWhileStuck`), or a crash caught it in a kind `stuck_for_the_operator` marks; `complete` otherwise.
    pub fn expected_end(&self, order: &str) -> &'static str {
        if (self.plan.kind == Kind::RestartWhileStuck || stuck_for_the_operator(&self.plan))
            && self.caught.iter().any(|o| o == order)
        {
            "stuck"
        } else {
            "complete"
        }
    }

    /// How a failing check names this run.
    pub fn name(&self) -> String {
        format!(
            "seed {} ({:?} {:?})",
            self.plan.seed, self.plan.shape, self.plan.kind
        )
    }
}

/// The plan seed `seed` draws.
pub fn plan_of(seed: u64) -> Plan {
    let mut draw = Seeded::new(seed ^ 0x1a77_a410);
    let shape = if draw.below(2) == 0 {
        Shape::Base
    } else {
        Shape::Joined
    };
    let mut kind = Kind::drawn(&mut draw);
    // The faults that hold of the base route only: a silent final deposit and a refusing one need its second segment
    // to be the destination's, and so do on the joined one; the plan keeps the draw as it is.
    if shape == Shape::Joined && matches!(kind, Kind::SilentAtTheEnd) {
        kind = Kind::FailsAtTheEnd;
    }
    if shape == Shape::Joined && matches!(kind, Kind::RestartWhileStuck) {
        kind = Kind::RestartAfterRecord(1);
    }
    Plan { seed, shape, kind }
}

/// Every run: the pinned ones, then the seeded.
pub fn runs() -> &'static [Sim] {
    static RUNS: OnceLock<Vec<SendSim>> = OnceLock::new();
    let all = RUNS.get_or_init(|| {
        let mut plans: Vec<Plan> = PINNED.to_vec();
        plans.extend(seeds(&DEFAULT_SEEDS).into_iter().map(plan_of));
        plans.into_iter().map(|p| SendSim(play(p))).collect()
    });
    // SAFETY-free: `SendSim` is a plain wrapper; see its note.
    unsafe { std::slice::from_raw_parts(all.as_ptr().cast::<Sim>(), all.len()) }
}

/// A [`Sim`] kept in a static. It holds plain data only (the run's entries, statements and books).
struct SendSim(Sim);
unsafe impl Sync for SendSim {}
unsafe impl Send for SendSim {}

fn given_of(shape: Shape) -> Given {
    match shape {
        Shape::Base => Given::one_reserve(Staged::AtOnce),
        Shape::Joined => crate::execution_support::given(&[IN, EXCHANGE, RAMP, OUT]),
    }
}

fn is_crash(panic: &(dyn std::any::Any + Send)) -> bool {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .is_some_and(|m| m.contains("the assembled core crashed"))
}

fn order_request(shape: Shape, name: &str, session: &Id, amount: &str) -> OrderRequest {
    match shape {
        Shape::Base => within(name, session, amount, RESERVE, OTHER),
        Shape::Joined => request(
            name,
            session,
            amount,
            IN,
            outer("sim-join-in-bank", "COP"),
            outer("sim-join-out-bank", "USD"),
            OUT,
            OTHER,
            "lowest_total_cost",
        ),
    }
}

/// Plays `plan`: the orders opened, quoted and accepted, the fault planted, the routes run and the failures followed
/// as `T-024-06` follows them, and the run read back. A run of a restarting kind (`Kind::restarts`) restarts Core and
/// its platform where its crash falls, or where its stuck order is declared, and carries every order on from the log
/// (`T-024-21`).
pub fn play(plan: Plan) -> Sim {
    let scenario = Scenario::new(
        "invariants",
        if plan.shape == Shape::Base {
            BASE_AC
        } else {
            JOINED_AC
        },
        given_of(plan.shape),
    );
    let (core, _) = scenario.core(plan.seed, &mut Reference);
    let mut driving = Driving {
        core: Some(core),
        scenario: &scenario,
        plan,
        notes: Vec::new(),
        caught: Vec::new(),
        snapshots: Vec::new(),
    };
    let mut orders: Vec<Id> = Vec::new();
    let crashed = match drive(&mut driving, &mut orders) {
        Ok(()) => false,
        Err(Stopped) => true,
    };
    let Driving {
        core,
        notes,
        caught,
        snapshots,
        ..
    } = driving;
    let run = Run::read(
        &scenario,
        plan.seed,
        core.expect("Core, as the run left it"),
        orders,
        crashed,
    );
    Sim {
        plan,
        run,
        notes,
        caught,
        snapshots,
    }
}

/// Core stopped on a planted crash, and the run's kind doesn't restart it.
struct Stopped;

/// A run being driven: Core as it stands, restarted in place where the run's kind says.
struct Driving<'a> {
    core: Option<Core>,
    scenario: &'a Scenario,
    plan: Plan,
    notes: Vec<String>,
    caught: Vec<String>,
    snapshots: Vec<(Snapshot, Snapshot)>,
}

/// The orders Core knows, by id: the state Core follows each in and the amount its request asks.
pub type Snapshot = BTreeMap<String, (Option<String>, Option<String>)>;

impl Driving<'_> {
    fn core(&mut self) -> &mut Core {
        self.core.as_mut().expect("Core")
    }

    /// Core and its platform restarted after a crash (`Scenario::restart`, `T-024-21`): the orders Core knows, their
    /// states and amounts, read before and after it (`T-020-61`, item 9's Low).
    fn restart(&mut self, why: &str) {
        let core = self.core.take().expect("Core");
        let before = core.order_snapshot();
        let core = self.scenario.restart(core, self.plan.seed, &mut Reference);
        self.snapshots.push((before, core.order_snapshot()));
        self.core = Some(core);
        self.notes.push(format!("restarted: {why}"));
    }

    /// Runs `call` on Core. Where a planted crash stops it, a run of a restarting kind restarts Core and its platform
    /// and answers `Ok(None)`, the step to be carried on from the log; any other run stops there.
    fn guarded<T>(&mut self, call: impl FnOnce(&mut Core) -> T) -> Result<Option<T>, Stopped> {
        let core = self.core();
        match catch_unwind(AssertUnwindSafe(|| call(core))) {
            Ok(done) => Ok(Some(done)),
            Err(panic) if is_crash(&*panic) => {
                if self.plan.kind.restarts() {
                    self.restart("after the planted crash");
                    Ok(None)
                } else {
                    Err(Stopped)
                }
            }
            Err(panic) => resume_unwind(panic),
        }
    }
}

/// `late`, a provider's signed result, stated otherwise by the same provider: its first piece of evidence named
/// anew, signed again with the provider's statement key the world holds. `None` where it states no evidence.
fn contradicting(core: &mut Core, late: &[u8]) -> Option<Vec<u8>> {
    let mut statement: serde_json::Value = serde_json::from_slice(late).ok()?;
    let maker = statement["maker"].as_str()?.to_owned();
    let reference = statement["statement"]["outcome"]["evidence"][0]["reference"]
        .as_str()?
        .to_owned();
    statement["statement"]["outcome"]["evidence"][0]["reference"] =
        format!("{reference}-restated").into();
    core.world()
        .sign_as(&maker, &mut statement)
        .then(|| serde_json::to_vec(&statement).ok())
        .flatten()
}

/// The segments of `order` whose outcome a run left unknown.
fn unknown_of(ran: &[rutent_core_orch::segments::Ran], order: &Id, unknown: &mut Vec<(Id, Id)>) {
    for part in ran {
        if let rutent_core_orch::segments::Ran::Segments(segments) = part {
            for s in segments {
                if matches!(
                    s.outcome,
                    rutent_core_orch::segments::SegmentOutcome::Unknown(_)
                ) {
                    unknown.push((order.clone(), s.segment.clone()));
                }
            }
        }
    }
}

fn drive(driving: &mut Driving<'_>, orders: &mut Vec<Id>) -> Result<(), Stopped> {
    let plan = driving.plan;
    let mut draw = Seeded::new(plan.seed ^ 0x0de1_7a5e);
    let customer = kit::customer();
    let mut session = driving.core().open_session(&customer).expect("a session");
    driving.core().swap_to = Some(reference());
    let amounts: Vec<String> = match plan.shape {
        // Two orders of at most 20 100 000 each fit the simulated reserve's opening balance.
        Shape::Base => (0..1 + draw.below(2))
            .map(|_| (100 * (1_000 + draw.below(200_000))).to_string())
            .collect(),
        Shape::Joined => vec![kit::AMOUNT.to_owned()],
    };
    let mut unknown: Vec<(Id, Id)> = Vec::new();
    let (last, middle) = match plan.shape {
        Shape::Base => (RESERVE, RESERVE),
        Shape::Joined => (OUT, EXCHANGE),
    };
    {
        let core = driving.core();
        match plan.kind {
            Kind::FailsAtTheEnd => {
                core.world()
                    .script_for(last, "deposit", Behaviour::act(Act::Fail))
            }
            Kind::FailsInTheMiddle => {
                let (provider, op) = if plan.shape == Shape::Joined {
                    (middle, "conversion")
                } else {
                    (middle, "deposit")
                };
                core.world()
                    .script_for(provider, op, Behaviour::act(Act::Fail));
            }
            Kind::SilentAtTheEnd | Kind::RestartWhileStuck => {
                core.world().script_for(last, "deposit", Behaviour::late());
            }
            Kind::CrashAfterRecord(nth) | Kind::RestartAfterRecord(nth) => plant(
                core,
                &Fault::CrashAfterRecord {
                    call: "effect",
                    nth,
                },
            ),
            Kind::CrashBeforeEffect(nth) => {
                plant(core, &Fault::CrashBeforeEffect(nth));
            }
            Kind::RestartAfterEffect(nth) => {
                plant(core, &Fault::CrashAfterEffect(nth));
            }
            // Planted as the first order runs, so the answers counted are its segments', not a quote's.
            Kind::RestartAfterAnswer(_) => {}
            Kind::Repeated => {
                // The same operation sent to a provider twice, under its transaction, on both routes. A provider that
                // declares no safe retry (the simulated exchange, which `T-024-01` models as moving again on a repeat)
                // is never repeated to by Core (`REQ-CAT-10`), so the repeat is injected beneath Core, and the
                // platform refuses it (`T-013-09`): the joined route's `INV-8` holds the platform, and not only the
                // provider.
                plant(core, &Fault::DeliverTwice);
                let delivery = if draw.below(2) == 0 {
                    Delivery::Repeated
                } else {
                    Delivery::Reordered
                };
                plant(core, &Fault::Deliver(delivery));
                plant(core, &Fault::SwapBeforeEffect(draw.below(3) as usize));
            }
            Kind::Clean | Kind::Rerun => {}
        }
    }
    let _ = Paid::Full;
    for (n, amount) in amounts.iter().enumerate() {
        let name = format!("req-inv-{}-{n}", plan.seed);
        let request = order_request(plan.shape, &name, &session, amount);
        let core = driving.core();
        let told = core.request(request).expect("the request opens an order");
        let order = Core::order_of(&told).expect("an order");
        let told = core.quote(&customer, &order).expect("quoted");
        let rutent_core_orch::intake::Told::Exists {
            status: rutent_core_orch::intake::Status::Quoted { digest, .. },
            ..
        } = told
        else {
            panic!(
                "seed {} {:?}: the order is quoted: {told:?}",
                plan.seed, plan.shape
            );
        };
        core.accept(&customer, &order, &digest).expect("accepted");
        orders.push(order.clone());
        if let (0, Kind::RestartAfterAnswer(nth)) = (n, plan.kind) {
            plant(
                driving.core(),
                &Fault::CrashAfterRecord {
                    call: rutent_contracts::provider_role::RESULT_SHAPE,
                    nth,
                },
            );
        }
        match driving.guarded(|core| core.ran_on_tick(&order))? {
            Some(Ok(ran)) => unknown_of(&ran, &order, &mut unknown),
            Some(Err(e)) => driving.notes.push(format!("run {n}: {e:?}")),
            // Restarted where the crash fell: every order accepted so far carries on from the log, and the session the
            // restarted Core's policy holds is the one the next order is asked in.
            None => {
                driving.caught.push(order.as_str().to_owned());
                session = driving.core().open_session(&customer).expect("a session");
                for order in orders.clone() {
                    match driving.guarded(|core| core.ran_on_tick(&order))? {
                        Some(Ok(ran)) => unknown_of(&ran, &order, &mut unknown),
                        Some(Err(e)) => driving.notes.push(format!("carried on: {e:?}")),
                        None => {}
                    }
                }
            }
        }
        driving
            .core()
            .flush()
            .expect("the state machine takes what the log did");
    }
    if plan.kind == Kind::Rerun {
        for order in orders.clone() {
            if let Err(e) = driving.core().ran_on_tick(&order) {
                driving.notes.push(format!("rerun: {e:?}"));
            }
        }
    }
    // A planted crash that never fell: Core and its platform are restarted all the same, once every order ran, and
    // each order is carried on from the log.
    if matches!(
        plan.kind,
        Kind::RestartAfterRecord(_) | Kind::RestartAfterEffect(_) | Kind::RestartAfterAnswer(_)
    ) && driving.core().restarts() == 0
    {
        // The crash never fell: it is taken back, so it never falls in the restart's own records.
        rutent_scenarios::harness::unplant_crashes(driving.core());
        driving.restart("after the orders ran, the planted crash having never fallen");
        for order in orders.clone() {
            match driving.guarded(|core| core.ran_on_tick(&order))? {
                Some(Err(e)) => driving.notes.push(format!("carried on: {e:?}")),
                Some(Ok(ran)) => unknown_of(&ran, &order, &mut unknown),
                None => {}
            }
        }
    }
    // The order a crash caught after its segment's effect was recorded and before its outcome was is held by the
    // restarted Core until its time passes, and then declared stuck, for the operator: only those, of the kinds
    // `stuck_for_the_operator` marks, are timed out. Every other order of a restarted run must have completed by itself.
    if stuck_for_the_operator(&plan) && !driving.caught.is_empty() {
        driving.core().clock.advance(2 * 86_400 + 60);
        for order in driving.caught.clone() {
            let order = rutent_contracts::router_api::Id::new(&order).expect("an order id");
            if let Ok(Some(watched)) = driving.guarded(|core| core.watch(&order)) {
                driving.notes.push(format!(
                    "watch after the restart: {}",
                    format!("{watched:?}").chars().take(40).collect::<String>()
                ));
            }
        }
        driving
            .core()
            .flush()
            .expect("the state machine takes what the log did");
    }
    // The failures, followed: a part that can't go on is recalculated, and left stuck where no route is left; a
    // provider that never answered is timed.
    for order in orders.clone() {
        match plan.kind {
            Kind::FailsAtTheEnd | Kind::FailsInTheMiddle => {
                match driving.core().recalculated_on_tick(&order) {
                    Ok(r) => {
                        driving.notes.push(format!(
                            "recalculate: {}",
                            format!("{r:?}").chars().take(60).collect::<String>()
                        ));
                        // The recalculated route runs on Core's next tick; where none was found, Core's run declared the
                        // part left with no route on the same tick (`REQ-ORD-28`).
                        if let Recalculation::Recalculated(_) = &r
                            && let Err(e) = driving.core().ran_once_on_tick(&order)
                        {
                            driving.notes.push(format!("run recalculated: {e:?}"));
                        }
                    }
                    Err(e) => driving.notes.push(format!("recalculate: {e:?}")),
                }
            }
            Kind::SilentAtTheEnd | Kind::RestartWhileStuck => {
                driving.core().clock.advance(2 * 86_400 + 60);
                match driving.core().watch(&order) {
                    Ok(w) => driving.notes.push(format!(
                        "watch: {}",
                        format!("{w:?}").chars().take(40).collect::<String>()
                    )),
                    Err(e) => driving.notes.push(format!("watch: {e:?}")),
                }
                // Declared stuck, Core and its platform crash and are restarted: the stuck order is followed on from
                // the log (`T-024-21`).
                if plan.kind == Kind::RestartWhileStuck && driving.core().restarts() == 0 {
                    driving.caught.extend(
                        unknown
                            .iter()
                            .filter(|(o, _)| *o == order)
                            .map(|(o, _)| o.as_str().to_owned()),
                    );
                    driving.restart("with the order stuck");
                }
                // A stuck order starts no new route and hands nothing over (`AC-5`): both are asked, and what the log
                // then holds is what the invariants read.
                match driving.core().recalculated_on_tick(&order) {
                    Ok(r) => driving.notes.push(format!(
                        "recalculate: {}",
                        format!("{r:?}").chars().take(60).collect::<String>()
                    )),
                    Err(e) => driving.notes.push(format!("recalculate: {e:?}")),
                }
                if let Err(e) = driving.core().ran_on_tick(&order) {
                    driving.notes.push(format!("run again: {e:?}"));
                }
                // The provider's answer comes late, twice: kept once, and it moves nothing.
                for (o, segment) in unknown.iter().filter(|(o, _)| *o == order) {
                    if let Some(late) = driving.core().provider_again(o, segment) {
                        for _ in 0..2 {
                            if let Err(e) = driving.core().late_delivery(o, segment, &late) {
                                driving.notes.push(format!("late delivery: {e:?}"));
                            }
                        }
                        // Then the provider states something else about it (`T-020-47`'s shape): the same result with
                        // its evidence named otherwise, signed anew by the provider. Kept as a further statement that
                        // moves nothing: `INV-4` holds it to that (`T-024-21`).
                        if plan.kind == Kind::SilentAtTheEnd
                            && let Some(other) = contradicting(driving.core(), &late)
                            && let Err(e) = driving.core().late_delivery(o, segment, &other)
                        {
                            driving.notes.push(format!("contradicting delivery: {e:?}"));
                        }
                    }
                }
            }
            _ => {}
        }
        driving
            .core()
            .flush()
            .expect("the state machine takes what the log did");
    }
    Ok(())
}

// --- What the log, read back, says ---------------------------------------------------------------------------------

use std::collections::BTreeMap;

use rutent_conformance::sim::{Entry as BookEntry, Spec, recorded_segments};
use rutent_contracts::core_internal::Entry;
use serde_json::Value;

use crate::execution_support::spec_of;

/// A number a statement states.
pub fn n(v: &Value) -> u128 {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("a figure the statement states: {v}"))
}

/// One segment Core handed to a provider, as the log read back and the statements it commits to tell it.
#[derive(Debug, Clone)]
pub struct Seg {
    /// Its order.
    pub order: String,
    /// Its id.
    pub segment: String,
    /// The provider it was handed to.
    pub provider: String,
    /// The place of its `move_value` effect in the log.
    pub effect: usize,
    /// How the effect ended, as the log's `result` entry says: `done` or `failed`; none where no result was logged.
    pub result: Option<String>,
    /// The place of that result in the log.
    pub result_at: Option<usize>,
    /// The operation the operator stated for it.
    pub op: Option<Value>,
    /// What the provider stated of it.
    pub res: Option<Value>,
}

impl Seg {
    /// The outcome's status the provider stated.
    pub fn status(&self) -> Option<&str> {
        self.res
            .as_ref()
            .and_then(|r| r["outcome"]["status"].as_str())
    }

    /// Whether the provider stated the segment succeeded and the log's result says it was done.
    pub fn settled(&self) -> bool {
        self.result.as_deref() == Some("done") && self.status() == Some("succeeded")
    }

    /// What the provider stated it took in.
    pub fn amount_in(&self) -> u128 {
        n(&self.res.as_ref().expect("a result")["outcome"]["amount_in"])
    }

    /// What it stated it gave out.
    pub fn amount_out(&self) -> u128 {
        n(&self.res.as_ref().expect("a result")["outcome"]["amount_out"])
    }

    /// The fee it stated.
    pub fn fee(&self) -> u128 {
        n(&self.res.as_ref().expect("a result")["outcome"]["fee"]["amount"])
    }

    /// The kind of operation.
    pub fn kind(&self) -> &str {
        self.op
            .as_ref()
            .and_then(|o| o["kind"].as_str())
            .unwrap_or("")
    }

    /// The transaction it ran under.
    pub fn transaction(&self) -> Option<String> {
        self.op
            .as_ref()
            .and_then(|o| o["transaction"].as_str().map(str::to_owned))
    }
}

/// Each entry of the log as JSON, in its place.
pub fn entries(run: &Run) -> Vec<Value> {
    run.log
        .iter()
        .map(|e| serde_json::to_value(e).expect("an entry is JSON"))
        .collect()
}

/// The statement of `shape` the evidence store kept for `transaction`.
fn kept(run: &Run, shape: &str, transaction: &str) -> Option<Value> {
    run.statements
        .iter()
        .find(|s| {
            s["shape"].as_str().is_some_and(|x| x.ends_with(shape))
                && s["statement"]["transaction"].as_str() == Some(transaction)
        })
        .map(|s| s["statement"].clone())
}

/// Every segment the log says Core handed over, in the order of the effects, read from the log's entries and the
/// statements they commit to: nothing a driver kept.
pub fn segments(run: &Run) -> Vec<Seg> {
    let log = entries(run);
    let mut segs: Vec<Seg> = Vec::new();
    for (at, e) in log.iter().enumerate() {
        let (what, who) = (&e["what"], e["kind"].as_str());
        if who == Some("verified")
            && what["call"] == "effect"
            && what["action"]["kind"] == "move_value"
        {
            let (order, segment) = (
                what["action"]["order"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                what["action"]["segment"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            );
            // The statements about this segment: the operator's operation and the provider's result.
            let mut transaction = None;
            for s in &log {
                if s["kind"] == "stated" && s["about"]["segment"].as_str() == Some(segment.as_str())
                {
                    transaction = s["about"]["transaction"].as_str().map(str::to_owned);
                }
            }
            let (op, res) = match &transaction {
                Some(t) => (kept(run, "operation", t), kept(run, "result", t)),
                None => (None, None),
            };
            segs.push(Seg {
                order,
                segment,
                provider: what["action"]["provider"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                effect: at,
                result: None,
                result_at: None,
                op,
                res,
            });
        }
    }
    for (at, e) in log.iter().enumerate() {
        if e["kind"] == "verified" && e["what"]["call"] == "result" {
            let index = e["what"]["effect"]["index"].as_u64().unwrap_or(u64::MAX) as usize;
            if let Some(seg) = segs.iter_mut().find(|s| s.effect == index) {
                seg.result = e["outcome"]["status"].as_str().map(str::to_owned);
                seg.result_at = Some(at);
            }
        }
    }
    segs
}

/// The providers a shape serves, as the specs the providers' books are replayed with.
pub fn specs_of(shape: Shape) -> Vec<Spec> {
    match shape {
        Shape::Base => vec![spec_of(RESERVE)],
        Shape::Joined => [IN, EXCHANGE, RAMP, OUT]
            .iter()
            .map(|p| spec_of(p))
            .collect(),
    }
}

/// One operation a provider's book says it did: the provider, the operation as stated, and the segment its publisher records.
pub type BookSegment = (String, Value, rutent_conformance::sim::InnerSegment);

/// What a provider's own book says it did: each operation that moved value, as its publisher records it.
pub fn book_segments(sim: &Sim) -> Vec<BookSegment> {
    let mut all = Vec::new();
    for spec in specs_of(sim.plan.shape) {
        let journal: Vec<BookEntry> = sim.run.books[&spec.provider]
            .iter()
            .map(|e| serde_json::from_value(e.clone()).expect("a book entry"))
            .filter(|e: &BookEntry| {
                serde_json::to_value(&e.operation.statement).expect("json")["dry_run"] != true
            })
            .collect();
        let ctx = rutent_scenarios::assembly::context();
        for seg in recorded_segments(&spec, &ctx, &journal) {
            let op = journal
                .iter()
                .find(|e| {
                    serde_json::to_value(&e.operation.statement).expect("json")["transaction"]
                        == seg.transaction
                })
                .map(|e| serde_json::to_value(&e.operation.statement).expect("json"))
                .expect("its operation");
            all.push((spec.provider.clone(), op, seg));
        }
    }
    all
}

/// The `Entry`s of the log, typed.
pub fn typed(run: &Run) -> &[Entry] {
    &run.log
}

/// Groups segments by order, each in the order of its effects.
pub fn by_order(segs: Vec<Seg>) -> BTreeMap<String, Vec<Seg>> {
    let mut by: BTreeMap<String, Vec<Seg>> = BTreeMap::new();
    for s in segs {
        by.entry(s.order.clone()).or_default().push(s);
    }
    by
}
