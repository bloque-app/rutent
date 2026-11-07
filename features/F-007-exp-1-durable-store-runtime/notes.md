# F-007 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`. The
experiment's own questions and defaults are in `experiments/2026-11-06-runtime-host/decisions.json`.

## T-007-02

- **Built before the freeze.** `T-007-02` depends on `T-006-01`, the freeze `C1`, which had not happened. The store
  was built and tested against host contract 0.1.0, and its results recorded as a pre-freeze development run,
  `EXP-1-pb-dev-20261106-1`, in the scoring sheet's section 4. No `pb` cell of the sheet is filled (a development run may fill cells marked pre-freeze, but this one does not): the plan's "done when"
  asks for a run id, and that is the scored run after `C1`.
- **Where it sits.** A module of `rutent-stm`, behind that crate's `purpose-built-store` feature, off by default: the
  only place the boundary check allows it without a change (`exp1-pb-placement`). The flag being off means CI's
  workspace steps don't build it, so CI has its own steps that lint, document and test it with
  `--features rutent-stm/purpose-built-store`.
- **Test names.** The plan's test for `T-007-02` is "the host-contract suite", so the suite tests say they pass it.
  The two tests beyond it take their words from `T-005-03`'s names where those fit: `crash between record and
  effect leaves no effect when the process is killed mid write`, and `a log torn mid record reopens at its last
  whole record`.
- **What the host contract leaves open, and how the store reads it.**
  - Every append is one frame, synced before the next, so only the last frame can be torn. A bad frame is cut as a
    torn tail only when no whole frame follows it and the bytes from it to the end fit in one frame (fewer than a
    header, or no more than the header and the length it declares). Anything more, or a whole frame after it, held
    synced entries: the store refuses to open and cuts nothing.
  - `halt` on a halted host and `resume` on a running one write nothing and succeed, as the reference host's do.
  - A write or sync that fails is undone (`set_len` back to the last synced frame, then `sync_all`) and stops the
    host until it is opened again. If the undo fails too, a marker, `log.unverified`, refuses every open until an
    operator has verified the log and removed it.
  - The store's directory is synced on every open, not only when the log is created, so a crash between creating
    the log and syncing its directory can't leave the name unsynced. The directory's own entry in its parent is
    the caller's.
  - A store whose format entry names other contract versions than the host's is refused at open.
  - Export lists each order's records in the order the orders first appeared, then the refusals, rather than the
    log's interleaving; `StreamCheck` allows either, and the import keeps what it is given.
  - An import's log starts with a format entry that says it starts an import, written and synced as one frame,
    so no crash leaves an import's log that opens as an empty store. A log whose first frame is torn is refused
    rather than cut, since it can't say whether it was an import's start. A crash before the first byte reaches
    the log leaves an empty log, which opens as a fresh store; the import that crashed never returned.
  - Damage after an import's first frame is `ImportIncomplete` only if no import-complete entry lies in the whole
    frames past it. If one does, the import finished and the store was used since, so the damage is corruption
    (`Unavailable`): `ImportIncomplete`'s remedy, redoing the import on a fresh store, would drop every record made
    after it.
  - Damage confined to the last frame, its length intact, is cut as a torn write: at most one acknowledged entry
    can be lost that way. A torn last frame whose length is unreadable is refused, not cut. Both are a gap for
    `EXP-1` to score (the run note's "A gap for `EXP-1` to score"), not defects fixed in `T-007-02`.
  - The lock is advisory `flock`, emulated on NFS; Windows is not supported.

## A flaky store test (2026-11-07)

- **What failed.** Two of sixty runs of the feature tests refused a reopen with "the store is open in another
  host". The cause is in the tests: the real-kill test spawns a child while other tests drop and reopen stores, and
  a spawned child holds copies of the parent's descriptors until it execs. A `flock` belongs to the open file
  description, so a just-dropped store stays locked for that window. The tests now serialise spawns with a gate, and
  `a_process_spawned_beside_a_closing_store_never_keeps_its_lock` fails without it.
- **For `F-010`.** The same window exists in production for any process that spawns children (module workers, for
  one) while it reopens a store. The store fails closed, refusing the reopen, so no record is at risk, but the
  platform must not count on reopening a store while it spawns, or `F-010` must retry such a refusal.

## A finding of the re-score: canonical JSON is not RFC 8785 (T-007-03, 2026-11-07)

- **What the contract says.** The host contract's `exactly-once` guarantee defines equal inputs as "the same
  canonical JSON under RFC 8785 (JCS)" (`contracts/src/state_machine/host/mod.rs:88`, repeated in
  `contracts/state-machine/host/README.md:76` and `contracts/state-machine/host/CHANGELOG.md:30`).
- **What the hosts write.** The VOS program dedups on byte-equal canonical JSON, which it writes as serde_json's
  value model with no whitespace (`experiments/2026-11-06-runtime-host/vos-adapter/program/src/lib.rs`,
  `canonical`); the native client writes the same form, as the first run's adapter did. That is not RFC 8785:
  serde_json sorts object keys by their UTF-8 bytes, where RFC 8785 sorts by UTF-16 code units, and writes numbers
  its own way (`1.0`, `-0.0`, `25000000000.0`, `1e+21`), where RFC 8785 writes ECMAScript's shortest form.
- **The corpus evidence** (`corpus` in `EXP-1-vos-rt1-20261107-2`): 28 of 28 cases byte-equal between the native
  build and the guest, so the two builds agree; 27 of 28 keep RFC 8785's key order, the one that doesn't being keys
  U+E000 and U+1F600, which serde_json orders the other way.
- **Why nothing failed.** Dedup compares a host's bytes with its own, and `pb` and the reference host compare
  `RecordedInput` values, not bytes, so no row depends on the form. The port of `contracts/` to `no_std` (`DEC-4`,
  `DEC-20`) should either give the hosts an RFC 8785 writer or change the guarantee's wording to the canonical form
  they write. `contracts/` is a `DEC-20` path, so this note does not change it.
