//! The link to Core (`T-015-04` part 1, `DEC-28`): the `reader` contract served over mTLS under the operator's CA.
//!
//! - **TLS** is `rustls` with its `ring` provider, TLS 1.2 or 1.3 and nothing lower, no session resumption and no
//!   early data; `aws-lc-rs` is not in the build.
//! - **Core's certificate.** The host takes a connection only from a client certificate the operator's CA issued for
//!   Core: its chain must lead to the operator's CA, the one trust anchor (no built-in root store), and it must be
//!   valid for the subject name the host's configuration names for Core. A connection with no certificate, one the CA
//!   didn't issue, or one for another name fails its handshake, and nothing is read from it.
//! - **The host's certificate** is the one the operator's CA issued under the subject name the listing names. Core
//!   holds it to that CA and that name, never to a pinned leaf, so the leaf may be rotated under the same CA and name
//!   with no listing change; the host reads its chain and key at start.
//! - **The frames.** Each question is one frame (its length as four little-endian bytes, then its JSON, at most
//!   [`MAX_QUESTION`] bytes), and each answer one frame back: the reader's signed answer, or an empty frame where the
//!   host has nothing to sign for, which Core reads as unreadable.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use rustls::client::danger::HandshakeSignatureValid;
use rustls::client::verify_server_name;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::server::{NoServerSessionStorage, ParsedCertificate, WebPkiClientVerifier};
use rustls::{
    CertificateError, DigitallySignedStruct, DistinguishedName, RootCertStore, ServerConfig,
    ServerConnection, SignatureScheme, StreamOwned,
};

use crate::host::Host;
use crate::wire::{read_frame, write_frame};

/// The largest question the host reads, in bytes.
pub const MAX_QUESTION: usize = 64 * 1024;

/// The most connections from Core the host serves at once; one more is closed unread.
pub const MAX_CONNECTIONS: usize = 64;

/// How long a connection may sit idle, or a handshake take, before the host closes it.
pub const IDLE: Duration = Duration::from_secs(60);

/// Why the link's TLS wasn't built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkError(pub &'static str);

impl core::fmt::Display for LinkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

impl core::error::Error for LinkError {}

/// The link's TLS: the operator's CA as the one anchor for Core's certificate, the subject name Core's must carry, and
/// the host's own chain and key.
#[derive(Clone)]
pub struct LinkTls {
    config: Arc<ServerConfig>,
}

impl core::fmt::Debug for LinkTls {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("LinkTls")
    }
}

impl LinkTls {
    /// The link's TLS from PEM: `operator_ca` (the operator's CA certificate, the only anchor), `chain` (the host's
    /// server certificate, then any intermediates), `key` (its private key) and `core_name`, the subject name the
    /// operator's CA issues Core's client certificate under.
    pub fn new(
        operator_ca: &[u8],
        chain: &[u8],
        key: &[u8],
        core_name: &str,
    ) -> Result<Self, LinkError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut roots = RootCertStore::empty();
        for certificate in CertificateDer::pem_slice_iter(operator_ca) {
            let certificate = certificate.map_err(|_| LinkError("the operator's CA is not PEM"))?;
            roots
                .add(certificate)
                .map_err(|_| LinkError("the operator's CA certificate is not usable"))?;
        }
        if roots.is_empty() {
            return Err(LinkError("the operator's CA file holds no certificate"));
        }
        let core = ServerName::try_from(core_name.to_owned())
            .map_err(|_| LinkError("Core's subject name is not a DNS name"))?;
        let inner = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
            .build()
            .map_err(|_| LinkError("no verifier for Core's certificate"))?;
        let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(chain)
            .collect::<Result<_, _>>()
            .map_err(|_| LinkError("the host's certificate chain is not PEM"))?;
        if chain.is_empty() {
            return Err(LinkError(
                "the host's certificate file holds no certificate",
            ));
        }
        let key = PrivateKeyDer::from_pem_slice(key)
            .map_err(|_| LinkError("the host's key is not PEM"))?;
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|_| LinkError("no TLS version is available"))?
            .with_client_cert_verifier(Arc::new(CoreOnly { inner, core }))
            .with_single_cert(chain, key)
            .map_err(|_| LinkError("the host's certificate and key don't make a pair"))?;
        config.session_storage = Arc::new(NoServerSessionStorage {});
        config.max_early_data_size = 0;
        config.send_tls13_tickets = 0;
        Ok(LinkTls {
            config: Arc::new(config),
        })
    }
}

/// Core's certificate, as the host takes it: chained to the operator's CA by the web PKI's rules, and valid for the
/// subject name configured for Core. Transport identity is Core's alone; it isn't authorisation of anything but
/// asking.
#[derive(Debug)]
struct CoreOnly {
    inner: Arc<dyn ClientCertVerifier>,
    core: ServerName<'static>,
}

impl ClientCertVerifier for CoreOnly {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.inner.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        let verified = self
            .inner
            .verify_client_cert(end_entity, intermediates, now)?;
        let parsed = ParsedCertificate::try_from(end_entity)?;
        verify_server_name(&parsed, &self.core)
            .map_err(|_| rustls::Error::InvalidCertificate(CertificateError::NotValidForName))?;
        Ok(verified)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// The link, serving on a listener until it is dropped.
pub struct Link {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    accepter: Option<JoinHandle<()>>,
}

impl core::fmt::Debug for Link {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Link")
            .field("address", &self.address)
            .finish()
    }
}

impl Link {
    /// Serves `host` on `listener` under `tls`, each connection on a thread of its own, at most [`MAX_CONNECTIONS`]
    /// at once.
    pub fn serve(listener: TcpListener, tls: LinkTls, host: Arc<Host>) -> io::Result<Link> {
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let open = Arc::new(AtomicUsize::new(0));
        let stopping = stop.clone();
        let accepter = std::thread::Builder::new()
            .name("reader host link".to_owned())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    let Ok(stream) = stream else { continue };
                    if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        open.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let (tls, host, open) = (tls.clone(), host.clone(), open.clone());
                    std::thread::spawn(move || {
                        let _ = serve_connection(stream, &tls, &host);
                        open.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            })?;
        Ok(Link {
            address,
            stop,
            accepter: Some(accepter),
        })
    }

    /// The address it listens on.
    pub fn address(&self) -> SocketAddr {
        self.address
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accepter, which sees the stop and returns.
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_secs(1));
        if let Some(accepter) = self.accepter.take() {
            let _ = accepter.join();
        }
    }
}

/// One connection: the handshake, which holds Core's certificate, then a question and its answer at a time.
fn serve_connection(tcp: TcpStream, tls: &LinkTls, host: &Host) -> io::Result<()> {
    tcp.set_read_timeout(Some(IDLE))?;
    tcp.set_write_timeout(Some(IDLE))?;
    let connection = ServerConnection::new(tls.config.clone()).map_err(io::Error::other)?;
    let mut stream = StreamOwned::new(connection, tcp);
    while stream.conn.is_handshaking() {
        stream.conn.complete_io(&mut stream.sock)?;
    }
    loop {
        let Some(question) = read_frame(&mut stream, MAX_QUESTION)? else {
            return Ok(());
        };
        let answer = host.answer(&question).unwrap_or_default();
        write_frame(&mut stream, &answer)?;
    }
}

/// Reads a frame from any stream: the link's, for a client's own use (Core's adapter, a test).
pub fn read_answer(stream: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    read_frame(stream, 1 << 20)
}

/// Writes a question as a frame to any stream.
pub fn write_question(stream: &mut impl Write, question: &[u8]) -> io::Result<()> {
    write_frame(stream, question)
}
