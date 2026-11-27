//! A process role's statement keys and nothing else of its bucket, for the reader host (`T-015-05`, `REQ-MDL-6`,
//! `DEC-9`): what `crate::keys` re-exports in place of the bucket, its resolvers, its token and its transport.
//!
//! [`StatementKeys`] is built from a process role's settings and the role's token, and resolves the private half of a
//! statement key by the public key's text, under the reader's name, as the `keys` resolver of
//! [`crate::proxy::kv_bucket`] does, built the same way from the role's server, mount, `keys` path and namespace. The
//! bucket's declared secrets are never reached: no resolver is built for the role's `secrets` path. The token and the
//! transport stay inside: nothing of them is given back.

use core::fmt;

use rutent_contracts::module_interface::{Identifier, Name, SecretValue};

use super::https::HttpsTransport;
use super::kv::{InvalidConfig, KvConfig, KvResolver, Server, Token};
use super::resolver::{ResolveError, Resolver};
use super::settings::{InvalidSettings, RoleSettings};

/// The statement keys of one process role's vault bucket, over HTTPS to its store (or plain HTTP to a loopback
/// address, for development), read with the role's token. It resolves keys only: never the bucket's declared secrets.
/// Its one field is private and names no type of the bucket's, so its interface gives none of them back
/// (`tools/boundary-check` holds this file's public items to that).
pub struct StatementKeys {
    keys: Box<dyn Resolver + Send + Sync>,
}

impl StatementKeys {
    /// The statement keys of `role`'s bucket, read with `token`, the role's store token as the deployment delivers it.
    /// Refused if the token is empty or holds a control character, if the role's store can't be reached as its
    /// settings say (an HTTPS store needs its CA file), or if its paths aren't plain.
    pub fn for_role(role: &RoleSettings, token: String) -> Result<Self, InvalidSettings> {
        let token = Token::new(token).ok_or_else(|| {
            InvalidSettings(
                "the key store's token is empty or holds a control character".to_owned(),
            )
        })?;
        let transport =
            HttpsTransport::for_role(role).map_err(|e| InvalidSettings(e.to_string()))?;
        let invalid = |e: InvalidConfig| InvalidSettings(e.to_string());
        let mut config = KvConfig::new(&role.mount, &role.keys, token).map_err(invalid)?;
        if let Some(namespace) = &role.namespace {
            config = config.with_namespace(namespace).map_err(invalid)?;
        }
        let keys = match role.server {
            Server::OpenBao => KvResolver::openbao(config, transport),
            Server::Vault => KvResolver::vault(config, transport),
        };
        Ok(StatementKeys {
            keys: Box::new(keys),
        })
    }
}

impl Resolver for StatementKeys {
    fn resolve(&self, provider: &Identifier, name: &Name) -> Result<SecretValue, ResolveError> {
        self.keys.resolve(provider, name)
    }

    fn resolve_until(
        &self,
        provider: &Identifier,
        name: &Name,
        due: Option<std::time::Instant>,
    ) -> Result<SecretValue, ResolveError> {
        self.keys.resolve_until(provider, name, due)
    }
}

impl fmt::Debug for StatementKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StatementKeys")
    }
}
