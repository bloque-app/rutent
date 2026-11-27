//! Signing outside the reader (`T-015-04` part 3, `REQ-MDL-6` applied to readers). Alone in its test binary: it sets
//! the host's environment, where the production key store reads the readers' keys, before any thread starts.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use rutent_reader_host::egress::Network;
use rutent_reader_host::keys::EnvKeys;
use rutent_reader_host::records::MemoryRecords;
use rutent_reader_host::{Host, Services};

// verifies: REQ-MDL-6
#[test]
fn reader_never_holds_its_statement_key() {
    let reader = rpc_reader("base", &["inspect"]);
    let secret = secret_of("base");
    let variable = EnvKeys::variable(&reader.name, &reader.statement_key.name);
    // SAFETY: the only test of this binary, before the host or any other thread of it starts.
    unsafe { std::env::set_var(&variable, secret.as_str()) };

    let host = Host::start(
        host_config(vec![reader.clone()]),
        Services {
            keys: Arc::new(EnvKeys),
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

    // Each answer is signed with the reader's listed key: `ask` verifies it as Core does.
    for asked in [network_rules(), status_of("0xmove")] {
        assert!(unreadable(&ask(&host, &reader, asked)).is_none());
    }

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
            !line.contains(&variable),
            "the host's environment reached the reader: {line}"
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

    // No key in the store, no signature: the host gives nothing rather than an answer it can't sign.
    // SAFETY: the host's workers read no environment variable of the test's.
    unsafe { std::env::remove_var(&variable) };
    let question = question("base", "base", network_rules());
    assert_eq!(host.answer(&serde_json::to_vec(&question).unwrap()), None);
}
