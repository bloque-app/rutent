//! Decision records for the quote module's tests (`T-020-07`), routed by `RouteFinder` over snapshots written as
//! router-api's JSON: a reserve's withdrawal and deposit, with an exchange between them where a test asks, each
//! confirmed in a choice round by its provider's answer, so the router chooses a candidate as it would for Core.
//! Signatures are placeholders: the router reads them as Core's precondition and verifies none.

#![allow(dead_code)]

use rutent_contracts::router_api::{
    Candidate, CatalogueSnapshot, DecisionRecord, PolicyBounds, Request, Router,
};
use rutent_router::RouteFinder;
use serde_json::{Value, json};

pub const DEPLOYMENT: &str = "rutent-test";
pub const OPERATOR_KEY: &str = "JddFbtX5xFFtDV_hZttVr9nJBVV9TlPxp99l7N3ZRlA";
pub const PROVIDER_KEY: &str = "DVP_bDfejcFXRBdFhgleHZTDfHjingJbVFhJlkpiLdE";
pub const SIGNATURE: &str =
    "lsrpbtB5rahgeQi2ugPWVTFXrjVELKI6MgYK5A_9vBVN_u10yk2KC0jqpnZr_3pYfecOg0seNUTRDVphTCJIGA";
pub const WIDE: &str = "1000000000000000000000000000000";
/// When the choice round's snapshot is taken.
pub const TAKEN_AT: &str = "2026-11-06T15:02:00Z";
/// When the shortlist it refines was taken.
pub const SHORTLIST_AT: &str = "2026-11-06T15:00:00Z";
/// When each provider answered, after the shortlist.
pub const ANSWERED_AT: &str = "2026-11-06T15:01:40Z";
/// The reserve at both ends.
pub const RESERVE: &str = "p-res";
/// The exchange between them.
pub const EXCHANGE: &str = "p-x";
/// Three days, in seconds: within them a segment is bound, past them free.
pub const THREE_DAYS: u64 = 259_200;

fn parse<T: serde::de::DeserializeOwned>(what: &str, value: Value) -> T {
    serde_json::from_value(value).unwrap_or_else(|e| panic!("the test's {what} is valid: {e}"))
}

pub fn unit(id: &str) -> Value {
    json!({ "id": id, "decimals": 6 })
}

pub fn outer(asset: &str) -> Value {
    json!({ "node": { "network": "polygon" }, "asset": unit(asset) })
}

pub fn inner(asset: &str) -> Value {
    json!({ "node": "hub", "asset": unit(asset) })
}

pub fn fee(flat: &str, numerator: &str, denominator: &str) -> Value {
    json!({ "flat": flat, "proportional": { "numerator": numerator, "denominator": denominator } })
}

fn deployment() -> Value {
    json!({ "name": DEPLOYMENT, "operator_key": OPERATOR_KEY })
}

fn origin(provider: &str) -> Value {
    json!({
        "kind": "provider",
        "provider": provider,
        "publisher": format!("{provider}-group"),
        "provider_version": "1.0.0",
        "module_version": "1.0.0",
    })
}

fn envelope(maker: &str, body: Value) -> Value {
    json!({
        "deployment": deployment(),
        "protocol_version": rutent_contracts::provider_role::VERSION,
        "shape": "urn:rutent:provider-role:result",
        "maker": maker,
        "made_at": ANSWERED_AT,
        "signature": { "scheme": "ed25519", "key": PROVIDER_KEY, "value": SIGNATURE },
        "statement": body,
    })
}

/// How a reserve's tuple is priced and answered: its price's flat fee and settlement seconds, the fee its answer
/// states, whether its answer reports gross (what it gives out holds the charge) or net, and the asset.
#[derive(Debug, Clone)]
pub struct Leg {
    pub asset: &'static str,
    pub price_fee: u64,
    pub seconds: u64,
    pub answer_fee: u64,
    pub gross: bool,
}

impl Leg {
    /// A free leg in `asset`, settling in a minute, reporting net.
    pub fn free(asset: &'static str) -> Self {
        Leg {
            asset,
            price_fee: 0,
            seconds: 60,
            answer_fee: 0,
            gross: false,
        }
    }
}

/// A choice round's snapshot under construction.
pub struct Net {
    pub figures: Vec<Value>,
    amount: u64,
}

impl Net {
    /// A route of `amount` units, to build.
    pub fn new(amount: u64) -> Self {
        Net {
            figures: Vec::new(),
            amount,
        }
    }

    fn advertised(&mut self, id: &str, provider: &str, value: Value) {
        self.figures.push(json!({
            "figure_id": id,
            "origin": origin(provider),
            "age_seconds": 60,
            "value": value,
        }));
    }

    fn terms(gross: bool) -> Value {
        json!({
            "bands": [{ "min": "1", "max": WIDE }],
            "reporting": if gross { "gross" } else { "net" },
            "required_inputs": [],
        })
    }

    /// A reserve tuple `id` of [`RESERVE`] from `from` to `to`, priced and answered as `leg`; what it is handed
    /// is `amount_in`, and what its answer gives out is returned.
    fn reserve_leg(
        &mut self,
        id: &str,
        kind: &str,
        (from, to): (Value, Value),
        leg: &Leg,
        amount_in: u64,
    ) -> u64 {
        self.advertised(
            id,
            RESERVE,
            json!({ "kind": "reserve_tuple", "tuple": {
                "origin": from, "destination": to, "asset": leg.asset,
                "decimals": { "origin": 6, "destination": 6 },
                "terms": Self::terms(leg.gross),
            }}),
        );
        self.advertised(
            &format!("{id}-price"),
            RESERVE,
            json!({
                "kind": "price", "tuple": id, "fee": fee(&leg.price_fee.to_string(), "0", "1"),
                "settlement_seconds": leg.seconds,
            }),
        );
        // Gross: what it gives out holds the charge; net: the charge is on top of it.
        let amount_out = if leg.gross {
            amount_in
        } else {
            amount_in - leg.answer_fee
        };
        let body = json!({
            "kind": kind,
            "transaction": format!("tx-{id}"),
            "outcome": {
                "status": "dry_run_accepted",
                "amount_in": amount_in.to_string(),
                "amount_out": amount_out.to_string(),
                "fee": { "asset": leg.asset, "amount": leg.answer_fee.to_string() },
                "valid_until": "2026-11-06T15:03:40Z",
                "voided_by": ["validity_passes", "amount_changes"],
                "settlement_seconds": leg.seconds,
            },
        });
        self.figures.push(json!({
            "figure_id": format!("{id}-dry"),
            "origin": origin(RESERVE),
            "age_seconds": 20,
            "valid_for_seconds": 120,
            "value": {
                "kind": "dry_run",
                "tuple": id,
                "amount_in": amount_in.to_string(),
                "result": envelope(RESERVE, body),
            },
        }));
        amount_out
    }

    /// The reserve's withdrawal from `polygon` into `hub`, as `leg`: what it gives out.
    pub fn withdrawal(&mut self, leg: &Leg) -> u64 {
        let amount = self.amount;
        self.reserve_leg(
            "w0",
            "withdrawal",
            (json!({ "network": "polygon" }), json!("hub")),
            leg,
            amount,
        )
    }

    /// The reserve's deposit from `hub` out to `polygon`, as `leg`, handed `amount_in`: what it gives out.
    pub fn deposit(&mut self, leg: &Leg, amount_in: u64) -> u64 {
        self.reserve_leg(
            "d0",
            "deposit",
            (json!("hub"), json!({ "network": "polygon" })),
            leg,
            amount_in,
        )
    }

    /// [`EXCHANGE`]'s tuple at `hub`, from USDC to EURC at a fixed 0.9 with no fee, declaring `seconds`, confirmed
    /// by a fresh conversion quote for `amount_in` with `slippage`: what it gives out.
    pub fn exchange(&mut self, amount_in: u64, seconds: u64, slippage: Value) -> u64 {
        self.advertised(
            "x0",
            EXCHANGE,
            json!({ "kind": "exchange_tuple", "tuple": {
                "node": "hub", "from": unit("USDC"), "to": unit("EURC"), "terms": Self::terms(false),
            }}),
        );
        self.advertised(
            "x0-price",
            EXCHANGE,
            json!({
                "kind": "price", "tuple": "x0", "fee": fee("0", "0", "1"),
                "rate": { "kind": "fixed", "rate": "0.9" }, "settlement_seconds": seconds,
            }),
        );
        let amount_out = amount_in * 9 / 10;
        let body = json!({
            "kind": "conversion",
            "transaction": "tx-x0",
            "outcome": {
                "status": "dry_run_accepted",
                "amount_in": amount_in.to_string(),
                "amount_out": amount_out.to_string(),
                "rate": "0.9",
                "slippage": slippage,
                "fee": { "asset": "USDC", "amount": "0" },
                "direction": "exact_in",
                "valid_until": "2026-11-06T15:03:40Z",
                "voided_by": ["validity_passes", "amount_changes"],
                "settlement_seconds": seconds,
            },
        });
        self.figures.push(json!({
            "figure_id": "x0-quote",
            "origin": origin(EXCHANGE),
            "age_seconds": 20,
            "valid_for_seconds": 120,
            "value": { "kind": "fresh_quote", "tuple": "x0", "quote": envelope(EXCHANGE, body) },
        }));
        amount_out
    }

    pub fn build(&self) -> CatalogueSnapshot {
        parse(
            "snapshot",
            json!({ "snapshot_id": "snap-choice", "taken_at": TAKEN_AT, "figures": self.figures }),
        )
    }
}

/// Bounds with the operator's `operator_fee` and these slippage limits, `(provider, from, to, max_bps)`.
pub fn bounds(operator_fee: Value, limits: &[(&str, Value, Value, u16)]) -> PolicyBounds {
    let limits: Vec<Value> = limits
        .iter()
        .map(|(provider, from, to, max_bps)| {
            json!({ "tuple": { "provider": provider, "from": from, "to": to }, "max_bps": max_bps })
        })
        .collect();
    parse(
        "bounds",
        json!({
            "deployment": deployment(),
            "protocol_version": rutent_contracts::provider_role::VERSION,
            "policy_version": "policy-1",
            "max_hops": 4,
            "max_parts": 1,
            "excluded_providers": [],
            "excluded_tuples": [],
            "provider_ceilings": [],
            "publisher_ceilings": [],
            "operator_fee": operator_fee,
            "slippage_limits": limits,
        }),
    )
}

/// The exchange's limit of `max_bps`.
pub fn exchange_limit(max_bps: u16) -> (&'static str, Value, Value, u16) {
    (EXCHANGE, inner("USDC"), inner("EURC"), max_bps)
}

/// A choice round's request, refining the shortlist taken at [`SHORTLIST_AT`], from `polygon` in `from` to
/// `polygon` in `to`, of `amount`, with `floor` where given.
pub fn request(from: &str, to: &str, amount: u64, floor: Option<u64>) -> Request {
    let mut request = json!({
        "request_id": "req-choice",
        "round": { "kind": "choice", "refines": { "request_id": "req-shortlist", "taken_at": SHORTLIST_AT } },
        "from": outer(from),
        "to": outer(to),
        "amount": amount.to_string(),
        "objective": { "kind": "lowest_total_cost" },
    });
    if let Some(floor) = floor {
        request["floor"] = json!(floor.to_string());
    }
    parse("request", request)
}

/// Routes, and reads the record back from its bytes, which runs the contract's check.
pub fn routed(
    request: &Request,
    snapshot: &CatalogueSnapshot,
    bounds: &PolicyBounds,
) -> DecisionRecord {
    let record = RouteFinder::route(request, snapshot, bounds);
    let bytes = serde_json::to_vec(&record).expect("a record serialises");
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("the contract refuses the record: {e}"))
}

/// The candidate `record` chose.
pub fn chosen(record: &DecisionRecord) -> &Candidate {
    record
        .chosen()
        .unwrap_or_else(|| panic!("the router chose nothing: {:#?}", record.candidates()))
}

/// A withdrawal then a deposit of USDC at [`RESERVE`], of `amount`, as `withdrawal` and `deposit`, under
/// `operator_fee`, with `floor` where given.
pub fn reserve_route(
    amount: u64,
    withdrawal: &Leg,
    deposit: &Leg,
    operator_fee: Value,
    floor: Option<u64>,
) -> DecisionRecord {
    let mut net = Net::new(amount);
    let out = net.withdrawal(withdrawal);
    net.deposit(deposit, out);
    routed(
        &request("USDC", "USDC", amount, floor),
        &net.build(),
        &bounds(operator_fee, &[]),
    )
}

/// A withdrawal of USDC, an exchange to EURC declaring `seconds` with `slippage`, and a deposit of EURC, all at
/// a minute but the exchange, of `amount`, under `limits`.
pub fn exchange_route(
    amount: u64,
    seconds: u64,
    slippage: Value,
    limits: &[(&str, Value, Value, u16)],
) -> DecisionRecord {
    let mut net = Net::new(amount);
    let out = net.withdrawal(&Leg::free("USDC"));
    let converted = net.exchange(out, seconds, slippage);
    net.deposit(&Leg::free("EURC"), converted);
    routed(
        &request("USDC", "EURC", amount, None),
        &net.build(),
        &bounds(fee("0", "0", "1"), limits),
    )
}
