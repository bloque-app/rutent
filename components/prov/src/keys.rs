//! The statement keys' resolver, for the reader host (`T-015-04`, `REQ-MDL-6`, `REQ-MDL-17`, `DEC-9`): the one path of
//! the credential proxy that `rutent-reader-host` may name (`tools/boundary-check`'s admitted paths).
//!
//! The host resolves each reader's statement key as the platform resolves a provider's: through `T-014-01`'s
//! [`Resolver`], by name at call time, keyed by the reader as a provider is keyed, from the keys of a vault bucket
//! that [`kv_bucket`] builds over OpenBao or Vault ([`KvResolver`]) from a process role's [`RoleSettings`], reached by
//! an [`HttpsTransport`]. These are re-exports of [`crate::proxy`]'s items, each from the file that defines it, nothing
//! more: the proxy's judging, recording and signing stay out of the host's reach.

pub use crate::proxy::https::HttpsTransport;
pub use crate::proxy::kv::{InvalidConfig, KvConfig, KvResolver, Server, Token};
pub use crate::proxy::resolver::{ResolveError, Resolver};
pub use crate::proxy::settings::{
    Bucket, InvalidSettings, RoleSettings, StoreAddress, StoreScheme, kv_bucket,
};
pub use crate::proxy::transport::TransportError;
