//! The per-network interface for grade 1 and 2 claims: finality, reorg rule, movement-to-segment matching
//! (ADR-0007, `DEC-13`). A stub network stands in; the simulated network is `T-015-02`.

use rutent_evidence::{
    Check, Expected, Finality, Movement, MovementId, NetworkRules, PublicNetwork, ReorgRule,
    Status, Unmatched, public_record_check, verify_movement,
};

struct Stub(Status);

impl PublicNetwork for Stub {
    fn network(&self) -> &str {
        "chain-a"
    }
    fn rules(&self) -> NetworkRules {
        NetworkRules {
            finality: Finality::Confirmations(12),
            reorg: ReorgRule::DepthBound(6),
        }
    }
    fn status(&self, _: &MovementId) -> Status {
        self.0.clone()
    }
}

fn movement() -> Movement {
    Movement {
        destination: "addr-1".into(),
        asset: "USDC".into(),
        amount: "1000".into(),
    }
}

fn expected() -> Expected {
    Expected {
        network: "chain-a".into(),
        destination: "addr-1".into(),
        asset: "USDC".into(),
        amount: "1000".into(),
    }
}

fn id() -> MovementId {
    MovementId::new("tx-1").unwrap()
}

// verifies: DEC-13
#[test]
fn final_movement_matching_the_segment_is_a_final_public_record() {
    let net = Stub(Status::Final(movement()));
    assert_eq!(verify_movement(&net, &id(), &expected()), Ok(()));
    let check = public_record_check(&net, &id(), &expected());
    assert_eq!(check.movement(), Some(&id()));
}

// verifies: DEC-13
#[test]
fn movement_not_yet_final_evidences_nothing() {
    let net = Stub(Status::Pending(movement()));
    assert_eq!(
        verify_movement(&net, &id(), &expected()),
        Err(Unmatched::NotFinal)
    );
    assert_eq!(
        public_record_check(&net, &id(), &expected()),
        Check::unchecked()
    );
}

// verifies: DEC-13
#[test]
fn reorged_or_unknown_movement_evidences_nothing() {
    assert_eq!(
        verify_movement(&Stub(Status::ReorgedOut), &id(), &expected()),
        Err(Unmatched::ReorgedOut)
    );
    assert_eq!(
        verify_movement(&Stub(Status::Unknown), &id(), &expected()),
        Err(Unmatched::UnknownMovement)
    );
}

// verifies: DEC-13
#[test]
fn movement_that_differs_from_the_segment_in_any_field_is_not_matched() {
    let net = Stub(Status::Final(Movement {
        destination: "addr-2".into(),
        ..movement()
    }));
    assert_eq!(
        verify_movement(&net, &id(), &expected()),
        Err(Unmatched::WrongDestination)
    );
    let net = Stub(Status::Final(Movement {
        asset: "DAI".into(),
        ..movement()
    }));
    assert_eq!(
        verify_movement(&net, &id(), &expected()),
        Err(Unmatched::WrongAsset)
    );
    let net = Stub(Status::Final(Movement {
        amount: "999".into(),
        ..movement()
    }));
    assert_eq!(
        verify_movement(&net, &id(), &expected()),
        Err(Unmatched::WrongAmount)
    );
    let other_network = Expected {
        network: "chain-b".into(),
        ..expected()
    };
    assert_eq!(
        verify_movement(&Stub(Status::Final(movement())), &id(), &other_network),
        Err(Unmatched::WrongNetwork)
    );
}
