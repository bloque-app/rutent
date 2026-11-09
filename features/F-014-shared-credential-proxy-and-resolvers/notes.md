# F-014 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-014-01

- **The interface.** `proxy::Resolver::resolve(provider, name)` returns the contract's `SecretValue`, whose `Debug`
  is redacted. It takes the provider as an argument, so credentials are keyed by provider from the start (`DEC-9`):
  the same name under two providers is two secrets. It checks nothing about who asks; answering only for the
  calling process's own provider and an outside request in flight is `T-014-02`.
- **One resolver, two servers.** OpenBao and Vault speak the same KV version 2 read: same path, same
  `X-Vault-Token` and `X-Vault-Namespace` headers, same answer shape. So there is one `KvResolver`, built with
  `KvResolver::openbao` or `KvResolver::vault`, and `Server` records which one it talks to. A later difference
  between the two goes in that type, behind the same interface.
- **No Vault crate.** The resolver speaks the HTTP API directly, through a `Transport` trait. No Vault or OpenBao
  client crate enters the build: Vault's `BUSL-1.1` line in `licences/RECORD` stays `not-adopted ... pending`
  until the owner rules it (the gate in `F-014`'s header), and no crate is needed for either server. The only
  dependency change is `serde_json` becoming a normal dependency of `rutent-prov`; it was already locked, so
  `Cargo.lock` and `licences/RECORD` are unchanged.
- **Path layout.** Provider `p`'s secret `n` is read from `/v1/{mount}/data/{prefix}/{p}/{n}`, field `value`. The
  mount and prefix come from the deployment's `KvConfig`; they are where vault buckets are separated by process
  role (`DEC-9`, ADR-0004), with one token per role that the store's policy confines to that role's paths. Reading
  those from deployment settings is `T-014-02`.
- **Names can't leave their provider's keys.** `Identifier` and `Name` only check that the text is non-empty, so
  the resolver lets a provider or name into a path only when it is one plain segment: 1 to 255 bytes of ASCII
  letters, digits, `-`, `_`, `.`, and not `.` or `..`. Anything else (`/`, `..`, `?`, `#`, `%`, spaces, non-ASCII) is refused as
  `InvalidName` before a request is sent, rather than percent-encoded. The mount, prefix and namespace are held to
  the same rule per segment. A property test checks that no generated provider or name reaches a path outside that
  provider's keys.
- **Rotation needs no release.** Each lookup asks the store for its current version (no `version` parameter) and
  nothing is cached, so a rotated secret is used from the next lookup on (`REQ-MDL-6`).
- **Answers.** 200 with a non-empty text `value` resolves. 404, and a 200 or 404 whose `data.data` is `null` (a
  deleted or destroyed current version), are `NotFound`. 401 and 403 are `Denied`; 429 and 5xx `Unavailable`; any
  other status, a redirect included, is `Unexpected`, since a transport must not follow redirects. A 200 without a
  non-empty text `value` is `Malformed`. No error carries the body, the token or a value (`THR-50`), and neither
  `Request`, `Response`, `Token` nor `KvConfig` prints them through `Debug`.
- **The token.** `KvConfig` holds one static token per resolver. How a token is obtained and renewed (an auth
  method, a lease) is the deployment's and `T-014-02`'s; a resolver gets a new one by being rebuilt with a new
  configuration, which is not a module release.
- **Recorded responses.** The fixtures under `components/prov/tests/fixtures/proxy/` were recorded from dev-mode
  servers with KV version 2 at `secret`. Vault's came from image `hashicorp/vault:2.0.3`, which reports version
  2.0.3. OpenBao's came from image `openbao/openbao:2.4.1`, which reports version 2.4.1, at digest
  `sha256:597f62847dd382382056a1d6704d50465908c2040038c4611832a23269a67112`. The fixtures are bodies only, with no
  headers, and hold no token. Each answer has the same shape on both servers. Vault adds a `mount_type` field and
  OpenBao doesn't, and the parser reads neither.
- **Against both servers in CI.** The ordinary tests need no server: they run both resolvers on recorded
  responses through a fake transport. `live_servers_resolve_and_rotate_a_secret_when_configured` runs against real
  servers when `RUTENT_LIVE_OPENBAO_ADDR`/`_TOKEN` and `RUTENT_LIVE_VAULT_ADDR`/`_TOKEN` are set, and skips
  otherwise. With `RUTENT_LIVE_REQUIRED` set, a missing server fails the test instead. The `ci` job's step "Live
  resolver test against OpenBao and Vault" runs it with every variable set. That step starts both servers with
  `docker run` in dev mode, pinned by tag and digest, inside the existing job. A step adds no new check name to
  `develop`'s protection, where a separate job would. Each server runs apart in its own container, so neither
  enters a lockfile or `licences/RECORD`.
- **Gap: no production transport.** This task ships the `Transport` trait, not an HTTPS client. The live test
  carries its own plain-HTTP transport, for a loopback dev server only. The production transport has to verify
  TLS with a pure-Rust client (`rustls`), and its licence lines go in `licences/RECORD` when it is added. Check the
  root store's licence: `webpki-roots` may sit outside the standing permissive list. That transport is carried to
  `T-014-02`, where the proxy is deployed.
- **For `T-014-02`: no plain HTTP off loopback.** The `Transport` trait's documentation sets the rule. A transport
  speaks HTTPS with the server's certificate verified, and refuses plain HTTP to any address but loopback, which is
  allowed for a development server only. Every request carries the store token, so it must never cross a network
  in clear text. `T-014-02`'s production transport enforces this and tests it: a plain-HTTP address off loopback is
  refused before anything is sent.
- **`DEC-20`.** The credential proxy and its resolvers are a `DEC-20` path; the commit body says so.

## T-014-02

- **The proxy.** `proxy::CredentialProxy` is v0's one shared credential proxy (`DEC-9`). Each lookup comes with a
  `Caller`, the provider whose process sent it, which the platform knows from the process's channel and never reads
  from the lookup. The lookup's contract shape, `SecretLookup`, names no provider. The proxy resolves only from the
  caller's own provider's keys, in its role's bucket. The platform tells the proxy three things: which release each
  provider runs (`admit`), which operation its module is handling (`begin_operation`/`end_operation`), and which
  outside request is in flight (`begin_request`/`end_request`). A lookup resolves only for the transaction and
  operation of that request, and only a secret its operation declares (`validate_lookup`). Admitting a new release
  replaces the provider's running one and ends whatever it was handling.
- **One rule, applied by the platform too.** `proxy::judge` is the lookup rule on its own: the request in flight is
  the caller's, the release is the caller's, the lookup names that request's transaction and operation, and the
  operation declares the secret. `module/platform.rs`'s `lookup` now calls `judge` instead of its inline check. The
  edit is minimal: about 25 lines inside that one function, and its imports. `World`, `wire.rs` and the test world are unchanged. So the
  module-boundary suite's secret probes, which run on the platform, go through the proxy's rule. Making
  `CredentialProxy` the platform's production `World`, with the platform calling `begin_*`/`end_*` from its own
  context, moved to `T-014-05` (below). That work changes the `World::secret` hook as well. The hook returns
  `Option<String>`, so a store failure would reach the module as `not_found`; it needs a result that tells a
  missing secret from a failed store.
- **Admission.** `admit` takes the accepted `AdmissionArtefact` and the advertisement the catalogue accepted for its
  provider. It refuses an artefact that isn't accepted yet, an advertisement of another provider, and a provider the
  settings give no role. It does not re-run `validate_artefact`, which admission (`F-013`) already runs. Its own
  checks are acceptance and what its paths need.
- **The 255-byte cap.** The carried item is held at admission, not only at call time. `admit` refuses
  (`UnaddressableName`) a release whose provider, or any secret name it declares, isn't one plain path segment of at
  most 255 bytes. The contract's `Name` allows any non-empty text, so without this a release could pass admission
  and then fail every lookup. The resolver keeps its own check as well. A property test drives generated names of 1
  to 300 bytes through admission and lookup.
- **Refusals on the wire.** The contract has two reasons, `undeclared_secret` and `not_found`, and `contracts/` is
  `F-010`'s, so no reason is added. `Refusal::wire_reason` maps `NotFound` to `not_found`, and every other refusal
  to `undeclared_secret`: no role, no release, no request in flight, a request of another provider, another
  transaction or operation, an undeclared operation or secret. That matches what the platform already answered, and
  it tells a module nothing about another provider or the deployment. A store failure (denied token, unavailable
  store, transport) has no reason in the contract. `wire_reason` gives `None` and `proxy::answer` no answer, so the
  outside part learns no answer came. The record keeps the true cause in each case. A contract reason for a store
  failure moved to `T-014-05`.
- **Every use recorded.** Each `lookup` and `statement_key` call, resolved or refused, leaves one `Use`: the
  provider, its running module version, the transaction, the outside operation (`None` for the statement key), what
  was asked for, and the outcome. A `Use` never holds the value, and neither does its `Debug` (`THR-50`). The
  transaction is the one the lookup names. A resolved use's transaction is always the one in flight, since the
  lookup is refused otherwise. `take_uses` drains them, so the audit layer can take them. Where they are kept is
  `CMP-CORE-AUDIT`'s, not this task's.
- **Buckets by process role.** `proxy::ProxySettings`, read as JSON from the deployment, maps each process role to
  its bucket: server, store address, CA file, mount, a `secrets` prefix and a `keys` prefix, and an optional
  namespace. It maps each provider to one role. No SPEC or ARCHITECTURE text names the roles, so their names are the
  deployment's own. The tests use `internal` and `integrations`, after ADR-0004's wording. A provider with no role
  is refused. The settings are refused unless:
  - every role, provider, mount, prefix and namespace is plain segments;
  - every provider's role exists;
  - no two buckets on one store nest, compared segment by segment by the path the store resolves, a role's own
    secrets and keys included (as corrected in review, below).

  Without the last rule, prefix `a` with provider `b` would read the same paths as prefix `a/b`. Tokens are not in
  the settings. `kv_bucket` builds a role's two resolvers from its settings, a token and a transport. The deployment
  supplies the token the way it supplies other secrets. `CredentialProxy::new` runs the same check itself, so settings
  built some other way than `from_json` can't skip it.
- **The statement key.** `statement_key(caller, transaction)` resolves the private half of the key the caller's
  admitted release declares as `statement_key`. It does so only while the caller's module handles an operation for
  that transaction. Signing a result follows the sandboxed part's answer, not an outside request, so no outside
  request need be in flight. It refuses, before asking the store, a key the provider's accepted advertisement
  doesn't list, a key it lists for `advertising` (`THR-4`), and the deployment's operator key. The key is read from
  the role's `keys` prefix, never its `secrets` one, at `{keys}/{provider}/{public key text}`. A declared secret
  therefore can never reach a key, even one named like the key. The base64url text is already one plain segment.
  Reading of "by the deployment's name" in the secret-lookup schema: the key sits where the deployment's settings
  put it (the role's `keys` prefix), under its provider. It is named by the public key the release declares, so
  only the declared key can be addressed. The contract gives this lookup no wire shape, and `wire.rs` has no
  message for it, so the platform doesn't call it yet. The statement key's wire message moved to `T-014-05`.
- **The production transport.** `proxy::HttpsTransport` sends one HTTP/1.0 `GET` per connection, so no answer is
  chunked. It speaks TLS through rustls 0.23, a pure-Rust TLS implementation, over ring's primitives
  (`Apache-2.0 AND ISC`). It uses no OpenSSL, and no aws-lc, whose C build needs cmake. The handshake, and so the
  certificate and name check, finishes before a byte of the request is written. It trusts no built-in roots. The
  deployment's settings name the CA file its store's certificate must chain to, since the store is the operator's
  own. So `webpki-roots`, and the licence question the T-014-01 notes raised about it, never enter the build.
  `HttpsTransport::for_role` builds a role's transport from its settings: the store's address, and the CA read from
  its `ca_file`. Plain HTTP is allowed only to an address that is literally loopback (`127.0.0.0/8`, `::1`). It is refused when
  the transport is built, before anything is resolved or sent, and so is any host name, `localhost` included. The
  transport has a timeout for connecting, sending and reading (10 s by default) and a cap on the answer (1 MiB by
  default). It never follows a redirect: the status goes back to the resolver, which reports it as `Unexpected`.
  Its tests run against a local rustls server. That server's test-only certificates are under
  `components/prov/tests/fixtures/proxy/tls/`, valid until 2126, and no deployment trusts them.
- **The transport against a real store.** CI's live test
  (`live_servers_resolve_and_rotate_a_secret_when_configured`) reads through `HttpsTransport`, over plain HTTP to
  the dev servers at `127.0.0.1`, so CI runs the production transport against OpenBao and Vault. Only its writes,
  which the test alone makes, go through the test's own plain-HTTP client. It couldn't be run here, where no live
  server runs; it skips without `RUTENT_LIVE_*`.
- **Dependencies.** `rustls` (with `ring`, `std` and `tls12`, no default features) and `rustls-pki-types` become
  normal dependencies of `rutent-prov`. `Cargo.lock` gains `ring`, `rustls`, `rustls-pki-types`, `rustls-webpki`,
  `subtle`, `untrusted`, `zeroize`, `getrandom` 0.2, `wasi` 0.11 and the `windows-*` 0.52 family. No locked
  version changes. Most of those versions were already in `licences/RECORD` from the experiments' locks.
  `licences/RECORD` gains three lines, all permissive standing: `rustls 0.23.46`, `rustls-webpki 0.103.16`,
  `zeroize 1.9.1`.
- **Property tests (`DEC-18`).** `proxy/tests/properties.rs` checks four invariants:
  - over random sequences of operations, requests and lookups by four callers, a lookup resolves exactly when the
    caller's own request in flight matches and the secret is declared, and it asks only the caller's role's bucket
    for the caller's own path;
  - only a declared statement key listed for `statements`, and not the operator key, is ever asked for, and never
    one listed for `advertising`;
  - every lookup leaves one record with its caller's transaction, and no record prints a value;
  - a declared name reaches the store only as one plain segment of at most 255 bytes.
- **`DEC-20`.** This task touches the credential proxy and its resolvers. Its commit says so.
- **From review.** A reviewer's findings, answered in a follow-up commit:
  - **Buckets compared by their real path.** The overlap check compared prefixes only within one store, namespace
    and mount. So mount `secret` with prefix `data/x` aliased mount `secret/data` with prefix `x`, and namespace
    `ns1` with mount `secret` aliased mount `ns1/secret`. It now compares the path the store resolves, segment by
    segment, per store: namespace, mount, `data`, then the prefix. The store's host is compared without case.
  - **A body cut short.** An answer with no `Content-Length` stands only if the connection ended cleanly, with
    TLS's close_notify or the end of a plain connection. One that just stops is refused.
  - **The statement key's purpose and the operator key.** A key the advertisement lists more than once is refused
    if any entry lists it for `advertising`. The operator key now comes from the proxy's settings: `ProxySettings`
    names the deployment, `{name, operator_key}`. `admit` refuses an advertisement of another deployment, so an
    advertisement can't move the operator key.
  - **Only the crate builds a `Caller`.** `Caller::process_of` is `pub(crate)`, so no code outside `rutent-prov`
    can claim to be a provider's process. A `compile_fail` doc test keeps it so. The proxy's tests therefore moved
    from `tests/` into the crate, at `components/prov/src/proxy/tests/`. A dev-dependency of the crate on itself,
    which would have kept them outside, is a dependency of ours that the `DEC-2` boundary check refuses. The HTTPS
    transport's tests need no caller and stay in `tests/proxy_https.rs`.
  - **Moved to `T-014-05`**, the row the orchestrator adds:
    - an egress `Connector` built on this transport's `ClientConfig` and handshake;
    - the production `World`, with the platform calling `begin_*`/`end_*`, and the `World::secret` change;
    - the store failure's contract reason;
    - the statement key's wire message.
