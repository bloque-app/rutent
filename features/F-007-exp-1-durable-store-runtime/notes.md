# F-007 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`. The
experiment's own questions and defaults are in `experiments/2026-11-06-runtime-host/decisions.json`.

## T-007-02

- **Built before the freeze.** `T-007-02` depends on `T-006-01`, the freeze `C1`, which had not happened. The store
  was built and tested against host contract 0.1.0, and its results recorded as a pre-freeze development run,
  `DEV-pb-20261106-1`, in the scoring sheet's section 4. No `pb` cell of the sheet is filled: the plan's "done when"
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
