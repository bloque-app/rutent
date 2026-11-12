---
name: rutent for integrators
status: informative. Explains what SPEC.md v0.4.0 (accepted) leaves to an integrator; SPEC.md decides
date: 2026-11-05
---

# What an integrator does around rutent

An integrator is whoever builds a product on rutent and files orders for its customers. rutent routes and records; a few steps around an order are the integrator's, by the owner's rulings.

## Register the destination before quoting

A dry-run changes nothing at a provider (`REQ-CAT-14`, R133). So when a destination account has to exist at a provider before it can receive, for example a wallet created on demand for each of a partner's users, the integrator registers it first, then asks for a quote.

1. Create or register the destination where the customer will receive.
2. Ask for a quote. Each reserve's dry-run checks it can deposit there, and rejects a destination it can't (`REQ-ORD-35`), so only routes that can deliver are offered.
3. File the order with that destination. It's resolved when the route is accepted (`REQ-ORD-24`).

A destination registered for a provider that isn't chosen stays with that provider. Register with the providers you expect to use.

## Scheduling is yours

rutent has no schedules, series or mandates (R132). An integrator running charges on a schedule, or under a mandate a customer gave once, keeps the timing, retries and cancellation itself, and files each occurrence as an ordinary order. Batching several payouts into one movement is likewise a construction above rutent, not a feature of it (R88).

## Notices

rutent notifies every change of an order's state (`REQ-ORD-20`). Notices are authenticated, safe to receive twice or out of order, and you can always ask for the current state. Act on them, not on polling.

- Each notice is signed for you alone. Take it only once its signature checks against the deployment's notice key, and once it names this deployment and you as its receiver.
- Each notice of an order carries the order's whole state after a change, with a sequence number. Keep the highest sequence you hold, and ignore any notice with a lower or equal one.
- A late or lost notice below the one you hold changes nothing: the later one already carries the whole state. A lost latest notice shows in no sequence, so whenever you must be sure before acting, ask for the current state.

## When the final deposit fails

The order isn't rolled back (R139, R140). rutent first tries another route to the same destination. If none is left, it waits for your customer to choose another destination, or to accept a new order that starts from where the value sits. Plan for asking your customer.
