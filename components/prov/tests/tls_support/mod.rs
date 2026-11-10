//! Real TLS on loopback for the egress connector's tests (`T-014-05`, `B-18`): a certificate authority and leaf
//! certificates generated in the test with `rcgen`, rustls servers on loopback ports, and a dialer that maps each
//! public address a test uses to the loopback server standing for it.
//!
//! The egress layer refuses every special-purpose address, loopback included, so the names in a test resolve to
//! public addresses. [`MappedDialer`] is the one test seam: it opens the TCP connection to the loopback server
//! mapped to the checked address it was given. Everything above TCP is the production connector's: the handshake,
//! SNI, the chain and name check against the generated authority, the request and the answer.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rcgen::{BasicConstraints, Certificate, CertificateParams, IsCa, KeyPair};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use rutent_prov::egress::Dialer;

/// A certificate authority generated for one test, and the leaves it signs.
pub struct Pki {
    ca: Certificate,
    key: KeyPair,
}

impl Pki {
    /// A fresh authority.
    pub fn new() -> Self {
        let key = KeyPair::generate().expect("a key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("parameters");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = params.self_signed(&key).expect("a CA certificate");
        Pki { ca, key }
    }

    /// The authority's certificate, PEM: what a deployment's roots file holds.
    pub fn roots_pem(&self) -> Vec<u8> {
        self.ca.pem().into_bytes()
    }

    /// A server configuration presenting a leaf for `name`, signed by this authority.
    pub fn server_for(&self, name: &str) -> Arc<ServerConfig> {
        let key = KeyPair::generate().expect("a key");
        let leaf = CertificateParams::new(vec![name.to_owned()])
            .expect("parameters")
            .signed_by(&key, &self.ca, &self.key)
            .expect("a leaf");
        let chain = vec![leaf.der().clone(), self.ca.der().clone()];
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        Arc::new(
            builder()
                .with_single_cert(chain, key)
                .expect("a server configuration"),
        )
    }
}

impl Default for Pki {
    fn default() -> Self {
        Self::new()
    }
}

/// A server configuration that offers no certificate at all: its handshake can't prove any name.
pub fn server_with_no_certificate() -> Arc<ServerConfig> {
    Arc::new(builder().with_cert_resolver(Arc::new(NoCertificate)))
}

/// A server configuration presenting a self-signed leaf for `name`, which chains to no authority a test trusts.
pub fn self_signed_for(name: &str) -> Arc<ServerConfig> {
    let key = KeyPair::generate().expect("a key");
    let leaf = CertificateParams::new(vec![name.to_owned()])
        .expect("parameters")
        .self_signed(&key)
        .expect("a self-signed leaf");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    Arc::new(
        builder()
            .with_single_cert(vec![leaf.der().clone()], key)
            .expect("a server configuration"),
    )
}

fn builder() -> rustls::ConfigBuilder<ServerConfig, rustls::server::WantsServerCert> {
    ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("TLS versions")
        .with_no_client_auth()
}

#[derive(Debug)]
struct NoCertificate;

impl ResolvesServerCert for NoCertificate {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        None
    }
}

/// A TLS server on a loopback port. Each connection's handshake is made with its configuration; a request read
/// after a completed handshake is kept, and `answer` written back: a status line's code and reason, and the
/// headers after it.
pub struct TlsServer {
    /// The loopback address it listens on.
    pub address: SocketAddr,
    accepted: Arc<AtomicUsize>,
    handshakes: Arc<AtomicUsize>,
    bytes: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl TlsServer {
    /// Serves with `config`, answering every request `200 OK`.
    pub fn serve(config: Arc<ServerConfig>) -> TlsServer {
        TlsServer::answering(config, "200 OK", &[])
    }

    /// Serves with `config`, answering every request with `status` and `headers`.
    pub fn answering(
        config: Arc<ServerConfig>,
        status: &str,
        headers: &[(&str, &str)],
    ) -> TlsServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("its address");
        let accepted = Arc::new(AtomicUsize::new(0));
        let handshakes = Arc::new(AtomicUsize::new(0));
        let bytes = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut answer = format!("HTTP/1.0 {status}\r\nContent-Length: 2\r\n");
        for (name, value) in headers {
            answer.push_str(&format!("{name}: {value}\r\n"));
        }
        answer.push_str("\r\nok");
        let (count, shook, read, kept) = (
            accepted.clone(),
            handshakes.clone(),
            bytes.clone(),
            requests.clone(),
        );
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                count.fetch_add(1, Ordering::SeqCst);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                let Ok(connection) = ServerConnection::new(config.clone()) else {
                    continue;
                };
                let mut tls = StreamOwned::new(connection, stream);
                let mut handshaken = true;
                while tls.conn.is_handshaking() {
                    if tls.conn.complete_io(&mut tls.sock).is_err() {
                        handshaken = false;
                        break;
                    }
                }
                if !handshaken {
                    continue;
                }
                shook.fetch_add(1, Ordering::SeqCst);
                let Some(request) = read_request(&mut tls, &read) else {
                    continue;
                };
                kept.lock().unwrap().push(request);
                let _ = tls.write_all(answer.as_bytes());
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
        });
        TlsServer {
            address,
            accepted,
            handshakes,
            bytes,
            requests,
        }
    }

    /// How many connections it accepted.
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// How many handshakes completed.
    pub fn handshakes(&self) -> usize {
        self.handshakes.load(Ordering::SeqCst)
    }

    /// How many bytes of application data it read: none, if no handshake completed.
    pub fn bytes_received(&self) -> usize {
        self.bytes.load(Ordering::SeqCst)
    }

    /// The requests it read, head and body, as text.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

/// Reads one request: its head, and a body of its `Content-Length`. Counts every byte read into `read`.
fn read_request<S: Read>(stream: &mut S, read: &AtomicUsize) -> Option<String> {
    let mut request = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        read.fetch_add(n, Ordering::SeqCst);
        request.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&request);
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|l| {
                    let (name, value) = l.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if request.len() >= end + 4 + length {
                break;
            }
        }
    }
    (!request.is_empty()).then(|| String::from_utf8_lossy(&request).into_owned())
}

/// Opens each TCP connection to the loopback server mapped to the address dialled, and records every address it
/// was asked for. An address with no server mapped is refused, as an unreachable one would be.
#[derive(Default, Clone)]
pub struct MappedDialer {
    map: Arc<Mutex<HashMap<SocketAddr, SocketAddr>>>,
    dialled: Arc<Mutex<Vec<SocketAddr>>>,
}

impl MappedDialer {
    /// Maps `public`, port 443, to `server`.
    pub fn with(self, public: &str, server: &TlsServer) -> Self {
        let public = SocketAddr::new(public.parse().expect("an address"), 443);
        self.map.lock().unwrap().insert(public, server.address);
        self
    }

    /// Every address dialled, in order.
    pub fn dialled(&self) -> Vec<SocketAddr> {
        self.dialled.lock().unwrap().clone()
    }
}

impl Dialer for MappedDialer {
    fn connect(&self, to: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        self.dialled.lock().unwrap().push(to);
        let mapped = self.map.lock().unwrap().get(&to).copied();
        match mapped {
            Some(server) => TcpStream::connect_timeout(&server, timeout),
            None => Err(io::Error::from(io::ErrorKind::ConnectionRefused)),
        }
    }
}
