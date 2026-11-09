//! The outside part's egress (`F-014`, `REQ-MDL-7`, `REQ-MDL-8`, ADR-0004).
//!
//! This is the credential proxy's egress, a `DEC-20` path: a change here reaches `main` only with the owner's
//! click.
//!
//! An outside part reaches only what its running declaration lists for the operation in flight. [`Egress`] makes
//! every check on each connection of an outbound call, the first and each redirect:
//!
//! - **The destination.** The scheme, the exact host and the port must be a destination the operation in flight
//!   lists. A redirect is followed only to one; plain `http` never is, since the contract's [`Scheme`] has no
//!   plain variant.
//! - **The address (`B-17`).** The host is resolved once per connection, through a [`NameResolver`]. Every
//!   address it gives must lie within the destination's declared addresses, and the [`Connector`] is handed the
//!   checked address, never the name to resolve again. A name re-resolved to an undeclared address between two
//!   calls, or between two hops of one, is refused, and that address receives nothing.
//! - **TLS (`B-18`).** The connector verifies the peer for the declared host. A failed check is refused, never
//!   retried on another address and never downgraded.
//! - **The record (`REQ-MDL-8`).** Every call leaves one [`OutboundCallRecord`] in a [`CallLog`], refused calls
//!   included, naming the transaction, the provider, the module version, the operation, the secrets resolved
//!   for it, and each hop's destination and connected address.
//!
//! The TLS client is not here: [`Connector`] is the seam a `rustls` client (`T-014-02`) satisfies.
//!
//! [`Scheme`]: rutent_contracts::module_interface::Scheme
//! [`OutboundCallRecord`]: rutent_contracts::module_interface::OutboundCallRecord

mod connector;
mod layer;
mod resolve;

pub use connector::{
    ConnectError, Connector, Dial, Header, InvalidRequest, Method, Request, Response,
};
pub use layer::{Call, CallLog, Egress, Endpoint, MAX_HOPS, Outcome, Refusal};
pub use resolve::{NameResolver, SystemResolver};
