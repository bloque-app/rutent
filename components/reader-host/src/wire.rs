//! The wire between the host and a reader's process: its one channel, the standard input and output the host hands
//! it, carrying frames of JSON.
//!
//! A frame is its length as four little-endian bytes, then that many bytes. The host writes [`ToReader`] frames to
//! the reader's standard input and reads [`FromReader`] frames from its standard output, each one bounded
//! ([`crate::config::ReaderLimits::max_frame`]). Nothing on this wire is a key: the reader answers in the clear, and the
//! host signs.
//!
//! The reader's process has no network of its own. To read its network it asks the host: an [`FromReader::Rpc`] for a
//! pool of RPC endpoints, which the host sends to each endpoint named and answers only where the quorum agree, or
//! [`FromReader::Open`], [`FromReader::Send`] and [`FromReader::Close`] for a light client's connections, which the
//! host opens only by the reader's protocol and port rule and its cap.

use std::io::{self, Read, Write};

use rutent_contracts::reader::{NetworkIdentity, ReaderQuestion, TrustRule};
use rutent_contracts::router_api::{Id, Version};
use serde::{Deserialize, Serialize};

use crate::config::{LightClientRule, Protocol};

/// Writes one frame.
pub fn write_frame(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    let length = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too long"))?;
    out.write_all(&length.to_le_bytes())?;
    out.write_all(bytes)?;
    out.flush()
}

/// Reads one frame of at most `max` bytes, or `None` at the end of the stream. A longer frame is refused from its
/// length alone: nothing is allocated for it.
pub fn read_frame(input: &mut impl Read, max: usize) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too long"));
    }
    let mut bytes = vec![0u8; length];
    input.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}

/// What the host tells a reader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "to_reader", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToReader {
    /// The first frame: who the reader is, what it reads, and what it may reach. It carries no key.
    Start {
        /// The reader's name on this host.
        reader: Id,
        /// The network it reads.
        network: Id,
        /// Its version.
        version: Version,
        /// The network's identity, which its start-up checks must find.
        identity: NetworkIdentity,
        /// Its trust rule.
        trust: TrustRule,
        /// A light client's rule, if it is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        light_client: Option<LightClientRule>,
    },
    /// A question from Core, as Core sent it: its request id and its deadline are Core's.
    Question {
        /// The question.
        question: ReaderQuestion,
    },
    /// The host's answer to an [`FromReader::Rpc`].
    Rpc {
        /// The call it answers.
        call: u64,
        /// What the pool agreed on, or why there is nothing to read.
        result: RpcResult,
    },
    /// The host's answer to an [`FromReader::Open`].
    Opened {
        /// The call it answers.
        call: u64,
        /// The connection, or why there is none.
        result: OpenResult,
    },
    /// What a connection gave after an [`FromReader::Send`]: what arrived within the host's wait, which may be nothing.
    Received {
        /// The connection.
        conn: u64,
        /// The bytes, or why the connection is gone.
        result: ReceiveResult,
    },
}

/// What an RPC pool agreed on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "rpc", rename_all = "snake_case", deny_unknown_fields)]
pub enum RpcResult {
    /// At least the quorum of the endpoints named answered this body, and no other answer reached the quorum.
    Agreed {
        /// The body, as text.
        body: String,
    },
    /// Nothing to read: too few answers, answers that disagree, or a destination the reader did not declare.
    Unreadable {
        /// Why, as a label.
        why: String,
    },
}

/// A connection opened, or refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "open", rename_all = "snake_case", deny_unknown_fields)]
pub enum OpenResult {
    /// Open, under this number.
    Open {
        /// The connection's number.
        conn: u64,
    },
    /// Refused or unreachable.
    Refused {
        /// Why, as a label.
        why: String,
    },
}

/// What a connection gave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "receive", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReceiveResult {
    /// These bytes, possibly none.
    Bytes {
        /// The bytes.
        bytes: Vec<u8>,
    },
    /// The connection is closed, or never was.
    Closed,
}

/// What a reader tells the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "from_reader", rename_all = "snake_case", deny_unknown_fields)]
pub enum FromReader {
    /// Its start-up checks passed: the identity of the network it found. The host holds it to its configuration, and
    /// the reader answers nothing until it matches.
    Checked {
        /// The network's identity, as the reader found it.
        identity: NetworkIdentity,
    },
    /// One JSON-RPC request, `POST /`, to the pool endpoints named.
    Rpc {
        /// The call's number, the reader's.
        call: u64,
        /// The endpoints to ask, as the trust rule writes them.
        endpoints: Vec<String>,
        /// The request's body.
        body: String,
    },
    /// Opens a connection for a light client.
    Open {
        /// The call's number, the reader's.
        call: u64,
        /// The protocol.
        protocol: Protocol,
        /// The peer's host: a DNS name or an IP address.
        host: String,
        /// The port.
        port: u16,
    },
    /// Sends bytes on a connection, and asks for what comes back.
    Send {
        /// The connection.
        conn: u64,
        /// The bytes.
        bytes: Vec<u8>,
    },
    /// Closes a connection.
    Close {
        /// The connection.
        conn: u64,
    },
    /// Its answer to the question it is on: a `reader` answer's body, unsigned (`urn:rutent:reader:answer#/$defs/answer`).
    /// The host reads it as a value, and signs it only if it names the question asked and holds to its shape.
    Answer {
        /// The answer's body.
        answer: serde_json::Value,
    },
    /// A line for the host's log of the reader.
    Log {
        /// The line.
        line: String,
    },
}
