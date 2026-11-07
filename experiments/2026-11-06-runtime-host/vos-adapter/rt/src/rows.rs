//! The re-score's new rows on VOS (`R-01` to `R-09`, re-score brief section 5), and the byte-equality corpus.
//! Each prints its measurements; the verdict against the pre-fixed criterion (`decisions.json`,
//! `exp1-rescore-criteria`) is printed beside them, and the run note copies it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rutent_conformance::host::probe::{self, Probe};
use rutent_contracts::state_machine::host::{
    Delivery, Host, HostError, SnapshotEntry, SnapshotStream,
};
use rutent_contracts::state_machine::{RecordedInput, StateMachine};
use rutent_vos_program::{OrderRow, RecordRow, RefusalRow, Submitted};
use rutent_vos_rt::cluster::{Cluster, ClusterConfig, INSTANCE};
use rutent_vos_rt::host::{
    self, Methods, RtHarness, RtHost, RtStore, query, submit_arguments, unavailable,
};
use rutent_vos_rt::platform::Backend;
use vos_node::client::Client;
use vos_wire::Encode;
use vos_wire::ids::AgentId;

use crate::passes::{Env, backend_proof, short};

/// The inputs of `count` orders, each probe's seven, under `prefix`.
pub fn orders(prefix: &str, count: usize) -> Vec<Vec<RecordedInput>> {
    (0..count)
        .map(|n| probe::order_inputs(&format!("{prefix}{n}"), (n as u32 % 7000) * 7))
        .collect()
}

/// A host on a fresh cluster at `base/name`.
pub fn fresh(
    env: &Env,
    config: ClusterConfig,
    base: &Path,
    name: &str,
) -> Result<RtHost, HostError> {
    let mut harness = RtHarness::new(config, base.to_path_buf(), name);
    let store = harness.fresh().map_err(|e| unavailable(e.to_string()))?;
    let _ = env;
    RtHost::open(store)
}

/// A client that calls node `n` alone: no routes, so no failover.
fn pinned(cluster: &Cluster, n: usize, seed: u8) -> Client {
    Client::new(
        &cluster.target(n),
        ed25519_dalek::SigningKey::from_bytes(&[seed; 32]),
    )
}

/// One node's whole state, as its own replica reads it: every order's stored rows, and the refusal log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct View {
    pub journals: BTreeMap<Vec<u8>, Vec<(u64, RecordRow)>>,
    pub refusals: Vec<(u64, RefusalRow)>,
}

impl View {
    pub fn records(&self) -> usize {
        self.journals.values().map(Vec::len).sum()
    }

    /// The record ids this view holds, each with how many records carry it.
    pub fn record_ids(&self) -> BTreeMap<String, usize> {
        let mut ids = BTreeMap::new();
        for rows in self.journals.values() {
            for (_, row) in rows {
                if let Ok(input) = serde_json::from_slice::<RecordedInput>(&row.input) {
                    *ids.entry(input.record_id.as_str().to_string()).or_insert(0) += 1;
                }
            }
        }
        ids
    }
}

/// Node `n`'s view, read by a client pinned to it.
pub fn view(
    cluster: &Cluster,
    n: usize,
    agent: AgentId,
    methods: &Methods,
) -> Result<View, HostError> {
    let client = pinned(cluster, n, 11);
    let mut view = View::default();
    let mut from = 1u64;
    loop {
        let page: Vec<(u64, OrderRow)> =
            query(&client, agent, methods.orders_from, &from.encode())?;
        let Some((last, _)) = page.last() else { break };
        from = last + 1;
        for (place, row) in page {
            let mut rows = Vec::new();
            let mut seq = 1u64;
            while seq <= row.len {
                let records: Vec<(u64, RecordRow)> = query(
                    &client,
                    agent,
                    methods.records_from,
                    &[place.encode(), seq.encode()].concat(),
                )?;
                let Some((last, _)) = records.last() else {
                    break;
                };
                seq = last + 1;
                rows.extend(records);
            }
            view.journals.insert(row.id, rows);
        }
    }
    let mut from = 1u64;
    loop {
        let page: Vec<(u64, RefusalRow)> =
            query(&client, agent, methods.refusals_from, &from.encode())?;
        let Some((last, _)) = page.last() else { break };
        from = last + 1;
        view.refusals.extend(page);
    }
    Ok(view)
}

/// Waits until every listed node's view equals the first's, or the wait ends: the views, and whether they agree.
/// A node whose view can't be read counts as disagreeing; the last such error is printed once the wait ends.
fn converge(
    cluster: &Cluster,
    nodes: &[usize],
    agent: AgentId,
    methods: &Methods,
    wait: Duration,
) -> (Vec<View>, bool) {
    let until = Instant::now() + wait;
    loop {
        let read: Vec<Result<View, HostError>> = nodes
            .iter()
            .map(|n| view(cluster, *n, agent, methods))
            .collect();
        let failed = read.iter().find_map(|r| r.as_ref().err().cloned());
        let views: Vec<View> = read.into_iter().map(Result::unwrap_or_default).collect();
        let equal = failed.is_none() && !views.is_empty() && views.windows(2).all(|w| w[0] == w[1]);
        if equal || Instant::now() > until {
            if let Some(e) = failed {
                println!(
                    "A node's view could not be read when the wait ended: {}",
                    short::<(), _>(&Err(e))
                );
            }
            return (views, equal);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Submits through `client`, as the host's submit does.
pub fn submit_via(
    client: &mut Client,
    agent: AgentId,
    methods: &Methods,
    input: &RecordedInput,
) -> Result<Delivery, HostError> {
    let arguments = submit_arguments(input)?;
    let answer: Submitted = host::answer(client.call(agent, INSTANCE, methods.submit, &arguments))?;
    host::delivery(input.clone(), answer)
}

/// What a set of acknowledged records needs of a view: each present, once, with the same bytes.
fn lost(acknowledged: &BTreeMap<String, (Vec<u8>, u64)>, view: &View) -> usize {
    let ids = view.record_ids();
    acknowledged
        .iter()
        .filter(|(id, (order, seq))| {
            ids.get(*id) != Some(&1)
                || !view
                    .journals
                    .get(order)
                    .is_some_and(|rows| rows.iter().any(|(s, _)| s == seq))
        })
        .count()
}

/// Records an acknowledged delivery: its record id, order and sequence number.
fn acknowledge(
    acknowledged: &mut BTreeMap<String, (Vec<u8>, u64)>,
    delivered: &Result<Delivery, HostError>,
) {
    if let Ok(Delivery::Recorded(r) | Delivery::AlreadyRecorded(r)) = delivered {
        acknowledged.insert(
            r.input.record_id.as_str().to_string(),
            (r.order_id.as_str().as_bytes().to_vec(), r.seq),
        );
    }
}

fn verdict(pass: bool) -> &'static str {
    if pass { "pass" } else { "fail" }
}

/// A fresh cluster with the program, one input submitted and every node made to run the program by a query of
/// its own: the pids that must name the program in their perf maps.
pub fn name_the_program_on_a_fresh_cluster(
    env: &Env,
    config: &ClusterConfig,
    base: &Path,
) -> Vec<u32> {
    let Ok(mut host) = fresh(env, config.clone(), base, "p") else {
        return Vec::new();
    };
    let input = probe::order_inputs("ord-proof", 0).remove(0);
    let _ = host.submit(input);
    let nodes: Vec<usize> = (1..=host.cluster.config.replicas).collect();
    let _ = converge(
        &host.cluster,
        &nodes,
        host.agent,
        &host.methods,
        Duration::from_secs(10),
    );
    host.cluster.pids().into_iter().map(|(_, p)| p).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// The byte-equality corpus

/// JSON values whose serialisation is easy to get wrong: floats, `-0`, exponents, large integers, and keys whose
/// UTF-8 byte order and UTF-16 code-unit order disagree.
pub fn corpus_values() -> Vec<(&'static str, String)> {
    vec![
        ("a float", "1.5".into()),
        ("a float that is not exact", "0.1".into()),
        ("a float with many digits", "123456789.123456789".into()),
        ("the smallest subnormal", "5e-324".into()),
        ("the largest double", "1.7976931348623157e308".into()),
        ("a small exponent", "1e-7".into()),
        (
            "1e21, where ECMAScript switches to exponents",
            "1e21".into(),
        ),
        ("an upper-case exponent with a sign", "2.5E+10".into()),
        ("a negative zero integer", "-0".into()),
        ("a negative zero float", "-0.0".into()),
        ("2^53 + 1", "9007199254740993".into()),
        ("u64's largest", "18446744073709551615".into()),
        ("i64's smallest", "-9223372036854775808".into()),
        ("past u64", "18446744073709551616".into()),
        ("an integer written as a float", "1.0".into()),
        (
            "keys out of UTF-16 order: U+E000 and U+1F600",
            "{\"\u{1F600}\":2,\"\u{E000}\":1,\"\u{e9}\":3,\"z\":4,\"a\":5}".into(),
        ),
        (
            "nested objects, keys unsorted",
            "{\"b\":{\"d\":1,\"c\":[{\"y\":0,\"x\":-0.0}]},\"a\":null}".into(),
        ),
        (
            "escapes",
            "{\"s\":\"\\u0000 \\u001f \\\" \\\\ / \\u2028 \\ud83d\\ude00\"}".into(),
        ),
        (
            "whitespace everywhere",
            " { \"k\" :\t[ 1 , 2.50 , true , false , null ] } ".into(),
        ),
    ]
}

/// Native canonical JSON, as the first run's adapter and this one's host write it for `contracts/`'s types:
/// serde_json's value model, written with no whitespace.
fn native_canonical(json: &str, as_input: bool) -> Result<Vec<u8>, String> {
    let value: serde_json::Value = if as_input {
        let input: RecordedInput = serde_json::from_str(json).map_err(|e| e.to_string())?;
        serde_json::to_value(&input).map_err(|e| e.to_string())?
    } else {
        serde_json::from_str(json).map_err(|e| e.to_string())?
    };
    serde_json::to_vec(&value).map_err(|e| e.to_string())
}

/// Whether every object in `value` has its keys in RFC 8785's order, by UTF-16 code units.
fn keys_in_utf16_order(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            let keys: Vec<Vec<u16>> = map.keys().map(|k| k.encode_utf16().collect()).collect();
            keys.windows(2).all(|w| w[0] <= w[1]) && map.values().all(keys_in_utf16_order)
        }
        serde_json::Value::Array(items) => items.iter().all(keys_in_utf16_order),
        _ => true,
    }
}

pub fn corpus(env: &Env, base: PathBuf) -> bool {
    let config = env.cluster(1, 0, false, env.backend());
    env.header("corpus", Some(&config));
    let host = match fresh(env, config.clone(), &base, "q") {
        Ok(host) => host,
        Err(e) => {
            println!("The host did not open: {e:?}");
            return false;
        }
    };
    let mut cases: Vec<(String, String, bool)> = corpus_values()
        .into_iter()
        .map(|(what, json)| (what.to_string(), json, false))
        .collect();
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../contracts/state-machine/fixtures/valid"
    );
    let mut fixtures: Vec<_> = std::fs::read_dir(dir)
        .expect("the fixtures")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("recorded-input-"))
        })
        .collect();
    fixtures.sort();
    for path in fixtures {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        cases.push((
            format!("fixture `{name}`"),
            std::fs::read_to_string(&path).unwrap(),
            true,
        ));
    }
    println!(
        "| Case | As | Native bytes | Guest bytes | Byte-equal | Keys in RFC 8785 (UTF-16) order |"
    );
    println!("|---|---|---|---|---|---|");
    let mut equal = 0;
    let mut utf16 = 0;
    for (what, json, as_input) in &cases {
        let native = native_canonical(json, *as_input);
        let guest = match host.canonical_of(json.as_bytes(), *as_input) {
            Ok(guest) => guest,
            Err(e) => Err(format!("{e:?}")),
        };
        let same = match (&native, &guest) {
            (Ok(a), Ok(b)) => a == b,
            (Err(_), Err(_)) => true,
            _ => false,
        };
        let ordered = native
            .as_ref()
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
            .is_none_or(|v| keys_in_utf16_order(&v));
        equal += usize::from(same);
        utf16 += usize::from(ordered);
        let show = |r: &Result<Vec<u8>, String>| match r {
            Ok(b) if b.len() <= 60 => {
                format!("`{}`", String::from_utf8_lossy(b).replace('|', "\\|"))
            }
            Ok(b) => format!("{} bytes", b.len()),
            Err(e) => format!(
                "refused: {}",
                e.chars().take(50).collect::<String>().replace('|', "\\|")
            ),
        };
        println!(
            "| {what} | {} | {} | {} | {} | {} |",
            if *as_input {
                "a recorded input"
            } else {
                "a JSON value"
            },
            show(&native),
            show(&guest),
            if same { "yes" } else { "**no**" },
            if ordered { "yes" } else { "**no**" }
        );
    }
    println!(
        "\n{equal} of {} cases byte-equal between the native build and the guest; {utf16} of {} keep RFC 8785's key order.",
        cases.len(),
        cases.len()
    );
    let pids: Vec<u32> = host.cluster.pids().into_iter().map(|(_, p)| p).collect();
    drop(host);
    let proved = backend_proof(env, config.backend, &pids);
    println!(
        "\nVerdict: {}.",
        if equal == cases.len() {
            "every case byte-equal"
        } else {
            "**bytes differ**"
        }
    );
    proved && equal == cases.len()
}

// ---------------------------------------------------------------------------------------------------------------
// R-01: the leader killed mid-submit, on three nodes

/// Submits inputs from a thread until told to stop, recording what was acknowledged and what was in flight.
struct Load {
    acknowledged: Arc<Mutex<BTreeMap<String, (Vec<u8>, u64)>>>,
    in_flight: Arc<Mutex<Option<RecordedInput>>>,
    stop: Arc<AtomicBool>,
    sent: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<Vec<RecordedInput>>>,
}

impl Load {
    fn start(
        cluster: &Cluster,
        agent: AgentId,
        methods: Methods,
        inputs: Vec<RecordedInput>,
    ) -> Load {
        let acknowledged = Arc::new(Mutex::new(BTreeMap::new()));
        let in_flight = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let sent = Arc::new(AtomicU64::new(0));
        let mut client = cluster.client_with([21; 32], 1);
        let (a, f, s, n) = (
            acknowledged.clone(),
            in_flight.clone(),
            stop.clone(),
            sent.clone(),
        );
        let thread = std::thread::spawn(move || {
            let mut unanswered = Vec::new();
            for input in inputs {
                if s.load(Ordering::SeqCst) {
                    break;
                }
                *f.lock().unwrap() = Some(input.clone());
                n.fetch_add(1, Ordering::SeqCst);
                let delivered = submit_via(&mut client, agent, &methods, &input);
                match &delivered {
                    Ok(_) => acknowledge(&mut a.lock().unwrap(), &delivered),
                    Err(HostError::RecordIdReused { .. }) => {}
                    Err(_) => unanswered.push(input),
                }
                *f.lock().unwrap() = None;
            }
            unanswered
        });
        Load {
            acknowledged,
            in_flight,
            stop,
            sent,
            thread: Some(thread),
        }
    }

    fn finish(mut self) -> Vec<RecordedInput> {
        self.stop.store(true, Ordering::SeqCst);
        self.thread
            .take()
            .map(|t| t.join().unwrap_or_default())
            .unwrap_or_default()
    }
}

/// A small seeded generator, so a run's kill points can be run again.
struct Random(u64);

impl Random {
    fn draw(&mut self, below: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % below.max(1)
    }
}

pub fn r01(env: &Env, base: PathBuf) -> bool {
    let config = env.cluster(3, 0, true, env.backend());
    env.header("r01", Some(&config));
    let mut host = match fresh(env, config.clone(), &base, "r") {
        Ok(host) => host,
        Err(e) => {
            println!("The host did not open: {e:?}");
            return false;
        }
    };
    let (agent, methods) = (host.agent, host.methods);
    let mut acknowledged = BTreeMap::new();
    let mut client = host.cluster.client_with([31; 32], 1);
    // Some records and a refusal first, so the refusal log has something to agree on.
    let warm = orders("ord-w", 4);
    for input in warm.iter().flatten() {
        let delivered = submit_via(&mut client, agent, &methods, input);
        acknowledge(&mut acknowledged, &delivered);
    }
    let _ = submit_via(&mut client, agent, &methods, &probe::reusing(&warm[0][1]));
    println!(
        "| Placement | Leader killed | In flight at the kill | Its fate | Resubmitted | Acknowledged lost | Nodes agree |"
    );
    println!("|---|---|---|---|---|---|---|");
    let mut all_ok = true;
    // The deterministic placement: every node holds its commits, the submit computes on the leader, the leader is
    // killed, and the followers' holds are lifted.
    {
        let input = probe::order_inputs("ord-held", 100).remove(0);
        let leader = host.cluster.leader().unwrap_or(1);
        let ok = host.cluster.set_faults_all("hold_commits").is_ok();
        let before = host.cluster.stage_count(leader, "refine");
        let mut sender = host.cluster.client_with([41; 32], leader);
        let sent = input.clone();
        let thread = std::thread::spawn(move || submit_via(&mut sender, agent, &methods, &sent));
        let until = Instant::now() + Duration::from_secs(20);
        while host.cluster.stage_count(leader, "refine") <= before && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        let computed = host.cluster.stage_count(leader, "refine") > before;
        host.cluster.kill(leader);
        for n in (1..=3).filter(|n| *n != leader) {
            let _ = host.cluster.set_faults(n, "");
        }
        let answered = thread.join().ok();
        if let Some(delivered) = &answered {
            acknowledge(&mut acknowledged, delivered);
        }
        let row = check_round(&mut host, leader, &input, &mut acknowledged, &mut client);
        println!(
            "| deterministic: `hold_commits` on all three, the leader killed once its refine counted the submit ({}) | {leader} | `{}` | {} | {} | {} | {} |",
            if ok && computed {
                "held and computed"
            } else {
                "**the hold or the compute was not seen**"
            },
            input.record_id.as_str(),
            row.fate,
            row.resubmitted,
            row.lost,
            row.agree
        );
        all_ok &= ok && computed && row.ok();
    }
    // Twenty random placements, under a stream of submits.
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let stream: Vec<RecordedInput> = orders("ord-r", 200).into_iter().flatten().collect();
    let mut rest = stream.as_slice();
    for round in 1..=20 {
        let take = rest.len().min(40);
        let (now, later) = rest.split_at(take);
        rest = later;
        let leader = host.cluster.leader().unwrap_or(1);
        let load = Load::start(&host.cluster, agent, methods, now.to_vec());
        let delay = random.draw(400);
        std::thread::sleep(Duration::from_millis(delay));
        let in_flight = load.in_flight.lock().unwrap().clone();
        host.cluster.kill(leader);
        let unanswered = load.finish_after_kill();
        acknowledged.extend(load_acknowledged(&unanswered.0));
        let input = in_flight.unwrap_or_else(|| now[0].clone());
        let row = check_round(&mut host, leader, &input, &mut acknowledged, &mut client);
        for left in unanswered.1 {
            let delivered = submit_via(&mut client, agent, &methods, &left);
            acknowledge(&mut acknowledged, &delivered);
        }
        println!(
            "| random {round}: after {delay} ms, {} submits sent | {leader} | `{}` | {} | {} | {} | {} |",
            unanswered.2,
            input.record_id.as_str(),
            row.fate,
            row.resubmitted,
            row.lost,
            row.agree
        );
        all_ok &= row.ok();
    }
    let (views, agree) = converge(
        &host.cluster,
        &[1, 2, 3],
        agent,
        &methods,
        Duration::from_secs(30),
    );
    let lost_at_end = views
        .first()
        .map_or(acknowledged.len(), |v| lost(&acknowledged, v));
    println!(
        "\nAt the end: {} acknowledged records, {lost_at_end} lost; {} records and {} refusals on each node; the three nodes {}.",
        acknowledged.len(),
        views.first().map_or(0, View::records),
        views.first().map_or(0, |v| v.refusals.len()),
        if agree {
            "agree, journals and refusal logs byte-equal"
        } else {
            "**disagree**"
        }
    );
    all_ok &= agree && lost_at_end == 0;
    println!("\nVerdict: {}.", verdict(all_ok));
    let pids: Vec<u32> = host.cluster.pids().into_iter().map(|(_, p)| p).collect();
    drop(host);
    backend_proof(env, config.backend, &pids)
}

impl Load {
    /// Ends the load right after a kill: the acknowledged records, the inputs left unanswered, and how long the
    /// load ran.
    fn finish_after_kill(self) -> (BTreeMap<String, (Vec<u8>, u64)>, Vec<RecordedInput>, u64) {
        let ran = self.sent.load(Ordering::SeqCst);
        let acknowledged = self.acknowledged.clone();
        let unanswered = self.finish();
        let acknowledged = acknowledged.lock().unwrap().clone();
        (acknowledged, unanswered, ran)
    }
}

fn load_acknowledged(map: &BTreeMap<String, (Vec<u8>, u64)>) -> BTreeMap<String, (Vec<u8>, u64)> {
    map.clone()
}

struct Round {
    fate: String,
    resubmitted: String,
    lost: usize,
    agree: String,
    good: bool,
}

impl Round {
    fn ok(&self) -> bool {
        self.good
    }
}

/// After a leader's kill: a new leader, the in-flight input's fate, its resubmission, the old leader back, and the
/// three nodes compared.
fn check_round(
    host: &mut RtHost,
    killed: usize,
    input: &RecordedInput,
    acknowledged: &mut BTreeMap<String, (Vec<u8>, u64)>,
    client: &mut Client,
) -> Round {
    let (agent, methods) = (host.agent, host.methods);
    let _ = host.cluster.leader();
    let survivor = (1..=3).find(|n| *n != killed).unwrap_or(1);
    let before = view(&host.cluster, survivor, agent, &methods).unwrap_or_default();
    let count = before
        .record_ids()
        .get(input.record_id.as_str())
        .copied()
        .unwrap_or(0);
    let fate = match count {
        0 => "absent".to_string(),
        1 => "recorded whole".to_string(),
        n => format!("**recorded {n} times**"),
    };
    let resubmit = submit_via(client, agent, &methods, input);
    acknowledge(acknowledged, &resubmit);
    let resubmitted = match (&resubmit, count) {
        (Ok(Delivery::AlreadyRecorded(_)), 1) => "already recorded".to_string(),
        (Ok(Delivery::Recorded(_)), 0) => "recorded once, now".to_string(),
        (Err(HostError::RecordIdReused { .. }), _) => {
            "refused as reused (another input under its id)".to_string()
        }
        (other, _) => format!("**{}**", short(other)),
    };
    let resubmit_ok = matches!(
        (&resubmit, count),
        (Ok(Delivery::AlreadyRecorded(_)), 1)
            | (Ok(Delivery::Recorded(_)), 0)
            | (Err(HostError::RecordIdReused { .. }), _)
    );
    let restarted = host.cluster.start(killed).is_ok();
    let (views, agree) = converge(
        &host.cluster,
        &[1, 2, 3],
        agent,
        &methods,
        Duration::from_secs(30),
    );
    let lost_now = views
        .first()
        .map_or(acknowledged.len(), |v| lost(acknowledged, v));
    let twice = views
        .first()
        .is_some_and(|v| v.record_ids().values().any(|c| *c > 1));
    Round {
        fate,
        resubmitted,
        lost: lost_now,
        agree: if agree && restarted && !twice {
            "yes".into()
        } else {
            "**no**".into()
        },
        good: count <= 1 && resubmit_ok && lost_now == 0 && agree && restarted && !twice,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// R-02 and R-03: a replica replaced from the survivors, with and without its data

fn fill(
    host: &mut RtHost,
    prefix: &str,
    count: usize,
    acknowledged: &mut BTreeMap<String, (Vec<u8>, u64)>,
) {
    let (agent, methods) = (host.agent, host.methods);
    let mut client = host.cluster.client_with([51; 32], 1);
    for input in orders(prefix, count).iter().flatten() {
        let delivered = submit_via(&mut client, agent, &methods, input);
        acknowledge(acknowledged, &delivered);
    }
}

pub fn r02(env: &Env, base: PathBuf) -> bool {
    let config = env.cluster(3, 1, false, env.backend());
    env.header("r02", Some(&config));
    let mut host = match fresh(env, config.clone(), &base, "a") {
        Ok(host) => host,
        Err(e) => {
            println!("The host did not open: {e:?}");
            return false;
        }
    };
    let mut acknowledged = BTreeMap::new();
    fill(&mut host, "ord-a", 20, &mut acknowledged);
    let leader = host.cluster.leader().unwrap_or(1);
    let gone = (1..=3).find(|n| *n != leader).unwrap();
    host.cluster.kill(gone);
    let started = Instant::now();
    let placed = host.cluster.replace(gone, 4);
    fill(&mut host, "ord-b", 5, &mut acknowledged);
    let nodes: Vec<usize> = (1..=4).filter(|n| *n != gone).collect();
    let (views, agree) = converge(
        &host.cluster,
        &nodes,
        host.agent,
        &host.methods,
        Duration::from_secs(60),
    );
    let caught_up = started.elapsed();
    let lost_now = views
        .first()
        .map_or(acknowledged.len(), |v| lost(&acknowledged, v));
    println!("| What | Value |\n|---|---|");
    println!(
        "| Replica replaced | node {gone}, a follower, killed first; the leader was node {leader} |"
    );
    println!(
        "| Its replacement | node 4, a key of its own made at its first run, no data copied |"
    );
    println!("| The replacement's answer | {} |", short(&placed));
    println!(
        "| Records acknowledged, before and after | {} |",
        acknowledged.len()
    );
    println!("| Nodes compared | {nodes:?} |");
    println!(
        "| Every journal and refusal log byte-equal across them | {} |",
        if agree { "yes" } else { "**no**" }
    );
    println!("| Acknowledged records lost | {lost_now} |");
    println!(
        "| From the replace to every node equal | {} ms |",
        caught_up.as_millis()
    );
    let ok = placed.is_ok() && agree && lost_now == 0;
    println!("\nVerdict: {}.", verdict(ok));
    let pids: Vec<u32> = host.cluster.pids().into_iter().map(|(_, p)| p).collect();
    drop(host);
    backend_proof(env, config.backend, &pids) && ok
}

pub fn r03(env: &Env, base: PathBuf) -> bool {
    let config = env.cluster(3, 2, false, env.backend());
    env.header("r03", Some(&config));
    let mut host = match fresh(env, config.clone(), &base, "l") {
        Ok(host) => host,
        Err(e) => {
            println!("The host did not open: {e:?}");
            return false;
        }
    };
    let mut acknowledged = BTreeMap::new();
    fill(&mut host, "ord-l", 20, &mut acknowledged);
    println!(
        "| Who lost its data | Node | Replaced by | The replacement's answer | Acknowledged | Lost | Nodes equal |"
    );
    println!("|---|---|---|---|---|---|---|");
    let mut ok = true;
    let mut replicas: Vec<usize> = vec![1, 2, 3];
    for (who, spare) in [("the leader", 4usize), ("a follower", 5usize)] {
        let leader = host.cluster.leader().unwrap_or(replicas[0]);
        let gone = if who == "the leader" {
            leader
        } else {
            *replicas.iter().find(|n| **n != leader).unwrap()
        };
        let lost_dir = host.cluster.lose(gone).is_ok();
        let _ = host.cluster.leader();
        let placed = host.cluster.replace(gone, spare);
        replicas.retain(|n| *n != gone);
        replicas.push(spare);
        fill(&mut host, &format!("ord-l{spare}-"), 3, &mut acknowledged);
        let (views, agree) = converge(
            &host.cluster,
            &replicas,
            host.agent,
            &host.methods,
            Duration::from_secs(60),
        );
        let lost_now = views
            .first()
            .map_or(acknowledged.len(), |v| lost(&acknowledged, v));
        println!(
            "| {who} | {gone} (data directory removed: {lost_dir}) | {spare} | {} | {} | {lost_now} | {} |",
            short(&placed),
            acknowledged.len(),
            if agree { "yes" } else { "**no**" }
        );
        ok &= placed.is_ok() && lost_now == 0 && agree && lost_dir;
    }
    println!(
        "\nNo store was copied: each replacement caught up from the survivors. Verdict: {}.",
        verdict(ok)
    );
    let pids: Vec<u32> = host.cluster.pids().into_iter().map(|(_, p)| p).collect();
    drop(host);
    backend_proof(env, config.backend, &pids) && ok
}

// ---------------------------------------------------------------------------------------------------------------
// R-04: the last batch damaged

/// The newest file of fjall's journal in node `n`'s store: where the last batch is.
fn last_journal(cluster: &Cluster, n: usize) -> Option<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "jnl")
                || path.parent().is_some_and(|p| p.ends_with("journals"))
            {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&cluster.data(n).join(vos_node::STORE), &mut files);
    files
        .into_iter()
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0))
        .max_by_key(|p| {
            (
                p.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.split('.').next())
                    .and_then(|n| n.parse::<u64>().ok())
                    .unwrap_or(0),
                std::fs::metadata(p).and_then(|m| m.modified()).ok(),
            )
        })
}

/// Damages the last batch of `file`: cuts its last 8 bytes (its trailer), or flips the bits of its last byte.
fn damage(file: &Path, how: &str) -> Result<u64, String> {
    let len = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    match how {
        "none" => {}
        "truncated" => {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(file)
                .map_err(|e| e.to_string())?;
            f.set_len(len.saturating_sub(8))
                .map_err(|e| e.to_string())?;
        }
        _ => {
            let mut bytes = std::fs::read(file).map_err(|e| e.to_string())?;
            if let Some(last) = bytes.last_mut() {
                *last ^= 0xff;
            }
            std::fs::write(file, bytes).map_err(|e| e.to_string())?;
        }
    }
    Ok(len)
}

/// Starts node `n` and says whether it opened: `Err` with the end of its log when it refused.
fn reopen(cluster: &mut Cluster, n: usize) -> Result<(), String> {
    cluster.start(n).map_err(|e| {
        e.chars()
            .rev()
            .take(200)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    })
}

pub fn r04a(env: &Env, base: PathBuf, replicas: usize) -> bool {
    let config = env.cluster(replicas, 0, false, env.backend());
    env.header(
        if replicas == 1 { "r04a-1" } else { "r04a-3" },
        Some(&config),
    );
    println!(
        "| Damage | Node | Journal file, bytes before | Opened | Acknowledged | Lost after the open | Caught up | Its replica after: term, commit, applied, last index (the leader's commit) | Result |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    let mut ok = true;
    let mut pids = Vec::new();
    // `none` is the control: the same kill and restart with no damage, so the harness's own reads are shown sound.
    for (i, how) in ["none", "truncated", "flipped"].into_iter().enumerate() {
        let mut host = match fresh(env, config.clone(), &base, &format!("d{i}-")) {
            Ok(host) => host,
            Err(e) => {
                println!("The host did not open: {e:?}");
                return false;
            }
        };
        let mut acknowledged = BTreeMap::new();
        fill(&mut host, &format!("ord-d{i}-"), 10, &mut acknowledged);
        let leader = host.cluster.leader().unwrap_or(1);
        let node = if replicas == 1 {
            1
        } else {
            (1..=3).find(|n| *n != leader).unwrap()
        };
        host.cluster.kill(node);
        let file = last_journal(&host.cluster, node);
        let before = file.as_ref().map(|f| damage(f, how));
        let opened = reopen(&mut host.cluster, node);
        // Idle first; then, if the node has not caught up, one more submit: a follower learns the commit index
        // from the leader's next entries, so an idle cluster can leave it behind.
        let mut idle = None;
        let (lost_now, caught) = if opened.is_ok() {
            let nodes: Vec<usize> = (1..=replicas).collect();
            let (mut views, mut agree) = converge(
                &host.cluster,
                &nodes,
                host.agent,
                &host.methods,
                Duration::from_secs(10),
            );
            idle = Some(agree);
            if !agree {
                let next = probe::order_inputs(&format!("ord-d{i}-next"), 0).remove(0);
                let delivered = host.submit(next);
                acknowledge(&mut acknowledged, &delivered);
                (views, agree) = converge(
                    &host.cluster,
                    &nodes,
                    host.agent,
                    &host.methods,
                    Duration::from_secs(30),
                );
            }
            let mine = view(&host.cluster, node, host.agent, &host.methods).unwrap_or_default();
            (lost(&acknowledged, &mine), agree && !views.is_empty())
        } else {
            (0, false)
        };
        let pass = match &opened {
            Err(_) => true,
            Ok(()) if replicas == 1 => lost_now == 0,
            Ok(()) => lost_now == 0 && caught,
        };
        if how != "none" {
            ok &= pass;
        }
        let leader_now = host.cluster.leader().unwrap_or(leader);
        let leader_commit = host.cluster.status(leader_now).map_or(0, |s| s.commit);
        let replica = host.cluster.status(node).map_or("unread".to_string(), |s| {
            format!(
                "{}, {}, {}, {} ({leader_commit})",
                s.term, s.commit, s.applied, s.last_index
            )
        });
        println!(
            "| {} | {node}{} | {} | {} | {} | {lost_now} | {} | {replica} | {} |",
            if how == "none" {
                "none (the control)".to_string()
            } else {
                format!("last batch {how}")
            },
            if replicas == 1 { "" } else { " (a follower)" },
            match (&file, &before) {
                (Some(f), Some(Ok(len))) =>
                    format!("`{}`, {len}", f.file_name().unwrap().to_string_lossy()),
                (_, Some(Err(e))) => format!("**not damaged: {e}**"),
                _ => "**no journal file found**".into(),
            },
            match &opened {
                Ok(()) => "yes".to_string(),
                Err(e) => format!("no, refused: {}", e.replace('|', "\\|")),
            },
            acknowledged.len(),
            match (replicas, idle, caught) {
                (1, _, _) => "n/a (one node)".to_string(),
                (_, Some(true), _) => "yes, idle".into(),
                (_, _, true) =>
                    "yes, after one more submit (idle, it waited behind the commit index)".into(),
                _ => "no".into(),
            },
            if pass {
                "pass"
            } else {
                "**fail: opened short**"
            }
        );
        pids.extend(host.cluster.pids().into_iter().map(|(_, p)| p));
        drop(host);
    }
    println!(
        "\nVerdict: {}. With a durable-index watermark: n/a, no such patch at vos-core 5c78975, and the experiment does not patch VOS.",
        verdict(ok)
    );
    backend_proof(env, config.backend, &pids) && ok
}

pub fn r04b(env: &Env, base: PathBuf) -> bool {
    let config = env.cluster(3, 0, false, env.backend());
    env.header("r04b", Some(&config));
    let mut host = match fresh(env, config.clone(), &base, "b") {
        Ok(host) => host,
        Err(e) => {
            println!("The host did not open: {e:?}");
            return false;
        }
    };
    let mut acknowledged = BTreeMap::new();
    fill(&mut host, "ord-pl", 10, &mut acknowledged);
    let leader = host.cluster.leader().unwrap_or(1);
    let follower = (1..=3).find(|n| *n != leader).unwrap();
    println!(
        "| Node | Before: commit, last index (its last acknowledged) | Journal file, bytes before | Opened |"
    );
    println!("|---|---|---|---|");
    let statuses: Vec<_> = [leader, follower]
        .iter()
        .map(|n| (*n, host.cluster.status(*n)))
        .collect();
    host.cluster.kill(leader);
    host.cluster.kill(follower);
    let mut opened = Vec::new();
    for (n, status) in statuses {
        let file = last_journal(&host.cluster, n);
        let before = file.as_ref().map(|f| damage(f, "truncated"));
        let open = reopen(&mut host.cluster, n);
        println!(
            "| {n}{} | {} | {} | {} |",
            if n == leader {
                " (leader)"
            } else {
                " (follower)"
            },
            status.map_or("unknown".into(), |s| format!(
                "{}, {}",
                s.commit, s.last_index
            )),
            match (&file, &before) {
                (Some(f), Some(Ok(len))) =>
                    format!("`{}`, {len}", f.file_name().unwrap().to_string_lossy()),
                _ => "**not damaged**".into(),
            },
            match &open {
                Ok(()) => "yes".to_string(),
                Err(e) => format!("no, refused: {}", e.replace('|', "\\|")),
            }
        );
        opened.push(open.is_ok());
    }
    let refused = opened.iter().any(|o| !o);
    let (views, agree) = converge(
        &host.cluster,
        &[1, 2, 3],
        host.agent,
        &host.methods,
        Duration::from_secs(60),
    );
    let lost_now = views
        .first()
        .map_or(acknowledged.len(), |v| lost(&acknowledged, v));
    println!(
        "\nAcknowledged: {}; lost after both reopened: {lost_now}; the nodes {}.",
        acknowledged.len(),
        if agree { "agree" } else { "disagree" }
    );
    let ok = refused || lost_now == 0;
    println!("\nVerdict: {}.", verdict(ok));
    let pids: Vec<u32> = host.cluster.pids().into_iter().map(|(_, p)| p).collect();
    drop(host);
    backend_proof(env, config.backend, &pids) && ok
}

// ---------------------------------------------------------------------------------------------------------------
// R-05: concurrent submitters

/// Latencies' p50 and p99, in microseconds.
pub fn percentiles(latencies: &mut [u64]) -> (u64, u64) {
    latencies.sort_unstable();
    let at = |q: f64| {
        latencies
            .get(((latencies.len() as f64 - 1.0) * q).round() as usize)
            .copied()
            .unwrap_or(0)
    };
    (at(0.50), at(0.99))
}

/// The total of a strace -c summary's calls, over every file in `dir` ending in `.strace`.
pub fn strace_calls(dir: &Path) -> (u64, String) {
    let mut total = 0;
    let mut by = BTreeMap::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.path().extension().is_none_or(|e| e != "strace") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
        for line in text.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            if let Some(name) = words.last()
                && ["fsync", "fdatasync", "sync_file_range"].contains(name)
                && words.len() >= 5
                && let Ok(calls) = words[3].parse::<u64>()
            {
                total += calls;
                *by.entry(name.to_string()).or_insert(0u64) += calls;
            }
        }
    }
    (total, format!("{by:?}"))
}

pub fn r05(env: &Env, base: PathBuf, replicas: usize, fsyncs: bool) -> bool {
    let shown = env.cluster(replicas, 0, false, env.backend());
    env.header(
        &format!("r05-{replicas}{}", if fsyncs { "-fsyncs" } else { "" }),
        Some(&shown),
    );
    let mut ok = true;
    let mut pids = Vec::new();
    println!(
        "| Candidate | Submitters | Records | Records/s | p50, µs | p99, µs | fsyncs/record | Guarantees under load |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    {
        for submitters in [1usize, 8, 64] {
            let mut config = env.cluster(replicas, 0, false, env.backend());
            let dir = base.join(format!("s{replicas}-{submitters}"));
            if fsyncs {
                std::fs::create_dir_all(&dir).unwrap();
                config.strace = Some(dir.clone());
            }
            let host = match fresh(env, config.clone(), &dir, "c") {
                Ok(host) => host,
                Err(e) => {
                    println!(
                        "| vos-rt{replicas} | {submitters} | the host did not open: {} |||||",
                        short::<(), _>(&Err(e))
                    );
                    ok = false;
                    continue;
                }
            };
            let per = (800 / submitters).max(14) / 7;
            let measured = load(&host, submitters, per);
            let (views, _) = converge(
                &host.cluster,
                &(1..=replicas).collect::<Vec<_>>(),
                host.agent,
                &host.methods,
                Duration::from_secs(30),
            );
            let holds = measured.guarantees(views.first());
            pids.extend(host.cluster.pids().into_iter().map(|(_, p)| p));
            let mut host = host;
            host.cluster.kill_all();
            let fsync = if fsyncs {
                let (calls, by) = strace_calls(&dir);
                format!(
                    "{:.2} ({calls} calls: {by})",
                    calls as f64 / measured.records.max(1) as f64
                )
            } else {
                format!("counted in `r05-{replicas}-fsyncs`")
            };
            let (p50, p99) = percentiles(&mut measured.latencies.clone());
            println!(
                "| vos-rt{replicas} | {submitters} | {} | {:.0} | {p50} | {p99} | {fsync} | {} |",
                measured.records,
                measured.records as f64 / measured.elapsed.as_secs_f64(),
                if holds { "hold" } else { "**broken**" }
            );
            ok &= holds;
        }
    }
    println!(
        "\nMeasured values on the runner's disk, comparing the candidates in one job, not either against production. Each submitter is a client of its own, with its own session, calling a node of its own first; under strace the nodes run slower, so `-fsyncs` passes count calls only."
    );
    let _ = pids;
    backend_proof(env, env.backend(), &[]) && ok
}

/// What a load measured.
pub struct Measured {
    pub records: usize,
    pub elapsed: Duration,
    pub latencies: Vec<u64>,
    pub submitted: Vec<Vec<RecordedInput>>,
    pub errors: usize,
}

impl Measured {
    /// Under load, every submitted order's journal is whole, in order, once per record id, and its states follow
    /// the probe's fold.
    pub fn guarantees(&self, view: Option<&View>) -> bool {
        let Some(view) = view else { return false };
        if self.errors > 0 || view.record_ids().values().any(|c| *c != 1) {
            return false;
        }
        self.submitted.iter().all(|inputs| {
            let order = inputs[0].subject.order.as_str().as_bytes().to_vec();
            let Some(rows) = view.journals.get(&order) else {
                return false;
            };
            let read: Vec<RecordedInput> = rows
                .iter()
                .filter_map(|(_, r)| serde_json::from_slice(&r.input).ok())
                .collect();
            let states: Vec<Option<rutent_contracts::state_machine::Order>> = rows
                .iter()
                .map(|(_, r)| {
                    r.state
                        .as_ref()
                        .and_then(|s| serde_json::from_slice(s).ok())
                })
                .collect();
            &read == inputs && states == probe::fold(&read)
        })
    }
}

/// `submitters` clients, each its own session, each submitting `per` orders' inputs one after another.
pub fn load(host: &RtHost, submitters: usize, per: usize) -> Measured {
    let (agent, methods) = (host.agent, host.methods);
    let started = Instant::now();
    let threads: Vec<_> = (0..submitters)
        .map(|t| {
            let mut client = host.cluster.client_with(
                [100u8.wrapping_add(t as u8); 32],
                1 + t % host.cluster.config.replicas,
            );
            let inputs = orders(&format!("ord-c{t}-"), per);
            std::thread::spawn(move || {
                let mut latencies = Vec::new();
                let mut errors = 0;
                for input in inputs.iter().flatten() {
                    let at = Instant::now();
                    match submit_via(&mut client, agent, &methods, input) {
                        Ok(_) => latencies.push(at.elapsed().as_micros() as u64),
                        Err(_) => errors += 1,
                    }
                }
                (latencies, errors, inputs)
            })
        })
        .collect();
    let mut measured = Measured {
        records: 0,
        elapsed: Duration::ZERO,
        latencies: Vec::new(),
        submitted: Vec::new(),
        errors: 0,
    };
    for thread in threads {
        let (latencies, errors, inputs) = thread.join().unwrap();
        measured.records += latencies.len();
        measured.latencies.extend(latencies);
        measured.errors += errors;
        measured.submitted.extend(inputs);
    }
    measured.elapsed = started.elapsed();
    measured
}

// ---------------------------------------------------------------------------------------------------------------
// R-06: the program upgraded over a running instance

pub fn r06a(env: &Env, base: PathBuf, replicas: usize) -> bool {
    let config = env.cluster(replicas, 0, false, env.backend());
    env.header(
        if replicas == 1 { "r06a-1" } else { "r06a-3" },
        Some(&config),
    );
    let mut host = match fresh(env, config.clone(), &base, "u") {
        Ok(host) => host,
        Err(e) => {
            println!("The host did not open: {e:?}");
            return false;
        }
    };
    let (agent, methods) = (host.agent, host.methods);
    let inputs = orders("ord-u", 6);
    let mut client = host.cluster.client_with([61; 32], 1);
    // Each order's first four inputs before the upgrade.
    for order in &inputs {
        for input in &order[..4] {
            let _ = submit_via(&mut client, agent, &methods, input);
        }
    }
    let before: Vec<_> = inputs
        .iter()
        .map(|o| Host::<Probe>::journal(&host, &o[0].subject.order).unwrap_or_default())
        .collect();
    let refusals_before = Host::<Probe>::refusals(&host).unwrap_or_default();
    let status_before = host.status().ok();
    let next = std::fs::read(&env.next_elf).unwrap_or_default();
    // The fifth input of order 0 in flight while the next version is installed.
    let in_flight = inputs[0][4].clone();
    let mut sender = host.cluster.client_with([62; 32], 1);
    let sent = in_flight.clone();
    let thread = std::thread::spawn(move || submit_via(&mut sender, agent, &methods, &sent));
    let installed = host.cluster.install(&next);
    let in_flight_answer = thread.join().ok();
    let methods_after = host::methods(&host.cluster, agent);
    if let Ok(m) = methods_after {
        host.methods = m;
    }
    let status_after = host.status().ok();
    let after: Vec<_> = inputs
        .iter()
        .map(|o| Host::<Probe>::journal(&host, &o[0].subject.order).unwrap_or_default())
        .collect();
    let survived = before.iter().zip(&after).enumerate().all(|(i, (b, a))| {
        if i == 0 {
            a.starts_with(b) && a.len() <= b.len() + 1
        } else {
            a == b
        }
    });
    let refusals_after = Host::<Probe>::refusals(&host).unwrap_or_default();
    let recorded = after[0]
        .iter()
        .filter(|r| r.input.record_id == in_flight.record_id)
        .count();
    let resubmitted = host.submit(in_flight.clone());
    // The next input of order 1 applies to the state from before the upgrade.
    let expected = before[1]
        .last()
        .and_then(|r| r.state_after.clone())
        .map(|s| Probe::transition(&s, &inputs[1][4]));
    let next_input = host.submit(inputs[1][4].clone());
    let applied = matches!(&next_input, Ok(Delivery::Recorded(r)) if r.state_after == expected);
    println!("| What | Value |\n|---|---|");
    println!(
        "| Before: program, format, records | {} |",
        status_before.map_or("unknown".into(), |s| format!(
            "{}, {}, {}",
            s.program, s.format, s.records
        ))
    );
    println!(
        "| The install of the next version over the running instance | {} |",
        short(&installed)
    );
    println!(
        "| The in-flight submit's answer | {} |",
        in_flight_answer.map_or("none".into(), |a| short(&a))
    );
    println!(
        "| After: program, format, records | {} |",
        status_after.as_ref().map_or("unknown".into(), |s| format!(
            "{}, {}, {}",
            s.program, s.format, s.records
        ))
    );
    println!(
        "| Journals and refusal log survived | {} |",
        if survived && refusals_after == refusals_before {
            "yes"
        } else {
            "**no**"
        }
    );
    println!("| The in-flight input recorded | {recorded} times |");
    println!("| Resubmitted | {} |", short(&resubmitted));
    println!(
        "| The next input applied to the state from before the upgrade | {} |",
        if applied { "yes" } else { "**no**" }
    );
    let upgraded = status_after.is_some_and(|s| s.program == 2);
    let ok = installed.is_ok()
        && upgraded
        && survived
        && refusals_after == refusals_before
        && recorded <= 1
        && applied;
    println!("\nVerdict: {}.", verdict(ok));
    let pids: Vec<u32> = host.cluster.pids().into_iter().map(|(_, p)| p).collect();
    drop(host);
    backend_proof(env, config.backend, &pids) && ok
}

/// Whether the program refused the call because its rows are in another format.
fn format_refused<T>(r: &Result<T, HostError>) -> bool {
    matches!(r, Err(HostError::Unavailable { reason }) if reason.contains("refuses them"))
}

pub fn r06b(env: &Env, base: PathBuf, replicas: usize) -> bool {
    let config = env.cluster(replicas, 0, false, env.backend());
    env.header(&format!("r06b-{replicas}"), Some(&config));
    let mut host = match fresh(env, config.clone(), &base, "f") {
        Ok(host) => host,
        Err(e) => {
            println!("The host did not open: {e:?}");
            return false;
        }
    };
    let inputs = orders("ord-f", 3);
    for input in inputs.iter().flatten().take(10) {
        let _ = host.submit(input.clone());
    }
    let journal_before =
        Host::<Probe>::journal(&host, &inputs[0][0].subject.order).unwrap_or_default();
    let next_format = std::fs::read(&env.next_format_elf).unwrap_or_default();
    let installed = host.cluster.install(&next_format);
    if let Ok(m) = host::methods(&host.cluster, host.agent) {
        host.methods = m;
    }
    let status = host.status();
    let submitted = host.submit(inputs[2][4].clone());
    let read = Host::<Probe>::journal(&host, &inputs[0][0].subject.order);
    let halted = Host::<Probe>::halt(&mut host);
    // The previous version back: it reads its rows again, unchanged.
    let previous = std::fs::read(&env.program_elf).unwrap_or_default();
    let back = host.cluster.install(&previous);
    if let Ok(m) = host::methods(&host.cluster, host.agent) {
        host.methods = m;
    }
    let journal_after =
        Host::<Probe>::journal(&host, &inputs[0][0].subject.order).unwrap_or_default();
    println!("| What | Value |\n|---|---|");
    println!(
        "| Installed: a version whose record rows gained a field, its format 2 | {} |",
        short(&installed)
    );
    println!(
        "| Its status: program, format it reads, format stored | {} |",
        status.as_ref().map_or("unknown".into(), |s| format!(
            "{}, {}, {}",
            s.program, s.format, s.stored_format
        ))
    );
    println!("| A submit | {} |", short(&submitted));
    println!("| A journal read | {} |", short(&read));
    println!("| A halt | {} |", short(&halted));
    println!("| The previous version installed back | {} |", short(&back));
    println!(
        "| The journal read again by it, equal to before | {} |",
        if journal_after == journal_before && !journal_before.is_empty() {
            "yes"
        } else {
            "**no**"
        }
    );
    let ok = installed.is_ok()
        && format_refused(&submitted)
        && format_refused(&read)
        && format_refused(&halted)
        && journal_after == journal_before;
    println!(
        "\nThe refusal is the program's own format check: at the tip, `Admin::Install` checks nothing (`docs/design/light-node.md:500-507`). Verdict: {}.",
        verdict(ok)
    );
    let pids: Vec<u32> = host.cluster.pids().into_iter().map(|(_, p)| p).collect();
    drop(host);
    backend_proof(env, config.backend, &pids) && ok
}

// ---------------------------------------------------------------------------------------------------------------
// R-07: across VOS commits

/// R-07a: the swap stream the first run's adapter exported from VOS at 1aadf44, as JSON lines in `R07_STREAM`,
/// imported into the tip program, and checked as `H-13` checks a swap.
pub fn r07a(env: &Env, base: PathBuf) -> bool {
    let config = env.cluster(1, 0, false, env.backend());
    env.header("r07a", Some(&config));
    let path = std::env::var("R07_STREAM").unwrap_or_default();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let entries: Vec<SnapshotEntry> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if entries.is_empty() {
        println!("No stream at `R07_STREAM` ({path}).");
        return false;
    }
    let mut harness = RtHarness::new(config.clone(), base.clone(), "m");
    let store: RtStore = harness.fresh().unwrap();
    let stream: SnapshotStream<'_> = Box::new(entries.clone().into_iter().map(Ok));
    let mut host = match RtHost::import(store, stream) {
        Ok(host) => host,
        Err(e) => {
            println!("The import was refused: {e:?}\n\nVerdict: fail.");
            return false;
        }
    };
    // Equal: every order's journal and the refusal log, as the stream holds them.
    let mut journals: BTreeMap<String, Vec<_>> = BTreeMap::new();
    let mut refusals = Vec::new();
    for entry in &entries {
        match entry {
            SnapshotEntry::Record(r) => journals
                .entry(r.order_id.as_str().to_string())
                .or_default()
                .push(r.clone()),
            SnapshotEntry::Refusal(r) => refusals.push(r.clone()),
            _ => {}
        }
    }
    let orders_read: BTreeSet<String> = Host::<Probe>::orders(&host)
        .unwrap_or_default()
        .iter()
        .map(|o| o.as_str().to_string())
        .collect();
    let journals_equal = journals.iter().all(|(order, records)| {
        rutent_contracts::state_machine::Id::new(order.clone())
            .and_then(|id| Host::<Probe>::journal(&host, &id).ok())
            .is_some_and(|j| &j == records)
    }) && orders_read == journals.keys().cloned().collect::<BTreeSet<_>>();
    let refusals_equal = Host::<Probe>::refusals(&host).unwrap_or_default() == refusals;
    let resumed = Host::<Probe>::resume(&mut host);
    // The same further inputs reach the same states: each order runs to its end on the new host.
    let mut runs_on = true;
    for records in journals.values() {
        let last = records.len();
        let order = records[0].order_id.as_str().to_string();
        let full = probe::order_inputs(&order, 0);
        let mut state = records.last().and_then(|r| r.state_after.clone());
        for input in full.iter().skip(last) {
            let mut input = input.clone();
            input.recorded_at = records[0].input.recorded_at.clone();
            let expected = match &state {
                Some(s) => Some(Probe::transition(s, &input)),
                None => Probe::open(&input),
            };
            match host.submit(input) {
                Ok(Delivery::Recorded(r)) if r.state_after == expected => state = r.state_after,
                _ => runs_on = false,
            }
        }
    }
    println!("| What | Value |\n|---|---|");
    println!(
        "| The stream | {} entries from the first run's adapter on VOS 1aadf44 (`R07_STREAM`) |",
        entries.len()
    );
    println!(
        "| Every order's journal equal, and `orders` lists each | {} |",
        if journals_equal { "yes" } else { "**no**" }
    );
    println!(
        "| The refusal log equal | {} |",
        if refusals_equal { "yes" } else { "**no**" }
    );
    println!("| Resumed | {} |", short(&resumed));
    println!(
        "| Further inputs reach the probe's states, each order to its end | {} |",
        if runs_on { "yes" } else { "**no**" }
    );
    let ok = journals_equal && refusals_equal && resumed.is_ok() && runs_on;
    println!("\nVerdict: {} (as `H-13`).", verdict(ok));
    let pids: Vec<u32> = host.cluster.pids().into_iter().map(|(_, p)| p).collect();
    drop(host);
    backend_proof(env, config.backend, &pids) && ok
}

/// R-07b: the first run's data directory, from VOS at 1aadf44 (`R07_DATA`), opened with the tip's node. Record
/// only: it passes when the node refuses it cleanly, or opens it with a journal byte-equal to the source.
pub fn r07b(env: &Env, base: PathBuf) -> bool {
    println!("#### Pass `r07b`, run {}\n", env.run_id);
    let source = PathBuf::from(std::env::var("R07_DATA").unwrap_or_default());
    let copy = base.join("pin-data");
    let copied = std::process::Command::new("cp")
        .arg("-R")
        .arg(&source)
        .arg(&copy)
        .status();
    let mut child = std::process::Command::new(&env.scored)
        .args(["node", "run", "--data"])
        .arg(&copy)
        .arg("--agent")
        .arg(&env.agent_elf)
        .env("VOS_PVM_BACKEND", env.backend().env())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("vos runs");
    let until = Instant::now() + Duration::from_secs(30);
    let mut exited = None;
    while Instant::now() < until {
        if let Ok(Some(status)) = child.try_wait() {
            exited = Some(status);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if exited.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let output = child.wait_with_output().ok();
    let stderr = output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stderr).into_owned())
        .unwrap_or_default();
    let stdout = output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let last: String = stderr
        .lines()
        .filter(|l| !l.contains("WARNING"))
        .last()
        .unwrap_or("")
        .chars()
        .take(300)
        .collect();
    println!("| What | Value |\n|---|---|");
    println!(
        "| The source | the data directory the first run's adapter left on VOS 1aadf44 (`R07_DATA`), copied: {} |",
        copied.is_ok_and(|s| s.success())
    );
    println!(
        "| The tip's node on it | {} |",
        match exited {
            Some(status) => format!("exited, {status}"),
            None => format!(
                "served (it printed `{}`), and was stopped",
                stdout.lines().last().unwrap_or("").replace('|', "\\|")
            ),
        }
    );
    println!(
        "| Its last line | `{}` |",
        last.replace('|', "\\|").replace('`', "'")
    );
    let refused = exited.is_some_and(|s| !s.success());
    println!(
        "\nVerdict: {}.",
        if refused {
            "pass: the tip's node refuses the pinned commit's store cleanly"
        } else {
            "**the node opened the pinned store: it must be read for a byte-equal journal, which this pass does not do, so it fails**"
        }
    );
    refused
}

// ---------------------------------------------------------------------------------------------------------------
// R-08: a timed halted export, then an import

pub fn r08(env: &Env, base: PathBuf, replicas: usize) -> bool {
    let config = env.cluster(replicas, 0, false, env.backend());
    env.header(if replicas == 1 { "r08-1" } else { "r08-3" }, Some(&config));
    println!(
        "| Records | Filled by | Fill, s | Halt to export done, ms | Import start to ready, ms | RTO, ms (their sum) | Imported equal | RPO |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    let mut ok = true;
    let mut pids = Vec::new();
    for records in env.r08_sizes() {
        let mut source = match fresh(env, config.clone(), &base, &format!("x{records}-")) {
            Ok(host) => host,
            Err(e) => {
                println!(
                    "| {records} | the host did not open: {} |||||||",
                    short::<(), _>(&Err(e))
                );
                ok = false;
                continue;
            }
        };
        let fill_started = Instant::now();
        let per = records / 7 / 64 + 1;
        let measured = load(&source, 64, per);
        let fill = fill_started.elapsed();
        let halt_started = Instant::now();
        let exported: Result<Vec<SnapshotEntry>, HostError> = Host::<Probe>::halt(&mut source)
            .and_then(|()| Host::<Probe>::export(&source))
            .and_then(|stream| stream.collect());
        let export = halt_started.elapsed();
        let Ok(entries) = exported else {
            println!(
                "| {records} | the export failed: {} |||||||",
                short(&exported)
            );
            ok = false;
            continue;
        };
        let mut harness = RtHarness::new(config.clone(), base.clone(), &format!("y{records}-"));
        let store = harness.fresh().unwrap();
        let import_started = Instant::now();
        let imported = RtHost::import(store, Box::new(entries.clone().into_iter().map(Ok)));
        let import = import_started.elapsed();
        let equal = match &imported {
            Ok(target) => Host::<Probe>::export(target)
                .and_then(|s| s.collect::<Result<Vec<_>, _>>())
                .is_ok_and(|again| again == entries),
            Err(_) => false,
        };
        println!(
            "| {} | 64 concurrent submitters, 7 inputs an order | {:.1} | {} | {} | {} | {} | the export interval: what was acknowledged since the last export |",
            measured.records,
            fill.as_secs_f64(),
            export.as_millis(),
            import.as_millis(),
            (export + import).as_millis(),
            if equal {
                "yes, the export of the import equals the export"
            } else {
                "**no**"
            }
        );
        ok &= equal;
        pids.extend(source.cluster.pids().into_iter().map(|(_, p)| p));
        if let Ok(target) = imported {
            pids.extend(target.cluster.pids().into_iter().map(|(_, p)| p));
        }
    }
    println!("\nMeasured values, `G-39`'s recovery figures; no target is set before `M4`.");
    backend_proof(env, config.backend, &pids) && ok
}

// ---------------------------------------------------------------------------------------------------------------
// R-09: gas and microseconds per step, the JIT against the interpreter

pub fn r09(env: &Env, base: PathBuf, replicas: usize) -> bool {
    let shown = env.cluster(replicas, 0, false, env.backend());
    env.header(&format!("r09-{replicas}"), Some(&shown));
    let mut ok = true;
    println!(
        "| Backend | Steps | `refine_gas` mean | `step_gas` mean | Of the 10M default | `refine` mean, µs | `batch` mean, µs | `commit` mean, µs |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    let legs: &[Backend] = match (env.dev, replicas) {
        (true, _) => &[Backend::Interpreter],
        (false, 1) => &[Backend::Jit, Backend::Interpreter],
        (false, _) => &[Backend::Jit],
    };
    for backend in legs {
        let config = env.cluster(replicas, 0, false, *backend);
        let host = match fresh(
            env,
            config.clone(),
            &base,
            if *backend == Backend::Jit { "j" } else { "i" },
        ) {
            Ok(host) => host,
            Err(e) => {
                println!(
                    "| {} | the host did not open: {} |||||||",
                    backend.env(),
                    short::<(), _>(&Err(e))
                );
                ok = false;
                continue;
            }
        };
        let measured = load(&host, 1, 30);
        let leader = host.cluster.leader().unwrap_or(1);
        let stages = host.cluster.stages(leader).unwrap_or_default();
        let mean = |name: &str| {
            stages
                .iter()
                .find(|(n, c, _)| n == name && *c > 0)
                .map_or(0, |(_, c, t)| t / c)
        };
        println!(
            "| `{}` | {} | {} | {} | {:.1}% | {} | {} | {} |",
            backend.env(),
            measured.records,
            mean("refine_gas"),
            mean("step_gas"),
            mean("refine_gas") as f64 / 100_000.0,
            mean("refine"),
            mean("batch"),
            mean("commit")
        );
        let pids: Vec<u32> = host.cluster.pids().into_iter().map(|(_, p)| p).collect();
        drop(host);
        ok &= backend_proof(env, *backend, &pids);
    }
    println!(
        "\nOne submitter, single-input batches, read from the leader's stats. Gas counts the program's handler and the agent's work on the input; the handler's own limit is 100M (`#[msg(gas = 100_000_000)]`), against VOS's 10M default (`agent/wire/src/defaults.rs:59-66`)."
    );
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: REQ-STM-4
    #[test]
    fn keys_sorted_by_their_utf8_bytes_leave_rfc_8785_order_for_a_supplementary_character() {
        let sorted: serde_json::Value =
            serde_json::from_str("{\"\u{1F600}\":2,\"\u{E000}\":1}").unwrap();
        // serde_json's map sorts U+E000 (EE 80 80) before U+1F600 (F0 9F 98 80); UTF-16 puts D83D first.
        assert!(!keys_in_utf16_order(&sorted));
        let ascii: serde_json::Value =
            serde_json::from_str("{\"b\":{\"d\":1,\"c\":2},\"a\":[]}").unwrap();
        assert!(keys_in_utf16_order(&ascii));
    }

    // verifies: REQ-STM-4
    #[test]
    fn every_recorded_input_fixture_reads_back_to_the_same_canonical_bytes_natively() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../../contracts/state-machine/fixtures/valid"
        );
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("recorded-input-") {
                continue;
            }
            let text = std::fs::read_to_string(entry.path()).unwrap();
            let once = native_canonical(&text, true).unwrap();
            let twice = native_canonical(std::str::from_utf8(&once).unwrap(), true).unwrap();
            assert_eq!(once, twice, "{name}");
        }
    }

    // verifies: DEC-6
    #[test]
    fn percentiles_take_the_median_and_the_ninety_ninth() {
        let mut latencies: Vec<u64> = (1..=100).rev().collect();
        assert_eq!(percentiles(&mut latencies), (51, 99));
        assert_eq!(percentiles(&mut []), (0, 0));
    }

    // verifies: DEC-6
    #[test]
    fn strace_summaries_are_summed_over_every_node_and_call() {
        let dir = std::env::temp_dir().join(format!("exp1-strace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let summary = "% time     seconds  usecs/call     calls    errors syscall\n\
                       ------ ----------- ----------- --------- --------- ----------------\n\
                        90.00    0.009000          10       900           fdatasync\n\
                        10.00    0.001000          10       100           fsync\n\
                       ------ ----------- ----------- --------- --------- ----------------\n\
                       100.00    0.010000          10      1000           total\n";
        std::fs::write(dir.join("node-1.strace"), summary).unwrap();
        std::fs::write(dir.join("node-2.strace"), summary).unwrap();
        std::fs::write(dir.join("node-1.log"), summary).unwrap();
        let (calls, _) = strace_calls(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(calls, 2000);
    }
}
