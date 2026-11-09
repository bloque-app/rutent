//! Tests of the credential proxy's production transport (`T-014-02`): HTTPS with the store's certificate verified
//! against the deployment's CA, and plain HTTP only to a loopback address, for development. Every request carries
//! the store token, so it must never cross a network in clear text (`DEC-9`, ADR-0004).
//!
//! A local rustls server stands in for the store. Its certificates under `tests/fixtures/proxy/tls/` are for tests
//! only: `ca.pem` is a test CA, `store.pem` and `store.key` a certificate and key it issued for `localhost` alone,
//! and `other-ca.pem` an unrelated CA. They were made with `openssl req` and `openssl x509` (P-256, valid until
//! 2126), and no deployment trusts them.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rutent_contracts::module_interface::{Identifier, Name};
use rutent_prov::proxy::{
    HttpsTransport, KvConfig, KvResolver, Resolver, RoleSettings, Server, StoreAddress,
    StoreScheme, Token, Transport,
};

fn tls_fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/proxy/tls")
            .join(name),
    )
    .expect("fixture is readable")
}

/// What the stand-in store received: the HTTP request's head, once a connection got that far.
#[derive(Clone, Default)]
struct Received(Arc<Mutex<Vec<String>>>);

impl Received {
    fn requests(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// Reads one request head from `stream`, records it, and writes `answer`.
fn serve_one<S: Read + Write>(stream: S, answer: &[u8], received: &Received) {
    let mut reader = BufReader::new(stream);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        head.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    received.0.lock().unwrap().push(head);
    let stream = reader.get_mut();
    let _ = stream.write_all(answer);
    let _ = stream.flush();
}

/// A stand-in store over TLS on a loopback port, answering each connection with `answer`. It serves
/// `connections` connections, then stops.
fn tls_store(answer: Vec<u8>, connections: usize) -> (u16, Received, JoinHandle<()>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certs: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(&tls_fixture("store.pem"))
            .collect::<Result<_, _>>()
            .unwrap();
    let key = PrivateKeyDer::from_pem_slice(&tls_fixture("store.key")).unwrap();
    let config = Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Received::default();
    let seen = received.clone();
    let handle = std::thread::spawn(move || {
        for _ in 0..connections {
            let Ok((tcp, _)) = listener.accept() else {
                return;
            };
            tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let connection = rustls::ServerConnection::new(Arc::clone(&config)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, tcp);
            serve_one(&mut stream, &answer, &seen);
            stream.conn.send_close_notify();
            let _ = stream.flush();
        }
    });
    (port, received, handle)
}

fn https(host: &str, port: u16) -> StoreAddress {
    StoreAddress {
        scheme: StoreScheme::Https,
        host: host.to_owned(),
        port,
    }
}

fn http(host: &str, port: u16) -> StoreAddress {
    StoreAddress {
        scheme: StoreScheme::Http,
        host: host.to_owned(),
        port,
    }
}

fn answer(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.0 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

const BODY: &str = r#"{"data":{"data":{"value":"over-tls"},"metadata":{"version":3}}}"#;
const TOKEN: &str = "test-token-not-a-secret";

fn resolver(transport: HttpsTransport) -> KvResolver<HttpsTransport> {
    KvResolver::openbao(
        KvConfig::new("secret", "rutent", Token::new(TOKEN).unwrap()).unwrap(),
        transport,
    )
}

fn lookup(resolver: &KvResolver<HttpsTransport>) -> Result<String, String> {
    resolver
        .resolve(
            &Identifier::new("bridge-pay").unwrap(),
            &Name::new("api-key").unwrap(),
        )
        .map(|v| v.as_str().to_owned())
        .map_err(|e| e.to_string())
}

// verifies: REQ-MDL-6, DEC-9
#[test]
fn store_reached_over_https_with_its_certificate_verified_answers() {
    let (port, received, server) = tls_store(answer("200 OK", BODY), 1);
    let transport =
        HttpsTransport::new(&https("localhost", port), Some(&tls_fixture("ca.pem"))).unwrap();

    assert_eq!(lookup(&resolver(transport)), Ok("over-tls".to_owned()));

    server.join().unwrap();
    let requests = received.requests();
    assert_eq!(requests.len(), 1);
    let head = &requests[0];
    assert!(
        head.starts_with("GET /v1/secret/data/rutent/bridge-pay/api-key HTTP/1."),
        "{head}"
    );
    assert!(
        head.contains(&format!("Host: localhost:{port}\r\n")),
        "{head}"
    );
    assert!(
        head.contains(&format!("X-Vault-Token: {TOKEN}\r\n")),
        "{head}"
    );
}

// verifies: REQ-MDL-6, DEC-9
#[test]
fn store_certificate_not_signed_by_the_deployments_ca_is_refused_before_the_request_is_sent() {
    let (port, received, server) = tls_store(answer("200 OK", BODY), 1);
    let transport = HttpsTransport::new(
        &https("localhost", port),
        Some(&tls_fixture("other-ca.pem")),
    )
    .unwrap();

    assert!(lookup(&resolver(transport)).is_err());

    server.join().unwrap();
    assert!(received.requests().is_empty(), "the request was sent");
}

// verifies: REQ-MDL-6, DEC-9
#[test]
fn store_certificate_for_another_name_is_refused_before_the_request_is_sent() {
    // The certificate names `localhost` only; reached as `127.0.0.1`, it doesn't match.
    let (port, received, server) = tls_store(answer("200 OK", BODY), 1);
    let transport =
        HttpsTransport::new(&https("127.0.0.1", port), Some(&tls_fixture("ca.pem"))).unwrap();

    assert!(lookup(&resolver(transport)).is_err());

    server.join().unwrap();
    assert!(received.requests().is_empty(), "the request was sent");
}

// verifies: REQ-MDL-6, DEC-9
#[test]
fn plain_http_off_loopback_is_refused_before_anything_is_sent() {
    // Refused when the transport is built: it never resolves or connects.
    for host in [
        "192.0.2.1",
        "10.0.0.1",
        "0.0.0.0",
        "localhost",
        "bao.internal",
        "::ffff:192.0.2.1",
    ] {
        assert!(
            HttpsTransport::new(&http(host, 8200), None).is_err(),
            "plain HTTP to {host} was allowed"
        );
    }
}

// verifies: REQ-MDL-6, DEC-9
#[test]
fn https_without_the_deployments_ca_is_refused() {
    assert!(HttpsTransport::new(&https("localhost", 8200), None).is_err());
    assert!(HttpsTransport::new(&https("localhost", 8200), Some(b"not a certificate")).is_err());
    assert!(
        HttpsTransport::new(
            &https("not a host name", 8200),
            Some(&tls_fixture("ca.pem"))
        )
        .is_err()
    );
}

// verifies: REQ-MDL-17
#[test]
fn plain_http_to_loopback_reaches_a_development_server() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Received::default();
    let seen = received.clone();
    let server = std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        serve_one(tcp, &answer("200 OK", BODY), &seen);
    });
    let transport = HttpsTransport::new(&http("127.0.0.1", port), None).unwrap();

    assert_eq!(lookup(&resolver(transport)), Ok("over-tls".to_owned()));
    server.join().unwrap();
    assert_eq!(received.requests().len(), 1);
}

// verifies: REQ-MDL-17, DEC-9
#[test]
fn redirect_is_returned_not_followed() {
    let redirect = b"HTTP/1.0 307 Temporary Redirect\r\nLocation: https://elsewhere.example/v1/x\r\nContent-Length: 0\r\n\r\n".to_vec();
    let (port, received, server) = tls_store(redirect, 1);
    let transport =
        HttpsTransport::new(&https("localhost", port), Some(&tls_fixture("ca.pem"))).unwrap();

    let error = lookup(&resolver(transport)).unwrap_err();

    assert!(error.contains("307"), "{error}");
    server.join().unwrap();
    assert_eq!(received.requests().len(), 1);
}

// verifies: REQ-MDL-17
#[test]
fn answer_over_the_size_limit_is_refused() {
    let body = format!(
        r#"{{"data":{{"data":{{"value":"{}"}}}}}}"#,
        "x".repeat(4096)
    );
    let (port, _, server) = tls_store(answer("200 OK", &body), 1);
    let transport = HttpsTransport::new(&https("localhost", port), Some(&tls_fixture("ca.pem")))
        .unwrap()
        .with_max_body(1024);

    assert!(lookup(&resolver(transport)).is_err());
    server.join().unwrap();
}

// verifies: REQ-MDL-17
#[test]
fn store_that_never_answers_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (tcp, _): (TcpStream, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_millis(800));
        drop(tcp);
    });
    let transport = HttpsTransport::new(&http("127.0.0.1", port), None)
        .unwrap()
        .with_timeout(Duration::from_millis(200));

    let started = std::time::Instant::now();
    let outcome = transport.get(&probe_request());
    assert!(outcome.is_err());
    assert!(
        started.elapsed() < Duration::from_millis(700),
        "{:?}",
        started.elapsed()
    );
    server.join().unwrap();
}

/// A request as a resolver makes it, taken from a resolver over a transport that keeps it.
fn probe_request() -> rutent_prov::proxy::Request {
    struct Keep(Mutex<Option<rutent_prov::proxy::Request>>);
    impl Transport for Keep {
        fn get(
            &self,
            request: &rutent_prov::proxy::Request,
        ) -> Result<rutent_prov::proxy::Response, rutent_prov::proxy::TransportError> {
            *self.0.lock().unwrap() = Some(request.clone());
            Err(rutent_prov::proxy::TransportError::new("kept"))
        }
    }
    let keep = KvResolver::openbao(
        KvConfig::new("secret", "rutent", Token::new(TOKEN).unwrap()).unwrap(),
        Keep(Mutex::new(None)),
    );
    let _ = keep.resolve(
        &Identifier::new("bridge-pay").unwrap(),
        &Name::new("api-key").unwrap(),
    );
    keep.transport().0.lock().unwrap().take().unwrap()
}

// verifies: REQ-MDL-6, DEC-9
#[test]
fn transport_for_a_role_trusts_the_ca_file_its_settings_name() {
    let (port, _, server) = tls_store(answer("200 OK", BODY), 1);
    let role = |ca_file: Option<std::path::PathBuf>| RoleSettings {
        server: Server::OpenBao,
        store: https("localhost", port),
        ca_file,
        mount: "secret".to_owned(),
        secrets: "rutent/internal/secrets".to_owned(),
        keys: "rutent/internal/keys".to_owned(),
        namespace: None,
    };
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proxy/tls");

    // HTTPS with no CA file, or one that can't be read, is refused.
    assert!(HttpsTransport::for_role(&role(None)).is_err());
    assert!(HttpsTransport::for_role(&role(Some(fixtures.join("missing.pem")))).is_err());

    let transport = HttpsTransport::for_role(&role(Some(fixtures.join("ca.pem")))).unwrap();
    assert_eq!(lookup(&resolver(transport)), Ok("over-tls".to_owned()));
    server.join().unwrap();
}
