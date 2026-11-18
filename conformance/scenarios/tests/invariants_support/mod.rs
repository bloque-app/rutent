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
}

impl Kind {
    /// The kind of fault seed `seed` draws, when it isn't pinned.
    fn drawn(draw: &mut Seeded) -> Kind {
        match draw.below(8) {
            0 => Kind::Clean,
            1 => Kind::FailsAtTheEnd,
            2 => Kind::FailsInTheMiddle,
            3 => Kind::SilentAtTheEnd,
            4 => Kind::CrashAfterRecord(draw.below(3) as usize),
            5 => Kind::CrashBeforeEffect(draw.below(3) as usize),
            6 => Kind::Repeated,
            _ => Kind::Rerun,
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
/// kind has a meaning on both.
pub const PINNED: [Plan; 12] = [
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
}

impl Sim {
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
/// as `T-024-06` follows them, and the run read back.
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
    let (mut core, _) = scenario.core(plan.seed, &mut Reference);
    let mut orders: Vec<Id> = Vec::new();
    let mut notes = Vec::new();
    let done = catch_unwind(AssertUnwindSafe(|| {
        drive(&mut core, plan, &mut orders, &mut notes);
    }));
    let crashed = match done {
        Ok(()) => false,
        Err(panic) if is_crash(&*panic) => true,
        Err(panic) => resume_unwind(panic),
    };
    let run = Run::read(&scenario, plan.seed, core, orders, crashed);
    Sim { plan, run, notes }
}

fn drive(core: &mut Core, plan: Plan, orders: &mut Vec<Id>, notes: &mut Vec<String>) {
    let mut draw = Seeded::new(plan.seed ^ 0x0de1_7a5e);
    let customer = kit::customer();
    let session = core.open_session(&customer).expect("a session");
    core.swap_to = Some(reference());
    let amounts: Vec<String> = match plan.shape {
        // Two orders of at most 20 100 000 each fit the simulated reserve's opening balance.
        Shape::Base => (0..1 + draw.below(2))
            .map(|_| (100 * (1_000 + draw.below(200_000))).to_string())
            .collect(),
        Shape::Joined => vec![kit::AMOUNT.to_owned()],
    };
    let mut unknown: Vec<(Id, Id)> = Vec::new();
    let (last, middle, kind) = match plan.shape {
        Shape::Base => (RESERVE, RESERVE, plan.kind),
        Shape::Joined => (OUT, EXCHANGE, plan.kind),
    };
    match plan.kind {
        Kind::FailsAtTheEnd => core
            .world()
            .script_for(last, "deposit", Behaviour::act(Act::Fail)),
        Kind::FailsInTheMiddle => {
            let (provider, op) = if plan.shape == Shape::Joined {
                (middle, "conversion")
            } else {
                (middle, "deposit")
            };
            core.world()
                .script_for(provider, op, Behaviour::act(Act::Fail));
        }
        Kind::SilentAtTheEnd => core.world().script_for(last, "deposit", Behaviour::late()),
        Kind::CrashAfterRecord(nth) => plant(
            core,
            &Fault::CrashAfterRecord {
                call: "effect",
                nth,
            },
        ),
        Kind::CrashBeforeEffect(nth) => plant(core, &Fault::CrashBeforeEffect(nth)),
        Kind::Repeated => {
            // The same operation sent to a provider twice, under its transaction. A provider that declares no safe
            // retry (the simulated exchange, which `T-024-01` models as moving again on a repeat) is never repeated
            // to by Core (`REQ-CAT-10`), and nothing below Core refuses a duplicate to it: that gap is recorded in
            // `F-024`'s notes, so the joined route's repeats are the state machine's alone.
            if plan.shape == Shape::Base {
                plant(core, &Fault::DeliverTwice);
            }
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
    let _ = (kind, Paid::Full);
    for (n, amount) in amounts.iter().enumerate() {
        let name = format!("req-inv-{}-{n}", plan.seed);
        let request = order_request(plan.shape, &name, &session, amount);
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
        match core.run(&order) {
            Ok(ran) => {
                for part in &ran {
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
            Err(e) => notes.push(format!("run {n}: {e:?}")),
        }
        core.flush()
            .expect("the state machine takes what the log did");
    }
    if plan.kind == Kind::Rerun {
        for order in orders.clone() {
            if let Err(e) = core.run(&order) {
                notes.push(format!("rerun: {e:?}"));
            }
        }
    }
    // The failures, followed: a part that can't go on is recalculated, and left stuck where no route is left; a
    // provider that never answered is timed.
    for order in orders.clone() {
        match plan.kind {
            Kind::FailsAtTheEnd | Kind::FailsInTheMiddle => match core.recalculate(&order) {
                Ok(r) => {
                    notes.push(format!(
                        "recalculate: {}",
                        format!("{r:?}").chars().take(60).collect::<String>()
                    ));
                    match &r {
                        Recalculation::Recalculated(recalculated) => {
                            if let Err(e) = core.run_recalculated(&order, recalculated) {
                                notes.push(format!("run recalculated: {e:?}"));
                            }
                        }
                        Recalculation::NoRouteLeft { .. } => {
                            if let Err(e) = core.record_no_route(&order, &r) {
                                notes.push(format!("no route: {e:?}"));
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => notes.push(format!("recalculate: {e:?}")),
            },
            Kind::SilentAtTheEnd => {
                core.clock.advance(2 * 86_400 + 60);
                match core.watch(&order) {
                    Ok(w) => notes.push(format!(
                        "watch: {}",
                        format!("{w:?}").chars().take(40).collect::<String>()
                    )),
                    Err(e) => notes.push(format!("watch: {e:?}")),
                }
                // A stuck order starts no new route and hands nothing over (`AC-5`): both are asked, and what the log
                // then holds is what the invariants read.
                match core.recalculate(&order) {
                    Ok(r) => notes.push(format!(
                        "recalculate: {}",
                        format!("{r:?}").chars().take(60).collect::<String>()
                    )),
                    Err(e) => notes.push(format!("recalculate: {e:?}")),
                }
                if let Err(e) = core.run(&order) {
                    notes.push(format!("run again: {e:?}"));
                }
                // The provider's answer comes late, twice: kept once, and it moves nothing.
                for (o, segment) in unknown.iter().filter(|(o, _)| *o == order) {
                    if let Some(late) = core.provider_again(o, segment) {
                        for _ in 0..2 {
                            if let Err(e) = core.late_delivery(o, segment, &late) {
                                notes.push(format!("late delivery: {e:?}"));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        core.flush()
            .expect("the state machine takes what the log did");
    }
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

/// What a provider's own book says it did: each operation that moved value, as its publisher records it.
pub fn book_segments(sim: &Sim) -> Vec<(String, Value, rutent_conformance::sim::InnerSegment)> {
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
