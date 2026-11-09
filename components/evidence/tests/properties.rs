//! Property tests for the invariants of the grading and matching rules (`DEC-18`).

mod common;

use common::{other_receipt, public_record};
use proptest::prelude::*;
use rutent_contracts::provider_role::{EvidenceGrade, EvidencePiece};
use rutent_evidence::{
    Check, Claim, Expected, Finality, GradeExt, Movement, MovementId, NetworkRules, PublicNetwork,
    Refusal, ReorgRule, Status, grade_outcome, public_record_check,
};

fn grade() -> impl Strategy<Value = EvidenceGrade> {
    (1u8..=4).prop_map(|n| EvidenceGrade::try_from(n).unwrap())
}

fn check() -> impl Strategy<Value = Check> {
    prop_oneof![
        Just(Check::unchecked()),
        "[a-c]".prop_map(|id| public_record(&id)),
        Just(other_receipt()),
    ]
}

fn claims() -> impl Strategy<Value = Vec<Claim>> {
    prop::collection::vec(
        (grade(), check(), "[a-z]{1,4}").prop_map(|(grade, check, reference)| Claim {
            piece: EvidencePiece {
                grade,
                reference,
                by: "acme".into(),
            },
            check,
        }),
        0..6,
    )
}

proptest! {
    // verifies: REQ-ORD-39, DEC-13
    #[test]
    fn graded_outcome_is_never_stronger_than_the_declared_grade(declared in grade(), claims in claims()) {
        if let Ok(graded) = grade_outcome(declared, claims) {
            prop_assert!(!graded.grade.is_stronger_than(declared));
        }
    }
}

proptest! {
    // verifies: REQ-ORD-39, DEC-13
    #[test]
    fn any_claim_stronger_than_declared_is_refused(declared in grade(), claims in claims()) {
        let over = claims.iter().any(|c| c.piece.grade.is_stronger_than(declared));
        let result = grade_outcome(declared, claims);
        if over {
            let refused = matches!(result, Err(Refusal::ClaimAboveDeclared { .. }) | Err(Refusal::NoEvidence));
            prop_assert!(refused);
        }
    }
}

proptest! {
    // verifies: REQ-ORD-39
    #[test]
    fn nothing_is_graded_as_proof_without_a_check_that_backs_it(declared in grade(), claims in claims()) {
        let unchecked: Vec<Claim> = claims
            .into_iter()
            .map(|c| Claim { check: Check::unchecked(), ..c })
            .collect();
        if let Ok(graded) = grade_outcome(declared, unchecked) {
            prop_assert!(!graded.is_proof());
        }
    }
}

proptest! {
    // verifies: REQ-ORD-39
    #[test]
    fn grading_does_not_depend_on_the_order_of_the_pieces(declared in grade(), claims in claims()) {
        let mut reversed = claims.clone();
        reversed.reverse();
        let forward = grade_outcome(declared, claims).map(|g| g.grade);
        let backward = grade_outcome(declared, reversed).map(|g| g.grade);
        prop_assert_eq!(forward.is_ok(), backward.is_ok());
        if let (Ok(a), Ok(b)) = (forward, backward) {
            prop_assert_eq!(a, b);
        }
    }
}

struct Fixed(Status);

impl PublicNetwork for Fixed {
    fn network(&self) -> &str {
        "chain-a"
    }
    fn rules(&self) -> NetworkRules {
        NetworkRules {
            finality: Finality::Instant,
            reorg: ReorgRule::Never,
        }
    }
    fn status(&self, _: &MovementId) -> Status {
        self.0.clone()
    }
}

proptest! {
    // verifies: DEC-13
    #[test]
    fn only_a_final_movement_equal_to_the_segment_is_a_public_record(
        dest in "[a-c]", asset in "[a-c]", amount in "[1-3]", kind in 0u8..4,
    ) {
        let movement = Movement { destination: dest.clone(), asset: asset.clone(), amount: amount.clone() };
        let status = match kind {
            0 => Status::Final(movement.clone()),
            1 => Status::Pending(movement.clone()),
            2 => Status::ReorgedOut,
            _ => Status::Unknown,
        };
        let expected = Expected {
            network: "chain-a".into(),
            destination: "a".into(),
            asset: "a".into(),
            amount: "1".into(),
        };
        let backed = public_record_check(&Fixed(status), &MovementId::new("t").unwrap(), &expected)
            .movement().is_some();
        let should = kind == 0 && dest == "a" && asset == "a" && amount == "1";
        prop_assert_eq!(backed, should);
    }
}
