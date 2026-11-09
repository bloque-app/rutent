//! Doubles for the egress tests: a resolver that answers from a table and can change its answer, and a connector
//! that answers from a script and keeps every dial. Neither touches a network.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;
use std::sync::Mutex;

use rutent_conformance::role::placeholder_key;
use rutent_contracts::module_interface::{Declaration, Host, Identifier, Name, Scheme, Time};
use rutent_prov::egress::{
    Call, ConnectError, Connector, Dial, Method, NameResolver, Request, Response,
};
use serde_json::json;

pub const PROVIDER: &str = "acme";
pub const VERSION: &str = "2.1.0";

/// A public address `api` resolves to.
pub const API: &str = "93.184.216.34";
/// Another public address, inside `files`' declared block.
pub const FILES: &str = "93.184.216.80";
/// The cloud metadata address: link-local, so never public.
pub const METADATA: &str = "169.254.169.254";

pub fn ip(text: &str) -> IpAddr {
    text.parse().expect("an address")
}

pub fn name(text: &str) -> Name {
    Name::new(text.to_owned()).expect("a name")
}

/// `acme` 2.1.0. Operation `payout` reaches `api` (any public address) and `files` (only `93.184.216.0/24`) with
/// secret `api.key`; operation `status` reaches `status` with none. All three over HTTPS on 443.
pub fn declaration() -> Declaration {
    serde_json::from_value(json!({
        "provider": PROVIDER,
        "version": VERSION,
        "interface": rutent_contracts::module_interface::VERSION,
        "sandboxed": { "handles": ["withdrawal"] },
        "outside": {
            "operations": [
                { "name": "payout", "destinations": ["api", "files"], "secrets": ["api.key"] },
                { "name": "status", "destinations": ["status"], "secrets": [] },
            ],
            "destinations": [
                { "name": "api", "scheme": "https", "host": "api.acme.example", "port": 443,
                  "addresses": "public" },
                { "name": "files", "scheme": "https", "host": "files.acme.example", "port": 443,
                  "addresses": ["93.184.216.0/24"] },
                { "name": "status", "scheme": "https", "host": "status.acme.example", "port": 443,
                  "addresses": "public" },
            ],
            "secrets": ["api.key"],
            "calls": [],
            "statement_key": placeholder_key(1),
        },
    }))
    .expect("a valid declaration")
}

/// What a call needs from the platform's state, owned so a test can lend it.
pub struct Flight {
    pub declaration: Declaration,
    pub in_flight: Name,
    pub transaction: Identifier,
    pub secrets: Vec<Name>,
    pub started_at: Time,
}

impl Flight {
    /// `payout` in flight for `transaction`, with `api.key` resolved for it.
    pub fn payout(transaction: &str) -> Self {
        Flight {
            declaration: declaration(),
            in_flight: name("payout"),
            transaction: Identifier::new(transaction.to_owned()).expect("an id"),
            secrets: vec![name("api.key")],
            started_at: Time::new("2026-11-09T12:00:00Z".to_owned()).expect("a time"),
        }
    }

    /// The call, asking for the operation in flight.
    pub fn call(&self) -> Call<'_> {
        self.asking(self.in_flight.as_str())
    }

    /// The call, asking for `asked`.
    pub fn asking<'a>(&'a self, asked: &'a str) -> Call<'a> {
        Call {
            declaration: &self.declaration,
            in_flight: &self.in_flight,
            asked,
            transaction: &self.transaction,
            secrets: &self.secrets,
            started_at: &self.started_at,
        }
    }
}

pub fn post(target: &str) -> Request {
    Request::new(
        Method::Post,
        target,
        Vec::new(),
        b"{\"amount\":\"10\"}".to_vec(),
    )
    .expect("a request")
}

/// A resolver from a table: each host has a queue of answers, given in order, the last one repeated. It counts
/// every question.
#[derive(Default)]
pub struct TableResolver {
    answers: Mutex<HashMap<String, VecDeque<Vec<IpAddr>>>>,
    asked: Mutex<Vec<String>>,
}

impl TableResolver {
    /// Adds `addresses` as `host`'s next answer.
    pub fn then(&self, host: &str, addresses: &[&str]) -> &Self {
        self.answers
            .lock()
            .unwrap()
            .entry(host.to_owned())
            .or_default()
            .push_back(addresses.iter().map(|a| ip(a)).collect());
        self
    }

    /// Every host asked, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl NameResolver for TableResolver {
    fn resolve(&self, host: &Host, _port: NonZeroU16) -> Vec<IpAddr> {
        self.asked.lock().unwrap().push(host.as_str().to_owned());
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

/// One dial a connector was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialled {
    pub scheme: Scheme,
    pub name: String,
    pub to: SocketAddr,
    pub method: Method,
    pub target: String,
    pub body: Vec<u8>,
}

/// A connector that answers by the address dialled, from a script, and keeps every dial. An address with no
/// script is unreachable.
#[derive(Default)]
pub struct ScriptedConnector {
    script: Mutex<HashMap<IpAddr, Result<Response, ConnectError>>>,
    dials: Mutex<Vec<Dialled>>,
}

impl ScriptedConnector {
    pub fn answer(&self, address: &str, answer: Result<Response, ConnectError>) -> &Self {
        self.script.lock().unwrap().insert(ip(address), answer);
        self
    }

    pub fn dials(&self) -> Vec<Dialled> {
        self.dials.lock().unwrap().clone()
    }
}

impl Connector for ScriptedConnector {
    fn exchange(&self, dial: &Dial<'_>, request: &Request) -> Result<Response, ConnectError> {
        self.dials.lock().unwrap().push(Dialled {
            scheme: dial.scheme,
            name: dial.name.as_str().to_owned(),
            to: dial.to,
            method: request.method(),
            target: request.target().to_owned(),
            body: request.body().to_vec(),
        });
        self.script
            .lock()
            .unwrap()
            .get(&dial.to.ip())
            .cloned()
            .unwrap_or(Err(ConnectError::Unreachable))
    }
}

pub fn ok() -> Result<Response, ConnectError> {
    Ok(Response {
        status: 200,
        headers: Vec::new(),
        body: b"{}".to_vec(),
    })
}

pub fn redirect(status: u16, location: &str) -> Result<Response, ConnectError> {
    Ok(Response {
        status,
        headers: vec![("Location".to_owned(), location.to_owned())],
        body: Vec::new(),
    })
}
