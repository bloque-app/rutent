# ADR-014: Public checkpoints, and replay at a private node's exit

Non-binding. See `README.md`.

## Context

The owner ruled that checkpoints must be public, and described them as "just succint proofs", not "a real indicator of activity, especially if they're issued in a regular fashion" (R123). He also proposed replaying a private node's recorded transitions when the value leaves onto a public node, as "a complementary proof" (R121). `REQ-AUD-14` and `REQ-ORD-41` state the properties, and this record is about how.

## Options

**For a checkpoint:**
- A signed root of the authenticated log (`ADR-004`), posted at each tick. Cheap, and anyone holding history can check it, given the random value for that tick. It shows the history matches, and nothing about whether the rules were followed.
- A zero-knowledge proof over the same root, that a run of log entries followed the rules. The owner compares this to what `aledgent` does. It proves more, at far more cost.
- Publishing on a public chain, a transparency log, or the operator's own site. A chain or log adds a witness to when, and to there being one version. The operator's site adds neither.

**For hiding activity:** a fixed tick with a fixed size. A log's size is the giveaway in most root formats, so a raw tree size can't go in the checkpoint. An unchanged root would also show an idle period, so each checkpoint has to bind the root to its tick and a fresh random value, and consecutive ones never match.

**For replay:** recompute each recorded inner segment from its stated inputs, rate and fee, then compare the last value with the public movement out.

## What we'd try first, and why

A fixed tick publishing a signed commitment to the root, salted as above, with no size in it, wherever the operator can publish. A transparency log is better, but requiring an outside service pulls against `REQ-OPS-4`. It meets the properties with the least machinery, and a proof over the same root can be added later without changing what readers check. The tick's period is the operator's.

Run replay in the read-only checker from `ADR-013`, so it isn't the provider's module checking itself.

## What would change this

A regulator or customer needing proof that the rules were followed, not just that history is unchanged. Then the proof is worth its cost.

## Limits

A checkpoint commits to what was recorded, and says nothing about what wasn't. A provider can make up inner segments that match its exit (`THR-131`), so a match is only consistency.
