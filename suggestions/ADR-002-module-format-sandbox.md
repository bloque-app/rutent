# ADR-002: Portable module format and sandbox

Non-binding. See `README.md`.

## Context

The platform accepts one or more portable binary formats, from any language that compiles to them, and makes the machine code itself (`REQ-MDL-15`, R45). The sandboxed part runs under stated restrictions and limits, with bounded reach (`REQ-MDL-18`, `REQ-MDL-11`, `REQ-MDL-9`). R27's "PVM or WASM" and its special op codes are the owner's example, not a choice.

## Options

**Wasm in-process.** Capabilities are explicit imports; language support varies. A compiler bug is an escape. Wasmtime shows 62 advisories in 2026 to date, 5 Critical. Much of the rise follows LLM-assisted search, so this doesn't rank runtimes.

**Wasm, one process per provider.** Same capabilities, and an escape meets operating-system limits. More memory and IPC.

**Wasm inside a microVM or gVisor.** Two independent boundaries. Highest operating cost.

**PVM.** Runtime crates are Apache-2.0. Derivation is described as bit-reproducible, and nobody has run it for us. Only a Rust guest exists today, against `AC-20`'s two languages. No second layer, and no containment fuzzing was found.

**V8 isolates.** JavaScript and Wasm only. Its own project asks for an outer sandbox with possibly malicious code.

## What we'd try first, and why

Treat format and boundary as two choices. Use one process per provider whichever format wins, because the operators with published designs don't rely on the language boundary alone. Then run one stand-in module per category, from two languages, on both Wasm and PVM, and see which passes `AC-20`. Accept a second format only if it earns its upkeep. Run the compiler in its own isolation, since it reads hostile input.

## Consequences

The operator takes on fast patching (`REQ-OPS-3`). A compiler fix can mean re-deriving the catalogue (`REQ-MDL-2`). Runtime-heavy languages carry their interpreter, and how they fit either limit set is untested. Wasmtime's aarch64 sits in a lower support tier. Whether modules may share a process is unruled.

This doesn't solve an unknown escape or side channel (`THR-85`), timing leaks (`THR-41`), or a module compromised by a hostile response with no escape at all. A signature says who, not whether a module is safe (`THR-87`).

## Requirements served

`REQ-MDL-9`, `REQ-MDL-11`, `REQ-MDL-15`, `REQ-MDL-18`, `REQ-OPS-3`, `AC-20`.

## Sources

- `research/2026-11-02-provider-modules.md`: Wasmtime counts checked, read 2026-11-02. One default-settings Critical applies before v43, so publication date isn't exposure date. The rest are report's tags.
- `research/2026-11-02-vos.md`: PVM facts are report's tags, not run.
- R27, R45.
