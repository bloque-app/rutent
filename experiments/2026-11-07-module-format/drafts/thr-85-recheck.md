---
status: proposed
experiment: EXP-2
decision_item: exp2-adopt-format (pending)
---

# Draft: the THR-85 re-check

A draft of `EXP-2` (Narthex `REQ-EXP-3`). `DEC-7` has the experiment's outcome re-run the `THR-85` check of
`ARCHITECTURE.md` §6. That row says the sandbox technology's own escape class is unknown until the experiment runs.
This draft holds the re-checked row until promotion; `ARCHITECTURE.md` itself is not edited here. It is written from
the [outcome](../outcome.md) for the recommended option of item `exp2-adopt-format`, Wasm only. [If the owner
chooses PolkaVM](#if-the-owner-chooses-polkavm) says what changes.

## The row as it should now read

| Threat area | Concrete design here | Status after this design |
|---|---|---|
| `THR-85` (sandbox escape), `THR-18` (build tampering) | ADR-0003 as amended by `EXP-2`: Wasm components on Wasmtime 48.0.8, an LTS release, are the one admitted format. Each provider's module runs in its own OS process. That process is the PID-1 child of `unshare`, in new user, network, IPC, UTS, mount and PID namespaces. Before it reads any input it sets `no_new_privs`, pivots into an empty read-only root, drops every capability, applies Landlock with no rule, and installs a seccomp allow-list for x86_64 only. Inherited descriptors are closed before `exec` and again in the jail. The admitting compiler is a fresh process per admission, in the same jail, with 8 GiB of address space and 60 CPU seconds. It refuses precompiled code, and a provider loads only machine code whose digest matches its artefact's | **Narrowed; the residual stays accepted (`R72`).** An escape from Wasmtime now lands in one provider's jailed process, with no network, no file, no other process, no signal to one and no inherited descriptor. Each was probed on the scoring platform (`EXP-2-wasm-20261107-2`). A provider's process that stops answering is killed at the operation's deadline, and the outside part's fuel is one budget per operation. Going further needs a second flaw, in the kernel, reached through the allowed calls. Not closed: unknown flaws and hardware side channels as a class. Spectre settings are Wasmtime's defaults (`G-09`), the pin isn't checked against 2026's advisories (`G-02`), no fuzzer-made binary was admitted (`B-09`), and the jail's layers were probed only together. On Ubuntu 24.04 the jail needs an operator setting, an operator obligation (`F-033`). `THR-18`: each part's machine code is digested at admission and checked before load (`B-10`), and it was identical across two compiler processes. Re-derived on separate hardware (ADR-0014), it was identical on an AMD EPYC 9V74 and an EPYC 7763 (`B-11`, `G-04`, `EXP-2-wasm-20261107-2` and `-3`); no other vendor was tried. Re-check again at `F-025`. |

`SPEC.md` §8's ending for `THR-85`, "Accepted, R72", stands. The design narrows the blast radius and states the
layers, but it does not close the escape class, so no amendment to the Coverage table follows from this re-check.

## What the re-check rests on

### The isolation probes

**Linux, the scored run** (`EXP-2-wasm-20261107-2`, `x86_64-unknown-linux-gnu`, kernel 7.0.0-1017-azure, Landlock
ABI 8). Each child is started as the platform starts it. The platform meanwhile holds a file of `$HOME` open on a
descriptor with close-on-exec cleared, and listens on a Unix-domain socket. Both the admitting compiler and a
provider's process were probed:

| Probe | `rutent-wasm-compiler` | `rutent-wasm-provider` |
|---|---|---|
| Read the file in `$HOME` | refused | refused |
| Signal 0 to the platform's own process | refused | refused |
| TCP connect to `127.0.0.1:9` | refused | refused |
| Connect to the platform's Unix-domain socket | refused | refused |
| Create a file in its working directory | refused | refused |
| Start a process (`/bin/sh -c true`) | refused | refused |
| List other processes (`/proc`) | refused | refused |
| Read the platform's arguments (`/proc/<pid>/cmdline`) | refused | refused |
| Descriptors open above the standard three, the inherited one among them | none | none |
| Landlock ABI applied | 8 | 8 |

**macOS, development runs only** (`EXP-2-wasm-dev-20261107-2`, `aarch64-apple-darwin`). A `sandbox-exec` profile
that denies by default, and denies `process-info*` and `iokit-get-properties` again explicitly. Each of these was
refused for both executables: the `$HOME` read, signal 0 and `proc_pidinfo` on the parent, the IOKit serial number,
the TCP and Unix-domain connects, the file write and `/bin/sh`. No descriptor above the standard three was open.
That profile also allowed every `sysctl` read, which undid its `process-info*` denial. The security review of
`EXP-2-wasm-20261107-1` found a child under it could list 788 processes (`KERN_PROC_ALL`) and read its parent's
arguments (`KERN_PROCARGS2`). The profile now allows two page-size `sysctl`s by name only. Probed again on the
development Mac for `EXP-2-wasm-20261107-2`, both reads are refused for both executables, and both still start and
pass the suite. No status rests on these: macOS is not the scoring platform. The PolkaVM binding's macOS profile could not refuse a
System V object made with `IPC_PRIVATE` (`EXP-2-pvm-dev-20261107-3`). The Wasm macOS probes didn't try one. On
Linux, Wasm's allow-list doesn't admit `shmget`, and each child has its own IPC namespace.

### The layers probed together

The probes test the jail as a whole. A diagnostic `strace` of development Actions run 42357330795 shows seccomp
refusing each attempt first, with `EPERM`. The other layers stand behind it and were not probed one at a time: the
empty root, Landlock, the down loopback of the new network namespace, and the PID namespace. The allow-list was
written from reading what the standard library, glibc and Wasmtime call, not pruned from a trace. It may hold calls
the children never make (`EXP-2-wasm-20261107-2`, Gaps found). Until each layer is probed alone, the defence in
depth is designed but only partly shown (`F-025`).

### The AppArmor operator obligation

Ubuntu 24.04 sets `kernel.apparmor_restrict_unprivileged_userns = 1`. Under it, a user namespace made by an
unconfined process gets no capabilities, and the jail can't be built. Both scored runs set it to 0 for their job
only (`EXP-2-wasm-20261107-2`, `EXP-2-pvm-20261107-1`). It is not part of either binding. An operator running either
format's binding on such a host must do the same, or give the binding's executables an AppArmor profile that allows
user namespaces. It belongs with the operator obligations (`F-033`) and the deployment's host configuration
(`F-030`), not with `CMP-PROV`'s guarantees.

### The inherited-descriptor finding

The review of the first development runs found a gap the macOS profile could not close. A descriptor the platform
held open without close-on-exec reached the child, and the child could read it: a profile never mediates a read of
a descriptor already held (`EXP-2-wasm-dev-20261107-2`, What changed since run 1; `EXP-2-pvm-dev-20261107-3`). Every
child now closes every descriptor from 3 up before `exec`, close-on-exec or not. On Linux this is `close_range(3,
~0)`, done again inside the jail. The scored run's probe holds such a descriptor open and finds none in either
child. The lesson for `THR-85` is that the process boundary holds only if the platform hands the child nothing.

### The trust-binding finding

The review of `EXP-2-pvm-dev-20261107-1` found two flaws, and the Wasm binding was checked for both
(F-008 notes, `T-008-02`):

- **Host calls took the guest's word.** The outside part's secret lookups and outbound calls ran under the
  operation the guest named. Every host call is now judged against the outside request in flight, from the
  platform's own state. A guest naming another operation is refused as `undeclared_secret` or
  `undeclared_operation`, and the record names the operation in flight (Wasm: `tests/egress.rs`; PolkaVM: the
  operation-binding and kind tests, `EXP-2-pvm-dev-20261107-2`). The Wasm check missed cross-module calls: their
  kind was the guest's own string until the security review of `EXP-2-wasm-20261107-1`. A caller declared for one
  kind of a callee could forward the operation in hand to it by naming that kind. The kind is now the forwarded
  operation's, as PolkaVM's is. A guest naming another kind is refused as `undeclared_kind` (`tests/calls.rs`,
  `EXP-2-wasm-20261107-2`). The suite's undeclared-kind probe passed on the flawed code, since its guest names the
  forwarded kind.
- **The digest gate ran after start.** A provider's process now checks both parts' machine-code digests before it
  loads or instantiates either (`G-08`, `B-10`).

This bears on `THR-79` and `THR-86` as much as on `THR-85`. A guest that escapes Wasmtime still has to pass the
platform's checks of what it asks for. The module-boundary suite has no probe of the request-in-flight case: its
specs give each step one operation. So the binding's own test is what holds it, until the suite's next version
carries it (`F-024`).

### The escape class of the sandbox itself

- **The design itself.** Wasmtime keeps the guest in the host process, behind bounds-checked memory, a call stack
  out of the guest's reach and type-checked control flow (README, Sandboxing model). Its security page names no OS
  layer. The binding supplies that layer (`G-07`, pass (binding)).
- **Advisories.** Wasmtime published 62 advisories in 2026, 5 of them Critical. Whether 48.0.8 carries every fix
  that affects it is unchecked (`G-02`).
- **The host surface.** It is kept narrow: no WASI interface is linked, and exactly five WebAssembly features are
  on, each one the stand-ins need (`G-11`).
- **Spectre.** The memory layout and its mitigations are the defaults, not chosen (`G-09`).
- **Hostile input.** The admitting compiler parsed truncated, noisy, corrupted and oversized binaries. It refused
  each while a provider kept answering. No fuzzer-made binary or fuzz corpus was tried (`B-09`).

### Still open

- A guest's `read_state` keys (up to about 8 MiB each) are copied into the host for no fuel and logged in an uncapped `events.state_reads` (`wasm-binding/binding/src/platform.rs:313-341`), so platform memory isn't bounded per operation; `F-013`.

## If the owner chooses PolkaVM

The row's design would name PolkaVM 0.38.0's compiler and Linux sandbox, inside the binding's jail. Its status would
also record what `EXP-2-pvm-20261107-1` found:

- upstream says not to use it in production (`G-13`);
- the worker keeps every capability but `CAP_SYS_ADMIN`, in its own user namespace, because PolkaVM's sandbox needs
  some of them;
- the admitting compiler drops no capability and has no Landlock;
- the sandbox's own layers were not read during a run (`G-20`), and its prebuilt zygote was not rebuilt from source
  (`G-21`);
- a System V `IPC_PRIVATE` segment is allowed in the worker's own IPC namespace;
- for `THR-18`, re-derivation on a second CPU model is shown (`B-11`, `EXP-2-pvm-20261107-1` and `-2`), as for Wasm, on two AMD
  models only.

Choosing both formats would carry both designs and both lists.
