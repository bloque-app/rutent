//! Networks for the router's own tests, written as the contract's JSON so each reads as a real snapshot does.

#![allow(dead_code)]

use rutent_contracts::router_api::{
    CatalogueSnapshot, DecisionRecord, PolicyBounds, Request, Router,
};
use rutent_router::RouteFinder;
use serde_json::{Value, json};

pub const DEPLOYMENT: &str = "rutent-test";
pub const OPERATOR_KEY: &str = "JddFbtX5xFFtDV_hZttVr9nJBVV9TlPxp99l7N3ZRlA";
pub const PROVIDER_KEY: &str = "DVP_bDfejcFXRBdFhgleHZTDfHjingJbVFhJlkpiLdE";
pub const SIGNATURE: &str =
    "lsrpbtB5rahgeQi2ugPWVTFXrjVELKI6MgYK5A_9vBVN_u10yk2KC0jqpnZr_3pYfecOg0seNUTRDVphTCJIGA";
pub const WIDE: &str = "1000000000000000000000000000000";

pub fn parse<T: serde::de::DeserializeOwned>(what: &str, value: Value) -> T {
    serde_json::from_value(value).unwrap_or_else(|e| panic!("the test's {what} is valid: {e}"))
}

pub fn outer(network: &str) -> Value {
    json!({ "network": network })
}

pub fn unit(id: &str, decimals: u8) -> Value {
    json!({ "id": id, "decimals": decimals })
}

pub fn endpoint(node: Value, asset: &str, decimals: u8) -> Value {
    json!({ "node": node, "asset": unit(asset, decimals) })
}

pub fn terms(min: &str, max: &str) -> Value {
    json!({ "bands": [{ "min": min, "max": max }], "reporting": "net", "required_inputs": [] })
}

pub fn fee(flat: &str, numerator: &str, denominator: &str) -> Value {
    json!({ "flat": flat, "proportional": { "numerator": numerator, "denominator": denominator } })
}

pub fn deployment() -> Value {
    json!({ "name": DEPLOYMENT, "operator_key": OPERATOR_KEY })
}

/// A snapshot under construction, of advertised figures `age` seconds old and any statements added to it.
pub struct Net {
    pub id: String,
    pub taken_at: String,
    pub age: u64,
    pub figures: Vec<Value>,
}

impl Net {
    pub fn new(id: &str, taken_at: &str, age: u64) -> Self {
        Net {
            id: id.to_owned(),
            taken_at: taken_at.to_owned(),
            age,
            figures: Vec::new(),
        }
    }

    pub fn origin(provider: &str) -> Value {
        json!({
            "kind": "provider",
            "provider": provider,
            "publisher": format!("{provider}-group"),
            "provider_version": "1.0.0",
            "module_version": "1.0.0",
        })
    }

    pub fn advertised(&mut self, id: &str, provider: &str, value: Value) {
        self.figures.push(json!({
            "figure_id": id,
            "origin": Self::origin(provider),
            "age_seconds": self.age,
            "value": value,
        }));
    }

    /// A tuple `{id}` and its price `{id}-price`, both from `provider`. `price` holds the fee, an optional rate
    /// and the settlement seconds.
    pub fn tuple(&mut self, id: &str, provider: &str, kind: &str, tuple: Value, price: Value) {
        self.advertised(id, provider, json!({ "kind": kind, "tuple": tuple }));
        let mut price = price;
        price["kind"] = json!("price");
        price["tuple"] = json!(id);
        self.advertised(&format!("{id}-price"), provider, price);
    }

    /// A reserve tuple between `origin` and `destination` nodes, one asset with a unit at each end.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve(
        &mut self,
        id: &str,
        (origin, destination): (Value, Value),
        asset: &str,
        (from_decimals, to_decimals): (u8, u8),
        band: (&str, &str),
        price: Value,
    ) {
        self.tuple(
            id,
            &format!("p-{id}"),
            "reserve_tuple",
            json!({
                "origin": origin, "destination": destination, "asset": asset,
                "decimals": { "origin": from_decimals, "destination": to_decimals },
                "terms": terms(band.0, band.1),
            }),
            price,
        );
    }

    /// A ramp tuple between two inner nodes, one asset with a unit at each end.
    #[allow(clippy::too_many_arguments)]
    pub fn ramp(
        &mut self,
        id: &str,
        (origin, destination): (&str, &str),
        asset: &str,
        (from_decimals, to_decimals): (u8, u8),
        band: (&str, &str),
        price: Value,
    ) {
        self.tuple(
            id,
            &format!("p-{id}"),
            "ramp_tuple",
            json!({
                "origin": origin, "destination": destination, "asset": asset,
                "decimals": { "origin": from_decimals, "destination": to_decimals },
                "terms": terms(band.0, band.1),
            }),
            price,
        );
    }

    /// An exchange tuple at one inner node, from one asset to another.
    pub fn exchange(
        &mut self,
        id: &str,
        node: &str,
        (from, to): (Value, Value),
        band: (&str, &str),
        price: Value,
    ) {
        self.tuple(
            id,
            &format!("p-{id}"),
            "exchange_tuple",
            json!({ "node": node, "from": from, "to": to, "terms": terms(band.0, band.1) }),
            price,
        );
    }

    pub fn envelope(maker: &str, made_at: &str, shape: &str, body: Value) -> Value {
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

    /// A ramp's fresh transfer quote for tuple `tuple` at `amount_in`, made at `made_at` and valid until
    /// `valid_until`, with its age and validity in whole seconds at the snapshot's `taken_at`.
    #[allow(clippy::too_many_arguments)]
    pub fn transfer_quote(
        &mut self,
        id: &str,
        tuple: &str,
        (amount_in, fee_asset, fee, amount_out): (&str, &str, &str, &str),
        settlement_seconds: u64,
        (made_at, valid_until): (&str, &str),
        (age, valid): (u64, u64),
    ) {
        let provider = format!("p-{tuple}");
        let body = json!({
            "kind": "transfer",
            "transaction": format!("tx-{id}"),
            "outcome": {
                "status": "dry_run_accepted",
                "amount_in": amount_in,
                "amount_out": amount_out,
                "fee": { "asset": fee_asset, "amount": fee },
                "direction": "exact_in",
                "valid_until": valid_until,
                "voided_by": ["validity_passes", "amount_changes"],
                "settlement_seconds": settlement_seconds,
            },
        });
        self.figures.push(json!({
            "figure_id": id,
            "origin": Self::origin(&provider),
            "age_seconds": age,
            "valid_for_seconds": valid,
            "value": {
                "kind": "fresh_quote",
                "tuple": tuple,
                "quote": Self::envelope(&provider, made_at, "urn:rutent:provider-role:result", body),
            },
        }));
    }

    pub fn build(&self) -> CatalogueSnapshot {
        parse(
            "snapshot",
            json!({ "snapshot_id": self.id, "taken_at": self.taken_at, "figures": self.figures }),
        )
    }
}

pub fn bounds(max_hops: u8) -> PolicyBounds {
    parse(
        "bounds",
        json!({
            "deployment": deployment(),
            "protocol_version": rutent_contracts::provider_role::VERSION,
            "policy_version": "policy-1",
            "max_hops": max_hops,
            "max_parts": 1,
            "excluded_providers": [],
            "excluded_tuples": [],
            "provider_ceilings": [],
            "publisher_ceilings": [],
        }),
    )
}

pub fn shortlist() -> Value {
    json!({ "kind": "shortlist" })
}

pub fn choice_of(shortlist: &str, taken_at: &str) -> Value {
    json!({ "kind": "choice", "refines": { "request_id": shortlist, "taken_at": taken_at } })
}

pub fn request(id: &str, round: Value, from: Value, to: Value, amount: &str) -> Request {
    parse(
        "request",
        json!({
            "request_id": id,
            "round": round,
            "from": from,
            "to": to,
            "amount": amount,
            "objective": { "kind": "lowest_total_cost" },
        }),
    )
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

/// The tuple figures each of a candidate's operations takes, by id: the first figure it cites.
pub fn tuples(candidate: &rutent_contracts::router_api::Candidate) -> Vec<String> {
    candidate.parts[0]
        .operations
        .iter()
        .map(|o| o.figures.first().as_str().to_owned())
        .collect()
}

/// The candidate that takes exactly these tuples.
pub fn on<'a>(
    record: &'a DecisionRecord,
    route: &[&str],
) -> &'a rutent_contracts::router_api::Candidate {
    record
        .candidates
        .iter()
        .find(|c| tuples(c) == route)
        .unwrap_or_else(|| panic!("no candidate takes {route:?}"))
}
