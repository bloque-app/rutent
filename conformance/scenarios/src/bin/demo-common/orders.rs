//! The customer's requests, the operator's staff, and the evidence a decision about a stuck order cites: what both
//! binaries build their orders from.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::{Actor, ActorKind, Destination, Entry, Parties, Role};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::OuterNode;
use rutent_contracts::router_api::Id;
use rutent_core_authz::roles::Permission;
use rutent_core_orch::intake::OrderRequest;
use rutent_scenarios::assembly::{Core, ORIGIN_ACCOUNT};
use rutent_scenarios::kit::{self, id};
use serde_json::{Value, json};

/// The address the customer gives for a deposit on chain.
pub const ADDRESS: &str = "0x5e11a9d0c0ffee";
/// The most segments a route may have in the demo: enough to cross two publishers' ledgers and a chain.
pub const MAX_HOPS: u8 = 6;
/// The public node the operator's wallet holds, where act 3's order is left stuck.
pub const HELD_NODE: &str = "sim-polygon";

/// An asset in two decimals, as an endpoint's unit.
pub fn unit(asset: &str) -> Value {
    json!({ "id": asset, "decimals": 2 })
}

/// An outer node's endpoint in `asset`.
pub fn outer(network: &str, asset: &str) -> Value {
    json!({ "node": { "network": network }, "asset": unit(asset) })
}

/// An inner node's endpoint in `asset`.
pub fn inner(node: &str, asset: &str) -> Value {
    json!({ "node": node, "asset": unit(asset) })
}

/// What one order asks: where from and where to, through which reserves, delivered to which account, by which
/// objective, and how much.
pub struct Asked<'a> {
    /// The request's name, from which its identifiers follow.
    pub name: &'a str,
    /// The customer's session.
    pub session: &'a Id,
    /// The reserve the value is withdrawn at.
    pub origin: &'a str,
    /// The endpoint it leaves.
    pub from: Value,
    /// The endpoint it reaches.
    pub to: Value,
    /// The reserve it is deposited by.
    pub at: &'a str,
    /// The account it is delivered to.
    pub account: &'a str,
    /// `lowest_total_cost` or `fastest_settlement`.
    pub objective: &'a str,
    /// What leaves the origin, in its asset's smallest unit.
    pub amount: &'a str,
}

/// The customer's order `asked` names: the customer is requester, beneficiary and payer.
pub fn request(asked: &Asked<'_>) -> OrderRequest {
    let customer = kit::customer();
    let network = asked.to["node"]["network"]
        .as_str()
        .map_or_else(|| format!("{}-bank", asked.at), str::to_owned);
    OrderRequest {
        request: id(asked.name),
        caller: customer.clone(),
        parties: Parties {
            requester: customer.party.clone(),
            beneficiary: customer.party.clone(),
            settlement_account_at: None,
            payer: customer.party.clone(),
        },
        origin_reserve: id(asked.origin),
        session: asked.session.clone(),
        destination: Destination {
            reserve: id(asked.at),
            node: OuterNode { network },
            account: id(asked.account),
        },
        route: serde_json::from_value(json!({
            "request_id": format!("{}-route", asked.name),
            "round": {"kind": "shortlist"},
            "from": asked.from,
            "to": asked.to,
            "amount": asked.amount,
            "objective": {"kind": asked.objective},
        }))
        .expect("a route request"),
        inputs: BTreeMap::from([
            ("payer.bank_account".to_owned(), ORIGIN_ACCOUNT.to_owned()),
            (
                "beneficiary.bank_account".to_owned(),
                asked.account.to_owned(),
            ),
        ]),
        needs: Vec::new(),
    }
}

/// The customer's address on the chains, which the reserves' registers hold as theirs.
pub fn register_address(core: &Core) {
    core.registers().accounts.insert(
        ADDRESS.to_owned(),
        kit::customer().party.as_str().to_owned(),
    );
}

/// One of the operator's staff: a person on `desk`, acting for the operator, holding `permission` alone.
pub fn staff(core: &Core, desk: &str, login: &str, permission: Permission) -> Actor {
    let person = Actor {
        party: id(desk),
        acts: id(login),
        role: Role::Operator,
        kind: ActorKind::Person,
    };
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
///
/// # Errors
///
/// If the log holds no provider's result about it.
pub fn evidence_about(core: &Core, order: &Id) -> Result<Vec<Digest>, String> {
    core.log()
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
                    && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order) =>
            {
                Some(vec![s.commitment.clone()])
            }
            _ => None,
        })
        .ok_or_else(|| "no provider's statement about the order".to_owned())
}

/// The entries `audit-check` printed as not borne out by their evidence, by index.
pub fn not_borne_out(output: &str) -> Vec<usize> {
    output
        .lines()
        .filter_map(|l| l.trim().strip_prefix("NotBorneOut { index: "))
        .filter_map(|rest| rest.trim_end_matches(" }").parse().ok())
        .collect()
}

/// Whether `record` is a provider's inner-segments statement.
pub fn is_inner_segments(record: Option<&Vec<u8>>) -> bool {
    record
        .and_then(|r| serde_json::from_slice::<Value>(r).ok())
        .is_some_and(|v| {
            v["what"]["statement"]
                .as_str()
                .is_some_and(|s| s.ends_with(":inner-segments"))
        })
}

/// `n` candidate routes, in words.
pub fn candidates(n: usize) -> String {
    if n == 1 {
        "1 candidate route".to_owned()
    } else {
        format!("{n} candidate routes")
    }
}

/// What `audit-check`'s exit code says.
pub fn verdict(code: Option<i32>) -> &'static str {
    match code {
        Some(0) => "it holds",
        Some(1) => "the checker found something: each finding is printed above",
        _ => "the checker couldn't run",
    }
}
