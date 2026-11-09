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
