//! One destination per route, checked as the assumption it is (`DEC-13`).

use rutent_contracts::provider_role::{Denomination, NodeRef, OuterNode};
use rutent_contracts::state_machine::{Endpoint, Id, OperationKind, SegmentPlan};
use rutent_evidence::{DestinationViolation, one_destination};

fn endpoint(node: NodeRef, asset: &str) -> Endpoint {
    Endpoint {
        node,
        asset: Denomination {
            id: asset.into(),
            decimals: 6,
        },
    }
}

fn inner(name: &str) -> NodeRef {
    NodeRef::Inner(name.into())
}

fn outer(network: &str) -> NodeRef {
    NodeRef::Outer(OuterNode {
        network: network.into(),
    })
}

fn segment(id: &str, kind: OperationKind, from: NodeRef, to: NodeRef) -> SegmentPlan {
    SegmentPlan {
        segment_id: Id::new(id).unwrap(),
        provider: Id::new("p").unwrap(),
        kind,
        from: endpoint(from, "USDC"),
        to: endpoint(to, "USDC"),
    }
}

fn withdrawal() -> SegmentPlan {
    segment("s1", OperationKind::Withdrawal, outer("bank"), inner("hub"))
}

fn deposit(id: &str, network: &str) -> SegmentPlan {
    segment(id, OperationKind::Deposit, inner("hub"), outer(network))
}

// verifies: DEC-13
#[test]
fn route_with_one_deposit_has_that_deposit_as_its_destination() {
    let route = vec![withdrawal(), deposit("s2", "chain-a")];
    let destination = one_destination(&route).unwrap();
    assert_eq!(destination.to.node, outer("chain-a"));
}

// verifies: DEC-13
#[test]
fn route_with_two_destinations_violates_the_assumption() {
    let route = vec![
        withdrawal(),
        deposit("s2", "chain-a"),
        deposit("s3", "chain-b"),
    ];
    assert_eq!(
        one_destination(&route),
        Err(DestinationViolation::MoreThanOneDestination)
    );
}

// verifies: DEC-13
#[test]
fn route_with_no_destination_violates_the_assumption() {
    let route = vec![withdrawal()];
    assert_eq!(
        one_destination(&route),
        Err(DestinationViolation::NoDestination)
    );
    assert_eq!(
        one_destination(&[]),
        Err(DestinationViolation::NoDestination)
    );
}

// verifies: DEC-13
#[test]
fn destination_that_is_not_a_last_deposit_violates_the_assumption() {
    let route = vec![
        withdrawal(),
        deposit("s2", "chain-a"),
        segment("s3", OperationKind::Route, inner("hub"), inner("other")),
    ];
    assert_eq!(
        one_destination(&route),
        Err(DestinationViolation::DestinationNotLast)
    );
}
