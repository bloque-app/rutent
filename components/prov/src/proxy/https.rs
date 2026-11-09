//! The credential proxy's production transport (`T-014-02`): HTTPS with the store's certificate verified, and plain
//! HTTP only to a loopback address, for a development server.
//!
//! Every request carries the store token, so it must never cross a network in clear text (`DEC-9`, ADR-0004). TLS
//! is rustls, a pure-Rust implementation, over ring's primitives; no OpenSSL. It trusts no built-in roots: the
//! deployment names the certificate authorities its store's certificate chains to, since the store is the
//! operator's own.

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use super::settings::{RoleSettings, StoreAddress, StoreScheme};
use super::transport::{Request, Response, Transport, TransportError};

/// The default for each of connecting, sending and reading.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// The default largest answer read, head and body: far above a KV version 2 read of one secret.
pub const DEFAULT_MAX_ANSWER: usize = 1 << 20;

/// The production [`Transport`]: one HTTP/1.0 `GET` per connection, over TLS to the store's address, or plain to a
/// loopback address for development. It never follows a redirect: the redirect's status comes back to the
/// resolver.
#[derive(Clone)]
pub struct HttpsTransport {
    host: String,
    port: u16,
    tls: Option<(ServerName<'static>, Arc<ClientConfig>)>,
    timeout: Duration,
    max_answer: usize,
}

impl core::fmt::Debug for HttpsTransport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HttpsTransport")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls.is_some())
            .field("timeout", &self.timeout)
            .field("max_answer", &self.max_answer)
            .finish()
    }
}

impl HttpsTransport {
    /// A transport to `address`. For HTTPS, `ca_pem` holds the PEM certificates of the authorities the store's
    /// certificate must chain to, and is required. Plain HTTP is allowed only to an address that is literally a
    /// loopback one (`127.0.0.0/8` or `::1`), never to a host name, so it is refused here, before anything is
    /// resolved or sent.
    pub fn new(address: &StoreAddress, ca_pem: Option<&[u8]>) -> Result<Self, TransportError> {
        let tls = match address.scheme {
            StoreScheme::Http => {
                let loopback = address
                    .host
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback());
                if !loopback {
                    return Err(TransportError::new(
                        "plain HTTP is allowed only to a loopback address",
                    ));
                }
                None
            }
            StoreScheme::Https => {
                let pem = ca_pem.ok_or(TransportError::new(
                    "HTTPS needs the deployment's CA certificates",
                ))?;
                let mut roots = RootCertStore::empty();
                for certificate in CertificateDer::pem_slice_iter(pem) {
                    let certificate = certificate
                        .map_err(|_| TransportError::new("the CA certificates are not PEM"))?;
                    roots
                        .add(certificate)
                        .map_err(|_| TransportError::new("a CA certificate is not usable"))?;
                }
                if roots.is_empty() {
                    return Err(TransportError::new("the CA file holds no certificate"));
                }
                let config = ClientConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .map_err(|_| TransportError::new("no TLS version is available"))?
                .with_root_certificates(roots)
                .with_no_client_auth();
                let name = ServerName::try_from(address.host.as_str())
                    .map_err(|_| TransportError::new("the store's host is not a valid TLS name"))?
                    .to_owned();
                Some((name, Arc::new(config)))
            }
        };
        if address.port == 0 {
            return Err(TransportError::new("the store's port is 0"));
        }
        Ok(HttpsTransport {
            host: address.host.clone(),
            port: address.port,
            tls,
            timeout: DEFAULT_TIMEOUT,
            max_answer: DEFAULT_MAX_ANSWER,
        })
    }

    /// The transport to `role`'s store, as the deployment's settings describe it: its address, and for HTTPS the
    /// CA certificates read from its `ca_file`, which HTTPS requires.
    pub fn for_role(role: &RoleSettings) -> Result<Self, TransportError> {
        let ca = match &role.ca_file {
            Some(path) => Some(
                std::fs::read(path)
                    .map_err(|_| TransportError::new("the role's CA file could not be read"))?,
            ),
            None => None,
        };
        HttpsTransport::new(&role.store, ca.as_deref())
    }

    /// The same transport, with `timeout` for each of connecting, sending and reading.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The same transport, refusing an answer, head and body, longer than `bytes`.
    pub fn with_max_body(mut self, bytes: usize) -> Self {
        self.max_answer = bytes;
        self
    }

    fn connect(&self) -> Result<TcpStream, TransportError> {
        let addresses: Vec<SocketAddr> = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|_| TransportError::new("the store's host did not resolve"))?
            .collect();
        if self.tls.is_none() && !addresses.iter().all(|a| a.ip().is_loopback()) {
            return Err(TransportError::new(
                "plain HTTP is allowed only to a loopback address",
            ));
        }
        let stream = addresses
            .iter()
            .find_map(|a| TcpStream::connect_timeout(a, self.timeout).ok())
            .ok_or(TransportError::new("could not connect to the store"))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .and_then(|()| stream.set_write_timeout(Some(self.timeout)))
            .map_err(|_| TransportError::new("could not set the connection's timeouts"))?;
        Ok(stream)
    }

    /// The request's bytes. A path or header that could break the request's framing is refused.
    fn head(&self, request: &Request) -> Result<Vec<u8>, TransportError> {
        let clean = |text: &str| !text.bytes().any(|b| b.is_ascii_control() || b == b' ');
        if !request.path().starts_with('/') || !clean(request.path()) {
            return Err(TransportError::new("the request path is not sendable"));
        }
        let host = match self.host.parse::<IpAddr>() {
            Ok(IpAddr::V6(ip)) => format!("[{ip}]:{}", self.port),
            _ => format!("{}:{}", self.host, self.port),
        };
        let mut head = format!(
            "GET {} HTTP/1.0\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\n",
            request.path()
        );
        for header in request.headers() {
            let value_clean = !header
                .value()
                .bytes()
                .any(|b| b.is_ascii_control() && b != b'\t');
            if !clean(header.name()) || header.name().contains(':') || !value_clean {
                return Err(TransportError::new("a request header is not sendable"));
            }
            head.push_str(header.name());
            head.push_str(": ");
            head.push_str(header.value());
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        Ok(head.into_bytes())
    }

    /// Writes `head` and reads the whole answer, to the connection's end or the declared length.
    fn exchange<S: Read + Write>(
        &self,
        stream: &mut S,
        head: &[u8],
    ) -> Result<Response, TransportError> {
        stream.write_all(head).map_err(io_error)?;
        stream.flush().map_err(io_error)?;
        let mut answer = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    answer.extend_from_slice(&chunk[..n]);
                    if answer.len() > self.max_answer {
                        return Err(TransportError::new("the store's answer is too large"));
                    }
                    if complete(&answer) {
                        break;
                    }
                }
                // A peer that closes without TLS's close_notify: the answer stands only if it is complete.
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(io_error(e)),
            }
        }
        parse(&answer)
    }
}

impl Transport for HttpsTransport {
    fn get(&self, request: &Request) -> Result<Response, TransportError> {
        let head = self.head(request)?;
        let tcp = self.connect()?;
        match &self.tls {
            None => {
                let mut tcp = tcp;
                self.exchange(&mut tcp, &head)
            }
            Some((name, config)) => {
                let connection = ClientConnection::new(Arc::clone(config), name.clone())
                    .map_err(|_| TransportError::new("could not start TLS"))?;
                let mut stream = StreamOwned::new(connection, tcp);
                // Finish the handshake, and so the certificate check, before a byte of the request is written.
                while stream.conn.is_handshaking() {
                    stream.conn.complete_io(&mut stream.sock).map_err(|_| {
                        TransportError::new("the TLS handshake or certificate check failed")
                    })?;
                }
                self.exchange(&mut stream, &head)
            }
        }
    }
}

fn io_error(error: std::io::Error) -> TransportError {
    match error.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => TransportError::new("the store timed out"),
        ErrorKind::InvalidData => {
            TransportError::new("the TLS handshake or certificate check failed")
        }
        _ => TransportError::new("the connection to the store failed"),
    }
}

/// Where the head ends, and the body's declared length if any.
fn split(answer: &[u8]) -> Option<(usize, Option<usize>)> {
    let end = answer.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let head = std::str::from_utf8(&answer[..end]).ok()?;
    let length = head.split("\r\n").skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())?
    });
    Some((end, length))
}

/// Whether `answer` holds its whole body, by its declared length.
fn complete(answer: &[u8]) -> bool {
    matches!(split(answer), Some((end, Some(length))) if answer.len() >= end + length)
}

/// The status and body of an HTTP/1.x answer. A chunked body is refused: an HTTP/1.0 request gets none.
fn parse(answer: &[u8]) -> Result<Response, TransportError> {
    let malformed = || TransportError::new("the store's answer is not HTTP");
    let (end, length) = split(answer).ok_or_else(malformed)?;
    let head = std::str::from_utf8(&answer[..end]).map_err(|_| malformed())?;
    let status_line = head.split("\r\n").next().ok_or_else(malformed)?;
    let mut parts = status_line.splitn(3, ' ');
    let (version, status) = (parts.next(), parts.next());
    if !version.is_some_and(|v| v == "HTTP/1.0" || v == "HTTP/1.1") {
        return Err(malformed());
    }
    let status = status
        .filter(|s| s.len() == 3)
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|s| (100..=599).contains(s))
        .ok_or_else(malformed)?;
    let chunked = head.split("\r\n").skip(1).any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("transfer-encoding")
                && !value.trim().eq_ignore_ascii_case("identity")
        })
    });
    if chunked {
        return Err(TransportError::new(
            "the store's answer has a transfer encoding",
        ));
    }
    let body = &answer[end..];
    let body = match length {
        Some(length) if body.len() < length => {
            return Err(TransportError::new("the store's answer was cut short"));
        }
        Some(length) => &body[..length],
        None => body,
    };
    Ok(Response {
        status,
        body: body.to_vec(),
    })
}
