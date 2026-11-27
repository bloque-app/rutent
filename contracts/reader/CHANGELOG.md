# reader changelog

## 1.0.0
- The seventh contract, the protocol Core and a reader host speak over mTLS (`T-020-59`; `DEC-28`, ADR-0007 as amended
  on the owner's finding 7 on the demo of 2026-11-25 and his form answers `reader-registration-path` and
  `reader-answer-trust` of 2026-11-26). Each public network's reader is a reader service outside Core, and outer code
  never names Core's internal contract, so the protocol is a contract of its own. Three schemas: `common` (the request
  id, a network's identity, its finality and reorg rule, a reader's trust rule, an RPC endpoint, the question asked),
  `question` (`ReaderQuestion`: the reader, the network, `network_rules`, `movement_status` or `movement_of`, each
  read-only, under a `RequestId` Core draws at random, never a counter, and a deadline Core checks on its own clock) and
  `answer` (`ReaderAnswer`, a statement in the contracts' envelope, shape `urn:rutent:reader:answer`, made by the reader
  and signed with its statement key, naming the request, the network, the reader's version and what was asked, and
  answering it or saying it is unreadable). `check_reader_answer` refuses an answer to another request, question,
  network, movement or transaction, from another reader, under another key or version, or past its deadline on Core's
  clock (`ReaderMismatch`). The trust rule is `light_client`, or `rpc_pool` with its endpoints and a quorum of at least
  two (`LEAST_QUORUM`); a pool with a quorum below two, an endpoint twice, or fewer endpoints than its quorum is refused.
  12 valid fixtures and 26 invalid, two of them refused by the Rust types alone. (DEC-4, DEC-28, REQ-ORD-39, REQ-AUD-4,
  T-020-59)
- The review of `T-020-59` (#343), folded into this unreleased version: a network's identity is paired with its trust
  rule, a chain id read from an RPC pool and a genesis hash by a light client (`check_pairing`, `NetworkRules::check`,
  `network_rules`' `allOf`; M1); a pool names each host once, whatever the scheme, port or trailing `/`
  (`RpcEndpoint::host`; M2); an endpoint names a host's DNS name of two labels or more, never an IP literal
  (`is_host_name`; L1), with no path beyond `/`, so nothing that could be a key is in a listing (L2); and
  `check_reader_answer` takes `Expected` (the deployment, the protocol versions in force, the listed key and version),
  holding the envelope's deployment and protocol version and the answer's own rules (`OfAnotherDeployment`,
  `UnderAProtocolVersionNotInForce`, `BreaksItsShape`; L4). 12 valid fixtures and 36 invalid, five of them refused by
  the Rust types alone. (DEC-4, DEC-28, REQ-ORD-39, REQ-PRT-1, REQ-MDL-6, T-020-59)
