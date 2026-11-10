//! The production world (`T-014-05`): the credential proxy and the egress layer as the module platform's
//! [`World`].
//!
//! The platform makes its own checks first, from its own state, and tells this world what each provider's module
//! is handling: the release it runs ([`World::running`]), the operation it handles and the outside request in
//! flight (`begin_*` and `end_*`). The proxy then judges each lookup again from what it was told, so a secret is
//! answered only inside the outside request in flight, for that request's transaction and operation, from the
//! caller's own provider's keys (`DEC-9`, `REQ-MDL-6`, `REQ-MDL-9`, `REQ-MDL-17`). Every lookup the platform
//! serves leaves one [`Use`](super::Use), refused ones included. Outbound calls go through [`Egress`] over a
//! [`NameResolver`] and a [`Connector`], in production the operating system's resolver behind a
//! [`ThreadedResolver`](crate::egress::ThreadedResolver), so each lookup is given up at the deadline, and
//! [`TlsConnector`](crate::egress::TlsConnector): each host is resolved once per connection, only the checked
//! address is dialled (`B-17`), and a peer not verified over TLS is refused (`B-18`).
//!
//! This is the credential proxy's path, a `DEC-20` path.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Instant;

use rutent_contracts::module_interface::{
    AdmissionArtefact, Identifier, Name, OutboundCallRecord, SecretLookup, SecretRefusalReason,
    SecretValue,
};
use rutent_contracts::provider_role::{Advertisement, Statement};

use super::resolver::Resolver;
use super::shared::{Caller, CredentialProxy, InFlight, Refusal};
use crate::egress::{self, Call, Connector, Egress, NameResolver, Outcome};
use crate::module::platform::World;
use crate::module::types::Endpoint;

/// Each provider's own state, as the outside part reads it.
pub trait StateStore {
    /// The value of `provider`'s state entry `key`, if any.
    fn read(&mut self, provider: &str, key: &str) -> Option<String>;
}

/// A store with no state in it: every read finds nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoState;

impl StateStore for NoState {
    fn read(&mut self, _: &str, _: &str) -> Option<String> {
        None
    }
}

/// The module platform's production world: the shared credential proxy over its resolvers, the egress layer over
/// a name resolver and a connector, and each provider's state.
pub struct ProductionWorld<R, N, C, S = NoState> {
    proxy: CredentialProxy<R>,
    egress: Egress<N, C>,
    state: S,
    advertisements: BTreeMap<String, Statement<Advertisement>>,
    unadmitted: Vec<(Identifier, Refusal)>,
}

impl<R: Resolver, N: NameResolver, C: Connector> ProductionWorld<R, N, C, NoState> {
    /// The world over `proxy`, resolving names with `names` and connecting with `connector`, holding no state.
    pub fn new(proxy: CredentialProxy<R>, names: N, connector: C) -> Self {
        ProductionWorld::with_state(proxy, names, connector, NoState)
    }
}

impl<R: Resolver, N: NameResolver, C: Connector, S: StateStore> ProductionWorld<R, N, C, S> {
    /// [`ProductionWorld::new`], reading each provider's state from `state`.
    pub fn with_state(proxy: CredentialProxy<R>, names: N, connector: C, state: S) -> Self {
        ProductionWorld {
            proxy,
            egress: Egress::new(names, connector),
            state,
            advertisements: BTreeMap::new(),
            unadmitted: Vec::new(),
        }
    }

    /// Holds `advertisement`, the one the catalogue accepted for its provider, for the proxy to admit the
    /// provider's releases against when they start. A later one replaces it, and the release already running is
    /// judged against its keys from the next signing on: a statement key it revokes signs nothing more, with no
    /// restart. An advertisement of another deployment is refused, and nothing is held; so is one naming a module
    /// whose running release is the operator's own (`T-016-04`).
    pub fn advertise(&mut self, advertisement: Statement<Advertisement>) -> Result<(), Refusal> {
        self.proxy.advertise(&advertisement)?;
        self.advertisements
            .insert(advertisement.statement.provider.clone(), advertisement);
        Ok(())
    }

    /// The proxy, to read what it recorded ([`CredentialProxy::take_uses`]) or what it has in flight.
    pub fn proxy(&mut self) -> &mut CredentialProxy<R> {
        &mut self.proxy
    }

    /// The outside request the proxy has in flight for `provider`'s module, if any.
    pub fn in_flight(&self, provider: &Identifier) -> Option<InFlight> {
        self.proxy.in_flight(provider)
    }

    /// The releases that started but that the proxy didn't admit, and why, since this was last asked. Every
    /// lookup of such a provider is refused.
    pub fn take_unadmitted(&mut self) -> Vec<(Identifier, Refusal)> {
        std::mem::take(&mut self.unadmitted)
    }
}

impl<R: Resolver, N: NameResolver, C: Connector, S: StateStore> World
    for ProductionWorld<R, N, C, S>
{
    fn secret(&mut self, _: &str, _: &str) -> Option<String> {
        // Every lookup goes through `lookup`, with its caller and the platform's verdict; nothing answers by name
        // alone.
        None
    }

    fn state(&mut self, provider: &str, key: &str) -> Option<String> {
        self.state.read(provider, key)
    }

    fn route(&mut self, _: &Endpoint) -> Option<(IpAddr, Option<Endpoint>)> {
        // Every outbound call goes through `send`; nothing is routed by lookup.
        None
    }

    fn deliver(&mut self, _: &Endpoint, _: &str) {}

    fn running(&mut self, artefact: &AdmissionArtefact) {
        let provider = artefact.declaration().provider.clone();
        // The operator's own release, signed with the operator key the proxy's settings name, is a module the
        // operator publishes, such as a public node's holding account (`T-016-04`): it is admitted with no
        // advertisement, whatever advertisement the world holds under its name, and that one is dropped. Any other
        // release is admitted against the advertisement the catalogue accepted for its provider.
        let operators = self.proxy.is_operators_release(artefact);
        let admitted = match self.advertisements.get(provider.as_str()) {
            Some(advertisement) if !operators => self.proxy.admit(artefact, advertisement),
            _ => self.proxy.admit_operator(artefact),
        };
        if operators && admitted.is_ok() {
            self.advertisements.remove(provider.as_str());
        }
        if let Err(refusal) = admitted {
            // The proxy keeps no release of the provider's then, so it refuses every lookup until one is admitted.
            self.proxy.retire(&provider);
            self.unadmitted.push((provider, refusal));
        }
    }

    fn begin_operation(&mut self, caller: &Caller, transaction: &Identifier, due: Instant) {
        // A refusal here (no release admitted) leaves nothing in flight, so the proxy refuses every lookup.
        let _ = self
            .proxy
            .begin_operation_until(caller, transaction, Some(due));
    }

    fn end_operation(&mut self, caller: &Caller) {
        self.proxy.end_operation(caller);
    }

    fn begin_request(&mut self, caller: &Caller, transaction: &Identifier, operation: &Name) {
        let _ = self.proxy.begin_request(caller, transaction, operation);
    }

    fn end_request(&mut self, caller: &Caller) {
        self.proxy.end_request(caller);
    }

    fn lookup(
        &mut self,
        caller: &Caller,
        lookup: &SecretLookup,
        judged: Result<(), Refusal>,
    ) -> Result<SecretValue, SecretRefusalReason> {
        match judged {
            // Refused by the platform: recorded, and the store isn't asked.
            Err(refusal) => Err(self.proxy.refuse(caller, lookup, refusal).wire_reason()),
            Ok(()) => self
                .proxy
                .lookup(caller, lookup)
                .map_err(|refusal| refusal.wire_reason()),
        }
    }

    fn statement_key(
        &mut self,
        caller: &Caller,
        transaction: &Identifier,
    ) -> Result<SecretValue, SecretRefusalReason> {
        self.proxy
            .statement_key(caller, transaction)
            .map_err(|refusal| refusal.wire_reason())
    }

    fn send(
        &mut self,
        call: &Call<'_>,
        endpoint: egress::Endpoint,
        request: egress::Request,
        log: &mut Vec<OutboundCallRecord>,
    ) -> Outcome {
        self.egress.send(call, endpoint, request, log)
    }
}
