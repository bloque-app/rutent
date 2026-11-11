//! Property tests for replay (`DEC-18`).

use proptest::prelude::*;
use rutent_evidence::replay::{
    NodeRecord, PublicExit, RecordedSegment, Replay, RouteEnd, Stated, replay_node, replay_route,
};
use rutent_evidence::{
    Finality, Movement, MovementId, NetworkRules, PublicNetwork, ReorgRule, Status,
};

struct Fake(Status);

impl PublicNetwork for Fake {
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

fn exit_of(asset: &str, amount: &str) -> RouteEnd {
    RouteEnd::Public(PublicExit {
        movement: MovementId::new("tx").unwrap(),
        network: "chain-a".into(),
        destination: "addr-1".into(),
        stated: Stated {
            asset: asset.into(),
            amount: amount.into(),
        },
    })
}

/// Same-asset chain: each hop takes in what the last gave out, and the fee accounts for the difference.
fn chain() -> impl Strategy<Value = Vec<RecordedSegment>> {
    (1_000u64..1_000_000, prop::collection::vec(0u64..100, 1..6)).prop_map(|(start, fees)| {
        let mut amount = start;
        fees.into_iter()
            .map(|fee| {
                let out = amount - fee;
                let s = RecordedSegment {
                    asset_in: "USDC".into(),
                    amount_in: amount.to_string(),
                    asset_out: "USDC".into(),
                    amount_out: out.to_string(),
                    fee: fee.to_string(),
                };
                amount = out;
                s
            })
            .collect()
    })
}

proptest! {
    // verifies: REQ-ORD-41
    #[test]
    fn replay_is_a_pure_function_of_the_recorded_segments(segments in chain()) {
        let a = replay_node("n", &segments);
        let b = replay_node("n", &segments.clone());
        prop_assert_eq!(&a, &b);
        let last = segments.last().unwrap();
        let out = a.unwrap();
        prop_assert_eq!(out.amount, last.amount_out.clone());

        // The public network is read, never consulted for the replay itself.
        let nodes = [NodeRecord { node: "n".into(), segments: segments.clone() }];
        let net = Fake(Status::Final(Movement {
            destination: "addr-1".into(),
            asset: "USDC".into(),
            amount: last.amount_out.clone(),
        }));
        let end = exit_of("USDC", &last.amount_out);
        prop_assert_eq!(replay_route(&nodes, &end, &net), replay_route(&nodes, &end, &net));
    }

    // verifies: REQ-ORD-41
    #[test]
    fn a_perturbed_record_turns_consistency_into_a_mismatch(
        segments in chain(),
        pick in any::<prop::sample::Index>(),
        how in 0u8..2,
    ) {
        let last = segments.last().unwrap().amount_out.clone();
        let consistent = |segs: Vec<RecordedSegment>| {
            let net = Fake(Status::Final(Movement {
                destination: "addr-1".into(),
                asset: "USDC".into(),
                amount: last.clone(),
            }));
            let nodes = [NodeRecord { node: "n".into(), segments: segs }];
            replay_route(&nodes, &exit_of("USDC", &last), &net)
        };
        prop_assert!(matches!(consistent(segments.clone()), Replay::Consistent(_)));
        let mut broken = segments;
        let i = pick.index(broken.len());
        if how == 0 {
            // An amount in changed by one breaks the fee sum, or the link to the segment before.
            let n: u64 = broken[i].amount_in.parse().unwrap();
            broken[i].amount_in = (n + 1).to_string();
        } else {
            // A fee changed by one breaks the fee sum.
            let n: u64 = broken[i].fee.parse().unwrap();
            broken[i].fee = (n + 1).to_string();
        }
        prop_assert!(matches!(consistent(broken), Replay::Mismatch(_)));
    }

    // verifies: REQ-ORD-41, AC-35
    #[test]
    fn any_exit_amount_other_than_the_replayed_one_is_a_mismatch(
        segments in chain(),
        delta in 1u64..1000,
    ) {
        let last = segments.last().unwrap().amount_out.parse::<u64>().unwrap();
        let wrong = (last + delta).to_string();
        let nodes = [NodeRecord { node: "n".into(), segments }];
        let net = Fake(Status::Final(Movement {
            destination: "addr-1".into(),
            asset: "USDC".into(),
            amount: wrong.clone(),
        }));
        let replay = replay_route(&nodes, &exit_of("USDC", &wrong), &net);
        prop_assert!(matches!(replay, Replay::Mismatch(_)));
    }

    // verifies: AC-35
    #[test]
    fn a_route_ending_inside_the_node_never_records_a_check(segments in chain()) {
        let nodes = [NodeRecord { node: "n".into(), segments }];
        let replay = replay_route(&nodes, &RouteEnd::InsideNode, &Fake(Status::Unknown));
        prop_assert_eq!(replay, Replay::PublishersWord);
    }
}
