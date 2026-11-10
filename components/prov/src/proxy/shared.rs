//! The shared credential proxy (`T-014-02`, `DEC-9`, ADR-0004).
//!
//! v0 runs one credential proxy for every provider (`DEC-9`). What keeps a compromised module from another
//! provider's secrets is that the proxy answers each lookup only for the calling process's own provider, known from
//! the process, never from the lookup (`REQ-MDL-9`), and only while the platform has an outside request of that
//! provider's module in flight, for that request's transaction and operation (`REQ-MDL-6`). It resolves only a
//! secret that operation of the admitted release declares (`REQ-MDL-3`), and the statement key only as the admitted
//! release declares it, never a key the provider's advertisement lists for `advertising` (`THR-4`). Every use,
//! refused ones included, is recorded with its transaction, never with a value (`REQ-MDL-6`, `THR-50`).

use std::collections::BTreeMap;

use rutent_contracts::module_interface::{
    AdmissionArtefact, Declaration, Identifier, Name, Resolved, SecretAnswer, SecretLookup,
    SecretOutcome, SecretRefusalReason, SecretRefused, SecretValue, validate_lookup,
};
use rutent_contracts::provider_role::{
    Advertisement, Deployment, KeyPurpose, PublicKey, SigningKey, Statement,
};

use super::resolver::{ResolveError, Resolver, plain_segment};
use super::settings::{Bucket, InvalidSettings, ProxySettings, RoleSettings};

/// The provider a lookup comes from, as the platform attributes it: the provider whose process the lookup came
/// from, never anything the lookup says (`DEC-9`). One OS process runs one provider's module (ADR-0003), so the
/// platform knows it from the channel the lookup arrived on.
///
/// Only code inside this crate can build one, so nothing outside it can claim to be a provider's process:
///
/// ```compile_fail
/// use rutent_contracts::module_interface::Identifier;
///
/// let _ = rutent_prov::proxy::Caller::process_of(Identifier::new("beta").unwrap());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller(Identifier);

impl Caller {
    /// The caller whose process is `provider`'s. Only the platform, which owns each provider's process and its
    /// channel, builds one; it is not public.
    pub(crate) fn process_of(provider: Identifier) -> Self {
        Caller(provider)
    }

    /// The caller's provider.
    pub fn provider(&self) -> &Identifier {
        &self.0
    }
}

/// The outside request the platform has in flight for one provider's module: the provider whose sandboxed part
/// asked for it, its transaction and its declared outside operation, all from the platform's own state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    /// The provider whose module the request is for.
    pub provider: Identifier,
    /// The request's transaction.
    pub transaction: Identifier,
    /// The request's outside operation.
    pub operation: Name,
}

/// Why the proxy refused a lookup, an admission or a request. No variant carries a value or a token (`THR-50`).
///
/// On the wire the contract has three reasons ([`Refusal::wire_reason`]): `not_found` when the caller's provider
/// holds no secret by a name it may resolve, `store_unavailable` when the store couldn't be asked or gave no usable
/// answer, whatever the failure, and `undeclared_secret` for every other refusal, so a module learns nothing about
/// another provider, another request or the deployment from the reason. The record keeps the true cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The deployment settings give the provider no process role, so it has no bucket.
    NoRole,
    /// No release of the provider is admitted.
    NoRelease,
    /// The release offered for admission isn't accepted yet.
    NotAccepted,
    /// The release judged is another provider's than the caller's.
    ReleaseOfAnotherProvider,
    /// The advertisement offered with a release is another provider's.
    AdvertisementOfAnotherProvider,
    /// The advertisement offered with a release belongs to another deployment than the proxy's settings name.
    AdvertisementOfAnotherDeployment,
    /// A release offered with no advertisement isn't the operator's own: it isn't of the proxy's deployment, or
    /// isn't signed with the `operator_key` the proxy's settings name (`T-016-04`).
    NotTheOperatorsRelease,
    /// An advertisement names a module whose running release is the operator's own: no advertisement vouches for
    /// its keys (`T-016-04`).
    AdvertisementOfAnOperatorsModule,
    /// The release's provider or one of its secret names isn't one plain path segment of at most 255 bytes, so
    /// the store can't address it under that provider's keys.
    UnaddressableName,
    /// The platform has no outside request in flight for the caller's module, or, for the statement key, no
    /// operation of its being handled.
    NoRequestInFlight,
    /// The request in flight is another provider's module's.
    RequestOfAnotherProvider,
    /// The lookup names another transaction or operation than the request in flight.
    NotTheRequestInFlight,
    /// The release declares no outside operation by that name.
    UndeclaredOperation,
    /// The operation in flight doesn't declare the secret.
    UndeclaredSecret,
    /// The release's statement key isn't one of its provider's advertised keys.
    StatementKeyNotAdvertised,
    /// The release's statement key is listed for `advertising`: it is never resolved (`THR-4`).
    StatementKeyForAdvertising,
    /// The release's statement key is the deployment's operator key.
    StatementKeyIsOperatorKey,
    /// The caller's provider holds no current secret by that name.
    NotFound,
    /// The store couldn't be asked or didn't answer usefully.
    Store(ResolveError),
}

impl Refusal {
    /// The contract's reason for this refusal. Every store failure is `store_unavailable`, never `not_found`
    /// (module-interface 4.0.0, `T-010-09`).
    pub fn wire_reason(&self) -> SecretRefusalReason {
        match self {
            Refusal::NotFound => SecretRefusalReason::NotFound,
            Refusal::Store(_) => SecretRefusalReason::StoreUnavailable,
            _ => SecretRefusalReason::UndeclaredSecret,
        }
    }
}

/// Judges a lookup as the proxy answers it (`DEC-9`, `REQ-MDL-6`, `REQ-MDL-9`): `release` is the caller's own
/// running release, an outside request is in flight for the caller's module, the lookup names that request's
/// transaction and operation, and that operation declares the secret.
///
/// The module platform applies it to every lookup it serves, from its own state: the caller is the provider whose
/// process asked, and the request in flight the one it passed to that provider's outside part.
pub fn judge(
    caller: &Caller,
    release: &Declaration,
    in_flight: Option<&InFlight>,
    lookup: &SecretLookup,
) -> Result<(), Refusal> {
    let in_flight = in_flight.ok_or(Refusal::NoRequestInFlight)?;
    if &in_flight.provider != caller.provider() {
        return Err(Refusal::RequestOfAnotherProvider);
    }
    if &release.provider != caller.provider() {
        return Err(Refusal::ReleaseOfAnotherProvider);
    }
    if lookup.transaction != in_flight.transaction || lookup.operation != in_flight.operation {
        return Err(Refusal::NotTheRequestInFlight);
    }
    validate_lookup(release, lookup).map_err(|_| Refusal::UndeclaredSecret)
}

/// The contract's answer to `lookup` for `result`.
pub fn answer(lookup: &SecretLookup, result: &Result<SecretValue, Refusal>) -> SecretAnswer {
    let outcome = match result {
        Ok(value) => SecretOutcome::Resolved(Resolved {
            value: value.clone(),
        }),
        Err(refusal) => SecretOutcome::Refused(SecretRefused {
            reason: refusal.wire_reason(),
        }),
    };
    SecretAnswer {
        transaction: lookup.transaction.clone(),
        operation: lookup.operation.clone(),
        name: lookup.name.clone(),
        outcome,
    }
}

/// What a use asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseOf {
    /// A declared secret, by name.
    Secret(Name),
    /// The statement key's private half, by the public key the caller's admitted release declares; `None` if no
    /// release of the caller's is admitted.
    StatementKey(Option<PublicKey>),
}

/// One use of the proxy, resolved or refused: who asked, for which transaction and operation, for what, and how
/// it ended (`REQ-MDL-6`). It never holds the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    /// The caller's provider.
    pub provider: Identifier,
    /// The version of the caller's admitted release, if one is admitted.
    pub module_version: Option<Identifier>,
    /// The transaction the use was for, as the caller named it; a resolved use's is the transaction in flight.
    pub transaction: Identifier,
    /// The outside operation it was for; `None` for the statement key, which serves the operation handled.
    pub operation: Option<Name>,
    /// What it asked for.
    pub of: UseOf,
    /// `Ok` if resolved, else why not.
    pub outcome: Result<(), Refusal>,
}

/// One provider's admitted release, and what vouches for its statement key.
struct Release {
    declaration: Declaration,
    vouched: Vouched,
}

/// What vouches for a running release's statement key.
enum Vouched {
    /// The provider's accepted advertisement, by the keys it lists.
    Advertisement(Vec<SigningKey>),
    /// The operator's own release (`T-016-04`): a module the operator publishes, such as a public node's holding
    /// account, has no advertisement.
    Operator,
}

/// What the platform is handling for one provider's module: the operation's transaction, and the outside
/// request it is serving, if any.
struct Handling {
    transaction: Identifier,
    serving: Option<Name>,
    /// When the operation must end: the store is given up then (`REQ-MDL-11`).
    due: Option<std::time::Instant>,
}

/// v0's shared credential proxy (`DEC-9`, ADR-0004), over one [`Bucket`] per process role.
///
/// The platform tells it which release each provider runs ([`CredentialProxy::admit`]) and what each provider's
/// module is handling ([`CredentialProxy::begin_operation`], [`CredentialProxy::begin_request`] and their ends),
/// and passes on each lookup with the [`Caller`] it came from.
pub struct CredentialProxy<R> {
    deployment: Deployment,
    roles: BTreeMap<String, String>,
    buckets: BTreeMap<String, Bucket<R>>,
    releases: BTreeMap<String, Release>,
    handling: BTreeMap<String, Handling>,
    uses: Vec<Use>,
}

impl<R: Resolver> CredentialProxy<R> {
    /// The proxy `settings` describe, with each role's bucket built by `bucket_for`. The settings are checked
    /// first ([`ProxySettings::check`]), however they were made, so no proxy runs over buckets that nest or a
    /// provider whose role is undefined.
    pub fn new<E: From<InvalidSettings>>(
        settings: &ProxySettings,
        mut bucket_for: impl FnMut(&str, &RoleSettings) -> Result<Bucket<R>, E>,
    ) -> Result<Self, E> {
        settings.check()?;
        let mut buckets = BTreeMap::new();
        for (role, role_settings) in &settings.roles {
            buckets.insert(role.clone(), bucket_for(role, role_settings)?);
        }
        Ok(CredentialProxy {
            deployment: settings.deployment.clone(),
            roles: settings.providers.clone(),
            buckets,
            releases: BTreeMap::new(),
            handling: BTreeMap::new(),
            uses: Vec::new(),
        })
    }

    /// Makes `artefact`'s release its provider's running one, with `advertisement`, the one the catalogue accepted
    /// for that provider. The release must be accepted, its provider given a role, and every name it lets into a
    /// store path addressable. A new admission replaces the provider's running release and ends what it was
    /// handling.
    pub fn admit(
        &mut self,
        artefact: &AdmissionArtefact,
        advertisement: &Statement<Advertisement>,
    ) -> Result<(), Refusal> {
        let provider = artefact.declaration().provider.as_str();
        if artefact.acceptance.accepted.is_none() {
            return Err(Refusal::NotAccepted);
        }
        if advertisement.statement.provider != provider {
            return Err(Refusal::AdvertisementOfAnotherProvider);
        }
        if advertisement.deployment != self.deployment {
            return Err(Refusal::AdvertisementOfAnotherDeployment);
        }
        self.install(
            artefact,
            Vouched::Advertisement(advertisement.statement.keys.clone()),
        )
    }

    /// Makes `artefact`'s release, of a module the operator publishes (`T-016-04`; `REQ-CAT-23`), its provider's
    /// running one, with no advertisement: its assertion must be the operator's own, of the proxy's deployment and
    /// signed with that deployment's `operator_key`, the one the proxy's settings name, never one an advertisement
    /// or the assertion names. The rest is as [`CredentialProxy::admit`]. Its statement key is then resolved as
    /// the release declares it, never the operator key ([`CredentialProxy::statement_key`]).
    pub fn admit_operator(&mut self, artefact: &AdmissionArtefact) -> Result<(), Refusal> {
        if artefact.acceptance.accepted.is_none() {
            return Err(Refusal::NotAccepted);
        }
        if !self.is_operators_release(artefact) {
            return Err(Refusal::NotTheOperatorsRelease);
        }
        self.install(artefact, Vouched::Operator)
    }

    /// Whether `artefact`'s assertion is the operator's own: of the proxy's deployment, and signed with the
    /// `operator_key` the proxy's settings name.
    pub fn is_operators_release(&self, artefact: &AdmissionArtefact) -> bool {
        let assertion = &artefact.assertion;
        assertion.deployment == self.deployment
            && assertion.signature.key == self.deployment.operator_key
    }

    /// Makes the release its provider's running one, vouched for by `vouched`, once its provider has a role and
    /// every name it lets into a store path is addressable.
    fn install(&mut self, artefact: &AdmissionArtefact, vouched: Vouched) -> Result<(), Refusal> {
        let declaration = artefact.declaration();
        let provider = declaration.provider.as_str();
        if !self.roles.contains_key(provider) {
            return Err(Refusal::NoRole);
        }
        let names = declaration.outside.secrets.iter().chain(
            declaration
                .outside
                .operations
                .iter()
                .flat_map(|o| &o.secrets),
        );
        if !plain_segment(provider) || !names.into_iter().all(|n| plain_segment(n.as_str())) {
            return Err(Refusal::UnaddressableName);
        }
        self.handling.remove(provider);
        self.releases.insert(
            provider.to_owned(),
            Release {
                declaration: declaration.clone(),
                vouched,
            },
        );
        Ok(())
    }

    /// Takes `advertisement`, the one the catalogue now accepts for its provider, as the keys that provider's running
    /// release is judged against from the next statement key on: a key the new advertisement no longer lists for
    /// `statements`, or lists for `advertising`, stops being resolved at once, with no restart. An advertisement of
    /// another deployment is refused, and one of a provider with no release admitted changes nothing here. One
    /// naming a module whose running release is the operator's own is refused: no advertisement vouches for its
    /// keys (`T-016-04`).
    pub fn advertise(&mut self, advertisement: &Statement<Advertisement>) -> Result<(), Refusal> {
        if advertisement.deployment != self.deployment {
            return Err(Refusal::AdvertisementOfAnotherDeployment);
        }
        match self.releases.get_mut(&advertisement.statement.provider) {
            Some(Release {
                vouched: Vouched::Operator,
                ..
            }) => Err(Refusal::AdvertisementOfAnOperatorsModule),
            Some(Release {
                vouched: Vouched::Advertisement(keys),
                ..
            }) => {
                *keys = advertisement.statement.keys.clone();
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Starts handling an operation for `transaction` in the caller's module. It replaces whatever that module was
    /// handling.
    pub fn begin_operation(
        &mut self,
        caller: &Caller,
        transaction: &Identifier,
    ) -> Result<(), Refusal> {
        self.begin_operation_until(caller, transaction, None)
    }

    /// [`CredentialProxy::begin_operation`], for an operation that must end by `due`: every store asked for it,
    /// a secret or the statement key, is given up then (`REQ-MDL-11`).
    pub fn begin_operation_until(
        &mut self,
        caller: &Caller,
        transaction: &Identifier,
        due: Option<std::time::Instant>,
    ) -> Result<(), Refusal> {
        let provider = caller.provider().as_str();
        if !self.releases.contains_key(provider) {
            return Err(Refusal::NoRelease);
        }
        self.handling.insert(
            provider.to_owned(),
            Handling {
                transaction: transaction.clone(),
                serving: None,
                due,
            },
        );
        Ok(())
    }

    /// Puts an outside request in flight for the caller's module: for the transaction it is handling, and an
    /// outside operation its release declares.
    pub fn begin_request(
        &mut self,
        caller: &Caller,
        transaction: &Identifier,
        operation: &Name,
    ) -> Result<(), Refusal> {
        let provider = caller.provider().as_str();
        let release = self.releases.get(provider).ok_or(Refusal::NoRelease)?;
        let handling = self
            .handling
            .get_mut(provider)
            .ok_or(Refusal::NoRequestInFlight)?;
        if &handling.transaction != transaction {
            return Err(Refusal::NotTheRequestInFlight);
        }
        if !release
            .declaration
            .outside
            .operations
            .iter()
            .any(|o| &o.name == operation)
        {
            return Err(Refusal::UndeclaredOperation);
        }
        handling.serving = Some(operation.clone());
        Ok(())
    }

    /// Ends the outside request in flight for the caller's module, once it is answered.
    pub fn end_request(&mut self, caller: &Caller) {
        if let Some(handling) = self.handling.get_mut(caller.provider().as_str()) {
            handling.serving = None;
        }
    }

    /// Ends the operation the caller's module was handling.
    pub fn end_operation(&mut self, caller: &Caller) {
        self.handling.remove(caller.provider().as_str());
    }

    /// The outside request in flight for `provider`'s module, if any.
    pub fn in_flight(&self, provider: &Identifier) -> Option<InFlight> {
        let handling = self.handling.get(provider.as_str())?;
        handling.serving.as_ref().map(|operation| InFlight {
            provider: provider.clone(),
            transaction: handling.transaction.clone(),
            operation: operation.clone(),
        })
    }

    /// Forgets `provider`'s running release and whatever its module was handling, so every later lookup of it is
    /// refused until a release is admitted again: a release that started but couldn't be admitted here never
    /// runs on its predecessor's declaration.
    pub fn retire(&mut self, provider: &Identifier) {
        self.releases.remove(provider.as_str());
        self.handling.remove(provider.as_str());
    }

    /// Records `lookup` as refused for `refusal`, a judgement made before it reached the proxy, and returns the
    /// refusal. The store isn't asked.
    pub fn refuse(&mut self, caller: &Caller, lookup: &SecretLookup, refusal: Refusal) -> Refusal {
        let result = Err(refusal.clone());
        self.record(
            caller,
            &lookup.transaction,
            Some(lookup.operation.clone()),
            UseOf::Secret(lookup.name.clone()),
            &result,
        );
        refusal
    }

    /// Resolves `lookup` for the caller (`DEC-9`, `REQ-MDL-6`, `REQ-MDL-9`): from the caller's own provider's keys,
    /// in its role's bucket, only for the outside request in flight for its module and a secret that request's
    /// operation declares. It records the use, refused or not.
    pub fn lookup(
        &mut self,
        caller: &Caller,
        lookup: &SecretLookup,
    ) -> Result<SecretValue, Refusal> {
        let result = self.resolve_secret(caller, lookup);
        self.record(
            caller,
            &lookup.transaction,
            Some(lookup.operation.clone()),
            UseOf::Secret(lookup.name.clone()),
            &result,
        );
        result
    }

    fn resolve_secret(
        &self,
        caller: &Caller,
        lookup: &SecretLookup,
    ) -> Result<SecretValue, Refusal> {
        let provider = caller.provider();
        let bucket = self.bucket_of(provider)?;
        let release = self
            .releases
            .get(provider.as_str())
            .ok_or(Refusal::NoRelease)?;
        let in_flight = self.handling.get(provider.as_str()).and_then(|h| {
            h.serving.as_ref().map(|operation| InFlight {
                provider: provider.clone(),
                transaction: h.transaction.clone(),
                operation: operation.clone(),
            })
        });
        judge(caller, &release.declaration, in_flight.as_ref(), lookup)?;
        let due = self.handling.get(provider.as_str()).and_then(|h| h.due);
        resolved(bucket.secrets.resolve_until(provider, &lookup.name, due))
    }

    /// Resolves the private half of the statement key the caller's admitted release declares, while its module
    /// handles an operation for `transaction` (`DEC-4`, `THR-4`). It is resolved only if the provider's accepted
    /// advertisement lists that key for `statements`, never for `advertising`, or, for a module the operator
    /// publishes, if the operator's own release declares it ([`CredentialProxy::admit_operator`]); and never if it
    /// is the deployment's operator key. It is read from the bucket's keys, never its secrets, by the public key's text. It records
    /// the use, refused or not.
    pub fn statement_key(
        &mut self,
        caller: &Caller,
        transaction: &Identifier,
    ) -> Result<SecretValue, Refusal> {
        let result = self.resolve_statement_key(caller, transaction);
        let key = self
            .releases
            .get(caller.provider().as_str())
            .map(|r| r.declaration.outside.statement_key.clone());
        self.record(caller, transaction, None, UseOf::StatementKey(key), &result);
        result
    }

    fn resolve_statement_key(
        &self,
        caller: &Caller,
        transaction: &Identifier,
    ) -> Result<SecretValue, Refusal> {
        let provider = caller.provider();
        let bucket = self.bucket_of(provider)?;
        let release = self
            .releases
            .get(provider.as_str())
            .ok_or(Refusal::NoRelease)?;
        let handling = self
            .handling
            .get(provider.as_str())
            .ok_or(Refusal::NoRequestInFlight)?;
        if &handling.transaction != transaction {
            return Err(Refusal::NotTheRequestInFlight);
        }
        let key = &release.declaration.outside.statement_key;
        // A provider's key is vouched for by its advertisement. Every entry it lists for the key counts: one
        // listing it for `advertising` is enough to refuse it, whatever another entry says. A module the operator
        // publishes has no advertisement; its own release, signed with the operator key, vouches for its key.
        if let Vouched::Advertisement(keys) = &release.vouched {
            let mut purposes = keys
                .iter()
                .filter(|k| &k.public_key == key)
                .map(|k| k.purpose)
                .peekable();
            if purposes.peek().is_none() {
                return Err(Refusal::StatementKeyNotAdvertised);
            }
            if purposes.any(|p| p == KeyPurpose::Advertising) {
                return Err(Refusal::StatementKeyForAdvertising);
            }
        }
        // The operator key is the deployment's, from the proxy's settings, never from an advertisement.
        if key == &self.deployment.operator_key {
            return Err(Refusal::StatementKeyIsOperatorKey);
        }
        let name = Name::new(key.as_str()).ok_or(Refusal::UnaddressableName)?;
        resolved(bucket.keys.resolve_until(provider, &name, handling.due))
    }

    /// The records of every use since they were last taken, oldest first.
    pub fn take_uses(&mut self) -> Vec<Use> {
        std::mem::take(&mut self.uses)
    }

    fn bucket_of(&self, provider: &Identifier) -> Result<&Bucket<R>, Refusal> {
        self.roles
            .get(provider.as_str())
            .and_then(|role| self.buckets.get(role))
            .ok_or(Refusal::NoRole)
    }

    fn record(
        &mut self,
        caller: &Caller,
        transaction: &Identifier,
        operation: Option<Name>,
        of: UseOf,
        result: &Result<SecretValue, Refusal>,
    ) {
        let module_version = self
            .releases
            .get(caller.provider().as_str())
            .map(|r| r.declaration.version.clone());
        self.uses.push(Use {
            provider: caller.provider().clone(),
            module_version,
            transaction: transaction.clone(),
            operation,
            of,
            outcome: result.as_ref().map(|_| ()).map_err(Clone::clone),
        });
    }
}

/// A resolver's answer as the proxy's: a missing secret is `NotFound`, any other failure the store's.
fn resolved(result: Result<SecretValue, ResolveError>) -> Result<SecretValue, Refusal> {
    result.map_err(|e| match e {
        ResolveError::NotFound => Refusal::NotFound,
        other => Refusal::Store(other),
    })
}
