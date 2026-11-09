//! The operator's bound per tuple (`REQ-CAT-1`).

use std::collections::HashMap;

use super::grant::TupleKey;

/// What the operator allows of one tuple of one provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TupleBound {
    /// The tuple is not to be used at all.
    Restricted,
    /// Segments through it are at most this amount, in the tuple's input unit.
    Max(u128),
}

/// The operator's bounds, by provider and tuple. A tuple with no entry is bounded only by its grant.
#[derive(Debug, Clone, Default)]
pub struct OperatorBounds(HashMap<(String, TupleKey), TupleBound>);

impl OperatorBounds {
    pub(super) fn set(&mut self, provider: &str, tuple: TupleKey, bound: TupleBound) {
        self.0.insert((provider.to_owned(), tuple), bound);
    }

    pub(super) fn get(&self, provider: &str, tuple: &TupleKey) -> Option<&TupleBound> {
        self.0.get(&(provider.to_owned(), tuple.clone()))
    }
}
