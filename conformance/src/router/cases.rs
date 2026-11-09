//! The suite's own inputs: requests, snapshots and bounds a router is run on, each with the route it must choose.
//!
//! The AC-2 network runs from a Colombian bank account (`co-ach`, COP in cents) to a wallet on Polygon (USDC,
//! 6 decimals), across a reserve, an exchange and a ramp, as `AC-2` asks:
//!
//! - `f-w`, a withdrawal at `reserve-bank-co` into `ledger-co`, fee 1/1000, 600 s;
//! - three exchanges at `ledger-co`, COP to USDC: `f-ex-fast` (fee 3/1000, fixed 0.00025, 60 s), `f-ex-slow` (fee
//!   1/1000, levels 0.000263 up to 333,333.33 COP then 0.0002517, 7,200 s), `f-ex-c` (fee 2/1000, 0.000252, 900 s);
//! - `f-ex-back`, an exchange at `ledger-co` from USDC back to COP at 4,100: through it, a route can come back to
//!   COP with more than it left with, a rate cycle that implies free profit;
//! - two ramps from `ledger-co` to `ledger-us`, USDC from 6 to 18 decimals: `f-ramp-x` (flat 1 USDC, 1,800 s) and
//!   `f-ramp-y` (1/100 with a maximum of 2 USDC, counted at the maximum, 300 s);
//! - two deposits from `ledger-us` to Polygon, USDC from 18 to 6 decimals: `f-dep` (1/2000, 120 s) and `f-dep-2`
//!   (free, 120 s).
//!
//! Worked by hand, for 1,000,000.00 COP (`100000000`):
//!
//! - `f-w`: fee ceil(100,000,000 / 1000) = 100,000, so 99,900,000 arrives.
//! - `f-ex-slow`: fee 99,900, net 99,800,100; 33,333,333 × 2.63 + 66,466,767 × 2.517 = 254,963,518.329, rounded
//!   down once to 254,963,518 USDC units (each level rounded first would give 254,963,517).
//! - `f-ex-fast`: fee 299,700, net 99,600,300 × 2.5 = 249,000,750.
//! - `f-ramp-x` on 254,963,518: fee 1,000,000, so 253,963,518 × 10^12 arrives.
//! - `f-ramp-y` on 249,000,750: fee 2,000,000 (the formula alone would say ceil(2,490,007.5) = 2,490,008), so
//!   247,000,750 × 10^12.
//! - `f-dep` on 253,963,518 × 10^12: fee 126,981,759 × 10^9, so floor(253,836,536.241) = 253,836,536 arrives;
//!   on 247,000,750 × 10^12: fee 123,500,375 × 10^9, so floor(246,877,249.625) = 246,877,249.
//!
//! So the slow route (`f-w`, `f-ex-slow`, `f-ramp-x`, `f-dep`) arrives with the most, 253.836536 USDC, in
//! 9,720 s, and the fast one (`f-w`, `f-ex-fast`, `f-ramp-y`, `f-dep`) with 246.877249 USDC in 1,080 s. In the
//! choice round, only those two are confirmed; `f-dep-2` refuses the slow route's amount (`AC-3`), and
//! `f-ex-c`'s quote is past its validity.

use rutent_contracts::router_api::{CatalogueSnapshot, PolicyBounds, Request};
use serde_json::{Value, json};

/// One input the suite routes, and what a router must choose on it.
#[derive(Debug, Clone)]
pub struct Case {
    /// The case's name, as reports print it.
    pub name: &'static str,
    /// The request.
    pub request: Request,
    /// The snapshot.
    pub snapshot: CatalogueSnapshot,
    /// The bounds.
    pub bounds: PolicyBounds,
    /// What the router must choose.
    pub expected: Expected,
    /// Whether the case is one of the AC-2 pair: the same snapshot and bounds under the two objectives.
    pub objective_pair: bool,
    /// Candidates the record must name, by the tuple figures they take, with the drop reason each must carry
    /// (`REQ-AUD-4`).
    pub drops: Vec<(Vec<&'static str>, &'static str)>,
}

/// What a router must choose on a case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    /// A shortlist round, which chooses nothing.
    Shortlist,
    /// A choice round that chooses the route through these tuple figures, in order.
    Route(Vec<&'static str>),
    /// A choice round with no route.
    NoRoute,
}

const DEPLOYMENT: &str = "rutent-test";
const OPERATOR_KEY: &str = "JddFbtX5xFFtDV_hZttVr9nJBVV9TlPxp99l7N3ZRlA";
const PROVIDER_KEY: &str = "DVP_bDfejcFXRBdFhgleHZTDfHjingJbVFhJlkpiLdE";
const SIGNATURE: &str =
    "lsrpbtB5rahgeQi2ugPWVTFXrjVELKI6MgYK5A_9vBVN_u10yk2KC0jqpnZr_3pYfecOg0seNUTRDVphTCJIGA";
const WIDE: &str = "1000000000000000000000000";

const SHORTLIST_AT: &str = "2026-11-06T14:59:00Z";
const CHOICE_AT: &str = "2026-11-06T15:00:00Z";
const MADE_AT: &str = "2026-11-06T14:59:55Z";
const VALID_UNTIL: &str = "2026-11-06T15:01:55Z";

fn parse<T: serde::de::DeserializeOwned>(what: &str, value: Value) -> T {
    serde_json::from_value(value).unwrap_or_else(|e| panic!("the suite's {what} is valid: {e}"))
}

fn outer(network: &str) -> Value {
    json!({ "network": network })
}

fn unit(id: &str, decimals: u8) -> Value {
    json!({ "id": id, "decimals": decimals })
}

fn terms(max: &str) -> Value {
    json!({ "bands": [{ "min": "1", "max": max }], "reporting": "net", "required_inputs": [] })
}

fn deployment() -> Value {
    json!({ "name": DEPLOYMENT, "operator_key": OPERATOR_KEY })
}

/// A snapshot under construction: advertised figures carry `age`, statements are made at `MADE_AT`.
struct Snapshot {
    id: &'static str,
    taken_at: &'static str,
    age: u64,
    figures: Vec<Value>,
}

/// A provider and its publisher.
type Party = (&'static str, &'static str);

impl Snapshot {
    fn new(id: &'static str, taken_at: &'static str, age: u64) -> Self {
        Snapshot {
            id,
            taken_at,
            age,
            figures: Vec::new(),
        }
    }

    fn origin((provider, publisher): Party) -> Value {
        json!({
            "kind": "provider",
            "provider": provider,
            "publisher": publisher,
            "provider_version": "1.0.0",
            "module_version": "1.0.0",
        })
    }

    fn advertised(&mut self, id: &str, by: Party, value: Value) {
        self.figures.push(json!({
            "figure_id": id,
            "origin": Self::origin(by),
            "age_seconds": self.age,
            "value": value,
        }));
    }

    /// A tuple and its price, `{id}` and `{id}-price`. `price` holds the fee, an optional rate and the settlement
    /// seconds.
    fn tuple(&mut self, id: &str, by: Party, kind: &str, tuple: Value, price: Value) {
        self.advertised(id, by, json!({ "kind": kind, "tuple": tuple }));
        let mut price = price;
        price["kind"] = json!("price");
        price["tuple"] = json!(id);
        self.advertised(&format!("{id}-price"), by, price);
    }

    /// A statement figure, with its age and validity in whole seconds: from its envelope's `made_at` to the
    /// snapshot's `taken_at`, rounded up, and to its `valid_until`, rounded down.
    fn statement(&mut self, id: &str, by: Party, timing: (u64, Option<u64>), value: Value) {
        let mut figure = json!({
            "figure_id": id,
            "origin": Self::origin(by),
            "age_seconds": timing.0,
            "value": value,
        });
        if let Some(valid) = timing.1 {
            figure["valid_for_seconds"] = json!(valid);
        }
        self.figures.push(figure);
    }

    fn envelope(maker: &str, made_at: &str, shape: &str, body: Value) -> Value {
        json!({
            "deployment": deployment(),
            "protocol_version": rutent_contracts::provider_role::VERSION,
            "shape": shape,
            "maker": maker,
            "made_at": made_at,
            "signature": { "scheme": "ed25519", "key": PROVIDER_KEY, "value": SIGNATURE },
            "statement": body,
        })
    }

    /// An accepted dry run of a reserve, `withdrawal` or `deposit`, for `tuple` at `amount_in`.
    #[allow(clippy::too_many_arguments)]
    fn reserve_answer(
        &mut self,
        id: &str,
        by: Party,
        kind: &str,
        tuple: &str,
        (amount_in, fee_asset, fee, amount_out): (&str, &str, &str, &str),
        settlement_seconds: u64,
    ) {
        let body = json!({
            "kind": kind,
            "transaction": format!("tx-{id}"),
            "outcome": {
                "status": "dry_run_accepted",
                "amount_in": amount_in,
                "amount_out": amount_out,
                "fee": { "asset": fee_asset, "amount": fee },
                "valid_until": VALID_UNTIL,
                "voided_by": ["validity_passes", "amount_changes"],
                "settlement_seconds": settlement_seconds,
            },
        });
        let value = json!({
            "kind": "dry_run",
            "tuple": tuple,
            "amount_in": amount_in,
            "result": Self::envelope(by.0, MADE_AT, "urn:rutent:provider-role:result", body),
        });
        self.statement(id, by, (5, Some(120)), value);
    }

    /// A refused dry run of a reserve for `tuple` at `amount_in`.
    fn reserve_refusal(&mut self, id: &str, by: Party, kind: &str, tuple: &str, amount_in: &str) {
        let body = json!({
            "kind": kind,
            "transaction": format!("tx-{id}"),
            "outcome": { "status": "refused", "refusal": { "class": "final" } },
        });
        let value = json!({
            "kind": "dry_run",
            "tuple": tuple,
            "amount_in": amount_in,
            "result": Self::envelope(by.0, MADE_AT, "urn:rutent:provider-role:result", body),
        });
        self.statement(id, by, (5, None), value);
    }

    /// A fresh quote: a ramp's transfer quote, or with `rate` an exchange's conversion quote.
    #[allow(clippy::too_many_arguments)]
    fn quote(
        &mut self,
        id: &str,
        by: Party,
        tuple: &str,
        (amount_in, fee_asset, fee, amount_out): (&str, &str, &str, &str),
        rate: Option<&str>,
        settlement_seconds: u64,
        (made_at, valid_until, timing): (&str, &str, (u64, Option<u64>)),
    ) {
        let mut outcome = json!({
            "status": "dry_run_accepted",
            "amount_in": amount_in,
            "amount_out": amount_out,
            "fee": { "asset": fee_asset, "amount": fee },
            "direction": "exact_in",
            "valid_until": valid_until,
            "voided_by": ["validity_passes", "amount_changes"],
            "settlement_seconds": settlement_seconds,
        });
        let kind = match rate {
            Some(rate) => {
                outcome["rate"] = json!(rate);
                outcome["slippage"] = json!({ "regime": "firm" });
                "conversion"
            }
            None => "transfer",
        };
        let body = json!({ "kind": kind, "transaction": format!("tx-{id}"), "outcome": outcome });
        let value = json!({
            "kind": "fresh_quote",
            "tuple": tuple,
            "quote": Self::envelope(by.0, made_at, "urn:rutent:provider-role:result", body),
        });
        self.statement(id, by, timing, value);
    }

    fn eligible(&mut self, id: &str, by: Party, tuple: &str, subject: &str) {
        let body = json!({
            "subject": subject,
            "class": "kyc.natural_person",
            "answer": "yes",
            "valid_until": "2026-11-13T14:59:55Z",
        });
        let value = json!({
            "kind": "eligibility",
            "tuple": tuple,
            "judgement": Self::envelope(by.0, MADE_AT, "urn:rutent:provider-role:judgement", body),
        });
        self.statement(id, by, (5, Some(604_800)), value);
    }

    fn build(self) -> CatalogueSnapshot {
        parse(
            "snapshot",
            json!({ "snapshot_id": self.id, "taken_at": self.taken_at, "figures": self.figures }),
        )
    }
}

const RESERVE_BANK: Party = ("reserve-bank-co", "bank-co-group");
const EXCHANGE_FAST: Party = ("exchange-fast", "acme");
const EXCHANGE_SLOW: Party = ("exchange-slow", "slow-co");
const EXCHANGE_C: Party = ("exchange-c", "acme");
const EXCHANGE_LOOP: Party = ("exchange-loop", "loop-co");
const RAMP_X: Party = ("ramp-x", "ramp-x");
const RAMP_Y: Party = ("ramp-y", "ramp-y");
const RESERVE_WALLET: Party = ("reserve-wallet", "wallet-co");
const RESERVE_WALLET_2: Party = ("reserve-wallet-2", "wallet-co");
const VERIFIER: Party = ("verifier-kyc", "kyc-co");

fn fee(flat: &str, numerator: &str, denominator: &str) -> Value {
    json!({ "flat": flat, "proportional": { "numerator": numerator, "denominator": denominator } })
}

fn withdrawal(s: &mut Snapshot) {
    s.tuple(
        "f-w",
        RESERVE_BANK,
        "reserve_tuple",
        json!({
            "origin": outer("co-ach"), "destination": "ledger-co", "asset": "COP",
            "decimals": { "origin": 2, "destination": 2 }, "terms": terms(WIDE),
        }),
        json!({ "fee": fee("0", "1", "1000"), "settlement_seconds": 600 }),
    );
}

fn exchange(s: &mut Snapshot, id: &str, by: Party, (from, to): (Value, Value), price: Value) {
    s.tuple(
        id,
        by,
        "exchange_tuple",
        json!({ "node": "ledger-co", "from": from, "to": to, "terms": terms(WIDE) }),
        price,
    );
}

fn cop_to_usdc() -> (Value, Value) {
    (unit("COP", 2), unit("USDC", 6))
}

fn ramp(s: &mut Snapshot, id: &str, by: Party, max: &str, price: Value) {
    s.tuple(
        id,
        by,
        "ramp_tuple",
        json!({
            "origin": "ledger-co", "destination": "ledger-us", "asset": "USDC",
            "decimals": { "origin": 6, "destination": 18 }, "terms": terms(max),
        }),
        price,
    );
}

fn deposit(s: &mut Snapshot, id: &str, by: Party, price: Value) {
    s.tuple(
        id,
        by,
        "reserve_tuple",
        json!({
            "origin": "ledger-us", "destination": outer("polygon"), "asset": "USDC",
            "decimals": { "origin": 18, "destination": 6 }, "terms": terms(WIDE),
        }),
        price,
    );
}

/// The AC-2 network's advertised figures.
fn ac2_advertised(s: &mut Snapshot) {
    withdrawal(s);
    exchange(
        s,
        "f-ex-fast",
        EXCHANGE_FAST,
        cop_to_usdc(),
        json!({ "fee": fee("0", "3", "1000"), "rate": { "kind": "fixed", "rate": "0.00025" }, "settlement_seconds": 60 }),
    );
    exchange(
        s,
        "f-ex-slow",
        EXCHANGE_SLOW,
        cop_to_usdc(),
        json!({
            "fee": fee("0", "1", "1000"),
            "rate": { "kind": "levels", "levels": [
                { "up_to": "33333333", "rate": "0.000263" },
                { "up_to": "1000000000", "rate": "0.0002517" },
            ] },
            "settlement_seconds": 7200,
        }),
    );
    exchange(
        s,
        "f-ex-c",
        EXCHANGE_C,
        cop_to_usdc(),
        json!({ "fee": fee("0", "2", "1000"), "rate": { "kind": "fixed", "rate": "0.000252" }, "settlement_seconds": 900 }),
    );
    exchange(
        s,
        "f-ex-back",
        EXCHANGE_LOOP,
        (unit("USDC", 6), unit("COP", 2)),
        json!({ "fee": fee("0", "0", "1"), "rate": { "kind": "fixed", "rate": "4100" }, "settlement_seconds": 60 }),
    );
    ramp(
        s,
        "f-ramp-x",
        RAMP_X,
        WIDE,
        json!({ "fee": fee("1000000", "0", "1"), "settlement_seconds": 1800 }),
    );
    let mut capped = fee("0", "1", "100");
    capped["maximum"] = json!("2000000");
    ramp(
        s,
        "f-ramp-y",
        RAMP_Y,
        WIDE,
        json!({ "fee": capped, "settlement_seconds": 300 }),
    );
    deposit(
        s,
        "f-dep",
        RESERVE_WALLET,
        json!({ "fee": fee("0", "1", "2000"), "settlement_seconds": 120 }),
    );
    deposit(
        s,
        "f-dep-2",
        RESERVE_WALLET_2,
        json!({ "fee": fee("0", "0", "1"), "settlement_seconds": 120 }),
    );
}

/// The AC-2 network as the shortlist round reads it: advertised figures only.
fn ac2_shortlist_snapshot() -> CatalogueSnapshot {
    let mut s = Snapshot::new("snap-ac2-shortlist", SHORTLIST_AT, 3540);
    ac2_advertised(&mut s);
    s.build()
}

/// The AC-2 network refreshed for the choice round: the advertised figures, a minute older, and the shortlisted
/// providers' answers.
fn ac2_choice_snapshot() -> CatalogueSnapshot {
    let mut s = Snapshot::new("snap-ac2-choice", CHOICE_AT, 3600);
    ac2_advertised(&mut s);
    let now = (MADE_AT, VALID_UNTIL, (5, Some(120)));
    s.reserve_answer(
        "f-w-dry-run",
        RESERVE_BANK,
        "withdrawal",
        "f-w",
        ("100000000", "COP", "100000", "99900000"),
        600,
    );
    s.quote(
        "f-ex-slow-quote",
        EXCHANGE_SLOW,
        "f-ex-slow",
        ("99900000", "COP", "99900", "254963518"),
        Some("0.0002552"),
        7200,
        now,
    );
    s.eligible("f-ex-slow-eligibility", VERIFIER, "f-ex-slow", "subj-42");
    s.quote(
        "f-ex-fast-quote",
        EXCHANGE_FAST,
        "f-ex-fast",
        ("99900000", "COP", "299700", "249000750"),
        Some("0.00025"),
        60,
        now,
    );
    // Past its validity: made ten minutes before the snapshot, valid for two.
    s.quote(
        "f-ex-c-quote",
        EXCHANGE_C,
        "f-ex-c",
        ("99900000", "COP", "199800", "251244504"),
        Some("0.000252"),
        900,
        (
            "2026-11-06T14:50:00Z",
            "2026-11-06T14:52:00Z",
            (600, Some(120)),
        ),
    );
    s.quote(
        "f-ramp-x-quote",
        RAMP_X,
        "f-ramp-x",
        ("254963518", "USDC", "1000000", "253963518000000000000"),
        None,
        1800,
        now,
    );
    s.quote(
        "f-ramp-y-quote",
        RAMP_Y,
        "f-ramp-y",
        ("249000750", "USDC", "2000000", "247000750000000000000"),
        None,
        300,
        now,
    );
    s.reserve_answer(
        "f-dep-dry-run-slow",
        RESERVE_WALLET,
        "deposit",
        "f-dep",
        (
            "253963518000000000000",
            "USDC",
            "126981759000000000",
            "253836536",
        ),
        120,
    );
    s.reserve_answer(
        "f-dep-dry-run-fast",
        RESERVE_WALLET,
        "deposit",
        "f-dep",
        (
            "247000750000000000000",
            "USDC",
            "123500375000000000",
            "246877249",
        ),
        120,
    );
    // The cheaper deposit refuses the slow route's amount, so that route goes through f-dep (AC-3).
    s.reserve_refusal(
        "f-dep-2-dry-run-refused",
        RESERVE_WALLET_2,
        "deposit",
        "f-dep-2",
        "253963518000000000000",
    );
    s.build()
}

fn bounds(max_hops: u8, provider_ceilings: Value, excluded_providers: Value) -> PolicyBounds {
    parse(
        "bounds",
        json!({
            "deployment": deployment(),
            "protocol_version": rutent_contracts::provider_role::VERSION,
            "policy_version": "policy-1",
            "max_hops": max_hops,
            "max_parts": 1,
            "excluded_providers": excluded_providers,
            "excluded_tuples": [],
            "provider_ceilings": provider_ceilings,
            "publisher_ceilings": [],
        }),
    )
}

fn request(id: &str, round: Value, to: Value, objective: &str, floor: Option<&str>) -> Request {
    let mut request = json!({
        "request_id": id,
        "round": round,
        "from": { "node": outer("co-ach"), "asset": unit("COP", 2) },
        "to": to,
        "amount": "100000000",
        "objective": { "kind": objective },
    });
    if let Some(floor) = floor {
        request["floor"] = json!(floor);
    }
    parse("request", request)
}

fn to_polygon() -> Value {
    json!({ "node": outer("polygon"), "asset": unit("USDC", 6) })
}

/// A choice round refining the shortlist round `shortlist`, whose snapshot was taken at `SHORTLIST_AT`.
fn choice_of(shortlist: &str) -> Value {
    json!({ "kind": "choice", "refines": { "request_id": shortlist, "taken_at": SHORTLIST_AT } })
}

const SLOW_ROUTE: [&str; 4] = ["f-w", "f-ex-slow", "f-ramp-x", "f-dep"];
const FAST_ROUTE: [&str; 4] = ["f-w", "f-ex-fast", "f-ramp-y", "f-dep"];

/// The policy network, a shortlist round over advertised figures: an excluded exchange, an exchange whose USDC
/// output passes a ceiling at it, a ramp whose band stops short, a deposit that keeps everything, a floor, an
/// exchange with no price and a ramp whose fee is above what reaches it.
fn policy_case() -> Case {
    let mut s = Snapshot::new("snap-policy", SHORTLIST_AT, 3540);
    withdrawal(&mut s);
    let fixed = |rate: &str| json!({ "fee": fee("0", "3", "1000"), "rate": { "kind": "fixed", "rate": rate }, "settlement_seconds": 60 });
    exchange(
        &mut s,
        "f-ex-fast",
        EXCHANGE_FAST,
        cop_to_usdc(),
        fixed("0.00025"),
    );
    exchange(
        &mut s,
        "f-ex-banned",
        ("exchange-banned", "banned-co"),
        cop_to_usdc(),
        fixed("0.0003"),
    );
    exchange(
        &mut s,
        "f-ex-capped",
        ("exchange-capped", "capped-co"),
        cop_to_usdc(),
        fixed("0.00028"),
    );
    ramp(
        &mut s,
        "f-ramp-x",
        RAMP_X,
        WIDE,
        json!({ "fee": fee("1000000", "0", "1"), "settlement_seconds": 1800 }),
    );
    let mut capped = fee("0", "1", "100");
    capped["maximum"] = json!("2000000");
    ramp(
        &mut s,
        "f-ramp-y",
        RAMP_Y,
        WIDE,
        json!({ "fee": capped, "settlement_seconds": 300 }),
    );
    // Its book is 500,000.00 COP deep, short of the 996,003.00 COP net it would be asked: beyond its levels.
    exchange(
        &mut s,
        "f-ex-shallow",
        ("exchange-shallow", "shallow-co"),
        cop_to_usdc(),
        json!({
            "fee": fee("0", "3", "1000"),
            "rate": { "kind": "levels", "levels": [{ "up_to": "50000000", "rate": "0.00025" }] },
            "settlement_seconds": 60,
        }),
    );
    ramp(
        &mut s,
        "f-ramp-narrow",
        ("ramp-narrow", "ramp-narrow"),
        "100000000",
        json!({ "fee": fee("0", "0", "1"), "settlement_seconds": 60 }),
    );
    deposit(
        &mut s,
        "f-dep",
        RESERVE_WALLET,
        json!({ "fee": fee("0", "1", "2000"), "settlement_seconds": 120 }),
    );
    deposit(
        &mut s,
        "f-dep-greedy",
        ("reserve-greedy", "greedy-co"),
        json!({ "fee": fee("0", "1", "1"), "settlement_seconds": 120 }),
    );
    // Two tuples no operation can state, which the record still names (`REQ-AUD-4`, router-api 3.0.0): an
    // exchange advertised with no price, and a ramp whose flat 300 USDC fee is above the about 249 USDC that
    // reaches it.
    let unpriced = ("exchange-unpriced", "unpriced-co");
    s.advertised(
        "f-ex-unpriced",
        unpriced,
        json!({
            "kind": "exchange_tuple",
            "tuple": { "node": "ledger-co", "from": unit("COP", 2), "to": unit("USDC", 6), "terms": terms(WIDE) },
        }),
    );
    ramp(
        &mut s,
        "f-ramp-dear",
        ("ramp-dear", "ramp-dear"),
        WIDE,
        json!({ "fee": fee("300000000", "0", "1"), "settlement_seconds": 60 }),
    );
    // The capped exchange would produce about 278.88 USDC; its ceiling is 200 USDC in flight there.
    let ceilings =
        json!([{ "provider": "exchange-capped", "asset": unit("USDC", 6), "amount": "200000000" }]);
    Case {
        name: "policy-shortlist",
        request: request(
            "req-policy-shortlist",
            json!({ "kind": "shortlist" }),
            to_polygon(),
            "lowest_total_cost",
            Some("247000000"),
        ),
        snapshot: s.build(),
        bounds: bounds(4, ceilings, json!(["exchange-banned"])),
        expected: Expected::Shortlist,
        objective_pair: false,
        drops: vec![
            (vec!["f-w", "f-ex-unpriced"], "no_price"),
            (vec!["f-w", "f-ex-fast", "f-ramp-dear"], "fee_above_amount"),
        ],
    }
}

/// Two exchanges with the same price, so the two routes tie under either objective; the lower `candidate_id`
/// must win. The destination is the inner ledger itself.
fn tie_case() -> Case {
    let mut s = Snapshot::new("snap-tie", CHOICE_AT, 3600);
    withdrawal(&mut s);
    let same = json!({ "fee": fee("0", "2", "1000"), "rate": { "kind": "fixed", "rate": "0.00025" }, "settlement_seconds": 60 });
    exchange(
        &mut s,
        "f-ex-t1",
        ("exchange-t1", "t1-co"),
        cop_to_usdc(),
        same.clone(),
    );
    exchange(
        &mut s,
        "f-ex-t2",
        ("exchange-t2", "t2-co"),
        cop_to_usdc(),
        same,
    );
    let now = (MADE_AT, VALID_UNTIL, (5, Some(120)));
    s.reserve_answer(
        "f-w-dry-run",
        RESERVE_BANK,
        "withdrawal",
        "f-w",
        ("100000000", "COP", "100000", "99900000"),
        600,
    );
    // 99,900,000 less 199,800, × 2.5 = 249,250,500 at either.
    for (id, by, tuple) in [
        ("f-ex-t1-quote", ("exchange-t1", "t1-co"), "f-ex-t1"),
        ("f-ex-t2-quote", ("exchange-t2", "t2-co"), "f-ex-t2"),
    ] {
        s.quote(
            id,
            by,
            tuple,
            ("99900000", "COP", "199800", "249250500"),
            Some("0.00025"),
            60,
            now,
        );
    }
    Case {
        name: "tie-choice",
        request: request(
            "req-tie-choice",
            choice_of("req-tie-shortlist"),
            json!({ "node": "ledger-co", "asset": unit("USDC", 6) }),
            "lowest_total_cost",
            None,
        ),
        snapshot: s.build(),
        bounds: bounds(2, json!([]), json!([])),
        expected: Expected::Route(vec!["f-w", "f-ex-t1"]),
        objective_pair: false,
        drops: Vec::new(),
    }
}

/// Every case of the suite.
pub fn cases() -> Vec<Case> {
    let ac2_bounds = || bounds(6, json!([]), json!([]));
    let choice = ac2_choice_snapshot();
    let ac2_choice =
        |name: &'static str, objective: &str, floor: Option<&str>, expected, pair| Case {
            name,
            request: request(
                name,
                choice_of("ac2-shortlist"),
                to_polygon(),
                objective,
                floor,
            ),
            snapshot: choice.clone(),
            bounds: ac2_bounds(),
            expected,
            objective_pair: pair,
            drops: Vec::new(),
        };
    vec![
        Case {
            name: "ac2-shortlist",
            request: request(
                "ac2-shortlist",
                json!({ "kind": "shortlist" }),
                to_polygon(),
                "lowest_total_cost",
                None,
            ),
            snapshot: ac2_shortlist_snapshot(),
            bounds: ac2_bounds(),
            expected: Expected::Shortlist,
            objective_pair: false,
            drops: Vec::new(),
        },
        ac2_choice(
            "ac2-choice-lowest-total-cost",
            "lowest_total_cost",
            None,
            Expected::Route(SLOW_ROUTE.to_vec()),
            true,
        ),
        ac2_choice(
            "ac2-choice-fastest-settlement",
            "fastest_settlement",
            None,
            Expected::Route(FAST_ROUTE.to_vec()),
            true,
        ),
        // 300 USDC is more than any route delivers.
        ac2_choice(
            "ac2-choice-floor-above-every-route",
            "lowest_total_cost",
            Some("300000000"),
            Expected::NoRoute,
            false,
        ),
        policy_case(),
        tie_case(),
    ]
}
