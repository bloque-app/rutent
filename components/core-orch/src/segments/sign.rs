//! Writing the operation Core hands a provider: the chosen route's operation, at the amount the segment moves, as a
//! provider-role operation in its envelope, signed with the operator key.

use std::collections::BTreeMap;

use rutent_contracts::provider_role::{
    Conversion, Deposit, Direction, NodeRef, OPERATION_SHAPE, Operation, Signature,
    SignatureScheme, Statement, Terms, Transfer, Withdrawal, signing_input,
};
use rutent_contracts::router_api::{
    self, CatalogueSnapshot, Endpoint, FigureValue, Id, OperationKind, PolicyBounds,
    PositiveAmount, Timestamp,
};

use crate::rounds::OperatorKey;

/// Why no operation could be written for a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unwritten {
    /// The route's operation has ends its kind can't take, or a withdrawal has no session to be asked in.
    Ends,
    /// The operator key couldn't sign it now.
    Unsigned,
}

/// What a segment moves, and under which transaction and inputs.
pub(super) struct Moving<'a> {
    pub operation: &'a router_api::Operation,
    pub transaction: &'a Id,
    pub amount: &'a PositiveAmount,
    pub session: Option<&'a Id>,
    pub inputs: BTreeMap<String, String>,
}

/// The real operation, `exact_in` at the amount moved: a withdrawal in the user's session, every other kind with
/// none.
pub(super) fn operation(moving: &Moving<'_>) -> Result<Operation, Unwritten> {
    let op = moving.operation;
    let (from, to) = (&op.from, &op.to);
    let provider = op.provider.as_str().to_owned();
    let transaction = moving.transaction.as_str().to_owned();
    let amount = moving.amount.amount().as_str().to_owned();
    let inputs = moving.inputs.clone();
    let inner = |e: &Endpoint| match &e.node {
        NodeRef::Inner(n) => Ok(n.clone()),
        NodeRef::Outer(_) => Err(Unwritten::Ends),
    };
    let outer = |e: &Endpoint| match &e.node {
        NodeRef::Outer(n) => Ok(n.clone()),
        NodeRef::Inner(_) => Err(Unwritten::Ends),
    };
    Ok(match op.kind {
        OperationKind::Withdrawal => Operation::Withdrawal(Withdrawal {
            transaction,
            provider,
            session: Some(moving.session.ok_or(Unwritten::Ends)?.as_str().to_owned()),
            origin: outer(from)?,
            destination: inner(to)?,
            asset: from.asset.id.clone(),
            amount,
            dry_run: false,
            inputs,
        }),
        OperationKind::Deposit => Operation::Deposit(Deposit {
            transaction,
            provider,
            origin: inner(from)?,
            destination: outer(to)?,
            asset: from.asset.id.clone(),
            amount,
            dry_run: false,
            inputs,
        }),
        OperationKind::Route => Operation::Transfer(Transfer {
            transaction,
            provider,
            origin: inner(from)?,
            destination: inner(to)?,
            asset: from.asset.id.clone(),
            direction: Direction::ExactIn,
            amount,
            dry_run: false,
            inputs,
        }),
        OperationKind::Exchange => Operation::Conversion(Conversion {
            transaction,
            provider,
            node: inner(from)?,
            from: from.asset.id.clone(),
            to: to.asset.id.clone(),
            direction: Direction::ExactIn,
            amount,
            dry_run: false,
            inputs,
        }),
    })
}

/// `operation` in its envelope, made by `maker` at `made_at` for the bounds' deployment and protocol version, signed
/// with the operator key: the bytes as signed, which are what is logged and what is handed over.
pub(super) fn sign(
    key: &mut dyn OperatorKey,
    bounds: &PolicyBounds,
    maker: &Id,
    made_at: &Timestamp,
    operation: Operation,
) -> Result<Vec<u8>, Unwritten> {
    let mut statement = Statement {
        deployment: bounds.deployment.clone(),
        protocol_version: bounds.protocol_version.as_str().to_owned(),
        shape: OPERATION_SHAPE.to_owned(),
        maker: maker.as_str().to_owned(),
        made_at: made_at.as_str().to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: key.key(),
            value: base64url(&[0u8; 64]),
        },
        statement: operation,
    };
    let unsigned = serde_json::to_vec(&statement).map_err(|_| Unwritten::Unsigned)?;
    let input = signing_input(&unsigned).map_err(|_| Unwritten::Unsigned)?;
    statement.signature.value = base64url(&key.sign(&input).ok_or(Unwritten::Unsigned)?);
    serde_json::to_vec(&statement).map_err(|_| Unwritten::Unsigned)
}

/// What the snapshot a route was chosen over says of one of its operations: the terms of the tuple it runs through
/// and the settlement time its price declared, from the figures the operation cites.
pub(super) struct Cited<'a> {
    pub terms: &'a Terms,
    pub declared_seconds: Option<u64>,
}

/// The tuple figure and price an operation cites, in `snapshot`.
pub(super) fn cited<'a>(
    snapshot: &'a CatalogueSnapshot,
    operation: &router_api::Operation,
) -> Option<Cited<'a>> {
    let figure = |id: &Id| snapshot.figures.iter().find(|f| &f.figure_id == id);
    let mut terms = None;
    let mut declared_seconds = None;
    for id in operation.figures.iter() {
        let Some(f) = figure(id) else { continue };
        if f.origin.provider() != Some(&operation.provider) {
            continue;
        }
        match &f.value {
            FigureValue::ReserveTuple { tuple } => terms = terms.or(Some(&tuple.terms)),
            FigureValue::RampTuple { tuple } => terms = terms.or(Some(&tuple.terms)),
            FigureValue::ExchangeTuple { tuple } => terms = terms.or(Some(&tuple.terms)),
            FigureValue::Price {
                settlement_seconds, ..
            } => declared_seconds = declared_seconds.or(Some(u64::from(*settlement_seconds))),
            _ => {}
        }
    }
    Some(Cited {
        terms: terms?,
        declared_seconds,
    })
}

/// Bytes as base64url without padding.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}
