//! Checks built the only way a caller can: through the checker's own functions.
#![allow(dead_code)]

use rutent_evidence::{
    Check, Expected, Finality, Movement, MovementId, NetworkRules, PublicNetwork, PublisherId,
    ReorgRule, Status, public_record_check, receipt_check,
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

/// A receipt check from a publisher other than the provider's.
pub fn other_receipt() -> Check {
    receipt_check(&publisher("acme"), &publisher("other"))
}
