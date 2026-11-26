//! `rutent-demo`: a scripted, narrated run of rutent on a network of simulated providers, for an operator to show a
//! customer how the system works. Nothing in it holds or moves real value: every provider is one of
//! `rutent-scenarios`'s simulated providers, admitted to the real module platform and signing for real, and Core is
//! the scenario assembly's, the same one `M3`'s scenarios run.
//!
//! The network (act 0) is eighteen providers making a graph of 23 edges between 10 nodes: three publishers' private
//! ledgers with their banks in COP, MXN and USD, two public chains in USDC joined by three bridges, and the
//! customer's addresses on each chain (`network`). The router searches it for routes of up to six segments. The story,
//! in three acts and an epilogue:
//!
//! 1. Pesos at a Colombian bank to dollars at a US bank, across two private ledgers and a chain, converting twice,
//!    with the first exchange failing mid-route: the reversal owed, asked and shown back, the route recalculated from
//!    where the value is, the order completed with no new yes from the customer, and its executed price reported.
//! 2. One request from one chain to the other, quoted under both objectives: the cheapest and the fastest route take
//!    different bridges, and the fastest runs.
//! 3. Mexican pesos to USDC at an address, stuck at a public chain where the operator's wallet holds the value: one
//!    operator desk decides to hand it on to a linked order, another moves it, and the linked order runs.
//! 4. The log, written to a folder with its checkpoints and its evidence, checked from outside by `audit-check`, then
//!    a copy with one figure changed, which the checker rejects.
//!
//! The failure comes first because a second order through the same reserve doesn't get the private ledger's evidence
//! the reversal needs (reported to `M3`).
//!
//! ```text
//! cargo run -p rutent-scenarios --bin rutent-demo -- [--out DIR] [--seed N] [--step]
//! ```
//!
//! `--out` is the folder the log and the evidence are written to (`<target>/rutent-demo` by default), `--seed` the run's
//! seed (every identifier Core draws follows from it), and `--step` waits for Enter between acts, for a live
//! presentation. `conformance/demo/README.md` is the presenter's guide.

// Each binary uses part of what the two share.
#[allow(dead_code)]
#[path = "../demo-common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::io::BufRead as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{Actor, Decided, Resolution};
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Id, PositiveAmount, Request};
use rutent_contracts::state_machine::OrderState;
use rutent_core_audit::checkpoint::tick_of;
use rutent_core_authz::roles::Permission;
use rutent_core_orch::intake::{OrderRequest, Status, Told};
use rutent_core_orch::recalc::Recalculation;
use rutent_core_orch::segments::{Accounted, Ran, SegmentOutcome};
use rutent_core_orch::stuck::{Moved, Resolved};
use rutent_scenarios::assembly::checking::{
    CHECKPOINT_EVERY, CoreCheckpointer, history_of, openings_of,
};
use rutent_scenarios::assembly::linked::LinkedOrder;
use rutent_scenarios::assembly::machine::{Delivery, Reference};
use rutent_scenarios::assembly::{Core, DESTINATION_ACCOUNT, ORIGIN_ACCOUNT, Staged, StepError};
use rutent_scenarios::harness::{Given, Run, Scenario};
use rutent_scenarios::kit::{self, id};
use serde_json::{Value, json};

use common::orders::{
    ADDRESS, Asked, HELD_NODE, MAX_HOPS, candidates, inner, is_inner_segments, not_borne_out,
    outer, verdict,
};
use common::{audit, network, orders, relaunch, show};
use show::{Transcript, money, receipt};

/// The reserve of pesos at the Colombian bank, in sim-andina's ledger.
const PESOS: &str = "sim-andina-cop";
/// The exchange from COP to USDC in that ledger that act 1 makes fail.
const FAILING_EXCHANGE: &str = "sim-andina-exchange";
/// The reserve of dollars at the US bank, in sim-liberty's ledger.
const DOLLARS: &str = "sim-liberty-usd";
/// The reserve of Mexican pesos at the Mexican bank, in sim-azteca's ledger.
const MEXICAN_PESOS: &str = "sim-azteca-mxn-desk";
/// The reserves between each chain and its addresses.
const POLYGON_WALLETS: &str = "sim-polygon-wallets";
const BASE_WALLETS: &str = "sim-base-wallets";
/// What each order sends, in its asset's smallest unit: 50,000.00. The simulated exchanges and ramps serve no less
/// than 10,000.00 (their tuples' band), and every step's fee must leave that much for the next.
const AMOUNT: &str = "5000000";

/// What the command line set.
struct Options {
    out: PathBuf,
    seed: u64,
    step: bool,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        out: relaunch::default_out("rutent-demo"),
        seed: 1,
        step: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => options.out = args.next().ok_or("--out names a folder")?.into(),
            "--seed" => {
                options.seed = args
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or("--seed is a whole number")?;
            }
            "--step" => options.step = true,
            "-h" | "--help" => {
                return Err(
                    "usage: rutent-demo [--out DIR] [--seed N] [--step]\n  see conformance/demo/README.md"
                        .to_owned(),
                );
            }
            other => return Err(format!("unknown argument {other}; see --help")),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    relaunch::from_deps("rutent_demo");
    let options = match options() {
        Ok(options) => options,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
    };
    let started = Instant::now();
    let mut demo = match Demo::new(&options) {
        Ok(demo) => demo,
        Err(why) => {
            eprintln!("the demo couldn't start: {why}");
            return ExitCode::FAILURE;
        }
    };
    let story = demo
        .network()
        .and_then(|()| demo.pause())
        .and_then(|()| demo.provider_fails_mid_route())
        .and_then(|()| demo.pause())
        .and_then(|()| demo.two_objectives())
        .and_then(|()| demo.pause())
        .and_then(|()| demo.stuck_order_handed_on())
        .and_then(|()| demo.pause())
        .and_then(|()| demo.checked_from_outside(started.elapsed()));
    match story {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("\nthe outside check didn't come out as the story says: see above");
            ExitCode::FAILURE
        }
        Err(why) => {
            eprintln!("\nthe demo stopped: {why}");
            ExitCode::FAILURE
        }
    }
}

/// An order quoted: its id, the decision record its route is from, the quote's digest, and what the customer would
/// receive, in the destination asset's smallest unit.
struct Quoted {
    order: Id,
    record: rutent_contracts::router_api::DecisionRecord,
    digest: rutent_contracts::module_interface::Digest,
    shown_out: String,
}

/// The demo as it runs: Core, the checkpointer over its log, and what it has printed and done.
struct Demo {
    scenario: Scenario,
    seed: u64,
    out: PathBuf,
    step: bool,
    core: Option<Core>,
    session: Id,
    checkpointer: Option<CoreCheckpointer>,
    /// Every checkpoint published, as an outside checker received it.
    received: Vec<Vec<u8>>,
    /// The orders the story opened.
    orders: Vec<Id>,
    t: Transcript,
}

type Step<T = ()> = Result<T, String>;

fn failed(what: &str) -> impl Fn(StepError) -> String + '_ {
    move |e| format!("{what}: {e:?}")
}

impl Demo {
    fn new(options: &Options) -> Step<Self> {
        let given = Given {
            providers: network::specs(),
            staging: Staged::AtOnce,
            deadline: Duration::from_secs(2),
            delivery: Delivery::Once,
        };
        let scenario = Scenario::new("rutent demo", "AC-1", given);
        println!(
            "Putting Core together and admitting the simulated providers (their modules compile on first use)…"
        );
        let (mut core, _) = scenario.core(options.seed, &mut Reference);
        core.set_max_hops(std::num::NonZeroU8::new(MAX_HOPS).expect("more than none"));
        let checkpointer = core
            .checkpointer()
            .map_err(|e| format!("the checkpointer refused the log: {e:?}"))?;
        Ok(Demo {
            scenario,
            seed: options.seed,
            out: options.out.clone(),
            step: options.step,
            core: Some(core),
            session: id("unopened"),
            checkpointer: Some(checkpointer),
            received: Vec::new(),
            orders: Vec::new(),
            t: Transcript::default(),
        })
    }

    fn core(&mut self) -> &mut Core {
        self.core.as_mut().expect("Core runs until the epilogue")
    }

    fn pause(&mut self) -> Step {
        if self.step {
            eprint!("\n  [Enter for the next act] ");
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
        }
        Ok(())
    }

    /// Prints the entries the log took since the last time.
    fn timeline(&mut self) {
        let entries = self
            .core
            .as_ref()
            .expect("Core runs")
            .log()
            .entries()
            .to_vec();
        self.t.entries_since(&entries);
    }

    /// Publishes every checkpoint now due, as the log's own key signs them, and keeps them as the checker gets them.
    fn publish(&mut self) -> Step<usize> {
        let checkpointer = self.checkpointer.as_mut().expect("the checkpointer runs");
        let ran = checkpointer.run_due();
        if let Some(stopped) = ran.stopped.as_ref() {
            return Err(format!("a checkpoint wasn't issued: {stopped:?}"));
        }
        self.received.extend(ran.published.iter().cloned());
        checkpointer
            .acknowledge(&ran)
            .map_err(|e| format!("a checkpoint wasn't acknowledged: {e:?}"))?;
        Ok(ran.published.len())
    }

    // Act 0 -------------------------------------------------------------------------------------------------------

    fn network(&mut self) -> Step {
        self.t.act(
            "ACT 0",
            "The network: synthetic providers on a private network, holding no real value",
        );
        self.t.say("Every provider here is simulated. Each runs as a real Wasm module on rutent's module platform,");
        self.t.say("signs its statements with its own key, and was admitted by a decided catalogue change in the log.");
        self.t.gap();
        self.t.detail(
            "Nodes: OUTER = a bank rail or chain outside rutent, where money starts or ends;",
        );
        self.t
            .detail("       PRIVATE = one publisher's own ledger, seen only by its providers;");
        self.t.detail("       PUBLIC = a shared ledger on the operator's list, where anyone can see what moved.");
        self.t.gap();
        let kinds = network::nodes();
        let kind_of = |n: &str| {
            kinds
                .iter()
                .find(|(name, _)| name == n)
                .map_or("?", |(_, k)| k.name())
        };
        self.t.detail(format!(
            "{} providers, {} nodes, {} edges. Each line is one edge: a provider moving value from one node to another.",
            network::PROVIDERS.len(),
            kinds.len(),
            network::edges().len()
        ));
        self.t.gap();
        for e in network::edges() {
            let from = format!("{} ({})", e.from, kind_of(&e.from));
            let to = if e.from == e.to {
                "same node".to_owned()
            } else {
                format!("{} ({})", e.to, kind_of(&e.to))
            };
            let terms = match (e.fee, e.seconds) {
                (0, Some(s)) => format!("no fee, {s} s"),
                (f, Some(s)) => format!("fee {}, {s} s", money(&f.to_string(), &e.asset_in)),
                (0, None) => "no fee".to_owned(),
                (f, None) => format!("fee {}", money(&f.to_string(), &e.asset_in)),
            };
            self.t.detail(format!(
                "{:<26} {:<10} {:<36} → {:<32} {:<12} {terms}",
                e.provider,
                e.does,
                from,
                to,
                e.assets()
            ));
        }
        let public: Vec<String> = network::public_nodes().into_iter().collect();
        for node in &public {
            if !self.core().catalogue().nodes().is_public(node) {
                return Err(format!(
                    "{node} isn't on the operator's list of public nodes"
                ));
            }
        }
        let public: Vec<&str> = public.iter().map(String::as_str).collect();
        show::public_nodes(&public);
        std::fs::create_dir_all(&self.out).map_err(|e| e.to_string())?;
        std::fs::write(
            self.out.join("network.dot"),
            network::dot(kit::OPERATOR_WALLET, HELD_NODE),
        )
        .map_err(|e| e.to_string())?;
        self.t.gap();
        self.t.detail("The network is a graph, written as network.dot (Graphviz) in the output folder; conformance/demo/network.svg draws it.");
        self.t.detail(format!(
            "The router searches it for every path of at most {MAX_HOPS} edges, and picks one for the order's objective."
        ));
        let session = self
            .core()
            .open_session(&kit::customer())
            .map_err(failed("the customer's session"))?;
        self.session = session.clone();
        self.t.gap();
        self.t.step(format!(
            "The customer \"{}\" signs in: session {}",
            kit::customer().party.as_str(),
            session.as_str()
        ));
        self.publish()?;
        self.t.step("The log publishes its first checkpoint, signed by the log's own key, not the operator's");
        self.t.say("Before any order, the log already holds each provider's grant and signed advertisement:");
        self.timeline();
        Ok(())
    }

    // Act 2 -------------------------------------------------------------------------------------------------------

    fn two_objectives(&mut self) -> Step {
        self.t.act(
            "ACT 2",
            "One request, two objectives, two routes: USDC on sim-polygon to an address on sim-base",
        );
        self.t.say(format!(
            "The customer asks to move {} from their address on sim-polygon to one on sim-base.",
            money(AMOUNT, "USDC")
        ));
        self.t.say("Two bridges join the chains: sim-bridge charges 0.10 USDC and takes 15 minutes, sim-bridge-fast");
        self.t.say("charges 3.00 USDC and takes 20 seconds. The customer asks for a quote under each objective.");
        self.register_address();
        let ask = |name: &str, objective: &str, session: &Id| {
            orders::request(&Asked {
                name,
                session,
                origin: POLYGON_WALLETS,
                from: outer("sim-polygon-addresses", "USDC"),
                to: outer("sim-base-addresses", "USDC"),
                at: BASE_WALLETS,
                account: ADDRESS,
                objective,
                amount: AMOUNT,
            })
        };
        let cheapest = ask("req-demo-2-cheapest", "lowest_total_cost", &self.session);
        let cheapest = self.quote(cheapest, "lowest total cost")?;
        let fastest = ask("req-demo-2-fastest", "fastest_settlement", &self.session);
        let Quoted {
            order,
            record,
            digest,
            ..
        } = self.quote(fastest, "fastest settlement")?;
        self.t.say(format!(
            "Same request, same graph, different edges: the cheapest quote delivers {}, the fastest settles in seconds.",
            money(&cheapest.shown_out, "USDC")
        ));
        self.accept(&order, &digest)?;
        let ran = self.core().run(&order).map_err(failed("the order runs"))?;
        self.segments(
            "Core hands each segment to its provider, in order",
            &ran,
            &record,
        )?;
        let state = self.state(&order)?;
        self.t.step(format!("The order is {state:?}"));
        self.price(&order)?;
        self.timeline();
        self.orders.push(order);
        Ok(())
    }

    /// The customer's address on the chains, which the reserves' registers hold as theirs.
    fn register_address(&mut self) {
        orders::register_address(self.core());
    }

    /// Opens `request`'s order, prints its quote and the route the router chose, and accepts it as the customer.
    fn quote_and_accept(
        &mut self,
        request: OrderRequest,
    ) -> Step<(Id, rutent_contracts::router_api::DecisionRecord)> {
        let quoted = self.quote(request, "lowest total cost")?;
        self.accept(&quoted.order, &quoted.digest)?;
        Ok((quoted.order, quoted.record))
    }

    /// The customer accepts `order`'s quote, on its digest.
    fn accept(&mut self, order: &Id, digest: &rutent_contracts::module_interface::Digest) -> Step {
        self.core()
            .accept(&kit::customer(), order, digest)
            .map_err(failed("the acceptance"))?;
        self.t.step(format!(
            "The customer accepts it, on the quote's digest {}",
            show::short(digest.value.as_str())
        ));
        Ok(())
    }

    /// Opens `request`'s order and prints its quote, the route the router chose under `objective` and how many it
    /// weighed: the order, the decision record and the quote's digest.
    fn quote(&mut self, request: OrderRequest, objective: &str) -> Step<Quoted> {
        let customer = kit::customer();
        let told = self
            .core()
            .request(request)
            .map_err(failed("the request"))?;
        let order = Core::order_of(&told).ok_or_else(|| format!("no order opened: {told:?}"))?;
        self.t.step(format!("Core opens order {}", order.as_str()));
        let told = self
            .core()
            .quote(&customer, &order)
            .map_err(failed("the quote"))?;
        let Told::Exists {
            status: Status::Quoted { quote, digest },
            ..
        } = told
        else {
            if let Some(record) = self.core().decision_records().last() {
                let lines: Vec<String> = record
                    .candidates()
                    .iter()
                    .map(|c| {
                        let path: Vec<&str> = c
                            .parts
                            .iter()
                            .flat_map(|p| p.operations.iter())
                            .map(|o| o.provider.as_str())
                            .collect();
                        format!("{} — dropped: {:?}", path.join(" → "), c.dropped)
                    })
                    .collect();
                self.t.step(format!(
                    "The router weighed {} candidate routes and dropped each:",
                    lines.len()
                ));
                for line in lines {
                    self.t.detail(line);
                }
            }
            return Err(format!("the order wasn't quoted: {told:?}"));
        };
        let record = self
            .core()
            .decision_records()
            .into_iter()
            .find(|r| r.record_id() == &quote.decision_record)
            .ok_or("Core signed no decision record for the quote")?;
        self.t.step(format!(
            "The router weighed {} and chose this one for {objective}; Core signed the decision record {}:",
            candidates(record.candidates().len()),
            record.record_id().as_str()
        ));
        for line in show::route(&record) {
            self.t.detail(line);
        }
        let shown = serde_json::to_value(&quote.price).unwrap_or(Value::Null);
        self.quote_lines("The customer is shown a quote", &shown);
        Ok(Quoted {
            order,
            record,
            digest,
            shown_out: shown["shown_out"].as_str().unwrap_or("?").to_owned(),
        })
    }

    /// Prints what a shown quote or price says, segment by segment.
    fn quote_lines(&mut self, heading: &str, shown: &Value) {
        let asset_in = shown["segments"][0]["from"]["asset"]["id"]
            .as_str()
            .unwrap_or("?")
            .to_owned();
        let segments = shown["segments"].as_array().cloned().unwrap_or_default();
        let asset_out = segments
            .last()
            .and_then(|s| s["to"]["asset"]["id"].as_str())
            .unwrap_or("?")
            .to_owned();
        self.t.step(format!(
            "{heading} ({}):",
            shown["shown_as"].as_str().unwrap_or("?")
        ));
        self.t.detail(format!(
            "leaves the origin         {}",
            money(shown["amount_in"].as_str().unwrap_or("?"), &asset_in)
        ));
        for s in &segments {
            let asset = s["from"]["asset"]["id"].as_str().unwrap_or("?");
            let out = s["to"]["asset"]["id"].as_str().unwrap_or("?");
            self.t.detail(format!(
                "  {:<22} takes {:>18}, fee {:>14}, gives {:>18}",
                s["provider"].as_str().unwrap_or("?"),
                money(s["amount_in"].as_str().unwrap_or("?"), asset),
                money(s["fee"]["amount"].as_str().unwrap_or("?"), asset),
                money(s["amount_out"].as_str().unwrap_or("?"), out),
            ));
        }
        self.t.detail(format!(
            "operator's fee            {}",
            money(
                shown["operator_fee"]["amount"].as_str().unwrap_or("?"),
                &asset_out
            )
        ));
        self.t.detail(format!(
            "the customer receives     {}",
            money(shown["shown_out"].as_str().unwrap_or("?"), &asset_out)
        ));
    }

    /// Prints each segment `ran` ran: its provider, what came of it, and the receipt of Core's effect.
    fn segments(
        &mut self,
        heading: &str,
        ran: &[Ran],
        record: &rutent_contracts::router_api::DecisionRecord,
    ) -> Step<Vec<Accounted>> {
        let providers = show::providers(record);
        let assets = show::assets_out(record);
        let mut all = Vec::new();
        self.t.step(heading);
        for r in ran {
            match r {
                Ran::Segments(segments) => {
                    for (n, s) in segments.iter().enumerate() {
                        let provider = providers.get(n).map_or("?", String::as_str);
                        let outcome = match &s.outcome {
                            SegmentOutcome::Settled { amount_out, .. } => format!(
                                "settled, gave out {}",
                                money(
                                    serde_json::to_value(amount_out)
                                        .ok()
                                        .and_then(|v| v.as_str().map(str::to_owned))
                                        .unwrap_or_default()
                                        .as_str(),
                                    assets.get(n).map_or("?", String::as_str)
                                )
                            ),
                            other => format!("{other:?}"),
                        };
                        let effect = s.effect.as_ref().map(receipt).unwrap_or_default();
                        self.t.detail(format!(
                            "{:<22} {provider:<22} {outcome:<36} {effect}",
                            s.segment.as_str()
                        ));
                    }
                    all.extend(segments.iter().cloned());
                }
                Ran::Refused { refused, .. } => {
                    return Err(format!("the route was refused: {refused:?}"));
                }
            }
        }
        Ok(all)
    }

    fn state(&mut self, order: &Id) -> Step<OrderState> {
        Ok(self
            .core()
            .status(&kit::customer(), order)
            .map_err(failed("the customer reads the order"))?
            .state)
    }

    /// Prints the executed price Core reported once the order completed.
    fn price(&mut self, order: &Id) -> Step {
        let (digest, at) = self
            .core()
            .price_reported(order)
            .ok_or("Core reported no executed price")?;
        let shown = self
            .core()
            .price_of(order)
            .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
            .ok_or("no figures for the price")?;
        self.quote_lines(
            "Core reports the executed price, from what each segment did",
            &shown,
        );
        self.t.detail(format!(
            "recorded once as the price: {}, digest {}",
            receipt(&at),
            show::short(digest.value.as_str())
        ));
        Ok(())
    }

    // Act 1 -------------------------------------------------------------------------------------------------------

    fn provider_fails_mid_route(&mut self) -> Step {
        self.t.act(
            "ACT 1",
            "Pesos in Colombia to dollars in the US, across two private ledgers and a chain, one exchange failing",
        );
        self.t.say(format!(
            "The customer asks to send {} from their peso account at sim-co-bank to a dollar account at",
            money(AMOUNT, "COP")
        ));
        self.t.say("sim-us-bank, by lowest total cost. No single provider can do it: the route crosses sim-andina's");
        self.t
            .say("ledger, a public chain and sim-liberty's ledger, converting twice.");
        self.t.say(format!(
            "On this order, {FAILING_EXCHANGE} will fail the conversion after the withdrawal has settled."
        ));
        self.core()
            .world()
            .script_for(FAILING_EXCHANGE, "conversion", Behaviour::act(Act::Fail));
        self.t.step(format!(
            "(scripted on the simulated world: {FAILING_EXCHANGE} fails its next conversion)"
        ));
        let request = orders::request(&Asked {
            name: "req-demo-1",
            session: &self.session,
            origin: PESOS,
            from: outer("sim-co-bank", "COP"),
            to: outer("sim-us-bank", "USD"),
            at: DOLLARS,
            account: DESTINATION_ACCOUNT,
            objective: "lowest_total_cost",
            amount: AMOUNT,
        });
        let (order, record) = self.quote_and_accept(request)?;
        let ran = self.core().run(&order).map_err(failed("the order runs"))?;
        let segments = self.segments(
            "Core hands each segment to its provider, in order",
            &ran,
            &record,
        )?;
        let failed_at = segments
            .iter()
            .find(|s| matches!(s.outcome, SegmentOutcome::Failed))
            .ok_or("no segment failed")?
            .segment
            .clone();
        let state = self.state(&order)?;
        self.t.step(format!(
            "Nothing is handed over after the failure; the order is {state:?}"
        ));
        let owing = self
            .core()
            .owe_reversal(&order, &failed_at)
            .map_err(failed("the reversal owed"))?;
        self.t.step(format!(
            "Core records the reversal owed: {} owes {} back, in full, in the asset it took in",
            owing.provider.as_str(),
            money(&owing.amount, &owing.asset)
        ));
        let asked = self
            .core()
            .ask_reversal(&order, &failed_at)
            .map_err(failed("the reversal asked"))?;
        let completed = asked
            .completed
            .ok_or_else(|| format!("the reversal didn't complete: {:?}", asked.why))?;
        let whose = match &completed.word {
            rutent_core_orch::reversal::Word::Reserves => "the origin reserve's word".to_owned(),
            rutent_core_orch::reversal::Word::Publishers(publisher) => {
                format!(
                    "its publisher {}'s word, labelled as the publisher's",
                    publisher.as_str()
                )
            }
        };
        self.t.step(format!(
            "Core asks {} to reverse; the value is shown back at the node it came from, on {whose}",
            owing.provider.as_str()
        ));
        self.t.say("A provider's own word never undoes its own failure: inside a private node the word counted is");
        self.t
            .say("its publisher's, labelled so, and outside one it must be another party's.");
        let outcome = self
            .core()
            .recalculate(&order)
            .map_err(failed("the recalculation"))?;
        let Recalculation::Recalculated(recalculated) = outcome else {
            return Err(format!("no route from where the value is: {outcome:?}"));
        };
        self.t.step(format!(
            "Core recalculates from where the value is shown back, {}, to the same destination ({}):",
            show::endpoint(&serde_json::to_value(&recalculated.from).unwrap_or(Value::Null)),
            candidates(recalculated.record.candidates().len())
        ));
        for line in show::route(&recalculated.record) {
            self.t.detail(line);
        }
        self.t.say(
            "The customer isn't asked again: the new route reaches the destination they accepted.",
        );
        let ran = self
            .core()
            .run_recalculated(&order, &recalculated)
            .map_err(failed("the recalculated route"))?;
        self.segments("The recalculated route runs", &[ran], &recalculated.record)?;
        let state = self.state(&order)?;
        self.t.step(format!("The order is {state:?}"));
        if state != OrderState::Complete {
            return Err(format!("the recovered order is {state:?}, not complete"));
        }
        self.price(&order)?;
        self.timeline();
        self.orders.push(order);
        Ok(())
    }

    // Act 3 -------------------------------------------------------------------------------------------------------

    fn stuck_order_handed_on(&mut self) -> Step {
        self.t.act(
            "ACT 3",
            "A stuck order, handed on by two people under policy",
        );
        self.t.say(format!(
            "Mexican pesos at sim-mx-bank to USDC at the customer's address {ADDRESS} on sim-polygon. The route"
        ));
        self.t.say(format!(
            "converts in sim-azteca's ledger, ramps onto sim-base and bridges back to {HELD_NODE}, which the"
        ));
        self.t.say("operator's own wallet holds. The last step, the deposit to the address, will be refused its start.");
        self.register_address();
        let customer = kit::customer();
        let wallet = self
            .core()
            .install_wallet(HELD_NODE, "USDC")
            .map_err(|e| format!("the operator's wallet: {e:?}"))?;
        self.t.step(format!(
            "The operator's plain wallet ({}, a real module) is installed as {HELD_NODE}'s holding account",
            wallet.provider
        ));
        let asked = orders::request(&Asked {
            name: "req-demo-3",
            session: &self.session,
            origin: MEXICAN_PESOS,
            from: outer("sim-mx-bank", "MXN"),
            to: outer("sim-polygon-addresses", "USDC"),
            at: POLYGON_WALLETS,
            account: ADDRESS,
            objective: "lowest_total_cost",
            amount: AMOUNT,
        });
        let (order, record) = self.quote_and_accept(asked)?;
        let hops = show::providers(&record).len();
        let part = self
            .core()
            .first_part(&order)
            .ok_or("the order has no part")?;
        let begun = self.core().faults.begun;
        self.core().faults.refuse_begin = Some(begun + hops - 1);
        self.t.step("(planted at the platform's seam: the deposit out of the public node is refused its start)");
        let ran = self.core().run(&order).map_err(failed("the order runs"))?;
        let ran = self.segments("Core runs the route", &ran, &record)?;
        let resting = ran
            .iter()
            .rev()
            .find_map(|s| match &s.outcome {
                SegmentOutcome::Settled { amount_out, .. } => serde_json::to_value(amount_out)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned)),
                _ => None,
            })
            .ok_or("nothing settled before the refusal")?;
        self.core()
            .leave_no_route(&order)
            .map_err(failed("no route left"))?;
        if !self.core().is_stuck(&order, &part) {
            return Err("the order isn't stuck".to_owned());
        }
        let state = self.state(&order)?;
        self.t.step(format!(
            "No route is left from {HELD_NODE}: the order is {state:?}, {} resting in the operator's wallet",
            money(&resting, "USDC")
        ));
        self.core()
            .accept_release(&wallet)
            .map_err(|e| format!("the wallet's release: {e:?}"))?;
        let linked_order = LinkedOrder {
            from: order.clone(),
            holder: id(&wallet.provider),
            customer: customer.clone(),
            session: self.session.clone(),
            route: serde_json::from_value::<Request>(json!({
                "request_id": "req-demo-3-linked",
                "round": {"kind": "shortlist"},
                "from": inner(HELD_NODE, "USDC"),
                "to": outer("sim-polygon-addresses", "USDC"),
                "amount": resting,
                "objective": {"kind": "lowest_total_cost"},
            }))
            .map_err(|e| format!("the linked route request: {e}"))?,
            inputs: BTreeMap::from([
                ("payer.bank_account".to_owned(), ORIGIN_ACCOUNT.to_owned()),
                ("beneficiary.address".to_owned(), ADDRESS.to_owned()),
            ]),
        };
        let linked = self
            .core()
            .accept_linked(&linked_order)
            .map_err(failed("the linked order"))?;
        self.t.step(format!(
            "The customer accepts a linked order {} from where the value rests; it starts only when handed on",
            linked.order.as_str()
        ));
        let alice = self.staff("desk-a", "alice", Permission::DecideStuck);
        let bob = self.staff("desk-b", "bob", Permission::MoveStuck);
        let resolution = Resolution::HandOn {
            by: id(&wallet.provider),
            linked_order: linked.order.clone(),
            asset: Denomination {
                id: "USDC".into(),
                decimals: 2,
            },
            amount: PositiveAmount::new(resting.as_str()).ok_or("an amount")?,
        };
        let evidence = orders::evidence_about(self.core(), &order)?;
        let decision = self
            .core()
            .decide_stuck(&order, &alice, &resolution, &evidence)
            .map_err(failed("the decision"))?;
        self.t.step(format!(
            "alice (operator desk A) decides to hand it on, citing the provider's kept statement: {:?}",
            decision.outcome
        ));
        let decided = Decided {
            request: decision.request.clone(),
            decision: decision.decision.clone(),
        };
        let by_alice = self.core().move_stuck(&order, &decided, &alice);
        self.t.step(format!(
            "alice tries to carry out her own decision: refused, {}",
            match &by_alice {
                Err(StepError::Resolve(rutent_core_orch::stuck::ResolveError::SameParty)) => {
                    "since whoever decides can't also move it".to_owned()
                }
                Ok(moved) => format!("{moved:?}"),
                Err(e) => format!("{e:?}"),
            }
        ));
        if matches!(by_alice, Ok(Moved::CarriedOut { .. })) {
            return Err("the one who decided moved the value".to_owned());
        }
        let moved = self
            .core()
            .move_stuck(&order, &decided, &bob)
            .map_err(failed("bob's move"))?;
        let Moved::CarriedOut {
            effect,
            resolved: Resolved::Done,
            ..
        } = moved
        else {
            return Err(format!("the move wasn't done: {moved:?}"));
        };
        self.t.step(format!(
            "bob (operator desk B) carries it out: {}",
            receipt(&effect)
        ));
        let handed = self
            .core()
            .hand_on(&order, &effect, &bob)
            .map_err(|e| format!("the hand-on: {e:?}"))?;
        self.t.step(format!(
            "The wallet's own withdrawal record starts the linked order at {}",
            show::endpoint(&serde_json::to_value(&handed.at).unwrap_or(Value::Null))
        ));
        let ran = self
            .core()
            .run_linked(&order, &linked_order, &linked)
            .map_err(failed("the linked order runs"))?;
        let linked_record = self
            .core()
            .decision_records()
            .into_iter()
            .find(|r| r.record_id() == &linked.route.decision_record)
            .ok_or("no decision record for the linked route")?;
        let segments = self.segments(
            "The linked order runs its accepted route",
            &ran,
            &linked_record,
        )?;
        if !segments
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. }))
        {
            return Err("the linked route didn't settle".to_owned());
        }
        let notices = self
            .core()
            .notify()
            .map_err(|e| format!("the notices: {e:?}"))?;
        self.t.step(format!(
            "Core signs and sends {notices} notices to the customer and the operator about what happened"
        ));
        self.timeline();
        self.orders.push(order);
        self.orders.push(linked.order);
        Ok(())
    }

    /// One of the operator's staff: a person on `desk`, acting for the operator, holding `permission` alone.
    fn staff(&mut self, desk: &str, login: &str, permission: Permission) -> Actor {
        orders::staff(self.core(), desk, login, permission)
    }

    // Epilogue ----------------------------------------------------------------------------------------------------

    fn checked_from_outside(&mut self, took: Duration) -> Step<bool> {
        self.t.act("EPILOGUE", "Anyone can check the log");
        self.core().clock.advance(CHECKPOINT_EVERY);
        let published = self.publish()?;
        self.t.step(format!(
            "An hour later by Core's clock, the log publishes {published} more checkpoint, committing to every entry so far"
        ));
        let history = history_of(&self.core().log());
        let checkpointer = self.checkpointer.take().expect("the checkpointer runs");
        let openings = openings_of(checkpointer.kept());
        let through = checkpointer
            .kept()
            .last()
            .and_then(|k| tick_of(k.opening.tick.as_str()))
            .ok_or("no checkpoint was published")?;
        drop(checkpointer);
        let core = self.core.take().expect("Core runs");
        let records = core.decision_record_bytes();
        let run = Run::read(&self.scenario, self.seed, core, self.orders.clone(), false);
        let handed = audit::write(
            &self.out,
            &history,
            &self.received,
            &openings,
            &run.kept,
            through,
        )
        .map_err(|e| format!("the folder {}: {e}", self.out.display()))?;
        let records_dir = self.out.join("decision-records");
        std::fs::create_dir_all(&records_dir).map_err(|e| e.to_string())?;
        for (name, bytes) in &records {
            std::fs::write(records_dir.join(format!("{name}.json")), bytes)
                .map_err(|e| e.to_string())?;
        }
        self.t.step(format!("Written to {}:", self.out.display()));
        self.t.detail(format!(
            "log/history.jsonl       {} entries, one record a line",
            history.len()
        ));
        self.t.detail(format!(
            "log/checkpoints.jsonl   {} checkpoints, as an outside checker received them",
            self.received.len()
        ));
        self.t.detail(
            "log/openings.jsonl      what each checkpoint commits to, handed over by the operator",
        );
        self.t.detail(format!(
            "evidence/kept/          {} statements, each salted and filed under its commitment",
            run.kept.len()
        ));
        self.t.detail(format!(
            "decision-records/       {} records the router chose and Core signed",
            records.len()
        ));
        self.t
            .step("What each provider's own book holds, as the simulated world kept it:");
        for (provider, book) in &run.books {
            self.t
                .detail(format!("{provider:<22} {} book entries", book.len()));
        }

        let binary = audit::binary()?;
        let full = audit::arguments(&handed, &handed.history, true);
        std::fs::write(self.out.join("verify.sh"), audit::script(&binary, &full))
            .map_err(|e| e.to_string())?;
        self.t.step(
            "audit-check, the checker's own tool, reads only those files and the log's public key.",
        );
        self.t.detail(format!(
            "{} …   (the whole command is in verify.sh)",
            binary.display()
        ));

        self.t
            .step("1. The history against the checkpoints the checker received:");
        let checkpoints =
            audit::check(&binary, &audit::arguments(&handed, &handed.history, false))?;
        self.findings(&checkpoints);
        let holds = checkpoints.code == Some(0);

        self.t.step(
            "2. Every provider's statement the log records, against the evidence kept for it:",
        );
        let evidence = audit::check(&binary, &full)?;
        self.findings(&evidence);
        let gap: Vec<usize> = not_borne_out(&evidence.output);
        let all_inner = !gap.is_empty() && gap.iter().all(|&n| is_inner_segments(history.get(n)));
        let borne_out = match evidence.code {
            Some(0) => true,
            Some(1) if all_inner && evidence.output.lines().all(|l| !l.contains("DoesNotHold")) => {
                self.t.detail(format!(
                    "These {} are the providers' inner-segments statements, which audit-check's evidence read doesn't yet",
                    gap.len()
                ));
                self.t.detail("match to the order they are about: a known gap in rutent-core-audit, reported to M3, not tampering.");
                true
            }
            _ => false,
        };

        let tampered_path = self.out.join("tampered").join("history.jsonl");
        std::fs::create_dir_all(tampered_path.parent().expect("a folder"))
            .map_err(|e| e.to_string())?;
        let changed = audit::tamper(&history, &tampered_path, AMOUNT).map_err(|e| e.to_string())?;
        self.t.step(format!(
            "3. A copy with one figure changed in entry #{} ({} → {}0), as someone with the operator's full access could write it:",
            changed.map_or("?".to_owned(), |n| n.to_string()),
            AMOUNT,
            AMOUNT
        ));
        let tampered = audit::check(&binary, &audit::arguments(&handed, &tampered_path, false))?;
        self.findings(&tampered);
        let caught = tampered.code == Some(1) && tampered.output.contains("DoesNotHold");

        self.t.gap();
        self.t.say("The operator can't rewrite what the log says without every checker who holds a checkpoint noticing,");
        self.t.say("and each provider's statement is kept as signed, so the log's account of who stated what is checkable.");
        self.t.gap();
        self.t.line(format!(
            "Done in {:.1}s (seed {}). Nothing held or moved real value.",
            took.as_secs_f64(),
            self.seed
        ));
        std::fs::write(self.out.join("timeline.txt"), self.t.text()).map_err(|e| e.to_string())?;
        Ok(holds && borne_out && caught && changed.is_some())
    }

    /// Prints what one run of `audit-check` said, and its exit code.
    fn findings(&mut self, checked: &audit::Checked) {
        for line in checked.output.lines() {
            self.t.detail(format!("│ {line}"));
        }
        self.t.detail(format!(
            "exit {:?}: {}",
            checked.code,
            verdict(checked.code)
        ));
    }
}
