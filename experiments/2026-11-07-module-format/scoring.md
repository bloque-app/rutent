# EXP-2 scoring sheet

What `EXP-2` scores each format and language on (`DEC-7`): the module-boundary suite (`T-003-03`, `DEC-5`) against the frozen module-interface contract, what
`DEC-7`, ADR-0003 and ADR-0014 ask of a format binding beyond the suite, each candidate's gaps as its own sources
state them, and the licence question. One row per criterion. Every result cell is empty until a run fills it, and a
filled cell always has its run id beside it (Narthex `REQ-EXP-4`, Narthex `REQ-CHN-17`). Run ids and their notes
follow [the record's rule](README.md#runs). The candidates' and languages' codes are in
[the record](README.md#candidates).

## Legend

| Result | Means |
|---|---|
| `pass` | The criterion holds, by the format and its runtime as pinned, with the binding doing only what the suite or the contract asks of any binding |
| `pass (binding)` | The gap is open in the format or its runtime, and the binding's own code closes it (`T-008-02`'s or `T-008-03`'s), for rutent's use. It is never the format's pass, and the run note names the code that closes it |
| `fail` | The criterion does not hold; the run note says how it failed |
| A measured value, with its unit | A measurement, never a target (`PLAN.md` §6.2, risk 9) |
| Empty | Not yet run |
| `<run id>`, pre-freeze | The cell was filled by a pre-freeze development run (`EXP-2-<candidate code>-dev-<yyyymmdd>-<n>`); the scored run after `C1` replaces it, and no outcome rests on it |

A cell that cannot apply to a format or language is filled by the run that finds it so, with that run's id and the
reason in its note; nothing is marked as not applying in advance.

| Kind | Rows | Source |
|---|---|---|
| Guarantee: the module-boundary suite | `M-01` to `M-16` | The suite's 15 `Check`s and its report, `conformance/src/boundary/` (`T-003-03`) |
| Guarantee: the binding's obligations | `B-01` to `B-19` | `DEC-7`, ADR-0003, ADR-0004, ADR-0014, `REQ-MDL-2`, `REQ-MDL-7`, `REQ-MDL-11`, `REQ-MDL-15`, `REQ-MDL-16`, `REQ-MDL-18`, the frozen module-interface contract, and what the suite leaves uncovered |
| Gap | `G-01` to `G-31` | Wasmtime at `v48.0.8` (`G-01` to `G-12`) and PolkaVM at `v0.38.0` (`G-13` to `G-31`), as [the record](README.md#what-the-candidates-are-from-primary-sources) cites them |
| Licence | `L-01` to `L-05` | `REQ-MOD-5`, `licences/RECORD` |

Columns: `ref` is the suite's in-process reference double; `wasm rs`, `wasm c`, `pvm rs` and `pvm c` are each format
with each language. Each result column has its run column beside it.

## 1. The module-boundary suite

`T-003-03`'s suite, `conformance::boundary` (`conformance/src/boundary/`, `conformance/README.md`, "Module-boundary
suite"), run through `run_boundary_suite` on each binding with the frozen module-interface contract. One row per
`Check`, the suite's 15 (`conformance/src/boundary/mod.rs:368`), in the order the enum gives them, and one row for
the whole report. Each row lists the fixtures and controls (`Exercised`, `:428`) it counts; the 41 of
`Exercised::ALL` (`:563`) are each listed once. `RecordsTellTheTruth` has none of its own: it judges every outbound
call record the other rows' probes produce. The report gives findings by check and `uncovered` by fixture, with no
map between the two: the grouping here is this sheet's reading of the enums' docs and the probe code at `a443665`,
and a change to the suite is mapped again from those lines.

A cell is `pass` when the report holds no finding under that check in that language, and none of the row's
fixtures and controls is in `uncovered` for that language; otherwise `fail`, and the run note copies the findings,
which name a channel, never a value. A fixture can be left uncovered by another row's enforcement: a widening
fixture of `M-14` stays uncovered when a reach control fails, which is `M-05`'s, `M-06`'s or `M-10`'s to flag, so a
run note names the check behind each uncovered fixture. The `ref` column is the suite's reference double (`ReferenceDouble`), whose two
languages are nominal, so it fills one cell per row. The run note records the report's `run_id`, `format`,
`languages` and `output_limit`.

| ID | Check | Fixtures and controls it counts | Passes when | Serves | `ref` | `ref` run | `wasm rs` | `wasm rs` run | `wasm c` | `wasm c` run | `pvm rs` | `pvm rs` run | `pvm c` | `pvm c` run |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `M-01` | `DeclaredReachWorks` | `OwnSecretResolved`, `OwnStateRead`, `DeclaredCallCompleted`, `DeclaredRedirectFollowed`, `DeclaredCallDelivered`, `HonestOutputPassed` | What the module declares works: its own secret, its own state, a declared endpoint, a listed redirect, a declared cross-module call, its honest output | `AC-20`, `REQ-MDL-15` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-02` | `OtherProvidersSecrets` | `ForeignSecretRefusedToOutside`, `ForeignSecretRefusedToSandboxed` | Neither part obtains another provider's secret; its canary appears in no channel | `REQ-MDL-9`, `AC-20`, `DEC-9` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-03` | `OtherProvidersState` | `ForeignStateRefusedToOutside`, `ForeignStateRefusedToSandboxed` | Neither part reads another provider's state; its canary appears in no channel | `REQ-MDL-9`, `AC-20` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-04` | `SandboxedPartHoldsNoSecret` | `SandboxedLookupRefused` | The sandboxed part never reaches the proxy, not even for its own provider's secret | `REQ-MDL-6`, `GUA-5` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-05` | `UndeclaredSecretRefused` | `UndeclaredSecretNameRefused` | A lookup of an undeclared secret name is refused as `undeclared_secret` | `REQ-MDL-6` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-06` | `UndeclaredDestinationRefused` | `UndeclaredDestinationRefused`, `OtherProvidersEndpointRefused` | A call to an endpoint its operation doesn't declare, another provider's included, is refused as `undeclared_destination`, and the endpoint receives nothing | `REQ-MDL-7`, `REQ-MDL-9` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-07` | `RedirectChecked` | `UndeclaredRedirectRefused`, `RedirectChainRefused` | A redirect to an unlisted destination is refused as `redirect_to_undeclared_destination` at its hop, alone or after declared hops | `REQ-MDL-7`, ADR-0004 | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-08` | `ConnectedAddressChecked` | `AddressRefused`, `RedirectToPrivateAddressRefused`, `Ipv6LoopbackRefused`, `Ipv6UniqueLocalRefused`, `Ipv4MappedRefused` | A declared host at an address outside its declared addresses is refused as `address_not_allowed`, directly or after a redirect, over IPv4 or IPv6 | `REQ-MDL-7`, ADR-0004, `THR-79` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-09` | `RecordsTellTheTruth` | None of its own; every outbound call record of the other probes | Every outbound call record is valid under the contract, about its own call, and names only secrets its operation declares | `REQ-MDL-8` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-10` | `UndeclaredCallRefused` | `UndeclaredCallRefused`, `UndeclaredKindRefused` | A cross-module call to an undeclared callee, or of an undeclared kind, is refused and recorded with that reason; the callee receives nothing | `REQ-MDL-21`, `REQ-MDL-9`, `THR-86` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-11` | `OutputShapeChecked` | `UnparseableOutputRefused`, `OutputOfAnotherKindRefused`, `OutputForAnotherTransactionRefused`, `OutputWithAnUnknownFieldRefused` | Output that isn't a result of the operation's shape is refused | `REQ-MDL-12` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-12` | `OutputSizeBounded` | `OutputAtTheLimitPassed`, `OversizedOutputRefused` | Output at the configured limit is passed on, and one byte over it is refused | `REQ-MDL-12`, `THR-57` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-13` | `AdmissionRecordsWhatItAdmits` | `FirstReleaseAdmitted`, `AlteredReleaseRefused` | A release with either part altered is refused as that part altered; a first release's artefact is valid, received as asserted, and waits for a fresh acceptance | `REQ-MDL-1`, `REQ-MDL-2` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-14` | `WideningNeedsFreshAcceptance` | `WideningReleaseHeld`, `PendingReleaseUnreachable`, `NarrowingReleaseRan`, `NarrowedReachRefused`, `WideningBySecretHeld`, `WideningByCalleeHeld`, `WideningByAddressHeld` | A release widening by a destination, a secret, a callee or an address waits for a fresh acceptance and lends no reach until then; a narrowing runs at once and its dropped reach is refused | `AC-21`, `REQ-MDL-3` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-15` | `ProvidersAreIsolated` | `OtherProviderUndisturbed`, `FailedUpgradeKeptThePrevious`, `RestartedProviderAnswered` | One provider's upgrade, failed upgrade or restart leaves the other running in its own process and answering as before; a failed upgrade leaves the previous version in force | `AC-19`, `REQ-MDL-13`, `REQ-MDL-4` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `M-16` | The whole report, `Report::passed()` | All 41, and `missing_languages` | No finding, nothing uncovered, and at least two languages: `missing_languages` is 0. One result per format, in both of its language cells | `AC-20`, `DEC-7` | |  | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |

### 1.1 What the suite leaves to other rows

- **Restarting one provider's process** (`T-008-02`'s test, `REQ-MDL-13`) is in the suite: `M-15`, through
  `RestartedProviderAnswered` and `OtherProviderUndisturbed`. The suite sees processes only through the binding's
  `process` and `restart`, so whether each is a real OS process is scored by `B-07`.
- **`REQ-MDL-11`'s limits other than output**, CPU, memory, outbound calls, state and pending work: `B-03`, `B-04`
  and `B-14`.
- **DNS rebinding and TLS**, which the suite can't express yet: `B-17` and `B-18`.
- **Signatures**, which the suite leaves as placeholders and verifies nowhere: `M2` and `M3` work, not scored here.
- **The output shape's patterns and lengths**, which the suite checks only through the contract's Rust types, which
  don't hold them: `B-19`.

## 2. The binding's obligations

What `DEC-7`, ADR-0003, ADR-0004 and ADR-0014 ask of a format binding that the suite does not test, and what the
suite leaves uncovered ([§1.1](#11-what-the-suite-leaves-to-other-rows)). Each is a property the
platform must have whatever the format, so `pass (binding)` is common here, and expected for the process rows.

### 2.1 Per format and language

| ID | Criterion | How to check | Serves | `wasm rs` | `wasm rs` run | `wasm c` | `wasm c` run | `pvm rs` | `pvm rs` run | `pvm c` | `pvm c` run |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `B-01` | Each provider category's stand-in, reserve, exchange, ramp and verifier, is admitted and answers its role's operations | Admit the four stand-ins; run the role suite (`T-002-03`) through the binding; record each stand-in's binary digest | `DEC-7`, `REQ-MDL-15`, `AC-20` | fail | `EXP-2-wasm-20261107-1` | fail | `EXP-2-wasm-20261107-1` | | | | |
| `B-02` | The sandboxed part has no outside reach: its binary imports only the binding's declared surface, and a binary importing anything else is refused at admission | List each stand-in's imports; admit a sandboxed part importing a clock, a file, a socket or an undeclared host function, and see it refused | `REQ-MDL-16`, `REQ-MDL-6` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | pass (binding) | `EXP-2-pvm-dev-20261107-3`, pre-freeze | pass (binding) | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `B-03` | A guest that hits its CPU limit is stopped, and its transaction is left in a known state | Run a guest that loops; record the limit, how it stopped (fuel, epoch, gas) and the transaction's state | `REQ-MDL-11`, `THR-57` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | | | | |
| `B-04` | A guest that hits its memory limit is stopped or refused the memory, and its transaction is left in a known state | Run a guest that grows its memory past the bound; record the bound and the outcome | `REQ-MDL-11`, `THR-57` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-wasm-20261107-1` | | | | |
| `B-05` | A guest trap, or a host panic a guest triggers, ends at most its own provider's process; the others keep running, and the transaction is in a known state | Trap in a guest; drive a malformed host call; check the other providers' processes and orders | `REQ-MDL-11`, `REQ-MDL-13`, ADR-0003 | pass (binding) | `EXP-2-wasm-20261107-1` | pass (binding) | `EXP-2-wasm-20261107-1` | | | | |
| `B-06` | Measured: portable binary size, compile time, instantiation time and resident memory per instance, for each stand-in | Record each as a measured value with its unit, on the scoring platform | `DEC-7`, measurement only | portable 90,710–90,763 + 84,619 bytes; machine code 265,720 + 248,576–248,584 bytes; compile both parts 261.9–268.1 ms; process start median 5.5–6.0 ms (4.7–7.9 ms) and resident memory median 7,004–7,132 KiB (6,456–7,488 KiB), both languages mixed | `EXP-2-wasm-20261107-1` | portable 23,714–23,718 + 22,031–22,035 bytes; machine code 73,952 + 70,760–70,768 bytes; compile both parts 54.4–55.6 ms; process start and resident memory as `rs` | `EXP-2-wasm-20261107-1` | blobs 9374 to 9388 B sandboxed, 8197 to 8212 B outside; compile ~150 µs, load ~9.2 ms (medians, interpreter, both languages) | `EXP-2-pvm-dev-20261107-3`, pre-freeze | blobs 7932 B sandboxed, 7034 to 7042 B outside; compile ~150 µs, load ~9.2 ms (medians, interpreter, both languages) | `EXP-2-pvm-dev-20261107-3`, pre-freeze |

### 2.2 Per format

| ID | Criterion | How to check | Serves | `wasm` | `wasm` run | `pvm` | `pvm` run |
|---|---|---|---|---|---|---|---|
| `B-07` | Each provider's module runs in its own OS process, and the process `M-15` restarts is a real one | Admit two providers; list their OS processes and check they share none; restart one through the binding and check the other's process id is unchanged | ADR-0003, `DEC-7` | pass (binding) | `EXP-2-wasm-20261107-1` | pass (binding) | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `B-08` | The admitting compiler runs in its own isolation, apart from every provider's process and from the platform | Show the compiler's process and its restrictions (namespaces, seccomp or equivalent); compile `B-09`'s inputs in it | ADR-0003, `DEC-7`, `THR-85` | pass (binding) | `EXP-2-wasm-20261107-1` | pass (binding), macOS only | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `B-09` | A malformed or hostile portable binary is refused at admission, and nothing but the compiler's isolated process is touched | Admit truncated, invalid, oversized and fuzzer-made binaries, the candidate's own fuzz corpora among them; check the platform and the providers keep running | `REQ-MDL-1`, `THR-85` | | | | |
| `B-10` | The admission artefact is filled: `build.platform_version` names the binding and runtime version, and `build.machine_code` is the digest of the machine code the format runs, for each part; the artefact validates under the frozen module-interface contract, unchanged | Write the artefact of a stand-in; validate it with `validate_artefact`; recompute each machine-code digest | `REQ-MDL-2`, `REQ-MDL-15`, `DEC-4` | pass | `EXP-2-wasm-20261107-1` | fail | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `B-11` | Re-deriving the machine code from the admitted binary with the same platform version, on separate hardware, gives the same digest | Compile each stand-in on two machines of the scoring target with different CPU models, the target and flags set as the binding sets them; compare digests | `REQ-MDL-2`, ADR-0014, `DEC-14`, `THR-18` | | | | |
| `B-12` | The format is named in the release assertion by a name the contract's `format` pattern accepts, and the binding needs no change to `contracts/` | Assert a stand-in's release; validate it; list any contract change the binding needed | `DEC-4`, `REQ-MDL-15` | pass | `EXP-2-wasm-20261107-1` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `B-13` | The sandboxed part's safety restrictions are stated, and each is tested | List the restrictions in the run note; point each at the row or test that checks it | `REQ-MDL-18` | pass (binding) | `EXP-2-wasm-20261107-1` | | |
| `B-14` | Outbound calls, state and pending work are limited per module and per provider, and a limit hit leaves the transaction in a known state | Drive each limit; record the bound and the outcome | `REQ-MDL-11`, `THR-57` | fail | `EXP-2-wasm-20261107-1` | | |
| `B-15` | The platform makes the machine code from the portable binary; a publisher's precompiled code is never loaded | Read the binding's admission path; offer a precompiled artefact and see it refused | `REQ-MDL-15`, `THR-18` | pass | `EXP-2-wasm-20261107-1` | | |
| `B-16` | Upkeep, measured: the release cadence, the pin's support window, the advisories published during the experiment, and the time from each to a patched pin | Record each from the candidate's releases and advisories over the experiment | `DEC-7` | | | | |
| `B-17` | A destination's name that resolves to an allowed address when checked and to a disallowed one when connected is refused: the connected address is the one checked | Serve a declared host whose resolution changes between check and connect; see the call refused as `address_not_allowed` and the address receive nothing. The suite can't express this yet | `REQ-MDL-7`, ADR-0004, `THR-79` | | | | |
| `B-18` | A connection is made over TLS to the endpoint its declared host certifies | Serve a declared host with a certificate for another name, and with none; see each call refused and recorded. The suite can't express this yet | `REQ-MDL-7`, ADR-0004, `THR-79` | | | | |
| `B-19` | Output is checked against its operation's schema, patterns and lengths included, not only against the contract's Rust types | Return output that deserialises into the Rust type but breaks a pattern or a `maxLength`; see it refused, as `M-11` refuses the rest | `REQ-MDL-12` | fail | `EXP-2-wasm-20261107-1` | | |

## 3. The candidates' gaps

Every gap, constraint and absence the candidate's own sources state at its pin, restated as a criterion a run can
check. Each is written as what would close it, so `pass` means closed. **Source** cites
[the record's sources](README.md#what-the-candidates-are-from-primary-sources): `Code` read in source, `Doc` the
repository's documents, `Inference` this record's reasoning, `Not found` searched for and absent. Nothing was built
or run in opening the sheet, so every row is a claim to check. **Bears on** names the row or identifier a gap
touches.

### 3.1 Wasmtime, `v48.0.8`

| ID | Criterion | How to check | Source | Bears on | `wasm` | `wasm` run |
|---|---|---|---|---|---|---|
| `G-01` | The scoring platform is a Tier 1 target | Record the target triple: `x86_64-unknown-linux-gnu` is Tier 1; `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin` are Tier 2, missing continuous fuzzing | `docs/stability-tiers.md:21-29`, `:81-86`, `Doc` | `THR-85`, `B-16` | pass | `EXP-2-wasm-20261107-1` |
| `G-02` | The pin carries the fix of every 2026 advisory that affects it | For each of the 62 advisories of 2026 (5 Critical), read its vulnerable ranges and patched versions for `wasmtime` and compare with 48.0.8 | GitHub's advisory API, 2026-11-07, `Doc` | `THR-85`, `B-16` | | |
| `G-03` | Machine code doesn't depend on the compiling machine's CPU: the binding sets the target and its flags explicitly | Read the binding's `Config`; by default the compiler infers CPU features from the host, and `Config::target` turns that off | `crates/wasmtime/src/config.rs:390-404`, `:2930-2940`, `Code` | `B-11`, ADR-0014 | pass (binding) | `EXP-2-wasm-20261107-1` |
| `G-04` | Compilation is deterministic across processes and machines, not only within one engine | The upstream test compiles small modules twice with one engine on one host; `B-11` checks across machines with the stand-ins | `tests/all/module.rs:98`, `Code` | `B-11`, `REQ-MDL-2` | | |
| `G-05` | A module's whole memory is bounded, Wasmtime's own structures and the embedder's allocations included | `ResourceLimiter` bounds memories, tables and instance counts only; record what else bounds the process (a resource limit or a cgroup) | `crates/wasmtime/src/runtime/limits.rs:14-25`, `Code` | `B-04`, `REQ-MDL-11` | fail | `EXP-2-wasm-20261107-1` |
| `G-06` | The CPU limit is deterministic, or its nondeterminism is stated | Record whether the binding uses fuel (deterministic) or epochs (about 10% slowdown, not deterministic) | `docs/examples-interrupting-wasm.md:25-51`, `Doc` | `B-03` | pass | `EXP-2-wasm-20261107-1` |
| `G-07` | Guest execution sits behind a second, OS-level layer | The security page names no process boundary or OS layer; record the binding's | `docs/security.md`, `Not found` | `B-07`, `B-08`, `THR-85` | pass (binding) | `EXP-2-wasm-20261107-1` |
| `G-08` | The binding never deserialises a precompiled module from outside the platform | Read the binding: `Module::deserialize` is unsafe on untrusted input (`research/2026-11-02-provider-modules.md` §1) | Research report §1, `Doc` | `B-15`, `THR-18` | pass (binding) | `EXP-2-wasm-20261107-1` |
| `G-09` | Spectre mitigations are chosen on purpose and recorded | Record the memory configuration and `use_csdb` on aarch64; bounds-check mitigations need dynamic memories, and `csdb` is off by default | `docs/security.md:123-140`, `Doc` | `THR-85` | fail | `EXP-2-wasm-20261107-1` |
| `G-10` | The second language builds a component the pin runs | Build the C stand-ins with `wit-bindgen`'s C generator and a WASI SDK; record versions; run `B-01`. The guide says support "varies across languages" | `language-support.md:4`, `:26`, *(digest and raw)*, `Doc` | `AC-20`, `B-01` | pass | `EXP-2-wasm-20261107-1` |
| `G-11` | The binding enables only the proposals and WASI interfaces it needs, each listed | List every enabled proposal and interface in the run note; the research report sorts most 2026 advisories into the WASI host interfaces and the component model | Research report §1, `Inference` | `THR-85`, `B-02` | pass (binding) | `EXP-2-wasm-20261107-1` |
| `G-12` | The pin is in its support window at every scored run | 48 is an LTS: a multiple of 12, supported 24 months from its release on 2026-08-20 | `docs/stability-release.md:9-12`, `Doc` | `B-16` | pass | `EXP-2-wasm-20261107-1` |

### 3.2 PolkaVM, `v0.38.0`

| ID | Criterion | How to check | Source | Bears on | `pvm` | `pvm` run |
|---|---|---|---|---|---|---|
| `G-13` | Upstream holds the pin fit for production | The README says "Do not use it in production!"; record what it says at the pin and at each scored run | `README.md:5`, `Doc` | `B-16`, `THR-85` | fail | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-14` | The compiler backend and a production sandbox exist on the scoring platform | They exist only on x86_64: Linux, or the experimental generic sandbox on macOS or FreeBSD; elsewhere the VM is interpreted, in-process. Record the backend and sandbox each run used | `crates/polkavm/src/lib.rs:11-19`, `Code` | `B-07`, `B-10`, `THR-85` | fail | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-15` | No experimental sandbox is used | The `generic-sandbox` feature is "EXPERIMENTAL and is not meant for production use"; record the features the binding enables | `crates/polkavm/Cargo.toml:46-49`, `Code` | `THR-85` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-16` | The Linux sandbox's needs are met and stated: unprivileged user namespaces | Record the kernel and the sysctls on the scoring host; the sandbox refuses to start without user namespaces | `crates/polkavm/src/sandbox/linux.rs:324-330`, `:441`, `Code` | `B-07`, operator obligations | | |
| `G-17` | The compiler's CPU needs are met and stated: BMI2 on AMD64, and AVX2 for the full gas cost model | Record the CPU model; module creation fails without them | `crates/polkavm/src/api.rs:448-456`, `Code` | `B-11`, operator obligations | | |
| `G-18` | The machine code the binding runs is available to digest for `build.machine_code` | `Module::machine_code()` returns `None` under the interpreter and is "mostly only useful for debugging"; record how the binding obtains the bytes | `crates/polkavm/src/api.rs:870-888`, `Code` | `B-10`, `DEC-4`, `PLAN.md` §6.2 risk 1 | fail | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-19` | Compilation is deterministic across processes and machines | No upstream test compares machine-code bytes across compilations (the one that compares `machine_code()` compares pointers); `B-11` checks it | `crates/polkavm/src/tests.rs:7573-7578`, `Not found` | `B-11`, `REQ-MDL-2` | | |
| `G-20` | Each layer of the Linux sandbox is active in the worker running a guest: the new namespaces, `pivot_root`, the seccomp filter, `no_new_privs` and the resource limits | Read the worker's `/proc/<pid>/status` and namespaces during a run | `sandbox/linux.rs:324-330`, `:1200-1226`; `polkavm-zygote/src/main.rs:718-801`, `Code` | `B-07`, `THR-85` | | |
| `G-21` | The sandbox's prebuilt zygote binary is rebuilt from source at the pin and matches the bytes the crate embeds | Rebuild `crates/polkavm-zygote` at the pin; compare with the embedded file | `sandbox/linux.rs:799`, `Code` | Supply, `THR-18` | | |
| `G-22` | A second guest language, C, builds and runs | The tree's guests are Rust; `capi/polkavm_guest.h` exists, with no C guest program or test found. Build the C stand-ins with clang to a RISC-V ELF, link them with `polkavm-linker`, run `B-01` | `capi/polkavm_guest.h`, `Code`, `Not found` | `AC-20`, `PLAN.md` §6.2 risk 3 | pass (binding) | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-23` | Each feature the binding relies on is in the code at the pin, not only in the README | The README says the VM "currently only targets the RV32EM", while the linker ships a 64-bit target; cite the code for each feature used | `README.md:34`, `crates/polkavm-linker/targets/`, `Code`, `Doc` | Every row above | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-24` | A guest that runs out of gas stops within the limit, under the metering kind the binding uses | Record `Sync` or `Async`; asynchronous metering may run "slightly longer" | `crates/polkavm/src/config.rs:482-497`, `Code` | `B-03` | | |
| `G-25` | A security policy or advisory channel exists, by which a fix would be known | GitHub's API returned no published advisory; look for a security policy in the repository | GitHub's advisory API, 2026-11-07, `Not found` | `B-16`, `THR-85` | | |
| `G-26` | An admitted binary stays runnable on the next release, or its re-admission path is stated | Pre-1.0; the README's "versioned operational semantics" is a goal under "not all of these are currently true". Run the stand-ins' blobs on the next minor if one appears | `README.md:11`, `Doc`, `Inference` | ADR-0014, `B-16` | | |
| `G-27` | Dynamic paging is off, or its needs are met and stated: Linux 6.7 and unprivileged `userfaultfd` | Record the setting | `crates/polkavm/src/config.rs:377-384`, `Code` | Operator obligations | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-28` | The stand-ins need no floating point or SIMD | Neither is supported; record any stand-in that needs one | `README.md:33`, `Doc` | `AC-20`, `B-01` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-29` | A guest builds with a toolchain the project can pin | Record the toolchain and flags the stand-ins build with; the custom targets suggest a nightly `build-std` | `crates/polkavm-linker/targets/`, `Inference` | `NFR-7`, `B-01` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-30` | No setting that must never be on in production is on: sandboxing enabled, cross-checking off, experimental settings off | Record the engine's settings in each run note | `crates/polkavm/src/config.rs:317-345`, `:429-436`, `Code` | `THR-85` | pass | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `G-31` | The binding's closure takes no dependency outside the permissive licences, or each such one is stated | `polkavm-linker` 0.38.0 depends on MPL-2.0 `option-ext` 0.2.0 through `dirs` 5.0.1 and `dirs-sys` 0.4.1, for one cache-directory lookup in `target_json_path`; the binding patches `dirs` out with its own MIT stub (`pvm-binding/stubs/dirs`). Check that `option-ext` is in neither lock | `polkavm-linker` 0.38.0 `Cargo.toml`, `src/lib.rs` (`target_json_path`), `Code`, `T-008-03` | `L-04`, `L-05`, `REQ-MOD-5` | pass (binding) | `EXP-2-pvm-dev-20261107-3`, pre-freeze |

## 4. The licence question

Each package a binding brings needs the owner's licence decision recorded in `licences/RECORD`, as `REQ-MOD-5`
requires of every dependency, case by case. The licence-record check (`T-001-04`) keeps a dependency with no
recorded decision off `main`. This experiment scores what the decisions are about; the decisions themselves are the
owner's, and reach the owner's form with the adoption item at `T-008-04`. The licences below are stated as their
sources state them, as fact only.

| ID | Criterion | How to check | `wasm` | `wasm` run | `pvm` | `pvm` run |
|---|---|---|---|---|---|---|
| `L-01` | Each crate in the Wasm binding's closure whose only licence expression is `Apache-2.0 WITH LLVM-exception` has the owner's decision in `licences/RECORD` before it reaches `main` | List the closure's licences from `Cargo.lock` and each crate's manifest; `wasmtime`, `cranelift-codegen`, `wasmtime-internal-cranelift` and `wasmtime-wasi` state that expression at `v48.0.8`, and the rest are to be read. At the opening `licences/RECORD` holds no line whose only expression is `Apache-2.0 WITH LLVM-exception`; two lines carry it as one choice of an `OR` (`wasip2`, `wit-bindgen`). Whether the standing permission covers it alone is the owner's | 29 crates state `Apache-2.0 WITH LLVM-exception` as their only expression (`wasmtime`, `wasmtime-environ`, `wasmtime-internal-*`, `cranelift-*`, `pulley-*`, `regalloc2`, `target-lexicon`, all at their locked versions); none has a line in `licences/RECORD` | `EXP-2-wasm-20261107-1` | | |
| `L-02` | Each package the Wasm binding brings into `Cargo.lock` has a line in `licences/RECORD` | Compare `Cargo.lock` with the flag on and off; run the licence check with the flag on | The binding has its own `Cargo.lock` (136 packages) and its Rust stand-ins another (33); the root `Cargo.lock` and `licences/RECORD` are unchanged; 105 and 16 of those packages have no line in `licences/RECORD` | `EXP-2-wasm-20261107-1` | | |
| `L-03` | Each PolkaVM crate's line records the expression its manifest states, `MIT/Apache-2.0`, with the owner's decision | Read each crate's manifest in `Cargo.lock`'s versions; ADR-0003 says Apache-2.0, GitHub's API says Apache-2.0, the README says either at the user's option | | | Facts: `polkavm`, `polkavm-common`, `polkavm-assembler`, `polkavm-linux-raw`, `polkavm-linker`, `polkavm-derive`, `polkavm-derive-impl` and `polkavm-derive-impl-macro` 0.38.0 each state `MIT/Apache-2.0`; `licences/RECORD` has no line for any | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `L-04` | Each package the PVM binding brings into `Cargo.lock` has a line in `licences/RECORD` | As `L-02` | | | Facts: the binding's own `Cargo.lock`, not the root's, holds 45 packages, 4 of them rutent's own, and the guests' 10, 1 of them rutent's own, listed with their licences in the run note; none has a line in `licences/RECORD`; `option-ext` 0.2.0, MPL-2.0, is in neither, patched out (`G-31`) | `EXP-2-pvm-dev-20261107-3`, pre-freeze |
| `L-05` | Each tool that builds the stand-ins (compilers, linkers, binding generators) is named in the run note with its version and licence, and has a line in `licences/RECORD` if it enters `Cargo.lock` | Read the run notes; run the licence check | rustc 1.98.0 (`MIT OR Apache-2.0`); Ubuntu clang 18.1.3 and the toolchain's `rust-lld`, LLD 22.1.8 (`Apache-2.0 WITH LLVM-exception`); `wit-bindgen` and `wit-bindgen-c` 0.61.1, `wit-component` 0.254.2 (`Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`); none enters the root `Cargo.lock` | `EXP-2-wasm-20261107-1` | Facts: rustc `nightly-2026-09-20` (MIT OR Apache-2.0), Homebrew clang 22.1.4 and the nightly's `rust-lld` (LLD 23.1.1; Apache-2.0 WITH LLVM-exception), `polkavm_guest.h` at `471a5b6`; none enters a `Cargo.lock` but `polkavm-linker`, run as `rutent-pvm-tool` | `EXP-2-pvm-dev-20261107-3`, pre-freeze |

## 5. Pre-freeze development runs (not scored)

Runs made before the contract freeze `C1` (`T-006-01`), against the contracts as they stood, to develop a binding.
Each has a development id, `EXP-2-<candidate code>-dev-<yyyymmdd>-<n>` ([README, Runs](README.md#runs)), so it can't
be taken for a scored run, and its note is in `runs/` like any other. A development run may fill cells above, each
marked `pre-freeze`; the scored run after `C1` replaces every such cell, and no outcome rests on one. This section
lists every development run, whether or not it filled cells.

| Run | Candidate | Module-interface contract | What ran | Result | Note |
|---|---|---|---|---|---|
| [`EXP-2-pvm-dev-20261107-1`](runs/EXP-2-pvm-dev-20261107-1.md) | `pvm`, interpreted, on `aarch64-apple-darwin` | 1.0.0, frozen at `C1`; a development run because it is off the scoring platform | The module-boundary suite on the PolkaVM binding, once per category (reserve, exchange, ramp, verifier), in `rs` and `c`; admission checks | Every report passed: no finding, nothing uncovered, `missing_languages` 0, in all four categories; every hostile part refused. Superseded by `-2`: its worker and admitting compiler ran under a `sandbox-exec` profile that allowed by default, so its `B-08` was wrong, and it bound no host call to the request in flight | [run note](runs/EXP-2-pvm-dev-20261107-1.md) |
| [`EXP-2-pvm-dev-20261107-2`](runs/EXP-2-pvm-dev-20261107-2.md) | `pvm`, interpreted, on `aarch64-apple-darwin` | 1.0.0, frozen at `C1`; a development run because it is off the scoring platform | As `-1`, after review: a deny-by-default `sandbox-exec` profile and its probes; every host call bound to the outside request in flight, with hostile tests of the operation and the kind; the digest gate before instantiation | Every report passed in all four categories, `rs` and `c`; every hostile part refused; the isolation probes, the operation-binding test and the kind test passed. The Linux isolation has not run. Superseded by `-3`: its profile left other processes' information and IOKit properties readable, and inherited descriptors open | [run note](runs/EXP-2-pvm-dev-20261107-2.md) |
| [`EXP-2-pvm-dev-20261107-3`](runs/EXP-2-pvm-dev-20261107-3.md) | `pvm`, interpreted, on `aarch64-apple-darwin` | 1.0.0, frozen at `C1`; a development run because it is off the scoring platform | As `-2`, after its review: process information and IOKit properties denied; inherited descriptors closed before exec; each child's standard error its own pipe, kept capped and labelled; a waiting release kept apart from the running one, and a version admitted once; the C guest header vendored | Every report passed in all four categories, `rs` and `c`; every hostile part refused; the isolation probes, the operation-binding, kind and waiting-release tests passed. The Linux isolation has not run | [run note](runs/EXP-2-pvm-dev-20261107-3.md) |
| `EXP-2-wasm-dev-20261107-1` | `wasm` | 1.0.0, frozen; run before the scoring platform, so a development run (see the note) | The module-boundary suite once per category's stand-ins, in `rs` and `c`, on `aarch64-apple-darwin`; the binding's own 18 tests | Every report `passed()`: no finding, nothing uncovered, `missing_languages` 0; `M-01` to `M-16` `pass` in both languages. Its cells now name run 2 | [`runs/EXP-2-wasm-dev-20261107-1.md`](runs/EXP-2-wasm-dev-20261107-1.md) |
| `EXP-2-wasm-dev-20261107-2` | `wasm` | 1.0.0, frozen; off the scoring platform, so a development run (see the note) | As run 1, after the review of `7af940e`: inherited descriptors closed, the exact feature set, `denied` naming no operation, process information and IOKit denied, re-admission refused; the binding's own 19 tests | Every report `passed()`; `M-01` to `M-16` `pass` in both languages. Its cells now name the scored run, `EXP-2-wasm-20261107-1` ([run note](runs/EXP-2-wasm-20261107-1.md)) | [`runs/EXP-2-wasm-dev-20261107-2.md`](runs/EXP-2-wasm-dev-20261107-2.md) |
