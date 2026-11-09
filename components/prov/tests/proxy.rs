//! Tests of the credential proxy's resolvers (`T-014-01`): resolve-by-name at call time against OpenBao and Vault,
//! with credentials keyed by provider (`DEC-9`, ADR-0004).
//!
//! No server runs here. Each resolver talks to a fake [`Transport`] that answers with recorded responses, from a
//! Vault 2.0.3 dev server and an OpenBao 2.4.1 dev server. `live_servers_resolve_and_rotate_a_secret_when_configured`
//! runs against real servers only when their addresses are set in the environment.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rutent_contracts::module_interface::{Identifier, Name};
use rutent_prov::proxy::{
    KvConfig, KvResolver, Request, ResolveError, Resolver, Response, Server, Token, Transport,
    TransportError,
};

const TOKEN: &str = "test-token-not-a-secret";

/// A fake server: answers each path with the response stored for it, 404 otherwise, and keeps every request.
#[derive(Default)]
struct Fake {
    answers: Mutex<HashMap<String, Response>>,
    seen: Mutex<Vec<Request>>,
}

impl Fake {
    fn answer(&self, path: &str, response: Response) {
        self.answers
            .lock()
            .unwrap()
            .insert(path.to_owned(), response);
    }

    fn seen(&self) -> Vec<Request> {
        self.seen.lock().unwrap().clone()
    }
}

impl Transport for Fake {
    fn get(&self, request: &Request) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push(request.clone());
        Ok(self
            .answers
            .lock()
            .unwrap()
            .get(request.path())
            .cloned()
            .unwrap_or_else(|| Response {
                status: 404,
                body: b"{\"errors\":[]}".to_vec(),
            }))
    }
}

/// A transport whose connection always fails.
struct Down;

impl Transport for Down {
    fn get(&self, _request: &Request) -> Result<Response, TransportError> {
        Err(TransportError::new("connection refused"))
    }
}

fn recorded(server: Server, fixture: &str) -> Vec<u8> {
    let directory = match server {
        Server::OpenBao => "openbao",
        Server::Vault => "vault",
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/proxy")
        .join(directory)
        .join(format!("{fixture}.json"));
    std::fs::read(&path).expect("fixture is readable")
}

fn response(server: Server, status: u16, fixture: &str) -> Response {
    Response {
        status,
        body: recorded(server, fixture),
    }
}

fn config() -> KvConfig {
    KvConfig::new("secret", "rutent", Token::new(TOKEN).unwrap()).unwrap()
}

fn resolver<T: Transport>(server: Server, transport: T) -> KvResolver<T> {
    match server {
        Server::OpenBao => KvResolver::openbao(config(), transport),
        Server::Vault => KvResolver::vault(config(), transport),
    }
}

fn provider(text: &str) -> Identifier {
    Identifier::new(text).unwrap()
}

fn name(text: &str) -> Name {
    Name::new(text).unwrap()
}

const SERVERS: [Server; 2] = [Server::OpenBao, Server::Vault];
const API_KEY_PATH: &str = "/v1/secret/data/rutent/bridge-pay/api-key";

// verifies: REQ-MDL-17
#[test]
fn secret_is_resolved_by_name_through_each_resolver() {
    for server in SERVERS {
        let fake = Arc::new(Fake::default());
        fake.answer(API_KEY_PATH, response(server, 200, "read-first"));
        let resolver = resolver(server, Arc::clone(&fake));

        let value = resolver
            .resolve(&provider("bridge-pay"), &name("api-key"))
            .unwrap_or_else(|e| panic!("{server:?}: {e}"));

        assert_eq!(value.as_str(), "first-key-value", "{server:?}");
        assert_eq!(resolver.server(), server);
        let seen = fake.seen();
        assert_eq!(seen.len(), 1, "{server:?}");
        assert_eq!(seen[0].path(), API_KEY_PATH, "{server:?}");
        let token = seen[0]
            .headers()
            .iter()
            .find(|h| h.name() == "X-Vault-Token")
            .expect("the token is sent");
        assert_eq!(token.value(), TOKEN);
        assert!(token.is_sensitive());
    }
}

// verifies: REQ-MDL-6
#[test]
fn rotated_secret_is_used_with_no_new_release() {
    for server in SERVERS {
        let fake = Arc::new(Fake::default());
        fake.answer(API_KEY_PATH, response(server, 200, "read-first"));
        // One resolver for the whole test: nothing is rebuilt, reconfigured or released between the two lookups.
        let resolver = resolver(server, Arc::clone(&fake));
        let before = resolver
            .resolve(&provider("bridge-pay"), &name("api-key"))
            .unwrap();

        // The operator rotates the secret in the store.
        fake.answer(API_KEY_PATH, response(server, 200, "read-rotated"));
        let after = resolver
            .resolve(&provider("bridge-pay"), &name("api-key"))
            .unwrap();

        assert_eq!(before.as_str(), "first-key-value", "{server:?}");
        assert_eq!(after.as_str(), "rotated-key-value", "{server:?}");
        // Each lookup asked the server, for the current version: no cache, no pinned version.
        let seen = fake.seen();
        assert_eq!(seen.len(), 2, "{server:?}");
        assert!(seen.iter().all(|r| r.path() == API_KEY_PATH), "{server:?}");
    }
}

// verifies: DEC-9, REQ-MDL-9
#[test]
fn credentials_are_keyed_by_provider() {
    for server in SERVERS {
        let fake = Arc::new(Fake::default());
        fake.answer(API_KEY_PATH, response(server, 200, "read-first"));
        let resolver = resolver(server, Arc::clone(&fake));

        // The same name under another provider is another secret, and that provider holds none.
        let other = resolver.resolve(&provider("other-pay"), &name("api-key"));

        assert_eq!(other, Err(ResolveError::NotFound), "{server:?}");
        assert_eq!(
            fake.seen()[0].path(),
            "/v1/secret/data/rutent/other-pay/api-key"
        );
    }
}

// verifies: DEC-9, REQ-MDL-9
#[test]
fn name_that_would_leave_the_providers_keys_is_refused_before_any_request() {
    let leaving = [
        ("bridge-pay", "../other-pay/api-key"),
        ("bridge-pay", "nested/api-key"),
        ("bridge-pay", ".."),
        ("bridge-pay", "."),
        ("bridge-pay", "api-key?version=1"),
        ("bridge-pay", "api-key#x"),
        ("bridge-pay", "%2e%2e"),
        ("bridge-pay", "api key"),
        ("bridge-pay", "api\\key"),
        ("../other-pay", "api-key"),
        ("other-pay/x", "api-key"),
        ("..", "api-key"),
    ];
    for server in SERVERS {
        let fake = Arc::new(Fake::default());
        let resolver = resolver(server, Arc::clone(&fake));
        for (p, n) in leaving {
            assert_eq!(
                resolver.resolve(&provider(p), &name(n)),
                Err(ResolveError::InvalidName),
                "{server:?}: {p} {n}"
            );
        }
        assert!(fake.seen().is_empty(), "{server:?}: nothing was sent");
    }
}

/// A small seeded generator, so a failing case replays from its seed (`DEC-18`).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn text(&mut self) -> String {
        const ALPHABET: &[char] = &[
            'a', 'Z', '0', '-', '_', '.', '.', '/', '%', '?', '#', ' ', '\\', '~', '\u{e9}', '\n',
        ];
        let len = 1 + (self.next() % 6) as usize;
        (0..len)
            .map(|_| ALPHABET[(self.next() % ALPHABET.len() as u64) as usize])
            .collect()
    }
}

fn plain_segment(text: &str) -> bool {
    text != "."
        && text != ".."
        && !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

// verifies: DEC-9, REQ-MDL-9
#[test]
fn no_provider_or_name_reaches_a_path_outside_that_providers_keys() {
    for seed in 1..=2_000u64 {
        let mut rng = Rng(seed);
        let (p, n) = (rng.text(), rng.text());
        let fake = Arc::new(Fake::default());
        let resolver = resolver(SERVERS[(seed % 2) as usize], Arc::clone(&fake));

        let outcome = resolver.resolve(&provider(&p), &name(&n));

        let seen = fake.seen();
        if !(plain_segment(&p) && plain_segment(&n)) {
            assert_eq!(outcome, Err(ResolveError::InvalidName), "seed {seed}");
            assert!(seen.is_empty(), "seed {seed}: a request was sent");
            continue;
        }
        assert_eq!(seen.len(), 1, "seed {seed}");
        let path = seen[0].path();
        let rest = path
            .strip_prefix(&format!("/v1/secret/data/rutent/{p}/"))
            .unwrap_or_else(|| panic!("seed {seed}: {path} is outside {p}'s keys"));
        assert!(plain_segment(rest), "seed {seed}: {path}");
        assert!(
            path.split('/').all(|s| s != "." && s != ".."),
            "seed {seed}: {path}"
        );
    }
}

// verifies: REQ-MDL-17
#[test]
fn missing_or_deleted_secret_is_not_found() {
    for server in SERVERS {
        for (status, fixture) in [(404, "not-found"), (404, "deleted"), (200, "deleted")] {
            let fake = Arc::new(Fake::default());
            fake.answer(API_KEY_PATH, response(server, status, fixture));
            let resolver = resolver(server, Arc::clone(&fake));

            assert_eq!(
                resolver.resolve(&provider("bridge-pay"), &name("api-key")),
                Err(ResolveError::NotFound),
                "{server:?}: {status} {fixture}"
            );
        }
    }
}

// verifies: REQ-MDL-17
#[test]
fn refused_token_is_reported_as_denied() {
    for server in SERVERS {
        for status in [401, 403] {
            let fake = Arc::new(Fake::default());
            fake.answer(API_KEY_PATH, response(server, status, "denied"));
            let resolver = resolver(server, Arc::clone(&fake));

            assert_eq!(
                resolver.resolve(&provider("bridge-pay"), &name("api-key")),
                Err(ResolveError::Denied),
                "{server:?}"
            );
        }
    }
}

// verifies: REQ-MDL-17
#[test]
fn unavailable_server_or_broken_connection_is_reported_as_such() {
    for server in SERVERS {
        for status in [429, 500, 503] {
            let fake = Arc::new(Fake::default());
            fake.answer(
                API_KEY_PATH,
                Response {
                    status,
                    body: b"{\"errors\":[\"Vault is sealed\"]}".to_vec(),
                },
            );
            let resolver = resolver(server, Arc::clone(&fake));
            assert_eq!(
                resolver.resolve(&provider("bridge-pay"), &name("api-key")),
                Err(ResolveError::Unavailable { status }),
                "{server:?}"
            );
        }
        let resolver = resolver(server, Down);
        assert!(matches!(
            resolver.resolve(&provider("bridge-pay"), &name("api-key")),
            Err(ResolveError::Transport(_))
        ));
    }
}

// verifies: REQ-MDL-17
#[test]
fn malformed_answer_is_refused_without_a_value() {
    let bodies: [&[u8]; 6] = [
        b"not json",
        b"{}",
        b"{\"data\":{\"data\":{}}}",
        b"{\"data\":{\"data\":{\"value\":\"\"}}}",
        b"{\"data\":{\"data\":{\"value\":42}}}",
        b"{\"data\":{\"data\":{\"other\":\"x\"}}}",
    ];
    for server in SERVERS {
        for body in bodies {
            let fake = Arc::new(Fake::default());
            fake.answer(
                API_KEY_PATH,
                Response {
                    status: 200,
                    body: body.to_vec(),
                },
            );
            let resolver = resolver(server, Arc::clone(&fake));
            assert_eq!(
                resolver.resolve(&provider("bridge-pay"), &name("api-key")),
                Err(ResolveError::Malformed),
                "{server:?}: {}",
                String::from_utf8_lossy(body)
            );
        }
        let fake = Arc::new(Fake::default());
        fake.answer(
            API_KEY_PATH,
            Response {
                status: 302,
                body: Vec::new(),
            },
        );
        assert_eq!(
            resolver(server, Arc::clone(&fake)).resolve(&provider("bridge-pay"), &name("api-key")),
            Err(ResolveError::Unexpected { status: 302 })
        );
    }
}

// verifies: THR-50, REQ-MDL-6
#[test]
fn token_value_and_raw_answer_never_print() {
    for server in SERVERS {
        let fake = Arc::new(Fake::default());
        fake.answer(API_KEY_PATH, response(server, 200, "read-first"));
        let resolver = resolver(server, Arc::clone(&fake));
        let value = resolver
            .resolve(&provider("bridge-pay"), &name("api-key"))
            .unwrap();

        let printed = [
            format!("{value:?}"),
            format!("{:?}", fake.seen()),
            format!("{:?}", response(server, 200, "read-first")),
            format!("{:?}", config()),
            format!("{:?}", Token::new(TOKEN).unwrap()),
        ];
        for text in &printed {
            assert!(!text.contains(TOKEN), "{server:?}: token in {text}");
            assert!(
                !text.contains("first-key-value"),
                "{server:?}: value in {text}"
            );
        }

        let errors = [
            ResolveError::InvalidName,
            ResolveError::NotFound,
            ResolveError::Denied,
            ResolveError::Malformed,
            ResolveError::Unavailable { status: 503 },
            ResolveError::Unexpected { status: 302 },
            ResolveError::Transport(TransportError::new("connection refused")),
        ];
        for error in errors {
            let text = format!("{error} {error:?}");
            assert!(!text.contains(TOKEN) && !text.contains("first-key-value"));
        }
    }
}

// verifies: DEC-9
#[test]
fn namespace_is_sent_only_when_one_is_configured() {
    for server in SERVERS {
        let fake = Arc::new(Fake::default());
        let plain = resolver(server, Arc::clone(&fake));
        let _ = plain.resolve(&provider("bridge-pay"), &name("api-key"));
        let spaced = match server {
            Server::OpenBao => KvResolver::openbao(
                config().with_namespace("integrations").unwrap(),
                Arc::clone(&fake),
            ),
            Server::Vault => KvResolver::vault(
                config().with_namespace("integrations").unwrap(),
                Arc::clone(&fake),
            ),
        };
        let _ = spaced.resolve(&provider("bridge-pay"), &name("api-key"));

        let namespace = |r: &Request| {
            r.headers()
                .iter()
                .find(|h| h.name() == "X-Vault-Namespace")
                .map(|h| h.value().to_owned())
        };
        let seen = fake.seen();
        assert_eq!(namespace(&seen[0]), None, "{server:?}");
        assert_eq!(
            namespace(&seen[1]).as_deref(),
            Some("integrations"),
            "{server:?}"
        );
    }
}

// verifies: DEC-9
#[test]
fn configuration_that_would_leave_its_mount_is_refused() {
    let token = || Token::new(TOKEN).unwrap();
    assert!(KvConfig::new("secret", "rutent", token()).is_ok());
    assert!(KvConfig::new("kv/internal", "rutent/integrations", token()).is_ok());
    for (mount, prefix) in [
        ("", "rutent"),
        ("secret", ""),
        ("../sys", "rutent"),
        ("secret", "rutent/../.."),
        ("secret/", "rutent"),
        ("secret", "/rutent"),
        ("secret", "rutent?list=true"),
    ] {
        assert!(
            KvConfig::new(mount, prefix, token()).is_err(),
            "{mount} {prefix}"
        );
    }
    assert!(config().with_namespace("a/../b").is_err());
    assert!(Token::new("").is_none());
    assert!(Token::new("bad\r\ntoken").is_none());
}

/// Live servers, only when configured: set `RUTENT_LIVE_OPENBAO_ADDR` or `RUTENT_LIVE_VAULT_ADDR` to a dev server's
/// `host:port` on loopback, with `RUTENT_LIVE_OPENBAO_TOKEN` or `RUTENT_LIVE_VAULT_TOKEN`. The server needs KV v2
/// mounted at `secret`, as a dev server has. The transport here is plain HTTP and is for such a dev server only.
mod live {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    pub struct PlainHttp {
        pub address: String,
    }

    impl PlainHttp {
        fn exchange(&self, head: &str, body: &[u8]) -> Result<Response, TransportError> {
            let fail = |_| TransportError::new("plain HTTP exchange failed");
            let mut stream = TcpStream::connect(&self.address).map_err(fail)?;
            stream.write_all(head.as_bytes()).map_err(fail)?;
            stream.write_all(body).map_err(fail)?;
            let mut raw = Vec::new();
            stream.read_to_end(&mut raw).map_err(fail)?;
            let split = raw
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .ok_or(TransportError::new("no header end"))?;
            let status = std::str::from_utf8(&raw[..split])
                .ok()
                .and_then(|h| h.split(' ').nth(1))
                .and_then(|s| s.parse().ok())
                .ok_or(TransportError::new("no status"))?;
            Ok(Response {
                status,
                body: raw[split + 4..].to_vec(),
            })
        }

        /// Writes `value` as the current version of the KV v2 secret at `path`.
        pub fn write(&self, path: &str, token: &str, value: &str) {
            let body = format!("{{\"data\":{{\"value\":\"{value}\"}}}}");
            let head = format!(
                "POST {path} HTTP/1.0\r\nHost: {}\r\nX-Vault-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                self.address,
                body.len()
            );
            let answer = self.exchange(&head, body.as_bytes()).unwrap();
            assert!(answer.status < 300, "write answered {}", answer.status);
        }
    }

    impl Transport for PlainHttp {
        fn get(&self, request: &Request) -> Result<Response, TransportError> {
            // HTTP/1.0, so the server closes the connection and never chunks the body.
            let mut head = format!(
                "GET {} HTTP/1.0\r\nHost: {}\r\n",
                request.path(),
                self.address
            );
            for header in request.headers() {
                head.push_str(&format!("{}: {}\r\n", header.name(), header.value()));
            }
            head.push_str("\r\n");
            self.exchange(&head, &[])
        }
    }
}

// verifies: REQ-MDL-17, REQ-MDL-6
#[test]
fn live_servers_resolve_and_rotate_a_secret_when_configured() {
    let mut ran = 0;
    for (server, label) in [(Server::OpenBao, "OPENBAO"), (Server::Vault, "VAULT")] {
        let (Ok(address), Ok(token)) = (
            std::env::var(format!("RUTENT_LIVE_{label}_ADDR")),
            std::env::var(format!("RUTENT_LIVE_{label}_TOKEN")),
        ) else {
            eprintln!("{server:?}: no live server configured, skipped");
            continue;
        };
        let transport = live::PlainHttp { address };
        let config =
            KvConfig::new("secret", "rutent-live", Token::new(token.as_str()).unwrap()).unwrap();
        let path = "/v1/secret/data/rutent-live/bridge-pay/api-key";
        transport.write(path, &token, "live-first");
        let resolver = match server {
            Server::OpenBao => KvResolver::openbao(config, transport),
            Server::Vault => KvResolver::vault(config, transport),
        };
        let lookup = || resolver.resolve(&provider("bridge-pay"), &name("api-key"));

        assert_eq!(lookup().unwrap().as_str(), "live-first", "{server:?}");
        resolver.transport().write(path, &token, "live-rotated");
        assert_eq!(lookup().unwrap().as_str(), "live-rotated", "{server:?}");
        assert_eq!(
            resolver.resolve(&provider("other-pay"), &name("api-key")),
            Err(ResolveError::NotFound),
            "{server:?}"
        );
        ran += 1;
    }
    eprintln!("live servers checked: {ran}");
}
