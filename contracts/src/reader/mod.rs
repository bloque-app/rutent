//! The reader contract: what Core and a reader host say to each other over mTLS (`DEC-28`, ADR-0007 as amended,
//! `T-020-59`).
//!
//! Each public network's reader is a reader service, a separate process of the operator's own, run by a reader host
//! (`CMP-EVIDENCE`'s standalone part) and never linked into Core. The reader service is outside Core, so its protocol
//! is a contract of its own, the seventh, versioned under `DEC-4` from 1.0.0. Its artefacts are the JSON Schemas under
//! `contracts/reader/schemas/`; `contracts/reader/README.md` lists every rule and where it is enforced.
//!
//! # A question
//!
//! A [`ReaderQuestion`] names the reader asked, the network, what is asked ([`Asked`]: `network_rules`,
//! `movement_status` or `movement_of`, each read-only), a request id Core draws at random for it ([`RequestId`],
//! never from a counter, so a host can't compute an answer before it is asked) and a deadline its answer must meet on
//! Core's own clock, never the host's. Only Core asks.
//!
//! # An answer
//!
//! A [`ReaderAnswer`] is a statement in the contracts' envelope ([`crate::provider_role::Statement`], shape
//! [`ANSWER_SHAPE`]), made by the reader and signed with its statement key, which the reader host holds and never hands
//! to the reader's process. It names the request, the network, the reader's version and what was asked, and answers it
//! ([`Answered`]) or says it is unreadable. [`check_reader_answer`] refuses an answer to another request, question,
//! network, movement or transaction, from another reader, under another key or version, or past its deadline.
//!
//! `network_rules` answers the network's identity ([`NetworkIdentity`]: a genesis hash or a chain id), its finality
//! definition and reorg rule, and the reader's trust rule ([`TrustRule`]): `light_client`, or `rpc_pool` with its
//! endpoints and a quorum of at least two ([`LEAST_QUORUM`]). A pool with a quorum below two, or with fewer endpoints
//! than its quorum, is refused.
//!
//! # Rust is as strict as the schemas
//!
//! Every identifier, time, digest and count is a checked newtype, optional fields refuse `null`, every struct refuses
//! unknown fields, and deserialising a [`TrustRule`] or a [`ReaderAnswer`] runs its `check`, which also enforces the
//! rules JSON Schema can't state.

mod check;
mod types;

pub use check::{ReaderMismatch, check_reader_answer};
pub use types::*;

/// The contract's version: the first `## ` heading of `contracts/reader/CHANGELOG.md`.
pub const VERSION: &str = "1.0.0";

/// A rule of the contract that a value breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation(pub &'static str);

impl core::fmt::Display for Violation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

impl core::error::Error for Violation {}
