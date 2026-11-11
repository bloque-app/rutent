//! The snapshot read as a network: each advertised tuple is a segment, an edge from one endpoint (node and asset in
//! a unit) to another (`REQ-QTE-11`, `REQ-QTE-14`), priced by its one price where the snapshot holds one. A tuple
//! with no price is still a segment, so a path that reaches it is a candidate, dropped as `no_price` (router-api
//! 3.0.0, `REQ-AUD-4`).
//!
//! A segment of any kind connects what its ends say, so a route needs neither local money nor an exchange: a
//! reserve takes value between an outer and an inner node, a ramp between two inner nodes in one asset, an
//! exchange between two assets at one node (`REQ-QTE-14`). Segments are kept in order of their tuple figure's
//! id, so nothing the search finds depends on the order the snapshot lists its advertised figures in.

use std::collections::BTreeMap;

use rutent_contracts::provider_role::{Band, Denomination, NodeRef};
use rutent_contracts::router_api::{
    CatalogueSnapshot, Endpoint, FeeFormula, Figure, FigureValue, Id, OperationKind, Pricing,
};

use crate::price::whole;

/// One tuple the router can route through: its figure, its price if the snapshot holds one, and its two ends.
pub(crate) struct Segment<'a> {
    /// The tuple figure.
    pub(crate) tuple: &'a Figure,
    /// Its one price figure, and that price's fee formula; `None` for a tuple advertised with no price.
    pub(crate) price: Option<(&'a Figure, &'a FeeFormula)>,
    /// The price's rate, for an exchange.
    pub(crate) rate: Option<&'a Pricing>,
    /// The price's declared settlement seconds; never read for a tuple with no price, which no ranked candidate
    /// takes.
    pub(crate) settlement_seconds: u64,
    /// The tuple's provider.
    pub(crate) provider: &'a Id,
    /// The operation the tuple performs.
    pub(crate) kind: OperationKind,
    /// Where value enters it, in its input unit.
    pub(crate) from: Endpoint,
    /// Where value leaves it, in its output unit.
    pub(crate) to: Endpoint,
    /// The amounts it serves, in its input unit.
    pub(crate) bands: &'a [Band],
}

impl Segment<'_> {
    /// Whether the tuple serves `amount` entering it: within one of its bands, in its own input unit.
    pub(crate) fn serves(&self, amount: &num_bigint::BigUint) -> bool {
        self.bands
            .iter()
            .any(|band| match (whole(&band.min), whole(&band.max)) {
                (Some(min), Some(max)) => &min <= amount && amount <= &max,
                _ => false,
            })
    }
}

fn unit(id: &str, decimals: u8) -> Denomination {
    Denomination {
        id: id.to_owned(),
        decimals,
    }
}

/// The tuple's operation and its two ends, or `None` for a figure that isn't a tuple.
fn ends(figure: &Figure) -> Option<(OperationKind, Endpoint, Endpoint, &[Band])> {
    let at = |node: NodeRef, asset| Endpoint { node, asset };
    Some(match &figure.value {
        FigureValue::ReserveTuple { tuple } => (
            match tuple.origin {
                NodeRef::Outer(_) => OperationKind::Withdrawal,
                NodeRef::Inner(_) => OperationKind::Deposit,
            },
            at(
                tuple.origin.clone(),
                unit(&tuple.asset, tuple.decimals.origin),
            ),
            at(
                tuple.destination.clone(),
                unit(&tuple.asset, tuple.decimals.destination),
            ),
            &tuple.terms.bands,
        ),
        FigureValue::RampTuple { tuple } => (
            OperationKind::Route,
            at(
                NodeRef::Inner(tuple.origin.clone()),
                unit(&tuple.asset, tuple.decimals.origin),
            ),
            at(
                NodeRef::Inner(tuple.destination.clone()),
                unit(&tuple.asset, tuple.decimals.destination),
            ),
            &tuple.terms.bands,
        ),
        FigureValue::ExchangeTuple { tuple } => (
            OperationKind::Exchange,
            at(NodeRef::Inner(tuple.node.clone()), tuple.from.clone()),
            at(NodeRef::Inner(tuple.node.clone()), tuple.to.clone()),
            &tuple.terms.bands,
        ),
        _ => return None,
    })
}

/// Every segment of the snapshot, in order of tuple figure id, a tuple with no price included.
pub(crate) fn segments(snapshot: &CatalogueSnapshot) -> Vec<Segment<'_>> {
    let prices: BTreeMap<&str, &Figure> = snapshot
        .figures
        .iter()
        .filter_map(|f| match &f.value {
            FigureValue::Price { tuple, .. } => Some((tuple.as_str(), f)),
            _ => None,
        })
        .collect();
    let mut found: Vec<Segment<'_>> = snapshot
        .figures
        .iter()
        .filter_map(|tuple| {
            let (kind, from, to, bands) = ends(tuple)?;
            let (price, rate, settlement_seconds) = match prices.get(tuple.figure_id.as_str()) {
                Some(&price) => match &price.value {
                    FigureValue::Price {
                        fee,
                        rate,
                        settlement_seconds,
                        ..
                    } => (Some((price, fee)), rate.as_ref(), settlement_seconds.get()),
                    _ => return None,
                },
                None => (None, None, u64::MAX),
            };
            Some(Segment {
                tuple,
                price,
                rate,
                settlement_seconds,
                provider: tuple.origin.provider()?,
                kind,
                from,
                to,
                bands,
            })
        })
        .collect();
    found.sort_by(|a, b| a.tuple.figure_id.as_str().cmp(b.tuple.figure_id.as_str()));
    found
}
