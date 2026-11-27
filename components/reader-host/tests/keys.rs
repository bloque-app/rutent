//! Signing outside the reader (`T-015-04` part 3, `REQ-MDL-6` applied to readers). The host resolves each reader's key
//! as the platform resolves a provider's, through `T-014-01`'s resolver over a vault bucket's keys (`KvResolver`, here
//! over a stand-in store on loopback). Alone in its test binary: it sets the host's environment, where the binary reads
//! its store's token, before any thread starts.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use rutent_prov::keys::{
    HttpsTransport, RoleSettings, Server, StoreAddress, StoreScheme, Token, kv_bucket,
};
use rutent_reader_host::egress::Network;
use rutent_reader_host::records::MemoryRecords;
use rutent_reader_host::{Host, Services};

/// A stand-in KV version 2 store on loopback: it answers a read of `path` with `value` while `value` is set, and keeps
/// each request's path and token.
struct Store {
    port: u16,
    value: Arc<Mutex<Option<String>>>,
    asked: Arc<Mutex<Vec<(String, String)>>>,
}

impl Store {
    fn serve(path: String, value: String) -> Store {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (value, asked) = (
            Arc::new(Mutex::new(Some(value))),
            Arc::new(Mutex::new(Vec::new())),
        );
        let (held, seen) = (value.clone(), asked.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let target = line.split(' ').nth(1).unwrap_or_default().to_owned();
                let mut token = String::new();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, v)) = header.split_once(':')
                        && name.eq_ignore_ascii_case("x-vault-token")
                    {
                        token = v.trim().to_owned();
                    }
                }
                seen.lock().unwrap().push((target.clone(), token));
                let current = held.lock().unwrap().clone();
                let (status, body) = match current {
                    Some(v) if target == path => (
                        "200 OK",
                        serde_json::json!({"data": {"data": {"value": v}}}).to_string(),
                    ),
                    _ => ("404 Not Found", "{}".to_owned()),
                };
                let _ = stream.write_all(
                    format!("HTTP/1.0 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                        .as_bytes(),
                );
            }
        });
        Store { port, value, asked }
    }
}

// verifies: REQ-MDL-6
#[test]
fn reader_never_holds_its_statement_key() {
    let reader = rpc_reader("base", &["inspect"]);
    let secret = secret_of("base");
    // Where the binary reads its store's token: the host's environment, which never reaches a reader.
    let token = "s.reader-host-test-token";
    // SAFETY: the only test of this binary, before the host or any other thread of it starts.
    unsafe { std::env::set_var("RUTENT_READER_KEY_STORE_TOKEN", token) };

    // The platform's resolver over a vault bucket's keys: the reader is the provider, the listed key's text the name.
    let path = format!(
        "/v1/secret/data/rutent/readers/keys/base/{}",
        reader.statement_key.as_str()
    );
    let store = Store::serve(path.clone(), secret.as_str().to_owned());
    let role = RoleSettings {
        server: Server::Vault,
        store: StoreAddress {
            scheme: StoreScheme::Http,
            host: "127.0.0.1".to_owned(),
            port: store.port,
        },
        ca_file: None,
        mount: "secret".to_owned(),
        secrets: "rutent/readers/secrets".to_owned(),
        keys: "rutent/readers/keys".to_owned(),
        namespace: None,
    };
    let transport = HttpsTransport::for_role(&role).unwrap();
    let keys = kv_bucket(&role, Token::new(token).unwrap(), transport)
        .unwrap()
        .keys;

    let host = Host::start(
        host_config(vec![reader.clone()]),
        Services {
            keys: Arc::new(keys),
            network: Network {
                resolver: Arc::new(StubResolver::default()),
                connector: honest_pool(),
                dialer: Arc::new(MappedDialer::default()),
            },
            records: Arc::new(MemoryRecords::new()),
        },
    )
    .unwrap();
    assert!(host.wait_ready(&reader.name, Duration::from_secs(30)));

    // Each answer is signed with the reader's listed key, resolved for it: `ask` verifies it as Core does.
    for asked in [network_rules(), status_of("0xmove")] {
        assert!(unreadable(&ask(&host, &reader, asked)).is_none());
    }
    let asked = store.asked.lock().unwrap().clone();
    assert_eq!(
        asked.len(),
        2,
        "resolved by name for each answer, no copy kept: {asked:?}"
    );
    assert!(
        asked.iter().all(|(p, t)| p == &path && t == token),
        "{asked:?}"
    );

    // What the reader's process saw: its arguments, its environment and every frame the host sent it.
    let seen = host.log(&reader.name);
    assert!(
        seen.iter().any(|l| l.starts_with("args ")),
        "the reader logged what it saw: {seen:?}"
    );
    assert!(
        seen.iter().filter(|l| l.starts_with("frame ")).count() >= 3,
        "{seen:?}"
    );
    let seed_hex: String = key_of("base")
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    for line in &seen {
        assert!(
            !line.contains(secret.as_str()),
            "the key reached the reader: {line}"
        );
        assert!(
            !line.contains(&seed_hex),
            "the key reached the reader: {line}"
        );
        assert!(
            !line.contains(token),
            "the store's token reached the reader: {line}"
        );
    }
    // Empty, but for what macOS sets in every process's environment itself.
    let environment: Vec<&String> = seen
        .iter()
        .filter(|l| l.starts_with("env ") && !l.starts_with("env __CF_USER_TEXT_ENCODING="))
        .collect();
    assert!(
        environment.is_empty(),
        "the reader's environment is empty: {environment:?}"
    );

    // The store no longer gives the key: no signature, so the host gives nothing rather than an answer it can't sign.
    *store.value.lock().unwrap() = None;
    let question = question("base", "base", network_rules());
    assert_eq!(host.answer(&serde_json::to_vec(&question).unwrap()), None);
}
