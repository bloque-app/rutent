//! Name resolution for the egress layer, behind a trait so a test resolves from a table of its own.

use std::net::{IpAddr, ToSocketAddrs};
use std::num::NonZeroU16;

use rutent_contracts::module_interface::Host;

/// Resolves a declared host name to the addresses it has now.
///
/// The egress layer asks once per connection, checks every address given against the destination's declared
/// addresses, and hands the connector the one it checked. Nothing else resolves the name for that connection.
pub trait NameResolver {
    /// The addresses `host` resolves to, in the resolver's order; none if it doesn't resolve.
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr>;
}

impl<T: NameResolver + ?Sized> NameResolver for &T {
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr> {
        (**self).resolve(host, port)
    }
}

impl<T: NameResolver + ?Sized> NameResolver for std::sync::Arc<T> {
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr> {
        (**self).resolve(host, port)
    }
}

impl<T: NameResolver + ?Sized> NameResolver for Box<T> {
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr> {
        (**self).resolve(host, port)
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
