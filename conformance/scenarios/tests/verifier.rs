//! The verifier half of `REQ-CPL-12` and of `AC-24`, at the assembly with the verifier stand-in (`T-024-12`, `DEC-16`):
//! a requester, a beneficiary and a payer checked by the judgements their providers' rules name, a tuple offered only
//! with a cited judgement for each need (`REQ-AUZ-5`), and a provider relying on a verifier's judgement leaving the
//! `reliance` entry Core writes (`T-020-23`), naming the judge and the relying provider. A false judgement, the
//! verifier saying yes without checking, is recorded as its maker's word and its relier's reliance.
//!
//! The verifier stand-in is the role only: it is no part of the catalogue's routes, takes no script and has no book.
//! The routing asks it by the order's needs (`rounds::Need`), as provider-role 3.0.0's tuples name no verifier.

use std::time::Duration;

use rutent_conformance::sim::{Role, Spec};
use rutent_contracts::core_internal::{Action, Entry, Parties, Receipt, What};
use rutent_contracts::provider_role::JUDGEMENT_SHAPE;
use rutent_contracts::router_api::{DecisionRecord, FigureValue, Id};
use rutent_core_orch::intake::{OrderRequest, Status, Told};
use rutent_core_orch::rounds::Need;
use rutent_scenarios::assembly::machine::{Delivery, Reference};
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::assembly::{DESTINATION_ACCOUNT, ORIGIN_ACCOUNT};
use rutent_scenarios::harness::{Given, Scenario, base_request};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};

mod execution_support;
use execution_support::{core_of, outer, request as route_request, spec_of};

const CLASS: &str = "kyc.natural_person";
const VERIFIER: &str = "sim-verifier";
const LAX: &str = "sim-verifier-lax";
// The two verifiers of the route scenario, whose placeholder keys differ from the route providers' ([`Spec::seed`]).
const JUDGE_A: &str = "sim-verifier-b";
const JUDGE_B: &str = "sim-verifier-c";
const PAYER_INPUT: &str = "payer.bank_account";
const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const RAMP: &str = "sim-join-ramp";
const OUT: &str = "sim-join-out";
const ROUTE: [&str; 4] = [IN, EXCHANGE, RAMP, OUT];

/// The order that carries `AMOUNT` of COP from `sim-join-in`'s bank to USD at `sim-join-out`'s.
fn across(name: &str, session: &Id) -> OrderRequest {
    route_request(
        name,
        session,
        AMOUNT,
        IN,
        outer("sim-join-in-bank", "COP"),
        outer("sim-join-out-bank", "USD"),
        OUT,
        DESTINATION_ACCOUNT,
        "lowest_total_cost",
    )
}

fn core_with(verifiers: &[&str]) -> (Core, Id) {
    let mut providers = vec![Spec::of(Role::Reserve, RESERVE)];
    providers.extend(verifiers.iter().map(|v| Spec::of(Role::Verifier, v)));
    let given = Given {
        providers,
        staging: Staged::AtOnce,
        deadline: Duration::from_secs(2),
        delivery: Delivery::Once,
    };
    let (mut core, _) = Scenario::new("verifier", "AC-24", given).core(1, &mut Reference);
    let session = core.open_session(&kit::customer()).expect("a session");
    (core, session)
}

fn withdrawal() -> rutent_contracts::router_api::TupleKey {
    kit::tuple(RESERVE, kit::bank(RESERVE), kit::ledger(RESERVE))
}

fn deposit() -> rutent_contracts::router_api::TupleKey {
    kit::tuple(RESERVE, kit::ledger(RESERVE), kit::bank(RESERVE))
}

fn need(
    tuple: rutent_contracts::router_api::TupleKey,
    person: &str,
    verifier: &str,
    subject: &str,
) -> Need {
    Need {
        tuple,
        person: id(person),
        verifier: id(verifier),
        subject: subject.to_owned(),
        class: CLASS.to_owned(),
    }
}

/// The order `request` opens, quoted; what quoting told.
fn quoted(core: &mut Core, request: OrderRequest) -> (Id, Told) {
    let told = core.request(request).expect("opens");
    let order = Core::order_of(&told).expect("an order");
    let told = core.quote(&kit::customer(), &order).expect("quoted");
    (order, told)
}

fn offered(told: &Told) -> bool {
    matches!(
        told,
        Told::Exists {
            status: Status::Quoted { .. },
            ..
        }
    )
}

fn accepted(core: &mut Core, order: &Id, told: &Told) {
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("a quote is shown: {told:?}");
    };
    core.accept(&kit::customer(), order, digest)
        .expect("accepted");
}

/// The reliance entries of the log: where each stands, and what it names.
fn reliances(core: &Core) -> Vec<(usize, Receipt, String, String, Id)> {
    core.log()
        .entries()
        .iter()
        .enumerate()
        .filter_map(|(at, (_, e))| match e {
            Entry::Verified(v) => match &v.what {
                What::Reliance {
                    order,
                    judgement,
                    judge,
                    relied_on_by,
                    ..
                } => Some((
                    at,
                    judgement.clone(),
                    judge.as_str().to_owned(),
                    relied_on_by.as_str().to_owned(),
                    order.clone(),
                )),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect()
}

/// Where the log holds the first effect that moves value at the reserve.
fn first_move(core: &Core) -> usize {
    core.log()
        .entries()
        .iter()
        .position(|(_, e)| {
            matches!(e, Entry::Verified(v)
                if matches!(&v.what, What::Effect { action: Action::MoveValue { .. }, .. }))
        })
        .expect("a move of value")
}

fn record_of(core: &Core, told: &Told) -> DecisionRecord {
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

/// The judgements the chosen route's operations cite, per operation: its kind, and each judgement's maker and
/// subject.
fn cited(record: &DecisionRecord) -> Vec<(String, Vec<(String, String)>)> {
    let chosen = record.chosen().expect("a route was chosen");
    chosen
        .parts
        .iter()
        .flat_map(|p| p.operations.iter())
        .map(|operation| {
            let judgements = operation
                .figures
                .iter()
                .filter_map(|id| record.snapshot.figures.iter().find(|f| &f.figure_id == id))
                .filter_map(|f| match &f.value {
                    FigureValue::Eligibility { judgement, .. } => {
                        Some((judgement.maker.clone(), judgement.statement.subject.clone()))
                    }
                    _ => None,
                })
                .collect();
            (format!("{:?}", operation.kind), judgements)
        })
        .collect()
}

/// A request whose payer is the party `payer` and whose withdrawal needs the verifier's judgement of it, run to
/// the end: the order, the quote and the log's judgement.
fn judged_and_run(verifiers: &[&str], verifier: &str, subject: &str) -> (Core, Id, Told) {
    let (mut core, session) = core_with(verifiers);
    let mut request = base_request("req-judged", AMOUNT, &session);
    request.needs.push(need(
        withdrawal(),
        kit::customer().party.as_str(),
        verifier,
        subject,
    ));
    let (order, told) = quoted(&mut core, request);
    assert!(offered(&told), "offered with the judgement: {told:?}");
    accepted(&mut core, &order, &told);
    let judgement = core
        .judgement_of(verifier)
        .expect("the verifier's judgement");
    core.rests_on(&order, PAYER_INPUT, judgement)
        .expect("declared before anything is handed over");
    core.run(&order).expect("runs");
    (core, order, told)
}

/// A provider that relied on a verifier's judgement leaves a `reliance` entry naming the verifier, the judgement and
/// the provider, written before the operation it relied on was sent; the verifier's stated judgement is the one the
/// entry cites, and no record charges its loss to the operator or the customer.
// verifies: REQ-CPL-12
#[test]
fn provider_relying_on_a_verifiers_judgement_leaves_a_reliance_entry_naming_the_verifier_and_the_provider()
 {
    let (core, order, _) = judged_and_run(&[VERIFIER], VERIFIER, "payer-ref");
    let judgement = core.judgement_of(VERIFIER).expect("the judgement");
    let written = reliances(&core);
    assert_eq!(
        written.len(),
        1,
        "one judgement, one provider, one reliance: {written:?}"
    );
    let (at, cited_receipt, judge, relied_on_by, of) = written.into_iter().next().unwrap();
    assert_eq!(
        cited_receipt, judgement.receipt,
        "it cites the verifier's judgement"
    );
    assert_eq!(judge, VERIFIER, "its maker, the verifier");
    assert_eq!(relied_on_by, RESERVE, "the provider that relied on it");
    assert_eq!(of, order);
    assert!(
        at < first_move(&core),
        "written before the value moved at the provider that relies on it"
    );

    // Neither the operator nor the customer is the judge or the relier, and no other record cites the judgement: the
    // loss of a false one has no entry that puts it on them.
    for party in ["operator", kit::customer().party.as_str(), kit::CORE_PARTY] {
        assert_ne!(party, judge);
        assert_ne!(party, relied_on_by);
    }
    let log = core.log();
    let digest = serde_json::to_string(&judgement.receipt.entry).unwrap();
    let citing = log
        .entries()
        .iter()
        .filter(|(_, e)| serde_json::to_string(e).unwrap().contains(&digest))
        .count();
    assert_eq!(
        citing, 1,
        "the reliance cites the judgement, and no other entry of the log does"
    );
    let stated: Vec<_> = log
        .entries()
        .iter()
        .filter(
            |(_, e)| matches!(e, Entry::Stated(s) if s.what.statement.as_str() == JUDGEMENT_SHAPE),
        )
        .collect();
    assert_eq!(stated.len(), 1, "the one judgement the order asked for");
    // The log here is the real one, which admits an entry as it takes it: the reliance is in it, so it was admitted.
}

/// A verifier that says yes without checking states a false judgement, which the record keeps as its word: the
/// reliance names it as the judge and the reserve that relied on it, as it names an honest verifier, and the same
/// subject put to an honest verifier is not judged fit.
// verifies: REQ-CPL-12
#[test]
fn false_judgement_is_recorded_as_its_verifiers_word_and_its_reliers_reliance() {
    use rutent_contracts::provider_role::JudgementAnswer;
    let subject = "unfit-payer";
    // The answers of the judgements the routing's records carry whole, by maker.
    let answer_of = |core: &Core, verifier: &str| -> JudgementAnswer {
        core.decision_records()
            .iter()
            .flat_map(|r| r.snapshot.figures.iter())
            .find_map(|f| match &f.value {
                FigureValue::Eligibility { judgement, .. } if judgement.maker == verifier => {
                    Some(judgement.statement.answer)
                }
                _ => None,
            })
            .expect("a judgement in a record")
    };

    // The false yes: the order is offered, run, and the record is whole.
    let (core, order, told) = judged_and_run(&[LAX], LAX, subject);
    assert_eq!(
        answer_of(&core, LAX),
        JudgementAnswer::Yes,
        "said yes to what it didn't check"
    );
    assert_eq!(
        cited(&record_of(&core, &told))[0].1,
        vec![(LAX.to_owned(), subject.to_owned())],
        "the route cites the verifier's word about that subject"
    );
    let written = reliances(&core);
    assert_eq!(written.len(), 1, "{written:?}");
    let (_, receipt, judge, relied_on_by, of) = written.into_iter().next().unwrap();
    assert_eq!(receipt, core.judgement_of(LAX).unwrap().receipt);
    assert_eq!((judge.as_str(), relied_on_by.as_str()), (LAX, RESERVE));
    assert_eq!(of, order);
    assert!(
        core.log().entries().iter().all(|(_, e)| !matches!(
            e,
            Entry::Verified(v) if matches!(&v.what, What::Reliance { relied_on_by, .. }
                if relied_on_by.as_str() == "operator" || relied_on_by.as_str() == kit::customer().party.as_str())
        )),
        "no reliance names the operator or the customer as the one that relied"
    );

    // The honest verifier, asked of the same subject, says no.
    let (mut core, session) = core_with(&[VERIFIER]);
    let mut request = base_request("req-honest", AMOUNT, &session);
    request.needs.push(need(
        withdrawal(),
        kit::customer().party.as_str(),
        VERIFIER,
        subject,
    ));
    let _ = quoted(&mut core, request);
    assert_eq!(answer_of(&core, VERIFIER), JudgementAnswer::No);
}

/// A requester, a beneficiary and a payer who differ are recorded apart, and each is checked by the judgement its
/// provider's rules name, over a route of four providers: the withdrawing reserve's rules name the payer, the
/// exchange's the requester, the depositing reserve's the beneficiary, each by the verifier its rule names, and the
/// ramp's name none. A provider judges one subject only (the snapshot's own rule), so each party is judged by its own.
// verifies: AC-24, REQ-AUZ-5
#[test]
fn requester_beneficiary_and_payer_are_checked_by_the_judgements_their_providers_rules_name() {
    let customer = kit::customer();
    let parties = Parties {
        requester: customer.party.clone(),
        beneficiary: id("acme-payee"),
        settlement_account_at: None,
        payer: id("acme-payer"),
    };
    let route = |name: &str, session: &Id| {
        let mut request = across(name, session);
        request.parties = parties.clone();
        request
    };
    let world = || {
        let mut providers: Vec<Spec> = ROUTE.iter().map(|n| spec_of(n)).collect();
        providers.extend([JUDGE_A, JUDGE_B].map(|v| Spec::of(Role::Verifier, v)));
        let (core, session) = core_of(
            "AC-24",
            Given {
                providers,
                staging: Staged::AtOnce,
                deadline: Duration::from_secs(2),
                delivery: Delivery::Once,
            },
        );
        // The payer's account at the origin and the beneficiary's at the destination, each held by its own party.
        {
            let mut registers = core.registers();
            registers
                .accounts
                .insert(ORIGIN_ACCOUNT.to_owned(), "acme-payer".to_owned());
            registers
                .accounts
                .insert(DESTINATION_ACCOUNT.to_owned(), "acme-payee".to_owned());
        }
        (core, session)
    };

    // The route the order takes with no need, to name each provider's tuple as its operation does.
    let (mut core, session) = world();
    let (_, told) = quoted(&mut core, route("req-bare", &session));
    assert!(offered(&told), "{told:?}");
    let tuples: Vec<rutent_contracts::router_api::TupleKey> = {
        let record = record_of(&core, &told);
        record.chosen().unwrap().parts[0]
            .operations
            .iter()
            .map(|o| rutent_contracts::router_api::TupleKey {
                provider: o.provider.clone(),
                from: o.from.clone(),
                to: o.to.clone(),
            })
            .collect()
    };
    assert_eq!(
        tuples
            .iter()
            .map(|t| t.provider.as_str())
            .collect::<Vec<_>>(),
        [IN, EXCHANGE, RAMP, OUT],
        "the route runs through four providers"
    );

    let (mut core, session) = world();
    let mut request = route("req-three", &session);
    request.needs = vec![
        Need {
            tuple: tuples[0].clone(),
            ..need(withdrawal(), "acme-payer", JUDGE_A, "payer-ref")
        },
        Need {
            tuple: tuples[1].clone(),
            ..need(
                withdrawal(),
                customer.party.as_str(),
                JUDGE_B,
                "requester-ref",
            )
        },
        Need {
            tuple: tuples[3].clone(),
            ..need(withdrawal(), "acme-payee", JUDGE_A, "beneficiary-ref")
        },
    ];
    let (order, told) = quoted(&mut core, request);
    assert!(offered(&told), "{told:?}");
    accepted(&mut core, &order, &told);

    // Each recorded apart.
    let recorded = core
        .verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::Acceptance {
                order: o, parties, ..
            } if o == order => Some(parties),
            _ => None,
        })
        .expect("the acceptance records its parties");
    assert_eq!(recorded, parties);

    // Each put to the verifier its provider's rule names, about the person it names.
    let mut questions: Vec<(String, String)> = core
        .verified_whats()
        .into_iter()
        .filter_map(|w| match w {
            What::AskAboutPerson { question } if question.order == order => Some((
                question.person.as_str().to_owned(),
                question.ask[0].as_str().to_owned(),
            )),
            _ => None,
        })
        .collect();
    questions.sort();
    assert_eq!(
        questions,
        vec![
            ("acme-customer".to_owned(), JUDGE_B.to_owned()),
            ("acme-payee".to_owned(), JUDGE_A.to_owned()),
            ("acme-payer".to_owned(), JUDGE_A.to_owned()),
        ],
        "policy was asked about each party once, of the verifier its rule names"
    );

    // And each operation cites the judgement of the party its provider's rules name, by the verifier named, and no more.
    assert_eq!(
        cited(&record_of(&core, &told)),
        vec![
            (
                "Withdrawal".to_owned(),
                vec![(JUDGE_A.to_owned(), "payer-ref".to_owned())]
            ),
            (
                "Exchange".to_owned(),
                vec![(JUDGE_B.to_owned(), "requester-ref".to_owned())]
            ),
            ("Route".to_owned(), vec![]),
            (
                "Deposit".to_owned(),
                vec![(JUDGE_A.to_owned(), "beneficiary-ref".to_owned())]
            ),
        ]
    );
}

/// A tuple whose rules need a judgement is not offered unless the route cites one for each need: where the verifier
/// is silent or unknown, where it answers about another class, or where one of two needs goes unmet, nothing is
/// offered and nothing is accepted; the same order with every judgement given is offered.
// verifies: REQ-AUZ-5
#[test]
fn tuple_needing_a_judgement_is_not_offered_without_one() {
    let customer = kit::customer();
    let try_with = |verifiers: &[&str], needs: Vec<Need>, name: &str| -> (Core, Told) {
        let (mut core, session) = core_with(verifiers);
        let mut request = base_request(name, AMOUNT, &session);
        request.needs = needs;
        let (_, told) = quoted(&mut core, request);
        (core, told)
    };
    let party = customer.party.as_str();

    // Control: the judgement given, the route is offered.
    let (_, told) = try_with(
        &[VERIFIER],
        vec![need(withdrawal(), party, VERIFIER, "r")],
        "ok",
    );
    assert!(offered(&told), "{told:?}");

    // A verifier no one installed: it answers nothing, so no judgement is cited.
    let (core, told) = try_with(
        &[],
        vec![need(withdrawal(), party, "no-such-verifier", "r")],
        "silent",
    );
    assert!(!offered(&told), "{told:?}");
    assert!(cited_none(&core), "no route was chosen on it");

    // A class the verifier doesn't judge: it refuses, and no judgement is cited.
    let mut other_class = need(withdrawal(), party, VERIFIER, "r");
    other_class.class = "kyc.legal_entity".to_owned();
    let (_, told) = try_with(&[VERIFIER], vec![other_class], "class");
    assert!(!offered(&told), "{told:?}");

    // Two needs of one tuple, one met: the unmet one holds the route back.
    let (_, told) = try_with(
        &[VERIFIER],
        vec![
            need(withdrawal(), party, VERIFIER, "r"),
            need(withdrawal(), party, "no-such-verifier", "r"),
        ],
        "half",
    );
    assert!(!offered(&told), "{told:?}");

    // A need on the deposit goes unmet where the withdrawal's is met: the route as a whole isn't offered.
    let (_, told) = try_with(
        &[VERIFIER],
        vec![
            need(withdrawal(), party, VERIFIER, "r"),
            need(deposit(), party, "no-such-verifier", "r"),
        ],
        "deposit",
    );
    assert!(!offered(&told), "{told:?}");
}

/// No acceptance stands in the log of an order that was not offered.
fn cited_none(core: &Core) -> bool {
    !core
        .verified_whats()
        .iter()
        .any(|w| matches!(w, What::Acceptance { .. } | What::Shown { .. }))
}
