//! The production [`Connector`] (`T-014-05`): rustls over `T-014-02`'s client configuration and handshake, so the
//! credential proxy's transport and the egress layer verify peers one way.
//!
//! It keeps every clause of the [`Connector`] contract:
//!
//! - **It dials an address.** The connection goes to [`Dial::to`], the address the egress layer checked, through a
//!   [`Dialer`]; nothing here resolves [`Dial::name`] (`B-17`). It reads no proxy setting from the environment.
//! - **It verifies as a name.** SNI is [`Dial::name`], and the peer's chain must lead to the deployment's roots and be
//!   valid for that name. The handshake finishes before a byte of the request is written, and a failed one, a
//!   timeout included, is [`ConnectError::NotVerified`], never retried and never downgraded (`B-18`). Only the
//!   roots in the deployment's PEM are trusted: no built-in root store enters the build.
//! - **Nothing is pooled.** One connection per exchange, closed after it, and no TLS session resumed: nothing made
//!   for one name or address serves another. No TLS 1.3 early data is sent.
//! - **Redirects come back.** A `3xx` is returned as a [`Response`] with its `Location`; the egress layer decides.
//!
//! It speaks HTTP/1.0 with `Connection: close`, as the proxy's transport does, so no answer is chunked, and an
//! answer with a transfer encoding is refused as [`ConnectError::NoAnswer`].

use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use rutent_contracts::module_interface::Scheme;

use super::connector::{ConnectError, Connector, Dial, Request, Response};
use crate::proxy::{AnswerError, client_config, exchange, handshake};

/// The default for each of connecting, the handshake, sending and reading.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// The default largest answer read, head and body.
pub const DEFAULT_MAX_ANSWER: usize = 1 << 20;

/// Opens the TCP connection to one checked address. The production one, [`DirectDialer`], connects to that
/// address and nowhere else; a test's may map it to a loopback listener standing for it.
pub trait Dialer {
    /// A TCP connection to `to`, within `timeout`.
    fn connect(&self, to: SocketAddr, timeout: Duration) -> io::Result<TcpStream>;
}

impl<T: Dialer + ?Sized> Dialer for &T {
    fn connect(&self, to: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        (**self).connect(to, timeout)
    }
}

impl<T: Dialer + ?Sized> Dialer for Arc<T> {
    fn connect(&self, to: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        (**self).connect(to, timeout)
    }
}

/// Connects to the address given, with no intermediary.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirectDialer;

impl Dialer for DirectDialer {
    fn connect(&self, to: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        TcpStream::connect_timeout(&to, timeout)
    }
}

/// Why a [`TlsConnector`] wasn't built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidRoots(&'static str);

impl core::fmt::Display for InvalidRoots {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

impl core::error::Error for InvalidRoots {}

/// The production [`Connector`]: TLS through rustls, verified against the deployment's roots, one connection per
/// exchange.
#[derive(Clone)]
pub struct TlsConnector<D = DirectDialer> {
    config: Arc<ClientConfig>,
    dialer: D,
    timeout: Duration,
    max_answer: usize,
}

impl<D> core::fmt::Debug for TlsConnector<D> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TlsConnector")
            .field("timeout", &self.timeout)
            .field("max_answer", &self.max_answer)
            .finish()
    }
}

impl TlsConnector<DirectDialer> {
    /// A connector trusting only the certificate authorities in `roots_pem`, the deployment's PEM bundle.
    pub fn new(roots_pem: &[u8]) -> Result<Self, InvalidRoots> {
        TlsConnector::with_dialer(roots_pem, DirectDialer)
    }
}

impl<D: Dialer> TlsConnector<D> {
    /// [`TlsConnector::new`], opening its connections through `dialer`.
    pub fn with_dialer(roots_pem: &[u8], dialer: D) -> Result<Self, InvalidRoots> {
        Ok(TlsConnector {
            config: client_config(roots_pem).map_err(InvalidRoots)?,
            dialer,
            timeout: DEFAULT_TIMEOUT,
            max_answer: DEFAULT_MAX_ANSWER,
        })
    }

    /// The same connector, with `timeout` for each of connecting, the handshake, sending and reading.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The same connector, refusing an answer, head and body, longer than `bytes`.
    pub fn with_max_answer(mut self, bytes: usize) -> Self {
        self.max_answer = bytes;
        self
    }
}

impl<D: Dialer> Connector for TlsConnector<D> {
    fn exchange(&self, dial: &Dial<'_>, request: &Request) -> Result<Response, ConnectError> {
        // One exchange is one request and one answer; a WebSocket session isn't, so `wss` is refused before any
        // connection (the `Connector` contract).
        if dial.scheme != Scheme::Https {
            return Err(ConnectError::Unreachable);
        }
        let name = ServerName::try_from(dial.name.as_str().to_owned())
            .map_err(|_| ConnectError::NotVerified)?;
        let tcp = self
            .dialer
            .connect(dial.to, self.timeout)
            .map_err(|_| ConnectError::Unreachable)?;
        // The handshake has the same bound: a peer that stalls it is not verified, and receives nothing.
        tcp.set_read_timeout(Some(self.timeout))
            .and_then(|()| tcp.set_write_timeout(Some(self.timeout)))
            .map_err(|_| ConnectError::Unreachable)?;
        let mut stream =
            handshake(&self.config, name, tcp).map_err(|()| ConnectError::NotVerified)?;
        let bytes = request_bytes(dial, request);
        let answer = exchange(&mut stream, &bytes, self.max_answer).map_err(|e| match e {
            AnswerError::TooLarge
            | AnswerError::Io(_)
            | AnswerError::Malformed
            | AnswerError::Encoded
            | AnswerError::CutShort => ConnectError::NoAnswer,
        })?;
        Ok(Response {
            status: answer.status,
            headers: answer.headers,
            body: answer.body,
        })
    }
}

/// The bytes of `request` as sent to `dial`: the request line, `Host` from the declared name (with the port when
/// it isn't 443), `Content-Length` from the body, `Connection: close`, the request's own headers, then the body.
/// [`Request::new`] has already refused a target or header that could break this framing.
pub(crate) fn request_bytes(dial: &Dial<'_>, request: &Request) -> Vec<u8> {
    let port = dial.to.port();
    let name = dial.name.as_str();
    let host = match (name.parse::<IpAddr>(), port) {
        (Ok(IpAddr::V6(ip)), 443) => format!("[{ip}]"),
        (Ok(IpAddr::V6(ip)), port) => format!("[{ip}]:{port}"),
        (_, 443) => name.to_owned(),
        (_, port) => format!("{name}:{port}"),
    };
    let mut head = format!(
        "{} {} HTTP/1.0\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n",
        request.method().as_str(),
        request.target(),
        request.body().len()
    );
    for header in request.headers() {
        head.push_str(header.name());
        head.push_str(": ");
        head.push_str(header.value());
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(request.body());
    bytes
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use rutent_contracts::module_interface::{Host, Scheme};

    use super::request_bytes;
    use crate::egress::{Dial, Header, Method, Request};

    /// A seeded xorshift64*, so a failure names the seed that replays it (`DEC-18`).
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
        }

        fn text(&mut self, alphabet: &[u8], least: u64, most: u64) -> String {
            let length = least + self.below(most - least + 1);
            (0..length)
                .map(|_| alphabet[self.below(alphabet.len() as u64) as usize] as char)
                .collect()
        }
    }

    const TOKEN: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.!#$%&'*+^`|~";
    const VALUE: &[u8] = b"abcXYZ019 \t:;/?=&%\"'<>[]{}()@,";
    const PATH: &[u8] = b"abcXYZ019-._~/?=&%:@!$'()*+,;";

    /// The connector writes one request whose framing is its own: the request line, `Host` from the declared
    /// name and port, `Content-Length` from the body, `Connection: close`, then the request's headers in order,
    /// an empty line and the body, whole. Nothing in a request a connector accepts adds a line, names another
    /// host or moves the body's end.
    // verifies: REQ-MDL-7, THR-79
    #[test]
    fn the_connector_frames_every_request_itself_with_the_declared_host() {
        let methods = [
            Method::Get,
            Method::Post,
            Method::Put,
            Method::Patch,
            Method::Delete,
        ];
        let mut framed = 0;
        for seed in 0..400u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let method = methods[rng.below(5) as usize];
            let target = format!("/{}", rng.text(PATH, 0, 40));
            let headers: Vec<Header> = (0..rng.below(4))
                .map(|_| {
                    let name = format!("x-{}", rng.text(TOKEN, 1, 12));
                    let value = rng.text(VALUE, 0, 30);
                    if rng.below(2) == 0 {
                        Header::plain(name, value)
                    } else {
                        Header::sensitive(name, value)
                    }
                })
                .collect();
            let body: Vec<u8> = (0..rng.below(300)).map(|_| rng.below(256) as u8).collect();
            let Ok(request) = Request::new(method, target.clone(), headers.clone(), body.clone())
            else {
                continue;
            };
            let host =
                Host::new(format!("{}.example", rng.text(b"abcdefghij", 1, 10))).expect("a host");
            let port = if rng.below(2) == 0 { 443 } else { 8443 };
            let dial = Dial {
                scheme: Scheme::Https,
                name: &host,
                to: SocketAddr::new([203, 0, 113, 7].into(), port),
            };
            let bytes = request_bytes(&dial, &request);
            let end = bytes
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .expect("a head");
            let head = std::str::from_utf8(&bytes[..end]).expect("a text head");
            let lines: Vec<&str> = head.split("\r\n").collect();
            let expected_host = if port == 443 {
                host.as_str().to_owned()
            } else {
                format!("{}:{port}", host.as_str())
            };
            assert_eq!(lines.len(), 4 + headers.len(), "seed {seed}: {head:?}");
            assert_eq!(
                lines[0],
                format!("{} {target} HTTP/1.0", method.as_str()),
                "seed {seed}"
            );
            assert_eq!(lines[1], format!("Host: {expected_host}"), "seed {seed}");
            assert_eq!(
                lines[2],
                format!("Content-Length: {}", body.len()),
                "seed {seed}"
            );
            assert_eq!(lines[3], "Connection: close", "seed {seed}");
            for (line, header) in lines[4..].iter().zip(&headers) {
                assert_eq!(
                    *line,
                    format!("{}: {}", header.name(), header.value()),
                    "seed {seed}"
                );
            }
            assert_eq!(&bytes[end + 4..], body.as_slice(), "seed {seed}");
            framed += 1;
        }
        assert!(framed > 300, "{framed}");
    }
}
