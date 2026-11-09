//! The proxy's deployment settings: vault buckets separated by process role (`DEC-9`, ADR-0004).

use std::collections::BTreeMap;
use std::path::PathBuf;

use core::fmt;

use serde::Deserialize;

use super::kv::{KvConfig, KvResolver, Server, Token};
use super::resolver::{plain_path, plain_segment};
use super::transport::Transport;

/// The credential proxy's deployment settings, read from the deployment, never from a module.
///
/// Each **process role** names a vault bucket: the store that holds it, where in that store its providers'
/// secrets and statement keys sit, and how the proxy reaches it. Each provider is given one role, so its lookups
/// reach only its role's bucket. ADR-0004 records the owner's mitigation for the shared proxy in these words:
/// internal vault buckets apart from external integrations' buckets, by process role at deployment. The role
/// names are the deployment's own.
///
/// In JSON:
///
/// ```json
/// {
///   "roles": {
///     "internal": {
///       "server": "openbao",
///       "store": { "scheme": "https", "host": "bao.internal", "port": 8200 },
///       "ca_file": "/etc/rutent/bao-ca.pem",
///       "mount": "secret",
///       "secrets": "rutent/internal/secrets",
///       "keys": "rutent/internal/keys"
///     }
///   },
///   "providers": { "bridge-pay": "internal" }
/// }
/// ```
///
/// No token is written here: a role's token comes from the deployment's secret delivery when the proxy is built.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxySettings {
    /// Each process role's bucket, by role name.
    pub roles: BTreeMap<String, RoleSettings>,
    /// Each provider's process role, by provider identifier.
    pub providers: BTreeMap<String, String>,
}

/// One process role's vault bucket.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleSettings {
    /// Which store holds it.
    pub server: Server,
    /// Where the store is reached.
    pub store: StoreAddress,
    /// The PEM file of the certificate authorities the store's certificate must chain to. Required for HTTPS:
    /// the proxy trusts no built-in roots.
    #[serde(default)]
    pub ca_file: Option<PathBuf>,
    /// The KV version 2 engine's mount path.
    pub mount: String,
    /// The path under the mount that holds the role's providers' declared secrets.
    pub secrets: String,
    /// The path under the mount that holds the private halves of the role's providers' statement keys. It is
    /// kept apart from `secrets`, so no declared secret's name can reach a key.
    pub keys: String,
    /// The store namespace, sent as `X-Vault-Namespace`, if any.
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Where a store is reached.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreAddress {
    /// HTTPS, or plain HTTP to a loopback address for development.
    pub scheme: StoreScheme,
    /// The host name or address.
    pub host: String,
    /// The port.
    pub port: u16,
}

/// How a store is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreScheme {
    /// HTTPS, with the store's certificate verified against the role's CA.
    Https,
    /// Plain HTTP, allowed only to a loopback address, for a development server.
    Http,
}

/// Settings the proxy refuses, with a fixed reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidSettings(pub String);

impl fmt::Display for InvalidSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::error::Error for InvalidSettings {}

impl ProxySettings {
    /// The settings in `json`, checked.
    pub fn from_json(json: &str) -> Result<Self, InvalidSettings> {
        let settings: ProxySettings = serde_json::from_str(json)
            .map_err(|_| InvalidSettings("the settings are not the proxy's settings".to_owned()))?;
        settings.check()?;
        Ok(settings)
    }

    /// Checks the settings' rules a parse can't: every role and provider name is one plain path segment; every
    /// provider's role exists; every mount, prefix and namespace is plain path segments; and no two buckets in one
    /// store, namespace and mount nest, so no role's paths, and no secret's, reach another bucket's.
    pub fn check(&self) -> Result<(), InvalidSettings> {
        let invalid = |what: String| Err(InvalidSettings(what));
        let mut buckets = Vec::new();
        for (role, settings) in &self.roles {
            if !plain_segment(role) {
                return invalid(format!("role {role:?} is not one plain path segment"));
            }
            for (part, text) in [
                ("mount", &settings.mount),
                ("secrets", &settings.secrets),
                ("keys", &settings.keys),
            ] {
                if !plain_path(text) {
                    return invalid(format!("role {role:?}'s {part} is not plain path segments"));
                }
            }
            if let Some(namespace) = &settings.namespace
                && !plain_path(namespace)
            {
                return invalid(format!(
                    "role {role:?}'s namespace is not plain path segments"
                ));
            }
            let place = (
                &settings.store,
                settings.namespace.as_deref(),
                settings.mount.as_str(),
            );
            buckets.push((role, place, settings.secrets.as_str()));
            buckets.push((role, place, settings.keys.as_str()));
        }
        for (provider, role) in &self.providers {
            if !plain_segment(provider) {
                return invalid(format!(
                    "provider {provider:?} is not one plain path segment"
                ));
            }
            if !self.roles.contains_key(role) {
                return invalid(format!(
                    "provider {provider:?}'s role {role:?} is not defined"
                ));
            }
        }
        for (i, (role, place, prefix)) in buckets.iter().enumerate() {
            for (other, other_place, other_prefix) in &buckets[i + 1..] {
                if place == other_place && nests(prefix, other_prefix) {
                    return invalid(format!(
                        "role {role:?}'s and role {other:?}'s paths nest: {prefix:?} and {other_prefix:?}"
                    ));
                }
            }
        }
        Ok(())
    }

    /// The role `provider` is given, if any.
    pub fn role_of(&self, provider: &str) -> Option<&str> {
        self.providers.get(provider).map(String::as_str)
    }
}

/// One process role's bucket as two resolvers: one for its providers' declared secrets, one for their statement
/// keys.
#[derive(Debug)]
pub struct Bucket<R> {
    /// Resolves declared secrets.
    pub secrets: R,
    /// Resolves statement keys' private halves, by the public key's text.
    pub keys: R,
}

/// The bucket `role` names, over `transport`, read with `token`.
pub fn kv_bucket<T: Transport + Clone>(
    role: &RoleSettings,
    token: Token,
    transport: T,
) -> Result<Bucket<KvResolver<T>>, InvalidSettings> {
    let config = |prefix: &str| {
        let config = KvConfig::new(&role.mount, prefix, token.clone())
            .map_err(|e| InvalidSettings(e.to_string()))?;
        match &role.namespace {
            Some(namespace) => config
                .with_namespace(namespace)
                .map_err(|e| InvalidSettings(e.to_string())),
            None => Ok(config),
        }
    };
    let resolver = |config: KvConfig, transport: T| match role.server {
        Server::OpenBao => KvResolver::openbao(config, transport),
        Server::Vault => KvResolver::vault(config, transport),
    };
    Ok(Bucket {
        secrets: resolver(config(&role.secrets)?, transport.clone()),
        keys: resolver(config(&role.keys)?, transport),
    })
}

/// Whether two paths, compared segment by segment, are equal or one starts the other.
fn nests(a: &str, b: &str) -> bool {
    a.split('/').zip(b.split('/')).all(|(x, y)| x == y)
}
