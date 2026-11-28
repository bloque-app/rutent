//! A key store's statement keys and nothing else of a vault bucket, for the reader host (`T-015-05`, `REQ-MDL-6`,
//! `DEC-9`): what `crate::keys` re-exports in place of the bucket, its resolvers, its token, its transport and a
//! process role's settings.
//!
//! [`StatementKeys`] is built from [`KeyStoreSettings`] and the store's token, and resolves the private half of a
//! statement key by the public key's text, under the reader's name, as the `keys` resolver of
//! [`crate::proxy::kv_bucket`] does, from the same server, mount, keys path and namespace. Its settings name the keys'
//! path and no other: they carry no `secrets` path, as a process role's settings do, and a settings file that names one
//! is refused. That keeps the host's code from naming a bucket's declared secrets by that field, but bounds nothing by
//! itself: the mount and the keys path are free strings, so a settings file could point them at any path the store
//! holds. The real bound is the token's policy on the store: a deployment gives the reader host a token whose policy
//! reaches the readers' keys alone (the milestone reviewer's check of `T-015-05` at `3a53de34`, Low 1). The token and the
//! transport stay inside: nothing of them is given back.

use core::fmt;
use std::path::PathBuf;

use rutent_contracts::module_interface::{Identifier, Name, SecretValue};
use serde::Deserialize;

use super::https::HttpsTransport;
use super::kv::{InvalidConfig, KvConfig, KvResolver, Server, Token};
use super::resolver::{ResolveError, Resolver};
use super::settings::{InvalidSettings, StoreAddress};

/// Where the readers' statement keys are kept: a KV version 2 store, its mount, and the one path under it that holds
/// the keys. In JSON, a process role's settings without its `secrets` path, which is refused:
///
/// ```json
/// {
///   "server": "openbao",
///   "store": { "scheme": "https", "host": "bao.internal", "port": 8200 },
///   "ca_file": "/etc/rutent/bao-ca.pem",
///   "mount": "secret",
///   "keys": "rutent/readers/keys"
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyStoreSettings {
    /// Which store holds the keys.
    pub server: Server,
    /// Where the store is reached.
    pub store: StoreAddress,
    /// The PEM file of the certificate authorities the store's certificate must chain to. Required for HTTPS: no
    /// built-in roots are trusted.
    #[serde(default)]
    pub ca_file: Option<PathBuf>,
    /// The KV version 2 engine's mount path.
    pub mount: String,
    /// The path under the mount that holds the private halves of the readers' statement keys.
    pub keys: String,
    /// The store namespace, sent as `X-Vault-Namespace`, if any.
    #[serde(default)]
    pub namespace: Option<String>,
}

/// The statement keys at [`KeyStoreSettings`]' keys path, over HTTPS to its store (or plain HTTP to a loopback address,
/// for development), read with the store's token. It builds one resolver, for that path: none for any other. Its one
/// field is private and names no type of the bucket's, so its interface gives none of them back (`tools/boundary-check`
/// holds this file's public items to that).
pub struct StatementKeys {
    keys: Box<dyn Resolver + Send + Sync>,
}

impl StatementKeys {
    /// The statement keys `settings` names, read with `token`, the store's token as the deployment delivers it.
    /// Refused if the token is empty or holds a control character, if the store can't be reached as the settings say
    /// (an HTTPS store needs its CA file), or if its paths aren't plain.
    pub fn new(settings: &KeyStoreSettings, token: String) -> Result<Self, InvalidSettings> {
        let token = Token::new(token).ok_or_else(|| {
            InvalidSettings(
                "the key store's token is empty or holds a control character".to_owned(),
            )
        })?;
        let ca = match &settings.ca_file {
            Some(path) => Some(std::fs::read(path).map_err(|_| {
                InvalidSettings("the key store's CA file could not be read".to_owned())
            })?),
            None => None,
        };
        let transport = HttpsTransport::new(&settings.store, ca.as_deref())
            .map_err(|e| InvalidSettings(e.to_string()))?;
        let invalid = |e: InvalidConfig| InvalidSettings(e.to_string());
        let mut config = KvConfig::new(&settings.mount, &settings.keys, token).map_err(invalid)?;
        if let Some(namespace) = &settings.namespace {
            config = config.with_namespace(namespace).map_err(invalid)?;
        }
        let keys = match settings.server {
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

#[cfg(test)]
mod tests {
    use super::KeyStoreSettings;

    // verifies: REQ-MDL-6
    #[test]
    fn key_store_settings_that_name_a_secrets_path_are_refused() {
        let keys = r#"{"server": "vault", "store": {"scheme": "http", "host": "127.0.0.1", "port": 8200},
            "mount": "secret", "keys": "rutent/readers/keys"}"#;
        let settings: KeyStoreSettings = serde_json::from_str(keys).expect("the keys' path alone");
        assert_eq!(settings.keys, "rutent/readers/keys");
        // A process role's settings, with the path of its declared secrets beside its keys: refused.
        let role = keys.replace(
            r#""mount""#,
            r#""secrets": "rutent/readers/secrets", "mount""#,
        );
        let refused = serde_json::from_str::<KeyStoreSettings>(&role).unwrap_err();
        assert!(refused.to_string().contains("secrets"), "{refused}");
    }
}
