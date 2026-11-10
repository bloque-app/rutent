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
//!
//! `T-014-04` gives cross-module calls on each module's own credentials:
//!
//! - [`judge_call`]: a call only where the caller's release declares it and the route names it ([`RouteCalls`]),
//!   in the contract's order of checks, which the module platform applies to every call it routes. The callee's
//!   lookups are its own process's, so it resolves only its own secrets.
//!
//! `T-014-05` makes them the module platform's production world:
//!
//! - [`ProductionWorld`]: the proxy and the egress layer as the platform's `World`, told what each provider's
//!   module handles (`begin_*` and `end_*`), so every lookup is judged again by the proxy and recorded, and every
//!   outbound call goes through the egress layer over a real resolver and TLS connector.
//! - [`sign_result`]: the platform's results signed with the provider's statement key, resolved through the proxy
//!   and never held by the module.

mod calls;
mod https;
mod kv;
mod resolver;
mod settings;
mod shared;
mod signing;
mod transport;
mod world;

#[cfg(test)]
mod tests;

pub use calls::{
    CallAsked, CallJudgement, NO_ROUTE_RECORD, NO_ROUTE_SEGMENT, NamedCall, RouteCalls,
    RouteNotNamed, judge_call, no_route,
};
pub use https::HttpsTransport;
pub(crate) use https::{AnswerError, Bounded, client_config, exchange, handshake, left};
pub use kv::{InvalidConfig, KvConfig, KvResolver, Server, Token};
pub use resolver::{ResolveError, Resolver};
pub use settings::{
    Bucket, InvalidSettings, ProxySettings, RoleSettings, StoreAddress, StoreScheme, kv_bucket,
};
pub use shared::{Caller, CredentialProxy, InFlight, Refusal, Use, UseOf, answer, judge};
pub use signing::{Answering, SigningError, sign_result};
pub use transport::{Header, Request, Response, Transport, TransportError};
pub use world::{NoState, ProductionWorld, StateStore};
