//! Replay of a private node's recorded inner segments against its public exit (`REQ-ORD-41`, `DEC-24`, ADR-0005).
//! A local fake network stands in; the simulated network is `T-015-02`.

use rutent_evidence::replay::{
    Mismatch, NodeRecord, PublicExit, RecordedSegment, Replay, RouteEnd, Stated, Unreplayable,
    replay_node, replay_route,
};
use rutent_evidence::{
    Finality, Movement, MovementId, NetworkRules, PublicNetwork, ReorgRule, Status, Unmatched,
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

fn movement(asset: &str, amount: &str) -> Movement {
    Movement {
        destination: "addr-1".into(),
        asset: asset.into(),
        amount: amount.into(),
    }
}

fn final_exit(asset: &str, amount: &str) -> Fake {
    Fake(Status::Final(movement(asset, amount)))
}

fn seg(
    asset_in: &str,
    amount_in: &str,
    asset_out: &str,
    amount_out: &str,
    fee: &str,
) -> RecordedSegment {
    RecordedSegment {
        asset_in: asset_in.into(),
        amount_in: amount_in.into(),
        asset_out: asset_out.into(),
        amount_out: amount_out.into(),
        fee: fee.into(),
    }
}

fn node(name: &str, segments: Vec<RecordedSegment>) -> NodeRecord {
    NodeRecord {
        node: name.into(),
        segments,
    }
}

fn exit(asset: &str, amount: &str) -> RouteEnd {
    RouteEnd::Public(PublicExit {
        movement: MovementId::new("tx-1").unwrap(),
        network: "chain-a".into(),
        destination: "addr-1".into(),
        stated: Stated {
            asset: asset.into(),
            amount: amount.into(),
        },
    })
}

// verifies: REQ-ORD-41, AC-35
#[test]
fn mismatch_with_the_public_movement_out_is_recorded() {
    let nodes = [node(
        "vault",
        vec![seg("USDC", "1000", "USDC", "990", "10")],
    )];
    let net = final_exit("USDC", "900");
    let replay = replay_route(&nodes, &exit("USDC", "990"), &net);
    assert_eq!(
        replay,
        Replay::Mismatch(vec![Mismatch::PublicExit(Unmatched::WrongAmount)])
    );
}

// verifies: REQ-ORD-41, AC-35
#[test]
fn mismatch_with_the_stated_outcome_is_recorded() {
    let nodes = [node(
        "vault",
        vec![seg("USDC", "1000", "USDC", "990", "10")],
    )];
    let net = final_exit("USDC", "990");
    let replay = replay_route(&nodes, &exit("USDC", "995"), &net);
    assert_eq!(
        replay,
        Replay::Mismatch(vec![Mismatch::StatedOutcome {
            replayed_asset: "USDC".into(),
            replayed_amount: "990".into(),
        }])
    );
}

// verifies: REQ-ORD-41
#[test]
fn a_broken_chain_inside_the_node_is_recorded() {
    let nodes = [node(
        "vault",
        vec![
            seg("USDC", "1000", "EURC", "900", "0"),
            seg("EURC", "800", "EURC", "790", "10"),
        ],
    )];
    let net = final_exit("EURC", "790");
    let replay = replay_route(&nodes, &exit("EURC", "790"), &net);
    assert_eq!(
        replay,
        Replay::Mismatch(vec![Mismatch::BrokenChain {
            node: "vault".into(),
            at: 1
        }])
    );
}

// verifies: REQ-ORD-41
#[test]
fn a_fee_that_does_not_account_for_the_difference_is_recorded() {
    let nodes = [node("vault", vec![seg("USDC", "1000", "USDC", "990", "3")])];
    let net = final_exit("USDC", "990");
    let replay = replay_route(&nodes, &exit("USDC", "990"), &net);
    assert_eq!(
        replay,
        Replay::Mismatch(vec![Mismatch::FeeMismatch {
            node: "vault".into(),
            at: 0
        }])
    );
}

// verifies: REQ-ORD-41
#[test]
fn an_amount_that_is_not_a_decimal_integer_is_recorded() {
    let nodes = [node("vault", vec![seg("USDC", "1e3", "USDC", "990", "10")])];
    let net = final_exit("USDC", "990");
    let replay = replay_route(&nodes, &exit("USDC", "990"), &net);
    assert_eq!(
        replay,
        Replay::Mismatch(vec![Mismatch::Malformed {
            node: "vault".into(),
            at: 0
        }])
    );
}

// verifies: REQ-ORD-41
#[test]
fn an_exit_the_network_does_not_hold_final_is_recorded() {
    let nodes = [node(
        "vault",
        vec![seg("USDC", "1000", "USDC", "990", "10")],
    )];
    for (status, why) in [
        (Status::Unknown, Unmatched::UnknownMovement),
        (
            Status::Pending(movement("USDC", "990")),
            Unmatched::NotFinal,
        ),
        (Status::ReorgedOut, Unmatched::ReorgedOut),
    ] {
        let replay = replay_route(&nodes, &exit("USDC", "990"), &Fake(status));
        assert_eq!(replay, Replay::Mismatch(vec![Mismatch::PublicExit(why)]));
    }
}

// verifies: AC-35
#[test]
fn match_is_recorded_as_consistency_only() {
    let nodes = [node(
        "vault",
        vec![seg("USDC", "1000", "USDC", "990", "10")],
    )];
    let net = final_exit("USDC", "990");
    let replay = replay_route(&nodes, &exit("USDC", "990"), &net);
    let Replay::Consistent(consistency) = replay else {
        panic!("expected consistency, got {replay:?}");
    };
    assert_eq!(consistency.nodes, 1);
    assert_eq!(consistency.asset, "USDC");
    assert_eq!(consistency.amount, "990");
    assert!(!consistency.proves_segments_happened());
}

// verifies: AC-35
#[test]
fn route_ending_inside_the_node_records_no_replay_check() {
    let nodes = [node(
        "vault",
        vec![seg("USDC", "1000", "USDC", "990", "10")],
    )];
    let replay = replay_route(&nodes, &RouteEnd::InsideNode, &final_exit("USDC", "1"));
    assert_eq!(replay, Replay::PublishersWord);
}

// verifies: REQ-ORD-41
#[test]
fn a_public_exit_with_nothing_recorded_cannot_be_replayed() {
    let net = final_exit("USDC", "990");
    assert_eq!(
        replay_route(&[], &exit("USDC", "990"), &net),
        Replay::Unreplayable(Unreplayable::NoNodes)
    );
    let nodes = [node("vault", vec![])];
    assert_eq!(
        replay_route(&nodes, &exit("USDC", "990"), &net),
        Replay::Unreplayable(Unreplayable::NothingRecorded("vault".into()))
    );
}

// verifies: DEC-24
#[test]
fn route_across_two_private_nodes_replays_each() {
    let first = node("vault-a", vec![seg("USDC", "1000", "USDC", "995", "5")]);
    let second = node("vault-b", vec![seg("USDC", "995", "USDC", "990", "5")]);
    let net = final_exit("USDC", "990");
    let replay = replay_route(&[first.clone(), second.clone()], &exit("USDC", "990"), &net);
    let Replay::Consistent(consistency) = replay else {
        panic!("expected consistency, got {replay:?}");
    };
    assert_eq!(consistency.nodes, 2);

    // A fault inside the first node is found even though the second and the exit agree.
    let bad_first = node("vault-a", vec![seg("USDC", "1000", "USDC", "995", "1")]);
    assert_eq!(
        replay_route(&[bad_first, second.clone()], &exit("USDC", "990"), &net),
        Replay::Mismatch(vec![Mismatch::FeeMismatch {
            node: "vault-a".into(),
            at: 0
        }])
    );

    // What one node hands the next must be what the next recorded taking in.
    let off = node("vault-b", vec![seg("USDC", "994", "USDC", "990", "4")]);
    assert_eq!(
        replay_route(&[first, off], &exit("USDC", "990"), &net),
        Replay::Mismatch(vec![Mismatch::HandoffBroken {
            from: "vault-a".into(),
            to: "vault-b".into()
        }])
    );
}

// verifies: REQ-ORD-41
#[test]
fn replaying_a_node_gives_what_leaves_it() {
    let out = replay_node(
        "vault",
        &[
            seg("USDC", "1000", "EURC", "900", "0"),
            seg("EURC", "900", "EURC", "890", "10"),
        ],
    )
    .unwrap();
    assert_eq!((out.asset.as_str(), out.amount.as_str()), ("EURC", "890"));
}

// verifies: REQ-ORD-41
#[test]
fn a_fee_larger_than_what_went_in_is_recorded_even_when_the_asset_changes() {
    let nodes = [node(
        "vault",
        vec![seg("USDC", "1000", "EURC", "900", "1001")],
    )];
    let net = final_exit("EURC", "900");
    let replay = replay_route(&nodes, &exit("EURC", "900"), &net);
    assert_eq!(
        replay,
        Replay::Mismatch(vec![Mismatch::FeeMismatch {
            node: "vault".into(),
            at: 0
        }])
    );
}

// verifies: REQ-ORD-41
#[test]
fn an_empty_record_replays_to_a_distinct_mismatch() {
    assert_eq!(
        replay_node("vault", &[]),
        Err(vec![Mismatch::NoSegments {
            node: "vault".into()
        }])
    );
}

// verifies: REQ-ORD-41, AC-35
#[test]
fn a_fault_inside_a_node_does_not_hide_a_mismatch_with_the_public_exit() {
    let nodes = [node("vault", vec![seg("USDC", "1000", "USDC", "990", "3")])];
    let net = final_exit("USDC", "900");
    let replay = replay_route(&nodes, &exit("USDC", "995"), &net);
    assert_eq!(
        replay,
        Replay::Mismatch(vec![
            Mismatch::FeeMismatch {
                node: "vault".into(),
                at: 0
            },
            Mismatch::PublicExit(Unmatched::WrongAmount),
            Mismatch::StatedOutcome {
                replayed_asset: "USDC".into(),
                replayed_amount: "990".into()
            },
        ])
    );
}

// verifies: REQ-ORD-41
#[test]
fn a_chain_whose_asset_changes_between_segments_is_recorded_as_broken() {
    // The amounts link up; only the asset doesn't: the first gives out EURC, the second takes in GBPC.
    let segments = [
        seg("USDC", "1000", "EURC", "900", "0"),
        seg("GBPC", "900", "GBPC", "890", "10"),
    ];
    assert_eq!(
        replay_node("vault", &segments),
        Err(vec![Mismatch::BrokenChain {
            node: "vault".into(),
            at: 1
        }])
    );
}

// verifies: REQ-ORD-41, DEC-24
#[test]
fn a_handoff_of_another_asset_between_private_nodes_is_recorded() {
    // The amount handed on is the amount taken in; only the asset differs.
    let first = node("vault-a", vec![seg("USDC", "1000", "USDC", "995", "5")]);
    let second = node("vault-b", vec![seg("EURC", "995", "EURC", "990", "5")]);
    let net = final_exit("EURC", "990");
    assert_eq!(
        replay_route(&[first, second], &exit("EURC", "990"), &net),
        Replay::Mismatch(vec![Mismatch::HandoffBroken {
            from: "vault-a".into(),
            to: "vault-b".into()
        }])
    );
}

// verifies: REQ-ORD-41, AC-35
#[test]
fn a_stated_outcome_in_another_asset_is_recorded() {
    // The replay and the public exit agree on USDC 990; the provider states EURC 990.
    let nodes = [node(
        "vault",
        vec![seg("USDC", "1000", "USDC", "990", "10")],
    )];
    let net = final_exit("USDC", "990");
    assert_eq!(
        replay_route(&nodes, &exit("EURC", "990"), &net),
        Replay::Mismatch(vec![Mismatch::StatedOutcome {
            replayed_asset: "USDC".into(),
            replayed_amount: "990".into(),
        }])
    );
}

// verifies: REQ-ORD-41
#[test]
fn an_amount_with_a_leading_zero_or_a_sign_is_recorded_as_malformed() {
    // Both parse as integers that would balance the segment; neither is the one canonical spelling.
    for segment in [
        seg("USDC", "1000", "USDC", "0990", "10"),
        seg("USDC", "1000", "USDC", "995", "+5"),
    ] {
        assert_eq!(
            replay_node("vault", &[segment]),
            Err(vec![Mismatch::Malformed {
                node: "vault".into(),
                at: 0
            }])
        );
    }
}

// verifies: REQ-ORD-41
#[test]
fn a_fee_of_all_that_went_in_is_not_a_mismatch() {
    // No fee may exceed what went in; one equal to it may stand.
    assert!(replay_node("vault", &[seg("USDC", "1000", "USDC", "0", "1000")]).is_ok());
}
