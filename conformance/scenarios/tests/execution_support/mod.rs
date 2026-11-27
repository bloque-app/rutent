//! What the execution scenarios share (`T-024-04`): a Core put together over simulated providers by name, orders
//! requested, quoted, accepted and run through Core's own `request`, `quote`, `accept` and `run`, and the log read
//! back as who did what and when.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::time::Duration;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::{real, sign_value};
use rutent_conformance::sim::{Role, Spec};
use rutent_contracts::core_internal::{
    Actor, Destination, Entry, Parties, PublicMovement, VerifiedEntry, What,
};
use rutent_contracts::provider_role::{EvidenceGrade, KeyPurpose, OuterNode};
use rutent_contracts::router_api::{DecisionRecord, Id};
use rutent_contracts::state_machine::OrderState;
use rutent_core_orch::arrival::{Filing, Report};
use rutent_core_orch::intake::{OrderRequest, Status, Told};
use rutent_core_orch::segments::Ran;
use rutent_scenarios::assembly::machine::{Delivery, Reference};
use rutent_scenarios::assembly::{Core, DESTINATION_ACCOUNT, ORIGIN_ACCOUNT, Staged};
use rutent_scenarios::harness::{Given, Scenario};
use rutent_scenarios::kit::{self, id};
use serde_json::{Value, json};

/// The role a simulated provider's name says it plays: an exchange or a ramp by its name, else a reserve.
pub fn spec_of(name: &str) -> Spec {
    let role = if name.contains("exchange") {
        Role::Exchange
    } else if name.contains("ramp") {
        Role::Ramp
    } else {
        Role::Reserve
    };
    Spec::of(role, name)
}

/// What a scenario is given: the providers named, staging at once.
pub fn given(names: &[&str]) -> Given {
    Given {
        providers: names.iter().map(|name| spec_of(name)).collect(),
        staging: Staged::AtOnce,
        deadline: Duration::from_secs(2),
        delivery: Delivery::Once,
    }
}

/// Core, put together with the providers `names`, and a session open for the customer.
pub fn core_with(accepts: &'static str, names: &[&str]) -> (Core, Id) {
    core_of(accepts, given(names))
}

/// Core, put together as `given` says, and a session open for the customer.
pub fn core_of(accepts: &'static str, given: Given) -> (Core, Id) {
    let (mut core, _) = Scenario::new("execution", accepts, given).core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    (core, session)
}

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

/// One order's request, as the customer makes it: `amount` from the endpoint `from` of the reserve `origin` to the
/// endpoint `to` of the reserve `at`, whose outer node is `network`, delivered to `account`, ranked by `objective`.
/// The inputs are the base order's: the payer's account at the origin and the beneficiary's at the destination.
#[allow(clippy::too_many_arguments)]
pub fn request(
    name: &str,
    session: &Id,
    amount: &str,
    origin: &str,
    from: Value,
    to: Value,
    at: &str,
    account: &str,
    objective: &str,
) -> OrderRequest {
    let customer = kit::customer();
    // The outer node the order is delivered at: the route's own, where it ends at one, else the reserve's bank.
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
            "amount": amount,
            "objective": {"kind": objective},
        }))
        .expect("a route request"),
        inputs: BTreeMap::from([
            ("payer.bank_account".to_owned(), ORIGIN_ACCOUNT.to_owned()),
            ("beneficiary.bank_account".to_owned(), account.to_owned()),
        ]),
        needs: Vec::new(),
    }
}

/// The base order: the reserve `reserve`'s bank back to its bank, to `account`, by lowest total cost.
pub fn within(
    name: &str,
    session: &Id,
    amount: &str,
    reserve: &str,
    account: &str,
) -> OrderRequest {
    let bank = outer(&format!("{reserve}-bank"), "COP");
    request(
        name,
        session,
        amount,
        reserve,
        bank.clone(),
        bank,
        reserve,
        account,
        "lowest_total_cost",
    )
}

/// The order `request` opens, quoted; what quoting it told.
pub fn quoted(core: &mut Core, request: OrderRequest) -> (Id, Told) {
    let told = core.request(request).expect("the request opens an order");
    let order = Core::order_of(&told).unwrap_or_else(|| panic!("an order: {told:?}"));
    let told = core.quote(&kit::customer(), &order).expect("quoted");
    (order, told)
}

/// The order `request` opens, quoted and accepted by the customer; not run.
pub fn accepted(core: &mut Core, request: OrderRequest) -> Id {
    let (order, told) = quoted(core, request);
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("the order is quoted: {told:?}");
    };
    core.accept(&kit::customer(), &order, &digest)
        .expect("accepted");
    order
}

/// The order `request` opens, quoted, accepted and run.
pub fn run(core: &mut Core, request: OrderRequest) -> (Id, Vec<Ran>) {
    let order = accepted(core, request);
    let ran = core.run(&order).expect("the order ran");
    (order, ran)
}

/// The records the publishers keep of the segments inside the private node `node`, as Core took them when its closing last
/// read them (`REQ-ORD-41`, `T-020-58`): one for each provider that served in it, in the order the providers are installed.
pub fn published(
    core: &mut Core,
    node: &str,
) -> Vec<rutent_contracts::provider_role::Statement<rutent_contracts::provider_role::InnerSegments>>
{
    core.records_taken(node)
}

/// Where the order stands, as the customer reads it.
pub fn state(core: &mut Core, order: &Id) -> OrderState {
    core.status(&kit::customer(), order)
        .expect("the customer reads its order")
        .state
}

/// Every verified entry of the log, in order.
pub fn entries(core: &Core) -> Vec<VerifiedEntry> {
    core.log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) => Some(v.clone()),
            Entry::Stated(_) => None,
        })
        .collect()
}

/// One piece of evidence the log holds for a segment of `order`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidenced {
    /// The segment.
    pub segment: Id,
    /// Its grade.
    pub grade: EvidenceGrade,
    /// Whose evidence it is.
    pub whose: Id,
    /// The public movement Core checked for it.
    pub movement: Option<PublicMovement>,
    /// The publisher whose word it is, inside a private node.
    pub publisher: Option<Id>,
}

/// The evidence of `order`, in the order the log holds it.
pub fn evidence_of(core: &Core, order: &Id) -> Vec<Evidenced> {
    entries(core)
        .into_iter()
        .filter_map(|v| match v.what {
            What::Evidence {
                order: o,
                segment,
                grade,
                whose,
                movement,
                publisher,
                ..
            } if &o == order => Some(Evidenced {
                segment,
                grade,
                whose,
                movement,
                publisher,
            }),
            _ => None,
        })
        .collect()
}

/// The decision record Core signed that the order's quote cites.
pub fn record_of(core: &Core, told: &Told) -> DecisionRecord {
    let Told::Exists {
        status: Status::Quoted { quote, .. },
        ..
    } = told
    else {
        panic!("a quote is shown: {told:?}");
    };
    core.decision_records()
        .into_iter()
        .find(|r| r.record_id() == &quote.decision_record)
        .expect("Core signed the record")
}

/// The providers the chosen route runs through, in order.
pub fn providers(record: &DecisionRecord) -> Vec<String> {
    record
        .chosen()
        .expect("a route was chosen")
        .parts
        .iter()
        .flat_map(|p| p.operations.iter())
        .map(|o| o.provider.as_str().to_owned())
        .collect()
}

/// The name an endpoint's node goes by: an outer node's network, or an inner node's identifier.
pub fn node_name(node: &rutent_contracts::provider_role::NodeRef) -> String {
    match node {
        rutent_contracts::provider_role::NodeRef::Inner(n) => n.clone(),
        rutent_contracts::provider_role::NodeRef::Outer(o) => o.network.clone(),
    }
}

/// The accounts the base scenarios deliver to.
pub const OTHER: &str = DESTINATION_ACCOUNT;
/// The payer's own account.
pub const SAME: &str = ORIGIN_ACCOUNT;

/// The simulated reserve `reserve`'s statements key, the real one its placeholder stands for.
pub fn statements_key(reserve: &str) -> ed25519_dalek::SigningKey {
    let keys = spec_of(reserve)
        .make(&SuiteContext::default())
        .advertisement()
        .into_statement()
        .statement
        .keys;
    let placeholder = keys
        .iter()
        .find(|k| k.purpose == KeyPurpose::Statements)
        .expect("a statements key")
        .public_key
        .clone();
    real(placeholder.as_str())
}

/// The reserve `reserve`'s notice that `amount` of `asset` arrived at its outer node `network`, for the account `account`
/// of the customer, signed, naming the transaction it gave the credit, as of Core's clock.
pub fn report(
    core: &Core,
    reserve: &str,
    network: &str,
    asset: Value,
    amount: &str,
    credit: &str,
) -> Report {
    let mut notice = json!({
        "deployment": rutent_scenarios::assembly::context().deployment,
        "protocol_version": rutent_contracts::provider_role::VERSION,
        "shape": rutent_contracts::provider_role::ARRIVAL_NOTICE_SHAPE,
        "maker": reserve,
        "made_at": core.clock.timestamp().as_str(),
        "signature": { "scheme": "ed25519", "key": "", "value": "" },
        // provider-role's `ArrivalNotice`: what Core reads the arrival's figures from.
        "statement": {
            "transaction": credit,
            "node": OuterNode { network: network.to_owned() },
            "asset": asset,
            "amount": amount,
            "account": rutent_scenarios::assembly::context().account,
            "holder": kit::customer().party,
        },
    });
    sign_value(&mut notice, &statements_key(reserve));
    Report {
        reserve: id(reserve),
        notice: serde_json::to_vec(&notice).expect("bytes"),
    }
}

/// `filer`'s filing of the arrival `arrival` as an order, as `request` would have asked for it, in `session`.
pub fn filing(
    name: &str,
    filer: Actor,
    arrival: rutent_contracts::core_internal::Receipt,
    session: &Id,
    request: OrderRequest,
) -> Filing {
    let customer = kit::customer().party;
    Filing {
        request: id(name),
        filer,
        arrival,
        parties: Parties {
            requester: customer.clone(),
            beneficiary: customer.clone(),
            settlement_account_at: None,
            payer: customer,
        },
        session: session.clone(),
        destination: request.destination,
        route: request.route,
        inputs: request.inputs,
        needs: Vec::new(),
    }
}
