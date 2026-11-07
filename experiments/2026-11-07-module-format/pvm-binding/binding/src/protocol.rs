//! What crosses between the platform (the binding's own process) and its other processes: a provider's worker and
//! the admitting compiler. One JSON message per line, on the child's standard input and output; binaries in
//! hexadecimal.
//!
//! The worker names no provider in anything it sends. The platform knows which provider a message is from by the
//! pipe it came on, as the credential proxy must (`DEC-9`, ADR-0003): a worker can't ask for another provider's
//! secret or state, whatever it writes.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

/// From the platform to a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToWorker {
    /// Compile and start a release; keep the running one if it doesn't start.
    Load {
        /// The release's version.
        version: String,
        /// The sandboxed part's blob.
        sandboxed: String,
        /// The outside part's blob.
        outside: String,
        /// The digests admission recorded for the sandboxed part, which the worker's own compilation must
        /// give before anything is instantiated.
        expect_sandboxed: String,
        /// The same, for the outside part.
        expect_outside: String,
        /// The SHA-256 of the sandboxed part's blob, as admission received it.
        received_sandboxed: String,
        /// The same, for the outside part.
        received_outside: String,
    },
    /// Handle one provider-role operation, in its envelope, as JSON.
    Handle {
        /// The operation.
        operation: String,
        /// Its transaction.
        transaction: String,
    },
    /// The answer to the worker's last host call.
    Reply(Reply),
}

/// From a worker to the platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromWorker {
    /// Started; each part's digest, as the worker's own compilation made it, and the sandbox worker processes.
    Loaded {
        /// The sandboxed part's.
        sandboxed: String,
        /// The outside part's.
        outside: String,
        /// The PolkaVM sandbox worker processes, under the Linux sandbox.
        guest_pids: Vec<u32>,
    },
    /// A part's blob or its compilation isn't what admission recorded; nothing was instantiated, and the previous
    /// release, if any, still runs.
    DigestMismatch {
        /// Which part, and which digest.
        reason: String,
    },
    /// The release didn't start; the previous one, if any, still runs.
    NotStarted {
        /// Why.
        reason: String,
    },
    /// The outside part asks a host call, which the platform answers with a [`Reply`].
    Host(HostCall),
    /// The sandboxed part sends an outside request. The platform checks it and answers with a [`Reply::Status`],
    /// 0 to let the outside part serve it; until the request's [`FromWorker::OutsideDone`], every host call is
    /// bound to it.
    Outside {
        /// The request, as the sandboxed part wrote it.
        request: String,
    },
    /// The outside part has served the request in flight. The platform checks the response and answers with a
    /// [`Reply::Status`], 0 to pass it to the sandboxed part.
    OutsideDone {
        /// The response the outside part wrote, if any.
        response: Option<String>,
    },
    /// The sandboxed part returned its output.
    Handled {
        /// The bytes, in hexadecimal.
        output: String,
    },
    /// The guest trapped, ran out of gas or broke the protocol.
    Failed {
        /// Why.
        reason: String,
    },
}

/// A host call of the outside part, for the platform to answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostCall {
    /// Resolve a secret for the outside request in flight.
    Secret {
        /// The outside operation the guest names, which must be the request's in flight.
        operation: String,
        /// The secret's name.
        name: String,
    },
    /// Read a state entry of the module's own provider.
    State {
        /// The entry.
        key: String,
    },
    /// Call an endpoint.
    Egress {
        /// The outside operation the guest names, which must be the request's in flight.
        operation: String,
        /// `scheme://host:port`.
        url: String,
        /// The request's body.
        body: String,
    },
    /// Call another provider's module.
    Call {
        /// The callee.
        callee: String,
        /// The operation kind asked.
        kind: String,
    },
}

/// The platform's answer to a host call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    /// A value: a resolved secret or a state entry.
    Value(String),
    /// No value: refused, or not found.
    Nothing,
    /// A status: 0 completed or delivered, anything else not.
    Status(u64),
}

/// To the admitting compiler: the two parts of one release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmitRequest {
    /// The sandboxed part's blob.
    pub sandboxed: String,
    /// The outside part's blob.
    pub outside: String,
}

/// From the admitting compiler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmitResponse {
    /// Both parts are readable, import only what their part may, and compiled.
    Compiled {
        /// The sandboxed part's digest.
        sandboxed: String,
        /// The outside part's digest.
        outside: String,
        /// Whether the digests are of machine code.
        machine_code: bool,
        /// The machine code's lengths, if any.
        machine_code_len: Option<(usize, usize)>,
        /// How long compiling both took, in microseconds.
        micros: u128,
    },
    /// Refused.
    Refused {
        /// Why.
        reason: String,
    },
}

/// Writes one message as a line.
pub fn send<T: Serialize>(out: &mut impl Write, message: &T) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(message).map_err(std::io::Error::other)?;
    line.push(b'\n');
    out.write_all(&line)?;
    out.flush()
}

/// Reads one message line; `None` at the end of the stream.
pub fn receive<T: for<'de> Deserialize<'de>>(
    input: &mut impl BufRead,
) -> std::io::Result<Option<T>> {
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(std::io::Error::other)
}

/// Bytes in lower-case hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Bytes from hexadecimal.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: REQ-MDL-15
    #[test]
    fn hexadecimal_round_trips_and_refuses_odd_lengths() {
        let bytes = [0u8, 1, 0xab, 0xff];
        assert_eq!(unhex(&hex(&bytes)), Some(bytes.to_vec()));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }
}
