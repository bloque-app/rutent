//! Grading an outcome's evidence (ADR-0007, `REQ-ORD-39`, `DEC-13`).

use crate::network::MovementId;
use rutent_contracts::provider_role::{EvidenceGrade, EvidencePiece};

/// What the checker could establish about one piece of evidence, apart from the provider's own statement.
///
/// A caller cannot build a backing check by hand. Only two paths produce one:
///
/// - [`public_record_check`](crate::public_record_check), when [`verify_movement`](crate::verify_movement) finds a
///   final public record of the segment on the network the caller passes;
/// - [`receipt_check`], on a [`VerifiedReceipt`], which only [`verify_receipt`] builds, over the
///   [`ReceiptVerifier`] the caller passes.
///
/// Each check is bound to what it checked: a movement, or the reference a receipt acknowledges. It backs only the
/// piece whose reference that is.
///
/// The checker owns the rules, not the trust anchors. A check is only as good as the network reader, the receipt
/// verifier and the publisher identities its caller supplies: this crate imports nothing of ours but the contracts
/// (`DEC-2`), so it holds no catalogue, no keys and no signature scheme. Core supplies them from the catalogue it
/// accepted and the provider library's strict verifier.
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
    Receipt(String),
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

    /// What the check is bound to: the movement of a public-record check, or the reference a receipt acknowledges.
    /// `None` for no check.
    pub fn bound_to(&self) -> Option<&str> {
        match &self.0 {
            Kind::Unchecked => None,
            Kind::PublicRecord(id) => Some(id.as_str()),
            Kind::Receipt(reference) => Some(reference),
        }
    }

    fn is_public_record(&self) -> bool {
        matches!(self.0, Kind::PublicRecord(_))
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

/// A receipt as presented for checking: the party that made it, the reference it acknowledges, and the statement
/// as received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Receipt<'a> {
    /// The party that made the receipt, such as the next provider on the route.
    pub maker: &'a str,
    /// The reference it acknowledges: the piece of evidence it is meant to back.
    pub reference: &'a str,
    /// The receipt's statement, as received.
    pub received: &'a [u8],
}

/// How a receipt's signature and its maker's publisher are established. This crate holds neither a signature scheme
/// nor a catalogue (`DEC-2`), so its caller supplies them: Core implements this over the catalogue it accepted and
/// the provider library's strict verifier. A receipt is only as good as the verifier it is checked with.
pub trait ReceiptVerifier {
    /// Whether `receipt.received` is a statement signed by `receipt.maker`, under a key the catalogue accepted for
    /// it, that acknowledges `receipt.reference`.
    fn verifies(&self, receipt: &Receipt<'_>) -> bool;

    /// The publisher behind a maker, as the catalogue records it (`REQ-CAT-20`), or `None` for a maker it doesn't
    /// know.
    fn publisher_of(&self, maker: &str) -> Option<PublisherId>;
}

/// A receipt [`verify_receipt`] accepted: its maker's publisher and the reference it acknowledges. Nothing else
/// builds one.
///
/// ```compile_fail
/// let forged = rutent_evidence::VerifiedReceipt {
///     publisher: rutent_evidence::PublisherId::new("other").unwrap(),
///     reference: "ref-1".into(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedReceipt {
    publisher: PublisherId,
    reference: String,
}

impl VerifiedReceipt {
    /// The publisher behind the receipt's maker.
    pub fn publisher(&self) -> &PublisherId {
        &self.publisher
    }

    /// The reference the receipt acknowledges.
    pub fn reference(&self) -> &str {
        &self.reference
    }
}

/// Why a receipt is not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptRefused {
    /// It acknowledges no reference, so it can back no piece.
    NoReference,
    /// The verifier doesn't find it signed by its maker, acknowledging its reference.
    DoesNotVerify,
    /// The verifier knows no publisher behind its maker.
    UnknownMaker,
}

/// Checks a receipt with `verifier`, and returns it verified: signed by its maker, acknowledging its reference, with
/// the publisher behind the maker. The only way to build a [`VerifiedReceipt`].
pub fn verify_receipt(
    verifier: &dyn ReceiptVerifier,
    receipt: &Receipt<'_>,
) -> Result<VerifiedReceipt, ReceiptRefused> {
    if receipt.reference.is_empty() {
        return Err(ReceiptRefused::NoReference);
    }
    if !verifier.verifies(receipt) {
        return Err(ReceiptRefused::DoesNotVerify);
    }
    let publisher = verifier
        .publisher_of(receipt.maker)
        .ok_or(ReceiptRefused::UnknownMaker)?;
    Ok(VerifiedReceipt {
        publisher,
        reference: receipt.reference.to_owned(),
    })
}

/// One piece of evidence with the check made on it. The check backs the piece only when it is bound to the piece's
/// reference ([`Check::bound_to`]); a check for another movement or reference backs nothing.
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

/// The check a verified receipt earns: another party's, bound to the reference the receipt acknowledges, only when
/// the publisher behind its maker differs from the provider's. One publisher running both sides of a hop gives no
/// grade 2 (`THR-124`, ADR-0007). `provider_publisher` is the provider's publisher as the catalogue records it.
pub fn receipt_check(provider_publisher: &PublisherId, receipt: &VerifiedReceipt) -> Check {
    if *provider_publisher == receipt.publisher {
        Check::unchecked()
    } else {
        Check(Kind::Receipt(receipt.reference.clone()))
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
    let bound = claim.check.bound_to() == Some(claim.piece.reference.as_str());
    let backed = match claim.piece.grade {
        EvidenceGrade::PublicRecord => bound && claim.check.is_public_record(),
        // Any check bound to the piece backs grade 2: a public record, or another party's receipt.
        EvidenceGrade::SignedByAnotherParty => bound,
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
/// is graded as a statement, never as proof. A check backs a piece only when it is bound to the piece's reference,
/// and grade 1 only when it is a public record: a receipt backs grade 2 at most. The outcome's grade is its best
/// piece's.
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
