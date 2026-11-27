//! The record of each call a reader's process asks the host to make (`REQ-MDL-8` applied to readers): its
//! destination, the reader, its version and the request it served, refused calls included.
//!
//! An RPC pool's request is the egress layer's own record ([`OutboundCallRecord`]): `provider` is the reader,
//! `module_version` its version, `transaction` the request id it served (or [`START_UP`], for the reader's start-up
//! checks, which serve no request) and `operation` the kind of question. A light client's connection, which the egress
//! layer has no session for, is a [`ConnectionRecord`] with the same four facts.

use std::sync::Mutex;

use rutent_contracts::module_interface::OutboundCallRecord;
use serde::{Deserialize, Serialize};

use crate::config::Protocol;
use rutent_prov::egress::CallLog;

/// What a record names as its request when the call served a reader's start-up checks, not a question: never a
/// request id, which is Core's to draw.
pub const START_UP: &str = "start-up";

/// One call a reader's process asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum CallRecord {
    /// A request to an RPC endpoint, through the egress layer.
    Request(OutboundCallRecord),
    /// A light client's connection.
    Connection(ConnectionRecord),
}

impl CallRecord {
    /// The reader that asked for it.
    pub fn reader(&self) -> &str {
        match self {
            CallRecord::Request(r) => r.provider.as_str(),
            CallRecord::Connection(c) => &c.reader,
        }
    }

    /// The request it served, or [`START_UP`].
    pub fn request(&self) -> &str {
        match self {
            CallRecord::Request(r) => r.transaction.as_str(),
            CallRecord::Connection(c) => &c.request,
        }
    }
}

/// A light client's connection, opened or refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRecord {
    /// The reader.
    pub reader: String,
    /// Its version.
    pub version: String,
    /// The request it served, or [`START_UP`].
    pub request: String,
    /// When it was asked for, by the host's wall clock.
    pub started_at: String,
    /// The protocol asked for.
    pub protocol: Protocol,
    /// The host asked for.
    pub host: String,
    /// The port asked for.
    pub port: u16,
    /// The address connected to, if a connection was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// How it ended.
    pub outcome: ConnectionOutcome,
}

/// How a connection's opening ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionOutcome {
    /// Connected.
    Connected,
    /// Refused: a protocol or port the reader didn't declare, or a host that is no peer's name.
    UndeclaredDestination,
    /// Refused: the name resolved to an address no peer may have (a special-purpose one), or to none.
    AddressNotAllowed,
    /// Refused: the reader held as many connections as its cap allows.
    LimitReached,
    /// Nothing answered at the address checked.
    Unreachable,
}

/// Where the host's records go.
pub trait CallRecords: Send + Sync {
    /// Keeps `record`.
    fn record(&self, record: CallRecord);
}

/// Records kept in memory, in order.
#[derive(Debug, Default)]
pub struct MemoryRecords {
    records: Mutex<Vec<CallRecord>>,
}

impl MemoryRecords {
    /// No records yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every record so far.
    pub fn all(&self) -> Vec<CallRecord> {
        self.records.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

impl CallRecords for MemoryRecords {
    fn record(&self, record: CallRecord) {
        if let Ok(mut records) = self.records.lock() {
            records.push(record);
        }
    }
}

/// Records written to the host's standard error, one JSON line each: what an ECS task's log keeps.
#[derive(Debug, Default)]
pub struct StderrRecords;

impl CallRecords for StderrRecords {
    fn record(&self, record: CallRecord) {
        if let Ok(line) = serde_json::to_string(&record) {
            eprintln!("{line}");
        }
    }
}

/// The egress layer's call log, handing each record to the host's.
pub(crate) struct ToRecords<'a>(pub &'a dyn CallRecords);

impl CallLog for ToRecords<'_> {
    fn record(&mut self, record: OutboundCallRecord) {
        self.0.record(CallRecord::Request(record));
    }
}
