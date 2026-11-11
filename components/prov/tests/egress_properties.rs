//! Property tests of the egress layer's invariants (`DEC-18`), seeded so a failure names the seed that replays it.
//! Each case draws a declaration, the operation asked for, a resolver whose answers change from one question to
//! the next, and a connector answering each address with a success, a failure, a redirect anywhere, a failed TLS
//! check or no answer. No network is touched.

mod common;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex};

use common::{CASES, Rng};
use rutent_conformance::role::placeholder_key;
use rutent_contracts::module_interface::{
    Addresses, CallOutcome, Declaration, Host, Identifier, Name, SPECIAL_PURPOSE, Scheme, Time,
    validate_declaration, validate_record, validate_record_secrets,
};
use rutent_prov::egress::{
    Call, ConnectError, Connector, Dial, Egress, Endpoint, MAX_HOPS, Method, NameResolver, Outcome,
    Request, Response,
};
use serde_json::{Value, json};

/// The addresses a resolver may answer with: public ones inside and outside the declared blocks, and
/// special-purpose ones a rebinding attack would aim at.
const ADDRESSES: &[&str] = &[
    "93.184.216.34",
    "93.184.216.80",
    "8.8.8.8",
    "2606:4700::1111",
    "2001:4860::8888",
    "10.0.0.7",
    "127.0.0.1",
    "169.254.169.254",
    "192.168.1.1",
    "::1",
    "fd00::1",
    "::ffff:93.184.216.34",
    "64:ff9b::a9fe:a9fe",
];

const BLOCKS: &[&str] = &["93.184.216.0/24", "2606:4700::/32"];

/// One thing the doubles saw, in the order they saw it.
#[derive(Debug, Clone)]
enum Seen {
    Resolved(String, Vec<IpAddr>),
    Dialled {
        scheme: Scheme,
        name: String,
        to: SocketAddr,
    },
}

type Journal = Arc<Mutex<Vec<Seen>>>;

/// Answers each host with a fresh draw from its own seeded stream: the same name rarely resolves the same way
/// twice.
struct Rebinding {
    seed: u64,
    asked: Mutex<HashMap<String, u64>>,
    journal: Journal,
}

impl NameResolver for Rebinding {
    fn resolve(&self, host: &Host, _port: NonZeroU16) -> Vec<IpAddr> {
        let mut asked = self.asked.lock().unwrap();
        let n = asked.entry(host.as_str().to_owned()).or_default();
        *n += 1;
        let salt = host
            .as_str()
            .bytes()
            .fold(0u64, |h, b| h.wrapping_mul(31) ^ u64::from(b));
        let mut rng = Rng::new(self.seed ^ salt ^ (*n << 32));
        let addresses: Vec<IpAddr> = (0..rng.below(2))
            .chain(std::iter::once(0))
            .map(|_| {
                // Mostly public, the first five; a rebinding answer otherwise.
                let pool = if rng.chance(4) { ADDRESSES.len() } else { 5 };
                ADDRESSES[rng.below(pool as u64) as usize].parse().unwrap()
            })
            .collect();
        let addresses = if rng.chance(10) {
            Vec::new()
        } else {
            addresses
        };
        self.journal
            .lock()
            .unwrap()
            .push(Seen::Resolved(host.as_str().to_owned(), addresses.clone()));
        addresses
    }
}

/// Answers each dial with a draw from the case's stream.
struct Chaos {
    rng: Mutex<Rng>,
    locations: Vec<String>,
    journal: Journal,
}

impl Connector for Chaos {
    fn exchange(&self, dial: &Dial<'_>, _request: &Request) -> Result<Response, ConnectError> {
        self.journal.lock().unwrap().push(Seen::Dialled {
            scheme: dial.scheme,
            name: dial.name.as_str().to_owned(),
            to: dial.to,
        });
        let mut rng = self.rng.lock().unwrap();
        let answer = |status: u16, headers: Vec<(String, String)>| Response {
            status,
            headers,
            body: Vec::new(),
        };
        match rng.below(8) {
            0 => Ok(answer(200, Vec::new())),
            1 => Ok(answer(500, Vec::new())),
            2 => Err(ConnectError::NotVerified),
            3 => Err(ConnectError::NoAnswer),
            4 => Err(ConnectError::Unreachable),
            _ => {
                let status = [301, 302, 303, 307, 308, 300][rng.below(6) as usize];
                let location =
                    self.locations[rng.below(self.locations.len() as u64) as usize].clone();
                Ok(answer(status, vec![("Location".to_owned(), location)]))
            }
        }
    }
}

struct Case {
    declaration: Declaration,
    endpoints: Vec<Endpoint>,
    locations: Vec<String>,
}

fn scheme_text(scheme: Scheme) -> &'static str {
    match scheme {
        Scheme::Https => "https",
        Scheme::Wss => "wss",
    }
}

/// A declaration of up to four destinations on three hosts, and two operations listing random subsets of them.
fn case(rng: &mut Rng) -> Case {
    let hosts = ["a.acme.example", "b.acme.example", "c.acme.example"];
    let count = 1 + rng.below(4) as usize;
    let mut destinations = Vec::new();
    let mut endpoints = Vec::new();
    for i in 0..count {
        let scheme = if rng.chance(4) {
            Scheme::Wss
        } else {
            Scheme::Https
        };
        let host = hosts[rng.below(3) as usize];
        let port = if rng.chance(3) { 8443 } else { 443 };
        let addresses = if rng.chance(2) {
            json!("public")
        } else {
            json!([BLOCKS[rng.below(2) as usize]])
        };
        if endpoints.contains(&(scheme, host, port)) {
            continue;
        }
        endpoints.push((scheme, host, port));
        destinations.push(json!({
            "name": format!("d{i}"), "scheme": scheme_text(scheme), "host": host, "port": port,
            "addresses": addresses,
        }));
    }
    let names: Vec<Value> = destinations.iter().map(|d| d["name"].clone()).collect();
    let pick = |rng: &mut Rng| -> Vec<Value> {
        names.iter().filter(|_| !rng.chance(4)).cloned().collect()
    };
    let (first, second) = (pick(rng), pick(rng));
    let declaration: Declaration = serde_json::from_value(json!({
        "provider": "acme",
        "version": "3.0.0",
        "interface": rutent_contracts::module_interface::VERSION,
        "sandboxed": { "handles": ["withdrawal"] },
        "outside": {
            "operations": [
                { "name": "op0", "destinations": first, "secrets": ["s0"] },
                { "name": "op1", "destinations": second, "secrets": [] },
            ],
            "destinations": destinations,
            "secrets": ["s0"],
            "calls": [],
            "statement_key": placeholder_key(2),
        },
    }))
    .expect("a declaration");
    validate_declaration(&declaration).expect("a valid declaration");

    let mut all: Vec<Endpoint> = Vec::new();
    for scheme in [Scheme::Https, Scheme::Wss] {
        for host in hosts.iter().chain(&["evil.example"]) {
            for port in [443, 8443] {
                all.push(Endpoint {
                    scheme,
                    host: (*host).to_owned(),
                    port,
                });
            }
        }
    }
    let mut locations: Vec<String> = all
        .iter()
        .map(|e| format!("{}://{}:{}/next", scheme_text(e.scheme), e.host, e.port))
        .collect();
    locations.extend(
        [
            "/same",
            "http://a.acme.example/plain",
            "https://user@a.acme.example/",
            "//b.acme.example/scheme-relative",
            "not a url",
        ]
        .map(str::to_owned),
    );
    // Redirects to declared destinations too, more often, so chains are followed and re-checked.
    for d in &declaration.outside.destinations {
        let url = format!(
            "{}://{}:{}/declared",
            scheme_text(d.scheme),
            d.host.as_str(),
            d.port
        );
        locations.extend([url.clone(), url, "/same".to_owned()]);
    }
    Case {
        declaration,
        endpoints: all,
        locations,
    }
}

/// Whether `address` lies within `addresses`, as ADR-0004 states it, and in no special-purpose range.
fn allowed(addresses: &Addresses, address: IpAddr) -> bool {
    let within = |ip: IpAddr, block: IpAddr, len: u8| match (ip, block) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            len == 0 || (u32::from(a) ^ u32::from(b)) >> (32 - u32::from(len)) == 0
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            len == 0 || (u128::from(a) ^ u128::from(b)) >> (128 - u32::from(len)) == 0
        }
        _ => false,
    };
    let special = SPECIAL_PURPOSE
        .iter()
        .any(|(range, len)| within(address, range.parse().unwrap(), *len));
    !special
        && match addresses {
            Addresses::Public(_) => true,
            Addresses::Blocks(blocks) => blocks.iter().any(|b| {
                let (ip, len) = b.parts();
                within(address, ip, len)
            }),
        }
}

fn name(text: &str) -> Name {
    Name::new(text.to_owned()).unwrap()
}

/// Runs one seeded case: one call, through a rebinding resolver and a chaotic connector.
fn run(
    seed: u64,
) -> (
    Case,
    String,
    Vec<Seen>,
    Outcome,
    Vec<rutent_contracts::module_interface::OutboundCallRecord>,
) {
    let mut rng = Rng::new(seed);
    let case = case(&mut rng);
    let journal: Journal = Arc::default();
    let resolver = Rebinding {
        seed,
        asked: Mutex::default(),
        journal: journal.clone(),
    };
    let connector = Chaos {
        rng: Mutex::new(Rng::new(seed.wrapping_add(7))),
        locations: case.locations.clone(),
        journal: journal.clone(),
    };
    let egress = Egress::new(resolver, connector);
    let asked = if rng.chance(8) { "op1" } else { "op0" }.to_owned();
    let (in_flight, transaction) = (name("op0"), Identifier::new(format!("tx-{seed}")).unwrap());
    let secrets = [name("s0")];
    let started_at = Time::new("2026-11-09T12:00:00Z".to_owned()).unwrap();
    let call = Call {
        declaration: &case.declaration,
        in_flight: &in_flight,
        asked: &asked,
        transaction: &transaction,
        secrets: &secrets,
        started_at: &started_at,
        due: None,
    };
    // Mostly a destination the operation in flight lists, so the checks past the first are reached; sometimes any endpoint.
    let listed: Vec<_> = case.declaration.outside.operations[0].destinations.clone();
    let declared: Vec<_> = case
        .declaration
        .outside
        .destinations
        .iter()
        .filter(|d| listed.contains(&d.name))
        .collect();
    let endpoint = if declared.is_empty() || rng.chance(4) {
        case.endpoints[rng.below(case.endpoints.len() as u64) as usize].clone()
    } else {
        let d = &declared[rng.below(declared.len() as u64) as usize];
        Endpoint {
            scheme: d.scheme,
            host: d.host.as_str().to_owned(),
            port: d.port.get(),
        }
    };
    let request = Request::new(Method::Post, "/start", Vec::new(), b"{}".to_vec()).unwrap();
    let mut log = Vec::new();
    let outcome = egress.send(&call, endpoint, request, &mut log);
    let seen = journal.lock().unwrap().clone();
    (case, asked, seen, outcome, log)
}

/// Every connection is made to a destination the operation in flight lists, at an address the name resolved to
/// on the question just before it and that lies within the destination's addresses, however the name rebinds and
/// wherever the redirects point.
// verifies: REQ-MDL-7, THR-79
#[test]
fn connector_is_reached_only_at_a_listed_destination_and_the_address_just_checked() {
    for seed in 0..CASES {
        let (case, asked, seen, _, _) = run(seed);
        let outside = &case.declaration.outside;
        let listed: Vec<&Name> = outside
            .operations
            .iter()
            .filter(|o| o.name.as_str() == "op0" && asked == "op0")
            .flat_map(|o| &o.destinations)
            .collect();
        let dials = seen
            .iter()
            .filter(|s| matches!(s, Seen::Dialled { .. }))
            .count();
        assert!(dials <= MAX_HOPS, "seed {seed}: {dials} dials");
        for (i, event) in seen.iter().enumerate() {
            let Seen::Dialled { scheme, name, to } = event else {
                continue;
            };
            let destination = outside
                .destinations
                .iter()
                .find(|d| {
                    listed.contains(&&d.name)
                        && d.scheme == *scheme
                        && d.host.as_str() == name
                        && d.port.get() == to.port()
                })
                .unwrap_or_else(|| panic!("seed {seed}: dialled an unlisted {name}:{}", to.port()));
            let Some(Seen::Resolved(host, answer)) = i.checked_sub(1).map(|j| &seen[j]) else {
                panic!("seed {seed}: dial {i} not right after a resolution");
            };
            assert_eq!(host, name, "seed {seed}");
            assert_eq!(
                answer.first(),
                Some(&to.ip()),
                "seed {seed}: the checked address"
            );
            assert!(
                answer.iter().all(|a| allowed(&destination.addresses, *a)),
                "seed {seed}: {answer:?} not all within {:?}",
                destination.addresses
            );
        }
    }
}

/// Every call leaves exactly one record, which the contract's rules and the declaration accept, naming the
/// transaction and module version, with one hop per connection attempted, connected unless TLS failed.
// verifies: REQ-MDL-8, REQ-MDL-7
#[test]
fn every_call_leaves_one_record_its_declaration_accepts() {
    let mut kinds = std::collections::BTreeMap::new();
    for seed in 0..CASES {
        let (case, _, seen, outcome, log) = run(seed);
        assert_eq!(log.len(), 1, "seed {seed}");
        let record = &log[0];
        validate_record(record).unwrap_or_else(|e| panic!("seed {seed}: {e:?} {record:?}"));
        validate_record_secrets(&case.declaration, record)
            .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
        assert_eq!(record.transaction.as_str(), format!("tx-{seed}"));
        assert_eq!(record.module_version.as_str(), "3.0.0");
        assert_eq!(record.operation.as_str(), "op0");
        let dials = seen
            .iter()
            .filter(|s| matches!(s, Seen::Dialled { .. }))
            .count();
        let connected = record.hops.iter().filter(|h| h.address.is_some()).count();
        // A dial that failed its TLS check, or connected to nothing, leaves its hop with no address.
        let unconnected = usize::from(matches!(
            outcome,
            Outcome::Refused(rutent_prov::egress::Refusal::NotVerified)
                | Outcome::RefusedAfterDelivery(rutent_prov::egress::Refusal::NotVerified)
                | Outcome::Unreachable
        ));
        assert_eq!(connected + unconnected, dials, "seed {seed}: {record:?}");
        assert!(record.hops.len() <= MAX_HOPS, "seed {seed}");
        let status = match (&outcome, &record.outcome) {
            (Outcome::Completed(_), CallOutcome::Completed(_))
            | (Outcome::Failed(_), CallOutcome::Failed(_))
            | (Outcome::NoAnswer | Outcome::Unreachable, CallOutcome::Unknown(_)) => true,
            (
                Outcome::Refused(r) | Outcome::RefusedAfterDelivery(r),
                CallOutcome::Refused(refused),
            ) => r.reason() == refused.reason,
            _ => false,
        };
        // A refusal is told as one only when nothing connected: once a hop was sent the request, it is a refusal
        // after delivery, which the module is told as unknown (`REQ-ORD-1`).
        match &outcome {
            Outcome::Refused(_) => assert_eq!(connected, 0, "seed {seed}: {record:?}"),
            Outcome::RefusedAfterDelivery(_) => {
                assert!(connected > 0, "seed {seed}: {record:?}");
            }
            _ => {}
        }
        assert!(
            status,
            "seed {seed}: {outcome:?} recorded as {:?}",
            record.outcome
        );
        let kind = match &outcome {
            Outcome::Refused(r) | Outcome::RefusedAfterDelivery(r) => format!("{r:?}"),
            other => format!("{other:?}").split('(').next().unwrap().to_owned(),
        };
        *kinds.entry(kind).or_insert(0) += 1;
        if connected >= 2 {
            *kinds.entry("followed a redirect".to_owned()).or_insert(0) += 1;
        }
        if matches!(outcome, Outcome::RefusedAfterDelivery(_)) {
            *kinds
                .entry("refused after delivery".to_owned())
                .or_insert(0) += 1;
        }
    }
    // The cases reach every outcome, and redirects that were followed, so the properties are not held vacuously.
    for kind in [
        "Completed",
        "Failed",
        "NoAnswer",
        "Unreachable",
        "UndeclaredOperation",
        "UndeclaredDestination",
        "RedirectToUndeclaredDestination",
        "AddressNotAllowed",
        "NotVerified",
        "followed a redirect",
        "refused after delivery",
    ] {
        assert!(
            kinds.get(kind).is_some_and(|n| *n > 0),
            "no case reached {kind}: {kinds:?}"
        );
    }
}
