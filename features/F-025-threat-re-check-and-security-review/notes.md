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
nothing here has run on Linux. The probes' first run, the run id and the rule-offs are CI's** (`jail-probes.yml`; the
main `ci.yml` runs the same file under `cargo test --workspace`). Row 18; touches `components/prov/src/module/` and
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
  Cargo's resolved features: `cargo tree -e normal,build,features -p rutent-prov` (the release command line) must show no
  `jail-layers`, while the same with `dev` must show `feature "jail-layers"` (the control, which shows the check can fail);
  and `cargo metadata` (which parses the manifest, a multi-line array included) must show no feature but `jail-layers`
  itself enabling it, `default` included.
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
