//! The HTTP surface a resolver speaks through, behind a trait so the resolvers are tested without a server.

use core::fmt;

/// Sends a resolver's `GET` to its store and returns the answer.
///
/// The transport owns the store's address, TLS and timeouts; the resolver gives it only the path and headers. It
/// must not follow redirects: a redirect comes back as its own status.
///
/// It must speak HTTPS with the server's certificate verified, and refuse plain HTTP to any address but loopback,
/// which is allowed for a development server only. A request carries the store token, so it must never cross a
/// network in clear text.
pub trait Transport {
    /// Sends `request` as an HTTP `GET`.
    fn get(&self, request: &Request) -> Result<Response, TransportError>;
}

impl<T: Transport + ?Sized> Transport for &T {
    fn get(&self, request: &Request) -> Result<Response, TransportError> {
        (**self).get(request)
    }
}

impl<T: Transport + ?Sized> Transport for std::sync::Arc<T> {
    fn get(&self, request: &Request) -> Result<Response, TransportError> {
        (**self).get(request)
    }
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn get(&self, request: &Request) -> Result<Response, TransportError> {
        (**self).get(request)
    }
}

/// An HTTP `GET`: an absolute path and its headers. `Debug` hides every sensitive header's value (`THR-50`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    path: String,
    headers: Vec<Header>,
}

impl Request {
    pub(super) fn new(path: String, headers: Vec<Header>) -> Self {
        Request { path, headers }
    }

    /// The absolute path, from `/v1/` on.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The headers to send, in order.
    pub fn headers(&self) -> &[Header] {
        &self.headers
    }
}

/// One request header.
#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    name: &'static str,
    value: String,
    sensitive: bool,
}

impl Header {
    pub(super) fn plain(name: &'static str, value: String) -> Self {
        Header {
            name,
            value,
            sensitive: false,
        }
    }

    pub(super) fn sensitive(name: &'static str, value: String) -> Self {
        Header {
            name,
            value,
            sensitive: true,
        }
    }

    /// The header's name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The header's value. A transport sends it as it is.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Whether the value is a credential, which a transport must never log.
    pub fn is_sensitive(&self) -> bool {
        self.sensitive
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value: &dyn fmt::Debug = if self.sensitive {
            &"<redacted>"
        } else {
            &self.value
        };
        f.debug_struct("Header")
            .field("name", &self.name)
            .field("value", value)
            .finish()
    }
}

/// The store's answer. `Debug` prints the status and the body's length, never the body, which may hold a secret
/// (`THR-50`).
#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    /// The HTTP status.
    pub status: u16,
    /// The body as received.
    pub body: Vec<u8>,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// A request that didn't reach the store, or whose answer didn't come back. Its reason is the transport's own
/// fixed text, never the request or an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    reason: &'static str,
}

impl TransportError {
    /// A failure with a fixed reason.
    pub fn new(reason: &'static str) -> Self {
        TransportError { reason }
    }

    /// The reason.
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason)
    }
}

impl core::error::Error for TransportError {}
