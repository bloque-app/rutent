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

## T-014-03

- **What it is.** `egress::Egress` runs the whole of an outbound call: it matches each hop against the destinations
  the operation in flight lists, resolves the host, checks the addresses, dials, reads the answer, follows or
  refuses a redirect, and writes one `OutboundCallRecord` per call to a `CallLog`, refused calls included
  (`REQ-MDL-7`, `REQ-MDL-8`). It takes the platform's state as a `Call`: the running declaration, the operation in
  flight, the operation the outside part named, the transaction, the names of the secrets resolved and the start
  time. A call naming another operation than the one in flight is refused as `undeclared_operation` and recorded
  under the one in flight, as the platform does today.
- **Two seams, no TLS stack.** `NameResolver` resolves a host (`SystemResolver` is the operating system's, with
  nothing cached). `Connector` makes one exchange to a `Dial`: the scheme, the declared host and the checked
  `SocketAddr`. No crate was added, so `Cargo.lock` and `licences/RECORD` are unchanged.
- **DNS rebinding (`B-17`).** The host is resolved once per hop, and the connector is handed the address checked;
  its contract forbids it to resolve the name again. Every address the answer holds must lie within the
  destination's addresses, or the hop is refused as `address_not_allowed`. Reading: a mixed answer is refused
  whole rather than filtered, so an attacker's extra record can't become the address connected to, and an answer
  with no address is refused the same way, since there is no checked address to dial. A `Blocks` destination
  whose name returns an address outside its blocks is a stale declaration, and failing closed is right for it.
  An IPv4-mapped IPv6 answer is judged as IPv6, where `::ffff:0:0/96` is special-purpose, as the platform's
  `allows` does.
- **Redirects.** Followed only for `301`, `302`, `303`, `307` and `308` with a `Location`, to an endpoint the
  operation in flight lists, each followed hop checked like the first, at most `MAX_HOPS` (5) connections, after
  which the call fails, as in the platform. `307` and `308` keep the method and body; the others become a `GET`
  with none. A location is read as an absolute `https` or `wss` URL, a scheme-relative one or an absolute path;
  its host is lower-cased and must be letters, digits, `-` and `.`, its fragment dropped. Plain `http`, another
  scheme, a user name in the authority, an IP literal in brackets, a bad port or anything else is refused as
  `redirect_to_undeclared_destination`. Such a location has no `Hop` shape (the contract's `Scheme` has no
  `http`), so no hop is added for it and the record ends at the hop that redirected. That is valid, since that
  hop connected.
- **TLS (`B-18`).** The `Connector` contract says TLS 1.2 or later, SNI and certificate checked for the declared
  host, no plain-text or lower-version fallback, no proxy from the environment, no connection or session reused
  across names, no redirect followed, and `Host` written by the connector from the declared name. A failed check
  is `ConnectError::NotVerified`, raised before any byte of the request is sent. Egress refuses it at once,
  never retries another address and never offers a plain scheme.
- **Requests can't name a host.** `Request::new` refuses a target that isn't origin form, which closes
  absolute-form, authority-form, `//` and `*`; this is the class of `GHSA-5qpp-g3j2-8frx`. It also refuses a `Host`
  header and any framing header (`content-length`, `transfer-encoding`, `connection`, `te`, `upgrade`, `trailer`,
  `keep-alive`, `proxy-connection`), and a header with a control character in its value. `Debug` never prints a
  sensitive header's value or a body (`THR-50`).
- **Gap in the contract, item raised: a TLS refusal reason.** `module-interface`'s `RefusalReason` has no reason
  for a failed TLS check. Until it has one (a minor bump of `contracts/`, a `DEC-20` path, outside this task),
  `Refusal::NotVerified` is recorded as `undeclared_destination`. The reading is that a peer which doesn't prove it
  is the declared host is not shown to be the declared destination. The hop keeps the destination it matched by
  name and has no address, since no verified connection was made. `Egress` itself returns the precise
  `Refusal::NotVerified`.
- **Gap in the contract, item raised: an unanswered hop that isn't refused.** `validate_record` allows an
  unconnected hop only as the last hop of a refused call, so a call whose dial failed has no valid shape with
  `address` absent. Reading: `ConnectError::Unreachable` and `ConnectError::NoAnswer` are both recorded as
  `unknown`, with the hop's `address` the one dialled, so the record says where the call went. `Egress` still tells
  the two apart (`Outcome::Unreachable` and `Outcome::NoAnswer`). Recording `unknown` and not `failed` errs the
  safe way for money: the caller checks rather than assuming nothing happened. The platform's current path,
  `World::route` returning `None`, records `failed` with no address, which `validate_record` refuses. That is
  noted here and not fixed (`module/` is `F-013`'s).
- **The platform's hook, not edited.** `module/platform.rs` is untouched. Its `World::route` then `World::deliver`
  splits the check from the use: against a real network, `deliver` would resolve again, which is the window
  `B-17` closes, and a redirect is known only after sending, not from a lookup. So the platform's loop can't
  wrap a real egress unchanged. At integration, the platform's `egress` should build a `Call` from its `Context`
  (`declaration`, `serving`'s operation, the claimed operation, `transaction`, `resolved`, `now()`). It should
  hand the call to `Egress::send` with its `Events.records` as the `CallLog` (a `Vec` is one), and map `Outcome`
  to its reply. Then `World::route` and `World::deliver` give way to the egress, and the platform's `allows`,
  `within` and `MAX_HOPS` to the egress layer's. The tests' `TestWorld` becomes a `NameResolver` and a `Connector`.
  `egress::Endpoint` mirrors `module::types::Endpoint`, so that the egress doesn't depend on `module/` while
  `T-013-02` and `T-013-03` edit it; one of the two can go at integration.
- **How it meets `T-014-02`'s transport.** `proxy::Transport` fixes the store's address in the transport and
  sends only a `GET`, so it can't carry a pinned address and a name to verify. Both traits should sit on one
  primitive of `T-014-02`'s `rustls` code: dial this `SocketAddr`, then handshake and verify as this `ServerName`
  with the shared `ClientConfig`. That code has to expose it. `Connector::exchange` is that primitive plus writing
  one HTTP/1.1 request (`Host` from `Dial::name`, `Content-Length` from the body) and reading one answer with its
  bounds. `proxy::Transport::get` is the same primitive with the store's address and name from configuration.
  Two cautions for that transport. Its plain-HTTP-on-loopback allowance for development servers must not be
  reachable through `Connector`, since `Scheme` has no plain variant and a `Dial` is always TLS. Its 3xx must come
  back as a response, as `proxy::Transport` already requires. `wss` needs the WebSocket upgrade, which the
  connector writes; no outside operation in `M2` uses `wss`.
- **What the tests show, and what waits.** `tests/egress.rs` has the row's four tests under their names, and more:
  a redirect re-resolved within one call, a mixed answer, pinned blocks, another operation's destination,
  downgrading redirects, a followed redirect, the hop bound, no answer, the operation in flight, and request
  hygiene. The loopback servers stand for addresses through a test connector that maps each public address to a
  listener of its own, so a test sees that a refused address received nothing. They announce the name they are
  certified for, and the connector checks it before sending, standing in for a TLS client. `tls_probe` is generic
  over the `Connector`. **`B-18` end to end, with a certificate for another name and with none, is not closed
  here.** It waits on running `tls_probe` with `T-014-02`'s `rustls` connector against loopback servers with real
  certificates, at integration. `B-17` is closed at the seam: the egress dials only the checked address, and a
  connector that honours `Dial::to` can't be rebound.
- **Property tests (`DEC-18`).** `tests/egress_properties.rs`, over seeded random declarations, a resolver whose
  answer changes on each question and a connector answering each dial with a success, a failure, any redirect, a
  failed TLS check or no answer. Two properties. Every dial is to a destination the operation in flight lists,
  right after its name's resolution, at that answer's first address, every address in the answer within the
  destination's addresses. Every call leaves exactly one record, which `validate_record` and
  `validate_record_secrets` accept, with one connected hop per dial except a failed TLS check. The second also checks
  that the cases reach every outcome and some followed redirect, so neither property holds vacuously. No
  `proptest`; the package's seeded `Rng`.
- **Tags.** `B-17` and `B-18` are rows of `EXP-2`'s scoring sheet, not identifiers `SPEC.md`, the ADRs or `PLAN.md`
  define, so the tests tag `REQ-MDL-7`, `REQ-MDL-8` and `THR-79` and name the rows in their doc comments.
- **`DEC-20`.** The credential proxy's egress is a `DEC-20` path; the commit body says so.

## T-014-03, review round

- **Merged `m2`** before answering the review.
- **Credentials don't follow a redirect to another endpoint.** `Request::redirected` dropped nothing, so a `302`
  from `api` to `files` carried `api`'s `Authorization` to `files` (the curl CVE-2018-1000007 class). Now every
  header built with `Header::sensitive` is dropped whenever the redirect changes the scheme, host or port. A
  redirect within the same endpoint keeps it. Plain headers follow either way. This replaces the earlier
  reading that a declared destination of the same operation may receive them. Test:
  `redirect_to_another_host_drops_the_credentials_meant_for_the_first`.
- **Pinned blocks get the special-purpose check too.** `allows` trusted that the declaration had passed
  `validate_declaration`, and nothing under `components/prov/src` calls it. The special-purpose ranges
  (`SPECIAL_PURPOSE`: loopback, link-local and the metadata address `169.254.169.254`, private, unique-local and
  the rest) are now refused for `Blocks` as for `public`. A block over one of them reaches none of it. Test:
  `pinned_block_over_a_special_purpose_range_reaches_none_of_it`. The property tests' model follows.
- **Connector contract.** It now forbids TLS 1.3 early data (0-RTT), which can be replayed and is sent before the
  peer is verified. It also states that `wss` isn't supported through one exchange: a `wss` dial is checked like
  any other, then the connector refuses it before connecting, as `Unreachable`. Serving WebSocket needs a session
  interface of its own; no `M2` operation uses `wss`.
- **The record test reaches a dial.** `every_outbound_call_is_recorded_with_destination_version_and_transaction`
  claimed more than it ran: `status` is refused before any dial. It now also makes a call that is dialled and gets
  no answer, and its comment says which calls are refused before a dial.
- **Moved to `T-014-05`, the orchestrator's new row.** Two pieces of work above are no longer this task's:
  - wiring `Egress` into `module/platform.rs` in place of `World::route` and `World::deliver`, together with
    running `tls_probe` against `T-014-02`'s `rustls` connector, which closes `B-18` end to end;
  - the `DEC-4` bump of `module-interface`: a refusal reason for a failed TLS check, and a valid shape for an
    unconnected last hop of a call that isn't refused. The two "Gap in the contract" items above are carried there.
- **Credentials dropped by name too (second round).** The drop went by the `Header::sensitive` flag, so a plain
  `Authorization` header still followed a redirect to another host. On a change of scheme, host or port the
  egress now drops every sensitive header and every header named in `egress::CREDENTIALS` (`authorization`,
  `proxy-authorization`, `cookie`), matched in any case. Once dropped, a credential is gone for the rest of the
  call: on a chain from A to B and back to A, it doesn't return to A. Tests:
  `plain_authorization_header_does_not_reach_another_host` and
  `credentials_never_return_to_the_first_host_after_a_redirect_away`.
- **Merged `m2` at `65c8ffe`**, which carries the new rows `T-010-09` and `T-014-05`.

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
  failure moved to `T-010-09`, the row that owns it.
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
    segment, per store: namespace, mount, `data`, then the prefix. The store's host is compared without case, and
    without one trailing dot, so `bao.internal.` and `bao.internal` are one store. A host written once as a name and
    once as an IP address is out of scope: the check doesn't resolve names, so it can't tell they are one store.
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
    - the statement key's wire message.

    The store failure's contract reason moved to `T-010-09`, which owns it.
