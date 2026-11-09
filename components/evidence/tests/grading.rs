//! Grading an outcome's evidence against the grade its provider declared (ADR-0007, `DEC-13`).

use rutent_contracts::provider_role::{EvidenceGrade, EvidencePiece};
mod common;

use common::{other_receipt, public_record, publisher};
use rutent_evidence::{Check, Claim, GradeExt, MovementId, Refusal, grade_outcome, receipt_check};

fn piece(grade: EvidenceGrade, by: &str) -> EvidencePiece {
    EvidencePiece {
        grade,
        reference: "ref-1".into(),
        by: by.into(),
    }
}

fn claim(grade: EvidenceGrade, check: Check) -> Claim {
    Claim {
        piece: piece(grade, "acme-co"),
        check,
    }
}

// verifies: REQ-ORD-39
#[test]
fn statement_alone_is_graded_as_a_statement_and_not_as_proof() {
    let graded = grade_outcome(
        EvidenceGrade::Statement,
        vec![claim(EvidenceGrade::Statement, Check::unchecked())],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::Statement);
    assert!(!graded.is_proof());
}

// verifies: REQ-ORD-39
#[test]
fn unchecked_claim_of_proof_is_graded_as_a_statement() {
    let graded = grade_outcome(
        EvidenceGrade::PublicRecord,
        vec![
            claim(EvidenceGrade::PublicRecord, Check::unchecked()),
            claim(EvidenceGrade::SignedByAnotherParty, Check::unchecked()),
        ],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::Statement);
    assert!(!graded.is_proof());
}

// verifies: REQ-ORD-39, DEC-13
#[test]
fn checked_public_record_is_graded_as_one() {
    let graded = grade_outcome(
        EvidenceGrade::PublicRecord,
        vec![
            claim(EvidenceGrade::Statement, Check::unchecked()),
            claim(EvidenceGrade::PublicRecord, public_record("tx-1")),
        ],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::PublicRecord);
    assert!(graded.is_proof());
}

// verifies: REQ-ORD-39, DEC-13
#[test]
fn claim_above_the_providers_declared_grade_is_refused() {
    let refused = grade_outcome(
        EvidenceGrade::SignedByProvider,
        vec![claim(EvidenceGrade::PublicRecord, public_record("tx-1"))],
    )
    .unwrap_err();
    assert_eq!(
        refused,
        Refusal::ClaimAboveDeclared {
            claimed: EvidenceGrade::PublicRecord,
            declared: EvidenceGrade::SignedByProvider,
            reference: "ref-1".into(),
        }
    );
}

// verifies: REQ-ORD-39
#[test]
fn outcome_with_no_evidence_is_refused() {
    assert_eq!(
        grade_outcome(EvidenceGrade::Statement, vec![]),
        Err(Refusal::NoEvidence)
    );
}

// verifies: DEC-13
#[test]
fn receipt_from_the_same_publisher_is_not_another_partys_receipt() {
    assert_eq!(
        receipt_check(&publisher("acme"), &publisher("acme")),
        Check::unchecked()
    );
    assert_eq!(
        receipt_check(&publisher("acme"), &publisher("other")),
        other_receipt()
    );
    // The same publisher under another spelling is still the same publisher.
    assert_eq!(
        receipt_check(&publisher("acme"), &publisher(" ACME ")),
        Check::unchecked()
    );
    let graded = grade_outcome(
        EvidenceGrade::SignedByAnotherParty,
        vec![claim(
            EvidenceGrade::SignedByAnotherParty,
            receipt_check(&publisher("acme"), &publisher("acme")),
        )],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::Statement);
}

// verifies: DEC-13
#[test]
fn receipt_from_a_different_publisher_is_graded_as_two() {
    let graded = grade_outcome(
        EvidenceGrade::SignedByAnotherParty,
        vec![claim(EvidenceGrade::SignedByAnotherParty, other_receipt())],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::SignedByAnotherParty);
    assert!(graded.is_proof());
}

// verifies: REQ-ORD-39
#[test]
fn provider_signed_evidence_is_graded_three_and_is_not_proof() {
    let graded = grade_outcome(
        EvidenceGrade::SignedByProvider,
        vec![claim(EvidenceGrade::SignedByProvider, Check::unchecked())],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::SignedByProvider);
    assert!(!graded.is_proof());
    assert!(EvidenceGrade::PublicRecord.is_stronger_than(EvidenceGrade::Statement));
}

// verifies: REQ-ORD-39, DEC-13
#[test]
fn one_movement_backing_two_pieces_is_refused() {
    let refused = grade_outcome(
        EvidenceGrade::PublicRecord,
        vec![
            claim(EvidenceGrade::PublicRecord, public_record("tx-1")),
            claim(EvidenceGrade::PublicRecord, public_record("tx-1")),
        ],
    )
    .unwrap_err();
    assert_eq!(
        refused,
        Refusal::MovementReused(MovementId::new("tx-1").unwrap())
    );
}

// verifies: DEC-13
#[test]
fn one_movement_named_for_two_segments_of_a_route_is_refused() {
    let tx1 = MovementId::new("tx-1").unwrap();
    let tx2 = MovementId::new("tx-2").unwrap();
    assert_eq!(rutent_evidence::distinct_movements([&tx1, &tx2]), Ok(()));
    assert_eq!(
        rutent_evidence::distinct_movements([&tx1, &tx2, &tx1]),
        Err(Refusal::MovementReused(tx1.clone()))
    );
}

// verifies: REQ-ORD-39
#[test]
fn distinct_movements_each_back_their_own_piece() {
    let graded = grade_outcome(
        EvidenceGrade::PublicRecord,
        vec![
            claim(EvidenceGrade::PublicRecord, public_record("tx-1")),
            claim(EvidenceGrade::PublicRecord, public_record("tx-2")),
        ],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::PublicRecord);
}
