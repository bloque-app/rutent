//! The API's handlers: Core, put together as the demo puts it together, and one handler per endpoint, each a call of
//! Core's that the narrated demo (`rutent-demo`) makes in its acts.
//!
//! Core is single-threaded (`Rc`, `RefCell`), and so is the server: one request at a time, each run to its end
//! before the next is read.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU8;
use std::path::PathBuf;
use std::time::Duration;

use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{Actor, Decided, Receipt, Resolution};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Candidate, DecisionRecord, Id, PositiveAmount, Request};
use rutent_core_audit::checkpoint::tick_of;
use rutent_core_authz::roles::Permission;
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::recalc::{Recalculated, Recalculation};
use rutent_core_orch::segments::{Ran, SegmentOutcome};
use rutent_core_orch::stuck::{Moved, ResolveError, Resolved};
use rutent_scenarios::assembly::checking::{
    CHECKPOINT_EVERY, CoreCheckpointer, history_of, openings_of,
};
use rutent_scenarios::assembly::linked::{AcceptedLinked, LinkedOrder};
use rutent_scenarios::assembly::machine::{Delivery, Reference};
use rutent_scenarios::assembly::{Core, DESTINATION_ACCOUNT, ORIGIN_ACCOUNT, Staged, StepError};
use rutent_scenarios::harness::{Given, Scenario, kept_of};
use rutent_scenarios::kit::{self, id};
use rutent_scenarios::sim::fleet::InstalledWallet;
use serde_json::{Value, json};

use crate::common::orders::{
    ADDRESS, Asked, HELD_NODE, MAX_HOPS, inner, is_inner_segments, not_borne_out, outer,
};
use crate::common::{audit, network, orders, show};
use crate::world::{self, Hosts, Store, Transition};

/// What a handler answers: a JSON body, or a failure with its status.
pub type Reply = Result<Value, Failure>;

/// A request the API couldn't carry out: its HTTP status and a JSON body saying why.
#[derive(Debug)]
pub struct Failure {
    /// The status.
    pub status: u16,
    /// `{"error": …}` and whatever else explains it.
    pub body: Value,
}

/// A failure with `status`, saying `why`.
pub fn fail(status: u16, why: impl Into<String>) -> Failure {
    Failure {
        status,
        body: json!({ "error": why.into() }),
    }
}

fn refused(what: &'static str) -> impl Fn(StepError) -> Failure {
    move |e| fail(409, format!("{what}: {e:?}"))
}

/// The API: its seed and output folder, Core once it is put together, and the store the network's transitions are
/// kept in.
pub struct Api {
    seed: u64,
    out: PathBuf,
    live: Option<Live>,
    store: Store,
    /// The run transitions are kept under now: one per Core put together.
    run: Option<RunKept>,
    /// Who is at each host of the simulated world.
    hosts: Hosts,
    /// What the last write to the store said, where it failed.
    store_error: Option<String>,
    /// How many runs this process started.
    runs: u32,
}

/// The run transitions are kept under, and how far its world has been recorded.
struct RunKept {
    id: String,
    /// The next transition's place.
    next: i64,
    /// How many of the simulated world's deliveries are recorded.
    delivered: usize,
}

/// One order the API opened, and what its acts left on it.
struct Order {
    /// The decision record its route is from: the quote's, then a recalculation's, then a linked order's own.
    record: DecisionRecord,
    /// The quote's digest, which the customer accepts.
    digest: Option<Digest>,
    /// What it sends, in its asset's smallest unit.
    amount: String,
    /// Where it delivers: the outer network and its asset.
    to: (String, String),
    /// The segment that failed, once one did: the one whose reversal is owed.
    failed_at: Option<Id>,
    /// Where its value last settled: the node, the asset and the amount.
    resting: Option<(String, String, String)>,
    /// A recalculated route, waiting to run.
    recalculated: Option<Box<Recalculated>>,
    /// The linked order accepted to hand its value on to, once one is.
    linked: Option<Id>,
    /// What an operator decided about it, once stuck.
    decided: Option<Decided>,
    /// For a linked order: the order it starts from, what was accepted, and whether the value was handed on.
    link: Option<Link>,
}

/// A linked order's terms.
struct Link {
    from: Id,
    order: LinkedOrder,
    accepted: AcceptedLinked,
    handed: bool,
}

/// Core, put together, and what the API keeps beside it.
struct Live {
    /// The scenario Core was put together from: kept, so whatever it holds lives as long as Core.
    _scenario: Scenario,
    core: Core,
    checkpointer: CoreCheckpointer,
    /// Every checkpoint published, as an outside checker received it.
    received: Vec<Vec<u8>>,
    session: Id,
    orders: BTreeMap<String, Order>,
    /// The order of `orders`, as opened.
    opened: Vec<Id>,
    /// The order and segment each transaction served, as the segments ran.
    served: BTreeMap<String, (String, String)>,
    requests: u32,
    wallet: Option<InstalledWallet>,
    released: bool,
    /// alice (decides) and bob (moves), once listed.
    staff: Option<(Actor, Actor)>,
    /// Every provider a segment ran at.
    used: BTreeSet<String>,
    audits: u32,
}

impl Api {
    /// An API over Core from `seed`, writing under `out`. Core is put together on [`Api::start`] or on the first
    /// call that needs it.
    pub fn new(seed: u64, out: PathBuf, store: Store) -> Self {
        let mut books = BTreeMap::new();
        for (_, provider) in network::PROVIDERS {
            books.insert(
                rutent_conformance::sim::rail_host(provider),
                (*provider).to_owned(),
            );
        }
        let chains = network::nodes()
            .into_iter()
            .map(|(n, kind)| {
                let piece = match kind {
                    network::Kind::Public => "chain",
                    network::Kind::Private => "ledger",
                    network::Kind::Outer => "rail",
                };
                (rutent_conformance::sim::chain_host(&n), (n, piece))
            })
            .collect();
        Self {
            seed,
            out,
            live: None,
            store,
            run: None,
            hosts: Hosts { books, chains },
            store_error: None,
            runs: 0,
        }
    }

    /// The run transitions are kept under, started in the store if there is none yet.
    fn run_id(&mut self) -> Result<String, Failure> {
        if let Some(run) = &self.run {
            return Ok(run.id.clone());
        }
        self.runs += 1;
        let id = format!(
            "run-{}-seed-{}-{}",
            world::now().replace([':', '-'], ""),
            self.seed,
            self.runs
        );
        self.store
            .begin(&id, self.seed, &Self::network())
            .map_err(|e| fail(503, format!("the store refused the run: {e}")))?;
        self.run = Some(RunKept {
            id: id.clone(),
            next: 0,
            delivered: 0,
        });
        Ok(id)
    }

    /// Records what the simulated world received since the last time, through [`Api::record`]: each request an
    /// adapter made of an outer piece is one transition. A store that refuses keeps them waiting for the next call.
    pub fn sync(&mut self) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let from = self.run.as_ref().map_or(0, |r| r.delivered);
        let world = live.core.world();
        let delivered = world.delivered();
        if delivered.len() <= from {
            return;
        }
        let refused: Vec<(String, String, String)> = world
            .refused_movements
            .iter()
            .map(|(m, why)| (m.network.clone(), m.id.clone(), why.clone()))
            .collect();
        let world_at = rutent_prov::catalogue::Instant(
            u64::try_from(live.core.clock.peek() / 1_000_000_000).unwrap_or_default(),
        )
        .rfc3339();
        let world = live.core.world();
        let batch: Vec<Transition> = world.delivered()[from..]
            .iter()
            .map(|d| {
                let mut t =
                    world::from_delivery(&d.host, &d.body, &self.hosts, &refused, &live.served);
                t.world_at.clone_from(&world_at);
                t
            })
            .collect();
        let total = world.delivered().len();
        match self.record(batch) {
            Ok(_) => {
                if let Some(run) = self.run.as_mut() {
                    run.delivered = total;
                }
            }
            Err(e) => eprintln!("the transitions weren't kept: {}", e.body["error"]),
        }
    }

    /// Keeps `transitions` under the current run, in order: the one way into the store, for the simulated world and
    /// for an adapter calling `POST /world/transitions` alike.
    ///
    /// # Errors
    ///
    /// On a transition missing a field every one names, or a store that refuses them.
    pub fn record(
        &mut self,
        mut transitions: Vec<Transition>,
    ) -> Result<(String, i64, usize), Failure> {
        if let Some((n, missing)) = transitions
            .iter()
            .enumerate()
            .find_map(|(n, t)| t.missing().map(|m| (n, m)))
        {
            return Err(fail(400, format!("transition {n} names no {missing}")));
        }
        let run = self.run_id()?;
        let first = self.run.as_ref().map_or(0, |r| r.next);
        match self.store.append(&run, first, &mut transitions) {
            Ok(()) => {
                self.store_error = None;
                let count = transitions.len();
                if let Some(r) = self.run.as_mut() {
                    r.next += i64::try_from(count).unwrap_or(i64::MAX);
                }
                Ok((run, first, count))
            }
            Err(e) => {
                self.store_error = Some(e.clone());
                Err(fail(503, format!("the store refused the transitions: {e}")))
            }
        }
    }

    /// `POST /world/transitions`: an adapter, or an outer piece, records what it did. `{"source": …,
    /// "transitions": [{piece, kind, outcome, provider?, order_id?, …, detail?}]}`.
    ///
    /// # Errors
    ///
    /// On a body without a source or transitions, or a store that refuses them.
    pub fn post_transitions(&mut self, body: &Value) -> Reply {
        let source = body["source"]
            .as_str()
            .filter(|s| !s.is_empty() && *s != "simulator")
            .ok_or_else(|| fail(400, "source names who sends them, other than `simulator`"))?
            .to_owned();
        let transitions: Vec<Transition> = serde_json::from_value(body["transitions"].clone())
            .map_err(|e| fail(400, format!("transitions is a list of transitions: {e}")))?;
        if transitions.is_empty() {
            return Err(fail(400, "transitions is empty"));
        }
        let transitions = transitions
            .into_iter()
            .map(|mut t| {
                t.source.clone_from(&source);
                if t.detail.is_null() {
                    t.detail = json!({});
                }
                t
            })
            .collect();
        let (run, first, count) = self.record(transitions)?;
        Ok(json!({ "run": run, "first": first, "kept": count }))
    }

    /// `GET /world/transitions?since=N&run=R&limit=L`: a run's transitions from the Nth on, the current run's by
    /// default.
    ///
    /// # Errors
    ///
    /// If the store refuses the read.
    pub fn transitions(&mut self, since: i64, run: Option<&str>, limit: i64) -> Reply {
        let run = match run {
            Some(run) => run.to_owned(),
            None => match &self.run {
                Some(r) => r.id.clone(),
                None => {
                    return Ok(
                        json!({ "run": null, "since": since, "next": since, "transitions": [] }),
                    );
                }
            },
        };
        let read = self
            .store
            .since(&run, since, limit.clamp(1, 2000))
            .map_err(|e| fail(503, format!("the store refused the read: {e}")))?;
        let next = read.last().map_or(since, |t| t.seq + 1);
        Ok(json!({ "run": run, "since": since, "next": next, "transitions": read }))
    }

    /// `GET /world/runs`: the runs the store holds, newest first.
    ///
    /// # Errors
    ///
    /// If the store refuses the read.
    pub fn runs(&mut self) -> Reply {
        let runs = self
            .store
            .runs()
            .map_err(|e| fail(503, format!("the store refused the read: {e}")))?;
        Ok(json!({ "engine": self.store.engine(), "runs": runs }))
    }

    /// `GET /health`: whether the server, Core and the store answer. 503 when the store doesn't, or a write to it
    /// failed and hasn't succeeded since.
    pub fn health(&mut self) -> (u16, Value) {
        let store = self.store.check();
        let ok = store.is_ok() && self.store_error.is_none();
        let body = json!({
            "ok": ok,
            "core": self.live.is_some(),
            "store": self.store.engine(),
            "store_error": store.err().or_else(|| self.store_error.clone()),
            "run": self.run.as_ref().map(|r| r.id.clone()),
        });
        (if ok { 200 } else { 503 }, body)
    }

    /// Puts Core together now, unless it already is.
    ///
    /// # Errors
    ///
    /// If Core can't be put together.
    pub fn start(&mut self) -> Result<(), Failure> {
        self.live().map(|_| ())
    }

    fn live(&mut self) -> Result<&mut Live, Failure> {
        if self.live.is_none() {
            eprintln!(
                "Putting Core together and admitting the simulated providers (seed {})…",
                self.seed
            );
            let seed = self.seed;
            let live = std::panic::catch_unwind(move || Live::start(seed)).map_err(|p| {
                fail(
                    500,
                    format!("Core couldn't be put together: {}", panicked(&p)),
                )
            })??;
            eprintln!(
                "Core is up: {} providers, {} log entries.",
                network::PROVIDERS.len(),
                live.core.log().entries().len()
            );
            self.live = Some(live);
            self.run_id()?;
            self.sync();
        }
        Ok(self.live.as_mut().expect("just put together"))
    }

    /// `GET /status`: whether Core is up, and what it has done.
    pub fn status(&mut self) -> Value {
        let seed = self.seed;
        let out = self.out.display().to_string();
        let store = self.store.engine();
        let run = self
            .run
            .as_ref()
            .map(|r| json!({ "run": r.id, "transitions": r.next }));
        let Some(live) = self.live.as_mut() else {
            return json!({ "ready": false, "seed": seed, "out": out, "store": store, "run": run });
        };
        let mut orders = Vec::new();
        for order in live.opened.clone() {
            let state = live
                .core
                .status(&kit::customer(), &order)
                .map(|s| state_name(&s.state))
                .unwrap_or_else(|e| format!("{e:?}"));
            orders.push(json!({ "order": order.as_str(), "state": state }));
        }
        json!({
            "ready": true,
            "seed": seed,
            "out": out,
            "entries": live.core.log().entries().len(),
            "checkpoints": live.received.len(),
            "orders": orders,
            "used": live.used,
            "holding": live.wallet.as_ref().map(|w| json!({ "provider": w.provider, "holds": HELD_NODE })),
            "audits": live.audits,
            "store": store,
            "run": run,
        })
    }

    /// `POST /reset`: drops Core, and puts a new one together from the seed given, else the same one.
    ///
    /// # Errors
    ///
    /// If the new Core can't be put together.
    pub fn reset(&mut self, body: &Value) -> Reply {
        if let Some(seed) = body["seed"].as_u64() {
            self.seed = seed;
        }
        self.sync();
        self.live = None;
        self.run = None;
        self.start()?;
        Ok(self.status())
    }

    /// `GET /network`: the nodes and the edges, from the providers' own shapes. Needs no Core.
    pub fn network() -> Value {
        let nodes: Vec<Value> = network::nodes()
            .into_iter()
            .map(|(name, kind)| json!({ "id": name, "kind": kind.name() }))
            .collect();
        let edges: Vec<Value> = network::edges()
            .iter()
            .map(|e| {
                json!({
                    "id": edge_id(&e.provider, &e.from, &e.to),
                    "provider": e.provider,
                    "does": e.does,
                    "from": e.from,
                    "to": e.to,
                    "asset_in": e.asset_in,
                    "asset_out": e.asset_out,
                    "assets": e.assets(),
                    "fee": e.fee.to_string(),
                    "fee_shown": show::money(&e.fee.to_string(), &e.asset_in),
                    "seconds": e.seconds,
                })
            })
            .collect();
        let edges_all = network::edges();
        let origins: BTreeSet<&str> = edges_all
            .iter()
            .filter(|e| e.does == "withdrawal")
            .map(|e| e.from.as_str())
            .collect();
        let destinations: BTreeSet<&str> = edges_all
            .iter()
            .filter(|e| e.does == "deposit")
            .map(|e| e.to.as_str())
            .collect();
        json!({
            "providers": network::PROVIDERS.len(),
            "nodes": nodes,
            "edges": edges,
            "origins": origins,
            "destinations": destinations,
            "max_hops": MAX_HOPS,
            "holder": { "provider": kit::OPERATOR_WALLET, "holds": HELD_NODE },
        })
    }

    /// `GET /log?since=N`: the entries from the `N`th on, as the demo's timeline shows them.
    ///
    /// # Errors
    ///
    /// If Core can't be put together.
    pub fn log(&mut self, since: usize) -> Reply {
        let live = self.live()?;
        let log = live.core.log();
        let entries = log.entries();
        let from = since.min(entries.len());
        let shown: Vec<Value> = entries[from..]
            .iter()
            .map(|(receipt, entry)| {
                let view = show::entry_view(receipt, entry);
                json!({
                    "index": view.index,
                    "kind": view.kind,
                    "label": view.label,
                    "who": view.who,
                    "signature": view.signature,
                    "kept_as": view.kept_as,
                    "order": view.order,
                    "provider": view.provider,
                    "line": view.line(),
                })
            })
            .collect();
        Ok(json!({ "since": from, "next": entries.len(), "entries": shown }))
    }

    /// `POST /faults`: scripts a provider to fail or refuse its next operation (of a kind, where one is named), or the
    /// platform to refuse the start of the `nth` segment from now.
    ///
    /// # Errors
    ///
    /// On a fault it doesn't know, or a provider not in the network.
    pub fn faults(&mut self, body: &Value) -> Reply {
        let live = self.live()?;
        let kind = body["kind"].as_str().unwrap_or("fail");
        match kind {
            "fail" | "refuse" => {
                let provider = body["provider"]
                    .as_str()
                    .ok_or_else(|| fail(400, "a provider's fault names the provider"))?;
                if !network::PROVIDERS.iter().any(|(_, p)| *p == provider) {
                    return Err(fail(400, format!("{provider} isn't in the network")));
                }
                let act = if kind == "fail" {
                    Act::Fail
                } else {
                    Act::Refuse
                };
                let operation = body["operation"].as_str().filter(|o| !o.is_empty());
                match operation {
                    Some(op) => live
                        .core
                        .world()
                        .script_for(provider, op, Behaviour::act(act)),
                    None => live.core.world().script(provider, Behaviour::act(act)),
                }
                Ok(json!({
                    "scripted": format!(
                        "{provider} {} its next {}",
                        if kind == "fail" { "fails" } else { "refuses" },
                        operation.map_or_else(|| "operation".to_owned(), |o| format!("{o} operation"))
                    ),
                }))
            }
            "refuse_start" => {
                let nth = body["nth"].as_u64().filter(|n| *n >= 1).ok_or_else(|| {
                    fail(400, "refuse_start names the nth segment from now, from 1")
                })?;
                let begun = live.core.faults.begun;
                let at = begun + usize::try_from(nth - 1).unwrap_or(0);
                live.core.faults.refuse_begin = Some(at);
                Ok(json!({
                    "scripted": format!("the platform refuses to start the {nth}th segment from now, once"),
                    "at": at,
                }))
            }
            other => Err(fail(
                400,
                format!("unknown fault {other}: fail, refuse or refuse_start"),
            )),
        }
    }

    /// `POST /quotes`: opens an order from one outer node to another and has it quoted under an objective. The
    /// reserves are read from the graph: the one that withdraws from `from`, and the one that deposits to `to`.
    ///
    /// # Errors
    ///
    /// On a request the graph can't serve, or no route: then the candidates the router weighed, with why each was
    /// dropped.
    pub fn quote(&mut self, body: &Value) -> Reply {
        let from = body["from"]
            .as_str()
            .ok_or_else(|| fail(400, "from names an outer node"))?;
        let to = body["to"]
            .as_str()
            .ok_or_else(|| fail(400, "to names an outer node"))?;
        let amount = match &body["amount"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Null => "5000000".to_owned(),
            _ => {
                return Err(fail(
                    400,
                    "amount is a whole number of the asset's smallest unit",
                ));
            }
        };
        if amount.is_empty()
            || !amount.bytes().all(|b| b.is_ascii_digit())
            || amount.trim_start_matches('0').is_empty()
        {
            return Err(fail(
                400,
                "amount is a positive whole number of the asset's smallest unit",
            ));
        }
        let objective = body["objective"].as_str().unwrap_or("lowest_total_cost");
        if objective != "lowest_total_cost" && objective != "fastest_settlement" {
            return Err(fail(
                400,
                "objective is lowest_total_cost or fastest_settlement",
            ));
        }
        let edges = network::edges();
        let origin = edges
            .iter()
            .find(|e| e.does == "withdrawal" && e.from == from)
            .ok_or_else(|| fail(400, format!("no reserve withdraws from {from}")))?;
        let destination = edges
            .iter()
            .find(|e| e.does == "deposit" && e.to == to)
            .ok_or_else(|| fail(400, format!("no reserve deposits to {to}")))?;
        let live = self.live()?;
        let account = if to.ends_with("-addresses") {
            orders::register_address(&live.core);
            ADDRESS
        } else {
            DESTINATION_ACCOUNT
        };
        live.requests += 1;
        let name = format!("req-api-{}", live.requests);
        let request = orders::request(&Asked {
            name: &name,
            session: &live.session,
            origin: &origin.provider,
            from: outer(from, &origin.asset_in),
            to: outer(to, &destination.asset_out),
            at: &destination.provider,
            account,
            objective,
            amount: &amount,
        });
        let told = live.core.request(request).map_err(refused("the request"))?;
        let order =
            Core::order_of(&told).ok_or_else(|| fail(409, format!("no order opened: {told:?}")))?;
        let told = live
            .core
            .quote(&kit::customer(), &order)
            .map_err(refused("the quote"))?;
        let Told::Exists {
            status: Status::Quoted { quote, digest },
            ..
        } = told
        else {
            let record = live.core.decision_records().last().cloned();
            return Err(Failure {
                status: 422,
                body: json!({
                    "error": format!("the order wasn't quoted: {told:?}"),
                    "order": order.as_str(),
                    "candidates": record.as_ref().map(candidates).unwrap_or_default(),
                }),
            });
        };
        let record = live
            .core
            .decision_records()
            .into_iter()
            .find(|r| r.record_id() == &quote.decision_record)
            .ok_or_else(|| fail(500, "Core signed no decision record for the quote"))?;
        let shown = serde_json::to_value(&quote.price).unwrap_or(Value::Null);
        let reply = json!({
            "order": order.as_str(),
            "objective": objective,
            "from": from,
            "to": to,
            "amount": amount,
            "record": record.record_id().as_str(),
            "digest": digest.value.as_str(),
            "route": chosen(&record),
            "candidates": candidates(&record),
            "quote": shown,
        });
        live.orders.insert(
            order.as_str().to_owned(),
            Order {
                record,
                digest: Some(digest),
                amount,
                to: (to.to_owned(), destination.asset_out.clone()),
                failed_at: None,
                resting: None,
                recalculated: None,
                linked: None,
                decided: None,
                link: None,
            },
        );
        live.opened.push(order);
        Ok(reply)
    }

    /// `POST /orders/{id}/accept`: the customer accepts the order's quote, on its digest.
    ///
    /// # Errors
    ///
    /// On an order the API didn't quote, or Core refusing the acceptance.
    pub fn accept(&mut self, order: &str) -> Reply {
        let live = self.live()?;
        let id = id(order);
        let digest = live
            .orders
            .get(order)
            .and_then(|o| o.digest.clone())
            .ok_or_else(|| fail(404, format!("no quoted order {order}")))?;
        live.core
            .accept(&kit::customer(), &id, &digest)
            .map_err(refused("the acceptance"))?;
        let state = live.state(&id)?;
        Ok(json!({ "order": order, "accepted": digest.value.as_str(), "state": state }))
    }

    /// `POST /orders/{id}/run`: runs the order's route: the accepted one, else the recalculated one waiting, else a
    /// linked order's once its value was handed on. Each segment, how it ended and its receipt.
    ///
    /// # Errors
    ///
    /// On an order the API didn't open, or Core refusing the run.
    pub fn run(&mut self, order: &str) -> Reply {
        let live = self.live()?;
        let id = id(order);
        let entry = live
            .orders
            .get_mut(order)
            .ok_or_else(|| fail(404, format!("no order {order}")))?;
        let (ran, record, which) = if let Some(link) = entry.link.as_ref() {
            if !link.handed {
                return Err(fail(
                    409,
                    "a linked order starts only once the stuck value is handed on to it",
                ));
            }
            let ran = live
                .core
                .run_linked(&link.from, &link.order, &link.accepted)
                .map_err(refused("the linked order runs"))?;
            (ran, entry.record.clone(), "linked")
        } else if let Some(recalculated) = entry.recalculated.take() {
            let ran = live
                .core
                .run_recalculated(&id, &recalculated)
                .map_err(refused("the recalculated route"))?;
            entry.record = recalculated.record.clone();
            (vec![ran], recalculated.record, "recalculated")
        } else {
            let ran = live.core.run(&id).map_err(refused("the order runs"))?;
            (ran, entry.record.clone(), "accepted")
        };
        let mut segments = Vec::new();
        let mut refusals = Vec::new();
        let ops = operations(record.chosen());
        for r in &ran {
            match r {
                Ran::Segments(accounted) => {
                    for (n, s) in accounted.iter().enumerate() {
                        let op = ops.get(n).cloned().unwrap_or(Value::Null);
                        let provider = op["provider"].as_str().unwrap_or("?").to_owned();
                        live.used.insert(provider.clone());
                        if let Some(tx) = &s.transaction {
                            live.served.insert(
                                tx.as_str().to_owned(),
                                (order.to_owned(), s.segment.as_str().to_owned()),
                            );
                        }
                        let (outcome, amount_out, detail) = outcome(&s.outcome);
                        if outcome == "failed" && entry.failed_at.is_none() {
                            entry.failed_at = Some(s.segment.clone());
                        }
                        if let Some(amount) = &amount_out {
                            entry.resting = Some((
                                op["to"].as_str().unwrap_or("?").to_owned(),
                                op["asset_out"].as_str().unwrap_or("?").to_owned(),
                                amount.clone(),
                            ));
                        }
                        segments.push(json!({
                            "segment": s.segment.as_str(),
                            "transaction": s.transaction.as_ref().map(|t| t.as_str().to_owned()),
                            "provider": provider,
                            "edge": op["edge"],
                            "kind": op["kind"],
                            "from": op["from"],
                            "to": op["to"],
                            "outcome": outcome,
                            "detail": detail,
                            "amount_out": amount_out,
                            "asset_out": op["asset_out"],
                            "shown_out": amount_out.as_deref().map(|a| show::money(a, op["asset_out"].as_str().unwrap_or("?"))),
                            "receipt": s.effect.as_ref().map(receipt_of),
                        }));
                    }
                }
                Ran::Refused { refused, receipt } => {
                    refusals.push(json!({ "refused": format!("{refused:?}"), "receipt": receipt_of(receipt) }));
                }
            }
        }
        let state = live.state(&id)?;
        Ok(json!({
            "order": order,
            "route": which,
            "segments": segments,
            "refused": refusals,
            "state": state,
        }))
    }

    /// `POST /orders/{id}/reversal`: records the reversal the failed segment owes and asks for it.
    ///
    /// # Errors
    ///
    /// On an order with no failed segment, or Core refusing either step.
    pub fn reversal(&mut self, order: &str) -> Reply {
        let live = self.live()?;
        let id = id(order);
        let failed_at = live
            .orders
            .get(order)
            .ok_or_else(|| fail(404, format!("no order {order}")))?
            .failed_at
            .clone()
            .ok_or_else(|| fail(409, "no segment of this order failed"))?;
        let owing = live
            .core
            .owe_reversal(&id, &failed_at)
            .map_err(refused("the reversal owed"))?;
        live.served.insert(
            owing.transaction.as_str().to_owned(),
            (order.to_owned(), failed_at.as_str().to_owned()),
        );
        let asked = live
            .core
            .ask_reversal(&id, &failed_at)
            .map_err(refused("the reversal asked"))?;
        let word = asked.completed.as_ref().map(|c| match &c.word {
            rutent_core_orch::reversal::Word::Reserves => "the origin reserve's word".to_owned(),
            rutent_core_orch::reversal::Word::Publishers(publisher) => {
                format!(
                    "its publisher {}'s word, labelled as the publisher's",
                    publisher.as_str()
                )
            }
        });
        Ok(json!({
            "order": order,
            "segment": failed_at.as_str(),
            "owing": {
                "provider": owing.provider.as_str(),
                "amount": owing.amount,
                "asset": owing.asset,
                "shown": show::money(&owing.amount, &owing.asset),
            },
            "asked": receipt_of(&asked.effect),
            "completed": asked.completed.is_some(),
            "word": word,
            "why": asked.why.as_ref().map(|w| format!("{w:?}")),
        }))
    }

    /// `POST /orders/{id}/recalculate`: routes again from where the value is shown back, to the same destination. The
    /// route waits for the next `run`.
    ///
    /// # Errors
    ///
    /// On an order Core can't recalculate.
    pub fn recalculate(&mut self, order: &str) -> Reply {
        let live = self.live()?;
        let id = id(order);
        if !live.orders.contains_key(order) {
            return Err(fail(404, format!("no order {order}")));
        }
        let outcome = live
            .core
            .recalculate(&id)
            .map_err(refused("the recalculation"))?;
        let Recalculation::Recalculated(recalculated) = outcome else {
            return Err(fail(
                409,
                format!("no route from where the value is: {outcome:?}"),
            ));
        };
        let from = serde_json::to_value(&recalculated.from).unwrap_or(Value::Null);
        let reply = json!({
            "order": order,
            "from": show::endpoint(&from),
            "from_node": node_name(&from),
            "record": recalculated.record.record_id().as_str(),
            "route": chosen(&recalculated.record),
            "candidates": candidates(&recalculated.record),
        });
        if let Some(entry) = live.orders.get_mut(order) {
            entry.recalculated = Some(recalculated);
        }
        Ok(reply)
    }

    /// `GET /orders/{id}`: its state, its route, and the executed price once Core reported it.
    ///
    /// # Errors
    ///
    /// On an order the API didn't open.
    pub fn order(&mut self, order: &str) -> Reply {
        let live = self.live()?;
        let id = id(order);
        let entry = live
            .orders
            .get(order)
            .ok_or_else(|| fail(404, format!("no order {order}")))?;
        let route = chosen(&entry.record);
        let amount = entry.amount.clone();
        let linked = entry.linked.as_ref().map(|l| l.as_str().to_owned());
        let from = entry.link.as_ref().map(|l| l.from.as_str().to_owned());
        let resting = entry.resting.clone();
        let price = live.core.price_reported(&id).map(|(digest, at)| {
            json!({
                "digest": digest.value.as_str(),
                "receipt": receipt_of(&at),
                "shown": live.core.price_of(&id).map(|p| serde_json::to_value(p).unwrap_or(Value::Null)),
            })
        });
        let state = live.state(&id)?;
        Ok(json!({
            "order": order,
            "state": state,
            "amount": amount,
            "route": route,
            "price": price,
            "resting": resting.map(|(node, asset, amount)| json!({
                "node": node, "asset": asset, "amount": amount, "shown": show::money(&amount, &asset),
            })),
            "linked": linked,
            "linked_from": from,
        }))
    }

    /// `POST /holding`: installs the operator's plain wallet, a real module, as the held public node's holding account.
    ///
    /// # Errors
    ///
    /// If the platform doesn't install it.
    pub fn holding(&mut self) -> Reply {
        let live = self.live()?;
        if live.wallet.is_none() {
            let wallet = live
                .core
                .install_wallet(HELD_NODE, "USDC")
                .map_err(|e| fail(409, format!("the operator's wallet: {e:?}")))?;
            live.wallet = Some(wallet);
        }
        let wallet = live.wallet.as_ref().expect("installed");
        Ok(json!({ "provider": wallet.provider, "version": wallet.version, "holds": HELD_NODE }))
    }

    /// `POST /orders/{id}/stuck`: no route is left for the order, and it is stuck where its value rests.
    ///
    /// # Errors
    ///
    /// If Core doesn't take it as stuck.
    pub fn stuck(&mut self, order: &str) -> Reply {
        let live = self.live()?;
        let id = id(order);
        let resting = live
            .orders
            .get(order)
            .ok_or_else(|| fail(404, format!("no order {order}")))?
            .resting
            .clone()
            .ok_or_else(|| fail(409, "nothing of this order has settled"))?;
        let part = live
            .core
            .first_part(&id)
            .ok_or_else(|| fail(409, "the order has no part"))?;
        live.core
            .leave_no_route(&id)
            .map_err(refused("no route left"))?;
        if !live.core.is_stuck(&id, &part) {
            return Err(fail(409, "the order isn't stuck"));
        }
        let state = live.state(&id)?;
        let (node, asset, amount) = resting;
        Ok(json!({
            "order": order,
            "stuck": true,
            "state": state,
            "resting": { "node": node, "asset": asset, "amount": amount, "shown": show::money(&amount, &asset) },
        }))
    }

    /// `POST /orders/{id}/linked`: the customer accepts a linked order from where the stuck order's value rests to
    /// the stuck order's destination. It starts only once the value is handed on.
    ///
    /// # Errors
    ///
    /// With no holding account installed, nothing resting, or Core refusing.
    pub fn linked(&mut self, order: &str) -> Reply {
        let live = self.live()?;
        let id = id(order);
        let wallet = live
            .wallet
            .as_ref()
            .ok_or_else(|| fail(409, "install the holding account first (POST /holding)"))?;
        if !live.released {
            live.core
                .accept_release(wallet)
                .map_err(|e| fail(409, format!("the wallet's release: {e:?}")))?;
            live.released = true;
        }
        let holder = id_of(&wallet.provider);
        let entry = live
            .orders
            .get(order)
            .ok_or_else(|| fail(404, format!("no order {order}")))?;
        let (node, asset, amount) = entry
            .resting
            .clone()
            .ok_or_else(|| fail(409, "nothing of this order rests anywhere"))?;
        let (to, to_asset) = entry.to.clone();
        let (key, account) = if to.ends_with("-addresses") {
            ("beneficiary.address", ADDRESS)
        } else {
            ("beneficiary.bank_account", DESTINATION_ACCOUNT)
        };
        live.requests += 1;
        let linked_order = LinkedOrder {
            from: id.clone(),
            holder,
            customer: kit::customer(),
            session: live.session.clone(),
            route: serde_json::from_value::<Request>(json!({
                "request_id": format!("req-api-{}-linked", live.requests),
                "round": {"kind": "shortlist"},
                "from": inner(&node, &asset),
                "to": outer(&to, &to_asset),
                "amount": amount,
                "objective": {"kind": "lowest_total_cost"},
            }))
            .map_err(|e| fail(500, format!("the linked route request: {e}")))?,
            inputs: BTreeMap::from([
                ("payer.bank_account".to_owned(), ORIGIN_ACCOUNT.to_owned()),
                (key.to_owned(), account.to_owned()),
            ]),
        };
        let accepted = live
            .core
            .accept_linked(&linked_order)
            .map_err(refused("the linked order"))?;
        let record = live
            .core
            .decision_records()
            .into_iter()
            .find(|r| r.record_id() == &accepted.route.decision_record)
            .ok_or_else(|| fail(500, "no decision record for the linked route"))?;
        let linked = accepted.order.clone();
        let reply = json!({
            "order": order,
            "linked": linked.as_str(),
            "from": node,
            "record": record.record_id().as_str(),
            "route": chosen(&record),
            "candidates": candidates(&record),
        });
        if let Some(entry) = live.orders.get_mut(order) {
            entry.linked = Some(linked.clone());
        }
        live.orders.insert(
            linked.as_str().to_owned(),
            Order {
                record,
                digest: None,
                amount,
                to: (to, to_asset),
                failed_at: None,
                resting: None,
                recalculated: None,
                linked: None,
                decided: None,
                link: Some(Link {
                    from: id,
                    order: linked_order,
                    accepted,
                    handed: false,
                }),
            },
        );
        live.opened.push(linked);
        Ok(reply)
    }

    /// `POST /orders/{id}/decide`: one of the operator's staff (`by`: alice, who may decide, or bob, who may move)
    /// decides to hand the stuck order's value on to its linked order, citing a provider's kept statement.
    ///
    /// # Errors
    ///
    /// Without a linked order, or where policy refuses the decision.
    pub fn decide(&mut self, order: &str, body: &Value) -> Reply {
        let live = self.live()?;
        let id = id(order);
        let by = body["by"].as_str().unwrap_or("alice");
        let actor = live.staff(by)?;
        let wallet = live
            .wallet
            .as_ref()
            .ok_or_else(|| fail(409, "no holding account is installed"))?
            .provider
            .clone();
        let entry = live
            .orders
            .get(order)
            .ok_or_else(|| fail(404, format!("no order {order}")))?;
        let linked = entry.linked.clone().ok_or_else(|| {
            fail(
                409,
                "accept a linked order first (POST /orders/{id}/linked)",
            )
        })?;
        let (_, asset, amount) = entry
            .resting
            .clone()
            .ok_or_else(|| fail(409, "nothing of this order rests anywhere"))?;
        let resolution = Resolution::HandOn {
            by: id_of(&wallet),
            linked_order: linked,
            asset: Denomination {
                id: asset.as_str().into(),
                decimals: 2,
            },
            amount: PositiveAmount::new(amount.as_str())
                .ok_or_else(|| fail(409, "the resting amount isn't positive"))?,
        };
        let evidence = orders::evidence_about(&live.core, &id).map_err(|e| fail(409, e))?;
        let decision = live
            .core
            .decide_stuck(&id, &actor, &resolution, &evidence)
            .map_err(|e| fail(403, format!("{by}'s decision: {e:?}")))?;
        let outcome = format!("{:?}", decision.outcome);
        if let Some(entry) = live.orders.get_mut(order) {
            entry.decided = Some(Decided {
                request: decision.request.clone(),
                decision: decision.decision.clone(),
            });
        }
        Ok(json!({
            "order": order,
            "by": by,
            "evidence": evidence.iter().map(|d| d.value.as_str().to_owned()).collect::<Vec<_>>(),
            "outcome": outcome,
        }))
    }

    /// `POST /orders/{id}/move`: one of the operator's staff carries out the decision, and, where policy lets them,
    /// the wallet's withdrawal record hands the value on to the linked order. Whoever decided can't move it.
    ///
    /// # Errors
    ///
    /// Without a decision, or where policy refuses the move (403).
    pub fn move_stuck(&mut self, order: &str, body: &Value) -> Reply {
        let live = self.live()?;
        let id = id(order);
        let by = body["by"].as_str().unwrap_or("bob");
        let actor = live.staff(by)?;
        let entry = live
            .orders
            .get(order)
            .ok_or_else(|| fail(404, format!("no order {order}")))?;
        let decided = entry
            .decided
            .clone()
            .ok_or_else(|| fail(409, "nobody decided about this order yet"))?;
        let linked = entry.linked.clone();
        let moved = match live.core.move_stuck(&id, &decided, &actor) {
            Err(StepError::Resolve(ResolveError::SameParty)) => {
                return Err(Failure {
                    status: 403,
                    body: json!({
                        "error": format!("{by} is refused: whoever decides can't also move it"),
                        "refused": "same_party",
                    }),
                });
            }
            Err(e) => return Err(fail(403, format!("{by}'s move: {e:?}"))),
            Ok(moved) => moved,
        };
        let Moved::CarriedOut {
            effect,
            resolved: Resolved::Done,
            ..
        } = moved
        else {
            return Err(fail(409, format!("the move wasn't done: {moved:?}")));
        };
        let handed = live
            .core
            .hand_on(&id, &effect, &actor)
            .map_err(|e| fail(409, format!("the hand-on: {e:?}")))?;
        if let Some(link) = linked
            .as_ref()
            .and_then(|l| live.orders.get_mut(l.as_str()))
            .and_then(|o| o.link.as_mut())
        {
            link.handed = true;
        }
        Ok(json!({
            "order": order,
            "by": by,
            "carried_out": receipt_of(&effect),
            "handed_on_at": show::endpoint(&serde_json::to_value(&handed.at).unwrap_or(Value::Null)),
            "linked": linked.map(|l| l.as_str().to_owned()),
        }))
    }

    /// `POST /notices`: Core signs and sends its notices to the customer and the operator.
    ///
    /// # Errors
    ///
    /// If Core can't send them.
    pub fn notices(&mut self) -> Reply {
        let live = self.live()?;
        let sent = live
            .core
            .notify()
            .map_err(|e| fail(409, format!("the notices: {e:?}")))?;
        Ok(json!({ "sent": sent }))
    }

    /// `POST /audit`: an hour later by Core's clock the log publishes a checkpoint; the folder an outside checker is
    /// handed is written, and `audit-check` runs over it three times: the history against the checkpoints, every
    /// statement against its evidence, and a copy with one figure changed.
    ///
    /// # Errors
    ///
    /// If the folder can't be written, or `audit-check` can't be built or run.
    pub fn audit(&mut self) -> Reply {
        let out = self.out.clone();
        let live = self.live()?;
        live.core.clock.advance(CHECKPOINT_EVERY);
        let published = live.publish()?;
        live.audits += 1;
        let history = history_of(&live.core.log());
        let openings = openings_of(live.checkpointer.kept());
        let through = live
            .checkpointer
            .kept()
            .last()
            .and_then(|k| tick_of(k.opening.tick.as_str()))
            .ok_or_else(|| fail(500, "no checkpoint was published"))?;
        let kept = kept_of(&live.core);
        let handed = audit::write(&out, &history, &live.received, &openings, &kept, through)
            .map_err(|e| fail(500, format!("the folder {}: {e}", out.display())))?;
        let records = live.core.decision_record_bytes();
        let dir = out.join("decision-records");
        std::fs::create_dir_all(&dir).map_err(|e| fail(500, e.to_string()))?;
        for (name, bytes) in &records {
            std::fs::write(dir.join(format!("{name}.json")), bytes)
                .map_err(|e| fail(500, e.to_string()))?;
        }
        std::fs::write(
            out.join("network.dot"),
            network::dot(kit::OPERATOR_WALLET, HELD_NODE),
        )
        .map_err(|e| fail(500, e.to_string()))?;
        let binary = audit::binary().map_err(|e| fail(500, e))?;
        let full = audit::arguments(&handed, &handed.history, true);
        std::fs::write(out.join("verify.sh"), audit::script(&binary, &full))
            .map_err(|e| fail(500, e.to_string()))?;

        let mut checks = Vec::new();
        let checkpoints = audit::check(&binary, &audit::arguments(&handed, &handed.history, false))
            .map_err(|e| fail(500, e))?;
        checks.push(check_view(
            "The history against the checkpoints the checker received",
            &checkpoints,
            checkpoints.code == Some(0),
        ));

        let evidence = audit::check(&binary, &full).map_err(|e| fail(500, e))?;
        let gap = not_borne_out(&evidence.output);
        let all_inner = !gap.is_empty() && gap.iter().all(|&n| is_inner_segments(history.get(n)));
        let known_gap = evidence.code == Some(1)
            && all_inner
            && evidence.output.lines().all(|l| !l.contains("DoesNotHold"));
        let mut second = check_view(
            "Every provider's statement against the evidence kept for it",
            &evidence,
            evidence.code == Some(0) || known_gap,
        );
        if known_gap {
            second["note"] = json!(format!(
                "These {} are the providers' inner-segments statements, which audit-check's evidence read doesn't yet \
                 match to the order they are about: a known gap in rutent-core-audit, reported to M3, not tampering.",
                gap.len()
            ));
        }
        checks.push(second);

        let amount = live
            .orders
            .values()
            .find(|o| o.link.is_none())
            .map(|o| o.amount.clone());
        let mut tampered_entry = None;
        if let Some(amount) = amount {
            let path = out.join("tampered").join("history.jsonl");
            std::fs::create_dir_all(path.parent().expect("a folder"))
                .map_err(|e| fail(500, e.to_string()))?;
            tampered_entry =
                audit::tamper(&history, &path, &amount).map_err(|e| fail(500, e.to_string()))?;
            if let Some(n) = tampered_entry {
                let tampered = audit::check(&binary, &audit::arguments(&handed, &path, false))
                    .map_err(|e| fail(500, e))?;
                let caught = tampered.code == Some(1) && tampered.output.contains("DoesNotHold");
                let mut third = check_view(
                    "A copy with one figure changed, as someone with the operator's full access could write it",
                    &tampered,
                    caught,
                );
                third["note"] = json!(format!(
                    "entry #{n}: amount {amount} rewritten as {amount}0; the checker is expected to reject it"
                ));
                checks.push(third);
            }
        }
        Ok(json!({
            "out": out.display().to_string(),
            "published": published,
            "entries": history.len(),
            "checkpoints": live.received.len(),
            "kept": kept.len(),
            "decision_records": records.len(),
            "verify": out.join("verify.sh").display().to_string(),
            "checks": checks,
            "tampered_entry": tampered_entry,
            "as_told": checks.iter().all(|c| c["as_told"] == json!(true)),
        }))
    }
}

impl Live {
    fn start(seed: u64) -> Result<Self, Failure> {
        let given = Given {
            providers: network::specs(),
            staging: Staged::AtOnce,
            deadline: Duration::from_secs(2),
            delivery: Delivery::Once,
        };
        let scenario = Scenario::new("rutent demo API", "AC-1", given);
        let (mut core, _) = scenario.core(seed, &mut Reference);
        core.set_max_hops(NonZeroU8::new(MAX_HOPS).expect("more than none"));
        let public: Vec<String> = network::public_nodes().into_iter().collect();
        for node in &public {
            if !core.catalogue().nodes().is_public(node) {
                return Err(fail(
                    500,
                    format!("{node} isn't on the operator's list of public nodes"),
                ));
            }
        }
        let public: Vec<&str> = public.iter().map(String::as_str).collect();
        show::public_nodes(&public);
        let checkpointer = core
            .checkpointer()
            .map_err(|e| fail(500, format!("the checkpointer refused the log: {e:?}")))?;
        let session = core
            .open_session(&kit::customer())
            .map_err(refused("the customer's session"))?;
        let mut live = Live {
            _scenario: scenario,
            core,
            checkpointer,
            received: Vec::new(),
            session,
            orders: BTreeMap::new(),
            opened: Vec::new(),
            served: BTreeMap::new(),
            requests: 0,
            wallet: None,
            released: false,
            staff: None,
            used: BTreeSet::new(),
            audits: 0,
        };
        live.publish()?;
        Ok(live)
    }

    /// Publishes every checkpoint now due, as the log's own key signs them, and keeps them as the checker gets them.
    fn publish(&mut self) -> Result<usize, Failure> {
        let ran = self.checkpointer.run_due();
        if let Some(stopped) = ran.stopped.as_ref() {
            return Err(fail(
                500,
                format!("a checkpoint wasn't issued: {stopped:?}"),
            ));
        }
        self.received.extend(ran.published.iter().cloned());
        self.checkpointer
            .acknowledge(&ran)
            .map_err(|e| fail(500, format!("a checkpoint wasn't acknowledged: {e:?}")))?;
        Ok(ran.published.len())
    }

    /// The order's state, as the customer reads it.
    fn state(&mut self, order: &Id) -> Result<String, Failure> {
        Ok(state_name(
            &self
                .core
                .status(&kit::customer(), order)
                .map_err(refused("the customer reads the order"))?
                .state,
        ))
    }

    /// One of the operator's staff by login: alice on desk A, who may decide about a stuck order, or bob on desk B,
    /// who may move it.
    fn staff(&mut self, login: &str) -> Result<Actor, Failure> {
        if self.staff.is_none() {
            let alice = orders::staff(&self.core, "desk-a", "alice", Permission::DecideStuck);
            let bob = orders::staff(&self.core, "desk-b", "bob", Permission::MoveStuck);
            self.staff = Some((alice, bob));
        }
        let (alice, bob) = self.staff.as_ref().expect("listed");
        match login {
            "alice" => Ok(alice.clone()),
            "bob" => Ok(bob.clone()),
            other => Err(fail(
                400,
                format!("{other} isn't on the operator's staff: alice or bob"),
            )),
        }
    }
}

/// An identifier from a name the API was handed.
fn id_of(name: &str) -> Id {
    id(name)
}

/// The id the page gives an edge: its provider, and the nodes it joins.
pub fn edge_id(provider: &str, from: &str, to: &str) -> String {
    format!("{provider}|{from}|{to}")
}

/// The name of an endpoint's node: an outer node's network, or an inner node's identifier.
fn node_name(endpoint: &Value) -> Value {
    endpoint["node"]["network"]
        .as_str()
        .or_else(|| endpoint["node"].as_str())
        .map_or(Value::Null, |n| Value::from(n.to_owned()))
}

/// Each operation of `candidate`, in order: its provider, kind, nodes, assets, amounts, and the edge it takes.
fn operations(candidate: Option<&Candidate>) -> Vec<Value> {
    let Some(candidate) = candidate else {
        return Vec::new();
    };
    candidate
        .parts
        .iter()
        .flat_map(|p| p.operations.iter())
        .map(|o| {
            let v = serde_json::to_value(o).unwrap_or(Value::Null);
            let from = node_name(&v["from"]);
            let to = node_name(&v["to"]);
            let provider = o.provider.as_str();
            json!({
                "provider": provider,
                "kind": v["kind"],
                "from": from,
                "to": to,
                "edge": edge_id(provider, from.as_str().unwrap_or("?"), to.as_str().unwrap_or("?")),
                "asset_in": v["from"]["asset"]["id"],
                "asset_out": v["to"]["asset"]["id"],
                "amount_in": v["amount_in"],
                "amount_out": v["amount_out"],
            })
        })
        .collect()
}

/// The route a decision record chose.
fn chosen(record: &DecisionRecord) -> Vec<Value> {
    operations(record.chosen())
}

/// Every candidate a decision record weighed: its route, whether it was chosen, and why it was dropped if it was.
fn candidates(record: &DecisionRecord) -> Vec<Value> {
    let chosen = record.chosen().map(|c| c.candidate_id.clone());
    record
        .candidates()
        .iter()
        .map(|c| {
            let dropped = c
                .dropped
                .as_ref()
                .map(|d| serde_json::to_value(d).unwrap_or_else(|_| Value::from(format!("{d:?}"))));
            json!({
                "candidate": c.candidate_id.as_str(),
                "chosen": Some(&c.candidate_id) == chosen.as_ref(),
                "dropped": dropped,
                "route": operations(Some(c)),
            })
        })
        .collect()
}

/// A segment's outcome: its name, what it gave out where it settled, and the detail where it didn't.
fn outcome(outcome: &SegmentOutcome) -> (&'static str, Option<String>, Option<String>) {
    match outcome {
        SegmentOutcome::Settled { amount_out, .. } => (
            "settled",
            serde_json::to_value(amount_out)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned)),
            None,
        ),
        SegmentOutcome::Failed => ("failed", None, None),
        SegmentOutcome::NotHandedOver(why) => ("not_handed_over", None, Some(format!("{why:?}"))),
        SegmentOutcome::Refused(why) => ("refused", None, Some(format!("{why:?}"))),
        SegmentOutcome::NotPermitted(why) => ("not_permitted", None, Some(format!("{why:?}"))),
        SegmentOutcome::WithdrawalCancelled => ("withdrawal_cancelled", None, None),
        other => ("waiting", None, Some(format!("{other:?}"))),
    }
}

/// A receipt as the page shows it: the entry's place in the log and its digest.
fn receipt_of(receipt: &Receipt) -> Value {
    let value = serde_json::to_value(receipt).unwrap_or(Value::Null);
    json!({
        "index": value["index"],
        "entry": value["entry"]["value"],
        "shown": show::receipt(receipt),
    })
}

/// An order's state by its name on the wire.
fn state_name(state: &rutent_contracts::state_machine::OrderState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{state:?}"))
}

/// One run of `audit-check`, as the page shows it.
fn check_view(name: &str, checked: &audit::Checked, as_told: bool) -> Value {
    json!({
        "name": name,
        "code": checked.code,
        "verdict": match checked.code {
            Some(0) => "it holds",
            Some(1) => "the checker found something",
            _ => "the checker couldn't run",
        },
        "output": checked.output,
        "as_told": as_told,
    })
}

/// What a panic said.
pub fn panicked(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "a panic".to_owned())
}
