//! The resolver interface (`REQ-MDL-17`, `DEC-9`).

use core::fmt;

use rutent_contracts::module_interface::{Identifier, Name, SecretValue};

use super::transport::TransportError;

/// Resolves a provider's secret by name, at call time (`REQ-MDL-17`, `DEC-9`).
///
/// Credentials are keyed by provider: the same name under two providers is two secrets. An implementation asks its
/// store on every call and keeps no copy, so a rotated secret is used from the next lookup on, with no new release
/// (`REQ-MDL-6`).
///
/// It checks nothing about who asks. Answering only for the calling process's own provider, and only for an outside
/// request in flight, is the proxy's job (`T-014-02`).
pub trait Resolver {
    /// The current value of `provider`'s secret `name`.
    fn resolve(&self, provider: &Identifier, name: &Name) -> Result<SecretValue, ResolveError>;
}

/// Why a secret wasn't resolved. No variant carries the store's answer, the token or a value (`THR-50`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The provider or the name isn't one plain path segment, so it could address keys outside that provider's.
    /// Nothing was sent.
    InvalidName,
    /// The provider holds no current secret by that name: it never existed, or its current version was deleted or
    /// destroyed.
    NotFound,
    /// The store refused the resolver's token.
    Denied,
    /// The store couldn't answer now: rate-limited, sealed or failing.
    Unavailable {
        /// The HTTP status it answered with.
        status: u16,
    },
    /// The store answered with a status the resolver doesn't expect.
    Unexpected {
        /// The HTTP status it answered with.
        status: u16,
    },
    /// The store answered 200 without a non-empty text `value`.
    Malformed,
    /// The request didn't reach the store, or its answer didn't come back.
    Transport(TransportError),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::InvalidName => {
                f.write_str("provider or secret name is not a plain path segment")
            }
            ResolveError::NotFound => {
                f.write_str("no current secret by that name for that provider")
            }
            ResolveError::Denied => f.write_str("the secret store refused the resolver's token"),
            ResolveError::Unavailable { status } => {
                write!(f, "the secret store is unavailable (HTTP {status})")
            }
            ResolveError::Unexpected { status } => {
                write!(f, "the secret store answered an unexpected HTTP {status}")
            }
            ResolveError::Malformed => f.write_str("the secret store's answer has no usable value"),
            ResolveError::Transport(error) => write!(f, "transport: {error}"),
        }
    }
}

impl core::error::Error for ResolveError {}

/// The longest path segment let into a request, in bytes.
pub(super) const MAX_SEGMENT: usize = 255;

/// Whether `text` is one plain path segment: 1 to [`MAX_SEGMENT`] bytes of ASCII letters, digits, `-`, `_` and `.`,
/// and not `.` or `..`. Nothing else is let into a path, so no provider or name can step outside its provider's
/// keys, add a query or need escaping (`DEC-9`, `REQ-MDL-9`).
pub(super) fn plain_segment(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_SEGMENT
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Whether `text` is one or more plain segments joined by `/`.
pub(super) fn plain_path(text: &str) -> bool {
    text.split('/').all(plain_segment)
}
