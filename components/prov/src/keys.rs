//! A reader's statement keys, for the reader host (`T-015-04`, `T-015-05`, `REQ-MDL-6`, `REQ-MDL-17`, `DEC-9`): the one
//! path of the credential proxy that `rutent-reader-host` may name (`tools/boundary-check`'s admitted paths).
//!
//! The host resolves each reader's statement key as the platform resolves a provider's: through `T-014-01`'s
//! [`Resolver`], by name at call time, keyed by the reader as a provider is keyed, from the keys of a process role's
//! vault bucket over OpenBao or Vault, which [`StatementKeys`] reads from the role's [`RoleSettings`] and token. That
//! facade resolves statement keys only: the bucket's declared secrets, its resolvers, its token and its transport stay
//! inside the proxy. These are re-exports of [`crate::proxy`]'s items, each from the file that defines it, nothing
//! more: the proxy's judging, recording and signing stay out of the host's reach.

pub use crate::proxy::kv::Server;
pub use crate::proxy::resolver::{ResolveError, Resolver};
pub use crate::proxy::settings::{InvalidSettings, RoleSettings, StoreAddress, StoreScheme};
pub use crate::proxy::statement_keys::StatementKeys;
