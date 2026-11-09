//! Tests of the outside part's egress (`T-014-03`): only declared destinations, every redirect checked, the
//! connected address the one checked (`B-17`), TLS verified and never downgraded (`B-18`), and every call recorded
//! (`REQ-MDL-7`, `REQ-MDL-8`, ADR-0004).
//!
//! No test needs a network. Names resolve from a table, and the addresses they give are served by servers on
//! loopback: [`LoopbackConnector`] maps each public address a test uses to a loopback listener of its own, so a
//! test can see which address received what, and that a refused one received nothing.
//!
//! There is no TLS stack here. The loopback servers announce the name they are certified for, and the connector
//! checks it before sending anything, standing in for the certificate check a TLS client makes. The TLS probe,
//! [`tls_probe`], is generic over the connector, so the same probe runs against `T-014-02`'s `rustls` client when
//! the two are joined (see the `F-014` notes).

#[path = "egress_support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rutent_contracts::module_interface::{
    Address, CallOutcome, Nothing, OutboundCallRecord, RefusalReason, Refused, Scheme,
    validate_record, validate_record_secrets,
};
use rutent_prov::egress::{
    ConnectError, Connector, Dial, Egress, Endpoint, Header, InvalidRequest, MAX_HOPS, Method,
    Outcome, Refusal, Request, Response,
};
use support::*;

/// A server on loopback, standing for one address of a destination.
struct Loopback {
    address: SocketAddr,
    accepted: Arc<AtomicUsize>,
    received: Arc<Mutex<Vec<String>>>,
}

impl Loopback {
    /// Serves on a fresh loopback port. Each connection is greeted with the name the server is certified for
    /// (`CERT name`), or `NONE` for no certificate, then whatever request comes is kept and `answer` sent back:
    /// a status, and a location after a space for a redirect.
    fn serve(certificate: Option<&str>, answer: &str) -> Loopback {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("its address");
        let accepted = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(Vec::new()));
        let greeting = match certificate {
            Some(name) => format!("CERT {name}\n"),
            None => "NONE\n".to_owned(),
        };
        let answer = format!("{answer}\n");
        let (count, kept) = (accepted.clone(), received.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                count.fetch_add(1, Ordering::SeqCst);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                if stream.write_all(greeting.as_bytes()).is_err() {
                    continue;
                }
                let mut request = String::new();
                let _ = stream.read_to_string(&mut request);
                if !request.is_empty() {
                    kept.lock().unwrap().push(request);
                    let _ = stream.write_all(answer.as_bytes());
                }
            }
        });
        Loopback {
            address,
            accepted,
            received,
        }
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    fn received(&self) -> Vec<String> {
        self.received.lock().unwrap().clone()
    }
}

/// A connector over loopback: the address dialled is mapped to the loopback server standing for it. It reads the
/// server's certificate greeting and, standing in for a TLS client's check, sends nothing unless the server is
/// certified for the dialled name.
#[derive(Default)]
struct LoopbackConnector {
    servers: HashMap<IpAddr, SocketAddr>,
    dials: Mutex<Vec<(String, SocketAddr)>>,
}

impl LoopbackConnector {
    fn with(mut self, address: &str, server: &Loopback) -> Self {
        self.servers.insert(ip(address), server.address);
        self
    }

    fn dials(&self) -> Vec<(String, SocketAddr)> {
        self.dials.lock().unwrap().clone()
    }
}

impl Connector for LoopbackConnector {
    fn exchange(&self, dial: &Dial<'_>, request: &Request) -> Result<Response, ConnectError> {
        self.dials
            .lock()
            .unwrap()
            .push((dial.name.as_str().to_owned(), dial.to));
        let server = self
            .servers
            .get(&dial.to.ip())
            .ok_or(ConnectError::Unreachable)?;
        let stream = TcpStream::connect(server).map_err(|_| ConnectError::Unreachable)?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|_| ConnectError::Unreachable)?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|_| ConnectError::Unreachable)?);
        let mut greeting = String::new();
        reader
            .read_line(&mut greeting)
            .map_err(|_| ConnectError::NotVerified)?;
        if greeting.trim_end() != format!("CERT {}", dial.name.as_str()) {
            let _ = stream.shutdown(Shutdown::Both);
            return Err(ConnectError::NotVerified);
        }
        let mut writer = stream;
        let mut message = format!(
            "{} {}\nhost: {}\n",
            request.method().as_str(),
            request.target(),
            dial.name.as_str()
        );
        for header in request.headers() {
            message.push_str(&format!("{}: {}\n", header.name(), header.value()));
        }
        message.push('\n');
        let mut bytes = message.into_bytes();
        bytes.extend_from_slice(request.body());
        writer
            .write_all(&bytes)
            .and_then(|()| writer.shutdown(Shutdown::Write))
            .map_err(|_| ConnectError::NoAnswer)?;
        let mut answer = String::new();
        reader
            .read_to_string(&mut answer)
            .map_err(|_| ConnectError::NoAnswer)?;
        let answer = answer.trim_end();
        let (status, location) = answer.split_once(' ').unwrap_or((answer, ""));
        let status = status.parse().map_err(|_| ConnectError::NoAnswer)?;
        let headers = if location.is_empty() {
            Vec::new()
        } else {
            vec![("location".to_owned(), location.to_owned())]
        };
        Ok(Response {
            status,
            headers,
            body: Vec::new(),
        })
    }
}

fn https(host: &str) -> Endpoint {
    Endpoint {
        scheme: Scheme::Https,
        host: host.to_owned(),
        port: 443,
    }
}

fn refused(reason: RefusalReason) -> CallOutcome {
    CallOutcome::Refused(Refused { reason })
}

fn address(text: &str) -> Option<Address> {
    Address::new(text.to_owned())
}

/// Checks that `record` is one the contract and the declaration accept.
fn valid(record: &OutboundCallRecord) {
    validate_record(record).expect("a valid record");
    validate_record_secrets(&declaration(), record).expect("secrets declared for the operation");
}

/// `B-17`: a declared name that resolves to an allowed address on one call and to a disallowed one on the next is
/// refused on the next, and the disallowed address receives nothing. The address connected to is the one the
/// layer checked; the name is not resolved again on the way.
// verifies: REQ-MDL-7, THR-79
#[test]
fn destination_re_resolved_to_an_undeclared_address_is_refused() {
    let api = Loopback::serve(Some("api.acme.example"), "200");
    let metadata = Loopback::serve(Some("api.acme.example"), "200");
    let resolver = TableResolver::default();
    resolver
        .then("api.acme.example", &[API])
        .then("api.acme.example", &[METADATA]);
    let connector = LoopbackConnector::default()
        .with(API, &api)
        .with(METADATA, &metadata);
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-1");
    let mut log = Vec::new();

    let first = egress.send(
        &flight.call(),
        https("api.acme.example"),
        post("/payouts"),
        &mut log,
    );
    let second = egress.send(
        &flight.call(),
        https("api.acme.example"),
        post("/payouts"),
        &mut log,
    );

    assert!(matches!(first, Outcome::Completed(_)), "{first:?}");
    assert_eq!(second, Outcome::Refused(Refusal::AddressNotAllowed));
    assert_eq!(api.received().len(), 1);
    assert_eq!(
        metadata.accepted(),
        0,
        "the disallowed address receives nothing"
    );
    assert_eq!(resolver.asked(), ["api.acme.example", "api.acme.example"]);
    assert_eq!(
        connector.dials(),
        [("api.acme.example".to_owned(), SocketAddr::new(ip(API), 443))],
        "one dial, to the checked address"
    );
    assert_eq!(log[1].outcome, refused(RefusalReason::AddressNotAllowed));
    assert_eq!(log[1].hops.len(), 1);
    assert_eq!(log[1].hops[0].destination, Some(name("api")));
    assert_eq!(log[1].hops[0].address, None);
    log.iter().for_each(valid);
}

/// `B-17` within one call: a redirect back to the same declared host, now resolving elsewhere, is refused.
// verifies: REQ-MDL-7, THR-79
#[test]
fn redirect_to_a_declared_host_re_resolved_to_an_undeclared_address_is_refused() {
    let resolver = TableResolver::default();
    resolver
        .then("api.acme.example", &[API])
        .then("api.acme.example", &["10.0.0.7"]);
    let connector = ScriptedConnector::default();
    connector
        .answer(API, redirect(307, "/payouts/retry"))
        .answer("10.0.0.7", ok());
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-2");
    let mut log = Vec::new();

    let outcome = egress.send(
        &flight.call(),
        https("api.acme.example"),
        post("/payouts"),
        &mut log,
    );

    assert_eq!(outcome, Outcome::Refused(Refusal::AddressNotAllowed));
    assert_eq!(connector.dials().len(), 1);
    assert_eq!(connector.dials()[0].to, SocketAddr::new(ip(API), 443));
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].hops.len(), 2);
    assert_eq!(log[0].hops[0].address, address(API));
    assert_eq!(log[0].hops[1].address, None);
    valid(&log[0]);
}

/// A name whose answer mixes an allowed address with a disallowed one is refused whole: the layer doesn't pick
/// among them, so an attacker's extra answer can't be the one connected to.
// verifies: REQ-MDL-7, THR-79
#[test]
fn name_resolving_to_any_undeclared_address_is_refused_and_nothing_is_dialled() {
    for answer in [&[API, METADATA][..], &[METADATA, API], &["::1"], &[]] {
        let resolver = TableResolver::default();
        resolver.then("api.acme.example", answer);
        let connector = ScriptedConnector::default();
        connector.answer(API, ok());
        let egress = Egress::new(&resolver, &connector);
        let flight = Flight::payout("tx-3");
        let mut log = Vec::new();

        let outcome = egress.send(
            &flight.call(),
            https("api.acme.example"),
            post("/p"),
            &mut log,
        );

        assert_eq!(
            outcome,
            Outcome::Refused(Refusal::AddressNotAllowed),
            "{answer:?}"
        );
        assert!(connector.dials().is_empty(), "{answer:?}");
        valid(&log[0]);
    }
}

/// A destination pinned to blocks is reached only at an address inside them, however public the other is.
// verifies: REQ-MDL-7
#[test]
fn pinned_destination_resolving_outside_its_blocks_is_refused() {
    let resolver = TableResolver::default();
    resolver
        .then("files.acme.example", &[FILES])
        .then("files.acme.example", &["8.8.8.8"]);
    let connector = ScriptedConnector::default();
    connector.answer(FILES, ok()).answer("8.8.8.8", ok());
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-4");
    let mut log = Vec::new();

    let inside = egress.send(
        &flight.call(),
        https("files.acme.example"),
        post("/f"),
        &mut log,
    );
    let outside = egress.send(
        &flight.call(),
        https("files.acme.example"),
        post("/f"),
        &mut log,
    );

    assert!(matches!(inside, Outcome::Completed(_)), "{inside:?}");
    assert_eq!(outside, Outcome::Refused(Refusal::AddressNotAllowed));
    assert_eq!(connector.dials().len(), 1);
    log.iter().for_each(valid);
}

/// A redirect to a host the operation doesn't list is refused before that host is resolved or reached, and the
/// record shows where the call was sent.
// verifies: REQ-MDL-7, THR-79
#[test]
fn redirect_to_an_undeclared_destination_is_refused() {
    let api = Loopback::serve(
        Some("api.acme.example"),
        "302 https://collector.attacker.example/steal",
    );
    let attacker = Loopback::serve(Some("collector.attacker.example"), "200");
    let resolver = TableResolver::default();
    resolver
        .then("api.acme.example", &[API])
        .then("collector.attacker.example", &["198.51.99.1"]);
    let connector = LoopbackConnector::default()
        .with(API, &api)
        .with("198.51.99.1", &attacker);
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-5");
    let mut log = Vec::new();

    let outcome = egress.send(
        &flight.call(),
        https("api.acme.example"),
        post("/payouts"),
        &mut log,
    );

    assert_eq!(
        outcome,
        Outcome::Refused(Refusal::RedirectToUndeclaredDestination)
    );
    assert_eq!(
        attacker.accepted(),
        0,
        "the undeclared destination receives nothing"
    );
    assert_eq!(resolver.asked(), ["api.acme.example"]);
    let record = &log[0];
    assert_eq!(
        record.outcome,
        refused(RefusalReason::RedirectToUndeclaredDestination)
    );
    assert_eq!(record.hops.len(), 2);
    assert_eq!(record.hops[1].host.as_str(), "collector.attacker.example");
    assert_eq!(record.hops[1].destination, None);
    assert_eq!(record.hops[1].address, None);
    valid(record);
}

/// A redirect to a destination the module declares, but for another operation, is refused: what the operation in
/// flight lists is what counts, not the whole allow-list.
// verifies: REQ-MDL-7
#[test]
fn redirect_to_a_destination_only_another_operation_lists_is_refused() {
    let resolver = TableResolver::default();
    resolver
        .then("api.acme.example", &[API])
        .then("status.acme.example", &["93.184.216.99"]);
    let connector = ScriptedConnector::default();
    connector
        .answer(API, redirect(302, "https://status.acme.example/"))
        .answer("93.184.216.99", ok());
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-6");
    let mut log = Vec::new();

    let outcome = egress.send(
        &flight.call(),
        https("api.acme.example"),
        post("/p"),
        &mut log,
    );

    assert_eq!(
        outcome,
        Outcome::Refused(Refusal::RedirectToUndeclaredDestination)
    );
    assert_eq!(connector.dials().len(), 1);
    assert_eq!(log[0].hops[1].destination, None);
    valid(&log[0]);
}

/// `B-18`: a redirect never downgrades. `http`, another port, another scheme, a user name in the authority and a
/// location that isn't a URL are all refused, and nothing more is dialled.
// verifies: REQ-MDL-7, THR-79
#[test]
fn redirect_to_plain_http_or_an_unreadable_location_is_refused() {
    for location in [
        "http://api.acme.example/payouts",
        "HTTP://api.acme.example/payouts",
        "//api.acme.example:80/payouts",
        "https://api.acme.example:8443/payouts",
        "ftp://api.acme.example/payouts",
        "https://user@api.acme.example/payouts",
        "https://api.acme.example@collector.attacker.example/",
        "payouts/relative",
        "https://",
        "https://api.acme.example:99999/",
        "",
    ] {
        let resolver = TableResolver::default();
        resolver.then("api.acme.example", &[API]);
        let connector = ScriptedConnector::default();
        connector.answer(API, redirect(301, location));
        let egress = Egress::new(&resolver, &connector);
        let flight = Flight::payout("tx-7");
        let mut log = Vec::new();

        let outcome = egress.send(
            &flight.call(),
            https("api.acme.example"),
            post("/p"),
            &mut log,
        );

        assert_eq!(
            outcome,
            Outcome::Refused(Refusal::RedirectToUndeclaredDestination),
            "{location:?}"
        );
        assert_eq!(connector.dials().len(), 1, "{location:?}");
        valid(&log[0]);
    }
}

/// A redirect to a destination the operation in flight lists is followed, re-checked and recorded as its own hop.
/// `303` turns the request into a `GET` with no body; `307` keeps both.
// verifies: REQ-MDL-7, REQ-MDL-8
#[test]
fn redirect_to_a_declared_destination_is_followed_and_recorded() {
    for (status, method, body) in [
        (303, Method::Get, Vec::new()),
        (307, Method::Post, b"{\"amount\":\"10\"}".to_vec()),
    ] {
        let resolver = TableResolver::default();
        resolver
            .then("api.acme.example", &[API])
            .then("files.acme.example", &[FILES]);
        let connector = ScriptedConnector::default();
        connector
            .answer(
                API,
                redirect(status, "https://FILES.acme.example:443/r/1?x=y"),
            )
            .answer(FILES, ok());
        let egress = Egress::new(&resolver, &connector);
        let flight = Flight::payout("tx-8");
        let mut log = Vec::new();

        let outcome = egress.send(
            &flight.call(),
            https("api.acme.example"),
            post("/p"),
            &mut log,
        );

        assert!(matches!(outcome, Outcome::Completed(_)), "{outcome:?}");
        let dials = connector.dials();
        assert_eq!(dials.len(), 2);
        assert_eq!(dials[1].name, "files.acme.example");
        assert_eq!(dials[1].to, SocketAddr::new(ip(FILES), 443));
        assert_eq!(dials[1].target, "/r/1?x=y");
        assert_eq!((dials[1].method, &dials[1].body), (method, &body));
        let record = &log[0];
        assert_eq!(record.outcome, CallOutcome::Completed(Nothing {}));
        let hops: Vec<_> = record
            .hops
            .iter()
            .map(|h| (h.destination.clone(), h.address.clone()))
            .collect();
        assert_eq!(
            hops,
            [
                (Some(name("api")), address(API)),
                (Some(name("files")), address(FILES))
            ]
        );
        valid(record);
    }
}

/// A chain of redirects stops at [`MAX_HOPS`] connections and fails, every hop recorded.
// verifies: REQ-MDL-7
#[test]
fn redirects_stop_after_the_most_hops_a_call_may_make() {
    let resolver = TableResolver::default();
    resolver.then("api.acme.example", &[API]);
    let connector = ScriptedConnector::default();
    connector.answer(API, redirect(302, "/again"));
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-9");
    let mut log = Vec::new();

    let outcome = egress.send(
        &flight.call(),
        https("api.acme.example"),
        post("/p"),
        &mut log,
    );

    assert!(
        matches!(outcome, Outcome::Failed(ref r) if r.status == 302),
        "{outcome:?}"
    );
    assert_eq!(connector.dials().len(), MAX_HOPS);
    assert_eq!(log[0].hops.len(), MAX_HOPS);
    assert_eq!(log[0].outcome, CallOutcome::Failed(Nothing {}));
    valid(&log[0]);
}

/// The `B-18` probe, generic over the connector so the same probe runs against a real TLS client: one address is
/// served for another name, one with no certificate. Each call to the declared destination is refused, recorded,
/// dialled once with no retry, and neither server receives a byte of the request.
fn tls_probe<C: Connector>(
    connector: &C,
    wrong_name: &str,
    no_certificate: &str,
    received: &dyn Fn() -> usize,
) {
    for served in [wrong_name, no_certificate] {
        let resolver = TableResolver::default();
        resolver.then("api.acme.example", &[served]);
        let egress = Egress::new(&resolver, connector);
        let flight = Flight::payout("tx-10");
        let mut log = Vec::new();

        let outcome = egress.send(
            &flight.call(),
            https("api.acme.example"),
            post("/p"),
            &mut log,
        );

        assert_eq!(outcome, Outcome::Refused(Refusal::NotVerified), "{served}");
        assert_eq!(resolver.asked().len(), 1, "no other address tried");
        let record = &log[0];
        assert_eq!(record.outcome, refused(RefusalReason::PeerNotVerified));
        assert_eq!(record.hops.len(), 1);
        assert_eq!(record.hops[0].destination, Some(name("api")));
        assert_eq!(record.hops[0].address, None, "no verified connection");
        valid(record);
    }
    assert_eq!(received(), 0, "no request reaches an unverified peer");
}

/// `B-18`: a declared destination whose server is certified for another name, or offers no certificate, is
/// refused, and receives nothing.
// verifies: REQ-MDL-7, THR-79
#[test]
fn failed_tls_check_to_a_declared_destination_is_refused() {
    let impostor = Loopback::serve(Some("collector.attacker.example"), "200");
    let bare = Loopback::serve(None, "200");
    let connector = LoopbackConnector::default()
        .with(API, &impostor)
        .with("93.184.216.35", &bare);

    tls_probe(&connector, API, "93.184.216.35", &|| {
        impostor.received().len() + bare.received().len()
    });

    assert_eq!(
        connector.dials().len(),
        2,
        "each dialled once, never retried"
    );
    assert_eq!((impostor.accepted(), bare.accepted()), (1, 1));
}

/// `REQ-MDL-8`: every call leaves exactly one record, whether it completed, failed, got no answer or was refused
/// before any dial (another operation's destination, an undeclared host), naming the transaction, provider,
/// module version and operation, the secrets' names, and each hop's destination and address.
// verifies: REQ-MDL-8
#[test]
fn every_outbound_call_is_recorded_with_destination_version_and_transaction() {
    let resolver = TableResolver::default();
    resolver
        .then("api.acme.example", &[API])
        .then("files.acme.example", &[FILES])
        .then("files.acme.example", &["93.184.216.81"])
        .then("status.acme.example", &["93.184.216.99"]);
    let connector = ScriptedConnector::default();
    connector
        .answer(API, ok())
        .answer(
            FILES,
            Ok(Response {
                status: 500,
                headers: Vec::new(),
                body: Vec::new(),
            }),
        )
        .answer("93.184.216.81", Err(ConnectError::NoAnswer))
        .answer("93.184.216.99", ok());
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-11");
    let mut log = Vec::new();

    let outcomes = [
        egress.send(
            &flight.call(),
            https("api.acme.example"),
            post("/a"),
            &mut log,
        ),
        egress.send(
            &flight.call(),
            https("files.acme.example"),
            post("/b"),
            &mut log,
        ),
        egress.send(
            &flight.call(),
            https("status.acme.example"),
            post("/c"),
            &mut log,
        ),
        egress.send(
            &flight.call(),
            https("unknown.example"),
            post("/d"),
            &mut log,
        ),
        egress.send(
            &flight.call(),
            https("files.acme.example"),
            post("/e"),
            &mut log,
        ),
    ];

    assert!(matches!(outcomes[0], Outcome::Completed(_)));
    assert!(matches!(outcomes[1], Outcome::Failed(_)));
    assert_eq!(
        outcomes[2],
        Outcome::Refused(Refusal::UndeclaredDestination)
    );
    assert_eq!(
        outcomes[3],
        Outcome::Refused(Refusal::UndeclaredDestination)
    );
    assert_eq!(outcomes[4], Outcome::NoAnswer);
    assert_eq!(log.len(), outcomes.len(), "one record per call");
    for record in &log {
        assert_eq!(record.transaction.as_str(), "tx-11");
        assert_eq!(record.provider.as_str(), PROVIDER);
        assert_eq!(record.module_version.as_str(), VERSION);
        assert_eq!(record.operation, name("payout"));
        assert_eq!(record.secrets, [name("api.key")]);
        assert_eq!(record.started_at.as_str(), "2026-11-09T12:00:00Z");
        valid(record);
    }
    assert_eq!(log[0].outcome, CallOutcome::Completed(Nothing {}));
    assert_eq!(log[0].hops[0].destination, Some(name("api")));
    assert_eq!(log[0].hops[0].address, address(API));
    assert_eq!(log[1].outcome, CallOutcome::Failed(Nothing {}));
    assert_eq!(log[1].hops[0].destination, Some(name("files")));
    assert_eq!(log[1].hops[0].address, address(FILES));
    assert_eq!(log[2].hops[0].host.as_str(), "status.acme.example");
    assert_eq!(
        log[2].hops[0].destination, None,
        "status is another operation's"
    );
    assert_eq!(log[3].hops[0].host.as_str(), "unknown.example");
    assert_eq!(log[4].outcome, CallOutcome::Unknown(Nothing {}));
    assert_eq!(log[4].hops[0].destination, Some(name("files")));
    assert_eq!(log[4].hops[0].address, address("93.184.216.81"));
    assert_eq!(
        connector.dials().len(),
        3,
        "status and unknown are never dialled"
    );
}

/// A call whose peer fails the TLS check is refused for that, not as an undeclared destination: the destination
/// is declared, and the record says which check failed.
// verifies: DEC-4, REQ-MDL-6
#[test]
fn outbound_call_that_failed_its_tls_check_names_its_own_reason() {
    let resolver = TableResolver::default();
    resolver.then("api.acme.example", &[API]);
    let connector = ScriptedConnector::default();
    connector.answer(API, Err(ConnectError::NotVerified));
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-13");
    let mut log = Vec::new();

    let outcome = egress.send(
        &flight.call(),
        https("api.acme.example"),
        post("/p"),
        &mut log,
    );

    assert_eq!(outcome, Outcome::Refused(Refusal::NotVerified));
    let record = &log[0];
    let reason = serde_json::to_value(&record.outcome).unwrap()["reason"].clone();
    assert_ne!(reason, "undeclared_destination");
    assert_eq!(reason, "peer_not_verified");
    assert_eq!(record.hops[0].destination, Some(name("api")));
    assert_eq!(record.hops[0].address, None, "no verified connection");
    valid(record);
}

/// A connection that drops after the request may have left is recorded as unknown with the address it connected
/// to. One that can't be made at all is recorded as unknown too, its hop naming the destination it matched and no
/// address, since nothing connected.
// verifies: REQ-MDL-8, REQ-AUD-1, DEC-4
#[test]
fn call_with_no_answer_is_recorded_as_unknown_and_one_that_connected_to_nothing_names_no_address() {
    let resolver = TableResolver::default();
    resolver
        .then("api.acme.example", &[API])
        .then("files.acme.example", &[FILES]);
    let connector = ScriptedConnector::default();
    connector.answer(API, Err(ConnectError::NoAnswer));
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-12");
    let mut log = Vec::new();

    let dropped = egress.send(
        &flight.call(),
        https("api.acme.example"),
        post("/a"),
        &mut log,
    );
    let unreachable = egress.send(
        &flight.call(),
        https("files.acme.example"),
        post("/b"),
        &mut log,
    );

    assert_eq!(dropped, Outcome::NoAnswer);
    assert_eq!(unreachable, Outcome::Unreachable);
    for (record, (destination, connected)) in
        log.iter().zip([("api", address(API)), ("files", None)])
    {
        assert_eq!(record.outcome, CallOutcome::Unknown(Nothing {}));
        assert_eq!(record.hops[0].destination, Some(name(destination)));
        assert_eq!(record.hops[0].address, connected);
        valid(record);
    }
}

/// `REQ-MDL-8`: a call naming another operation than the one in flight is refused, recorded under the operation
/// in flight, and nothing is resolved or dialled.
// verifies: REQ-MDL-8, REQ-MDL-7
#[test]
fn call_naming_another_operation_than_the_one_in_flight_is_refused() {
    let resolver = TableResolver::default();
    resolver.then("status.acme.example", &["93.184.216.99"]);
    let connector = ScriptedConnector::default();
    connector.answer("93.184.216.99", ok());
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-13");
    let mut log = Vec::new();

    let outcome = egress.send(
        &flight.asking("status"),
        https("status.acme.example"),
        post("/s"),
        &mut log,
    );

    assert_eq!(outcome, Outcome::Refused(Refusal::UndeclaredOperation));
    assert!(resolver.asked().is_empty());
    assert!(connector.dials().is_empty());
    assert_eq!(log[0].operation, name("payout"));
    assert_eq!(log[0].outcome, refused(RefusalReason::UndeclaredOperation));
    valid(&log[0]);
}

/// A request can't name a host of its own: not by a `Host` header, which could reach another virtual host behind
/// a checked address, and not by an absolute-form target. Nor can it frame the message itself.
// verifies: REQ-MDL-7, THR-79
#[test]
fn request_naming_its_own_host_or_framing_is_not_built() {
    let header = |name: &str, value: &str| {
        Request::new(
            Method::Get,
            "/",
            vec![Header::plain(name, value)],
            Vec::new(),
        )
    };
    assert_eq!(
        header("Host", "collector.attacker.example"),
        Err(InvalidRequest::Framing)
    );
    assert_eq!(
        header("transfer-encoding", "chunked"),
        Err(InvalidRequest::Framing)
    );
    assert_eq!(header("Content-Length", "0"), Err(InvalidRequest::Framing));
    assert_eq!(
        header("x-a", "1\r\nHost: evil"),
        Err(InvalidRequest::Header)
    );
    assert_eq!(header("x a", "1"), Err(InvalidRequest::Header));
    for target in [
        "https://collector.attacker.example/",
        "//collector.attacker.example/",
        "collector.attacker.example:443",
        "*",
        "/a b",
        "/a#frag",
        "",
    ] {
        assert_eq!(
            Request::new(Method::Get, target, Vec::new(), Vec::new()),
            Err(InvalidRequest::Target),
            "{target:?}"
        );
    }
    assert!(header("Authorization", "Bearer x").is_ok());
    let sensitive = Request::new(
        Method::Get,
        "/a?b=c",
        vec![Header::sensitive("Authorization", "Bearer s3cr3t")],
        b"secret body".to_vec(),
    )
    .expect("a request");
    let printed = format!("{sensitive:?}");
    assert!(
        !printed.contains("s3cr3t") && !printed.contains("secret body"),
        "{printed}"
    );
}

/// A redirect to another host never carries the credentials meant for the first: sensitive headers are dropped
/// when the scheme, host or port changes, and kept on a redirect within the same endpoint. Plain headers follow
/// either way (the curl CVE-2018-1000007 class).
// verifies: REQ-MDL-7, THR-79
#[test]
fn redirect_to_another_host_drops_the_credentials_meant_for_the_first() {
    let resolver = TableResolver::default();
    resolver
        .then("api.acme.example", &[API])
        .then("files.acme.example", &[FILES]);
    let connector = ScriptedConnector::default();
    connector
        .answer(API, redirect(307, "https://files.acme.example/upload"))
        .answer(FILES, redirect(307, "/upload/again"));
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-14");
    let request = Request::new(
        Method::Post,
        "/p",
        vec![
            Header::sensitive("Authorization", "Bearer api-only"),
            Header::plain("X-Request-Id", "r-1"),
        ],
        Vec::new(),
    )
    .expect("a request");
    let mut log = Vec::new();

    egress.send(&flight.call(), https("api.acme.example"), request, &mut log);

    let dials = connector.dials();
    assert_eq!(dials.len(), MAX_HOPS);
    let names =
        |dial: &Dialled| -> Vec<String> { dial.headers.iter().map(|(n, _)| n.clone()).collect() };
    assert_eq!(
        names(&dials[0]),
        ["Authorization", "X-Request-Id"],
        "the first host gets it"
    );
    for (i, dial) in dials.iter().enumerate().skip(1) {
        assert_eq!(dial.name, "files.acme.example");
        assert_eq!(
            names(dial),
            ["X-Request-Id"],
            "dial {i}: never to another host"
        );
    }
}

/// A pinned block is held to the same special-purpose ranges as `public`, whether or not the declaration was
/// validated on its way in: a block covering loopback, link-local (the cloud metadata address), private or
/// unique-local addresses reaches none of them.
// verifies: REQ-MDL-7, THR-79
#[test]
fn pinned_block_over_a_special_purpose_range_reaches_none_of_it() {
    for (block, resolved) in [
        ("169.254.169.254/32", METADATA),
        ("127.0.0.0/8", "127.0.0.1"),
        ("10.0.0.0/8", "10.0.0.7"),
        ("0.0.0.0/0", "192.168.1.1"),
        ("::/0", "::1"),
        ("fc00::/7", "fd00::1"),
    ] {
        let mut declaration = declaration();
        let files = declaration
            .outside
            .destinations
            .iter_mut()
            .find(|d| d.name.as_str() == "files")
            .expect("files");
        files.addresses = serde_json::from_value(serde_json::json!([block])).expect("a block");
        let resolver = TableResolver::default();
        resolver.then("files.acme.example", &[resolved]);
        let connector = ScriptedConnector::default();
        connector.answer(resolved, ok());
        let egress = Egress::new(&resolver, &connector);
        let flight = Flight {
            declaration,
            ..Flight::payout("tx-15")
        };
        let mut log = Vec::new();

        let outcome = egress.send(
            &flight.call(),
            https("files.acme.example"),
            post("/f"),
            &mut log,
        );

        assert_eq!(
            outcome,
            Outcome::Refused(Refusal::AddressNotAllowed),
            "{block}"
        );
        assert!(connector.dials().is_empty(), "{block}");
    }
}

/// A credential header goes by its name as well as its flag: `Authorization`, `Proxy-Authorization` and
/// `Cookie`, in any case, built as plain headers, still don't reach another host.
// verifies: REQ-MDL-7, THR-79
#[test]
fn plain_authorization_header_does_not_reach_another_host() {
    let resolver = TableResolver::default();
    resolver
        .then("api.acme.example", &[API])
        .then("files.acme.example", &[FILES]);
    let connector = ScriptedConnector::default();
    connector
        .answer(API, redirect(302, "https://files.acme.example/upload"))
        .answer(FILES, ok());
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-16");
    let request = Request::new(
        Method::Get,
        "/p",
        vec![
            Header::plain("Authorization", "Bearer api-only"),
            Header::plain("proxy-AUTHORIZATION", "Basic x"),
            Header::plain("COOKIE", "session=api-only"),
            Header::plain("X-Request-Id", "r-1"),
        ],
        Vec::new(),
    )
    .expect("a request");
    let mut log = Vec::new();

    egress.send(&flight.call(), https("api.acme.example"), request, &mut log);

    let dials = connector.dials();
    assert_eq!(dials.len(), 2);
    assert_eq!(dials[0].headers.len(), 4, "the first host gets them all");
    let names: Vec<&str> = dials[1].headers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["X-Request-Id"]);
}

/// Credentials dropped on the way to another host never come back: on a chain from `api` to `files` and back to
/// `api`, the last hop carries none of them, sensitive or plain.
// verifies: REQ-MDL-7, THR-79
#[test]
fn credentials_never_return_to_the_first_host_after_a_redirect_away() {
    let resolver = TableResolver::default();
    resolver
        .then("api.acme.example", &[API])
        .then("files.acme.example", &[FILES]);
    let connector = ScriptedConnector::default();
    connector
        .answer(API, redirect(302, "https://files.acme.example/hop"))
        .answer(FILES, redirect(302, "https://api.acme.example/back"));
    let egress = Egress::new(&resolver, &connector);
    let flight = Flight::payout("tx-17");
    let request = Request::new(
        Method::Get,
        "/p",
        vec![
            Header::sensitive("Authorization", "Bearer api-only"),
            Header::plain("Cookie", "session=api-only"),
            Header::plain("X-Request-Id", "r-1"),
        ],
        Vec::new(),
    )
    .expect("a request");
    let mut log = Vec::new();

    egress.send(&flight.call(), https("api.acme.example"), request, &mut log);

    let dials = connector.dials();
    assert!(dials.len() >= 3);
    for (i, dial) in dials.iter().enumerate().skip(1) {
        let names: Vec<&str> = dial.headers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["X-Request-Id"], "dial {i} to {}", dial.name);
    }
    assert_eq!(dials[2].name, "api.acme.example", "back at the first host");
}
