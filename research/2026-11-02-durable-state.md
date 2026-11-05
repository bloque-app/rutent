---
name: Durable execution, multi-party operations and provable history
status: informative
date: 2026-11-02
---

# Durable execution, multi-party operations and provable history

## Basis

**Read on 2026-11-02, inside this project:** `CLAUDE.md`, `PRODUCT.md` (`PS-10`, `PS-11`, `PS-12`, `PS-19`, `PS-21`), `SPEC.md` (skeleton, `REQ-STM-*`, `REQ-AUD-*`, section 5, `OQ-9`).

**Read on 2026-11-02, outside this project:** only public external sources, all listed under References with the version and date each one shows. Nothing from the directories the brief excludes was opened, and no production system or admin tool was queried.

**How sources were read.** Web pages went through a fetch tool that returns a model-written digest of the page, so a digest can misread its page. PDFs were read directly as pages or extracted text. A claim marked *(search excerpt)* comes only from a search result's excerpt, and its page was not opened. Treat those as leads. Direct quotation is kept to one phrase; everything else is paraphrase.

**Tags.**
- **Source:** the cited document says this.
- **Measured:** the cited document reports this as a measurement, under the conditions given.
- **Inference:** my reasoning from sources. It is not stated by any of them.

This report decides nothing. It names no winner and writes no requirement.

**Orchestrator's review, 2026-11-02.** Checked against the sources: the impossibility result (van Glabbeek et al.), the Jepsen Conductor findings and figures, RFC 9943's status and section 9.2, and Temporal's activity guarantee. Those matched, except that this report first said the eventual-termination variant needs partial synchrony, where the paper's abstract says without synchrony; corrected above. Everything else is unchecked and rests on this report's own tags. Tamper-evidence, licences and scale figures need checking again before `SPEC.md` or a suggestion relies on one.

## Findings that matter most

1. **Exactly-once claims stop at the engine's own record.** The engines checked protect the recorded result of a step, not the external effect. The two narrower exceptions do not change this for a third party: DBOS's exactly-once covers only database work in its own transaction, and Step Functions Standard's holds only without `Retry`, which leaves a lost response unresolved. Stopping a third-party provider from acting twice depends on that provider honouring a stable caller identifier. So `REQ-STM-*` depends on what `REQ-CAT-*` and `REQ-MDL-*` can demand of a provider. (Inference, from section 1.)
2. **Atomicity across independent providers is not available, and compensation is not unilateral on real rails.** A time-bounded cross-party payment is proven impossible without synchrony, and that proof already assumes escrows the parties can use. On Pix and SEPA a return can be refused, and on Pix a refusal for lack of balance can end in court. (Source, section 2.)
3. **A log proves what was recorded, not that everything was recorded or that it is true.** Tamper-evidence needs someone other than the operator checking. Engine histories are designed for recovery rather than as evidence. (Source and Inference, section 3.)

## 1. Landscape

### Comparison

Footprint and "embeddable" describe what the application must run, as the docs describe it. "Independent test" means a Jepsen analysis on the Jepsen index read on 2026-11-02; both analyses cited here were funded by the vendor under test. No other independent analysis of these engines turned up, though the search for one was not exhaustive.

| System | Model | What the repeat-protection claim covers | Footprint | Licence | Embeddable as a library | Languages | Independent test | Version or date seen |
|---|---|---|---|---|---|---|---|---|
| Temporal | Code workflows, replayed from an event history | Workflow runs effectively once. Activity is observed complete once but may run several times | Four server roles plus a database; visibility store optional | MIT | No: server plus SDK workers | Go, Java, Python, TypeScript, .NET, Ruby, PHP, Rust | None listed | Docs undated; licence file 2025 |
| Cadence | Code workflows; Temporal's precursor (Source: Temporal case study) | Not stated on the page read | Server plus Cassandra, MySQL 8 or Postgres; SQLite for local use only | Apache 2.0 | Not established; docs describe workers and a server | Go, Java, Python official; Ruby community; .NET in development | None listed | Docs undated |
| Restate | Handlers with a journal; server proxies calls | Invocation runs once per the docs; a 2023 company post says side effects may repeat in the failure window | One server binary, single node or cluster | BUSL 1.1; no public hosted platform service; becomes Apache 2.0 four years after release | No: server required | Go, Python, Java/Kotlin, JS/TS, Rust | None listed | 1.2 post 2025-02-18 |
| AWS Step Functions | Declarative state machines (Amazon States Language) | Standard: tasks not run twice unless `Retry` is set. Express: at-least-once or at-most-once, no state kept between transitions | Managed AWS service | Proprietary service (Inference) | No | Any, by API (Inference) | None listed | Docs read 2026-11-02 |
| DBOS Transact | Library; workflows checkpointed to Postgres | Exactly once only for steps whose work is database operations in the checkpoint transaction; otherwise at least once | Postgres only; optional control plane outside the execution path | MIT (Python repo) | **Yes** | Python, TypeScript, Go, Java; one language per application | None listed | Docs 2026; post 2025-08-07 |
| Camunda 8 / Zeebe | BPMN processes on a partitioned, replicated broker | At-least-once job delivery; worker must be idempotent | Gateways, brokers, exporters | Source under Camunda License v1; production needs a paid edition from 8.6 | No | Java, TypeScript, Python, C#; Rust, Go, PHP in preview | None listed | 8.10 docs |
| Conductor | JSON workflows, server with workers | At-least-once delivery (Source: Jepsen, on the Orkes build) | Java server plus database and queue, for example Postgres and Redis | Apache 2.0 (conductor-oss) | No | Java, Python, JS, Go, C#; Ruby, Rust incubating | **Yes: Orkes 5.3.2, 2026-10-20** | Repo read 2026-11-02 |
| Azure Durable Functions / Durable Task | Code orchestrators, event-sourced and replayed | Not stated as a delivery guarantee on the page read | Host plus a storage provider or managed scheduler | MIT (`durabletask-dotnet` SDK); other components not checked | SDK is a library but needs a sidecar or managed backend | C#, JavaScript, Python, Java, PowerShell | None listed | ms.date 2026-04-22 |
| XState | In-process state machines, statecharts, actors | No durability claim; the caller stores snapshots | None; zero dependencies | MIT | **Yes** | JavaScript, TypeScript (Inference) | None listed | v5 announced 2023-12-01 |
| SCXML | XML notation for statecharts | Not an executable system; no persistence in the part read | None; a notation | W3C Recommendation | Not applicable | Neutral | Not applicable | 2015-09-01 |
| Open Workflow Specification (site redirected from Serverless Workflow) | Workflow DSL with conformance kit | Depends on the runtime | Depends on the runtime; six listed | Not checked | Not applicable | Neutral, with SDKs | Not applicable | 1.0.3 |
| Event sourcing | Pattern: state as an ordered sequence of events | None by itself | Depends on the store | Depends on the store; see KurrentDB below | Pattern | Any | Not applicable | Fowler 2005 |

The next system is adjacent, because it is a ledger database and not a workflow engine:

| System | Model | Claim | Footprint | Licence | Embeddable | Independent test |
|---|---|---|---|---|---|---|
| TigerBeetle | Accounting database with reserve, post and void transfers | Strong serializability claimed; requests execute at most once | Replicated server cluster | Apache 2.0 | No | **Yes: 0.16.11, 2025-06-06**, funded by the vendor |

### What the claims really say

**Temporal.**
- Source: a Workflow Execution runs effectively once and to completion, and resumes from recorded history ([execution docs](https://docs.temporal.io/workflow-execution)).
- Source: Temporal guarantees an Activity will be "observed as completed exactly once", yet the Activity may execute several times. Idempotency keys are enforced by the service being called, not by the Activity ([activity definition](https://docs.temporal.io/encyclopedia/activities/activity-definition)).
- Source: the default retry policy in the Python SDK has unlimited attempts *(search excerpt)* ([error handling](https://docs.temporal.io/develop/python/error-handling)).
- Source: production persistence is Cassandra, PostgreSQL or MySQL; SQLite is for development and testing ([persistence](https://docs.temporal.io/temporal-service/persistence)).
- Source: a history is limited to 51,200 events or 50 MB, with a warning at 10,240 events or 10 MB ([limits](https://docs.temporal.io/workflow-execution/limits)).
- Source: when changed code replays an older history and produces different commands, replay cannot continue. The docs offer patching and Worker Versioning *(search excerpt)* ([safe deployments](https://docs.temporal.io/develop/safe-deployments)).

**Restate.**
- Source: the docs say a tracked invocation runs exactly once regardless of failures, and that a duplicate request with an idempotency key gets the original result ([key concepts](https://docs.restate.dev/foundations/key-concepts)).
- Source: a company post dated 2023-08-24 is more careful. Side effects may run more than once if a failure comes before they finish or before the result is durable; by the time the result is used, it is durable and deterministic ([why we built Restate](https://restate.dev/blog/why-we-built-restate)).
- Inference: read the docs' wording as describing the recorded result, and the post as describing the external effect. I found no current doc that defines this window.
- Source: the licence bars running a public service on which third parties register and invoke their own services ([LICENSE](https://raw.githubusercontent.com/restatedev/restate/main/LICENSE)).

**AWS Step Functions.**
- Source: Standard workflows never run a task or state twice unless `Retry` is set, and AWS names processing payments as a fit. Asynchronous Express is at-least-once and synchronous Express is at-most-once; neither persists state between transitions ([workflow types](https://docs.aws.amazon.com/step-functions/latest/dg/concepts-standard-vs-express.html)).
- Source: Standard history is capped at 25,000 events, and reaching the cap fails the execution. History is removed 90 days after an execution closes, and that can be reduced to 30 days but the page offers no longer period. Executions run for at most one year. An HTTP Task has a hard 60-second limit ([quotas](https://docs.aws.amazon.com/step-functions/latest/dg/service-quotas.html)).
- Inference: the page does not say what a task's exactly-once means when the remote side acted and the response was lost. A 60-second limit makes that case easy to reach.

**DBOS.**
- Source: the library has no separate orchestration server and needs only Postgres. Once a step is checkpointed it is not re-run, and steps should be idempotent because they can be retried ([architecture](https://docs.dbos.dev/architecture)).
- Source: exactly-once holds only for steps doing database operations, because the step and its checkpoint share a transaction. In general a step may run twice if failure comes between execution and checkpoint ([2025-08-07 post](https://www.dbos.dev/blog/why-postgres-durable-execution)).

**Camunda 8 / Zeebe.**
- Source: job delivery is at-least-once and worker code must be idempotent ([job workers](https://docs.camunda.io/docs/components/concepts/job-workers/)).
- Source: a job may be given to another worker on timeout, after side effects have happened; the guide warns that non-idempotent handlers can duplicate payments *(search excerpt)* ([writing good workers](https://docs.camunda.io/docs/components/best-practices/development/writing-good-workers/)).
- Source: from 8.6 the source is under Camunda License v1 and production use requires the Enterprise Edition ([licences](https://docs.camunda.io/docs/reference/licenses/)).

**Conductor, the only engine on this list with a Jepsen report.** Orkes Conductor 5.3.2 to 5.5.0-rc2, published 2026-10-20, funded by Orkes ([Jepsen](https://jepsen.io/analyses/orkes-conductor-5.3.2)). All Source:
- Tasks ran concurrently on several workers, in healthy clusters. Locks reduce this but cannot prevent it.
- Workflows were abandoned in a running state forever (serious). A database write succeeded, the enqueue failed, and the sweeper looked only at the queue.
- Acknowledged workflows vanished permanently (serious). After a failed enqueue the server deleted the record it had written.
- Never-run workflows were marked completed (serious). Two non-transactional writes were separated by a failure.
- In one short run, 1,752 of 25,920 calls that reported a definite connection failure had in fact created workflows. The client had retried internally.
- Fixes for all serious safety issues shipped in 5.5.6; two minor client-visible issues remain under investigation.
- Not tested: multi-step workflows, failure workflows, clock skew, high latency. Jepsen can show bugs but not prove their absence.

I could not determine how much code the tested Orkes build shares with the conductor-oss repository.

**Azure Durable Task.**
- Source: history is an append-only record of orchestrator actions and orchestrator code must be deterministic. Azure Storage gives no transactional guarantee between its tables and queues, so that provider relies on eventual consistency patterns. Microsoft also warns the history table format may change and calls it useful for debugging ([orchestrations](https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-orchestrations)).

**XState.**
- Source: the caller stores a snapshot and restores it. Actions are not re-run on restore, because they are assumed to have run; invocations restart. A snapshot may become incompatible if the logic changes. The page makes no durability guarantee and points to event sourcing for replaying actions ([persistence](https://stately.ai/docs/persistence)).

**SCXML and Open Workflow Specification.**
- Source: SCXML is a notation for state machines whose data model is local to a session; no persistence appears in the first 100,000 of its 228,000 characters, which is what I read ([W3C](https://www.w3.org/TR/scxml/)).
- Source: Open Workflow Specification is a vendor-neutral DSL, currently 1.0.3, with six listed runtimes ([site](https://open-workflow-specification.org/)). Its compensation rules were seen only in the 0.6 text *(search excerpt)* ([0.6](https://github.com/serverlessworkflow/specification/blob/0.6.x/specification.md)).

**Event sourcing.**
- Source: every change is stored as an ordered event. Replaying events that call external systems goes wrong, because those systems cannot tell replay from real processing. Replay of external queries needs the original responses to be stored ([Fowler, 2005](https://martinfowler.com/eaaDev/EventSourcing.html)).
- Source: KurrentDB, one dedicated store, moved to Kurrent License v1 at 25.0 *(search excerpt)* ([release](https://www.kurrent.io/releases/kurrentdb/25-0/)). From 26.2 running across more than one node needs an Enterprise licence ([2026-08-13](https://kurrentdb.kurrent.io/blog/licensing-in-kurrentdb-v26-2-and-beyond-what-s-free-and-what-s-licensed)).

### Use for moving money

Each of these is published by the engine vendor. I found no independent account, and none gives failure rates.
- Source: a conference talk abstract says Temporal is in the path of all inbound and outbound money transfers at Cash App ([2024-10-28](https://temporal.io/resources/on-demand/great-tech-is-not-enough)).
- Source: Coinbase replaced an in-house saga engine with Temporal/Cadence for crypto transactions. It also evaluated Zeebe and Step Functions, and found predefining execution paths costly as saga complexity grew ([2024-08-29](https://temporal.io/resources/case-studies/coinbase)).

### Reading across the table

Inference. The owner asked whether a solid implementation exists so the wheel need not be reinvented. On what was checked:
- Engines solve a real and hard part: recording progress and resuming after a crash. Conductor's serious bugs were all in that part, which is a reason not to underrate it.
- No engine checked solves an unknown provider outcome. All hand it back to the step's author and the called service.
- No engine checked offers tamper-evidence of its history. None of the pages read claims it.
- `PS-10` asks for a state machine reusable as a standalone library. Among those checked, XState is a library without durability, and DBOS is a durable library tied to Postgres and to one language per application. Everything else requires running a server.

## 2. Multi-step operations across parties that cannot commit together

### What each pattern guarantees

| Pattern | Guarantees | Does not guarantee | Needs from the other party |
|---|---|---|---|
| Saga (Garcia-Molina and Salem, 1987) | Either all steps run, or some steps run and their compensations run in reverse order | Return to the original state; isolation, since sagas may see each other's partial results | A compensating action that exists and succeeds |
| Try-Confirm/Cancel, tentative operation | Nothing moves finally until confirmed; a hold can lapse | Agreement if timeouts differ; that confirm cannot fail | A reservation offered by the provider |
| Escrow, hold, two-phase transfer | Capacity is reserved up front, and resolution happens once | Anything beyond the system holding the funds | The provider or ledger actually holds the funds |
| Conditional transfer across ledgers (Interledger) | Sender gets a receipt or the funds back, per the 2016 paper | Time-bounded success without synchrony | Ledger escrow, or bilateral reconciliation between peers |
| Notary group (Interledger atomic mode) | Transfers execute or abort together if more than two-thirds of notaries are reliable | Anything once that threshold fails | Ledgers that obey the notaries |
| Forward recovery | Retries toward completion instead of undoing | Completion, if the provider stays down or refuses | Repeat-safe operations |

Sources:
- **Saga.** A compensating transaction undoes a step from a semantic point of view and need not restore the prior state. The guarantee is the sequence T1..Tn or T1..Tj, Cj..C1. Sagas may view each other's partial results ([Princeton TR CS-TR-070-87, pp. 2-4](https://www.cs.princeton.edu/techreports/1987/070.pdf)).
- **Compensation in practice.** Compensations can fail, must be repeatable, and may need manual intervention. Some steps are irreversible, so points of no return should be declared and irreversible steps should come after all critical validation ([Azure, ms.date 2026-04-16](https://learn.microsoft.com/en-us/azure/architecture/patterns/compensating-transaction)).
- **Isolation.** The lack of isolation can expose anomalies, and countermeasures need careful analysis ([microservices.io](https://microservices.io/patterns/data/saga.html)).
- **Tentative operation.** It uses explicit confirmation and implicit cancellation, usually by timeout, and whether confirm can fail is left open. Not all providers support it ([Enterprise Integration Patterns](https://www.enterpriseintegrationpatterns.com/patterns/conversation/TryConfirmCancel.html)).
- **TCC timeouts.** Some participants may confirm while the transaction times out for others ([Oracle MicroTx 24.4](https://docs.oracle.com/en/database/oracle/transaction-manager-for-microservices/24.4/tmmdg/transaction-timeout.html)).
- **Two-phase transfer.** A reservation is checked against balance limits when created, can be posted, voided or expired, and resolves once ([TigerBeetle docs](https://docs.tigerbeetle.com/coding/two-phase-transfers/)). Jepsen notes that its timeouts are not exactly deterministic ([Jepsen](https://jepsen.io/analyses/tigerbeetle-0.16.11)).
- **Interledger.** The 2016 paper describes both modes ([WWW 2016 companion](https://archives.iw3c2.org/www2016/WWW2016-Proceedings/companion/p281.pdf)). The two-thirds notary threshold is as van Glabbeek et al. report it in their related work ([arXiv 1912.04513](https://arxiv.org/pdf/1912.04513)); I did not read the original whitepaper. The current ILPv4 text does not assume ledgers enforce conditions, and peers use them to reconcile. It uses short timeouts partly to limit the free option problem ([ILPv4](https://interledger.org/developers/rfcs/interledger-protocol/)). A retrospective by a protocol author says atomic mode was deprioritised *(search excerpt)* ([Schwartz](https://emschwartz.me/simplifying-interledger-the-graveyard-of-possible-protocol-features/)).

### The limit, stated plainly

Source: van Glabbeek, Gramoli and Tholoniat (2019) prove that no algorithm solves the time-bounded cross-chain payment problem without synchrony, even if participants are correct or only crash. A variant that relaxes the problem to eventual termination is solvable without synchrony, even with Byzantine failures. They also cite the view that, when parties may be malicious, the classical notion of atomicity cannot be guaranteed, and the most one can ensure is that honest parties are not cheated ([arXiv 1912.04513](https://arxiv.org/pdf/1912.04513)).

Inference: that model assumes escrows customers and connectors can use. Third-party providers may expose no escrow at all, so rutent's position is likely weaker than the model's. This bears directly on `OQ-9`. "Atomic" needs a substitute meaning, and the candidates differ in what they promise:
- Value is always accounted for, even while a stage is in doubt.
- A party who followed the rules loses nothing.
- Every order ends completed or compensated within a bound, under stated assumptions about providers.
- Every order ends in a known state, with escalation to a person allowed.

### Compensation on real rails is a request

Source, SEPA ([EPC clarification paper v3.1, 2025-04-25, section 2.9](https://www.europeanpaymentscouncil.eu/sites/default/files/kb/file/2025-04/EPC131-17%20v3.1%20Clarification%20Paper%20SCT%20and%20SCT%20Inst%20scheme%20rulebooks.pdf)):
- A recall is limited to duplicates, technical problems and fraud. The window is 10 banking days for the first two and 13 months for fraud.
- The beneficiary's bank has 15 banking days to answer.
- The beneficiary can refuse. If the beneficiary does not respond, the bank must send a negative answer.

Source, Pix, per v4.3 of the [BCB refund guide](https://www.bcb.gov.br/content/estabilidadefinanceira/pix/Guia_MED.pdf), in Portuguese. Its cover says v4.4 takes effect in stages on 2026-09-01 and 2026-10-26, so v4.4 is now in force. I did not check it, and these points may have changed:
- A recipient may voluntarily return funds within 90 days.
- For an operational failure, the payer's institution can request a return, and the recipient's institution has 48 hours to answer on the merits.
- If a return is rejected for lack of balance, the recipient's institution need not use its own funds, and the guide recommends the payer's institution go to court.
- The special return mechanism for suspected fraud is explicitly not a chargeback.

I could not determine the equivalent rules for SPEI or Bre-B.

Source: in December 2021 a UK bank paid 75,000 payments twice, about USD 176 million, and had to work with other banks to recover funds. It attributed this to a scheduling issue *(search excerpt)* ([NBC News](https://www.nbcnews.com/business/business-news/bank-accidentally-deposits-176-million-peoples-accounts-christmas-day-rcna10538)). I found no published root-cause report.

### Known failure modes

**Unknown outcome.**
- Source: a timed-out call leaves the caller unsure whether the action happened, and forces costly reconciliation. AWS's preferred approach is a caller-provided request identifier with a semantically equivalent response for every repeat ([Builders' Library](https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/)).
- Source: Jepsen recommends treating all Conductor errors as possibly indefinite. Its TigerBeetle report recommends separate definite and indefinite error types, and notes requests that never time out turn definite errors into indefinite ones ([Conductor](https://jepsen.io/analyses/orkes-conductor-5.3.2), [TigerBeetle](https://jepsen.io/analyses/tigerbeetle-0.16.11)).
- Inference: each wrong guess has a price. Treating unknown as failed and retrying can move value twice. Treating it as failed and compensating can undo something that never happened. Treating it as success can release value that never arrived. Waiting has a price too, since funds stay held.
- Inference: what can settle an unknown outcome depends on the provider: a safe repeat under the same identifier, a status query, a later notification, or an independent statement. Without any of these only a person can settle it. A rail may define a formal inquiry; the SEPA paper's change log mentions an inquiry procedure, which I did not read in detail.
- Source, weak: practitioner and provider guides advise recording a pending state before the call and reconciling afterwards, and note some providers offer no idempotency and that keys are retained for a limited time *(search excerpts, not primary)* ([example](https://hackernoon.com/idempotency-key-misconceptions-it-isnt-an-idempotency-guarantee), [example](https://www.routable.com/resources/idempotency-mass-payout-apis-guide/)). I could not establish typical retention from a primary source.

**Duplicate execution by the engine itself.** Source: Conductor concurrency, Zeebe job timeouts, Temporal activities, the DBOS step window, the Restate side-effect window (all above).

**Two writes that cannot commit together.**
- Source: this caused all three serious Conductor findings, and it is why the Azure Storage provider relies on eventual consistency (above).
- Inference: `PS-19` gives modules shared and per-transaction state. If that state and the engine's record are stored separately, the same gap exists inside the platform, not only at the provider boundary.

**A record that claims something false.** Source: never-run workflows marked completed (Conductor). Inference: for `PS-21` this is worse than a stuck order, because the audit trail itself is wrong.

**Silent stall.** Source: Conductor workflows abandoned in a running state, and tasks stalled until a response timeout that defaults to one hour.

**Compensation that fails or cannot happen.** Source: Azure, SEPA, Pix (above).

**Check-then-act races.**
- Source: ACIDRain found 22 vulnerabilities across 12 e-commerce platforms deployed on over two million sites, and reports a concurrency attack that shut down a Bitcoin exchange in 2014 ([summary](https://blog.acolyer.org/2017/08/07/acidrain-concurrency-related-attacks-on-database-backed-web-applications/), [DOI](https://doi.org/10.1145/3035918.3064037)).
- Inference: `PS-15` and `PS-31` give a yes-or-no balance verification. A yes is not a reservation. Two orders can pass the same check.

**Free option.** Source: ILPv4 names this as a reason for short timeouts. Inference: the longer value or a quote is held, the more a counterparty gains from waiting to see the market before acting. This touches `REQ-QTE-*`.

**Upgrade while in flight.** Source: Temporal replay mismatch and XState snapshot incompatibility (above). Inference: `PS-18` hot-loading makes this normal rather than rare.

**Timing assumptions.** Source: the van Glabbeek proof depends on known message bounds and handles clock skew explicitly; Oracle reports mixed outcomes from differing timeouts.

**Outliving the engine's limits.** Inference: a SEPA recall may arrive 13 months after the transfer for fraud, past Step Functions' one-year execution limit and well past its 90-day history retention.

## 3. Making execution provable

| Approach | Can prove | To whom | Assumes | Cannot prove |
|---|---|---|---|---|
| Engine history (Temporal, Step Functions, Durable Task) | What the engine recorded; supports recovery and inspection | Whoever trusts the operator | Operator and store are honest | Absence of later edits (Inference); subject to size and retention limits |
| Signed entries with hash chain (Schneier and Kelsey, 1999) | Entries made before compromise cannot be undetectably changed or destroyed | Holder of the verification material | Logger honest until compromised | Anything after compromise |
| History tree / Merkle log (Crosby and Wallach, 2009) | An event is present; the current log is consistent with an earlier one | Any auditor with a commitment | Hashes and signatures hold; at least one honest auditor; auditing actually happens | Availability; secrecy; events never logged |
| Certificate Transparency log (RFC 9162, 2021) | Inclusion and append-only behaviour, against signed tree heads | Any client or monitor | Log is treated as a trusted third party for consistent views | That all clients saw the same view |
| Witness cosigning (C2SP) | A checkpoint extends what the witness saw; timestamped cosignature | Clients requiring a witness quorum | Enough witnesses honest and independent | Truth of the entries |
| Transparency service with receipts (RFC 9943, 2026) | An identified issuer made a statement and it was registered, with no fork in the sequence | Relying parties | The service's own requirements hold | That the statement is true |
| Accountability by replay (PeerReview, 2007) | A node deviated from what a correct node would do, with irrefutable evidence linked to it | Other nodes, administrators | Correct behaviour is deterministic; signatures cannot be forged | Faults that leave no observable deviation |
| Database ledger feature (SQL Ledger, 2021) | Stored data has not been altered against digests | Whoever holds the digests | Digests kept outside the database *(search excerpt)* | Whether data was true when written |

Sources:
- [Schneier and Kelsey, 1999](https://www.schneier.com/academic/archives/1999/05/secure_audit_logs_to.html).
- [Crosby and Wallach, USENIX Security 2009](https://static.usenix.org/event/sec09/tech/full_papers/crosby.pdf). A hash chain could require an 800 MB trace to prove one event in a log of 80 million; their tree gives a 3 KB proof. A log never examined can be tampered with undetected. Clients must spread commitments to auditors, for example by gossip, or a logger can show different views.
- [RFC 9162](https://www.rfc-editor.org/rfc/rfc9162.html), Experimental. A misbehaving log can show inconsistent views to different clients, and mechanisms against this are out of its scope.
- [C2SP tlog-witness](https://github.com/C2SP/C2SP/blob/main/tlog-witness.md), editor's copy.
- [RFC 9943, June 2026](https://www.rfc-editor.org/rfc/rfc9943.html), Standards Track. Section 9.2 says registration only proves an issuer produced the statement.
- [PeerReview](https://peerreview.mpi-sws.org/). I read the project page, not the full paper.
- [SQL Ledger](https://azure.microsoft.com/en-gb/resources/research/sql-ledger-cryptographically-verifiable-data-in-azure-sql-database) *(search excerpt)*.

**Production signal.** Source: Sigstore's Rekor v2, a tile-backed log, is in production but opt-in; the public default stays on v1 for now because of client breakage ([2026-06-28](https://blog.sigstore.dev/rekor-evolution/)). Source: AWS ended support for QLDB, its verifiable-journal database, on 2025-07-31 and pointed customers to Aurora PostgreSQL, which keeps no permanent immutable record of changes ([InfoQ, 2024-07-26](https://www.infoq.com/news/2024/07/aws-kill-qldb)). Inference: a verifiable history bound to one vendor's product can disappear with that product.

### Mapping `PS-21`: who, what, when, outcome

All Inference, built on the sources above.
- **Who.** A log binds an entry to a key. It proves the actor only as far as the key maps to a person or system. With authentication external (`PS-13`), part of this claim rests on that external system.
- **What.** This is the strongest case: integrity of the content from registration onward.
- **When.** A log gives order. Wall-clock time is only the logger's say-so, unless bounded from outside, for example by a witness's timestamped cosignature.
- **Outcome.** Provider-reported outcomes are statements by that provider. rutent can prove what it was told and when, and cannot prove the statement is true. An outcome counts as a fact only if the provider signs it or an independent record matches.
- **Completeness.** A log cannot prove nothing was left out. The `SPEC.md` stub "no action without its record" is checkable only if a counterparty keeps something to compare, such as a receipt or the provider's own record. Recording intent before acting narrows the gap, but does not remove it.
- **Replay.** Recomputing a decision needs the logic version and every input, including provider responses. Replaying against a live provider is unsafe (Fowler, above), so those responses must be recorded.

**Privacy tension.** Inference: `PS-30` and `PS-31` limit disclosure, while broad verifiability favours publishing. Merkle structures can prove one entry without revealing the others. Whether append-only records are compatible with erasure duties is a legal question I did not settle. EDPB Guidelines 02/2025 on blockchain processing exist, but I read only the [consultation page](https://www.edpb.europa.eu/our-work-tools/documents/public-consultations/2025/guidelines-022025-processing-personal-data_en), not the guideline.

## 4. Scale evidence

All **Measured** by the party named, except quotas. Almost all are vendor-run against mock or no-op work, on different hardware, and none includes a real provider. They are not comparable with each other.

| System | Figure | Conditions | Who, when |
|---|---|---|---|
| Restate 1.2 | 23,131 rps, p50 16 ms, p99 40 ms | 1-step workflow, high load, 3 nodes, 3-way replication; hardware not stated | [Restate, 2025-02-18](https://restate.dev/blog/announcing-restate-1.2) |
| Restate 1.2 | 16,844 rps, p50 58 ms, p99 98 ms | 3-step, same cluster | Same |
| Restate 1.2 | 8,571 rps, p50 116 ms, p99 163 ms | 9-step, same cluster | Same |
| Restate 1.2 | 549 rps, p50 15 ms, p99 69 ms | 3-step, low load | Same |
| Restate 1.1.0 | 17,453 rps, p99 37.5 ms | Single step, mock service on the same host, c6id.8xlarge, local NVMe | [Restate, 2024-09-11](https://restate.dev/blog/restate-1.1.0-and-multiple-sdks-released) |
| Restate 1.1.0 | 10,409 rps, p99 60.1 ms | Same, EBS io2 | Same |
| Restate 1.1.0 | 4,118 rps, p99 140 ms | m6g.2xlarge, EBS gp3 | Same |
| DBOS | 144K writes/s | Raw single-table inserts, RDS db.m7i.24xlarge, 96 vCPU, 384 GB, 120K IOPS io2 | [DBOS, 2026-04-23](https://www.dbos.dev/blog/benchmarking-workflow-execution-scalability-on-postgres) |
| DBOS | 43K workflows/s | No-op, no steps, directly started, same host; no latency given | Same |
| DBOS | 12.1K workflows/s; 30.6K | No-op, queued: single queue; many queues | Same |
| DBOS | 20K/s on 1 shard to 1M/s on 50 | No-op, single logical queue, db.m7i.4xlarge shards, PgDog router | [DBOS, 2026-10-28](https://www.dbos.dev/blog/one-million-workflows) |
| Temporal | 150 raised to 1,350 state transitions/s | Tuning walkthrough, not a maximum; 2 pods per service, 512 shards, MySQL 4 CPU, 32 GB; start p95 under 150 ms target | [Temporal, 2023-05-23](https://temporal.io/blog/scaling-temporal-the-basics) |
| Zeebe 8.3 | p50 about 0.2 s, p99 about 0.75 s, at a fixed 150 PI/s | 1 task, 50 ms worker, 3 brokers, 3 partitions, RF 3; approximate | [Camunda, 2024-02-05](https://camunda.com/blog/2024/02/zeebe-performance-2023-year-in-review/) |
| Zeebe | About 210 PI/s, near the maximum | 1 task, 32 KiB payload, 3 brokers, 6 partitions, RF 3, 1.35 CPU, 4 GiB each, extra checks on | [Camunda, 2023-12-20](https://camunda.github.io/zeebe-chaos/2023/12/20/Broker-scaling-performance/) |
| Zeebe | 40 PI/s on 3 brokers; 65 PI/s on 6 | 10-task model, with backpressure | Same |
| Orkes Conductor | About 1,100 workflows/s; task p50 about 500 µs, p99 about 10 ms | Jepsen's concurrency test, not a benchmark: 2 servers, 3 workers, near-zero network latency | [Jepsen, 2026-10-20](https://jepsen.io/analyses/orkes-conductor-5.3.2) |
| TigerBeetle | 454,518 tx/s (909,162 transfers/s) | Large batches, ordered IDs; hardware and replicas not stated; authors call it rough; no latency | [TigerBeetle, 2026-09-17](https://tigerbeetle.com/blog/2026-09-17-performant-use-of-tigerbeetle/) |
| TigerBeetle under fault | Min latency under 1 ms rising to about 10 s; 1-50 ms rising to about 100 s | One node killed: 5 nodes with pinned clients; 3 nodes at higher load, about 1,000 s until restart. Versions before later ring fixes | [Jepsen, 2025-06-06](https://jepsen.io/analyses/tigerbeetle-0.16.11) |
| Tamper-evident log | 1,750 events/s; 10,500 with signatures offloaded | 80M-event syslog trace, one CPU core, 2009 prototype | [Crosby and Wallach, 2009](https://static.usenix.org/event/sec09/tech/full_papers/crosby.pdf) |
| TesseraCT, a transparency log | About 10,750-11,900 writes/s at about 12,000 offered; author judges 8,000 writes/s with 4,000 reads/s safe | POSIX backend, 20-core host, ZFS on SSD; one operator's lab | [IPng, 2025-07-26](https://ipng.ch/s/articles/2025/07/26/certificate-transparency-part-1-tesseract/) |
| TesseraCT | About 400 writes/s judged safe | S3 and MySQL backend, same operator | Same |

**Quotas, not measurements.** Source: Step Functions Standard `StartExecution` has a 1,300 bucket refilling at 300/s in N. Virginia, Oregon and Ireland, and 800 at 150/s elsewhere. State transitions are 5,000 at 5,000/s and 800 at 800/s respectively. Express starts are 6,000 at 6,000/s. HTTP Tasks are 300 at 300/s. There is a default of 1,000,000 open executions. The page marks the throttling quotas for Standard state transitions and for API actions as soft, meaning they can be raised ([quotas](https://docs.aws.amazon.com/step-functions/latest/dg/service-quotas.html)).

Inference:
- `ASM-1` still has no number or unit, so nothing here sets a bound. If "thousands" were to mean per second, several of these results sit in range under vendor conditions, and none shows whether that holds with real provider waits.
- Throughput of orders started is only one dimension. Others are open orders held concurrently, the duration of each, and behaviour during a fault. The TigerBeetle row shows fault-time latency can differ from healthy latency by orders of magnitude.
- A provider's own speed and rate limits probably set the real ceiling, and I found no published figures for that.

## 5. What this implies

### 5a. Capabilities to think about stating

None names a mechanism; each could be met in more than one way. These are prompts for the spec authors, not requirements.

**Outcome and retry** (`REQ-STM-*`, `REQ-ORD-*`, from `PS-19`, `PS-21`)
- "Outcome unknown" exists as a state of a stage, distinct from success and failure, and is left only on stated evidence.
- A caller can tell "this did not happen" from "this is not known".
- Intent is recorded before an external action is attempted, and its outcome afterward.
- Repeating a request, or re-attempting a stage, does not move value twice for any provider operation declared safe to repeat.
- Overlapping attempts of one stage, which cannot be ruled out entirely, stay safe.

**What providers declare** (`REQ-CAT-*`, `REQ-MDL-*`, from `PS-14` to `PS-19`)
- Each provider operation declares what it offers: safe repeat under a caller identifier, status query, hold, cancel, reversal, and how long each stays available.
- Routing can take those declarations into account, including refusing a stage that offers no way to learn its outcome.

**Termination and truthfulness** (`REQ-STM-*`)
- Every order reaches a terminal or escalated state within a declared bound; none stalls silently.
- A terminal state claims only what happened; a stage never run is never reported as done.
- An acknowledged order is never lost.

**Partial failure** (`REQ-ORD-*`, `OQ-9`)
- The meaning of partial failure is stated per kind of order, including points of no return.
- Compensation is a stage in its own right: it can fail or be refused, can take weeks, and is recorded like any other.
- Human intervention, if allowed, is authorised (`REQ-AUZ-*`) and recorded.
- Conservation can be stated with value in doubt counted explicitly, rather than assumed moved or unmoved.
- A balance verification is not treated as a reservation (`PS-15`, `PS-31`).

**Component and change** (`REQ-MOD-*`, `REQ-OPS-*`, from `PS-10`, `PS-12`, `PS-18`)
- The state machine can be used and tested without the rest of rutent and without a specific hosted service.
- Each decision records which definition and module version made it; orders in flight survive an upgrade with defined behaviour.
- Orders can stay open as long as the slowest rail's dispute window requires.

**Verification** (invariants section, `oss-ledger-1`)
- Correctness can be checked under injected faults at each boundary, including a crash between acting and recording and a lost response. Jepsen reports that single-node faults were enough to trigger several Conductor bugs.

**Audit** (`REQ-AUD-*`, `PS-21`, `PS-22`)
- Who, what, when and outcome are recorded for every transition and every amendment in the state.
- Tampering is detectable by a party other than the operator.
- All verifiers can confirm they see the same history.
- One record can be proven without disclosing others (`PS-30`, `PS-31`).
- Decisions can be recomputed from recorded inputs, including provider responses, without calling providers.
- rutent's own assertions are kept distinct from facts a provider attests to.
- Omission can be detected, using something a counterparty keeps.
- The source of time is stated, along with how far it can be trusted.
- The audit trail outlives execution, in a documented format independent of any runtime's internals.
- Retention and erasure duties can both be met.

### 5b. Candidate mechanisms for non-binding suggestions

For `suggestions/`. No ranking is implied. Facts are sourced above; trade-offs are Inference.

**Execution**

| Candidate | For | Against, given `PS-10`, `PS-12` and an open-source project |
|---|---|---|
| Run a durable engine as a dependency (Temporal, Cadence) | Mature recovery and permissive licences; Temporal has published money-movement use | The state-machine component is no longer standalone without a server; several services and a database to operate; history limits and code-versioning discipline apply; no independent safety analysis found |
| Restate | One binary; low operational surface | BUSL is not open source and restricts a public platform service, which matters if `PS-23`'s open direction ever means hosting others' services; server still required |
| Camunda 8 / Zeebe | BPMN notation, readable by non-engineers | Paid licence for production from 8.6, which is hard to square with an independently operated open project |
| Conductor | Apache 2.0; published fault testing and fixes | Server, database and queue to operate; Jepsen tested only single-task workflows, and the tested build may differ from the open repo |
| Step Functions | Nothing to operate; exactly-once wording for non-idempotent tasks | Single vendor, conflicting with `PS-12`; 25,000 events, 1 year, 90-day retention |
| Embedded durable library (DBOS-style) | Closest fit to "a library"; only Postgres | Tied to one database and one language per application; a single-language library sits awkwardly beside `PS-20`'s many languages; its failure window is still there for external steps |
| Own transition core with pluggable durability, as Durable Task does with storage providers | Standalone by construction; can sit on top of an engine or a database | Rebuilds the part where Conductor's serious bugs were; needs a fault-testing programme from the start |
| Pure state-machine library or notation (XState, SCXML, Open Workflow Specification) for definition only | Definitions can be checked and shared | Provides no durability, so it complements another candidate rather than replacing one; XState is JS/TS only |
| Event-sourced store as both record and execution state | One source for recovery and audit | External effects must be guarded on replay; KurrentDB is not OSI-licensed and multi-node needs a paid licence; events need versioning |

**Multi-party control**

| Candidate | For | Against |
|---|---|---|
| Caller identifier per stage attempt, reused on repeat | Simplest protection against double effect; widely used | Works only if the provider honours it, and only for as long as it retains it |
| Holds or two-phase transfers where offered | Stronger than compensation, since nothing is final until confirmed | Not offered by all providers; expiry must line up across parties; free-option exposure |
| Internal ledger with reserve, post and void (TigerBeetle or equivalent) | Clear in-doubt accounting; independently tested | Reserves only what that ledger controls, not what a provider holds; depends on `oq-custody` |
| Writing the state change and the outgoing message in one transaction (transactional outbox) | Closes the gap behind Conductor's serious findings ([Azure pointer](https://learn.microsoft.com/en-us/azure/architecture/patterns/compensating-transaction)) | Needs one store for both; delivery is then at-least-once |
| Independent reconciliation against provider statements | Works without provider cooperation at call time; catches silent divergence | Delayed; can only confirm what the provider reports |

**Evidence**

| Candidate | For | Against |
|---|---|---|
| Hash chain with periodic external anchoring | Simple; cheap to write | Linear audit cost; proving one entry can mean a large trace |
| Merkle log with signed checkpoints | Short inclusion and consistency proofs; one entry provable alone | Needs auditing to mean anything; split views remain unless something else addresses them |
| Merkle log plus independent witnesses | Addresses split views | Requires independent witnesses to exist and be run; adds a dependency to writes or checkpoints |
| RFC 9943-style signed statements with receipts | Standards-track shape for who said what, and receipts counterparties can keep, which helps omission detection | Young standard (June 2026); framed around supply chains; I did not check implementations |
| Database ledger feature | Little to build | Binds evidence to one vendor; see the QLDB end of support |
| Provider-signed outcome receipts | Turns an outcome claim into an attested statement | Providers must agree to sign; only some will |

## 6. Questions only the owner can answer

- What should "atomic" mean for an order, given that independent providers cannot commit together (`OQ-9`)?
- What is the longest an order may stay unresolved before it must escalate?
- When a stage's outcome is unknown, may funds stay held, and who bears the loss if the provider acted?
- Is a person resolving a stuck order an acceptable end state, and who may do it?
- Must every provider operation accept a caller-supplied identifier, or a way to query status, as a condition of joining the catalogue?
- Will a route ever include a provider with no way to learn the outcome of an action?
- Does rutent ever hold value itself (`OQ-10`)? The answer changes which hold patterns are possible.
- Provable to whom: the operator only, providers, regulators, end users, or the public?
- Must the audit trail hold up even if the operator itself is dishonest?
- May anything about the audit trail be public, even commitments that reveal no content?
- How long must history be retained, and do erasure duties apply to it?
- Whose clock defines "when"?
- Must rutent be able to prove a provider's outcome, or only what the provider said?
- Are source-available licences, paid-for production licences, or managed-only services acceptable as required dependencies?
- May the standalone state machine require a server, or must it run embedded?
- Must the state machine library itself be usable from several languages, or only provider modules?
- What number replaces "thousands of requests" (`ASM-1`), and does it mean requests, orders started, or orders open at once?

## 7. What I could not determine

- Whether Temporal, Cadence, Restate, Zeebe, Step Functions or DBOS have any independent safety analysis. The Jepsen index lists none. I found none elsewhere, and that is not evidence of safety.
- Any published throughput figure for Temporal Cloud, or any maximum for self-hosted Temporal. Any authoritative figure for Cadence.
- Whether Restate has a current document defining the side-effect failure window.
- How much code the Jepsen-tested Orkes build shares with conductor-oss, and the licence of the Orkes build.
- Licences of the wider Durable Task components, and of the Open Workflow Specification.
- Whether Open Workflow Specification 1.0 keeps the 0.6 compensation rules.
- Reversal and return rules for SPEI and Bre-B.
- Whether Pix guide v4.4, in force since 2026-10-26, changes the v4.3 points cited in section 2.
- Typical provider retention of idempotency keys, from a primary source.
- A published root-cause report for the 2021 duplicate payment incident, or any published post-mortem of a durable engine causing a money loss.
- What EDPB Guidelines 02/2025 say on erasure.
- Full text of Helland's work on activities and tentative operations. ACM Queue returned 403 and the CIDR copy's certificate was refused, so it is not used here.
- Real provider latency or rate limits at the scales in section 4.
- Implementations of RFC 9943, and whether Rekor v2 ships witnessing.

## References

All accessed 2026-11-02. Version or date as shown by the source; "undated" means none was visible.

**Engines and libraries**
- Temporal: [execution](https://docs.temporal.io/workflow-execution), [activities](https://docs.temporal.io/activities), [activity definition](https://docs.temporal.io/encyclopedia/activities/activity-definition), [limits](https://docs.temporal.io/workflow-execution/limits), [persistence](https://docs.temporal.io/temporal-service/persistence), [server](https://docs.temporal.io/temporal-service/temporal-server), [SDKs](https://docs.temporal.io/encyclopedia/temporal-sdks), [safe deployments](https://docs.temporal.io/develop/safe-deployments), [LICENSE](https://raw.githubusercontent.com/temporalio/temporal/main/LICENSE); undated docs.
- Cadence: [docs](https://cadenceworkflow.io/docs/concepts/open-source-workflow-engine); undated.
- Restate: [key concepts](https://docs.restate.dev/foundations/key-concepts), [journaling](https://docs.restate.dev/develop/ts/journaling-results), [durable execution guide](https://restate.dev/what-is-durable-execution), [LICENSE](https://raw.githubusercontent.com/restatedev/restate/main/LICENSE), [2023-08-24](https://restate.dev/blog/why-we-built-restate), [1.1.0, 2024-09-11](https://restate.dev/blog/restate-1.1.0-and-multiple-sdks-released), [1.2, 2025-02-18](https://restate.dev/blog/announcing-restate-1.2).
- AWS Step Functions: [workflow types](https://docs.aws.amazon.com/step-functions/latest/dg/concepts-standard-vs-express.html), [quotas](https://docs.aws.amazon.com/step-functions/latest/dg/service-quotas.html); undated.
- DBOS: [architecture](https://docs.dbos.dev/architecture), [repo](https://github.com/dbos-inc/dbos-transact-py), [2025-08-07](https://www.dbos.dev/blog/why-postgres-durable-execution), [2026-04-23](https://www.dbos.dev/blog/benchmarking-workflow-execution-scalability-on-postgres), [2026-10-28](https://www.dbos.dev/blog/one-million-workflows).
- Camunda 8.10: [architecture](https://docs.camunda.io/docs/components/zeebe/technical-concepts/architecture/), [job workers](https://docs.camunda.io/docs/components/concepts/job-workers/), [licences](https://docs.camunda.io/docs/reference/licenses/), [clients](https://docs.camunda.io/docs/apis-tools/working-with-apis-tools/).
- Conductor: [conductor-oss](https://github.com/conductor-oss/conductor); [Jepsen, Orkes Conductor 5.3.2, 2026-10-20](https://jepsen.io/analyses/orkes-conductor-5.3.2).
- Durable Task: [orchestrations, ms.date 2026-04-22](https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-orchestrations), [SDK repo](https://github.com/microsoft/durabletask-dotnet).
- XState: [repo](https://github.com/statelyai/xstate), [v5, 2023-12-01](https://stately.ai/blog/2023-12-01-xstate-v5), [persistence](https://stately.ai/docs/persistence).
- [SCXML, W3C Recommendation, 2015-09-01](https://www.w3.org/TR/scxml/). [Open Workflow Specification 1.0.3](https://open-workflow-specification.org/).
- [Event Sourcing, Fowler, 2005-12-12](https://martinfowler.com/eaaDev/EventSourcing.html).
- TigerBeetle: [repo](https://github.com/tigerbeetle/tigerbeetle), [two-phase transfers](https://docs.tigerbeetle.com/coding/two-phase-transfers/), [Jepsen 0.16.11, 2025-06-06](https://jepsen.io/analyses/tigerbeetle-0.16.11), [2026-09-17](https://tigerbeetle.com/blog/2026-09-17-performant-use-of-tigerbeetle/).
- [Jepsen analyses index](https://jepsen.io/analyses).

**Multi-party operations**
- [Garcia-Molina and Salem, Sagas, CS-TR-070-87, January 1987](https://www.cs.princeton.edu/techreports/1987/070.pdf); published at SIGMOD 1987.
- [Compensating Transaction pattern, Azure, 2026-04-16](https://learn.microsoft.com/en-us/azure/architecture/patterns/compensating-transaction). [Saga, microservices.io](https://microservices.io/patterns/data/saga.html).
- [Tentative Operation, Enterprise Integration Patterns](https://www.enterpriseintegrationpatterns.com/patterns/conversation/TryConfirmCancel.html). [Oracle MicroTx 24.4 timeouts](https://docs.oracle.com/en/database/oracle/transaction-manager-for-microservices/24.4/tmmdg/transaction-timeout.html).
- [Hope-Bailie and Thomas, WWW 2016 companion](https://archives.iw3c2.org/www2016/WWW2016-Proceedings/companion/p281.pdf). [ILPv4](https://interledger.org/developers/rfcs/interledger-protocol/).
- [van Glabbeek, Gramoli, Tholoniat, arXiv 1912.04513, 2019-12-10](https://arxiv.org/pdf/1912.04513).
- [Making retries safe with idempotent APIs, AWS Builders' Library](https://aws.amazon.com/builders-library/making-retries-safe-with-idempotent-APIs/).
- [ACIDRain, SIGMOD 2017](https://doi.org/10.1145/3035918.3064037), via [summary](https://blog.acolyer.org/2017/08/07/acidrain-concurrency-related-attacks-on-database-backed-web-applications/).
- [EPC131-17 v3.1, 2025-04-25](https://www.europeanpaymentscouncil.eu/sites/default/files/kb/file/2025-04/EPC131-17%20v3.1%20Clarification%20Paper%20SCT%20and%20SCT%20Inst%20scheme%20rulebooks.pdf). [BCB Pix refund guide v4.3](https://www.bcb.gov.br/content/estabilidadefinanceira/pix/Guia_MED.pdf).

**Provable history**
- [Schneier and Kelsey, ACM TISSEC, 1999](https://www.schneier.com/academic/archives/1999/05/secure_audit_logs_to.html). [Crosby and Wallach, USENIX Security 2009](https://static.usenix.org/event/sec09/tech/full_papers/crosby.pdf). [PeerReview, SOSP 2007](https://peerreview.mpi-sws.org/).
- [RFC 9162, December 2021](https://www.rfc-editor.org/rfc/rfc9162.html). [RFC 9943, June 2026](https://www.rfc-editor.org/rfc/rfc9943.html). [C2SP tlog-witness, editor's copy](https://github.com/C2SP/C2SP/blob/main/tlog-witness.md).
- [Rekor evolution, 2026-06-28](https://blog.sigstore.dev/rekor-evolution/). [IPng TesseraCT, 2025-07-26](https://ipng.ch/s/articles/2025/07/26/certificate-transparency-part-1-tesseract/). [QLDB, InfoQ, 2024-07-26](https://www.infoq.com/news/2024/07/aws-kill-qldb).
