//! The stand-in reserve states what arrived (`T-024-15`, `REQ-ORD-1`, `REQ-ORD-14`, `REQ-ORD-23`): it signs an arrival
//! notice of a credit at its simulated bank, answers the question of what arrived under the question's transaction,
//! and knows two notices of one credit as one. The role suite holds what a reserve signs to the contract's schemas'
//! types and to `check_answer` and `check_one_credit`, so a reserve that departs from them is flagged.

#![cfg(feature = "signing")]

use rutent_conformance::role::stand_in::Reserve;
use rutent_conformance::role::{
    BankCredit, Check, Exercised, Received, RoleHarness, RoleProvider, SuiteContext, WithoutHooks,
    run_role_suite,
};
use rutent_conformance::signing::{Ed25519Verifier, Signing};
use rutent_contracts::provider_role::{
    ARRIVAL_QUESTION_SHAPE, ArrivalAnswer, ArrivalNotice, ArrivalOutcome, ArrivalQuestion, Credits,
    Denomination, NodeRef, OuterNode, Signature, SignatureScheme, Statement, check_answer,
    check_one_credit,
};

/// The bank network, the asset and the unit of the stand-in reserve's first tuple.
fn bank_of(reserve: &Reserve) -> (OuterNode, Denomination) {
    let advertisement = reserve.advertisement().into_statement().statement;
    let tuple = advertisement
        .offers
        .reserve
        .expect("a reserve")
        .tuples
        .into_iter()
        .next()
        .expect("a tuple");
    let NodeRef::Outer(node) = tuple.origin else {
        panic!("the first tuple starts at the bank");
    };
    (
        node,
        Denomination {
            id: tuple.asset,
            decimals: tuple.decimals.origin,
        },
    )
}

fn credit(reserve: &Reserve, context: &SuiteContext, reference: &str, amount: &str) -> BankCredit {
    let (node, asset) = bank_of(reserve);
    BankCredit {
        reference: reference.to_owned(),
        node,
        asset,
        amount: amount.to_owned(),
        account: context.account.clone(),
        holder: "the-holder".to_owned(),
    }
}

fn question(
    context: &SuiteContext,
    reserve: &Reserve,
    transaction: &str,
    account: &str,
) -> Statement<ArrivalQuestion> {
    let (node, asset) = bank_of(reserve);
    Statement {
        deployment: context.deployment.clone(),
        protocol_version: context.protocol_version.clone(),
        shape: ARRIVAL_QUESTION_SHAPE.to_owned(),
        maker: context.operator.clone(),
        made_at: "2026-11-06T15:04:05Z".to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: context.deployment.operator_key.clone(),
            value: rutent_conformance::role::placeholder_signature(),
        },
        statement: ArrivalQuestion {
            transaction: transaction.to_owned(),
            provider: Reserve::PROVIDER.to_owned(),
            session: context.session.clone(),
            account: account.to_owned(),
            node,
            asset,
        },
    }
}

fn answer(reserve: &mut Reserve, question: &Statement<ArrivalQuestion>) -> Received<ArrivalAnswer> {
    reserve.arrival(question).expect("the reserve answers")
}

fn notices(answer: &Statement<ArrivalAnswer>) -> Vec<Statement<ArrivalNotice>> {
    match &answer.statement.outcome {
        ArrivalOutcome::Arrived(arrived) => arrived.notices.clone(),
        other => panic!("expected credits, got {other:?}"),
    }
}

// verifies: REQ-ORD-14, REQ-ORD-23
#[test]
fn stand_in_reserve_signs_an_arrival_notice_that_passes_the_role_suite() {
    let context = SuiteContext::default();
    // The suite credits the reserve's bank, asks what arrived, and holds what it signs: with every signature verified.
    let report = run_role_suite(
        &mut Signing(Reserve::new(&context)),
        &context,
        &Ed25519Verifier,
    );
    assert!(
        report.passed(),
        "{:?} {:?}",
        report.findings,
        report.uncovered
    );
    assert!(report.exercised.contains(&Exercised::ArrivalNoticed));
    assert!(report.exercised.contains(&Exercised::NothingArrived));

    // The notice states the bank's credit, in the contract's shape, made by the reserve.
    let mut reserve = Reserve::new(&context);
    let bank = credit(&reserve, &context, "credit-1", "123456");
    assert!(reserve.credit_arrival(&bank));
    let notice = reserve.arrival_notice("credit-1").expect("a notice");
    let notice = notice.statement();
    assert_eq!(
        notice.shape,
        rutent_contracts::provider_role::ARRIVAL_NOTICE_SHAPE
    );
    assert_eq!(notice.maker, Reserve::PROVIDER);
    assert!(notice.check_shape().is_ok());
    let body = &notice.statement;
    assert_eq!(body.transaction, "credit-1");
    assert_eq!(body.node, bank.node);
    assert_eq!(body.asset, bank.asset);
    assert_eq!(body.amount, "123456");
    assert_eq!(body.account, bank.account);
    assert_eq!(body.holder, "the-holder");
    // A credit the bank never made is no notice.
    assert!(reserve.arrival_notice("credit-2").is_none());

    // A reserve that states a notice of an account the credit didn't reach, or under another question, is flagged.
    for (name, flawed) in [
        (
            "a reserve stating credits of any account",
            Reserve::new(&context).stating_credits_of_any_account(),
        ),
        (
            "a reserve answering under another transaction",
            Reserve::new(&context).answering_under_another_transaction(),
        ),
        (
            "a reserve answering questions from anyone",
            Reserve::new(&context).answering_questions_from_anyone(),
        ),
        (
            "a reserve answering in any session",
            Reserve::new(&context).answering_arrivals_in_any_session(),
        ),
    ] {
        let report = run_role_suite(&mut Signing(flawed), &context, &Ed25519Verifier);
        assert!(
            report.flagged().contains(&Check::ArrivalsStated),
            "{name}: {:?}",
            report.findings
        );
    }
    // Hookless, the same reserve breaks nothing: what a hook provokes is left uncovered.
    let hookless = run_role_suite(
        &mut WithoutHooks(Signing(Reserve::new(&context))),
        &context,
        &Ed25519Verifier,
    );
    assert!(hookless.findings.is_empty(), "{:?}", hookless.findings);
    assert!(hookless.exercised.contains(&Exercised::NothingArrived));
}

// verifies: REQ-ORD-1
#[test]
fn stand_in_reserve_answers_the_arrival_question_under_the_questions_transaction() {
    let context = SuiteContext::default();
    let mut reserve = Reserve::new(&context);

    // Before any credit, nothing arrived, under the question's transaction.
    let asked = question(&context, &reserve, "question-1", &context.account);
    let nothing = answer(&mut reserve, &asked);
    assert_eq!(nothing.statement().statement.transaction, "question-1");
    assert!(matches!(
        nothing.statement().statement.outcome,
        ArrivalOutcome::NothingArrived(_)
    ));
    assert!(check_answer(&asked, nothing.statement()).is_ok());

    // After one, the notice of it, under the question's transaction and not the credit's.
    let bank = credit(&reserve, &context, "credit-1", "5000");
    assert!(reserve.credit_arrival(&bank));
    let asked = question(&context, &reserve, "question-2", &context.account);
    let arrived = answer(&mut reserve, &asked);
    assert_eq!(arrived.statement().statement.transaction, "question-2");
    assert!(check_answer(&asked, arrived.statement()).is_ok());
    let carried = notices(arrived.statement());
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].statement.transaction, "credit-1");
    assert_eq!(carried[0].statement.amount, "5000");

    // Asked of another account, nothing arrived.
    let asked = question(&context, &reserve, "question-3", "another-account");
    let other = answer(&mut reserve, &asked);
    assert!(matches!(
        other.statement().statement.outcome,
        ArrivalOutcome::NothingArrived(_)
    ));
    assert_eq!(other.statement().statement.transaction, "question-3");
}

// verifies: REQ-ORD-23
#[test]
fn two_notices_of_one_credit_from_the_stand_in_reserve_are_one_credit() {
    let context = SuiteContext::default();
    let mut reserve = Reserve::new(&context);
    let bank = credit(&reserve, &context, "credit-1", "5000");
    assert!(reserve.credit_arrival(&bank));
    let second = credit(&reserve, &context, "credit-2", "5000");
    assert!(reserve.credit_arrival(&second));

    let of_its_own_accord = reserve.arrival_notice("credit-1").unwrap();
    let asked = question(&context, &reserve, "question-1", &context.account);
    let carried = notices(answer(&mut reserve, &asked).statement());
    assert_eq!(carried.len(), 2, "each credit has a notice of its own");
    let again = reserve.arrival_notice("credit-1").unwrap();
    let one = carried
        .iter()
        .find(|n| n.statement.transaction == "credit-1")
        .unwrap();
    assert!(matches!(
        check_one_credit(of_its_own_accord.statement(), one),
        Ok(Credits::One)
    ));
    assert!(matches!(
        check_one_credit(one, again.statement()),
        Ok(Credits::One)
    ));
    // Another credit of the same figures is another credit.
    let other = carried
        .iter()
        .find(|n| n.statement.transaction == "credit-2")
        .unwrap();
    assert!(matches!(check_one_credit(one, other), Ok(Credits::Two)));

    // A reserve that restates a credit with other figures, or under a fresh reference each time, is flagged.
    for (name, flawed) in [
        (
            "a reserve restating a credit with another amount",
            Reserve::new(&context).restating_a_credit_with_another_amount(),
        ),
        (
            "a reserve giving each notice a reference of its own",
            Reserve::new(&context).giving_each_notice_a_new_reference(),
        ),
    ] {
        let report = run_role_suite(&mut Signing(flawed), &context, &Ed25519Verifier);
        assert!(
            report.flagged().contains(&Check::ArrivalsStated),
            "{name}: {:?}",
            report.findings
        );
    }
}

#[allow(dead_code)]
fn _is_harness(_: &dyn RoleHarness) {}
