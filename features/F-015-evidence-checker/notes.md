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
