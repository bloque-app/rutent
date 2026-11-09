//! Grading an outcome's evidence (ADR-0007, `REQ-ORD-39`, `DEC-13`).

use crate::network::MovementId;
use rutent_contracts::provider_role::{EvidenceGrade, EvidencePiece};

/// What the checker could establish about one piece of evidence, apart from the provider's own statement.
///
/// A caller cannot build a backing check: only [`verify_movement`](crate::verify_movement) (through
/// [`public_record_check`](crate::public_record_check)) and [`receipt_check`] produce one, so a proof can't be
/// forged.
///
/// ```compile_fail
/// let forged = rutent_evidence::Check::public_record(rutent_evidence::MovementId::new("tx").unwrap());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check(Kind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Unchecked,
    PublicRecord(MovementId),
    Receipt,
}

impl Check {
    /// Nothing beyond the provider's own statement.
    pub fn unchecked() -> Self {
        Check(Kind::Unchecked)
    }

    /// A final public record of this movement was found and matches the segment.
    pub(crate) fn public_record(id: MovementId) -> Self {
        Check(Kind::PublicRecord(id))
    }

    /// The movement a public-record check rests on, if it is one.
    pub fn movement(&self) -> Option<&MovementId> {
        match &self.0 {
            Kind::PublicRecord(id) => Some(id),
            _ => None,
        }
    }

    fn is_public_record(&self) -> bool {
        matches!(self.0, Kind::PublicRecord(_))
    }

    fn is_receipt(&self) -> bool {
        matches!(self.0, Kind::Receipt)
    }
}

/// A publisher's identifier, normalised (trimmed, lower-cased) so two spellings of one publisher compare equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PublisherId(String);

impl PublisherId {
    /// The normalised identifier, or `None` when nothing is left of it.
    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim().to_lowercase();
        (!text.is_empty()).then_some(Self(text))
    }
}

/// One piece of evidence with the check made on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// What the provider reported, with the grade it claims.
    pub piece: EvidencePiece,
    /// What the checker established.
    pub check: Check,
}

/// A piece with the grade the checker stands behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradedPiece {
    /// The piece as reported.
    pub piece: EvidencePiece,
    /// The grade it earns: its claimed grade where backed, otherwise a statement.
    pub effective: EvidenceGrade,
}

/// An outcome's evidence, graded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graded {
    /// The outcome's grade: its best piece's effective grade.
    pub grade: EvidenceGrade,
    /// Each piece, with the grade it earned.
    pub pieces: Vec<GradedPiece>,
}

impl Graded {
    /// Whether the outcome rests on proof someone else can check (grade 1 or 2), rather than on the provider's word.
    pub fn is_proof(&self) -> bool {
        self.grade.is_proof()
    }
}

/// Why an outcome's evidence is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The outcome carries no evidence, and the contract requires at least one piece.
    NoEvidence,
    /// A piece claims a grade stronger than the provider declared (`DEC-13`).
    ClaimAboveDeclared {
        /// The grade claimed.
        claimed: EvidenceGrade,
        /// The grade the provider declared.
        declared: EvidenceGrade,
        /// The piece's reference.
        reference: String,
    },
    /// One movement backs more than one piece: it can evidence only one.
    MovementReused(MovementId),
}

/// What the scale says about a grade.
pub trait GradeExt: Copy {
    /// Grades 1 and 2: checkable by someone other than the provider.
    fn is_proof(self) -> bool;
    /// Whether this grade is stronger (a lower number) than `other`.
    fn is_stronger_than(self, other: Self) -> bool;
}

impl GradeExt for EvidenceGrade {
    fn is_proof(self) -> bool {
        self <= EvidenceGrade::SignedByAnotherParty
    }

    fn is_stronger_than(self, other: Self) -> bool {
        self < other
    }
}

/// The check a receipt earns: another party's only when its publisher differs from the provider's. One publisher
/// running both sides of a hop gives no grade 2 (`THR-124`, ADR-0007).
pub fn receipt_check(provider_publisher: &PublisherId, receipt_publisher: &PublisherId) -> Check {
    if provider_publisher == receipt_publisher {
        Check::unchecked()
    } else {
        Check(Kind::Receipt)
    }
}

/// Refuses a movement named more than once: one movement evidences one segment or piece, never two.
pub fn distinct_movements<'a>(
    ids: impl IntoIterator<Item = &'a MovementId>,
) -> Result<(), Refusal> {
    let mut seen = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            return Err(Refusal::MovementReused(id.clone()));
        }
        seen.push(id);
    }
    Ok(())
}

fn effective(claim: &Claim) -> EvidenceGrade {
    let backed = match claim.piece.grade {
        EvidenceGrade::PublicRecord => claim.check.is_public_record(),
        EvidenceGrade::SignedByAnotherParty => {
            claim.check.is_public_record() || claim.check.is_receipt()
        }
        EvidenceGrade::SignedByProvider | EvidenceGrade::Statement => true,
    };
    if backed {
        claim.piece.grade
    } else {
        EvidenceGrade::Statement
    }
}

/// Grades an outcome's evidence against the grade its provider declared (`REQ-ORD-39`, `DEC-13`).
///
/// A piece claiming a grade stronger than declared refuses the outcome. A claim of grade 1 or 2 that no check backs
/// is graded as a statement, never as proof. The outcome's grade is its best piece's.
pub fn grade_outcome(declared: EvidenceGrade, claims: Vec<Claim>) -> Result<Graded, Refusal> {
    if claims.is_empty() {
        return Err(Refusal::NoEvidence);
    }
    if let Some(over) = claims
        .iter()
        .find(|c| c.piece.grade.is_stronger_than(declared))
    {
        return Err(Refusal::ClaimAboveDeclared {
            claimed: over.piece.grade,
            declared,
            reference: over.piece.reference.clone(),
        });
    }
    distinct_movements(claims.iter().filter_map(|c| c.check.movement()))?;
    let pieces: Vec<GradedPiece> = claims
        .into_iter()
        .map(|claim| GradedPiece {
            effective: effective(&claim),
            piece: claim.piece,
        })
        .collect();
    let grade = pieces
        .iter()
        .map(|p| p.effective)
        .min()
        .unwrap_or(EvidenceGrade::Statement);
    Ok(Graded { grade, pieces })
}
