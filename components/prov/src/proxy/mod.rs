//! The shared credential proxy and its resolvers (`F-014`, `DEC-9`, ADR-0004).
//!
//! This is a `DEC-20` path: a change here reaches `main` only with the owner's click.
//!
//! `T-014-01` gives the resolver interface and its two implementations:
//!
//! - [`Resolver`]: resolve one secret by name, at call time, for one provider. Credentials are keyed by provider
//!   from the start, so splitting the shared proxy into one per provider later moves configuration, not data
//!   (ADR-0004).
//! - [`KvResolver`]: the resolver over the KV version 2 HTTP API that OpenBao and Vault both speak, built with
//!   [`KvResolver::openbao`] or [`KvResolver::vault`]. It speaks the HTTP API directly through a [`Transport`];
//!   no Vault client crate enters the build (`licences/RECORD`, `REQ-MOD-5`).
//!
//! Rotating a secret takes no new release (`REQ-MDL-6`): every lookup asks the store for the current version, and
//! nothing is cached.
//!
//! The proxy itself, which answers only for the calling process's own provider and an outside request in flight,
//! is `T-014-02`.

mod kv;
mod resolver;
mod transport;

pub use kv::{InvalidConfig, KvConfig, KvResolver, Server, Token};
pub use resolver::{ResolveError, Resolver};
pub use transport::{Header, Request, Response, Transport, TransportError};
