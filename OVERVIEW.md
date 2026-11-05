---
name: rutent overview
status: informative. Digests SPEC.md v0.4.1, accepted by the owner on 2026-11-05 (R142). SPEC.md decides
date: 2026-11-03
---

# rutent in a page

rutent is a funds and assets router. It moves value between any nodes and assets its catalogue of providers can reach, and taking local money to on-chain balances and back is one outcome of that. It keeps an authenticated log of what happened. It isn't a ledger, and it never holds customer value. Between segments, value rests in each node's holding account: a plain wallet the operator registers at a public node, or the publisher's record of funds at a private one. rutent's one ledger-like duty is checking that each holding account's balance is the sum held there for its orders.

## How it's divided

Three libraries, bound by a core. Each library works alone, without the core or the others, and can be reused elsewhere. The core reaches each one only through its public contract.

| Part | What it does | How it sees an example route |
|---|---|---|
| **Router** | Reads the catalogue's published quotes, and calculates routes and splits for the customer's objective | A graph: `(node, asset)` pairs as vertices, each provider's segment as an edge, and a route as one path from a reserve to a reserve |
| **State machine** | Gives every order and segment exactly one current state, which changes only on a recorded input | An automaton over nodes and assets, where each segment is a transition |
| **Provider system** | Runs the catalogue and providers' modules, so every provider reaches the platform through one common interface | A list of modules, each with a role to comply with |
| **Core** | Binds the three together | The whole order |

Around them:

- **A module** is a provider's published code: signed, hot-loaded and sandboxed. The part that talks to the outside world is split from the sandboxed part, reaches only what it declares, and is the only part that can unseal that provider's secrets. Existing providers are handled separately from rutent, and no component depends on a particular one.
- **Authorisation and policy** are rutent's own and independent. Authentication may be external, and it says who is calling and nothing more. Every action that moves value, posts a route or changes the catalogue passes rutent's policy.
- **The protocol** is what parties share. Its specification is public in v1, and a new kind of operation needs a protocol change.
- **A deployment** stands alone, with no higher-order platform, and humans and agents use one contract. MIT licensed.

Route calculation sits outside the state machine's deterministic part, so a chosen route enters it only as a recorded input. How to store the log, and what runtime to use, are design decisions the specification leaves open.

## How a route is built

A route is an ordered list of typed operations, and each role has one rule:

- **Reserves are endpoints.** Every route starts by authorising a withdrawal from a reserve, and ends by depositing into one. What is behind a reserve (a book, a bank account, a third party, a wallet) is opaque to rutent.
- **Ramps route between networks**, keeping the same asset.
- **Exchanges run inside one network**, converting assets. Each change of asset is its own segment, even one that changes no value.
- **Verifiers** answer eligibility questions, and move no value.

Providers post quotes, and only the operator posts routes. A module may take internal steps, but it complies with the contract of the role it advertises, and it can't change the route. One publisher can advertise several providers and private nodes, which it must declare, and the operator publishes the list of public ones. A new kind of step needs a protocol change.

## How an order goes

1. A user asks to move value, or a reserve that has declared it starts a route on an automatic deposit, with no user present.
2. Routing checks the reserve's yes, asked in the user's session (not on an automatic deposit), and each provider on the shortlisted route checks eligibility.
3. The customer accepts a quote, not a promise. On an automatic deposit, the deposit counts as acceptance. An order may split across several routes, accepted as one quote, and it closes once every part has ended.
4. The route starts when the reserve proves the withdrawal completed. If the withdrawal fails, the reserve cancels it and the order is cancelled, with no effect on later providers beyond a failed commitment.
5. Each segment's outcome carries the best evidence its provider can give, graded and recorded, and the operator sets a floor per tuple. A provider is bound from when its segment starts.
6. Every change of state is notified to the user and the operator, and a floor the customer set is checked at each settled segment.
7. The order closes at its final deposit, on a public record of the movement plus the reserve's signed statement for its book. A rail with no public record, such as a bank, is declared a private node, and the order closes on the public movement into it plus that statement.

## When something goes wrong

- **A segment fails.** The provider truly undoes it at its own cost, back to that segment's origin node. The route is then recalculated automatically, at the user's cost, to the same destination. An order itself is never rolled back: recalculation finds another path. Fees on segments that settled stay paid.
- **Nothing can be proven yet.** Past twice a provider's p99, or twice its advertised time before it has history, the order is stuck. It's also stuck with no route left. The operator resolves it and bears it, and its processing is recorded.
- **The starting segment is undone after the route started.** The order is marked failed, and the origin reserve carries the loss.
- **The final deposit fails.** The order isn't rolled back. It waits for the customer to choose another route from where the value sits, or the value is handed on to a new order starting from that node's holding account. The two orders point to each other.
- **A destination later reports a return.** The order stays complete, and rutent reports it.
- **One part of a split order fails.** That part follows these same rules and ends by itself. The other parts carry on, so an order can end partly delivered.

## Proof

Every action is recorded: who, what, when and outcome. The same recorded inputs always reach the same states. A checker the operator must appoint, and name to customers and providers, can see changed history. rutent also produces public checkpoints on a fixed schedule, which commit to the history and show nothing else, not even how busy the operator is. None of this says anything about what was never recorded, or whether an entry is true.

Inside a private node, a provider's word may be all there is, and it's labelled as such. Where the value then leaves onto a public node, rutent replays the recorded inner segments against the public exit. A match shows consistency, not that they happened.

Erasure never touches the log. It reaches only what a reference names, held outside it, and how long records are kept is the operator's call for its jurisdiction.

## Fees

Each provider's fee is a formula on amount and tuple, with flat and percentage parts, and the operator's own fee is shown apart. The final deposit's provider keeps the operator's fee back, so the operator earns only on success. Each settled segment records the fee captured. rutent keeps no balances of fees.

## The risks accepted

The largest: at a reserve that can't require the user, an intruder at the operator, or a stolen session, can move funds. Others are listed in section 8 of `SPEC.md`, among them agreeing providers, customers relying on the operator's solvency, and false judgements, whose loss falls on the provider that relied on them.

## Where this stands

`SPEC.md` v0.3.3 is accepted, with no open threat or question, and `STEP-1` is done. v0.4.0 amends it after an end-to-end trace, accepted on 2026-11-05. Scheduling charges is the integrator's, out of scope, as is registering a destination before quoting (`INTEGRATORS.md`). It sets requirements, not design, so nothing is built yet. Deferred past v1: open operation, order books, third-party publishers, reliance between unrelated parties under FATF, and an ISO 20022 mapping.

## Where to look

`SPEC.md` decides. `PRODUCT.md` holds the owner's words. `suggestions/` proposes how, and binds nothing.
