//! What the platform and its children say to each other, one JSON message per frame
//! (`rutent_wasm_sandbox::write_frame`). A provider's process and the admitting compiler each read their machine
//! code or binaries as raw frames after a message announcing them.

use serde::{Deserialize, Serialize};

/// The platform to a provider's process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToProvider {
    /// Load this machine code: two raw frames follow, the sandboxed part's and the outside part's, whose SHA-256
    /// digests must be these, as the admission artefact records them.
    Load {
        /// The sandboxed part's machine-code digest.
        sandboxed: String,
        /// The outside part's machine-code digest.
        outside: String,
    },
    /// Hand this provider-role operation, in the operator's envelope, to the sandboxed part.
    Handle {
        /// The operation, as JSON.
        operation: String,
    },
}

/// A provider's process to the platform: an answer to a [`ToProvider`] message, or a request made while handling
/// one, which the platform answers with a [`Reply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FromProvider {
    /// Loaded, and both parts started.
    Ready,
    /// It couldn't load or a part didn't start.
    Failed {
        /// What went wrong, for the operator.
        message: String,
    },
    /// The sandboxed part's output, hex-encoded bytes.
    Done {
        /// The bytes as the part emitted them, in lower-case hexadecimal.
        output: String,
    },
    /// The sandboxed part trapped, ran out of fuel or memory, or its call failed; both parts were set up again.
    Trapped {
        /// What happened.
        message: String,
    },
    /// The sandboxed part's outside request, to be checked.
    OutsideRequest {
        /// The request as the part wrote it.
        json: String,
    },
    /// The outside part's response to the request being served, to be checked.
    OutsideResponse {
        /// The response as the part wrote it.
        json: String,
    },
    /// The sandboxed part tried to look up a secret, which it has no route to.
    DeniedSecret {
        /// The secret's name.
        name: String,
    },
    /// The sandboxed part tried to read state, which it has no route to.
    DeniedState {
        /// The entry.
        key: String,
    },
    /// The outside part's secret lookup.
    Lookup {
        /// The transaction it names.
        transaction: String,
        /// The outside operation it names.
        operation: String,
        /// The secret's name.
        name: String,
    },
    /// The outside part's state read.
    State {
        /// The entry.
        key: String,
    },
    /// The outside part's outbound call.
    Egress {
        /// The outside operation it is for.
        operation: String,
        /// `https` or `wss`.
        scheme: String,
        /// The host.
        host: String,
        /// The port.
        port: u16,
        /// The request body.
        body: String,
    },
    /// The outside part's cross-module call.
    Module {
        /// The provider called.
        callee: String,
        /// The provider-role operation kind asked.
        kind: String,
    },
}

/// The platform's answer to a request a provider's process made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Reply {
    /// The outside request passed: hand it to the outside part.
    Perform,
    /// The outside response the sandboxed part gets, as JSON.
    Answer {
        /// The response.
        json: String,
    },
    /// Noted; nothing comes back.
    Noted,
    /// The secret's current value.
    Secret {
        /// The value.
        value: String,
    },
    /// The lookup was refused.
    SecretRefused {
        /// `undeclared_secret` or `not_found`.
        reason: String,
    },
    /// The state entry, if the provider has one.
    State {
        /// Its value.
        value: Option<String>,
    },
    /// What came of an outbound call.
    Egress {
        /// `completed`, `failed`, `unknown` or `refused`.
        outcome: String,
    },
    /// What came of a cross-module call.
    Module {
        /// `delivered` or `refused`.
        outcome: String,
    },
}

/// The platform to the admitting compiler: two raw frames follow, the sandboxed part's binary and the outside
/// part's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileRequest {
    /// The platform version the compiler must be, refused if it isn't.
    pub platform_version: String,
}

/// The compiler's verdict on one part; a raw frame with its machine code follows, empty if refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compiled {
    /// `sandboxed` or `outside`.
    pub part: String,
    /// Why it was refused, if it was.
    pub refusal: Option<String>,
    /// Every import the component names.
    pub imports: Vec<String>,
    /// Every export it names.
    pub exports: Vec<String>,
}

/// Lower-case hexadecimal of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

/// The bytes of lower-case or upper-case hexadecimal, or `None` if it isn't.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let text = text.as_bytes();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

/// The SHA-256 digest of `bytes`, in lower-case hexadecimal.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(bytes))
}
