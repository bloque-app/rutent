//! The production world on the running platform (`T-014-05`): the credential proxy and the egress layer as the
//! platform's `World` (`proxy::ProductionWorld`), with the platform telling the proxy what each module handles.
//!
//! Every test runs real provider processes, admitted against a catalogue holding the providers' signed
//! advertisements. The world's seams are the tests' only stand-ins: the store behind the proxy's resolvers, the
//! name resolver, and the TCP dialer under the production `TlsConnector`, which maps each public address a test
//! uses to a TLS server of its own on loopback. Everything else, the platform's checks, the proxy's judgement, the
//! egress layer, rustls' handshake against an authority generated here, is production code.
//!
//! `B-17` and `B-18` are rows of `EXP-2`'s scoring sheet, not identifiers the plan defines, so the tests tag the
//! requirements and name the rows in their doc comments.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;
#[path = "holding_support/mod.rs"]
mod support;
#[path = "tls_support/mod.rs"]
mod tls;

use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::Probe;
use rutent_conformance::boundary::{Endpoint, ModuleSpec, Output, Step};
use rutent_contracts::module_interface::{
    Address, CallOutcome, CallRefusalReason, CrossModuleOutcome, Declaration, Host, Identifier,
    Name, OperationKind, PartName, RefusalReason, Refused, RouteReference, SecretRefusalReason,
    SecretValue, validate_record, validate_record_secrets,
};
use rutent_contracts::provider_role::VERSION;
use rutent_prov::catalogue::{Catalogue, Instant, tuples_of};
use rutent_prov::egress::{NameResolver, ThreadedResolver, TlsConnector};
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::{self, Admitted, Events, Handled, LookupOutcome};
use rutent_prov::proxy::{
    Bucket, CredentialProxy, InvalidSettings, NamedCall, ProductionWorld, ProxySettings,
    ResolveError, Resolver, UseOf,
};
use serde_json::json;
use signed::catalogue_support::{
    advertising_key, base64url, deployment, grant, join_made_at, public_key, ramp_advertisement,
    reserve_advertisement, statements_key,
};
use signed::{AT, RAMP, RESERVE};

/// What the store holds for one provider's name: a value, or the failure it gives.
type Held = Result<String, ResolveError>;

/// The one store behind the proxy: each provider's values by name, or the failure the store gives for one, and
/// every question it was asked, in order.
#[derive(Clone, Default)]
struct Store {
    values: Arc<Mutex<BTreeMap<(String, String), Held>>>,
    asked: Arc<Mutex<Vec<(String, String)>>>,
}

impl Store {
    fn holds(&self, provider: &str, name: &str, value: Result<&str, ResolveError>) {
        self.values.lock().unwrap().insert(
            (provider.to_owned(), name.to_owned()),
            value.map(str::to_owned),
        );
    }

    fn asked(&self) -> Vec<(String, String)> {
        self.asked.lock().unwrap().clone()
    }
}

impl Resolver for Store {
    fn resolve(&self, provider: &Identifier, name: &Name) -> Result<SecretValue, ResolveError> {
        let key = (provider.as_str().to_owned(), name.as_str().to_owned());
        self.asked.lock().unwrap().push(key.clone());
        match self.values.lock().unwrap().get(&key) {
            Some(Ok(value)) => SecretValue::new(value.clone()).ok_or(ResolveError::Malformed),
            Some(Err(error)) => Err(error.clone()),
            None => Err(ResolveError::NotFound),
        }
    }
}

/// A name resolver answering each host from a queue: each question takes the next answer, and the last one
/// stays. Every question is kept.
#[derive(Clone, Default)]
struct Names {
    answers: Arc<Mutex<BTreeMap<String, VecDeque<Vec<IpAddr>>>>>,
    asked: Arc<Mutex<Vec<String>>>,
    /// Hosts whose lookup never answers, as a resolver that hangs: each waits until the test releases them (or
    /// 20 seconds pass), then answers nothing.
    hanging: Arc<(Mutex<Option<String>>, std::sync::Condvar)>,
}

impl Names {
    fn then(&self, host: &str, addresses: &[&str]) -> &Self {
        self.answers
            .lock()
            .unwrap()
            .entry(host.to_owned())
            .or_default()
            .push_back(
                addresses
                    .iter()
                    .map(|a| a.parse().expect("an address"))
                    .collect(),
            );
        self
    }

    /// Makes every lookup of `host` hang.
    fn hang(&self, host: &str) {
        *self.hanging.0.lock().unwrap() = Some(host.to_owned());
    }

    /// Ends every hanging lookup.
    fn release(&self) {
        *self.hanging.0.lock().unwrap() = None;
        self.hanging.1.notify_all();
    }

    fn asked(&self, host: &str) -> usize {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .filter(|h| *h == host)
            .count()
    }
}

impl NameResolver for Names {
    fn resolve(&self, host: &Host, _: NonZeroU16) -> Vec<IpAddr> {
        self.asked.lock().unwrap().push(host.as_str().to_owned());
        let (gate, released) = &*self.hanging;
        let mut hanging = gate.lock().unwrap();
        let started = std::time::Instant::now();
        while hanging.as_deref() == Some(host.as_str())
            && started.elapsed() < Duration::from_secs(20)
        {
            hanging = released
                .wait_timeout(hanging, Duration::from_millis(200))
                .unwrap()
                .0;
        }
        if hanging.as_deref() == Some(host.as_str()) {
            return Vec::new();
        }
        drop(hanging);
        let mut answers = self.answers.lock().unwrap();
        let Some(queue) = answers.get_mut(host.as_str()) else {
            return Vec::new();
        };
        if queue.len() > 1 {
            queue.pop_front().unwrap_or_default()
        } else {
            queue.front().cloned().unwrap_or_default()
        }
    }
}

/// The production world as deployed, with the tests' seams: the store, the names behind the threaded resolver that
/// bounds each lookup by the deadline, and the dialer under the TLS connector.
type World = ProductionWorld<Store, ThreadedResolver<Names>, TlsConnector<tls::MappedDialer>>;

/// A running platform over the production world, and the catalogue it admits against.
struct Running {
    platform: Platform<World>,
    catalogue: Catalogue,
    secrets: Store,
    keys: Store,
    names: Names,
    dialer: tls::MappedDialer,
}

impl Running {
    /// The platform over a proxy whose one role holds both providers' keys in `secrets`, the egress layer over
    /// `names` and a `TlsConnector` trusting only `pki`'s authority, dialling through `dialer`.
    fn new(pki: &tls::Pki, dialer: tls::MappedDialer) -> Self {
        let mut catalogue = Catalogue::new();
        catalogue.add_public_node("polygon").expect("a public node");
        for (i, ad) in [reserve_advertisement(), ramp_advertisement()]
            .iter()
            .enumerate()
        {
            catalogue
                .grant(grant(
                    &format!("g-{i}"),
                    &ad.provider,
                    tuples_of(ad),
                    4_000_000_000,
                ))
                .expect("a grant");
            join_made_at(&mut catalogue, ad, Instant(AT.0 - 86_400), AT)
                .expect("the advertisement joins");
        }
        let settings = ProxySettings::from_json(
            &json!({
                "deployment": deployment(),
                "roles": {
                    "integrations": {
                        "server": "openbao",
                        "store": { "scheme": "https", "host": "bao.internal", "port": 8200 },
                        "ca_file": "/etc/rutent/bao-ca.pem",
                        "mount": "secret",
                        "secrets": "rutent/integrations/secrets",
                        "keys": "rutent/integrations/keys"
                    }
                },
                "providers": {
                    RESERVE: "integrations",
                    RAMP: "integrations",
                    support::POLYGON_WALLET: "integrations"
                }
            })
            .to_string(),
        )
        .expect("valid settings");
        let (secrets, keys, names) = (Store::default(), Store::default(), Names::default());
        // Each provider's statements key, under its public half, as the deployment stores it: the platform signs
        // every result with it, through the proxy.
        for provider in [RESERVE, RAMP, support::POLYGON_WALLET] {
            let key = statements_key(provider);
            keys.holds(
                provider,
                public_key(&key).as_str(),
                Ok(&base64url(key.as_bytes())),
            );
        }
        let proxy = CredentialProxy::new(&settings, |_, _| {
            Ok::<_, InvalidSettings>(Bucket {
                secrets: secrets.clone(),
                keys: keys.clone(),
            })
        })
        .expect("a proxy");
        let connector = TlsConnector::with_dialer(&pki.roots_pem(), dialer.clone())
            .expect("a connector")
            .with_timeout(Duration::from_secs(5));
        let mut world =
            ProductionWorld::new(proxy, ThreadedResolver::new(names.clone()), connector);
        for provider in [RESERVE, RAMP] {
            world
                .advertise(
                    catalogue
                        .accepted_advertisement(provider)
                        .expect("an accepted advertisement")
                        .clone(),
                )
                .expect("the deployment's advertisement");
        }
        let mut platform = Platform::new(
            common::paths(),
            world,
            types::Envelope {
                deployment: deployment(),
                protocol_version: VERSION.to_owned(),
            },
        )
        .expect("a platform");
        platform.set_output_limit(4096);
        // Upgrades run at once here; a test that stages one sets its own staging.
        platform.set_staging(None);
        Running {
            platform,
            catalogue,
            secrets,
            keys,
            names,
            dialer,
        }
    }

    /// Admits and accepts a release of `declaration` whose outside part takes `steps`, signed by its provider.
    fn install(&mut self, declaration: &Declaration, steps: Vec<Step>) {
        self.install_probing(declaration, steps, &[]);
    }

    /// [`Running::install`], with the tests' own `probes` after the steps.
    fn install_probing(&mut self, declaration: &Declaration, steps: Vec<Step>, probes: &[Probe]) {
        let provider = declaration.provider.as_str();
        let spec = ModuleSpec {
            declaration: declaration.clone(),
            steps,
            output: Output::Honest,
            starts: true,
        };
        let binaries = common::build_standin(&spec, &language(), "reserve", probes)
            .expect("the stand-in builds");
        let received = signed::assertion(declaration, &binaries, &advertising_key(provider));
        let Admitted::Artefact(artefact) =
            self.platform
                .admit(&received, &binaries, &mut self.catalogue, AT)
        else {
            panic!("{provider}'s release wasn't admitted");
        };
        if artefact.acceptance.accepted.is_none() {
            self.platform
                .accept(
                    provider,
                    &artefact.assertion_digest,
                    "test-reviewer",
                    &self.catalogue,
                    AT,
                )
                .expect("the release is accepted");
        }
        assert!(
            self.platform.world.take_unadmitted().is_empty(),
            "the proxy didn't admit {provider}'s running release"
        );
    }

    /// Hands `provider` its withdrawal `n`: how it ended, and what it set off.
    fn run(&mut self, provider: &str, n: usize) -> (Handled, Events) {
        self.platform.take_events();
        let handled = self
            .platform
            .handle(provider, &common::withdrawal(provider, n));
        (handled, self.platform.take_events())
    }
}

/// The language the stand-ins are built in: Rust, if the tests run it.
fn language() -> String {
    let languages = common::languages();
    if languages.iter().any(|l| l == "rs") {
        "rs".to_owned()
    } else {
        languages[0].clone()
    }
}

fn id(text: &str) -> Identifier {
    Identifier::new(text).expect("an identifier")
}

fn api(provider: &str) -> String {
    format!("api.{provider}.example")
}

fn pinned(provider: &str) -> String {
    format!("pinned.{provider}.example")
}

/// `provider`'s release: the outside operation `api.call`, first, so its stand-in serves it, reaching `api` (any
/// public address) and `pinned` (only `185.12.64.10`) with the secret `own.key`; and `admin.call` with
/// `admin.key`. It may call each of `callees` for withdrawals, and declares its provider's statements key.
fn release(provider: &str, callees: &[&str]) -> Declaration {
    let destination = |name: &str, host: String, addresses: serde_json::Value| json!({ "name": name, "scheme": "https", "host": host, "port": 443, "addresses": addresses });
    serde_json::from_value(json!({
        "provider": provider,
        "version": "1.0.0",
        "interface": rutent_contracts::module_interface::VERSION,
        "sandboxed": { "handles": ["withdrawal"] },
        "outside": {
            "operations": [
                { "name": "api.call", "destinations": ["api", "pinned"], "secrets": ["own.key"] },
                { "name": "admin.call", "destinations": [], "secrets": ["admin.key"] },
            ],
            "destinations": [
                destination("api", api(provider), json!("public")),
                destination("pinned", pinned(provider), json!(["185.12.64.10/32"])),
            ],
            "secrets": ["own.key", "admin.key"],
            "calls": callees
                .iter()
                .map(|c| json!({ "callee": c, "kinds": ["withdrawal"] }))
                .collect::<Vec<_>>(),
            "statement_key": public_key(&statements_key(provider)),
        },
    }))
    .expect("a valid declaration")
}

fn call(host: &str) -> Step {
    Step::Call {
        operation: "api.call".to_owned(),
        endpoint: Endpoint::https(host),
    }
}

fn look_up(part: PartName, operation: &str, name: &str) -> Step {
    Step::LookUpSecret {
        part,
        operation: operation.to_owned(),
        name: name.to_owned(),
    }
}

fn at(address: &str) -> SocketAddr {
    SocketAddr::new(address.parse().expect("an address"), 443)
}

/// `B-17` on the platform's path: the name of a declared destination is resolved once per call, and the TLS
/// connection goes to the address checked, never to the one the name re-resolves to afterwards. A pinned
/// destination whose name resolves outside its block is refused, and that address is never dialled.
// verifies: REQ-MDL-6, REQ-MDL-7, REQ-MDL-8, THR-79
#[test]
fn running_platform_resolves_a_destination_once_and_dials_only_the_checked_address() {
    let pki = tls::Pki::new();
    let checked = tls::TlsServer::serve(pki.server_for(&api(RESERVE)));
    let rebound = tls::TlsServer::serve(pki.server_for(&api(RESERVE)));
    let outside_block = tls::TlsServer::serve(pki.server_for(&pinned(RESERVE)));
    let dialer = tls::MappedDialer::default()
        .with("185.12.64.21", &checked)
        .with("185.12.64.22", &rebound)
        .with("185.12.64.23", &outside_block);
    let mut running = Running::new(&pki, dialer);
    // The first answer is checked and dialled; any later question would get the rebound address.
    running
        .names
        .then(&api(RESERVE), &["185.12.64.21"])
        .then(&api(RESERVE), &["185.12.64.22"])
        .then(&pinned(RESERVE), &["185.12.64.23"]);
    running.secrets.holds(RESERVE, "own.key", Ok("canary-own"));
    running.install(
        &release(RESERVE, &[]),
        vec![
            look_up(PartName::Outside, "api.call", "own.key"),
            call(&api(RESERVE)),
            call(&pinned(RESERVE)),
        ],
    );

    let (handled, events) = running.run(RESERVE, 1);

    assert!(matches!(handled, Handled::Answered(_)), "{handled:?}");
    assert_eq!(
        running.names.asked(&api(RESERVE)),
        1,
        "resolved once for the call"
    );
    assert_eq!(running.names.asked(&pinned(RESERVE)), 1);
    assert_eq!(
        running.dialer.dialled(),
        vec![at("185.12.64.21")],
        "only the checked address was dialled"
    );
    assert_eq!(
        checked.requests().len(),
        1,
        "the checked address received the call"
    );
    assert!(
        checked.requests()[0]
            .starts_with(&format!("POST / HTTP/1.0\r\nHost: {}\r\n", api(RESERVE))),
        "{}",
        checked.requests()[0]
    );
    assert_eq!((rebound.accepted(), outside_block.accepted()), (0, 0));
    assert_eq!(events.records.len(), 2, "{:?}", events.records);
    let (first, second) = (&events.records[0], &events.records[1]);
    assert!(
        matches!(first.outcome, CallOutcome::Completed(_)),
        "{first:?}"
    );
    assert_eq!(first.hops.len(), 1);
    assert_eq!(first.hops[0].address, Address::new("185.12.64.21"));
    assert_eq!(first.transaction.as_str(), format!("tx-{RESERVE}-1"));
    assert_eq!(first.secrets, vec![Name::new("own.key").expect("a name")]);
    assert_eq!(
        second.outcome,
        CallOutcome::Refused(Refused {
            reason: RefusalReason::AddressNotAllowed
        })
    );
    assert_eq!(second.hops[0].address, None);
    for record in &events.records {
        assert_eq!(validate_record(record), Ok(()), "{record:?}");
        assert_eq!(
            validate_record_secrets(&release(RESERVE, &[]), record),
            Ok(()),
            "{record:?}"
        );
    }
}

/// `B-18` end to end: the platform's outbound call to a declared destination whose server is certified for
/// another name, and one to a server offering no certificate, are refused as `peer_not_verified`, recorded
/// with no address, and neither server reads a byte of the request. The certificates are generated here.
// verifies: REQ-MDL-6, REQ-MDL-7, THR-79
#[test]
fn tls_check_against_a_wrong_name_or_no_certificate_is_refused_end_to_end() {
    let pki = tls::Pki::new();
    let impostor = tls::TlsServer::serve(pki.server_for("collector.attacker.example"));
    let bare = tls::TlsServer::serve(tls::server_with_no_certificate());
    let dialer = tls::MappedDialer::default()
        .with("185.12.64.31", &impostor)
        .with("185.12.64.32", &bare);
    let mut running = Running::new(&pki, dialer);
    running
        .names
        .then(&api(RESERVE), &["185.12.64.31"])
        .then(&api(RESERVE), &["185.12.64.32"]);
    running.install(
        &release(RESERVE, &[]),
        vec![call(&api(RESERVE)), call(&api(RESERVE))],
    );

    let (handled, events) = running.run(RESERVE, 1);

    assert!(matches!(handled, Handled::Answered(_)), "{handled:?}");
    assert_eq!(
        running.dialer.dialled(),
        vec![at("185.12.64.31"), at("185.12.64.32")],
        "each dialled once, never retried"
    );
    assert_eq!(events.records.len(), 2, "{:?}", events.records);
    for record in &events.records {
        assert_eq!(
            record.outcome,
            CallOutcome::Refused(Refused {
                reason: RefusalReason::PeerNotVerified
            }),
            "{record:?}"
        );
        assert_eq!(record.hops.len(), 1);
        assert_eq!(record.hops[0].destination, Name::new("api"));
        assert_eq!(record.hops[0].address, None, "no verified connection");
        assert_eq!(validate_record(record), Ok(()));
    }
    assert_eq!((impostor.accepted(), bare.accepted()), (1, 1));
    assert_eq!((impostor.handshakes(), bare.handshakes()), (0, 0));
    assert_eq!(
        impostor.bytes_received() + bare.bytes_received(),
        0,
        "no request reaches an unverified peer"
    );
}

/// The proxy answers a secret only inside the outside request in flight. Each step of the module is an outside
/// request of its own: the store is asked for `own.key` inside `api.call`'s request and for `admin.key` inside
/// `admin.call`'s, each for that operation's transaction, and never for a lookup that names another operation
/// than the one in flight, or for the sandboxed part. Every lookup the platform served leaves one use in the
/// proxy, and when each operation ends nothing is left in flight. A store that can't answer is refused as
/// `store_unavailable`, never `not_found`, and the next operation uses the rotated value.
// verifies: REQ-MDL-17, REQ-MDL-9, REQ-MDL-6
#[test]
fn secret_is_answered_only_inside_the_outside_request_in_flight_on_the_running_platform() {
    let pki = tls::Pki::new();
    let mut running = Running::new(&pki, tls::MappedDialer::default());
    running
        .secrets
        .holds(RESERVE, "admin.key", Ok("canary-admin"));
    // The forger names `admin.call` while `api.call`'s request is in flight.
    let forge = Probe::Forge {
        claimed: "admin.call".to_owned(),
        secret: "admin.key".to_owned(),
        endpoint: Endpoint::https(&api(RESERVE)),
    };
    running.install_probing(
        &release(RESERVE, &[]),
        vec![
            look_up(PartName::Outside, "api.call", "own.key"),
            look_up(PartName::Outside, "admin.call", "admin.key"),
            look_up(PartName::Sandboxed, "api.call", "own.key"),
        ],
        &[forge],
    );
    let provider = id(RESERVE);
    let stored = [
        Ok("first-value"),
        Err(ResolveError::Unavailable { status: 503 }),
        Ok("rotated-value"),
    ];

    for (n, value) in stored.into_iter().enumerate() {
        let n = n + 1;
        running.secrets.holds(RESERVE, "own.key", value.clone());
        let asked_before = running.secrets.asked().len();
        let (handled, events) = running.run(RESERVE, n);
        let transaction = format!("tx-{RESERVE}-{n}");

        let Handled::Answered(result) = handled else {
            panic!("operation {n} wasn't answered: {handled:?}");
        };
        // The store was asked once for each request's own secret, and not for the forged lookup.
        assert_eq!(
            running.secrets.asked()[asked_before..],
            [
                (RESERVE.to_owned(), "own.key".to_owned()),
                (RESERVE.to_owned(), "admin.key".to_owned()),
            ],
            "operation {n}"
        );
        let outside: Vec<_> = events
            .lookups
            .iter()
            .filter(|e| e.part == PartName::Outside)
            .collect();
        assert_eq!(outside.len(), 3, "{outside:?}");
        let expected = match &value {
            Ok(_) => LookupOutcome::Resolved,
            Err(_) => LookupOutcome::Refused(SecretRefusalReason::StoreUnavailable),
        };
        let seen: Vec<_> = outside
            .iter()
            .map(|e| {
                (
                    e.lookup.operation.as_str(),
                    e.lookup.name.as_str(),
                    e.outcome,
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                ("api.call", "own.key", Some(expected)),
                ("admin.call", "admin.key", Some(LookupOutcome::Resolved)),
                (
                    "api.call",
                    "admin.key",
                    Some(LookupOutcome::Refused(
                        SecretRefusalReason::UndeclaredSecret
                    ))
                ),
            ],
            "operation {n}"
        );
        assert!(
            events
                .lookups
                .iter()
                .filter(|e| e.part == PartName::Sandboxed)
                .all(|e| e.outcome.is_none()),
            "the sandboxed part's lookup reached the proxy"
        );
        // The statement key's use first, resolved once the operation began and before the module was handed it,
        // to sign the result; then one use per lookup served, in order, with the lookup's transaction and
        // operation.
        let mut uses = running.platform.world.proxy().take_uses();
        assert!(!uses.is_empty(), "no use was recorded");
        let signing = uses.remove(0);
        assert_eq!(
            signing.of,
            UseOf::StatementKey(Some(public_key(&statements_key(RESERVE))))
        );
        assert_eq!(signing.transaction.as_str(), transaction);
        assert_eq!(signing.outcome, Ok(()));
        assert_eq!(uses.len(), outside.len(), "{uses:?}");
        for (used, event) in uses.iter().zip(&outside) {
            assert_eq!(used.provider, provider);
            assert_eq!(used.transaction.as_str(), transaction);
            assert_eq!(used.transaction, event.lookup.transaction);
            assert_eq!(used.operation.as_ref(), Some(&event.lookup.operation));
            assert_eq!(used.of, UseOf::Secret(event.lookup.name.clone()));
            assert_eq!(
                used.outcome.is_ok(),
                event.outcome == Some(LookupOutcome::Resolved)
            );
        }
        // Nothing is left in flight once the operation ends.
        assert_eq!(running.platform.world.in_flight(&provider), None);
        // The result is signed with the declared statements key, which the module never held.
        assert_eq!(result.signature.key, public_key(&statements_key(RESERVE)));
        assert_eq!(
            rutent_conformance::signing::verify(&serde_json::to_vec(&*result).expect("JSON")),
            Ok(())
        );
        let reference = serde_json::to_value(&result.statement).expect("JSON")["outcome"]["evidence"]
            [0]["reference"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        match value {
            Ok(value) => assert!(reference.contains(value), "{reference}"),
            Err(_) => assert!(!reference.contains("-value"), "{reference}"),
        }
    }
}

/// A call back into a provider already running is refused on purpose: A calls B, B calls back into A, and the
/// call back is refused as `callee_unavailable`. A's outside request stays in flight in the proxy through B's
/// call, so A's own lookup after it resolves, and B's lookups are B's own.
// verifies: REQ-MDL-21, REQ-MDL-9
#[test]
fn call_back_into_a_running_provider_leaves_its_request_in_flight_in_the_proxy() {
    let pki = tls::Pki::new();
    let mut running = Running::new(&pki, tls::MappedDialer::default());
    running.secrets.holds(RESERVE, "own.key", Ok("reserve-own"));
    running.secrets.holds(RAMP, "own.key", Ok("ramp-own"));
    running.install(
        &release(RESERVE, &[RAMP]),
        vec![
            Step::CallModule {
                callee: RAMP.to_owned(),
                kind: OperationKind::Withdrawal,
            },
            look_up(PartName::Outside, "api.call", "own.key"),
        ],
    );
    running.install(
        &release(RAMP, &[RESERVE]),
        vec![
            look_up(PartName::Outside, "api.call", "own.key"),
            Step::CallModule {
                callee: RESERVE.to_owned(),
                kind: OperationKind::Withdrawal,
            },
        ],
    );
    let transaction = id(&format!("tx-{RESERVE}-1"));
    let segment = RouteReference {
        decision_record: id("decision-1"),
        segment: id("segment-1"),
    };
    for (caller, callee) in [(RESERVE, RAMP), (RAMP, RESERVE)] {
        running.platform.routes.name(
            &transaction,
            NamedCall {
                route: segment.clone(),
                caller: id(caller),
                callee: id(callee),
                kinds: vec![OperationKind::Withdrawal],
            },
        );
    }

    let (handled, events) = running.run(RESERVE, 1);

    assert!(matches!(handled, Handled::Answered(_)), "{handled:?}");
    assert_eq!(events.calls.len(), 2, "{:?}", events.calls);
    assert!(
        matches!(
            &events.calls[0].outcome,
            CrossModuleOutcome::Refused(r) if r.reason == CallRefusalReason::CalleeUnavailable
        ),
        "{:?}",
        events.calls[0]
    );
    assert!(
        matches!(events.calls[1].outcome, CrossModuleOutcome::Delivered(_)),
        "{:?}",
        events.calls[1]
    );
    let resolved: Vec<_> = events
        .lookups
        .iter()
        .map(|e| (e.provider.as_str(), e.outcome))
        .collect();
    assert_eq!(
        resolved,
        vec![
            (RAMP, Some(LookupOutcome::Resolved)),
            (RESERVE, Some(LookupOutcome::Resolved)),
        ]
    );
    let uses = running.platform.world.proxy().take_uses();
    let secrets: Vec<_> = uses
        .iter()
        .filter(|u| matches!(u.of, UseOf::Secret(_)))
        .map(|u| (u.provider.as_str(), u.outcome.is_ok()))
        .collect();
    assert_eq!(secrets, vec![(RAMP, true), (RESERVE, true)], "{uses:?}");
    // Each result, the callee's and the caller's, was signed with its own provider's key, each resolved before its
    // module was handed the operation: the caller's first, then the callee's.
    let signers: Vec<_> = uses
        .iter()
        .filter(|u| matches!(u.of, UseOf::StatementKey(_)))
        .map(|u| (u.provider.as_str(), u.outcome.is_ok()))
        .collect();
    assert_eq!(signers, vec![(RESERVE, true), (RAMP, true)], "{uses:?}");
    assert_eq!(
        running.secrets.asked(),
        vec![
            (RAMP.to_owned(), "own.key".to_owned()),
            (RESERVE.to_owned(), "own.key".to_owned()),
        ]
    );
    for provider in [RESERVE, RAMP] {
        assert_eq!(running.platform.world.in_flight(&id(provider)), None);
    }
    running.platform.routes.end(&transaction);
}

/// A declared destination whose name resolves to nothing records its own reason, `address_not_allowed`, with no
/// address, and a record the contract accepts; one whose checked address can't be reached is `unknown`, with no
/// address, which the contract accepts since 4.0.0. Before, the platform's no-route path recorded `failed`, which
/// `validate_record` refused.
// verifies: REQ-MDL-8, REQ-MDL-7
#[test]
fn call_with_no_route_records_its_own_reason_and_a_valid_record() {
    let pki = tls::Pki::new();
    let mut running = Running::new(&pki, tls::MappedDialer::default());
    running.names.then(&pinned(RESERVE), &["185.12.64.10"]);
    running.install(
        &release(RESERVE, &[]),
        vec![call(&api(RESERVE)), call(&pinned(RESERVE))],
    );

    let (_, events) = running.run(RESERVE, 1);

    assert_eq!(events.records.len(), 2, "{:?}", events.records);
    assert_eq!(
        events.records[0].outcome,
        CallOutcome::Refused(Refused {
            reason: RefusalReason::AddressNotAllowed
        })
    );
    assert!(
        matches!(events.records[1].outcome, CallOutcome::Unknown(_)),
        "{:?}",
        events.records[1]
    );
    assert_eq!(running.dialer.dialled(), vec![at("185.12.64.10")]);
    for record in &events.records {
        assert_eq!(record.hops[0].address, None);
        assert_eq!(validate_record(record), Ok(()), "{record:?}");
    }
}

/// The platform signs a result only with the declared statement key, resolved through the proxy: a store holding
/// another key under the declared one's name, or none, signs nothing. Either is judged before the module is handed
/// the operation: another key is `StatementKeyMisconfigured`, and a store that can't answer is
/// `StatementKeyUnavailable`, which may pass. Neither is `Unavailable`.
// verifies: REQ-MDL-6, THR-4
#[test]
fn result_is_passed_on_only_signed_with_the_declared_statement_key() {
    let pki = tls::Pki::new();
    let mut running = Running::new(&pki, tls::MappedDialer::default());
    running.install(&release(RESERVE, &[]), Vec::new());
    let declared = public_key(&statements_key(RESERVE));

    let (handled, _) = running.run(RESERVE, 1);
    let Handled::Answered(result) = handled else {
        panic!("{handled:?}");
    };
    assert_eq!(result.signature.key, declared);
    assert_eq!(
        rutent_conformance::signing::verify(&serde_json::to_vec(&*result).expect("JSON")),
        Ok(())
    );

    // Another key, stored under the declared one's name.
    let other = advertising_key(RESERVE);
    running
        .keys
        .holds(RESERVE, declared.as_str(), Ok(&base64url(other.as_bytes())));
    let (handled, _) = running.run(RESERVE, 2);
    assert!(
        matches!(
            handled,
            Handled::Refused(types::PlatformRefusal::StatementKeyMisconfigured)
        ),
        "{handled:?}"
    );

    // No key at all: the store can't answer.
    running.keys.holds(
        RESERVE,
        declared.as_str(),
        Err(ResolveError::Unavailable { status: 503 }),
    );
    let (handled, _) = running.run(RESERVE, 3);
    assert!(
        matches!(
            handled,
            Handled::Refused(types::PlatformRefusal::StatementKeyUnavailable)
        ),
        "{handled:?}"
    );
}

/// A key store that can't give the statement key refuses the operation before the module is handed it
/// (`begin_operation`): no outbound call is made or recorded, no cross-module call is made or recorded, and the
/// callee is handed nothing. It is `StatementKeyUnavailable`, never `Unavailable`, and with the key back the same
/// module makes both calls.
// verifies: REQ-ORD-1, REQ-MDL-6, THR-4
#[test]
fn key_store_outage_refuses_the_operation_before_anything_of_it_runs() {
    let pki = tls::Pki::new();
    let server = tls::TlsServer::serve(pki.server_for(&api(RESERVE)));
    let dialer = tls::MappedDialer::default().with("185.12.64.40", &server);
    let mut running = Running::new(&pki, dialer);
    running.names.then(&api(RESERVE), &["185.12.64.40"]);
    running.install(
        &release(RESERVE, &[RAMP]),
        vec![
            call(&api(RESERVE)),
            Step::CallModule {
                callee: RAMP.to_owned(),
                kind: OperationKind::Withdrawal,
            },
        ],
    );
    running.install(&release(RAMP, &[]), Vec::new());
    running.platform.routes.name(
        &id(&format!("tx-{RESERVE}-1")),
        NamedCall {
            route: RouteReference {
                decision_record: id("decision-1"),
                segment: id("segment-1"),
            },
            caller: id(RESERVE),
            callee: id(RAMP),
            kinds: vec![OperationKind::Withdrawal],
        },
    );
    let declared = public_key(&statements_key(RESERVE));
    running.keys.holds(
        RESERVE,
        declared.as_str(),
        Err(ResolveError::Unavailable { status: 503 }),
    );

    let (handled, events) = running.run(RESERVE, 1);

    assert_eq!(
        handled,
        Handled::Refused(types::PlatformRefusal::StatementKeyUnavailable)
    );
    assert!(events.records.is_empty(), "{:?}", events.records);
    assert!(events.calls.is_empty(), "{:?}", events.calls);
    assert!(running.dialer.dialled().is_empty(), "something was dialled");
    assert!(server.requests().is_empty(), "the endpoint got a request");
    assert_eq!(running.platform.operations_received(RAMP), 0);
    assert!(
        running
            .secrets
            .asked()
            .iter()
            .all(|(provider, _)| provider != RESERVE),
        "the module ran and looked a secret up"
    );

    // The control: with the key back, the same operation makes both calls.
    running.keys.holds(
        RESERVE,
        declared.as_str(),
        Ok(&base64url(statements_key(RESERVE).as_bytes())),
    );
    let (handled, events) = running.run(RESERVE, 1);
    assert!(matches!(handled, Handled::Answered(_)), "{handled:?}");
    assert_eq!(events.records.len(), 1, "{:?}", events.records);
    assert!(
        matches!(
            events.calls.first().map(|c| &c.outcome),
            Some(CrossModuleOutcome::Delivered(_))
        ),
        "{:?}",
        events.calls
    );
}

/// A callee that runs past its deadline (its outbound call's name lookup hangs) leaves its caller's call
/// unanswered, and the caller's module is told `unanswered`, never `refused`: the callee may have acted. The
/// callee's deadline ends before its caller's, so the caller hears it and still answers its own operation.
// verifies: REQ-ORD-1, REQ-MDL-21, REQ-MDL-11
#[test]
fn callee_past_its_deadline_is_unanswered_to_its_caller_never_refused() {
    let pki = tls::Pki::new();
    let mut running = Running::new(&pki, tls::MappedDialer::default());
    running.platform.set_deadline(Duration::from_secs(2));
    running.platform.set_call_budget(Duration::from_secs(1));
    running.names.hang(&api(RAMP));
    running.install_probing(
        &release(RESERVE, &[RAMP]),
        Vec::new(),
        &[Probe::Ask {
            callee: RAMP.to_owned(),
            kind: OperationKind::Withdrawal,
        }],
    );
    running.install(&release(RAMP, &[]), vec![call(&api(RAMP))]);
    running.platform.routes.name(
        &id(&format!("tx-{RESERVE}-1")),
        NamedCall {
            route: RouteReference {
                decision_record: id("decision-1"),
                segment: id("segment-1"),
            },
            caller: id(RESERVE),
            callee: id(RAMP),
            kinds: vec![OperationKind::Withdrawal],
        },
    );
    let overdue = running.platform.measurements.overdue;

    let (handled, events) = running.run(RESERVE, 1);
    running.names.release();

    assert_eq!(
        running.platform.measurements.overdue,
        overdue + 1,
        "the callee wasn't stopped at its deadline"
    );
    let Handled::Answered(result) = &handled else {
        panic!("the caller didn't answer: {handled:?}");
    };
    let reference = serde_json::to_value(&result.statement).expect("JSON")["outcome"]["evidence"]
        [0]["reference"]
        .clone();
    assert_eq!(
        reference,
        format!("{RESERVE}-1.0.0-unanswered"),
        "the caller's module was told another outcome"
    );
    assert_eq!(events.calls.len(), 1, "{:?}", events.calls);
    assert!(
        matches!(events.calls[0].outcome, CrossModuleOutcome::Unanswered(_)),
        "{:?}",
        events.calls[0]
    );
    assert_eq!(running.platform.operations_received(RAMP), 1);

    // The callee's process was killed at its deadline, and is started again once the caller's answer was handed
    // back: asked directly, it answers.
    let (direct, _) = running.run(RAMP, 2);
    assert!(
        matches!(direct, Handled::Answered(_)),
        "the callee wasn't started again: {direct:?}"
    );
}

/// Publishes the operator's `polygon` wallet, admits its release signed with the operator key and accepts it, and
/// checks that the proxy admitted it.
fn run_operator_wallet(running: &mut Running) {
    use support::{OPERATOR, POLYGON_WALLET, wallet_release};
    running
        .catalogue
        .publish_operator_module(POLYGON_WALLET)
        .expect("the operator publishes its wallet");
    let (declaration, binaries) = wallet_release(POLYGON_WALLET, "1.0.0");
    let received = signed::assertion_by(
        &declaration,
        &binaries,
        OPERATOR,
        &signed::catalogue_support::operator_key(),
    );
    let Admitted::Artefact(artefact) =
        running
            .platform
            .admit(&received, &binaries, &mut running.catalogue, AT)
    else {
        panic!("the operator's release wasn't admitted");
    };
    running
        .platform
        .accept(
            POLYGON_WALLET,
            &artefact.assertion_digest,
            "test-reviewer",
            &running.catalogue,
            AT,
        )
        .expect("the release is accepted");
    assert_eq!(running.platform.world.take_unadmitted(), Vec::new());
}

/// The operator's deposit `n` into its `polygon` wallet.
fn wallet_deposit(
    n: usize,
) -> rutent_contracts::provider_role::Statement<rutent_contracts::provider_role::Operation> {
    use rutent_prov::holding::wallet::Operator as _;
    use support::{POLYGON_WALLET, TestOperator};
    TestOperator.statement(
        serde_json::from_value(json!({
            "kind": "deposit",
            "transaction": format!("tx-o-{n}"),
            "provider": POLYGON_WALLET,
            "origin": "polygon",
            "destination": { "network": POLYGON_WALLET },
            "asset": "USDC",
            "amount": "5",
            "dry_run": false,
            "inputs": { "order": format!("o-{n}") },
        }))
        .expect("a valid operation"),
    )
}

/// Asks the operator's `polygon` wallet for deposit `n` and checks that its answer is passed on, signed with the
/// statement key it declares, resolved through the proxy.
fn assert_wallet_answers_signed(running: &mut Running, n: usize) {
    assert_wallet_answers_signed_with(
        running,
        n,
        &public_key(&statements_key(support::POLYGON_WALLET)),
    );
}

/// [`assert_wallet_answers_signed`], with the statement key `key`.
fn assert_wallet_answers_signed_with(
    running: &mut Running,
    n: usize,
    key: &rutent_contracts::provider_role::PublicKey,
) {
    let handled = running
        .platform
        .handle(support::POLYGON_WALLET, &wallet_deposit(n));
    let Handled::Answered(result) = handled else {
        panic!("the wallet's answer {n} wasn't passed on: {handled:?}");
    };
    assert_eq!(&result.signature.key, key, "answer {n}");
    assert_eq!(
        rutent_conformance::signing::verify(&serde_json::to_vec(&*result).expect("JSON")),
        Ok(())
    );
}

/// A staged release that declares a new statement key, rolled back (`T-013-06`): the proxy is told which release
/// handles each operation, so the version in force answers signed with its own key throughout, the staged one with
/// the new key, and after the rollback the version in force still signs with its own.
// verifies: REQ-MDL-4, REQ-MDL-6, THR-4
#[test]
fn rolled_back_release_with_a_new_statement_key_leaves_the_version_in_force_signing_with_its_own() {
    use rutent_prov::module::lifecycle::Staging;
    use support::{OPERATOR, POLYGON_WALLET, wallet_release};

    let pki = tls::Pki::new();
    let mut running = Running::new(&pki, tls::MappedDialer::default());
    run_operator_wallet(&mut running);
    assert_wallet_answers_signed(&mut running, 1);

    // 1.1.0 declares a new statement key, which the deployment stores; it is staged on every new segment.
    let new_key = signed::catalogue_support::signing_key(POLYGON_WALLET, 7);
    let new_public = public_key(&new_key);
    running.keys.holds(
        POLYGON_WALLET,
        new_public.as_str(),
        Ok(&base64url(new_key.as_bytes())),
    );
    running
        .platform
        .set_staging(Some(Staging::new(100, 0, 0).expect("a policy")));
    let (mut declaration, binaries) = wallet_release(POLYGON_WALLET, "1.1.0");
    declaration.outside.statement_key = new_public.clone();
    let received = signed::assertion_by(
        &declaration,
        &binaries,
        OPERATOR,
        &signed::catalogue_support::operator_key(),
    );
    let Admitted::Artefact(artefact) =
        running
            .platform
            .admit(&received, &binaries, &mut running.catalogue, AT)
    else {
        panic!("the wallet's 1.1.0 wasn't admitted");
    };
    if artefact.acceptance.accepted.is_none() {
        running
            .platform
            .accept(
                POLYGON_WALLET,
                &artefact.assertion_digest,
                "test-reviewer",
                &running.catalogue,
                AT,
            )
            .expect("accepted");
    }
    assert_eq!(
        running.platform.lifecycle().staged(POLYGON_WALLET),
        Some("1.1.0")
    );

    // While it is staged, the version in force signs with its own key and a segment on the staged one with the new.
    assert_wallet_answers_signed(&mut running, 2);
    running
        .platform
        .begin_segment(POLYGON_WALLET, &id("tx-o-3"))
        .expect("a segment on the staged release");
    assert_wallet_answers_signed_with(&mut running, 3, &new_public);
    assert_wallet_answers_signed(&mut running, 4);

    // Rolled back, the version in force still signs with its own key.
    running
        .platform
        .roll_back(POLYGON_WALLET, OPERATOR, AT)
        .expect("rolled back");
    running.platform.end_segment("tx-o-3").expect("its record");
    assert_wallet_answers_signed(&mut running, 5);
    assert_eq!(running.platform.world.take_unadmitted(), Vec::new());
}

/// No advertisement vouches for the keys of a module the operator publishes (`T-016-04`): one naming it, held by
/// the world before its release starts, doesn't decide how that release is admitted, and one offered while it runs
/// is refused. Either way its results stay signed with the key its own release declares.
// verifies: REQ-MDL-1, REQ-CAT-23, THR-4
#[test]
fn advertisement_naming_an_operators_module_never_judges_its_release() {
    use rutent_prov::proxy::Refusal;
    use support::POLYGON_WALLET;

    let pki = tls::Pki::new();
    let mut running = Running::new(&pki, tls::MappedDialer::default());
    // An advertisement under the wallet's name, listing a provider's keys and not the wallet's statement key.
    let mut impostor = running
        .catalogue
        .accepted_advertisement(RESERVE)
        .expect("the reserve's advertisement")
        .clone();
    impostor.statement.provider = POLYGON_WALLET.to_owned();
    impostor.maker = POLYGON_WALLET.to_owned();

    // Held before the wallet's release starts: the release is still admitted as the operator's own.
    running
        .platform
        .world
        .advertise(impostor.clone())
        .expect("no release of the wallet runs yet");
    run_operator_wallet(&mut running);
    assert_wallet_answers_signed(&mut running, 1);

    // Offered while it runs: refused, and the wallet's key is unchanged.
    assert_eq!(
        running.platform.world.advertise(impostor),
        Err(Refusal::AdvertisementOfAnOperatorsModule)
    );
    assert_wallet_answers_signed(&mut running, 2);
}

/// A holding account the operator publishes (`T-016-04`) runs on the production world like any module: the proxy
/// admits its release on the operator's own signature, with no advertisement, and resolves the statement key it
/// declares, so its results are signed and value lands in it.
// verifies: REQ-CAT-23, REQ-MDL-1, INV-5
#[test]
fn operator_wallets_results_are_signed_on_the_production_world() {
    use rutent_prov::holding::wallet::Holdings;
    use support::{POLYGON_WALLET, TestOperator};

    let pki = tls::Pki::new();
    let mut running = Running::new(&pki, tls::MappedDialer::default());
    run_operator_wallet(&mut running);

    // Its answer is signed with the statement key it declares, resolved through the proxy.
    assert_wallet_answers_signed(&mut running, 0);

    // As a node's holding account, it takes value landing at its node.
    let mut holdings = Holdings::default();
    holdings
        .register(
            &mut running.catalogue,
            &running.platform,
            "polygon",
            POLYGON_WALLET,
            "USDC",
        )
        .expect("the operator registers its wallet");
    holdings
        .land(
            &mut running.platform,
            &TestOperator,
            "polygon",
            "USDC",
            "o-1",
            "100",
        )
        .expect("the wallet takes the value");
    let readings = holdings.readings(&mut running.platform, &TestOperator);
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].1.as_ref().expect("a reading").balance, 105);
}

/// A peer that answers one byte at a time, each well within a read's own timeout, is cut off at the operation's
/// deadline (`REQ-MDL-11`): the platform doesn't wait on it past the deadline, the call is recorded `unknown`, and
/// another provider's operation runs right after. The withdrawal's call reached the peer before the deadline
/// passed, so the operation is an outcome unknown, never a refusal: the deadline it hit is recorded as a limit,
/// and as what ended it (`REQ-ORD-1`).
// verifies: REQ-MDL-11, REQ-MDL-7, REQ-MDL-8, REQ-ORD-1
#[test]
fn slow_peer_is_cut_off_at_the_operation_deadline_and_another_provider_still_runs() {
    let pki = tls::Pki::new();
    let slow = tls::TlsServer::dribbling(pki.server_for(&api(RESERVE)), Duration::from_millis(300));
    let dialer = tls::MappedDialer::default().with("185.12.64.41", &slow);
    let mut running = Running::new(&pki, dialer);
    running.platform.set_deadline(Duration::from_secs(2));
    running.names.then(&api(RESERVE), &["185.12.64.41"]);
    running.install(&release(RESERVE, &[]), vec![call(&api(RESERVE))]);
    running.install(&release(RAMP, &[]), Vec::new());

    let started = std::time::Instant::now();
    let (handled, events) = running.run(RESERVE, 1);
    let took = started.elapsed();

    assert_eq!(slow.requests().len(), 1, "the slow peer got the call");
    assert_eq!(
        handled,
        Handled::Refused(types::PlatformRefusal::OutcomeUnknown)
    );
    let deadline = types::LimitHit {
        kind: types::LimitKind::Deadline,
        scope: None,
    };
    assert_eq!(
        events.limits.iter().map(|l| l.hit).collect::<Vec<_>>(),
        vec![deadline],
        "the deadline wasn't recorded as the limit hit"
    );
    assert_eq!(events.limits[0].transaction, format!("tx-{RESERVE}-1"));
    assert_eq!(events.unknown.len(), 1, "{:?}", events.unknown);
    assert_eq!(
        events.unknown[0].ended_by,
        types::PlatformRefusal::LimitReached
    );
    assert!(took < Duration::from_secs(4), "held for {took:?}");
    assert_eq!(events.records.len(), 1, "{:?}", events.records);
    assert!(
        matches!(events.records[0].outcome, CallOutcome::Unknown(_)),
        "{:?}",
        events.records[0]
    );
    assert_eq!(validate_record(&events.records[0]), Ok(()));

    let started = std::time::Instant::now();
    let (handled, _) = running.run(RAMP, 1);
    assert!(matches!(handled, Handled::Answered(_)), "{handled:?}");
    assert!(started.elapsed() < Duration::from_secs(4));
}

/// A name lookup that never answers is given up at the operation's deadline (`REQ-MDL-11`): the platform doesn't
/// wait on the resolver past it, the call is recorded `unknown` with nothing dialled, and another provider's
/// operation runs right after. Nothing of the call was sent, no hop connected, so the operation ends as the limit
/// it hit, the deadline, which is recorded: an outcome unknown would claim an effect that couldn't have happened.
// verifies: REQ-MDL-11, REQ-MDL-7, REQ-MDL-8, REQ-ORD-1
#[test]
fn hanging_name_lookup_is_given_up_at_the_operation_deadline_and_another_provider_still_runs() {
    let pki = tls::Pki::new();
    let mut running = Running::new(&pki, tls::MappedDialer::default());
    running.platform.set_deadline(Duration::from_secs(2));
    running.names.hang(&api(RESERVE));
    running.install(&release(RESERVE, &[]), vec![call(&api(RESERVE))]);
    running.install(&release(RAMP, &[]), Vec::new());

    let started = std::time::Instant::now();
    let (handled, events) = running.run(RESERVE, 1);
    let took = started.elapsed();

    assert!(took < Duration::from_secs(4), "held for {took:?}");
    assert_eq!(
        handled,
        Handled::Refused(types::PlatformRefusal::LimitReached)
    );
    assert!(events.unknown.is_empty(), "{:?}", events.unknown);
    assert_eq!(
        events.limits.iter().map(|l| l.hit.kind).collect::<Vec<_>>(),
        vec![types::LimitKind::Deadline]
    );
    assert_eq!(events.records.len(), 1, "{:?}", events.records);
    assert!(
        matches!(events.records[0].outcome, CallOutcome::Unknown(_)),
        "{:?}",
        events.records[0]
    );
    assert_eq!(events.records[0].hops[0].address, None);
    assert_eq!(validate_record(&events.records[0]), Ok(()));
    assert!(running.dialer.dialled().is_empty(), "nothing was dialled");

    let started = std::time::Instant::now();
    let (handled, _) = running.run(RAMP, 1);
    assert!(matches!(handled, Handled::Answered(_)), "{handled:?}");
    assert!(started.elapsed() < Duration::from_secs(4));
    running.names.release();
}
