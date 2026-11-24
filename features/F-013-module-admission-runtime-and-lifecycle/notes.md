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

## T-013-04, admission

- **What admission checks, in order.** `Platform::admit(received, binaries, judge, at)` takes the assertion as the
  bytes it arrived in, never a parsed statement, and admits what those bytes say. `module/admission.rs::read` parses
  them, requires a signing input (`canonicalize`), the release-assertion shape and the platform's own deployment and
  protocol version (`check_context`). The judge then verifies the signature and keys; then the binaries against the
  digests the assertion states (`PartAltered`), their format and size (`UnreadableBinary`), a version admitted or a
  release waiting (`VersionAlreadyAdmitted`), and the contract's `validate_artefact` and `validate_release` against
  the release it follows (an assertion signed with a statement key an earlier release declared, `THR-4`). Only then
  does the admitting compiler run. Once it has, the declared statement key is recorded with the judge.
- **The judge.** `module/admission.rs` has a `ReleaseJudge` trait and its production judge, `Catalogue`:
  `Catalogue::verify_statement` (context, the key listed for advertising by the last accepted advertisement and by
  the one in force at `made_at`, strict Ed25519 over the signing input) and `validate_assertion_key` against
  `Catalogue::accepted_advertisement` of the releasing provider, never an advertisement that arrives with the
  release. `Catalogue::record_statement_key` is called for each admitted release. The platform holds no judge, and
  every admission names one; there is no default and no way to skip it. `catalogue/` is untouched.
- **Nested statements (F-012's note).** A release assertion carries no statement nested in it, so the context check
  the note asks for is the assertion's own: `read` makes it against the platform's envelope and `verify_statement`
  against the catalogue's, so a catalogue and a platform told different deployments admit nothing.
- **The assertion's digest is of the bytes as received.** It was a re-serialisation of the parsed statement before,
  which the contract's "as received" does not allow; the test's first run failed on exactly this.
- **Acceptance bound to the digest.** `Platform::accept(provider, assertion_digest, by)` accepts only the waiting
  release whose `assertion_digest` is the one given; another digest, or a `by` that is no identifier, accepts nothing
  and leaves the waiting release and the running version as they were. `Platform::pending(provider)` shows what waits.
  The suite's `ModuleBinding::accept` names a version, so the tests' binding looks the digest up there.
- **A new admission while one waits (reading).** Refused as `VersionAlreadyAdmitted` under any version, before the
  compiler runs. The contract has no closer reason (its doc speaks of "this version"); the brief names this one.
  Item for `T-010-09`, whose row is the refusal reasons `M2`'s tasks found missing: an `AdmissionPending` reason. There is also no way yet to
  withdraw a waiting release, so a release nobody accepts holds its provider's admissions; declining one is left to
  `T-013-06`'s lifecycle.
- **An unverified assertion (reading).** The contract's `AdmissionRefusal` has no reason for it, and stretching
  `UnreadableBinary` or `PartAltered` would mislead. The platform's own `Admitted` gains `Unverified(Unverified)`,
  which says which check failed (unreadable, no signing input, wrong shape or context, the catalogue's
  `StatementRefusal`, no advertisement, the contract's rule violations, the statement key). Item for `T-010-09`
  too: an `AssertionUnverified` reason.
- **The module-boundary and role suites run with a judge that trusts every assertion (reading).** The suite signs its
  assertions with placeholders and declares `placeholder_key(1)` as the statement key, which is no point of the curve,
  so no advertisement the catalogue accepts can list it, and re-keying the assertion would break the suite's check
  that the artefact holds the assertion submitted. The tests' binding (`tests/module_support/`) passes
  `TrustsEveryAssertion`, which exists only there, and serialises the suite's statement to bytes; signed admission is
  tested against the catalogue in `module_admission_signed.rs`. Item for `conformance/`'s owner: sign the suite's
  assertions with real keys and give it a signature verifier, as the role suite has.
- **Re-judged at acceptance** since the review round below.
- **The platform build version with the machine code** was already recorded (`Build`, `T-013-01`); the artefact's
  build is now filled in after compiling, the rest of the artefact before, so the judge and the contract's rules see
  it as it will be recorded.
- **Tests.** `tests/module_admission_signed.rs`: `release_with_an_unverified_assertion_is_refused` (an altered
  assertion, the statements key, another deployment, an unknown provider, another provider as maker, a retired
  advertising key, each refused before compiling), `acceptance_of_another_assertion_digest_does_not_admit_the_release`,
  `admission_while_one_is_pending_is_refused`, `widened_release_needs_fresh_acceptance`.
  `tests/module_admission_signed_properties.rs` (`DEC-18`, seeded, 400 cases each, none compiling per case): one
  changed byte of a signed assertion is never admitted nor compiled; no digest but the admitted assertion's accepts
  it; while a release waits no other release of the provider is admitted or compiled. The catalogue's keys and
  advertisements are `tests/common`'s, through `tests/module_support/signed.rs`. "Not compiled" is read from
  `measurements.compiles`, which counts compilations that succeeded; the binaries are honest stand-ins, so any that
  ran would be counted. Test-first: all failed against a
  stub of the new API (unverified admitted, another version admitted while one waited, the digest re-serialised,
  any digest accepted), but `widened_release_needs_fresh_acceptance`, which passed at once because `T-013-01` built
  the widening rule.
- **An existing test changed.** `a_part_importing_anything_beyond_its_parts_interface_is_refused_at_admission` now
  accepts its control release before admitting the hostile ones, which the new rule would otherwise refuse while the
  control waits.
- **DEC-20 paths.** Module admission and signature checking: `module/admission.rs`, `module/platform.rs`'s `admit`,
  `accept` and `pending`, and admission's calls into `Catalogue::verify_statement` and
  `Catalogue::record_statement_key`. `platform.rs` is shared with `T-013-02`: the edits are the imports, `admit`
  around the unchanged `compile` match, `accept` and the new `pending`; `host.rs` is untouched.

## T-013-04, review round

- **Acceptance judges the assertion again.** `Platform::accept(provider, assertion_digest, by, judge, at)` keeps the
  waiting release's assertion as received and runs `judge` on it once more. A release signed with a key retired
  since its admission, as a provider retires a key it believes stolen, is no longer accepted, and stays waiting:
  `acceptance_after_the_signing_keys_rotation_is_refused` (failed first, the stub accepting it). A refused release
  still holds its provider's admissions until it is withdrawn, which `T-013-06` gains.
- **Exact reasons, and the unsigned cases.** `release_with_an_unverified_assertion_is_refused` now names each case's
  `Unverified` variant, and adds an assertion whose signature value is no signature (64 zero bytes:
  `BadSignature(DoesNotVerify)`) and one with no signature at all (`Unreadable`). These passed on first run: the
  admission code already gave these reasons; the test was only loose.
- **The statement key after compiling.** `admit`'s documentation now says that the judge's refusal to record the
  statement key is the one refusal, beside the compiler's, left after compiling, and why the catalogue's can't
  happen once its judgement passed: the accepted advertisement lists the key for statements, and `join` and
  `trust_advertising_key` keep a statements key from ever becoming an advertising key.
- **`ReleaseJudge` is public and unsealed.** Any crate can write a judge, as the tests' trusting one does. Once
  `conformance/` signs its assertions with real keys and the trusting judge is gone, `T-010-10` seals the trait, or
  makes `admit` and `accept` take the catalogue itself. Not changed here.

## T-010-10: the trusting judge is gone

- **`TrustsEveryAssertion` is removed.** The test binding (`tests/module_support/`) now holds a `Catalogue`: it
  grants each provider its advertisement's tuples, trusts the key the advertisement is signed with, and joins the
  advertisement the boundary suite hands it (`BoundaryHarness::list_provider`), then admits and accepts through
  `Platform::admit` and `Platform::accept` with that catalogue. An `Admitted::Unverified` is reported to the suite as
  `AdmissionRefusal::AssertionUnverified`, which `T-010-09` added; the platform's own `Unverified` stays richer.
- **`ReleaseJudge` is sealed** (`admission.rs`): a private-module supertrait, implemented only for `Catalogue`, with a
  `compile_fail` doctest that a crate's own judge doesn't compile. `admit` and `accept` keep their signatures, as
  `platform.rs` is shared with `T-013-02`. The tests that admit without the suite (`install`, `module_egress`,
  `module_admission` and the rest) get the provider's standard signed advertisement when they have not listed one
  themselves, and sign their assertions with its standard advertising key.
- **Declarations.** `declaration()` in the support module names the provider's real statements key, and
  `module_egress.rs` its own declaration likewise; `egress_properties.rs` and `egress_support` keep placeholder keys
  since they never admit.
- **Role modules through the platform** (`module_role.rs`) list the stand-in's advertisement, signed for real
  (`Signing`), and sign the release with the advertising key it lists. Results the platform sends are still
  placeholders; see F-010's notes for the reading.
- **Signed admission is still tested apart** (`module_admission_signed.rs`, with its own catalogue and keys); the
  boundary suite's run now exercises the same verifier for every release, including a forged one.

## T-013-06, the release lifecycle

- **Where it lives.** `module/lifecycle.rs` is pure: which version is in force, the release staged beside it, the
  segments in flight and the version each runs on, the operator's marks and every decision with its actor. It
  starts no process. `platform.rs` asks it which version an operation runs on, and which versions must have a
  process (`Lifecycle::wanted`), and makes it so (`reconcile`). The property tests run against it with no compile.
- **Two versions of one provider at once (reading).** Staging, and a segment finishing on an older version, need
  more than one process per provider. A provider's `Slot` keeps the version in force in its place and any other
  version wanted in `others`, each in its own process. They are all the one provider's code, so `DEC-7`'s one
  process per provider still holds between providers. A version nothing runs on any more stops as it leaves.
  A crashed version restarts in its place (`respawn`), so a staged or draining version is never put in force by a
  restart.
- **Staging (ADR-0014, §6.2 risk 7).** `Platform::set_staging(Some(Staging::new(share, window, minimum)))`. Off by
  default (`None`), so every suite and test that expects an accepted release to run at once is unchanged. Nothing
  in the repository sets it yet: until a deployment's settings do, every automatically accepted release goes live
  at once, the case ADR-0014 guards against. Item for whoever assembles the deployment (the operator's settings):
  call `set_staging` from them, on by default. With it on, an accepted release of a provider that has a version in
  force is staged, by the automatic path in `admit` as much as by `accept`. The share is in percent of the segments
  that begin, taken in order: the `n`th takes the staged version if `(taken + 1) * 100 <= share * n`. That is
  deterministic, so tests and an operator can predict it.
- **Promotion and rollback.** `promote` is the operator's decision, refused until both the window has passed
  (`WindowOpen`) and the staged version has handled the minimum (`TooFewObserved`). "Handled" means operations it
  answered or the platform refused for it; `Observation` gives both counts and the version in force's beside them,
  for `REQ-AUD-7`'s comparison, which is the operator's and not the platform's. `roll_back` revokes the staged
  version and ends the staging; the version in force stays. Neither is ever automatic.
- **A release while one is staged** is refused as `ReleaseStaged`, a new module-interface 4.0.0 reason, before
  compiling.
- **Segments (reading).** The platform is synchronous, and an operation names only its transaction, so a segment in
  flight needs a caller-named start. `Platform::begin_segment(provider, transaction)` begins one and pins its
  version. `handle` runs an operation of that transaction on the pinned version. `end_segment` ends it and hands
  over its `SegmentRecord`: the version it began on, the one it ran on, and each move. That record is "recorded per
  segment" (`REQ-MDL-5`); keeping it durably is Core's. An operation whose transaction no segment began runs on the
  version in force, as before, and doesn't count towards staging. Core begins segments once it hands the platform
  route segments; until then the suites keep calling `handle` directly. A cross-module call runs on the callee's
  version in force: the caller's segment is the caller's, and names no version of the callee. This is a one-line
  shared edit in `cross_module` (`handle_in(callee, &asked, None)`).
- **Transactions (`REQ-MDL-14`, `INV-8`).** A transaction that a segment holds or held never begins another
  segment, at any provider. An operation naming it at another provider, or after its segment ended, is refused as
  `PlatformRefusal::DuplicateTransaction`, a new 4.0.0 reason, before any module sees it. Asked again inside its
  segment it is the same transaction, a retry, and is answered. A refused begin records nothing. The set of ended
  transactions lives in the platform's memory; the durable uniqueness is the state machine's journal (`F-010`).
- **Stale (`REQ-MDL-5`, `REQ-MDL-19`).** `mark_stale` is the operator's decision. No input of a publisher's reaches
  it: publishing newer releases moves no segment and marks nothing. That is how the test reads "a publisher can't
  mark its own version stale", since the platform has no authorisation of its own (`REQ-AUZ-6` is
  `CMP-CORE-AUTHZ`'s; the platform records who decided). It is refused while a release is staged
  (`StagingInProgress`, ADR-0014's rule), and for the version in force (`NoNewerVersion`). Each segment on the stale
  version moves at once to the version in force, which is the newest version available: the newer ones are staged,
  revoked or never started. Each move is recorded with its reason and clock.
- **Revoked and exploited (`REQ-MDL-20`).** A revoked version takes no new segment, and its segments finish on it.
  Revoking the version in force puts the one in force before it back, if one is unmarked, recorded as `Reinstated`.
  That is the only way back to an older version, so it is a recorded decision and never an effect of publishing
  (`REQ-MDL-4`). A version declared exploited is revoked and runs nothing more. Its process stops, and its segments
  are refused as `VersionExploited`, a new 4.0.0 reason. They are not moved: `THR-88` reads a false declaration as
  stranding orders, and moving them onto a version they never ran on would be a second, unobserved upgrade. That
  declaration is ADR-0014's emergency path; who may make it is `REQ-AUZ-6`'s.
- **An older signed release (`REQ-MDL-4`, `THR-17`).** Versions are publisher names, not ordered, so the platform
  orders by the assertion's `made_at`. A release asserted strictly before the provider's last accepted one is
  refused as `OlderRelease`, a new 4.0.0 reason, before compiling. Equal times pass, since every test and suite
  release shares one `made_at`. The same version again is still `VersionAlreadyAdmitted`.
  `catalogue/mod.rs` gains `pub(crate) use time::Stamp` for this, one line, which admission now relies on.
- **A failed upgrade.** A release whose process doesn't start is accepted but never placed: the version in force
  stays, in its own process, and under staging nothing is staged. The failed release stays the last accepted one,
  so the next release compares with it. Its capabilities were accepted, even though they never ran (reading).
- **Withdrawal (`T-013-04`'s review).** `Platform::withdraw(provider, assertion_digest, by, at)` withdraws the
  waiting release named by that digest, by someone. That includes one that acceptance refused when it judged it
  again. The decision is recorded. Its version stays named, so it isn't admitted again; the provider's next
  release is. While a release waits, a release under another version is now refused as `AdmissionPending`, the
  4.0.0 reason `T-010-09` added for this; the same version is still `VersionAlreadyAdmitted`. Three expectations in
  `module_admission_signed.rs` and its properties changed with it.
- **Acceptance re-checks its assertion**, as `T-013-04`'s review built: `acceptance_after_the_signing_keys_rotation_is_refused`
  and `T-016-04`'s operator-key change in `operator_published_holding_module_is_admitted_on_the_operators_own_release`.
  It is not redone. The withdrawal test starts from such a refused release.
- **Per-transaction and shared state (`REQ-MDL-10`).** There is no write in the WIT world, and the WIT is
  `T-013-07`'s, so the scope is a key namespace the platform enforces. `transaction/<transaction>/<entry>` is that
  transaction's alone, and any other key is shared. The scope comes from the transaction the platform is handling,
  never from the module. Another transaction's key is answered as not found and is never asked of the world, so
  neither `World::state` nor the production world changed. This is a one-edit change in `serve`'s state branch.
  Item: "each change to shared state attributable to a transaction and a version" has nothing to attribute until a
  state write exists. When one is added, it should carry `Context.transaction` and `Context.version`.
- **Operator-published modules (`T-016-04`).** The lifecycle is keyed by module name and knows nothing of who
  publishes. An operator's wallet upgrade is staged, takes segments and rolls back as a provider's does
  (`operator_published_module_upgrade_is_staged_and_rolled_back_as_a_providers_is`).
- **Item for `F-014` (the proxy).** `World::running` is told each version as it starts, so with two versions of one
  provider running, the production world holds the one started last. If a staged release rotates its statement
  key, the version in force's results go unsigned (`Unavailable`) until the staging ends. Telling the world which
  release handles each operation, or holding one running release per version in the proxy, is `F-014`'s. Not
  changed here.
- **Tests.** `tests/module_lifecycle.rs`:
  - `failed_upgrade_leaves_the_previous_version_in_force`, with a release that doesn't start, staged and not, and
    a bad staged release, observed and rolled back;
  - `upgrade_of_one_provider_interrupts_no_order_at_another`;
  - `low_volume_release_stays_staged_until_its_minimum_is_observed`;
  - `segment_on_a_revoked_version_finishes_on_it_unless_it_is_exploited`;
  - `newer_release_never_moves_a_segment_and_a_stale_mark_moves_it_to_the_version_in_force`;
  - `transaction_cannot_read_another_transactions_state`;
  - `duplicate_transaction_identifier_is_refused`;
  - `release_waiting_for_acceptance_can_be_withdrawn`;
  - `publishing_an_older_signed_release_changes_nothing`;
  - `operator_published_module_upgrade_is_staged_and_rolled_back_as_a_providers_is`.

  `tests/module_lifecycle_properties.rs` (`DEC-18`, seeded, 400 cases each, no compile):
  - a segment never runs a version that wasn't accepted and started, or one declared exploited, and moves only by
    a recorded stale move to the version in force;
  - a transaction identifier never begins two segments;
  - a staged release takes its share exactly and is promoted only after its window and minimum.

  The contract has `lifecycle_refusals_are_distinct_reasons_that_print_their_names`. The unit test
  `a_transaction_reads_shared_state_and_its_own_only` is in `lifecycle.rs`. Putting the previous version back in
  force when the one in force is revoked is covered only by the property test, not through the platform's
  processes.
- **Test-first.** The tests were written against the API, then run with each rule disabled: the duplicate check,
  the exploited refusal, the minimum, the stale-while-staged refusal, the state scope, the pinned versions'
  processes, `OlderRelease`, `AdmissionPending` and withdrawal. Nine of the ten integration tests and all three
  properties failed on the disabled rule, and they passed once it was restored. The operator-module test passed at
  once: no rule disabled was its own.
- **`DEC-20`.** This task touches these `DEC-20` paths:
  - module admission: `module/lifecycle.rs` (new), and `module/platform.rs`'s `admit` (`AdmissionPending`,
    `ReleaseStaged`, `OlderRelease`, staging on the automatic path), `accept`, the new `withdraw` and the lifecycle
    decisions;
  - the catalogue that admission relies on: `catalogue/mod.rs`'s `Stamp` re-export, for `OlderRelease`;
  - `contracts/`: module-interface 4.0.0 gains four Rust-only reasons, with the README, the changelog and a test,
    and `VersionAlreadyAdmitted`'s doc names a withdrawn version too.

  Shared edits in `platform.rs` outside admission:
  - `Slot.others`;
  - `start` split into `spawn`, `respawn`, `launch` and `reconcile`;
  - `run_in_process` taking and returning the handling version's process;
  - `handle` choosing the version (`handle_in`);
  - the state branch's scope check;
  - the one line in `cross_module`.

  `host.rs`, the lookup and egress paths, `engine.rs` and the compiler are untouched.

## T-013-06, review round

- **The world is told which release handles each operation.** `launch` told the world that the staged release ran;
  after a rollback or an exploited declaration the version in force kept its process, so the world was never told
  again. The production proxy then judged lookups against the revoked release, recorded uses under its version,
  and, for a changed statement key, refused every result as `Unavailable`. `run_in_process` now calls
  `World::running` with the handling version's release, before `begin_operation`, whenever it isn't the last one
  told for that provider (`Platform::told`). This replaces the earlier item for `F-014`.
  `rolled_back_release_with_a_new_statement_key_leaves_the_version_in_force_signing_with_its_own`
  (`tests/production_world.rs`) stages the operator's wallet with a new statement key. The version in force answers
  signed with its own key while staged and after the rollback, and a segment on the staged release answers with the
  new key. It failed with the re-telling disabled.
- **Staging is on by default (reading).** `Lifecycle::default()` holds `DEFAULT_STAGING`: a 10 % share, a one-day
  window and a minimum of 100 operations (`DEFAULT_SHARE`, `DEFAULT_WINDOW`, `DEFAULT_MINIMUM`). The values are
  this task's reading. The deployment's settings may change them in `M4`, and `set_staging(None)` puts upgrades in
  force at once. This replaces the earlier note's "off by default" and its item. The tests' platforms
  (`WasmBinding::new`, `Signed::new`, `production_world.rs`'s `Running::new`) call `set_staging(None)`, since the
  suites expect an accepted upgrade to run at once; the lifecycle's tests stage their own.
  `platform_stages_upgrades_by_default` failed with the default `None`.
- **A release follows the version in force.** Widenings, the history and `compared_with` were computed against the
  last accepted release, even one rolled back or declared exploited. So a release widening as a rolled-back one did
  passed as automatic. `admit` now follows the version in force, or else the newest accepted release neither
  revoked nor exploited (`release_followed`), and the contract's `validate_release` judges against that.
  - The history therefore leaves out a release that was withdrawn from use. Two checks still look at every accepted
    release: `OlderRelease` against the newest accepted, and a new check refusing an assertion signed with a
    statement key any accepted release declared (`Unverified::Release(AssertionSignedWithPreviousStatementKey)`).
  - A release that never started is followed only if nothing is in force.
  - This replaces the earlier reading on a failed upgrade's successor.
  - `release_after_a_rolled_back_one_is_compared_with_the_version_in_force` failed when it compared with the last
    accepted.
- **Only operations a process ran are observed.** `NotHandled`, and an operation that found its process gone,
  counted towards the minimum. A promotion could then drop the old process with no version in force. The platform
  now counts an operation for a version only once its process took it and said something of it (`Platform::runs`).
  `promote` is refused as `LifecycleRefusal::StagedNotRunning` while the staged version has no live process.
  `only_operations_the_staged_version_ran_count_and_it_is_promoted_only_while_it_runs` failed with either part
  disabled.
- **A transaction holding `/` owns no per-transaction entry** (`state_readable`). So `transaction/a/b/x` is only
  ever `a`'s entry `b/x`, never a transaction `a/b`'s. The owner was already the key's first segment, so this held
  before: the unit test passed with the explicit check disabled, and the check now says it outright. No refusal
  was added for such an identifier.
- **The merge.** `m2` at `661ee1a` (`T-013-05`, `T-013-07`, `T-010-06`) was merged first. The notes and
  module-interface's changelog conflicted on appended sections and bullets, and both sides were kept.
- **`DEC-20`.** This round touches one `DEC-20` path, module admission: `module/platform.rs` (`admit`'s
  `release_followed` and the statement-key check, `promote`, the re-telling in `run_in_process`, run counting) and
  `module/lifecycle.rs`. The re-telling calls into the proxy, a `DEC-20` path, through `World::running`; the
  proxy itself is unchanged.

## T-013-05, hardening the admitting compiler

- **Fuzzing (`B-09`, `THR-85`).** No fuzzing crate: nothing enters `Cargo.lock` or `licences/RECORD`. A seeded mutation
  fuzzer (`tests/module_support/fuzz.rs`: bit and byte changes, truncation, deletion, duplication, LEB128 edge values,
  and section-aware steps that corrupt, drop or swap a top-level section) makes the binaries; the same seed makes the
  same binaries. Each pair goes to the real admitting compiler, in its own isolated process (`children::compile`); a
  verdict, accepted or refused, is what it is for, and a compiler that failed or passed its deadline is a finding.
  - The checked-in corpus is `tests/fuzz-corpus/`: 30 files, under 120 KiB, made by the ignored test
    `the_corpus_is_written_when_asked` from eleven edge cases (empty, header only, a core module, an ELF header, a
    section length past the end, an overlong LEB128, and so on), the six hostile components and mutations of them,
    keeping a few of each kind of verdict. `fuzz_corpus_never_crashes_the_admitting_compiler` runs every file; the
    second test runs each as either part of a release through `Platform::admit` and then admits an honest release.
  - `fuzzing_for_a_while_never_crashes_the_admitting_compiler` is `#[ignore]`d and CI runs it for 120 s with the run
    id as the seed. Locally it is held to 120 s and the inputs to 512 KiB; the compiler's own jail bounds its memory on
    Linux (`RLIMIT_AS`), which macOS does not enforce, so a local run is bounded by the input size alone.
  - A local 90 s run (seed 20261109, macOS) made 2,711 pairs: 2,636 refused as not a component, 65 refused for an
    import or an export, 1 compiled, none crashed the compiler. Most mutations die at the parser, as expected of a
    byte-level fuzzer; coverage-guided fuzzing would reach deeper and
    is left to `F-025`'s review if it wants it.
- **Spectre settings (`G-09`, `THR-85`).** Chosen: `enable_heap_access_spectre_mitigation=true` and
  `enable_table_access_spectre_mitigation=true`, in `engine::SPECTRE`, set on the Cranelift configuration and listed in
  `engine::settings()`, with the reason in the constant's documentation. Both are Wasmtime's defaults, so the machine
  code is unchanged and `BINDING_VERSION` stays `0.2.0`; setting them stops a change of default in a later Wasmtime
  turning them off unseen. The settings test compares machine code: the engine's equals one with each flag set `true`
  and differs from one with it `false` (so each mitigation is in the code the compiler makes). Setting the flag takes
  an `unsafe` call (`cranelift_flag_set`), commented. Not claimed: protection against speculation across the host
  boundary or the processor's predictors; those rest on the process jail and one process per provider (`DEC-7`). The
  cost of the mitigations is not measured.
- **Re-derivation on another vendor (`B-11`, `G-04`).** "Another vendor" is read as an x86_64 CPU whose `vendor_id` is not
  `AuthenticAMD` (nor `HygonGenuine`, the licensed AMD design): the compiler's target is fixed (`engine::TARGET`), so
  machine code from an arm64 runner (`ubuntu-24.04-arm`) is for another target and would compare nothing, whatever its
  vendor. In `ci.yml` the existing `ci` job records, after its Test step, the stand-ins it built and the digest of
  each part's machine code with its CPU (`a_derivation_records_...`), and uploads them; a new job `rederive`
  (`needs: ci`, in the same workflow, no required check name added, which is why it is a job and not a step: it must
  run on another runner) first fails if `/proc/cpuinfo` names an AMD vendor or none, then runs
  `re_derived_machine_code_matches_on_a_cpu_of_another_vendor` with `RUTENT_REDERIVE_REQUIRED=1` against the very
  binaries the first job built. The test fails on an AMD or unreadable CPU, on a reference that records no vendor or the same
  vendor as this machine (two Intel runners compare nothing across vendors; the `ci` job, a required check, takes no
  vendor rule so it never fails at random), on a platform version that differs and on any part whose digest differs,
  and writes this CPU's vendor, model and digests. **GitHub gives `ubuntu-latest` either
  vendor, so the job skips on an AMD machine or the same vendor as the reference's and has to be re-run (see the review round)**: this is the reading of `T-012-01`'s writer that it
  can't count as a second AMD run, kept. Run unconfigured (no reference, as locally on macOS) the named test passes
  doing nothing and says so; the pieces it uses are tested always (`a_cpu_is_named_by_its_vendor_and_amd_is_told_apart`,
  `a_derivation_records_its_cpu_and_...`).
- **Red steps.** The settings test failed first to compile (`engine::SPECTRE` did not exist); its behavioural halves
  passed on their first run, since the mitigations are Wasmtime's defaults. The re-derivation test with
  `RUTENT_REDERIVE_REQUIRED=1` fails with no reference and, on macOS, with no `/proc/cpuinfo`; its vendor rule
  (`cpu::crosses_vendors`) is tested on fixed vendor pairs and, with the corpus and fuzz tests, passed on first run.
  The corpus tests passed at once: the compiler had no crash to find. Setting the flags changes no machine code
  (checked: the digests of all sixteen parts, the same binaries, with and without the explicit flags, are identical
  on macOS arm64), so `BINDING_VERSION` stays `0.2.0`.
- **What only CI shows.** The fuzz step and the `rederive` job have not run: their run ids are to be added here when
  they do, which the orchestrator does once CI has run (`T-013-05`'s Done-when, not met until then). Also whether a
  re-run of the `rederive` job alone can download the artefact its first attempt's workflow run uploaded. Not run on Linux here: the compiler's jail with fuzzed input (seccomp, Landlock,
  `RLIMIT_AS`), and `/proc/cpuinfo`.
- **`DEC-20` paths touched.** Module admission (the admitting compiler's engine configuration, `module/engine.rs`) and
  `.github/workflows/ci.yml`. `module/platform.rs`, `module/admission.rs`, `holding/`, `proxy/`, `egress/` and the
  compiler binary are untouched. Tests are in `tests/module_hardening.rs` with `tests/module_support/fuzz.rs` and
  `cpu.rs`, which `module_support/mod.rs` doesn't declare (the test includes them by path), so the shared support
  module is unchanged.

## T-013-05, review round

- **`rederive` skips instead of failing on a bad draw.** When this runner is AMD's or unreadable, or the reference
  records no vendor or the same one, the job's CPU step sets `skip=true`, writes a warning and a line to the step
  summary, and the later steps don't run: green, but not a re-derivation, and the summary says so. It is red only when
  the vendors differ and the digests don't (or the platform version differs). This replaces the earlier "fails on an
  AMD machine" reading above; the test's own vendor rule stays as a backstop. The plan row's Done-when now reads that the
  re-derivation has run on a CPU of another vendor at least once, its run id recorded; a green skipped run does not
  count.
- **Re-runs.** Both uploads (`rederivation-reference`, `fuzz-findings-…`) set `overwrite: true`, so a full "Re-run all
  jobs" no longer fails with a 409 on the duplicate name; the fuzz step has `timeout-minutes: 15`.
- **A guard on the engine's settings.** `engine::engine_from(config)` refuses an engine whose Wasmtime compatibility
  hash differs from the binding's own `config()`'s, which covers the Spectre mitigations and every other compiler
  setting; `engine::engine()` goes through it. The earlier claim in `engine.rs` that Wasmtime itself refuses an engine
  with the mitigations off was wrong and is gone. Test:
  `engine_settings_that_turn_a_spectre_mitigation_off_are_refused_when_the_engine_is_built` (it failed first to compile,
  `engine_from` missing).
- **The fuzzer goes inside nested core modules.** `fuzz::mutate_nested` splits the component, then a nested core module,
  into sections and changes one with the framing rebuilt, so lengths hold: a custom section added or filled, a data
  segment changed, a constant in a function body changed, a name's letter swapped, or a plain byte edit of any section
  (imports, types, code); also names and custom sections of the component itself. Half the pairs of the long run change
  one part only, so a refused part doesn't hide the other. Test first:
  `most_fuzzed_binaries_are_still_components_the_compiler_goes_on_to_check` (seeded, 300 mutants of the hostile
  components, at least 40% must be components): 11 of 300 before; 203 of 300 after. The 90 s local run (seed 20261109)
  before: 2,636 of 2,711 pairs refused as "not a component" (97%). After: 827 pairs, 276 refused as "not a component"
  (33%), 68 compiled, 483 refused for an import or an export the part may not have; none crashed the compiler.
- **Corpus.** `mutated-23`, `-25`, `-27` were copies of hostile seeds. The corpus is made again with the new fuzzer, the
  generator skipping any case equal to a seed or one kept: 30 files, none identical to another or to a seed.
- **Run ids.** CI run 42754273532 (branch `m2-ci-t01305`, commit `0bb16a3`): the `ci` job, fuzz step included, and the
  `rederive` job passed. The reference was made on AuthenticAMD (AMD EPYC 9V45 96-Core Processor) and re-derived on
  GenuineIntel (INTEL(R) XEON(R) PLATINUM 8573C); the digests matched. That meets the Done-when's re-derivation on a CPU
  of another vendor. That run used the first fuzzer. The deeper fuzzer ran in CI run 42756806042 (branch
  `m2-ci-t01305`, commit `1bfb734`): the `ci` job, its fuzz step included, passed, but its `rederive` job skipped
  (an AMD runner, so nothing was compared across vendors) and reported green. **Corrected 2026-11-10 (the review of
  `M2`'s pull request):** only 42754273532, at `0bb16a3`, compared across vendors. Both runs of the pull request's
  head (42772739775 and 42776086908) skipped too, so **there is no cross-vendor
  comparison at the head yet**; the job's skip is now a step of its own, "Note: no cross-vendor re-derivation on this runner", and re-running it until it lands on an
  Intel runner is what would give one.

## T-013-07

- **The two gaps `T-014-05` found are closed in the binding's world** (`wit/binding.wit`, package `0.2.0`,
  `BINDING_VERSION` `0.3.0`):
  - `proxy.secret-refusal` gains `store-unavailable`; `host.rs` maps the wire reason `store_unavailable` to it, so a
    guest is no longer told `undeclared-secret` for a store that is down.
  - `egress.call` takes a `request` record: a `method` (`get`, `post`, `put`, `patch`, `delete`), a `path`, a list of
    `header` records (name, value, `sensitive`) and a `body`. `FromProvider::Egress` carries the same fields
    (`WireHeader`). The sandboxed interfaces are unchanged; the package number moved with the rest, so the allowed
    imports in `host.rs` and the hostile components in the guests' tool name `0.2.0`.
- **The platform checks the request, never the guest.** `host.rs` only transcribes. `platform.rs`'s `egress`
  builds `egress::Request::new`, the one place that checks the target is an origin-form path, each header is a token
  with no control character, and none is a header the connector sets (`Host`, `Content-Length`, ...). Path and
  header bytes count with the body against `MAX_BODY` and the `OutboundBytes` budget, and the header count against
  the new `MAX_HEADERS` (64), so headers are no way round `REQ-MDL-11`.
- **Credential headers from a guest follow the rule `T-014-03` set.** The guest's `sensitive` flag becomes
  `Header::sensitive`, and a header named `authorization`, `proxy-authorization` or `cookie` is a credential by
  name, whatever the flag says. A redirect to another endpoint drops both; the test shows a flagged `X-Api-Key` and
  an unflagged `Authorization` dropped while a plain header and the method follow a `307`.
- **Superseded by the review round below:** a request the platform cannot build is now recorded.
- **(Earlier reading) a request the platform cannot build is refused unrecorded.** An unknown method, a target that is not
  origin-form or a framing header has no reason in module-interface 4.0.0 to be recorded under (no refusal stands in
  another's name), and no destination was judged. It is refused the way an unknown scheme is, with the outcome
  `refused` and no record. If a record of it is wanted, it needs a reason in the contract: an item for `F-010`.
- **Reading: "records" is the call's record, not the request's content.** Every call leaves its one
  `OutboundCallRecord`, refused ones included, as before. The record does not carry the path, the headers or the
  body: a query string or a header can hold a secret, and `outbound-call-record-carrying-a-secret-value` is the
  contract saying the record names no value. `contracts/` is untouched.
- **`World::receive`**, a new defaulted hook, is where the default `World::send` hands a request to the world it
  serves (method, target, headers, body); its default passes the body to `deliver`, so `deliver`'s signature, the
  production world in `proxy/` and the conformance trait's `received` are unchanged. The suite's `call` step and
  `Forge` still send `POST /` with no headers, so the evidence references the suite compares are unchanged.
- **Guests, both languages.** The Rust and C stand-ins send the new request shape for every existing step, and read
  two binding-only program lines the tests use: `fetch <operation> <host> <port> <method> <path> <headers>` and
  `reason <operation> <name>`. `Probe::Fetch` and `Probe::Reason` write them. The C step type gained a
  constructor (`mk`) so that adding fields does not touch every initializer.
- **Tests** (`tests/module_wit_shapes.rs`, each in Rust and C): `guest_tells_an_unreachable_secret_store_from_a_missing_secret`,
  `outbound_requests_method_path_and_headers_reach_only_its_declared_destination` and
  `outbound_request_naming_another_host_in_its_path_or_headers_is_refused_before_it_is_sent`. Shown red by mutation
  (the host mapping dropped to `undeclared-secret`; the `sensitive` flag ignored), since the stand-ins and the world
  were written together.
- **`DEC-20`.** The module boundary (`wit/binding.wit`, `module/host.rs`, `module/wire.rs`) and the platform's
  outbound-call path in `module/platform.rs`. Shared edits: the `BINDING_VERSION` line in `module/engine.rs`
  (`T-013-05`'s file) and the new `MAX_HEADERS` in `module/limits.rs`.

## T-013-07, review round

- **A refused request is recorded (blocker).** A request the platform won't send (an unknown method, a target that
  is not a path, a header that is not a token or that frames the message) was refused with no record, though the
  call was already charged and `REQ-MDL-8` wants every call recorded. module-interface 4.0.0 (unreleased) gains the
  reason `request_not_allowed`: the schema's `refusal_reason`, `RefusalReason::RequestNotAllowed`, a valid fixture,
  a contract test, the README and the changelog. The platform records it with one hop (the endpoint asked for) and
  none of the request, as it records a call refused for a limit (`record_refused_call`). `contracts/` is a `DEC-20`
  path, and this commit touches it.
- **Forwarding headers.** `Forwarded`, `X-Forwarded-Host`, `X-Forwarded-For`, `X-Forwarded-Proto`,
  `X-Forwarded-Port`, `X-Real-IP`, `X-Original-URL`, `X-Rewrite-URL` and `X-Original-Host` join the headers a request
  may not set, for the reason `Host` is one.
- **Limits tested.** Past `MAX_HEADERS`, and past `MAX_BODY` in a path, in a header value and in a body and a path
  together, the operation is refused as a limit with a `MessageSize` event, a record refused `limit_reached` and
  nothing sent; the bound itself sends. The stand-ins' `fetch` line takes a trailing `*N` on a path or value (N bytes
  of `x`) so a program in an 8 KiB slot can make a large request; the C stand-in's header cap is 256.
- **`WireHeader`'s `Debug`** hides the value of a flagged header and of one named `authorization`,
  `proxy-authorization` or `cookie` (`THR-50`). `limits.rs`'s docs say what is counted now.
- **Shown red:** the record tests failed with no record; the two limit tests failed with the header count and the
  header and path bytes taken out of the check.

## Which reason wins for a bad request to an undeclared host (re-review of T-013-07, 2026-11-10)

The platform checks a guest's request (method, path, framing and forwarding headers, size) before it judges the
operation's declared destinations. So a request that is both malformed and aimed at an undeclared host is recorded as
`request_not_allowed`, not `undeclared_destination`, and its hop looks the same as one to a declared host. It is still
recorded and still refused; only the reason differs. Left as it is, and stated here, so an audit reads
`request_not_allowed` as "refused before any destination was judged".

## T-013-08: the flaky deadline test (`plan/m3-owed-rows`)

`callee_past_its_deadline_is_unanswered_to_its_caller_never_refused` (`components/prov/tests/production_world.rs`,
`T-014-05`'s) failed once in CI and passed on the same commit. Its margins are a 2 s deadline and a 1 s call budget.
`T-013-08` makes it deterministic by removing the race, not by widening the margins. It sits in an `M3` wave, as
`T-011-07` does.

### Design (T-013-08)

- **Cause, reproduced.** The platform starts a callee only while `caller.due - now >= call_budget + call_budget / 2`.
  With a 2 s deadline and a 1 s budget the caller has 0.5 s from the start of its operation to reach the call. A
  delay of 600 ms in front of that check on the old code (a temporary sleep, not kept) fails the test at its first
  assertion, `the callee wasn't stopped at its deadline`, `left: 0, right: 1`, which is the CI failure: the call was
  refused as `deadline_too_near`, not started, so nothing was `unanswered`. Plain CPU load did not reproduce it: 60
  runs of the old test passed with 36 and with 100 busy loops on 12 cores (the second at the lowest priority); the
  caller's start-up on that machine stays far under 0.5 s.
- **Second CI failure, same file.** `hanging_name_lookup_is_given_up_at_the_operation_deadline_and_another_provider_still_runs`
  failed at `took < 4 s` (`production_world.rs:1336` at `34c700e`) on the push run `43424104071`, and passed on the
  pull-request run. That is a bound on the machine's speed, not on the platform: a runner that stalls for 4 s
  anywhere in the operation breaks it. With a temporary 4.5 s stall in the platform after the deadline was set, the
  old test fails at that assertion (`held for 4.547706083s`), as do the old `slow_peer_...` (its twin bound) and the
  old `callee_past_...`; the new tests pass under the same stall.
- **The seam.** `OperationClock` (`module/platform.rs`), held by the platform (`set_operation_clock`). `Monotonic`
  is the default and is `Instant::now()`, as before. `SteppedClock` stands still until `step` moves it. The clock is
  read for the operation's own deadline, the caller's remaining time at a call, and the wait for a process's next
  message; the deadlines handed outward go through `wall_due`.
- **Production unchanged.** On `Monotonic` the wait is the same single `receive_until(due)`, and the deadlines handed
  to the proxy world and to name lookups are the same instants.
- **On a stepped clock** the wait polls the clock every 2 ms and a deadline it has reached has passed, whatever is
  waiting. Deadlines handed outward are either the operating system's time left on the clock
  (`SteppedClock::with_outward_waits_on_wall_time`, for tests of a lookup or a peer given up at the deadline, which
  then take the deadline's 2 s) or a fixed 60 s that a step of the clock ends first (`SteppedClock::new`).

### Tests converted (T-013-08)

In `components/prov/tests/production_world.rs`:
- `callee_past_its_deadline_is_unanswered_to_its_caller_never_refused_however_slowly_the_caller_started` (the
  flaky one, renamed to the row's name): the clock stands still, and a stepper thread steps it 1.5 s once the callee
  is stuck in its lookup.
- `callee_is_started_only_while_its_callers_remaining_time_on_the_platforms_clock_covers_its_budget_and_reserve`
  (new): a deadline of exactly 1.5 budgets starts the callee, one nanosecond less refuses it.
- `hanging_name_lookup_is_given_up_at_the_operation_deadline_and_another_provider_still_runs`: stepped clock, a
  stepper steps the 2 s deadline once the lookup is asked; the `took < 4 s` bounds are gone.
- `slow_peer_is_cut_off_at_the_operation_deadline_and_another_provider_still_runs`: the same, stepped once the peer
  got the call; its two `< 4 s` bounds are gone.

In `components/prov/tests/module_calls.rs`, stepped clock standing still:
- `call_with_too_little_deadline_left_is_refused` and `call_needs_the_callees_budget_and_the_callers_reserve_left`:
  their controls (a callee started with 3 s of deadline and a 1 s budget) needed the caller to reach its call within
  1.5 s.

In `components/prov/tests/module_limits.rs`:
- `a_guest_that_loops_is_stopped_when_its_fuel_runs_out`: the clock stands still, so only fuel can stop the guest;
  its `took < OPERATION_DEADLINE / 4` bound (2.5 s of a machine's speed) is gone, and the time is still printed.

### Not converted through the seam (T-013-08)

- `module_limits.rs::a_provider_process_that_misses_the_operation_deadline_is_killed_and_started_again`: its 1 ms
  deadline is one-sided (the guest spins until its fuel runs out, seconds later), and the platform's wait for it
  begins before any message could arrive; there is nothing to interlock a step with. Left on real time.
- `module_admission.rs::admitting_compiler_past_its_wall_time_deadline_is_stopped`: the admitting compiler's
  deadline is a wall-time bound on another process's exchange, not an operation deadline, and is not read from the
  seam; its 1 ms deadline is one-sided, and its `took < 20 s` bound is a thousand times the margin. Owner: `F-013`.
- `proxy_https.rs` (`store_that_never_answers_times_out` and the one after it): they call the HTTPS transport with
  a real `Instant` deadline and a read timeout, without a platform, so no platform clock reaches them; their bounds
  (700 ms for a 200 ms timeout over an 800 ms silence, 1.5 s for 800 ms) are a machine's speed. Owner: `F-014`
  (the proxy's transport). The `F-014` row is `T-014-06` (wave 25), which also takes the three tests of `egress.rs` with the same kind of bound.
- `module_lifecycle.rs` polls up to 5 s for a killed process to be seen gone: a wait for the operating system, not
  a deadline or budget of the platform.
- The 10 s `OPERATION_DEADLINE` and `START_DEADLINE` the other platform tests run under are read on the
  operating system's time still, as in production; a runner that takes 10 s to start a process fails them, which is
  the deadline doing its job.

### Follow-up (T-013-08): stepped clock in every cross-module call test

`route_calls.rs::call_a_route_doesnt_name_is_refused_when_the_route_comes_from_the_router` failed once in a full
workspace run under load, and passed alone and in 10 later runs. It ran on the real `Monotonic` clock, so spawning
the wasm children of p-r1, p-r2 and p-z could pass the 10 s `OPERATION_DEADLINE`, and the call was then refused on
its deadline instead of as `NotNamedByRoute`. Its binding now has a `SteppedClock`, with the assertions unchanged.
The same class was closed in every other test that makes a cross-module call on a platform with no operation clock:
`module_calls.rs` (4 tests), `module_limits.rs` (4 tests), `proxy_calls.rs` (3 tests) and `production_world.rs`
(4 tests). Not changed, because they are about real time or never start a callee: the single-process tests of
`module_limits.rs`, `module_lifecycle.rs`, `module_process.rs` and the others, and `egress.rs`, whose deadlines are
the outward ones. `START_DEADLINE` is still read on the operating system's time (the platform hands it to the
spawn directly); a start that passes it fails the install, not the assertion.

The scenarios' fleet had the same flaw in principle, and it is fixed as defence, but it was not why `execution_routes`
failed. `Fleet::with_context` built its platform on the real clock, so under load a simulated provider's call could pass
its real deadline, or its start `START_DEADLINE`. Every `Fleet` platform now runs on a `ScenarioClock` that stands still
(a scenario that scripts a `silent` or `late` provider is about a real deadline passing, so queueing that behaviour makes
the clock follow the operating system's time from then on, without a jump), and `Platform::set_start_deadline` (new,
default `START_DEADLINE`, so production is unchanged) is set to 120 s on a fleet, since a start is the machine's work
and no scenario is about start-up time. Edited: `module/platform.rs` (F-013's), `fleet.rs` and `world.rs` (F-024's).

The real cause of the five `execution_routes` failures (`CantProceed { class: NoRouteRightNow }` where a quote is
expected, within 8 to 20 s, and only while other worktrees were building, which is why load seemed to matter) was the
shared simulated-provider build. `conformance/scenarios/src/sim/disk.rs::built_guests` built the guests into
`<target>/rutent-sim-guests` and rebuilt them only when the `built` marker was older than the newest file of the
current worktree's sources. Every worktree in a build lane shares one target directory, so a worktree on an older base
whose files had just been edited rebuilt the guests from its own sources and left the marker newest; the next worktree
saw a marker newer than its sources and ran on guests without the simulated providers `T-024-04` added. Confirmed
deterministically: `execution_routes` passed on `f013i`; the guests were then rebuilt from a scratch checkout of
`b8c416e0` into the same directory; the same test run again from `f013i` failed 5 of 5 in 13 s. Fixed: the build is now
in `<target>/rutent-sim-guests/<key>`, where the key is a hash of the paths and bytes of the guests' sources
(`module-guests`, the WIT, `conformance/src`, `contracts/src`), so two trees never share a build and a file's age
decides nothing; the newest three other builds are kept and older ones, and the loose files of the earlier layout, are
removed. `RUTENT_SIM_GUESTS` still overrides it.
Review of that fix added: builds are taken one at a time under a lock directory in the target (they share cargo's
intermediate `sim-guests-build`, which another key's build could overwrite between `cargo build` and `componentize`),
each is made in `<key>.tmp-<pid>` and renamed into place whole, a build's use is its `built` marker's time and only
builds unused for more than a day are removed (a running test may still read an old one), and the key also covers
`conformance/Cargo.toml`, `contracts/Cargo.toml` and `rust-toolchain.toml`. The prov tests' two platform builders
(`module_support/mod.rs`, `signed.rs`) get the same 120 s start deadline as the fleet, for `holding_handed_on`'s
"the wallet runs" under load. CI, with one checkout, was never affected.

### Defaults (T-013-08)

- The clock is `Monotonic` unless a test sets another. `STEPPED_POLL` 2 ms; `STEPPED_WAIT_CAP` 60 s.
- The reproduced test's outcomes are unchanged (`unanswered`, never `refused`, callee stopped, started again). Its
  deadline (2 s) and budget (1 s) are as before.

### Rule-off table (T-013-08)

See the commit body.

## T-013-09: a repeat under a carried out transaction (`f-013/t-013-09-repeat-refused`)

`T-024-08`'s review found that a repeat reaching the platform while its segment is held moved value twice for a provider
that declares no safe retry (seed 110, the simulated exchange), and that nothing under `components/` read `safe_retry`.

### Design (T-013-09)

- **Where the platform reads `safe_retry`.** From the catalogue's advertisement, through the sealed `ReleaseJudge`, which
  gains `safe_retry(provider) -> Option<bool>` (`Catalogue::declared_by(..).safe_retry`). The platform reads it each time it
  admits or accepts a release of the provider and keeps it in the provider's slot. No contract change: the declaration was
  already in `provider-role`'s `declarations.safe_retry`. `None` (no advertisement, a verifier, a module the operator
  publishes) and `Some(false)` both mean no safe retry.
- **What is a repeat.** `Platform::handle_in` keeps, per transaction, the operations it carried out by their kind (the module
  ran, whatever it came to: an answer, an outcome unknown, a refusal after it ran), until the transaction's segment ends
  (`Platform::end_segment`; after that `version_for` refuses it as before). A second operation under that transaction is a repeat, whatever its kind (a transaction's identifier is one
  operation's, `INV-8`). Dry runs are no repeat: they move nothing, and neither carry out nor repeat anything. Nor are the kinds that only ask (`balance_verification`, `judgement`, `holding_balance`); any other kind, one
  added later too, counts.
- **What a repeat gets.** For a provider without safe retry, `PlatformRefusal::DuplicateTransaction` (the contract's reason
  for `INV-8`; no new reason is needed, so `contracts/` is untouched): the module doesn't run, nothing moves, and the caller
  reads the refusal. For one with safe retry, the first answer from the platform's record, byte for byte, the module not run.
  If the first run gave no answer (a refusal after it ran), the module runs again, which its declaration allows. The first
  answer stays the record. An operation under a transaction carried out for another provider is refused whatever it declares.
- **Envelope.** The platform verifies no signature, so for a provider without safe retry a changed key or protocol
  version is a retry and is refused like any repeat. For a provider with safe retry an operation whose deployment,
  protocol version, key or terms differ from the first's is not answered from the record but reaches the module, which
  refuses it as foreign (`REQ-PRT-1`); the role suite's replay probes
  (`conformance/scenarios/tests/sim_providers.rs`) need that, and the platform did not check envelopes before this row
  either. So a foreign replay of a carried out transaction to a provider without safe retry still rests on the module's own
  envelope check.
- **Existing tests changed (artefacts).** They reused one transaction id for different operations or to re-read a
  version: `holding_wallet.rs`'s `deposit_answer` now names the node and asset in its transaction;
  `module_lifecycle.rs` asks a fresh transaction for the version after a promotion, and reads a stale mark's move from the
  segment's record instead of re-asking its transaction.
- **New accessor.** `Platform::operations_run(provider)`: how many operations the module ran, so a test can say a repeat
  never reached it.
- **Tests.** `components/prov/tests/module_repeat.rs`: the two named tests, and three more (a provider whose advertisement
  declares nothing is refused, another provider's carried out transaction is refused even for a safe provider, a withdrawn
  safe-retry declaration is read when the next release is admitted). `conformance/scenarios/tests/repeat_joined.rs`: the
  named `repeated delivery on the joined route moves nothing twice`. `T-024-08`'s
  `invariants_support/mod.rs` (an `F-024` file, one edit) plants `DeliverTwice` on both routes again; the invariants pass.

### Defaults (T-013-09)

- No declaration, or one the platform can't read, is no safe retry: the repeat is refused (fail closed).
- The key is the transaction alone (one operation per transaction), in two scopes: the platform's own, and each callee a
  module calls (`handle_in(.., called)`), a callee that declares safe retry running each call again (each is an
  operation within its own budgets), so a module calling a callee without safe retry twice under one transaction is
  refused the second call (the caller sees it unanswered), and the callee's record never collides with the caller's.
- A dry run and the read-only kinds are exempt. Reversible.

### Gaps (T-013-09)

- The platform reads the declaration when a release is admitted or accepted, not when the advertisement changes: a
  provider that re-advertises `safe_retry: false` is read at its next release. Owner: the assembly (`F-020`), to refresh
  declarations when a successor advertisement joins; until then a provider can't withdraw safe retry without a release.
- The record is kept until the transaction's segment ends; a transaction handled with no segment begun is kept until the
  platform stops. Core begins a segment for every transaction, so nothing grows there; owner: `F-013`, if a deployment
  ever calls `handle` with no segment. **Closed by `T-013-10`:** the record of an unsegmented operation is freed once
  its provider's evidence window has passed, and it survives a restart.
- A foreign-envelope replay to a provider with safe retry rests on the module's envelope check, which the role suite
  asserts of every module. Owner: `T-025-05`, to review. **Closed by `T-013-10`:** the platform refuses an operation
  under a carried-out transaction of another envelope or other terms whatever the provider declares, in a callee's
  scope too.
- `DEC-20` path: `components/prov/src/module/` waits for the owner's click at the release into `main`.

### Rule-off table (T-013-09)

See the commit body.

## T-013-10: the platform's carried-out record survives a restart (`f-013/t-013-10-platform-record-durable`)

Finding 3 of `M3`'s milestone review of 2026-11-24 (PR #143), `F-025`'s medium M1, and the review's Low on the same
record. What the platform carried out under each transaction (`carried`), the segments in flight and the
transactions that ended (`lifecycle.rs`), and the runs it counted lived in its memory alone, so a platform restarted
mid-segment handed a repeat to the module again (`INV-8`).

### Design (T-013-10)

- **The port.** `module/record.rs`: `Record`, an append-only sequence of lines (`load`, `append`, `replace`), which
  the platform writes and reads and nothing else interprets. Two implementations: `FileRecord`, one JSON entry per
  line, each appended with `sync_data` before the platform goes on, `replace` written to `<name>.replacing`, synced
  and renamed over, with an exclusive lock on `<name>.lock` held while it is open (a second platform's opening is
  refused, `WouldBlock`); and `MemoryRecord`, whose clones share one sequence, for the tests that need no disk.
  A last line with no line break is a write a stop cut off and is not read; any other line the platform can't read
  refuses the opening.
- **Opening.** `Platform::new` holds a `MemoryRecord`, as before in effect. `Platform::open(paths, world, envelope,
  record)` reads every entry back, frees what its evidence window let go of, and replaces the record with what it
  holds (the transactions that ended, the segments in flight, what was carried out, the runs counted), so the
  record holds what is live, not every entry ever written. A reopened platform holds no release: each is admitted
  again, as at any start, and a segment in flight runs on its version once that version is.
- **The entries, and when each is written.** `carrying` (the operation, its transaction, scope and provider) before
  the module is handed an operation carried out once (`T-013-09`'s `repeat_key`), unless the record holds that
  transaction's scope already; `ran` (the provider's version, and the carried-out record where it changed) once the
  module ran, which also counts the run; `withdrawn` where a `carrying` operation never reached the module (refused
  as not handled, its process gone before a word); `segment` when a segment begins and after a stale mark moves it;
  `ended` when a segment ends or a record is freed. `carried` and `runs` are the snapshot `open` writes.
- **Fail closed, write by write.** A `carrying` entry that can't be written refuses the operation as `Unavailable`,
  nothing of it run (the contract's reason for "nothing of the operation took effect"; no contract change). A
  `segment` entry that can't be written refuses `begin_segment` as `Unavailable` and forgets the segment. Every
  later write is best effort, since what is already written fails closed: a lost `ran` leaves the operation
  carried out with no answer (refused, or run again where the provider declares a repeat safe); a lost `withdrawn`
  leaves it carried out; a lost `ended` leaves the segment in flight and its record held.
- **A platform stopped while the module ran** reopens with the operation carried out and no answer, from its
  `carrying` entry: a repeat is refused for a provider without safe retry, as one whose first run gave no answer.
- **`begin_segment` consults the record** (the review's Low): a transaction the platform carried out an operation
  of with no segment is refused as `DuplicateTransaction`, at its provider or another. A dry run and the read-only
  kinds carry nothing out, so a segment may begin on theirs.
- **An unsegmented operation's record is freed** once its provider's declared evidence availability has passed
  (`REQ-CAT-10`). The sealed `ReleaseJudge` gains `evidence_window(provider) -> Option<u64>`, read with `safe_retry`
  when a release is admitted or accepted (`declarations.evidence.availability`, bounded seconds; unbounded,
  undeclared or no advertisement is `None`, kept). The record's `free_at` is the world's clock (`World::now`) when
  the module ran plus that window, for a transaction no segment holds; the platform frees due records at each
  top-level `handle`, each `begin_segment` and on opening. Freed means the operations and answers are dropped and
  the transaction is spent (`Lifecycle::spend`, the same `ended` set a segment's end writes): a repeat is refused,
  never run, so freeing never trades `INV-8` for storage.
- **Other terms or another envelope at a safe-retry provider** (the review's Low, `REQ-MDL-14`): `repeated` now
  refuses as `DuplicateTransaction` an operation under a carried-out transaction whose statement, deployment,
  protocol version or key differ from the first's, whatever the provider declares; only the same operation is a
  repeat, answered from the record, or run again where its first run gave no answer. The role suite's `Through`
  adapters (`module_role.rs`, `sim_providers.rs`) already read `DuplicateTransaction` as the final refusal the replay
  probe expects, the twin's own, so a foreign replay at a safe provider now stops at the platform as it did for an
  unsafe one. (The fix round below extends this to callee scopes, and updates both adapters' comments.)
- **Existing test changed.** `T-013-09`'s unnamed `a_retry_of_another_envelope_reaches_the_module_of_a_provider_with_safe_retry`
  asserted what this row reverses; it is replaced in `module_repeat.rs` by the row's named test. `T-013-09`'s
  three named tests are unchanged and pass.
- **Accessors.** `Platform::carries(transaction)`, whether the platform keeps a record under it;
  `Lifecycle::segments`, `ended`, `has_ended`, `spend`, `restore_segment` and `forget_segment`.
- **Tests.** `components/prov/tests/module_record.rs`: the four named restart, segment and window tests, and three
  more (an operation the record can't hold isn't handed over; a record another platform holds isn't opened; and,
  inside the first, a platform stopped while the module ran). `module_repeat.rs`: the named other-terms test.
  `record.rs`'s unit tests: a cut-off last line is dropped, the lock survives `replace`, memory clones share lines.

### Defaults (T-013-10)

- A record entry the platform can't write before it acts refuses the act (`Unavailable`); later entries are best
  effort, each loss failing closed. Reversible.
- A freed record spends its transaction: after the evidence window a repeat is refused even at a safe-retry
  provider, rather than answered (the evidence is gone) or run (a second move). Reversible.
- Unbounded or undeclared evidence keeps the record for as long as the platform's record lasts.
- `free_at` runs on the world's clock at the module's answer; a world whose clock can't be read frees nothing.
- An operation at a safe-retry provider with other terms or another envelope is refused at the platform, not
  judged by the module.

### Gaps (T-013-10)

- Wiring a `FileRecord` into an assembly that restarts the platform: `T-024-21` (Core and the platform restarted
  in the invariant runs), and `F-030`'s production assembly for where the file lives. `M3`'s assembly opens
  `Platform::new`, the memory record, as before.
- A restarted platform holds no release until each is admitted again: which releases were accepted, which was in
  force or staged, the staging's counts and the withdrawn versions are not in the record, and a restart's
  re-admission re-establishes them in the order releases are admitted. The operator's marks and decisions are in it
  since the fix round below. Owner: `T-013-11`, part (3) of its row (PLAN PR #267), with `F-030`'s start-up
  re-admitting releases. **Closed by `T-013-11`** (below): the state is in the record; the re-admission stays
  `F-030`'s.
- The `ended` set grows by one line per transaction ever ended, in memory and in the record, as it did in memory
  before, and is rewritten whole at each opening. Owner: `F-013`, if a deployment's transaction count makes it
  matter (left as a gap by the coordinator in the fix round).
- Freeing is swept at each top-level `handle` and `begin_segment`, linear in the records held (the segments in flight
  and the unsegmented operations within their windows). Owner: `F-013`, an index by `free_at` if it matters.
- The jail-probes need Linux; CI runs them. This row touches no jail code.
- `holding_support`'s wallet build counts as current when the shared target directory's `rutent-plain-wallet/built`
  is newer than this worktree's guest sources, so a worktree with an older guest runs another's newer wallet:
  locally, `holding_wallet` failed twice on `T-016-05`'s wallet (an unknown order now `Unknown`, not `Refused`) and
  passed on this tree's own wallet, built into a scratch directory and named by `RUTENT_PLAIN_WALLET`. CI builds
  fresh. Owner: `F-016`, to key the build by its sources' digest rather than their times.
- `DEC-20` path: `components/prov/src/module/` waits for the owner's click at the release into `main`.

### Rule-off table (T-013-10)

| Rule switched off | Test that went red |
| --- | --- |
| The `carrying` entry written before the hand-over | `a repeat after the platform restarts moves nothing twice` (stopped while the module ran) |
| The `Unavailable` refusal of an operation the record can't hold | `an_operation_the_platform_cannot_record_is_not_handed_to_the_module` |
| Reading the record back in `Platform::open` | `a repeat after the platform restarts moves nothing twice` |
| The `ended` entry written by `end_segment` | `ended transaction stays ended after the platform restarts` |
| `begin_segment` consulting the carried-out record | `segment begun on a transaction already carried out is refused` |
| Freeing after the evidence window (`free_spent`) | `unsegmented operation's record is freed after its evidence window` |
| A freed record spending its transaction (`Lifecycle::spend`) | `unsegmented operation's record is freed after its evidence window` |
| Other terms or another envelope refused at a safe-retry provider (`repeated`) | `repeat with other terms under a carried out transaction is refused even with safe retry` |
| `FileRecord`'s lock | `a_record_another_platform_holds_is_not_opened` |
| Dropping a cut-off last line (`whole_lines`) | `record::tests::a_file_record_reads_back_what_it_appended_and_drops_a_line_cut_off` |

### Fix round 1 (T-013-10): the security review of `e173cfac` (PR #291)

The fresh reviewer traced every crash point and found none that runs a transaction twice. Its findings, and what
each became:

1. **A failed append left a fragment for the next entry to be glued to** (should-fix). `FileRecord::append` notes the
   file's length before it writes; on any failure it cuts the file back to it and syncs, and marks the record
   broken: every later `append` and `replace` is refused until the record is opened again, so the platform refuses
   each operation it can't record (`Unavailable`). A cut that fails too leaves a fragment with no line break at the
   end, which an opening doesn't read, since nothing more is appended. Test:
   `record::tests::a_write_cut_short_leaves_every_whole_entry_readable_and_the_record_takes_no_more` (a
   `#[cfg(test)]` short write after a `carrying` entry; the reopening reads that entry back).
2. **A safe-retry callee ran a call with other terms under one transaction** (should-fix). `repeated` compares the
   operation before the early return for callee scopes: a callee runs again only the same call. Test:
   `platform::tests::a_call_with_other_terms_into_a_safe_retry_callee_under_one_transaction_is_refused`, a unit test
   on `repeated`, since the boundary suite's stand-ins make each call from the caller's own operation and can't vary
   its terms without a change to that suite.
3. **The operator's marks weren't kept across a restart** (low, done now). Each decision (`withdraw`, `promote`,
   `roll_back`, `revoke`, `declare_exploited`, `mark_stale`) is taken on a clone of the lifecycle, and what it
   changes, the marks it sets, the segments it moves and the decisions it records, is written as one `decided` entry
   before the platform holds it; one the record can't hold is refused as the new `LifecycleRefusal::Unrecorded`,
   nothing changed (amended in fix round 2: only a widening decision is refused so). `Platform::open` reads marks and decisions back (`Lifecycle::restore_mark`,
   `restore_decision`), and `Lifecycle::started` never puts a marked version in force or staging, so an exploited
   release admitted again after a restart runs nothing. Tests:
   `segment_restored_after_a_restart_refuses_a_version_declared_exploited`,
   `a_decision_the_platform_cannot_record_is_not_taken`. Releases themselves stay a gap (above), owned by `T-013-11`.
4. **Another name got past the lock, and a missing record was made silently** (low). `FileRecord::create` makes a new
   record (refused where anything is at the path); `FileRecord::open` refuses a missing one (`NotFound`), a path that
   is a symbolic link or names a file with a second name (`nlink` above 1, on Unix) (`InvalidInput`), and locks
   `<canonical path>.lock`, so a directory reached through a link takes the same lock. Tests:
   `record::tests::a_record_is_opened_by_its_own_name_and_never_made_by_opening`, and
   `a_file_record_is_held_by_one_platform_at_a_time` through a linked directory.
5. **The directory sync after `replace` was best effort** (low). It is an error now, except where the system can't
   sync a directory at all (`Unsupported`, `EINVAL`); `FileRecord::create` syncs the directory too. A `replace` that
   fails marks the record broken, since which file holds its lines is no longer certain.
6. **The identical-repeat check left out the maker, the shape and the signature scheme** (low). All three are
   compared; `made_at` and the signature value are left out, as the doc comment says: a retry is the same operation
   made again later and signed again, and the platform verifies no signature. The named other-terms test adds the
   maker and the shape (the contract has one signature scheme).
7. **Stale text** (low). `T-013-09`'s two gaps above are marked closed, and both `Through` comments
   (`module_role.rs`, and `F-024`'s `sim_providers.rs`, a comment only, at the coordinator's request) say what the
   platform now refuses.

#### Defaults (fix round 1)

- A record that failed a write takes nothing more until reopened: every operation the platform can't record is
  refused, which stops the platform carrying out anything until a restart. Fail closed; reversible.
- A decision the record can't hold is refused, not taken in memory alone.
- A record is created by an explicit step; opening never creates one.
- A hard-linked record file is refused outright rather than locked by inode.

#### Rule-off table (fix round 1)

| Rule switched off | Test that went red |
| --- | --- |
| `append` cutting back to the last whole line (1) | `record::tests::a_write_cut_short_leaves_every_whole_entry_readable_and_the_record_takes_no_more` |
| No write after a failed one (1) | `record::tests::a_write_cut_short_leaves_every_whole_entry_readable_and_the_record_takes_no_more` |
| Terms compared before a callee's early return (2) | `platform::tests::a_call_with_other_terms_into_a_safe_retry_callee_under_one_transaction_is_refused` |
| A decision written before it is taken (3) | `segment_restored_after_a_restart_refuses_a_version_declared_exploited` |
| A decision refused as `Unrecorded` when it can't be written (3) | `a_decision_the_platform_cannot_record_is_not_taken` |
| Marks read back on opening (3) | `segment_restored_after_a_restart_refuses_a_version_declared_exploited` |
| A marked version never started into force (3) | `segment_restored_after_a_restart_refuses_a_version_declared_exploited` |
| A symbolic link refused (the link check and the not-a-file check, which catches a link too) (4) | `record::tests::a_record_is_opened_by_its_own_name_and_never_made_by_opening` |
| A second name (hard link) refused (4) | `record::tests::a_record_is_opened_by_its_own_name_and_never_made_by_opening` |
| Opening never creating a record (4) | `record::tests::a_record_is_opened_by_its_own_name_and_never_made_by_opening` |
| Maker and shape compared (6) | `repeat with other terms under a carried out transaction is refused even with safe retry` |

### Fix round 2 (T-013-10): the re-review of `b3b1b684` (PR #291)

The re-review found the seven fixes hold and no new crash point. Its findings, and what each became:

1. **An exploit declaration the record couldn't hold was refused, and the exploited version kept running**
   (should-fix; a regression of fix round 1). After a failed write the record refuses everything, so the operator
   couldn't stop an exploited version, and dry runs and the read-only kinds still reached it. `decide` now tells a
   decision that restricts what runs from one that widens it:
   - **Restrictive** (`declare_exploited`, `revoke`, `mark_stale`): where the record can't hold it, it is taken
     in memory all the same, by its in-place form (`Lifecycle::declare_exploited_in_place`, `revoke_in_place`,
     and `mark_stale` itself), its process stopped, and answered `LifecycleRefusal::TakenUnrecorded`: not durable,
     to be taken again after a restart. The in-place form puts no version back in force: retiring the version in
     force leaves none in force, since reinstating an older one would widen what runs.
   - **Widening** (`promote`, `roll_back`, `withdraw`): refused as `Unrecorded`, nothing changed, as before.
   Tests: `an_exploit_declaration_the_record_cannot_hold_still_stops_the_version` (the reviewer's probe: the mark is
   held, the process stopped, a dry run refused `VersionExploited`, `operations_run` 0),
   `a_revocation_the_record_cannot_hold_puts_no_version_back_in_force`, and
   `a_widening_decision_the_platform_cannot_record_is_not_taken` (replacing
   `a_decision_the_platform_cannot_record_is_not_taken`).
2. **The canonical-path rule can be tested** (low). `record::tests::a_record_opened_through_a_directory_link_stays_in_the_directory_it_named`,
   the reviewer's test: a record opened through a directory link that is then pointed elsewhere still replaces and
   appends in the directory it named; without canonicalising, the entries split into the other directory.
3. **The directory-sync error can be tested** (low). A `#[cfg(test)]` thread-local hook, `DIR_SYNC_FAILS`, read by
   `sync_directory_of` as the cut-short write's hook is read by `append`.
   `record::tests::a_failed_directory_sync_is_an_error_unless_the_system_cannot_sync_one`: a failed sync refuses a
   `replace` and breaks the record, refuses a `create`, and `Unsupported` and `EINVAL` are let through.
4. **The gaps** (low): the release state is `T-013-11`'s part (3); the `ended` set and now the decision list grow
   without bound and are rewritten whole at every opening, left as gaps by the coordinator.

The fix round 1 note that two rules had no test that turns them red no longer holds: both have one now (above).

#### Defaults (fix round 2)

- A restrictive decision the record can't hold is taken in memory and reported `TakenUnrecorded`; it is not
  retried by the platform. The operator takes it again once the platform reopens on a record it can write, and
  before the release it concerns is admitted again: a version declared exploited unrecorded is unmarked after a
  restart, and would run once re-admitted. Owner of the start-up that makes this order hold: `T-013-11` part (3),
  with `F-030`'s start-up.
- `Record`'s contract says that after a failed write every later write fails until the record is opened again,
  since each `decided` entry holds only what changed since what the platform holds, so a record that recovered in
  place would hold later state without an unrecorded mark. The platform holds itself to it whatever the
  implementation does (`F-030`'s included): after one failed write it sends the record nothing more. Test:
  `after_a_failed_write_the_platform_writes_nothing_more_even_to_a_record_that_recovered` (approval lows of PR
  #291).
- Taken in place, retiring the version in force leaves none in force until the operator decides.

#### Rule-off table (fix round 2)

| Rule switched off | Test that went red |
| --- | --- |
| A restrictive decision taken when the record can't hold it (1) | `an_exploit_declaration_the_record_cannot_hold_still_stops_the_version` |
| Taken in place, no version put back in force (1) | `a_revocation_the_record_cannot_hold_puts_no_version_back_in_force` |
| A widening decision refused when the record can't hold it (1) | `a_widening_decision_the_platform_cannot_record_is_not_taken` |
| The record opened by its canonical path (2) | `record::tests::a_record_opened_through_a_directory_link_stays_in_the_directory_it_named` |
| A failed directory sync an error (3) | `record::tests::a_failed_directory_sync_is_an_error_unless_the_system_cannot_sync_one` |
| `EINVAL` let through (3) | `record::tests::a_failed_directory_sync_is_an_error_unless_the_system_cannot_sync_one` |


## T-013-11: the platform's status query, promotion on answers, releases that survive a restart (`f-013/t-013-11-status-promotion-releases`)

Findings 4 and 7 of `M3`'s milestone review of 2026-11-24 (PR #143), left open since `M2`, three parts, and
`T-020-45`'s gap 6 (the platform's own call judgement).

### Design (T-013-11)

**(1) The status query** (`REQ-ORD-1`, `INV-4`).

- `Platform::status(provider, asking, about)` (`platform.rs`). `about` is an operation the platform carried out at
  `provider` (a record of it under its transaction, or its segment in flight there); the platform makes the question
  from it, `Operation::StatusQuery {transaction: asking, provider, about: <its transaction>}`, in the same deployment,
  protocol version, maker and time, and hands it through the ordinary `handle_in` on the version `about`'s segment
  runs on, so the module is asked only if its release lists `status_query` in `handles` (else `NotHandled`, never
  run), the answer is judged against the result schema and signed with the provider's statements key as any result is.
- It moves nothing: `status_query` joins the kinds that only ask (`READ_ONLY_KINDS`), so it is no operation carried
  out, no `carrying` entry, no record under its transaction; the transaction asked about, its segment and its
  record are as they were. The test reads the record's lines for the asking transaction and finds none.
- Refused, nothing run: `Unavailable` where the platform carried nothing out under `about` at this provider;
  `DuplicateTransaction` where `asking` is `about`'s transaction or one the platform holds a record of, a segment
  of, or ended (a question has a transaction of its own).
- **Signing the nested result** (`proxy/signing.rs`, a `DEC-20` path; `T-020-45`'s gap 12 in part). The signer refused
  every result-shaped statement nested in the provider's name (`NestedOfAnotherShape`), so a status answer carrying
  the provider's own result, the case the question exists for, could never pass the platform. A result nested in the
  answer to a status question is now signed if it is a result of the transaction the question names in `about`, and
  of an operation (never a status answer or a hold); anything else, and a result nested in answer to another kind of
  question, is `NestedNotAsked`. `nested_statements_of_others_are_kept_and_of_another_shape_refused`'s last
  assertion moves from `NestedOfAnotherShape` to `NestedNotAsked` for the same input (it is now judged by what was
  asked, not by its shape alone). Terms and holds nested by a provider stay `T-020-47`'s gap 12.
- **Not edited here:** `core-orch`'s `impl Modules for Platform` (`components/core-orch/src/segments/ports.rs`) still
  answers the trait's default, `NotHandled`, because that file belongs to the wave's `core-orch` authors. What it
  needs is one method, owner `T-020-47` (whose row asks `Modules::status`):

```rust
fn status(&mut self, effect: &Effect, provider: &Id, asking: &Id, about: &Statement<Operation>) -> Answered {
    let _permitted = effect;
    let Some(asking) = Identifier::new(asking.as_str()) else { return Answered::Refused(PlatformRefusal::NotHandled) };
    match Platform::status(self, provider.as_str(), &asking, about) {
        Handled::Answered(result) => serde_json::to_vec(&*result)
            .map(Answered::Bytes)
            .unwrap_or(Answered::Refused(PlatformRefusal::OutcomeUnknown)),
        Handled::Refused(refusal) => Answered::Refused(refusal),
    }
}
```

**(2) Promotion counts answers** (`REQ-MDL-4`, ADR-0014; form item `promotion-observations`, default taken).

- `Lifecycle::promote` reads `staged_counts.answered`, not `handled()`. What counts as answered (`is_an_answer`,
  `platform.rs`): the platform passed on the module's result and its outcome is not `refused`, `unknown` or
  `not_known`: done or failed, and the answers to a question (a dry run's quote, a balance, a judgement). A refusal the
  platform made after the module ran (bad or oversized output, a limit, a stop), a result that is the module's own
  refusal, and an outcome unknown are counted as `refused`, reported apart in the staging's summary
  (`Observation::staged_counts.refused`, which existed). An operation refused before it reached a running process
  counts as neither, as before.
- `TooFewObserved.observed` is the answers. The module's docs and `Staging::new`'s say so. ADR-0014's staging is
  otherwise unchanged.
- Existing tests moved: `module_lifecycle_properties.rs`'s model counts only the observations that were answers (it
  drew them at random); no other count moved.

**(3) Releases survive a restart** (`REQ-MDL-4`, `INV-8`).

- **The entries** (`record.rs`). `releases`: one provider's releases as they stood after a change, the last one
  written holding: the accepted versions with who accepted each and when, in the order they were accepted (what a
  restart admits them again in), the version in force, the versions put in force in order, and the staged release
  with its start, policy, share taken and the counts of the staged version and of the version in force beside it.
  `began`: a segment as it began, with the releases entry where beginning it changed the staging's counts (the
  segments begun and the share taken), in one write. `decided` gains `releases`: the entries a promotion, a roll
  back or a revocation of the version in force changed, written in the decision's one entry. `ran` gains `answered`,
  written while a release is staged: the count towards the staging, replayed on opening. The withdrawn versions
  are the `withdrawn` decisions the record already held, read back into the provider's withdrawn set.
- **Written before they take effect.** `accept_and_start` (replacing `launch`) starts the process, builds the next
  lifecycle on a clone, writes the releases entry for it, and only then takes it: where the write fails the process
  is dropped, the release isn't accepted and `accept` answers `None`, the release still waiting. An automatic
  acceptance (`admit`) the record can't hold is left waiting as one a person gives, its artefact saying so.
- **Opening** (`Platform::open`) reads the entries back, and `held()` writes a releases entry per provider into the
  snapshot it replaces the record with. A restarted platform holds each provider's state (`Lifecycle::release_state`,
  `restore_release_state`) and no process.
- **Admitting again.** A release whose version the record holds as accepted is accepted as it was (the same
  `Accepted {by, at}`, so the artefact is the one it was), is never refused as `ReleaseStaged`, and is put where the
  record says it stood (`start_restored`): the version in force in its place, the staged version and a version a
  segment in flight runs on beside it, none of them staged as a new upgrade and none counted again. A version
  declared exploited is held and runs nothing, as in `T-013-10`. A version withdrawn while it waited is refused
  `VersionAlreadyAdmitted`.
- **Tests.** `module_status.rs` (the two named, and the version the question runs on); `module_lifecycle.rs` (the two
  promotion tests); `module_record.rs` (`release_in_force_and_staged_survives_a_platform_restart`, and
  `a_release_the_record_cannot_hold_is_not_accepted`); `proxy/tests/calls.rs` and `proxy/signing.rs` (gap 6, the
  nested result).

**Gap 6 of `T-020-45`** (the platform's call judgement). `judge_call` (`proxy/calls.rs`) refuses a status question and
a hold request as `undeclared_kind`, however a declaration or a route names them, as it does `holding_balance`.

### Defaults (T-013-11)

- A result counts as an answer unless its outcome is `refused`, `unknown` or `not_known`: the form's reading of "done or
  failed, not refused or left unanswered", wide enough to take a quote or a judgement as an answer to the question
  that asked for one. Reversible by changing `is_an_answer`.
- A release the record can't hold isn't accepted: the platform stays as it was and the release waits. Fail closed.
- `Platform::status` answers `Unavailable` for a transaction the platform kept no record of at this provider: after a
  segment ends or an unsegmented record is freed, there is nothing to ask about.
- The staging policy is the deployment's setting at each start (`Platform::set_staging`), not the record's; the
  counts and the staged release under it are the record's.
- A version in force or staged in the record that doesn't start when admitted again leaves force or staging, as one
  that doesn't start at its acceptance does, until the operator decides.

### Gaps (T-013-11)

- `core-orch`'s `impl Modules for Platform::status`, above. Owner: `T-020-47`.
- Core keeps a status answer as the provider's statement only after holding it to its question with
  `check_status_answer`; the platform checks the answer's shape, transaction and signer but not its `about`.
  Owner: `T-020-47`.
- The question's maker signature is the original operation's (the platform verifies none). A caller that signs its
  own questions hands the platform the one it signed through `Platform::handle`. Owner: `T-020-47`.
- A restart re-admits releases in the order they were accepted, as the record lists them; the start-up that does
  it, and the order of providers, is `F-030`'s. A release the start-up never admits again has no process, and
  operations for it are `Unavailable`; the version in force in the record is not put in force by another.
- The record grows by one releases entry per acceptance, per promotion and per segment that changes a staging's
  counts, and is rewritten whole at each opening, as the `ended` set is (`T-013-10`'s gap). Owner: `F-013`.
- `Slot`'s `pending` release (waiting for its acceptance) is not in the record: it is admitted again.
- Terms and holds a provider nests in its answers are still passed on unsigned or refused by the signer: `T-020-47`.
- `DEC-20` paths: `components/prov/src/module/` and `components/prov/src/proxy/` wait for the owner's click at the
  release into `main`.

### Rule-off table (T-013-11)

Each rule switched off alone on the tree of the row's commit's parent of this note (script `ruleoff.py`, with
`ruleoff2.py` for the reruns, in the row's scratch folder); the named test went red on an assertion each time. The
two status tests and the restart test passed on their first run, since their code was written beside them: what
shows each is red is the table.

| Rule switched off | Test that went red |
| --- | --- |
| `status_query` among the kinds that only ask (a question is no operation carried out) | `status_question_is_handed_to_a_module_that_handles_it_and_moves_nothing` |
| A question about a transaction the platform carried nothing out of is refused | `status_question_is_handed_to_a_module_that_handles_it_and_moves_nothing` |
| A question has a transaction of its own (the asking one is no other's) | `status_question_is_handed_to_a_module_that_handles_it_and_moves_nothing` |
| The question runs on the version its transaction's segment runs on | `status_question_runs_on_the_version_its_transaction_runs_on` |
| The module's `handles` list gates the question (existing check, `handle_on`) | `module_that_handles_no_status_question_is_not_asked_one` |
| Promotion reads the answers, not the operations handled | `staged_version_that_refuses_every_operation_is_never_promoted` |
| A result whose outcome is a refusal, unknown or not known is no answer | `staged_version_is_promoted_on_the_operations_it_answered` |
| `judge_call` bars `status_query` and `hold` | `status_query_and_hold_are_refused_as_a_declared_call_and_as_a_kind_a_route_names` |
| The signer signs a nested result only for the transaction asked about | `result_nested_in_a_status_answer_is_signed_only_for_the_transaction_asked_about` |
| The signer refuses a nested status answer or hold | `result_nested_in_a_status_answer_is_signed_only_for_the_transaction_asked_about` |
| An acceptance written to the record before it takes effect | `accepted_releases_are_held_by_a_platform_restarted_at_once` |
| Releases entries read back | `accepted_releases_are_held_by_a_platform_restarted_at_once` |
| A re-admitted release isn't refused as staged | `release_in_force_and_staged_survives_a_platform_restart` |
| Counts replayed from `ran` entries | `release_in_force_and_staged_survives_a_platform_restart` |
| A segment's counts written with it | `release_in_force_and_staged_survives_a_platform_restart` |
| A decision's releases written with it | `release_in_force_and_staged_survives_a_platform_restart` |
| Withdrawn versions read back from the decisions | `release_in_force_and_staged_survives_a_platform_restart` |
| An acceptance refused where the record can't hold it | `a_release_the_record_cannot_hold_is_not_accepted` |
| An automatic acceptance the record can't hold left waiting | `a_release_the_record_cannot_hold_is_not_accepted` |

One rule-off found a hidden redundancy rather than a missing test: with the releases entries not read back, the
restart test still passed, because a later `began` entry holds the same state. `accepted_releases_are_held_by_a_platform_restarted_at_once`
restarts the platform with nothing after the acceptances, which is what turns it red. Likewise the question's
"own transaction" check is three conditions that cover one another (the asked-about transaction holds a segment, a
record, or is `asking`): the table switches the whole check off.

### Fix round 1 (T-013-11): the security review of `5039ba3a` (PR #298)

One blocker and four should-fixes, each reproduced by a probe, and two lows. What each became:

1. **Blocker: a restored acceptance wasn't bound to the release accepted.** `AcceptedEntry` holds the accepted
   assertion's digest. A release is restored only if its assertion's digest is the recorded one; any other
   assertion under a version the record holds as accepted is refused `VersionAlreadyAdmitted` (a version names one
   release), so it never inherits the person's acceptance and its attribution. Test:
   `a_different_release_under_an_accepted_version_is_refused_after_a_restart`.
2. **A version declared exploited unrecorded came back after a restart.** The platform now writes a `closed` entry
   when it is dropped with a record it could write to the end and nothing waiting. `Platform::open` on a record whose
   last entry isn't `closed` (and isn't empty) holds every provider with a release in force or staged out of force:
   nothing of it runs (`handle`, `begin_segment`, a started process), whatever is admitted again, until the operator
   takes any restrictive decision again and calls the new `Platform::confirm(provider, by, at)`, a recorded decision
   (`DecisionKind::Confirmed`), after which the releases start. A platform dropped with a provider unconfirmed writes
   no `closed`, so the next opening finds the same. Tests:
   `an_exploit_declaration_the_record_could_not_hold_is_not_undone_by_a_restart`,
   `releases_of_a_platform_that_ended_unclean_run_once_the_operator_confirms_them`. **Cost:** any end the record
   can't vouch for (a crash, a full disk) now takes the operator's confirmation, not only the one with an exploit
   unrecorded: the record can't tell them apart. `a_repeat_after_the_platform_restarts_moves_nothing_twice` (its
   stopped-while-the-module-ran half) confirms after reopening, the one existing test that changed.
3. **Status answers counted towards promotion.** A status question counts for nothing: neither observed nor written
   as `answered` in `ran`. The module's docs say that a dry run a staged version answers counts as an answer, as the
   form's reading takes it. Test: `status_questions_count_for_nothing_towards_a_promotion`.
4. **`handle` skipped `status`'s checks.** `Platform::handle` refuses a status question (`NotHandled`, the module
   not run); `Platform::status` is the one way in. A caller that signs its own questions has no path yet (gap,
   `T-020-47`). Test: `status_question_handed_to_handle_is_refused_and_the_module_not_run`.
5. **The signer tied a status answer to the transaction id only.**
   - (a) The answer's `about` and its own transaction must be the question's: `result_matches` (platform) and
     `sign_result` (`SigningError::AnswerOfAnotherQuestion`).
   - (b) `Answering` carries `about: Option<AboutAsked>`: the operation the platform carried out under the
     transaction asked about, taken from its record (the caller's only where a segment holds it and no record does),
     and the final answer it holds. A nested result must be of that operation's kind and transaction; with no
     operation held, none is nested.
   - (c) Where the held answer is final (`succeeded` or `failed`), a nested result that says anything else (compared
     without signature values) isn't signed.
   Tests: `result_nested_in_a_status_answer_is_signed_only_for_the_transaction_asked_about` (other `about`, other
   `transaction`, other kind, status or hold nested, no operation held),
   `nested_result_that_differs_from_a_final_answer_the_platform_holds_is_not_signed`,
   `a_status_answer_about_another_transaction_is_no_result_of_the_question`. The call into the signer from
   `handle_on` is exercised end to end by the role stand-in only for a refusal; an answer nesting a result needs a
   module that does, which none of the stand-ins is: the signer's own tests are the evidence.
6. **Low: an unsegmented question ran on the version in force now.** `Carried` and its record entry keep the version
   that carried the operation out; the question runs on it, and is `Unavailable` where the record doesn't keep it or
   no process runs it. Test: `status_question_about_an_unsegmented_transaction_runs_on_the_version_that_carried_it`.
   `status_question_runs_on_the_version_its_transaction_runs_on` now reads `Platform::operations_run_on`, since the
   status question no longer counts.
7. **Low: a status answer about a reversal confirmation.** Supported: a statement nested in a status answer is judged
   against the operation asked about, so a nested confirmation is signed for its segment, asset and amount. Test:
   `confirmation_nested_in_a_status_answer_is_judged_against_the_operation_asked_about`.

Defaults: the operator's confirmation after an unclean end is per provider and recorded; reversible by confirming.

#### Rule-off table (fix round 1)

| Rule switched off | Test that went red |
| --- | --- |
| A restored release is accepted only if its assertion is the one accepted (1) | `a_different_release_under_an_accepted_version_is_refused_after_a_restart` |
| A record not closed whole holds the releases out of force (2) | `an_exploit_declaration_the_record_could_not_hold_is_not_undone_by_a_restart` |
| An unconfirmed provider's segments are refused (2) | same |
| An unconfirmed release isn't started on admission (2) | same |
| The clean close is written (2) | `releases_of_a_platform_that_ended_unclean_run_once_the_operator_confirms_them` |
| A status question counts for nothing towards a promotion (3) | `status_questions_count_for_nothing_towards_a_promotion` |
| `handle` refuses a status question (4) | `status_question_handed_to_handle_is_refused_and_the_module_not_run` |
| A status answer is about the transaction asked about, in `result_matches` (5a) | `a_status_answer_about_another_transaction_is_no_result_of_the_question` |
| A status answer is to the question asked, in the signer (5a) | `result_nested_in_a_status_answer_is_signed_only_for_the_transaction_asked_about` |
| A nested result is of the kind of the operation carried out (5b) | same |
| A nested result is the final answer held (5c) | `nested_result_that_differs_from_a_final_answer_the_platform_holds_is_not_signed` |
| A nested confirmation is judged against the operation asked about (7) | `confirmation_nested_in_a_status_answer_is_judged_against_the_operation_asked_about` |
| An unsegmented question runs on the version that carried it (6) | `status_question_about_an_unsegmented_transaction_runs_on_the_version_that_carried_it` |

One rule has no test of its own: the refusal of an unconfirmed provider's operations in `handle_in` stays green with it
off, because an unconfirmed provider has no process to run (the start on admission is refused too). It is a second
layer for a process that exists some other way, not a rule a test can reach. The first draft of the final-answer test
parsed no failed result and passed vacuously; it parses one now.

### Fix round 2 (T-013-11): the re-review of `bbbe3ddd` (PR #298)

1. **Blocker: core-authz's scan pins every state-changing public method of the platform.** `confirm` is listed in
   `CARRY_OUT` (`change_catalogue: confirm_releases`: it puts releases back in force), `status` in
   `CARRIES_OUT_NOTHING` (a read that moves nothing). `components/core-authz/src/engine/entry_points.rs` is a `DEC-20`
   path. Core's path to `confirm` therefore has to go through a decision.
2. **`confirm` is a sixth lifecycle action** beside `accept`, `withdraw`, `revoke`, `declare_exploited`, `mark_stale`
   (and `promote`, `roll_back`): it needs its own operator permission, and the platform checks none itself, as for the
   others. **Gap, owners:** `T-021-15` (the permission and its fixed rule) with `T-020-49` for Core's decided path.
   **Missing contract shape (not edited here):** a `CatalogueChange` for confirm in core-internal, as `RollBack` is
   and `accept`, `withdraw`, `revoke`, `declare_exploited`, `mark_stale` will be (`T-020-45`'s round 5): a variant
   naming the provider (and the versions confirmed), so the log can record the decided action.
3. **A release admitted while its provider waits for confirmation** is accepted and placed as a restored one is (so a
   restart finds it), with no process and no operation, and runs when the operator confirms. Test:
   `a_release_admitted_while_its_provider_waits_for_confirmation_starts_nothing`.
4. **A segment begun and nothing carried out in it** no longer takes the operation's kind from the caller: the
   question is `Unavailable`. Test: `status_question_about_a_segment_with_nothing_carried_out_in_it_is_refused`.
5. **A held final refusal binds the nested result too** (`refused` with class `final`); one that lifts doesn't. Test:
   `nested_result_that_differs_from_a_final_answer_the_platform_holds_is_not_signed`.
6. **End-to-end evidence for the signer's 5b and 5c** (the nested result's kind and the held final answer): none of
   the stand-ins nests a result, so the signer's tests stand for them here; `T-020-47`'s scenario "segment left
   unknown is asked its status and ends failed on the provider's answer" is the end-to-end one, once a simulated
   provider answers a status question with its result.

#### Rule-off table (fix round 2)

| Rule switched off | Test that went red |
| --- | --- |
| A provider waiting for confirmation starts no new release (3) | `a_release_admitted_while_its_provider_waits_for_confirmation_starts_nothing` |
| The operation's kind is the record's, never the caller's (4) | `status_question_about_a_segment_with_nothing_carried_out_in_it_is_refused` |
| A refusal for good binds the nested result (5) | `nested_result_that_differs_from_a_final_answer_the_platform_holds_is_not_signed` |
| `confirm` and `status` pinned in the scan (1) | `carry_out_is_pinned` in `cargo test -p rutent-core-authz --lib`: red at `bbbe3ddd` as the re-review found, not switched off again here; green now |
