# The reader contract

The protocol Core and a reader host speak over mTLS (`DEC-28`, ADR-0007 as amended, `T-020-59`). Each public
network's reader is a reader service: a separate process of the operator's own, run by a reader host
(`CMP-EVIDENCE`'s standalone part, `T-015-04`), never linked into Core. The reader service is outside Core, and outer
code never names Core's internal contract, so the protocol is a contract of its own, the seventh, versioned under
`DEC-4` from 1.0.0. The listing of a public network, which names its reader, is a catalogue change of Core's internal
contract and uses this contract's network identity and trust rule.

Rust: `contracts/src/reader/` (`rutent_contracts::reader`).

## The shapes

| Shape | Schema | Rust |
|---|---|---|
| A question | `urn:rutent:reader:question` | `ReaderQuestion`, `Asked` |
| An answer | `urn:rutent:reader:answer` | `Statement<ReaderAnswer>`, `Answered` |
| The shared atoms | `urn:rutent:reader:common` | `RequestId`, `NetworkIdentity`, `Finality`, `ReorgRule`, `TrustRule`, `RpcEndpoint`, `Reason` |

## A question

A question names the reader asked (by its name on its host), the network (as an outer node names it), what is asked,
a request id and a deadline. It travels over the mTLS link under the operator's CA and is not a signed statement:
only Core asks. Each question is read-only:

- `network_rules`: the network's identity, its finality definition and reorg rule, the reader's version and its trust
  rule, what Core's start-up check compares with the listing.
- `movement_status`: where a movement stands, by its id: `unknown`, `pending` or `final` with its destination, asset
  and amount, or `reorged_out`.
- `movement_of`: the movement the network ties to a segment's transaction, by the network's own matching, or none.

The link needs no sequencer on three conditions Core holds (`DEC-28`): only Core asks; Core draws each request id at
random, never from a counter, so a host can't compute an answer before it is asked; and an answer's deadline is
checked against Core's clock, never the host's.

## An answer

An answer is a statement in the contracts' envelope (`urn:rutent:provider-role:statement-envelope`), shape
`urn:rutent:reader:answer`, whose maker is the reader, signed with the reader's statement key. The reader host signs
it, so the reader's process never holds the key. It names the request it answers, the network, the reader's version
and the question as asked, and answers it, or says it is `unreadable`, with why, for any question: a trust rule not
met, start-up checks not passed, or a reader that crashed, hung or hit a limit. Unreadable settles nothing and grades
nothing down.

## Rules, and where each is enforced

| Rule | Schema | Rust |
|---|---|---|
| A request id is 32 lower-case hexadecimal characters, 128 bits, its top 64 bits never all zero, as a counter's would be | `request_id` | `RequestId` (`from_random` for Core's draw) |
| A question names its request id and its deadline | `question`'s `required` | `ReaderQuestion` |
| A network's identity is a genesis hash (`0x` and 64 lower-case hex) or a chain id (decimal, no leading zero) | `network_identity` | `NetworkIdentity`, `GenesisHash`, `ChainId` |
| A pool's quorum is at least two | `trust_rule`'s `minimum` | `TrustRule::check` |
| A pool names each endpoint once | `uniqueItems` | `TrustRule::check` |
| A pool names each host once, whatever the scheme, port or trailing `/`: a quorum is of nodes, not of spellings. Separate names that resolve to one provider are beyond the contract | none | `TrustRule::check` (`RpcEndpoint::host`) |
| A pool names at least as many endpoints as its quorum | at least two endpoints (`minItems`) | `TrustRule::check` |
| An endpoint is an `https` or `wss` URL to a host's DNS name of two labels or more, never an IP literal, with an optional port and at most a trailing `/`: no user, path, query or fragment, so nothing that could be a key is in a listing. A node that needs a key or a path gets it from the reader host, through the egress layer's secret lookup | `rpc_endpoint`'s `pattern` and `not` | `RpcEndpoint`, `is_host_name` |
| A network's identity is paired with its trust rule: a chain id is read from an RPC pool, a genesis hash by a light client | `network_rules`' `allOf` | `NetworkRules::check`, `check_pairing` |
| An answer answers the kind of question it names, or is unreadable | none | `ReaderAnswer::check` |
| An answer is held to the question asked: its request, network, kind, movement and transaction | none | `check_reader_answer` |
| An answer is the listed reader's: its maker the reader asked, its key the listed statement key, its version the listed version | none | `check_reader_answer` |
| An answer reaches Core by the question's deadline, by Core's clock; the answer's `made_at` is never read for it | none | `check_reader_answer` |
| An answer names Core's deployment and a protocol version in force (`REQ-PRT-1`), and holds to its own rules, built in memory as read | the envelope's | `check_reader_answer` (`Expected`, `Statement::check_context`, `ReaderAnswer::check`) |
| An answer's envelope names its shape | `const` | `Statement::check_shape`, in `check_reader_answer` |

`check_reader_answer` holds what the answer's signature covers. Verifying the signature, and the host's certificate
against the operator's CA and the listed subject name, are Core's (`T-020-60`). Every struct refuses unknown fields,
and optional fields refuse `null`.

## Fixtures

`fixtures/valid/<shape>-<case>.json` each validate and read back unchanged through Rust; `fixtures/invalid/` each is
refused by the schema and by Rust, save the five the test names that only Rust refuses
(`answer-of-an-rpc-pool-with-fewer-endpoints-than-its-quorum`,
`answer-of-a-movements-status-to-a-network-rules-question`, and the three pools naming one host twice). No test
verifies a fixture's signature.
