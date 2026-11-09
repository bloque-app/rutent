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

## T-015-03

- The contracts define no record of a private node's inner segments, so `components/evidence` defines one
  (`replay::RecordedSegment`): asset and amount in, asset and amount out, and the fee in the asset that went in.
  Amounts are canonical decimal integer strings; anything else (`1e3`, a leading zero, a sign) is recorded as
  malformed rather than guessed at.
- Replay checks what can be checked without a rate: each segment takes in what the one before gave out, and where
  the asset stays the same, amount in equals amount out plus the fee. Where the asset changes the rate is the node's
  own, so no fee check is made.
- A route across several private nodes is replayed node by node in route order; what one node hands on must be what
  the next records taking in. Only the last node's output is compared with the public movement out and the stated
  outcome. A fault inside a node is recorded alone, because that node's output can't be trusted for later
  comparisons.
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
