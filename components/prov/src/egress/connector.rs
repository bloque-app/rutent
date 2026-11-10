//! The connection surface the egress layer speaks through: a [`Connector`] dials one checked address and
//! verifies the peer as one name. It sits behind a trait so the layer is tested without a network, and so a
//! TLS client (`T-014-02`'s, over `rustls`) satisfies it without the layer knowing which one.

use core::fmt;
use std::net::SocketAddr;
use std::time::Instant;

use rutent_contracts::module_interface::{Host, Scheme};

/// Where a connector connects, and as whom it verifies the peer.
///
/// The egress layer resolves the name, checks the answer and pins the address here. The connector connects to
/// [`to`](Self::to) and nowhere else: it never resolves [`name`](Self::name), so the address it connects to is
/// the one the layer checked (DNS rebinding, `B-17`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dial<'a> {
    /// The scheme, always over TLS: the contract's [`Scheme`] has no plain variant.
    pub scheme: Scheme,
    /// The declared host name: the name the connector sends as SNI and in the `Host` header, and the name the
    /// peer's certificate must be valid for.
    pub name: &'a Host,
    /// The checked address and the declared port.
    pub to: SocketAddr,
    /// When the operation the call serves must end, if it has a deadline: every step of the exchange, the
    /// connection, the handshake and each read and write, is bounded by what is left until then (`REQ-MDL-11`).
    pub due: Option<Instant>,
}

/// Makes one exchange over one connection: dials an address the egress layer checked, verifies the peer, sends
/// one request and reads one answer.
///
/// An implementation must:
///
/// - connect to [`Dial::to`] only. It never resolves [`Dial::name`], reads no proxy setting from the
///   environment (`HTTPS_PROXY` and the like) and never sends through an intermediary;
/// - speak TLS, version 1.2 or later, with SNI [`Dial::name`], and verify the peer's certificate chain against
///   its root store and for [`Dial::name`]. A peer that offers no certificate, one for another name, an
///   untrusted or expired chain, or a version below the minimum fails as [`ConnectError::NotVerified`], before
///   any byte of the request is sent. It never falls back to plain text or a lower version, and never retries
///   on another address (`B-18`);
/// - send no TLS 1.3 early data (0-RTT): early data can be replayed, and is sent before the handshake has
///   verified the peer;
/// - not reuse a connection or a TLS session made for another name or address;
/// - send the `Host` header itself, from [`Dial::name`] (with the port when it isn't the scheme's default). A
///   [`Request`] never carries one;
/// - never follow a redirect: a `3xx` comes back as a [`Response`] with its `Location`, and the egress layer
///   decides whether to follow it;
/// - bound the time and the size of what it reads, and give [`ConnectError::NoAnswer`] when a bound is reached;
/// - give up at [`Dial::due`], whatever each step's own bound: as [`ConnectError::Unreachable`] before the
///   handshake has finished, since nothing of the request was sent, and as [`ConnectError::NoAnswer`] after.
///
/// One exchange is one HTTP request and one answer. A WebSocket session is not, so `wss` is not supported
/// through a connector: a `wss` dial is checked like any other (destination, address, TLS), and then the
/// connector refuses it before connecting, as [`ConnectError::Unreachable`]. No outside operation of `M2` uses
/// `wss`; serving it needs a session interface of its own.
pub trait Connector {
    /// Sends `request` over a connection to `dial`, and returns the answer.
    fn exchange(&self, dial: &Dial<'_>, request: &Request) -> Result<Response, ConnectError>;
}

impl<T: Connector + ?Sized> Connector for &T {
    fn exchange(&self, dial: &Dial<'_>, request: &Request) -> Result<Response, ConnectError> {
        (**self).exchange(dial, request)
    }
}

impl<T: Connector + ?Sized> Connector for std::sync::Arc<T> {
    fn exchange(&self, dial: &Dial<'_>, request: &Request) -> Result<Response, ConnectError> {
        (**self).exchange(dial, request)
    }
}

impl<T: Connector + ?Sized> Connector for Box<T> {
    fn exchange(&self, dial: &Dial<'_>, request: &Request) -> Result<Response, ConnectError> {
        (**self).exchange(dial, request)
    }
}

/// Why an exchange gave no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectError {
    /// The peer was not verified as the declared name: no TLS, no certificate, a certificate for another name,
    /// an untrusted chain or a version below the minimum. Nothing of the request was sent.
    NotVerified,
    /// No connection was established to the checked address. Nothing of the request was sent.
    Unreachable,
    /// A verified connection was made, and no complete answer came: the request may have reached the peer.
    NoAnswer,
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ConnectError::NotVerified => "the peer was not verified as the declared host",
            ConnectError::Unreachable => "no connection was established",
            ConnectError::NoAnswer => "no complete answer came",
        })
    }
}

impl core::error::Error for ConnectError {}

/// An HTTP method an outside part may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `PATCH`.
    Patch,
    /// `DELETE`.
    Delete,
}

impl Method {
    /// The method's name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// Headers a request may not set: the connector frames the message and names the host itself, so a request
/// that set one could reach another virtual host behind a checked address, or smuggle a second message.
const FRAMING: &[&str] = &[
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "te",
    "upgrade",
    "trailer",
    "keep-alive",
    "proxy-connection",
];

/// Headers that carry a credential whatever flag they were built with, matched in any case. A redirect to
/// another endpoint drops them.
pub const CREDENTIALS: &[&str] = &["authorization", "proxy-authorization", "cookie"];

/// One request header. `Debug` hides a sensitive header's value (`THR-50`).
#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    name: String,
    value: String,
    sensitive: bool,
}

impl Header {
    /// A header whose value may be logged.
    pub fn plain(name: impl Into<String>, value: impl Into<String>) -> Self {
        Header {
            name: name.into(),
            value: value.into(),
            sensitive: false,
        }
    }

    /// A header whose value is a credential, which nothing logs.
    pub fn sensitive(name: impl Into<String>, value: impl Into<String>) -> Self {
        Header {
            name: name.into(),
            value: value.into(),
            sensitive: true,
        }
    }

    /// The header's name, as given.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The header's value. A connector sends it as it is.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Whether the value is a credential, which a connector must never log.
    pub fn is_sensitive(&self) -> bool {
        self.sensitive
    }

    /// Whether the header carries a credential: built as sensitive, or named in [`CREDENTIALS`].
    fn is_credential(&self) -> bool {
        self.sensitive
            || CREDENTIALS
                .iter()
                .any(|c| self.name.eq_ignore_ascii_case(c))
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

/// Why a [`Request`] wasn't built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidRequest {
    /// The target isn't in origin form: one `/`-led path with an optional query, of visible ASCII, with no
    /// fragment. An absolute-form target (`https://other.example/...`) or an authority would name a host of its
    /// own.
    Target,
    /// A header's name isn't an HTTP token, or its value holds a control character, so it could end the header
    /// block early.
    Header,
    /// A header the connector sets itself: `Host`, or one that frames the message.
    Framing,
}

impl fmt::Display for InvalidRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InvalidRequest::Target => "the target is not an origin-form path",
            InvalidRequest::Header => "a header is malformed",
            InvalidRequest::Framing => "a header names the host or frames the message",
        })
    }
}

impl core::error::Error for InvalidRequest {}

/// One request an outside part sends: a method, an origin-form target, headers and a body. It names no host: the
/// egress layer gives the connector the declared one. `Debug` hides the body and every sensitive header's value.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    method: Method,
    target: String,
    headers: Vec<Header>,
    body: Vec<u8>,
}

impl Request {
    /// A request, checked: its target in origin form, its headers well formed and none of them one the connector
    /// sets.
    pub fn new(
        method: Method,
        target: impl Into<String>,
        headers: Vec<Header>,
        body: Vec<u8>,
    ) -> Result<Self, InvalidRequest> {
        let target = target.into();
        if !origin_form(&target) {
            return Err(InvalidRequest::Target);
        }
        for header in &headers {
            if !token(&header.name)
                || header
                    .value
                    .bytes()
                    .any(|b| b.is_ascii_control() && b != b'\t')
            {
                return Err(InvalidRequest::Header);
            }
            if FRAMING.iter().any(|f| header.name.eq_ignore_ascii_case(f)) {
                return Err(InvalidRequest::Framing);
            }
        }
        Ok(Request {
            method,
            target,
            headers,
            body,
        })
    }

    /// The method.
    pub fn method(&self) -> Method {
        self.method
    }

    /// The origin-form target: the path and its query.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The headers, in order.
    pub fn headers(&self) -> &[Header] {
        &self.headers
    }

    /// The body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The same request at another target, as a redirect asks: `307` and `308` keep the method and the body,
    /// the others turn it into a `GET` with none. Unless the redirect stays on the same scheme, host and port,
    /// every credential header is dropped ([`CREDENTIALS`] by name, in any case, and every sensitive one), so a
    /// credential meant for one destination never reaches another (the curl CVE-2018-1000007 class); other
    /// headers follow. Once dropped, a credential is gone for the rest of the call, a redirect back included.
    pub(super) fn redirected(&self, status: u16, target: String, same_endpoint: bool) -> Request {
        let keep = matches!(status, 307 | 308);
        Request {
            method: if keep { self.method } else { Method::Get },
            target,
            headers: self
                .headers
                .iter()
                .filter(|h| same_endpoint || !h.is_credential())
                .cloned()
                .collect(),
            body: if keep { self.body.clone() } else { Vec::new() },
        }
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("target", &self.target)
            .field("headers", &self.headers)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Whether `target` is an origin-form request target: `/`, then visible ASCII with no `#`, and not `//`, which a
/// peer could read as an authority.
pub(super) fn origin_form(target: &str) -> bool {
    target.starts_with('/')
        && !target.starts_with("//")
        && target.bytes().all(|b| b.is_ascii_graphic() && b != b'#')
}

/// Whether `name` is an HTTP token (RFC 9110 §5.6.2).
fn token(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// A peer's answer. `Debug` prints the status and the body's length, never the body or a header's value, which
/// may hold a secret (`THR-50`).
#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    /// The HTTP status.
    pub status: u16,
    /// The headers as received, names in any case.
    pub headers: Vec<(String, String)>,
    /// The body as received.
    pub body: Vec<u8>,
}

impl Response {
    /// The value of the first header named `name`, in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers.len())
            .field("body_len", &self.body.len())
            .finish()
    }
}
