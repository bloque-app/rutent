# F-025 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-025-01

`EXP-2`'s `G-02` read again, on 2026-11-17: the Wasmtime pin against every advisory published up to that day, and the
x86-64 AVX advisory against the engine's explicit baseline target.

- **Outcome.** The pin is `wasmtime = "=48.0.8"` (`components/prov/Cargo.toml`, `Cargo.lock`, `engine::WASMTIME_VERSION`),
  `DEC-27`'s LTS line. 94 advisories are published against the `bytecodealliance/wasmtime` repository, the last of them on
  2026-11-16; none is dated 2026-11-17. **Six of them, all published on 2026-11-16, have vulnerable ranges that include
  48.0.8** (`GHSA-43j2-gcp4-m22g`, `GHSA-wvgq-g5hf-6gm6`, `GHSA-rc4p-x33f-vpr2`, `GHSA-2674-w29h-wqvj`,
  `GHSA-hj27-mpw9-ff4c`, `GHSA-xrvr-8678-5wp8`), each fixed in 48.0.9 (crates.io, 2026-11-16). `T-013-01`'s read of
  2026-11-09 found none, correctly: they did not exist then. **None is reachable with the engine's configuration**: each
  needs a proposal or a host function the engine does not have (reasons in the table). So the pin is not exploitable
  today, and the bump is a row for a clean reading, not a fix: `T-025-06`, a new row of this feature, bumps to 48.0.9.
  No row of the bump is done here.
- **Sources.** GitHub's advisory API for the repository (`repos/bytecodealliance/wasmtime/security-advisories`, state
  `published`, 94 entries), GitHub's global advisory database for the `wasmtime`, `cranelift-codegen`,
  `cranelift-frontend`, `cranelift-native`, `wasmtime-wasi`, `wasmtime-environ`, `wasmtime-internal-cranelift`,
  `wasmparser` and `wasmtime-cranelift` crates (47 entries, every one among the 94), and RustSec's `advisory-db`
  (`crates/wasmtime`, `crates/wasmtime-wasi`, `crates/wasmtime-wasi-http`, `crates/cranelift-codegen`: 85 entries with a
  GHSA alias, every one among the 94; no `cranelift-*` crate other than `cranelift-codegen`'s 2021 entry has one). Each
  range is the API's, checked by hand against the patched versions; a range that reads open is said so. **RustSec lags:**
  its newest entry is dated 2026-10-29, and none of the nine advisories of 2026-11-16 that name `wasmtime`, `wasmtime-wasi`
  or `wasmtime-wasi-http` is in it yet, so a scanner fed by RustSec alone (`cargo audit`) does not report them. The ones
  that include 48.0.8 are therefore found only on GitHub's side; `T-025-06` leaves the pin clear of them.
- **What is in the closure.** `wasmtime-wasi` and `wasmtime-wasi-http` are not (no WASI): an advisory naming only them is not
  affected whatever its range. The cargo features are `cranelift`, `runtime`, `component-model`, `std`, `anyhow`: no `gc`,
  `threads`, `async`, `component-model-async`, `pooling-allocator`, `profiling`, `cache`, `wat`, `coredump`, and no Winch.
  The engine's WebAssembly features are all off but `FEATURES` (floats, sign extension, bulk memory, reference types, the
  component model): no GC, no threads, no component-model async, no SIMD, no tail calls, no exceptions.
- **The AVX advisory, `GHSA-vc8c-j3xm-xj73`** (CVE-2026-24116, published 2026-01-27, medium). `f64.copysign` on x86-64
  with AVX loads 16 bytes where 8 are needed; the advisory's range reads open (`>= 29.0.0`) because only the 36, 40 and
  41 lines were patched (36.0.5, 40.0.3, 41.0.1), the fix being on main. Verdict: **not affected, by four independent
  reasons.** (1) The fix is in the pin: Cranelift 0.135.8's `fcopysign` lowering (`x64/lower.isle`) forces the operand into a
  register "so we don't sink a 128-bit load". (2) The engine compiles for an explicit baseline target and enables no AVX
  feature (the test below, which makes the x86-64 code too, on an arm64 host). (3) The advisory's three cases: with no
  guard region, which is the engine's (`memory_guard_size(0)`), the extra bytes are loaded and not visible to the guest,
  and the advisory says that case is not a security issue. (4) The segfault case needs signals-based traps off, which the
  engine does not set. Reasons 2 to 4 hold for any later Wasmtime too, so a bump can't reopen it unseen except by turning
  on AVX, which the test refuses.
- **The test, `engines_compile_target_enables_no_avx_feature`** (`components/prov/tests/module_engine_target.rs`,
  `THR-85`, `DEC-27`). It reads the engine's real configuration: `engine::engine()` for the build's own target and, for
  `x86_64-unknown-linux-gnu` and `x86_64-apple-darwin`, an engine made from `engine::config_for(..)` (new: `config()` is it
  with `TARGET`; `engine_from` refuses another target's settings by design, so these use `Engine::new`). It makes machine
  code of an empty module with each. Wasmtime writes the target triple and every Cranelift CPU flag a piece of machine code
  was made under into its `.wasmtime.engine` section, and has no accessor for them, so the test parses the section (ELF,
  then the head of the `postcard` metadata). It asserts the target is the one named; that no flag with `avx` in its name
  is on; and that no CPU flag at all is on beyond what Cranelift's own baseline for the triple has
  (`cranelift_codegen::isa::lookup`). The last is what the explicit target buys: with it set, Cranelift never reads the
  compiling CPU. Test-only dev-dependencies, exact pins: `cranelift-codegen =0.135.8` with its `x86` and `arm64` features
  (they put both backends into test builds, so the x86-64 code is made on an arm64 machine; Wasmtime's own build carries
  only the host's), `object =0.39.1`, `postcard =1.1.3`, `target-lexicon =0.13.5`; all four already in `Cargo.lock` at
  those versions.
- **A second test, `engine_leaves_off_the_proposals_the_runtime_advisories_of_2026_11_16_need`,** holds the ground of the
  GC, threads and component-model-async verdicts below: none of the three is in `engine::FEATURES`, and a core module that
  declares a non-final function type (a `sub` type, which only the GC proposal allows) is refused while the same module
  with a final one is compiled.

### The advisories, each by its vulnerable range and its patched versions

Ranges are the API's text for the `wasmtime` crate (for `wasmtime-wasi` and `wasmtime-wasi-http` advisories, theirs; the
verdict says so). "RustSec" is the id of the `advisory-db` entry that carries the GHSA as an alias. All 70 advisories
published in 2026, in order of their dates:

| Advisory | RustSec | Published | Severity | Subject | Vulnerable range | Patched | Verdict for 48.0.8 |
|---|---|---|---|---|---|---|---|
| `GHSA-vc8c-j3xm-xj73` (CVE-2026-24116) | RUSTSEC-2026-0006 | 2026-01-27 | medium | Wasmtime segfault or unused out-of-sandbox load with f64.copysign operator on x8... | >= 29.0.0 | 36.0.5, 40.0.3, 41.0.1 | not affected. The range is open (`>= 29.0.0`) because only the 36, 40 and 41 lines were patched; the fix is on main and in Cranelift 0.135.8's lowering (`fcopysign` forces the operand into a register, `x64/lower.isle`). Three further reasons, each enough: it needs x86-64 with AVX and the compile target enables none (`engine's compile target enables no avx feature`); it needs a guard region to fault on or none to read past, and the engine's guard is 0 bytes, which the advisory calls not a security issue; and it needs signals-based traps off, which they are not |
| `GHSA-243v-98vx-264h` (CVE-2026-27572) | RUSTSEC-2026-0021 | 2026-02-24 | medium | Panic adding excessive fields to a wasi:http/types.fields instance | < 24.0.6; >= 25.0.0, < 36.0.6; >= 37.0.0, < 40.0.4; >= 41.0.0, < 41.0.4 | 24.0.6; 36.0.6; 40.0.4; 41.0.4 | not affected: the range ends before the 48 line |
| `GHSA-xjhv-v822-pf94` (CVE-2026-27195) | RUSTSEC-2026-0022 | 2026-02-24 | medium | Panic when dropping a [Typed]Func::call_async future | >= 39.0.0, < 40.0.4, >= 41.0.0, < 41.0.4 | 40.0.4, 41.0.4 | not affected: the range ends before the 48 line |
| `GHSA-852m-cvvp-9p4w` (CVE-2026-27204) | RUSTSEC-2026-0020 | 2026-02-24 | medium | Guest-controlled resource exhaustion in WASI implementations | < 24.0.6; >= 25.0.0, < 36.0.6; >= 37.0.0, < 40.0.4; >= 41.0.0, < 41.0.4 | 24.0.6; 36.0.6; 40.0.4; 41.0.4 | not affected: the range ends before the 48 line |
| `GHSA-394w-hwhg-8vgm` (CVE-2026-35195) | RUSTSEC-2026-0091 | 2026-04-09 | medium | Out-of-bounds write or crash when transcoding component model strings | <= 24.0.6, >= 25.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 24.0.7, 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-hfr4-7c6c-48w2` (CVE-2026-34983) | RUSTSEC-2026-0090 | 2026-04-09 | low | Use-after-free bug after cloning wasmtime::Linker | 43.0.0 | 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-6wgr-89rj-399p` (CVE-2026-34988) | RUSTSEC-2026-0088 | 2026-04-09 | low | Data leakage between pooling allocator instances | >= 28.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-m9w2-8782-2946` (CVE-2026-34945) | RUSTSEC-2026-0086 | 2026-04-09 | low | Host data leakage with 64-bit tables and Winch | >= 25.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-q49f-xg75-m9xw` (CVE-2026-34946) | RUSTSEC-2026-0089 | 2026-04-09 | medium | Host panic when Winch compiler executes table.fill | >= 25.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-qqfj-4vcm-26hv` (CVE-2026-34944) | RUSTSEC-2026-0087 | 2026-04-09 | medium | Wasmtime segfault or unused out-of-sandbox load with f64x2.splat operator on Cra... | <= 24.0.6, >= 25.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 24.0.7, 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-f984-pcp8-v2p7` (CVE-2026-35186) | RUSTSEC-2026-0094 | 2026-04-09 | medium | Improperly masked return value from table.grow with Winch compiler backend | >= 25.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-jxhv-7h78-9775` (CVE-2026-34942) | RUSTSEC-2026-0092 | 2026-04-09 | medium | Panic when transcoding misaligned component model UTF-16 strings | <= 24.0.6, >= 25.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 24.0.7, 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-m758-wjhj-p3jq` (CVE-2026-34943) | RUSTSEC-2026-0085 | 2026-04-09 | medium | Panic when lifting flags component value | <= 24.0.6, >= 25.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 24.0.7, 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-jhxm-h53p-jm7w` (CVE-2026-34971) | RUSTSEC-2026-0096 | 2026-04-09 | critical | Miscompiled guest heap access enables sandbox escape on aarch64 Cranelift | >= 32.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-xx5w-cvp6-jv83` (CVE-2026-34987) | RUSTSEC-2026-0095 | 2026-04-09 | critical | Wasmtime with Winch compiler backend may allow a sandbox-escaping memory access | >= 25.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-hx6p-xpx3-jvvv` (CVE-2026-34941) | RUSTSEC-2026-0093 | 2026-04-09 | medium | Heap OOB read in component model UTF-16 to latin1+utf16 string transcoding | <= 24.0.6, >= 25.0.0, <= 36.0.6, >= 37.0.0, <= 42.0.1, 43.0.0 | 24.0.7, 36.0.7, 42.0.2, 43.0.1 | not affected: the range ends before the 48 line |
| `GHSA-p8xm-42r7-89xg` (CVE-2026-44216) | RUSTSEC-2026-0114 | 2026-04-30 | medium | Panic when allocating a table exceeding the size of the host's address space | >= 30.0.0, <= 36.0.7, >= 37.0.0, <= 43.0.1, 44.0.0, | 36.0.8, 43.0.2, 44.0.1 | not affected: the range ends before the 48 line |
| `GHSA-2r75-cxrj-cmph` (CVE-2026-47261) | RUSTSEC-2026-0149 | 2026-05-21 | high | WASI path_open(TRUNCATE) bypasses FilePerms::WRITE host restriction | < 44.0.1 | 45.0.0, 44.0.2, 36.0.10, 24.0.9 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-3p27-qvp9-27qf` (CVE-2026-54786) | RUSTSEC-2026-0182 | 2026-06-15 | low | Leak in WASIp1 fd_renumber implementation | < 24.0.10, >= 25.0.0, < 36.0.11, >= 37.0.0, < 44.0.3, 45.0.0, 45.0.1 | 24.0.10, 36.0.11, 44.0, 3, 45.0.2 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-4ch3-9j33-3pmj` (CVE-2026-58494) | RUSTSEC-2026-0188 | 2026-06-24 | medium | WASI hard links and renames bypass wasmtime-wasi's FilePerms for destination | < 24.0.11, >= 25.0.0, < 36.0.12, >= 37.0.0, < 45.0.3, 46.0.0 | 24.0.11, 36.0.12, 45.0.3, 46.0.1 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-hgjw-h833-99q9` (CVE-2026-115393) | RUSTSEC-2026-0222 | 2026-07-31 | low | Stores can mix up type indices between engines | <= 24.0.11; >= 25.0.0, <= 36.0.12; >= 37.0.0, <= 46.0.1; 47.0.0, 47.0.1, 47.0.2 | 24.0.12; 36.0.13; 46.0.2; 47.0.3 | not affected: the range ends before the 48 line |
| `GHSA-2hw9-mc66-jc2q` (CVE-2026-104855) | RUSTSEC-2026-0223 | 2026-07-31 | low | Preemption and traps during bulk operations enable breaking internal VM state | 46.0.0, 46.0.1, 47.0.0, 47.0.1, 47.0.2 | 46.0.2, 47.0.3 | not affected: the range ends before the 48 line |
| `GHSA-vqjp-4c8c-hfgg` (CVE-2026-115392) | RUSTSEC-2026-0269 | 2026-08-20 | high | Filesystem sandbox escape when paths or symlinks contain trailing slashes | <= 24.0.12; >= 25.0.0, <= 36.0.13; >= 37.0.0, <= 46.0.2; >= 47.0.0, <= 47.0.3 | 24.0.13; 36.0.14; 46.0.3; 47.0.4 | not affected: the range ends before the 48 line |
| `GHSA-x84v-gj2h-g759` (CVE-2026-115391) | RUSTSEC-2026-0268 | 2026-08-20 | medium | Guest controlled-size host heap allocation through WASIp3 streams | 46.0.0, 46.0.1, 46.0.2; 47.0.0, 47.0.1, 47.0.2, 47.0.3 | 46.0.3; 47.0.4 | not affected: the range ends before the 48 line |
| `GHSA-j2g9-4prp-pf6h` (CVE-2026-115387) | RUSTSEC-2026-0314 | 2026-09-24 | medium | wasmtime-wasi: guest can panic host through filesystem datetime overflow (wasip1... | < 36.0.16; >= 37.0.0, < 48.0.3; 49.0.0 | 36.0.16; 48.0.3; 49.0.1 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-c9gc-w9vx-w86p` (CVE-2026-115388) | RUSTSEC-2026-0313 | 2026-09-24 | medium | wasmtime-wasi-http: outgoing HTTP body write allows guest-driven host memory exh... | < 36.0.16; >= 37.0.0, < 48.0.3; 49.0.0 | 36.0.16; 48.0.3; 49.0.1 | not affected: `wasmtime-wasi-http` is not in the closure (no WASI) |
| `GHSA-jqpg-j7w6-42pr` (CVE-2026-115389) | RUSTSEC-2026-0316 | 2026-09-24 | low | Dynamic record lifting can allocate beyond the hostcall fuel limit | <= 36.0.15; >= 37.0.0, <= 48.0.2; 49.0.0 | 36.0.16; 48.0.3; 49.0.1 | not affected: fixed in 48.0.3, which the pin carries |
| `GHSA-m63x-6p34-q65x` (CVE-2026-115390) | RUSTSEC-2026-0315 | 2026-09-24 | medium | call_ref and exception catch can drop some fuel accounting, leading to exponenti... | >= 47.0.0, < 48.0.3; 49.0.0 | 48.0.3; 49.0.1 | not affected: fixed in 48.0.3, which the pin carries |
| `GHSA-96f6-r43r-8c24` (CVE-2026-115386) | RUSTSEC-2026-0323 | 2026-10-02 | low | fd_readdir copies uninitialized struct padding into guest memory | < 36.0.17; >= 37.0.0, < 48.0.4; >= 49.0.0, < 49.0.2 | 36.0.17; 48.0.4; 49.0.2 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-hw8m-q44c-ggrf` (CVE-2026-115379) | RUSTSEC-2026-0326 | 2026-10-02 | medium | Rooting for GC values live across try_call may be missing, causing GC heap corru... | >= 47.0.0, < 48.0.4; >= 49.0.0, < 49.0.2 | 48.0.4; 49.0.2 | not affected: fixed in 48.0.4, which the pin carries |
| `GHSA-j366-h8gg-77pm` (CVE-2026-115380) | RUSTSEC-2026-0321 | 2026-10-02 | medium | WASI preview 0 implementation of poll_oneoff circumvents fuel consumption | < 36.0.17; >= 37.0.0, < 48.0.4; >= 49.0.0, < 49.0.2 | 36.0.17; 48.0.4; 49.0.2 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-gqmc-89g8-p25r` (CVE-2026-115381) | RUSTSEC-2026-0322 | 2026-10-02 | medium | Excessive allocated memory on the host when guests don't have stdio | < 36.0.17; >= 37.0.0, < 48.0.4; >= 49.0.0, < 49.0.2 | 36.0.17; 48.0.4; 49.0.2 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-w4qr-p94g-mjhv` (CVE-2026-115382) | RUSTSEC-2026-0320 | 2026-10-02 | medium | Wasmtime wasi:http implementation panics with a zero timeout supplied | >= 46.0.0, < 48.0.4; >= 49.0.0, < 49.0.2 | 48.0.4; 49.0.2 | not affected: `wasmtime-wasi-http` is not in the closure (no WASI) |
| `GHSA-cfhf-m2cr-62wj` (CVE-2026-115383) | RUSTSEC-2026-0325 | 2026-10-02 | medium | Mis-typed WebAssembly tag imports can lead to GC heap corruption | >= 47.0.0, < 48.0.4; >= 49.0.0, < 49.0.2 | 48.0.4; 49.0.2 | not affected: fixed in 48.0.4, which the pin carries |
| `GHSA-32h6-97mm-8q3c` (CVE-2026-115384) | RUSTSEC-2026-0327 | 2026-10-02 | critical | Wasmtime component async-lifted callback result count is unvalidated, causing a ... | >= 39.0.0, < 48.0.4; >= 49.0.0, < 49.0.2 | 48.0.4; 49.0.2 | not affected: fixed in 48.0.4, which the pin carries |
| `GHSA-mr2v-56j5-cmfc` (CVE-2026-115385) | RUSTSEC-2026-0324 | 2026-10-02 | medium | Guest can panic host through filesystem timestamp before the epoch on wasip3 | >= 46.0.0, < 48.0.4; >= 49.0.0, < 49.0.2 | 48.0.4; 49.0.2 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-m9f7-9w4p-gv38` (no CVE) | RUSTSEC-2026-0352 | 2026-10-14 | high | Filesystem sandbox escape when renaming symlinks with trailing slashes | <= 36.0.17, >= 37.0.0, <= 48.0.5, >= 49.0.0, <= 49.0.2 | 36.0.18, 48.0.6, 49.0.3 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-5qpp-g3j2-8frx` (no CVE) | RUSTSEC-2026-0350 | 2026-10-14 | low | Guests can send absolute-form HTTP requests to different hosts | <= 36.0.17, >= 37.0.0, <= 48.0.5, >= 49.0.0, <= 49.0.2 | 36.0.18, 48.0.6, 49.0.3 | not affected: `wasmtime-wasi-http` is not in the closure (no WASI) |
| `GHSA-w36r-2hch-9xp9` (no CVE) | RUSTSEC-2026-0351 | 2026-10-14 | medium | wasi-http: Request with long url before query parameter crashes host | >= 46.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: `wasmtime-wasi-http` is not in the closure (no WASI) |
| `GHSA-54p9-vhhq-8f7q` (no CVE) | RUSTSEC-2026-0360 | 2026-10-14 | medium | Overlapping string transcode in a fused adapter can panic | >= 49.0.0, <= 49.0.2 | 49.0.3 | not affected: the range starts at 49.0.0 |
| `GHSA-p79w-pv2r-29qp` (no CVE) | RUSTSEC-2026-0364 | 2026-10-14 | low | Exception tags can cause host data structures to be unaligned | >= 47.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-q554-3cjc-rx7p` (no CVE) | RUSTSEC-2026-0363 | 2026-10-14 | medium | Component-model handle tables are capped only per instance and charged to no lim... | <36.0.18, >= 37.0.0, <48.0.6, >=49.0.0, <49.0.3 | 49.0.3, 48.0.6, 36.0.18 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-63h4-6cx9-pjfj` (no CVE) | RUSTSEC-2026-0355 | 2026-10-14 | medium | Panic viewing component model stream or future as the wrong type | >= 46.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-vcg5-q8m5-8379` (no CVE) | RUSTSEC-2026-0362 | 2026-10-14 | medium | Wasmtime may not properly enforce async-entrypoint requirement  | >= 42.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-93g3-36p8-x2gc` (no CVE) | RUSTSEC-2026-0357 | 2026-10-14 | medium | Out-of-bounds zero-length stream writes can panic  | >= 46.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-3r89-v5mq-j5xf` (no CVE) | RUSTSEC-2026-0354 | 2026-10-14 | medium | Host panic when component call_async is cancelled under some conditions | >= 46.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-7v24-42x9-pxf7` (no CVE) | RUSTSEC-2026-0356 | 2026-10-14 | medium | Panic on erroneous call-to-host from guest realloc call | >= 46.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-5hhx-4wrr-3j9g` (no CVE) | RUSTSEC-2026-0358 | 2026-10-14 | medium | Yielding in a component start function can lead to a host panic | >= 46.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-m6w2-jwq2-cv2c` (no CVE) | RUSTSEC-2026-0353 | 2026-10-14 | medium | Panic when invoking an async-lifted function via call | >= 46.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-hmwq-g22x-qjw5` (no CVE) | RUSTSEC-2026-0361 | 2026-10-14 | medium | Panic when pending calls are run after a trap | >= 46.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-wjc8-633q-pwxr` (no CVE) | RUSTSEC-2026-0359 | 2026-10-14 | medium | Stack overflow dropping a long chain of recursive types | >= 42.0.0, < 48.0.6, >= 49.0.0, < 49.0.3 | 48.0.6, 49.0.3 | not affected: fixed in 48.0.6, which the pin carries |
| `GHSA-r8xh-cgf7-392j` (no CVE) | RUSTSEC-2026-0376 | 2026-10-19 | low | wasi-http: Guests can add conflicting content lengths to messages | < 36.0.19, >= 37.0.0, < 48.0.7, >= 49.0.0, < 49.0.4 | 36.0.19, 48.0.7, 49.0.4 | not affected: `wasmtime-wasi-http` is not in the closure (no WASI) |
| `GHSA-jxcf-8qw5-5c33` (no CVE) | RUSTSEC-2026-0377 | 2026-10-19 | low | wasi-http p3: forwarded trailers aren't checked against is_forbidden_header hook | >= 46.0.0, < 48.0.7, >= 49.0.0, < 49.0.4 | 48.0.7, 49.0.4 | not affected: `wasmtime-wasi-http` is not in the closure (no WASI) |
| `GHSA-9prg-84vf-g5fc` (no CVE) | RUSTSEC-2026-0375 | 2026-10-19 | medium | wasi-http p3: Guests can set incorrect content-length for spliced response bodie... | >= 46.0.0, < 48.0.7, >= 49.0.0, < 49.0.4 | 48.0.7, 49.0.4 | not affected: `wasmtime-wasi-http` is not in the closure (no WASI) |
| `GHSA-mghf-wjfp-p5xm` (CVE-2026-115893) | RUSTSEC-2026-0379 | 2026-10-19 | critical | Cranelift backend on aarch64 can jump to incorrect target on large Wasm br_table... | <36.0.19, >=37.0.0, <48.0.7, >=49.0.0, <49.0.4 | 36.0.19, 48.0.7, 49.0.4 | not affected: fixed in 48.0.7, which the pin carries |
| `GHSA-897f-g5gm-fmxm` (CVE-2026-115889) | RUSTSEC-2026-0380 | 2026-10-19 | critical | Bit-packing in Cranelift compiler causes silent overflow and miscompilation | <36.0.19, >= 37.0.0, <43.0.0 | 36.0.19 | not affected: the range ends before the 48 line |
| `GHSA-9pvw-hwfg-92h5` (CVE-2026-115892) | RUSTSEC-2026-0378 | 2026-10-19 | medium | Panic lowering a host-created component model stream into a guest that expects a... | >= 46.0.0, < 48.0.7; >= 49.0.0, < 49.0.4 | 48.0.7; 49.0.4 | not affected: fixed in 48.0.7, which the pin carries |
| `GHSA-hmmr-87qq-258j` (no CVE) | RUSTSEC-2026-0420 | 2026-10-29 | medium | wasip3 UDP send can deliver a datagram to an address only allowed for receiving | >= 48.0.0, < 48.0.8; >= 49.0.0, < 49.0.5; 50.0.0 | 48.0.8; 49.0.5; 50.0.1 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-qw58-7h65-crq2` (no CVE) | RUSTSEC-2026-0423 | 2026-10-29 | low | Cross-engine type confusion with host-created struct-types-with-supertypes | >= 47.0.0, < 48.0.8; >= 49.0.0, < 49.0.5; 50.0.0 | 48.0.8; 49.0.5; 50.0.1 | not affected: fixed in 48.0.8, which the pin carries |
| `GHSA-3982-jqvv-277m` (no CVE) | RUSTSEC-2026-0422 | 2026-10-29 | low | Typed functions with i31 or nullref arguments may panic due to insufficient type... | >= 47.0.0, < 48.0.8; >= 49.0.0, < 49.0.5; 50.0.0 | 48.0.8; 49.0.5; 50.0.1 | not affected: fixed in 48.0.8, which the pin carries |
| `GHSA-6fr9-xmw9-5352` (no CVE) | RUSTSEC-2026-0424 | 2026-10-29 | medium | Unchecked memory usage in transferring borrowed resources between components | < 36.0.20; >= 37.0.0, < 48.0.8; >= 49.0.0, < 49.0.5; 50.0.0 | 36.0.20; 48.0.8; 49.0.5; 50.0.1 | not affected: fixed in 48.0.8, which the pin carries |
| `GHSA-h4wp-g4x3-7398` (no CVE) | RUSTSEC-2026-0421 | 2026-10-29 | low | wasi-http p2: outgoing content-length can be invalid or not match body | < 36.0.20; >= 37.0.0, < 48.0.8; >= 49.0.0, < 49.0.5; 50.0.0 | 36.0.20; 48.0.8; 49.0.5; 50.0.1 | not affected: `wasmtime-wasi-http` is not in the closure (no WASI) |
| `GHSA-xrvr-8678-5wp8` (no CVE) | not yet in RustSec | 2026-11-16 | low | Panic when a component cancels a stream or future copy on the wrong end | >= 46.0.0, < 48.0.9; >= 49.0.0, < 49.0.6; >= 50.0.0, < 50.0.2 | 48.0.9; 49.0.6; 50.0.2 | range includes 48.0.8, fixed in 48.0.9; **not affected**: it needs the component-model async feature (`component-model-async`, off in the cargo features and in `FEATURES`) and streams passed between components |
| `GHSA-5m8g-xhq4-4h93` (no CVE) | not yet in RustSec | 2026-11-16 | medium | wasmtime-wasi: Unbounded host memory growth via empty writes to AsyncWriteStream | <36.0.21; >=37.0.0, <48.0.9; >=49.0.0, <49.0.6; >=50.0.0, <50.0.2 | 36.0.21; 48.0.9; 49.0.6; 50.0.2 | not affected: `wasmtime-wasi` is not in the closure (no WASI) |
| `GHSA-43j2-gcp4-m22g` (no CVE) | not yet in RustSec | 2026-11-16 | medium | Stale GC heap base pointer in compiled code lets a Wasm guest abort the host pro... | >=50.0.0, <=50.0.1; >=49.0.0, <=49.0.5; >=47.0.0, <=48.0.8 | 50.0.2; 49.0.6; 48.0.9 | range includes 48.0.8, fixed in 48.0.9; **not exploitable here**: needs the GC proposal, which the engine turns off (`WasmFeatures::GC` is not in `FEATURES`) and the `gc` cargo feature is off; the advisory names that as the case that is not affected |
| `GHSA-wvgq-g5hf-6gm6` (no CVE) | not yet in RustSec | 2026-11-16 | low | Process abort when a GC heap's reservation isn't a multiple of the page size | >=50.0.0, <=50.0.1; >=49.0.0, <=49.0.5; >=47.0.0, <=48.0.8 | 50.0.2; 49.0.6; 48.0.9 | range includes 48.0.8, fixed in 48.0.9; **not exploitable here**: needs the GC proposal on and a GC heap that cannot move with a reservation that is not a page multiple; GC is off, and the engine's memory reservation (16 MiB) is a page multiple |
| `GHSA-hj27-mpw9-ff4c` (no CVE) | not yet in RustSec | 2026-11-16 | low | Panic in Caller::get_module_export when a host function isn't called from WebAss... | <36.0.21; >=37.0.0, <48.0.9; >=49.0.0, <49.0.6; >=50.0.0, <50.0.2 | 36.0.21; 48.0.9; 49.0.6; 50.0.2 | range includes 48.0.8, fixed in 48.0.9; **not affected**: it needs a host function that calls `Caller::get_module_export`, and the engine has none (no host function does; the parts are components, which the advisory says are not affected) |
| `GHSA-rc4p-x33f-vpr2` (no CVE) | not yet in RustSec | 2026-11-16 | low | Abort in Wasmtime builds without the gc cargo feature | >=50.0.0, <=50.0.1; >=49.0.0, <=49.0.5; >=47.0.0, <=48.0.8 | 50.0.2; 49.0.6; 48.0.9 | range includes 48.0.8, fixed in 48.0.9; the build is the affected kind (default features off, no `gc`), so the exploit depends on the GC proposal being on, whose function subtyping reaches the stubbed builtin. The engine turns it off (all features off, then exactly `FEATURES`), the advisory's own workaround (`wasm_gc(false)`); `engine_leaves_off_the_proposals_the_runtime_advisories_of_2026_11_16_need` shows it. **Not exploitable here** |
| `GHSA-f8mm-ggr3-89wh` (no CVE) | not yet in RustSec | 2026-11-16 | medium | wasmtime-wasi-http: Empty Body Writes can result in Unbound Heap Usage with HTTP... | <36.0.21; >=37.0.0, <48.0.9; >=49.0.0, <49.0.6; >=50.0.0, <50.0.2 | 36.0.21; 48.0.9; 49.0.6; 50.0.2 | not affected: `wasmtime-wasi-http` is not in the closure (no WASI) |
| `GHSA-2674-w29h-wqvj` (no CVE) | not yet in RustSec | 2026-11-16 | low | Abort in Wasmtime builds without the threads cargo feature | >= 39.0.0, < 48.0.9; >=49.0.0, <49.0.6; >=50.0.0, <50.0.2 | 48.0.9; 49.0.6; 50.0.2 | range includes 48.0.8, fixed in 48.0.9; the build is the affected kind (no `threads` cargo feature), and it needs the threads proposal turned on through `Config::wasm_features`. `FEATURES` doesn't hold `THREADS`. **Not exploitable here** |

Advisories published before 2026 (24, from GHSA-hpqh-2wqx-7qp5, 2021-05-21, to GHSA-hc7m-r6v8-hg9q, 2025-11-11): **none
affects 48.0.8.** Every `wasmtime` range ends at 38.0.3 or earlier (the newest, `GHSA-hc7m-r6v8-hg9q`, is `<= 38.0.3`,
patched in 38.0.4, 37.0.3, 36.0.3 and 24.0.5, an unsound shared-memory API the engine does not use). The rest name
`cranelift-codegen` (`<= 0.73.0`, `< 0.85.0`, `<= 0.85.1`, `<= 0.93.0`), `wasmtime-wasi` (`<= 34.0.1`, absent) or
`wasmtime-c-api` (37.x, absent). Ids: GHSA-hpqh-2wqx-7qp5, GHSA-q879-9g95-56mx, GHSA-4873-36h9-wv49,
GHSA-v4cp-h94r-m7xf, GHSA-88xq-w8cq-xfg7, GHSA-gwc9-348x-qwv2, GHSA-jqwc-c49r-4w2x, GHSA-5fhj-g3p3-pq9g,
GHSA-7f6x-jwh5-m9r4, GHSA-h84q-m8rr-3v9q, GHSA-wh6w-3828-g9qf, GHSA-44mr-8vmm-wjhg, GHSA-xm67-587q-r2vw,
GHSA-ff4p-7xrq-q5r8, GHSA-ch89-5g45-qwc7, GHSA-gw5p-q8mj-p7gh, GHSA-75hq-h6g9-h4q5, GHSA-7qmx-3fpx-r45m,
GHSA-q8hx-mm92-4wvg, GHSA-c2f5-jxjv-2hh8, GHSA-fm79-3f68-h2fc, GHSA-vvp9-h8p2-xwfc, GHSA-4h67-722j-5pmc,
GHSA-hc7m-r6v8-hg9q.

Counting the 2026 table: 6 include 48.0.8 (none reachable), 1 is the open-range AVX one (fixed, not emitted), 21 are
fixed in 48.0.3 to 48.0.8 (carried by the pin), 20 name `wasmtime-wasi` or `wasmtime-wasi-http` (not in the closure), and
the rest end before the 48 line or start at 49.0.0.

### Rule-off table

Each rule switched off once in `components/prov/src/module/engine.rs`, the test run, the file restored (the run script
is the scratch file `ruleoff.sh`; the whole of it ran in one lock hold; the test was written before `config_for` existed
and first failed to compile; the red runs below are the assertion failures).

| Rule switched off | Test | Result |
|---|---|---|
| `has_avx` enabled for the x86-64 targets (`config.cranelift_flag_enable("has_avx")`, test-side edit of `config_for`) | `engines_compile_target_enables_no_avx_feature` | red: `x86_64-unknown-linux-gnu: AVX flags on: ["has_avx"]` |
| The explicit target not set (`config.target(..)` removed from `config_for`), so Cranelift reads the compiling CPU | `engines_compile_target_enables_no_avx_feature` | red: `aarch64-apple-darwin: CPU flags on beyond the triple's baseline: ["has_lse", "has_pauth", "has_fp16", "has_dotprod", "has_i8mm", "sign_return_address", "sign_return_address_with_bkey"]` (an Apple M-series host; on an x86-64 CPU with AVX the same step turns on `has_avx`, `has_avx2` and the rest, which the `avx` assertion names) |
| Unedited | `engines_compile_target_enables_no_avx_feature` | green, also for the two x86-64 targets made on this arm64 host |

The second test, `engine_leaves_off_the_proposals_the_runtime_advisories_of_2026_11_16_need`:

| Rule switched off | Result |
|---|---|
| `WasmFeatures::GC` added to `FEATURES` | red: the assertion that the engine's features hold no `gc` fails |
| Unedited | green: a core module and a component whose core module declare a non-final function type are both refused with wasmparser's "gc proposal must be enabled to use subtypes" (checked by message, so another refusal fails the test), and the same two with a final type are compiled |

The header parse also asserts that the CPU flags and the shared flags it read are not empty, so a change of Wasmtime's
metadata format can't let the flag assertions pass on nothing.

### Defaults taken

- **A bump is a new row, not done here (`T-025-06`).** The six advisories of 2026-11-16 that include 48.0.8 are none of them
  reachable, so the bump is a hygiene row (a scanner on `Cargo.lock` lists the pin as vulnerable), put in wave 19 with the
  re-read of the advisories at its commit. A reversible default: if the owner prefers to stay on 48.0.8 until a reachable
  advisory appears, the row is dropped and the six verdicts here stand; `T-025-04` lists it as a dependency.
- **Verdicts by reachability go beyond the range.** A range that includes the pin is read against the features and
  settings the engine has; where the advisory itself names the case that is not affected (`GHSA-43j2-gcp4-m22g`: "if you
  disable the Wasm GC proposal, you are not affected"; `GHSA-rc4p-x33f-vpr2`'s workaround, `wasm_gc(false)`;
  `GHSA-2674-w29h-wqvj`'s, not enabling `THREADS`), that is the ground, and a test holds it.
- **`config()` is unchanged in behaviour.** `config_for(target)` is its body with the target a parameter, so the engine
  every process shares is the same engine, with the same compatibility hash.

### Gaps

- **RustSec does not yet carry the nine advisories of 2026-11-16** (its newest entry is 2026-10-29), so `cargo audit` is
  silent on the six that include the pin. Owner: none; it closes itself when RustSec imports them. Until then the read is
  GitHub's.
- **The x86-64 flags are read from code made on the build's host (here arm64) with Cranelift's x86 backend added to test
  builds**, not from a run of x86-64 machine code on an x86-64 CPU. A CI run on x86-64 Linux, where the build's own
  target is x86-64 too, is the other half; it was not seen from this arm64 machine. Owner: `T-025-02`'s Linux job.
- **`F-025`'s other rows** (`T-025-02` to `T-025-05`) are not read here; `T-025-04` restates `THR-85` with this reading.

## T-025-06

The Wasmtime pin bumped from 48.0.8 to 48.0.9 (`components/prov/Cargo.toml`, `Cargo.lock`, `engine::WASMTIME_VERSION`,
`licences/RECORD`), and the advisories read again at this row's commit, on 2026-11-17.

- **The bump.** `wasmtime =48.0.9` (the exact requirement), through the lock: the fourteen crates of Wasmtime's own
  release set (`wasmtime`, `wasmtime-environ`, `wasmtime-internal-*`, `pulley-*`) move to 48.0.9 and the thirteen
  `cranelift-*` crates to 0.135.9, the Cranelift release that goes with it; the test-only dev-dependency
  `cranelift-codegen =0.135.9` moves with them, as `T-025-01` has it follow the runtime's own. No other package changes
  version. `cargo update` also re-pointed four Windows-only edges (`errno`, `rustix`, `tempfile`, `winapi-util`) from
  `windows-sys 0.61.2` to `0.52.0`, both of which are in the lock; those four were put back by hand, so the lock's diff
  is the version and checksum lines only (`cargo metadata --locked` accepts it).
- **What 48.0.9's release changes.** The release is three commits from 48.0.8: the eight fixes of its notes, and the version
  numbers. No default and no compiler setting changes. The only code-generation change is in `bounds_checks.rs`, on paths
  that only GC object accesses take (`BoundsCheck::StaticObjectField` and the object-end case), which this engine never
  makes (no GC). `BINDING_VERSION` stays `0.3.0`: the platform version already names the runtime's version, which is what
  changes the machine code's header, and nothing the binding sets changed. If CI's `engine_from` guard (compatibility
  hash) trips on the bump, that is the reason to bump the binding to `0.3.1`.
- **The digests.** The sixteen stand-ins' machine-code digests are not in the repository: `ci` derives them in
  `a_derivation_records_its_cpu_and_one_digest_for_each_part_and_tells_a_changed_part` (the file records the CPU, the
  platform version and one digest for each part) and the `rederive` job compares them with a re-derivation on a CPU of
  another vendor (`B-11`, `G-04`). Nothing is stored to change. They are re-derived by CI at this commit, and are
  expected to differ from 48.0.8's, since Wasmtime writes its own version into the machine code. **Not re-derived
  here**: this machine is arm64 macOS, which has no `/proc/cpuinfo` and cannot be a cross-vendor run. The run ids of
  `ci` and of a `rederive` run that landed on another vendor than the reference's are to be added here by the
  orchestrator when they exist; a skipped `rederive` run doesn't count (see `T-013-05`'s review round).
  **Recorded:** the PR's CI run 44134656776 at `0f523b38`: `ci` job 134220782586 (success, the binding's
  compatibility hash unchanged, `BINDING_VERSION` stays `0.3.0`) and `rederive` job 134247456427 (success, re-derived on
  GenuineIntel against a reference made on AuthenticAMD; reference artifact 14353433711).
- **The advisories read again, 2026-11-17.** GitHub's advisory API for `bytecodealliance/wasmtime` (state `published`):
  still 94, the newest published on 2026-11-16; none is dated after 48.0.9's release (crates.io, 2026-11-16) or on
  2026-11-17. The six that included 48.0.8 (`GHSA-43j2-gcp4-m22g`, `GHSA-wvgq-g5hf-6gm6`, `GHSA-rc4p-x33f-vpr2`,
  `GHSA-2674-w29h-wqvj`, `GHSA-hj27-mpw9-ff4c`, `GHSA-xrvr-8678-5wp8`) are in 48.0.9's release notes as fixed; the
  release's other two (`GHSA-5m8g-xhq4-4h93` and `GHSA-f8mm-ggr3-89wh`, in `wasmtime-wasi` and `wasmtime-wasi-http`) are
  of crates not in the closure. So **no advisory names 48.0.9 in its range.** RustSec's `advisory-db` (commit of
  2026-11-17 10:14 UTC): its newest `crates/wasmtime` entries are of 2026-10-30; the advisories of 2026-11-16 are
  still not in it, so `cargo audit` stays silent on 48.0.8 and on 48.0.9 alike, and GitHub's side is the one that
  shows them. The web pages were not fetched: the API is the same source as `security/advisories`.
- **Two of the six name a missing cargo feature** (`GHSA-rc4p-x33f-vpr2`: abort in builds without `gc`;
  `GHSA-2674-w29h-wqvj`: abort in builds without `threads`), and this engine has neither feature. `T-025-01`'s verdict
  stands: the abort needs a guest that uses GC or threads, and `engine_leaves_off_the_proposals_...` holds that the
  engine refuses both. The fix makes the question moot.
- **The test, `pinned_runtime_is_the_patched_release`** (`components/prov/tests/module_engine_target.rs`, `THR-85`,
  `DEC-27`): `engine::WASMTIME_VERSION`, the exact requirement in `components/prov/Cargo.toml` and the locked versions of
  `wasmtime`, `wasmtime-environ`, `wasmtime-internal-cranelift`, `wasmtime-internal-core` and `pulley-interpreter` are
  48.0.9. It is red with the constant at 48.0.8 (see the rule-off table). `engines_compile_target_enables_no_avx_feature`
  and the other module suites are unchanged.
- **`licences/RECORD`.** The `dependency licence record` job reads every `Cargo.lock` of the repository, and one record
  serves them all, with one line per name and version. The experiment's own lock
  (`experiments/2026-11-07-module-format/wasm-binding`) still locks the 48.0.8 and 0.135.8 sets, as the record of what
  `EXP-2` scored, so the 27 old lines stay and the 27 lines of the new versions are added beside them (same licences,
  same decisions, `licence-llvm-exception`). The root lock holds only the new ones now; `components/prov/module-guests`
  holds none of these crates.
- **Rule-off.** `pinned_runtime_is_the_patched_release` with `WASMTIME_VERSION` left at `48.0.8`: red on the first
  assertion. Restoring it: green.

## T-025-02

Each jail layer probed alone, on Linux, with the release build scanned for a way to turn one off. **Written on macOS:
the Linux evidence is `jail-probes.yml`'s runs, recorded at the end of this section** (`ci.yml`'s
`cargo test --workspace` skips the probes, which need `--features jail-layers`; this job runs them). Row 18; touches `components/prov/src/module/` and
`.github/workflows/`, both `DEC-20` paths, so it waits for the owner's click at the release into `main`.

- **The builder.** `components/prov/src/module/layers.rs` names the layers (`Layer`, fifteen: the six namespaces,
  `no_new_privs`, the empty root, dropped capabilities, Landlock, seccomp, descriptors closed before `exec` and in the
  jail, `RLIMIT_AS`, and the other rlimits) and `Layers`. `sandbox::isolated` and `confine` now call
  `isolated_layers` / `confine_layers` with `Layers::ALL`, and each step asks `layers.on(Layer::X)`. In a shipped
  build `Layers` is a zero-sized type whose `on` is `true`: no name, parser or setter is compiled in. With the
  `jail-layers` feature the set is real, and `module::jail` (`isolated`, `confine`, `unshare_arguments`) composes
  layers. The feature is not a default; nothing turns it on but the command line of the job that runs the probes
  (`--features jail-layers`; see the second review below), and `rutent-jail-probe` (the hostile probe, a binary with
  `required-features`) exists only with it. One source of truth: the shipped jail and the builder's all-layers jail are
  the same code, and a test shows `unshare_arguments(ALL) == UNSHARE`.
- **The probes** (`components/prov/tests/jail_layers.rs`), each run twice: in a control jail without the layer, where the
  hostile act must succeed (so no probe passes for want of the power to do harm), then with the layer alone, where it must
  be refused. A jail that can't be established makes the probe exit 3 with no result and the test fails (fail closed).
  User: the host's root owner shows as 65534 and `setresuid(1)` fails `EINVAL`. Network: a connect to a listener the test
  holds, which sees no connection. IPC: `shmat` of the test's segment. UTS: the jail renames itself, the host's name
  is unchanged. PID: `SIGTERM` to a host `sleep`, which survives (`ESRCH`). Mount: user, mount and the root pivot, which sees
  no entry in `/`. `no_new_privs`: `sudo -n id -u` is not root. Capabilities: `chroot(".")` is `EPERM`. Landlock:
  reading `/etc/passwd` is `EACCES`. Seccomp: raw `socket` is `EPERM`. Descriptors: an inherited `/dev/null` is gone
  after either close alone, and from the shipped provider's own `--probe`. `RLIMIT_AS`: a mapping of 4 GiB + 4 KiB is
  `ENOMEM` while a page maps, and the shipped provider's `--probe` reports the 4 GiB and the refusal (**`G-05`, run id
  in every line: `jail-probe run <GITHUB_RUN_ID>`**). The release scan reads `rutent-wasm-provider` built `--release`
  (the Linux job builds it and passes `RUTENT_RELEASE_PROVIDER`; unset, the test builds it itself): it carries none of the
  fifteen `jail-layer:` names, which the probe binary, the control, carries all of.
- **Layers that can't stand alone, named with their reasons** (`Layers::check`, refused by the builder, tested):
  the network, IPC, UTS, mount and PID namespaces each need the user namespace; the root pivot needs the mount
  namespace (and so the user one); dropped capabilities need the user namespace to hold any; seccomp and Landlock
  need `no_new_privs` or the user namespace, so their probes run over `no_new_privs`, which stops nothing they try.
- **Defaults (reversible, fail closed).** A kernel without Landlock fails the Landlock probe (the jail then protects
  nothing there; the shipped jail still starts, by design). The `no_new_privs` control needs passwordless `sudo`
  (GitHub's Ubuntu runner has it); without it the test fails, saying the hazard isn't shown. The seccomp list is
  x86-64, so the probes fail on another architecture, as the jail itself refuses to start there. Probes run with
  `--test-threads=1` in the Linux job. The rlimits are one layer, `Limits`, not in `THR-85`'s list, so not probed.
- **Gaps and owner items.** (1) The UTS namespace stops nothing alone against an unprivileged jail: the user namespace
  already refuses `sethostname` (`EPERM`), and the probe shows only that a rename made with the power stays inside.
  The row's rule makes this **an item for the owner's form**: keep the namespace as depth, or drop it. Owner: Pablo.
  (2) The release scan is a string scan of a binary: it shows the names are not compiled in, not that no other
  switch can exist; `T-025-05`'s review reads the diff. Owner: `T-025-05`. (3) `ci.yml`'s `cargo test --workspace`
  builds a release provider inside the scan test when `RUTENT_RELEASE_PROVIDER` is unset, which costs minutes there;
  setting it is a `ci.yml` edit, `F-024`'s file. Owner: `F-024`. (4) The Linux code was not even type-checked here (no
  Linux target installed, none installed on purpose); the first CI compile may find errors in `jail_probe.rs` and the
  `rig` module. Owner: this row's author at the first red run.
- **Rule-off table.** The jail rules were not switched off here: they can't run on this machine. Each probe's control
  does that in CI, run by run: the same hostile act in a jail without the layer must succeed, and the test fails if it
  doesn't. To switch a layer off for good, remove its `layers.on(..)` step in `sandbox.rs` and its probe goes red.

| Rule switched off | Test that must go red | Seen |
|---|---|---|
| user namespace | `probe_for_the_hosts_users_is_refused_by_the_user_namespace_alone` | CI only |
| network namespace | `network_probe_is_refused_by_the_network_namespace_alone` | CI only |
| IPC namespace | `shared_memory_probe_is_refused_by_the_ipc_namespace_alone` | CI only |
| UTS namespace | `hostname_change_is_refused_by_the_uts_namespace_alone` | CI only |
| PID namespace | `signal_to_a_host_process_is_refused_by_the_pid_namespace_alone` | CI only |
| mount namespace, root pivot | `probe_in_the_mount_namespace_sees_only_the_empty_root` | CI only |
| `no_new_privs` | `privilege_gain_is_refused_by_no_new_privs_alone` | CI only |
| dropped capabilities | `capability_use_is_refused_by_the_dropped_capabilities_alone` | CI only |
| Landlock | `filesystem_probe_is_refused_by_landlock_alone` | CI only |
| seccomp | `forbidden_system_call_is_refused_by_seccomp_alone` | CI only |
| either descriptor close | `inherited_descriptor_is_closed_inside_the_jail` | CI only |
| `RLIMIT_AS` | `mapping_past_the_address_space_limit_is_refused` | CI only |
| `jail-layers` in a release build | `release_build_of_the_provider_binary_has_no_way_to_turn_a_jail_layer_off` | CI only |

### T-025-02, after review of `16dc2296`

- **Compile fixes for Linux:** the duplicate `use std::time::Duration` in the tests, and an `unused_mut` in
  `sandbox::isolated_layers` (the Linux branch). Every `cfg(target_os = "linux")` line the row added was read again by hand.
- **Controls now differ from the jail by the one layer.** Network, IPC and PID compare `User` with `User + X`; mount
  compares `User + Mount` (root visible) with `User + Mount + EmptyRoot` (empty). Capabilities, Landlock, seccomp and UTS
  already did.
- **The release scan has a second prong that doesn't depend on the optimiser.** A provider built with the feature never
  calls the builder, so its names may fold away and can't serve as the string scan's control. The test therefore also reads
  Cargo's resolved features, in the inverted tree `cargo tree -e features -i rutent-prov` (a forward tree never lists the
  root package's own features): for the release command line it must show `feature "default"` and no `jail-layers`, and with
  `--features jail-layers` it must show `rutent-prov feature "jail-layers"` (the control, which shows the check can fail);
  and `cargo metadata` over the workspace must show no feature, dependent crate or self-dependency enabling it.
- **The composition test runs on every platform** (`unshare_arguments(ALL) == UNSHARE` and the refusals of a layer that
  can't stand alone), no longer ignored off Linux.
- **The UTS test keeps `PLAN.md`'s name**, `hostname_change_is_refused_by_the_uts_namespace_alone`, since the row's test
  names are written there; what it shows is said in its comment and messages: a rename made in the namespace stays inside
  it, and the user namespace already refuses a rename without it. The owner item stands (gap 1 above).
- **`jail-probes` is not a required check** of the repository (a branch-protection setting, which isn't ours to change),
  so a red run doesn't block a merge by itself. Owner: Pablo, to add it once the first run is green.

### T-025-02, after the first Linux run (`a92e45a9`, run 44237249072)

- **13 of 14 probes passed on Linux**; the run's lines carry the run id, which the row's Done-when cites.
- **The descriptor control did inherit** (the log read `fds=[3, 142, 145]`): the test made its descriptor inheritable, but
  asserted the child's list was exactly `[fd]`, and the runner had left two others open (3, 142 and 145 are all inherited:
  `close_range` removes them in the jail, which `inherited_descriptor_is_closed_inside_the_jail` shows with `fds=[]`). The
  control now asserts that its list *contains* the test's descriptor.
- **No self dev-dependency** (`tools/boundary-check` refuses a package that depends on itself, and its paths are
  `DEC-20`'s, `F-020`'s). The `jail_layers` test and `rutent-jail-probe` carry `required-features = ["jail-layers"]`, and
  `jail-probes.yml` runs `cargo test -p rutent-prov --features jail-layers --test jail_layers`. **`cargo test --workspace`
  in `ci.yml` therefore skips the probes: the `jail-probes` job is the one that runs them.** Local clippy for them is
  `cargo clippy -p rutent-prov --all-targets --features jail-layers`, which `ci.yml`'s workspace clippy doesn't cover.
- **The resolved-features check follows:** `cargo tree` for the release command line shows no `jail-layers`, and the same
  with `--features jail-layers` shows it (the control); `cargo metadata` over the workspace shows no feature, no dependent
  crate and no self-dependency enabling it.
- **A gap for `F-024`:** `ci.yml`'s evidence table (`T-024-09`) reads the results of `cargo test --workspace`, which no
  longer holds these 13 tests, so `THR-85`, `THR-79`, `GUA-5` and `REQ-MDL-11` rows they verify would read as untested
  there unless the table also reads the `jail-probes` job's results. Owner: `F-024`.
- **The resolved-features check's control was wrong** in `19fb8314` (a forward tree can't show the root's features); it
  reads the inverted tree now, whose two outputs were reproduced on cargo 1.98.0. `jail-probes.yml` also lints the probes:
  `cargo clippy --locked -p rutent-prov --all-targets --features jail-layers -- -D warnings`.

### T-025-02: Linux evidence

Run 44242098797 (commit `19fb8314`, `jail-probes`), per probe, control then with the layer; every one as the test asserts:

| Layer | Control | With the layer |
|---|---|---|
| user namespace | `[]`: `owner=0 switch=refused EPERM` | `[user]`: `owner=65534 switch=refused EINVAL` |
| network namespace | `[user]`: allowed | `[user,net]`: refused `ENETUNREACH` |
| IPC namespace | `[user]`: allowed | `[user,ipc]`: refused `EINVAL` |
| UTS namespace | `[user]`: refused `EPERM` | `[user,uts]`: allowed, `hostname=rutent-jail-probe`, the host's unchanged (depth only) |
| PID namespace | `[user]`: allowed | `[user,pid]`: refused `ESRCH` |
| mount namespace and root pivot | `[user,mount]`: `entries=26 passwd=readable` | `[user,mount,empty-root]`: `entries=0 passwd=absent` |
| `no_new_privs` | `[]`: allowed `euid=0` | `[no-new-privs]`: refused |
| capabilities | `[user]`: allowed | `[user,caps]`: refused `EPERM` |
| Landlock (ABI 8) | `[no-new-privs]`: allowed | `[no-new-privs,landlock]`: refused `EACCES` |
| seccomp | `[no-new-privs]`: allowed | `[no-new-privs,seccomp]`: refused `EPERM` |
| descriptors | `[]`: `fds=[3, 142, 145]` | `[close-before-exec]` and `[close-in-jail]`: `fds=[]` |
| `RLIMIT_AS` | `[]`: `limit=0 past=allowed` | `[address-space]`: `limit=4294967296 small=allowed past=refused ENOMEM` |
| `G-05`, the shipped provider's `--probe` | | `address_space_limit` 4294967296, `address_space_denied` true, `landlock_abi` 8, `open_descriptors` [] |

The release scan found no layer name in the release provider. Green run's id (the whole job, with the resolved-features
check fixed): run `44243240737` (`jail-probes`, at `943fad16`: 14 passed, 0 failed), with `ci` run `44243240720` green at the same commit.

## T-025-03

`EXP-1`'s `G-26` (no provider credential and no module state readable by a VOS space's operators) re-checked by a scan,
`THR-79`, `REQ-AUD-6`. Row 24; adds a step to `.github/workflows/stm-vos.yml` and a branch to its triggers, a `DEC-20`
path, so it waits for the owner's click at the release into `main`. **Written on macOS: the VOS run is `stm-vos.yml`'s
(it needs VOS's toolchain and three nodes), and its run id is recorded at the end of this section once it is green.**
The scan itself, its controls and its refusals run on the reference host here and in `ci.yml`'s `cargo test --workspace`.

- **What it scans.** `rutent_scenarios::secrets` (`conformance/scenarios/src/secrets.rs`) plants a run, plays the base
  order twice through to its end (`full_run`) and reads, while Core is still up: every order's state and journal on the
  host, the host's whole snapshot (halted, exported, resumed), Core's log records on its disk and the evidence store's
  shelf on it (every kept statement); then the log as read
  back and the inputs fed to the host. On VOS (`components/stm/vos-program/scenarios/tests/secrets.rs`) it also reads
  every regular file under the clusters' directories, which is the nodes' state, in 1 MiB chunks that overlap.
- **What it seeks, per kind.** Credential: a value the world holds as each provider's secret `api-key`
  (`SimWorld::hold_secret`; the platform's lookup would give it only to that provider's outside part). Secret
  plaintext: each provider's real private statement key, in the two forms the world holds it in (the 32 bytes and their
  base64url text), and the operator's real signing key. Module state: a private state entry (`private/notes`) the world
  holds for each provider. Each value is sought as it is, JSON-escaped, in lower and upper hex, in base64 and
  base64url at each of the three alignments it can take inside a longer string, as the JSON array of bytes serde_json
  writes a byte string as, and percent-encoded; a value under 8 bytes is refused.
- **Where module state sits (`CMP-PROV`).** In the platform's `World` (`World::state`), in the process that runs
  `CMP-PROV`, which is Core's own: a provider's entries are asked of it only while an outside request is in flight, and
  its books are the journal the world keeps for its rail. `Scanned::module_state` reads it off the world after each
  run (the test prints it), and it is none of the state machine's host (the orders' rows), Core's log or the VOS
  nodes' files: the scan finds no value of it in any of them. In production the `World` is the deployment's store behind
  the platform (`F-030`'s assembly); this row shows that no path of the assembled Core copies it into the state machine.
- **The controls.** A scan that finds nothing is not shown to see unless it finds a leak, so for each kind the run is
  repeated with the boundary open once (`run_leaking`): before the surfaces are read, a copy of an input that opens an
  order is submitted to the live host under an order id, a record id and a decision record that carry the value, which is what a Core
  that wrote a credential into an input would do. The scan must find that value, and only that one, in the host's
  journal and snapshot (reference host, `scan_finds_each_kind_of_value_let_through_the_state_machines_boundary`) and, on
  three VOS replicas, **also in the nodes' own files**
  (`scan_finds_each_kind_of_value_let_into_three_vos_replicas_journal_and_files`): if it isn't there, VOS keeps its
  state in a form the scan can't read (compressed, encrypted) and the clean scan of those files meant nothing, so the
  test fails. The redaction control: `SecretValue`'s `Debug` prints `<redacted>`, so a record that formats it that way
  carries nothing, and the scan finds the plaintext in a record that wrote `as_str()` instead
  (`scan_finds_a_secret_written_with_its_redaction_off`). The scan's own refusals (no value, a short value, no
  surface, an empty surface) are tested, so a scan of nothing can't pass.
- **Defaults (reversible, fail closed).** A scan with no value, a value under `MIN_LEN` (8), two values of one name, no
  surface or an empty one fails. A clean scan of the nodes' files needs at least one file and one byte. The run must
  complete (every order hosted, no route stopped, a non-empty log) or the scan says nothing. Three seeds (`1,2,3`)
  for the clean run, one per kind for the controls. The leaked input rides in an order id, a record id and a payload field.
- **Gaps and owner items.** (1) **The credential and the private state now travel** (review of `a3c3172c`). The simulated providers declare
  the secret `api-key` on `sim.settle` and in the outside part's `secrets` (`conformance/scenarios/src/sim/fleet.rs`);
  the guest's outside part (`components/prov/module-guests/sim/`, a `T-024-01` file) resolves it through the proxy
  (`proxy::look-up`) for each settlement and sends it as a sensitive `authorization: Bearer` header on the rail
  call, and reads the provider's `private/notes` at the start of each operation and hands it back to no one. The
  platform's records of both reach Core's log (`secret_lookup`, `state_read`: `run_looks_the_planted_credential_up_and_reads_the_private_state_it_holds`),
  and the world's rail receives the credential in a header; the clean scan finds none of the planted values in
  the journal, the log, the shelf or the nodes' files. What still needs `F-030`: the production `CredentialProxy`'s
  own use records and the deployment's store (the assembly's world is a simulation, `NoProxy` serves the segments),
  so the scan shows no path of the assembled Core copies a resolved value, not that the production proxy's records
  keep it out. Owner: `F-030` re-runs the scan against them; `T-025-04` cites this limit. The real keys flow
  too (each provider's statement key is resolved for every statement it signs; the operator's signs every decision).
  (2) The scan sees a plaintext value in the forms above, not one a record
  encodes some other way (compression, a cipher, a hash of a part), and not a leak by length or by timing. Owner:
  `T-025-05`'s review. (3) The module state the simulated modules also read (`book`, the deployment) is not
  seeked: the book repeats what the log records (ids and amounts), so a hit would not tell a leak from the record
  itself. (4) The VOS nodes' directories are read after Core is dropped and the cluster stopped, so the files
  are what the nodes left, not a live read. (5) `stm-vos.yml`'s earlier scenarios step now names its test targets
  (`--test scenarios --test wire`) so the scan's two tests run once, in their own step. (6) `REQ-AUD-6` also covers the holding-account balances and the like, which this scan does not seek (it seeks
  credentials, secrets and module state): owner `T-025-04`, to restate that coverage with this row's limits. (7) The VOS test and the
  workflow step are only type-checked here (`cargo check --features vos` with the stable toolchain); they have not run.
- **What only CI shows.** The two VOS tests (`vos journal after a full run holds no credential or module state` and its
  controls) and the step that runs them: whether the nodes' files hold the journal's bytes in a form the scan reads,
  and so whether a clean scan of them is worth anything, and whether the cluster's files fit the runner's disk and
  time. Green on three VOS replicas in `stm-vos.yml` run `44266493160`, at `b6f18e7a`: the leak control found each planted
  value in the nodes' own files, and a full run (seeds 1, 2, 3) left none.
- **Rule-off table.** Each rule of the scan switched off once, on the reference host, and the test that went red:

| Rule switched off | Test that went red | Seen |
|---|---|---|
| every form but the raw one | `scan_finds_a_value_in_each_form_a_record_writes_it` | red |
| base64 at alignments 1 and 2 | `scan_finds_a_value_in_each_form_a_record_writes_it` | red |
| the 8-byte minimum | `scan_with_nothing_to_seek_or_nothing_to_look_in_is_refused` | red |
| the refusal of an empty surface | `scan_with_nothing_to_seek_or_nothing_to_look_in_is_refused` | red |
| the refusal of no surface | `scan_with_nothing_to_seek_or_nothing_to_look_in_is_refused` | red |
| the chunks' overlap | `scan_of_a_directory_finds_a_value_in_a_file_and_across_a_chunk_boundary` | red |
| planting (no value seeded) | `reference_run_after_planting_holds_no_credential_secret_or_module_state` | red |
| Core's log records not read | `reference_run_after_planting_holds_no_credential_secret_or_module_state` | red |
| both hex forms | `scan_finds_a_value_in_each_form_a_record_writes_it` | red |
| the JSON-escaped form | `scan_finds_a_value_in_each_form_a_record_writes_it` | red |
| the JSON byte-array form | `scan_finds_a_value_in_each_form_a_record_writes_it` | red |
| the percent-encoded form | `scan_finds_a_value_in_each_form_a_record_writes_it` | red |
| the evidence shelf not read | `reference_run_after_planting_holds_no_credential_secret_or_module_state` | red |
| the leak's payload field | `leaked_input_carries_the_value_in_its_payload_and_not_only_in_its_ids` | red |
| the host's snapshot not read | `scan_finds_each_kind_of_value_let_through_the_state_machines_boundary` | red |
| the boundary opened once with no value in the input (the control's leak) | `scan_finds_each_kind_of_value_let_through_the_state_machines_boundary` | red |
| `SecretValue`'s redaction (`contracts/`, not ours to edit) | the control formats the value both ways; the rule was not switched off | not switched |
| the nodes' files not read, or not readable by the scan | `scan_finds_each_kind_of_value_let_into_three_vos_replicas_journal_and_files` | CI only |

## T-025-04

The threat re-check of SAD §6 (`ARCHITECTURE.md` §6, "Threat-model revisit"), re-run on 2026-11-18 at `m3`'s `3d9b6a45`,
on the row of `T-008-04`'s draft (`experiments/2026-11-07-module-format/drafts/thr-85-recheck.md`) and the evidence
of `T-025-01`, `T-025-02`, `T-025-03` and `T-025-06`, above, and of the rows named per area. **Docs only: no code, no
test and no contract changes; `ARCHITECTURE.md` and `SPEC.md` are not edited.** The amendments are drafted in
`amendments.md`, beside this file, for the owner. The reading follows `T-025-01` to `T-025-03`'s own: a status rests on
a test or a note that is cited, and on a CI run only where the note records its id.

### Evidence in one place

| Evidence | What it shows | Run or place |
|---|---|---|
| `T-025-01`'s reading of 2026-11-17 | 94 advisories against `bytecodealliance/wasmtime`; none reachable with the engine's configuration; the AVX advisory not affected; `engines_compile_target_enables_no_avx_feature`, `engine_leaves_off_the_proposals_the_runtime_advisories_of_2026_11_16_need` | above, `T-025-01` |
| `T-025-06` | Wasmtime 48.0.9, the release that fixes the six advisories whose ranges included 48.0.8; none names 48.0.9; `pinned_runtime_is_the_patched_release`; machine-code digests re-derived on another vendor | `ci` job `134220782586` and `rederive` job `134247456427` of run `44134656776`, at `0f523b38` (`rederive`: GenuineIntel against a reference made on AuthenticAMD, artifact `14353433711`) |
| `T-025-02` | Each of the jail's layers refused the hostile act alone, each against a control without it; `G-05`'s `RLIMIT_AS` on the shipped provider; no layer name in the release build | `jail-probes` run `44242098797` at `19fb8314` (**red**: the descriptor control was fixed in `943fad16`; its per-probe table is above); green run `44243240737` at `943fad16`, 14 passed, 0 failed, with `ci` run `44243240720` green at the same commit; and at this row's head `8c1dbcb3`, `jail-probes` run `44317628090` (14 passed, 0 failed) with `ci` run `44317628245` (job `ci` success; its `rederive` job skipped, the runner being AMD, which this row does not rest on) |
| `T-025-03` | No planted credential, secret or module state in the host's journal and snapshot, Core's log, the evidence shelf or, on three VOS replicas, the nodes' own files; the leak controls find each planted value | `stm-vos.yml` run `44266493160` at `b6f18e7a` (the run that first showed it); at `m3`'s `3d9b6a45`, the base of this row, run `44316784467` (`m3`) and run `44308204207` (`m3-next`), each with its step "No credential, secret or module state in the journal or the nodes' files of three VOS replicas (`T-025-03`)" green; reference-host tests in `ci.yml` |
| `T-014-05` (`F-014`'s notes) | The name lookup is waited for only until the operation's deadline; abandoned lookups' threads are capped per provider | `lookups_past_the_deadline_are_abandoned_and_bounded_per_provider` (`components/prov/tests/egress.rs`) |

**At this row's commit.** This row changes no code, so its code is `m3`'s `3d9b6a45`. *The probes:* `jail-probes` ran at
this row's head `8c1dbcb3` in run `44317628090` (14 passed, 0 failed), and `ci` in run `44317628245`. No jail-layer file
(`sandbox.rs`, `layers.rs`), probe file or `jail-probes.yml` has changed since `943fad16` either. *The scan:* the scan's
world did change after run `44266493160` at `b6f18e7a` (`conformance/scenarios/src/sim/*`,
`components/prov/src/module/platform.rs`), so that run is not the scan at this code; the scan at this code is `stm-vos.yml`
at `3d9b6a45`: run `44316784467` (`m3`) and run `44308204207` (`m3-next`), each with the `T-025-03` step green (read with
`gh run view` on 2026-11-18). Each run's `vos` job is green; the `exit code 101` annotation each carries belongs to the
G-54 step, whose result "is recorded and need not pass". What ran on the reference host (arm64 macOS) is in "Run here" below.

### The restatement, area by area

**1. `THR-85` (sandbox escape) and `THR-18` (build tampering).** SAD §6's row stands, with its status moved.

- *`THR-85` status:* **Narrowed further; the residual stays accepted (`R72`).** What was "probed only together" is now
  probed alone: each of the fifteen layers of the jail (the six namespaces, `no_new_privs`, the empty root, dropped
  capabilities, Landlock, seccomp, descriptors closed before `exec` and in the jail, `RLIMIT_AS`, the other rlimits) has
  a probe that must succeed in a control without the layer and fail with it alone, on Linux (`T-025-02`, runs
  `44242098797` and `44243240737`); the release build of the provider holds none of the layer names and no feature
  that turns one off (the same job; `release_build_of_the_provider_binary_has_no_way_to_turn_a_jail_layer_off`).
  `G-05` is seen: the shipped provider's own `--probe` reports the 4 GiB limit and the refused mapping (`T-025-02`).
  The `G-02` gap is closed: the pin was read against every advisory published to 2026-11-17 (`T-025-01`), six of the
  2026-11-16 ones included 48.0.8 and none was reachable, and the pin moved to 48.0.9, which no advisory names
  (`T-025-06`; `pinned_runtime_is_the_patched_release`). The AVX advisory is not affected for four reasons, one of
  them a test that refuses an engine target with an AVX feature on.
- *`THR-85` still open, as accepted:* unknown flaws and hardware side channels as a class; the Spectre settings are
  Wasmtime's defaults (`G-09`); no fuzzer-made binary was admitted (`B-09`); the seccomp allow-list was written from
  reading, not pruned from a trace. None of these four is the subject of a row of `F-025` (`T-025-01`, `-02`, `-03`, `-06`
  touch none), so each stands as `T-008-04`'s draft stated it. **The jail's UTS namespace stops nothing alone** against an
  unprivileged jail (the user namespace already refuses `sethostname`); the probe shows only that a rename stays inside
  (`T-025-02`, gap 1): depth only, an owner item. **`jail-probes` is not a required check** of the repository, so a red
  run does not block a merge by itself (`T-025-02`): an owner item. The AppArmor setting on Ubuntu 24.04 is an operator
  obligation, placed with `F-033` and `F-030` (`M4`, `DEC-27`), not an `M3` item.
- *`THR-18` status:* **Mitigated (detects only), the vendor gap narrowed.** The digests are the platform's own, checked
  before load. After the bump, `rederive` re-derived them on GenuineIntel against a reference made on AuthenticAMD and
  they matched (job `134247456427`): the draft's "no other vendor was tried" no longer holds, and the check now spans two
  vendors. `BINDING_VERSION` stayed `0.3.0` (the compatibility hash did not change, `ci` job `134220782586`).
  Residual: a compiler flaw that makes the same wrong code on every machine is still seen only by someone who
  re-derives with another compiler; unchanged.
- *Gaps recorded, no owner row:* RustSec does not yet carry the 2026-11-16 advisories, so `cargo audit` is silent on
  them (`T-025-01`, `T-025-06`; closes itself when RustSec imports them); the x86-64 CPU-flag test was seen on code made
  on an arm64 host, and a run on x86-64 Linux is the other half (`T-025-01`, gap 2; the notes record no run id of it);
  the release scan is a string scan (`T-025-02`, gap 2, for `T-025-05`'s review).

**2. `THR-132` and `THR-130` (checkpoints).** SAD §6's row stands, **restated as built in `F-022`**.

- *Status:* **Narrowed, as the SAD said, and now in code.** A checkpoint is fixed-length (a 16-digit tick, a SHA-256
  commitment, an Ed25519 signature) and holds no size, no root, no salt and no count; the salt and size are the
  checkpoint's opening, kept apart (`F-022`'s notes, "What is published, and what is not"). `AC-37`'s bar is shown by
  `checkpoints_of_busy_and_idle_periods_cant_be_told_apart` and `checkpoints_of_busy_and_idle_periods_still_cant_be_told_apart`
  (`components/core-audit/tests/checkpoints.rs`), `a_commitment_binds_every_part_and_the_salt_is_not_guessable`, and
  `busy_and_idle_periods_give_indistinguishable_checkpoints` (`conformance/scenarios/tests/audit_scenarios.rs`);
  `altered_history_fails_against_a_checkpoint` shows `THR-132`'s detection. These run in `ci.yml`'s
  `cargo test --workspace` (green at `943fad16`, run `44243240720`); no run id of these tests alone is recorded in `F-022`'s notes.
- *Residual (`THR-130`):* **a party's receipt names the log's size when it was issued**, so a party asking for receipts
  at two times learns how much the log grew between them: activity, which `REQ-AUD-14` keeps out of checkpoints. `F-022`'s
  notes put it "on the form as accepted for v0". Also recorded by `F-022` for `T-025-05`: a receipt's inclusion proof
  holds neighbours' leaf hashes (linkage, no balance); an erased file's blocks may stay on disk (the deployment's storage,
  `F-030`). `THR-131` is unchanged: a checkpoint proves only that the log is self-consistent. `THR-132` stays
  detect-only: a skipped tick shows against the schedule, and the checker reports a tick the operator never published.

**3. `THR-10`, `THR-135`, `THR-136` (route computation, split-gaming).** SAD §6's row stands: **`THR-10` and `THR-136` detect-only,
`THR-135` bounds, as `SPEC.md` ends them; no new mitigation.** Since the SAD: the router is a pure function of (request, snapshot)
with ties broken on the lowest candidate id by bytes (`F-004`'s notes; `THR-10`), every result is a split of one part
whatever the limit (`every_candidate_is_one_part_whatever_the_part_limit`, `every_result_is_a_split_of_one_part`:
`v0`'s router sends the whole amount as one part, `DEC-8`; `THR-135`), the per-provider and per-publisher ceilings drop a candidate over a ceiling
(`F-004`'s notes, `THR-136`, `REQ-ORD-8`), and an order of several parts closes only when each has ended
(`an_order_of_two_parts_closes_only_when_both_have_ended`). The bulk measures (`REQ-AUD-7`) exist in `core-audit`
(`T-022-06`) and orchestration now calls `Measures::measure` when a segment ends (`T-020-31`, merged): quotes against
executed prices, dry runs against outcomes, refusals, reversals. **Residual:** `failure.market` stays absent, since no
part holds a market figure in `M3` (`T-020-31`, gap 3), so "failures against market direction" is not measured; the
production assembly owns calling `write_measures` (`F-030`, `T-020-31` gap 1); a provider still sees only its own part
(`THR-136`, unchanged, accepted).

**4. `THR-79`, `THR-80`, `THR-86` (egress, over-scope credentials, cross-module calls).**

- *`THR-79` status:* **Narrowed, with the accepted concentration risk unchanged** (`credential-proxy-scope`, ADR-0004;
  `PLAN.md` §6.2 risk 5). New in `M2`/`M3`: the proxy hands out a secret only for the caller's declared operation; the
  platform's wait on a network step is bounded by the operation's deadline (`T-014-05`/`T-014-06`, slow-peer and
  store-dribbling tests); and **`T-025-03`'s scan** shows no planted credential, signing key or module state in the
  host's journal and snapshot, Core's log, the evidence shelf or three VOS replicas' files, the scan seeing a leak when one
  is let in (`stm-vos.yml` runs `44316784467` and `44308204207` at `3d9b6a45`; first seen in `44266493160` at `b6f18e7a`). *Limits, as `T-025-03` states them:* (1) production's
  `CredentialProxy` use records and the deployment's store are not scanned (the assembly's world is a simulation):
  `F-030` re-runs the scan against them; (2) a value encoded some other way (a cipher, a hash of a part), a leak by length
  or by timing, is not seen; (3) the module state the simulated modules also read (`book`) is not seeked; (4) the VOS
  files are read after the nodes stop, not live; (5) **`REQ-AUD-6`'s other half, "no balance", is not what the scan seeks**
  (it seeks credentials, secrets and module state): the claim this row can make is "no secret plaintext in the
  records a full run leaves", not "no balance". Residual either way: misuse at an allowed host and data leaving in a
  request to one.
- *`THR-80`:* **Unchanged, Mitigated (bounds), `OBL-1`.** The credential's scope is the provider's side; rutent can only
  bound what a declared operation does with it. Nothing in `M2` or `M3` changes it.
- *`THR-86` status:* **Narrowed to the route, with tests.** A module can only call a provider its route names, and only
  where it declares the call (`REQ-MDL-21`: `components/prov/tests/proxy_calls.rs`, `route_calls.rs`; the router's
  `calls.rs` refuses `not_named_by_route`, `F-011`'s notes); a call back into a provider already running is refused on
  purpose (`T-014-05`, re-entrance decided against the stack). The statement signer signs only what was asked
  (`T-014-05`: `judgement_about_another_subject_than_asked_is_not_signed`).
- *Name resolution (`T-014-05`'s residual): see "Residuals", `R-DNS`.* Bears on `THR-57` (Mitigated, bounds,
  `REQ-MDL-11`), which SAD §6 and `SPEC.md`'s ending do not mention.

**5. `THR-2`, `THR-66`, `THR-67` (operator and role-overlap compromise).** **Unchanged: Accepted, `R119`**, as ruled in
`R115`, `R48` and `R50`. Nothing in `M2` or `M3` narrows them. The one control added since, ADR-0011's separation of
duties for a stuck order's resolution, requires two actors where one rogue actor sufficed, and by its own text "a fully
compromised operator account ... defeats this entirely" (ADR-0011, Consequences); it changes no ending. The `T-025-03`
scan does not bear on these rows: it reads what a full run leaves, not what an intruder in the operator's systems does.

### Residuals

Each is an item on the owner's form (proposed in this row's report: the lead adds them). The ids are kebab-case.

| Id | Residual | Threat | Evidence | Owner today |
|---|---|---|---|---|
| `R-DNS` (`name-lookup-threads`) | The lookup through `getaddrinfo` takes no timeout. `T-014-05` made the platform stop waiting at the deadline, so the **operation** is bounded (`ThreadedResolver`); the abandoned lookup's **thread** lives until the operating system's resolver returns, and is counted against its provider's cap of 4 (`MAX_LOOKUPS_IN_FLIGHT`), after which that provider's lookups are refused as `limit_reached` until a thread ends. One provider's hung resolver can use its own four, not another's. | `THR-57`, `THR-79` | `lookups_past_the_deadline_are_abandoned_and_bounded_per_provider`; `F-014`'s notes, "T-014-05, the name lookup bounded by the deadline". `retrospectives/M2.md` and `PLAN.md`'s `M3` left-over row still say the lookup is "not bounded": that is the state before the follow-up, and should be reworded | none: a fix, if chosen, is a new row |
| `R-UTS` (`uts-namespace-depth`) | The UTS namespace stops nothing alone | `THR-85` | `T-025-02`, gap 1 | Pablo |
| `R-JAIL-GATE` (`jail-probes-required`) | `jail-probes` is not a required check | `THR-85` | `T-025-02`, after review | Pablo (branch protection) |
| `R-SAD-AMEND` (`recheck-amendments`) | The drafted amendments to SAD §6 and `SPEC.md` §8, for the owner to apply | all five areas | `amendments.md` | Pablo |
| `R-BALANCE-SCAN` (`balance-scan-coverage`) | `REQ-AUD-6`'s "no balance" half is not scanned | `THR-49`, `THR-79` | `T-025-03`, gap 6 | Pablo, to say whether the SPEC test line stays one scan |

Already owned elsewhere, restated and not new items: the production proxy's use records and the store behind it
(`F-030` re-runs `T-025-03`'s scan); the AppArmor obligation (`F-033`, `F-030`, `DEC-27`); `failure.market` (a part that
holds a market figure); `THR-130`'s receipt size (on the form, accepted for v0); RustSec's lag (closes itself); the x86-64
CI half of `T-025-01`'s flag test; the unpruned allow-list, `G-09` and `B-09`.

### Run here (reference host, arm64 macOS), 2026-11-18

The commands were `cargo test -p rutent-scenarios --test secrets_scan` and
`cargo test -p rutent-prov --features jail-layers --test jail_layers`, each under a 1500 s alarm, through the lock.
- **`T-025-03`'s scan, reference host:** 9 passed, 0 failed (the clean run after planting, each leak control, the
  redaction control, the scan's refusals). The VOS tests are not run here.
- **`T-025-02`'s probes, off Linux:** 1 passed (`a_layer_that_cannot_stand_alone_is_refused_with_its_reason`), 13 ignored by
  design ("the jail's layers are Linux's"). So the Linux probes, including `network_probe_is_refused_by_the_network_namespace_alone`,
  are not re-run here: CI's `jail-probes` is.
- `cargo fmt --all -- --check`, `plan-check --strict` (0 findings) and `tag-check --strict` (2057 tests, 0 findings): clean.

### Defaults taken

- **Statuses by what the notes record.** A status cites a test or a note; where only CI showed it, the run id the notes
  record, and none where none is recorded (said so). A reversible default.
- **`THR-57` is named in the re-check** though SAD §6's table does not list it, since the one residual `PLAN.md` hands to
  this row (name resolution) is its. The draft amendment adds it as a row, for the owner to take or drop.
- **No SPEC ending changes except two sentences** (`THR-130`, `THR-57`), drafted in `amendments.md`: every other ending
  stands, as the draft of `T-008-04` found for `THR-85`.

### Gaps

- `stm-vos.yml` does not run on this branch: the scan is cited at `3d9b6a45` (this row's base, the same code), not at the row's head.
- The first row of "Residuals" shows `PLAN.md` row 605 and `retrospectives/M2.md` out of date by the follow-up. Neither is
  `F-025`'s file to edit. Owner: the orchestrator.

## T-025-05

The security review of what `M3` changed on `DEC-20`'s paths, by a reviewer that wrote none of it. Branch
`f-025/t-025-05-security-review`, cut from `m3` at `0e3f68db`. The diff is `fb883e01..0e3f68db`: `fb883e01` is
`origin/develop` and the merge base (`M2` merged), so it holds `M3`'s changes alone. `m3` at `0e3f68db` holds every `M3`
row but `T-020-35`'s report-pair half, which is in review and touches no `DEC-20` path. **Docs only: no code, no test, no
contract and no decision changes; `ARCHITECTURE.md` and `SPEC.md` are not edited.**

**The paths.** `DEC-20`'s prefixes as `HOUSE-RULES.md` and `.github/CODEOWNERS` list them, with
`components/prov/src/module/` whole, `tools/boundary-check/` and `conformance/evidence/v0-scope`, which this row adds:
1,463 paths, 156 of them code, schemas, docs and workflows, and 1,307 contract fixtures. Each is in
`t-025-05-paths.md`, beside this file, with its verdict. No `experiments/…-binding/`, `tools/licence-check/`,
`components/prov/src/catalogue/signature.rs` or `components/prov/src/module/bin/compiler.rs` path changed.

**How far it was read.** Read whole: the two handles and the entry types (`contracts/src/core_internal/calls.rs`), the
log's attribution (`attribution.rs`), `admit_entry` and the rows that move or free value (`effect-permits-a-decided-action`,
`stuck-effect-permits-a-stuck-move`, `permit-backs-one-effect`, `decision-is-for-its-caller`, the hold, arrival and erasure
rows), the log and its register (`core-audit/src/log/mod.rs`, `keys.rs`), the checkpointer's store and cursor, the
receipt issuer's signing, the evidence store's `fetch`, the policy engine's consultation, verdict and `decide`
(`core-authz/src/engine/mod.rs`), `held` and the move's authorisation check (`funds`), the stuck rules (`stuck`), every
`components/prov/src/module/`, `proxy/` and `egress/` hunk, the three workflow diffs, `tools/boundary-check`,
`tools/contract-guard`'s new rule, the scope file and its parser, and `licences/RECORD`'s hunk. Read by purpose and spot
checks: the other 90-odd citation rows, `standing.rs`, `check.rs`, the measures, comparisons and `audit-check`, the
session, roles, parties and person-question rules, the contract schemas and changelogs. Fixtures: grouped, searched for key
material (none), and taken as `contracts/tests/`' to read valid or refused. Read on the orchestration side, outside
`DEC-20`, for the row's named areas: the production stuck mover's operation (`core-orch/src/stuck/mover.rs`, built from
the effect's own action), `carry_out` (`stuck/mod.rs`), the question identifiers (`rounds/ports.rs`) and the assembly's
policy side (`conformance/scenarios/src/assembly/policy.rs`).

### Named tests, at `0e3f68db`

Run through the lock on the reference host (arm64 macOS), each under a 1500 s alarm:

| Test | Crate, binary | Result |
|---|---|---|
| `every value moving action is refused when policy denies` [`REQ-AUZ-1`] | `rutent-core-authz --lib` | 1 passed |
| `rewrite with full operator access is detected by the checker` [`REQ-AUD-9`] | `rutent-core-audit --test checkpoints` | 1 passed |
| `statement signed by a key not current at its making is refused before the record` [`THR-3`] | `rutent-core-orch --test snapshot_statements` | 1 passed |

No finding is high or critical, so no new test is owed by `DEC-20`'s rule for this row.

### Findings

None high or critical. Each is routed to its owning feature as a new row or an item on a row; none is fixed here.

| # | Severity | Area | Where | Scenario | Owner, row |
|---|---|---|---|---|---|
| M1 | Medium | `INV-8` at the platform (`T-013-09`) | `components/prov/src/module/platform.rs:509` (`carried`), `:549`, `:2351` (`repeated`); `lifecycle.rs:330`, `:597` (`ended`) | The record of what was carried out under each transaction, and the set of ended transactions, live in the platform's memory alone. After the platform's process restarts mid-segment, a repeat under a transaction already carried out for a provider that declares no safe retry finds no record and reaches the module, which moves it a second time: `T-024-08`'s seed-110 double move, after a crash instead of in one run. `F-013`'s gap says the record lasts "until the platform stops" but names no consequence. Probe for the row, written first: open a platform, carry out a `deposit` for a provider without safe retry, drop the platform, open a new one on the same paths and world, hand it the same operation: today it runs; it must be refused `DuplicateTransaction` | `F-013` with `F-030`: a new row, "the platform's carried-out record survives a restart" (a durable record, or Core's journal handing it back on opening), test `a repeat after the platform restarts moves nothing twice` [`INV-8`]. Before production (`M4`); `M3`'s assembly never restarts a platform |
| L1 | Low | The question identifiers (`REQ-CAT-5`, `T-020-06`) | `conformance/scenarios/src/kit.rs:180-221` (`Seeded`), `components/core-orch/src/rounds/ports.rs:26-31` (`Fresh`) | The assembly draws each question's transaction from SplitMix64 over a seed (`DEC-18`). SplitMix64's output is invertible, so a provider holding one identifier can recover the state and every later one, and two providers holding one each can tell how many questions lie between them: the count `Fresh`'s documentation says no identifier carries. Right for a replayable test assembly, wrong for a deployment | `F-030`: an item on its assembly row, "`Fresh` in production draws 128 bits from the operating system's generator"; nothing in `M3` |
| L2 | Low | The scope file's switch (`T-024-09`) | `tools/records/src/evidence.rs:130-140` | `parse_scope` takes every `strict:` line and the last wins, with no finding for a second one, so a file reading `strict: on` at the top and `strict: off` lower down runs off. Once `develop` reads `on`, the ratchet catches a head that reads off; until then, and for a reader skimming the top of the file, it hides | `F-024`, an item on `T-024-10`: refuse a second `strict:` line as `bad-row` |
| L3 | Low | The assembly's policy side | `conformance/scenarios/src/assembly/policy.rs:589-604` | `PolicySide::decline_outcome` still answers `Unavailable` and says the engine has no `decline_outcome`, though `T-021-09` built it (`components/core-authz/src/stuck/mod.rs:597`). Fails closed (a wrong report holds its part), so no value moves wrongly; the assembled Core can't decline a report | `F-024`: forward to the engine, as the other calls do |
| L4 | Low | CI's jail step | `.github/workflows/ci.yml:64-69` | The jail probes' step in `ci.yml` sets no `RUTENT_RELEASE_PROVIDER`, so the release scan builds its own release provider inside the test (minutes), as `T-024-18`'s low said. Not a hole: the scan and the resolved-graph check still run | `F-024` (`ci.yml`), the lead's call |

### The areas the row names, with their verdicts

- **`core-internal` and the router-api bump.** Sound. The general handle's `GeneralEntry` can't be built or read holding
  a decision kind or a writer-attributed statement (`calls.rs:104-118`), policy's `DecisionEntry` holds nothing else
  (`:140-150`), and the log holds the path to the kind again before its retry rule (`log/mod.rs`, `append_through`) and in
  `admit_entry` (`relations.rs:6138`). An effect rests on an unused permitting decide of the same action
  (`effect-permits-a-decided-action`, `permit-backs-one-effect`); a stuck order's value only on a stuck move
  (`stuck-effect-permits-a-stuck-move`), and `decide` refuses `resolve_stuck` unrecorded. Router-api 4.0.0's held quote is
  bound to its record by id and digest, and capped by its figures' expiry (changelog). Residuals R-HANDLES, R-NODE,
  R-RELIANCE, R-HOLDER below.
- **`CMP-CORE-AUTHZ`'s policy engine.** Sound. One consultation mints a permit (`engine/mod.rs`, `consult`): the fixed rules
  first, the deployment's policy after; a refusal beats not known, and not known never becomes a permit; a missing input
  of an order record refuses as `not_permitted`, so an order that isn't the caller's reads like one that doesn't exist.
  `held` only lowers an answer (`funds/mod.rs:542`); a move is held to its authorisation's destination, asset and floor,
  compared by value (`router_api/atoms.rs:336`). The stuck rules keep deciding, moving and confirming as separate
  permissions, decider and mover in the operator's role and the operator's by the register, neither the holder nor the
  order's customer, and the mover another accountable actor than the decider. The engine holds only `L: PolicyLog`.
- **The log's and the checkpoints' signing.** Sound. Receipts and checkpoints are signed with the log's own key, which
  both issuers refuse to be the deployment's `operator_key`, as provider-role signs (Ed25519 over the RFC 8785 JSON
  without `signature.value`), checked by the strict verifier, and kept apart by `shape`; the commitment is hashed under a
  domain of its own (`checkpoint/mod.rs:54`). The shared key is a residual, R-KEY.
- **`T-020-05`'s binding of statements to keys.** Sound. Orchestration binds through the catalogue
  (`snapshot/statements.rs`), refusing a statement dated more than the skew ahead; the log binds again, independently,
  from its own register (`keys.rs`): a provider's key signs at `made_at` only where a grant is in force then and both the
  advertisement in force then and the one accepted last list it for statements, so a key dropped by a rotation signs
  nothing, backdated or not. The register is fed only by entries the log recorded (catalogue changes on permitted
  effects with a `done` result, advertisements whose kept bytes it re-checks) and the deployment's settings, never by
  orchestration: the row's "statement keys" residual holds as stated (R-KEYS). Residuals R-BACKDATE, R-FUTURE.
- **The jail's test-only builder (`T-025-02`).** Sound. In a shipped build `Layers` is a unit type whose `on` is always
  true (`layers.rs:44-58`), the shipped `isolated` and `confine` pass `Layers::ALL`, and `jail` exists only with the
  `jail-layers` feature. `T-025-02`'s gap 2 (the release scan is a string scan) is answered in the same test: it also reads
  the resolved feature graph of the release command line and every workspace manifest through `cargo metadata`, and fails
  if any package's feature or any dependency on `rutent-prov` enables `jail-layers` (`components/prov/tests/jail_layers.rs:430-500`).
  The comment in `components/prov/Cargo.toml` credits `tools/boundary-check` with refusing a self-dependency; the metadata
  check is the one that refuses the scenario assembly turning the feature on, which the boundary check allows.
- **Every workflow edit of `M3`** (`ci.yml`, `jail-probes.yml`, `stm-vos.yml`). Sound. Each keeps `contents: read`,
  `persist-credentials: false` and actions pinned by commit; no `pull_request_target`, no secret is read, and no step runs
  code from the `develop` checkout (`.evidence-base` is read, and hidden from the test walk). The toolchain is pinned to
  1.98.0. `jail-probes` is still not a required check (R-JAIL-GATE, `T-025-04`'s). Low L4.
- **`conformance/evidence/v0-scope`, the input that switches a gate.** Sound at `0e3f68db`. `strict: off`, as `T-024-09`
  leaves it for `T-024-10`. Its classes are exactly `T-024-09`'s: `m4` `REQ-OPS-4`, `REQ-PRT-2`, `REQ-PRT-4`; deferred
  `REQ-QTE-16`, `REQ-ORD-42`, `AC-38` (`DEC-8`), `REQ-CPL-1`, `-4`, `-7`, `-10`, `AC-16` (`DEC-16`), `REQ-PRT-3`, `GUA-6`
  (`DEC-17`), `REQ-EXT-1` (`F-052`), `NFR-1` (`ASM-1`, risk 9); partly met `REQ-CAT-5` (`form:cat5-timing-residual`);
  inspection `REQ-MOD-2`, `REQ-MOD-3`, `REQ-MOD-5`, `NFR-7`, `REQ-EXT-2`. 211 rows are `M3`'s. Every identifier `SPEC.md`
  defines in the five families has a row (the two `REQ-CHN` ids `SPEC.md` cites are Narthex's, not defined there). No
  `M3` row is reclassified. Only the `--ci` step reads the switch, and the ratchet against `develop` starts once the file
  is there (`compare_with_base`), so until `M3` reaches `develop` this review and `T-024-10`'s restriction are the only
  guard. Low L2.
- **The unlinkable questions (`T-020-06`).** Reviewed, **not counted closed**: each question is its own transaction, an
  identifier drawn fresh, carrying no order reference. The timing residual stands for the owner (`cat5-timing-residual`,
  on the form): providers comparing when their questions arrived can still line them up. Low L1 adds to it for a
  deployment's identifiers.
- **The fuzzing verdict for the admitting compiler (`B-09`).** See below.
- **The contract rounds after `T-020-01`** (provider-role's arrival notice `T-020-18`, `T-020-21`, `T-020-30`, `T-020-32`,
  `T-022-07`'s checkpoint shapes). Sound. An arrival naming an order must name its notice (`check.rs:528-531`), the notice
  must be the reserve's verified `arrival-notice` and nothing else (`relations.rs:2959`), and one deposit opens one order,
  keyed on the notice's signed transaction. A category grant attributes a verifier's judgements of its classes alone
  (`attribution.rs`, `classes_alone`).
- **The production stuck mover and its hand-on arm (`T-020-16`, `T-020-17`).** Sound. `carry_out` asks policy's
  `move_stuck` and appends the effect before anything is handed to the platform; the operation the mover signs is built
  from the effect's own `resolve_stuck` action (`stuck/mover.rs:530-600`), so what moves is what the log admitted against
  the decided resolution. Residual R-HOLDER.
- **Signed holds and `redeemed_by` (`T-020-15`).** Sound. A held quote rests on the reserve's verified hold about its own
  transaction and no order, and no second order redeems it (`hold-is-redeemed-by-one-order`, keyed on the hold's
  transaction, so a copy kept under a new commitment is the same hold). The hold's validity and its order or session are
  checked by Core on intake from the kept bytes, as the row says; the log holds only the commitment.
- **The arrival read from the reserve's signed notice (`T-020-19`), and `NOT_A_NOTICE`'s hold case.** Closed: the
  negative list is gone, and the positive shape check above replaces it.
- **The writer of `reliance` entries (`T-020-23`).** Sound as built; residual R-RELIANCE (`T-020-34`'s gap 1).
- **Carry-on and `AcceptFault` reads (`T-020-24`), the final deposit's writer (`T-020-22`), the holding account's
  withdrawal record (`T-020-26`), the late-status ask and the later-node reversal (`T-020-27`), a report's correction
  (`T-020-20`), policy's order-record answer (`T-021-08`), its decline and late-status rule (`T-021-09`).** Read by their
  rows in the citation table and the engine; no finding. The order-record answer (`Sources::order_record`) is `Missing`
  by default, so an order no record knows is refused.
- **The log's two handles (`T-020-01`).** Sound while Core is one process: residual R-HANDLES.
- **A question under a false label.** As the row says: the log binds a question to the order it names, and only a
  comparison of its signed account and amount with that order's catches one orchestration asked for another
  (`core-internal`'s README). Residual R-LABEL.
- **The log's opening (`T-022-08`).** Closed as the row sent it: `read_back` runs the append's attribution again over each
  stated entry the store marks `verified`, against the history and register as they stood, and refuses one the kept
  statement contradicts or whose statement is gone without a decided erasure. Residual R-REOPEN (gap 5 of `T-022-08`).
- **A maker's resend and a relay's copy (`T-022-02`).** Residual R-RELAY, as recorded.
- **The checkpoint store's cursor (`T-022-04`).** Residual R-CURSOR.
- **The platform's repeat refusal (`T-013-09`, `T-024-08`'s finding 1).** Closed for one run: `repeated` refuses a second
  operation of any non-read kind under a carried-out transaction for a provider without a declared safe retry, and
  answers a safe one's identical repeat from its record; an unreadable declaration is no safe retry. Medium M1 for a
  restart. The foreign-envelope replay to a safe-retry provider rests on the module (R-ENVELOPE).
- **`F-024`'s stand-ins.** Closed: `PolicySide` forwards every call to the engine (`assembly/policy.rs:516-617`), and no copy
  of `consult` or `verdict` is left. The log's skew rules can't trip in the assembly (one clock); they are tested in
  `core-audit`'s own tests.

### Residuals

Each with its evidence and owner; "accepted" means accepted for v0 as it stands, "row" that it needs a row or an item of
its owning feature, and "form" that it waits on the owner's acceptance (the items are in this row's report; the lead adds
them).

| Id | Residual | Evidence | Owner | v0 |
|---|---|---|---|---|
| R-HANDLES | Decision entries reach the log only through policy's handle while Core's parts share one process. Split across processes, a decision would need the engine's signature for the log to tell it from a forgery. `Policy::new` and `General::new` are public: the boundary (only the assembly reaches `core-audit` and `core-authz`) is what keeps a second policy handle out | `calls.rs:13-18`, `log/mod.rs` (`General`, `Policy`), `tools/boundary-check/tests/graph.rs` | `F-021` with `F-022`, at any split of Core | form |
| R-KEYS | The log's statement keys hold only while it takes them from advertisements it recorded under permitted `change_catalogue` effects: confirmed, the register reads nothing from orchestration | `keys.rs` (`Register::record`, `join`, `admits`) | `F-022` | accepted |
| R-BACKDATE | `THR-3`: a key is checked as current at the statement's own `made_at`, so after a grant's revocation a key the provider's latest advertisement still lists can sign a statement dated inside the grant's window, and it is attributed `verified`. A rotation that drops the key does stop it. Use is bounded elsewhere: a judgement is carried only while its verifier's grant is in force at Core's clock (`T-020-05`), and a statement about an order needs a transaction Core asked under | `keys.rs:563-588`, `attribution.rs:487-494` | `F-022` | form |
| R-FUTURE | The log holds no stated entry's `made_at` to its own clock (only an advertisement's `received_at`, and a catalogue change's time); orchestration's catalogue check refuses one more than the skew ahead, the log doesn't. Rows that compare times also read log positions | `attribution.rs:480-496`, `prov/src/catalogue/advertisements.rs:413` | `F-022` | accepted (low) |
| R-LABEL | A question under a false label: bound by the log to the order it names, caught only by comparing its signed account and amount with that order's | `contracts/core-internal/README.md` | `F-020` | form |
| R-OPMODULE | Which module is the operator's: one whose release was done with no grant of it in the log and no advertisement. A grant after it changes nothing, so a provider can't be granted in after its release; the release itself rests on a permitted `accept_module` | `keys.rs:331-338`, `F-022`'s notes | `F-021` (the policy rule behind `accept_module`) | accepted |
| R-KEY | The log's one key signs receipts and checkpoints; the shapes and the commitment's domain keep them apart. A deployment may hand the checkpointer another key | `checkpoint/mod.rs:54`, `:214-217`, `receipts/receipt.rs:41` | `F-030` (the deployment's keys) | accepted |
| R-DOMAIN | The commitment's domain and byte layout are `core-audit`'s, not in the contract, so a checker outside rutent's crates can't recompute a commitment (`T-022-07`'s gap). Not a v0 security finding: `audit-check` is the checker | `checkpoint/mod.rs:54`, `:155` | `F-022`, at the next contract round if an outside checker is named (`F-033`) | accepted |
| R-CURSOR | The checkpoint store's cursor records are unsigned, so whoever writes the store can mark a checkpoint published that never was; the outside checker reports the tick missing against the schedule (`Finding::Missing`) | `checkpoint/mod.rs:128-131`, `:285-292`, `checker.rs:35` | `F-022` | accepted |
| R-OPENING | The checkpoint store holds each opening in the clear, so whoever reads it learns each tick's size, as whoever holds the history does; a receipt names the log's size (`THR-130`, on the form) and its proof holds neighbours' leaf hashes (linkage, no balance) | `F-022`'s notes | `F-022` | accepted (`THR-130` on the form) |
| R-REOPEN | The opening runs no `admit_entry` over what it reads back, so someone with full access to the log's file who forges a whole erasure path is taken for an erasure until the next checkpoint's check | `log/mod.rs` (`read_back`), `T-022-08`'s gap 5 | `F-022`; operator compromise is `R119`'s | accepted |
| R-FSYNC | After a failed `fsync` a later sync may succeed with the data gone; a record the store answered after such a failure isn't re-checked against the disk | `log/store.rs:105-135` | `F-030` (the deployment's storage) | accepted |
| R-RELAY | A maker's resend of its own statement and a relay's copy under one presenter identity read alike; only `Presented::presented_by` tells them apart | `F-022`'s notes, finding 4 | `F-022` | accepted |
| R-NODE | The log can't tell a node declared private from the catalogue's declarations in its relations: a final deposit, a holding record or a reversal naming a private node is let off the network comparison. The register does hold each accepted advertisement's private nodes (`keys.rs`, `nodes`), but `admit_entry` reads the history alone | `relations/closing.rs`, `F-020`'s notes (`T-020-22`, `T-020-26`) | `F-020`, a contract round reading the declarations the log recorded | row (low) |
| R-RELIANCE | A `reliance` entry naming a judgement that said no isn't refused by the writer or the log; the offer already refuses such a route, so it is defence in depth | `T-020-34`'s gap 1 | `F-020`, a contract item: the relation reads the judgement's answer | row (low) |
| R-HOLDER | The log admits a holding record whose holder an earlier failed `resolve_stuck` named, and the record and first move may sit at another node than where the stuck value rests; Core writes neither | `T-020-33`'s residuals | `F-020` | accepted |
| R-PUBLISHER | A node's early binding record and the later one a replay cites are never compared, and a record's segments aren't each held to a provider stating for its publisher | `T-020-32`'s residual, `F-020`'s notes (`T-020-36`, item 6) | `F-020` | row (low) |
| R-ENVELOPE | A foreign-envelope replay to a provider that declares safe retry reaches the module, whose envelope check the role suite asserts; the platform checks no signature | `platform.rs:2351-2378`, `F-013`'s gaps | `F-013` | accepted |
| R-PERSONAL | A tuple that requires a payer's personal detail gets it through Core, and the evidence store keeps a copy in the signed operation, where `REQ-CPL-11` wants it with the provider alone. v0's tuples require account references only | `F-021`'s notes (`T-021-07`) | `F-020` with `F-022` | form |
| R-CHECKLOG | A balance check is recorded in the catalogue's check log, not the audit log (`GUA-3`) | `F-024`'s notes | `F-021` | row (low) |
| R-FUZZ | No coverage-guided fuzzing of the admitting compiler; see the verdict | below | `F-013` | form |
| R-SCAN | `T-025-03`'s scan sees plaintext only, not a value encoded otherwise nor a leak by length or timing | `T-025-03`, limit 2 | `F-030`, which re-runs the scan on production's stores | accepted |

Restated, already owned and not new: `cat5-timing-residual` (on the form), R-JAIL-GATE, R-UTS and R-BALANCE-SCAN
(`T-025-04`'s), the checker and checkpointer outside the assembled Core (`F-030`), `R-DNS`.

### The fuzzing verdict (`B-09`, left by `T-013-05`)

**Not required for v0; the residual is accepted, with a condition.** What is admitted is a third party's component, and
the admitting compiler is Wasmtime 48.0.9's Cranelift (`engine.rs`), run in a jailed process of its own with `RLIMIT_AS`,
seccomp, Landlock and a deadline (`children::compile`), so a crash or a hang is a refusal of the release, not the host's.
Wasmtime's parser, validator and Cranelift are fuzzed continuously upstream with coverage guidance (OSS-Fuzz), which
covers the code a guided fuzzer of ours would mostly reach; what is ours is the judgement around it (imports, exports,
the declaration, the digests), which the seeded, section-aware mutation fuzzer reaches: after `T-013-05`'s review, 67% of
mutants passed the parser and 551 of 827 were judged on imports, exports or compiled, none crashed (CI runs 42754273532
and 42756806042). The condition: the pin is kept on a patched Wasmtime release (`T-025-01`, `T-025-06`), and a guided
fuzzer is a row of `F-013` before a module from a publisher outside the operator's contracts is admitted, or with
`F-030`'s hardening, whichever comes first. A fuzzer-made binary was never admitted (`B-09`), which stays under `THR-85`'s
accepted residual.

### Defaults taken

- **The path set** is `HOUSE-RULES.md`'s list with `components/prov/src/module/` whole, `tools/boundary-check/` and the
  scope file, as the brief gives it; the per-path record is a file of its own beside this one (`t-025-05-paths.md`), since
  1,463 rows would bury this section. A reversible default.
- **Fixtures are reviewed as data,** grouped, not one by one: they are what `contracts/tests/` reads, valid or refused.
- **Severity by reach in `M3`'s code:** what needs a production assembly that doesn't exist yet (M1, L1) is rated for
  the release that will carry it, and routed there.

### Gaps

- The review is a reading at `0e3f68db` and three re-run tests; no probe test was written, since nothing is high or
  critical. M1's probe is described for its row. The other 90-odd citation rows were read by name and purpose, not
  line by line.
- The Linux-only paths (the jail's layers, `stm-vos.yml`) were read, not run here: CI's `jail-probes` and `stm-vos`
  runs at `m3` are their evidence (`T-025-02`, `T-025-03`, `T-025-04`'s "Evidence in one place").
- `T-020-35`'s report-pair half is not on `m3` at this commit; it touches no `DEC-20` path, so a later diff of it needs
  only `DEC-20`'s check that it still doesn't.

### Addendum: T-024-19

`T-024-19` (pull request #241, `f-024/t-024-19-review-lows`, `9994463e..559b1d58`) answers L2, L3 and L4; its one `DEC-20`
path is `.github/workflows/ci.yml`. **Approved at `559b1d58`.** *The workflow:* a step `cargo build --release --locked -p
rutent-prov --bin rutent-wasm-provider` before the jail step, and `RUTENT_RELEASE_PROVIDER: ${{ github.workspace
}}/target/release/rutent-wasm-provider` on it. `ci.yml` sets no `CARGO_TARGET_DIR`, no cache and no `--target-dir` (its only
`GITHUB_ENV` write is `CLANG`), and the toolchain is pinned to 1.98.0, so the build lands at `target/release/` of the
checkout, the path named; the build adds no permission, secret or untrusted input (the job keeps `contents: read`, the
command line names no feature, so no `jail-layers`), and the probes' `--features jail-layers` test build goes to
`target/debug/`, leaving the release binary as built. What the results file gains is the scan probe's own line, no longer
shared with a build's output: in the `ci` job of run 44372822190 (green) the probe reads `test
release_build_of_the_provider_binary_has_no_way_to_turn_a_jail_layer_off ... ok`, no `Compiling rutent-prov` precedes it,
and both evidence-table steps read `GUA-5 | m3 | verified | 4 of 4 tests passed` (232 rows: 211 verified, 0 unverified, 0
table findings). `jail-probes`, `contract-guard`, `records` and the licence record are green on the same head. *The scope
file:* a second `strict:` line is now a `bad-row` finding and the first stands, so a file showing `strict: on` at its top
reads on; with a first `strict: off`, a later `on` is reported and the file reads off, as its top line shows, which is the
switch's own reading (report mode until `T-024-10`). Closes L2. *The assembly:* `decline_outcome` forwards to the engine,
as the other calls do, tested through the assembly (a decline of nothing is the log's refusal, nothing recorded). Closes
L3. L4 is closed. A nit, not blocking: the new comment in `ci.yml` says that without the variable "the probe skips"; it
builds its own release provider instead (`jail_layers.rs:380-402`), as `F-024`'s note on the row says correctly.

## The owner's answers of 2026-11-18 (applied 2026-11-23)

- **`recheck-amendments` (`R-SAD-AMEND`): "Apply all".** `amendments.md`'s A.1 to A.6 are applied to
  `ARCHITECTURE.md` §6 (the five rows, the `THR-57` row and the closing sentence; the pin in A.1's design cell now
  48.0.9) and B's two sentences to `SPEC.md` §8 (`THR-130`, `THR-57`), in `SPEC.md` 0.4.2. B's optional `THR-79`
  Coverage entry was "not proposed" and is not applied.
- **`cat5-timing-residual`: accepted for v0**, an accepted limit of `THR-47` in `SPEC.md` 0.4.2; the scope file classes
  `REQ-CAT-5` as `m3`.
- **`balance-scan-coverage` (`R-BALANCE-SCAN`):** a scan row, `T-024-20`, `F-024`'s since it owns the files.
- **`thr3-backdated-after-revocation`:** the `THR-3` residual this row recorded becomes `T-022-09`.
- **`T-025-05` runs again** in wave 36, over what the answers' rows change on `DEC-20`'s paths, against the commit
  of its first review.

## T-025-05, second pass (2026-11-24)

The second pass the row asks for, over what the owner's answers of 2026-11-18 changed on `DEC-20`'s paths, by a
reviewer that wrote none of it and none of the first pass. Branch `f-025/t-025-05-second-pass`, cut from `m3` at
`2caa6f55` (waves 33 to 35 merged: `T-020-39` to `T-020-43`, `T-021-11` to `T-021-13`, `T-022-09`, `T-022-10`,
`T-024-20`). The diff is `e63afbeb..2caa6f55`: `e63afbeb` is the first pass's tip on `m3` (it holds `0e3f68db`, the
commit the first pass read, and the `T-024-19` addendum). The rows' reviews and fix rounds are on pull requests #243 and
#256 to #264, and their notes in `F-020`, `F-021`, `F-022` and `F-024`. **Docs only: no code, test, contract or
decision changes; `PLAN.md`, `ARCHITECTURE.md` and `SPEC.md` are not edited** (the row records the pass's verdicts
here, and names nothing for `PLAN.md` to record).

**The paths.** `DEC-20`'s prefixes, as in the first pass: 92 paths, 33 of them code, schemas, docs and tests, and 59 new
contract fixtures. Each is in `t-025-05-paths.md`, section "Second pass", with its verdict. No workflow,
`components/prov/`, `tools/boundary-check/`, `tools/contract-guard/`, `tools/licence-check/` or `licences/` path changed,
so no `M3` workflow edit is new since the first pass. `conformance/evidence/v0-scope` changed by one line, `REQ-CAT-5`.

**How far it was read.** Read whole: the contract round's Rust (`attribution.rs`, `calls.rs`, `check.rs`, `types.rs`, the
`relations.rs` hunks, `provider_role/types.rs` and `validate.rs`, `router_api/check.rs`), `core-audit`'s `log/keys.rs`
and `receipts/store.rs` hunks, `core-authz`'s `roles/mod.rs`, `funds/mod.rs`, `engine/mod.rs` and `entry_points.rs`
hunks, the scope file's line. Read on the orchestration side, outside `DEC-20`, for the areas the brief names:
`stuck/party.rs` whole, `stuck/mod.rs`'s hunks, `segments/roll_back.rs` whole and `Segments::promote`, `erasure.rs`,
`arrival/filing.rs`'s hunks, the notice hunks (`notice/mod.rs`, `inbox.rs`, `summary.rs`), and the assembly's wiring
(`assembly/erasure.rs`, `releases.rs`, `mod.rs`, `policy.rs`'s `kept`). Read by name and purpose: the test files, the
contract tests, the changelogs and the README's tables. Fixtures: all 59 are new (`A`), grouped by row, searched for key
material (none: the keys in them are the suites' fixed public test keys), and taken as `contracts/tests/` reads them.

### Named tests and probes, at `2caa6f55`

Run through the lock on the reference host (arm64 macOS), each under a 1500 s alarm; the tree restored after the probe.

| Test | Crate, binary | Result |
|---|---|---|
| `every value moving action is refused when policy denies` [`REQ-AUZ-1`] (17 actions now, roll back among them) | `rutent-core-authz --lib` | 1 passed |
| `rewrite with full operator access is detected by the checker` [`REQ-AUD-9`] | `rutent-core-audit --test checkpoints` | 1 passed |
| `statement signed by a key not current at its making is refused before the record` [`THR-3`] | `rutent-core-orch --test snapshot_statements` | 1 passed |
| `no_public_entry_point_of_core_reaches_an_effect_without_a_decision`, `staging_is_rolled_back_only_on_a_permitted_decisions_effect` | `rutent-core-authz --lib` | 2 passed |
| `T-022-09`'s five key tests (refused after, attributed before, made again, another grant stands, ties) | `rutent-core-audit --test log` | 5 passed |
| `T-022-10`'s four store tests and `the_override_is_read_from_the_permitting_decision` | `rutent-core-audit --test receipts`, `--lib` | 5 passed |
| `stuck::party`'s four tests (the walk over every admission case among them) | `rutent-core-orch --lib` | 4 passed |
| `roll_back`'s five seam tests; `erasure`'s nine (`T-020-37`'s and `T-020-42`'s) | `rutent-core-orch --test roll_back`, `--test erasure` | 14 passed |

Probe (L5, written for this pass, not committed): with a grant's `revoke_grant` effect appended and its `done` result
not yet, a statement made before the revocation and received ten seconds after the effect's time is attributed
`verified`; the same statement appended after the `done` result is not (`PROBE between=true after=false`).

No finding is high or critical, so no new test is owed by `DEC-20`'s rule for this pass.

### Findings

None high or critical, none medium. Numbered on from the first pass. Each is routed to its owning feature; none is
fixed here.

| # | Severity | Area | Where | Scenario | Owner, item |
|---|---|---|---|---|---|
| L5 | Low | The received-time hold (`T-022-09`), between a revocation's effect and its result | `components/core-audit/src/log/keys.rs:323-360` (the register takes a grant or revocation only from its `done` result), `:459-489`; `contracts/src/core_internal/attribution.rs:151-161`, `:309` (the contract reads only changes "this log holds done") | A `revoke_grant` is admitted as an effect (dated within the skew), carried out on the catalogue, and only then resulted. A statement the log appends in between, made inside the grant's window and received after the effect's time, is attributed `verified`, and stays so on the opening and for an outside checker, both reading the history as it stood (probe above). The window is the time between the effect and its `done` result: within one call in the assembly, longer where the result's append is unavailable and retried. Orchestration's catalogue check doesn't close it: it reads the key at `made_at`, as the first pass recorded | `F-022` with a `core-internal` item: count a revocation from its admitted effect until a `failed` result undoes it, or hold the maker's statements unavailable while a `revoke_grant` of it has no result. `M4`, before an outside provider is granted |
| L6 | Low | The roll-back seam (`T-021-12`'s review low), and `promote` beside it | `components/core-orch/src/segments/roll_back.rs:134-148`, `:76-84`; `components/core-orch/src/segments/mod.rs:2789-2803` | `Segments::roll_back`, as `promote`, reads the staging before it asks policy: a caller policy would refuse learns that nothing is staged, or, through `NotTheStagedVersion { staged }`, the version staged, and its attempt leaves no decision in the log, where a refusal policy gives would be recorded, so such a probe goes unrecorded. The staged version is in the log already, for a checker, and the callers are the operator's logins through the assembly, so it discloses little | `F-020` (the seam) with `F-021`: ask policy first and check the staging after the permit, as `carry_out_roll_back` does again before the platform, with one answer for "nothing to do" and "not permitted". `M4`, or `M3` at the lead's call; it changes no contract |
| L7 | Low | `REQ-QTE-10` for an automatic deposit (`T-020-41`) | `components/core-orch/src/arrival/filing.rs:900`, `:782-790`; `contracts/src/core_internal/relations.rs:3665` (`acceptance-is-of-the-quote-shown` cites only an acceptance given in a session) | The rule that a route with no firm rate starts only on its customer's acceptance is orchestration's (`asks_the_customer`). The log admits an `automatic_deposit` acceptance of a quote it shows `quote_without_firm_rate`, or with `awaiting`, as of any quote, so a filing that skipped the wait would start a slow route on the deposit alone. Defence in depth: as built, orchestration holds it, and its tests pin it | `F-020`, an item for the next `core-internal` round: an `automatic_deposit` acceptance's quote is no `shown` without a firm rate (a row, or a field of `automatic-deposit-rests-on-its-arrival`). `M4` |

Informational, no severity: `carry_out_roll_back` (`segments/roll_back.rs:103`) is `pub` and ends a staging on any
`Effect` it is handed, which anyone can build; the scan holds it to an `Effect` by name only. It is the shape of the
`Modules` port (`promote`, `carry_out`), and only the assembly reaches `core-orch` (`tools/boundary-check`), which holds
the platform directly anyway. `F-020` may narrow it to the crate when its tests move inside.

### The areas the brief names, with their verdicts

- **The contract round (`T-020-39`).** Sound.
  - *The party rule for a stuck order's evidence* (`relations.rs:5494`, `linked_orders`; `:5563`, `party_to_the_order`):
    a maker is a party where a route the log gave the order, or an order a hand-on joins it to, has a segment of its
    provider; where a move or a resolution of the order went to it; where it is an arrival's reserve, a permitted decide
    step's mover or a holding withdrawal's holder; or where its party is a holding withdrawal's publisher or the
    publisher a replay or evidence of the order binds a crossed node to. The review's two should-fixes hold: no holding
    check names a party, and a refused decide step names no one and joins no orders. It reads only what the log
    admitted under its own rows. Residual R-MOVER below (a permitted decide's mover is a party whoever it is).
  - *The erasure override* (`relations.rs:212`, `overrides_citations`; `:3586`): read from the permitting decision's own
    action, by the effect's permit receipt, never from the effect's word, and, for the `erasure` record, through a
    `done` erase effect of its commitment. A permitting decision is itself held to the effect's action
    (`effect-permits-a-decided-action`), so an effect can't add the flag. The log lifts every citation under an
    override, the store only those before the decision (below).
  - *`given_on_deposit`* (`check.rs:452-468`; `types.rs`, `on_a_deposit`, `in_a_session`): no yes, a session; it rests on
    the deposit's arrival at its origin reserve, and, as `given` does, on a `shown` of its quote and on a session the
    log opened for the requester, still open. Low L7 for what the log doesn't hold.
  - *Batched payouts* (provider-role 3.1.0, router-api 4.0.0): only a deposit tuple declares `payouts`, absent reads per
    order; the candidate's half bounds the named operation (`router_api/check.rs:297`) before the record's half indexes it
    (`:1572`). A reserve that declares falsely harms only its own orders, which wait (`movement-evidences-one-segment`).
  - *Revocation at receipt* (`attribution.rs:309`): every grant in force at `made_at`, by each grant's last change at or
    before it, the later effect on a tie, revoked after `made_at` and no later than `received`; false where none was in
    force. The register's reading (`keys.rs:475`) agrees on the cases and is a port, not a call: no test runs the
    contract's function against the register over the attribution fixtures (an `F-022` gap to note, no finding).
  - *The waiting notice's shapes* (`types.rs`, `Awaiting`, `SlowSegment`, `NoticeOf::Waiting`): the requester, a
    validity and slow places, no provider, amount or account (`REQ-AUZ-7`); checked for shape alone (`check.rs:599`).
    Residual R-AWAITING.
- **Core's party check port (`stuck/party.rs`, `T-020-40`).** Sound. It is the contract's rule ported line for line,
  and held to it by a walk over every admission case that cites stuck evidence, both ways (admitted: each cited maker a
  party; refused on `party`: one not), with floors on the counts. It requires the attribution `verified`, which the
  contract leaves to other fields, so it is the stricter. A confirmation or decline on the reporter's own party is
  refused (`from_a_party`, `stuck/mod.rs:2842`). The history is the caller's word (`decide` and the seven others take
  it), which the assembly gives from the log; a forged one would only pass to the log, which refuses it.
  `digests_from_a_party` failed open on a digest the history holds no statement for; closed by `T-020-53`.
- **The received-time hold in core-audit (`T-022-09`).** Sound with two residual windows. A stated entry whose maker
  has a revocation dated after its `received_at` must have that time within the skew of the log's clock
  (`keys.rs:401-409`); later it is unavailable, and a retry must carry the new time, which attributes it to no one.
  Elsewhere `received_at` stays the writer's word, where it decides nothing. Left: the skew window (owed item, below)
  and the effect-to-result window (L5).
- **The erasure override end to end (`T-020-42`, `T-021-13`, `T-022-10`).** Sound. Policy permits an override only to
  an operator's login listed for both `EraseStatement` and `EraseOverridingCitations`, no other role and never Core's
  own actors (`roles/mod.rs:216`). Orchestration sets the flag only on the operator's call
  (`erase_statement_overriding`), looks before the decision and again after the effect, the second time refusing a
  citation made after the decision (`erasable_overriding_since`, `store.rs:726`). The store reads the override from the
  permitting decision (`store.rs:797`), keeps every other rule (kept, named by a stated entry, never an advertisement),
  and reports a citation made after the decision as `ERASED_YET_CITED` (`cited`, `store.rs:574`). The assembly's
  operator holds the override's permission in no scenario that doesn't grant it. Residual R-OVERRIDE.
- **The roll-back seam and its operator wiring (`T-021-12`, `T-020-42`).** Sound, with low L6. Policy decides
  `CatalogueChange::RollBack` under `Change(RollBack)` alone (17 decided actions; `each permission alone` over it); Core
  appends the effect on the permit before the platform is called and its result after; `carry_out_roll_back` acts only
  on a `roll_back` effect naming the version staged at that moment; the scan lists `roll_back` as carrying out
  (`entry_points.rs:84`) and its completeness check would catch a new unlisted platform method. The assembly's
  `Core::roll_back_as` is the only caller, and the operator's login holds `Change(RollBack)` there; a deployment's
  register must list it (`T-021-12`'s gap 2, configuration).
- **The origin held by policy (`T-021-11`).** Sound. `origin_in_force` keeps the first account named, and a part none
  named yet may have one named once, by the payer (`funds/mod.rs:310`, `:472`); restating the account in force still
  passes; the payer check and the account register (`Source::Account`) still guard the naming. Policy is narrower than
  `core-internal`'s rule, so the two agree on every log policy wrote (`T-021-11`'s gap 1 is a log policy never wrote).
  Orchestration never moves the origin (`hand_on/rule.rs`, `ChangesOrigin`).
- **The new public entry points, and core-authz's scan.** Sound. New: `BoundStore::erasable_overriding` and
  `erasable_overriding_since` (answers, no effect), `erase_statement_overriding`, `Segments::roll_back` and
  `carry_out_roll_back`, `Arrivals::awaiting` and `accept` (the customer's acceptance asks policy for the customer's own
  authorisation and records `given_on_deposit`), `Inbox::waiting_on`, the assembly's `roll_back_as`,
  `erase_statement_overriding_as` and `rebound_store`, and `revoked_before_receipt` in the contract (a pure reading). Each
  that carries out does so on a permit's effect; the scan's three rules pass at `2caa6f55`, its new test pins
  `roll_back`'s listing and a planted bypass. Informational note on `carry_out_roll_back` above.
- **`conformance/evidence/v0-scope`'s `REQ-CAT-5` row**, against the first pass's commit. Sound. The row moves from
  `partly | form:cat5-timing-residual` to `m3`, citing the owner's acceptance of 2026-11-18 and `SPEC.md` 0.4.2's
  accepted limit of `THR-47`, which is in `SPEC.md` (§8, `THR-47`'s row). No other line changed; one `strict:` line
  (`strict: on`, set by `T-024-10`); no row is reclassified away from `m3`. The ratchet against `develop` still starts
  once the file reaches `develop`.

### The owed items

- **`T-021-12`'s low, a refused caller tells "nothing staged" from "not permitted".** Confirmed, and wider: the version
  staged is in the refusal, and the attempt goes unrecorded. Low L6.
- **`T-020-40`'s fail-open digest** (on the form as `stuck-decision-digest-scope`, default accept). Closed by `T-020-53`, which
  supersedes the item: orchestration refuses a digest naming no logged statement. The text below is the reading before it.
  Accept for v0. In the
  assembled Core it is closed by policy: every digest of a stuck decision must be one the store keeps
  (`Source::Kept`, `Said::Yes`, else `missing_input`; `core-authz/src/stuck/mod.rs:223-230`), and the assembly answers
  `Kept` only for a commitment a stated entry of the log holds (`assembly/policy.rs:301-318`). So every digest names a
  logged statement, and orchestration then holds that statement to a verified party. The condition, for `F-030`: a
  production assembly's `Source::Kept` reads the log's stated entries as the kit's does, or orchestration fails closed.
- **`T-022-09`'s skew window.** Confirmed: within the skew (300 s by default) of the log's clock, orchestration can stamp
  a statement received a few minutes before a revocation that recent, and it is attributed. It needs orchestration to
  lie about `received_at`, which it writes for every statement; the revocation's own time is orchestration's within
  the same skew. For a compromised key alone, the owner's case, the rule holds. Accept for v0 (form item below); the
  narrowing for later is the log stamping `received_at` itself at the append.
- **`T-020-41`'s two lows.** Not security findings. `wait_ended` has no summary in the log, so a restart with no
  ledger sends it again; a receiver's inbox takes a repeat as `Repeat` (`inbox.rs`, by the entry's index). A `shown`
  and its refusal in one unread batch send `wait_ended` with no waiting notice before it (`notice/mod.rs:403-420`): the
  customer is told a wait ended that it was never told of, its own order, with no provider, amount or account. Accept
  for v0; `F-023`/`F-020` may add the summary and order the two.
- **`T-020-43`'s gap 4, a batching deposit in the middle of a route.** No security finding: it fails closed. The
  middle deposit's movement evidences one segment (`movement-evidences-one-segment`), so a second order on it waits on
  evidence and closes nothing wrongly; no value moves twice. A liveness question for the owner's scope (form item
  below).

### Residuals, after the second pass

Only those the second pass changes or adds; every other residual of the first pass's table stands as recorded there
(R-HANDLES, R-LABEL, R-PERSONAL and R-FUZZ still wait on the form; the rest as accepted or routed).

| Id | Change | Residual now | Evidence | Owner | v0 |
|---|---|---|---|---|---|
| R-BACKDATE | **Narrowed by `T-022-09`**; the owner's item `thr3-backdated-after-revocation` answered ("F-022 row") | A statement received after the revocation of every grant that would attribute it is attributed to no one, whatever its `made_at`, a genuine late result too. Left: (a) the skew window, where orchestration stamps `received_at` just before a revocation of the last 300 s; (b) the window between a revocation's effect and its `done` result (L5). Closed against a compromised provider key with an honest orchestration, save (b) | `keys.rs:401-409`, `:475-489`; `attribution.rs:309`; `a_late_statement_dated_before_a_revocation_is_not_taken`; the L5 probe | `F-022` | (a) form (`thr3-skew-window`, reserved: a security residual only the owner may accept, so no default); (b) row (low), L5, `T-022-11` |
| R-FUTURE | Unchanged in substance | The log holds no stated entry's `made_at` to its clock. Its `received_at` is now held to the clock where a later revocation of its maker exists, and nowhere else | `keys.rs:401-409` | `F-022` | accepted (low) |
| R-KEYS | Unchanged, re-confirmed | `T-022-09` adds no input to the register: grants and revocations are still taken from `done` results of effects the log admitted, and the statement's own two times | `keys.rs:323-360` | `F-022` | accepted |
| R-HOLDER | Widened slightly | A `resolve_stuck` effect's mover is a party to the order for evidence whatever the effect's result, as is a permitted decide step's (`relations.rs:5596-5611`) | `party_to_the_order`, `stuck/party.rs` | `F-020` | accepted |
| R-MOVER | New | A stuck decide step's mover (`resolution.by`) is a party to the order once policy permits the step, and policy doesn't hold the mover to the order (`core-authz/src/stuck/mod.rs`: only that the decider isn't it or its chain). So the operator's decider can make an outside provider's word evidence for a later report or confirmation. Bounded: the decider is the operator's, holding `DecideStuck`; the confirmer is another actor, not the customer, and not of the reporter's party | `relations.rs:5596-5611`, `stuck/party.rs` | `F-021` (policy may hold `by` to a holder the order involves) | accepted (`R119`, operator) |
| R-OVERRIDE | New | An operator's login holding both `EraseStatement` and `EraseOverridingCitations` erases the bytes of a statement a value-moving decision rested on; the commitment and the citing entries stay and the history checks, but the decision's basis can't be re-verified after. The log lifts every citation under an override; the store lifts those before the decision and reports one made after as `ERASED_YET_CITED`, which orchestration's look after the effect refuses first (a citation landing after that look is erased and reported, not prevented) | `relations.rs:3586`, `store.rs:574`, `:726`, `:797`; `roles/mod.rs:216` | `F-021` (the register's lists) | accepted (the owner's ruling `erasure-of-cited-evidence`, `REQ-AUD-13`) |
| R-AWAITING | New | `shown.awaiting` is checked for its shape alone: the log binds neither its requester to the arrival's holder nor its slow places to the quote. Orchestration fills it from the filing it held to the arrival, and a wrong requester would send another party a waiting notice (order, quote digest, validity, slow places; no provider, amount or account) | `check.rs:599`, `filing.rs:924-930`, `notice/mod.rs:403` | `F-020`, a `core-internal` item | accepted (low) |
| R-DIGEST | New (`T-020-40`'s item); closed by `T-020-53` | Orchestration let a stuck decision's digest that names no logged statement pass (now `NoEvidence`); policy's `Kept` closes it where the assembly answers it from the log | `stuck/mod.rs:2884`, `core-authz/src/stuck/mod.rs:223-230`, `assembly/policy.rs:301-318` | `F-020`; `F-030` for the production assembly | closed (`T-020-53`; the form item `stuck-decision-digest-scope` is superseded) |
| R-MIDDLE | New (`T-020-43`'s gap 4) | A route through a batching reserve's deposit mid-route isn't dropped; a second order on its shared movement waits | `F-020`'s notes, `T-020-43` | `F-020` | form (scope, below); fails closed |

Restated, not new: R-NODE (a private node crossed with no replay or evidence binds no publisher, so its publisher's word
is refused: `T-020-40`'s gap 1, failing closed); R-REOPEN (an override is one more decided path a forger with full access
to the log's file would have to write); `cat5-timing-residual` is now accepted for v0 (`SPEC.md` 0.4.2, `THR-47`).

### Items for `M4`, with owners

| Item | Owner | From |
|---|---|---|
| A revocation counts from its admitted effect, or the maker's statements wait while a `revoke_grant` of it has no result | `F-022`, with a `core-internal` item | L5 |
| `promote` and `roll_back` ask policy before they read the staging, one answer for nothing to do and not permitted | `F-020` with `F-021` (`M3` at the lead's call) | L6 |
| The log refuses an `automatic_deposit` acceptance of a quote shown without a firm rate | `F-020`, next `core-internal` round | L7 |
| ~~The production assembly answers `Source::Kept` from the log's stated entries, or orchestration's digest check fails closed~~ Closed: orchestration's check fails closed (`T-020-53`) | none | R-DIGEST |
| The log stamps a statement's `received_at` at the append (where the owner wants the skew window closed) | `F-022` | R-BACKDATE (a) |
| Policy holds a stuck decide step's mover to a holder the order involves | `F-021` | R-MOVER |
| `shown.awaiting`'s requester held to the arrival's holder | `F-020`, a `core-internal` item | R-AWAITING |
| `wait_ended` summarised in the log; a waiting notice sent before its end in one batch | `F-023` with `F-020` | `T-020-41`'s lows |
| A test running the contract's `revoked_before_receipt` against the register over the attribution fixtures | `F-022` | the port's drift |
| `carry_out_roll_back` narrowed to the crate | `F-020` | informational |

### For the owner's form

- **`thr3-skew-window`** (group: authority and evidence). Within the log's clock skew (300 s), orchestration can stamp
  a statement as received just before a revocation that recent, and it is attributed. Options: (a) accept for v0;
  (b) an `M4` row: the log stamps `received_at` itself at the append; (c) now: hold every stated entry's `received_at` to
  the clock (six checkpoint test worlds need rewriting); Other (in notes). **Recommendation: (a), with (b) for `M4`.** It
  needs orchestration to lie, and orchestration writes the revocation's time within the same skew. Reserved: only the owner may
  accept this residual, so the item has no default (worded as "default accept" above until `M3`'s milestone review of
  2026-11-24 found the two readings).
- **`stuck-decision-digest-scope`** (on the form already; superseded by `T-020-53`, which closed it). The reviewer's verdict: accept for v0, as the default says,
  on the condition that a production assembly answers `Source::Kept` from the log's stated entries (`F-030`).
- **`batching-middle-deposit`** (group: deposits). Is a deposit at a batching reserve in the middle of a route in the
  scope of the ruling `shared-movement-reading`? Options: (a) out of scope for v0: it fails closed, the second order
  waiting on evidence; (b) the router drops such routes too, and Core's guard refuses them (an `F-020` row); Other (in
  notes). **Recommendation: (a).** No value moves wrongly either way.

### Verdict for `M3`'s exit

No high or critical finding is open, from either pass; the second pass adds three lows and one informational note, each
routed. From this review's side `M3` may exit. What the exit still needs is `T-024-10`'s strict run at the tip (CI at
`2caa6f55` was pending when this pass started) and the owner's `DEC-20` click at the release into `main`, which this
record is the security review for.

### Defaults taken

- **The base** is `e63afbeb`, the first pass's tip on `m3`, as the brief gives it; the row says "against the commit of
  its first review" (`0e3f68db`). Between the two, `e63afbeb`'s own `DEC-20` paths are `T-024-19`'s `ci.yml` line, which
  the first pass's addendum reviewed, so nothing is left out. A reversible default.
- **The residual table** is restated only where it changes, beside the first pass's, so the first record stays as it
  was read.
- **Severity by reach**, as in the first pass: L5 and L7 need orchestration to misbehave or a narrow timing, so they are
  low and routed to `M4`.

### Gaps

- A reading at `2caa6f55`, the tests above and one probe; CI at that commit was pending. The orchestration side
  (`core-orch`, the assembly) was read for the areas named, not whole.
- The contract's 59 new fixtures were read as data, not one by one; the walk in `stuck::party` and `contracts/tests/`
  are their evidence.

## T-025-05, third pass (2026-11-28)

The third pass the row asks for ("It runs a third time…"), over what the response to `M3`'s milestone review of
2026-11-24 (PR #143) changed on `DEC-20`'s paths, by a reviewer that wrote none of it and none of the earlier passes.
Branch `f-025/t-025-05-third-pass`, cut from `m3` at `47a2b2a0` (every row before the exits merged). The diff is
`dcefd125..47a2b2a0`: `dcefd125` is the second pass's merge into `m3`. It holds wave 38 (`T-021-14`, `T-020-44`),
`T-020-45`'s round, `T-013-10`, `T-013-11`, `T-021-15` to `T-021-17`, `T-022-11`, `T-024-21`, `T-024-24`, `T-020-54`'s
scope rows and waves 44 to 46 (`T-020-59`, `T-015-04`, `T-015-05`, `T-020-60`, `T-024-25`, `T-020-61`, `T-020-62`,
`T-020-63`, `T-020-67`), with the rows between (`T-020-46` to `T-020-58`, `T-020-64` to `T-020-66`, `T-001-06`,
`T-001-07`, `T-016-05`, `T-023-04`, `T-024-22`, `T-024-23`). The milestone reviewer's checks of each row on #143's
channel were read for what each verified; this pass read the code itself and did not take a check's word.

**The paths.** `DEC-20`'s prefixes as `HOUSE-RULES.md` and `.github/CODEOWNERS` list them, now with
`components/reader-host/` and the provider system's paths the reader host reaches (`confine.rs`, `keys.rs`, `lib.rs`,
`module/sandbox.rs`, `module/mod.rs`, `Cargo.toml`), with `components/prov/src/module/` whole and
`conformance/evidence/v0-scope`, as the earlier passes took them: 935 paths, 152 of them code, schemas, docs, tests and
workflows (`HOUSE-RULES.md`, whose `DEC-20` list itself changed, among them), and 783 contract fixtures (291 new, 492 changed). Each is in `t-025-05-paths.md`, section "Third pass",
with its verdict. No `experiments/…-binding/`, `catalogue/signature.rs`, `module/engine.rs`, `module/sandbox.rs` or
`module/bin/compiler.rs` path changed.

**How far it was read.** Read whole: the reader host's `link.rs`, `keys.rs`, `host.rs` and `worker.rs`'s answer
handling; Core's adapter (`core-orch/src/closing/readers/mod.rs`'s `ask_listed`, `exchange`, `draw` and `start`, and
`tls.rs`); `contracts/src/reader/check.rs`; policy's `session/mod.rs` and `roles/mod.rs` hunks, the named test's 31
actions and the permission sweep; `core-audit/src/log/keys.rs`'s register and received-time hunks; the platform's
`open`, `read_back`, `handle_in`, `repeated`, `repeat_operation`, `recorded_answer` and `status`
(`prov/src/module/platform.rs`) and `record.rs`'s entries; the relations `repeat-is-of-an-unsettled-segment-at-its-provider`,
`reversal-is-asked-again-only-after-a-refusal` and `reliance-names-its-judges-statement`; the reversal commands
(`core-orch/src/reversal/command.rs`), `Reversals::confirm_reversal`, `accept_fault` and `end`; `stuck/reopen.rs`'s
rule on moves in motion; `segments/judge.rs`'s `Began`; the boundary check's `parse_manifest`,
`test_only_features_enabled` and `lines_beyond_admitted`; the contract guard's ignorable rule and fold rule; every
workflow hunk. Read by purpose and spot checks: the reader host's `egress.rs`, `jail.rs` and `config.rs`, which the
milestone reviewer and a separate security reader each read twice (`T-015-04`, `T-015-05`); the proxy's nested-statement
signing (`proxy/signing.rs`); `core_sources`; the licence check; the contract schemas and changelogs. Fixtures: grouped,
searched for key material (none: the `secrets` fields are a declaration's secret names, no values), and taken as
`contracts/tests/` reads them; most changed admission fixtures gain `T-020-52`'s `refile_arrival` decision in their
shared history.

### Named tests and probes, at `47a2b2a0`

Run through the lock on the reference host (arm64 macOS), each under a 1500 s alarm, after `touch contracts/src/lib.rs`.

| Test | Crate, binary | Result |
|---|---|---|
| `every value moving action is refused when policy denies` [`REQ-AUZ-1`] (31 decided actions) | `rutent-core-authz --lib` | 1 passed |
| `rewrite with full operator access is detected by the checker` [`REQ-AUD-9`] | `rutent-core-audit --test checkpoints` | 1 passed |
| `statement signed by a key not current at its making is refused before the record` [`THR-3`] | `rutent-core-orch --test snapshot_statements` | 1 passed |

Probes, written for this pass and removed:

- **The test-only feature pin, by name** (the routed question): `test_only_features_enabled` and `parse_manifest` over
  five manifests that turn `reader-test-hooks` on. A target table, a `package =` rename and a `workspace = true`
  dependency are each refused by `parse_manifest`, which `load` runs over every member. A dotted
  `[dependencies.rutent-core-orch]` table is found by the pin. A key spelled `rutent_core_orch` is missed by both, and
  `cargo metadata` refuses it ("no matching package named `rutent_contracts`", tried on a scratch crate), so it can't
  build. The pin holds.
- **A balance in the log** (M2): no new probe was needed. `components/core-audit/tests/compare.rs:1238-1244` already
  asserts a raise of `{"kind": "holding", "direction": "over", "by": "42", …}` for an account read at 42 with nothing
  held there.

A real attack was tried on each area the brief names:

- **A forged or replayed reader answer.** Refused. Core signs nothing it reads, verifies the answer's signature
  against the listing's statement key over the bytes it received (`closing/readers/mod.rs:556`), and holds it to the
  question by `check_reader_answer`: maker, key and version as listed, deployment and protocol version in force at
  Core's clock, the request id, network, question and movement or transaction, and the deadline on Core's clock. Each
  request id is 16 bytes of `ring`'s secure random, top half non-zero, never a counter, and a scan test pins that the
  crate builds none otherwise (`core_makes_a_request_id_only_from_its_random_source`). An old answer replayed names
  another request, and one copied to another network or movement breaks the binding.
- **A hostile reader host.** Within `DEC-28`'s trust. The host holds no log handle and makes no decision. It signs
  with each reader's key, which it resolves at signing from the key store, so a compromised host can sign any reading
  for the readers it runs. Core then takes it as that reader's word, bound to the request Core drew. Residual
  R-READER-HOST. A hostile reader in a sound host can sign only an answer to the question it was asked, under its own
  configured version, rules and trust (`worker.rs:688-720`). Its content stays the operator's to trust (R-READER-CONTENT).
  The host's server certificate is no client to it, and Core's client certificate is no server to Core (each verifier
  holds the web PKI's key usage, and Core's names the listed subject name).
- **A backdated statement past a cutover.** Refused. The log judges an advertisement at its received time, which it
  holds to its own clock, never at the maker's `made_at` (`keys.rs:614-626`). A result is judged by the versions in
  force when its segment began, which is the `made_at` Core stamped on its own operation (`segments/judge.rs`,
  `Began`), so a provider dating its result back gains nothing. A reader's answer is judged at Core's clock when it
  arrives. The platform judges an assertion at its admission time (`admission.rs:244`). A revocation now counts from its
  admitted effect (`T-022-11`), which closes the second pass's L5.
- **A decided resolution that moves value twice across a restart.** Refused, twice over. A `resolve_stuck` effect with
  no final result is never acted on again after a reopen (`stuck/reopen.rs`, "the moves in motion"), and its
  transaction is the decide step's own. Beneath that, the platform's record (`T-013-10`) is written before each
  hand-over and read back on opening. So a second hand-over under the same transaction is refused as
  `DuplicateTransaction`, unless the provider's accepted advertisement declares a safe retry
  (`platform.rs:3293-3330`), and an `Ended` transaction never runs again. A reversal is asked again only where every
  earlier ask failed, which the log holds (`reversal-is-asked-again-only-after-a-refusal`, four fixtures). Each test
  restarts a real platform and a real Core: `a_repeat_after_the_platform_restarts_moves_nothing_twice` and
  `restart_after_the_provider_moved_and_before_its_answer_was_logged_moves_nothing_twice`.
- **A policy bypass through a new entry point.** No value moves without a decision. Every new value-moving or
  carrying-out action is decided: `repeat_operation`, `read_recorded_answer`, `read_node_record`,
  `read_holding_balance`, `ask_status` and the settings. The entry-point scan lists the new carriers (`entry_points.rs`).
  Two lows where a consequence that moves no value goes undecided or unbound: an operator's fault acceptance (L8), and
  `end_requested` or a dispute's declaration, which no relation binds to its permit (L10).
- **A boundary-check bypass.** None found for the reader host. The path scan finds `rutent_prov` anywhere in the code,
  comments and literals blanked, so a raw identifier or a fully qualified path is caught. The facade's re-exports are
  pinned. One gap was found and fixed: the contracts' `rule-switches` feature, which turns a shape rule off on a thread,
  was pinned to tests by nothing (L11).

### Findings

None high or critical. Numbered on from the first two passes (M1, L1 to L7). L11 is fixed here, and the rest are routed.

| # | Severity | Area | Where | Scenario | Owner, item |
|---|---|---|---|---|---|
| M2 | Medium | `REQ-AUD-6`: a holding difference's figure | `contracts/core-internal/schemas/raise.schema.json:376-418` (`by`, `:403`); `contracts/src/core_internal/types.rs:2128-2139`; `components/core-audit/src/compare/holding.rs:73-86`; the scan's exclusion `conformance/scenarios/src/secrets.rs:629-630`, `:659-668`, `:692` | A `holding` raise in the log carries `by`, the amount the account is over or short. Where Core holds nothing at the account, `T-020-63` makes that routine: `by` is the account's whole balance (`compare.rs:1238-1244`, balance 42 raised `by: 42`). Elsewhere, balance = held + `by`, and the held amounts are the log's own. So anyone who reads the trail reads the balance, a private node's publisher's included. That is what `REQ-AUD-6` keeps out of the log ("never in the log or a record"), and what the log's own holding check leaves out by design (`verified-entry.schema.json:1181`). The secrets scan nulls `by` before it looks, so the scan can't see it | `T-020-68` (#358, plan `a33e090c`, in `m3` at `f616967d`, wave 47), item (1): `difference_holding` carries `direction`, `asset` and `word` only, folded into unreleased `core-internal` 1.0.0; `judge` stops forming `by`; the scan drops `without_the_difference`, and the tests that assert `by` assert the direction. Fixed in `M3`, before the release into `main` (the owner's pre-production rule); `T-024-10` depends on it |
| L8 | Low | An operator's fault acceptance (`REQ-ORD-11`, `REQ-AUZ-6`) | `components/core-orch/src/reversal/command.rs:147-200` (no decision for the operator); `components/core-orch/src/reversal/mod.rs:1796-1803`, `:1856`; `components/core-authz/src/roles/mod.rs:452-457` | The customer's acceptance of a showing of fault is policy's decision. The operator's is a command, held only to the register's binding of the login to the operator's party and role. So any login of the operator, even one listed for nothing, can accept a fault for the operator, and the operator then carries the reversal. An operator's dispute of the same showing needs `Permission::DisputeFault`. Bounded: it harms only the operator's own party, and the command is recorded | `T-020-68` (#358, plan `a33e090c`): a permission of the operator's own for a fault acceptance, decided as `dispute_fault` is. `M3`, wave 47 |
| L9 | Low | `REQ-CPL-12`'s reserve half: `Segments::rests_on` | `components/core-orch/src/segments/mod.rs:869-880`; callers only `conformance/scenarios/tests/reliance.rs:81` and unit tests | The by-hand declaration that an input rests on a judgement has no production caller. Core's own path is `Segments::declare_cited` (`segments/mod.rs:890-941`, called by `schedule/run.rs:555`). It declares reliance on whatever stated judgement an operation's eligibility figure cites, a reserve's included, and `F-020`'s R14 rule-off holds it. So no v0 behaviour rests on the by-hand port, but its tests are some of `REQ-CPL-12`'s evidence | `T-020-68` (#358, plan `a33e090c`): narrow `rests_on` to the crate's tests or name it a seam, and `REQ-CPL-12`'s evidence names the `declare_cited` tests. `M3`, wave 47 |
| L10 | Low | `end_order` and `dispute_fault` bound to their permits (`REQ-ORD-12`, `REQ-ORD-11`, `REQ-AUZ-1`'s spirit) | `contracts/core-internal/CHANGELOG.md:44-52` ("No relation of their own"); `components/core-orch/src/reversal/mod.rs:1986` (`Reversals::end`), `:1938` (`Reversals::dispute`), both `pub` and lent to the deployment through `Ports::with_reversal_desk` (`schedule/jobs.rs:247`) | Policy decides both, but the log admits an `end_requested` input, or a dispute's stuck declaration, with no permitted decision behind it. A caller holding the lent `Reversals` reaches `end` and `dispute` without asking policy, and a checker can't tell such an input from a decided one. Bounded: an end moves nothing (only before a part's first segment moves value), and a dispute only declares the part stuck, whose resolution is decided in two steps. The stale module doc that said `end` has no decided action is fixed here | `T-020-68` (#358, plan `a33e090c`): `end_requested`, and a dispute's declaration, cite their permitting decision in a `core-internal` item, and both methods are narrowed to the crate. `M3`, wave 47 |
| L11 | Low, **fixed** | The contracts' rule switches (`rule-switches`, `without_rule`) | `contracts/Cargo.toml:18`; `contracts/src/rule_switch.rs`; `tools/boundary-check/src/lib.rs:261-270` | `without_rule` turns a shape rule's refusal off on the calling thread, the log's checks of an entry among them (`core_internal/check.rs:30-34`). The feature is built only by the contracts' own tests today, but nothing held it there. `reader-test-hooks` is held by `TEST_ONLY_FEATURES`. `jail-layers` is held otherwise: by `required-features` on its test targets (`components/prov/Cargo.toml:22`, `:29`) and by the jail probes' scan of the resolved feature graph (`components/prov/tests/jail_layers.rs`), not by `TEST_ONLY_FEATURES`. Turned on in Core's build, it would let orchestration append through `without_rule` and take an entry past a shape rule the log holds it to | Fixed in this branch: `TEST_ONLY_FEATURES` holds `rule-switches` to dev-dependencies, with the test `contracts_rule_switches_are_enabled_only_from_dev_dependencies` [`DEC-20`, `REQ-MOD-3`], seen red before the fix (a normal dependency turning it on went unreported) |

Informational, no severity:

- **I4.** An operator's cutover counts from its admitted effect (`keys.rs:438-460`). A later cutover date keeps an older
  protocol version in force from the effect on, before its result. It is the operator's permitted setting, and a
  `failed` result undoes it.
- **I5.** `Platform::status` builds the status question from the operation asked about by swapping its statement and
  keeping its signature (`platform.rs:3089-3100`). The signature then signs other content, so a module that verifies
  the operator's signature refuses the question. Its doc says a caller that signs its own questions hands them through
  `Platform::handle`, which refuses every status query. The question moves nothing, and a refused one falls back to the
  repeat or the recorded answer. `F-013`: let Core sign the question, or fix the doc.

### Rulings on the questions routed to this pass

1. **`REQ-AUD-6`: a holding raise's `by` equal to the balance** (#347's review). A raise may not carry the figure. It
   carries a flag: the direction, with the asset and whose word the balance was. A bucket is no answer either: where
   nothing is held, a bucket bounds the balance. The operator who must act on a difference reads the amount through
   the decided `read_holding_balance`, as evidence kept outside the log, which `REQ-AUD-6` allows. Finding M2.
2. **`without_the_difference` hiding a balance from the secrets scan** (the check of `T-020-63` at `be306133`). Not
   accepted as a standing exclusion: it exempts the one field that equals a balance where nothing is held. It goes
   with M2's fix, and the scan then reads every raise whole. Until then it stands only as M2's, on the record.
3. **The log alone can't show that a `repeat_operation` was declared safe** (#347, round 2). Accepted for v0 as
   residual R-REPEAT. The guard needs nothing from Core: the platform reads `safe_retry` from the catalogue's accepted
   advertisement (`admission.rs:186`, `platform.rs:2864`, `:2910`) and refuses `DuplicateTransaction` otherwise
   (`platform.rs:3301-3303`), and the log admits a repeat only of an unsettled segment at its provider under its
   question's transaction. A checker with the evidence store can read the declaration from the advertisement's kept
   bytes, which the log holds by commitment. Were an outside checker that holds only the log named (`F-033`), the
   effect should cite the advertisement's commitment (`F-022`).
4. **`confirm_undone`'s confirmation question is not a decided action** (from `T-020-61`). It needs no decision.
   Like an arrival or a balance question, it moves nothing. The log binds it to the order and segment before it is sent
   (`append_question`, `reversal/mod.rs:1559`), and its answer counts only as the origin reserve's attributed word.
   What follows that moves value is decided on its own. The entry point is unauthenticated, but asking is bounded by the
   cadence and `MAX_REVERSAL_TRIES` (`reversal/mod.rs:1500-1508`), and a caller can't make the answer. Residual
   R-CONFIRM, accepted.
5. **`REQ-CPL-8`: a reserve's judgement, which no operation's figure carries** (from `T-020-61`, round 2). v0 needs no
   further caller. `REQ-CPL-8` is met by the reserve's own signed answer to its own segment: attributed to it, and
   counting for nothing when presented in another's name (`conformance/scenarios/tests/person_records.rs:33`). It runs on
   Core's production path. Reliance on a reserve's judgement, where an eligibility figure cites one, is declared by
   `declare_cited`. `Segments::rests_on` is a by-hand port with test callers only (L9). `T-024-10` keeps `REQ-CPL-8` as
   `m3` on `person_records`.
6. **boundary-check's `reader-test-hooks` pin looks a dependency up by its own name only** (#356, round 3, a Low).
   Sound, and accepted. `parse_manifest`, which `load` runs over every member, refuses every other way a manifest can
   name a dependency: a rename, a workspace inheritance, a target table, a nested dependency table. Cargo refuses a key
   that isn't the package's name (probe above). The pin now holds `rule-switches` too (L11). Moving both pins to the
   resolved feature graph (`cargo metadata`, as `jail-layers`' check does) would need no list of spellings at all
   (`F-024`, when the tools are next touched).

### Why neither earlier pass found the milestone review's finding 1

Finding 1 of the review of 2026-11-24 was High. No fixed rule decided who may grant or revoke a read permission. A
grant was checked for its shape and for `ActsFor`, which bound an actor's party and kind but not its role. So a
customer's login stating the operator's role granted itself trail read as a regulator. Neither pass found it, for
three reasons, each a method gap:

1. **Depth went by value.** Both passes read whole what moves or frees value: `decide`, the effect rows, the stuck
   rules and `held`. Policy's other calls were read by purpose and spot checks, and the first pass's "how far" names
   "the session, roles, parties" among them. A read grant moves no value, so its rule sat in the shallow tier, though it
   decides who reads the whole trail (`REQ-AUZ-7`, `REQ-AUD-12`).
2. **The passes took the tests' coverage as the surface.** The named test, the permission sweep
   (`each_permission_alone_allows_only_its_own_action`) and the entry-point scan all enumerate decided actions: what a
   `DecisionRequest` carries, and the state-changing methods of `core-orch` and the platform. `grant_permission` and
   `revoke_permission` are methods of policy's `Reads` trait, not actions of `decide`, so none of the three reached them.
   Both passes cited the sweep as evidence for the rules it didn't sweep.
3. **A binding was read as its name.** The passes read `ActsFor` as the register's binding of an actor, which they took
   to cover every field the request states. It bound the party and the kind, as its doc said, and a stated role was
   taken on its word.

This pass changes the method on each count. It listed policy's whole call surface from `core-internal`'s traits
(`decide`, `authorise`, `open_session`, `check_read`, `grant_permission`, `revoke_permission`, `charge`, the four stuck
calls, `decline_outcome` and `ask_about_person`), not only what `decide` carries, and checked that each has a fixed
rule and that each actor it names is bound in all four fields (`acts_for`, `roles/mod.rs:676-684`, with `role` since
`T-021-14`). It found no other call left out. The sweep's match over `Permission` is exhaustive since `T-021-14`
(`roles/tests.rs:900-920`), so a new permission breaks the build until it is swept. A reviewer of a later pass should
start from the trait list, not the tests' lists.

### Residuals, after the third pass

New, or changed by this pass's diff. Every other residual of the two earlier tables stands as recorded there.

| Id | Change | Residual now | Evidence | Owner | v0 |
|---|---|---|---|---|---|
| R-BACKDATE | (b) closed by `T-022-11` | The effect-to-result window (L5) is closed: a revocation counts from its admitted effect until a `failed` result undoes it. (a), the skew window, is the owner's item `thr3-skew-window`, answered on the decision form on 2026-11-25 at 23:08 UTC: "Accept for v0, fix in M4 (recommended)" (the log stamps `received_at` itself at the append) | `keys.rs:330-352`, `attribution.rs` | `F-022`; (a) `M4` | (a) accepted for v0 (the owner, 2026-11-25), fixed in `M4` |
| R-HANDLES | Re-confirmed | The reader host is a second process, but it holds no log handle and makes no decision. It signs statements Core verifies, so no decision entry crosses a process boundary. The residual stands as the first pass recorded it | `calls.rs`, `reader-host/src/host.rs` | `F-021` with `F-022` | form (as before) |
| R-REPEAT | New (routed question 3) | The log alone can't show that a repeated operation's provider declares a safe retry. The platform holds it from the catalogue, and the evidence store's kept advertisement shows it | `platform.rs:3301`, `relations.rs:2583-2620` | `F-022` | accepted |
| R-CONFIRM | New (routed question 4) | A reversal's confirmation question is asked on an undecided, unauthenticated trigger. It moves nothing, and it is bounded by the cadence and a cap | `reversal/command.rs:316`, `reversal/mod.rs:1500-1563` | `F-020` | accepted |
| R-READER-HOST | New (`DEC-28`) | The reader host holds each reader's signing key, resolved per answer. A compromised host signs any reading for its readers, and Core takes it as that reader's word for the request Core drew. Bounded by the host's isolation as a task of its own, the key store's token scoped to the readers' keys, and Core's checks of key, request, deadline and listing | `reader-host/src/keys.rs`, `host.rs:150-180`, `closing/readers/mod.rs:511-575` | `F-030` (the deployment's isolation and the token's scope) | accepted (`DEC-28`) |
| R-READER-CONTENT | New (`DEC-28`, the milestone reviewer's Low on `T-015-04`) | A reader's content is the operator's to trust. An RPC pool's quorum binds which endpoints agreed on what the host fetched, not what the reader says they said | `reader-host/src/lib.rs` part 5, `worker.rs:650-678` | `F-015` | accepted (`DEC-28`) |
| R-SLOT | New (`T-015-05`'s Low) | Core's reserved handshake slot is keyed by source address, so a peer sharing Core's address (NAT, a proxy) can take it for the handshake window | `reader-host/src/link.rs:9-17`, `CoreSlots` | `F-030` (the deployment notes) | accepted |

Closed since the second pass, each by its row, verified in the code here: M1 (the platform's record survives a restart,
`T-013-10`); L5 (`T-022-11`); L6 and the informational note on `carry_out_roll_back` (`T-020-49`, the scan's rule 4);
L7 and R-AWAITING (`T-020-45`); R-MOVER (`T-021-16`).

### Verdict for `M3`'s exit

No high or critical finding is open, from any of the three passes. The third pass adds one medium (M2), four lows (L8
to L11, L11 fixed here) and two informational notes, each routed with an owner. From this review's side, `M3` may exit.
M2, L8, L9 and L10 are `T-020-68`'s (#358, plan `a33e090c`, in `m3` at `f616967d`, wave 47), built in `M3` before the
release into `main` by the owner's pre-production rule; `T-024-10` depends on it, and this pass confirms its fix as a
delta once it lands (`T-020-68`'s Done-when). What the exit still needs is `T-024-10`'s third run at the tip, and the owner's `DEC-20` click
at the release into `main`, which this record is the security review for.

### Defaults taken

- **The base** is `dcefd125`, the second pass's merge into `m3`, as the brief gives it. The row says "against the commit
  of its second pass". `dcefd125` holds the second pass's record, which changed no `DEC-20` path, so nothing is left out.
- **The path set** is the earlier passes' prefixes plus the reader host and the prov paths it reaches. `HOUSE-RULES.md`
  and `CODEOWNERS` added these in this diff.
- **M2 rated medium, not high:** a `MUST` of `REQ-AUD-6` broken in the log, readable by those who read the trail, but
  moving no value and taking no control.
- **L11 fixed here** because it is small and sits in a `DEC-20` path this row may change. M2 and L8 to L10 change
  contracts or policy's tables, so they are routed: since 2026-11-28 all four are `T-020-68`'s (#358, plan `a33e090c`).

### Gaps

- A reading at `47a2b2a0`, the three named tests, two probes and L11's test. The orchestration side outside the areas
  named (`core-orch`'s 38 thousand added lines, the assembly) was read for those areas, not whole. Its rows each had a
  fresh reviewer and the milestone reviewer's check.
- The Linux-only paths (the reader host's jail probes, `stm-vos.yml`) were read, not run here. Their CI runs on this
  pull request and on `m3` are the evidence.
- The 783 fixtures were read as data, grouped, not one by one.
