//! The shape of a tuple (`REQ-CAT-13`, `REQ-CAT-21`): a reserve connects an inner node with an outer one, an
//! exchange stays inside one node, and a ramp keeps its asset between two different nodes.

use rutent_contracts::provider_role::{Category, Denomination, NodeRef, OuterNode};
use rutent_contracts::router_api::{Endpoint as Typed, OperationKind, check_typed_operation};

use super::grant::{Endpoint, TupleKey};

/// Why a tuple is refused: the rule of the router-api contract it breaks, in the contract's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TupleRefusal(pub &'static str);

fn typed(end: &Endpoint, asset: &str) -> Typed {
    Typed {
        node: match end {
            Endpoint::Inner(id) => NodeRef::Inner(id.clone()),
            Endpoint::Outer(network) => NodeRef::Outer(OuterNode {
                network: network.clone(),
            }),
        },
        // The units don't matter to the shape of a tuple.
        asset: Denomination {
            id: asset.to_owned(),
            decimals: 0,
        },
    }
}

/// Checks a tuple's shape, with the typed-operation rules of the router-api contract, which the state machine
/// uses too, so the catalogue and the order can't disagree on what a segment is.
///
/// An exchange spanning two nodes, a ramp within one, and a reserve that doesn't connect an inner node with an
/// outer one are refused; so is a `to` asset on any tuple but an exchange's, or none on an exchange's.
pub fn check_tuple(tuple: &TupleKey) -> Result<(), TupleRefusal> {
    let (kind, to_asset) = match tuple.category {
        Category::Reserve => (
            if matches!(tuple.destination, Endpoint::Outer(_)) {
                OperationKind::Deposit
            } else {
                OperationKind::Withdrawal
            },
            None,
        ),
        Category::Exchange => {
            let Some(to) = tuple.to_asset.as_deref() else {
                return Err(TupleRefusal("an exchange states a to asset"));
            };
            (OperationKind::Exchange, Some(to))
        }
        Category::Ramp => (OperationKind::Route, None),
        Category::Verifier => {
            return Err(TupleRefusal("a verifier advertises classes, not tuples"));
        }
    };
    if tuple.category != Category::Exchange && tuple.to_asset.is_some() {
        return Err(TupleRefusal("only an exchange has a to asset"));
    }
    let from = typed(&tuple.origin, &tuple.asset);
    let to = typed(&tuple.destination, to_asset.unwrap_or(&tuple.asset));
    check_typed_operation(kind, &from, &to).map_err(|v| TupleRefusal(v.0))
}
