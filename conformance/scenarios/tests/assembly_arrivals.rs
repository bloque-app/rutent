//! Arrivals in the assembly (`T-020-04`): value arriving at the simulated reserve with no order opens none, on the
//! real log, which verifies the reserve's notice by the keys it holds; and a filing on it is held to the reserve's
//! declaration in Core's catalogue, the simulated reserve declaring it doesn't start routes on automatic deposits.
//! `AC-10`, `AC-11` and `AC-26` run end to end in `T-024-04`.

use rutent_conformance::signing::{real, sign_value};
use rutent_conformance::sim::{Role as SimRole, Spec};
use rutent_contracts::core_internal::{ArrivalMatch, Parties, What};
use rutent_contracts::provider_role::{Denomination, KeyPurpose, OuterNode, RESULT_SHAPE};
use rutent_contracts::router_api::PositiveAmount;
use rutent_core_orch::arrival::{ArrivalError, Arrived, Filing, Report};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged, StepError, context};
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};
use serde_json::json;

/// The simulated reserve's statements key, the real one its placeholder stands for.
fn statements_key() -> ed25519_dalek::SigningKey {
    let keys = Spec::of(SimRole::Reserve, RESERVE)
        .make(&rutent_conformance::role::SuiteContext::default())
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

/// The reserve's notice that `amount` arrived, signed, naming no transaction, made at Core's clock.
fn report(core: &Core, amount: &str) -> Report {
    let request = base_request("req-shape", amount, &id("session-shape"));
    let rutent_contracts::provider_role::NodeRef::Outer(node) = request.route.from.node.clone()
    else {
        panic!("the route starts at an outer node");
    };
    let asset: Denomination = request.route.from.asset.clone();
    let mut notice = json!({
        "deployment": context().deployment,
        "protocol_version": rutent_contracts::provider_role::VERSION,
        "shape": RESULT_SHAPE,
        "maker": RESERVE,
        "made_at": core.clock.timestamp().as_str(),
        "signature": { "scheme": "ed25519", "key": "", "value": "" },
        "statement": { "kind": "arrival", "amount": amount, "reference": "credit-1" },
    });
    sign_value(&mut notice, &statements_key());
    Report {
        reserve: id(RESERVE),
        node: OuterNode {
            network: node.network,
        },
        asset,
        amount: PositiveAmount::new(amount).expect("an amount"),
        account: id(&context().account),
        holder: kit::customer().party,
        notice: serde_json::to_vec(&notice).expect("bytes"),
    }
}

// verifies: REQ-ORD-14, REQ-ORD-13
#[test]
fn value_arriving_with_no_order_opens_none_and_an_undeclared_reserve_starts_nothing() {
    let (mut core, _) = Scenario::new("Arrivals", "AC-10", Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    let reported = report(&core, AMOUNT);
    let arrived = core.arrive(&reported).expect("the arrival is taken");
    let Arrived::NoOrder { arrival } = arrived else {
        panic!("{arrived:?}");
    };
    assert!(core.verified_whats().iter().any(|w| matches!(
        w,
        What::Arrival {
            order: None,
            matched: ArrivalMatch::NoOrder,
            ..
        }
    )));
    // The simulated reserve declares it doesn't start routes on automatic deposits: the operator's filing starts
    // nothing.
    let session = core.open_session(&kit::customer()).expect("a session");
    let request = base_request("req-deposit", AMOUNT, &session);
    let customer = kit::customer().party;
    let filing = Filing {
        request: id("deposit-1"),
        filer: kit::operator_login(),
        arrival,
        parties: Parties {
            requester: customer.clone(),
            beneficiary: customer.clone(),
            settlement_account_at: None,
            payer: customer,
        },
        session,
        destination: request.destination,
        route: request.route,
        inputs: request.inputs,
        needs: Vec::new(),
    };
    let entries = core.verified_whats().len();
    assert!(matches!(
        core.file(filing),
        Err(StepError::Arrival(ArrivalError::NotDeclared))
    ));
    assert_eq!(core.verified_whats().len(), entries, "nothing recorded");
}
