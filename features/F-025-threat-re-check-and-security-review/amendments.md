# F-025 `T-025-04`: drafted amendments for the owner

**Drafts only. Nothing here is applied: `ARCHITECTURE.md` and `SPEC.md` are not edited by this row.** The reading behind
each is in `notes.md`, "T-025-04". Applying them is the owner's call, item `recheck-amendments` on the form. Statuses
are as of `m3`'s `3d9b6a45`.

## A. `ARCHITECTURE.md` §6, "Threat-model revisit": the table, as it should read

Replaces the table's five rows. The sentence "Re-checked by `EXP-2` ... re-check again at `F-025`" in the first row is
replaced by what follows. The paragraph after the table keeps its last sentence out: "Where the experiment ... is still
open, the threat re-check is correspondingly provisional" no longer applies (`EXP-2` closed, `DEC-27`); see A.6.

### A.1 `THR-85`, `THR-18`

Concrete design: unchanged (ADR-0003 as amended by `EXP-2`, `DEC-27`), with the pin now Wasmtime 48.0.9.

Status, replacing the row's third cell:

> **Narrowed; the residual stays accepted (`R72`).** An escape from Wasmtime lands in one provider's jailed process, with no
> network, no file, no other process, no signal to one and no inherited descriptor. Each of the jail's layers is probed
> alone on Linux, against a control without it, and the release build holds no way to turn one off (`T-025-02`, `jail-probes`
> runs `44242098797` and `44243240737`). The runtime is pinned to 48.0.9, read against every advisory published to
> 2026-11-17: none names it, and the six that named 48.0.8 were not reachable with the engine's configuration
> (`T-025-01`, `T-025-06`). A provider's process that stops answering is killed at the operation's deadline, and the outside
> part's fuel is one budget per operation. Going further needs a second flaw, in the kernel, reached through the allowed
> calls. Not closed: unknown flaws and hardware side channels as a class. Spectre settings are Wasmtime's defaults (`G-09`),
> no fuzzer-made binary was admitted (`B-09`), the seccomp allow-list was written from reading and not pruned from a trace,
> and the UTS namespace stops nothing alone (depth only). On Ubuntu 24.04 the jail needs an operator setting, an operator
> obligation (`F-033`). `THR-18`: each part's machine code is digested at admission and checked before load (`B-10`); it was
> identical across two compiler processes and re-derived on an AMD EPYC 9V74 and an EPYC 7763 (`B-11`, `G-04`) and, at
> 48.0.9, on a GenuineIntel CPU against an AuthenticAMD reference (`T-025-06`, `rederive` job `134247456427`). Re-checked at
> `F-025` (`T-025-04`).

### A.2 `THR-132`, `THR-130`

Concrete design: unchanged (ADR-0005). Status:

> **Narrowed, and built (`F-022`)**: a checkpoint is fixed-length, salted and holds no size, so `AC-37`'s bar is met by
> construction and shown by test, not by policy. `THR-132` stays detect-only. Residual: a party's receipt names the log's
> size when it was issued, so a party that asks twice learns how much the log grew between (accepted for v0, `F-022`); a
> checkpoint still only proves the log is self-consistent, never that recorded segments happened (`THR-131`, unchanged,
> accepted).

### A.3 `THR-10`, `THR-135`, `THR-136`

Concrete design: unchanged (ADR-0006). Status:

> **`THR-10` and `THR-136` detect-only, `THR-135` bounds, as `SPEC.md` states each; no new mitigation.** The record is replayable and ties break on the lowest candidate
> id; `v0` sends an order as one part; ceilings per provider and publisher drop a candidate over them. Bulk measures
> exist and orchestration calls them when a segment ends (`T-020-31`); the production assembly owns calling them
> (`F-030`), and a failure is not measured against the market's direction, since no part holds a market figure in `M3`.
> Accepted residual, not a gap this SAD closes.

### A.4 `THR-79`, `THR-80`, `THR-86`

Concrete design: unchanged (ADR-0004). Status:

> **Narrowed, with an accepted concentration risk**: a compromised module can't reach another provider's secrets (a lookup is
> scoped to the caller's declared operation, and a module reaches only a provider its route names, `REQ-MDL-21`), but one
> exploit of the shared proxy reaches all of them (`THR-79`'s unbounded case), accepted by `credential-proxy-scope` while the
> roster is the operator's own; Pablo's mitigation is vault buckets separated by process role. A scan of a full simulated
> run finds no planted credential, signing key or module state in the host's journal, Core's log, the evidence shelf or
> three VOS replicas' files, and finds each when one is let in (`T-025-03`, `stm-vos.yml` runs `44316784467` and `44308204207` at `3d9b6a45`). It does not
> scan production's proxy records (`F-030` re-runs it), a value encoded another way, or balances. `THR-80` is unchanged
> (`OBL-1`). Residual either way: misuse at an allowed host, or data leaving in a request to one.

### A.5 `THR-2`, `THR-66`, `THR-67`

> **Unchanged.** `SPEC.md` accepts these under `R119`; `M2` and `M3` add no control that narrows them (ADR-0011's
> separation of duties for stuck orders requires two actors where one rogue one sufficed, and a compromised operator
> account defeats it).

### A.6 A sixth row and the closing paragraph

New row, for the owner to take or drop:

> | `THR-57` (resource abuse, work that never ends), the name-resolution part | `REQ-MDL-11`: every network step waits at most until the operation's deadline; the lookup through the system resolver runs on a thread of its own and is given up at the deadline (`T-014-05`), at most 4 per provider | **Bounded, with a residual**: `getaddrinfo` takes no timeout, so an abandoned lookup's thread lives until the resolver returns. One provider's hung resolver uses its own four; after them its lookups are refused until one ends. A fix, if chosen, is a new row (item `name-lookup-threads`). |

Closing paragraph, replacing its last sentence: "`EXP-2` closed with `DEC-27`, and `F-025` re-ran this table on 2026-11-18 (`T-025-04`);
no threat in the pass was new."

## B. `SPEC.md` §8

The Coverage table lists threats per area and holds no status; no area gains or loses a threat from this re-check, and the
endings of `THR-85`, `THR-18`, `THR-79`, `THR-80`, `THR-86`, `THR-10`, `THR-135`, `THR-136`, `THR-2`, `THR-66`, `THR-67`
and `THR-132` stand as written. Two threat rows would take a sentence each:

- **`THR-130`**, ending now "Mitigated (prevents): `REQ-AUD-14`", adds: "Checkpoints reveal no size or timing of activity. A party's
  receipt names the log's size when it was issued, so that party can learn growth between two receipts (accepted for v0)."
  Why: "prevents" is true of checkpoints only.
- **`THR-57`**, ending now "Mitigated (bounds): `REQ-MDL-11`, `REQ-CAT-1`", adds: "A name lookup is given up at the deadline; its thread lives
  until the resolver returns, at most four per provider." Why: the row says "calls don't time out", and a lookup was the
  one step the deadline did not bound until `T-014-05`.

Optional, if the owner wants the Coverage table to show the new evidence: in the area "The part outside the sandbox", nothing
changes; in "Sandbox escape, resource abuse, state isolation", `THR-79` could be added, since `T-025-03` is the state-isolation
scan. Not proposed: `THR-79` already has an area, and the table maps areas to threats, not tests to them.
