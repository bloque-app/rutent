//! Property tests for the simulated network's finality and reorg invariants (`DEC-18`).

use proptest::prelude::*;
use rutent_evidence::{
    Expected, Finality, Movement, MovementId, NetworkRules, PublicNetwork, ReorgRule,
    SimulatedNetwork, Status, public_record_check,
};

#[derive(Debug, Clone)]
enum Op {
    Record(u8),
    Advance(u64),
    Reorg(u64),
    Checkpoint,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..4).prop_map(Op::Record),
        (0u64..4).prop_map(Op::Advance),
        (0u64..6).prop_map(Op::Reorg),
        Just(Op::Checkpoint),
    ]
}

fn rules() -> impl Strategy<Value = NetworkRules> {
    let finality = prop_oneof![
        Just(Finality::Instant),
        (0u32..6).prop_map(Finality::Confirmations),
        Just(Finality::Checkpoint),
    ];
    let reorg = prop_oneof![
        Just(ReorgRule::Never),
        (0u32..6).prop_map(ReorgRule::DepthBound),
        Just(ReorgRule::UntilFinal),
    ];
    (finality, reorg).prop_map(|(finality, reorg)| NetworkRules { finality, reorg })
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

fn mid(i: u8) -> MovementId {
    MovementId::new(format!("tx-{i}")).unwrap()
}

fn apply(n: &mut SimulatedNetwork, op: &Op) {
    match op {
        Op::Record(i) => {
            let _ = n.record(mid(*i), movement());
        }
        Op::Advance(b) => n.advance(*b),
        Op::Reorg(d) => {
            n.reorg(*d);
        }
        Op::Checkpoint => n.checkpoint(),
    }
}

fn statuses(n: &SimulatedNetwork) -> Vec<Status> {
    (0..4).map(|i| n.status(&mid(i))).collect()
}

proptest! {
    // verifies: REQ-ORD-39, DEC-13, DEC-18
    #[test]
    fn a_final_movement_never_stops_being_final(rules in rules(), ops in prop::collection::vec(op(), 0..30)) {
        let mut n = SimulatedNetwork::new("chain-a", rules);
        let mut was_final: Vec<Option<Status>> = vec![None; 4];
        for op in &ops {
            apply(&mut n, op);
            for (i, status) in statuses(&n).into_iter().enumerate() {
                if let Some(before) = &was_final[i] {
                    prop_assert_eq!(&status, before);
                }
                if matches!(status, Status::Final(_)) {
                    was_final[i] = Some(status);
                }
            }
        }
    }

    // verifies: DEC-13, DEC-18
    #[test]
    fn a_network_that_never_reorgs_never_reports_a_reorg(finality in prop_oneof![
        Just(Finality::Instant), (0u32..6).prop_map(Finality::Confirmations), Just(Finality::Checkpoint)
    ], ops in prop::collection::vec(op(), 0..30)) {
        let mut n = SimulatedNetwork::new("chain-a", NetworkRules { finality, reorg: ReorgRule::Never });
        for op in &ops {
            apply(&mut n, op);
            prop_assert!(!statuses(&n).contains(&Status::ReorgedOut));
        }
    }

    // verifies: DEC-13, DEC-18
    #[test]
    fn a_movement_deeper_than_the_reorg_bound_is_never_undone(bound in 0u32..6, extra in 0u64..4, depth in 0u64..20) {
        let mut n = SimulatedNetwork::new("chain-a", NetworkRules {
            finality: Finality::Checkpoint,
            reorg: ReorgRule::DepthBound(bound),
        });
        n.record(mid(0), movement()).unwrap();
        n.advance(u64::from(bound) + extra);
        n.reorg(depth);
        prop_assert!(matches!(n.status(&mid(0)), Status::Pending(_)));
    }

    // verifies: REQ-ORD-39, DEC-13, DEC-18
    #[test]
    fn a_movement_earns_a_public_record_only_while_final(rules in rules(), ops in prop::collection::vec(op(), 0..30)) {
        let mut n = SimulatedNetwork::new("chain-a", rules);
        for op in &ops {
            apply(&mut n, op);
            for i in 0..4 {
                let earned = public_record_check(&n, &mid(i), &expected()).movement().is_some();
                prop_assert_eq!(earned, matches!(n.status(&mid(i)), Status::Final(_)));
            }
        }
    }

    // verifies: DEC-13, DEC-18
    #[test]
    fn a_movement_is_unknown_only_until_it_is_recorded(rules in rules(), ops in prop::collection::vec(op(), 0..30)) {
        let mut n = SimulatedNetwork::new("chain-a", rules);
        let mut seen = [false; 4];
        for op in &ops {
            apply(&mut n, op);
            if let Op::Record(i) = op { seen[usize::from(*i)] = true; }
            for (i, status) in statuses(&n).into_iter().enumerate() {
                prop_assert_eq!(status == Status::Unknown, !seen[i]);
            }
        }
    }

    // verifies: DEC-13, DEC-18
    #[test]
    fn nothing_recorded_after_the_last_checkpoint_is_final_under_checkpoint_finality(
        reorg in prop_oneof![Just(ReorgRule::Never), (0u32..6).prop_map(ReorgRule::DepthBound), Just(ReorgRule::UntilFinal)],
        ops in prop::collection::vec(op(), 0..30),
    ) {
        let mut n = SimulatedNetwork::new("chain-a", NetworkRules { finality: Finality::Checkpoint, reorg });
        let mut after_checkpoint = [false; 4];
        for op in &ops {
            match op {
                Op::Checkpoint => after_checkpoint = [false; 4],
                Op::Record(i) if !matches!(n.status(&mid(*i)), Status::Pending(_) | Status::Final(_)) => {
                    after_checkpoint[usize::from(*i)] = true;
                }
                _ => {}
            }
            apply(&mut n, op);
            for (i, status) in statuses(&n).into_iter().enumerate() {
                if after_checkpoint[i] {
                    prop_assert!(!matches!(status, Status::Final(_)));
                }
            }
        }
    }
}
