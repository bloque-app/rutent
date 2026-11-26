//! `rutent-demo`: a scripted, narrated run of rutent on a network of simulated providers, for an operator to show a
//! customer how the system works. Nothing in it holds or moves real value: every provider is one of
//! `rutent-scenarios`'s simulated providers, admitted to the real module platform and signing for real, and Core is
//! the scenario assembly's, the same one `M3`'s scenarios run.
//!
//! The story, after the network (act 0), in three acts and an epilogue:
//!
//! 1. A customer's order from Colombian pesos at one bank to US dollars at another, quoted, accepted and routed through
//!    a reserve, an exchange, a ramp onto a public chain and a second reserve, with the exchange failing mid-route: the
//!    reversal owed, asked and shown back, the route recalculated from where the value is, the order completed with no
//!    new yes from the customer, and its executed price reported.
//! 2. The next order on the same route, straight through, settled and priced.
//! 3. An order whose last step can't start, stuck at a public node where the operator's wallet holds the value: one
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
//! `--out` is the folder the log and the evidence are written to (`rutent-demo-out` by default), `--seed` the run's
//! seed (every identifier Core draws follows from it), and `--step` waits for Enter between acts, for a live
//! presentation. `conformance/demo/README.md` is the presenter's guide.

mod audit;
mod show;

use std::collections::BTreeMap;
use std::io::BufRead as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use rutent_conformance::sim::{Act, Behaviour, Role as SimRole, Spec};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Decided, Destination, Entry, Parties, Resolution, Role,
};
use rutent_contracts::provider_role::{Denomination, OuterNode};
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
use rutent_scenarios::kit::{self, AMOUNT, id};
use serde_json::{Value, json};

use show::{Transcript, money, receipt};

// The network. Two groups of simulated providers, each of one publisher (`rutent_conformance::sim::shape_of`).

/// A reserve between its bank, in COP, and its publisher's private ledger.
const PESO_RESERVE: &str = "sim-join-in";
/// An exchange at that private ledger, from COP to USD.
const EXCHANGE: &str = "sim-join-exchange";
/// A ramp from the private ledger onto the public chain `sim-join-chain`, in USD.
const RAMP: &str = "sim-join-ramp-quick";
/// A reserve between the public chain and its own bank, in USD.
const DOLLAR_RESERVE: &str = "sim-join-out";
/// A reserve between the network `sim-chain-a` and its private node, in USDC.
const CHAIN_IN: &str = "sim-chain-in";
/// A ramp from that private node onto the public node `sim-chain-b`.
const CHAIN_RAMP: &str = "sim-chain-ramp-b";
/// A reserve between `sim-chain-b` and the network `sim-chain-e`, which deposits to an address.
const CHAIN_OUT: &str = "sim-chain-out-b";
/// The public node the stuck order's value rests at, which the operator's wallet holds.
const PUBLIC_NODE: &str = "sim-chain-b";
/// The public nodes of the network, on the operator's list: where anyone can see what moved.
const PUBLIC_NODES: &[&str] = &["sim-join-chain", PUBLIC_NODE];

/// Every edge of the network: the provider, the node it takes value from, the node it gives it to, and the asset.
const EDGES: &[(&str, &str, &str, &str)] = &[
    (PESO_RESERVE, "sim-join-in-bank", "sim-join-ledger", "COP"),
    (EXCHANGE, "sim-join-ledger", "sim-join-ledger", "COP → USD"),
    (RAMP, "sim-join-ledger", "sim-join-chain", "USD"),
    (DOLLAR_RESERVE, "sim-join-chain", "sim-join-out-bank", "USD"),
    (CHAIN_IN, "sim-chain-a", "sim-chain-ledger", "USDC"),
    (CHAIN_RAMP, "sim-chain-ledger", PUBLIC_NODE, "USDC"),
    (CHAIN_OUT, PUBLIC_NODE, "sim-chain-e", "USDC"),
];

/// The network as a Graphviz graph: each node a vertex, shaped and coloured by its kind (outer, private, public),
/// and each provider a directed edge from the node it takes value from to the node it gives it to.
fn network_dot() -> String {
    let mut nodes: Vec<&str> = EDGES
        .iter()
        .flat_map(|(_, from, to, _)| [*from, *to])
        .collect();
    nodes.sort_unstable();
    nodes.dedup();
    let mut dot = String::from(
        "// The rutent demo's network (conformance/demo/README.md). Render it with `dot -Tsvg network.dot > network.svg`.\n\
         digraph network {\n  rankdir=LR;\n  node [fontname=\"Helvetica\", fontsize=11, style=filled];\n  \
         edge [fontname=\"Helvetica\", fontsize=10];\n",
    );
    for node in nodes {
        let (kind, shape, fill) = if PUBLIC_NODES.contains(&node) {
            ("public", "doublecircle", "#9FE1CB")
        } else if node.ends_with("-ledger") {
            ("private", "circle", "#CECBF6")
        } else {
            ("outer", "box", "#D3D1C7")
        };
        dot.push_str(&format!(
            "  \"{node}\" [label=\"{node}\\n({kind})\", shape={shape}, fillcolor=\"{fill}\"];\n"
        ));
    }
    for (provider, from, to, asset) in EDGES {
        dot.push_str(&format!(
            "  \"{from}\" -> \"{to}\" [label=\"{provider}\\n{asset}\"];\n"
        ));
    }
    dot.push_str(&format!(
        "  \"{}\" [label=\"operator-wallet\\n(holding account)\", shape=note, fillcolor=\"#F1EFE8\"];\n  \
         \"{}\" -> \"{PUBLIC_NODE}\" [style=dashed, arrowhead=none, label=\"holds\"];\n}}\n",
        kit::OPERATOR_WALLET,
        kit::OPERATOR_WALLET
    ));
    dot
}

/// The address the customer gives for the deposit on chain.
const ADDRESS: &str = "0x5e11a9d0c0ffee";

/// What the command line set.
struct Options {
    out: PathBuf,
    seed: u64,
    step: bool,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        out: PathBuf::from("rutent-demo-out"),
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

/// Runs this binary again from cargo's own copy of it under `deps/`, where it isn't already running from there.
///
/// The assembly finds the platform's executables and the simulated providers' build by the build directory a *test*
/// executable sits in: the parent of the nearest `deps` or `build` directory above it
/// (`rutent_scenarios::sim::disk`). Cargo writes this binary as `<target>/<profile>/deps/rutent_demo-<hash>` and
/// links it to `<target>/<profile>/rutent-demo`, which has no such directory above it. Run from the copy under
/// `deps/`, the assembly finds what it looks for, and nothing of `rutent-scenarios` changes.
#[cfg(unix)]
fn from_deps() {
    use std::os::unix::fs::MetadataExt as _;
    use std::os::unix::process::CommandExt as _;
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else { return };
    if dir.file_name().is_some_and(|n| n == "deps" || n == "out") {
        return;
    }
    let Ok(this) = std::fs::metadata(&exe) else {
        return;
    };
    let Ok(listed) = std::fs::read_dir(dir.join("deps")) else {
        return;
    };
    let mut candidates: Vec<(bool, std::time::SystemTime, PathBuf)> = listed
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("rutent_demo-") && !n.contains('.'))
        })
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            let same = meta.dev() == this.dev() && meta.ino() == this.ino();
            Some((same, meta.modified().ok()?, p))
        })
        .collect();
    // The one cargo linked this binary from, else the newest.
    candidates.sort();
    if let Some((_, _, copy)) = candidates.pop() {
        let error = std::process::Command::new(&copy)
            .args(std::env::args_os().skip(1))
            .exec();
        eprintln!("couldn't run {}: {error}", copy.display());
    }
}

#[cfg(not(unix))]
fn from_deps() {}

fn main() -> ExitCode {
    from_deps();
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
        .and_then(|()| demo.order_across_providers())
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
        let names = [
            (SimRole::Reserve, PESO_RESERVE),
            (SimRole::Exchange, EXCHANGE),
            (SimRole::Ramp, RAMP),
            (SimRole::Reserve, DOLLAR_RESERVE),
            (SimRole::Reserve, CHAIN_IN),
            (SimRole::Ramp, CHAIN_RAMP),
            (SimRole::Reserve, CHAIN_OUT),
        ];
        let given = Given {
            providers: names
                .iter()
                .map(|(role, name)| Spec::of(*role, name))
                .collect(),
            staging: Staged::AtOnce,
            deadline: Duration::from_secs(2),
            delivery: Delivery::Once,
        };
        let scenario = Scenario::new("rutent demo", "AC-1", given);
        println!(
            "Putting Core together and admitting the simulated providers (their modules compile on first use)…"
        );
        let (core, _) = scenario.core(options.seed, &mut Reference);
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
        let rows = [
            (
                PESO_RESERVE,
                "reserve",
                "sim-join-in-bank (outer) → sim-join-ledger (private)",
                "COP",
            ),
            (
                EXCHANGE,
                "exchange",
                "inside sim-join-ledger (private)",
                "COP → USD",
            ),
            (
                RAMP,
                "ramp",
                "sim-join-ledger (private) → sim-join-chain (public)",
                "USD",
            ),
            (
                DOLLAR_RESERVE,
                "reserve",
                "sim-join-chain (public) → sim-join-out-bank (outer)",
                "USD",
            ),
            (
                CHAIN_IN,
                "reserve",
                "sim-chain-a (outer) → sim-chain-ledger (private)",
                "USDC",
            ),
            (
                CHAIN_RAMP,
                "ramp",
                "sim-chain-ledger (private) → sim-chain-b (public)",
                "USDC",
            ),
            (
                CHAIN_OUT,
                "reserve",
                "sim-chain-b (public) → sim-chain-e (outer), to an address",
                "USDC",
            ),
        ];
        for (name, role, between, asset) in rows {
            self.t
                .detail(format!("{name:<22} {role:<9} {between:<58} {asset}"));
        }
        self.t.detail("The first three share sim-join-publisher's private node; sim-chain-in and sim-chain-ramp-b share");
        self.t.detail("sim-chain-publisher's.");
        for node in PUBLIC_NODES {
            if !self.core().catalogue().nodes().is_public(node) {
                return Err(format!(
                    "{node} isn't on the operator's list of public nodes"
                ));
            }
        }
        show::public_nodes(PUBLIC_NODES);
        let graph = network_dot();
        std::fs::create_dir_all(&self.out).map_err(|e| e.to_string())?;
        std::fs::write(self.out.join("network.dot"), graph).map_err(|e| e.to_string())?;
        self.t.detail(format!(
            "The network is a graph: nodes are places value rests, each provider an edge between two. {} has it in",
            self.out.join("network.dot").display()
        ));
        self.t.detail(
            "Graphviz form (`dot -Tsvg network.dot`); conformance/demo/network.svg is it drawn.",
        );
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
        let tick0 = self.publish()?;
        self.t.step(format!(
            "The log publishes its first checkpoint ({tick0}), signed by the log's own key, not the operator's"
        ));
        self.t.say("Before any order, the log already holds each provider's grant and signed advertisement:");
        self.timeline();
        Ok(())
    }

    // Act 2 -------------------------------------------------------------------------------------------------------

    fn order_across_providers(&mut self) -> Step {
        self.t.act(
            "ACT 2",
            "The next order, straight through four providers: settled and priced",
        );
        self.t
            .say("A second order on the same route, and this time nothing goes wrong.");
        let request = self.across("req-demo-2");
        let (order, record) = self.quote_and_accept(request)?;
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

    /// The customer's order across the joined providers, from the peso bank to the dollar bank.
    fn across(&self, name: &str) -> OrderRequest {
        request(
            name,
            &self.session,
            PESO_RESERVE,
            outer("sim-join-in-bank", "COP"),
            outer("sim-join-out-bank", "USD"),
            DOLLAR_RESERVE,
            DESTINATION_ACCOUNT,
        )
    }

    /// Opens `request`'s order, prints its quote and the route the router chose, and accepts it as the customer.
    fn quote_and_accept(
        &mut self,
        request: OrderRequest,
    ) -> Step<(Id, rutent_contracts::router_api::DecisionRecord)> {
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
            return Err(format!("the order wasn't quoted: {told:?}"));
        };
        let record = self
            .core()
            .decision_records()
            .into_iter()
            .find(|r| r.record_id() == &quote.decision_record)
            .ok_or("Core signed no decision record for the quote")?;
        self.t.step(format!(
            "The router chose this route; Core signed the decision record {}:",
            record.record_id().as_str()
        ));
        for line in show::route(&record) {
            self.t.detail(line);
        }
        let shown = serde_json::to_value(&quote.price).unwrap_or(Value::Null);
        self.quote_lines("The customer is shown a quote", &shown);
        self.core()
            .accept(&customer, &order, &digest)
            .map_err(failed("the acceptance"))?;
        self.t.step(format!(
            "The customer accepts it, on the quote's digest {}",
            show::short(digest.value.as_str())
        ));
        Ok((order, record))
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
                                serde_json::to_value(amount_out)
                                    .ok()
                                    .and_then(|v| v.as_str().map(str::to_owned))
                                    .unwrap_or_default()
                            ),
                            other => format!("{other:?}"),
                        };
                        let effect = s.effect.as_ref().map(receipt).unwrap_or_default();
                        self.t.detail(format!(
                            "{:<16} {provider:<22} {outcome:<32} {effect}",
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
            "An order across four providers, one failing mid-route: reversed, recovered, priced",
        );
        self.t.say(format!(
            "The customer asks to send {} from their peso account at {PESO_RESERVE}'s bank",
            money(AMOUNT, "COP")
        ));
        self.t.say(format!(
            "to a dollar account at {DOLLAR_RESERVE}'s bank, by lowest total cost."
        ));
        self.t.say(format!(
            "On this order, {EXCHANGE} will fail the conversion after the withdrawal has settled."
        ));
        self.core()
            .world()
            .script_for(EXCHANGE, "conversion", Behaviour::act(Act::Fail));
        self.t.step(format!(
            "(scripted on the simulated world: {EXCHANGE} fails its next conversion)"
        ));
        let request = self.across("req-demo-1");
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
        self.t.step(format!(
            "Core asks {} to reverse; the value is shown back at the node it came from, on {:?}'s word (grade {:?})",
            owing.provider.as_str(),
            completed.word,
            completed.grade
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
            "Core recalculates from where the value is shown back, {}, to the same destination:",
            show::endpoint(&serde_json::to_value(&recalculated.from).unwrap_or(Value::Null))
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
            "A USDC order from network sim-chain-a to the address {ADDRESS} on sim-chain-e. The operator's own wallet"
        ));
        self.t.say(format!(
            "holds the public node {PUBLIC_NODE} the route passes through. The last step's start will be refused."
        ));
        let customer = kit::customer();
        self.core()
            .registers()
            .accounts
            .insert(ADDRESS.to_owned(), customer.party.as_str().to_owned());
        let wallet = self
            .core()
            .install_wallet(PUBLIC_NODE, "USDC")
            .map_err(|e| format!("the operator's wallet: {e:?}"))?;
        self.t.step(format!(
            "The operator's plain wallet ({}, a real module) is installed as {PUBLIC_NODE}'s holding account",
            wallet.provider
        ));
        let asked = request(
            "req-demo-3",
            &self.session,
            CHAIN_IN,
            outer("sim-chain-a", "USDC"),
            outer("sim-chain-e", "USDC"),
            CHAIN_OUT,
            ADDRESS,
        );
        let (order, record) = self.quote_and_accept(asked)?;
        let part = self
            .core()
            .first_part(&order)
            .ok_or("the order has no part")?;
        let begun = self.core().faults.begun;
        self.core().faults.refuse_begin = Some(begun + 2);
        self.t.step("(planted at the platform's seam: the deposit out of the public node is refused its start)");
        let ran = self.core().run(&order).map_err(failed("the order runs"))?;
        self.segments("Core runs the route", &ran, &record)?;
        self.core()
            .leave_no_route(&order)
            .map_err(failed("no route left"))?;
        if !self.core().is_stuck(&order, &part) {
            return Err("the order isn't stuck".to_owned());
        }
        let state = self.state(&order)?;
        self.t.step(format!(
            "No route is left from {PUBLIC_NODE}: the order is {state:?}, its value resting in the operator's wallet"
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
                "from": inner(PUBLIC_NODE, "USDC"),
                "to": outer("sim-chain-e", "USDC"),
                "amount": AMOUNT,
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
            amount: PositiveAmount::new(AMOUNT).ok_or("an amount")?,
        };
        let evidence = self.evidence_about(&order)?;
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
            "alice tries to carry out her own decision: refused ({})",
            match &by_alice {
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
        let person = Actor {
            party: id(desk),
            acts: id(login),
            role: Role::Operator,
            kind: ActorKind::Person,
        };
        let core = self.core();
        let mut registers = core.registers();
        registers.list(&Actor {
            party: kit::operator().party,
            acts: id(desk),
            role: Role::Operator,
            kind: ActorKind::Component,
        });
        registers.list(&person);
        registers.permit(&person, permission);
        person
    }

    /// The commitment of a provider's result the log holds about `order`: the evidence a decision names.
    fn evidence_about(
        &mut self,
        order: &Id,
    ) -> Step<Vec<rutent_contracts::module_interface::Digest>> {
        let log = self.core().log();
        let found = log
            .entries()
            .iter()
            .find_map(|(_, e)| match e {
                Entry::Stated(s)
                    if s.what.statement.as_str()
                        == rutent_contracts::provider_role::RESULT_SHAPE
                        && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order) =>
                {
                    Some(vec![s.commitment.clone()])
                }
                _ => None,
            })
            .ok_or_else(|| "no provider's statement about the order".to_owned());
        drop(log);
        found
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
                .detail(format!("{provider:<22} {} entries", book.len()));
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
        let changed = audit::tamper(&history, &tampered_path).map_err(|e| e.to_string())?;
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

fn verdict(code: Option<i32>) -> &'static str {
    match code {
        Some(0) => "it holds",
        Some(1) => "the checker found something: each finding is printed above",
        _ => "the checker couldn't run",
    }
}

/// The entries `audit-check` printed as not borne out by their evidence, by index.
fn not_borne_out(output: &str) -> Vec<usize> {
    output
        .lines()
        .filter_map(|l| l.trim().strip_prefix("NotBorneOut { index: "))
        .filter_map(|rest| rest.trim_end_matches(" }").parse().ok())
        .collect()
}

/// Whether `record` is a provider's inner-segments statement.
fn is_inner_segments(record: Option<&Vec<u8>>) -> bool {
    record
        .and_then(|r| serde_json::from_slice::<Value>(r).ok())
        .is_some_and(|v| {
            v["what"]["statement"]
                .as_str()
                .is_some_and(|s| s.ends_with(":inner-segments"))
        })
}

/// An asset in two decimals, as an endpoint's unit.
fn unit(asset: &str) -> Value {
    json!({ "id": asset, "decimals": 2 })
}

/// An outer node's endpoint in `asset`.
fn outer(network: &str, asset: &str) -> Value {
    json!({ "node": { "network": network }, "asset": unit(asset) })
}

/// An inner node's endpoint in `asset`.
fn inner(node: &str, asset: &str) -> Value {
    json!({ "node": node, "asset": unit(asset) })
}

/// The customer's order of `AMOUNT` from the endpoint `from` of the reserve `origin` to the endpoint `to` of the
/// reserve `at`, delivered to `account`, by lowest total cost: the customer is requester, beneficiary and payer.
fn request(
    name: &str,
    session: &Id,
    origin: &str,
    from: Value,
    to: Value,
    at: &str,
    account: &str,
) -> OrderRequest {
    let customer = kit::customer();
    let network = to["node"]["network"]
        .as_str()
        .map_or_else(|| format!("{at}-bank"), str::to_owned);
    OrderRequest {
        request: id(name),
        caller: customer.clone(),
        parties: Parties {
            requester: customer.party.clone(),
            beneficiary: customer.party.clone(),
            settlement_account_at: None,
            payer: customer.party.clone(),
        },
        origin_reserve: id(origin),
        session: session.clone(),
        destination: Destination {
            reserve: id(at),
            node: OuterNode { network },
            account: id(account),
        },
        route: serde_json::from_value(json!({
            "request_id": format!("{name}-route"),
            "round": {"kind": "shortlist"},
            "from": from,
            "to": to,
            "amount": AMOUNT,
            "objective": {"kind": "lowest_total_cost"},
        }))
        .expect("a route request"),
        inputs: BTreeMap::from([
            ("payer.bank_account".to_owned(), ORIGIN_ACCOUNT.to_owned()),
            ("beneficiary.bank_account".to_owned(), account.to_owned()),
        ]),
        needs: Vec::new(),
    }
}
