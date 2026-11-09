//! The simulated public network: finality, reorgs and matching, as the reference for real networks (`T-015-02`).

use rutent_evidence::{
    Expected, Finality, Movement, MovementId, NetworkRules, PublicNetwork, RecordRefused,
    ReorgRule, SimulatedNetwork, Status, Unmatched, public_record_check, verify_movement,
};

fn net(finality: Finality, reorg: ReorgRule) -> SimulatedNetwork {
    SimulatedNetwork::new("chain-a", NetworkRules { finality, reorg })
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

// verifies: REQ-ORD-39, DEC-13
#[test]
fn movement_reorged_out_before_finality_evidences_no_segment() {
    let mut n = net(Finality::Confirmations(3), ReorgRule::UntilFinal);
    n.record(id(), movement()).unwrap();
    n.advance(1);
    assert_eq!(n.reorg(5), vec![id()]);
    assert_eq!(n.status(&id()), Status::ReorgedOut);
    assert_eq!(
        verify_movement(&n, &id(), &expected()),
        Err(Unmatched::ReorgedOut)
    );
    assert_eq!(public_record_check(&n, &id(), &expected()).movement(), None);
}

// verifies: DEC-13
#[test]
fn movement_to_another_destination_matches_no_segment() {
    let mut n = net(Finality::Instant, ReorgRule::Never);
    n.record(
        id(),
        Movement {
            destination: "addr-2".into(),
            ..movement()
        },
    )
    .unwrap();
    assert_eq!(
        verify_movement(&n, &id(), &expected()),
        Err(Unmatched::WrongDestination)
    );
}

// verifies: DEC-13
#[test]
fn movement_matching_the_segment_is_a_final_record_once_confirmed() {
    let mut n = net(Finality::Confirmations(3), ReorgRule::DepthBound(2));
    n.record(id(), movement()).unwrap();
    assert_eq!(
        verify_movement(&n, &id(), &expected()),
        Err(Unmatched::NotFinal)
    );
    n.advance(2);
    assert_eq!(verify_movement(&n, &id(), &expected()), Ok(()));
}

// verifies: DEC-13
#[test]
fn checkpoint_finality_waits_for_a_checkpoint_that_covers_the_movement() {
    let mut n = net(Finality::Checkpoint, ReorgRule::UntilFinal);
    n.record(id(), movement()).unwrap();
    n.advance(100);
    assert!(matches!(n.status(&id()), Status::Pending(_)));
    n.checkpoint();
    assert!(matches!(n.status(&id()), Status::Final(_)));
    let later = MovementId::new("tx-2").unwrap();
    n.advance(1);
    n.record(later.clone(), movement()).unwrap();
    assert!(matches!(n.status(&later), Status::Pending(_)));
}

// verifies: DEC-13
#[test]
fn final_movement_survives_a_reorg() {
    let mut n = net(Finality::Confirmations(2), ReorgRule::UntilFinal);
    n.record(id(), movement()).unwrap();
    n.advance(1);
    assert!(n.reorg(10).is_empty());
    assert!(matches!(n.status(&id()), Status::Final(_)));
}

// verifies: DEC-13
#[test]
fn a_network_that_never_reorgs_keeps_every_pending_movement() {
    let mut n = net(Finality::Confirmations(5), ReorgRule::Never);
    n.record(id(), movement()).unwrap();
    assert!(n.reorg(10).is_empty());
    assert!(matches!(n.status(&id()), Status::Pending(_)));
}

// verifies: DEC-13
#[test]
fn depth_bound_reorg_leaves_movements_deeper_than_the_bound() {
    let mut n = net(Finality::Confirmations(10), ReorgRule::DepthBound(2));
    n.record(id(), movement()).unwrap();
    n.advance(2);
    assert!(n.reorg(10).is_empty());
    assert!(matches!(n.status(&id()), Status::Pending(_)));
}

// verifies: DEC-13
#[test]
fn reorg_reaches_only_as_deep_as_asked() {
    let mut n = net(Finality::Confirmations(10), ReorgRule::UntilFinal);
    n.record(id(), movement()).unwrap();
    n.advance(3);
    assert!(n.reorg(2).is_empty());
    assert_eq!(n.reorg(4), vec![id()]);
}

// verifies: DEC-13
#[test]
fn unknown_movement_is_unknown_and_matches_nothing() {
    let n = net(Finality::Instant, ReorgRule::Never);
    assert_eq!(n.status(&id()), Status::Unknown);
    assert_eq!(
        verify_movement(&n, &id(), &expected()),
        Err(Unmatched::UnknownMovement)
    );
}

// verifies: DEC-13
#[test]
fn movement_on_another_network_matches_no_segment() {
    let mut n = net(Finality::Instant, ReorgRule::Never);
    n.record(id(), movement()).unwrap();
    let elsewhere = Expected {
        network: "chain-b".into(),
        ..expected()
    };
    assert_eq!(
        verify_movement(&n, &id(), &elsewhere),
        Err(Unmatched::WrongNetwork)
    );
}

// verifies: DEC-13
#[test]
fn a_recorded_movement_id_cannot_be_recorded_twice_until_reorged_out() {
    let mut n = net(Finality::Confirmations(5), ReorgRule::UntilFinal);
    n.record(id(), movement()).unwrap();
    assert!(n.record(id(), movement()).is_err());
    n.reorg(1);
    n.record(id(), movement()).unwrap();
    assert!(matches!(n.status(&id()), Status::Pending(_)));
}

// verifies: DEC-13
#[test]
fn movement_recorded_after_a_checkpoint_is_not_covered_by_it() {
    let mut n = net(Finality::Checkpoint, ReorgRule::UntilFinal);
    n.checkpoint();
    n.record(id(), movement()).unwrap();
    assert!(matches!(n.status(&id()), Status::Pending(_)));
    n.checkpoint();
    assert!(matches!(n.status(&id()), Status::Final(_)));
}

// verifies: DEC-13
#[test]
fn confirmations_finality_turns_final_on_exactly_the_nth_confirmation() {
    for n_conf in 2u32..6 {
        let mut n = net(Finality::Confirmations(n_conf), ReorgRule::Never);
        n.record(id(), movement()).unwrap();
        n.advance(u64::from(n_conf) - 2);
        assert!(matches!(n.status(&id()), Status::Pending(_)));
        n.advance(1);
        assert!(matches!(n.status(&id()), Status::Final(_)));
    }
}

// verifies: DEC-13
#[test]
fn zero_confirmations_behaves_as_one() {
    let mut n = net(Finality::Confirmations(0), ReorgRule::Never);
    n.record(id(), movement()).unwrap();
    assert!(matches!(n.status(&id()), Status::Final(_)));
}

// verifies: DEC-13
#[test]
fn a_reorged_out_id_cannot_come_back_with_different_content() {
    let mut n = net(Finality::Confirmations(5), ReorgRule::UntilFinal);
    n.record(id(), movement()).unwrap();
    n.reorg(1);
    let other = Movement {
        amount: "1".into(),
        ..movement()
    };
    assert_eq!(
        n.record(id(), other),
        Err(RecordRefused::ContentChanged(id()))
    );
    assert_eq!(n.status(&id()), Status::ReorgedOut);
}
