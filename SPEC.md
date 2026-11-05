---
name: rutent specification
version: 0.4.1
status: accepted by the lead on 2026-11-05 (R142); v0.4.1 renames the quoting engine to the router, at the lead's request (R143)
profile: oss-ledger (project-local stand-in, profile.oss-ledger.md)
level: STEP-1
input: PRODUCT.md
date: 2026-11-05
---

# rutent: specification

## 0. How to read this document

States what rutent must do, never how (`PS-62`). MUST, MUST NOT, SHOULD, SHOULD NOT and MAY carry their RFC 2119 meaning, and appear only in tables of numbered items. Mechanisms belong in `suggestions/`, which is non-binding. References name identifiers, not section numbers. An identifier is stable and never reused.

"Pending `OQ-n`" means a statement holds as written, and that open question will add to it. Requirements that go furthest beyond a ruling are listed at the end of section 4. Nothing here describes an existing system (`PS-61`).

## 1. Purpose and scope

rutent is "a funds (or assets) router" (`PS-1`): it routes value between any nodes and assets its catalogue can reach. Taking local money to on-chain balances and back is one outcome of that, not its scope. It is an authenticated log, not a ledger (`PS-33`), and it never holds customer value (`PS-79`). Between segments, value rests in each node's holding account, and rutent's one ledger-like duty is checking that each such account's balance is the sum held there for its orders (`PS-168`). Given an order, it finds a route across providers, follows the order to its end, and records who did what, when, and with what outcome (`PS-21`).

It serves humans and agents on one contract (`PS-2`, `PS-11`). It is deployed and operated independently of any higher-order platform (`PS-12`), with authentication that may be external and authorisation and policy that are its own (`PS-13`). It's built from standalone libraries bound by a core (`PS-10`). Providers join a common catalogue as hot-loaded, signed, sandboxed modules written in many languages (`PS-14` to `PS-20`). It is meant to grow towards an open protocol and order books (`PS-23`), and its protocol specification is part of v1 (`PS-6`).

**Out of scope:** more privacy than each provider already gives (`PS-30`); disclosing balances (`PS-31`); being a ledger (`PS-33`); value that arrives with no order (`PS-34`); a delivered payment later returned, beyond reporting it (`PS-35`); integrators and referrers as parties, and fee shares with them (`PS-36`); what is behind a reserve (`PS-37`); a transfer inside a book (`PS-39`); several orders sharing one movement as a native feature (`PS-38`); scheduling charges, or series of them, which is the integrator's (`PS-161`). Nothing else is out of scope for now (`PS-32`).

**Context, not requirements:** rutent succeeds an existing Bloque service (`PS-40`). Its neighbours and worked examples are deployment-level (`PS-41`), and existing providers are handled separately from rutent (`PS-67`). VOS is the owner's preferred suggestion, not a constraint (`PS-66`). Sector profile `oss-ledger`, adding `reg-fintech` if regulation demands (`PS-60`). The spec stays jurisdiction-neutral (`PS-65`).

Owner's cost and operating facts: every provider charges fees (`PS-4`), and entry is by the operator's explicit grant (`PS-6`). Distribution channels and revenue are unknown (`PS-3`, `PS-5`), and there is no deadline, budget or platform constraint yet (`PS-63`). "Thousands of requests" has no number yet (`PS-50`).

## 2. Glossary

Owner's words where they exist.

| Term | Meaning | Source |
|---|---|---|
| Provider | A member of the catalogue, in one of four categories | `PS-14` |
| Reserve | "where value is stored"; "a special kind of ramps: ramps that route from external to internal segments, and vice versa". A route's endpoints: "reserves can only handle withdrawals and deposits". Whether it is "a book, a bank account, a third-party provider, or a wallet is opaque to us" | `PS-15`, `PS-91`, `PS-128`, `PS-129` |
| Drop provider | One where "the funds leave our book (get burned, redeemed, etc.)". rutent doesn't tell these apart, since what is behind a reserve is opaque | `PS-90`, `PS-129` |
| Exchange | "where asset types are interchanged" | `PS-16` |
| Ramp (provider category) | "intermediaries to get from reserves to exchanges"; "ramps keep the same asset". Ramps can also follow one another, with no exchange on the route | `PS-17`, `PS-70`, `PS-1` |
| Verifier | A provider "that exposes judgements given an identity"; sees a person and a class only. Moves no value | `PS-78`, `PS-95` |
| Node | "a network where multiple reserves might reside"; also "network, book, origin". A publisher can advertise nodes, including private networks. Never a step in a plan: steps are segments | `PS-70`, `PS-74`, `PS-130` |
| Outer node, inner node | Also external and internal: outside and inside the book. A reserve connects the two. An outer node is named for its network and its reserve, and records the origin or destination of funds. Every route starts with a withdrawal, from an outer node into an inner one, and ends with a deposit, from an inner node out to an outer one | `PS-91`, `PS-116`, `PS-126`, `PS-127`, `PS-129` |
| Tuple | "origin-destination" and "asset pair": what a provider advertises it can serve | `PS-70` |
| Segment | What a provider advertises a price to route through; one step of a route, at one provider. Segments "act as transitions on an NFA", some of them changing no value | `PS-76`, `PS-134` |
| Route, graph | "the plan for funds to get from an origin node" and a from asset to a destination node and a to asset. A plan for one order | `PS-74` |
| Order | A user-initiated request to move value along a route, from when it is requested to when it ends | `PS-21`, `PS-114` |
| Requester, beneficiary, payer | Who asks, who receives and who pays for an order. They can be three different parties | `PS-120` |
| Quote | Exchanges expose "detailed quotes with rates and fees"; a provider "advertises the price to route through a segment". The precalculated price is "just a quote, not a promise" | `PS-16`, `PS-76`, `PS-100` |
| Fee | Defined per provider by a formula, with flat and percentage parts, on amount and tuple only | `PS-101`, `PS-111` |
| Reversal | "Only a true undo counts": the value back at the segment's origin node, at the provider's cost | `PS-72`, `PS-92` |
| Stuck order | "funds take longer than twice the p99 to traverse a segment in the stated provider", or twice its advertised time until it has history; also an order with no route left | `PS-81`, `PS-97`, `PS-144` |
| Transaction | "one of the methods exposed by the provider", "uniquely identified" | `PS-19` |
| Amendment in the state | A change to what the system runs on, for example "provider upgrades" | `PS-21` |
| Module, sandboxed part, outside part | A provider's published code. "The part that interacts with the outer world must be split" from the part in the sandbox | `PS-20`, `PS-83` |
| Judgement | A verifier's answer about a person and a class | `PS-95` |
| Accountable party | Who answers for an agent, and whom limits on it bind | `PS-117` |
| Checker | Whoever the operator appoints, outside itself, to check that history hasn't changed | `PS-104`, `PS-118` |
| Router (component) | The component that reads the catalogue's published quotes, builds a graph whose vertices are `(node, asset)` pairs and whose edges are providers' segments, and finds routes and splits for the customer's objective. The brief called it the quoting engine (`PS-10`). Not the product: rutent as a whole is "a funds router" | `PS-10`, `PS-1`, R143 |
| Holding account | Every node has one: where the node keeps value between segments, for many orders at once. At a public node the operator registers it, by default "just plain wallets"; at a private node the publisher gives the operator access to its record of funds | `PS-168` |
| Rollback | Undoing an order as a whole. Never done in normal conditions: recalculation finds another path. A segment's reversal is not a rollback | `PS-166`, `PS-91` |

## 3. Parties

| Party | Role | Source |
|---|---|---|
| End user | A human or an agent. Initiates orders, on one contract | `PS-2`, `PS-114` |
| Accountable party | Stands behind every agent | `PS-117` |
| Requester, beneficiary, payer | May be different people on one order, and each provider's rules say whose eligibility counts | `PS-120` |
| Reserve, exchange, ramp | Providers. Enter by the operator's explicit grant, and post quotes | `PS-6`, `PS-15` to `PS-17`, `PS-80` |
| Verifier | The fourth provider category | `PS-95` |
| Publisher | Publishes a provider's module, with a signed assertion, and can advertise several providers and nodes. The project's own team first; third parties deferred (`DEF-4`) | `PS-84`, `PS-113`, `PS-130` |
| Operator | Alone posts routes, grants entry, and resolves and bears stuck orders. May act for a user at a reserve that declares it | `PS-80`, `PS-93`, `PS-89`, `PS-98` |
| Checker | Outside the operator, appointed by it | `PS-118` |
| Regulator | Reads the audit trail | `PS-88` |

A provider may also be a requester, opening ordinary orders on the platform (`PS-139`). One party may hold several roles: "We design for separate entities fulfilling different roles", with each "recorded separately" (`PS-102`). Integrators and referrers are not parties (`PS-36`). An integrator is the customer's software.

## 4. Requirements

Normative. Each has a statement, what it derives from, and how it is verified. `THR-n` in "Derives from" marks a requirement derived from a threat, not from a ruling (`REQ-CHN-6`).

### `REQ-MOD-*`: components

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-MOD-1` | The state machine, the provider system and the router MUST each be usable as a standalone library, without the core and without each other. | `PS-10` | Demonstration: a program using only that component passes that component's tests. |
| `REQ-MOD-2` | Each component MUST publish documentation covering its whole public contract. | `PS-10` | Inspection: every public element is documented. |
| `REQ-MOD-3` | The core MUST reach each component only through that component's public contract. | `REQ-MOD-1` | Inspection: no use of anything non-public. |
| `REQ-MOD-4` | A component MUST NOT depend on any particular provider, and every provider, including those the project implements, MUST reach the platform only through the catalogue's common interface. | `PS-14`, `PS-67` | Inspection, and a test with a stand-in provider. |
| `REQ-MOD-5` | The platform MUST be released under the MIT licence. The licence of each required dependency MUST be decided and recorded case by case. | `PS-64` | Inspection of the licence and of the dependency record. |

### `REQ-STM-*`: state machine

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-STM-1` | Every order and every segment MUST have exactly one current state, from a published list. | `PS-10`, `PS-21` | Property test: no sequence of inputs gives zero or two states. |
| `REQ-STM-2` | The list MUST be able to express at least the states in the table below. Their names don't bind. Any further state or the path between states is a design decision. | `PS-21` | Inspection against the table. |
| `REQ-STM-3` | A state MUST change only on a recorded input, and the same recorded inputs MUST always reach the same states. Anything from the outside world, route calculation included, MUST enter only as a recorded input. | `PS-83`, `PS-21`, `PS-146` | Property test: replaying recorded inputs reaches the same states. |
| `REQ-STM-4` | An input repeated or arriving late MUST NOT change a state again or move value again. | `THR-3` | Property test with repeated and reordered inputs. |
| `REQ-STM-5` | Calls to a provider MUST be asynchronous. Elapsed time MUST NOT count as a segment's outcome. A check at routing is `REQ-CAT-19`. | `PS-81` | Test: a provider that never answers leaves the segment unresolved. |
| `REQ-STM-6` | An order that is complete, failed, handed on or ended without completing MUST NOT change state, with one exception: a complete order becomes failed when its starting segment is proven undone. Later reports MUST still be recorded. | `PS-107`, `PS-121` | Test: late reports after each ending. |
| `REQ-STM-7` | Segments MUST act as transitions of a nondeterministic automaton over nodes and assets, including transitions that change no value. Which transition a route takes MUST enter as a recorded input (`REQ-STM-3`). | `PS-134`, `PS-83` | Inspection; test: alternative routes from one state, and a transition that changes no value. |
| `REQ-STM-8` | The state machine MAY be reused wherever its rules hold. It MUST NOT be weakened into a free-form engine to widen that reuse. | `PS-10`, `PS-134`, `PS-132` | Review; demonstration under `REQ-MOD-1`. |

| States that must be expressible | Source |
|---|---|
| Order: not yet past its first segment | `PS-107` |
| Order: ended without completing, including cancelled by its reserve | `PS-107`, `PS-137` |
| Order: first segment settled | `PS-107` |
| Order: waiting for the customer | `PS-109`, `THR-60` |
| Order: stuck | `PS-81`, `PS-97` |
| Order: complete | `PS-21` |
| Order: failed | `PS-107` |
| Order: handed on, its value passed to a linked order | `PS-167` |
| Segment: started, from when its provider is bound | `PS-108` |
| Segment: settled | `PS-107` |
| Segment: failed, with the reversal owed | `PS-92` |
| Segment: outcome unknown | `PS-73`, `THR-3` |
| Segment: reversal complete | `PS-92`, `THR-13` |

### `REQ-QTE-*`: router

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-QTE-1` | A quote MUST state its provider, segment, amount, direction, fee, validity and what voids it and, where its category sets a rate, that rate and its slippage regime. It MUST be attributable to the provider, who can't deny it. | `PS-16`, `PS-21`, `PS-76` | Test: a quote lacking any element is refused; a forged one is detected. |
| `REQ-QTE-2` | Any difference between the sum of quotes and the price shown to the customer MUST be explicit. The operator's own fee MUST be defined by a formula under `REQ-QTE-5`, and shown apart. | `PS-76`, `PS-124`, `PS-141` | Test: shown price against the sum of quotes. |
| `REQ-QTE-3` | A provider's declared settlement time MUST be recorded with the quote and be measurable against actual time. | `PS-76`, `PS-94`, `THR-21` | Test: declared and actual time are both readable per segment. |
| `REQ-QTE-4` | Each figure used in a route decision MUST have a known age and origin. A figure past its stated validity MUST NOT be used. | `PS-16`, `PS-74`, `PS-83` | Test with expired figures. |
| `REQ-QTE-5` | A provider's fee MUST be defined by a formula it advertises, with flat and percentage parts, depending only on amount and tuple. Where the charge is known only after execution, the formula MUST give a maximum, and the charge MUST stay within it. Anyone entitled to the inputs MUST be able to compute the fee before use and check a charged fee against it. | `PS-101`, `PS-111`, `PS-119` | Test: formula against charge, including a charge above the maximum being flagged. |
| `REQ-QTE-6` | A provider's formula and slippage MUST bind it from when its segment starts. The route total before that is a quote. | `PS-108` | Test: a formula changed after segment start doesn't apply to it. |
| `REQ-QTE-7` | A price shown before execution MUST be labelled a quote, not a promise. The final executed price MUST be reported as the price. | `PS-100` | Test: what is shown before and after. |
| `REQ-QTE-8` | A route's quoted price MUST be calculated as the sum of its segments' advertised prices. | `PS-76` | Test across several segments. |
| `REQ-QTE-9` | For a segment settling within three calendar days, fees MUST be committed from segment start, and the rate MUST stay within a slippage the operator sets per tuple, recorded and shown. A longer segment is free of those bounds. | `PS-76`, `PS-108`, `PS-143` | Test at the three-day boundary. |
| `REQ-QTE-10` | A route with a segment over three calendar days MUST have no firm rate, MUST name that segment, and MUST be accepted by the customer first. | `PS-94` | Test: such a route isn't started without acceptance. |
| `REQ-QTE-11` | The router MUST compute routes from what the catalogue advertises, within what the operator allows, and MUST optimise for the customer's chosen objective, defaulting to lowest total cost. | `PS-74`, `PS-75`, `THR-57` | Test: objective changes the choice; a restricted tuple is never chosen. |
| `REQ-QTE-12` | Route building MUST check each segment's amount against that provider's band, in its own asset, after the fees taken before it. | `THR-117` | Test: an amount that clears the first band and misses a later one isn't routed through it. |
| `REQ-QTE-13` | Before an order starts, routing MUST check that the main path or an alternative arrives with a positive balance. This is a check at routing and no promise afterwards. | `PS-77` | Test: a route with no positive outcome isn't offered. |
| `REQ-QTE-14` | A route MUST be able to connect any origin node and asset to any destination node and asset the catalogue can reach. It MUST NOT need local money at either end, or an exchange along the way. | `PS-1`, `PS-74` | Demonstration: `AC-26`, and routes between local money and on-chain balances each way. |
| `REQ-QTE-15` | A reserve MAY get a quote for a new route and hold it before the route starts. A held quote MUST have a stated validity, and MUST stay a quote until each segment starts. | `PS-137`, `THR-127`, `PS-149` | Test: hold and expiry; a held quote binds no provider; a failed withdrawal has no effect beyond that. |
| `REQ-QTE-16` | The router MUST be able to calculate a split as well as a single route, within the same catalogue, policy and objective, and the decision record (`REQ-AUD-4`) MUST cover it. | `PS-157`, `THR-135` | Test: an order the router splits; recompute the choice from its record. |

### `REQ-CAT-*`: provider catalogue

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-CAT-1` | The operator MUST be able to bound and restrict what a provider advertises, tuple by tuple. | `PS-70`, `THR-57` | Test: a restricted tuple isn't usable. |
| `REQ-CAT-2` | Entry MUST be by the operator's explicit grant. A grant MUST be attributable and scoped to a named legal party, with categories and tuples, and MUST have a stated expiry. Losing standing MUST stop the provider being used in new routes. A party's earlier grants MUST stay visible at re-entry. | `PS-6`, `PS-22`, `THR-9` | Test: after expiry or revocation, new routes don't use the provider; inspection of re-entry. |
| `REQ-CAT-3` | A balance check MUST answer only yes or no, for a stated amount, and MUST happen only inside the session of the account holder or their delegate. How often MUST be bounded and logged, per account. Checks for the holder's own order MUST NOT count against the bound. Thresholds are free. A reserve that can't know yet MUST wait until it knows, and the route waits too, up to `REQ-CAT-19`. | `PS-15`, `PS-31`, `PS-87`, `PS-123`, `PS-125`, `PS-142`, `THR-39`, `THR-54` | Test: answers carry nothing else; a check outside a session is refused; the bound holds. |
| `REQ-CAT-4` | Anything that reveals whether funds suffice MUST count as a balance check, under whatever name, except for the holder's own order (`REQ-CAT-3`). | `PS-31`, `PS-87`, `THR-40` | Test: dry-runs and refusals against the bound. |
| `REQ-CAT-5` | Order information MUST reach only shortlisted providers, and only what the step needs. A provider MUST NOT be able to see a rival's quote, or that a rival was asked. | `PS-16`, `PS-95`, `THR-47` | Test: what each provider receives. |
| `REQ-CAT-6` | A reserve MUST declare, at publishing, whether it can require a user-initiated withdrawal, and whether it starts routes on automatic deposits. Each declaration MUST be recorded once, attributable to its maker, and shown to the user before they accept a route through that reserve. A later version that changes one MUST be a new declaration, shown again. | `PS-98`, `PS-136` | Test: the declaration is shown before acceptance. |
| `REQ-CAT-7` | A grant MUST record that the provider has shown it can pay for its reversals. Going silent MUST NOT discharge a reversal it owes. | `PS-105` | Inspection of the grant; test: silence leaves the reversal owed. |
| `REQ-CAT-8` | There MUST be one common interface defining the catalogue, with four categories: reserve, exchange, ramp and verifier. A category attaches to what is advertised, so one provider MAY advertise under several. | `PS-14`, `PS-95`, `PS-130`, `PS-131` | Inspection; a stand-in of each category passes the interface's tests. |
| `REQ-CAT-9` | A reserve MUST handle withdrawals and deposits, and nothing else that moves value, and MUST answer balance verifications for what can be withdrawn from it. An exchange MUST handle conversions and expose detailed quotes with rates and fees. A ramp MUST handle transfers and expose fees. A verifier MUST expose judgements. | `PS-15`, `PS-16`, `PS-17`, `PS-95`, `PS-129` | Test per category. |
| `REQ-CAT-10` | A provider MUST declare how long evidence of an outcome stays available, and whether a repeat is safe. Route choice and retries MUST follow what it declares, and a retry MUST NOT be sent where the provider doesn't declare it safe. | `PS-115`, `THR-106`, `THR-112` | Test: a provider declaring no safe retry never receives one. |
| `REQ-CAT-11` | A provider MUST declare, per tuple, its amount bands, any fill increments, its units, and whether its reports are gross or net of its charge. | `THR-107`, `THR-108`, `THR-117` | Inspection; test: a report in undeclared form is flagged. |
| `REQ-CAT-12` | A provider MUST declare the inputs a segment needs. An order MUST wait for the customer before entering a segment whose inputs it lacks, and those inputs MUST NOT go to providers not chosen. | `THR-116` | Test: a recalculated route through such a provider waits. |
| `REQ-CAT-13` | A provider MUST advertise every tuple it serves, as origin, destination and asset pair. An exchange states a from asset and a to asset, and its origin and destination MUST be one node. A ramp keeps the same asset, and its origin and destination MUST be different nodes, and a reserve states which assets it accepts for deposits or redeems. A verifier advertises classes, not tuples. | `PS-70`, `PS-95`, `PS-132` | Inspection; test: an unadvertised tuple isn't routed. |
| `REQ-CAT-14` | A provider MUST be able to dry-run a segment, and MUST reject a segment it can't serve. A dry-run MUST NOT change anything at the provider: registering a destination is the integrator's step before quoting, documented in `INTEGRATORS.md`. | `PS-71`, `PS-162` | Test: a rejected dry-run removes that option. |
| `REQ-CAT-15` | An exchange or ramp that can't truly undo a segment MUST decline it. Only a true undo counts as a reversal. A reserve doesn't decline for lacking a true undo: it is expected to be infallible (`ASM-4`), and a failed final deposit follows `REQ-ORD-43`. | `PS-72`, `PS-91`, `PS-165` | Test: an advertised reversal is exercised; inspection of declines. |
| `REQ-CAT-16` | To join the catalogue, a provider MUST offer an identifier or a status query, with no exceptions. | `PS-82` | Inspection at entry. |
| `REQ-CAT-17` | A reserve MUST connect an inner node with an outer node, in either direction. An outer node MUST be named for its network and its reserve, so that it records the origin or destination of funds. rutent MUST NOT depend on what is behind a reserve: a book, a bank account, a third-party provider or a wallet. A holding account standing as origin reserve for a route handed on (`REQ-ORD-16`) is the one reserve at an inner node. | `PS-91`, `PS-116`, `PS-126`, `PS-129` | Demonstration: stand-in reserves of each kind pass the same tests; test: a recorded origin and destination name their outer nodes. |
| `REQ-CAT-18` | A provider MUST apply limits by amount itself. Its refusal MUST state whether it lifts at a known time, waits on the person, or is final. | `PS-122`, `THR-120` | Test per kind of refusal. |
| `REQ-CAT-19` | A reserve that doesn't answer a check within a stated time MUST be marked ineligible for temporary unavailability, and an alert MUST be raised. | `PS-125` | Test: a silent reserve is marked and the alert fires (`NFR-8`). |
| `REQ-CAT-20` | A publisher MAY advertise several providers, and nodes, private networks included. The record MUST show the publisher behind each provider and node. | `PS-130`, `THR-124` | Test: one publisher with several providers and a private node. |
| `REQ-CAT-21` | Each role MUST have a contract, and a module MUST comply with the contract of the role it advertises: reserves are endpoints, exchanges run inside one network, and ramps route between networks. A module MAY take internal steps to do so. | `PS-132`, `PS-131` | Test: a stand-in that breaks its role's contract is refused or flagged; one with internal steps passes. |
| `REQ-CAT-22` | A provider MUST declare and publish its private nodes in the catalogue. The operator MUST publish a shared list of public nodes, so the two can be told apart. | `PS-148` | Inspection; test: every node on a route is on one list or the other. |
| `REQ-CAT-23` | Every node where value rests between segments MUST have a holding account. For a public node the operator MUST register it, by default a plain wallet, as a module outside the core. For a private node, its publisher MUST give the operator a way to interact with its holding account, such as an interface to its record of funds. | `PS-168`, `PS-148` | Inspection; test: a node without a holding account can't be on a route. |

### `REQ-MDL-*`: provider modules

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-MDL-1` | A module, in both its sandboxed and its outside part, MUST be admitted only after its integrity and the provider's signed assertion are checked. Acceptance MUST be recorded and MUST bind what was approved to what runs. A publisher MUST NOT be able to replace another's module. | `PS-20`, `PS-83`, `PS-84`, `PS-113` | Test: altered, unsigned and wrongly signed modules are refused. |
| `REQ-MDL-2` | What ran MUST be traceable to the admitted binary and to the platform version that made its machine code, and MUST be re-derivable after a flaw is found. | `PS-96`, `THR-18` | Test: re-derive and compare. |
| `REQ-MDL-3` | A module MUST declare the capabilities it needs. A release that widens them MUST be visibly different and MUST take a fresh acceptance, even where acceptance is otherwise automated. | `PS-84`, `THR-4` | Test: a widened release isn't accepted automatically. |
| `REQ-MDL-4` | An upgrade MUST be able to be staged, observed and rolled back, and a failed one MUST leave the previous version in force. Going back to an older version MUST be a recorded decision, never an effect of publishing. | `PS-18`, `PS-21`, `THR-17` | Test: failed upgrade; publishing an older signed version changes nothing. |
| `REQ-MDL-5` | An in-flight segment MUST move only to a version accepted under `REQ-MDL-1`, recorded per segment. Marking a version stale MUST be the operator's permission and not the publisher's. | `PS-86`, `THR-16` | Test: a publisher can't mark its own version stale. |
| `REQ-MDL-6` | The sandboxed part MUST NOT see secret plaintext. A provider's secrets MUST be unsealable only by its own outside part. Their use MUST be attributable per transaction, and rotating one MUST take no new release. | `PS-85`, `THR-78` | Test: attempts to read from the sandboxed part and from another provider. |
| `REQ-MDL-7` | The outside part MUST reach only destinations it has declared. Everything else MUST be refused. | `PS-83`, `THR-79` | Test with redirects and altered resolution. |
| `REQ-MDL-8` | The outside part MUST perform only operations the provider's interface declares, for an identified transaction. Every outbound call MUST be recorded with destination, version and transaction. | `PS-21`, `PS-83` | Test: an undeclared operation is refused; the record is complete. |
| `REQ-MDL-9` | The reach of a compromised module, in either part, MUST be bounded and stated. It MUST NOT be able to read or use another provider's state or secrets. | `PS-20`, `PS-85` | Test with a hostile stand-in module. |
| `REQ-MDL-10` | Per-transaction state MUST be unreadable by another transaction. Each change to shared state MUST be attributable to a transaction and a version. | `PS-19`, `PS-21` | Test across transactions. |
| `REQ-MDL-11` | CPU, memory, outbound calls, state and pending work MUST be limited per module and per provider, and a limit being hit MUST leave the transaction in a known state. | `PS-20`, `PS-81` | Test at each limit. |
| `REQ-MDL-12` | A module's output MUST be checked against its operation's declared shape. Everything downstream MUST treat provider output as untrusted. | `PS-20`, `PS-31` | Test with malformed and oversized output. |
| `REQ-MDL-13` | Adding, upgrading or removing one provider's module MUST NOT reload the service or interrupt other providers. | `PS-18` | Test: orders at other providers continue through a change (`NFR-2`). |
| `REQ-MDL-14` | A module MAY keep shared state and per-transaction state. A transaction MUST be uniquely identified. | `PS-19` | Test: both kinds of state; duplicate identifiers refused. |
| `REQ-MDL-15` | The platform MUST accept one or more common portable binary formats, verifiable, from any language that compiles to them, with a signed assertion. The platform MUST make the machine code. | `PS-20`, `PS-84`, `PS-96` | Demonstration: modules from at least two languages. |
| `REQ-MDL-16` | A module's interaction with the outside world MUST be split from the sandboxed part. The outside part is the provider's code, under the module's admission rules. | `PS-83`, `PS-113` | Inspection; test: the sandboxed part has no outside reach. |
| `REQ-MDL-17` | The platform MUST provide a common interface to store and unseal secrets. | `PS-85` | Test through the interface. |
| `REQ-MDL-18` | The sandboxed part MUST run under stated safety restrictions. | `PS-20` | Test each restriction. |
| `REQ-MDL-19` | The provider interface MUST stay fixed across module versions. A segment whose version is marked stale MUST run on the newest version available. | `PS-86` | Test: stale version during a segment. |
| `REQ-MDL-20` | A segment in flight on a revoked version MUST finish on it, unless that version is declared actively exploited. | `PS-86` | Test both cases. |
| `REQ-MDL-21` | A module MAY call another provider's module only where the route declares it, and each module MUST act on its own credentials. | `PS-85`, `PS-139`, `THR-86` | Test: an undeclared call is refused; a declared one uses the callee's own credentials and is recorded. |

### `REQ-ORD-*`: requesting and fulfilling an order

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-ORD-1` | A segment MUST have one final outcome. "Outcome unknown" MUST be distinct from success and failure, and MUST be left only on stated evidence. A result MUST count only if it is attributable to that segment's provider and matches that transaction. | `PS-19`, `PS-73`, `PS-81`, `PS-82` | Property test with forged, mismatched and repeated results. |
| `REQ-ORD-2` | A route MUST be recalculated only from a node where the value is evidenced to be. A silent segment, a stopped advertisement or a stale version MUST NOT release it. When a provider stops advertising, the resolution MUST be a route recalculation once the value is evidenced. | `PS-112`, `PS-86` | Simulation test: silent provider, then late delivery; value never moves twice. |
| `REQ-ORD-3` | A reversal MUST count as complete only on a statement from a party other than the provider that failed and its publisher, showing the value back at the segment's origin node, in full and in the asset it left. Where the failed provider is the reserve at that node, its own statement counts, labelled as its word. Inside a private node, its publisher's word counts, labelled as such. | `PS-72`, `PS-92`, `THR-13`, `PS-137`, `PS-152` | Test: a provider's own statement, or a short return, isn't enough; inside a private node the record labels it as the publisher's word. |
| `REQ-ORD-4` | Recalculation for one order MUST be bounded in count and MUST stay above any floor the customer set. Passing the same provider again without progress MUST be visible. Past the bound, the order counts as having no route left (`REQ-ORD-28`). | `PS-109`, `THR-28` | Test past the bound, and with repeated providers. |
| `REQ-ORD-5` | An order waiting between segments MUST have a recorded state and an age, visible to its customer and the operator. | `PS-73`, `THR-60` | Test: state and age are readable by both. |
| `REQ-ORD-6` | The figures behind a stuck call MUST be recorded when used. A material change in a provider's p99 MUST be raised to the operator. An order's stuck state MUST be recorded and visible to its customer and its provider. | `PS-21`, `PS-81`, `PS-93`, `THR-24` | Test: figures on record; visibility to each party. |
| `REQ-ORD-7` | Loss of the path that warns the operator MUST itself be noticed. | `PS-73`, `THR-59` | Test: cut the path and observe. |
| `REQ-ORD-8` | The operator MUST be able to set a ceiling on value in flight at one provider, and across one publisher's providers, and routes MUST respect it. A split order's parts at one provider or publisher MUST count together. Exposure on stuck orders MUST be readable at any time. | `PS-93`, `THR-74`, `THR-124` | Test: a route over either ceiling isn't used. |
| `REQ-ORD-9` | What one provider reports sending MUST be compared with what the next party reports receiving, and a difference MUST raise the order to the operator. This only detects. It MUST NOT complete an order or stand in for the primary path. | `PS-33`, `THR-14` | Test: a difference raises and changes nothing else. |
| `REQ-ORD-10` | What the customer was shown and when they accepted MUST be recorded, an agent's acceptance as an agent's. | `PS-75`, `PS-94` | Test: the record per acceptance. |
| `REQ-ORD-11` | The provider of a failed segment MUST carry the reversal unless it shows the fault was the customer's, or the operator's. A dispute MUST make the order stuck. | `PS-106`, `THR-118` | Test each attribution. |
| `REQ-ORD-12` | An order MAY end without completing until its first segment settles. After that it MUST NOT, except when its starting segment is proven undone, and then the whole order MUST be marked failed, or when its value is handed on to a linked order (`REQ-ORD-43`). | `PS-107` | Test before and after the first segment settles. |
| `REQ-ORD-13` | Every order MUST be initiated by a user. A reserve that has declared it (`REQ-CAT-6`) MAY start a route on an automatic deposit, authorising the withdrawal with no user present, and it or the operator fills out the details. An order the operator hands on under `REQ-ORD-43` MUST keep the original customer as requester, and record the operator as acting under `REQ-AUZ-4`. | `PS-114`, `PS-136`, `PS-167` | Test: a route started this way without the declaration is refused; an order with no user behind it is refused. |
| `REQ-ORD-14` | Value arriving with no order MUST NOT be treated as an order. It MUST NOT count against the operator as a stuck order. | `PS-114`, `PS-34` | Test: an unattributed arrival creates no order. |
| `REQ-ORD-15` | On a route with an automatic deposit, the deposit MUST count as acceptance, and MUST be recorded as such. | `PS-110` | Test: the record distinguishes this from an explicit acceptance. |
| `REQ-ORD-16` | Every route MUST start by authorising a withdrawal from a reserve, from an outer node to an inner node, and MUST end by depositing in a reserve, from an inner node to an outer node. A route without both MUST be refused. It MAY start and end at the same reserve, even the same account. Settling the first segment is that provider's responsibility. Such a route still needs its public records (`REQ-ORD-33`). A transfer inside a book has no such record, so it can't close, and it is outside rutent. A route handed on under `REQ-ORD-43` starts instead from the holding account where the value sits, which counts as its origin reserve. | `PS-116`, `PS-127`, `PS-129`, `PS-150`, `PS-167` | Test: a route starting or ending otherwise is refused; a two-segment route within one reserve runs and closes on its public records; a request with no public record doesn't close. |
| `REQ-ORD-17` | An order MUST record requester, beneficiary and payer separately. Which of them a provider checks MUST follow that provider's rules. The beneficiary MAY be a settlement account whose holder pays several payees from its book: the payees' checks then fall to that reserve, and the record MUST say so. | `PS-120`, `PS-164` | Test with three different parties. |
| `REQ-ORD-18` | When an order can't proceed, the caller MUST get a stable class of cause saying what it asks of them. "No route for this amount" and "no route right now" MUST differ. A provider's own failure text MUST NOT reach the customer. | `THR-114`, `THR-120` | Test: each cause; provider text with markup and third-party names. |
| `REQ-ORD-19` | Repeating an order request under the same identifier MUST NOT create a second order. A caller MUST be able to tell "refused, nothing exists" from "exists, outcome to come". | `THR-105` | Property test with repeated requests. |
| `REQ-ORD-20` | Every change of an order's state, failure included, MUST be notified to the originating user and the operator through a system-defined mechanism. A notice MUST be authenticated for its receiver, MUST be safe to receive repeated or out of order, and its receiver MUST be able to ask for the current state. | `PS-107`, `PS-163`, `THR-103` | Test: forged, repeated, reordered and lost notices. |
| `REQ-ORD-21` | The amount given to the next segment MUST come from what is evidenced at the node, not from the quote. A shortfall arriving there MUST NOT count as the next provider's failure. | `THR-109` | Test: less arrives than was priced. |
| `REQ-ORD-22` | Recalculation MUST be automatic, at the user's cost. The failed provider MUST cover only returning the value to the segment's origin node. | `PS-92`, `PS-99` | Test: who pays at each step. |
| `REQ-ORD-23` | An arrival that differs from the amount an open order states MUST NOT settle its first segment by itself. The reserve's report of it MUST be recorded, and the operator or an authorised reserve MAY file it anew. Once no order is open, it is `REQ-ORD-14`. | `PS-114`, `THR-122` | Test: short, excess and split arrivals against an open order. |
| `REQ-ORD-24` | A destination MUST be resolved when a route is accepted, and confirmed before the last segment where a provider can. "Not yet" MUST differ from "no". A change of owner MUST make the order wait for the customer. | `THR-111` | Test: an unresolved, a late and a changed destination. |
| `REQ-ORD-25` | An origin approval arriving after its order ended MUST start nothing, MUST be recorded, and MUST be visible to the customer. | `THR-119` | Test: late approval after the customer paid again. |
| `REQ-ORD-26` | A place a provider sends a person or agent to approve MUST be labelled as that provider's statement, and MUST pass `REQ-MDL-12`. | `THR-104` | Test with an altered response. |
| `REQ-ORD-27` | A customer MAY set a worst acceptable outcome. It MUST be checked at each settled segment, against the value evidenced at the node and the quotes still ahead. Past it, the order MUST wait for the customer. | `PS-109`, `PS-160` | Test at the floor. |
| `REQ-ORD-28` | With no route left, an order MUST become stuck. The operator resolves and bears it, and its processing MUST be recorded (`REQ-AUZ-4`). In normal conditions an order MUST NOT be rolled back: a failed segment is met by recalculation. | `PS-97`, `PS-93`, `PS-166` | Simulation test: no alternatives remain. |
| `REQ-ORD-29` | An order MUST become stuck when funds take longer than twice the p99 to traverse a segment at that provider, or twice its advertised time until it has history, and the operator MUST be warned. The stuck state MUST be recorded. The figure MAY be calculated outside the core. | `PS-73`, `PS-81`, `PS-144` | Simulation test at the boundary. |
| `REQ-ORD-30` | Evidence about an undone starting segment MUST be recorded as it arrives, even in pieces. After the route has started, the origin reserve carries that loss, and the customer keeping what was delivered doesn't change that. | `PS-116`, `THR-31`, `PS-137` | Test: partial evidence is kept. |
| `REQ-ORD-31` | A destination reporting complete and later returned MUST be recorded and notified under `REQ-ORD-20`. The order stays complete. | `PS-121`, `PS-35` | Test: return after complete. |
| `REQ-ORD-32` | An order MUST be routed only on the reserve's yes, asked in the user's session, except on an automatic deposit, where no user is present. | `PS-77`, `PS-142`, `PS-136` | Test: a route without that answer isn't offered; an answer carried by the user is refused. |
| `REQ-ORD-33` | An order MUST close at its final deposit on a public record of the movement, plus the reserve's signed statement for its own book, tying to that segment. A reserve that can't give them MUST NOT end a route. For a segment within a private node, the public movements into and out of that node are enough. The record MUST say whose evidence it is. Each part of a split order closes this way, and the order closes under `REQ-ORD-42`. A rail with no public record of its own MUST be declared as a private node, and a final deposit there closes on the public movement into that node, plus the reserve's signed statement. | `PS-90`, `PS-138`, `THR-36`, `THR-107`, `PS-148`, `PS-150`, `PS-151` | Test: a statement alone doesn't close an order; evidence for another segment doesn't either; a reserve declaring no such evidence isn't routed to. |
| `REQ-ORD-34` | A customer MUST be shown the quote and accept or decline it before an order starts, except on a route with an automatic deposit. | `PS-100`, `PS-110` | Test: an order isn't started without acceptance. |
| `REQ-ORD-35` | A deposit's destination MAY be an account or address the requester provides, and it is part of the destination that `REQ-AUZ-3` protects. The reserve MUST reject in its dry-run a destination it can't deposit to. | `PS-128`, `PS-129`, `THR-123` | Test: deposit to a provided address; one on the wrong network is rejected; recalculation keeps it. |
| `REQ-ORD-36` | Every change of asset MUST be its own segment at an exchange, including one that changes the asset and not the value. | `PS-130` | Test: a route with a one-to-one conversion shows that segment. |
| `REQ-ORD-37` | A route MUST be an ordered list of typed operations: withdrawal, route, exchange and deposit. The route MUST set the order. A module's internal steps MUST NOT add, remove or reorder a route's segments. One order MAY split across several routes (`REQ-ORD-42`), and each is its own ordered list. | `PS-132`, `PS-127`, `PS-131` | Test: a route is refused unless typed and ordered; a module can't change it. |
| `REQ-ORD-38` | A segment MUST belong to one order. A provider MAY share one movement among several orders inside its own module, and MAY open orders on the platform as their requester, which are ordinary orders. rutent MUST record each order's outcome on a shared movement as that provider's word unless its evidence is graded higher. | `PS-133`, `PS-139`, `THR-125`, `THR-128`, `PS-147` | Test: a stand-in sharing one movement across orders; each record names the provider's word. |
| `REQ-ORD-39` | Every segment's outcome MUST carry the best evidence its provider can give, and that evidence MUST be graded, declared and recorded. The operator MUST set a floor per tuple, and a provider below it MUST NOT be routed through. A statement alone MUST be recorded as a statement, not as proof. | `PS-138`, `PS-115`, `THR-126`, `PS-147` | Test: an outcome records its evidence and grade; a provider below the floor isn't routed. |
| `REQ-ORD-40` | The first segment is the withdrawal. A route MUST start when the reserve proves that withdrawal completed, and that proof settles the first segment. Before that, no value has moved: if the withdrawal fails, the reserve cancels it, the order is cancelled and notified under `REQ-ORD-20`, with no effect on further providers beyond a failed commitment. What happens before a reserve withdraws, and what the reserve then bears, are outside rutent. For a route handed on under `REQ-ORD-43`, the holding account's own record proves the withdrawal; at a private node, that's its publisher's word, labelled as such. | `PS-137`, `PS-167`, `PS-168` | Test: a failed withdrawal cancels the order and calls no later provider. |
| `REQ-ORD-41` | A provider on a private node SHOULD record each segment inside it: the asset and amount in and out of the segment, and the fee. Where the value then leaves onto a public node, rutent MUST replay those recorded segments, compare the result with the public movement out and with the stated outcomes, and record any mismatch. A match MUST be recorded as consistency with the public exit, not as proof the segments happened. A route ending inside the node has no such check, and stays its publisher's word. | `PS-153`, `THR-131` | Test: replay over a matching and a mismatching record; a route ending inside the node records no such check. |
| `REQ-ORD-42` | An order MAY split across several routes at once. The customer MUST accept a split as one quote, showing each part and the total, and the parts' amounts MUST add up to the order's, with one destination and to asset. Each part is a route: every rule about a route's failure, cancellation, recalculation, return or stuck state MUST apply to that part, and it MUST end by itself, without changing any other part. The order MUST close only when every part has ended, and its record MUST tie each part to the order and show each outcome. | `PS-157`, `THR-134`, `THR-136` | Simulation test: one part fails, is cancelled or sticks while others complete; amounts add up, and the order closes only when all have ended. |
| `REQ-ORD-43` | A failed final deposit MUST NOT roll the order back. It is met first by recalculation to the same destination (`REQ-ORD-22`). With no such route left, the order MUST wait for the customer, who MAY choose another route from the node where the value sits, to another destination; or the value MAY be handed on to a new order that starts from that node's holding account as its origin reserve. The operator MAY process a stuck order the same way, under `REQ-AUZ-4`. Another destination, or a new order, MUST be accepted by the customer (`REQ-ORD-34`, `REQ-AUZ-3`). Both orders' records MUST point to each other. | `PS-167`, `PS-166`, `PS-168`, `PS-89` | Simulation test: a destination rail refuses the deposit; each path, with both records linked. |

### `REQ-AUD-*`: provability and audit

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-AUD-1` | Who, what, when and outcome MUST be recorded for every action named in `PS-21`, for every manual action, and for refusals and requests that went unanswered. The source of "when" MUST be stated. What is published about the trail is `REQ-AUD-14`. | `PS-21` | Test: each action leaves its record; fault injection (`NFR-5`). |
| `REQ-AUD-2` | Records MUST keep what a party stated apart from what rutent verified. A statement MUST stay attributed to its maker, and a restatement by anyone else MUST be detectable. A relay's statement about its customer is that relay's statement. | `PS-21`, `THR-102` | Test: a restated quote or result is detected. |
| `REQ-AUD-3` | Records made before a compromise MUST NOT be alterable or removable without the operator's own review noticing. This claims nothing against someone with full operator control. | `PS-21`, `THR-20` | Test: alter and remove records, then review. |
| `REQ-AUD-4` | A route's decision record MUST hold the request, every candidate and why each was dropped, each input with its age and origin, the objective, and policy and module versions, enough to recompute the choice without calling a provider. | `PS-21`, `PS-75`, `PS-83`, `THR-10` | Test: recompute from the record alone. |
| `REQ-AUD-5` | Each party MUST be able to obtain and keep evidence of what it stated and what it was told about its own part of an order. | `PS-21`, `PS-93`, `THR-32` | Test: a party's evidence against a changed record. |
| `REQ-AUD-6` | Records and logs MUST hold no secret plaintext and no balance. Identity MUST appear only as a reference. | `PS-30`, `PS-31`, `PS-85` | Test: scan the records from a full simulated run. |
| `REQ-AUD-7` | Per provider and per verifier, these MUST be measurable in bulk: quotes against executed prices, dry-run against outcome, failures against market direction, refusals, reversals, and rates of no. | `PS-21`, `PS-71`, `PS-95`, `THR-27` | Test: each measure from simulated runs. |
| `REQ-AUD-8` | Each role MUST be recorded separately, even where one party holds several. | `PS-102` | Test: one party with several roles. |
| `REQ-AUD-9` | History MUST NOT be rewritable without a checker outside the operator seeing it. This claims nothing about what was never recorded, or about whether an entry is true. Retention and erasure are `REQ-AUD-13`. | `PS-104` | Test: rewrite history with full operator access; the checker detects it. |
| `REQ-AUD-10` | The operator MUST appoint a checker, who MUST be able to do its check. The appointment MUST be recorded, and the checker MUST be named to customers and providers. | `PS-118`, `PS-140` | Demonstration: an outside party runs the check; test: a customer can read who it is. |
| `REQ-AUD-11` | Each settled segment MUST have a record of the fee captured, including one billed separately, so a provider knows each fee it captured. rutent MUST attribute a fee only to a provider or the operator, and MUST keep no balances of them. A provider moves its fees out through ordinary orders, and a formula MAY charge zero. The operator's fee MUST be kept back by the final deposit's provider, and the operator moves it out the same way. A fee captured on a segment that settled MUST stay paid when the order later takes another route; a reversed segment keeps none (`REQ-ORD-3`). | `PS-124`, `PS-36`, `PS-141`, `PS-158`, `PS-159` | Test: fee records per segment; no running balance exists. |
| `REQ-AUD-12` | A regulator holding that permission MUST be able to read the audit trail. | `PS-88` | Test: a regulator's read, recorded. |
| `REQ-AUD-13` | Erasure MUST NOT remove or change the log. It MUST reach only what a reference names, held outside the log, and the reference MUST stay. How long anything is kept MUST be left to the operator, for its jurisdiction. | `PS-154`, `PS-65`, `THR-133` | Test: erase what a reference names; history still checks, and what it named is gone. |
| `REQ-AUD-14` | rutent MUST produce checkpoints of the audit trail on a schedule fixed in advance and independent of activity. Each MUST be a short commitment to the history so far: anyone holding that history MUST be able to check it against the checkpoint, and a checkpoint MUST reveal nothing else, neither orders, parties or amounts, nor how much activity there was. | `PS-155`, `THR-130`, `THR-132` | Test: runs with very different volumes, and a period with none, give checkpoints no observer can tell apart; altered history fails against a published one; a skipped one shows against the schedule. |
| `REQ-AUD-15` | rutent MUST be able to verify, for each holding account, that its balance equals the sum of the value held there for each order. A difference MUST raise to the operator, and only detects (as `REQ-ORD-9`). At a private node the balance is its publisher's word, labelled as such. | `PS-168`, `PS-33`, `THR-14` | Test: a matching account; one short and one over, each raised; a private node's balance labelled. |

### `REQ-CPL-*`: compliance proofs

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-CPL-1` | A judgement MUST state its maker, subject reference, class, time made and validity. "No", "not known" and "not asked" MUST be distinct. It MUST NOT be possible to fabricate one in its maker's name, and its maker MUST NOT be able to deny it. It MUST stay labelled second-hand, and MUST be usable only for its person and class, within its validity, by a presenter with that person's authority. | `PS-22`, `PS-95` | Test: forged, replayed, expired and misapplied judgements. |
| `REQ-CPL-2` | A party asking about a person MUST be identified and bounded, and MUST have authority from that person or from a shortlisted route. | `PS-78`, `PS-95`, `THR-45` | Test: a question with no authority is refused. |
| `REQ-CPL-3` | A judgement MUST be an input to rutent's policy decision, not the decision. Policy MUST be able to refuse a yes. | `PS-13` | Test: policy refuses on a yes. |
| `REQ-CPL-4` | A question to a verifier MUST carry a person and a class only. Classes MUST be declared in advance, and a class MUST NOT identify an amount, a route or a single order. | `PS-95` | Test: what a verifier receives; inspection of classes. |
| `REQ-CPL-5` | The set of providers and verifiers asked about one order, across recalculations, MUST be recorded and bounded. Past the bound, the order counts as having no route left (`REQ-ORD-28`). | `PS-95` | Test past the bound. |
| `REQ-CPL-6` | Every provider on the shortlisted route MUST validate eligibility when the route is constructed, for the parties its rules name (`REQ-ORD-17`). On a no, the route MUST be recalculated. A provider not shortlisted MUST NOT be asked. | `PS-78`, `PS-95`, `PS-120` | Test: who is asked, and what happens on a no. |
| `REQ-CPL-7` | A user or agent MAY present a judgement, and providers MAY vouch. | `PS-95` | Test each path. |
| `REQ-CPL-8` | A reserve MUST be able to prove whether the users it acts for may enact a set of transactions. Actors MUST be provable at compliance level. | `PS-22` | Test: a reserve's proof about a user can be obtained and attributed, and one in another's name is refused. |
| `REQ-CPL-9` | Where a reserve demands a person's presence, the reserve decides what an agent may do, and rutent MUST only report the outcome. | `PS-103` | Test: presence demanded of an agent. |
| `REQ-CPL-10` | Each provider MUST know a person under a different subject reference. Only the operator MUST be able to join them. A subject reference MUST be usable only by parties entitled to it. | `PS-30`, `PS-145`, `THR-48` | Test: two providers can't match a person's references; an unentitled party can't resolve one. |
| `REQ-CPL-11` | Personal details of a payer who isn't a customer MUST stay with the provider that received them, and rutent's records MUST hold a reference at most. | `THR-113` | Test: scan the records after a third-party payment. |
| `REQ-CPL-12` | The loss from a false judgement by a verifier or reserve MUST fall on the provider that relied on it. Records MUST show whose judgement it was and who relied on it, so that provider can recover under contract, which is outside this document. | `PS-156`, `THR-91` | Inspection: no path charges that loss to the operator or a customer; test: the record names the judgement's maker and the provider relying on it. |

### `REQ-AUZ-*`: authorisation and policy

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-AUZ-1` | External authentication MUST establish who is calling and nothing more. Every action that moves value, posts a route or changes the catalogue MUST pass rutent's own policy decision, and a path MUST NOT skip it. | `PS-13` | Test: every such action, with policy denying. |
| `REQ-AUZ-2` | A role other than the operator MUST NOT be able to post a route. A suggested route MUST have no standing until posted, and the record MUST name who suggested it. | `PS-74`, `PS-80` | Test: a provider posting a route is refused. |
| `REQ-AUZ-3` | An authorisation to move funds MUST name one order, its destination, its to asset and any floor the customer set. A recalculated route MUST act only within those. Anything else MUST need a new authorisation. | `PS-89`, `THR-11` | Property test: no recalculation changes destination or to asset. |
| `REQ-AUZ-4` | Resolving a stuck order MUST be an action under policy. It MUST name the evidence relied on, and a single actor MUST NOT both decide the outcome and move value. | `PS-21`, `PS-81`, `PS-93`, `THR-77` | Test: one actor attempts both. |
| `REQ-AUZ-5` | A decision missing an input from a verifier or the policy service MUST be a refusal. An unreachable verifier or policy service MUST give "not known", and value MUST NOT move on it. A reserve that can't answer is `REQ-CAT-3` and `REQ-CAT-19`. | `PS-78`, `THR-55` | Test: take each input away. |
| `REQ-AUZ-6` | Granting, accepting a module, revoking, marking stale, declaring a version exploited and setting a tuple's slippage MUST be separate permissions, each recorded with its actor. | `PS-6`, `PS-21`, `PS-84`, `PS-86`, `PS-143` | Test: each permission alone. |
| `REQ-AUZ-7` | A customer MUST read only its own orders, and a provider only its own segments. A regulator's or a checker's access MUST be an identified, recorded permission. | `PS-13`, `PS-88` | Test: reading across parties. |
| `REQ-AUZ-8` | Every agent MUST act for an identified accountable party, within a scope that party set. Limits on work and on asking about people MUST bind at that party. | `PS-117` | Test: many agents under one party hit one limit. |
| `REQ-AUZ-9` | A user MUST authorise at the origin, and providers carry that onward. At a reserve that declares it can't require the user's own withdrawal (`REQ-CAT-6`), the operator MAY act for the user, and a reserve that declares it MAY start a route on an automatic deposit (`REQ-ORD-13`). The record MUST say who acted. | `PS-89`, `PS-98`, `PS-136` | Test: the record of who acted. |

Number 10 in this family is withdrawn (R115), and isn't reused.

### `REQ-OPS-*`: deployment and operation

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-OPS-1` | The work one caller can cause MUST be bounded, and the requests it fans out to providers MUST count against it. | `PS-2`, `PS-11` | Test: a caller at its bound. |
| `REQ-OPS-2` | The operator MUST be able to see stuck orders by value and age, so volume can't hide a large one. | `PS-81`, `PS-93` | Test: one large order among many small. |
| `REQ-OPS-3` | Publishing and upgrades MUST be able to be halted during an incident, and the execution environment MUST be replaceable without losing orders in flight (`NFR-6`). | `PS-20`, `THR-84` | Simulation test: replace it mid-order. |
| `REQ-OPS-4` | rutent MUST be deployable and operable with no higher-order platform, and MUST stay decoupled from anything integrating with it. | `PS-12` | Demonstration: a stand-alone deployment. |
| `REQ-OPS-5` | Humans and agents MUST use one contract. Different interfaces MAY sit above it. | `PS-2` | Inspection; test with both kinds of caller. |

### `REQ-PRT-*`: protocol

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-PRT-1` | A party's statement MUST name the deployment and protocol version it belongs to, and MUST be void elsewhere. Safety MUST hold with the specification public. | `PS-6`, `PS-86` | Test: a statement replayed at another deployment. |
| `REQ-PRT-2` | Each protocol change MUST say whether a party may ignore it. A change that adds a protection MUST NOT be ignorable. The resolution of a segment MUST NOT depend on whether it was applied before or after the migration. | `PS-86`, `THR-64` | Test: a holdout at a cutover. |
| `REQ-PRT-3` | The protocol's specification MUST be published in v1. | `PS-6` | Inspection. |
| `REQ-PRT-4` | Protocol changes SHOULD be additive or backwards compatible. A protocol bump MUST force every provider at a stated cutover date. | `PS-86` | Inspection of the change record; test at a cutover. |
| `REQ-PRT-5` | A kind of operation beyond withdrawal, route, exchange and deposit MUST be added through a protocol change. | `PS-135`, `PS-86` | Inspection; test: an unknown kind of operation is refused. |

### `REQ-EXT-*`: staying open to what's next

| ID | Statement | Derives from | Verification |
|---|---|---|---|
| `REQ-EXT-1` | The protocol MUST NOT assume closed operation in a way that rules out open operation later. | `PS-23` | Review against `DEF-1`. |
| `REQ-EXT-2` | The router and catalogue MUST NOT rule out order books resolved by exchange providers. | `PS-23` | Review against `DEF-2`. |

### Requirements that go further than a ruling

Accepted by the lead under R108. Each row says what it adds. Any other requirement with a `THR-n` in "Derives from" spells out what a threat needs.

| Requirement | What it adds | Why |
|---|---|---|
| Adds to `REQ-AUZ-5` | A decision missing an input is a refusal | A verifier taken down must not pass eligibility (`THR-55`) |
| Adds to `REQ-MDL-1` | Admission covers the outside part too | R65 reads this way; the outside part holds real credentials (`THR-82`) |
| Adds to `REQ-AUD-4` | A record enough to recompute a route choice | `PS-21` asks who did what, when and the outcome (`THR-10`) |
| Adds to `REQ-AUD-5` | Parties hold their own evidence | The only check on the operator's records (`THR-32`, `THR-69`) |
| Adds to `REQ-AUD-7` | Bulk measures per provider and verifier | Some abuse shows only in bulk (`THR-27`) |
| Adds to `REQ-AUZ-4` | One actor can't both decide and move value | `THR-77` |
| Adds to `REQ-CAT-2` | Grants name a legal party, expire, and stay visible at re-entry | `THR-9` |
| Adds to `REQ-MDL-2` | Machine code can be traced and re-derived | `THR-18` |
| Adds to `REQ-MDL-4` | Upgrades are staged and can be rolled back | `THR-17` |
| Adds to `REQ-MDL-6` | A secret can rotate with no new release | `THR-78` |
| Adds to `REQ-ORD-6` | A change in a provider's p99 is raised | `THR-24` |
| Adds to `REQ-QTE-11` | The operator can restrict what routes may use | From my answer to your question under R10, not your words |
| Adds to `REQ-ORD-2` | A stale version or a stopped advertisement doesn't release value in flight | Narrows `PS-86` under `PS-112` (`THR-12`) |
| Adds to `REQ-ORD-3` | A provider, or its publisher, can't certify its own reversal | It would be marking its own work (`THR-13`, `THR-124`) |
| Adds to `REQ-ORD-4` | Recalculation is bounded in count, and past it the order is stuck | Colluding providers can take turns failing (`THR-28`). This puts that case on the operator |
| Adds to `REQ-ORD-11` | A provider can also show the fault was the operator's | R66 names only the customer; a cause on the operator's side shouldn't charge a provider (`THR-118`) |
| Adds to `REQ-ORD-18` | A stable class of cause for every stop | Customers otherwise repeat or abandon payments (`THR-120`) |
| Adds to `REQ-ORD-19` | Repeating an order request creates no second order | `THR-105` |
| Adds to `REQ-ORD-20` | Notices are authenticated and safe to repeat | R63 asks only that failures be notified (`THR-103`) |
| Adds to `REQ-ORD-21` | The next segment's amount comes from evidence | `THR-109` |
| Adds to `REQ-ORD-23` | A mismatched arrival settles nothing by itself | `THR-122` |
| Adds to `REQ-ORD-24` | A destination is confirmed before the last segment | `THR-111` |
| Adds to `REQ-ORD-25` | A late approval starts nothing | `THR-119` |
| Adds to `REQ-ORD-26` | An approval place is labelled as the provider's word | `THR-104` |
| Adds to `REQ-ORD-35` | The reserve's dry-run rejects a destination it can't deposit to | `THR-123` |
| Adds to `REQ-ORD-8` | A second ceiling across one publisher's providers | One publisher could otherwise spread past a ceiling (`THR-124`) |
| Adds to `REQ-CAT-9` | Balance verification stays at reserves you can withdraw from, though you said reserves only handle withdrawals and deposits | The brief gives reserves balance checks, and routing needs them (`PS-15`, `PS-77`) |
| Adds to `REQ-CAT-20` | The record shows the publisher behind each provider and node | `THR-124` |
| Adds to `REQ-CAT-3` | The bound is per account, and checks for the holder's own order don't count | `THR-39`, `THR-54`. The accepted row said one bound across callers, which R96 replaced |
| Adds to `REQ-CAT-6` | A version that changes a declaration declares again | R48 says recorded once, at publishing, and each version is published (added after R108) |
| Adds to `REQ-QTE-12` | Bands are checked after fees along the route | `THR-117` |
| Adds to `REQ-CAT-11` | Bands, increments, units and gross or net are declared | `THR-107`, `THR-108`, `THR-117` |
| Adds to `REQ-CAT-12` | Inputs are declared, and the order waits for what's missing | `THR-116` |
| Adds to `REQ-CPL-11` | A third-party payer's details stay at the provider | `THR-113` |
| Adds to `REQ-AUD-11` | The operator moves its fee out through ordinary orders, as a provider does | R129 names who keeps the fee back, not how it reaches the operator |
| Adds to `REQ-ORD-43` | The operator may process a stuck order by handing its value on | R140 is about a failed final deposit; `PS-166` leaves stuck orders to the operator |
| Adds to `REQ-AUD-15` | A difference only raises to the operator | R141 asks for the check, not what follows a difference |
| Adds to `REQ-ORD-13` | An order the operator hands on keeps the original customer as requester | `REQ-ORD-13` needs a user behind every order |
| Adds to `REQ-ORD-40` | A holding account's own record proves a handed-on withdrawal | R140 names the reserve, not its proof |
| Adds to `REQ-AUD-15` | A private node's balance is its publisher's word, labelled | As for other private-node statements (`PS-152`) |
| Adds to `INV-1` | Evidenced value at a node equals its holding account's balance | R141 asks for the means to verify it |
| Reading of `REQ-CAT-23` | "Every node" as every node where value rests between segments; "built-in outside-code blobs" as modules outside the core | Keeps `REQ-CAT-17` and `PS-37` |
| New order state | Handed on, its value passed to a linked order | So `REQ-ORD-12` and `REQ-STM-6` can end the first order |

## 5. Invariants

Normative. Each has at least one check (`oss-ledger-1`). "Report" means what a provider states, since rutent is not a ledger (`PS-33`); its one ledger-like check is on holding accounts (`REQ-AUD-15`). None of these can catch two providers agreeing false reports (`THR-15`).

| ID | Statement | Source | Check |
|---|---|---|---|
| `INV-1` | Conservation: for each order, value in MUST equal value delivered, plus declared fees, plus any reported remainder, plus value evidenced at a node, read across what providers report. Across all orders, value evidenced at a node MUST equal its holding account's balance, and a difference MUST be raised under `REQ-AUD-15`. | `PS-33`, `PS-92`, `PS-168`, `oss-ledger-1`, `THR-108` | Property test over simulated orders, with faults, reversals and recalculations. |
| `INV-2` | A failed segment MUST leave the value at its origin node, in full, at the provider's cost. | `PS-91`, `PS-92` | Simulation test: every failure path, with amounts compared. |
| `INV-3` | Atomicity is per segment: a segment MUST end settled, or reversed in full, or the order stuck. A failed withdrawal cancels the order before any of this. Independent providers MUST NOT be assumed to commit together. | `PS-91`, `PS-97`, `oss-ledger-1` | Simulation test: failure injected at every point, and no other end state appears. |
| `INV-4` | The same value of an order MUST never be moved from two places at once. A route MUST NOT restart until the value is evidenced at a node. | `PS-112`, `THR-76` | Property test: silence and late delivery never give two moves. |
| `INV-5` | rutent MUST hold no customer value, and so every route MUST start and end at a reserve, a holding account counting as one for a route handed on (`REQ-ORD-16`). Between segments, value rests in a node's holding account (`REQ-CAT-23`), never with rutent. | `PS-79`, `PS-127`, `PS-168` | Inspection of every operation; simulation test: rutent's own position stays empty. |
| `INV-6` | History MUST NOT change without a checker outside the operator seeing it. | `PS-104` | Test: rewriting history with full operator access is detected. |
| `INV-7` | An action MUST NOT take effect without its record. | `PS-21` | Simulation test: failure injected between recording and effect. |
| `INV-8` | A transaction's identifier MUST be unique, and repeating it MUST NOT move value twice. | `PS-19`, `THR-3` | Property test with repeats. |
| `INV-9` | A balance verification MUST reveal only whether funds suffice for a stated amount. | `PS-31` | Test: everything in an answer, across all reserves. |
| `INV-10` | Recalculation MUST NOT change an order's destination or to asset. Only the customer's own choice after a failed final deposit MAY (`REQ-ORD-43`). | `PS-89`, `THR-11` | Property test across recalculations. |

## 6. Acceptance criteria

Normative scenarios. Each names what it accepts. The worked examples are deployment-level, and these are the shapes the model must be able to express (`PS-41`).

| ID | Scenario | Accepts |
|---|---|---|
| `AC-1` | Given an order for one asset to another across three providers, when every segment settles, then the order is complete and the record shows who did what, when, and the final executed price. | `REQ-AUD-1`, `REQ-QTE-7`, `INV-1` |
| `AC-2` | Given a route across a reserve, an exchange and a ramp, when a customer changes the objective, then the chosen route may change, and the decision record explains the choice with no provider being called. | `REQ-QTE-11`, `REQ-AUD-4` |
| `AC-3` | Given a provider whose dry-run rejects an amount, when routing runs, then that option is dropped, and another provider serving the tuple is used if one exists. | `REQ-CAT-14`, `REQ-QTE-11` |
| `AC-4` | Given a segment that fails, when the provider truly undoes it at its own cost, then the value is shown back at the segment's origin by a party other than that provider and its publisher, or by a reserve at its own node as its word, in full and in the asset it left. The route is recalculated automatically at the user's cost, with destination and to asset unchanged. | `REQ-ORD-3`, `REQ-ORD-22`, `INV-2`, `INV-10` |
| `AC-5` | Given a provider that stops answering, when the time passes twice its p99, then the order is stuck, the operator is warned, the customer and provider can see it, and no new route starts. A late delivery moves nothing twice. | `REQ-ORD-29`, `REQ-ORD-6`, `REQ-ORD-2`, `INV-4` |
| `AC-6` | Given a failed segment with no route left, then the order is stuck, and the operator resolves it under policy, naming its evidence, with no single actor both deciding and moving value. | `REQ-ORD-28`, `REQ-AUZ-4` |
| `AC-7` | Given an order whose first segment hasn't settled, when it ends, then it ends without completing. After the first segment settles, no request can end it. | `REQ-ORD-12` |
| `AC-8` | Given a complete order, when its starting segment is proven undone, then the order is marked failed, and user and operator are notified. | `REQ-ORD-12`, `REQ-ORD-20`, `REQ-STM-6` |
| `AC-9` | Given a complete order, when the destination reports the payment returned, then it stays complete, and the return is recorded and notified. | `REQ-ORD-31` |
| `AC-10` | Given value arriving at a reserve with no order, then no order exists, and nothing counts against the operator. | `REQ-ORD-14` |
| `AC-11` | Given a route with an automatic deposit, when the operator or an authorised reserve files the order, then the deposit counts as acceptance, and the record says so. | `REQ-ORD-13`, `REQ-ORD-15` |
| `AC-12` | Given a customer floor, when recalculation would pass it, then the order waits for the customer, in a state visible to them with its age. | `REQ-ORD-27`, `REQ-ORD-5` |
| `AC-13` | Given a route with a segment over three days, then it has no firm rate, names that segment, and doesn't start until the customer accepts. | `REQ-QTE-10`, `REQ-ORD-10` |
| `AC-14` | Given a charge known only after execution, when a provider charges above its formula's maximum, then that is flagged to the operator. | `REQ-QTE-5` |
| `AC-15` | Given a balance check, then the answer is yes or no for the stated amount, and the check is logged and counted. A dry-run that reveals the same counts too. | `REQ-CAT-3`, `REQ-CAT-4`, `INV-9` |
| `AC-16` | Given a shortlisted route, then only its providers are asked, each verifier receives a person and a class only, and a no brings a recalculation. | `REQ-CPL-6`, `REQ-CPL-4` |
| `AC-17` | Given an unreachable verifier, then the decision is a refusal, and no value moves. | `REQ-AUZ-5` |
| `AC-18` | Given a thousand agents under one accountable party, then limits on work and on asking about people bind them together. | `REQ-AUZ-8`, `REQ-OPS-1` |
| `AC-19` | Given an upgrade of one provider's module, then orders at other providers aren't interrupted, and a failed upgrade leaves the previous version in force. | `REQ-MDL-13`, `REQ-MDL-4` |
| `AC-20` | Given a module from each of two languages, when admitted, then both run in the sandbox, and neither part can read another provider's state or secrets. | `REQ-MDL-15`, `REQ-MDL-9` |
| `AC-21` | Given a release that widens a module's capabilities, then it needs a fresh acceptance. | `REQ-MDL-3` |
| `AC-22` | Given full access at the operator, when someone rewrites past records, then a checker outside the operator detects it. | `REQ-AUD-9`, `INV-6` |
| `AC-23` | Given a repeated order request, then only one order exists, and the caller can tell what happened. | `REQ-ORD-19` |
| `AC-24` | Given an order where requester, beneficiary and payer differ, then each is recorded apart, and each provider checks those its rules name. | `REQ-ORD-17` |
| `AC-25` | Given a stand-alone deployment with an external authentication layer, when that layer says who is calling, then rutent's own policy still decides each action. | `REQ-OPS-4`, `REQ-AUZ-1` |
| `AC-26` | Given an on-chain origin whose reserve takes an automatic deposit from an on-chain transfer, two ramps across other networks, and a reserve at the destination, with no exchange and no local money, then the route can be expressed and run, and its origin and destination are recorded by their outer nodes. It closes on the public record plus the reserve's signed statement. | `REQ-QTE-14`, `REQ-CAT-17`, `REQ-ORD-15`, `REQ-ORD-16` |
| `AC-27` | Given an on-chain origin, one ramp, and a deposit into a reserve at an address the requester provided, then the route can be expressed and run, and the order's record shows the outer node and the address. Nothing in it depends on what kind of thing that reserve is. | `REQ-ORD-35`, `REQ-ORD-16`, `REQ-CAT-17` |
| `AC-28` | Given one reserve, when a route withdraws from one account and deposits into another, or into the same one, then that is a valid route of two segments and two nodes, closing under `REQ-ORD-33` on its public records. A transfer inside the reserve's book has no such record, so it can't close. | `REQ-ORD-16` |
| `AC-29` | Given one publisher advertising several providers and a private node, on a route that withdraws from a reserve, ramps through public and private nodes, converts assets within a node, including one conversion that changes no value, and deposits into a reserve, then each conversion is its own segment, and the record shows the publisher behind each provider and node. | `REQ-CAT-20`, `REQ-ORD-36`, `REQ-CAT-13`, `REQ-CAT-8` |
| `AC-30` | Given a module with internal steps, when it serves a segment, then it complies with its role's contract, and the route's segments are unchanged. An exchange spanning two networks and a ramp within one are refused. | `REQ-CAT-21`, `REQ-ORD-37`, `REQ-CAT-13` |
| `AC-31` | Given a provider sharing one movement among several orders, then each order keeps its own segment, and each record says the outcome is that provider's word, unless its evidence is graded higher. | `REQ-ORD-38` |
| `AC-32` | Given a final deposit, when the reserve gives a public record of the movement and its signed statement for its book, then the order closes, and the record says whose evidence each is. A statement alone doesn't close it. | `REQ-ORD-33`, `REQ-ORD-39` |
| `AC-33` | Given a route through a provider whose evidence grades below the operator's floor for that tuple, then that provider isn't routed through. | `REQ-ORD-39` |
| `AC-34` | Given a final deposit at a reserve on a rail with no public record, when that rail is declared as a private node, and there is a public movement into it and the reserve's signed statement, then the order closes, and the record says the step out is the reserve's word. An undeclared reserve with no public record can't end a route. | `REQ-ORD-33`, `REQ-CAT-22` |
| `AC-35` | Given value leaving a private node onto a public node, when its recorded inner segments are replayed, then a mismatch with the public movement out is recorded, and a match is recorded as consistency only. On a route ending inside the node, the record shows its publisher's word and no such check. | `REQ-ORD-41`, `REQ-ORD-3` |
| `AC-36` | Given an erasure request, then what the reference names is erased outside the log, the reference stays, and history still checks. | `REQ-AUD-13` |
| `AC-37` | Given periods with very different volumes, one of them idle, then checkpoints appear on the same schedule and can't be told apart by volume, and altered history fails against one. | `REQ-AUD-14` |
| `AC-38` | Given an order split across two routes, when one part fails and can't be recalculated while the other delivers, then each ends by itself, the amounts add up, the customer is notified, and the order closes with both outcomes on its record. | `REQ-ORD-42`, `REQ-QTE-16` |
| `AC-39` | Given a holding account shared by several orders, when its balance matches their sum, then nothing is raised; when it is short or over, then the operator is raised and nothing else changes. At a private node, the balance is labelled its publisher's word. | `REQ-AUD-15`, `REQ-CAT-23` |
| `AC-40` | Given a final deposit the destination rail refuses, then the order isn't rolled back, and recalculation to the same destination is tried first; with no such route, it waits for the customer; when the customer picks another destination, the route continues from where the value sits; when the value is handed on instead, a new order starts from that node's holding account, and the two records point to each other. Neither path proceeds without the customer's acceptance. | `REQ-ORD-43`, `INV-10`, `REQ-AUZ-3` |
| `AC-41` | Given an order moving through several states, then each change is notified to the originating user and the operator. | `REQ-ORD-20` |
| `AC-42` | Given a floor, when a segment settles and the value evidenced plus the quotes still ahead fall below it, then the order waits for the customer there, not only at recalculation. | `REQ-ORD-27` |
| `AC-43` | Given a dry-run, then nothing changes at the provider, and a destination the reserve can't deposit to is rejected. | `REQ-CAT-14`, `REQ-ORD-35` |

## 7. Non-functional requirements

Normative. A bound not set says so.

| ID | Statement | Measure | Bound | Source |
|---|---|---|---|---|
| `NFR-1` | rutent MUST serve the agreed load from humans and agents, with provider time excluded. | Sustained requests per second, with provider calls stubbed | Not set: stays an assumption (R103) | `PS-11`, `PS-50` |
| `NFR-2` | A change to one provider's module MUST NOT disturb others. | Orders at other providers failed or paused by the change | 0 | `PS-18` |
| `NFR-3` | One provider's outage or overload MUST NOT stop others. | Orders at other providers failed because of it | 0 | `PS-20`, `PS-81` |
| `NFR-4` | Replaying recorded inputs MUST reach the same states. | Share of replays reaching identical states | 100% | `PS-83`, `PS-21` |
| `NFR-5` | An action MUST NOT escape the record under failure. | Actions with effect and no record, under fault injection | 0 | `PS-21` |
| `NFR-6` | Replacing the execution environment MUST keep orders in flight. | Orders in flight lost; time to replace | 0 lost; time not set (R103) | `THR-84` |
| `NFR-7` | The public contract of each component MUST be documented. | Public elements with documentation | 100% | `PS-10` |
| `NFR-8` | A reserve that doesn't answer a check MUST be marked ineligible within a stated time. | Time from the check to the mark | Not set: "can't commit to numbers yet" (R80) | `PS-125` |

## 8. Threat model

Required by `REQ-CHN-7`: abuse cases, data classification, and threats by STRIDE category. Every threat has exactly one ending, and where a threat has a part that can be handled and a part that can't, each part has its own `THR-*`.

| Ending | Meaning |
|---|---|
| Mitigated (prevents) | If the requirement holds, the attack doesn't work. |
| Mitigated (bounds) | The attack still works, but its reach or size is limited. |
| Mitigated (detects only) | The attack succeeds. Someone can see it afterwards, and the row says who. |
| Accepted, Rn | The owner has ruled, with this risk named. |
| Open | Waits on the named form item. |

`[modules]`, `[routing]`, `[compliance]`, `[durable]` and `[verifier]` name reports in `research/`: the idea came from there, and that isn't proof. No legal claim here is stated as fact (`PS-65`).

### Abuse cases

Written from the attacker's side. Each names the threats it produces.

- **I am in the operator's systems.** At the reserves where the operator acts for its users, I am the operator. I withdraw for every user, to my own destination. The records are mine too, and the regulator trusts them. (`THR-66`, `THR-34`, `THR-67`)
- **I am a provider and the rate moved against me.** The dry-run passed, but honouring the rate now costs more than a reversal. So I fail, and the customer pays for the next route. If I can't afford even that, I stop answering: past twice my p99, it's the operator's problem. And I can run slow for a while first, to widen that p99. (`THR-27`, `THR-71`, `THR-24`)
- **I want to be paid twice.** My friend the provider delivers to me quietly and goes silent. The operator bears stuck orders, so it makes me whole. Later the provider "resolves", and we split it. (`THR-70`, `THR-76`)
- **I pay with something I can take back.** My origin payment settles over days. The route gives me final value well before that. Then I undo the origin. The origin reserve eats it, and it can refuse such payments under its own rules. (`THR-31`)
- **I own the only way out of this node.** My partner fails the segment. Value goes back to the segment's origin, where only I can move it on. Now I name the price. (`THR-62`, `THR-28`)
- **I can change a route after the user said yes.** The user authorised once, and providers carry that onward. If recalculation can reach a new destination, I spend their authority on a route they never saw. (`THR-11`, `THR-12`)
- **I want to know a balance.** I hold a delegate's session, and thresholds are free. Forty questions, spread over time. If that's limited, I ask through dry-runs and failed orders. (`THR-38`, `THR-39`, `THR-40`)
- **I want to know who is flagged.** I ask a verifier about people I have no order for. Or I force a no, and each recalculation tells more providers about the same person. (`THR-45`, `THR-44`)
- **I run a verifier or a vouching provider.** I say yes to win volume, and no to push a route off my rival. If the verifier goes down, I want the check to pass by default. (`THR-91`, `THR-56`, `THR-55`)
- **I got into the publish path.** The signature is valid: I used the real key or pipeline. Acceptance is automated at first. The stale rule moves in-flight segments onto the newest version, so my release reaches money at once. (`THR-4`, `THR-16`)
- **I am one person with a thousand agents.** The limits on work and on asking bind per caller, so they don't bind me. Each request fans out to several providers on the operator's credentials. I burn their quotas, and bury one large theft in a queue of small stuck orders. (`THR-52`, `THR-53`, `THR-58`)

### Data classification

Holders are roles. Levels: Secret (exposure means direct loss of value), Restricted (exposure harms people or breaks a
ruling), Confidential (commercially or operationally sensitive), Internal, Public. Checkpoints are the one public thing about the audit trail (`REQ-AUD-14`), and erasure reaches only what a reference names (`REQ-AUD-13`).
| Data | Held by | Level | Note |
|---|---|---|---|
| Provider credentials and sealed secrets | Plaintext: that provider's outside part only (R37). Sealed: whatever stores them (`PS-85`) | Secret | A single store concentrates every provider. |
| Publisher signing keys | Publisher | Secret | A stolen key gives a valid signature. |
| Authority to grant, accept, revoke, post routes, resolve | Operator | Secret | Exposure is the same as operator compromise. |
| Identity documents and files behind a judgement | Verifier, providers | Restricted | Not rutent, by `PS-30` and `PS-95`. Reliance is deferred (`DEF-3`). |
| Subject reference for a person | Operator, verifiers, providers | Restricted | Each provider has a different one, and only the operator joins them (`REQ-CPL-10`). |
| Judgements (person, class, verdict, validity) | Verifier, presenter, operator's records | Restricted | Bears no order (`PS-95`), so reusable until it expires. |
| Actual balance | Reserve only | Restricted | Never leaves it (`PS-31`). A reserve module may see it. |
| Balance-check log (asker, account, amount, answer) | Operator, reserve | Restricted | Thresholds in a log narrow a balance as well as the answers do. |
| Evidence that a user asked | The reserve, where it declares it can require the user; none otherwise (R48) | Restricted | Elsewhere, only the operator's record exists. |
| Order detail (amount, assets, nodes, destination) | Operator; each shortlisted provider for its part | Confidential | Market-sensitive while live. |
| Route decision records, candidates, reasons | Operator | Confidential | Restricted in aggregate. |
| Audit trail, in aggregate | Operator; regulators and the checker read it (`PS-88`, `PS-118`) | Restricted | More sensitive than any entry: it joins people, providers and time. |
| Live quotes | Provider, operator | Confidential | Kept from rivals. Stale ones are Internal. |
| Provider measures (p99, failures, refusals); stuck exposure | Operator | Confidential | Integrity matters most: they decide who pays. Exposure tells an attacker how near the operator's limit is. |
| Module binaries, machine code, catalogue, grants | Operator; publisher for its own binaries | Internal | Integrity is the concern. |
| Module shared and per-transaction state | Module, its host | Confidential | Crosses transactions by design (`PS-19`). |
| Protocol specification | Everyone (`PS-6`, `PS-64`) | Public | Safety can't depend on it being unread. |
| Details of a payer who isn't a customer | The provider that received them | Restricted | Rutent holds a reference at most (`REQ-CPL-11`). |
| Fee records | Provider, operator | Confidential | One record per settled segment, with no balances (`REQ-AUD-11`). |
| Evidence and its grade | Provider, operator; public where the rail is | Confidential | A public record at an endpoint shows amounts, by the rail's nature (`REQ-ORD-33`). |
| A reserve's signed book statement | Reserve, operator | Confidential | Holds no balance (`REQ-AUD-6`). |
| Lists of public and private nodes | Operator, providers | Public | `REQ-CAT-22`. |
| Held quotes | Provider, operator | Confidential | Kept from rivals, like live quotes. |
| Checkpoints of the audit trail | Operator, everyone | Public | Reveal nothing but a commitment to history (`REQ-AUD-14`). |

### Threats by STRIDE category

#### Spoofing

| ID | Actor | What they do | Cost, and to whom | From | Ending |
|---|---|---|---|---|---|
| `THR-1` | Holder of a stolen or forged login at the external authentication layer | Calls beyond what that user's policy allows, or passes login off as eligibility | Actions outside the account's rights; operator, providers | `PS-13` | Mitigated (prevents): `REQ-AUZ-1`, `REQ-CPL-3` |
| `THR-2` | Holder of a user's valid session | Requests an order from a reserve where the operator acts for that user, to the attacker's destination | The user's balance, final once it leaves | `PS-13`, `PS-89` | Accepted, R119, as ruled in R115 |
| `THR-3` | Outsider or rogue provider | Forges a provider's quote, outcome or late result. Calls are asynchronous, so results arrive on their own later | Wrong route, or value sent on or reversed wrongly; customer, operator | `PS-21`, `PS-81` | Mitigated (prevents): `REQ-ORD-1`, `REQ-QTE-1` |
| `THR-4` | Thief of a publisher's key, or attacker in the publish pipeline | Publishes a malicious module with a valid signature. Acceptance may be automated at first (R27) | All value that module touches | `PS-20`, `PS-84`, [modules] | Mitigated (detects only): `REQ-MDL-1`, `REQ-MDL-3`, `REQ-MDL-4`. Operator sees a widened capability or a bad staged release, if it looks |
| `THR-5` | Person or agent | Presents someone else's judgement, or one for another class | False eligibility; the relying provider | `PS-95` | Mitigated (prevents): `REQ-CPL-1` |
| `THR-6` | Agent | Claims to act for a person it doesn't represent, or beyond its scope | Unauthorised order; the person | `PS-2`, R34, [compliance] | Mitigated (bounds): `REQ-AUZ-8`, `REQ-CPL-2`. A party answers for the agent, and an agent can still exceed what it was told |
| `THR-7` | Attacker or insider short of full operator control | Forges a grant, changes a policy, slippage number or catalogue entry, or reaches an action by a path that skips policy | An attacker's segment in routes; all customers | `PS-6`, `PS-13`, `PS-84` | Mitigated (bounds): `REQ-AUZ-1`, `REQ-AUZ-6`, `REQ-CAT-2`. Short of full operator control |
| `THR-8` | Anyone running the published protocol | Reuses one deployment's quote, judgement or result at another, or relies on the specification being unread | False commitments; providers | `PS-6`, `PS-64` | Mitigated (prevents): `REQ-PRT-1` |
| `THR-9` | A legal party | Enters twice, re-enters after removal, or lets an unvetted party trade behind its grant | Removal stops meaning anything; collusion is hidden; operator | `PS-6`, [compliance] | Mitigated (detects only): `REQ-CAT-2`. Operator sees repeats at entry; a hidden principal may never show |
| `THR-102` | Relay of a request | States its customer's network origin wrongly, or leaves it out, where a compliance record calls for it | The record shows the relay, or lawful orders are refused | `PS-21`, `PS-36` | Mitigated (detects only): `REQ-AUD-2`. The record says it's the relay's word. A regulator reading the record sees it. |
| `THR-103` | Forger, or a lossy path | Forges, repeats, reorders or drops an outcome notice that an integrator or agent relies on | Goods released on a forgery, or a customer never told | `PS-107`, `PS-36` | Mitigated (bounds): `REQ-ORD-20`. A forged notice fails, and a lost one is recovered only by asking for the state |
| `THR-104` | Whoever alters a provider's response | Points a person or agent to a false approval page, on a rail whose host can't be known in advance | Credentials or approval given to an attacker | `PS-103`, `PS-83` | Mitigated (bounds): `REQ-ORD-26`, `REQ-MDL-12` |

#### Tampering

| ID | Actor | What they do | Cost, and to whom | From | Ending |
|---|---|---|---|---|---|
| `THR-10` | Attacker in the part that computes routes | Alters its inputs, so the route posted favours one path. This part sits outside the deterministic core (R26) | Customer overpays; value flows through a chosen provider; operator is accountable (`PS-80`) | `PS-80`, `PS-83` | Mitigated (detects only): `REQ-AUD-4`, `REQ-QTE-4`. Operator recomputes the decision afterwards |
| `THR-11` | Anyone who can trigger a recalculation | Spends the user's carried authority on a route with a new destination, or outside the customer's bounds | Theft or a loss; customer | `PS-89`, `PS-91` | Mitigated (prevents): `REQ-AUZ-3` |
| `THR-12` | Provider | Stops advertising, or gets marked stale, while value is on its segment, so a new route starts though the value hasn't returned | Value moves twice; operator | `PS-73`, `PS-86` | Mitigated (prevents): `REQ-ORD-2` |
| `THR-13` | Provider that failed | Says its reversal is done when it isn't, or refunds less, or in another asset. A true undo is the rule (`PS-72`), but it's marking its own work | Reroute starts on value that isn't there; customer, next provider | `PS-72`, `PS-92`, `PS-33` | Mitigated (bounds): `REQ-ORD-3`. A reserve at its own node still confirms its own reversal, labelled as its word |
| `THR-14` | Provider | Reports a wrong amount or a false success. Conservation is read from reports, since rutent isn't a ledger | Invariant holds on paper only; customer | `PS-33` | Mitigated (detects only): `REQ-ORD-9`, `REQ-AUD-15`. Operator sees two reports disagree. Detection never completes an order. A report that value reached a node is also checked against its holding account (`REQ-AUD-15`) |
| `THR-15` | Two adjacent providers | Agree on their reports | Undetectable from inside; customer or operator | `PS-33`, [compliance] | Accepted, R72. Where the value should rest in a holding account, its balance can show the agreement false (`REQ-AUD-15`) |
| `THR-16` | Compromised or malicious publisher | Ships a new version and gets the old one marked stale. In-flight segments then run on the newest (`PS-86`), which skips any gradual exposure | All in-flight value at that provider | `PS-18`, `PS-86` | Mitigated (bounds): `REQ-MDL-5` |
| `THR-17` | Attacker in the publish path | Reinstates an older signed version with a known flaw | A fixed exploit returns; that provider's orders | `PS-18`, [modules] | Mitigated (prevents): `REQ-MDL-4` |
| `THR-18` | Attacker in the platform's build step | Machine code stops matching the portable binary (`PS-96`). The publisher's assertion says nothing about it | Every module built; everyone | `PS-96`, [modules] | Mitigated (detects only): `REQ-MDL-2`. Operator can re-derive and compare, once it suspects |
| `THR-19` | A module | Poisons shared state for later transactions, or changes it off the record | Later decisions wrong and unexplained; provider, customers | `PS-19`, `PS-21` | Mitigated (bounds): `REQ-MDL-10` |
| `THR-20` | Outsider or insider short of full operator control | Alters or removes audit records | Fraud hidden from the operator itself | `PS-21` | Mitigated (detects only): `REQ-AUD-3`. Operator's own review sees it. Says nothing of `THR-33` or `THR-34` |
| `THR-21` | Provider | Picks its rate regime by its declared settlement time. Over three days frees it of bounds and takes the firm rate from the whole route (`PS-94`). Under three when the truth is longer breaks the commitment | Customer loses a firm price or gets a bad one | `PS-76`, `PS-94` | Mitigated (detects only): `REQ-QTE-3`, `REQ-ORD-10`. Customer sees the cause before accepting; operator measures declared against actual |
| `THR-22` | Provider | Always fills at the slippage limit and keeps the excess. Limits compound across segments | Hidden cost; customer | `PS-76`, R17 | Mitigated (bounds): `REQ-QTE-9`, `REQ-AUD-7`. The operator sets the limit per tuple, and filling at it stays visible only in bulk |
| `THR-23` | Provider or operator | Charges a fee not quoted, or shows a price above the sum of quotes | Customer | `PS-4`, `PS-76`, [routing] | Mitigated (detects only): `REQ-QTE-1`, `REQ-QTE-2`. Customer and operator compare stated with charged |
| `THR-24` | Provider | Runs slow on purpose, or sends itself traffic, to widen its own twice-p99 window | Value held longer; a longer option on the rate; customer, operator | `PS-81` | Mitigated (detects only): `REQ-ORD-6`. Operator sees the shift in its p99. No rule stops a slow provider being slow |
| `THR-25` | New provider | Has no p99, so the clock runs on its advertised time, which it chose | Longest blind spot at the least known provider; operator | `PS-81` | Mitigated (bounds): `REQ-ORD-29`, `REQ-QTE-3`. A clock runs from the first order, on a time the provider chose |
| `THR-26` | Provider | Answers every dry-run yes, advertises tuples it can't serve, or behaves differently in a dry-run, to win routes | Failed segments and reroutes; customers | `PS-70`, `PS-71`, [routing] | Mitigated (detects only): `REQ-AUD-7`. Operator measures dry-run against outcome, and can restrict or remove |
| `THR-27` | Provider | Passes the dry-run, then fails on purpose when the rate moves against it, since a reversal costs less than honouring it. Last look by another name | Customer pays through the next route | `PS-71`, `PS-76`, `PS-92` | Mitigated (detects only): `REQ-AUD-7`. Visible only across many orders, against market direction. The operator sees it. |
| `THR-28` | Colluding providers | Take turns failing, so recalculated routes pass the same fee-earner again. Fees on segments that settled stay paid (R130) | Fees drain from the customer | `PS-4`, `PS-91`, `PS-159` | Mitigated (bounds): `REQ-ORD-4`, `REQ-AUD-11` |
| `THR-29` | Customer | Presents a forged or old proof of funds at routing (`PS-77`) | Route built on nothing; operator, providers | `PS-77` | Mitigated (prevents): `REQ-ORD-32` |
| `THR-105` | Caller, or a confused customer | Asks again after no clear answer, or opens another order while a slow debit shows nothing | Two orders and two debits for one intent | `PS-21`, `PS-19` | Mitigated (bounds): `REQ-ORD-19`. A second order under a new identifier is still a second order |
| `THR-106` | Provider's own behaviour, or a retry | A repeat counts as a new attempt, or a gateway error hides a success, and a retry executes twice | Value sent twice, final once gone | `PS-71`, `PS-115` | Mitigated (bounds): `REQ-CAT-10`, `REQ-STM-4`. It rests on what the provider declares |
| `THR-107` | Adjacent providers | Report one value in different units, gross and net, or under a later identifier | Delivered value never confirmed, or matched to the wrong order | `PS-82`, `PS-33` | Mitigated (bounds): `REQ-CAT-11`, `REQ-ORD-9` |
| `THR-108` | Provider | Leaves a remainder nobody reports, because it fills in increments, or counts fees on top of the amount | Conservation fails with nothing visible | `PS-33` | Mitigated (detects only): `REQ-CAT-11`, `REQ-ORD-9`. Operator sees the difference |
| `THR-109` | Upstream provider | Delivers less than the next segment was priced for, so the next provider fails on value that never existed | The wrong provider is charged | `PS-92`, `PS-106` | Mitigated (prevents): `REQ-ORD-21` |
| `THR-110` | Provider | Bills the operator separately, so its fee is in no reported amount | Customer's price and operator's cost part ways | `PS-4`, `PS-124` | Mitigated (detects only): `REQ-AUD-11`. A provider and the operator see each fee on the record |
| `THR-111` | The world, or an attacker | A destination changes owner between routing and delivery, or reads as ineligible just after registration | The wrong person is paid, or a good route is lost | `PS-74`, `PS-89` | Mitigated (bounds): `REQ-ORD-24` |
| `THR-123` | Requester, or whoever misleads them | Provides a wrong address, or one a provider's rules wouldn't allow. Delivery to it is final | The customer's value; that provider's standing | `PS-128`, `PS-120` | Mitigated (bounds): `REQ-ORD-35`, `REQ-CPL-6`. An address that is valid but wrong still delivers |
| `THR-124` | One publisher on adjacent segments, or across a route | Its reports always agree, and it can stay under a ceiling per provider by using several. A private node has no other witness | Comparing reports checks nothing there; customer, operator | `PS-130`, `PS-33` | Mitigated (bounds): `REQ-ORD-8`, `REQ-ORD-3`, `REQ-CAT-20`. Agreement between its own reports stays undetectable (`THR-15`) |
| `THR-125` | Provider sharing one movement among several orders | Reports an outcome for each order that nothing else can check, since the movement is one | Each order's evidence is only that provider's word; customers, operator | `PS-133` | Mitigated (bounds): `REQ-ORD-38`, `REQ-ORD-39`. A floor limits it, and each order's outcome may still be that provider's word |
| `THR-126` | Provider on a private node | States an outcome with no proof anyone else can check. Its word is all there is | A false success moves value on; customer, next provider | `PS-138`, `PS-130` | Mitigated (bounds): `REQ-ORD-39`, `REQ-CAT-22`. The node is declared, and a floor limits it. Inside the node, its publisher's word may be all there is |
| `THR-127` | Reserve, or whoever drives it | Holds quotes it never means to use, as a free option or to tie up commitments | Providers' commitments, which cost nothing to make fail | `PS-137` | Mitigated (bounds): `REQ-QTE-15`, `REQ-OPS-1`. A held quote binds nobody, so it costs providers little |
| `THR-128` | Provider taking orders from its own customers and opening orders on the platform | Stands between rutent and the person it serves, so limits count against it and the person goes unseen | Providers' checks; regulators' view | `PS-139`, `PS-120` | Mitigated (bounds): `REQ-ORD-17`. Beneficiary and payer stay on the record, as far as that provider states them |
| `THR-129` | Operator | Appoints a friendly checker. Customers and providers can judge it, and nothing stops it | The check means less than it seems; everyone relying on the trail | `PS-118`, `PS-140` | Accepted, R119, as ruled in R92 |
| `THR-131` | Provider on a private node | Records inner segments made up to match its public exit | Replay passes on what never happened; customer | `PS-153`, `PS-152` | Mitigated (detects only): `REQ-ORD-41`. A match shows consistency with the exit, not that the segments happened |
| `THR-132` | Operator | Skips a checkpoint, or commits to a history other than the one it keeps | Public checking misses a rewrite; everyone relying on the trail | `PS-155`, `PS-104` | Mitigated (detects only): `REQ-AUD-14`. A missing one shows against the schedule, and a false one when someone holding that history checks it |
| `THR-134` | Any provider on one part of a split | Fails, or sticks, while other parts deliver | The customer ends with part at the destination and part elsewhere, or waits on the last part; customer, operator | `PS-157` | Mitigated (bounds): `REQ-ORD-42`, `INV-1`. Each part ends by itself, so a partly delivered order is a valid ending |
| `THR-135` | Attacker in the part that computes routes, or a provider gaming it | Splits an order to add fixed fees, or to send more value through a chosen provider | Customer overpays; customer, operator | `PS-157`, `PS-100` | Mitigated (bounds): `REQ-QTE-16`, `REQ-ORD-42`, `REQ-AUD-4`. The customer sees the total, and the choice can be recomputed |
| `THR-136` | Customer, or whoever drives the router | Uses splitting to keep each provider's share under an amount that would draw its checks | A provider judges only its own part, so its checks see less; provider, operator | `PS-157`, `PS-78` | Mitigated (detects only): `REQ-ORD-42`, `REQ-ORD-8`. The operator's records tie the parts together, and a provider still sees only its own |

#### Repudiation

| ID | Actor | What they do | Cost, and to whom | From | Ending |
|---|---|---|---|---|---|
| `THR-30` | Customer, or the operator | The customer denies asking when the operator acted. Or an honest user can't show they didn't ask | Dispute can't be settled; user or operator | `PS-88`, `PS-89` | Accepted, R119, as ruled in R109 |
| `THR-31` | Customer | Funds an order from an origin payment that can be undone later, takes final value at the destination, then undoes the origin | The origin reserve, which is out of both | `PS-91`, R17, `ASM-4` | Accepted, R119, as ruled in R97 |
| `THR-32` | Provider and operator in dispute | Disagree over who caused a stuck order. Only the operator holds a record | Provider can't contest; operator can't show cause when seeking recovery | `PS-93`, `PS-21` | Mitigated (detects only): `REQ-AUD-5`. Each party checks its own evidence against the other's |
| `THR-33` | Dishonest operator | Rewrites or omits its own records. A regulator who trusts them can't tell | Everyone who relies on the trail | `PS-88`, R21 | Mitigated (detects only): `REQ-AUD-9`. A checker outside the operator sees changed history, not what was never recorded |
| `THR-34` | Intruder with full operator access | Rewrites or omits records too. To a regulator it looks like the operator. R21's item named a dishonest operator, not this | Everyone who relies on the trail, and the operator, who is the victim | `PS-88` | Mitigated (detects only): `REQ-AUD-9`. As `THR-33`. The checker is whoever the operator appoints (R71) |
| `THR-35` | Any party | Leaves an action unrecorded. A log can't prove nothing is missing, and "when" is the logger's word | Gaps in "who did what, when"; regulators, operator | `PS-21`, R21, [durable] | Accepted, R21. Named on that item's lead |
| `THR-36` | Reserve at a final deposit | Reports the deposit. Nothing after it can confirm, since what is behind it is opaque | Order closes on one statement; customer | `PS-90`, `PS-151` | Mitigated (bounds): `REQ-ORD-33`. The book part stays the reserve's word, and on a rail with no public record so does the step out of the node |
| `THR-37` | Provider or verifier | Denies a quote, a refusal, a judgement or an outcome | The operator can't recover what it bore; customer | `PS-21`, R3 | Mitigated (bounds): `REQ-QTE-1`, `REQ-CPL-1`, `REQ-AUD-2`. Quotes and judgements can't be denied; other statements are kept as made |
| `THR-112` | Provider | Evidence of an outcome exists only briefly, or a notice is sent once | An order that delivered and can't show it | `PS-82`, `PS-115` | Mitigated (bounds): `REQ-CAT-10` |

#### Information disclosure

| ID | Actor | What they do | Cost, and to whom | From | Ending |
|---|---|---|---|---|---|
| `THR-38` | Account holder's delegate, or a session held by someone else | Binary-searches a balance within its allowance: about 40 answers. Limits slow this down; they don't stop it. Through the holder's own orders, only the bound on work (`REQ-OPS-1`) applies | The account holder's balance, in time | `PS-31`, `PS-87`, R20 | Accepted, R119, as ruled in R20. Named on that item's lead |
| `THR-39` | Many askers or agents | Spread checks, so no caller's allowance trips. R20 doesn't say what the limit binds to | `THR-38`, much faster; account holder | `PS-2`, `PS-87` | Mitigated (prevents): `REQ-CAT-3` |
| `THR-40` | Any caller | Learns whether funds suffice from something not called a check: a dry-run, a rejected order, a refused route | Goes round the limit and the log; account holder | `PS-31`, `PS-71`, `PS-87` | Mitigated (bounds): `REQ-CAT-4`. The holder's own orders are outside the bound |
| `THR-41` | Reserve module | Leaks a real balance through its output, errors or timing | `PS-31` fails; account holder | `PS-31`, [modules] | Mitigated (bounds): `REQ-MDL-12`. Timing remains |
| `THR-42` | Verifier | Learns a person is active now from the moment of the check, even when it sees only person and class | An activity pattern, joined with identity when the verifier also issued it; person | `PS-95`, [verifier] | Accepted, R72 |
| `THR-43` | Verifier or requester | Defines classes so fine that a class names an amount or a corridor | R33's promise fails quietly; person | `PS-95`, R33 | Mitigated (prevents): `REQ-CPL-4` |
| `THR-44` | Anyone who can force a no | Each no brings a new shortlist (R35), so more providers learn of the same person with each recalculation | R35's privacy wears away; person | `PS-95`, R35 | Mitigated (bounds): `REQ-CPL-5` |
| `THR-45` | Third party | Asks a verifier about people it has no order for: who is eligible, who is flagged | Compliance status of people leaks | `PS-78`, `PS-95` | Mitigated (prevents): `REQ-CPL-2` |
| `THR-46` | Shortlisted provider | Trades ahead or reprices on what it saw at the check or dry-run | Worse price; customer | `PS-71`, `PS-78`, [routing] | Mitigated (detects only): `REQ-AUD-7`. Operator sees it statistically, not per order |
| `THR-47` | Provider | Learns a rival's quote, or that a rival was asked | Quiet collusion; customers | `PS-16`, [routing] | Mitigated (prevents): `REQ-CAT-5` |
| `THR-48` | Operator, or whoever breaches it | Joins one person's activity across providers through a shared reference. Stopping that also stops anyone catching fronting | A full history of a person | `PS-30`, [compliance], [verifier] | Mitigated (bounds): `REQ-CPL-10`. The operator can still join them |
| `THR-49` | Intruder at the operator | Takes the audit trail and route records in bulk | Every customer's routes; a provider-by-provider map | `PS-21`, `PS-88` | Mitigated (bounds): `REQ-AUD-6` |
| `THR-50` | A bug or a module | Puts secret plaintext or a raw provider response into logs, errors or the trail | Credential theft; provider, operator | `PS-85`, R37 | Mitigated (bounds): `REQ-AUD-6`, `REQ-MDL-12`. A raw provider response isn't covered by name |
| `THR-51` | Customer, provider, or someone posing as a regulator | Reads orders or records that aren't theirs | Confidentiality; other customers | `PS-13`, `PS-88` | Mitigated (prevents): `REQ-AUZ-7` |
| `THR-113` | Provider, or whoever holds the records | Keeps the personal details of a payer who isn't a customer | That person's data, with no relationship behind it | `PS-30`, `PS-120` | Mitigated (bounds): `REQ-CPL-11`, `REQ-AUD-6` |
| `THR-114` | Provider | Its failure text, with markup or other parties' names, reaches a customer | Disclosure, and a reason nobody can act on | `PS-85` | Mitigated (prevents): `REQ-ORD-18` |
| `THR-130` | Anyone watching the checkpoints | Infers how busy the operator is from their timing or size | Volumes reach rivals; operator, providers | `PS-155` | Mitigated (prevents): `REQ-AUD-14` |
| `THR-133` | Anyone holding the log after an erasure | Links one person's orders through the reference that stays | Some linkage survives erasure; that person | `PS-154`, `PS-30` | Mitigated (bounds): `REQ-AUD-13`, `REQ-AUD-6`. The identity is gone, and the link between its orders stays |

#### Denial of service

| ID | Actor | What they do | Cost, and to whom | From | Ending |
|---|---|---|---|---|---|
| `THR-52` | Agents at scale | Flood quotes, routes, dry-runs and checks with no intent to order. One request fans out to several providers on real credentials | Provider quotas burn for everyone; operator's account at a provider may be blocked | `PS-2`, `PS-11`, `PS-71` | Mitigated (bounds): `REQ-OPS-1` |
| `THR-53` | One principal with many agents | Stays under every per-caller bound: the limits on work and on asking about people | `THR-52` and `THR-45` return | `PS-2`, [compliance] | Mitigated (bounds): `REQ-AUZ-8`, `REQ-OPS-1` |
| `THR-54` | Attacker | Uses up an account's check allowance so the owner's own orders fail. Checks only happen in a session, and the owner's own don't count | Victim is blocked | `PS-87` | Mitigated (prevents): `REQ-CAT-3` |
| `THR-55` | Attacker | Takes a verifier down so eligibility passes by default | Eligibility bypassed; providers, operator | `PS-78`, `PS-95` | Mitigated (prevents): `REQ-AUZ-5` |
| `THR-56` | Verifier or vouching provider | Says no falsely: blocks a lawful user, or steers a route away from a rival, since a no forces a recalculation | User; the rival | R35, [verifier] | Mitigated (detects only): `REQ-AUD-7`. Operator sees rates of no by maker |
| `THR-57` | Provider or module | Exhausts CPU, memory, outbound calls or state, or advertises so many tuples that routing slows. Never answering leaves work pending forever, since calls don't time out | Other providers; operator | `PS-18`, `PS-20`, `PS-70`, `PS-81` | Mitigated (bounds): `REQ-MDL-11`, `REQ-CAT-1` |
| `THR-58` | Attacker with many small orders on a slow provider | Fills the manual queue, so a large theft hides and warnings turn to noise | Operator; resolution is "mostly manual" | `PS-73`, `PS-81` | Mitigated (bounds): `REQ-OPS-2` |
| `THR-59` | Attacker | Suppresses the warning to the operator | Stuck orders go unseen while the operator carries the risk | `PS-73`, `PS-93` | Mitigated (detects only): `REQ-ORD-7`. Operator notices the warning path is down |
| `THR-60` | Absent customer, agent or person | Lets value wait between segments: for consent, for presence, for a route. The stuck rule covers only traversing a segment, so this is never stuck | Silent stall; customer | `PS-73`, `PS-81` | Mitigated (detects only): `REQ-ORD-5`. Operator and customer see age. |
| `THR-61` | Customer | Supplies input that makes a segment fail, repeatedly. The provider pays each reversal | Provider's money and willingness to serve | `PS-92`, R40 | Mitigated (bounds): `REQ-ORD-11` |
| `THR-62` | Provider or its affiliate | Makes value land where it alone can move it onward, then prices the reroute | Customer overpays with no alternative | `PS-74`, `PS-91`, `PS-99` | Mitigated (detects only): `REQ-AUD-7`, `REQ-ORD-4`. A floor limits it only where a customer set one. The operator sees it in bulk. |
| `THR-63` | Attacker, or a failing provider | Leaves value at a node with nothing onward, maybe in an asset the customer never wanted. Includes a reserve failing, despite `ASM-4` | Customer in the moment; then the operator, who bears it (R47). An attacker can force this, which bills the operator | `PS-91`, `ASM-4`, R47 | Mitigated (bounds): `REQ-ORD-8`. The size of the bill is limited |
| `THR-64` | Provider or attacker | Holds out at a forced protocol cutover, or ignores an "additive" change that added a protection | Routes break, or a protection is silently absent; customers | `PS-86` | Mitigated (bounds): `REQ-PRT-2` |
| `THR-65` | Agent, hijacked or mistaken | A route needs a person's presence. The agent stalls it, or pushes a person to approve something they don't understand | Customer | `PS-2`, `PS-89`, `PS-103` | Mitigated (detects only): `REQ-ORD-5`, `REQ-ORD-10`. The reserve decides (R51), and rutent reports what happened. Customer and operator see it. |
| `THR-115` | Caller | Opens orders in large numbers and never funds them. With no proven withdrawal, no route starts | They look like failures and never end | `PS-11`, `PS-107` | Mitigated (bounds): `REQ-OPS-1`, `REQ-ORD-40` |
| `THR-116` | Recalculation | Takes a route through a rail needing data the customer never gave | Value waits on an absent customer; asking up front tells providers never chosen | `PS-99` | Mitigated (bounds): `REQ-CAT-12` |
| `THR-117` | Attacker, or plain bad luck | An amount clears one band and lands below a later minimum, as fees shrink it | Value rests mid-route in an asset nobody wanted, and the operator bears it | `PS-70`, `PS-97` | Mitigated (bounds): `REQ-QTE-12`, `REQ-ORD-8` |

#### Elevation of privilege, and shifting cost

| ID | Actor | What they do | Cost, and to whom | From | Ending |
|---|---|---|---|---|---|
| `THR-66` | Intruder in the operator's systems | Moves funds at every reserve where the operator acts for users. No user asked, and nothing shows it | Total for those reserves, final, and at the scale of all users | `PS-88`, `PS-89` | Accepted, R119, as ruled in R48 |
| `THR-67` | Attacker of a party holding all roles | One compromise gives grant, publish, accept, judge and reserve. No role checks another | Every control in this table that relies on a second party | `PS-89`, `PS-95`, `PS-102`, R19 | Accepted, R119, as ruled in R50 |
| `THR-68` | Operator that is also a provider | Routes to itself against the customer's objective. Only it can replay the decision | Customer | `PS-75`, `PS-80`, `PS-102`, [routing] | Mitigated (detects only): `REQ-AUD-4`, `REQ-AUD-8`. A regulator can compare the decision record with each role, if it looks |
| `THR-69` | Operator, who bears stuck risk (R42) | Delays calling an order stuck, marks it failed to charge the provider, or avoids a rail it finds risky, whatever the customer's objective | Customer and provider. A money motive to shape its own records | `PS-81`, `PS-92`, `PS-93` | Mitigated (detects only): `REQ-ORD-6`, `REQ-AUD-5`. Customer and provider hold their own evidence. A regulator still sees only the operator's |
| `THR-70` | Customer and provider together | The provider delivers quietly and goes silent. The operator makes the customer whole. Stuck risk becomes insurance to defraud | Operator | `PS-82`, `PS-93` | Mitigated (bounds): `REQ-ORD-8`, `REQ-AUZ-4` |
| `THR-71` | Provider in trouble, or dodging a rate | Goes silent rather than failing. Failing costs it the reversal (`PS-92`). Silence past twice the p99 moves the risk to the operator (`PS-93`) | Operator pays. The rule rewards the worse behaviour | `PS-73`, `PS-92`, `PS-93` | Mitigated (bounds): `REQ-CAT-7`, `REQ-ORD-8` |
| `THR-72` | Provider | Can't pay for its reversal. Reversal capability is its own declaration (`PS-72`) | Value neither returned nor delivered; nobody is named | `PS-72`, `PS-92` | Mitigated (bounds): `REQ-CAT-7` |
| `THR-73` | Provider | Takes the value and disappears. "Assume the provider will eventually resolve it" fails | Operator; recovery is outside rutent | `PS-73`, `PS-93`, R42 | Accepted, R119, as ruled in R42 |
| `THR-74` | Any provider | Has no ceiling on value in flight, so `THR-71` and `THR-73` have no ceiling | Operator | `PS-93` | Mitigated (bounds): `REQ-ORD-8` |
| `THR-75` | Attacker who drives up stuck orders, or plain bad luck | Stuck losses exceed what the operator can pay. Customers then rely on its solvency | Customers | `PS-93` | Accepted, R72 |
| `THR-76` | Provider, late | Delivers after the operator rerouted or made the customer whole, as `PS-73` expects it may | Value paid twice; operator | `PS-73`, `PS-93` | Mitigated (bounds): `REQ-ORD-2`, `REQ-AUZ-4`. A reroute can't pay twice. An operator making a customer whole still can |
| `THR-77` | Rogue operator staff | Resolves a stuck order falsely, or to a friend, alone | Value; operator, customer | `PS-81`, `PS-93` | Mitigated (prevents): `REQ-AUZ-4` |
| `THR-78` | Code in the sandboxed part | Tries to read a secret, or asks the outside part for something beyond the declared operation | Money moved on real credentials; provider, operator | `PS-83`, R37 | Mitigated (prevents): `REQ-MDL-6`, `REQ-MDL-8` |
| `THR-79` | Attacker who compromises an outside part, through a crafted provider response or a bad release, or who poses as the provider's endpoint | Holds that provider's plaintext credentials, and acts or exfiltrates through allowed destinations | Money at that provider. Without a bound, at every provider | `PS-83`, `PS-85`, R37, [modules] | Mitigated (bounds): `REQ-MDL-9`, `REQ-MDL-7` |
| `THR-80` | Attacker using a declared operation | The credential allows more than rutent needs, and only the provider's side can limit it | Whatever the credential allows | `PS-83`, [modules] | Mitigated (bounds): `OBL-1` |
| `THR-81` | Holder of a stolen credential | Uses it from elsewhere. rutent never sees it; only the provider could | Money at that provider | `PS-85` | Accepted, R72 |
| `THR-82` | Publisher | Delivers an outside part that never passed admission, since `PS-84` and `PS-96` speak of modules | Unverified code with real credentials | `PS-83`, `PS-84` | Mitigated (prevents): `REQ-MDL-1` |
| `THR-83` | Whoever writes the outside part | Reads plaintext by design. R37 protects from the sandboxed part, not from this author. R27, R22 and R37 pull apart on what this part is | Every credential it handles | `PS-85`, R22, R27, R37 | Accepted, R119, as ruled in R65 |
| `THR-84` | A module | Breaks out through a known class of flaw, or reads another module's state or secrets | That reach, up to all providers on the host | `PS-20`, `PS-85` | Mitigated (bounds): `REQ-MDL-9`, `REQ-OPS-3` |
| `THR-85` | A module | Breaks out through an unknown flaw or a hardware side channel | Unbounded by anything in this document | `PS-20`, [modules] | Accepted, R72 |
| `THR-86` | Module A | Calls module B, so that B acts on B's credentials. `PS-85` lets modules interact and doesn't say across providers | One provider's code spends another's authority | `PS-85` | Mitigated (bounds): `REQ-MDL-21` |
| `THR-87` | Granted provider or publisher | Turns malicious. Entry and signature don't help: "the provider list remains trusted" | Whatever it touches, until removed. Holds only while third parties stay deferred | `PS-84`, R27 | Accepted, R119, as ruled in R27 |
| `THR-88` | Insider | Declares a version "actively exploited" falsely to halt a rival, or sits on a true one | Stranded orders, or continued loss | `PS-86`, R31 | Mitigated (detects only): `REQ-AUZ-6`, `REQ-AUD-1`. Operator's review sees who declared and when |
| `THR-89` | Attacker | Exploits a flawed version while in-flight orders finish on it, until someone declares | Those orders' value | `PS-86`, R31 | Accepted, R31 |
| `THR-90` | Provider | Posts a route, or has its suggestion treated as the operator's | Breaks `PS-80`; customers | `PS-74`, `PS-80` | Mitigated (prevents): `REQ-AUZ-2` |
| `THR-91` | Reserve or verifier | Says yes without checking, or checks wrongly. No signature shows a failure of truth | Provider, and the people it should have stopped. The provider that relied on it carries the loss (`REQ-CPL-12`) | `PS-22`, `PS-78`, [compliance] | Accepted, R72 |
| `THR-92` | Verified person | Fronts for others, or runs many agents. Possession of a credential doesn't show who benefits | Providers; regulators' view of them | `PS-2`, `PS-22`, [compliance] | Accepted, R72 |
| `THR-93` | Person | A yes outlives the facts, or is withdrawn mid-order. Yet "orders are not failible" | Value in flight for someone no longer eligible; operator | `PS-92`, `PS-95`, [verifier] | Accepted, R119, as ruled in R64 |
| `THR-94` | Customer | Passes a check, then spends the funds, or passes two orders on one balance. A yes is no reservation (R41). The withdrawal then fails, and the order is cancelled | Free work for the attacker; maybe a stuck order the operator pays for | `PS-77`, `PS-92`, R41 | Mitigated (bounds): `REQ-ORD-40`, `REQ-OPS-1`. It can't become a stuck order, but the work is still done |
| `THR-96` | Reserve | Declares it requires the user's own withdrawal when it doesn't, or stops doing so after declaring. Users accept a route on false assurance | The user's funds at that reserve | `PS-98` | Mitigated (detects only): `REQ-CAT-6`, `REQ-AUD-2`. A loss shows the statement was false, and it stays attributed to its maker. The user and the operator see it after a loss. |
| `THR-95` | Provider | Relies on a bare judgement with no path to the data behind it | That provider's own standing. Not stated here as law | `PS-95`, `DEF-3`, [verifier] | Accepted, R119, as ruled in R79 |
| `THR-97` | Provider | Changes its fee formula after a customer accepted the route, or keeps it unreadable, so the outcome isn't known before use | Customer pays more than a formula said | `PS-100`, `PS-101` | Mitigated (bounds): `REQ-QTE-6`, `REQ-QTE-5`. Binding starts at segment start (R57), so a formula can change after acceptance and before that |
| `THR-98` | Provider, or an operator that is also one | Quotes low to win routes and executes higher, since a quote isn't a promise and users accepted that | Customer, repeatedly; honest providers lose routes | `PS-75`, `PS-100` | Mitigated (detects only): `REQ-AUD-7`. Operator sees quotes against fills, in bulk |
| `THR-99` | Provider or operator | Prices badly on a route with an automatic deposit, where nobody accepted a price | Customer, unnoticed | `PS-100` | Accepted, R119, as ruled in R59 |
| `THR-100` | Operator, or an intruder there | Appoints nobody as checker, or changes the appointment | `REQ-AUD-9` checks nothing; everyone relying on the trail | `PS-118`, R71 | Mitigated (prevents): `REQ-AUD-10`, `OBL-4`. Both are required and recorded |
| `THR-101` | Reserve, or the operator | Opens an order on a user's deposit, with no instruction from that user, to a route the user never asked for | The user's deposit | `PS-114`, R67, R59 | Accepted, R119, as ruled in R91 |
| `THR-118` | Operator's own side | Moves its execution environment, so a provider refuses every call, and each failure charges the provider | Provider charged for the operator's change | `PS-106`, `PS-92` | Mitigated (bounds): `REQ-ORD-11`, `OBL-6`. A provider that can't show the cause is still charged or in dispute |
| `THR-119` | Customer's bank, in effect | An approval stays undecided long after the customer stopped waiting, then approves | Customer charged with nothing delivered, or charged twice | `PS-107` | Mitigated (detects only): `REQ-ORD-25`. Customer sees the late approval |
| `THR-120` | Anyone reading an unclear refusal | Can't tell "no route for this amount" from "no route right now" | Payments repeated or abandoned | `PS-21`, `PS-122` | Mitigated (prevents): `REQ-ORD-18`, `REQ-CAT-18` |
| `THR-121` | Provider | Charges above its formula's maximum | Customer | `PS-119` | Mitigated (detects only): `REQ-QTE-5`. Operator sees charge against formula |
| `THR-122` | Payer | Sends less, more or in pieces against an open order | A shortfall can't traverse anything, and an excess has no order to belong to | `PS-114` | Mitigated (prevents): `REQ-ORD-23`. A payment after the order ended is `PS-34` |

### Stated limits

Accepted by the owner under R72. rutent states these and doesn't solve them.

| Threat | Limit |
|---|---|
| Limit of `THR-15` | Two adjacent providers agreeing their reports can't be detected from inside. |
| Limit of `THR-92` | A verified person can front for others, or run many agents. |
| Limit of `THR-91` | A verifier or reserve can state something false. The provider that relied on it carries that loss (`REQ-CPL-12`). |
| Limit of `THR-42` | A verifier learns something just by being asked. |
| Limit of `THR-75` | Customers rely on the operator's solvency for stuck orders. |
| Limit of `THR-81` | A stolen credential can be used outside rutent. |
| Limit of `THR-85` | An unknown sandbox escape or hardware side channel isn't bounded by this document. |

### Accepted under earlier rulings

Each was ruled on its own item, and the lead confirmed them together under R119.

| Threat | Ruling | What is accepted |
|---|---|---|
| Limit of `THR-38` | R20 | A delegate or session holder can narrow a balance, since thresholds are free |
| Limit of `THR-87` | R27 | A granted provider or publisher can turn malicious, since the list is trusted |
| Limit of `THR-73` | R42 | A provider can take the value and go, and the operator bears it |
| Limit of `THR-66` | R48 | An intruder at the operator can move funds at reserves that can't require the user |
| Limit of `THR-67` | R50 | One party holding all roles has no role checking another |
| Limit of `THR-99` | R59 | A bad price on an automatic deposit, which nobody accepted |
| Limit of `THR-93` | R64 | A yes that lapses mid-order doesn't end the order |
| Limit of `THR-83` | R65 | Whoever writes the outside part reads plaintext |
| Limit of `THR-95` | R79 | A provider relying on a bare judgement, while reliance is deferred |
| Limit of `THR-101` | R91 | A reserve on automatic deposits can start a route the user never named |
| Limit of `THR-31` | R97 | A customer can keep what was delivered and undo the origin, at the origin reserve's cost |
| Limit of `THR-30` | R109 | A dispute where the operator acted has only the operator's record |
| Limit of `THR-2` | R115 | A stolen session acts as the user at those reserves |
| Limit of `THR-129` | R92 | A friendly checker: customers and providers can judge it, and nothing stops one |

### What can't be mitigated

Stated plainly, so nobody reads a control above as more than it is.

- **A dishonest operator.** It writes the records. A checker outside it sees changed history (`REQ-AUD-9`), not what was never written or whether an entry is true. `REQ-AUD-5` gives customers and providers their own evidence.
- **An intruder with full operator access looks the same.** Everything recorded after the intrusion is the intruder's word.
- **An operator acting for a user, where it is also the reserve.** Even a user's signed request is checked by the same party. It is evidence afterwards, not a gate.
- **A provider that takes the value and goes.** Nothing in rutent can recall it. `REQ-ORD-8` sizes the loss and prevents nothing.
- **Truth.** Signatures catch forgery, replay and staleness. They catch no false statement, from a verifier, a reserve, or two providers in agreement.
- **Completeness.** A record can't prove nothing was left out, except against something a counterparty kept.
- **Atomicity across independent providers.** [durable] reports a proof that a time-bounded payment across parties is impossible without synchrony, even with honest parties. A segment gets undone; nothing commits together.
- **Balance inference within a session (R20, R96).** Limits delay it, and the holder's own orders are outside them. Nothing stops it while thresholds are free.
- **What a verifier learns by being asked.** No design removes the signal in the moment of the check.
- **Unknown escapes, and malicious admitted code.** The sandbox is a boundary, not a proof, and a signature proves who, not whether it's safe. R27's trust holds only while third parties stay deferred (`PS-84`).

### Coverage

| Area | Threats |
|---|---|
| Module supply chain | `THR-4`, `THR-16`, `THR-17`, `THR-18`, `THR-82`, `THR-87` |
| Modules interacting; secrets | `THR-78`, `THR-81`, `THR-84`, `THR-86` |
| The part outside the sandbox | `THR-10`, `THR-78`, `THR-79`, `THR-80`, `THR-82`, `THR-83` |
| Authority to move funds, reroute or return | `THR-2`, `THR-11`, `THR-12`, `THR-13` |
| The operator acting for a user | `THR-2`, `THR-30`, `THR-66` |
| One party in several roles | `THR-67`, `THR-68`, `THR-42` |
| A provider unable to pay for its reversal | `THR-13`, `THR-72` |
| Eligibility checks at route-building | `THR-44`, `THR-45`, `THR-55`, `THR-56`, `THR-93` |
| Sandbox escape, resource abuse, state isolation | `THR-19`, `THR-57`, `THR-84`, `THR-85` |
| Quote and route manipulation | `THR-3`, `THR-10`, `THR-21`, `THR-22`, `THR-23`, `THR-46`, `THR-47`, `THR-90` |
| Repudiation and audit tampering | `THR-20`, `THR-30`, `THR-32`, `THR-33`, `THR-34`, `THR-35`, `THR-36`, `THR-131`, `THR-132` |
| Public checkpoints and erasure | `THR-130`, `THR-132`, `THR-133` |
| Splitting an order | `THR-134`, `THR-135`, `THR-136` |
| Inference from balance verifications | `THR-38`, `THR-39`, `THR-40`, `THR-41`, `THR-54` |
| False compliance proofs | `THR-5`, `THR-91`, `THR-92`, `THR-95` |
| Authorisation bypass at the authentication boundary | `THR-1`, `THR-2`, `THR-7`, `THR-51` |
| Denial of service by humans or agents | `THR-52`, `THR-53`, `THR-58`, `THR-57` |
| Compromise of the operator's own systems | `THR-7`, `THR-20`, `THR-34`, `THR-49`, `THR-66`, `THR-67`, `THR-77` |
| Dry-run then failure; gaming a reversal or the p99 rule | `THR-13`, `THR-24`, `THR-25`, `THR-26`, `THR-27`, `THR-71`, `THR-76` |
| Route recalculation | `THR-11`, `THR-12`, `THR-28`, `THR-63`, `THR-44`, `THR-56`, `THR-62` |
| Operator bearing stuck risk | `THR-69`, `THR-70`, `THR-71`, `THR-73`, `THR-74`, `THR-75`, `THR-77` |
| Published protocol, entry by grant | `THR-7`, `THR-8`, `THR-9`, `THR-64`, `THR-87` |
| Prices and slippage | `THR-21`, `THR-22`, `THR-23`, `THR-27` |
| Agents at scale | `THR-6`, `THR-39`, `THR-52`, `THR-53`, `THR-65`, `THR-92` |
| Correlation, fronting, collusion (from [compliance]) | `THR-9`, `THR-15`, `THR-48`, `THR-92` |

| Invariant stub | Attacked by |
|---|---|
| Conservation of value | `THR-13`, `THR-14`, `THR-15`, `THR-31`, `THR-36`, `THR-72`, `THR-73`, `THR-76` |
| Atomicity across an order | `THR-12`, `THR-63`, `THR-76`, `THR-93`, `THR-94`. `THR-63` is bounded after R47 |
| rutent holds no customer value | `THR-66`, `THR-67`. It holds for rutent. It doesn't protect value at an operator that is also a reserve |
| Completeness of the audit trail | `THR-20`, `THR-33`, `THR-34`, `THR-35`, `THR-50` |
| Uniqueness of a transaction's identifier | `THR-3` (`REQ-ORD-1`) |
| A balance verification reveals only sufficiency | `THR-38`, `THR-39`, `THR-40`, `THR-41` |

## 9. Project guarantees and operator obligations

The split `oss-ledger-2` requires. What rutent itself guarantees, against what whoever operates it has to do. "Orders are not failible" (`PS-92`) is an operator obligation, since software can't promise what independent providers do.

| ID | rutent guarantees | Source |
|---|---|---|
| `GUA-1` | rutent MUST hold no customer value. | `PS-79` |
| `GUA-2` | Routing MUST check for a positive arrival. That's a check at routing and no promise afterwards. | `PS-77` |
| `GUA-3` | Every action MUST have its record, and a checker outside the operator MUST be able to see changed history. This covers neither what was never recorded nor whether an entry is true. | `PS-21`, `PS-104` |
| `GUA-4` | A balance verification MUST reveal only whether funds suffice. | `PS-31` |
| `GUA-5` | A module's sandboxed part MUST NOT see secret plaintext, and a module MUST NOT reach another provider's state or secrets. This is a boundary, not a proof (`THR-85`). | `PS-20`, `PS-85` |
| `GUA-6` | The protocol's specification MUST be public. | `PS-6` |
| `GUA-7` | rutent MUST produce checkpoints of the audit trail on a fixed schedule, revealing nothing but a commitment to history. | `PS-155` |

| ID | The operator must | Source |
|---|---|---|
| `OBL-1` | The operator MUST hold, for each provider, the narrowest credential that provider offers, and record what it allows. | `PS-83`, `oss-ledger-2`, `THR-80` |
| `OBL-2` | The operator MUST resolve stuck orders and bear them. How it recovers from a provider is contract, outside this document. | `PS-93` |
| `OBL-3` | The operator MUST check a provider's ability to pay for reversals before granting entry. | `PS-105` |
| `OBL-4` | The operator MUST appoint a checker, and name it to customers and providers. | `PS-118`, `PS-140` |
| `OBL-5` | The operator SHOULD set ceilings on value in flight, and watch its exposure on stuck orders. Customers rely on its solvency (`THR-75`). | `PS-93`, `THR-74` |
| `OBL-6` | The operator MUST keep what each provider requires of it, such as an approved source address, in order when moving the execution environment. | `THR-118` |
| `OBL-7` | The operator MUST check price disclosure and other local rules for wherever it operates. A price shown as a quote may not meet them everywhere. | `PS-65`, `PS-100` |
| `OBL-8` | The operator MUST publish every checkpoint on its schedule. | `PS-155` |
| `OBL-9` | The operator MUST set how long records are kept and honour erasure, for its jurisdiction. | `PS-154`, `PS-65` |

## 10. Assumptions

| ID | Assumption | Source |
|---|---|---|
| `ASM-1` | "Thousands of requests" has no number for v1, so `NFR-1` has no bound | R5, R103, `PS-50` |
| `ASM-2` | No deadline, budget or target-platform constraint is known | R13, `PS-63` |
| `ASM-3` | Distribution channels and revenue structure are unknown | R12, `PS-3`, `PS-5` |
| `ASM-4` | Reserves are "expected to be infallible" | R39, `PS-91` |
| `ASM-5` | A settlement account may lawfully stand as the recorded beneficiary, with payee checks at its reserve. The owner's decision; counsel hasn't confirmed it | R135, `PS-164` |
| `ASM-6` | Whoever controls a public node's holding account holds value in transit there; for the operator, any custody duty this carries is unconfirmed by counsel | R141, `PS-168` |

## 11. Deferred scope

| ID | What | Source |
|---|---|---|
| `DEF-1` | Open operation; the protocol's specification itself is in v1 | `PS-23`, R44 |
| `DEF-2` | Order books | `PS-23` |
| `DEF-3` | Reliance between unrelated parties under FATF, while each provider belongs to the judging entity or checks for itself. | R32, R79 |
| `DEF-4` | Third-party publishers | `PS-84` |
| `DEF-5` | An ISO 20022 mapping | `oss-ledger-3`, R126 |

## 12. Open questions

| ID | Question | Item |
|---|---|---|
| `OQ-1` | Closed by R8: `oss-ledger`, adding `reg-fintech` if regulation demands | `oq-profile-correction` |
| `OQ-2` | Closed by R15: What are a route and a graph? | `oq-route-meaning` |
| `OQ-3` | Closed by R23: How does rutent relate to bezent and aledgent? | `oq-product-map` |
| `OQ-4` | Closed by R13: Deadline, budget, platforms? | `oq-constraints` |
| `OQ-5` | Closed by R12: Channels, cost structure, revenue structure? | `oq-problem-descriptors` |
| `OQ-6` | Closed by R102: What else is out of scope? | `oq-further-nongoals` |
| `OQ-7` | Closed by R14: "Ramp" names both a provider category and the product's role. How do they relate? | `oq-ramp-term` |
| `OQ-8` | Closed by R10: Who posts quotes and routes: only the operator, or third parties too? | `oq-actors` |
| `OQ-9` | Closed by R11: What must happen when an order stops partway across providers? | `oq-partial-failure` |
| `OQ-10` | Closed by R9: Does rutent itself ever hold customers' value? | `oq-custody` |
| `OQ-11` | Closed by R21: Who must be able to verify the audit trail? | `oq-audit-verifier` |
| `OQ-12` | Closed by R24: When a provider's outcome is unknown, who bears the risk? | `oq-unknown-outcome` |
| `OQ-13` | Closed by R25: What must a provider offer to join the catalogue? | `oq-provider-entry` |
| `OQ-14` | Closed by R26: May a provider be split into a sandboxed part and an outside part? | `oq-provider-io` |
| `OQ-15` | Closed by R27: Who writes provider modules in v1, and in which languages? | `oq-module-authors` |
| `OQ-16` | Closed by R29: Which licences may a required dependency have? | `oq-dependency-licences` |
| `OQ-17` | Closed by R28: May a provider module ever hold a credential in plaintext? | `oq-module-credentials` |
| `OQ-18` | Closed by R30: Who compiles a module to machine code? | `oq-module-compilation` |
| `OQ-19` | Closed by R31: May an order in flight finish on a revoked module version? | `oq-revoked-inflight` |
| `OQ-20` | Closed by R17: Must rutent offer one firm price for a whole route? | `oq-quote-commitment` |
| `OQ-21` | Closed by R16: What should rutent optimise a route for by default? | `oq-route-objective` |
| `OQ-22` | Closed by R18: Which countries first, and legal opinions before the spec? | `oq-first-jurisdictions` |
| `OQ-23` | Closed by R19: Is proving a provider said it enough, and who bears a false statement? | `oq-false-attestation` |
| `OQ-24` | Closed by R20: Who may run a balance check, and outside a real order? | `oq-balance-check-access` |
| `OQ-25` | Closed by R36: Who authorises each move of funds between segments? | `oq-execution-authority` |
| `OQ-26` | Closed by R37: After unsealing a secret, does module code see the plaintext? | `oq-credential-plaintext` |
| `OQ-27` | Closed by R38: Is VOS a binding constraint, or a suggestion? | `oq-vos-binding` |
| `OQ-28` | Closed by R39: What counts as a reversal on rails where payments are final? | `oq-return-meaning` |
| `OQ-29` | Closed by R40: Who pays when value returns to the origin? | `oq-return-cost` |
| `OQ-30` | Closed by R41: Is arriving with a positive balance a check or a promise? | `oq-routing-guarantee` |
| `OQ-31` | Closed by R42: Who may resolve a stuck order, and who bears the risk meanwhile? | `oq-stuck-risk` |
| `OQ-32` | Closed by R43: What firm price does a route get with a segment over three days? | `oq-long-segment` |
| `OQ-33` | Closed by R44: Is the protocol specification itself part of v1? | `oq-protocol-scope` |
| `OQ-34` | Closed by R45: Do publishers upload portable binaries or native machine code? | `oq-compile-format` |
| `OQ-35` | Closed by R46: Does any rail needed lack both an identifier and a status query? | `oq-provider-rails` |
| `OQ-36` | Closed by R32: Is a verifier a fourth provider category, or something else? | `oq-verifier-role` |
| `OQ-37` | Closed by R33: May a verifier see the order, or only a person and a class? | `oq-verifier-sees-order` |
| `OQ-38` | Closed by R34: Does a user ever present anything, or do providers speak for them? | `oq-user-presents` |
| `OQ-39` | Closed by R35: Should a provider never chosen learn that a check happened? | `oq-unchosen-check` |
| `OQ-40` | Closed by R47: What happens when no route remains after a failed segment? | `oq-no-route-left` |
| `OQ-41` | Closed by R48: When the operator acts for a user, what proves the user asked? | `oq-operator-acting` |
| `OQ-42` | Closed by R49: Who accepts and pays when a recalculated route costs more? | `oq-reroute-consent` |
| `OQ-43` | Closed by R50: May one party be operator, reserve, verifier and publisher at once? | `oq-role-overlap` |
| `OQ-44` | Closed by R51: How does an agent pass a reserve that demands a person's presence? | `oq-agent-presence` |
| `OQ-45` | Closed by R79: may FATF reliance stay deferred past v0? Yes unless changed | `oq-fatf-deferred` |
| `OQ-46` | Closed by R56: Does a provider that goes silent still owe the reversal? | `oq-silence-cost` |
| `OQ-47` | Closed by R61: May a route restart before the value is evidenced back? | `oq-reroute-before-return` |
| `OQ-48` | Closed by R62: Does your audit answer also cover an intruder at the operator? | `oq-audit-reconfirm` |
| `OQ-49` | Closed by R63: Who bears it when an origin payment is undone after delivery? | `oq-origin-clawback` |
| `OQ-50` | Closed by R64: When can an order still end without completing? | `oq-order-can-end` |
| `OQ-51` | Closed by R65: Who writes the outside part of a module, and under what rules? | `oq-outside-part` |
| `OQ-52` | Closed by R66: Who is "the provider that failed" when the cause is unclear? | `oq-fault-attribution` |
| `OQ-53` | Closed by R70: What do limits bind to, when one party runs many agents? | `oq-agent-accountable` |
| `OQ-54` | Closed by R72: Do you accept these seven as limits rutent states, not solves? | `oq-accepted-limits` |
| `OQ-55` | Closed by R57: What still binds a provider, now a route's price is only a quote? | `oq-provider-commitment` |
| `OQ-56` | Closed by R58: May a customer set a worst acceptable outcome? | `oq-price-floor` |
| `OQ-57` | Closed by R59: What stands in for acceptance on automatic-deposit routes? | `oq-auto-deposit` |
| `OQ-58` | Closed by R60: What may a provider's fee formula depend on? | `oq-fee-formula` |
| `OQ-59` | Closed by R71: Who is the checker outside the operator? | `oq-outside-checker` |
| `OQ-60` | Closed by R69: When an origin is undone after delivery, who carries the loss? | `oq-clawback-loss` |
| `OQ-61` | Closed by R67: Is value that arrives with no route an order? | `oq-unrouted-arrival` |
| `OQ-62` | Closed by R68: Should entry also require lasting evidence and safe retries? | `oq-entry-evidence` |
| `OQ-63` | Closed by R75: What if a destination reports complete, then later returned? | `oq-late-return` |
| `OQ-64` | Closed by R73: How is a fee known only after execution handled? | `oq-unknown-fee` |
| `OQ-65` | Closed by R74: Whose eligibility counts when requester, beneficiary and payer differ? | `oq-three-parties` |
| `OQ-66` | Closed by R76: How is a limit by amount judged, if verifiers don't see amounts? | `oq-amount-limits` |
| `OQ-67` | Closed by R77: May a reserve answer "can't tell" to a balance check? | `oq-reserve-unknown` |
| `OQ-68` | Closed by R78: Are integrators and referrers parties of their own? | `oq-integrator-party` |
| `OQ-69` | Closed by R91: What stands for the user's authorisation on an automatic deposit? | `oq-on-behalf-authority` |
| `OQ-70` | Closed by R92: Must an operator appoint a checker, and who sees who it is? | `oq-checker-required` |
| `OQ-71` | Closed by R94: How does the operator charge for its own service? | `oq-operator-fee` |
| `OQ-72` | Closed by R95: How does a provider know what it captured as fees? | `oq-fee-capture` |
| `OQ-73` | Closed by R93: How does a provider move its captured fees out? | `oq-fee-withdrawal` |
| `OQ-74` | Closed by R99: Who sets the protocol's slippage number? | `oq-slippage-setter` |
| `OQ-75` | Closed by R105: What starts the stuck clock before a provider has a p99? | `oq-stuck-clock` |
| `OQ-76` | Closed by R106: What is a user's proof that funds are at the origin? | `oq-proof-of-funds` |
| `OQ-77` | Closed by R100: What closes an order at its final deposit? | `oq-drop-evidence` |
| `OQ-78` | Closed by R107: May one reference link a person across providers? | `oq-linking` |
| `OQ-79` | Closed by R101: May one provider's module call another provider's module? | `oq-module-calls` |
| `OQ-80` | Closed by R102: Is anything else out of scope? | `oq-further-nongoals` |
| `OQ-81` | Closed by R80: What does a reserve answer when it can't know yet? | `oq-reserve-cant-know` |
| `OQ-82` | Closed by R96: Do balance checks happen only in a user's or delegate's session? | `oq-balance-check-session` |
| `OQ-83` | Closed by R97: The customer keeps what was delivered. Is it still the origin reserve? | `oq-clawback-confirm` |
| `OQ-84` | Closed by R103: What number should "thousands of requests" mean? | `oq-scale-number` |
| `OQ-85` | Closed by R108: Do you accept the requirements that go further than a ruling? | `oq-derived-review` |
| `OQ-86` | Closed by R110: Must the same recorded inputs always reach the same states? | `oq-replay` |
| `OQ-87` | Closed by R109: Does your ruling on the operator acting also accept a stolen session and unsettled disputes? | `oq-operator-acting-scope` |
| `OQ-88` | Closed by R104: Who confirms a reversal when the failed provider is the reserve at that node? | `oq-reversal-confirmer` |
| `OQ-89` | Closed by R98: Is "can't go unseen by the checker" what "never possible" means? | `oq-rewrite-meaning` |
| `OQ-90` | Closed by R81: What is rutent, beyond the brand line? | `oq-what-rutent-is` |
| `OQ-95` | Closed by R86: What may a module do beyond its role? | `oq-role-contract` |
| `OQ-96` | Closed by R87: What does "idempotent" mean for a conversion? | `oq-idempotent-meaning` |
| `OQ-97` | Closed by R88: Can several orders share one movement? | `oq-shared-movement` |
| `OQ-98` | Closed by R89: What kind of state machine is this? | `oq-state-machine-model` |
| `OQ-99` | Closed by R90: How does a new kind of step get in? | `oq-new-step-kinds` |
| `OQ-94` | Closed by R85: Does the route example under R84 stay? | `oq-example-stays` |
| `OQ-93` | Closed by R84: What can a reserve do, and what is behind it? | `oq-reserve-endpoints` |
| `OQ-92` | Closed by R83: What provider handles a last segment that exits to a wallet address? | `oq-wallet-exit` |
| `OQ-91` | Closed by R82: Does every route also end at a reserve, and is a drop provider one? | `oq-route-end` |
| `OQ-100` | Closed by R113: What must each transition prove, and what about a private node? | `oq-evidence-floor` |
| `OQ-101` | Closed by R114: What counts as third-party evidence at a final deposit? | `oq-third-party-meaning` |
| `OQ-102` | Closed by R116: Does a held quote bind the providers quoted? | `oq-reserved-quote` |
| `OQ-103` | Closed by R115: How far should policy go against a stolen session? | `oq-session-mitigation` |
| `OQ-104` | Closed by R112: May I add a reserved item confirming the accepted threats? | `oq-reserved-permission` |
| `OQ-105` | Closed by R111: Should the form's link fix go to Narthex? | `oq-narthex-fix` |
| `OQ-106` | Closed by R117: What does "a public record suffices" mean for a private node? | `oq-private-record-meaning` |
| `OQ-107` | Closed by R118: Can a deposit that stays in a book end a route? | `oq-book-only-deposit` |
| `OQ-108` | Closed by R119: Do you confirm these accepted threats? | `oq-accepted-rulings` |
| `OQ-109` | Closed by R120: How does a final deposit close on a rail with no public record? | `oq-fiat-final-deposit` |
| `OQ-110` | Closed by R121: Who confirms a reversal inside a private node? | `oq-private-reversal` |
| `OQ-111` | Closed by R122: How long are records kept, and can any be erased? | `oq-retention` |
| `OQ-112` | Closed by R123: May anything about the audit trail be public? | `oq-trail-public` |
| `OQ-113` | Closed by R124: Who carries the loss from a false judgement? | `oq-judgement-loss` |
| `OQ-114` | Closed by R127: Does v1 ship order splitting, and how do parts fail? | `oq-split-confirm` |
| `OQ-115` | Closed by R126: Is an ISO 20022 mapping relevant to rutent? | `oq-iso-20022` |
