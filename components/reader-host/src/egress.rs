//! A reader's egress, enforced by the host (`T-015-04` part 4, `REQ-MDL-7`, `REQ-MDL-8` and `THR-79` applied to
//! readers).
//!
//! A reader's process has no network of its own ([`crate::jail`]); every request or connection it makes is the host's
//! to make, here, and only to a destination the reader declares:
//!
//! - **An RPC pool**: each request goes through `CMP-PROV`'s egress layer
//!   ([`rutent_prov::egress::Egress`]) under a running declaration the host builds from the reader's trust rule, one
//!   destination per endpoint, by name. So the layer's checks hold for a reader as for a provider: only a declared
//!   destination, every redirect checked, the name resolved once per connection and every address it gives held to
//!   the public ranges (`B-17`), TLS verified for the declared name and never downgraded (`B-18`), and one record per
//!   call, refused ones included. The host hands the reader a body only where at least the pool's quorum of the
//!   endpoints asked answered it and no other body reached the quorum ([`rutent_contracts::reader::LEAST_QUORUM`] at
//!   least).
//! - **A light client**: its peers are discovered, so its rule is per protocol and port, not
//!   per address, with a cap on the connections it holds. The egress layer has no session interface (its
//!   [`rutent_prov::egress::Connector`] makes one request and one answer), so the host opens the connection itself
//!   with the layer's parts: the name resolved once through its [`rutent_prov::egress::NameResolver`], every address
//!   held to the public ranges, and the TCP connection opened by its [`rutent_prov::egress::Dialer`] to the address
//!   checked. It relays bytes; the light client speaks its own protocol, and its own handshake authenticates its peers.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::num::NonZeroU16;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rutent_contracts::module_interface::{
    Addresses, Declaration, Destination, Host, Identifier, Name, Outside, OutsideOperation,
    PublicAddresses, SPECIAL_PURPOSE, Sandboxed, Scheme, Time,
};
use rutent_contracts::reader::{TrustRule, VERSION};
use rutent_prov::egress::{
    Call, Connector, Dialer, DirectDialer, Egress, Endpoint, Header, InvalidRoots, Lookup,
    LookupRefused, Method, NameResolver, Outcome, Refusal, Request, SystemResolver,
    ThreadedResolver, TlsConnector,
};

use crate::config::{Protocol, ReaderConfig};
use crate::records::{CallRecord, CallRecords, ConnectionOutcome, ConnectionRecord, ToRecords};
use crate::time::now_rfc3339;
use crate::wire::{OpenResult, ReceiveResult, RpcResult};

/// The one outside operation of a reader's running declaration: reading its network.
const READ: &str = "read";

/// How long the host waits for the first byte a connection gives after a send.
const RECEIVE_WAIT: Duration = Duration::from_millis(2_000);

/// The most bytes one receive hands the reader.
const RECEIVE_MAX: usize = 64 * 1024;

/// The network the host reaches for its readers: the egress layer's name resolver and connector, and the dialer a
/// light client's connections are opened through. Each is shared by every reader; each reader's own rule is held by
/// its own egress, which keeps its light client's connections.
#[derive(Clone)]
pub struct Network {
    /// Resolves a declared name, once per connection.
    pub resolver: Arc<dyn NameResolver + Send + Sync>,
    /// Makes one checked RPC exchange.
    pub connector: Arc<dyn Connector + Send + Sync>,
    /// Opens a light client's TCP connection to a checked address.
    pub dialer: Arc<dyn Dialer + Send + Sync>,
}

impl core::fmt::Debug for Network {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Network")
    }
}

impl Network {
    /// The production network: the operating system's resolver, each lookup on a thread of its own, rustls verifying
    /// each RPC endpoint against `rpc_roots_pem`, and direct TCP. The resolver and the connector keep their own
    /// timeouts on the operating system's monotonic clock, the egress layer's default and the host's own.
    pub fn production(rpc_roots_pem: &[u8]) -> Result<Self, InvalidRoots> {
        Ok(Network {
            resolver: Arc::new(ThreadedResolver::new(SystemResolver)),
            connector: Arc::new(TlsConnector::new(rpc_roots_pem)?),
            dialer: Arc::new(DirectDialer),
        })
    }
}

/// What a call serves: a question, by its request id, or the reader's start-up checks.
#[derive(Debug, Clone)]
pub(crate) struct Serving {
    /// The request id, or [`crate::records::START_UP`].
    pub request: Identifier,
    /// When the phase the call serves must end, by the host's own bound (a question's `hang_after_ms`, the start-up
    /// checks' `start_within_ms`): every call is given up then, whatever its own bound.
    pub until: Option<Instant>,
}

/// One reader's egress: its running declaration, the egress layer, and the light client's open connections.
pub(crate) struct ReaderEgress {
    config: Arc<ReaderConfig>,
    declaration: Declaration,
    read: Name,
    egress: Egress<Arc<dyn NameResolver + Send + Sync>, Arc<dyn Connector + Send + Sync>>,
    network: Network,
    records: Arc<dyn CallRecords>,
    connections: BTreeMap<u64, TcpStream>,
    next_connection: u64,
}

/// An endpoint of a pool as the egress layer reaches it, or `None` if the text names none.
fn endpoint_of(text: &str) -> Option<Endpoint> {
    let (scheme, rest) = if let Some(rest) = text.strip_prefix("https://") {
        (Scheme::Https, rest)
    } else {
        (Scheme::Wss, text.strip_prefix("wss://")?)
    };
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok().filter(|p| *p != 0)?),
        None => (authority, 443),
    };
    (!host.is_empty() && !host.contains(['/', '?', '#', '@'])).then(|| Endpoint {
        scheme,
        host: host.to_owned(),
        port,
    })
}

/// The running declaration the egress layer holds a reader's requests to: its pool's endpoints, each a destination
/// by its name in the trust rule, reaching only public addresses, under one operation; none for a light client.
fn declaration(config: &ReaderConfig) -> Option<Declaration> {
    let endpoints: Vec<&str> = match &config.trust {
        TrustRule::RpcPool { endpoints, .. } => endpoints.iter().map(|e| e.as_str()).collect(),
        TrustRule::LightClient(_) => Vec::new(),
    };
    let mut destinations = Vec::new();
    for text in &endpoints {
        let endpoint = endpoint_of(text)?;
        destinations.push(Destination {
            name: Name::new(*text)?,
            scheme: endpoint.scheme,
            host: Host::new(endpoint.host)?,
            port: NonZeroU16::new(endpoint.port)?,
            addresses: Addresses::Public(PublicAddresses::Public),
        });
    }
    Some(Declaration {
        provider: Identifier::new(config.name.as_str())?,
        version: Identifier::new(config.version.as_str())?,
        interface: Identifier::new(format!("reader-{VERSION}"))?,
        sandboxed: Sandboxed {
            handles: Vec::new(),
        },
        outside: Outside {
            operations: vec![OutsideOperation {
                name: Name::new(READ)?,
                destinations: destinations.iter().map(|d| d.name.clone()).collect(),
                secrets: Vec::new(),
            }],
            destinations,
            secrets: Vec::new(),
            calls: Vec::new(),
            statement_key: config.statement_key.clone(),
        },
    })
}

/// Whether `ip` lies in a special-purpose range ([`SPECIAL_PURPOSE`]), where no public peer is. An IPv4 address and an
/// IPv6 range never match, so an IPv4-mapped IPv6 address is judged as IPv6, where `::ffff:0:0/96` is special.
fn special(ip: IpAddr) -> bool {
    let bits = |ip: IpAddr| match ip {
        IpAddr::V4(v4) => (u128::from(u32::from(v4)) << 96, true),
        IpAddr::V6(v6) => (u128::from(v6), false),
    };
    SPECIAL_PURPOSE.iter().any(|(range, len)| {
        range.parse::<IpAddr>().is_ok_and(|range| {
            let ((a, a4), (b, b4)) = (bits(ip), bits(range));
            let mask = if *len == 0 {
                0
            } else {
                u128::MAX << (128 - u32::from(*len))
            };
            a4 == b4 && a & mask == b & mask
        })
    })
}

impl ReaderEgress {
    /// The egress of the reader `config` names, over `network`, recording to `records`. `None` if its pool's
    /// endpoints can't be declared, which [`crate::HostConfig::check`]'s contract types already refuse.
    pub(crate) fn new(
        config: Arc<ReaderConfig>,
        network: Network,
        records: Arc<dyn CallRecords>,
    ) -> Option<Self> {
        Some(ReaderEgress {
            declaration: declaration(&config)?,
            read: Name::new(READ)?,
            egress: Egress::new(network.resolver.clone(), network.connector.clone()),
            config,
            network,
            records,
            connections: BTreeMap::new(),
            next_connection: 1,
        })
    }

    /// Drops every connection the reader held: its process is gone.
    pub(crate) fn reset(&mut self) {
        self.connections.clear();
    }

    /// When a call for `serving` is given up: its own bound, or the end of the phase it serves, whichever is first.
    fn due(&self, serving: &Serving) -> Option<Instant> {
        let own = Instant::now() + self.config.limits.egress_give_up_after();
        Some(serving.until.map_or(own, |until| until.min(own)))
    }

    /// Sends `body` to each endpoint named, through the egress layer, and gives what at least the quorum agreed on.
    /// The second value is true when the reader named a destination it did not declare: the host then answers the
    /// question it is on as unreadable, whatever the pool agreed, since a reader's own code is not trusted to keep to
    /// its list.
    pub(crate) fn rpc(
        &mut self,
        serving: &Serving,
        endpoints: &[String],
        body: &str,
    ) -> (RpcResult, bool) {
        let quorum = match &self.config.trust {
            TrustRule::RpcPool { quorum, .. } => *quorum as usize,
            // A light client declares no endpoint: every request below is refused, and recorded.
            TrustRule::LightClient(_) => usize::MAX,
        };
        let Some(started_at) = Time::new(now_rfc3339()) else {
            return (unreadable("no_time"), false);
        };
        let mut undeclared = false;
        let mut answers: Vec<(String, serde_json::Value)> = Vec::new();
        let mut asked: Vec<String> = Vec::new();
        for text in endpoints {
            let endpoint = endpoint_of(text).unwrap_or(Endpoint {
                scheme: Scheme::Https,
                host: text.clone(),
                port: 443,
            });
            // Each host is asked once: a quorum is of nodes, not of spellings.
            if asked.contains(&endpoint.host) {
                continue;
            }
            asked.push(endpoint.host.clone());
            let Ok(request) = Request::new(
                Method::Post,
                "/",
                vec![Header::plain("content-type", "application/json")],
                body.as_bytes().to_vec(),
            ) else {
                return (unreadable("malformed_request"), false);
            };
            let call = Call {
                declaration: &self.declaration,
                in_flight: &self.read,
                asked: READ,
                transaction: &serving.request,
                secrets: &[],
                started_at: &started_at,
                due: self.due(serving),
            };
            let host = endpoint.host.clone();
            let mut log = ToRecords(&*self.records);
            match self.egress.send(&call, endpoint, request, &mut log) {
                Outcome::Completed(response) => {
                    let value = serde_json::from_slice(&response.body).unwrap_or_else(|_| {
                        serde_json::Value::String(String::from_utf8_lossy(&response.body).into())
                    });
                    answers.push((host, value));
                }
                Outcome::Refused(Refusal::UndeclaredDestination | Refusal::UndeclaredOperation) => {
                    undeclared = true;
                }
                // An endpoint that redirects elsewhere, resolves where it mustn't, fails TLS, fails or doesn't answer
                // gave no answer: the others may still reach the quorum.
                _ => {}
            }
        }
        if undeclared {
            return (unreadable("undeclared_destination"), true);
        }
        let mut groups: Vec<(&serde_json::Value, usize)> = Vec::new();
        for (_, value) in &answers {
            match groups.iter_mut().find(|(v, _)| *v == value) {
                Some((_, n)) => *n += 1,
                None => groups.push((value, 1)),
            }
        }
        let reaching: Vec<&(&serde_json::Value, usize)> =
            groups.iter().filter(|(_, n)| *n >= quorum).collect();
        let result = match reaching.as_slice() {
            [(value, _)] => RpcResult::Agreed {
                body: match value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                },
            },
            [] if groups.len() > 1 => unreadable("rpc_answers_disagree"),
            [] => unreadable("too_few_rpc_answers"),
            _ => unreadable("rpc_answers_disagree"),
        };
        (result, false)
    }

    fn record(
        &self,
        serving: &Serving,
        protocol: Protocol,
        host: &str,
        port: u16,
        address: Option<SocketAddr>,
        outcome: ConnectionOutcome,
    ) {
        self.records
            .record(CallRecord::Connection(ConnectionRecord {
                reader: self.config.name.as_str().to_owned(),
                version: self.config.version.as_str().to_owned(),
                request: serving.request.as_str().to_owned(),
                started_at: now_rfc3339(),
                protocol,
                host: host.to_owned(),
                port,
                address: address.map(|a| a.ip().to_string()),
                outcome,
            }));
    }

    /// Opens a light client's connection to `host` and `port` over `protocol`, if the reader's rule declares that
    /// protocol and port and it holds fewer connections than its cap, to an address that is a public one. Every
    /// opening is recorded, refused ones included.
    pub(crate) fn open(
        &mut self,
        serving: &Serving,
        protocol: Protocol,
        host: &str,
        port: u16,
    ) -> OpenResult {
        let refused = |egress: &Self, outcome, why: &str| {
            egress.record(serving, protocol, host, port, None, outcome);
            OpenResult::Refused {
                why: why.to_owned(),
            }
        };
        let Some(rule) = &self.config.light_client else {
            return refused(
                self,
                ConnectionOutcome::UndeclaredDestination,
                "undeclared_destination",
            );
        };
        if !rule
            .ports
            .iter()
            .any(|p| p.protocol == protocol && p.port == port)
        {
            return refused(
                self,
                ConnectionOutcome::UndeclaredDestination,
                "undeclared_destination",
            );
        }
        if self.connections.len() >= rule.max_connections as usize {
            return refused(self, ConnectionOutcome::LimitReached, "limit_reached");
        }
        let addresses = match host.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => {
                let plain = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'.';
                let (Some(name), Some(nonzero)) = (
                    Host::new(host).filter(|_| !host.is_empty() && host.bytes().all(plain)),
                    NonZeroU16::new(port),
                ) else {
                    return refused(
                        self,
                        ConnectionOutcome::UndeclaredDestination,
                        "undeclared_destination",
                    );
                };
                let Some(provider) = Identifier::new(self.config.name.as_str()) else {
                    return refused(
                        self,
                        ConnectionOutcome::UndeclaredDestination,
                        "undeclared_destination",
                    );
                };
                match self.network.resolver.resolve_for(&Lookup {
                    host: &name,
                    port: nonzero,
                    provider: &provider,
                    due: self.due(serving),
                }) {
                    Ok(addresses) => addresses,
                    Err(LookupRefused::TooManyInFlight) => {
                        return refused(self, ConnectionOutcome::LimitReached, "limit_reached");
                    }
                    Err(LookupRefused::DeadlinePassed) => {
                        return refused(self, ConnectionOutcome::Unreachable, "unreachable");
                    }
                }
            }
        };
        let Some(address) = addresses
            .first()
            .copied()
            .filter(|_| addresses.iter().all(|a| !special(*a)))
        else {
            return refused(
                self,
                ConnectionOutcome::AddressNotAllowed,
                "address_not_allowed",
            );
        };
        let to = SocketAddr::new(address, port);
        let Ok(stream) = self
            .network
            .dialer
            .connect(to, self.config.limits.egress_give_up_after())
        else {
            self.record(
                serving,
                protocol,
                host,
                port,
                None,
                ConnectionOutcome::Unreachable,
            );
            return OpenResult::Refused {
                why: "unreachable".to_owned(),
            };
        };
        let _ = stream.set_write_timeout(Some(self.config.limits.egress_give_up_after()));
        self.record(
            serving,
            protocol,
            host,
            port,
            Some(to),
            ConnectionOutcome::Connected,
        );
        let conn = self.next_connection;
        self.next_connection += 1;
        self.connections.insert(conn, stream);
        OpenResult::Open { conn }
    }

    /// Sends `bytes` on a connection, and gives what comes back within the host's wait, which ends with the phase the
    /// send serves.
    pub(crate) fn send(&mut self, serving: &Serving, conn: u64, bytes: &[u8]) -> ReceiveResult {
        // The wait for what comes back ends with the phase it serves, as every call's does.
        let wait = serving.until.map_or(RECEIVE_WAIT, |until| {
            until
                .saturating_duration_since(Instant::now())
                .min(RECEIVE_WAIT)
        });
        let Some(stream) = self.connections.get_mut(&conn) else {
            return ReceiveResult::Closed;
        };
        if stream.write_all(bytes).is_err() {
            self.connections.remove(&conn);
            return ReceiveResult::Closed;
        }
        if wait.is_zero() {
            return ReceiveResult::Bytes { bytes: Vec::new() };
        }
        let _ = stream.set_read_timeout(Some(wait));
        let mut buffer = vec![0u8; RECEIVE_MAX];
        match stream.read(&mut buffer) {
            Ok(0) => {
                self.connections.remove(&conn);
                ReceiveResult::Closed
            }
            Ok(n) => {
                buffer.truncate(n);
                ReceiveResult::Bytes { bytes: buffer }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                ReceiveResult::Bytes { bytes: Vec::new() }
            }
            Err(_) => {
                self.connections.remove(&conn);
                ReceiveResult::Closed
            }
        }
    }

    /// Closes a connection.
    pub(crate) fn close(&mut self, conn: u64) {
        self.connections.remove(&conn);
    }
}

fn unreadable(why: &str) -> RpcResult {
    RpcResult::Unreadable {
        why: why.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{endpoint_of, special};

    // verifies: REQ-MDL-7
    #[test]
    fn light_client_peer_on_a_special_purpose_address_is_no_peer() {
        assert!(special("127.0.0.1".parse().unwrap()));
        assert!(special("10.1.2.3".parse().unwrap()));
        assert!(special("::1".parse().unwrap()));
        assert!(!special("93.184.216.34".parse().unwrap()));
        let endpoint = endpoint_of("https://rpc-b.base.example:8545").unwrap();
        assert_eq!(
            (endpoint.host.as_str(), endpoint.port),
            ("rpc-b.base.example", 8545)
        );
        assert!(endpoint_of("http://rpc.example").is_none());
    }
}
