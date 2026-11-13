//! Arrivals in the assembly (`T-020-04`): value arriving at the simulated reserve with no order opens none, on the
//! real log, which verifies the reserve's notice by the keys it holds; and a filing on it is held to the reserve's
//! declaration in Core's catalogue, the simulated reserve declaring it doesn't start routes on automatic deposits.
//! `AC-10`, `AC-11` and `AC-26` run end to end in `T-024-04`.

use rutent_conformance::signing::{real, sign_value};
use rutent_conformance::sim::{Role as SimRole, Spec};
use rutent_contracts::core_internal::{ArrivalMatch, Parties, What};
use rutent_contracts::provider_role::{Denomination, KeyPurpose, OuterNode};
use rutent_contracts::router_api::PositiveAmount;
use rutent_core_orch::arrival::{ArrivalError, Arrived, Filed, Filing, Report};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged, StepError, context};
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};
use serde_json::json;

/// The shape the scenario gives the reserve's notice: provider-role names none for a notice of value arriving
/// (`T-020-04`'s gap), so it is one no contract names.
const NOTICE_SHAPE: &str = "urn:rutent:scenario:arrival-notice";

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

/// The reserve's notice that `amount` arrived, signed, naming the transaction it gave the credit, made at Core's clock.
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
        "shape": NOTICE_SHAPE,
        "maker": RESERVE,
        "made_at": core.clock.timestamp().as_str(),
        "signature": { "scheme": "ed25519", "key": "", "value": "" },
        "statement": { "kind": "arrival", "transaction": "credit-1", "amount": amount },
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
            notice: Some(_),
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

/// A filing on an automatic deposit under the real policy engine (`T-020-01`'s round 2 review): the reserve declares
/// that it starts routes on automatic deposits, and the order, which has no acceptance when the filing asks policy
/// for its authorisation, is learned from the order record the filing hands over. The deposit starts.
// verifies: REQ-ORD-13
#[test]
fn a_filing_on_an_automatic_deposit_starts_under_the_real_policy() {
    let given = Given {
        providers: vec![
            Spec::of(SimRole::Reserve, RESERVE).starting_routes_on_automatic_deposits(),
        ],
        ..Given::one_reserve(Staged::AtOnce)
    };
    let (mut core, _) = Scenario::new("Arrivals", "AC-10", given).core(1, &mut Reference);
    let reported = report(&core, AMOUNT);
    let Arrived::NoOrder { arrival } = core.arrive(&reported).expect("the arrival is taken") else {
        panic!("the arrival opens no order");
    };
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
    let filed = core.file(filing);
    let Ok(Filed::Started { order, .. }) = filed else {
        panic!("the deposit starts: {filed:?}");
    };
    assert!(
        core.verified_whats()
            .iter()
            .any(|w| matches!(w, What::Acceptance { order: o, .. } if o == &order)),
        "the deposit's order is accepted"
    );
}

/// The deployment's operator files a differing arrival anew through the real log, whose operator states as one maker
/// (`conformance-operator`) for another party (`operator`): the operator's login, of the operator's party, is
/// admitted; an actor stating the operator's role for a party named as the maker is refused, by Core and by the log's
/// own row (`refiling-files-an-arrival-anew`).
// verifies: REQ-ORD-23
#[test]
fn the_operators_login_files_a_differing_arrival_anew_on_the_real_log() {
    use rutent_contracts::core_internal::{
        Actor, ActorKind, DeploymentOperator, Entry, Outcome, Role, Through, TimeSource,
        VerifiedEntry, VerifiedKind, When, admit_entry,
    };
    use rutent_core_orch::intake::{Status as IntakeStatus, Told};

    let (mut core, _) = Scenario::new("Arrivals", "AC-10", Given::one_reserve(Staged::AtOnce))
        .core(1, &mut Reference);
    let customer = kit::customer();
    let session = core.open_session(&customer).expect("a session");
    let told = core
        .request(base_request("req-open", AMOUNT, &session))
        .expect("the request is taken");
    let order = Core::order_of(&told).expect("the order opens");
    let Told::Exists {
        status: IntakeStatus::Quoted { digest, .. },
        ..
    } = core.quote(&customer, &order).expect("quoted")
    else {
        panic!("the order is quoted");
    };
    core.accept(&customer, &order, &digest).expect("accepted");

    // Less than the order states arrives at the payer's account: held against the order, differing.
    let less = (AMOUNT.parse::<u64>().expect("an amount") / 2).to_string();
    let arrived = core
        .arrive(&report(&core, &less))
        .expect("the arrival is taken");
    let Arrived::Held {
        order: held,
        matched: ArrivalMatch::Differs,
        arrival,
    } = arrived
    else {
        panic!("{arrived:?}");
    };
    assert_eq!(held, order);

    // The operator's maker and party differ in the assembly's log.
    let operator = kit::operator();
    assert_ne!(operator.acts, operator.party);
    let maker_as_party = Actor {
        party: operator.acts.clone(),
        acts: id("someone-stating-the-operator"),
        role: Role::Operator,
        kind: ActorKind::Person,
    };

    // The log's own row refuses a refiling by an actor whose party is the maker's name.
    let history: Vec<_> = core.log().entries().to_vec();
    let deployment_operator = DeploymentOperator {
        maker: operator.acts.clone(),
        party: operator.party.clone(),
    };
    let refiling_by = |who: &Actor| {
        Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: who.clone(),
            what: What::Refiling {
                arrival: arrival.clone(),
                order: order.clone(),
            },
            when: When {
                at: core.clock.timestamp(),
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::Observed {},
        })
    };
    assert!(
        admit_entry(
            &history,
            &refiling_by(&maker_as_party),
            Through::General,
            &deployment_operator
        )
        .is_err(),
        "the log refuses an actor whose party is the operator's maker"
    );
    assert!(
        admit_entry(
            &history,
            &refiling_by(&kit::operator_login()),
            Through::General,
            &deployment_operator
        )
        .is_ok(),
        "the log admits the operator's login"
    );
    // Core refuses the stranger before the log, and the operator's login's refiling is appended to the real log.
    assert!(matches!(
        core.refile(&maker_as_party, &arrival),
        Err(StepError::Arrival(ArrivalError::NotTheFiler))
    ));
    let refiled = core
        .refile(&kit::operator_login(), &arrival)
        .expect("the operator's login files it anew");
    let recorded = core
        .log()
        .entries()
        .iter()
        .find(|(r, _)| r == &refiled)
        .map(|(_, e)| e.clone())
        .expect("in the log");
    assert!(matches!(
        recorded,
        Entry::Verified(VerifiedEntry { what: What::Refiling { .. }, ref who, .. }) if who == &kit::operator_login()
    ));
}
