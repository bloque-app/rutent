//! The reader host as one process, one ECS task (`T-015-04`):
//!
//! ```text
//! rutent-reader-host <config.json> --listen <address:port> --operator-ca <ca.pem> --cert <chain.pem>
//!     --key <key.pem> --core-name <Core's subject name> --rpc-roots <roots.pem> --key-store <role.json>
//! ```
//!
//! It reads its configuration ([`rutent_reader_host::HostConfig::read`]) at each start, resolves the readers' keys as
//! the platform resolves a provider's, from the keys of the vault bucket `--key-store` describes (a process role's
//! settings, [`rutent_prov::keys::RoleSettings`], read with the token the deployment delivers in
//! `RUTENT_READER_KEY_STORE_TOKEN`), writes each call's record to its standard error as a JSON
//! line, and serves Core over mTLS until it is stopped.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use rutent_prov::keys::{HttpsTransport, RoleSettings, Token, kv_bucket};
use rutent_reader_host::egress::Network;
use rutent_reader_host::link::{Link, LinkTls};
use rutent_reader_host::records::StderrRecords;
use rutent_reader_host::{Host, HostConfig, Services, jail};

const FLAGS: [&str; 7] = [
    "--listen",
    "--operator-ca",
    "--cert",
    "--key",
    "--core-name",
    "--rpc-roots",
    "--key-store",
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rutent-reader-host: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    jail::harden_host().map_err(|e| format!("undumpable: {e}"))?;
    let mut args = std::env::args().skip(1);
    let config = PathBuf::from(
        args.next()
            .ok_or("usage: rutent-reader-host <config.json> --listen ...")?,
    );
    let mut flags = BTreeMap::new();
    while let Some(flag) = args.next() {
        if !FLAGS.contains(&flag.as_str()) {
            return Err(format!("unknown flag {flag}"));
        }
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        flags.insert(flag, value);
    }
    let flag = |name: &str| {
        flags
            .get(name)
            .cloned()
            .ok_or(format!("{name} is required"))
    };
    let read = |name: &str| -> Result<Vec<u8>, String> {
        let path = flag(name)?;
        std::fs::read(&path).map_err(|e| format!("{path}: {e}"))
    };
    let config = HostConfig::read(&config).map_err(|e| e.to_string())?;
    let tls = LinkTls::new(
        &read("--operator-ca")?,
        &read("--cert")?,
        &read("--key")?,
        &flag("--core-name")?,
    )
    .map_err(|e| e.to_string())?;
    let network = Network::production(&read("--rpc-roots")?).map_err(|e| e.to_string())?;
    let role: RoleSettings = serde_json::from_slice(&read("--key-store")?)
        .map_err(|_| "--key-store is not a process role's settings".to_owned())?;
    let token = std::env::var("RUTENT_READER_KEY_STORE_TOKEN")
        .ok()
        .and_then(Token::new)
        .ok_or("RUTENT_READER_KEY_STORE_TOKEN holds no token")?;
    // SAFETY: still one thread: no reader, worker or link has started.
    unsafe { std::env::remove_var("RUTENT_READER_KEY_STORE_TOKEN") };
    let transport = HttpsTransport::for_role(&role).map_err(|e| e.to_string())?;
    let keys = kv_bucket(&role, token, transport)
        .map_err(|e| e.to_string())?
        .keys;
    let host = Host::start(
        config,
        Services {
            keys: Arc::new(keys),
            network,
            records: Arc::new(StderrRecords),
        },
    )
    .map_err(|e| e.to_string())?;
    let listener = TcpListener::bind(flag("--listen")?).map_err(|e| e.to_string())?;
    let _link = Link::serve(listener, tls, Arc::new(host)).map_err(|e| e.to_string())?;
    loop {
        std::thread::park();
    }
}
