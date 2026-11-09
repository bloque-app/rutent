//! The in-process reference double the module-boundary suite runs against (`T-003-03`). Built only with the
//! `test-support` feature: it is a test stand-in, never a runtime.
//!
//! It is a format binding with no format: a "binary" names a [`ModuleSpec`] the double built, and running it
//! interprets the spec's steps. Everything a real binding enforces between the parts, the proxy, the egress layer
//! and the output check, it enforces in-process, from the module's admitted declaration. Its two languages are
//! nominal: they show the suite's coverage accounting, not `AC-20` itself.
//!
//! Each enforcement has a builder that breaks it alone, so the suite's own tests can show every check flagged by
//! itself. A broken egress check still writes the refusal into its record, but lets the request through: the
//! record stays valid, and only what the endpoint received gives it away.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::num::NonZeroU16;

use rutent_contracts::module_interface::{
    Acceptance, AcceptanceMode, Accepted, Address, Addresses, AdmissionArtefact, Build,
    CallOutcome, CallRefusalReason, CallRefused, Capabilities, CrossModuleCall, CrossModuleOutcome,
    Declaration, Destination, Digest, DigestAlgorithm, DigestValue, Hop, Host, Identifier, Name,
    Nothing, OperationKind, OutboundCallRecord, PartName, Parts, RefusalReason, Refused,
    ReleaseAssertion, RouteReference, SPECIAL_PURPOSE, SecretLookup, SecretRefusalReason, Time,
    Widening, validate_lookup, widening,
};
use rutent_contracts::provider_role::{
    Operation, RESULT_SHAPE, ResultBody, Signature, SignatureScheme, Statement,
};
use serde_json::{Value, json};

use super::sha256::hex_digest;
use super::{
    AdmissionRefusal, Admitted, BoundaryHarness, Endpoint, Events, Handled, LookupEvent,
    LookupOutcome, ModuleBinding, ModuleSpec, Output, PlatformRefusal, ProcessId, StateEvent, Step,
};
use crate::role::placeholder_signature;

/// The platform version the double says made its machine code.
pub const PLATFORM_VERSION: &str = "reference-double-0.1.0";

const BINARY_PREFIX: &str = "reference-module/";

/// The enforcements a [`ReferenceDouble`] can be built without, one per builder.
#[derive(Debug, Clone, Default)]
struct Breaks {
    any_providers_secret: bool,
    misstates_lookup_refusals: bool,
    resolves_undeclared_secrets: bool,
    shared_state: bool,
    sandboxed_reaches_proxy: bool,
    connects_undeclared: bool,
    shared_egress_allowlist: bool,
    follows_any_redirect: bool,
    checks_only_the_first_redirect: bool,
    trusts_resolution: bool,
    checks_the_address_on_the_first_hop_only: bool,
    checks_ipv4_addresses_only: bool,
    records_another_version: bool,
    records_no_secrets: bool,
    records_every_declared_secret: bool,
    refuses_every_cross_module_call: bool,
    delivers_undeclared_calls: bool,
    checks_only_the_callee: bool,
    misstates_call_refusals: bool,
    trusts_kind_and_transaction: bool,
    no_output_limit: bool,
    refuses_at_the_limit: bool,
    pads_short: bool,
    skips_digest_check: bool,
    checks_only_the_outside_digest: bool,
    accepts_widening_automatically: bool,
    ignores_secret_widenings: bool,
    ignores_callee_widenings: bool,
    ignores_address_widenings: bool,
    enforces_the_pending_declaration: bool,
    honours_the_pending_declaration_in_proxy_and_calls: bool,
    holds_non_starting_releases_as_waiting: bool,
    keeps_every_declarations_reach: bool,
    restarts_everyone: bool,
    drops_previous_on_failed_start: bool,
    refuses_releases_that_cannot_start: bool,
    one_process: bool,
    refuses_every_redirect: bool,
    refuses_declared_secrets: bool,
    ignores_steps: bool,
}

#[derive(Debug, Clone, Default)]
struct Slot {
    accepted: Vec<AdmissionArtefact>,
    pending: Option<(AdmissionArtefact, usize)>,
    built: BTreeMap<String, usize>,
    active: Option<String>,
    process: Option<ProcessId>,
}

/// An in-process format binding, correct unless built with one of its breakers.
#[derive(Debug, Clone)]
pub struct ReferenceDouble {
    languages: Vec<String>,
    output_limit: usize,
    step_limit: Option<usize>,
    built: Vec<ModuleSpec>,
    secrets: BTreeMap<(String, String), String>,
    delivered: BTreeMap<String, Vec<String>>,
    state: BTreeMap<(String, String), String>,
    endpoints: BTreeMap<Endpoint, (IpAddr, Option<Endpoint>)>,
    received: BTreeMap<Endpoint, Vec<String>>,
    operations_received: BTreeMap<String, usize>,
    providers: BTreeMap<String, Slot>,
    next_process: ProcessId,
    clock: u32,
    events: Events,
    breaks: Breaks,
}

impl Default for ReferenceDouble {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! breakers {
    ($($(#[$doc:meta])* $name:ident => $flag:ident;)*) => {
        $(
            $(#[$doc])*
            pub fn $name(mut self) -> Self {
                self.breaks.$flag = true;
                self
            }
        )*
    };
}

impl ReferenceDouble {
    /// A correct double, in two nominal languages.
    pub fn new() -> Self {
        Self {
            languages: vec!["reference-a".to_owned(), "reference-b".to_owned()],
            output_limit: 0,
            step_limit: None,
            built: Vec::new(),
            secrets: BTreeMap::new(),
            delivered: BTreeMap::new(),
            state: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            received: BTreeMap::new(),
            operations_received: BTreeMap::new(),
            providers: BTreeMap::new(),
            next_process: 1000,
            clock: 0,
            events: Events::default(),
            breaks: Breaks::default(),
        }
    }

    /// Offers one language only.
    pub fn with_one_language(mut self) -> Self {
        self.languages.truncate(1);
        self
    }

    /// Stops a module after `steps` of its steps in one operation, as a binding's per-operation limit on host
    /// calls would, and refuses its answer as [`PlatformRefusal::LimitReached`] (`REQ-MDL-11`). The double
    /// has no such limit unless given one.
    pub fn with_step_limit(mut self, steps: usize) -> Self {
        self.step_limit = Some(steps);
        self
    }

    breakers! {
        /// Its proxy answers a lookup from another provider's secrets when the caller has none by that name.
        answering_from_any_providers_secrets => any_providers_secret;
        /// Its proxy refuses a missing secret as undeclared.
        misstating_lookup_refusals => misstates_lookup_refusals;
        /// Its proxy resolves a secret the module doesn't declare, if the provider holds one by that name.
        resolving_undeclared_secrets => resolves_undeclared_secrets;
        /// Its modules read one state shared by every provider.
        sharing_state_between_providers => shared_state;
        /// Its sandboxed parts reach the proxy.
        letting_the_sandboxed_part_reach_the_proxy => sandboxed_reaches_proxy;
        /// It records a call to an undeclared endpoint as refused, but makes it.
        connecting_to_undeclared_destinations => connects_undeclared;
        /// Its egress allows what any running provider declares.
        sharing_one_egress_allowlist => shared_egress_allowlist;
        /// It records a redirect to an undeclared destination as refused, but follows it.
        following_any_redirect => follows_any_redirect;
        /// It checks the first redirect of a chain only.
        checking_only_the_first_redirect => checks_only_the_first_redirect;
        /// It records a connection to an address outside the declared ones as refused, but connects.
        trusting_resolution => trusts_resolution;
        /// It checks the connected address on the first hop only, not after a redirect.
        checking_the_address_on_the_first_hop_only => checks_the_address_on_the_first_hop_only;
        /// It checks IPv4 addresses only.
        checking_ipv4_addresses_only => checks_ipv4_addresses_only;
        /// Its records name another module version than the one that made the call.
        recording_another_module_version => records_another_version;
        /// Its records name no secret, whatever the call used.
        recording_no_secrets => records_no_secrets;
        /// Its records name every secret the operation declares, whatever the call used.
        recording_every_declared_secret => records_every_declared_secret;
        /// It refuses every cross-module call, a declared one as its callee being unavailable.
        refusing_every_cross_module_call => refuses_every_cross_module_call;
        /// It delivers a cross-module call to a callee the caller doesn't declare.
        delivering_undeclared_calls => delivers_undeclared_calls;
        /// It checks a cross-module call's callee, not its kind.
        checking_only_the_callee => checks_only_the_callee;
        /// It records a refused cross-module call with another reason than the one it refused it for.
        misstating_call_refusals => misstates_call_refusals;
        /// Its output check takes any kind and transaction the output names.
        trusting_the_outputs_kind_and_transaction => trusts_kind_and_transaction;
        /// It ignores the output limit it was configured with.
        without_an_output_limit => no_output_limit;
        /// It refuses output exactly at the limit, as well as over it.
        refusing_output_at_the_limit => refuses_at_the_limit;
        /// Its modules pad their output a byte short of what the spec asks.
        padding_output_short => pads_short;
        /// It admits binaries whatever their digests.
        skipping_the_digest_check => skips_digest_check;
        /// It compares the outside part's digest only.
        checking_only_the_outside_digest => checks_only_the_outside_digest;
        /// It accepts a widening release automatically.
        accepting_widening_releases_automatically => accepts_widening_automatically;
        /// It doesn't count a new secret as a widening.
        ignoring_secret_widenings => ignores_secret_widenings;
        /// It doesn't count a new callee as a widening.
        ignoring_callee_widenings => ignores_callee_widenings;
        /// It doesn't count grown addresses as a widening.
        ignoring_address_widenings => ignores_address_widenings;
        /// Its egress enforces a release waiting for acceptance, not the running one.
        enforcing_the_pending_declaration => enforces_the_pending_declaration;
        /// Its proxy and cross-module check honour a release waiting for acceptance, not the running one.
        honouring_the_pending_declaration_in_proxy_and_calls => honours_the_pending_declaration_in_proxy_and_calls;
        /// It holds a release that won't start as waiting for a fresh acceptance, so it is never started.
        holding_non_starting_releases_as_waiting => holds_non_starting_releases_as_waiting;
        /// Its egress allows what any accepted release of the provider declared.
        keeping_every_declarations_reach => keeps_every_declarations_reach;
        /// Restarting one provider restarts every provider.
        restarting_every_provider => restarts_everyone;
        /// A release that fails to start leaves its provider with no module running.
        dropping_the_previous_version_on_a_failed_start => drops_previous_on_failed_start;
        /// It refuses at admission a release that won't start, so no upgrade ever fails.
        refusing_releases_that_cannot_start => refuses_releases_that_cannot_start;
        /// Every provider runs in one process.
        running_every_provider_in_one_process => one_process;
        /// It refuses every redirect, a declared one included.
        refusing_every_redirect => refuses_every_redirect;
        /// Its proxy refuses every lookup, a declared one included.
        refusing_declared_secrets => refuses_declared_secrets;
        /// Its modules skip their steps: they attempt nothing, and so show no refusal.
        ignoring_the_modules_steps => ignores_steps;
    }

    fn now(&mut self) -> Time {
        self.clock += 1;
        let (h, m, s) = (self.clock / 3600, (self.clock / 60) % 60, self.clock % 60);
        Time::new(format!("2026-11-07T{:02}:{m:02}:{s:02}Z", h % 24)).expect("a time")
    }

    fn new_process(&mut self) -> ProcessId {
        if self.breaks.one_process {
            return 1;
        }
        self.next_process += 1;
        self.next_process
    }

    fn start(&mut self, provider: &str, version: &str, spec: usize) {
        let starts = self.built[spec].starts;
        let process = self.new_process();
        let slot = self.providers.entry(provider.to_owned()).or_default();
        slot.built.insert(version.to_owned(), spec);
        if starts {
            slot.active = Some(version.to_owned());
            slot.process = Some(process);
        } else if self.breaks.drops_previous_on_failed_start {
            slot.active = None;
            slot.process = None;
        }
    }

    fn active(&self, provider: &str) -> Option<(Declaration, ModuleSpec, String)> {
        let slot = self.providers.get(provider)?;
        let version = slot.active.clone()?;
        let spec = self.built[*slot.built.get(&version)?].clone();
        let declaration = slot
            .accepted
            .iter()
            .rev()
            .find(|a| a.declaration().version.as_str() == version)?
            .declaration()
            .clone();
        Some((declaration, spec, version))
    }

    /// The declarations the egress layer allows a provider's call by: its running one, unless broken.
    fn egress_declarations(&self, provider: &str, running: &Declaration) -> Vec<Declaration> {
        let slot = self.providers.get(provider);
        if self.breaks.enforces_the_pending_declaration
            && let Some((pending, _)) = slot.and_then(|s| s.pending.as_ref())
        {
            return vec![pending.declaration().clone()];
        }
        let mut declarations = vec![running.clone()];
        if self.breaks.keeps_every_declarations_reach {
            declarations.extend(
                slot.into_iter()
                    .flat_map(|s| s.accepted.iter().map(|a| a.declaration().clone())),
            );
        }
        if self.breaks.shared_egress_allowlist {
            for (name, _) in self
                .providers
                .iter()
                .filter(|(n, _)| n.as_str() != provider)
            {
                if let Some((declaration, _, _)) = self.active(name) {
                    declarations.push(declaration);
                }
            }
        }
        declarations
    }

    /// The declaration the proxy and the cross-module check hold a provider's module to: its running one, unless
    /// broken.
    fn ruling_declaration(&self, provider: &str, running: &Declaration) -> Declaration {
        match self
            .providers
            .get(provider)
            .and_then(|s| s.pending.as_ref())
        {
            Some((pending, _))
                if self
                    .breaks
                    .honours_the_pending_declaration_in_proxy_and_calls =>
            {
                pending.declaration().clone()
            }
            _ => running.clone(),
        }
    }

    fn proxy(
        &mut self,
        provider: &str,
        declaration: &Declaration,
        lookup: &SecretLookup,
    ) -> LookupOutcome {
        let name = lookup.name.as_str().to_owned();
        let declared = validate_lookup(declaration, lookup).is_ok();
        if !declared && !self.breaks.resolves_undeclared_secrets {
            return LookupOutcome::Refused(SecretRefusalReason::UndeclaredSecret);
        }
        if self.breaks.refuses_declared_secrets {
            return LookupOutcome::Refused(SecretRefusalReason::NotFound);
        }
        let own = self
            .secrets
            .get(&(provider.to_owned(), name.clone()))
            .cloned();
        let any = || {
            self.secrets
                .iter()
                .find(|((_, n), _)| *n == name)
                .map(|(_, v)| v.clone())
        };
        match own.or_else(|| self.breaks.any_providers_secret.then(any).flatten()) {
            Some(value) => {
                self.delivered
                    .entry(provider.to_owned())
                    .or_default()
                    .push(value);
                LookupOutcome::Resolved
            }
            None if self.breaks.misstates_lookup_refusals => {
                LookupOutcome::Refused(SecretRefusalReason::UndeclaredSecret)
            }
            None => LookupOutcome::Refused(SecretRefusalReason::NotFound),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn egress(
        &mut self,
        provider: &str,
        declaration: &Declaration,
        version: &str,
        transaction: &str,
        operation: &str,
        endpoint: &Endpoint,
        body: &str,
        secrets: Vec<Name>,
    ) {
        let allowed_by = self.egress_declarations(provider, declaration);
        let find = |current: &Endpoint| -> Option<Destination> {
            allowed_by.iter().enumerate().find_map(|(i, d)| {
                let listed: Vec<Name> = d
                    .outside
                    .operations
                    .iter()
                    .filter(|o| i > 0 || o.name.as_str() == operation)
                    .flat_map(|o| o.destinations.clone())
                    .collect();
                d.outside
                    .destinations
                    .iter()
                    .find(|dest| {
                        listed.contains(&dest.name)
                            && dest.scheme == current.scheme
                            && dest.host.as_str() == current.host
                            && dest.port.get() == current.port
                    })
                    .cloned()
            })
        };
        let mut hops = Vec::new();
        let mut current = endpoint.clone();
        let mut outcome = CallOutcome::Completed(Nothing {});
        for hop in 0..5 {
            let matched = find(&current);
            let (refusal, enforced) = match (hop, &matched) {
                (0, None) => (
                    Some(RefusalReason::UndeclaredDestination),
                    !self.breaks.connects_undeclared,
                ),
                (_, None) => (
                    Some(RefusalReason::RedirectToUndeclaredDestination),
                    !self.breaks.follows_any_redirect
                        && !(self.breaks.checks_only_the_first_redirect && hop >= 2),
                ),
                (1.., Some(_)) if self.breaks.refuses_every_redirect => {
                    (Some(RefusalReason::RedirectToUndeclaredDestination), true)
                }
                _ => (None, true),
            };
            let served = self.endpoints.get(&current).cloned();
            let (refusal, enforced) = match (refusal, &matched, &served) {
                (None, Some(destination), Some((address, _)))
                    if !allows(&destination.addresses, *address) =>
                {
                    let unchecked = self.breaks.trusts_resolution
                        || (self.breaks.checks_the_address_on_the_first_hop_only && hop >= 1)
                        || (self.breaks.checks_ipv4_addresses_only && address.is_ipv6());
                    (Some(RefusalReason::AddressNotAllowed), !unchecked)
                }
                _ => (refusal, enforced),
            };
            let host = Host::new(current.host.clone()).expect("a host");
            let port = NonZeroU16::new(current.port).expect("a port");
            let destination = matched.as_ref().map(|d| d.name.clone());
            if let Some(reason) = refusal {
                hops.push(Hop {
                    scheme: current.scheme,
                    host,
                    port,
                    destination,
                    address: None,
                });
                outcome = CallOutcome::Refused(Refused { reason });
                if !enforced {
                    self.received
                        .entry(current.clone())
                        .or_default()
                        .push(body.to_owned());
                }
                break;
            }
            let Some((address, redirect)) = served else {
                hops.push(Hop {
                    scheme: current.scheme,
                    host,
                    port,
                    destination,
                    address: None,
                });
                outcome = CallOutcome::Failed(Nothing {});
                break;
            };
            hops.push(Hop {
                scheme: current.scheme,
                host,
                port,
                destination,
                address: Address::new(address.to_string()),
            });
            self.received
                .entry(current.clone())
                .or_default()
                .push(body.to_owned());
            match redirect {
                Some(next) => current = next,
                None => break,
            }
        }
        let module_version = if self.breaks.records_another_version {
            "another-version"
        } else {
            version
        };
        let secrets = if self.breaks.records_no_secrets {
            Vec::new()
        } else if self.breaks.records_every_declared_secret {
            declaration
                .outside
                .operations
                .iter()
                .find(|o| o.name.as_str() == operation)
                .map(|o| o.secrets.clone())
                .unwrap_or_default()
        } else {
            secrets
        };
        let started_at = self.now();
        self.events.records.push(OutboundCallRecord {
            transaction: Identifier::new(transaction).expect("a transaction"),
            provider: Identifier::new(provider).expect("a provider"),
            module_version: Identifier::new(module_version).expect("a version"),
            operation: Name::new(operation).expect("an operation"),
            secrets,
            started_at,
            hops,
            outcome,
        });
    }

    fn cross_module(
        &mut self,
        provider: &str,
        declaration: &Declaration,
        version: &str,
        op: &Value,
        callee: &str,
        kind: OperationKind,
    ) {
        let refusal = match declaration
            .outside
            .calls
            .iter()
            .find(|c| c.callee.as_str() == callee)
        {
            None => Some(CallRefusalReason::UndeclaredCallee),
            Some(c) if !c.kinds.contains(&kind) && !self.breaks.checks_only_the_callee => {
                Some(CallRefusalReason::UndeclaredKind)
            }
            Some(_) if self.breaks.refuses_every_cross_module_call => {
                Some(CallRefusalReason::CalleeUnavailable)
            }
            Some(_) => None,
        };
        let delivered = refusal.is_none() || self.breaks.delivers_undeclared_calls;
        if delivered {
            *self
                .operations_received
                .entry(callee.to_owned())
                .or_default() += 1;
        }
        let mut asked = op.clone();
        asked["provider"] = Value::from(callee);
        let Ok(operation) = serde_json::from_value::<Operation>(asked) else {
            return;
        };
        let outcome = match refusal {
            Some(reason) if !delivered => CrossModuleOutcome::Refused(CallRefused {
                callee: Identifier::new(callee).expect("a callee"),
                kind,
                reason: if self.breaks.misstates_call_refusals {
                    CallRefusalReason::NotNamedByRoute
                } else {
                    reason
                },
            }),
            _ => CrossModuleOutcome::Delivered(Nothing {}),
        };
        self.events.calls.push(CrossModuleCall {
            transaction: Identifier::new(op["transaction"].as_str().unwrap_or_default())
                .expect("a transaction"),
            caller: Identifier::new(provider).expect("a provider"),
            caller_version: Identifier::new(version).expect("a version"),
            route: RouteReference {
                decision_record: Identifier::new("conformance-route").expect("an id"),
                segment: Identifier::new("segment-1").expect("an id"),
            },
            operation,
            outcome,
        });
    }
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

/// Whether the address `ip` lies within the block `outer/outer_len`.
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

fn digest(bytes: &[u8]) -> Digest {
    Digest {
        algorithm: DigestAlgorithm::Sha256,
        value: DigestValue::new(hex_digest(bytes)).expect("a digest"),
    }
}

/// The honest result for a withdrawal, whose evidence reference names the module and what it obtained.
fn honest(operation: &Value, provider: &str, version: &str, obtained: &[String]) -> Value {
    let mut reference = format!("{provider}-{version}");
    for value in obtained {
        reference.push('-');
        reference.push_str(value);
    }
    json!({
        "kind": "withdrawal",
        "transaction": operation["transaction"],
        "outcome": {
            "status": "succeeded",
            "amount_in": operation["amount"],
            "amount_out": operation["amount"],
            "fee": { "asset": operation["asset"], "amount": "0" },
            "evidence": [{ "grade": 3, "reference": reference, "by": provider }],
        },
    })
}

impl ReferenceDouble {
    /// The bytes a module's sandboxed part emits.
    fn render(
        &self,
        output: &Output,
        operation: &Value,
        provider: &str,
        version: &str,
        obtained: &[String],
    ) -> Vec<u8> {
        let mut value = honest(operation, provider, version, obtained);
        match output {
            Output::Honest => {}
            Output::PaddedTo(size) => {
                let size = if self.breaks.pads_short {
                    size.saturating_sub(1)
                } else {
                    *size
                };
                let length = serde_json::to_vec(&value).expect("json").len();
                if size > length {
                    let reference = &mut value["outcome"]["evidence"][0]["reference"];
                    let padded = format!(
                        "{}{}",
                        reference.as_str().unwrap_or_default(),
                        "x".repeat(size - length)
                    );
                    *reference = Value::from(padded);
                }
            }
            Output::Unparseable => return b"not a result {".to_vec(),
            Output::AnotherKind => value["kind"] = Value::from("deposit"),
            Output::AnotherTransaction => {
                let transaction = format!(
                    "{}-another",
                    operation["transaction"].as_str().unwrap_or_default()
                );
                value["transaction"] = Value::from(transaction);
            }
            Output::UnknownField => value["extra"] = Value::from(true),
        }
        serde_json::to_vec(&value).expect("json")
    }

    fn widenings(&self, previous: &Declaration, next: &Declaration) -> Vec<Widening> {
        widening(previous, next)
            .into_iter()
            .filter(|w| match w {
                Widening::Secret(_) | Widening::OperationSecret(_) => {
                    !self.breaks.ignores_secret_widenings
                }
                Widening::Call(_) | Widening::CallKind(_) => !self.breaks.ignores_callee_widenings,
                Widening::Addresses(_) => !self.breaks.ignores_address_widenings,
                _ => true,
            })
            .collect()
    }
}

fn result_matches(result: &ResultBody, operation: &Operation) -> bool {
    let (Ok(result), Ok(operation)) = (
        serde_json::to_value(result),
        serde_json::to_value(operation),
    ) else {
        return false;
    };
    result["kind"] == operation["kind"] && result["transaction"] == operation["transaction"]
}

impl ModuleBinding for ReferenceDouble {
    fn format(&self) -> String {
        "reference".to_owned()
    }

    fn languages(&self) -> Vec<String> {
        self.languages.clone()
    }

    fn set_output_limit(&mut self, bytes: usize) {
        self.output_limit = bytes;
    }

    fn admit(
        &mut self,
        assertion: &Statement<ReleaseAssertion>,
        binaries: &Parts<Vec<u8>>,
    ) -> Admitted {
        let received = Parts {
            sandboxed: digest(&binaries.sandboxed),
            outside: digest(&binaries.outside),
        };
        let asserted = &assertion.statement.parts;
        let sandboxed_differs = received.sandboxed != asserted.sandboxed.digest
            && !self.breaks.checks_only_the_outside_digest;
        let outside_differs = received.outside != asserted.outside.digest;
        if !self.breaks.skips_digest_check && (sandboxed_differs || outside_differs) {
            let part = if sandboxed_differs {
                PartName::Sandboxed
            } else {
                PartName::Outside
            };
            return Admitted::Refused(AdmissionRefusal::PartAltered(part));
        }
        let index = String::from_utf8(binaries.outside.clone())
            .ok()
            .and_then(|text| {
                text.strip_prefix(BINARY_PREFIX)?
                    .split('/')
                    .next()?
                    .parse::<usize>()
                    .ok()
            })
            .filter(|index| *index < self.built.len());
        let Some(index) = index else {
            return Admitted::Refused(AdmissionRefusal::UnreadableBinary);
        };
        // A version names one release: one admitted, or waiting for its acceptance, isn't admitted again.
        let asserted_version = assertion.statement.declaration.version.as_str();
        if let Some(slot) = self
            .providers
            .get(assertion.statement.declaration.provider.as_str())
        {
            let taken = slot
                .accepted
                .iter()
                .chain(slot.pending.iter().map(|(artefact, _)| artefact))
                .any(|artefact| artefact.declaration().version.as_str() == asserted_version);
            if taken {
                return Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted);
            }
        }
        if self.breaks.refuses_releases_that_cannot_start && !self.built[index].starts {
            return Admitted::Refused(AdmissionRefusal::WontStart);
        }
        let declaration = assertion.statement.declaration.clone();
        let provider = declaration.provider.as_str().to_owned();
        let version = declaration.version.as_str().to_owned();
        let previous = self
            .providers
            .get(&provider)
            .and_then(|slot| slot.accepted.last().cloned());
        let widenings = previous
            .as_ref()
            .map(|p| self.widenings(p.declaration(), &declaration))
            .unwrap_or_default();
        let fresh = previous.is_none()
            || (!widenings.is_empty() && !self.breaks.accepts_widening_automatically)
            || (self.breaks.holds_non_starting_releases_as_waiting && !self.built[index].starts);
        let history = previous
            .as_ref()
            .map(|p| {
                let mut history = p.history.clone();
                history.extend(p.as_history_entry());
                history
            })
            .unwrap_or_default();
        let machine_code = |bytes: &[u8]| {
            let mut code = bytes.to_vec();
            code.extend_from_slice(PLATFORM_VERSION.as_bytes());
            digest(&code)
        };
        let assertion_bytes = serde_json::to_vec(assertion).expect("json");
        let accepted = (!fresh).then(|| Accepted {
            by: None,
            at: self.now(),
        });
        let artefact = AdmissionArtefact {
            assertion: assertion.clone(),
            assertion_digest: digest(&assertion_bytes),
            received,
            build: Build {
                platform_version: Identifier::new(PLATFORM_VERSION).expect("a version"),
                machine_code: Parts {
                    sandboxed: machine_code(&binaries.sandboxed),
                    outside: machine_code(&binaries.outside),
                },
            },
            history,
            capabilities: Capabilities {
                compared_with: previous.as_ref().map(|p| p.declaration().version.clone()),
                widenings,
            },
            acceptance: Acceptance {
                required: if fresh {
                    AcceptanceMode::Fresh
                } else {
                    AcceptanceMode::Automatic
                },
                accepted,
            },
        };
        let slot = self.providers.entry(provider.clone()).or_default();
        if fresh {
            slot.pending = Some((artefact.clone(), index));
        } else {
            slot.accepted.push(artefact.clone());
            self.start(&provider, &version, index);
        }
        Admitted::Artefact(Box::new(artefact))
    }

    fn accept(&mut self, provider: &str, version: &str, by: &str) -> Option<AdmissionArtefact> {
        let at = self.now();
        let slot = self.providers.get_mut(provider)?;
        let (mut artefact, index) = slot.pending.take()?;
        if artefact.declaration().version.as_str() != version {
            slot.pending = Some((artefact, index));
            return None;
        }
        artefact.acceptance.accepted = Some(Accepted {
            by: Identifier::new(by),
            at,
        });
        slot.accepted.push(artefact.clone());
        self.start(provider, version, index);
        Some(artefact)
    }

    fn active_version(&self, provider: &str) -> Option<String> {
        self.providers.get(provider)?.active.clone()
    }

    fn process(&self, provider: &str) -> Option<ProcessId> {
        self.providers.get(provider)?.process
    }

    fn restart(&mut self, provider: &str) -> bool {
        if self.process(provider).is_none() {
            return false;
        }
        let names: Vec<String> = if self.breaks.restarts_everyone {
            self.providers
                .iter()
                .filter(|(_, slot)| slot.process.is_some())
                .map(|(name, _)| name.clone())
                .collect()
        } else {
            vec![provider.to_owned()]
        };
        for name in names {
            let process = self.new_process();
            if let Some(slot) = self.providers.get_mut(&name) {
                slot.process = Some(process);
            }
        }
        true
    }

    fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled {
        *self
            .operations_received
            .entry(provider.to_owned())
            .or_default() += 1;
        let Some((declaration, spec, version)) = self.active(provider) else {
            return Handled::Refused(PlatformRefusal::Unavailable);
        };
        if !declaration
            .sandboxed
            .handles
            .contains(&OperationKind::of(&operation.statement))
        {
            return Handled::Refused(PlatformRefusal::NotHandled);
        }
        let op = serde_json::to_value(&operation.statement).expect("json");
        let transaction = op["transaction"].as_str().unwrap_or_default().to_owned();
        let mut obtained: Vec<String> = Vec::new();
        let mut resolved: BTreeMap<String, Vec<Name>> = BTreeMap::new();
        let steps = if self.breaks.ignores_steps {
            Vec::new()
        } else {
            spec.steps.clone()
        };
        for (taken, step) in steps.into_iter().enumerate() {
            if self.step_limit.is_some_and(|limit| taken >= limit) {
                return Handled::Refused(PlatformRefusal::LimitReached);
            }
            match step {
                Step::LookUpSecret {
                    part,
                    operation,
                    name,
                } => {
                    let (Some(transaction), Some(op_name), Some(secret)) = (
                        Identifier::new(&transaction),
                        Name::new(&operation),
                        Name::new(&name),
                    ) else {
                        continue;
                    };
                    let lookup = SecretLookup {
                        transaction,
                        operation: op_name,
                        name: secret.clone(),
                    };
                    let outcome = (part == PartName::Outside
                        || self.breaks.sandboxed_reaches_proxy)
                        .then(|| {
                            let ruling = self.ruling_declaration(provider, &declaration);
                            self.proxy(provider, &ruling, &lookup)
                        });
                    if outcome == Some(LookupOutcome::Resolved) {
                        if let Some(value) = self.delivered.get(provider).and_then(|v| v.last()) {
                            obtained.push(value.clone());
                        }
                        resolved.entry(operation).or_default().push(secret);
                    }
                    self.events.lookups.push(LookupEvent {
                        provider: provider.to_owned(),
                        part,
                        lookup,
                        outcome,
                    });
                }
                Step::ReadState { part, key } => {
                    let own = self.state.get(&(provider.to_owned(), key.clone())).cloned();
                    let value = own.or_else(|| {
                        self.breaks
                            .shared_state
                            .then(|| {
                                self.state
                                    .iter()
                                    .find(|((_, k), _)| *k == key)
                                    .map(|(_, v)| v.clone())
                            })
                            .flatten()
                    });
                    self.events.state_reads.push(StateEvent {
                        provider: provider.to_owned(),
                        part,
                        key,
                        found: value.is_some(),
                    });
                    obtained.extend(value);
                }
                Step::Call {
                    operation,
                    endpoint,
                } => {
                    let body = obtained.join("\n");
                    let secrets = resolved.get(&operation).cloned().unwrap_or_default();
                    self.egress(
                        provider,
                        &declaration,
                        &version,
                        &transaction,
                        &operation,
                        &endpoint,
                        &body,
                        secrets,
                    );
                }
                Step::CallModule { callee, kind } => {
                    let ruling = self.ruling_declaration(provider, &declaration);
                    self.cross_module(provider, &ruling, &version, &op, &callee, kind);
                }
            }
        }
        let bytes = self.render(&spec.output, &op, provider, &version, &obtained);
        let over = if self.breaks.refuses_at_the_limit {
            bytes.len() >= self.output_limit
        } else {
            bytes.len() > self.output_limit
        };
        if over && !self.breaks.no_output_limit {
            return Handled::Refused(PlatformRefusal::OversizedOutput);
        }
        let Ok(result) = serde_json::from_slice::<ResultBody>(&bytes) else {
            return Handled::Refused(PlatformRefusal::MalformedOutput);
        };
        if !self.breaks.trusts_kind_and_transaction
            && !result_matches(&result, &operation.statement)
        {
            return Handled::Refused(PlatformRefusal::MalformedOutput);
        }
        Handled::Answered(Box::new(Statement {
            deployment: operation.deployment.clone(),
            protocol_version: operation.protocol_version.clone(),
            shape: RESULT_SHAPE.to_owned(),
            maker: provider.to_owned(),
            made_at: operation.made_at.clone(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: declaration.outside.statement_key.clone(),
                value: placeholder_signature(),
            },
            statement: result,
        }))
    }

    fn take_events(&mut self) -> Events {
        std::mem::take(&mut self.events)
    }
}

impl BoundaryHarness for ReferenceDouble {
    fn build(&mut self, spec: &ModuleSpec, language: &str) -> Option<Parts<Vec<u8>>> {
        if !self.languages.iter().any(|l| l == language) {
            return None;
        }
        self.built.push(spec.clone());
        let index = self.built.len() - 1;
        let part = |name: &str| format!("{BINARY_PREFIX}{index}/{language}/{name}").into_bytes();
        Some(Parts {
            sandboxed: part("sandboxed"),
            outside: part("outside"),
        })
    }

    fn delivered_secrets(&self, provider: &str) -> Vec<String> {
        self.delivered.get(provider).cloned().unwrap_or_default()
    }

    fn seed_secret(&mut self, provider: &str, name: &str, value: &str) {
        self.secrets
            .insert((provider.to_owned(), name.to_owned()), value.to_owned());
    }

    fn seed_state(&mut self, provider: &str, key: &str, value: &str) {
        self.state
            .insert((provider.to_owned(), key.to_owned()), value.to_owned());
    }

    fn serve(&mut self, endpoint: &Endpoint, address: IpAddr, redirect: Option<&Endpoint>) {
        self.endpoints
            .insert(endpoint.clone(), (address, redirect.cloned()));
    }

    fn received(&self, endpoint: &Endpoint) -> Vec<String> {
        self.received.get(endpoint).cloned().unwrap_or_default()
    }

    fn operations_received(&self, provider: &str) -> usize {
        self.operations_received
            .get(provider)
            .copied()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::role::{SuiteContext, placeholder_key};
    use rutent_contracts::module_interface::{BinaryFormat, BinaryPart, RELEASE_ASSERTION_SHAPE};
    use rutent_contracts::provider_role::OPERATION_SHAPE;

    const PROVIDER: &str = "limited-reserve";

    fn spec(version: &str, steps: usize) -> ModuleSpec {
        let declaration: Declaration = serde_json::from_value(json!({
            "provider": PROVIDER,
            "version": version,
            "interface": rutent_contracts::module_interface::VERSION,
            "sandboxed": { "handles": ["withdrawal"] },
            "outside": {
                "operations": [],
                "destinations": [],
                "secrets": [],
                "calls": [],
                "statement_key": placeholder_key(1),
            },
        }))
        .expect("a declaration");
        ModuleSpec {
            declaration,
            steps: (0..steps)
                .map(|i| Step::ReadState {
                    part: PartName::Outside,
                    key: format!("entry-{i}"),
                })
                .collect(),
            output: Output::Honest,
            starts: true,
        }
    }

    /// Builds `spec` in the double and asks for its admission.
    fn admit(double: &mut ReferenceDouble, spec: &ModuleSpec) -> Admitted {
        let binaries = double.build(spec, "reference-a").expect("built");
        let part = |bytes: &[u8]| BinaryPart {
            format: BinaryFormat::new(double.format()).expect("a format"),
            digest: Digest {
                algorithm: DigestAlgorithm::Sha256,
                value: DigestValue::new(hex_digest(bytes)).expect("a digest"),
            },
        };
        let context = SuiteContext::default();
        let assertion = Statement {
            deployment: context.deployment.clone(),
            protocol_version: context.protocol_version.clone(),
            shape: RELEASE_ASSERTION_SHAPE.to_owned(),
            maker: PROVIDER.to_owned(),
            made_at: "2026-11-09T00:00:00Z".to_owned(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: placeholder_key(2),
                value: placeholder_signature(),
            },
            statement: ReleaseAssertion {
                declaration: spec.declaration.clone(),
                parts: Parts {
                    sandboxed: part(&binaries.sandboxed),
                    outside: part(&binaries.outside),
                },
            },
        };
        double.admit(&assertion, &binaries)
    }

    fn withdrawal() -> Statement<Operation> {
        let context = SuiteContext::default();
        serde_json::from_value(json!({
            "deployment": context.deployment,
            "protocol_version": context.protocol_version,
            "shape": OPERATION_SHAPE,
            "maker": context.operator,
            "made_at": "2026-11-09T00:00:00Z",
            "signature": {
                "scheme": "ed25519",
                "key": context.deployment.operator_key,
                "value": placeholder_signature(),
            },
            "statement": {
                "kind": "withdrawal",
                "transaction": "tx-limit-1",
                "provider": PROVIDER,
                "origin": { "network": "conformance-network" },
                "destination": "conformance-ledger",
                "asset": "COP",
                "amount": "1000",
                "dry_run": false,
                "inputs": {},
            },
        }))
        .expect("an operation")
    }

    fn installed(double: &mut ReferenceDouble, steps: usize) {
        double.set_output_limit(1 << 16);
        assert!(matches!(
            admit(double, &spec("1.0.0", steps)),
            Admitted::Artefact(_)
        ));
        assert!(double.accept(PROVIDER, "1.0.0", "a-reviewer").is_some());
    }

    // verifies: REQ-MDL-11
    #[test]
    fn limit_hit_is_refused_with_its_own_reason() {
        let mut limited = ReferenceDouble::new().with_step_limit(2);
        installed(&mut limited, 3);
        let process = limited.process(PROVIDER);
        assert_eq!(
            limited.handle(PROVIDER, &withdrawal()),
            Handled::Refused(PlatformRefusal::LimitReached)
        );
        // Not the stand-in the bindings used before: the module is still admitted and running.
        assert_ne!(PlatformRefusal::LimitReached, PlatformRefusal::Unavailable);
        assert_eq!(limited.active_version(PROVIDER).as_deref(), Some("1.0.0"));
        assert_eq!(limited.process(PROVIDER), process);
        // Within the limit, the same module answers.
        let mut within = ReferenceDouble::new().with_step_limit(3);
        installed(&mut within, 3);
        assert!(matches!(
            within.handle(PROVIDER, &withdrawal()),
            Handled::Answered(_)
        ));
    }

    // verifies: REQ-MDL-1
    #[test]
    fn re_admitted_version_is_refused_with_its_own_reason() {
        let mut double = ReferenceDouble::new();
        installed(&mut double, 0);
        assert_eq!(
            admit(&mut double, &spec("1.0.0", 1)),
            Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted)
        );
        // One waiting for its acceptance names its version too.
        let mut waiting = ReferenceDouble::new();
        assert!(matches!(
            admit(&mut waiting, &spec("1.0.0", 0)),
            Admitted::Artefact(_)
        ));
        assert_eq!(
            admit(&mut waiting, &spec("1.0.0", 0)),
            Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted)
        );
    }
}
