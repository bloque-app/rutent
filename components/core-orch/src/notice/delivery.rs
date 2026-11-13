//! Delivery and its record (`T-023-02`, `REQ-ORD-20`): a notice the transport didn't take is retried within a bound,
//! each attempt is recorded, and what was delivered is kept durably, so a restart catches up without sending again.
//!
//! No contract holds a "notice delivered" record, and `core-internal`'s log has no such entry, so the record is this
//! module's own trait, [`Ledger`], which the assembly backs with a store that outlives the process. Core keeps no
//! delivery state of its own beyond it.
//!
//! **What a mark names.** A notice of an order is named by its receiver, the order and its sequence ([`Key::Order`]),
//! which the log rebuilds the same after a restart: its tries are counted from the ledger, and what was delivered is
//! not sent again. A raise is named by its id ([`Key::Raise`]); each try of it is recorded, but a raise is no part of
//! what the log rebuilds, so each time it is raised its tries count from none, and a raise still undelivered at a
//! restart is not retried: its entry recorded it undelivered. A charge counted far ahead has no key: it is retried
//! from memory, bounded alike, and after a restart a charge policy still sets apart is sent again, which the
//! operator's inbox holds once.
//!
//! **The bound** ([`BOUND`]). A notice is tried at most [`BOUND`] times in all, the first send included. The bound is
//! counted from the ledger, so a restart doesn't give it a fresh one. A notice that used its attempts is set apart
//! as abandoned ([`super::Notices::abandoned`]) and sent no more; its receiver recovers by asking for the current
//! state (`THR-103`).
//!
//! **Fails closed.** A mark that couldn't be kept ([`Ledger::record`] answering false) leaves the notice sendable
//! after a restart: at least once, never lost.

use std::fmt::Debug;

use super::wire::Receiver;

/// The most times one notice is tried, the first send included. Reversible default.
pub const BOUND: u32 = 5;

/// What a delivery mark names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// An order's notice to one receiver, at its sequence.
    Order {
        /// The receiver.
        to: Receiver,
        /// The order.
        order: String,
        /// The change's sequence.
        sequence: u64,
    },
    /// A raise to the operator, by its id.
    Raise {
        /// The raise's id.
        raise: String,
    },
}

/// One try at sending a notice, as it is recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// The notice.
    pub key: Key,
    /// Which try it was, counted from 1.
    pub number: u32,
    /// Whether the transport took the notice: a taken notice is delivered.
    pub taken: bool,
}

/// The durable record of delivery. The assembly backs it with a store that survives a restart.
pub trait Ledger: Debug {
    /// Whether the notice named `key` was delivered.
    fn delivered(&self, key: &Key) -> bool;
    /// How many times the notice named `key` was tried.
    fn attempts(&self, key: &Key) -> u32;
    /// Records `attempt`: true only where it was kept durably.
    fn record(&mut self, attempt: &Attempt) -> bool;
}

/// A ledger in memory, for a deployment whose notices don't outlive the process: it is what [`super::Notices::new`]
/// holds, and it forgets everything at a restart.
#[derive(Debug, Clone, Default)]
pub struct Volatile {
    history: Vec<Attempt>,
}

impl Volatile {
    /// Every attempt recorded, in order.
    pub fn history(&self) -> &[Attempt] {
        &self.history
    }
}

impl Ledger for Volatile {
    fn delivered(&self, key: &Key) -> bool {
        self.history.iter().any(|a| &a.key == key && a.taken)
    }

    fn attempts(&self, key: &Key) -> u32 {
        self.history
            .iter()
            .filter(|a| &a.key == key)
            .map(|a| a.number)
            .max()
            .unwrap_or(0)
    }

    fn record(&mut self, attempt: &Attempt) -> bool {
        self.history.push(attempt.clone());
        false
    }
}

/// What one pass of retrying did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Retried {
    /// Notices the transport took now.
    pub delivered: usize,
    /// Notices it didn't take, still within the bound.
    pub failed: usize,
    /// Notices that used their last attempt now, and are sent no more.
    pub abandoned: usize,
}
