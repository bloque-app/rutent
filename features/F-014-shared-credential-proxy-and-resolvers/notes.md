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
