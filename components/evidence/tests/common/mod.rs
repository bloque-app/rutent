//! Checks built the only way a caller can: through the checker's own functions.
#![allow(dead_code)]

use rutent_evidence::{
    Check, Expected, Finality, Movement, MovementId, NetworkRules, PublicNetwork, PublisherId,
    Receipt, ReceiptVerifier, ReorgRule, Status, VerifiedReceipt, public_record_check,
    receipt_check, verify_receipt,
};

struct Chain;

impl PublicNetwork for Chain {
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
        Status::Final(Movement {
            destination: "addr-1".into(),
            asset: "USDC".into(),
            amount: "1000".into(),
        })
    }
}

/// A public-record check for the movement `id`, earned by `verify_movement`.
pub fn public_record(id: &str) -> Check {
    let expected = Expected {
        network: "chain-a".into(),
        destination: "addr-1".into(),
        asset: "USDC".into(),
        amount: "1000".into(),
    };
    public_record_check(&Chain, &MovementId::new(id).unwrap(), &expected)
}

pub fn publisher(name: &str) -> PublisherId {
    PublisherId::new(name).unwrap()
}

/// A stand-in verifier. A receipt verifies when its bytes are its maker's signature over its reference, spelled
/// `signed <maker> <reference>`. The catalogue it reads puts `acme-co` and `acme-two` behind the publisher `acme`
/// (under two spellings), and `next-co` behind `other`.
pub struct Signer;

impl ReceiptVerifier for Signer {
    fn verifies(&self, receipt: &Receipt<'_>) -> bool {
        receipt.received == signed(receipt.maker, receipt.reference).as_slice()
    }

    fn publisher_of(&self, party: &str) -> Option<PublisherId> {
        match party {
            "acme-co" => PublisherId::new("acme"),
            "acme-two" => PublisherId::new(" ACME "),
            "next-co" => PublisherId::new("other"),
            _ => None,
        }
    }
}

/// The bytes [`Signer`] accepts as `maker`'s receipt for `reference`.
pub fn signed(maker: &str, reference: &str) -> Vec<u8> {
    format!("signed {maker} {reference}").into_bytes()
}

/// `maker`'s receipt for `reference`, verified by [`Signer`].
pub fn verified(maker: &str, reference: &str) -> VerifiedReceipt {
    let received = signed(maker, reference);
    verify_receipt(
        &Signer,
        &Receipt {
            maker,
            reference,
            received: &received,
        },
    )
    .unwrap()
}

/// The provider whose outcome is graded. Its publisher is `acme`.
pub const PROVIDER: &str = "acme-co";

/// The next provider, which gives receipts. Its publisher is `other`.
pub const NEXT: &str = "next-co";

/// A receipt check for `reference` from `next-co`, whose publisher is not the provider's (`acme`). It backs a piece
/// whose `by` is `next-co`.
pub fn other_receipt(reference: &str) -> Check {
    receipt_check(&Signer, PROVIDER, &verified(NEXT, reference))
}
