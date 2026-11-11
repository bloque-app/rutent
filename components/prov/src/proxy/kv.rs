//! The resolver over the KV version 2 HTTP API, against OpenBao and Vault (`DEC-9`, ADR-0004).

use core::fmt;

use rutent_contracts::module_interface::{Identifier, Name, SecretValue};
use serde_json::Value;

use super::resolver::{ResolveError, Resolver, plain_path, plain_segment};
use super::transport::{Header, Request, Transport};

/// Which store a [`KvResolver`] talks to. Both speak the same KV version 2 API; the server is kept so a deployment
/// and its records can say which one answered. In deployment settings it is written `openbao` or `vault`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum Server {
    /// OpenBao, the Linux Foundation's fork of Vault, under MPL-2.0.
    #[serde(rename = "openbao")]
    OpenBao,
    /// HashiCorp Vault, under BUSL-1.1, run by the operator apart from rutent.
    #[serde(rename = "vault")]
    Vault,
}

/// A store token. `Debug` never prints it (`THR-50`).
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// The token, or `None` if it is empty or holds a control character, which can't go in a header.
    pub fn new(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        (!text.is_empty() && !text.chars().any(char::is_control)).then_some(Token(text))
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

/// A [`KvConfig`] part that isn't one or more plain path segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidConfig(pub &'static str);

impl fmt::Display for InvalidConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl core::error::Error for InvalidConfig {}

/// Where a [`KvResolver`] reads, and as whom.
///
/// Provider `p`'s secret `n` is read from `/v1/{mount}/data/{prefix}/{p}/{n}`, field `value`. The mount and prefix
/// are the deployment's: they are how vault buckets are separated by process role (`DEC-9`), and a deployment
/// gives each role its own token, which the store's policy confines to that role's paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvConfig {
    mount: String,
    prefix: String,
    namespace: Option<String>,
    token: Token,
}

impl KvConfig {
    /// A configuration. `mount` is the KV version 2 engine's mount path and `prefix` the path under it that holds
    /// every provider's keys; each is one or more plain segments joined by `/`.
    pub fn new(mount: &str, prefix: &str, token: Token) -> Result<Self, InvalidConfig> {
        if !plain_path(mount) {
            return Err(InvalidConfig("the mount is not plain path segments"));
        }
        if !plain_path(prefix) {
            return Err(InvalidConfig("the prefix is not plain path segments"));
        }
        Ok(KvConfig {
            mount: mount.to_owned(),
            prefix: prefix.to_owned(),
            namespace: None,
            token,
        })
    }

    /// The same configuration, read in `namespace`, sent as `X-Vault-Namespace`.
    pub fn with_namespace(mut self, namespace: &str) -> Result<Self, InvalidConfig> {
        if !plain_path(namespace) {
            return Err(InvalidConfig("the namespace is not plain path segments"));
        }
        self.namespace = Some(namespace.to_owned());
        Ok(self)
    }
}

/// A [`Resolver`] over the KV version 2 HTTP API, for OpenBao or Vault.
///
/// It asks the store on every call, for the current version (no `version` parameter), and keeps nothing between
/// calls: a rotated secret is used from the next lookup on, with no new release (`REQ-MDL-6`).
#[derive(Debug)]
pub struct KvResolver<T> {
    server: Server,
    config: KvConfig,
    transport: T,
}

impl<T: Transport> KvResolver<T> {
    /// A resolver against OpenBao.
    pub fn openbao(config: KvConfig, transport: T) -> Self {
        KvResolver {
            server: Server::OpenBao,
            config,
            transport,
        }
    }

    /// A resolver against Vault.
    pub fn vault(config: KvConfig, transport: T) -> Self {
        KvResolver {
            server: Server::Vault,
            config,
            transport,
        }
    }

    /// The store it talks to.
    pub fn server(&self) -> Server {
        self.server
    }

    /// The transport it sends through.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    fn request(&self, provider: &str, name: &str) -> Request {
        let KvConfig {
            mount,
            prefix,
            namespace,
            token,
        } = &self.config;
        let mut headers = vec![Header::sensitive("X-Vault-Token", token.0.clone())];
        if let Some(namespace) = namespace {
            headers.push(Header::plain("X-Vault-Namespace", namespace.clone()));
        }
        Request::new(
            format!("/v1/{mount}/data/{prefix}/{provider}/{name}"),
            headers,
        )
    }
}

impl<T: Transport> Resolver for KvResolver<T> {
    fn resolve(&self, provider: &Identifier, name: &Name) -> Result<SecretValue, ResolveError> {
        self.resolve_until(provider, name, None)
    }

    fn resolve_until(
        &self,
        provider: &Identifier,
        name: &Name,
        due: Option<std::time::Instant>,
    ) -> Result<SecretValue, ResolveError> {
        let (provider, name) = (provider.as_str(), name.as_str());
        if !plain_segment(provider) || !plain_segment(name) {
            return Err(ResolveError::InvalidName);
        }
        let response = self
            .transport
            .get_until(&self.request(provider, name), due)
            .map_err(ResolveError::Transport)?;
        match response.status {
            200 => current_value(&response.body),
            // Vault and OpenBao answer 404 both for a path never written and for a deleted current version.
            404 => Err(ResolveError::NotFound),
            401 | 403 => Err(ResolveError::Denied),
            429 | 500..=599 => Err(ResolveError::Unavailable {
                status: response.status,
            }),
            status => Err(ResolveError::Unexpected { status }),
        }
    }
}

/// The `value` of a KV version 2 read's `data.data`. A `null` there is a deleted or destroyed version.
fn current_value(body: &[u8]) -> Result<SecretValue, ResolveError> {
    let answer: Value = serde_json::from_slice(body).map_err(|_| ResolveError::Malformed)?;
    let data = answer
        .get("data")
        .and_then(|d| d.get("data"))
        .ok_or(ResolveError::Malformed)?;
    if data.is_null() {
        return Err(ResolveError::NotFound);
    }
    data.get("value")
        .and_then(Value::as_str)
        .and_then(SecretValue::new)
        .ok_or(ResolveError::Malformed)
}
