# F-015 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-015-01

- The scale is not redefined. `EvidenceGrade`, `EvidencePiece` and `Evidence` (the per-provider declaration) already
  sit in the frozen provider-role contract; `components/evidence` imports them and adds the rules around them.
- "Claim above the declared grade" reads as a claim stronger than declared (a lower number): the declaration is what
  a floor can rely on, so a piece claiming more is refused for the whole outcome. A piece weaker than declared is not
  refused here; the outcome simply grades lower, and whether that clears the operator's floor is the router's call.
- "A statement alone is graded as a statement": a grade 1 or 2 claim counts only when a check backs it, a final
  matching public record (`verify_movement`) for 1, and that or a receipt from a different publisher for 2. An
  unbacked claim is graded 4, never as proof. Grades 3 and 4 need no check. Proof means grade 1 or 2. The outcome's
  grade is its best piece's.
- A receipt from the same publisher as the provider earns no check (`THR-124`): `receipt_check` returns `Unchecked`.
  It is downgraded to a statement rather than refusing the outcome, because the provider's own word still stands.
  Since the PR #60 review, `receipt_check` takes a `VerifiedReceipt`, not two publisher strings (see the review
  section below).
- The network interface carries finality and the reorg rule as declared data (`NetworkRules`), and the verdict on a
  movement as `Status` (unknown, pending, final, reorged out). The checker trusts the network to say when a movement
  is final; interpreting depth or checkpoints is each network's task in `F-031`. `T-015-02` builds the simulated
  network behind `PublicNetwork`.
- Movement-to-segment matching is by network, destination, asset and amount together, with the destination being the
  route's one destination. Amount alone never matches (`DEC-13`). Amounts are compared as the decimal strings the
  contracts use, in the segment's unit.
- One destination per route (`one_destination`): exactly one segment ends at an outer node, and it is the last, a
  deposit. Anything else is reported as a named violation, not panicked on and not guessed around.
- `proptest` is a dev-dependency (`DEC-18`); its lock additions have `licences/RECORD` lines, all permissive.

## T-015-02

- The simulated network is `SimulatedNetwork`, in its own module (`simnet`). It is a hand-driven chain: `record`
  puts a movement in the tip block, `advance` adds blocks, `checkpoint` covers every block to the tip, `reorg(depth)`
  undoes the newest `depth` blocks' movements. It imports nothing but the interface (`DEC-2`).
- Confirmations count the including block as the first, so `Confirmations(n)` is final when the tip is `n - 1`
  blocks past the including block. `Confirmations(0)` is read as `Confirmations(1)`. `Instant` is final on record.
  `Checkpoint` is final once a checkpoint taken at or after the including block's height exists.
- A reorg undoes a movement only when it is not final and the reorg rule allows it: `Never` protects every movement,
  `DepthBound(d)` protects one with more than `d` confirmations, `UntilFinal` protects only final ones. A final
  movement is never undone, whatever depth a reorg asks for. Reorgs do not lower the tip height.
- Recording a movement id the network already holds (not reorged out) is refused, as `T-015-01` reads a movement id
  as unreusable. A reorged-out id may be recorded again, in a new block, because a reorg puts the transaction back in
  the pool.
- A checkpoint covers the movements recorded before it, by order of recording, not by block: a movement recorded
  after a checkpoint, even into the same tip block, is not final until the next checkpoint. A reorged-out id may come
  back only with the movement it first named, since a transaction id fixes its content (`ContentChanged` otherwise).

## T-015-03

- The contracts define no record of a private node's inner segments, so `components/evidence` defines one
  (`replay::RecordedSegment`): asset and amount in, asset and amount out, and the fee in the asset that went in.
  Amounts are canonical decimal integer strings; anything else (`1e3`, a leading zero, a sign) is recorded as
  malformed rather than guessed at.
- Replay checks what can be checked without a rate: each segment takes in what the one before gave out, and where
  the asset stays the same, amount in equals amount out plus the fee. In every segment the fee may not exceed the
  amount in. Where the asset changes, the exchange rate itself stays unchecked: it is the node's own.
- A route across several private nodes is replayed node by node in route order; what one node hands on must be what
  the next records taking in. Only the last node's output is compared with the public movement out and the stated
  outcome. A fault inside a node does not stop the later comparisons: they run on what the record says,
  and every mismatch is recorded (`REQ-ORD-41`: record any mismatch). An empty record is a `NoSegments` mismatch from
  `replay_node`; `replay_route` still reports it as `Unreplayable`.
- The comparison with the public movement reuses `verify_movement` with the replayed value as the expectation, so
  the network alone decides finality (`T-015-01` reading). An unknown, pending or reorged-out exit is recorded as a
  `PublicExit` mismatch: there is no final public movement out to be consistent with.
- A public exit with no node recorded, or a node with no segments, is `Unreplayable`, neither a match nor a
  mismatch: `REQ-ORD-41` makes recording a SHOULD, so its absence is reported, not punished.
- A match is `Replay::Consistent` and produces no `Check`, so it cannot lift a grade; `proves_segments_happened`
  is always false (`THR-131`: a provider can invent segments to fit its exit). A route ending inside the node is
  `Replay::PublishersWord`, whatever was recorded.
- The module is `pub mod replay` and its items are not re-exported at the crate root, to keep `lib.rs` clear of
  `T-015-02`'s edits. One movement evidences one exit; reuse across routes is `distinct_movements`' job at the
  caller.

## PR #60 review (finding 2)

- A receipt check rests on a verified receipt. `receipt_check` takes a `VerifiedReceipt`, whose fields are private:
  only `verify_receipt` builds one, over a `ReceiptVerifier` the caller passes. `verify_receipt` refuses a receipt
  that acknowledges no reference, one the verifier doesn't find signed by its maker for that reference, and one
  whose maker has no publisher in the verifier's catalogue. The publisher-difference rule (`THR-124`) stays in
  `receipt_check`, against the provider's publisher.
- The crate doesn't verify signatures itself. That's a reversible default, for two reasons. `DEC-2` lets it import
  nothing of ours but the contracts, so it can't reuse the provider library's strict Ed25519 verifier, and a second
  copy of that verifier would be a second `DEC-20` signature-checking path. And no contract defines a receipt's
  shape, so the crate would have to invent one. The verifier is the trust anchor, on the same footing as the
  network reader behind grade 1: the `Check` docs say plainly that a check is only as good as the network reader,
  the receipt verifier and the publisher identities its caller supplies. Core implements the verifier over the
  catalogue it accepted and the provider library's verifier. If a receipt statement shape is later added to the
  provider-role contract, the evidence crate can read the maker and reference from the signed bytes.
- A check is bound to what it checked: a public record to its movement, a receipt to the reference it acknowledges
  (`Check::bound_to`). A check backs a piece only when that is the piece's `reference`; otherwise the piece is graded
  a statement, like any unbacked claim. The rule is applied in grading, not in a constructor, because `Claim`'s
  fields are public.
- A receipt backs grade 2 at most: a grade 1 claim on a receipt check is graded a statement, as before. A test and a
  property now pin it.
- The replay tests now pin four more rules, each with a fixture where only that rule can fire: an asset change
  between segments in one node, an asset change at the handoff between two nodes, a stated asset other than the
  replayed one, and the canonical spelling of amounts (`"0990"` and `"+5"`, which Rust's integer parse accepts).
