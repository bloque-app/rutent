//! An automatic deposit's destination as its reserve names it (`T-020-50`; finding 5 of `M3`'s milestone review of
//! 2026-11-24, item `automatic-deposit-destination`): the reserve's signed arrival notice names where the deposit goes,
//! from the parameters its customer configured, who set them and for whom; Core's filing takes that destination in
//! place of the holder rule, still through the destination's resolution (`REQ-ORD-24`) and inside the authorisation's
//! terms, and the order's record names the reserve's statement the destination came from.

use rutent_conformance::role::BankCredit;
use rutent_conformance::signing::{real, sign_value, signed};
use rutent_conformance::sim::{Role as SimRole, Spec};
use rutent_contracts::core_internal::{Destination, Entry, Parties, What};
use rutent_contracts::provider_role::{Denomination, KeyPurpose, OuterNode};
use rutent_core_orch::arrival::{ArrivalError, Arrived, Filed, Filing, Report};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged, StepError, context};
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};
use serde_json::{Value, json};

/// The account the customer's standing instruction at the reserve sends automatic deposits to.
const STANDING_ACCOUNT: &str = "acct-standing";

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

/// The destination the reserve names in a notice: the standing account, set by `set_by` for `set_for`.
fn named(set_by: &str, set_for: &str) -> Value {
    let request = base_request("req-shape", AMOUNT, &id("session-shape"));
    let asset: Denomination = request.route.to.asset.clone();
    json!({
        "reserve": RESERVE,
        "node": OuterNode { network: format!("{RESERVE}-bank") },
        "account": STANDING_ACCOUNT,
        "asset": asset,
        "set_by": set_by,
        "set_for": set_for,
    })
}

/// The reserve's signed notice that `AMOUNT` arrived at the holder's account, naming `destination` where one is given.
fn report(core: &Core, destination: Option<Value>) -> Report {
    let request = base_request("req-shape", AMOUNT, &id("session-shape"));
    let rutent_contracts::provider_role::NodeRef::Outer(node) = request.route.from.node.clone()
    else {
        panic!("the route starts at an outer node");
    };
    let asset: Denomination = request.route.from.asset.clone();
    let mut statement = json!({
        "transaction": "credit-1",
        "node": OuterNode { network: node.network },
        "asset": asset,
        "amount": AMOUNT,
        "account": context().account,
        "holder": kit::customer().party,
    });
    if let Some(destination) = destination {
        statement["destination"] = destination;
    }
    let mut notice = json!({
        "deployment": context().deployment,
        "protocol_version": rutent_contracts::provider_role::VERSION,
        "shape": rutent_contracts::provider_role::ARRIVAL_NOTICE_SHAPE,
        "maker": RESERVE,
        "made_at": core.clock.timestamp().as_str(),
        "signature": { "scheme": "ed25519", "key": "", "value": "" },
        "statement": statement,
    });
    sign_value(&mut notice, &statements_key());
    Report {
        reserve: id(RESERVE),
        notice: serde_json::to_vec(&notice).expect("bytes"),
    }
}

/// A core whose reserve starts routes on automatic deposits, the customer's standing account registered as theirs.
fn core() -> Core {
    core_of(Spec::of(SimRole::Reserve, RESERVE).starting_routes_on_automatic_deposits())
}

/// A core over `reserve`, the customer's standing account registered as theirs.
fn core_of(reserve: Spec) -> Core {
    let given = Given {
        providers: vec![reserve],
        ..Given::one_reserve(Staged::AtOnce)
    };
    let (core, _) = Scenario::new("Deposit destination", "AC-10", given).core(1, &mut Reference);
    core.registers().accounts.insert(
        STANDING_ACCOUNT.to_owned(),
        kit::customer().party.as_str().to_owned(),
    );
    core
}

/// The operator's filing on the deposit, delivered to `account` as the reserve's instruction names it.
fn filing_to(
    core: &mut Core,
    arrival: rutent_contracts::core_internal::Receipt,
    account: &str,
) -> Filing {
    let session = core.open_session(&kit::customer()).expect("a session");
    let mut request = base_request("req-deposit", AMOUNT, &session);
    request.destination = Destination {
        account: id(account),
        ..request.destination
    };
    request
        .inputs
        .insert("beneficiary.bank_account".to_owned(), account.to_owned());
    let customer = kit::customer().party;
    Filing {
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
    }
}

/// The notice the simulated reserve itself signs of a credit of `AMOUNT` to the customer's account, naming the
/// destination of the standing instruction an integrator set for the customer: the stand-in's own bytes, not hand-signed.
fn standin_report(reserve: &Spec) -> Report {
    let request = base_request("req-shape", AMOUNT, &id("session-shape"));
    let rutent_contracts::provider_role::NodeRef::Outer(node) = request.route.from.node.clone()
    else {
        panic!("the route starts at an outer node");
    };
    let mut standin = reserve.make(&context());
    assert!(standin.credit_arrival(&BankCredit {
        reference: "credit-1".to_owned(),
        node,
        asset: request.route.from.asset.clone(),
        amount: AMOUNT.to_owned(),
        account: context().account,
        holder: kit::customer().party.as_str().to_owned(),
    }));
    Report {
        reserve: id(RESERVE),
        // The stand-in signs with its placeholder key; the real key it stands for signs what the platform takes.
        notice: signed(
            standin
                .arrival_notice("credit-1")
                .expect("the reserve's notice")
                .bytes(),
        ),
    }
}

fn arrived(
    core: &mut Core,
    destination: Option<Value>,
) -> rutent_contracts::core_internal::Receipt {
    let reported = report(core, destination);
    let Arrived::NoOrder { arrival } = core.arrive(&reported).expect("the arrival is taken") else {
        panic!("the arrival opens no order");
    };
    arrival
}

/// The `arrival` entry that holds the deposit against `order`, and the order's authorisation.
fn record_of(core: &Core, order: &rutent_contracts::router_api::Id) -> (What, What) {
    let whats = core.verified_whats();
    let arrival = whats
        .iter()
        .find(|w| matches!(w, What::Arrival { order: Some(o), .. } if o == order))
        .expect("the order's arrival")
        .clone();
    let authorise = whats
        .iter()
        .find(
            |w| matches!(w, What::Authorise { authorisation, .. } if &authorisation.order == order),
        )
        .expect("the order's authorisation")
        .clone();
    (arrival, authorise)
}

// verifies: REQ-ORD-13
#[test]
fn automatic_deposit_goes_where_its_reserve_sets_it_from_the_customers_parameters() {
    // An integrator set the customer's standing instruction at the reserve; its own notice names the destination.
    let customer = kit::customer().party;
    let reserve = Spec::of(SimRole::Reserve, RESERVE)
        .starting_routes_on_automatic_deposits()
        .with_deposit_instruction(STANDING_ACCOUNT, "integrator-1", customer.as_str());
    let mut core = core_of(reserve.clone());
    let reported = standin_report(&reserve);
    let Arrived::NoOrder { arrival } = core.arrive(&reported).expect("the arrival is taken") else {
        panic!("the arrival opens no order");
    };
    // Another account the holder owns is no destination the reserve named: refused, the deposit still free.
    let elsewhere = filing_to(
        &mut core,
        arrival.clone(),
        rutent_scenarios::assembly::DESTINATION_ACCOUNT,
    );
    assert!(matches!(
        core.file(elsewhere),
        Ok(Filed::CantProceed { order: None, .. })
    ));
    let mut filing = filing_to(&mut core, arrival, STANDING_ACCOUNT);
    filing.request = id("deposit-2");
    let Ok(Filed::Started { order, .. }) = core.file(filing) else {
        panic!("the deposit starts at the destination its reserve names");
    };
    let (held, authorise) = record_of(&core, &order);
    let What::Authorise { authorisation, .. } = authorise else {
        panic!("an authorisation");
    };
    assert_eq!(authorisation.destination.account.as_str(), STANDING_ACCOUNT);
    let What::Arrival {
        destination: Some(named),
        ..
    } = held
    else {
        panic!("the arrival names the destination");
    };
    assert_eq!(named.set_by, "integrator-1");
}

// verifies: REQ-ORD-24
#[test]
fn automatic_deposit_whose_reserve_names_no_destination_goes_to_its_holder() {
    let mut core = core();
    let arrival = arrived(&mut core, None);
    let filing = filing_to(
        &mut core,
        arrival,
        rutent_scenarios::assembly::DESTINATION_ACCOUNT,
    );
    let Ok(Filed::Started { order, .. }) = core.file(filing) else {
        panic!("the deposit starts");
    };
    let (arrival, _) = record_of(&core, &order);
    assert!(matches!(
        arrival,
        What::Arrival {
            destination: None,
            ..
        }
    ));
    // Another owner's account is still no destination for the holder's deposit (`REQ-ORD-24`).
    let mut core = self::core();
    core.registers()
        .accounts
        .insert(STANDING_ACCOUNT.to_owned(), "someone-else".to_owned());
    let arrival = arrived(&mut core, None);
    let filing = filing_to(&mut core, arrival, STANDING_ACCOUNT);
    assert!(matches!(
        core.file(filing),
        Ok(Filed::CantProceed { order: None, .. })
    ));
}

// verifies: REQ-AUZ-9
#[test]
fn destination_named_by_a_reserve_that_never_declared_automatic_deposits_is_refused() {
    let (mut core, _) = Scenario::new(
        "Deposit destination",
        "AC-10",
        Given::one_reserve(Staged::AtOnce),
    )
    .core(1, &mut Reference);
    core.registers().accounts.insert(
        STANDING_ACCOUNT.to_owned(),
        kit::customer().party.as_str().to_owned(),
    );
    let customer = kit::customer().party;
    let arrival = arrived(&mut core, Some(named(customer.as_str(), customer.as_str())));
    let filing = filing_to(&mut core, arrival, STANDING_ACCOUNT);
    let entries = core.verified_whats().len();
    assert!(matches!(
        core.file(filing),
        Err(StepError::Arrival(ArrivalError::NotDeclared))
    ));
    assert_eq!(core.verified_whats().len(), entries, "nothing recorded");
}

// verifies: REQ-ORD-15
#[test]
fn orders_record_names_the_reserves_statement_its_destination_came_from() {
    let mut core = core();
    let customer = kit::customer().party;
    let arrival = arrived(&mut core, Some(named(customer.as_str(), customer.as_str())));
    let filing = filing_to(&mut core, arrival.clone(), STANDING_ACCOUNT);
    let Ok(Filed::Started { order, .. }) = core.file(filing) else {
        panic!("the deposit starts");
    };
    let (held, _) = record_of(&core, &order);
    let What::Arrival {
        notice: Some(notice),
        destination: Some(destination),
        earlier,
        ..
    } = held
    else {
        panic!("the order's arrival names the notice and the destination");
    };
    // The notice it names is the reserve's stated entry the destination was read from, by its commitment.
    let log = core.log();
    let stated = log
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s) if s.commitment == notice => Some(s.clone()),
            _ => None,
        })
        .expect("the reserve's notice is in the log");
    assert_eq!(
        stated.attribution.maker().acts.as_str(),
        RESERVE,
        "the reserve's statement"
    );
    assert_eq!(destination.account, STANDING_ACCOUNT);
    assert_eq!(destination.set_for, customer.as_str());
    assert_eq!(earlier, Some(arrival), "the deposit as first recorded");
}

/// A destination set for another party is no destination for this holder's deposit; so is one the filing doesn't name.
// verifies: REQ-AUZ-9, REQ-ORD-13
#[test]
fn destination_set_for_another_party_or_not_the_filings_is_refused() {
    let customer = kit::customer().party;
    // Set for someone else: refused before an order opens.
    let mut core = core();
    let arrival = arrived(&mut core, Some(named(customer.as_str(), "someone-else")));
    let filing = filing_to(&mut core, arrival, STANDING_ACCOUNT);
    let entries = core.verified_whats().len();
    assert!(matches!(
        core.file(filing),
        Ok(Filed::CantProceed { order: None, .. })
    ));
    assert!(
        !core
            .verified_whats()
            .iter()
            .skip(entries)
            .any(|w| matches!(w, What::Authorise { .. } | What::Acceptance { .. })),
        "no order opened"
    );
    // The filing names the holder's default where the notice names the standing account: not the reserve's.
    let mut core = self::core();
    let arrival = arrived(&mut core, Some(named(customer.as_str(), customer.as_str())));
    let filing = filing_to(
        &mut core,
        arrival,
        rutent_scenarios::assembly::DESTINATION_ACCOUNT,
    );
    assert!(matches!(
        core.file(filing),
        Ok(Filed::CantProceed { order: None, .. })
    ));
}
