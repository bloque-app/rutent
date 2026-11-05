---
name: rutent product specification
status: draft. Owner's words only; every gap is a form item, never filled in by the agent
contract: Narthex SPEC v0.3.0 §4.3
sources: notes/2026-11-02-brief.md (the brief, verbatim); notes/2026-11-02-rulings.md (R1 to R8); notes/2026-11-03-rulings.md (R9 to R128); notes/2026-11-05-rulings.md (R129 to R141); bloque-app/bloque-app.github.io (src/data/projects.ts)
date: 2026-11-05
---

# rutent: product specification

The input to `SPEC.md`. Every requirement there derives from a `PS-*` here, or says what else it derives from
(`REQ-CHN-6`). Quotes are the owner's own. Where this document and the brief disagree, the brief wins, unless a later
ruling says otherwise. Square brackets mark an edited word.

## Problem descriptors

| ID | Element | Content | Status |
|---|---|---|---|
| `PS-1` | What it is | "a funds (or assets) router", "in steroids" (R81). "it's not just "local money to on-chain balances, and back"". That brand line, "Takes local money (Bre-B, PIX, SPEI) to on-chain balances, and back", is one outcome: "the brand line is its outcome" (R14) | Stated |
| `PS-2` | Target users | "humans and agents alike", on one contract; different interfaces only if desirable at plan level (R6) | Stated |
| `PS-3` | Distribution channels | Unknown | Assumed unknown |
| `PS-4` | Cost structure | "Fees are charged by every provider" (R12) | Stated |
| `PS-5` | Revenue structure | Unknown | Assumed unknown |
| `PS-6` | Operating model | "Open protocol specification, closed operation. Providers register with an explicit grant from the operator." (R12) | Stated. In v1: "Yes; published protocol, operator grants entry" (R44) |

## Goals

| ID | Brief | Goal, in the owner's words |
|---|---|---|
| `PS-10` | 1 | "the state machine, the modular provider system, and the quoting engine (rates and graphs) should work as standalone, well-documented components (libraries), and while the core should bind them all together, they should be reused on other systems if needed" |
| `PS-11` | 2 | "Serving thousands of requests" from "humans and agents alike" |
| `PS-12` | 3 | "Be deployed and operated independently from a higher-order platform"; "decoupled from any external system integrating with it" |
| `PS-13` | 3 | "We can keep the authentication layer integrated to an external system, however, as long as our authorisation and policy system remains independent" |
| `PS-14` | 4, R32 | "A common interface that defines a catalogue of providers", in three categories, plus a fourth: verifiers (R32) |
| `PS-15` | 4 | Reserves: "where value is stored, handling deposits, redeems, and exposing balance verifications—whether or not an account has some funds, not how much an account has" |
| `PS-16` | 4 | Exchanges: "where asset types are interchanged, handling asset conversions, and exposing detailed quotes with rates and fees" |
| `PS-17` | 4 | Ramps: "intermediaries to get from reserves to exchanges, handling transfers, and exposing fees" |
| `PS-18` | 4.1 | "hot-load the catalogue of providers as plugins", so that a change to one provider does not reload the service |
| `PS-19` | 4.2 | "Providers' modules may store a shared state and a per-transaction state, where a transaction is one of the methods exposed by the provider and is uniquely identified" |
| `PS-20` | 4.3 | Providers' modules "developed using a wide range of programming languages, be properly compiled, signed and uploaded to our platform so they can execute in a sandboxed environment that has safety restrictions" |
| `PS-21` | 5, R3 | "Every transaction and/or amendment in the state (i.e. provider upgrades) must be properly provable and auditable, meaning we know who did what, when, and the outcome (i.e. advertising a provider, posting a quote, posting a route —a.k.a. graph—, requesting an order, fulfilling an order, etc)" |
| `PS-22` | 5 | "actors interacting within the system" are "provable at compliance level"; "Reserve providers should be able to prove whether the users on which they act on behalf are properly able to enact a set of transactions on our platform" |
| `PS-23` | 6 | "eventually extensible to serve a more open protocol, and advanced features like order books that can be quickly resolved by the exchange providers" |

### Goals added by rulings

| ID | Ruling | Goal, in the owner's words |
|---|---|---|
| `PS-70` | R11 | "Providers must also advertise every possible combination of tuples for origin-destination (a node being a network where multiple reserves might reside) and asset pair (i.e. exchanges contain an from asset and a to asset, ramps keep the same asset, and reserves define which [assets] they accept for deposits or redeems)." |
| `PS-71` | R11 | "Finish by another route, else return it." "A provider must be able to dry-run and retry or return to the origin node otherwise." "Routes might or might not have alternative paths to traverse a given pair of nodes" |
| `PS-72` | R11 | "If a reversal cannot be provided, the provider should decline itself." "Only a true undo counts" (R39) |
| `PS-73` | R11 | "There are edge cases, where an external failure causes the provider not to respond, even if the dry-run worked. In these cases, we should assume the provider will eventually resolve it, and warn the operator to intervene if necessary." |
| `PS-74` | R15 | "a route establishes the plan for funds to get from an origin node (a.k.a. network, book, origin, etc.) and a from asset type, to a destination node at a to asset type which can be the same asset type or not." "we can provide suggested routes", but "it's up to the protocol to calculate the best route (the same way Waze offers a different route depending on congestion or pricing surges)." |
| `PS-75` | R16 | Route objective: "Customer's choice, defaulting to lowest total cost" |
| `PS-76` | R17 | "Each provider advertises the price to route through a segment". "they must commit with fees as fixed, and provide a protocol set slippage for rates for up to three days. A longer than that is free of those bounds". "The provider is responsible with committing to a fixed rate". Holds from segment start: `PS-108` |
| `PS-77` | R12 | "an user provides a proof that funds are available at the origin reserve point." "the pre-routing must ensure that either the main path or any alternative paths guarantee funds arriving and and order being completed with a positive balance." "A check when routing; no promise afterwards" (R41) |
| `PS-78` | R19 | "Every provider must validate the elegibility of a requester beforehand (when constructing the route)." |
| `PS-79` | R9 | Custody: "Never; value stays at providers" |
| `PS-80` | R10 | "Providers post quotes; only the operator posts routes" |
| `PS-81` | R24 | "Provider's calls don't \"time out\". Instead, they must be entirely asynchronous. A stuck order is defined as when the funds take longer than twice the p99 to traverse a segment in the stated provider (we can calculate this outside the main core). The resolution is mostly manual in these cases." |
| `PS-82` | R25 | To join the catalogue: "Identifier or status query required, no exceptions" |
| `PS-83` | R26 | "The part that interacts with the outer world must be split. This includes the route calculation, as it's clearly non-deterministic at a given T point of time" |
| `PS-84` | R27 | "the platform accepts a common binary format (or common formats ...)" "we can verify", "with a signed assertion from the provider. Then, we can manually accept the submission". "while the protocol is semi-permissionless, the provider list remains trusted." Authors: "Our team first; third parties deferred" |
| `PS-85` | R28 | "Modules can interact between them, and can store (and unseal) secrets (we must provide a common trait for this" Plaintext: "Only the outside part sees it" (R37) |
| `PS-86` | R31 | "Protocol is fixed across versions, and providers must abide to them." A stale version: "the segment must be run using the newest version available". Stops advertising: "the resolution is a route recalculation". Protocol bump: "every provider is forced to bump at a given cutover date". "protocol changes are expected to be additive, or backwards compatible." Revoked in flight: "Finish on it unless actively exploited" |
| `PS-87` | R20 | Balance checks: "Limited and logged, thresholds free" |
| `PS-88` | R21, R62 | Who must verify the audit trail: "Regulators, trusting the operator's records". Narrowed by `PS-104` |
| `PS-89` | R36 | "User authorises at origin; providers carry it onward". "a Bre-B personal key or a hot wallet polygon address are considered reserves. These act as origin reserves, and the operator (Bloque in our example) acts on behalf of the user". Another reserve may "always require the user's approval with presence verification" |
| `PS-90` | R36 | "we are not a ledger, just a router. Some providers are considered \"drop\" providers, meaning the funds leave our book (get burned, redeemed, etc.). That's normal." |
| `PS-91` | R39 | "Segments are reversible. Orders are not." "You can consider reserves to be a special kind of ramps: ramps that route from external to internal segments, and vice versa. They are expected to be infallible." |
| `PS-92` | R40 | "segments are failible, and the provider must pay for the reversal. Orders are not failible." "The idea behind going back to the origin node in the segment whenever the provider fails is that the payment can be rerouted through a new provider." The provider "fully refunding", "without manual operators having to intervene at any time something goes south" |
| `PS-93` | R42 | Stuck orders: "Operator resolves and bears the risk" |
| `PS-94` | R43 | A route with a segment over three days: "Whole route has no firm rate; customer accepts first" |
| `PS-95` | R32 to R35 | Verifiers: "Fourth category in the catalogue". What a verifier sees: "Person and class only". Presenting: "User or agent may present; providers may also vouch". Eligibility: "Check only the shortlisted route; recalculate on a no" |
| `PS-96` | R45 | Modules: "Portable binary; platform makes machine code" |
| `PS-98` | R48 | "Declared per reserve, and shown to the user". "Each reserve declares whether it can do it or not. The declaration is recorded once (at provider publishing as metadata)." |
| `PS-99` | R49 | Recalculation: "Automatic. The provider only covers returning those funds to the origin node". "the user covers for the recalculation costs" |
| `PS-100` | R54, R49 | "the disclosure of precalculated pricing is just a quote, not a promise we can guarantee or uphold. The final price may vary, and users accept this or decline to use (routes based on automatic deposits sit as exceptions)." "The final executed pricing is the given." What still binds a provider: `PS-108` |
| `PS-101` | R53, R55 | "there are flat and percentage fees, not just flat fees." "define per-provider fees in a formula so the outcome is known before using the provider." |
| `PS-102` | R50 | One party may hold several roles: "We design for separate entities fulfilling different roles." "Overlap is not necessarily a matter that must be disclosed on the state, but ... recorded separately." |
| `PS-103` | R51 | A reserve demanding a person's presence, for an agent: "Reserve decides; rutent only reports the outcome" |
| `PS-104` | R62 | "No; require a checker outside the operator". "It's never possible to rewrite history". "while not a ledger—but an authenticated log". Who checks: `PS-118` |
| `PS-105` | R56 | A provider that goes silent: "Still owes it; ability to pay checked at entry" |
| `PS-106` | R66 | "Provider, unless it shows customer fault; disputes become stuck" |
| `PS-107` | R63, R64 | An order can end: "Until the first segment settles; never after". "Only in the case where the starting node segment is proven undone, the entire order is marked as failed." "Any failure must be notified as such, to the originating user via a system defined mechanism ... and the operator as well" |
| `PS-108` | R57 | "Provider bound at segment start; route total a quote". "This is the difference between using a metro with a fixed price (but restrictions with the routes and fixed times), and Uber (with unbounded times and routes)." |
| `PS-109` | R58 | "Optional floor; past it, order waits for the customer" |
| `PS-110` | R59 | Automatic-deposit routes: "The deposit itself counts as acceptance" |
| `PS-111` | R60 | A fee formula depends on: "Amount and tuple only" |
| `PS-112` | R61 | Rerouting: "No; only from evidenced value" |
| `PS-113` | R65 | The outside part: "Provider's code, under the module's admission rules" |
| `PS-97` | R47 | No route left after a failed segment: "It becomes stuck; operator resolves and bears it" |
| `PS-114` | R67 | Value arriving with no order: "Outside rutent; the reserve's own matter". "The operator (or the reserve, if properly authorised for) fills out these details." "Orders are always user-initiated, but this counts for a provider acting on behalf." |
| `PS-115` | R68 | "Providers declare evidence lifetime and retry safety" |
| `PS-116` | R69 | An origin undone after delivery: "The origin reserve". "Settling the first segment from origin (always an external node) to the first destination (an internal node) is always responsibility of that provider." |
| `PS-117` | R70 | "An accountable party behind every agent" |
| `PS-118` | R71 | The checker outside the operator: "Whoever the operator appoints, such as an auditor" |
| `PS-119` | R73 | A fee known only after execution: "Formula gives a maximum; the charge stays within it" |
| `PS-120` | R74 | Requester, beneficiary and payer: "All three, as each provider's rules require" |
| `PS-121` | R75 | A destination that reports complete, then returned: "The destination reserve's matter; rutent reports it" |
| `PS-122` | R76 | "The provider applies amount limits itself" |
| `PS-123` | R77 | A balance check: "No; only yes or no". "a reserve is queried in the context of an authenticated (or delegated) session of a user." |
| `PS-124` | R78 | "providers and the operator share these fees". "we must guarantee that providers know whenever they've captured assets as fees. This is important for their accountability." |
| `PS-125` | R80 | A reserve that can't know yet: "It waits until it knows; the route waits too". "With a timeout. If a reserve can't respond within a reasonable time (can't commit to numbers yet), then it must be marked as ineligible due to temporary unavailability, and an alert must be raised" |
| `PS-126` | R81 | "all reserves connect "inner" nodes with "outer" nodes, each named per its own reserve and network", "to record the origin or destination of some funds" |
| `PS-127` | R82 | Every route starting and ending at a reserve: "It's a rule. If the invariant is not to hold value, then reserves are mandatory actors in this system." |
| `PS-128` | R83 | Exiting "at an arbitrary (address provided) wallet": "This last segment is handled by a provider that can route these funds to a final wallet. This provider… is a reserve provider." |
| `PS-129` | R84 | "reserves can only handle withdrawals and deposits" because "these are endpoints in a route. Every route starts by authorising a withdrawal from a reserve. And ends by depositing in a reserve." A route may start and end at one reserve: "a valid (though stupid) route is kreivo to kreivo". "Whether the reserve provider is a book, a bank account, a third-party provider, or a wallet is opaque to us. The important part is whether a provider can serve its function." |
| `PS-130` | R84 | "the same "publisher" can advertise multiple providers and even nodes (for private networks)". A conversion is "idempotent, but still needed to be mentioned" |
| `PS-131` | R85 | "the clear role separation with standard interfaces is a new concept in `rutent`" |
| `PS-132` | R86 | "A module can add internal steps, if deemed necessary." "Each model must comply the contract, which is derived from the role it advertises to do." "Rules are clear: reserves are endpoints, exchanges run inside the same network, and ramps route between networks." |
| `PS-133` | R88 | "several orders can share a single movement, but that's a higher-order construction, not a native feature of the platform per se." "we rely on its word as outcome. Same risk." |
| `PS-134` | R89 | "Segments act as transitions on an NFA. We even have some lambda transitions". "the state machine, given its stated limitations, can be generic as long as its rules are held" |
| `PS-135` | R90 | A step that isn't one of the four operations has no home until the protocol adds it: "DEFINITELY YEAH. I adhere to this trade-off" |
| `PS-136` | R91 | "The reserve's published declaration is enough". "Automatic deposit reserves ... work as a sweepers. Their work is precisely start a route, and that implies authorising the withdrawal without a user presence. Same for instant-payment keys." |
| `PS-137` | R104 | "Anything that happens before a reserve withdrawing is not our matter. A reserve can get a  quote for a new route, and "reserve" it. The route starts at the minute the reserve proves the withdrawal successfully completed. If the withdrawal fails ..., then the provider just cancels it, and the order goes with it, cancelled as well. No effects for further providers, other than a failed commitment, no value." |
| `PS-138` | R100 | "Third-party evidence required, or the reserve can't end a route". "Must be more than just a statement." "even there we should ask for provable evidence, as much as possible." |
| `PS-139` | R101 | Modules calling across providers: "Between providers, when declared by the route." "What the carpool provider does is creating orders on top of the whole platform (now the carpool is the order taker)." |
| `PS-140` | R92 | The checker: "Required; named to customers and providers" |
| `PS-141` | R93 to R95 | Fees: "Ordinary orders; a formula may charge zero". "An operator fee, by formula, shown apart". "A record per settled segment; no balances kept" |
| `PS-142` | R96, R106 | Balance checks: "Only there; nobody asks in the open". Proof of funds: "The reserve's yes, asked in the user's session" |
| `PS-143` | R99 | Slippage: "The operator, per tuple, recorded and shown" |
| `PS-144` | R105 | "Twice advertised time until there's history; stuck recorded" |
| `PS-145` | R107 | "Different per provider; only the operator joins them" |
| `PS-146` | R110 | "Yes; replay reaches the same states" |
| `PS-147` | R113 | Evidence: "Best available, graded; operator sets a floor per tuple" |
| `PS-148` | R114 | "Public record of movement, plus reserve's signed book statement". "providers MUST declare and publish the private nodes in the catalogue; the operator publishes a shared list of public nodes, so we know the difference". Segments within private nodes: "none of the above is guaranteed to be met. In this case, a public record suffices." |
| `PS-149` | R116 | A held quote: "Still a quote until each segment starts" |
| `PS-150` | R117, R118 | A segment within a private node: "movements in and out are enough". A route between accounts at one reserve "is valid because there are two on-chain records (fully public, fully provable)". |
| `PS-151` | R120 | A final deposit on a rail with no public record: "Declared private; public movement in, plus signed statement" |
| `PS-152` | R121 | A reversal inside a private node: "Its publisher's word, labelled as such" |
| `PS-153` | R121 | When funds leave a private node: "we can replay the transitions undergone within the private node and verify the outcome vs stated set of outcomes", "a complementary proof" |
| `PS-154` | R122 | "Log stays; erasure reaches only what references name" |
| `PS-155` | R123 | "Checkpoints must be public": "just succint proofs", "not a real indicator of activity, especially if they're issued in a regular fashion" |
| `PS-156` | R124 | Loss from a false judgement: "The provider that relied on it" |
| `PS-157` | R125, R127 | Splitting an order: "v1 ships it; each part ends by itself". "We'll determine the best algorithm for this problem eventually", and it "should be processable in the component that calculates routes" |
| `PS-158` | R129 | The operator's fee: "Final deposit's provider keeps it back" |
| `PS-159` | R130 | Fees after a later failure: "Stay paid unless that segment was reversed" |
| `PS-160` | R131 | The customer's floor: "Check at each settled segment" |
| `PS-162` | R133 | Dry-runs: "No effects; integrator registers the destination first". "We need to document this properly, not just state in SPEC." |
| `PS-163` | R134 | Notices: "Notify every state change" |
| `PS-164` | R135 | A settlement account as beneficiary: "Accept it; the reserve answers for the checks" |
| `PS-165` | R136, R137 | Undo: "Keep the rule; confirm with providers first". Fiat payment providers "confirmed they can't reverse payments atomically"; "On-chain ramp providers always can, given they're properly funded to cover the fees"; on-chain exchanges "do a no-op in case a conversion is not possible" |
| `PS-166` | R138, R139 | "in normal conditions YOU SHOULD NEVER rollback a routing order. NEVER. That's why we designed a system that can find out alternative paths to go, so funds don't need a reversal." "Stuck orders in this state can be processed by the operator, but the processing must be recorded." |
| `PS-167` | R140 | A failed final deposit: "we can ask the user about an alternative route to send the funds, or even create a new order to return those funds back (considering the node where they sit-in right now as the "reserve"). But never rollback an order." |
| `PS-168` | R141 | "every node should have a holding account". Public ones are "registered by the operator by default", "just plain wallets, built-in outside-code blobs". For private nodes, "the publisher MUST provide means for the operator to interact with the "holding" account". "the platform must provide the means to verify that the given balance of a holding account is the sum of the funds contained for every order. This is the only thing that'll work as close to a ledger." |

## Non-goals

| ID | Brief | Non-goal | Status |
|---|---|---|---|
| `PS-30` | 5 | More privacy than each provider already gives: "protecting the anonymity of the end users is a responsibility of the providers themselves" | Stated |
| `PS-31` | 4 | Disclosing balances: reserves answer "whether or not an account has some funds, not how much" | Stated |
| `PS-33` | R36, R62 | Being a ledger: "we are not a ledger, just a router"; "an authenticated log" | Stated |
| `PS-34` | R67 | Value arriving with no order: "Outside rutent; the reserve's own matter" | Stated |
| `PS-35` | R75 | A delivered payment later returned: "The destination reserve's matter; rutent reports it" | Stated |
| `PS-36` | R78 | Integrators and referrers as parties: "Neither"; "integrators nor referrers share fees" | Stated |
| `PS-37` | R84 | Knowing what is behind a reserve: "a book, a bank account, a third-party provider, or a wallet is opaque to us" | Stated |
| `PS-38` | R88 | Several orders sharing one movement as a native feature: "a higher-order construction, not a native feature of the platform per se" | Stated |
| `PS-39` | R118 | A transfer inside a book: "An internal transfer within a book ... is completely out of our scope." | Stated |
| `PS-161` | R132 | Scheduled charges under a mandate: "No, not in v1". "leave scheduling to be handled by the integrator party in charge of enacting it", "out of scope, and in for the integrator party" | Stated |
| `PS-32` | R102 | Any further non-goals: "Nothing else for now" | Stated |

## Product map

| ID | Element | Content | Status |
|---|---|---|---|
| `PS-40` | Existing Bloque product it replaces | The service this design succeeds | Stated |
| `PS-41` | Bloque Open Source family | aledgent (ledger), bezent (reserve bridge), evmhot (EVM hot wallets) are listed beside rutent. "Depends on whether we want to map `aledgent`. If so, then `bezent` is a ramp, not a reserve, but `aledgent` is a reserve. Otherwise, `bezent` is a reserve." His worked examples are "deployment-level decisions" (R23) | Stated |

## Success measures

| ID | Element | Content | Status |
|---|---|---|---|
| `PS-50` | Scale | "thousands of requests": no number yet. Stays an assumption for v1 (R5, R103) | Assumed |

## Constraints

| ID | Element | Content | Status |
|---|---|---|---|
| `PS-60` | Sector | `oss-ledger`; `reg-fintech` "if regulation demands" (R4, R8) | Stated |
| `PS-61` | Method | No implementation-level detail of the existing system reaches the output | Stated |
| `PS-62` | Separation | No engineering decision at spec level (R2) | Stated |
| `PS-63` | Deadline, budget, platforms | "None known yet" (R13) | Assumed |
| `PS-64` | Licence | "Rutent gets written as open source by default. MIT for the platform in general, then decide per case for the dependencies." (R29) | Stated |
| `PS-65` | Jurisdictions | "Spec stays jurisdiction-neutral for now" (R18) | Stated |
| `PS-66` | VOS | "Whole runtime, as vos-core is heading" (R22) | A suggestion, not a constraint: "Suggestion; SPEC.md stays free of mechanism" (R38) |
| `PS-67` | Existing providers | "we are the first implementors of every existing provider ... but regardless, we should handle it separately from Rutent itself." (R17) | Stated |

## Open questions

Each is a form item: whatever `SPEC.md` raises next.
