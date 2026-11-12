//! The VOS host server's wire (`T-024-02`): one JSON request per line on the server's standard input, one JSON
//! answer per line on its standard output, carrying the host contract's calls and answers.
//!
//! `rutent-scenarios-vos` runs the assembled Core, whose module platform needs a newer toolchain than VOS's crates
//! build with, so it can't link the client: it drives the client's `VosHost` on three replicas in this package's
//! server, `rutent-stm-vos-serve` (feature `vos`), through this wire. The wire is plain serde over the contract's own
//! types, and links nothing of VOS.

use rutent_contracts::state_machine::host::{
    Delivery, HostError, RefusalRecord, SnapshotEntry, TransitionRecord,
};
use rutent_contracts::state_machine::{Id, Order, RecordedInput};
use serde::{Deserialize, Serialize};

/// One call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum Request {
    /// The first call: a fresh host on an empty store.
    Open,
    /// The first call: a fresh host holding exactly what `entries` hold, halted.
    Import {
        /// The snapshot stream's entries.
        entries: Vec<SnapshotEntry>,
    },
    /// Records and applies one input.
    Submit {
        /// The input.
        input: Box<RecordedInput>,
    },
    /// Every order with a record.
    Orders,
    /// An order's state.
    State {
        /// The order.
        order: Id,
    },
    /// An order's journal.
    Journal {
        /// The order.
        order: Id,
    },
    /// Stops taking submits.
    Halt,
    /// Takes submits again.
    Resume,
    /// Everything the host holds, as a snapshot stream's entries.
    Export,
    /// Stops the server, and its nodes.
    Stop,
}

/// One answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Answer {
    /// Done, with nothing to tell.
    Done,
    /// The input was new: its record.
    Recorded {
        /// The record.
        record: TransitionRecord,
    },
    /// An equal input was already recorded: that record.
    AlreadyRecorded {
        /// The record.
        record: TransitionRecord,
    },
    /// The orders.
    Orders {
        /// Each order.
        orders: Vec<Id>,
    },
    /// An order's state.
    State {
        /// The state, if it has one.
        state: Option<Order>,
    },
    /// An order's journal.
    Journal {
        /// Its records.
        records: Vec<TransitionRecord>,
    },
    /// The snapshot stream's entries.
    Entries {
        /// The entries.
        entries: Vec<SnapshotEntry>,
    },
    /// The host failed or refused.
    Failed {
        /// Why.
        error: Error,
    },
}

/// A [`HostError`], as the wire carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum Error {
    /// [`HostError::RecordIdReused`].
    RecordIdReused {
        /// The refusal's entry.
        refusal: Box<RefusalRecord>,
    },
    /// [`HostError::Halted`].
    Halted,
    /// [`HostError::NotHalted`].
    NotHalted,
    /// [`HostError::SnapshotRefused`].
    SnapshotRefused {
        /// Why.
        reason: String,
    },
    /// [`HostError::ImportIncomplete`].
    ImportIncomplete,
    /// [`HostError::Unavailable`].
    Unavailable {
        /// What failed.
        reason: String,
    },
}

impl From<HostError> for Error {
    fn from(e: HostError) -> Self {
        match e {
            HostError::RecordIdReused { refusal } => Error::RecordIdReused { refusal },
            HostError::Halted => Error::Halted,
            HostError::NotHalted => Error::NotHalted,
            HostError::SnapshotRefused { reason } => Error::SnapshotRefused { reason },
            HostError::ImportIncomplete => Error::ImportIncomplete,
            HostError::Unavailable { reason } => Error::Unavailable { reason },
        }
    }
}

impl From<Error> for HostError {
    fn from(e: Error) -> Self {
        match e {
            Error::RecordIdReused { refusal } => HostError::RecordIdReused { refusal },
            Error::Halted => HostError::Halted,
            Error::NotHalted => HostError::NotHalted,
            Error::SnapshotRefused { reason } => HostError::SnapshotRefused { reason },
            Error::ImportIncomplete => HostError::ImportIncomplete,
            Error::Unavailable { reason } => HostError::Unavailable { reason },
        }
    }
}

impl From<Result<Delivery, HostError>> for Answer {
    fn from(delivered: Result<Delivery, HostError>) -> Self {
        match delivered {
            Ok(Delivery::Recorded(record)) => Answer::Recorded { record },
            Ok(Delivery::AlreadyRecorded(record)) => Answer::AlreadyRecorded { record },
            Err(e) => Answer::Failed { error: e.into() },
        }
    }
}
