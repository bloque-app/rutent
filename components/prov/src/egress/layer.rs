//! The egress layer itself: the checks every hop of an outbound call passes, and the record each call leaves.

use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;

use rutent_contracts::module_interface::{
    Address, Addresses, CallOutcome, Declaration, Hop, Host, Identifier, Name, Nothing,
    OutboundCallRecord, RefusalReason, Refused, SPECIAL_PURPOSE, Scheme, Time,
};

use super::connector::{ConnectError, Connector, Dial, Request, Response, origin_form};
use super::resolve::NameResolver;

/// The most connections one outbound call makes, the first and each redirect followed.
pub const MAX_HOPS: usize = 5;

/// An endpoint an outside part asks to reach: a scheme, an exact host and a port.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    /// The scheme.
    pub scheme: Scheme,
    /// The host name.
    pub host: String,
    /// The port.
    pub port: u16,
}

/// What the platform knows about one outbound call, from its own state and never from what the outside part
/// says: the running declaration, the outside request in flight and its transaction.
#[derive(Debug, Clone, Copy)]
pub struct Call<'a> {
    /// The provider's running declaration, which names the provider and the module version.
    pub declaration: &'a Declaration,
    /// The outside operation of the request in flight, from the platform's state.
    pub in_flight: &'a Name,
    /// The operation the outside part named for this call. One other than [`in_flight`](Self::in_flight) is
    /// refused as `undeclared_operation`.
    pub asked: &'a str,
    /// The transaction of the request in flight.
    pub transaction: &'a Identifier,
    /// The names of the secrets resolved for the operation in flight, never their values.
    pub secrets: &'a [Name],
    /// When the call started.
    pub started_at: &'a Time,
}

/// Where outbound call records go. Every call [`Egress::send`] is asked to make leaves exactly one, refused calls
/// included (`REQ-MDL-8`).
pub trait CallLog {
    /// Keeps `record`.
    fn record(&mut self, record: OutboundCallRecord);
}

impl CallLog for Vec<OutboundCallRecord> {
    fn record(&mut self, record: OutboundCallRecord) {
        self.push(record);
    }
}

/// Why the egress layer refused a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The call named another operation than the one in flight, or one the declaration doesn't declare.
    UndeclaredOperation,
    /// The first endpoint isn't a destination the operation in flight lists.
    UndeclaredDestination,
    /// A redirect led to an endpoint the operation in flight doesn't list, or to one no destination could be:
    /// plain `http`, another scheme, a user name in the authority, or a malformed location.
    RedirectToUndeclaredDestination,
    /// The name resolved to an address outside the destination's declared addresses, or to none (`B-17`).
    AddressNotAllowed,
    /// The peer was not verified as the declared host over TLS (`B-18`).
    NotVerified,
}

impl Refusal {
    /// The reason the outbound call record gives. The contract has no reason for a failed TLS check, so
    /// [`Refusal::NotVerified`] is recorded as `undeclared_destination`: a peer that doesn't prove it is the
    /// declared host is not shown to be the declared destination (see the `F-014` notes).
    pub fn reason(self) -> RefusalReason {
        match self {
            Refusal::UndeclaredOperation => RefusalReason::UndeclaredOperation,
            Refusal::UndeclaredDestination | Refusal::NotVerified => {
                RefusalReason::UndeclaredDestination
            }
            Refusal::RedirectToUndeclaredDestination => {
                RefusalReason::RedirectToUndeclaredDestination
            }
            Refusal::AddressNotAllowed => RefusalReason::AddressNotAllowed,
        }
    }
}

/// How an outbound call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The last destination answered with a `2xx`.
    Completed(Response),
    /// The last destination answered otherwise, or redirected once more than [`MAX_HOPS`] allows.
    Failed(Response),
    /// No connection was established to the checked address; nothing was sent.
    Unreachable,
    /// A connection was made and no complete answer came; the effect is not known.
    NoAnswer,
    /// The egress layer refused the call.
    Refused(Refusal),
}

/// The egress layer: reaches only the destinations the operation in flight declares, checks every redirect,
/// connects only to the address it checked, never without TLS, and records every call.
#[derive(Debug, Clone)]
pub struct Egress<R, C> {
    resolver: R,
    connector: C,
}

impl<R: NameResolver, C: Connector> Egress<R, C> {
    /// An egress layer resolving with `resolver` and connecting with `connector`.
    pub fn new(resolver: R, connector: C) -> Self {
        Egress {
            resolver,
            connector,
        }
    }

    /// Sends `request` to `endpoint` for `call`, following redirects only to destinations the operation in
    /// flight lists, and writes the call's record to `log`, whatever its outcome.
    pub fn send(
        &self,
        call: &Call<'_>,
        endpoint: Endpoint,
        request: Request,
        log: &mut impl CallLog,
    ) -> Outcome {
        let (hops, outcome) = self.hops(call, endpoint, request);
        let recorded = match &outcome {
            Outcome::Completed(_) => CallOutcome::Completed(Nothing {}),
            Outcome::Failed(_) => CallOutcome::Failed(Nothing {}),
            Outcome::Unreachable | Outcome::NoAnswer => CallOutcome::Unknown(Nothing {}),
            Outcome::Refused(refusal) => CallOutcome::Refused(Refused {
                reason: refusal.reason(),
            }),
        };
        log.record(OutboundCallRecord {
            transaction: call.transaction.clone(),
            provider: call.declaration.provider.clone(),
            module_version: call.declaration.version.clone(),
            operation: call.in_flight.clone(),
            secrets: call.secrets.to_vec(),
            started_at: call.started_at.clone(),
            hops,
            outcome: recorded,
        });
        outcome
    }

    /// Makes the call's connections, each checked, and returns them as hops with how the call ended.
    fn hops(&self, call: &Call<'_>, endpoint: Endpoint, request: Request) -> (Vec<Hop>, Outcome) {
        let mut hops = Vec::new();
        let listed = call
            .declaration
            .outside
            .operations
            .iter()
            .find(|o| &o.name == call.in_flight)
            .filter(|_| call.asked == call.in_flight.as_str())
            .map(|o| o.destinations.as_slice());
        let Some(listed) = listed else {
            hops.extend(unconnected(&endpoint));
            return (hops, Outcome::Refused(Refusal::UndeclaredOperation));
        };
        let (mut current, mut request) = (endpoint, request);
        loop {
            let undeclared = if hops.is_empty() {
                Refusal::UndeclaredDestination
            } else {
                Refusal::RedirectToUndeclaredDestination
            };
            let destination = call.declaration.outside.destinations.iter().find(|d| {
                listed.contains(&d.name)
                    && d.scheme == current.scheme
                    && d.host.as_str() == current.host
                    && d.port.get() == current.port
            });
            let Some(destination) = destination else {
                hops.extend(unconnected(&current));
                return (hops, Outcome::Refused(undeclared));
            };
            let mut hop = Hop {
                scheme: destination.scheme,
                host: destination.host.clone(),
                port: destination.port,
                destination: Some(destination.name.clone()),
                address: None,
            };

            // Resolved once for this connection; the connector gets the address checked, never the name.
            let answer = self.resolver.resolve(&destination.host, destination.port);
            let checked = answer
                .first()
                .copied()
                .filter(|_| answer.iter().all(|a| allows(&destination.addresses, *a)))
                .and_then(|a| Address::new(a.to_string()).map(|text| (a, text)));
            let Some((checked, text)) = checked else {
                hops.push(hop);
                return (hops, Outcome::Refused(Refusal::AddressNotAllowed));
            };
            let dial = Dial {
                scheme: destination.scheme,
                name: &destination.host,
                to: SocketAddr::new(checked, destination.port.get()),
            };
            let answer = self.connector.exchange(&dial, &request);
            if answer != Err(ConnectError::NotVerified) {
                hop.address = Some(text);
            }
            hops.push(hop);
            let response = match answer {
                Ok(response) => response,
                Err(ConnectError::NotVerified) => {
                    return (hops, Outcome::Refused(Refusal::NotVerified));
                }
                Err(ConnectError::Unreachable) => return (hops, Outcome::Unreachable),
                Err(ConnectError::NoAnswer) => return (hops, Outcome::NoAnswer),
            };
            if (200..300).contains(&response.status) {
                return (hops, Outcome::Completed(response));
            }
            let location = match response.status {
                301 | 302 | 303 | 307 | 308 => response.header("location"),
                _ => None,
            };
            let Some(location) = location.filter(|_| hops.len() < MAX_HOPS) else {
                return (hops, Outcome::Failed(response));
            };
            let Some((next, target)) = follow(&current, location) else {
                return (
                    hops,
                    Outcome::Refused(Refusal::RedirectToUndeclaredDestination),
                );
            };
            request = request.redirected(response.status, target);
            current = next;
        }
    }
}

/// The hop of an endpoint no connection was made to, if the contract can record it: a host and a non-zero port.
fn unconnected(endpoint: &Endpoint) -> Option<Hop> {
    Some(Hop {
        scheme: endpoint.scheme,
        host: Host::new(endpoint.host.clone())?,
        port: NonZeroU16::new(endpoint.port)?,
        destination: None,
        address: None,
    })
}

/// Where a redirect from `current` to `location` leads: the endpoint and the origin-form target, or `None` if it
/// leads nowhere a destination could be. An absolute `https` or `wss` URL, a scheme-relative one or an absolute
/// path is read; plain `http`, any other scheme, a user name, an IP literal, a bad port or a relative path is not.
/// The host is lower-cased, as a URL's is, and the fragment dropped.
fn follow(current: &Endpoint, location: &str) -> Option<(Endpoint, String)> {
    let location = location.trim_matches([' ', '\t']);
    let location = location.split_once('#').map_or(location, |(l, _)| l);
    if location.starts_with('/') && !location.starts_with("//") {
        return origin_form(location).then(|| (current.clone(), location.to_owned()));
    }
    let (scheme, rest) = match location.strip_prefix("//") {
        Some(rest) => (current.scheme, rest),
        None => {
            let (scheme, rest) = location.split_once("://")?;
            let scheme = match scheme.to_ascii_lowercase().as_str() {
                "https" => Scheme::Https,
                "wss" => Scheme::Wss,
                _ => return None,
            };
            (scheme, rest)
        }
    };
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok().filter(|p| *p != 0)?),
        None => (authority, 443),
    };
    let plain = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'.';
    if host.is_empty() || !host.bytes().all(plain) {
        return None;
    }
    let target = match tail {
        "" => "/".to_owned(),
        _ if tail.starts_with('?') => format!("/{tail}"),
        _ => tail.to_owned(),
    };
    origin_form(&target).then(|| {
        let endpoint = Endpoint {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
        };
        (endpoint, target)
    })
}

/// Whether `address` is within `addresses`: for `public`, globally routable, in no special-purpose range.
fn allows(addresses: &Addresses, address: IpAddr) -> bool {
    match addresses {
        Addresses::Public(_) => !SPECIAL_PURPOSE.iter().any(|(range, len)| {
            range
                .parse::<IpAddr>()
                .is_ok_and(|range| within(address, range, *len))
        }),
        Addresses::Blocks(blocks) => blocks.iter().any(|block| {
            let (ip, len) = block.parts();
            within(address, ip, len)
        }),
    }
}

/// Whether `ip` lies within the block `outer/outer_len`. An IPv4 address and an IPv6 block never match, so an
/// IPv4-mapped IPv6 address is judged as IPv6, where `::ffff:0:0/96` is special-purpose.
fn within(ip: IpAddr, outer: IpAddr, outer_len: u8) -> bool {
    let bits = |ip: IpAddr| match ip {
        IpAddr::V4(v4) => (u128::from(u32::from(v4)) << 96, true),
        IpAddr::V6(v6) => (u128::from(v6), false),
    };
    let ((a, a4), (b, b4)) = (bits(ip), bits(outer));
    if a4 != b4 {
        return false;
    }
    let mask = if outer_len == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(outer_len))
    };
    a & mask == b & mask
}
