//! The link to Core (`T-015-04` part 1, `DEC-28`): mTLS under the operator's CA, on rustls with its ring provider.

mod common;

use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use rutent_reader_host::link::{Link, LinkTls, read_answer, write_question};

const HOST_NAME: &str = "readers.operator.example";
const CORE_NAME: &str = "core.operator.example";

/// A certificate authority, and the leaves it issues.
struct Authority {
    certificate: Certificate,
    key: KeyPair,
}

struct Leaf {
    chain: Vec<CertificateDer<'static>>,
    chain_pem: String,
    key_der: Vec<u8>,
    key_pem: String,
}

impl Authority {
    fn new() -> Self {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let certificate = params.self_signed(&key).unwrap();
        Authority { certificate, key }
    }

    fn pem(&self) -> String {
        self.certificate.pem()
    }

    fn issue(&self, name: &str, purpose: ExtendedKeyUsagePurpose) -> Leaf {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec![name.to_owned()]).unwrap();
        params.extended_key_usages = vec![purpose];
        let leaf = params
            .signed_by(&key, &self.certificate, &self.key)
            .unwrap();
        Leaf {
            chain: vec![leaf.der().clone()],
            chain_pem: leaf.pem(),
            key_der: key.serialize_der(),
            key_pem: key.serialize_pem(),
        }
    }
}

/// Core's side of the link: the operator's CA as its one anchor, the listed subject name, and a client certificate
/// or none.
fn client(operator: &Authority, certificate: Option<&Leaf>) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.add(operator.certificate.der().clone()).unwrap();
    let builder =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots);
    Arc::new(match certificate {
        Some(leaf) => builder
            .with_client_auth_cert(
                leaf.chain.clone(),
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf.key_der.clone())),
            )
            .unwrap(),
        None => builder.with_no_client_auth(),
    })
}

/// Asks one question over the link, holding the host to `name`: the answer's bytes, or why there are none.
fn over_link(
    config: Arc<ClientConfig>,
    at: SocketAddr,
    name: &str,
    question: &[u8],
) -> io::Result<Vec<u8>> {
    let tcp = TcpStream::connect_timeout(&at, Duration::from_secs(5))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30)))?;
    let connection = ClientConnection::new(config, ServerName::try_from(name.to_owned()).unwrap())
        .map_err(io::Error::other)?;
    let mut stream = StreamOwned::new(connection, tcp);
    write_question(&mut stream, question)?;
    match read_answer(&mut stream)? {
        Some(answer) if !answer.is_empty() => Ok(answer),
        _ => Err(io::Error::other("no answer")),
    }
}

fn serve(operator: &Authority, host_leaf: &Leaf, rig: Rig) -> Link {
    let tls = LinkTls::new(
        operator.pem().as_bytes(),
        host_leaf.chain_pem.as_bytes(),
        host_leaf.key_pem.as_bytes(),
        CORE_NAME,
    )
    .unwrap();
    Link::serve(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tls,
        Arc::new(rig.host),
    )
    .unwrap()
}

// verifies: DEC-28
#[test]
fn reader_host_takes_a_connection_only_from_a_client_certificate_the_operators_ca_issued() {
    let operator = Authority::new();
    let stranger = Authority::new();
    let reader = rpc_reader("base", &[]);
    let rig = Rig::start(vec![reader.clone()], honest_pool(), &[]);
    assert!(rig.host.wait_ready(&reader.name, Duration::from_secs(30)));
    let link = serve(
        &operator,
        &operator.issue(HOST_NAME, ExtendedKeyUsagePurpose::ServerAuth),
        rig,
    );

    let question = question("base", "base", status_of("0xmove"));
    let bytes = serde_json::to_vec(&question).unwrap();
    let ask_as = |certificate: Option<&Leaf>| {
        over_link(
            client(&operator, certificate),
            link.address(),
            HOST_NAME,
            &bytes,
        )
    };

    // Core's certificate, from the operator's CA: answered, and the answer holds.
    let core = operator.issue(CORE_NAME, ExtendedKeyUsagePurpose::ClientAuth);
    let answer = ask_as(Some(&core)).expect("Core is answered");
    let statement = as_core_reads(&answer, &question, &reader);
    assert!(unreadable(&statement.statement).is_none(), "{statement:?}");

    // No certificate; one another CA issued under Core's name; one the operator's CA issued for someone else.
    assert!(ask_as(None).is_err(), "no client certificate");
    let forged = stranger.issue(CORE_NAME, ExtendedKeyUsagePurpose::ClientAuth);
    assert!(
        ask_as(Some(&forged)).is_err(),
        "Core's name from another CA"
    );
    let other = operator.issue("desk.operator.example", ExtendedKeyUsagePurpose::ClientAuth);
    assert!(
        ask_as(Some(&other)).is_err(),
        "the operator's CA, not for Core"
    );

    // Core is still answered after them.
    assert!(ask_as(Some(&core)).is_ok());
}

// verifies: DEC-28
#[test]
fn core_holds_the_host_to_the_operators_ca_and_the_listed_name_and_takes_a_rotated_leaf() {
    let operator = Authority::new();
    let reader = rpc_reader("base", &[]);
    let core = operator.issue(CORE_NAME, ExtendedKeyUsagePurpose::ClientAuth);
    let question = serde_json::to_vec(&question("base", "base", network_rules())).unwrap();

    for _rotation in 0..2 {
        // Each start presents a new leaf, under the same CA and name: Core's configuration is the same.
        let rig = Rig::start(vec![reader.clone()], honest_pool(), &[]);
        assert!(rig.host.wait_ready(&reader.name, Duration::from_secs(30)));
        let link = serve(
            &operator,
            &operator.issue(HOST_NAME, ExtendedKeyUsagePurpose::ServerAuth),
            rig,
        );
        assert!(
            over_link(
                client(&operator, Some(&core)),
                link.address(),
                HOST_NAME,
                &question
            )
            .is_ok()
        );
        // A name the listing doesn't name is refused by Core.
        assert!(
            over_link(
                client(&operator, Some(&core)),
                link.address(),
                "other.operator.example",
                &question
            )
            .is_err()
        );
    }

    // A host whose leaf another CA issued, under the listed name, is refused by Core.
    let stranger = Authority::new();
    let rig = Rig::start(vec![reader.clone()], honest_pool(), &[]);
    let link = serve(
        &operator,
        &stranger.issue(HOST_NAME, ExtendedKeyUsagePurpose::ServerAuth),
        rig,
    );
    assert!(
        over_link(
            client(&operator, Some(&core)),
            link.address(),
            HOST_NAME,
            &question
        )
        .is_err()
    );
}
