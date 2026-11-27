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

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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

/// The most connections the host serves at once; one more is closed unread.
pub const MAX_CONNECTIONS: usize = 64;

/// The most of them still in their handshake, so not yet shown to be Core's; one more is closed unread, so peers that
/// never authenticate can't take the slots Core's connections need.
pub const MAX_HANDSHAKING: usize = 16;

/// The most of them from one source address, so one peer that never authenticates can't take every handshake slot
/// from Core.
pub const MAX_HANDSHAKING_PER_SOURCE: usize = 4;

/// How long a whole handshake may take, every read and write of it together, before the host closes the connection.
pub const HANDSHAKE: Duration = Duration::from_secs(10);

/// How long an authenticated connection may sit idle before the host closes it.
pub const IDLE: Duration = Duration::from_secs(60);

/// The link's bounds on its connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkLimits {
    /// [`MAX_CONNECTIONS`] by default.
    pub connections: usize,
    /// [`MAX_HANDSHAKING`] by default.
    pub handshaking: usize,
    /// [`MAX_HANDSHAKING_PER_SOURCE`] by default.
    pub handshaking_per_source: usize,
    /// [`HANDSHAKE`] by default.
    pub handshake: Duration,
    /// [`IDLE`] by default.
    pub idle: Duration,
}

impl Default for LinkLimits {
    fn default() -> Self {
        LinkLimits {
            connections: MAX_CONNECTIONS,
            handshaking: MAX_HANDSHAKING,
            handshaking_per_source: MAX_HANDSHAKING_PER_SOURCE,
            handshake: HANDSHAKE,
            idle: IDLE,
        }
    }
}

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
    /// Serves `host` on `listener` under `tls`, each connection on a thread of its own, under the default
    /// [`LinkLimits`].
    pub fn serve(listener: TcpListener, tls: LinkTls, host: Arc<Host>) -> io::Result<Link> {
        Link::serve_with(listener, tls, host, LinkLimits::default())
    }

    /// [`Link::serve`], under `limits`.
    pub fn serve_with(
        listener: TcpListener,
        tls: LinkTls,
        host: Arc<Host>,
        limits: LinkLimits,
    ) -> io::Result<Link> {
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let open = Arc::new(AtomicUsize::new(0));
        let handshaking = Arc::new(AtomicUsize::new(0));
        let sources: Arc<Mutex<HashMap<IpAddr, usize>>> = Arc::default();
        let stopping = stop.clone();
        let accepter = std::thread::Builder::new()
            .name("reader host link".to_owned())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    let Ok(stream) = stream else { continue };
                    if open.fetch_add(1, Ordering::SeqCst) >= limits.connections {
                        open.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let source = stream.peer_addr().map(|a| a.ip()).ok();
                    let admitted = source.is_some_and(|ip| {
                        sources.lock().is_ok_and(|mut by_source| {
                            admit(&mut by_source, &handshaking, &limits, ip)
                        })
                    });
                    let Some(source) = source.filter(|_| admitted) else {
                        open.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    };
                    let (tls, host) = (tls.clone(), host.clone());
                    let (open, handshaking, sources) =
                        (open.clone(), handshaking.clone(), sources.clone());
                    std::thread::spawn(move || {
                        let shaken = Shaking {
                            handshaking: &handshaking,
                            sources: &sources,
                            source,
                        };
                        let _ = serve_connection(stream, &tls, &host, &limits, shaken);
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

/// One connection: the handshake, which holds Core's certificate within one deadline for the whole of it, then a
/// question and its answer at a time.
fn serve_connection(
    tcp: TcpStream,
    tls: &LinkTls,
    host: &Host,
    limits: &LinkLimits,
    slot: Shaking<'_>,
) -> io::Result<()> {
    let shaken = handshake(tcp, tls, limits.handshake);
    slot.release();
    let mut stream = shaken?;
    stream.sock.set_read_timeout(Some(limits.idle))?;
    stream.sock.set_write_timeout(Some(limits.idle))?;
    loop {
        let Some(question) = read_frame(&mut stream, MAX_QUESTION)? else {
            return Ok(());
        };
        let answer = host.answer(&question).unwrap_or_default();
        write_frame(&mut stream, &answer)?;
    }
}

/// Takes a connection from `ip` into a handshake slot, if the link has one free and its source holds fewer than its
/// share: counted in the link's total and in its source's. The map of sources holds an entry only for a source with a
/// handshake under way, so a refused source leaves none behind and the map stays bounded by the slots (#348's L10).
fn admit(
    by_source: &mut HashMap<IpAddr, usize>,
    handshaking: &AtomicUsize,
    limits: &LinkLimits,
    ip: IpAddr,
) -> bool {
    let from_it = by_source.get(&ip).copied().unwrap_or(0);
    if from_it >= limits.handshaking_per_source {
        return false;
    }
    if handshaking.fetch_add(1, Ordering::SeqCst) >= limits.handshaking {
        handshaking.fetch_sub(1, Ordering::SeqCst);
        return false;
    }
    by_source.insert(ip, from_it + 1);
    true
}

/// A connection's handshake slot: counted in the link's total and its source's, both given back when the handshake
/// ends, done or not.
struct Shaking<'a> {
    handshaking: &'a AtomicUsize,
    sources: &'a Mutex<HashMap<IpAddr, usize>>,
    source: IpAddr,
}

impl Shaking<'_> {
    fn release(self) {
        self.handshaking.fetch_sub(1, Ordering::SeqCst);
        if let Ok(mut by_source) = self.sources.lock()
            && let Some(from_it) = by_source.get_mut(&self.source)
        {
            *from_it -= 1;
            if *from_it == 0 {
                by_source.remove(&self.source);
            }
        }
    }
}

/// The handshake, given up once `within` has passed since it began, however the peer spaces its bytes.
fn handshake(
    tcp: TcpStream,
    tls: &LinkTls,
    within: Duration,
) -> io::Result<StreamOwned<ServerConnection, TcpStream>> {
    let until = std::time::Instant::now() + within;
    let connection = ServerConnection::new(tls.config.clone()).map_err(io::Error::other)?;
    let mut stream = StreamOwned::new(connection, tcp);
    while stream.conn.is_handshaking() {
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the handshake took too long",
            ));
        }
        stream.sock.set_read_timeout(Some(left))?;
        stream.sock.set_write_timeout(Some(left))?;
        stream.conn.complete_io(&mut stream.sock)?;
    }
    Ok(stream)
}

/// Reads a frame from any stream: the link's, for a client's own use (Core's adapter, a test).
pub fn read_answer(stream: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    read_frame(stream, 1 << 20)
}

/// Writes a question as a frame to any stream.
pub fn write_question(stream: &mut impl Write, question: &[u8]) -> io::Result<()> {
    write_frame(stream, question)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{LinkLimits, Shaking, admit};

    // verifies: DEC-28
    #[test]
    fn refused_sources_leave_no_entry_and_the_map_of_sources_stays_bounded() {
        let limits = LinkLimits {
            handshaking: 2,
            handshaking_per_source: 1,
            ..LinkLimits::default()
        };
        let handshaking = AtomicUsize::new(0);
        let mut by_source: HashMap<IpAddr, usize> = HashMap::new();
        let ip = |n: u32| IpAddr::from((0x5db8_d800_u32 + n).to_be_bytes());
        // Two sources take the two slots; a second handshake from one of them is over its share.
        assert!(admit(&mut by_source, &handshaking, &limits, ip(1)));
        assert!(admit(&mut by_source, &handshaking, &limits, ip(2)));
        assert!(!admit(&mut by_source, &handshaking, &limits, ip(1)));
        // A thousand more, each from an address of its own, find every slot taken: each is refused, and none is kept.
        for n in 3..1003 {
            assert!(!admit(&mut by_source, &handshaking, &limits, ip(n)));
        }
        assert_eq!(
            by_source.len(),
            2,
            "only the sources with a handshake under way"
        );
        assert_eq!(handshaking.load(Ordering::SeqCst), 2);
        // Each handshake that ends gives its slot back and its source's entry at zero goes.
        let sources = Mutex::new(by_source);
        for n in [1, 2] {
            Shaking {
                handshaking: &handshaking,
                sources: &sources,
                source: ip(n),
            }
            .release();
        }
        assert!(sources.lock().unwrap().is_empty());
        assert_eq!(handshaking.load(Ordering::SeqCst), 0);
    }
}
