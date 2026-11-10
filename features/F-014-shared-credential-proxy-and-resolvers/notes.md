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
- **Gap in the contract, item raised: a TLS refusal reason (closed by `T-010-09`).** `module-interface`'s
  `RefusalReason` had no reason for a failed TLS check, so `Refusal::NotVerified` was recorded as
  `undeclared_destination`, on the reading that a peer which doesn't prove it is the declared host is not shown to
  be the declared destination. Module-interface 4.0.0 adds `peer_not_verified`, which `Refusal::reason` now gives.
  The hop keeps the destination it matched by name and has no address, since no verified connection was made.
- **Gap in the contract, item raised: an unanswered hop that isn't refused (closed by `T-010-09`).**
  `validate_record` allowed an unconnected hop only as the last hop of a refused call, so a call whose dial failed
  had no valid shape with `address` absent, and `ConnectError::Unreachable` was recorded with the address dialled.
  Since module-interface 4.0.0 an `unknown` call's last hop may match a destination and name no address, and
  `Egress` records it so: `ConnectError::NoAnswer` keeps the address it connected to, `Unreachable` names none.
  Both stay `unknown`, and `Egress` still tells them apart (`Outcome::Unreachable` and `Outcome::NoAnswer`).
  Recording `unknown` and not `failed` errs the safe way for money: the caller checks rather than assuming nothing
  happened. The platform's current path, `World::route` returning `None`, records `failed` with no address, which
  `validate_record` refuses. That is noted here and not fixed (`module/` is `F-013`'s); `T-010-09` left it too,
  since `T-014-05` replaces the loop with `Egress`.
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
  missing secret from a failed store. The contract reason for that, `store_unavailable`, came with `T-010-09`
  (module-interface 4.0.0); the hook change stays `T-014-05`'s. When it lands, `module/platform.rs`'s `lookup`
  must map `store_unavailable` explicitly: its reply's `_ =>` arm turns every reason but `not_found` into
  `"undeclared_secret"`, so a store failure would go out under the wrong reason (`T-010-09`'s review).
- **Admission.** `admit` takes the accepted `AdmissionArtefact` and the advertisement the catalogue accepted for its
  provider. It refuses an artefact that isn't accepted yet, an advertisement of another provider, and a provider the
  settings give no role. It does not re-run `validate_artefact`, which admission (`F-013`) already runs. Its own
  checks are acceptance and what its paths need.
- **The 255-byte cap.** The carried item is held at admission, not only at call time. `admit` refuses
  (`UnaddressableName`) a release whose provider, or any secret name it declares, isn't one plain path segment of at
  most 255 bytes. The contract's `Name` allows any non-empty text, so without this a release could pass admission
  and then fail every lookup. The resolver keeps its own check as well. A property test drives generated names of 1
  to 300 bytes through admission and lookup.
- **Refusals on the wire.** The contract had two reasons, `undeclared_secret` and `not_found`, and `contracts/` is
  `F-010`'s, so no reason was added here. `Refusal::wire_reason` maps `NotFound` to `not_found`, and every other refusal
  to `undeclared_secret`: no role, no release, no request in flight, a request of another provider, another
  transaction or operation, an undeclared operation or secret. That matches what the platform already answered, and
  it tells a module nothing about another provider or the deployment. A store failure (denied token, unavailable
  store, transport) had no reason in the contract, so `wire_reason` gave `None` and `proxy::answer` no answer. The
  record keeps the true cause in each case. `T-010-09` added `store_unavailable` (module-interface 4.0.0): every
  store failure now maps to it, and `wire_reason` and `answer` are total.
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

    The store failure's contract reason moved to `T-010-09`, which added it as `store_unavailable`.

## T-014-04

- **The rule.** `proxy::judge_call` is the cross-module call rule on its own, in the contract's order of checks:
  - `undeclared_callee` when the caller's own running release doesn't declare the callee. A release of another
    provider than the caller declares nothing for it.
  - `undeclared_kind` when it declares the callee but not the kind of the operation forwarded, or the caller's
    code named another kind (as `T-013-01` had it).
  - `not_named_by_route` when no outside request of the caller's is in flight, or no segment of the
    transaction's route names the caller's call to that callee for that kind.

  `module/platform.rs`'s `cross_module` calls it instead of its inline `match`, and its record names the route
  the judgement gives instead of the suite's placeholder. Whether the callee runs (`callee_unavailable`) stays
  the platform's, after the rule.
- **Gap: no contract shape says which calls a route names.** `REQ-MDL-21` makes a call only "where the route
  declares it", and the call record names a `RouteReference`. But the router-api decision record, its candidates
  and their operations name no cross-module call, and `contracts/` is `F-010`'s. Reading: the platform holds
  `proxy::RouteCalls`, the calls the routes in flight name, by transaction. Each is a `NamedCall`: the route
  reference, the caller, the callee and the kinds. Whoever hands the platform a route segment's operation names
  that segment's calls there (`Platform::routes`, a public field like `world`), and ends them with the route
  (`RouteCalls::end`). Nothing is carried from one transaction to another. A contract shape for a segment's named
  calls, in router-api, is an item for `F-010` and the router's feature. When it lands, `RouteCalls` is filled from
  it and the rule is unchanged.
- **The route a record names** (as corrected in review, below). A call refused by the route check is recorded
  against `proxy::no_route`. A call a segment names is recorded against that segment, whatever else it fails.
  Otherwise it is recorded against the first segment of the transaction's route that names any call of the
  caller's, the segment it is on, and failing that against `proxy::no_route`, since the record must name a route.
- **The callee's own credentials.** The call carries none. The callee runs in its own process, so the platform
  attributes each of its lookups to the callee (the top of its stack) and judges it with `proxy::judge` against
  the callee's own release and the callee's own outside request in flight. The caller's request in flight gives
  the callee nothing. The shared proxy keeps one `Handling` per provider, so a callee's operation and request sit
  beside its caller's, and its lookups reach only its own path in its role's bucket.
- **Re-entrance, for `T-014-05`.** A call back into a provider already on the stack (A calls B, B calls A) is
  refused as `callee_unavailable`: the platform takes a provider's process while that process runs an operation,
  so A has none to hand the call to. That matters once `CredentialProxy` is the platform's `World`. Its
  `begin_operation` replaces a provider's `Handling`, so a re-entrant operation would end A's outer request in the
  proxy. `T-014-05` must keep re-entrance refused, or make `Handling` a stack.
- **The suite's route.** The module-boundary suite has no routes. The tests' binding (`tests/module_support`,
  shared with `F-013`) therefore names, for each operation's transaction, every call an accepted release declares,
  under the suite's route `conformance-route`/`segment-1`, and ends them when the operation ends. Its records keep
  the reference they had. The platform still judges each call against the caller's running declaration first. The
  route tests (`tests/proxy_calls.rs`) hand operations to the platform directly and name the route's calls
  themselves.
- **The edit to `module/platform.rs`.** About 20 lines: the import, the `routes` field and its initialisation, the
  `judge_call` call in place of the inline `match` (14 lines), the record's route, and the function's doc comment.
  The limit, depth, deadline and `callee_unavailable` lines are untouched, since `T-010-09` rewrites them, and
  nothing else in `module/` changes.
- **Tests.**
  - `undeclared_cross_module_call_is_refused` (`tests/proxy_calls.rs`) covers several cases, and in each the callee
    receives nothing and the record passes `validate_call` and `validate_call_against`:
    - a call the caller doesn't declare, which a route names, is refused as `undeclared_callee`;
    - a declared call is refused as `not_named_by_route` when no route names it, or a route names it only for
      another caller, kind, callee or transaction;
    - the control, once the route names it, is delivered and recorded against that segment, and is refused again
      once the route ends.
  - `declared_call_runs_on_the_callees_own_credentials`: caller and callee each hold a secret of the same name with
    its own value.
    - The callee is handed only its own value, and the caller only its own.
    - The callee's lookups are attributed to the callee and the call's transaction.
    - Its lookup of the caller's other secret, by its name, is refused as `undeclared_secret`.
    - The call is delivered and recorded against the route's segment.
  - In the crate (`proxy/tests/calls.rs`):
    - one test has a callee's lookups beside its caller's request in flight in the shared proxy;
    - one judges a call against another provider's release.
- **Property tests (`DEC-18`).** Over generated declarations, route namings, ended routes and calls, `judge_call`
  allows a call exactly when the callee and kind are declared, the kind named is the one forwarded, a request is in
  flight and a segment of that transaction names that call. A refusal names the first check it fails. The route
  recorded is the naming segment, the caller's, or none. Every record built from a judgement passes
  `validate_call` and `validate_call_against`. The generator reaches every outcome more than 50 times.
- **How the tests were run.** Written first. The crate's tests failed to compile against the missing
  `judge_call`, `RouteCalls` and `NamedCall`. The credential assertions can't run on `m2`, where the test doesn't compile,
  but reading `m2`'s `lookup` says they would hold: the platform already attributed a callee's lookups to the
  callee. The test pins that. The route check is the red part: with
  the route check taken out of `judge_call`, `undeclared_cross_module_call_is_refused` fails on "a declared call no
  route names", which is delivered.
- **No owner yet fills `Platform::routes` in production.** No `PLAN.md` row names who hands the platform a
  segment's named calls, so until one does, every cross-module call on the running platform is refused as
  `not_named_by_route`. That fails closed. It is an item for the orchestrator, beside the contract gap above.
- **One transaction per operation.** Each operation the route tests hand out has its own transaction, so they
  keep passing once `T-013-06` refuses a duplicate transaction identifier (`REQ-MDL-14`).
- **`DEC-20`.** This task touches the credential proxy's path: `proxy/`, and `platform.rs`'s cross-module router,
  which decides whose credentials a call acts on. Its commit says so.

## T-014-04, review round

- **Merged `m2` (4c22cba).** router-api is 3.0.0 and module-interface 4.0.0. `T-011-06` now fills `RouteCalls`
  from the router's decision, `T-014-05` keeps re-entrance refused on purpose, and Core (`M3`) fills
  `Platform::routes` from live orders. That answers this task's gap and owner items. The conflicts were in the
  imports of `module/platform.rs`, and in the tests' binding, where `m2`'s admission by assertion bytes and
  acceptance by digest now sit beside the binding's record of accepted releases.
- **Re-entrance has a test.** `call_back_into_a_provider_already_running_is_refused_as_unavailable`: A calls B,
  and B calls back into A, with the route naming both calls.
  - B's call back is refused as `callee_unavailable`, and A's call to B is delivered.
  - A is handed only its own operation.
  - A makes one lookup, its own, after the call, and none inside B's call.

  It passed at once. The platform takes a provider's process while that process handles an operation
  (`run_in_process`), so the callee check finds A without one. The test pins that until `T-014-05` refuses
  re-entrance on purpose.
- **The callee's lookup of the caller's secret.** It now reaches the store. The callee declares `caller.only` too,
  so its own declaration lets the lookup through. It is `not_found`, because the callee's lookups are keyed by the
  callee and only the caller holds that name. Before, the callee's own declaration refused it as
  `undeclared_secret`, which showed nothing about whose keys were read.
- **The no-route placeholder.** Its ids are now `rutent:no-route` and `rutent:no-segment`, reserved as
  `proxy::NO_ROUTE_RECORD` and `NO_ROUTE_SEGMENT`. A decision record's id is its request's, and no router may issue
  one by that id. A call refused as `not_named_by_route` is always recorded against it. Before, a call with no
  request in flight could name a segment beside `not_named_by_route`. The property test checks this.

## T-014-05

- **Merged `m2` at `28f16cc`** (a fast-forward), which carries `T-010-10`: real signatures in the suites, admission
  through the catalogue's verifier, `ReleaseJudge` sealed. The orchestrator widened the row after `T-010-10`'s
  review: the platform's results are signed with the statement key (below).
- **The production world.** `proxy::ProductionWorld` is the credential proxy and the egress layer as
  `module/platform.rs`'s `World`. The platform tells it what each provider's module handles, from its own state:
  - `running(artefact)` when a release's process starts, which admits the release to the proxy against the
    accepted advertisement the deployment handed the world (`advertise`). A release the proxy refuses is retired
    from it (`CredentialProxy::retire`), so no lookup of that provider resolves; `take_unadmitted` names it.
  - `begin_operation`/`end_operation` around each operation, on every exit path (answer, trap, limit, deadline).
  - `begin_request`/`end_request` where the platform passes an outside request to the outside part and takes its
    response.
- **Two judgements, one rule.** The platform still judges every lookup itself (`proxy::judge`, from its stack), and
  then hands the world the lookup with that verdict (`World::lookup`). The production world judges again in the
  proxy, from what the hooks told it, and records the use. A lookup the platform refused is recorded in the proxy
  (`CredentialProxy::refuse`) without asking the store, so the proxy's uses match the platform's lookup events one
  to one. Whatever a world answers, the platform hands no value for a lookup it refused.
- **`World` keeps its four required hooks.** `secret`, `state`, `route` and `deliver` keep their shapes, so the
  suites' double (`TestWorld`) runs unchanged; every new hook has a default built on them. `World::secret`'s
  `Option` stays the double's: it means "holds none". The production path is `World::lookup`, which returns the
  contract's `SecretRefusalReason`, so a store failure is `store_unavailable`, never `not_found`.
- **`store_unavailable` at the lookup.** `platform.rs`'s `lookup` maps each reason explicitly (`secret_reason`):
  `not_found`, `store_unavailable`, `undeclared_secret`, and `undeclared_secret` for a reason it doesn't know. The
  lookup event records `StoreUnavailable`. **Gap, item for `F-013`:** the WIT's `secret-refusal` enum has only
  `undeclared-secret` and `not-found`, so a guest is told `undeclared-secret`; the record keeps the true reason.
  Adding the case is a change to the binding's interface and its guests, not made here.
- **Egress on the platform's path.** `platform.rs`'s `egress` builds a `Call` from its context and hands it to
  `World::send`; the platform's own hop loop, `allows`, `within` and `MAX_HOPS` are gone. Every world goes through
  `Egress`: the production world over its `NameResolver` and `Connector`, the double through `Served`, which reads
  `route` as the name's resolution and `deliver` as the exchange, answering `200`, or `307` with a `Location` for a
  redirect (so the body follows, as the old loop delivered it). The module-boundary suite passes through it
  unchanged. The binding's egress call carries only a body, so it goes as a `POST` to `/`; a target, a method and
  headers need the WIT to carry them (item for `F-013`). An unknown scheme on the wire is refused, unrecorded,
  where the old loop read it as `https`.
- **The no-route path records its own reason.** A declared destination whose name resolves to no address is refused
  as `address_not_allowed` (`T-014-03`'s reading: there is no checked address to dial); one whose checked address
  can't be reached is `unknown` with no address, which module-interface 4.0.0 accepts. Before, it was `failed`,
  which `validate_record` refused. Test: `call_with_no_route_records_its_own_reason_and_a_valid_record`.
- **The rustls connector.** `egress::TlsConnector`, on `proxy/https.rs`'s shared primitives (`client_config`,
  `handshake`, `exchange`), which `HttpsTransport` now uses too:
  - it dials `Dial::to` through a `Dialer` (`DirectDialer` in production) and never resolves the name; it reads no
    proxy setting;
  - SNI and the certificate check are for `Dial::name`, against the deployment's PEM roots only (no built-in root
    store, so no `webpki-roots`); the handshake finishes before a byte of the request is written, and any failure
    of it, a timeout included, is `NotVerified`;
  - one connection per exchange, session resumption disabled and TLS 1.3 early data off, both set explicitly in
    the shared configuration, so the proxy's transport has them too;
  - a `3xx` comes back with its `Location`; `wss` is refused before connecting, as the contract says;
  - HTTP/1.0 with `Connection: close`, as the transport speaks, so no answer is chunked; one with a transfer
    encoding, cut short or over 1 MiB is `NoAnswer`. Timeout 5 s each for connect, handshake, send and read.
  The connector's timeout is not yet bounded by what is left of the operation's deadline; the platform serves a
  call synchronously, so a slow peer spends the module's time. Noted, not changed.
- **`tls_probe` on real TLS.** `tests/tls_support/` generates a CA and leaves with `rcgen` in each test, runs rustls
  servers on loopback, and maps each public address to one (`MappedDialer`), since the egress layer refuses
  loopback. `tests/egress.rs` runs `tls_probe` with `TlsConnector` against a leaf for another name and a server
  offering no certificate (its resolver returns none), plus a control for the right name, a self-signed chain and
  a returned redirect. **`B-18` holds end to end**; `tls_check_against_a_wrong_name_or_no_certificate_is_refused_end_to_end`
  shows it on the running platform. **`B-17` holds on the platform's path**:
  `running_platform_resolves_a_destination_once_and_dials_only_the_checked_address` resolves once per call, dials
  only the first answer's address while the name would re-resolve elsewhere, and refuses a pinned destination
  resolving outside its block without dialling it.
- **Re-entrance refused on purpose.** `cross_module` refuses a callee already on the platform's stack (`reenters`)
  as `callee_unavailable`, before the process check: one provider handles one operation at a time, and the proxy
  keeps one operation and one request per provider. Test:
  `call_back_into_a_running_provider_leaves_its_request_in_flight_in_the_proxy`, where A's request stays in flight
  through B's refused call back and A's later lookup resolves. It passes with or without `reenters`, since the busy
  process refuses the call too; `reenters` makes the refusal not depend on that.
- **Signing the platform's results (the widened row).** The module never holds a key. The platform resolves the
  statement key through `World::statement_key` while the provider's module still handles the operation (the
  proxy's condition), then `proxy::sign_result` signs the result and every statement the module nested in it in the
  provider's name, innermost first, under the provider-role rule. Readings:
  - the store holds the private half as the unpadded base64url text of its 32-byte Ed25519 seed, under the public
    key's text in the role's `keys` prefix (`T-014-02`);
  - a key whose public half isn't the declared `statement_key` signs nothing;
  - a statement nested in the provider's name for another deployment is refused (`THR-8`); one of another maker
    keeps its own signature;
  - only a judgement or a reversal confirmation is signed when nested, the two shapes provider-role's result
    schema nests, each pinned by `const`; the result schema has no open object, and the output is checked against
    it before signing. A nested envelope of any other shape in the provider's name is refused, so the platform is
    no oracle for, say, a result of another transaction lifted out of one it signed;
  - a result that can't be signed is refused as `PlatformRefusal::Unavailable`, never passed on with a placeholder.
    The contract has no closer reason; an item for `F-010` if one is wanted;
  - an operation naming no transaction (a verifier's judgement) is known to the world as
    `rutent:untransacted:<sha256 of the operation>`, so the proxy can resolve the key for it.
  `TestWorld` holds statement keys (`hold_statement_key`), the suites' standard statements key by default; the
  signed harness holds its own. `tests/module_role.rs` runs with `Ed25519Verifier` and nothing deferred, and its
  in-process comparison runs signed too.
- **The statement key's wire message.** Read under the orchestrator's ruling that the key is never in the module:
  the statement key's answer goes from the proxy to the platform's signer, and no `FromProvider` message hands it
  to a provider's process. An outside part that signed would hold the key, which the ruling forbids.
- **Owner items.** None.
- **Dependencies.** `rcgen 0.13.2` as a dev-dependency (`ring` and `pem`, no aws-lc). `Cargo.lock` gains `rcgen`,
  `pem 3.0.6`, `base64 0.22.1`, `time 0.3.55`, `time-core 0.1.9`, `deranged 0.5.9`, `powerfmt`, `num-conv` and
  `yasna`; `licences/RECORD` gains `deranged 0.5.9` and `pem 3.0.6`, both permissive standing.
- **Property tests (`DEC-18`).** `proxy/tests/world.rs`: over seeded sequences of the platform's hooks and lookups
  with random verdicts, the world answers exactly when the verdict passed and the hooks put that request in
  flight, asks the store only then and only at the caller's path, and leaves one use per lookup; the proxy's
  request in flight follows the model. `egress/tls.rs`: the connector frames every request itself, `Host` from
  the declared name. `proxy/signing.rs`: for any key the result and the provider's nested judgement verify, and a
  key other than the declared one signs nothing; a seed has one text.
- **How the tests were run.** Each was run and then shown to fail on a mutation of what it guards: the handshake's
  failure mapped to `NoAnswer` (both TLS tests), the name resolved twice and the second answer dialled (`B-17`),
  `begin_request` not passed to the proxy (the secret test), results passed on unsigned (`module_role`).
- **`DEC-20`.** The credential proxy and its resolvers (`proxy/`), its egress (`egress/`), and the platform's
  credential path in `module/platform.rs` (`lookup`, `egress`, `cross_module`, result signing).

## T-014-05, review round

- **Merged `m2` at `5063302`** (`T-016-02`'s `holding_balance`, the new row `T-013-07`). The two WIT gaps above
  are `T-013-07`'s now.
- **The operation's deadline bounds every outbound step (blocker).** Each read had its own timeout and nothing
  bounded the whole exchange, so a peer sending a byte now and then held the platform, and every other provider,
  past `OPERATION_DEADLINE`. The platform now carries the operation's `due` into the exchange:
  - `Call::due` and `Dial::due` carry it into the egress layer;
  - `World::begin_operation` carries it into the proxy, which keeps it with what the module handles;
  - `Resolver::resolve_until` and `Transport::get_until` carry it into the store. Both default to ignoring it;
    `KvResolver` and `HttpsTransport` honour it.

  The TCP connection is wrapped (`proxy::Bounded`), so every read and write, the TLS handshake's included, waits
  at most `min(step, due - now)`. The connection's own timeout is cut the same way. Once the deadline has passed,
  every step fails as timed out. For an outbound call, a cut before the handshake finished is `Unreachable`, since
  nothing was sent, and a cut after is `NoAnswer`. Both are recorded `unknown`. The operation then ends by the
  deadline's own path: the process is overdue, it is killed, and the operation is refused as `LimitReached`.

  Tests:
  - `slow_peer_is_cut_off_at_the_operation_deadline_and_another_provider_still_runs`: a TLS peer dribbling one byte
    every 300 ms held the platform for 20 s before the fix, and is cut at the 2 s deadline now.
  - `store_dribbling_its_answer_is_cut_off_at_the_deadline`: the same for the store. It was red as a missing API.

  Name resolution through the operating system's resolver is not bounded by the deadline; `getaddrinfo` takes no
  timeout. Noted, not changed.
- **The signer signs only what was asked (blocker).** `sign_result` now takes what the result answers
  (`proxy::Answering`): the provider, the deployment, the protocol version and the operation handed out. A
  statement nested in the provider's name is signed only if it answers that operation:
  - a judgement must answer a `judgement` operation, with its `subject` and `class` equal;
  - a reversal confirmation must answer a `reversal_confirmation` operation, with its `segment`, `asset` and
    `amount` equal.

  Anything else is `NestedNotAsked`, and the result isn't passed on. Tests:
  `judgement_about_another_subject_than_asked_is_not_signed`, `confirmation_of_another_credit_than_asked_is_not_signed`.
- **The nested statement's protocol version (should-fix).** It must be the platform's, or the signer refuses with
  `NestedOfAnotherVersion` (`REQ-PRT-1`). Test: `nested_statement_of_another_version_or_deployment_is_not_signed`.
- **Re-entrance decides on its own (should-fix).** `reenters` takes the providers on the stack and is checked before
  the callee's process. A unit test covers the caller itself, its caller, the top-level provider and one not on the
  stack, and failed against a check of the caller alone.
- **A revoked statement key stops at once (nit).** `ProductionWorld::advertise` now passes the advertisement to the
  proxy (`CredentialProxy::advertise`), so the running release is judged against the new keys from the next
  signing on, with no restart. It refuses an advertisement of another deployment, so it returns a `Result`.
  Test: `a_statement_key_the_next_advertisement_revokes_signs_nothing_more`.
- **`T-016-02`'s wallet tests sign too.** After the merge, the wallets' results went unsigned, so they were refused:
  the signed harness's world held keys for the reserve and the ramp only. `TestWorld::derive_statement_keys` now
  derives every provider's key with the harness's own `statements_key`.
- **How the tests were run.** Each was written first and seen to fail:
  - the slow peer, against the old exchange;
  - the signing tests, with the new checks taken out;
  - re-entrance and revocation, against their old code.
- **`DEC-20`.** The credential proxy (`proxy/`), its egress (`egress/`), and the platform's credential path in
  `module/platform.rs`.
