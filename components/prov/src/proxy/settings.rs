//! The proxy's deployment settings: vault buckets separated by process role (`DEC-9`, ADR-0004).

use std::collections::BTreeMap;
use std::path::PathBuf;

use core::fmt;

use rutent_contracts::provider_role::Deployment;
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
///   "deployment": { "name": "rutent-prod", "operator_key": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA" },
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
    /// The deployment the proxy serves: its name and the operator's key (`REQ-PRT-1`). An advertisement of
    /// another deployment is not admitted, and the operator key is never resolved as a statement key.
    pub deployment: Deployment,
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
    /// provider's role exists; every mount, prefix and namespace is plain path segments; and no two buckets on one
    /// store nest, so no role's paths, and no secret's, reach another bucket's.
    ///
    /// Buckets are compared by the path the store resolves, segment by segment: the namespace, the mount, the KV
    /// version 2 `data` segment, then the prefix. So mount `secret` with prefix `data/x` is the same bucket as mount
    /// `secret/data` with prefix `x`, and namespace `ns1` with mount `secret` the same as mount `ns1/secret`. A
    /// store is its host, compared without case, and its port.
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
            let store = (
                settings.store.host.to_ascii_lowercase(),
                settings.store.port,
            );
            for prefix in [&settings.secrets, &settings.keys] {
                let path: Vec<&str> = settings
                    .namespace
                    .iter()
                    .chain([&settings.mount])
                    .flat_map(|part| part.split('/'))
                    .chain(["data"])
                    .chain(prefix.split('/'))
                    .collect();
                buckets.push((role, store.clone(), path));
            }
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
        for (i, (role, store, path)) in buckets.iter().enumerate() {
            for (other, other_store, other_path) in &buckets[i + 1..] {
                if store == other_store && nests(path, other_path) {
                    return invalid(format!(
                        "role {role:?}'s and role {other:?}'s paths nest: {} and {}",
                        path.join("/"),
                        other_path.join("/")
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
fn nests(a: &[&str], b: &[&str]) -> bool {
    a.iter().zip(b).all(|(x, y)| x == y)
}
