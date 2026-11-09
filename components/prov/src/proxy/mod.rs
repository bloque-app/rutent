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
//! `T-014-02` gives the proxy itself:
//!
//! - [`CredentialProxy`]: answers each lookup only for the calling process's own provider ([`Caller`]), only for
//!   the transaction and operation of the outside request in flight for that provider's module, and only a secret
//!   that operation of the admitted release declares. It resolves the statement key only as the admitted release
//!   declares it, and never a key the provider's advertisement lists for `advertising`. Every use, refused ones
//!   included, is recorded with its transaction ([`Use`]).
//! - [`judge`]: the lookup rule on its own, which the module platform applies to every lookup it serves.
//! - [`ProxySettings`]: vault buckets by process role, read from deployment settings; each provider's lookups
//!   reach only its role's [`Bucket`].
//! - [`HttpsTransport`]: the production [`Transport`], HTTPS with the store's certificate verified against the
//!   deployment's CA (rustls), and plain HTTP only to a loopback address, for development.

mod https;
mod kv;
mod resolver;
mod settings;
mod shared;
mod transport;

#[cfg(test)]
mod tests;

pub use https::HttpsTransport;
pub use kv::{InvalidConfig, KvConfig, KvResolver, Server, Token};
pub use resolver::{ResolveError, Resolver};
pub use settings::{
    Bucket, InvalidSettings, ProxySettings, RoleSettings, StoreAddress, StoreScheme, kv_bucket,
};
pub use shared::{Caller, CredentialProxy, InFlight, Refusal, Use, UseOf, answer, judge};
pub use transport::{Header, Request, Response, Transport, TransportError};
