# EXP-1 scoring sheet

What `EXP-1` scores each candidate on (`DEC-6`): the host-contract suite (`T-005-03`), the gap list of
`research/2026-11-02-vos.md`, and the licence question. One row per criterion. Every result cell is empty until a run
fills it, and a filled cell always has its run id beside it (Narthex `REQ-EXP-4`, Narthex `REQ-CHN-17`). Run ids and their
notes follow [the record's rule](README.md#runs). The candidates' codes are in [the record](README.md#candidates).

A result is `pass`, `fail`, or a measured value with its unit. In the `vos` column, `pass` means VOS's own code or
docs close the gap. `pass (adapter)` means the gap is open in VOS and is closed by the adapter's own code, the
journal actor or the host adapter of `T-007-03`, for rutent's use through `Host`. It is never a VOS pass. An empty
cell means not yet run. A cell that cannot
apply to a candidate is filled by the run that finds it so, with that run's id and the reason in its note; nothing is
marked as not applying in advance.

| Kind | Rows | Source |
|---|---|---|
| Guarantee | `H-01` to `H-17` | `GUARANTEES` in `contracts/src/state_machine/host/mod.rs`, host contract 0.1.0 |
| Gap | `G-01` to `G-53` | `research/2026-11-02-vos.md` at VOS commit `1aadf44fa2cb1eaedc015ee82343ac118cc89f1e` |
| Licence | `L-01` to `L-04` | `DEC-6`, `REQ-MOD-5`, `licences/RECORD` |

## 1. The host-contract suite

One row per guarantee and fault, as `GUARANTEES` states them at host contract 0.1.0. `T-005-03` was not finished when
this sheet opened: when it lands, each per-fault cell it reports is mapped onto one of these rows, and a cell with no
row gets a new row here. The swap rows run between two implementations; the run note names the other one and the
direction.

| ID | Guarantee | Fault or check | Passes when | `ref` | `ref` run | `pb` | `pb` run | `vos` | `vos` run |
|---|---|---|---|---|---|---|---|---|---|
| `H-01` | `durable` | Stop after received; reopen the store | The journals equal the records acknowledged before the stop, each entry whole | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-02` | `durable` | Stop after computed; reopen the store | As `H-01` | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-03` | `durable` | Stop after durable; reopen the store | The journals hold the acknowledged records and the one in flight, unacknowledged; submitting it again returns it as already recorded | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-04` | `durable` | Halt, stop, reopen | The host reopens halted, and a submit is refused with `Halted` | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-05` | `exactly-once` | Submit an input, then the same input unchanged | One record; the second submit returns it as already recorded and records nothing; no state moves twice | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-06` | `exactly-once` | Repeat a delivery after a stop at each crash point, and reopen | One record per record id; no state moves twice | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-07` | `exactly-once` | Repeat, on the new host, a delivery from before a swap | Recognised as already recorded; one record | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-08` | `exactly-once` | Reuse a record id with other content, the same input stamped with another `recorded_at` included | Refused with `RecordIdReused`; the refusal is in the refusal log, and still there after a stop and reopen | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-09` | `per-order-sequence` | Deliver one order's inputs out of their recorded order | The journal runs from 1 with no gap, in submit order, each state following from the one before | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-10` | `no-effect-without-record` | Stop after received; reopen | No record, no state change, nothing released; submitting again applies the input once | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-11` | `no-effect-without-record` | Stop after computed; reopen | As `H-10` | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-12` | `no-effect-without-record` | Stop after durable; reopen | The record is present and unacknowledged; submitting again releases it once, as already recorded | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-13` | `swap` | Halt mid-order, export, import into the other implementation on an empty store, resume | Every order's state and journal, the refusal log and the record ids seen are equal; the same further inputs reach the same states; the order runs to its end | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-14` | `swap` | Submit while halted | Refused with `Halted`; nothing half applied | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-15` | `swap` | Import a stream naming a version the host can't read | The import is refused | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-16` | `swap` | Stop the new host part-way through an import | Reopening refuses the store with `ImportIncomplete`; the import redone on a fresh store succeeds | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |
| `H-17` | `replay` | Fold `open` and `transition` over each journal of each listed order, in sequence order, through the state machine alone | Every recorded state is reached; `orders` lists every order with a record | | | | | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze |

`T-005-03`'s named tests fall on these rows: `crash between record and effect leaves no effect` on `H-10` to `H-12`;
`repeated input moves nothing twice` on `H-05` to `H-08`; `swap mid-order loses no order` on `H-13` to `H-16`;
`replay reaches the same states` on `H-17`.

## 2. The VOS gap list

Every gap, constraint and absence `research/2026-11-02-vos.md` states against VOS at the pinned commit, restated as
a criterion that a run can check. Each is written as what would close the gap, so `pass` means the gap is closed.

- **Source** is the report's section and its own evidence tag: `Code` read in source, `Doc` the repository's
  documents, `Inference` the report's reasoning, `Not in code` searched for and absent. Paths in "How to check" are
  the report's citations, at the pinned commit.
- **Re-checked** says whether the orchestrator's review recorded in the report's Basis checked the claim directly
  at the pinned commit. `T-007-01`, which opened this sheet, read no clone. `T-007-03`'s development run re-checked
  every row it filled against a clone at the pinned commit; its note gives the evidence.
- **Bears on** names the host guarantee a gap touches directly, or else what it bears on outside the host contract.
  A gap that bears on the module format or its sandbox is scored here because `DEC-6` names the whole list; what it
  means for the format is `EXP-2`'s.
- The `pb` columns score the purpose-built store where a criterion is a property any runtime can have.

### 2.1 Log, audit and history (`PS-21`)

| ID | Criterion | How to check | Source | Re-checked | Bears on | `vos` | `vos` run | `pb` | `pb` run |
|---|---|---|---|---|---|---|---|---|---|
| `G-01` | Every journal stays complete; nothing is compacted away | Read `node/host/src/node/config.rs:211-217`; run `H-17` on orders whose records pass 1,024 entries and 64 MiB, the report's compaction defaults | §3 `PS-21`, `Code` | Yes | `replay`, `durable` | pass (adapter) | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-02` | The store holds every input whole, not only its result | Read `node/host/src/leader/execute.rs:144-150` and `node/host/src/store/entry.rs:18-29`; after a stop and reopen, read each journal entry's input back and compare it, as canonical JSON, with what was submitted | §3 `PS-21`, `Code`, `Doc` | No | `replay`, `durable`, `exactly-once` | pass (adapter) | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-03` | The caller of a recorded input stays known after its session expires | Read `agent/wire/src/defaults.rs:6` and `agent/wire/src/ids.rs:40-48`; submit, let the session expire, read the journal: each input's source is intact | §3 `PS-21`, `Code`, `Doc` | No | `replay`, `REQ-AUD-1` | pass (adapter) | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-04` | A client's signature is kept with what it signed | Read `agent/wire/src/batch.rs:219-226` and `node/host/src/calls.rs:298-306`: a field holds the signature past ingress | §3 `PS-21`, `Code` | Yes (the `Request` fields) | `REQ-AUD-1`, `PS-21` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-05` | Each operator command's log entry names its operator | Read `agent/wire/src/batch.rs:351-382`, `agent/wire/src/result.rs:282` and `node/host/src/api/operator.rs:102-118`, `:259-289`; install a module or grant a role, then find the signer's key in the log | §3 `PS-21`, `Code`, `Doc` | No | `REQ-AUD-1`, `PS-21` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-06` | A module's log output reaches a sink | Read `agent/default/src/refine/invocation.rs:403-406`; log from a guest and find the line outside it | §3 `PS-21`, `Code` | No | `PS-21` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-07` | The API can read history, enough to serve `journal`, `refusals`, `orders` and `export` from the runtime | Read `node/api/src/lib.rs:54-74`; the adapter serves those four from the runtime's own reads | §3 `PS-21`, `Code` | No | `replay`, `swap` | pass (adapter) | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-08` | The runtime keeps each input's recorded time as submitted and orders nothing by its own clock | Read `docs/design/core-model.md:653-654`; `H-05`, `H-08` and `H-09` pass with `recorded_at` unchanged in every journal entry | §3 `PS-21`, `Doc` | No | `exactly-once`, `per-order-sequence` | pass (adapter) | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-09` | A replica verifies what it applies, so an operator cannot log an origin it never checked | Read `node/SECURITY.md:19-21` and `docs/design/core-model.md:112`, `:172` | §3 `PS-21`, `Doc` | No | `PS-21`, repudiation threats | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-10` | Evidence is retained apart from the log (the non-repudiation side store) | Read `node/SECURITY.md:21-22`; find the store in code | §3 `PS-21`, `Doc` | Yes (the non-repudiation line) | `PS-21` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-11` | Every table has tamper evidence, without opting in, including in a quiet space | Read `docs/design/core-model.md:1070-1071` and `docs/operations.md:108-112` | §3 `PS-21`, `Doc` | No | `PS-21` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-12` | A production build can anchor and submit to a venue | Read `node/venue/src/lib.rs:7-13` and `justfile:451-457`; build the production profile and find a venue client | §3 `PS-21`, `Code` | No | `PS-21`, `PS-12` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-13` | The proof stack's security bounds are externally reviewed, and the end-to-end bound is stated | Read `pvm/proof/SECURITY.md:38-39`, `:58-60` | §3 `PS-21`, `Doc` | No | `PS-21` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-14` | Admitting a trusted program id does not need an audit of each kernel | Read `pvm/proof/SECURITY.md:151` | §3 `PS-21`, `Doc` | No | `PS-21` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |

### 2.2 Loading and replacing modules (`PS-18`)

| ID | Criterion | How to check | Source | Re-checked | Bears on | `vos` | `vos` run | `pb` | `pb` run |
|---|---|---|---|---|---|---|---|---|---|
| `G-15` | Installing over an existing instance checks the old and new schema and state format | Read `agent/default/src/refine/admin.rs:55-58`, `:194-219` and `node/host/src/node/admin.rs:338`; install a program with another state format over a running one | §3 `PS-18`, `Code`, `Doc` | No | `PS-18`, module lifecycle | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-16` | A module can be uninstalled or retired as a platform operation | Read `agent/wire/src/batch.rs:351-382` for an `Admin` variant that removes one | §3 `PS-18`, `Code` | No | `PS-18`, module lifecycle | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-17` | The previous install record is kept, so version history is in state | Read `agent/default/src/refine/admin.rs:55-58`; install twice and read both records | §3 `PS-18`, `Code` | No | `PS-18`, `PS-21` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-18` | Canary, staged rollout and rollback are provided | Search the pinned tree for them | §3 `PS-18`, `Not in code` | No | `PS-18` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-19` | An install does not stall other instances in the agent, or stalls them for a measured time | Install while another instance takes submits; record the stall in the run's note | §7, not determined | No | `PS-18` | | | | |

### 2.3 Modules, languages and the sandbox (`PS-20`)

| ID | Criterion | How to check | Source | Re-checked | Bears on | `vos` | `vos` run | `pb` | `pb` run |
|---|---|---|---|---|---|---|---|---|---|
| `G-20` | The platform checks a publisher's signature on a module | Read `agent/wire/src/ingress.rs:138-142` and `node/host/src/api/operator.rs:217-258`; search `.rs` files for the check | §1, §3 `PS-20`, `Not in code` | Yes (the install record; no "vendor") | Module admission, `EXP-2` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-21` | A publisher can upload a module by a route other than the operator's install | Read `node/api/src/lib.rs:54-74`; search for a publisher role, registry or catalogue operation | §3 `PS-20`, `Not in code` | No | `REQ-CAT-*`, `EXP-2` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-22` | A guest SDK exists in a language other than Rust | Read `agent/sdk/` and `pvm/guest/README.md:3-6`; search for a non-Rust guest | §3 `PS-20`, `Code` | No | `AC-20`, `EXP-2` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-23` | A garbage-collected or interpreted guest fits the 512 KiB handler heap and the default 10M gas | Build such a guest and run one handler; record heap and gas | §3 `PS-20`, `Inference`, untested | No | `AC-20`, `EXP-2` | | | | |
| `G-24` | A module can reach its provider from inside the sandbox | Read `agent/wire/src/abi/surface.rs:57-59` and `agent/wire/src/abi.rs:203-313`; search `Cargo.lock` for an HTTP client | §3 `PS-20`, `Code` | Yes (no HTTP clients) | ADR-0004, `EXP-2` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-25` | An inbound webhook can reach a module | Read `node/http/src/routes.rs:115-118` | §3 `PS-20`, `Code`, `Inference` | No | ADR-0004, `EXP-2` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-26` | Provider credentials and module state are not readable by a space's operators | Read `docs/design/identity-and-authority.md:56` and `docs/design/core-model.md:117`, `:319`; search the node and agent paths for encryption | §3 `PS-20`, `Doc`, `Code` | Yes (the operator-visibility line) | ADR-0004, `THR-79` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-27` | Guest execution sits behind a second layer, such as seccomp or landlock | Search `pvm` and `node` for one | §3 `PS-20`, `Not in code` | Yes | `THR-85`, `EXP-2` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-28` | The JIT and the sandbox's containment have fuzz targets | List `pvm/proof/fuzz` and search for a target over the JIT, `derive` or containment | §2, §3 `PS-20`, `Code` | No | `THR-85`, `EXP-2` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-29` | An external audit names the runtime as a sandbox in its scope | Read `pvm/proof/SECURITY.md:469-490` | §3 `PS-20`, `Doc` | No | `THR-85`, `EXP-2` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-30` | Dynamic dispatch is safe in proved code | Read `pvm/compiler/src/derive/mod.rs:20-28` | §3 `PS-20`, `Code` | No | `EXP-2` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |

### 2.4 State, identity and limits (`PS-19`)

| ID | Criterion | How to check | Source | Re-checked | Bears on | `vos` | `vos` run | `pb` | `pb` run |
|---|---|---|---|---|---|---|---|---|---|
| `G-31` | The host can key records on rutent's record id, whatever identifiers the runtime exposes to guests | Read `agent/sdk/src/context.rs:137-199`; `H-05` to `H-08` pass with the record id passed in the arguments | §3 `PS-19`, `Code`, `Inference` | No | `exactly-once` | pass (adapter) | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-32` | A transition record, its input whole and the state it reached, fits the runtime's caps: key 1 KiB, value 64 KiB, request and reply 64 KiB, 256 KiB of writes a step | Read `agent/wire/src/abi/cap.rs:13`, `:17-18`; submit the largest input fixture against an order with the most parts and segments the fixtures hold, and record the largest record's size | §3 `PS-19`, §4, `Code` | No | `durable`, `replay` | pass: key 8 of 1,024 bytes, value about 3,663 of 65,536, request about 3,695 of 65,536 | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-33` | Storage has a quota per instance, or the journals' growth is bounded by something stated | Search `agent/wire`, `agent/default` and `node/host` for a quota | §3 `PS-19`, `Not in code` | No | `replay` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-34` | An order's lifetime is not capped by the runtime: no cap of 65,536 unended workflows, no result dropped an hour after ending | Read `agent/wire/src/defaults.rs:232`, `:235`; applies if the adapter maps orders to workflows | §3 `PS-19`, `Code` | No | `replay`, `REQ-ORD-*` | does not apply | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-35` | One record, or one refusal, is written atomically | Read `docs/design/core-model.md:505-510`; `H-01` to `H-03` find every entry whole | §3 `PS-19`, `Doc` | No | `durable` | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |

### 2.5 Deployment and operation (`PS-12`)

| ID | Criterion | How to check | Source | Re-checked | Bears on | `vos` | `vos` run | `pb` | `pb` run |
|---|---|---|---|---|---|---|---|---|---|
| `G-36` | The runtime meets its own invocation budget on the target platform, without needing Linux x86_64 on kernel 6.7 or later for its JIT | Read `pvm/runtime/src/backend.rs:103-118`; record the backend used and the invocation time in the run's note | §3 `PS-12`, `Code`, `Doc` | No | `PS-12`, operator obligations | fail: interpreter, refine 717 µs mean, unloaded | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-37` | It builds with a stable toolchain and portable codegen | Read `rust-toolchain.toml` and `.cargo/config.toml`; build on the run's machine and record the toolchain | §3 `PS-12`, `Code` | No | `PS-12`, `NFR-7` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-38` | A node's key can be rotated | Read `docs/operations.md:32-34`, `:50-65` | §3 `PS-12`, `Doc` | No | `PS-12`, operator obligations | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-39` | A store restored from a backup, or a copy of one, can run without counting as a compromised node | Read `docs/operations.md:67-72`, `:132-135`; `H-01` to `H-04` reopen the same store, and `H-13` imports onto a fresh one | §3 `PS-12`, `Doc` | No | `swap`, `durable` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-40` | A store written by one version of the runtime opens in the next, or has a documented upgrade path | Read `docs/design/core-model.md:928-934` | §3 `PS-12`, §5, `Doc` | No | `swap`, `NFR-6`, `REQ-OPS-3` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-41` | The runtime exports metrics beyond logs and its operator reads | Search `node` and `agent` for a metrics exporter | §3 `PS-12`, `Code` | No | `PS-12`, `STEP-5` signals | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |

### 2.6 Open protocol (`PS-23`)

| ID | Criterion | How to check | Source | Re-checked | Bears on | `vos` | `vos` run | `pb` | `pb` run |
|---|---|---|---|---|---|---|---|---|---|
| `G-42` | Throughput and p99 latency measured beyond one host sharing one disk | Read `docs/design/research/slice-7/scale-report.md:18`; record what a run measures in its note, as a measurement and not a target (`PLAN.md` §6.2, risk 9) | §3 `PS-23`, `Doc` | Yes (the scale-report line) | `NFR-1` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-43` | It tolerates Byzantine faults among the parties of one deployment | Read `docs/design/core-model.md:112` | §3 `PS-23`, `Doc` | No | `PS-23` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-44` | Parties in separate deployments can connect, by a bridge in code | Read `docs/design/clerk-across-spaces.md:1-4`; search `agent` and `node` for bridge code | §3 `PS-23`, `Doc`, `Not in code` | No | `PS-23` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |

### 2.7 Maturity and supply (§1, §2, §5, §7)

| ID | Criterion | How to check | Source | Re-checked | Bears on | `vos` | `vos` run | `pb` | `pb` run |
|---|---|---|---|---|---|---|---|---|---|
| `G-45` | It builds, and its own tests pass, at the pinned commit | Build and run its test suite at the pin; record the counts | §7, not determined | No | Every row above | 455 passed, 0 failed, 36 not finished, in 6 of 38 crates | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-46` | A tagged release exists to pin to | List its tags and releases | §2, `Code` | No | `REQ-MOD-5`, supply | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-47` | More than one maintainer writes and reviews it | Count authors in its history; find a reviewer other than its owner | §2, §5, `Code`, `Doc` | Yes (the commit counts) | Supply risk | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-48` | It depends on no fork held under a personal account, and on no unreleased git dependency | Read `Cargo.toml:108`, `:241`, `:258-259` and `node/gateway/Cargo.toml:35` | §5, `Code` | No | Supply risk, `REQ-MOD-5` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-49` | CI runs its checks | Search the tree for CI configuration; read `.githooks/pre-push` and `justfile:669` | §2, `Code` | No | Supply risk | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-50` | The critical gap a design review recorded in `cipher-clerk`'s currency check is fixed at the version the node depends on | Read `docs/design/identity-and-authority.md:1027-1029` and `Cargo.toml:241`; check that version's source | §5, `Doc`, not determined | No | Supply risk | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-51` | Its formats are stable: a segment-format change does not change every program id, and a new store layout migrates instead of being refused | Read `pvm/proof/SECURITY.md:173` and `docs/design/core-model.md:928-934` | §5, `Doc` | No | `swap`, coupling | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-52` | Each feature the adapter relies on is in the code at the pin, not only in the docs or on `master` | For each VOS feature `T-007-03` uses, cite the code; check it against the report's §1 table of docs that differ from code | §1, `Code`, `Not in code` | No | Every row above | pass | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `G-53` | The external audit required before value rides on a proof is done | Read `pvm/proof/SECURITY.md:469-471` | §2, `Doc` | No | `PS-21`, supply risk | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |

## 3. The licence question

`DEC-6`: VOS is AGPL-3.0-or-later. Adopting it needs the owner's licence decision recorded in `licences/RECORD`,
as `REQ-MOD-5` requires of every dependency, case by case, with no counsel step (`counsel-licences`). The
licence-record check (`T-001-04`) keeps a dependency with no recorded decision off `main`. This experiment scores
what the decision is about; the decision itself is the owner's, and reaches the owner's form with the adoption item
at `T-007-04`.

| ID | Criterion | How to check | `vos` | `vos` run | `pb` | `pb` run |
|---|---|---|---|---|---|---|
| `L-01` | The owner's decision on VOS's AGPL-3.0-or-later licence is recorded before any VOS code reaches `main` | `licences/RECORD`'s `vos` line holds a locked version and a decision other than `pending`; the licence check passes strictly. At the opening it reads `vos not-adopted AGPL-3.0-or-later pending` | fail | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `L-02` | Each VOS crate the adapter brings into `Cargo.lock` is listed with its own licence: the AGPL-3.0-or-later workspace crates, such as `vos-sdk`, `vos-wire` and `vos-raft`, apart from the Apache-2.0 PVM crates | Compare `Cargo.lock` with the flag on and off; check each new package's licence against the report's §2 and the crate's own manifest | fail: 25 VOS crates, 11 AGPL-3.0-or-later, 14 Apache-2.0, none listed | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `L-03` | Each of VOS's own dependencies that enters `Cargo.lock`, forks and git dependencies included, has a line in `licences/RECORD` | Run the licence check with the flag on | fail: 222 of 240 lack a line | `EXP-1-vos-dev-20261106-1`, pre-freeze | | |
| `L-04` | Each dependency the purpose-built store brings has a line in `licences/RECORD` with the owner's decision | Run the licence check with the flag on | | | | |

## 4. Pre-freeze development runs (not scored)

Runs made before the contract freeze `C1` (`T-006-01`), against the host contract as it stood, to develop a
candidate. Each has a development id, `EXP-1-<candidate code>-dev-<yyyymmdd>-<n>` (README, Runs), so it can't be
taken for a scored run, and its note is in `runs/` like any other. A development run may fill cells above, each
marked `pre-freeze`; the scored run after `C1` replaces every such cell, and no outcome rests on one. This section
lists every development run, whether or not it filled cells.

| Run | Candidate | Host contract | What ran | Result | Note |
|---|---|---|---|---|---|
| `EXP-1-pb-dev-20261106-1` | `pb` | 0.1.0, before `C1` | The host-contract suite with the fault seam, in three swap directions (`pb` to `ref`, `pb` to `pb`, `ref` to `pb`); a real SIGKILL of a writer process, twelve times; a log torn mid-record, seven ways; damage past one append, four ways; corruption inside synced history and inside an import, finished and unfinished; an import stopped right after its first frame, and that frame torn; another store format or contract version; the lock held from an import to its open; a failed write, a failed sync and a failed undo; one host per store | Every suite cell passes in each direction (16 of 16, three times); every test beyond the suite passes | [`runs/EXP-1-pb-dev-20261106-1.md`](runs/EXP-1-pb-dev-20261106-1.md) |
| `EXP-1-vos-dev-20261106-1` | `vos` | 0.1.0, before `C1` | The host-contract suite with the fault seam: VOS to the reference host, the reference host to VOS, and VOS with its log forced to compact; a control pass without VOS's commit hold; the gap list read and run against VOS at the report's pin | All 17 host rows pass; gap rows 39 fail, 4 pass, 6 pass (adapter), 1 n/a, 1 measured, 2 not run; licence rows fail, as facts | [`runs/EXP-1-vos-dev-20261106-1.md`](runs/EXP-1-vos-dev-20261106-1.md) |
