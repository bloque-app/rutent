# F-013 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-013-01

- **What moved.** `T-008-02`'s binding, from `experiments/2026-11-07-module-format/wasm-binding/`, into
  `components/prov/src/module/`: the engine configuration (`engine`), the WIT world (`wit/binding.wit`), the
  provider's process and the admitting compiler (`host`, `children`, `wire`, `bin/`), the OS jail (`sandbox`, the
  binding's `rutent-wasm-sandbox` crate folded in) and the platform (`platform`). The experiment's workspace is left
  as it is: it is `EXP-2`'s record, and its workflow still runs it.
- **`DEC-2` splits the binding in two.** The experiment's platform implemented the module-boundary suite's
  `ModuleBinding` and kept the suite's test world (seeded secrets and state, served endpoints) as fields. A
  `rutent-prov` that imports nothing of ours but `contracts/` can do neither. Read as: the platform is generic over a
  `World` trait (`secret`, `state`, `route`, `deliver`, which `F-014` implements with the proxy and the egress layer),
  answers with its own `types` (`Admitted`, `Handled`, `Events`, ...), and the suite's `ModuleBinding` and
  `BoundaryHarness` are implemented in the package's tests (`tests/module_support/`), where `rutent-conformance` is
  a dev-dependency, `DEC-2` allowing that edge. The suite's own types are mapped to and from the platform's there.
- **The guests are a separate package.** `components/prov/module-guests/` is its own Cargo workspace and lock, as the
  experiment's guests were: the Rust and C stand-ins of the boundary suite, the four role stand-ins (below), and the
  tool that componentizes them and writes the C bindings (`wit-component`, `wit-bindgen`), which the platform doesn't
  link. It is not `components/prov/guests/`, which `T-012-01`'s notes leave to `F-016`'s wallet guest. `build.sh` builds
  the stand-ins; the tests run it into the build directory the first time they need them and again when a guest's,
  the WIT world's or a contract's source is newer. `RUTENT_WASM_STANDINS` names a prebuilt directory instead. A host
  without the `wasm32-unknown-unknown` target or a wasm32 clang fails the tests with a message; it does not skip them.
- **The guests' lock is not read by the licence check.** `tools/licence-check` reads the root lock and every lock under
  `experiments/`, so nothing reads `module-guests/Cargo.lock`. It was seeded from the experiment's and pinned the
  same way (`wat` at the recorded 1.262.0), and each of its 59 external packages has a `licences/RECORD` line at its
  locked version, checked by script. Left for whoever owns the check; no `tools/` file was touched.
- **The role suite through the binding (`B-01`).** The suite's `RoleProvider` has an advertisement, `handle`, a books
  fingerprint and a status query, and only `handle` is an operation. The role stand-in of each category is the
  sandboxed part of a module (`module-guests/role/`, `M1`'s `Reserve`, `Exchange`, `Ramp` and `Verifier` compiled to
  wasm32, built when the part starts and kept in the instance), its outside part the module stand-in's. Read as: every
  operation goes through the platform and its answer is compared with a twin of the stand-in, run in process, which
  also supplies the advertisement, the fingerprint and the status query. A guest that answered otherwise than its
  twin fails the test. The run is the first report of a certification (no hooks), but for signatures: the platform's
  are placeholders until `M2`, so `SignatureVerifies` is the one check left deferred. The hooked report (outcome
  unknown, a forced failure, a credited reversal) is not run through the binding: the hooks act on a stand-in's
  next operation, and no operation carries them into a guest. `F-024`'s simulated providers can.
- **A finding of the role suite, fixed.** The first run through the platform flagged every foreign-envelope probe
  (`EnvelopeWellFormed`): the platform copied an operation's deployment and protocol version onto the result it
  passed on, so an operation of another deployment, or of a higher version, got a result that named that one. The
  platform now holds the deployment and protocol version it serves (`types::Envelope`) and names them on every
  result. The result's `made_at` is still the operation's, a placeholder with the signature until `M2` signs.
- **The pin and the advisories (`G-02`).** Wasmtime 48.0.8, tag `v48.0.8`, the latest release of the 48 line
  (crates.io, 2026-10-29). `T-008-02`'s run left `G-02` unchecked. Read on 2026-11-09 from GitHub's advisory API for
  `bytecodealliance/wasmtime`: 62 advisories published in 2026, 5 of them Critical. **None affects 48.0.8.** The
  verdict of each, by its vulnerable range and its patched versions. `wasmtime-wasi` and `wasmtime-wasi-http` are not in
  the closure (the engine's features are `cranelift`, `runtime`, `component-model`, `std`, `anyhow`; no WASI, no
  Winch, no async, no pooling allocator, no GC), so an advisory naming only them is not affected whatever its range.
  A range checked by hand: `GHSA-vc8c-j3xm-xj73`'s open range (`>= 29.0.0`) is the API's, its fixes are 36.0.5,
  40.0.3 and 41.0.1, and it concerns x86-64 AVX code that the explicit baseline target does not emit. `F-025` reads it
  again.

  | Advisory | CVE | Severity | Subject | Verdict for 48.0.8 |
  |---|---|---|---|---|
  | `GHSA-vc8c-j3xm-xj73` | CVE-2026-24116 | medium | Wasmtime segfault or unused out-of-sandbox load with f64.copysign operator ... | not affected: fixed in 41.0.1, so in 48.0.8; also x86-64 AVX only, and the target is the baseline |
  | `GHSA-243v-98vx-264h` | CVE-2026-27572 | medium | Panic adding excessive fields to a wasi:http/types.fields instance | not affected: range ends before the 48 line |
  | `GHSA-xjhv-v822-pf94` | CVE-2026-27195 | medium | Panic when dropping a [Typed]Func::call_async future | not affected: range ends before the 48 line |
  | `GHSA-852m-cvvp-9p4w` | CVE-2026-27204 | medium | Guest-controlled resource exhaustion in WASI implementations | not affected: range ends before the 48 line |
  | `GHSA-394w-hwhg-8vgm` | CVE-2026-35195 | medium | Out-of-bounds write or crash when transcoding component model strings | not affected: range ends before the 48 line |
  | `GHSA-hfr4-7c6c-48w2` | CVE-2026-34983 | low | Use-after-free bug after cloning wasmtime::Linker | not affected: range ends before the 48 line |
  | `GHSA-6wgr-89rj-399p` | CVE-2026-34988 | low | Data leakage between pooling allocator instances | not affected: range ends before the 48 line |
  | `GHSA-m9w2-8782-2946` | CVE-2026-34945 | low | Host data leakage with 64-bit tables and Winch | not affected: range ends before the 48 line |
  | `GHSA-q49f-xg75-m9xw` | CVE-2026-34946 | medium | Host panic when Winch compiler executes table.fill | not affected: range ends before the 48 line |
  | `GHSA-qqfj-4vcm-26hv` | CVE-2026-34944 | medium | Wasmtime segfault or unused out-of-sandbox load with f64x2.splat operator o... | not affected: range ends before the 48 line |
  | `GHSA-f984-pcp8-v2p7` | CVE-2026-35186 | medium | Improperly masked return value from table.grow with Winch compiler backend | not affected: range ends before the 48 line |
  | `GHSA-jxhv-7h78-9775` | CVE-2026-34942 | medium | Panic when transcoding misaligned component model UTF-16 strings | not affected: range ends before the 48 line |
  | `GHSA-m758-wjhj-p3jq` | CVE-2026-34943 | medium | Panic when lifting flags component value | not affected: range ends before the 48 line |
  | `GHSA-jhxm-h53p-jm7w` | CVE-2026-34971 | critical | Miscompiled guest heap access enables sandbox escape on aarch64 Cranelift | not affected: range ends before the 48 line |
  | `GHSA-xx5w-cvp6-jv83` | CVE-2026-34987 | critical | Wasmtime with Winch compiler backend may allow a sandbox-escaping memory ac... | not affected: range ends before the 48 line |
  | `GHSA-hx6p-xpx3-jvvv` | CVE-2026-34941 | medium | Heap OOB read in component model UTF-16 to latin1+utf16 string transcoding | not affected: range ends before the 48 line |
  | `GHSA-p8xm-42r7-89xg` | CVE-2026-44216 | medium | Panic when allocating a table exceeding the size of the host's address space | not affected: range ends before the 48 line |
  | `GHSA-2r75-cxrj-cmph` | CVE-2026-47261 | high | WASI path_open(TRUNCATE) bypasses FilePerms::WRITE host restriction | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-3p27-qvp9-27qf` | CVE-2026-54786 | low | Leak in WASIp1 fd_renumber implementation | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-4ch3-9j33-3pmj` | CVE-2026-58494 | medium | WASI hard links and renames bypass wasmtime-wasi's FilePerms for destination | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-hgjw-h833-99q9` | CVE-2026-115393 | low | Stores can mix up type indices between engines | not affected: range ends before the 48 line |
  | `GHSA-2hw9-mc66-jc2q` | CVE-2026-104855 | low | Preemption and traps during bulk operations enable breaking internal VM state | not affected: range ends before the 48 line |
  | `GHSA-vqjp-4c8c-hfgg` | CVE-2026-115392 | high | Filesystem sandbox escape when paths or symlinks contain trailing slashes | not affected: range ends before the 48 line |
  | `GHSA-x84v-gj2h-g759` | CVE-2026-115391 | medium | Guest controlled-size host heap allocation through WASIp3 streams | not affected: range ends before the 48 line |
  | `GHSA-j2g9-4prp-pf6h` | CVE-2026-115387 | medium | wasmtime-wasi: guest can panic host through filesystem datetime overflow (w... | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-c9gc-w9vx-w86p` | CVE-2026-115388 | medium | wasmtime-wasi-http: outgoing HTTP body write allows guest-driven host memor... | not affected: `wasmtime-wasi-http` is not in the closure |
  | `GHSA-jqpg-j7w6-42pr` | CVE-2026-115389 | low | Dynamic record lifting can allocate beyond the hostcall fuel limit | not affected: fixed in 48.0.3, which 48.0.8 carries |
  | `GHSA-m63x-6p34-q65x` | CVE-2026-115390 | medium | call_ref and exception catch can drop some fuel accounting, leading to expo... | not affected: fixed in 48.0.3, which 48.0.8 carries |
  | `GHSA-96f6-r43r-8c24` | CVE-2026-115386 | low | fd_readdir copies uninitialized struct padding into guest memory | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-hw8m-q44c-ggrf` | CVE-2026-115379 | medium | Rooting for GC values live across try_call may be missing, causing GC heap ... | not affected: fixed in 48.0.4, which 48.0.8 carries |
  | `GHSA-j366-h8gg-77pm` | CVE-2026-115380 | medium | WASI preview 0 implementation of poll_oneoff circumvents fuel consumption | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-gqmc-89g8-p25r` | CVE-2026-115381 | medium | Excessive allocated memory on the host when guests don't have stdio | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-w4qr-p94g-mjhv` | CVE-2026-115382 | medium | Wasmtime wasi:http implementation panics with a zero timeout supplied | not affected: `wasmtime-wasi-http` is not in the closure |
  | `GHSA-cfhf-m2cr-62wj` | CVE-2026-115383 | medium | Mis-typed WebAssembly tag imports can lead to GC heap corruption | not affected: fixed in 48.0.4, which 48.0.8 carries |
  | `GHSA-32h6-97mm-8q3c` | CVE-2026-115384 | critical | Wasmtime component async-lifted callback result count is unvalidated, causi... | not affected: fixed in 48.0.4, which 48.0.8 carries |
  | `GHSA-mr2v-56j5-cmfc` | CVE-2026-115385 | medium | Guest can panic host through filesystem timestamp before the epoch on wasip3 | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-m9f7-9w4p-gv38` | no CVE | high | Filesystem sandbox escape when renaming symlinks with trailing slashes | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-5qpp-g3j2-8frx` | no CVE | low | Guests can send absolute-form HTTP requests to different hosts | not affected: `wasmtime-wasi-http` is not in the closure |
  | `GHSA-w36r-2hch-9xp9` | no CVE | medium | wasi-http: Request with long url before query parameter crashes host | not affected: `wasmtime-wasi-http` is not in the closure |
  | `GHSA-54p9-vhhq-8f7q` | no CVE | medium | Overlapping string transcode in a fused adapter can panic | not affected: range ends before the 48 line |
  | `GHSA-p79w-pv2r-29qp` | no CVE | low | Exception tags can cause host data structures to be unaligned | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-q554-3cjc-rx7p` | no CVE | medium | Component-model handle tables are capped only per instance and charged to n... | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-63h4-6cx9-pjfj` | no CVE | medium | Panic viewing component model stream or future as the wrong type | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-vcg5-q8m5-8379` | no CVE | medium | Wasmtime may not properly enforce async-entrypoint requirement  | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-93g3-36p8-x2gc` | no CVE | medium | Out-of-bounds zero-length stream writes can panic  | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-3r89-v5mq-j5xf` | no CVE | medium | Host panic when component call_async is cancelled under some conditions | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-7v24-42x9-pxf7` | no CVE | medium | Panic on erroneous call-to-host from guest realloc call | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-5hhx-4wrr-3j9g` | no CVE | medium | Yielding in a component start function can lead to a host panic | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-m6w2-jwq2-cv2c` | no CVE | medium | Panic when invoking an async-lifted function via call | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-hmwq-g22x-qjw5` | no CVE | medium | Panic when pending calls are run after a trap | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-wjc8-633q-pwxr` | no CVE | medium | Stack overflow dropping a long chain of recursive types | not affected: fixed in 48.0.6, which 48.0.8 carries |
  | `GHSA-r8xh-cgf7-392j` | no CVE | low | wasi-http: Guests can add conflicting content lengths to messages | not affected: `wasmtime-wasi-http` is not in the closure |
  | `GHSA-jxcf-8qw5-5c33` | no CVE | low | wasi-http p3: forwarded trailers aren't checked against is_forbidden_header... | not affected: `wasmtime-wasi-http` is not in the closure |
  | `GHSA-9prg-84vf-g5fc` | no CVE | medium | wasi-http p3: Guests can set incorrect content-length for spliced response ... | not affected: `wasmtime-wasi-http` is not in the closure |
  | `GHSA-mghf-wjfp-p5xm` | CVE-2026-115893 | critical | Cranelift backend on aarch64 can jump to incorrect target on large Wasm br_... | not affected: fixed in 48.0.7, which 48.0.8 carries |
  | `GHSA-897f-g5gm-fmxm` | CVE-2026-115889 | critical | Bit-packing in Cranelift compiler causes silent overflow and miscompilation | not affected: range ends before the 48 line |
  | `GHSA-9pvw-hwfg-92h5` | CVE-2026-115892 | medium | Panic lowering a host-created component model stream into a guest that expe... | not affected: fixed in 48.0.7, which 48.0.8 carries |
  | `GHSA-hmmr-87qq-258j` | no CVE | medium | wasip3 UDP send can deliver a datagram to an address only allowed for recei... | not affected: `wasmtime-wasi` is not in the closure |
  | `GHSA-qw58-7h65-crq2` | no CVE | low | Cross-engine type confusion with host-created struct-types-with-supertypes | not affected: fixed in 48.0.8, which 48.0.8 carries |
  | `GHSA-3982-jqvv-277m` | no CVE | low | Typed functions with i31 or nullref arguments may panic due to insufficient... | not affected: fixed in 48.0.8, which 48.0.8 carries |
  | `GHSA-6fr9-xmw9-5352` | no CVE | medium | Unchecked memory usage in transferring borrowed resources between components | not affected: fixed in 48.0.8, which 48.0.8 carries |
  | `GHSA-h4wp-g4x3-7398` | no CVE | low | wasi-http p2: outgoing content-length can be invalid or not match body | not affected: `wasmtime-wasi-http` is not in the closure |

- **Not moved.** `rutent-wasm-run`, the experiment's runner that wrote a JSON report for the scoring sheet, needs the
  suite as a library, so it can't be a binary of `rutent-prov`; the tests run the suite per category and assert its
  `Report` instead. `rutent-wasm-guests` went to `module-guests/tool/`. The `pvm-binding` stays in the experiment.
- **What stayed in the binding's tests.** The binding's own tests moved with it (`tests/module_*.rs`), among them
  the two the follow-up table asks `F-013` to keep: an outside part naming another operation than the one in flight
  (`module_egress.rs`) and the undeclared-kind probe, a cross-module call judged by the kind forwarded and not the
  kind the guest names (`module_calls.rs`, `M-10`). They run against the platform through the tests' binding, so they
  hold what the suite does not.
- **Settings kept as they were.** The engine configuration, limits, deadlines and jail are the experiment's, line
  for line: an explicit target per build triple (`G-03`), only the WebAssembly features the sixteen stand-ins need,
  fuel per part per operation, 16 MiB of linear memory per part, `wasm-component` as the format name, and no WASI
  interface (the import allow-list, `B-02`). `G-05` (a provider process's whole memory), `G-09` (Spectre settings),
  `B-09` (fuzzed admission) and the rest of the follow-up table belong to the other rows of this feature and are not
  closed here; `B-11` and `G-04` (another CPU vendor) need hardware this task has not run on.
- **The tests need the jail's namespaces on Linux.** The platform starts each child under `unshare` in user namespaces.
  On Ubuntu 24.04 that needs `kernel.apparmor_restrict_unprivileged_userns=0`, which `.github/workflows/ci.yml` now
  sets on the runner, as the experiment's workflow did, and installs `wasm32-unknown-unknown` and, where the image
  has none that targets wasm32, `clang` for the C stand-ins. `F-030` and `F-033` carry the operator's obligation.
- **A profile override in the root manifest.** Unoptimised, Cranelift compiles a part in seconds and the boundary
  suite took about ten minutes. `Cargo.toml` now builds the compiler and runtime crates optimised in the dev profile,
  twelve `[profile.dev.package.*]` entries. Nothing else in the workspace changes.
- **`Cargo.lock` and the hostile components.** The experiment's admission test wrote its hostile components with
  `wit-component`'s dummy module as a dev-dependency, under a renamed `wit-parser`, which the boundary check refuses
  to read (it fails closed on `package =`, since a rename could hide one of ours). The hostile components are now
  written by the guests' tool (`rutent-wasm-guests hostile`, run by `build.sh`), and `rutent-prov` has no
  dev-dependency on `wit-component`. Every package `rutent-prov` adds to the root lock already has a `licences/RECORD`
  line at its locked version, so none was added.
- **Signing and the DEC-20 click.** The platform signs results with a placeholder, as the suite does. Admission here
  checks the digests the assertion states, not a signature: that is `T-013-04`, with `T-012-02`'s verifier. This task
  touches module admission, a `DEC-20` path, so the click comes with the release into `main`.

## T-013-01, review round and the contracts at 3.0

- **Merged `m2` (2cbbf38).** `module-interface` is 3.0.0 and `provider-role` gained `Received<T>`. The platform now
  uses the contract's `AdmissionRefusal` and `PlatformRefusal` (its own copies are gone; `types` re-exports them),
  and the tests' binding passes them to the suite as they are.
- **Refusal reasons.** A part importing what its kind may not, or in a type the binding doesn't serve, is refused as
  `DisallowedImport(part)`; the compiler marks it (`Compiled::disallowed_import`) and `children::CompileError` carries
  it, since a bare string told the platform nothing. A malformed, oversized or precompiled binary stays
  `UnreadableBinary`. A version already admitted or waiting is `VersionAlreadyAdmitted`, judged before the binaries
  are read, so each release of a test needs a version of its own. `LimitReached` is given when a part ran out of fuel
  or the operation's deadline passed. Reading: a guest that traps by itself, a refused memory growth the guest turns
  into a trap included, is still `Unavailable`; the store limiter does not report a refused growth to the platform.
  `T-013-02` owns limit reporting.
- **A call is delivered only when its callee answered.** A cross-module call that passed the checks is recorded
  `Delivered` if the callee's `handle` gave an answer, and `Unanswered` if not. The guest still sees `refused` for an
  unanswered call: the WIT has two outcomes and the interface is outside the contract (`DEC-4`). `T-013-02` carries
  the test of an unanswered callee.
- **`Received<T>`.** Only the role suite's `RoleProvider` uses it. The through-the-binding test wraps the platform's
  statement with `Received::sent`, the JSON `serde_json` writes for it; the platform signs nothing yet, so there are
  no signed bytes to keep. The role guest returns the body of `statement()`.
- **DEC-20 paths.** The credential proxy's path is touched too: `platform.rs`'s `lookup` and the `World::secret`
  resolver hook decide which secret a provider's outside part is handed.
- **Left as it is.** The result's `made_at` is the operation's, the recorded placeholder (see above).
- **Skipped on the coordinator's word.** The licence check reading every lock in the tree is `T-010-05`'s; it covers
  `module-guests/Cargo.lock`, and `tools/licence-check/` is untouched here.
- **CI.** The `ci` job also checks the guests' format and lints (the host tool, and the stand-ins for wasm32), and no
  longer installs `lld`: the guests link with the toolchain's `rust-lld`.

## T-013-03, output checks (`B-19`)

- **What is checked.** `module/output.rs` judges every output of a module's sandboxed part against the provider-role
  contract's own result schema (`#/$defs/result` of `urn:rutent:provider-role:result`), patterns and lengths
  included, before the platform types it. The platform reads the bytes once into a JSON value, judges that value,
  and types that same value; there is no second reading. A value the schema refuses, an unknown field, an amount with
  a sign or a leading zero, a reference past 512 characters, is `MalformedOutput`; a longer output than the
  binding's limit is `OversizedOutput`, judged first. `result_matches` (kind and transaction of the operation in
  flight) still runs after it.
- **The schemas come in with `include_str!`.** The contract crate builds for `no_std` and carries no schema text, so
  `output.rs` includes the nine files of `contracts/provider-role/schemas/` at build time and registers them under
  their `$id`s; `contracts/` is untouched, so no version bump. `jsonschema` 0.60.2 (already a dev-dependency of the
  workspace and in `licences/RECORD`) is now a dependency of `rutent-prov`. Reading: a validator that can't be built
  fails closed, the output being refused. The provider process and the compiler link it too, which the sandbox does
  not mind (it adds no system call); it is not used there.
- **A reading on the stand-ins' padding.** The boundary suite pads an honest result to exactly the output limit and
  measures what the platform passes on, and the stand-ins padded the evidence reference with `x`; past 512
  characters that breaks the schema, which the new check refuses, so the suite's "output exactly at the limit"
  answer failed. The stand-ins (Rust and C) now carry the padding in extra evidence pieces of at most 512 characters
  (the schema sets no limit on the pieces), sized so the result is exactly as long as asked. `conformance/` is
  untouched.
- **Guest strings.** Nothing a guest wrote reaches a refusal, a log or a report: refusals are the contract's enum, and
  `output::trap_reason` replaces the error text the platform kept for a trap (`Measurements::last_trap`) with
  wasmtime's own words for the trap's kind, since a backtrace carries the names in the guest's binary. The same
  holds for a part's `start` error string, the names of an import the admission refuses, and the component
  parser's message at the compiler: each is now a fixed sentence. Reading: the import's name, which an operator
  might want, is lost from the admission log; the part and the kind of refusal stay. The existing tests read only
  `fuel`, `deadline` and the digest sentence from these texts.
- **Tests.** `tests/module_output.rs` runs through the platform and the Rust stand-ins: the new `Probe::Output` and
  the Rust stand-in's `output set <pointer> <json>` replace one value of the honest result. The C stand-ins have no
  such mode, so the pattern and length cases and the canary run in Rust only; the C stand-ins still pass the
  boundary suite. `tests/module_output_properties.rs` holds the seeded property tests (`DEC-18`).
- **DEC-20 paths.** This task touches module admission (`module/`, a `DEC-20` path); the click comes with the
  release into `main`. `host.rs` and `platform.rs` are shared with `T-013-02`; the edits to them are a few lines
  each, and `T-013-02` carries the reporting of a limit a trap stands for.

## T-013-03, review round

- **Canary tests for the paths the task changed.** `an_imports_name_never_appears_in_the_reason_admission_gives`
  compiles a part importing an interface named with a canary (`hostile-canary.wasm`, written by the guests' tool) and
  checks the reason and the compiler's stderr; `a_start_error_the_guest_wrote_never_appears_in_why_the_release_did_not_start`
  makes the Rust stand-in's start fail with a canary string (`Probe::StartError`) and checks the process's refusal,
  events and stderr. The start test passed on first run, because the first commit had already replaced that text; only
  the import test failed first (it asked for the index). The parser's message at `compiler.rs` has no test of its own:
  a canary there needs a binary the parser quotes, which the tool cannot write.
- **Import index in the admission log.** The reason now reads `import #N is one its part may not name`, N the import's
  place among the part's imports, so an operator can find it without the guest's string.
- **`trap_reason`, small.** A non-`Trap` error keeps its kind for `OutOfMemory` (`resource limit: out of memory`); any
  other is `the part failed`. The host has no host-call error of its own to map (its imports answer, never fail), and
  the store limiter's refused growth reaches the guest as a return value, not an error. `T-013-02` may touch the same
  lines in `output.rs` and `host.rs`; the change is a few lines, to be merged by hand.
## T-013-02

- **What it adds.** `module/limits.rs`: the bounds on what a module's parts may ask of the platform, and the pure
  checks the platform makes against them (`B-14`). Nothing in it runs a part, so the invariants are property-tested
  with a seeded generator (`DEC-18`): no sequence of charges takes more than either budget allows, and a charge that
  doesn't fit is refused whole.
- **Two scopes, read from "per module and per provider".** `REQ-MDL-11` says "limited per module and per provider".
  Read as: a **module's operation** is one operation handed to one provider's module, which a module reached by a
  cross-module call has of its own; a **provider** is bounded across all its operations inside one top-level
  operation (the one the platform handed out and the calls it set off). A provider called eight times in one tree has
  the sum of its operations bounded, not only each one. Both budgets end with the top-level operation: nothing is
  carried from one operation to the next, since the platform has no clock of its own to expire a count by. A rate over
  time is the operator's, `F-014`'s and `F-030`'s. The numbers (`Bounds::DEFAULT`) are this task's choice, settable
  per platform for tests, and sit well above what the stand-ins and the role stand-ins use.
- **What is counted.** Outbound calls and the bytes of their bodies, refused ones included; state reads (a denied
  read of a sandboxed part included) and the bytes of the values returned; outside requests (the pending work a
  sandboxed part leaves with its outside part); cross-module calls that passed the declaration's checks; the depth of
  a chain of operations. Lengths: a state key (256 bytes), a name (128), a host (253), one body and one outside
  message (64 KiB). The platform judges a request where it arrives, before it copies a string into an event, asks the
  world or writes a record, and the provider's reader refuses a frame longer than 4 MiB from its length prefix, before
  it allocates for it (`read_frame_up_to`). The process's own copy of an oversized key, which Wasmtime lifts out of
  guest memory, is bounded by the part's memory limit.
- **What a limit hit does ("a known state").** The operation is refused as `PlatformRefusal::LimitReached`: the
  platform answers `Reply::Limit` to the request that went over and to every later one of that operation (no world
  asked, nothing recorded), the imports trap, the process sets the parts up again in the same process, and the
  platform ends the operation as a limit **from its own state**, whatever the process went on to say. The module stays
  admitted and its process keeps running; other providers are untouched. Every call made before the limit is on record
  and what the endpoint received is what completed. A call over the limit is recorded too, one hop, refused as
  `LimitReached` (`RefusalReason` has it). Each hit is a `LimitEvent` (provider, transaction, which bound and which
  budget), which the suite's events have no place for, so the tests' binding keeps them apart.
- **Gap: the contract has no limit reason for a cross-module call (closed by `T-010-09`).** `CallRefusalReason` was
  `undeclared_callee`, `undeclared_kind`, `not_named_by_route`, `callee_unavailable`, frozen at 3.0.0, so a call
  refused because the caller hit a count or depth bound, and one refused because too little of the caller's
  deadline was left, were both recorded as `callee_unavailable`. A deadline refusal leaves the caller's operation
  going (the guest sees `refused`); a count or depth bound ends it as a limit. Module-interface 4.0.0 (`T-010-09`)
  gives each its own reason, `limit_reached` and `deadline_too_near`, and `module/platform.rs` now records them;
  `callee_unavailable` is only a callee that isn't running.
- **The caller's deadline.** A cross-module call needs `MIN_CALL_BUDGET` (1 second, settable) of the caller's
  deadline left, else the callee isn't started. The number is this task's reading: the callee would be killed at the
  caller's deadline before it answered, and the caller's own answer lost with it.
- **A call is delivered only when its callee answered.** `T-013-01` made it so; this task carries the test of a callee
  that traps and one that answers garbage. Both leave the call `Unanswered` and the callee's received count up by one.
- **Memory limit reporting.** `T-013-01` left a part that was refused memory growth and then trapped as
  `Unavailable`. The store's limiter now records a refused growth, and the process says `limit reached` in its own
  words when the part then traps, which the platform maps to `LimitReached`. The outside part's limits are not
  reported that way: its fuel or memory running out leaves the outside call's outcome `unknown`, as before, and the
  operation goes on, which is a known state of its own.
- **The process's whole memory (`G-05`).** Two settings, both in the settings `T-013-01` kept line for line, so this
  note changes the claim: the engine reserves for a linear memory only what a part may hold
  (`memory_reservation` 16 MiB, no growth reservation, no guard region: Wasmtime's default is 4 GiB per memory, which
  bounds nothing a process may map), so the process's address space is its parts' memory and the runtime's; and on
  Linux the provider's jail sets `RLIMIT_AS` to 4 GiB (the compiler keeps 8 GiB), room for four memories while the
  process builds a part's instances again before it drops the old ones. The probe reports the limit and whether a
  mapping past it was refused. **Not run on Linux here: the dev host is macOS, where `RLIMIT_AS` isn't enforced and
  the probe reports `null`, and the container runtime was down. The Linux assertions, and the 4 GiB being enough for a
  provider under the seccomp and Landlock jail, are checked first by CI.** Explicit bounds checks make a part slower
  than with guard pages; not measured. The cgroup that bounds what a process may use is `F-030`'s.
- **A wall-time deadline on the admitting compiler.** `COMPILE_DEADLINE`, 120 seconds, settable. The exchange with the
  compiler runs on threads and the platform waits for its verdicts until the deadline, then kills it (on Linux the
  module's process first, then `unshare`) and reaps it. A compilation past it is refused as `UnreadableBinary`, as any
  admission with no verdict is, and nothing of it is kept: the same release is admitted again afterwards. Read: the
  contract's `AdmissionRefusal` has no timeout reason either.
- **Restarts and the platform's memory.** The list of children's standard errors was one entry for each child ever
  started, and `measurements.starts`, `compiles` and the overdue-compiler list one entry for each start. The standard
  errors are now the newest 64 (a log with a number for each entry, so a child's thread writing to an entry already
  dropped has its bytes read and dropped), and each measurement list the newest 256. Events are not capped: the
  caller drains them, and dropping an outbound call record would break `REQ-MDL-8`. A quiet child adds no standard
  error entry, so the test of the list drives noisy `/bin/sh` children; the platform test counts the starts and the
  working directories left.
- **Left as it is.** `B-11`, `G-04`, `G-09`, `B-09`, `B-19` and the rest of the follow-up table belong to the other
  rows of the feature. This task touches module admission and the credential proxy's path (`lookup`, `World::secret`),
  both `DEC-20` paths: the click comes with the release into `main`.
- **The platform version changes.** The memory reservation and the absent guard region change the machine code
  the compiler makes, so `BINDING_VERSION` is `0.2.0` (it was `0.1.0`), and `build.platform_version` with it
  (`REQ-MDL-2`): machine code made under the old settings is refused by Wasmtime anyway, and nothing persists across
  the two yet. The WIT world (`0.1.0`, the binding's interface) is unchanged.
- **A provider's frame is capped at 4 MiB.** `Done` carries the
  output hex-encoded, and the platform's reader refuses a longer frame from its prefix. An output over the platform's
  own `set_output_limit` and under 2 MiB is still `OversizedOutput`; one over 2 MiB the process refuses before encoding it, with a fixed sentence the platform gives as `OversizedOutput`; the
  process goes on (answer to review). The output limit in use (the suite's is 4 KiB) is far below it.
- **How the tests were run.** The tests were written first and failed to compile against the missing API, which is the
  red step; the first green attempt then failed on assertions of the tests' own (an event order in the binding's
  helper, a probe that grew a memory twice in one instance). Not run on Linux, so the probe's `RLIMIT_AS` and
  `ENOMEM` assertions, the 4 GiB, and the jail's seccomp list with the new limit are CI's to show.
- **Answer to review.** Secret lookups (of either part) are counted (`Metered::SecretLookups`) and so are cross-module
  calls the platform refuses for what the caller declares, which keep their reason. The provider's frame queue holds one
  frame, so a flooding process is stopped by its full pipe. A refused growth is forgotten when the next operation
  begins.
