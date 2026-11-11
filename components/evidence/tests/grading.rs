//! Grading an outcome's evidence against the grade its provider declared (ADR-0007, `DEC-13`).

use rutent_contracts::provider_role::{EvidenceGrade, EvidencePiece};
mod common;

use common::{
    NEXT, PROVIDER, Signer, other_receipt, public_record, publisher, signed, verified, verified_for,
};
use rutent_evidence::{
    Check, Claim, GradeExt, MovementId, PublisherId, Publishers, Receipt, ReceiptRefused,
    ReceiptVerifier, Refusal, grade_outcome, receipt_check, verify_receipt,
};
use std::cell::Cell;

fn piece(grade: EvidenceGrade, reference: &str, by: &str) -> EvidencePiece {
    EvidencePiece {
        grade,
        reference: reference.into(),
        by: by.into(),
    }
}

/// A piece the provider gives itself, with its check.
fn claim(grade: EvidenceGrade, reference: &str, check: Check) -> Claim {
    claim_by(grade, reference, PROVIDER, check)
}

/// A piece whose evidence is `by`'s, such as the next provider's receipt, with its check.
fn claim_by(grade: EvidenceGrade, reference: &str, by: &str, check: Check) -> Claim {
    Claim {
        piece: piece(grade, reference, by),
        check,
    }
}

// verifies: REQ-ORD-39
#[test]
fn statement_alone_is_graded_as_a_statement_and_not_as_proof() {
    let graded = grade_outcome(
        PROVIDER,
        EvidenceGrade::Statement,
        vec![claim(EvidenceGrade::Statement, "ref-1", Check::unchecked())],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::Statement);
    assert!(!graded.is_proof());
}

// verifies: REQ-ORD-39
#[test]
fn unchecked_claim_of_proof_is_graded_as_a_statement() {
    let graded = grade_outcome(
        PROVIDER,
        EvidenceGrade::PublicRecord,
        vec![
            claim(EvidenceGrade::PublicRecord, "tx-1", Check::unchecked()),
            claim(
                EvidenceGrade::SignedByAnotherParty,
                "rcpt-1",
                Check::unchecked(),
            ),
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
        PROVIDER,
        EvidenceGrade::PublicRecord,
        vec![
            claim(EvidenceGrade::Statement, "ref-1", Check::unchecked()),
            claim(EvidenceGrade::PublicRecord, "tx-1", public_record("tx-1")),
        ],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::PublicRecord);
    assert!(graded.is_proof());
}

// verifies: REQ-ORD-39, DEC-13
#[test]
fn a_check_for_another_movement_backs_no_piece() {
    // The public record of `tx-1` says nothing of the piece at `ref-1`, so the piece is the provider's word.
    let graded = grade_outcome(
        PROVIDER,
        EvidenceGrade::PublicRecord,
        vec![claim(
            EvidenceGrade::PublicRecord,
            "ref-1",
            public_record("tx-1"),
        )],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::Statement);
    assert!(!graded.is_proof());

    // Nor does a receipt for another reference back a grade 2 piece.
    let graded = grade_outcome(
        PROVIDER,
        EvidenceGrade::SignedByAnotherParty,
        vec![claim_by(
            EvidenceGrade::SignedByAnotherParty,
            "rcpt-1",
            NEXT,
            other_receipt("rcpt-2"),
        )],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::Statement);
}

// verifies: REQ-ORD-39, DEC-13
#[test]
fn claim_above_the_providers_declared_grade_is_refused() {
    let refused = grade_outcome(
        PROVIDER,
        EvidenceGrade::SignedByProvider,
        vec![claim(
            EvidenceGrade::PublicRecord,
            "tx-1",
            public_record("tx-1"),
        )],
    )
    .unwrap_err();
    assert_eq!(
        refused,
        Refusal::ClaimAboveDeclared {
            claimed: EvidenceGrade::PublicRecord,
            declared: EvidenceGrade::SignedByProvider,
            reference: "tx-1".into(),
        }
    );
}

// verifies: REQ-ORD-39
#[test]
fn outcome_with_no_evidence_is_refused() {
    assert_eq!(
        grade_outcome(PROVIDER, EvidenceGrade::Statement, vec![]),
        Err(Refusal::NoEvidence)
    );
}

// verifies: DEC-13
#[test]
fn receipt_from_the_same_publisher_is_not_another_partys_receipt() {
    // The provider's own receipt, and one from `acme-two`, behind the same publisher under another spelling.
    for maker in [PROVIDER, "acme-two"] {
        let same = verified(maker, "rcpt-1");
        assert_eq!(receipt_check(&same), Check::unchecked());
        let graded = grade_outcome(
            PROVIDER,
            EvidenceGrade::SignedByAnotherParty,
            vec![claim_by(
                EvidenceGrade::SignedByAnotherParty,
                "rcpt-1",
                maker,
                receipt_check(&same),
            )],
        )
        .unwrap();
        assert_eq!(graded.grade, EvidenceGrade::Statement);
    }
    assert_eq!(
        receipt_check(&verified(NEXT, "rcpt-1")),
        other_receipt("rcpt-1")
    );
}

// verifies: DEC-13
#[test]
fn a_receipt_for_a_provider_the_verifier_does_not_know_is_refused() {
    // The provider's publisher is the verifier's to say, not a separate input: an unknown provider gets nothing.
    let received = signed(NEXT, "rcpt-1");
    assert_eq!(
        verify_receipt(
            &Signer,
            "stranger",
            &Receipt {
                maker: NEXT,
                reference: "rcpt-1",
                received: &received,
            }
        ),
        Err(ReceiptRefused::UnknownProvider)
    );
}

/// A catalogue that changes under the checker: the first lookup finds the maker and the provider behind one
/// publisher, every later one behind two.
struct Changing {
    lookups: Cell<u32>,
}

impl ReceiptVerifier for Changing {
    fn verifies(&self, receipt: &Receipt<'_>) -> bool {
        Signer.verifies(receipt)
    }

    fn publishers(&self, _maker: &str, _provider: &str) -> Publishers {
        let first = self.lookups.get() == 0;
        self.lookups.set(self.lookups.get() + 1);
        let (maker, provider) = if first {
            ("acme", "acme")
        } else {
            ("other", "third")
        };
        Publishers {
            maker: PublisherId::new(maker),
            provider: PublisherId::new(provider),
        }
    }
}

// verifies: DEC-13
#[test]
fn both_publishers_come_from_one_lookup() {
    // Read from one snapshot, the maker and the provider share a publisher, so the receipt is no other party's.
    // Read from two, they would seem to differ and the receipt would earn grade 2 (`THR-124`).
    let changing = Changing {
        lookups: Cell::new(0),
    };
    let received = signed(NEXT, "rcpt-1");
    let receipt = verify_receipt(
        &changing,
        PROVIDER,
        &Receipt {
            maker: NEXT,
            reference: "rcpt-1",
            received: &received,
        },
    )
    .unwrap();
    assert_eq!(changing.lookups.get(), 1);
    assert_eq!(receipt.publisher(), receipt.provider_publisher());
    assert_eq!(receipt_check(&receipt), Check::unchecked());
}

// verifies: REQ-ORD-39, DEC-13
#[test]
fn a_receipt_check_backs_only_the_outcome_of_the_provider_it_was_judged_for() {
    // `next-co`'s receipt, judged for `third-co`'s outcome, is another party's there, but not in `acme-co`'s.
    let for_third = receipt_check(&verified_for("third-co", NEXT, "rcpt-1"));
    assert_eq!(for_third.receipt_provider(), Some("third-co"));
    let grade = |provider| {
        grade_outcome(
            provider,
            EvidenceGrade::SignedByAnotherParty,
            vec![claim_by(
                EvidenceGrade::SignedByAnotherParty,
                "rcpt-1",
                NEXT,
                for_third.clone(),
            )],
        )
        .unwrap()
        .grade
    };
    assert_eq!(grade("third-co"), EvidenceGrade::SignedByAnotherParty);
    assert_eq!(grade(PROVIDER), EvidenceGrade::Statement);
}

// verifies: DEC-13
#[test]
fn receipt_from_a_different_publisher_is_graded_as_two() {
    let graded = grade_outcome(
        PROVIDER,
        EvidenceGrade::SignedByAnotherParty,
        vec![claim_by(
            EvidenceGrade::SignedByAnotherParty,
            "rcpt-1",
            NEXT,
            other_receipt("rcpt-1"),
        )],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::SignedByAnotherParty);
    assert!(graded.is_proof());
}

// verifies: REQ-ORD-39, REQ-ORD-33, DEC-13
#[test]
fn a_receipt_backs_only_the_piece_its_maker_gave() {
    // `next-co`'s receipt for `rcpt-1` backs `next-co`'s piece, not one the provider says is its own.
    let graded = grade_outcome(
        PROVIDER,
        EvidenceGrade::SignedByAnotherParty,
        vec![claim_by(
            EvidenceGrade::SignedByAnotherParty,
            "rcpt-1",
            PROVIDER,
            other_receipt("rcpt-1"),
        )],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::Statement);
    assert!(!graded.is_proof());
}

// verifies: REQ-ORD-39, DEC-13
#[test]
fn one_receipt_backing_two_pieces_is_refused() {
    let refused = grade_outcome(
        PROVIDER,
        EvidenceGrade::SignedByAnotherParty,
        vec![
            claim_by(
                EvidenceGrade::SignedByAnotherParty,
                "rcpt-1",
                NEXT,
                other_receipt("rcpt-1"),
            ),
            claim_by(
                EvidenceGrade::SignedByAnotherParty,
                "rcpt-1",
                NEXT,
                other_receipt("rcpt-1"),
            ),
        ],
    )
    .unwrap_err();
    assert_eq!(refused, Refusal::ReceiptReused("rcpt-1".into()));
    // Two receipts, two pieces: each backs its own.
    let graded = grade_outcome(
        PROVIDER,
        EvidenceGrade::SignedByAnotherParty,
        vec![
            claim_by(
                EvidenceGrade::SignedByAnotherParty,
                "rcpt-1",
                NEXT,
                other_receipt("rcpt-1"),
            ),
            claim_by(
                EvidenceGrade::SignedByAnotherParty,
                "rcpt-2",
                NEXT,
                other_receipt("rcpt-2"),
            ),
        ],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::SignedByAnotherParty);
}

// verifies: REQ-ORD-39, DEC-13
#[test]
fn a_receipt_never_backs_a_public_record_claim() {
    // Another party's receipt, bound to the piece and its maker, earns grade 2 at most: a grade 1 claim on it is a
    // statement.
    let graded = grade_outcome(
        PROVIDER,
        EvidenceGrade::PublicRecord,
        vec![claim_by(
            EvidenceGrade::PublicRecord,
            "rcpt-1",
            NEXT,
            other_receipt("rcpt-1"),
        )],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::Statement);
    assert!(!graded.is_proof());
}

// verifies: DEC-13
#[test]
fn a_receipt_that_does_not_verify_is_refused() {
    let received = signed("next-co", "rcpt-1");
    let receipt = |maker, reference, received| Receipt {
        maker,
        reference,
        received,
    };
    let accepted =
        verify_receipt(&Signer, PROVIDER, &receipt("next-co", "rcpt-1", &received)).unwrap();
    assert_eq!(accepted.reference(), "rcpt-1");
    assert_eq!(accepted.publisher(), &publisher("other"));
    assert_eq!(accepted.maker(), "next-co");
    assert_eq!(accepted.provider(), PROVIDER);
    assert_eq!(accepted.provider_publisher(), &publisher("acme"));
    // Presented for another reference than the one signed.
    assert_eq!(
        verify_receipt(&Signer, PROVIDER, &receipt("next-co", "rcpt-2", &received)),
        Err(ReceiptRefused::DoesNotVerify)
    );
    // Presented as another maker's.
    assert_eq!(
        verify_receipt(&Signer, PROVIDER, &receipt("acme-co", "rcpt-1", &received)),
        Err(ReceiptRefused::DoesNotVerify)
    );
    // Signed, by a maker the catalogue doesn't know.
    let unknown = signed("stranger", "rcpt-1");
    assert_eq!(
        verify_receipt(&Signer, PROVIDER, &receipt("stranger", "rcpt-1", &unknown)),
        Err(ReceiptRefused::UnknownMaker)
    );
    // Acknowledging nothing, or only whitespace.
    for nothing in ["", " ", "\t\n"] {
        let empty = signed("next-co", nothing);
        assert_eq!(
            verify_receipt(
                &Signer,
                PROVIDER,
                &Receipt {
                    maker: "next-co",
                    reference: nothing,
                    received: &empty,
                }
            ),
            Err(ReceiptRefused::NoReference)
        );
    }
}

// verifies: REQ-ORD-39
#[test]
fn provider_signed_evidence_is_graded_three_and_is_not_proof() {
    let graded = grade_outcome(
        PROVIDER,
        EvidenceGrade::SignedByProvider,
        vec![claim(
            EvidenceGrade::SignedByProvider,
            "ref-1",
            Check::unchecked(),
        )],
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
        PROVIDER,
        EvidenceGrade::PublicRecord,
        vec![
            claim(EvidenceGrade::PublicRecord, "tx-1", public_record("tx-1")),
            claim(EvidenceGrade::PublicRecord, "tx-1", public_record("tx-1")),
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
        PROVIDER,
        EvidenceGrade::PublicRecord,
        vec![
            claim(EvidenceGrade::PublicRecord, "tx-1", public_record("tx-1")),
            claim(EvidenceGrade::PublicRecord, "tx-2", public_record("tx-2")),
        ],
    )
    .unwrap();
    assert_eq!(graded.grade, EvidenceGrade::PublicRecord);
    assert!(
        graded
            .pieces
            .iter()
            .all(|p| p.effective == EvidenceGrade::PublicRecord)
    );
}
