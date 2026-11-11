//! Name resolution for the egress layer, behind a trait so a test resolves from a table of its own.

use std::collections::BTreeMap;
use std::net::{IpAddr, ToSocketAddrs};
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use rutent_contracts::module_interface::{Host, Identifier};

/// One name lookup as the egress layer asks it: the host and port, the provider whose call it serves, and when
/// the operation that call serves must end.
#[derive(Debug, Clone, Copy)]
pub struct Lookup<'a> {
    /// The declared host.
    pub host: &'a Host,
    /// The declared port.
    pub port: NonZeroU16,
    /// The provider whose outbound call it serves.
    pub provider: &'a Identifier,
    /// When the operation must end, if it has a deadline.
    pub due: Option<Instant>,
}

/// Why a lookup gave no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupRefused {
    /// The operation's deadline passed before the resolver answered (`REQ-MDL-11`). Nothing was dialled.
    DeadlinePassed,
    /// The provider already has as many lookups in flight as it may: each one abandoned at a deadline still runs
    /// until the operating system answers it, and they are bounded per provider.
    TooManyInFlight,
}

/// Resolves a declared host name to the addresses it has now.
///
/// The egress layer asks once per connection, checks every address given against the destination's declared
/// addresses, and hands the connector the one it checked. Nothing else resolves the name for that connection.
pub trait NameResolver {
    /// The addresses `host` resolves to, in the resolver's order; none if it doesn't resolve.
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr>;

    /// The egress layer's question: [`NameResolver::resolve`], given up at `lookup`'s deadline. The default asks
    /// [`NameResolver::resolve`] and waits for it, which suits a resolver that answers from memory; one that may
    /// block, as the operating system's does, is put behind a [`ThreadedResolver`].
    fn resolve_for(&self, lookup: &Lookup<'_>) -> Result<Vec<IpAddr>, LookupRefused> {
        Ok(self.resolve(lookup.host, lookup.port))
    }
}

impl<T: NameResolver + ?Sized> NameResolver for &T {
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr> {
        (**self).resolve(host, port)
    }

    fn resolve_for(&self, lookup: &Lookup<'_>) -> Result<Vec<IpAddr>, LookupRefused> {
        (**self).resolve_for(lookup)
    }
}

impl<T: NameResolver + ?Sized> NameResolver for std::sync::Arc<T> {
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr> {
        (**self).resolve(host, port)
    }

    fn resolve_for(&self, lookup: &Lookup<'_>) -> Result<Vec<IpAddr>, LookupRefused> {
        (**self).resolve_for(lookup)
    }
}

impl<T: NameResolver + ?Sized> NameResolver for Box<T> {
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr> {
        (**self).resolve(host, port)
    }

    fn resolve_for(&self, lookup: &Lookup<'_>) -> Result<Vec<IpAddr>, LookupRefused> {
        (**self).resolve_for(lookup)
    }
}

/// The most lookups one provider may have in flight in a [`ThreadedResolver`] by default.
pub const MAX_LOOKUPS_IN_FLIGHT: usize = 4;

/// A resolver whose every lookup with a deadline runs on a thread of its own, waited for only until the deadline
/// (`REQ-MDL-11`). `getaddrinfo` takes no timeout, so a slow or hanging answer would otherwise hold the platform,
/// and every other provider, past the operation's deadline. A lookup given up is abandoned: its thread ends when
/// the resolver returns, and its answer is dropped. The threads are bounded: each provider may have at most
/// `cap` lookups in flight, abandoned ones included, and one more is refused ([`LookupRefused::TooManyInFlight`]).
///
/// The production resolver is `ThreadedResolver::new(SystemResolver)`.
pub struct ThreadedResolver<R> {
    inner: Arc<R>,
    cap: usize,
    in_flight: Arc<Mutex<BTreeMap<String, usize>>>,
}

impl<R> core::fmt::Debug for ThreadedResolver<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ThreadedResolver")
            .field("cap", &self.cap)
            .finish()
    }
}

impl<R: NameResolver + Send + Sync + 'static> ThreadedResolver<R> {
    /// `inner`, each lookup with a deadline on a thread of its own, [`MAX_LOOKUPS_IN_FLIGHT`] per provider.
    pub fn new(inner: R) -> Self {
        ThreadedResolver {
            inner: Arc::new(inner),
            cap: MAX_LOOKUPS_IN_FLIGHT,
            in_flight: Arc::default(),
        }
    }

    /// The same resolver, with at most `cap` lookups in flight per provider.
    pub fn with_cap(mut self, cap: usize) -> Self {
        self.cap = cap;
        self
    }

    /// How many lookups `provider` has in flight now, abandoned ones included.
    pub fn in_flight(&self, provider: &Identifier) -> usize {
        self.in_flight
            .lock()
            .map(|counts| counts.get(provider.as_str()).copied().unwrap_or_default())
            .unwrap_or_default()
    }
}

/// One of a provider's places among its lookups in flight, given back when it drops: when the lookup's thread
/// ends, whether its resolver returned or panicked.
struct Place {
    in_flight: Arc<Mutex<BTreeMap<String, usize>>>,
    provider: String,
}

impl Drop for Place {
    fn drop(&mut self) {
        give_back(&self.in_flight, &self.provider);
    }
}

/// Gives back one of `provider`'s lookups in flight.
fn give_back(in_flight: &Mutex<BTreeMap<String, usize>>, provider: &str) {
    if let Ok(mut counts) = in_flight.lock()
        && let Some(count) = counts.get_mut(provider)
    {
        *count = count.saturating_sub(1);
        if *count == 0 {
            counts.remove(provider);
        }
    }
}

impl<R: NameResolver + Send + Sync + 'static> NameResolver for ThreadedResolver<R> {
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr> {
        self.inner.resolve(host, port)
    }

    fn resolve_for(&self, lookup: &Lookup<'_>) -> Result<Vec<IpAddr>, LookupRefused> {
        let Some(due) = lookup.due else {
            return Ok(self.inner.resolve(lookup.host, lookup.port));
        };
        if Instant::now() >= due {
            return Err(LookupRefused::DeadlinePassed);
        }
        let provider = lookup.provider.as_str().to_owned();
        {
            let mut counts = self
                .in_flight
                .lock()
                .map_err(|_| LookupRefused::TooManyInFlight)?;
            let count = counts.entry(provider.clone()).or_default();
            if *count >= self.cap {
                return Err(LookupRefused::TooManyInFlight);
            }
            *count += 1;
        }
        let (sender, answer) = mpsc::channel();
        let (inner, host, port) = (Arc::clone(&self.inner), lookup.host.clone(), lookup.port);
        let place = Place {
            in_flight: Arc::clone(&self.in_flight),
            provider,
        };
        // The thread owns the place, so it is given back however the thread ends, a panic included; a thread that
        // can't be spawned drops it here.
        let spawned = std::thread::Builder::new()
            .name("rutent-name-lookup".to_owned())
            .spawn(move || {
                let _place = place;
                let addresses = inner.resolve(&host, port);
                // The receiver is gone if the lookup was given up; the answer is dropped.
                let _ = sender.send(addresses);
            });
        if spawned.is_err() {
            return Err(LookupRefused::TooManyInFlight);
        }
        match answer.recv_timeout(due.saturating_duration_since(Instant::now())) {
            Ok(addresses) => Ok(addresses),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(LookupRefused::DeadlinePassed),
            // The resolver's thread ended with no answer, as a panic ends it: the name resolved to nothing.
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(Vec::new()),
        }
    }
}

/// The operating system's resolver (`getaddrinfo`), with nothing cached here: each call asks again, so a
/// changed answer is seen, and checked, on the next connection.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

impl NameResolver for SystemResolver {
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr> {
        (host.as_str(), port.get())
            .to_socket_addrs()
            .map(|addresses| addresses.map(|a| a.ip()).collect())
            .unwrap_or_default()
    }
}
