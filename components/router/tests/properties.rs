//! Property tests (`DEC-18`): the invariants the search carries, checked over networks drawn at random from a
//! fixed seed, so a failure names a seed that reproduces it.
//!
//! Each network has inner and outer nodes, two assets counted in units that differ from node to node, and reserves,
//! ramps and exchanges with random bands, fees (some with a maximum) and rates (some by levels, an order book's
//! depth); some prices are past their validity, and some routes are cut by the bounds. Every record must pass the
//! contract and the router suite's recompute, and keep the invariants below.

mod common;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use common::{Net, WIDE, bounds, endpoint, fee, outer, parse, request, routed, shortlist};
use rutent_conformance::router::recompute;
use rutent_contracts::router_api::{DecisionRecord, OperationKind, PolicyBounds, Request, Router};
use rutent_router::RouteFinder;
use rutent_router::shortlist::{Question, Refusal, Wanted, asks};
use serde_json::{Value, json};

/// splitmix64: a small, fixed generator, so the draws are the same on every machine.
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn one_in(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

const INNER: [&str; 4] = ["n0", "n1", "n2", "n3"];
const OUTER: [&str; 2] = ["chain-a", "chain-b"];
const ASSETS: [&str; 2] = ["USDC", "EURC"];
const TAKEN_AT: &str = "2026-11-06T15:00:00Z";

/// An endpoint as the generator keeps it: its node (inner or outer), asset and unit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct End {
    node: String,
    outer: bool,
    asset: &'static str,
    decimals: u8,
}

impl End {
    fn json(&self) -> Value {
        let node = if self.outer {
            outer(&self.node)
        } else {
            json!(self.node)
        };
        endpoint(node, self.asset, self.decimals)
    }
}

/// How a generated network is shaped.
#[derive(Clone, Copy)]
struct Shape {
    /// Fees, a band's minimum and rates other than one.
    priced: bool,
    exchanges: bool,
    /// Prices past their validity, exclusions and ceilings.
    faults: bool,
}

/// A generated network: its snapshot, its tuples as edges, and each tuple's band, by figure id.
struct Network {
    snapshot: rutent_contracts::router_api::CatalogueSnapshot,
    edges: Vec<(String, End, End)>,
    bands: BTreeMap<String, (u128, u128)>,
    stale: BTreeSet<String>,
    providers: Vec<String>,
}

fn network(draw: &mut Draw, shape: Shape) -> Network {
    // Each asset at each node in one unit, so the generated tuples connect.
    let mut units = BTreeMap::new();
    for node in INNER.iter().chain(OUTER.iter()) {
        for asset in ASSETS {
            units.insert((node.to_string(), asset), *draw.pick(&[2u8, 6, 18]));
        }
    }
    let end = |node: &str, outer: bool, asset: &'static str| End {
        node: node.to_owned(),
        outer,
        asset,
        decimals: units[&(node.to_owned(), asset)],
    };
    let mut net = Net::new("snap-gen", TAKEN_AT, 120);
    let mut edges = Vec::new();
    let mut bands = BTreeMap::new();
    let mut stale = BTreeSet::new();
    let count = 3 + draw.below(7);
    for t in 0..count {
        let id = format!("t{t:02}");
        let kinds: &[u8] = if shape.exchanges {
            &[0, 1, 2, 2, 3]
        } else {
            &[0, 1, 2, 2]
        };
        let kind = *draw.pick(kinds);
        let asset = *draw.pick(&ASSETS);
        let a = draw.pick(&INNER).to_string();
        let price_fee = if shape.priced {
            let mut f = fee(
                &draw.below(1000).to_string(),
                &draw.below(30).to_string(),
                "1000",
            );
            if draw.one_in(5) {
                f["maximum"] = json!(draw.below(5000).to_string());
            }
            f
        } else {
            fee("0", "0", "1")
        };
        let min: u128 = if shape.priced && draw.one_in(3) {
            10u128.pow(draw.below(12) as u32)
        } else {
            1
        };
        let max = if shape.priced && draw.one_in(4) {
            (min * 10u128.pow(1 + draw.below(14) as u32)).to_string()
        } else {
            WIDE.to_owned()
        };
        let band = (min.to_string(), max.clone());
        let band_ref = (band.0.as_str(), band.1.as_str());
        let mut price = json!({ "fee": price_fee, "settlement_seconds": 1 + draw.below(10_000) });
        let (from, to) = match kind {
            0 | 1 => {
                let o = draw.pick(&OUTER).to_string();
                let (from, to) = if kind == 0 {
                    (end(&o, true, asset), end(&a, false, asset))
                } else {
                    (end(&a, false, asset), end(&o, true, asset))
                };
                let node = |e: &End| {
                    if e.outer {
                        outer(&e.node)
                    } else {
                        json!(e.node)
                    }
                };
                net.reserve(
                    &id,
                    (node(&from), node(&to)),
                    asset,
                    (from.decimals, to.decimals),
                    band_ref,
                    price,
                );
                (from, to)
            }
            2 => {
                let b = loop {
                    let b = draw.pick(&INNER).to_string();
                    if b != a {
                        break b;
                    }
                };
                let (from, to) = (end(&a, false, asset), end(&b, false, asset));
                net.ramp(
                    &id,
                    (&a, &b),
                    asset,
                    (from.decimals, to.decimals),
                    band_ref,
                    price,
                );
                (from, to)
            }
            _ => {
                let other = if asset == ASSETS[0] {
                    ASSETS[1]
                } else {
                    ASSETS[0]
                };
                let (from, to) = (end(&a, false, asset), end(&a, false, other));
                let rate = |draw: &mut Draw| {
                    if shape.priced {
                        draw.pick(&["0.9", "1", "1.05", "0.97531", "1.2"])
                            .to_string()
                    } else {
                        "1".to_owned()
                    }
                };
                price["rate"] = if draw.one_in(2) {
                    json!({ "kind": "fixed", "rate": rate(draw) })
                } else {
                    let first = 10u128.pow(from.decimals as u32 + draw.below(4) as u32);
                    json!({ "kind": "levels", "levels": [
                        { "up_to": first.to_string(), "rate": rate(draw) },
                        { "up_to": (first * 100).to_string(), "rate": rate(draw) },
                    ] })
                };
                net.exchange(
                    &id,
                    &a,
                    (
                        common::unit(from.asset, from.decimals),
                        common::unit(to.asset, to.decimals),
                    ),
                    band_ref,
                    price,
                );
                (from, to)
            }
        };
        if shape.faults && draw.one_in(8) {
            let at = net.figures.len() - 1;
            net.figures[at]["valid_for_seconds"] = json!(draw.below(120));
            stale.insert(format!("{id}-price"));
        }
        bands.insert(id.clone(), (min, max.parse().unwrap_or(u128::MAX)));
        edges.push((id, from, to));
    }
    let providers = edges.iter().map(|(id, ..)| format!("p-{id}")).collect();
    Network {
        snapshot: net.build(),
        edges,
        bands,
        stale,
        providers,
    }
}

fn policy(draw: &mut Draw, network: &Network, shape: Shape) -> PolicyBounds {
    let mut bounds = serde_json::to_value(bounds(1 + draw.below(4) as u8)).unwrap();
    if shape.faults && draw.one_in(4) {
        bounds["excluded_providers"] = json!([draw.pick(&network.providers)]);
    }
    if shape.faults && draw.one_in(4) {
        let (id, from, _) = draw.pick(&network.edges);
        bounds["provider_ceilings"] = json!([{
            "provider": format!("p-{id}"),
            "asset": common::unit(from.asset, from.decimals),
            "amount": 10u128.pow(from.decimals as u32 + draw.below(4) as u32).to_string(),
        }]);
    }
    parse("bounds", bounds)
}

/// A request from where some tuple starts to where another ends, of some whole number of assets.
fn ask(draw: &mut Draw, network: &Network, seed: u64) -> Option<Request> {
    let from = draw.pick(&network.edges).1.clone();
    let to = draw.pick(&network.edges).2.clone();
    if from == to {
        return None;
    }
    let amount = 10u128.pow(from.decimals as u32) * (1 + u128::from(draw.below(1000)));
    Some(request(
        &format!("r-{seed}"),
        shortlist(),
        from.json(),
        to.json(),
        &amount.to_string(),
    ))
}

fn number(text: &str) -> u128 {
    text.parse().expect("generated amounts fit in 128 bits")
}

/// Every generated case, with its seed.
fn generated(count: u64, shape: Shape) -> Vec<(u64, Network, Request, PolicyBounds)> {
    let mut out = Vec::new();
    let mut seed = 0;
    while (out.len() as u64) < count {
        seed += 1;
        let mut draw = Draw(seed);
        let network = network(&mut draw, shape);
        let Some(request) = ask(&mut draw, &network, seed) else {
            continue;
        };
        let bounds = policy(&mut draw, &network, shape);
        out.push((seed, network, request, bounds));
    }
    out
}

const ANY: Shape = Shape {
    priced: true,
    exchanges: true,
    faults: true,
};

// verifies: AC-2, REQ-AUD-4, REQ-QTE-11, DEC-18
#[test]
fn every_record_passes_the_contract_and_recomputes_from_itself() {
    for (seed, network, request, bounds) in generated(300, ANY) {
        let record = routed(&request, &network.snapshot, &bounds);
        let recomputed = recompute(&record);
        assert!(
            recomputed.passes(),
            "seed {seed}: {:#?}",
            recomputed.findings
        );
    }
}

// verifies: AC-2, REQ-AUD-4, DEC-18
#[test]
fn same_inputs_always_give_the_same_record_whatever_the_order_of_advertised_figures() {
    for (seed, network, request, bounds) in generated(150, ANY) {
        let bytes = |r: &DecisionRecord| serde_json::to_vec(r).unwrap();
        let first = RouteFinder::route(&request, &network.snapshot, &bounds);
        let second = RouteFinder::route(&request, &network.snapshot, &bounds);
        assert_eq!(bytes(&first), bytes(&second), "seed {seed}");
        let replayed = RouteFinder::route(&first.request, &first.snapshot, &first.bounds);
        assert_eq!(bytes(&first), bytes(&replayed), "seed {seed}");
        let mut reversed = network.snapshot.clone();
        reversed.figures.reverse();
        let again = RouteFinder::route(&request, &reversed, &bounds);
        assert_eq!(again.candidates, first.candidates, "seed {seed}");
    }
}

// verifies: REQ-QTE-12, DEC-18
#[test]
fn every_segment_takes_what_the_one_before_delivered_and_a_kept_one_is_within_its_band() {
    for (seed, network, request, bounds) in generated(300, ANY) {
        let record = routed(&request, &network.snapshot, &bounds);
        for candidate in &record.candidates {
            let operations = &candidate.parts[0].operations;
            assert_eq!(
                operations.first().amount_in,
                request.amount,
                "seed {seed}: what leaves the origin is the request's amount"
            );
            for pair in operations.windows(2) {
                assert_eq!(
                    pair[1].amount_in.amount(),
                    &pair[0].amount_out,
                    "seed {seed}: {}",
                    candidate.candidate_id
                );
            }
            if candidate.dropped.is_some() {
                continue;
            }
            for operation in operations.iter() {
                let (min, max) = network.bands[operation.figures.first().as_str()];
                let amount = number(operation.amount_in.amount().as_str());
                assert!(
                    min <= amount && amount <= max,
                    "seed {seed}: {} enters {} outside its band",
                    amount,
                    operation.figures.first()
                );
            }
        }
    }
}

// verifies: REQ-QTE-4, DEC-18
#[test]
fn no_kept_candidate_uses_a_figure_past_its_validity() {
    let mut seen = 0;
    for (seed, network, request, bounds) in generated(300, ANY) {
        let record = routed(&request, &network.snapshot, &bounds);
        for candidate in &record.candidates {
            let cites_stale = candidate.parts[0]
                .operations
                .iter()
                .flat_map(|o| o.figures.iter())
                .any(|f| network.stale.contains(f.as_str()));
            if cites_stale {
                seen += 1;
                assert!(candidate.dropped.is_some(), "seed {seed}: {candidate:#?}");
            }
        }
    }
    assert!(
        seen > 0,
        "the draws should hold some figures past their validity"
    );
}

// verifies: REQ-QTE-8, DEC-18
#[test]
fn in_one_asset_and_unit_a_route_costs_exactly_the_sum_of_its_segments_fees() {
    let mut seen = 0;
    for (seed, network, request, bounds) in generated(300, ANY) {
        let record = routed(&request, &network.snapshot, &bounds);
        for candidate in &record.candidates {
            let operations = &candidate.parts[0].operations;
            let first = operations.first();
            if !operations
                .iter()
                .all(|o| o.from.asset == first.from.asset && o.to.asset == first.from.asset)
            {
                continue;
            }
            seen += 1;
            let fees: u128 = operations.iter().map(|o| number(&o.fee.amount)).sum();
            assert_eq!(
                number(first.amount_in.amount().as_str())
                    - number(operations.last().amount_out.as_str()),
                fees,
                "seed {seed}: {}",
                candidate.candidate_id
            );
        }
    }
    assert!(
        seen > 0,
        "the draws should hold some routes in one asset and unit"
    );
}

// verifies: REQ-QTE-14, DEC-18
#[test]
fn every_destination_the_tuples_reach_within_the_hop_limit_gets_a_route() {
    let free = Shape {
        priced: false,
        exchanges: false,
        faults: false,
    };
    let mut routes = 0;
    for (seed, network, _, bounds) in generated(150, free) {
        let hops = usize::from(bounds.max_hops.get());
        let ends: BTreeSet<End> = network
            .edges
            .iter()
            .flat_map(|(_, a, b)| [a.clone(), b.clone()])
            .collect();
        for origin in &ends {
            // Breadth first through the tuples: every end reachable from the origin in at most `hops` segments.
            let mut depth: BTreeMap<&End, usize> = BTreeMap::from([(origin, 0)]);
            let mut queue = VecDeque::from([origin]);
            while let Some(at) = queue.pop_front() {
                let d = depth[at];
                for (_, a, b) in &network.edges {
                    if a == at && d < hops && !depth.contains_key(b) {
                        depth.insert(b, d + 1);
                        queue.push_back(b);
                    }
                }
            }
            for destination in &ends {
                if destination == origin {
                    continue;
                }
                let amount = 100 * 10u128.pow(origin.decimals as u32);
                let request = request(
                    &format!("r-{seed}"),
                    shortlist(),
                    origin.json(),
                    destination.json(),
                    &amount.to_string(),
                );
                let record = routed(&request, &network.snapshot, &bounds);
                let reached = !record.shortlist().is_empty();
                assert_eq!(
                    reached,
                    depth.contains_key(destination),
                    "seed {seed}: {origin:?} to {destination:?}"
                );
                routes += usize::from(reached);
            }
        }
    }
    assert!(routes > 100, "only {routes} routes found");
}

// The shortlist (`T-011-02`).

/// The answer to one question, made at the shortlist round's `taken_at` and valid for five minutes: accepted on
/// the terms the shortlist round evaluated, or refused.
fn answer(id: &str, question: &Question, (fee, out): (&str, &str), refuse: bool) -> Value {
    let provider = format!("p-{}", question.tuple.as_str());
    let amount_in = question.amount_in.amount().as_str();
    let kind = match question.kind {
        OperationKind::Withdrawal => "withdrawal",
        OperationKind::Deposit => "deposit",
        OperationKind::Route => "transfer",
        OperationKind::Exchange => "conversion",
    };
    let mut outcome = json!({ "status": "refused", "refusal": { "class": "final" } });
    if !refuse {
        outcome = json!({
            "status": "dry_run_accepted",
            "amount_in": amount_in,
            "amount_out": out,
            "fee": { "asset": question.from.asset.id, "amount": fee },
            "valid_until": "2026-11-06T15:05:00Z",
            "voided_by": ["validity_passes", "amount_changes"],
            "settlement_seconds": 30,
        });
        if question.wants == Wanted::FreshQuote {
            outcome["direction"] = json!("exact_in");
        }
    }
    let body = json!({ "kind": kind, "transaction": format!("tx-{id}"), "outcome": outcome });
    let statement = Net::envelope(&provider, TAKEN_AT, "urn:rutent:provider-role:result", body);
    let value = if refuse || question.wants == Wanted::DryRun {
        json!({ "kind": "dry_run", "tuple": question.tuple, "amount_in": amount_in, "result": statement })
    } else {
        json!({ "kind": "fresh_quote", "tuple": question.tuple, "quote": statement })
    };
    let mut figure = json!({
        "figure_id": id,
        "origin": Net::origin(&provider),
        "age_seconds": 0,
        "value": value,
    });
    if !refuse {
        figure["valid_for_seconds"] = json!(300);
    }
    figure
}

/// Each kept operation's provider, tuple and amount in.
fn kept_operations(record: &DecisionRecord) -> BTreeSet<(String, String, String)> {
    record
        .shortlist()
        .iter()
        .flat_map(|c| c.parts[0].operations.iter())
        .map(|o| {
            (
                o.provider.as_str().to_owned(),
                o.figures.first().as_str().to_owned(),
                o.amount_in.amount().as_str().to_owned(),
            )
        })
        .collect()
}

// verifies: REQ-CAT-5, REQ-QTE-13, DEC-18
#[test]
fn only_providers_of_kept_candidates_are_asked_each_about_its_own_tuples_at_one_amount() {
    let mut refused = 0;
    for (seed, network, request, bounds) in generated(300, ANY) {
        let record = routed(&request, &network.snapshot, &bounds);
        let asked = match asks(&record) {
            Ok(asked) => asked,
            Err(refusal) => {
                assert_eq!(refusal, Refusal::NoRoute, "seed {seed}");
                assert!(record.shortlist().is_empty(), "seed {seed}");
                refused += 1;
                continue;
            }
        };
        let mut questions = BTreeSet::new();
        for provider in asked.providers() {
            let of = asked.of(provider);
            assert!(
                !of.is_empty(),
                "seed {seed}: {provider:?} is listed with nothing to ask"
            );
            for question in of {
                assert_eq!(
                    format!("p-{}", question.tuple.as_str()),
                    provider.as_str(),
                    "seed {seed}: a provider is asked about another's tuple"
                );
                assert_eq!(
                    question.wants,
                    Wanted::for_kind(question.kind),
                    "seed {seed}"
                );
                questions.insert((
                    provider.as_str().to_owned(),
                    question.tuple.as_str().to_owned(),
                    question.amount_in.amount().as_str().to_owned(),
                ));
            }
        }
        // Each tuple a kept operation takes is asked about once, at one of the amounts that reach it.
        let kept = kept_operations(&record);
        assert!(questions.is_subset(&kept), "seed {seed}");
        let tuples = |set: &BTreeSet<(String, String, String)>| -> Vec<(String, String)> {
            set.iter().map(|(p, t, _)| (p.clone(), t.clone())).collect()
        };
        let asked_tuples = tuples(&questions);
        let mut kept_tuples = tuples(&kept);
        kept_tuples.dedup();
        assert_eq!(
            asked_tuples, kept_tuples,
            "seed {seed}: a tuple is asked at two amounts or none"
        );
        for provider in &network.providers {
            let on_shortlist = questions.iter().any(|(p, ..)| p == provider);
            let provider = rutent_contracts::router_api::Id::new(provider.as_str()).unwrap();
            assert_eq!(!asked.of(&provider).is_empty(), on_shortlist, "seed {seed}");
        }
        let mut reversed = network.snapshot.clone();
        reversed.figures.reverse();
        let again = routed(&request, &reversed, &bounds);
        assert_eq!(asks(&again).ok(), Some(asked), "seed {seed}");
    }
    assert!(
        refused > 0,
        "the draws should hold some routings with nothing kept"
    );
}

// verifies: AC-3, REQ-CAT-14, REQ-QTE-13, DEC-18
#[test]
fn a_refused_option_is_never_chosen_and_a_fully_confirmed_one_always_leaves_a_choice() {
    let shape = Shape {
        priced: true,
        exchanges: false,
        faults: true,
    };
    let (mut chosen, mut refusals, mut unasked) = (0, 0, 0);
    for (seed, network, request, bounds) in generated(600, shape) {
        let record = routed(&request, &network.snapshot, &bounds);
        let Ok(asked) = asks(&record) else {
            continue;
        };
        let mut draw = Draw(seed ^ 0xA5A5);
        let terms: BTreeMap<(String, String), (String, String)> = record
            .shortlist()
            .iter()
            .flat_map(|c| c.parts[0].operations.iter())
            .map(|o| {
                (
                    (
                        o.figures.first().as_str().to_owned(),
                        o.amount_in.amount().as_str().to_owned(),
                    ),
                    (o.fee.amount.clone(), o.amount_out.as_str().to_owned()),
                )
            })
            .collect();
        let mut snapshot = serde_json::to_value(&network.snapshot).unwrap();
        snapshot["snapshot_id"] = json!("snap-gen-choice");
        let mut refused_tuples = BTreeSet::new();
        let figures = snapshot["figures"].as_array_mut().unwrap();
        for provider in asked.providers() {
            for (n, question) in asked.of(provider).iter().enumerate() {
                let key = (
                    question.tuple.as_str().to_owned(),
                    question.amount_in.amount().as_str().to_owned(),
                );
                let (fee, out) = &terms[&key];
                let refuse = draw.one_in(2);
                if refuse {
                    refused_tuples.insert(key.clone());
                    refusals += 1;
                }
                figures.push(answer(
                    &format!("a-{}-{n}", key.0),
                    question,
                    (fee, out),
                    refuse,
                ));
            }
        }
        // An answer from a provider no one asked never enters the choice round.
        for candidate in record.candidates.iter().filter(|c| c.dropped.is_some()) {
            let operation = candidate.parts[0].operations.first();
            if !asked.of(&operation.provider).is_empty() {
                continue;
            }
            let question = Question {
                tuple: operation.figures.first().clone(),
                kind: operation.kind,
                from: operation.from.clone(),
                to: operation.to.clone(),
                amount_in: operation.amount_in.clone(),
                wants: Wanted::for_kind(operation.kind),
            };
            let figure = answer(
                &format!("u-{}", candidate.candidate_id.as_str()),
                &question,
                (&operation.fee.amount, operation.amount_out.as_str()),
                false,
            );
            let parsed: rutent_contracts::router_api::Figure = parse("figure", figure);
            assert!(
                !asked.admits(&parsed),
                "seed {seed}: an unasked answer is admitted"
            );
            unasked += 1;
        }
        let snapshot: rutent_contracts::router_api::CatalogueSnapshot =
            parse("choice snapshot", snapshot);
        assert!(
            snapshot.figures.iter().all(|f| asked.admits(f)),
            "seed {seed}"
        );
        let mut choice = serde_json::to_value(&request).unwrap();
        choice["request_id"] = json!(format!("r-{seed}-choice"));
        choice["round"] = json!({
            "kind": "choice",
            "refines": { "request_id": request.request_id, "taken_at": TAKEN_AT },
        });
        let choice: Request = parse("choice request", choice);
        let decided = routed(&choice, &snapshot, &bounds);
        let recomputed = recompute(&decided);
        assert!(
            recomputed.passes(),
            "seed {seed}: {:#?}",
            recomputed.findings
        );
        let refused_in = |c: &rutent_contracts::router_api::Candidate| {
            c.parts[0].operations.iter().any(|o| {
                refused_tuples.contains(&(
                    o.figures.first().as_str().to_owned(),
                    o.amount_in.amount().as_str().to_owned(),
                ))
            })
        };
        // A shortlisted route each of whose operations was asked at its amount and accepted.
        let confirmable = record.shortlist().into_iter().any(|c| {
            !refused_in(c)
                && c.parts[0].operations.iter().all(|o| {
                    asked
                        .of(&o.provider)
                        .iter()
                        .any(|q| &q.tuple == o.figures.first() && q.amount_in == o.amount_in)
                })
        });
        match decided.chosen() {
            Some(route) => {
                chosen += 1;
                assert!(
                    !refused_in(route),
                    "seed {seed}: a refused option is chosen"
                );
                assert!(
                    number(route.parts[0].operations.last().amount_out.as_str()) > 0,
                    "seed {seed}: the choice arrives with nothing"
                );
            }
            None => assert!(
                !confirmable,
                "seed {seed}: a confirmed shortlisted route is not chosen"
            ),
        }
    }
    assert!(chosen > 50, "only {chosen} choices");
    assert!(refusals > 50, "only {refusals} refusals");
    assert!(
        unasked > 0,
        "the draws should hold some providers off the shortlist"
    );
}
