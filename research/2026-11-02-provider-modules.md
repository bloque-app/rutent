---
name: Provider modules, covering sandboxing, signing, state and outbound access
status: informative
date: 2026-11-02
---

# Provider modules: sandboxing, signing, state and outbound access

## Basis

**Read on 2026-11-02, inside this project:** `CLAUDE.md`, `PRODUCT.md` (`PS-14` to `PS-21`), `SPEC.md` (`REQ-MDL-*`, the threat areas, `OQ-13` to `OQ-16`), and `research/2026-11-02-durable-state.md`. This report does not repeat that one. It points to it where the two meet.

**Read on 2026-11-02, outside this project:** only public external sources, listed under References with the version or date each one shows. Nothing from the directories the brief excludes was opened, and no production system or admin tool was queried. The VOS project is covered elsewhere and is not researched here.

**How sources were read.**
- Web pages went through a fetch tool that returns a model-written digest of the page, so a digest can misread its page.
- Two PDFs were read directly as text extracted with `pdftotext`: the Firecracker NSDI 2020 paper and the gVisor HotCloud 2019 paper.
- Advisory lists were read from GitHub's published security-advisory API with `gh api`, on 2026-11-02. Per-year and per-area counts are my own tally of what that API returned, not a figure any vendor publishes. A repository that returns nothing may simply not publish advisories there.
- A claim marked *(search excerpt)* comes only from a search result's excerpt, and its page was not opened. Treat those as leads.
- A claim marked *(title only)* rests on an advisory's title in the list, and the advisory itself was not opened.
- Everything from external sources is paraphrased except one short quotation, from the kernel documentation in section 1.

**Tags.**
- **Source:** the cited document says this.
- **Measured:** the cited document reports this as a measurement, under the conditions given.
- **Inference:** my reasoning from sources. It is not stated by any of them.

This report decides nothing. It names no winner and writes no requirement. Money-rail vendors are described generically.

**Orchestrator's review, 2026-11-02.** Checked at the source: Wasmtime's yearly and Critical counts (an independent pull of the advisory API matched exactly), the default-settings wording of GHSA-897f-g5gm-fmxm and GHSA-mghf-wjfp-p5xm, Wasmer's GHSA-v2h8-j899-r676 (Critical, no CVE, no patched version listed), and cosign's GHSA-fx35-mq7g-6g98. One nuance this report omits: GHSA-897f-g5gm-fmxm applies to Wasmtime before v43, so publication date is not exposure date. Everything else rests on this report's own tags.

## Findings that matter most

1. **Running a sandbox makes the operator responsible for patching it quickly, and one layer has not been enough.**
   - Wasmtime published 62 advisories in 2026 to date, against 4 in all of 2025. Much of that rise follows LLM-assisted bug search, and other runtimes may not have had the same scrutiny, so this is not a ranking of runtimes. Five are Critical. Two published on 2026-10-19 allow a sandbox escape, and neither needs a non-default setting on the affected platform (one covers any platform, one aarch64).
   - Wasmer published a Critical escape on 2026-11-02 with no patched release yet.
   - The operators with published designs do not rely on the language boundary alone. They add a process boundary, operating-system restrictions and placement by trust level.
   - This points to two things the spec authors may want to state for `REQ-MDL-*`: a bound on how fast the runtime can be replaced, and a limit on what one escape reaches. (Source and Inference, sections 1 and 2.)
2. **A valid signature proves who or what published a module, not that the module is honest.**
   - Malicious releases have gone out through a project's own legitimate publishing workflow, with genuine short-lived credentials. Others came from a person holding real maintainer rights. Signature verifiers have also had bypass bugs this year.
   - What limits the damage from a signed malicious update is what happens after verification: delay, staged rollout, rollback, the capabilities a module can reach, and retained artifacts. (Source and Inference, section 3.)
3. **Whether a module ever holds a credential is the largest open choice.**
   - Some platforms hand module code the plaintext secret. Others have the host make the call or inject the credential, so module code never sees it.
   - Host allow-lists help but have been bypassed several times, and they cannot see what a module writes in a request to an allowed host.
   - "Properly compiled" (`PS-20`) has a similar hidden choice. Native code compiled by a publisher is trusted input to at least one runtime, so who compiles to native code changes the threat model. (Source and Inference, sections 1 and 5.)

## 1. Sandboxing untrusted, multi-language code

### Comparison

Costs here come from different papers, years and hardware, and they are not comparable with each other. Advisory counts are not comparable either: projects differ in what they publish, where they publish it, and how hard their code has been searched. A short record does not mean safer. Vendor claims without stated conditions are in the bullets below, not in this table.

| Approach | Isolation boundary | Multi-language today | Limits it can enforce | Determinism | Measured start cost | Advisory record seen |
|---|---|---|---|---|---|---|
| WebAssembly, Wasmtime 50 | Compiler and runtime, in the host process | Any language with a Wasm toolchain; support varies | Memory and tables, CPU by fuel or epochs; network only if the host grants it | Fuel is deterministic; full determinism needs non-default settings | 5 µs instantiate, vendor, 2022 (below) | 62 in 2026; 5 Critical |
| WebAssembly, Wasmer | Same | Same, plus WASIX | Not read | Not read | Not read | Critical escape 2026-11-02, unpatched |
| WebAssembly, wazero | Go runtime in the host process | Core Wasm 1.0 and 2.0, WASI preview 1 | Not stated on pages read | Not read | Not read | None returned by the API |
| WebAssembly, WasmEdge | Runtime, plus native plug-ins that extend the host | Rust in most plug-in guests listed | Not read | Not read | Not read | 1 returned by the API |
| V8 isolate | JavaScript engine, in a shared process | JavaScript and Wasm; no native binaries | CPU, memory, startup time, subrequests (Workers) | None claimed (Inference) | No measured figure with conditions found | Chrome exploit data, below |
| gVisor | Application kernel in user space, plus host filters | Linux applications, within its compatibility list | By host cgroups; network policy at container level | None claimed (Inference) | 1.117 s lifecycle on KVM, 2019 | 1 CVE seen, by aggregator only |
| Firecracker | Hardware virtualisation, small monitor, jailer | Unmodified Linux binaries | CPU and memory by the VM's configuration and cgroups; disk and network rate limiters | None claimed (Inference) | Under 125 ms to application code, 2020 | 2 returned by the API, both 2026 |
| seccomp, namespaces, containers | Kernel, reduced | Any Linux program | cgroups | None | Not applicable | runc container escapes, 2024 and 2025 |
| eBPF | Kernel verifier | Not a general language runtime | Verifier bounds | Not applicable | Not applicable | Recurring verifier privilege escalations |

### WebAssembly and Wasmtime

**Boundary.**
- Source: guest memory access is bounds-checked, the call stack is out of the guest's reach, control transfers are type-checked, and everything external goes through explicit imports. Defence in depth includes a 2 GB guard region before linear memory and zeroing memory after use ([security docs](https://docs.wasmtime.dev/security.html)).
- Source: Wasmtime's authors say Rust does not prevent logic errors that cause miscompilation, which can lead to escapes; fuzzing gives statistical confidence, not proof; verification is partial ([2022-09-13](https://bytecodealliance.org/articles/security-and-correctness-in-wasmtime)).
- Source: Wasm 3.0's deterministic profile fixes NaN results and relaxed vector instructions, while `memory.grow` and `table.grow` stay non-deterministic ([spec appendix](https://webassembly.github.io/spec/core/appendix/profiles.html)).

**Support tiers matter.** Source: only three x86-64 targets are Tier 1. `aarch64-unknown-linux-gnu` is Tier 2, described as almost production ready, and several Tier 2 targets lack continuous fuzzing. The component model and the WASI interfaces for HTTP and sockets are Tier 1; `wasi-keyvalue` and `wasi-tls` are Tier 3 ([tiers](https://docs.wasmtime.dev/stability-tiers.html)). Inference: three of this year's five Critical advisories name aarch64 or have a proof of concept there, so the target platform is part of the security decision.

**Multi-language, honestly.**
- Source: the component model guide lists C/C++, C#, Go, JavaScript, Python, Rust and MoonBit, and says support varies by language ([language support](https://component-model.bytecodealliance.org/language-support.html)).
- Source: WASI 0.3 was released on 2026-06-11. Wasmtime 46 and later runs it by default; most other toolchains still target 0.2 *(search excerpt)* ([Bytecode Alliance](https://bytecodealliance.org/articles/WASI-0.3)).
- Source, weak: `componentize-py` output is large, and stripping can halve it *(search excerpt)* ([Golem guide](https://learn.golem.cloud/python-language-guide/building-components)).
- Inference: dynamic languages reach Wasm by shipping their interpreter inside the module, so size, memory and start cost differ sharply from Rust or C. "Wide range of languages" (`PS-20`) and a tight per-module limit pull against each other. Shopify's 256 kB binary limit shows the same tension (section 2).

**Limits.**
- Source: `ResourceLimiter` governs linear memory, tables and object counts, but not Wasmtime's own structures or embedder allocations, and not CPU ([docs](https://docs.rs/wasmtime/latest/wasmtime/trait.ResourceLimiter.html)).
- Source: CPU is limited by fuel, which is deterministic, or by epochs, which are not. Epoch checks cost about 10% ([interrupting](https://docs.wasmtime.dev/examples-interrupting-wasm.html)) and are, in some measurements, up to 2-3x faster than fuel ([Config, 50.0.1](https://docs.rs/wasmtime/latest/wasmtime/struct.Config.html)).
- Source: NaN canonicalisation and deterministic relaxed SIMD are off by default (same page). Inference: replay across machines, which `research/2026-11-02-durable-state.md` raises for recomputing decisions, would need these settings chosen on purpose.
- Source, *(title only)*: limits have had their own bugs in 2026, including fuel bypassed through a WASI call (GHSA-j366-h8gg-77pm), fuel amplification through `call_ref` (GHSA-m63x-6p34-q65x), and handle tables charged to no limit (GHSA-q554-3cjc-rx7p) ([list](https://github.com/bytecodealliance/wasmtime/security/advisories)).

**Costs.**
- Measured, vendor: instantiating a multi-megabyte SpiderMonkey module fell from about 2 ms to 5 µs, using copy-on-write heap mapping and lazy initialisation ([2022-09-06](https://bytecodealliance.org/articles/wasmtime-10-performance)). This excludes compilation and any guest start-up code.
- Source, weak: a maintainer's remark puts a host-function call at single-digit nanoseconds *(search excerpt, not a controlled benchmark)* ([Zulip](https://bytecodealliance.github.io/zulip-archive/stream/217126-wasmtime/topic/.E2.9C.94.20Host.20call.20performance.html)).
- Source, weak: SPEC CPU ran 45% slower in Firefox and 55% slower in Chrome than native, in browsers, in 2019 *(search excerpt)* ([Jangda et al.](https://usenix.org/conference/atc19/presentation/jangda)). Faasm reported about 5 ms to initialise a Wasm function against about 2 s for Docker, for a no-op, in 2020 *(search excerpt)* ([Shillaker and Pietzuch](https://arxiv.org/pdf/2002.09344)).
- Inference: a provider call waits on an external network, so per-call sandbox cost is unlikely to set rutent's latency. I found no measurement of that case.

**Escape record.** Tally from the advisory API on 2026-11-02:

| Year | 2021 | 2022 | 2023 | 2024 | 2025 | 2026 to date |
|---|---|---|---|---|---|---|
| Wasmtime advisories | 4 | 8 | 4 | 4 | 4 | 62 |
| of which Critical | 1 | 0 | 1 | 0 | 0 | 5 |

The five Critical advisories of 2026:

| Advisory | Published | What it allows | Applies to default settings? |
|---|---|---|---|
| [GHSA-xx5w-cvp6-jv83](https://osv.dev/vulnerability/CVE-2026-34987) | 2026-04-09 | Memory access outside the sandbox | No: the Winch compiler is not the default *(search excerpt)* |
| [GHSA-jhxm-h53p-jm7w](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-jhxm-h53p-jm7w) | 2026-04-09 | Arbitrary host read and write on aarch64 | No: needs Spectre mitigations or signal-based traps off |
| [GHSA-32h6-97mm-8q3c](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-32h6-97mm-8q3c) | 2026-10-02 | Guest writes about 16 KB to the native stack | Not stated; needs `component-model-async` |
| [GHSA-897f-g5gm-fmxm](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-897f-g5gm-fmxm) | 2026-10-19 | Escape through miscompilation, any platform | **Yes** |
| [GHSA-mghf-wjfp-p5xm](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-mghf-wjfp-p5xm) | 2026-10-19 | Jump to arbitrary code on aarch64; affects versions back to 2020 | **Yes**, on aarch64 |

- Source: 11 of the 12 advisories of 2026-04-09 were found with LLM tools in a three-week sprint, with diminishing returns after the first week. It was then the largest set ever published at once ([Bytecode Alliance](https://bytecodealliance.org/articles/wasmtime-security-advisories)).
- Inference: the jump in numbers reflects a change in how bugs are found, at least in part. It does not show the code got worse. It also says nothing about the code's safety in earlier years, when few looked this way. Other runtimes may simply not have been through the same search yet.
- Inference from titles, with my own coarse sorting: of the 62, about 9 concern code generation, 22 concern the WASI host interfaces, 18 concern the component model, and 13 other areas. 19 titles mention a panic. So most advisories are in the host side of capabilities, not in the compiler, although four of the five Criticals are in compilers. Every capability granted is also attack surface, and a guest-triggered host panic matters if several providers share a process.
- Source: Wasmtime releases monthly. Normal releases are supported for 2 months and every 12th is supported for 24 months, with security fixes backported to all supported releases ([release policy](https://docs.wasmtime.dev/stability-release.html)).

**Pre-compiled modules.** Source: `Module::deserialize` is unsafe because invalid input could break memory safety, and the docs say not to expose it to arbitrary user input ([docs, 51.0.0-dev](https://docs.wasmtime.dev/api/wasmtime/struct.Module.html)). Source: Wasmer's advisory says updating the compiler does not fix existing artifacts, which must be recompiled ([GHSA-v2h8-j899-r676](https://github.com/wasmerio/wasmer/security/advisories/GHSA-v2h8-j899-r676)). Inference: accepting a publisher's native code is close to running their native code. A compiler fix may also mean recompiling the whole catalogue.

**Bugs inside the sandbox.** Source: memory bugs in unsafe source languages carry into Wasm binaries, which lack stack canaries *(search excerpt)* ([Lehmann et al., 2020](https://www.usenix.org/conference/usenixsecurity20/presentation/lehmann)). Inference: a hostile response from an external provider can compromise a C or C++ module without any escape. That attacker then holds whatever the module holds, including any secret and any shared state.

**Side channels.** Source: Wasmtime calls Spectre mitigation ongoing research and leaves `csdb` off by default on aarch64 ([security docs](https://docs.wasmtime.dev/security.html)). Source: Swivel hardens Wasm against Spectre at under 10.3% (randomised) or 3.3% to 240.2% (deterministic) on the Wasm-compatible SPEC 2006 subset ([USENIX 2021](https://www.usenix.org/conference/usenixsecurity21/presentation/narayan)). Inference: isolation that stops memory-safety escapes can still leave speculative reads between tenants in one process open, unless they are mitigated separately.

### Other Wasm runtimes

- **Wasmer.** Source: a Critical advisory published on 2026-11-02 says a compiler defect in Singlepass (7.0.0-alpha.1 to 7.4.2) corrupts the native stack and can run native code in the host process, demonstrated on Linux x86-64 in a smart-contract deployment. It needs multi-value support and an attacker-supplied module, has no CVE, and has no patched release yet ([GHSA-v2h8-j899-r676](https://github.com/wasmerio/wasmer/security/advisories/GHSA-v2h8-j899-r676)). *(Title only)*: two High compiler and table advisories appeared the same day, and a filesystem rights bypass on 2026-10-27.
- **wazero.** Source: implements Core 1.0 and 2.0 and WASI preview 1, in Go with no platform dependencies. The pages read do not mention the component model or resource limits ([docs](https://wazero.io/docs/), [specs](https://wazero.io/specs/)).
- **WasmEdge.** Source: plug-ins add native host functions from shared libraries, which extend the host and do not run in Wasm ([plug-ins](https://wasmedge.org/docs/start/wasmedge/extensions/plugins/)). *(Title only)*: one published advisory, an integer wrap in a memory size check (CVE-2025-69261).
- **WAMR.** Source: several 2025 CVEs, including a filesystem escape through symlinks on Windows with `uvwasi` *(search excerpt)* ([CVE-2025-64713](https://nvd.nist.gov/vuln/detail/CVE-2025-64713), [CVE-2025-43853, GCVE listing](https://cpe.gcve.eu/cpes/1787163)).

### V8 isolates

- Source: Cloudflare runs many isolates in one process, accepts only JavaScript and Wasm, freezes the clock during execution, forbids threads and shared memory, and has no local filesystem API. It adds a second sandbox of namespaces and seccomp, moves suspicious Workers to their own process, and separates customers into cordons by trust level. It reports a V8 patch gap under 24 hours ([security model, 2026-10-18](https://developers.cloudflare.com/workers/reference/security-model/)).
- Source: the same runtime's open-source form warns that alone it lacks defence in depth against implementation bugs, and that possibly malicious code needs an outer sandbox such as a VM ([workerd](https://github.com/cloudflare/workerd)).
- Source: every Chrome exploit caught in the wild from 2021 to 2023 began with a renderer memory-corruption bug, 60% of them in V8. The V8 sandbox costs about 1% or less, and as of April 2024 still had open issues before becoming a strong boundary ([V8 blog, 2024-04-04](https://v8.dev/blog/sandbox)).
- Source, vendor and without conditions: isolates start in 5 ms and use about 3 MB, against 35 MB for a basic Node function; an OS context switch can cost up to 100 µs ([2018-11-09](https://blog.cloudflare.com/cloud-computing-without-containers/)).
- Source: limits include 128 MB per isolate, a CPU limit from 10 ms to 5 minutes by plan, and 1 second for startup ([limits, 2026-10-29](https://developers.cloudflare.com/workers/platform/limits/)).
- Inference: the two advisories published in the `workerd` repository say little, since the serious bugs would be in V8, tracked elsewhere.

### gVisor

- Source: the Sentry implements the system API, a separate Gofer handles files, and the Sentry's host access is filtered. It offers little against hardware side channels, and resource exhaustion depends on host cgroups ([security](https://gvisor.dev/docs/architecture_guide/security/)).
- Source: its policy treats a Sentry exploit alone as insufficient for an escape, and generally assigns CVEs only to issues crossing the sandbox boundary ([policy](https://github.com/google/gvisor/security/policy)). Inference: counts of gVisor CVEs and Wasmtime advisories are therefore not comparable.
- Source: CVE-2020-14386, a Linux kernel flaw usable to escape containers, did not affect gVisor, because the code path is absent and several layers would have blocked it ([2020-09-18](https://gvisor.dev/blog/2020/09/18/containing-a-real-vulnerability/)).
- Source, aggregator only: CVE-2026-95702, published 2026-10-09, gives code execution in the Sentry in versions before 20260831.0, with the Sentry still confined by seccomp and namespaces ([OpenCVE](https://app.opencve.io/cve/CVE-2026-95702)). NVD's page would not load for me.
- Measured: simple system calls at least 2.2x slower than runc; file open and close on an external tmpfs 216x; small reads 11x; large downloads 2.8x; container create and destroy 1.117 s on KVM against 1.014 s for runc. Conditions: CloudLab machine with two 8-core 2.4 GHz CPUs, Docker 18.09.3, 2019 ([Young et al.](https://usenix.org/system/files/hotcloud19-paper-young.pdf)). This predates the Systrap platform that gVisor now recommends. Its current [performance page](https://gvisor.dev/docs/architecture_guide/performance/) gives results only as charts.

### Firecracker

- Measured: end-to-end start is under 125 ms to application code; memory overhead about 3 MB per VM; p99 of 146 ms for pre-configured VMs at 50 launching concurrently. Conditions: v0.20.0, EC2 m5d.metal, Linux 4.14.94 guest, 1 vCPU, 256 MB ([NSDI 2020](https://www.usenix.org/system/files/nsdi20-paper-agache.pdf)). The paper's headline also claims 150 VMs per second per host, and less than 5 MB overhead.
- Source: the jailer adds a chroot, pid and network namespaces, dropped privileges, and a seccomp profile of 24 system calls and 30 ioctls, as of that paper (same).
- Source: production guidance requires one tenant per process, turning off SMT and KSM, patched microcode, and an external watchdog. It does not filter guest egress, so the host must, for example blocking the instance metadata address ([host setup](https://github.com/firecracker-microvm/firecracker/blob/main/docs/prod-host-setup.md)).
- Source: CVE-2026-5747 is a guest-root out-of-bounds write through the opt-in PCI transport. On a standard guest kernel it stays in guest memory ([GHSA-776c-mpj7-jm3r](https://github.com/firecracker-microvm/firecracker/security/advisories/GHSA-776c-mpj7-jm3r)). *(Title only)*: CVE-2026-1386 is a host file overwrite through a symlink in the jailer.
- Source: Firecracker and gVisor both execute more host kernel code than native Linux *(search excerpt)* ([Anjali et al., 2020](https://conf.researchr.org/details/vee-2020/vee-2020-papers/1/Blending-Containers-and-Virtual-Machines-A-Study-of-Firecracker-and-gVisor)).

### seccomp, containers and eBPF

- Source: "System call filtering isn't a sandbox", in the kernel's words. It minimises exposed kernel surface and is one component ([kernel docs](https://docs.kernel.org/userspace-api/seccomp_filter.html)). The man page warns that deny-lists can be bypassed and recommends allow-lists ([seccomp(2)](https://man7.org/linux/man-pages/man2/seccomp.2.html)).
- *(Title only)*: `runc` has published container-escape advisories, including CVE-2024-21626 and three on 2025-11-05 (CVE-2025-31133, CVE-2025-52565, CVE-2025-52881) ([advisories](https://github.com/opencontainers/runc/security/advisories)).
- Source, weak: eBPF verifier bugs have repeatedly enabled local privilege escalation, and unprivileged access is commonly turned off *(search excerpt)* ([CVE-2021-3490](https://access.redhat.com/security/cve/cve-2021-3490), [CVE-2026-53092](https://www.sentinelone.com/vulnerability-database/cve-2026-53092/)). Inference: it is a tool for the host's own policy, for example filtering a module's traffic. It is not a home for third-party code.

## 2. Precedents: third parties uploading code

| Platform | Restricted | What failed or nearly failed | Upgrade with work in flight |
|---|---|---|---|
| Cloudflare Workers | See section 1 | Shared state store outage, 2025 (below). No published production escape found | Traffic split per request across versions; only one version of a Durable Object at a time |
| Fastly Compute | Fresh instance per request, spaced virtual memory | Compiler bug in 2021, found through crash logs | No in-flight state to migrate on the design read (Inference) |
| Shopify Functions | No clock or randomness; 256 kB binary, 11M instructions, 10,000 kB memory, 128 kB input, 20 kB output; network only through the host, for some merchants | Nothing found | Stateless on the page read (Inference); versioning not read |
| Envoy proxy-wasm | Host functions only; named upstreams | Docs still call the filter experimental; no Wasm-specific CVE found | One main instance shared across listener updates |
| Extism | Hosts and paths denied by default | No advisories published in the repository | Not read |
| Figma plugins | JavaScript sandbox in the same VM, later a separate VM in Wasm | Several escapes in the first sandbox; replaced in 11 days | Paused all publishing and updates during the incident |
| CosmWasm | Rust contracts; state owned per contract; messages only at the end of execution | Chain crash, slow blocks, and metering bugs; escape through the Wasm compiler, 2026-11-02 | Not read |
| Polkadot runtime upgrades | Runtime is Wasm stored on chain, and governance installs it | Migrations can halt the chain if they panic or overrun | Effective immediately, with migration in the next block |
| Upgradeable EVM contracts | Proxy points to an implementation | Shared library destroyed; storage collision; signed malicious upgrade | Instant unless a delay is added |

**Fastly, 2021.**
- Source: a register-allocator bug in Cranelift let a module read or write just before its heap. An engineer noticed crash logs from a researcher's module. Layout limited the reach: heaps over 4 GiB apart with guard regions. A second, unrelated bug in heap-size parsing also blocked simple negative offsets. Fastly then scanned every module uploaded in the exposure window, within its isolated compile pipeline, and found none exploitable ([2021-05-21](https://www.fastly.com/blog/defense-in-depth-stopping-a-wasm-compiler-bug-before-it-became-a-problem)).
- Inference, three points. Containment was partly luck. The scan was only possible because uploads were retained and scannable. Crash telemetry was the detector.

**Figma.** Source: its sandbox shared a JavaScript VM with the host, which allowed object-confusion escapes. It stopped approving plugins on 14 September, disabled updates on 18 September, shipped a Wasm-hosted VM on 25 September and audited all published plugins, finding no exploitation. It credits a swappable architecture, and it says review cannot enforce a security boundary. The digest shows no year ([post](https://www.figma.com/blog/an-update-on-plugin-security/)). Inference: stopping updates is a security control that has to exist before the incident.

**Shopify.** Source: limits as in the table ([API 2026-10](https://shopify.dev/docs/api/functions/current)). The module does not call the network. A `fetch` target returns a request, Shopify performs it, and the response enters the input of a `run` target ([network access](https://shopify.dev/docs/apps/build/functions/input-output/network-access)). Any language compiling to Wasm is accepted, with first-party support for Rust and JavaScript ([Wasm](https://shopify.dev/docs/apps/build/functions/programming-languages/webassembly-for-functions)).

**Envoy and Istio.** Source: the Wasm filter is labelled experimental, with configuration likely to change ([filter docs](https://www.envoyproxy.io/docs/envoy/latest/configuration/listeners/network_filters/wasm_filter)). Istio checks a SHA-256, and documents no signature check on that page. Its `FAIL_OPEN` strategy skips a plugin on fatal error, which it advises against for authentication and authorisation ([WasmPlugin](https://istio.io/latest/docs/reference/config/proxy_extensions/wasm-plugin/)). Inference: what happens when a provider module fails to load is a policy decision, and fail-open has a specific meaning for money.

**Smart contracts.**
- Source: CosmWasm contracts are Rust-based, without bindings for other languages ([docs](https://cosmwasm.github.io/)). Inference: using Wasm does not by itself make a platform multi-language.
- *(Title only)*: CosmWasm's VM has had a malicious contract able to crash the chain, another slowing block production, and incorrect metering ([advisories](https://github.com/CosmWasm/wasmvm/security/advisories)).
- Source: the Wasmer advisory above names unpermissioned contract upload as a remotely exploitable case. Inference: who may upload at all is a control.
- Source: on Polkadot, a migration panic can brick the chain. Outside actors can inflate state to make a migration slower than tested, and passing tests does not show a migration is safe ([FRAME docs](https://paritytech.github.io/polkadot-sdk/master/polkadot_sdk_docs/reference_docs/frame_runtime_upgrades_and_migrations/index.html)).
- Source, secondary: a user made themselves owner of an uninitialised shared library contract and destroyed it, freezing 587 wallets holding 513,774 ETH. The issue had been reported months earlier *(search excerpt)* ([Sophos](https://news.sophos.com/?p=389272), [Computing](https://computing.co.uk/news/3021517/parity-knew-about-wallet-bug-in-august-still-no-fix-for-frozen-funds)). Inference: a shared component turned one fault into loss for every dependent.
- Source, secondary: a storage collision in an upgradeable proxy allowed repeat initialisation and about 18.5M tokens drained from a project's treasury in 2022 *(search excerpt)* ([BankInfoSecurity](https://www.bankinfosecurity.com/thief-steals-6m-tokens-from-audius-sells-them-for-1m-a-19639), [GoPlus](https://blog.gopluslabs.io/vulnerabilities-cases/smart-contract/proxy-contract/audius)).
- Source: in February 2025 attackers took over USD 1.4 billion from a centralised exchange's multisig cold wallet. Malicious JavaScript in a wallet web interface showed signers one transaction and had them sign another, which rewrote the proxy's implementation. Three valid signatures passed on-chain ([NCC Group, 2025-03-10](https://www.nccgroup.com/research/in-depth-technical-analysis-of-the-bybit-hack/)). Inference: a quorum protects only what the approvers can truly see.
- Source: timelock contracts add a mandatory delay so users can leave before a dangerous change ([OpenZeppelin](https://docs.openzeppelin.com/contracts/5.x/api/governance#TimelockController)). One framework for rollups asks for an exit window of at least 7 days at Stage 1 for upgrades from outside its security council, and 30 days at Stage 2, where instant upgrades are allowed only for bugs detected on-chain ([L2BEAT](https://l2beat.com/stages)).

**Hot-swap with work in flight.**
- Source: Workers routes each request independently by version percentage, and keeps the last 100 versions; version affinity can hold a user to one ([gradual deployments](https://developers.cloudflare.com/workers/configuration/versions-and-deployments/gradual-deployments/)). A Durable Object shuts down on a code update. A deferred strategy waits for hibernation, on a best-effort basis, and no shutdown hook exists, so state must be written incrementally ([rules](https://developers.cloudflare.com/durable-objects/best-practices/rules-of-durable-objects/)).
- Source: the BEAM keeps two versions of a module. Running code stays on the old version until it makes a fully qualified call. Loading a third version terminates processes still on the oldest. A failing `on_load` leaves the current code in place ([Erlang docs](https://www.erlang.org/doc/system/code_loading.html)).
- Source: Golem validates a snapshot from the old revision against the new revision's schema on a manual update, and stays on the old revision if validation fails *(search excerpt)* ([Golem](https://learn.golem.cloud/v1.6/how-to-guides/ts/golem-custom-snapshot-ts)).
- Inference: three patterns emerge. A short-lived module instance has nothing to migrate, so an in-flight transaction finishes on its version. Two versions can run side by side with a bound on the old one. Or the upgrade is an explicit state conversion that can be refused. The durable-state report covers replay mismatch. `PS-19` per-transaction state makes the choice unavoidable: a transaction that runs for weeks on a slow rail (see that report's dispute windows) would pin an old, possibly vulnerable version. So "finish on your version" conflicts with "remove a bad version now".

## 3. Signing and provenance

| Mechanism | What it proves | What it does not prove | Weakness seen |
|---|---|---|---|
| Sigstore, cosign | A signer authenticated with an identity at a time | That they should have signed, or that the artifact is safe | Verifier bypasses in 2026; depends on OIDC accounts |
| SLSA v1.2 | Build came from the expected source and process, by level | Producer's honest intent; dependencies in full | Misconfigured or poisoned pipelines still produce valid outputs (Inference) |
| in-toto | Which steps ran, by whom, in what order | That a step's content is benign (Inference) | Not read in detail |
| TUF | Fresh, consistent, correctly delegated metadata; survives some key compromises | Honesty of a trusted party; absence of vulnerabilities | Implementation bugs, including rollback |
| Reproducible builds | Same source and environment give identical bits | That the source is benign | Needs someone to really rebuild from the reviewed source (Inference) |
| Wasm module signatures | Bytes of a module or parts of it signed by a key | Anything about the signer's process | Still a draft discussion |
| Digest pinning | These exact bytes | Who made them | Trusts whoever set the digest (Inference) |

Sources:
- **Sigstore.** A keyless signature shows authentication with an identity at a time. Whether that signer should sign, and whether the artifact is safe, needs separate policy. An account compromise lets an attacker sign anything that identity covers, and Sigstore does not handle that. Signers should monitor logs for misuse of their identity. TUF allows marking key material with its compromise time, so earlier signatures still verify. For artifacts, the page suggests a policy blocklist ([threat model](https://docs.sigstore.dev/about/threat-model/)). Certificates last 10 minutes ([Fulcio](https://docs.sigstore.dev/certificate_authority/overview/)).
- **SLSA.** Build threats are its strongest area. Malicious producers cannot be directly mitigated by its controls. Bugdoors, collusion, rubber-stamped reviews and typosquatting are not addressed, and dependency threats are deferred ([threats, v1.2](https://slsa.dev/spec/v1.2/threats)).
- **in-toto.** It makes visible which steps ran, by whom and in what order ([overview](https://in-toto.io/docs/what-is-in-toto/)). I did not read the layout specification.
- **TUF.** It addresses rollback, freeze, mix-and-match, endless data and below-threshold key compromise, using separated roles, thresholds and expiring trust. It assumes a trusted file's provider is not malicious ([security](https://theupdateframework.io/docs/security/)).
- **Reproducible builds.** Any party can recreate identical bits from the same source, environment and instructions ([definition](https://reproducible-builds.org/docs/definition/)).
- **Wasm signatures.** Custom sections hold signatures, with detached and multiple-signer forms, and partial verification when enabled. The repository calls itself initial discussions ([design](https://github.com/wasm-signatures/design)).

### What signature verification does not protect against

**A legitimate pipeline running attacker input.**
- Source: Ultralytics' first malicious releases came through its existing trusted-publishing workflow, and Sigstore logs and PyPI attestations showed exactly that. Later analysis points to a poisoned Actions cache. A second round used an old API token, and PyPI spotted it partly because it lacked attestations ([PyPI, 2024-12-11](https://blog.pypi.org/posts/2024-12-11-ultralytics-attack-analysis/)). Inference: attestation verification would have accepted the first wave. The same record made forensics possible and exposed the second wave.
- Source: TanStack had 84 malicious versions across 42 packages published in about six minutes on 2026-05-11. A `pull_request_target` flaw, cache poisoning and an OIDC token read from runner memory combined, authenticating through the real trusted-publisher binding. It was detected externally after about 26 minutes, with no internal alert, and unpublishing was largely blocked by registry policy ([postmortem](https://tanstack.com/blog/npm-supply-chain-compromise-postmortem)). A separate report says the wave carried signed provenance *(search excerpt)* ([CSA](https://labs.cloudsecurityalliance.org/research-rb/csa-research-note-shai-hulud-ai-npm-supply-chain-20260514-cs/)); the postmortem as read does not state this.

**A malicious update by a legitimate publisher.**
- Source: the xz backdoor was shipped by a maintainer, signed by them, and present in release tarballs but not in git *(search excerpt)* ([Qualys](https://blog.qualys.com/vulnerabilities-threat-research/2024/03/29/xz-utils-sshd-backdoor), [Alpine](https://alpinelinux.org/posts/XZ-backdoor-CVE-2024-3094.html)). Alpine's fix was to rebuild from git. Inference: this is the case where rebuilding from reviewed source helps, and only if the source used is the one reviewed.
- Source: `event-stream` passed to a new maintainer, who added a malicious dependency *(search excerpt)* ([npm](https://blog.npmjs.org/post/180565383195/details-about-the-event-stream-incident)). `node-ipc`'s own maintainer added destructive code as a protest *(search excerpt)* ([Qualys](https://threatprotect.qualys.com/2022/03/22/node-ipc-npm-package-embedded-malicious-code-vulnerability-cve-2022-23812/)).

**A stolen account or key.**
- Source: a browser-extension vendor's staff approved a phishing OAuth prompt, a malicious update went out on the store's normal channel, and it auto-installed. Reports vary between under 24 and about 31 hours live *(search excerpt)* ([TechCrunch](https://techcrunch.com/2024/12/27/cyberhaven-says-it-was-hacked-to-publish-a-malicious-update-to-its-chrome-extension)).
- Source: a hardware-wallet vendor's front-end library was replaced after a former employee was phished; accounts differ on the mechanism and on losses of USD 600,000 to 700,000 *(search excerpt)* ([Checkmarx](https://checkmarx.com/blog/npm-account-takeover-results-in-crypto-supply-chain-attack.md)).
- Inference: a signature made with a stolen key is valid. Telling it apart needs an independent record of when it was made, plus monitoring by the true owner.

**A compromised build system.** Source: SUNSPOT sat on a build server and swapped a source file during compilation, with safeguards so builds would not fail *(search excerpt)* ([CrowdStrike](https://crowdstrike.com/blog/sunspot-malware-technical-analysis/)). Inference: signing after that point signs the implant. Only an independent rebuild would notice.

**The verifier itself.**
- Source: in cosign, a non-certificate value in a legacy bundle was treated as a public key, skipping identity checks while reporting success ([GHSA-fx35-mq7g-6g98, 2026-08-06](https://github.com/sigstore/cosign/security/advisories/GHSA-fx35-mq7g-6g98)). A bundle's Rekor entry was not checked against the artifact, a regression of a 2022 fix ([GHSA-whqx-f9j3-ch6m, 2026-01-09](https://github.com/sigstore/cosign/security/advisories/GHSA-whqx-f9j3-ch6m)).
- *(Title only)*: Fulcio and Rekor have published SSRF and memory advisories in 2026, and `go-tuf` had a rollback gap in 2022 (CVE-2022-29173).
- Inference: verification is a parser of attacker-supplied input. It needs the same isolation and patching as the sandbox.

**Things a signature never addresses.** Inference: a correct, honest module that is exploitable (section 1); a signed old version replayed after a fix, unless freshness is checked (TUF names this); a signed module whose behaviour depends on data fetched at run time; and approvers shown something other than what they sign (the February 2025 case above).

**Revocation.** Source: keyless certificates expire in minutes, so revoking one is not the mechanism; Sigstore suggests revoking artifacts through policy. TUF offers key replacement and expiry. Inference: rutent's own platform decides whether a module may run. So revocation here can mean a catalogue state change, effective at the next load, rather than a certificate operation. The harder questions are what happens to transactions in flight under a revoked version, and whether stopping them is safer than letting them finish.

## 4. Module state

| Platform | Shared or durable state | Per-invocation state | Isolation | Commits with the host's record? |
|---|---|---|---|---|
| Durable Objects | SQLite per object | Class properties, lost on eviction | Per object | Writes without an intervening await commit atomically; responses wait for durability |
| Workers KV | Global key-value | None | Per namespace | No; eventual, 60 s or longer across locations |
| proxy-wasm | Shared key-value with compare-and-swap, shared queues | Per-request context | Queues resolvable across plugins by VM ID | No; persistence not stated |
| Extism | Vars, size-capped | Plugin memory | Per plugin | Not described |
| WASI key-value proposal | Phase 2; provider-defined consistency; atomic on a single key | None | Host-defined | Not addressed |
| CosmWasm | Own state per contract | Execution | Each actor changes only its own state | Inference: yes, one state machine commits both |
| Shopify Functions | None seen | Input and output only | No shared memory | Host holds all state (Inference) |
| Golem | Oplog-backed durable state | Same | Per agent | Journal written by the platform *(search excerpt)* |

Sources: [Durable Objects rules](https://developers.cloudflare.com/durable-objects/best-practices/rules-of-durable-objects/), [gates, 2021-08-03](https://blog.cloudflare.com/durable-objects-easy-fast-correct-choose-three/), [KV](https://developers.cloudflare.com/kv/concepts/how-kv-works/), [proxy-wasm ABI](https://github.com/proxy-wasm/spec/blob/main/abi-versions/v0.2.1/README.md), [Extism manifest](https://extism.org/docs/concepts/manifest/), [wasi-keyvalue](https://github.com/WebAssembly/wasi-keyvalue), [CosmWasm actor model](https://cosmwasm.github.io/core/architecture/actor-model), [Golem durability](https://learn.golem.cloud/v1.6/how-to-guides/ts/golem-configure-durability-ts) *(search excerpt)*.

**The strongest guarantee seen.**
- Source: Durable Objects hold outgoing messages until pending writes are safe. If a write fails, those messages become errors and the object restarts from scratch. An external `fetch()` still lets other requests interleave, so the docs advise optimistic version checks. A transaction callback may rerun, and alarms may fire twice.
- Source: CosmWasm writes state first and sends messages only at the end of execution, which removes re-entrancy.
- Inference: both stop at the platform's own edge. Neither makes an external provider's action commit with a local record, which the durable-state report treats at length.

**Per-transaction isolation.** Source: Wasmtime recommends disposable instances per task, so a bug triggered by one input cannot reach later tasks ([2022](https://bytecodealliance.org/articles/security-and-correctness-in-wasmtime)). *(Title only)*: instance reuse has leaked before, as data leakage in the pooling allocator (2022 High, 2026 Low). Inference: `PS-19`'s per-transaction state is naturally a fresh instance plus an explicit store, which means instance memory is not the record.

**Failure modes when state and the host's record cannot commit together.** All Inference, with the sources above as anchors.
- **Module state ahead of the host.** The module recorded progress, the host crashed, and the host retries a stage the module believes is done. The module's shared counters, nonces or sequence numbers are now ahead.
- **Host ahead of the module.** The host recorded success and the module's write failed. The next transaction reads stale state, such as an unspent balance or an old token.
- **Effect without either record.** The external call happened and both records were lost. This is the unknown outcome in the durable-state report.
- **Retries of module code.** A rerun callback or a twice-fired alarm repeats effects unless the host deduplicates.
- **Shared state as a channel.** Shared state crosses transactions by design. A compromised transaction can poison later ones, or read what earlier ones left. It also forms a side path around a per-transaction audit trail if writes to it are not recorded.
- **Audit gap.** `PS-21` covers "amendment in the state". If a module's shared state changes outside the record, the host cannot say why a later decision was made.
- **State format across versions.** A new version reads old state. The migration may panic, overrun or be inflated by outside actors (Polkadot), and refusal may be the safe path (Golem).
- **Weak consistency.** Check-then-act on an eventually consistent store allows duplicates or lost updates, so a limit could be spent twice.
- **State store as shared failure domain.** Source: in June 2025 a storage failure behind Workers KV failed about 90% of its requests for about 2.5 hours and took down products built on it. Some failed closed, and one turned on a kill switch that allowed tokens to be redeemed more than once ([Cloudflare](https://blog.cloudflare.com/cloudflare-service-outage-june-12-2025/)). Inference: every provider module sharing one state service shares its outages, and the failure direction must be decided in advance.

## 5. How a module reaches the outside world

### Patterns

| Pattern | Example | Module sees the secret? | Destination control | What remains possible |
|---|---|---|---|---|
| Secret passed to module code | Workers `env` | Yes, in plaintext | Optional, separate | Anything the module can reach; exposed by any in-module bug |
| Allow-list on the module's own calls | Spin, Extism, Forge | Depends | Host list | Misuse of allowed hosts |
| Host-side proxy injects the credential | Tokenizer, Outbound Workers | No | Per-secret host list, or code | Using the secret at the allowed host for any request |
| Host performs a call the module describes | Shopify `fetch`, proxy-wasm `http_call` | No, if the host adds it (Inference) | Host | Whatever the module can express in the request |
| No network from the module | Default Shopify Functions, smart contracts | Not applicable | Total | Trust moves to whoever supplies the input |

Sources:
- **Workers.** Secrets reach code as environment values, passed through as defined. Changing a secret creates a new version ([secrets](https://developers.cloudflare.com/workers/configuration/secrets/)).
- **Outbound Workers.** The platform owner's Worker sees every `fetch` from a user Worker and can log, block or add credentials. With one enabled, `connect()` sockets are unavailable, but requests from Durable Objects and through mTLS bindings are not intercepted ([docs, 2026-04-21](https://developers.cloudflare.com/cloudflare-for-platforms/workers-for-platforms/configuration/outbound-workers/)). Inference: the choke point has documented gaps.
- **Tokenizer.** A stateless proxy holds a private key. The application stores only encrypted secrets and sends requests carrying them. The proxy decrypts and injects, or computes an HMAC, and can restrict each secret to named hosts. A compromised application can still use a secret at an allowed host; the restriction limits where it goes, not whether it is used ([Fly.io](https://fly.io/blog/tokenized-tokens/), [repo, Apache 2.0](https://github.com/superfly/tokenizer)).
- **Spin.** No outbound HTTP by default. Hosts are listed in the manifest, wildcards widen access, and `https://*:*` is for development only ([docs](https://spinframework.dev/v3/http-outbound)).
- **Extism.** An empty `allowed_hosts` denies all, and `null` allows all. An optional SHA-256 can pin a module ([manifest](https://extism.org/docs/concepts/manifest/)). Host functions are how other capabilities are granted ([docs](https://extism.org/docs/concepts/host-functions/)).
- **Forge.** Domains are declared in the manifest and undeclared hosts fail. Changing permissions or egress may need a major version, which users accept again, and redirects to third parties must also be declared ([egress, 2026-10-07](https://developer.atlassian.com/platform/forge/runtime-egress-permissions/)). Inference: this is the clearest precedent for a capability change needing fresh approval.
- **proxy-wasm.** Calls go to named upstreams, and the spec says hosts should keep a per-plugin list of external resources ([ABI](https://github.com/proxy-wasm/spec/blob/main/abi-versions/v0.2.1/README.md)).
- **Wasmtime.** `wasi-http` offers hooks over sending requests, plus a forbidden-header filter ([docs](https://docs.wasmtime.dev/api/wasmtime_wasi_http/index.html)). That page does not document a header-injection API directly, so I infer injection is possible through hooks.

### Bypasses seen

| Case | What happened | Source |
|---|---|---|
| Spin 2024 | With `self` allowed, a client's `Host` header chose the destination. Rated Critical, 9.1 | [CVE-2024-32980](https://github.com/spinframework/spin/security/advisories/GHSA-f3h7-gpjj-wcvh) |
| Wasmtime HTTP, 2026 | The request line could name a host other than the one filtered; a forward proxy honouring it would forward there | [GHSA-5qpp-g3j2-8frx](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-5qpp-g3j2-8frx) |
| Wasmtime UDP, 2026 | A race let a guest send to an address allowed only for receiving | [GHSA-hmmr-87qq-258j](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-hmmr-87qq-258j) |
| Wasmtime files, 2026 | A symlink rename gave a full filesystem escape on macOS and FreeBSD, after an earlier fix covered only opens | [GHSA-m9f7-9w4p-gv38](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-m9f7-9w4p-gv38) |
| Deno, 2026 | `fetch()` and WebSocket sandbox bypass through a missing DNS-resolution check *(title only)* | [advisories](https://github.com/denoland/deno/security/advisories) |
| Firecracker | Egress is not filtered at all unless the host does it, including the metadata address | [host setup](https://github.com/firecracker-microvm/firecracker/blob/main/docs/prod-host-setup.md) |

Inference: host names are checked at one layer and connections are made at another, and these bugs live in that gap. A filter applied to a name a module chose, separately from the address finally used, needs checking at both.

### Limiting exfiltration

- Source: DNS is a common covert channel, since it is often allowed and has many fields to hide data in ([MITRE ATT&CK T1071.004, v1.4](https://attack.mitre.org/techniques/T1071/004/)).
- Source: on CDNs, an allowed name can front for a different destination; a filter must block the whole domain or allow it *(search excerpt)* ([Fifield et al., 2015](https://www.petsymposium.org/popets/2015/popets-2015-0009.php)).
- Inference: an allow-list limits destinations and cannot read intent. Channels remaining with a perfect list:
  - The request body or headers to the allowed provider, which may be a large multi-tenant service where an attacker also has an account.
  - The module's return values to the host, including quotes and routes that could carry data, or lie.
  - Logs, error strings, shared state, retries and timing.
- Inference: `PS-31` has a specific form of this. A reserve module may legitimately see a real balance, and it must answer only whether funds suffice. The sandbox cannot enforce that unless the host can check the module's output shape, and even a yes-or-no output repeats into a measure.
- Inference: host-side credentials move risk rather than remove it. The module can no longer steal a key, but it can still act with it at the provider, including moving money. The proxy becomes a concentrated secret holder. Limits on what the provider's own credential may do, set on the provider's side, are the only limit that holds against both. Whether a provider offers such scoping varies, and I found no survey.

## 6. What this implies

### 6a. Capabilities to think about stating

None names a mechanism. Each could be met in more than one way. These are prompts for the spec authors, not requirements.

**Admission** (`REQ-MDL-*`, `PS-20`, `PS-21`)
- A module is admitted only after its publisher's identity and its integrity are checked, and the check is recorded.
- The catalogue states who may publish for which provider, and a publisher cannot replace another's module.
- Each module declares the capabilities it needs, and an upgrade that widens them is visibly different from one that does not.
- Every artifact ever admitted stays available, so the catalogue can be scanned or rebuilt after a discovered flaw.
- The spec states whether machine code supplied by a publisher is ever trusted, and what follows if it is.

**Isolation** (threat area: sandbox escape)
- The reach of one module's compromise is bounded and stated: what it can read, what it can act on, and which other providers it can affect.
- One module's failure, including a crash of what runs it, does not stop other providers beyond a stated bound.
- The design states which classes of attack it does not defend against, including hardware side channels.
- A module cannot read another module's memory, state or secrets, and cannot read state of another transaction except through declared shared state.

**Resource limits** (`PS-20`)
- CPU, memory, wall-clock time, outbound calls, outbound volume and state size are limited per module and per transaction, with a defined outcome when a limit is hit.
- A limit being hit leaves the transaction in a known state, not an unknown one.
- Limits cover resources consumed on the host's side on a module's behalf.

**Patching the platform** (threat area: sandbox escape)
- The execution environment can be replaced within a stated time from a published fix, without changing modules and without losing transactions in flight.
- After a compiler or runtime flaw, the platform can identify what ran, re-derive it, and check past exposure.
- There is a way to halt publishing and upgrades during an incident.
- Signals of probable escape attempts, such as crashes attributable to a module, reach someone.

**Upgrade** (`PS-18`, `PS-21`)
- A change to one module does not interrupt other providers.
- Each transaction records the exact module version that acted on it.
- Behaviour of in-flight transactions across an upgrade is defined, including when a version is revoked.
- An upgrade can be staged, observed, delayed and rolled back, and each of those steps is attributable.
- A failed upgrade leaves the previous version in force.
- Who approved an upgrade, and what they were shown, is recorded in a form that binds what was approved to what ran.

**State** (`PS-19`, `PS-21`)
- The commit relationship between module state and the host's record is stated, including what happens if only one of them commits.
- Changes to shared state are attributable to a transaction and a module version.
- State format changes across versions have a defined path, including refusal.
- Consistency of shared state is stated, so limits and counters are safe under concurrency.
- A rerun of module code does not repeat an external effect, for operations declared safe to repeat.

**Outbound and secrets** (threat areas: module supply chain, inference from balance verifications)
- A module reaches only declared destinations, with deny as the default, and this holds through redirects and name resolution.
- Secrets are never disclosed to whoever published the module, and the spec states whether module code may ever hold one.
- Secret use is attributable per transaction, and rotation does not require a new module release.
- Every outbound call is recorded with destination, module version and transaction.
- Outputs from a module are checked against the declared shape of its operation, so a reserve's answer can reveal only whether funds suffice.
- Provider outputs are treated as untrusted input by everything downstream, including quoting.

**Failure policy** (`REQ-OPS-*`)
- The behaviour when a module or its state service is unavailable is stated, and the system does not fail open on decisions that move value.

### 6b. Candidate mechanisms for a non-binding suggestion

For `suggestions/`. No ranking is implied. Facts are sourced above; trade-offs are Inference.

**Execution boundary**

| Candidate | For | Against |
|---|---|---|
| Wasm components in-process, one runtime | Very fast start; capabilities explicit in imports; many languages in principle; host owns I/O | Compiler bugs are escapes, and 2026's record shows they recur; side channels between tenants; a host panic hits all; interfaces still moving |
| Wasm in a separate process per provider or trust level | Keeps explicit capabilities, and an escape meets OS limits | More memory, IPC cost, more moving parts |
| Wasm inside a microVM or gVisor per tenant | Two independent boundaries; matches what the published operators layer | Highest operational cost; host requirements; two patch streams |
| V8 isolates | Mature at very large scale; fast | JavaScript and Wasm only; its own project says an outer sandbox is needed; a fast patching duty against a browser engine |
| gVisor | Runs ordinary Linux programs; strong language range | I/O-heavy cost seen in 2019; resource and network control left to the host; determinism none |
| Firecracker microVMs | Hardware boundary; any language; small monitor | About 125 ms to start; Linux and KVM hosts; no module-level capability model, so egress and secrets need separate work |
| Containers with seccomp | Simple; any language | Shared kernel; recurring runtime escapes; its own kernel docs say filtering is not a sandbox |

**Who compiles to native code**

| Candidate | For | Against |
|---|---|---|
| Publisher uploads a portable form; platform compiles | Publisher's native output never trusted; platform can recompile after a flaw | Compiler now runs on hostile input, so it needs its own isolation |
| Publisher uploads native code in a VM | Any toolchain; no compile step | Sandbox relies on the VM boundary alone; reproducibility harder |
| Platform builds from source | Strongest link from source to artifact; enables review | Takes on hostile build input and every language's toolchain; publishers may not want to share source |

**Provenance and distribution**

| Candidate | For | Against |
|---|---|---|
| Keyless signing with a transparency log (Sigstore) | No long-lived keys; public record; helps forensics | Identity policy is the verifier's job; OIDC account is the root; recent verifier bugs; public log may expose publisher activity |
| Publisher-held keys with a threshold | Works offline; no public service; quorum possible | Key custody; rotation; a quorum protects only what approvers see |
| TUF for the catalogue metadata | Rollback, freeze and mix-and-match addressed; compromise recovery | Adds roles and operations; does not address honesty |
| SLSA-level build provenance | Shows origin of a build; useful signal when absent | Valid output from poisoned pipelines; attests only as far as the builder |
| Reproducible builds plus an independent rebuilder | Catches compromised build systems | Hard across many languages; needs an independent party |
| Upgrade delay and staged rollout | Limits signed malicious updates; gives monitors time | Slows urgent fixes, so an emergency path is needed, which is itself a target |
| Capability diff requiring approval on widening | Catches the obvious attacks | Attacks inside granted capabilities pass; review fatigue |

**Secrets and egress**

| Candidate | For | Against |
|---|---|---|
| Host-side injection at a proxy or hook | Module never holds the secret; central log | Non-HTTP protocols and signed payloads need specific support; concentrated secret holder |
| Module describes the call, host performs it (split model, `OQ-14`) | Strongest control; easy to record intent; replay gets simpler | Providers with unusual protocols or stateful sessions fit badly; less module freedom |
| Secret in module with allow-list | Simple; any protocol | Any module bug or publisher intent exposes the secret |
| Host-side signing service | Key cannot leave; every signature recorded | Does not limit what is signed, unless policy can parse it |
| Provider-side scoping of credentials | The only limit holding against a compromised platform | Depends on each provider, and many may not offer it |

**State**

| Candidate | For | Against |
|---|---|---|
| Module state in the host's own transaction | State and record commit together; attribution is simple | Ties modules to one store; host carries load and format changes |
| Separate per-module store plus reconciliation | Isolation and independent scale | The gaps in section 4 become normal cases |
| Journal of module effects, replayed (Golem-style) | Recovery and audit share one source | Determinism needed; replay across versions is hard |
| Stateless modules, with state passed in and returned by the host | No hidden state; upgrade is simple; output checkable | Bigger messages; long sessions awkward; narrows what `PS-19` suggests |

**In-flight upgrade**

| Candidate | For | Against |
|---|---|---|
| Transaction pinned to its version | Simple, replayable | Keeps vulnerable versions alive for weeks on slow rails |
| Two versions at once, with a bound on the old | Known model (BEAM) | The third load kills stragglers, so needs a safe stop |
| Explicit state conversion | Handles long transactions | Migrations can fail, and each provider must write them |
| Each stage as a new short instance on the current version | Removes pinning | New code must accept old state at every stage |

## 7. Questions only the owner can answer

- Who compiles a module to machine code: the publisher, or the platform (`PS-20`)? Must rutent keep every uploaded artifact?
- Who writes modules in v1 (`OQ-15`): only the operator, vetted partners, or anyone? The answer changes how much of this threat model applies from day one.
- Which languages are truly needed in v1 (`OQ-15`)? Does "a wide range" mean any language, or a few, or a portable target?
- May a module ever hold a plaintext secret, and whose secrets are they: the operator's, or the provider's or publisher's?
- Must a module be able to reach the network itself, or may only the host do so (`OQ-14`)? Must every provider protocol fit that?
- May modules from different publishers share a process? May two providers ever share one, given that an escape then crosses between them?
- How long may a runtime with a known escape stay in service before providers must be stopped?
- May transactions in flight finish on a revoked version, or must they stop? Who bears the loss either way?
- Must every upgrade wait out a delay? Who may skip it, and who is told? May a provider force an upgrade the operator has not approved?
- Who approves an upgrade: the operator alone, the provider, or both? Are users of that provider told first?
- Does rutent ever fail open: when a module cannot load, or a state service is down, does it stop that provider or the whole route?
- Is deterministic replay of a module required, or is recording its inputs and outputs enough?
- Must module state commit with the host's own record, or is reconciliation acceptable? How much state do modules really need to keep (`PS-19`)?
- Which host platforms are in scope (`OQ-4`)? Does aarch64 count, given different support tiers?
- May a sandbox add 125 ms per transaction, or must it be near zero? Is density a constraint at all at the expected scale (`ASM-1`)?
- Must publishers be publicly identifiable? May a transparency log expose who published what, and when?
- Must third parties be able to verify which module version acted on an order (`OQ-11`)?
- Is a paid or single-vendor runtime acceptable as a required dependency (`OQ-16`)?
- May providers refuse to give scoped credentials and still join the catalogue (`OQ-13`)?

## 8. What I could not determine

- Whether any production escape has happened on Cloudflare Workers, Fastly Compute or Shopify Functions. I found none published, which is not evidence that none happened.
- Whether Wasmtime's rise in advisories reflects new tools only, or also new code such as the component model and WASI 0.3. I have the April batch's account, not one for October, and I found no primary post on the October set.
- Comparable figures across runtimes. I found no independent, current benchmark on one machine for Wasmtime, V8, gVisor and Firecracker, or one for a workload dominated by external calls.
- Current gVisor costs with Systrap as numbers, since its page shows charts only.
- NVD's own record of CVE-2026-95702, which would not load, and whether gVisor publishes advisories anywhere the API sees. The API returned none.
- wazero's advisory record. The API returned none under either repository name tried, and I did not search further. Also its current position on the component model and on resource limits.
- WasmEdge's record beyond one advisory, its component model status, and its limits.
- Whether Wasmer's Singlepass is the compiler used by default in common embeddings, and when a patched release will ship.
- Whether `component-model-async` is on by default in the versions affected by GHSA-32h6-97mm-8q3c.
- Fastly's secret and backend model. Its documentation pages returned 404 or did not answer, apart from a limit of 5 secret reads per request.
- Solana's upgrade authority behaviour. The page returned HTTP 429.
- The primary Parity postmortem, which did not load. Those facts rest on secondary excerpts.
- SLSA v1.2's levels page, which returned 404. Only the threats page was read, so levels are described only as far as that page does.
- The in-toto layout specification, and the Sigstore rule on verifying signatures after certificate expiry.
- Whether `wasi-keyvalue` batches are atomic. The `batch.wit` file was not read.
- How Golem's automatic, replay-based updates work, and its default for external HTTP calls.
- How Workers handles in-flight requests during a deploy, and whether Workflows pins a version. No page read says.
- Envoy Wasm-specific CVEs. One search found only other filters, and I did not read Envoy's advisory list.
- Whether the TanStack releases carried signed provenance. Secondary reports say so, and the postmortem as read does not.
- Adoption of `wasm-signatures` and whether any registry enforces it.
- Which real payment or exchange providers offer narrowly scoped credentials. This decides how much provider-side limits can help, and I found no survey.
- Whether a confidential-computing or attestation approach would help prove which module ran. I did not research it.

## References

All accessed 2026-11-02. Version or date as shown by the source; "undated" means none was visible.

**WebAssembly**
- Wasmtime: [security, undated](https://docs.wasmtime.dev/security.html), [tiers](https://docs.wasmtime.dev/stability-tiers.html), [release policy](https://docs.wasmtime.dev/stability-release.html), [interrupting](https://docs.wasmtime.dev/examples-interrupting-wasm.html), [Config 50.0.1](https://docs.rs/wasmtime/latest/wasmtime/struct.Config.html), [ResourceLimiter](https://docs.rs/wasmtime/latest/wasmtime/trait.ResourceLimiter.html), [Module 51.0.0-dev](https://docs.wasmtime.dev/api/wasmtime/struct.Module.html), [wasi-http 51.0.0-dev](https://docs.wasmtime.dev/api/wasmtime_wasi_http/index.html), [advisories](https://github.com/bytecodealliance/wasmtime/security/advisories).
- Bytecode Alliance: [security and correctness, 2022-09-13](https://bytecodealliance.org/articles/security-and-correctness-in-wasmtime), [performance, 2022-09-06](https://bytecodealliance.org/articles/wasmtime-10-performance), [April advisories, 2026](https://bytecodealliance.org/articles/wasmtime-security-advisories), [WASI 0.3](https://bytecodealliance.org/articles/WASI-0.3).
- [Component Model language support](https://component-model.bytecodealliance.org/language-support.html). [WebAssembly 3.0 profiles](https://webassembly.github.io/spec/core/appendix/profiles.html).
- [Wasmer advisories](https://github.com/wasmerio/wasmer/security/advisories). [wazero docs](https://wazero.io/docs/), [specs](https://wazero.io/specs/). [WasmEdge plug-ins](https://wasmedge.org/docs/start/wasmedge/extensions/plugins/).
- [Swivel, USENIX Security 2021](https://www.usenix.org/conference/usenixsecurity21/presentation/narayan). [Lehmann et al., USENIX Security 2020](https://www.usenix.org/conference/usenixsecurity20/presentation/lehmann). [Jangda et al., ATC 2019](https://usenix.org/conference/atc19/presentation/jangda). [Faasm, ATC 2020](https://arxiv.org/pdf/2002.09344).

**Isolates, VMs, kernels**
- Cloudflare: [security model, 2026-10-18](https://developers.cloudflare.com/workers/reference/security-model/), [limits, 2026-10-29](https://developers.cloudflare.com/workers/platform/limits/), [without containers, 2018-11-09](https://blog.cloudflare.com/cloud-computing-without-containers/), [workerd](https://github.com/cloudflare/workerd).
- [V8 sandbox, 2024-04-04](https://v8.dev/blog/sandbox).
- gVisor: [security](https://gvisor.dev/docs/architecture_guide/security/), [performance](https://gvisor.dev/docs/architecture_guide/performance/), [policy](https://github.com/google/gvisor/security/policy), [2020-09-18](https://gvisor.dev/blog/2020/09/18/containing-a-real-vulnerability/). [Young et al., HotCloud 2019](https://usenix.org/system/files/hotcloud19-paper-young.pdf). [Anjali et al., VEE 2020](https://conf.researchr.org/details/vee-2020/vee-2020-papers/1/Blending-Containers-and-Virtual-Machines-A-Study-of-Firecracker-and-gVisor).
- Firecracker: [Agache et al., NSDI 2020](https://www.usenix.org/system/files/nsdi20-paper-agache.pdf), [host setup](https://github.com/firecracker-microvm/firecracker/blob/main/docs/prod-host-setup.md), [advisories](https://github.com/firecracker-microvm/firecracker/security/advisories).
- [Seccomp filter, kernel docs](https://docs.kernel.org/userspace-api/seccomp_filter.html). [seccomp(2)](https://man7.org/linux/man-pages/man2/seccomp.2.html). [runc advisories](https://github.com/opencontainers/runc/security/advisories).

**Platforms**
- Cloudflare: [gradual deployments, 2026-10-18](https://developers.cloudflare.com/workers/configuration/versions-and-deployments/gradual-deployments/), [Durable Objects rules, 2026-10-18](https://developers.cloudflare.com/durable-objects/best-practices/rules-of-durable-objects/), [gates, 2021-08-03](https://blog.cloudflare.com/durable-objects-easy-fast-correct-choose-three/), [KV, 2026-04-21](https://developers.cloudflare.com/kv/concepts/how-kv-works/), [outbound Workers, 2026-04-21](https://developers.cloudflare.com/cloudflare-for-platforms/workers-for-platforms/configuration/outbound-workers/), [secrets, 2026-10-15](https://developers.cloudflare.com/workers/configuration/secrets/), [outage, June 2025](https://blog.cloudflare.com/cloudflare-service-outage-june-12-2025/).
- [Fastly, 2021-05-21](https://www.fastly.com/blog/defense-in-depth-stopping-a-wasm-compiler-bug-before-it-became-a-problem).
- Shopify: [limits, API 2026-10](https://shopify.dev/docs/api/functions/current), [Wasm](https://shopify.dev/docs/apps/build/functions/programming-languages/webassembly-for-functions), [network access](https://shopify.dev/docs/apps/build/functions/input-output/network-access).
- Envoy: [Wasm overview](https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/advanced/wasm), [filter](https://www.envoyproxy.io/docs/envoy/latest/configuration/listeners/network_filters/wasm_filter). [proxy-wasm ABI 0.2.1](https://github.com/proxy-wasm/spec/blob/main/abi-versions/v0.2.1/README.md). [Istio WasmPlugin](https://istio.io/latest/docs/reference/config/proxy_extensions/wasm-plugin/).
- Extism: [manifest](https://extism.org/docs/concepts/manifest/), [host functions](https://extism.org/docs/concepts/host-functions/). Spin: [outbound, v3](https://spinframework.dev/v3/http-outbound), [CVE-2024-32980](https://github.com/spinframework/spin/security/advisories/GHSA-f3h7-gpjj-wcvh). [Forge egress, 2026-10-07](https://developer.atlassian.com/platform/forge/runtime-egress-permissions/). [Tokenizer](https://fly.io/blog/tokenized-tokens/), [repo](https://github.com/superfly/tokenizer). [Figma](https://www.figma.com/blog/an-update-on-plugin-security/). [wasi-keyvalue](https://github.com/WebAssembly/wasi-keyvalue). [Erlang code loading](https://www.erlang.org/doc/system/code_loading.html). [Golem](https://learn.golem.cloud/v1.6/how-to-guides/ts/golem-custom-snapshot-ts). [Deno advisories](https://github.com/denoland/deno/security/advisories).

**Chains**
- CosmWasm: [docs](https://cosmwasm.github.io/), [actor model](https://cosmwasm.github.io/core/architecture/actor-model), [wasmvm advisories](https://github.com/CosmWasm/wasmvm/security/advisories). Polkadot: [upgrades, 2026-06-29](https://docs.polkadot.com/parachains/runtime-maintenance/runtime-upgrades/), [FRAME migrations](https://paritytech.github.io/polkadot-sdk/master/polkadot_sdk_docs/reference_docs/frame_runtime_upgrades_and_migrations/index.html). [NCC Group, 2025-03-10](https://www.nccgroup.com/research/in-depth-technical-analysis-of-the-bybit-hack/). [OpenZeppelin timelock](https://docs.openzeppelin.com/contracts/5.x/api/governance#TimelockController). [L2BEAT stages](https://l2beat.com/stages).

**Provenance**
- Sigstore: [threat model](https://docs.sigstore.dev/about/threat-model/), [Fulcio](https://docs.sigstore.dev/certificate_authority/overview/), [cosign advisories](https://github.com/sigstore/cosign/security/advisories). [SLSA v1.2 threats](https://slsa.dev/spec/v1.2/threats). [in-toto](https://in-toto.io/docs/what-is-in-toto/). [TUF security](https://theupdateframework.io/docs/security/). [Reproducible builds](https://reproducible-builds.org/docs/definition/). [Wasm signatures](https://github.com/wasm-signatures/design).
- Incidents: [PyPI on Ultralytics, 2024-12-11](https://blog.pypi.org/posts/2024-12-11-ultralytics-attack-analysis/). [TanStack, 2026-05](https://tanstack.com/blog/npm-supply-chain-compromise-postmortem). [CrowdStrike SUNSPOT](https://crowdstrike.com/blog/sunspot-malware-technical-analysis/). [npm on event-stream](https://blog.npmjs.org/post/180565383195/details-about-the-event-stream-incident).

**Exfiltration**
- [MITRE T1071.004, v1.4, 2026-09-03](https://attack.mitre.org/techniques/T1071/004/). [Fifield et al., PETS 2015](https://www.petsymposium.org/popets/2015/popets-2015-0009.php).
