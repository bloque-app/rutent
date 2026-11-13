# F-022 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-022-01

- **What the log is.** `AuditLog<S, K>` in `components/core-audit/src/log/` implements both of `core-internal`'s
  handles, `Log` and `PolicyLog`, on one log. `S: Store` is its own store (ADR-0005), apart from the state machine's
  and the provider system's; `K: KeptStatements` is the evidence store's read. `log::Unbuilt` stays, since the other
  rows' stubs and the assembly's `Assembly::unbuilt` still build on it.
- **One append path.** Every append, through either handle, runs the entry's `check`, then `attribute` for a stated
  entry, then the retry rule, then `admit_entry` on its path (`Through::General` or `Through::Policy`), then a
  durable write, and only then takes the entry into its history, its tree and its register and answers the
  receipt. A failed write answers `Unavailable` and changes nothing; since the write may still have reached the
  store (a crash between the write and its answer), the log reads its store back before its next append, so a retry
  never writes a record twice. Where that read-back fails, the append answers `Unavailable`, never `Refused`: the
  entry broke no rule, and its writer keeps it to append again (`REQ-MDL-8`). The read-back is built apart and
  taken only whole, and only where the history the log already answered is a prefix of it, so a read that fails
  part-way, or a store that comes back without a record the log answered, leaves the history as it was (and the
  append unavailable). Opening a log on a store it can't read whole is refused.
- **Kept bytes bear out their commitment.** On every stated entry's append, the bytes the evidence store answers
  must hash, after their salt, to the entry's commitment, or the append is refused.
- **The retry rule** is on the entry recorded, not the one handed in: an append whose recorded entry's digest is
  already in the log answers the earlier receipt. For a stated entry this means attribution runs again on a retry;
  with the register unchanged it gives the same entry. Indexing the handed-in entry instead would not survive a
  reopen, since the store holds only recorded entries.
- **The record** is the entry's RFC 8785 canonical JSON (`to_canonical` of the `Entry`), one per line of the store.
  The receipt's digest is SHA-256 of it; the Merkle leaf is RFC 9162's, SHA-256 of `0x00` and the record, and the
  root is RFC 9162's tree hash. On opening, every record must be an entry that checks and is in its canonical form,
  or the log refuses to open; a torn last line, which was never answered, is dropped. Opening doesn't run
  `admit_entry` again: the store is the log's own, and a rewrite of it, by anyone with the operator's access, is
  the outside checker's to detect against the checkpoints (`T-022-04`, `REQ-AUD-9`).
- **The register comes back whole, or the log doesn't open** (the coordinator's ruling, after review). The register
  is rebuilt from the records and the bytes the evidence store keeps for each recorded advertisement. Skipping one
  whose bytes are gone would lose a rotation (a key retired by a later advertisement would sign again) or, with an
  empty store, let first trust in anew and revive a dropped key by a replayed public advertisement. So opening is
  refused where any recorded advertisement's bytes aren't kept, or don't hash, after their salt, to its commitment
  (`ADVERTISEMENT_NOT_KEPT`). The log must be opened with the evidence store it ran with.
- **A rule for `T-022-03`: no erasure reaches an advertisement.** An advertisement is a provider's public data, not
  personal data, so `REQ-AUD-13`'s erasure never applies to it (the coordinator's ruling); the evidence store keeps
  every advertisement's bytes for as long as the log is kept. Other statements may be erased: opening reads no
  statement's bytes but an advertisement's.
- **Stores.** `MemoryStore`, and `FileStore`, one append-only file synced before each append answers. It holds the
  file open under an exclusive lock for its lifetime, so no second store, in this process or another, writes the
  same log; it syncs the file's directory on opening, so a file just created survives a crash; and it syncs the file
  before each read-back, so a write whose sync failed is read as the disk holds it (no test can make a sync fail
  short of a failing disk, so that one stands on review). A deployment's store is `F-030`'s (`M4`); the trait lets
  it plug in.
- **The read surface** for the later rows, which can't edit `log/`: `entries()`, `len()`, `root()`, `root_at(size)`,
  `inclusion(index, size)` with `merkle::verify_inclusion`, and the free functions `record` and `digest`. No
  consistency proof is built: `T-022-04`'s checker holds the whole history and recomputes `root_at`.
- **The checks' handle.** `log::General<L>` wraps a shared log and implements `Log` alone. `T-022-05` and `T-022-06`
  take it (or any `Log`), never `AuditLog`; a test holds that it implements no `PolicyLog`.
- **Policy's handle.** `log::Policy<L>` wraps a shared log, implements `PolicyLog` alone and isn't `Clone`, the
  match of `General`; a test holds both. The assembly (`F-024`'s) still holds its own `PolicyHandle`, and may take
  this one instead.
- **The path before the retry rule.** An append holds its path to its entry's kind (a decision only through
  policy's handle, anything else only through the general one) before the retry rule, as `admit_entry` holds it
  again after. The handles' types already keep each kind to its path, so no test reaches it, and no rule-off can
  witness it.
- **The evidence store's read** is a stand-in until `T-022-03` builds the store: `log::KeptInMemory`, a map from
  commitment to `KeptStatement` that the caller fills with a salt of its own. It has no restatement index, draws no
  salt and answers no `Evidence` read. `T-022-03`'s store replaces it through `KeptStatements`, with no change to
  the log.
- **Signatures** are checked by the catalogue's strict verifier, `rutent_prov::catalogue::verify_envelope`, the one
  admission uses, over the bytes as received (`log::Ed25519Strict`). The log adds no curve code of its own.

### Whose keys: the register

- **What the log reads keys from.** Its own records and the deployment's settings (`log::Settings`: the deployment,
  its operator and the log's own clock), never orchestration. A catalogue change counts only once the log holds a
  `done` result for its effect, as `core-internal` reads it. The operator signs as `Settings::operator` with the
  deployment's `operator_key`, and states for `Settings::operator_party`. A provider's grant is in force from a done
  `change_catalogue` effect of `grant` for one of its tuples until a done one of `revoke_grant` for it, each at its
  effect's own time. A provider's advertisement is a stated entry of the advertisement's shape whose bytes the
  evidence store keeps, received within the skew of the log's clock, held to provider-role's rules and to the
  catalogue's (`F-012`) rules on keys and publishers: this deployment and protocol version, `validate_statement`,
  every listed key a key to accept, a way to follow an outcome (`NoTracking`), dated no more than the skew ahead of
  its `received_at`, a grant in force then, the signature strict, a successor naming the same publisher
  (`ProviderChangedPublisher`), each private node it declares on record for no other publisher
  (`NodeDeclaredByAnotherPublisher`), no module the operator publishes (`ProviderIsTheOperators`), no dropped
  `statements` key listed again, and no key across purposes for any provider. The catalogue's other rules on an
  advertisement are not the register's, since they bear on routing, not on whose key signs: its declarations'
  record (`REQ-CAT-6`) and a private node the operator lists as public (`REQ-CAT-22`, whose list the log isn't
  given).
- **First trust is the grant's** (`core-internal`'s should-fix 5, after review; it replaced the deployment setting
  `Settings::first_trust` this row first took, which a checker outside the operator couldn't recompute). A `grant`
  names the provider's advertising keys, and `granted_advertising_keys` reads those of its latest grant in force.
  A provider's first advertisement, or one after a grant made since its last accepted (how a provider that lost its
  key gets back in, by a fresh grant naming a new one), is accepted only where it is signed with one of those keys,
  every advertising key it lists is one, and it is made after that grant's effect, by the effect's time, which the
  log held to its own clock (a hand-off from the contract review): no advertisement backdated before its grant rides
  on it. Every other one only as `validate_successor` of the one accepted last, and every one newer than that. The
  signer being a granted key is implied by `validate_statement` (an advertisement is signed with a key it lists for
  advertising) and the listed-keys rule together, so switching that one check off alone fails no test.
- **When a key signs.** A key signs a provider's statement where a grant of it is in force at the statement's
  `made_at`, and both the advertisement accepted last and the one in force at `made_at` list it for `statements`:
  the catalogue's own rule (`verify_statement`), stricter than `StatementKeys::signs_for`'s minimum, so a retired key
  can't sign a statement backdated to before its retirement. What the log recorded earlier stays as recorded.
- **An advertisement stays unverified as an entry.** It is signed with an advertising key, which `signs_for` never
  answers for, so its stated entry is recorded unverified; the register reads its bytes, not its attribution.
- **A change counts once done.** The register takes a grant, a revocation or a release's acceptance in only on the
  `done` result of its effect, which `admit_entry` admits only on an unused permit of a permitting `decide` of the
  same action, as `granted_advertising_keys` and `release_statement_keys` do; a change whose effect failed or went
  unanswered changes nothing. The ordering rule below reads every admitted grant and revocation, done or not.
- **Whose clock a grant runs on** (the coordinator's ruling, after review). A grant's and a revocation's times are
  their effects' `when.at`, which orchestration writes, so the log holds them to its own clock: `Settings::clock`, a
  `Clock` the deployment gives it (`SystemClock` by default), which orchestration never writes. A `change_catalogue`
  effect dated more than the skew (`Settings::clock_skew_seconds`) ahead of the log's clock when it is appended is
  refused (`CHANGE_OFF_THE_LOGS_CLOCK`), so a revocation can't be dated into 2100 and left with no force; one dated
  more than the skew behind it, as an append retried after the skew is, answers `Unavailable`, not a final refusal,
  so its writer keeps it (`REQ-MDL-8`) and nothing moves on it (`INV-7`); and a
  tuple's grants and revocations must be dated in the log's order, so a re-grant backdated behind a revocation is
  refused (`CHANGE_BEFORE_ITS_TUPLES_LAST`) and the window it would reopen stays shut. Opening doesn't consult the
  clock: the records were held to it when they were appended. A revocation still holds only once orchestration
  appends its effect, which is orchestration's to do on policy's decision.
- **When an advertisement joins.** At its stated entry's `received_at`, the time Core stamps, which the log holds
  to its own clock as it holds a catalogue change's (refused ahead of it, `ADVERTISEMENT_OFF_THE_LOGS_CLOCK`;
  unavailable behind it); a grant must be in force then.
- **Modules the operator publishes** (`REQ-CAT-23`) are verified by their releases: `attribute` holds a maker that
  states for the operator's party, other than the operator, to the statement key of a release of it the log
  accepted (`release_statement_keys`, with the statement's transaction, so a revoked release still signs for a
  segment that began or ran on it, and an exploited one signs nothing). `StatementKeys::publisher` answers from the
  log alone: a provider's publisher by the advertisement accepted last, and the operator's party for the operator
  and for a module the operator publishes: one the log held no grant of when an `accept_module` of it was done,
  and with no advertisement. No advertisement is accepted for such a module, so none can turn it into a
  provider's, and a grant of it after its release changes nothing.
- **Which module is the operator's** (a residual for `T-025-05`, the reviewer's should-fix on `371e45a`). No entry
  says whose a module is, so the log reads a module as the operator's only where, when a release of it is done, the
  log holds no grant (nor revocation) of it, and it has no advertisement. A granted provider whose release is done
  before its advertisement is recorded is no operator's module: its party stays unknown, and its statements
  unverified, until its advertisement is accepted, and then they verify under its own keys for its publisher.
  Before this fix, orchestration, which alone orders those two appends, could lock any released provider out of
  its advertisement for good. What is left: a module released before any grant of it is the operator's, and a later
  grant changes nothing, so a provider can't be granted in after its release; whether a module is the operator's
  needs a policy decision behind it either way.
- **A failed sync can lose data** (a residual for `T-025-05`). On Linux, a write whose `fsync` failed may have had
  its dirty pages dropped, so a later `sync_all` can succeed with the data gone. `FileStore` syncs before every
  read-back and answers the failed append `Unavailable`, and the read-back takes only what the file holds, so a
  record lost that way is appended again by its writer; but a record the store did answer after an earlier failed
  sync is not re-checked against the disk.

### Tests

- The coherent world of `fixtures/admission/world--*` is replayed through the real log: each placeholder receipt
  and commitment is rewritten to the one the log answered and the stand-in kept, every statement is signed for
  real (the conformance crate's `signing` keys, a dev-dependency the boundary check allows) and handed in
  unverified, and the world's five providers are granted and advertised first.
- `REQ-AUD-6`'s scan puts the run's secrets and a balance into kept statements' bodies, asserts the evidence
  store's stand-in holds each, and then that the log's file holds none of them, nor any kept statement's bytes or
  salt.
- Every rule of the append path, of the register and of the file store was switched off in turn, and its test
  failed: the `check`, `attribute`, `admit_entry`, the retry rule, the read-back after a failed write, writing
  before taking an entry in, the canonical form on opening, the read-back failing as unavailable, the read-back
  keeping what the log answered, an advertisement kept on reopening, kept bytes bearing out their commitment on
  reopening and on append, first trust's made-after-the-grant, listed-keys, grant-since-the-last-accepted and
  newer rules, the successor rule, the advertisement's signature, its context, `validate_statement`, its listed
  keys, the skew, the grant at the advertisement and at the statement, the revocation, a change counting once done,
  no advertisement for a released module, a released module stating for the operator's party, the log's clock for
  a change and for an advertisement, behind the clock answering unavailable, a tuple's changes in order, the
  publisher kept, tracking, a private node's publisher, the key in force at `made_at` and in the advertisement
  accepted last, the dropped and the crossed key, the operator's key, the publisher, the strict verifier and the
  file's lock. The path check before the retry rule can't be reached past the handles' types, the signer being a
  granted key is implied by two other rules, and the sync before a read-back can't be made to fail in a test.

## T-022-03

- **What the store is.** `receipts::EvidenceStore` in `components/core-audit/src/receipts/store.rs`. It keeps each
  statement's bytes as received beside a fresh 32-byte salt from the operating system (`getrandom`), under the
  commitment SHA-256 of the salt followed by the bytes, and answers `Evidence::keep` only once the bytes are durable.
  A salt that can't be drawn answers `Unavailable`; the caller never gives one. It replaces `log::KeptInMemory`
  through `KeptStatements`, with no change to the log; the stand-in stays, since `T-022-01`'s tests run on it.
- **Two handles, one call each, then bound once.** The unbound store, `EvidenceStore`, implements none of
  `core-internal`'s calls. It hands the log `LogsRead`, which implements `KeptStatements` alone (no read check:
  nothing it answers leaves Core's audit part), and orchestration and the parties `Keeper`, which implements
  `Evidence` alone and holds policy's read check (`calls::Reads`). Then `EvidenceStore::bind(log)` consumes it and
  answers a `BoundStore`, tied to the one log it serves, which alone erases (the review of `746e398`, B1: erasure
  first took any log per call, so a scratch log could authorise one). Only the assembly (`T-024-02`) calls
  `for_the_log` and `keeper`, once each, before binding; after it, no `LogsRead` can be made. A test holds that no
  handle, nor either store, implements another's call. The log is taken at `bind`, not when the store is made,
  since the log is opened with the store's `LogsRead`, so the store comes first. Binding once is held by consuming
  the store; binding the right log, the one opened with this store's read, rests on the assembly, since the log's
  evidence read is private to `log/` and `bind` can't compare it.
- **Where the bytes live.** A `Shelf`: `MemoryShelf`, or `DirectoryShelf`, a directory of the store's own apart from
  the log's file, locked for one store while it lives. Each statement is `kept/<commitment>.json`, written to a
  `.part` file, synced and renamed, the directory synced after; a `.part` left by a crash was never answered and is
  dropped on opening. Opening refuses a statement that doesn't bear out its commitment, or is filed under another's
  name (`NOT_A_KEPT_STATEMENT`). A deployment's store is `F-030`'s (`M4`); the trait lets it plug in.
- **The log reopens with it.** `AuditLog::open(settings, store, evidence.for_the_log())` reads every advertisement's
  bytes back from the directory, as `T-022-01`'s rule needs; with an empty store in its place, the log refuses to
  open (`ADVERTISEMENT_NOT_KEPT`), which a test holds.
- **The restatement index** files each statement by SHA-256 of its signed content (provider-role's `signing_input`),
  or of its bytes where they have none, and `keep` answers the earliest commitment still kept with the same
  content. It is rebuilt from the kept bytes on opening and written nowhere, so no file holds it, none of it enters
  the log, and an erasure leaves none of it behind. A consequence: a statement kept again after its erasure
  restates nothing.
- **What `keep` takes.** Any bytes but none (`NOTHING_TO_KEEP`). It doesn't refuse bytes that aren't an envelope,
  since orchestration keeps a refused statement's bytes as received (`T-020-05`'s `record_refused`), which may be
  malformed. A `holding_balance` result's bytes are kept here, and only here, as the form's default for
  `holding-balance-evidence` has it.
- **`fetch` asks policy first**, before anything is looked up, every time, so a refused reader can't tell kept,
  erased and never kept apart; its refusal is `Fetched::Refused`. After an allowed read: an erased statement answers
  `Erased`, a kept one `Kept`, and one never kept `Refused(NOT_KEPT)`, never `Erased`. A read of anything but
  `reads: evidence` is refused before policy is asked (`FETCH_READS_EVIDENCE`), and an answer naming another request
  is refused (`ANSWER_OF_ANOTHER_READ`). No borrow of the store is held while policy answers, since policy records
  the read in the log, which reads the store.
- **Whose a statement is** (`core-authz`'s `Source::Statement`). The store keeps bytes and nothing about who they
  concern; whose they are, the maker's party or the one told about its own part, is the log's stated entry naming
  the commitment, which the assembly reads to answer policy (`T-024-02`).
- **Erasure is decided, then carried out, then recorded** (core-internal's fold of `384c9be` and `1100765`, which
  made erasure a decided action). The path, each step admitted by the log on the one before: policy's permitting
  `decide` of `erase_statement` for the commitment, its effect, the effect's `done` result, the store's erasure,
  then the `erasure` entry, which the log admits only after that result (`erasure-is-carried-out-on-its-decision`),
  and all of it refused while anything rests on the statement (`erasure-waits-while-anything-rests-on-the-statement`).
  `BoundStore::erase(at)` takes the receipt of that `done` result, from its own log, and reads the path back: a
  `done` result of an `erase_statement` effect whose permit is a permitting `decide` of that action
  (`NO_DECIDED_ERASURE`). So the decision, and the reference that stays, are recorded before anything goes
  (`INV-7`). `BoundStore::erasable(commitment)` runs the store's checks beforehand, so no erasure is decided that the
  store would refuse: a stated entry of the log names the statement (`NAMED_BY_NO_ENTRY`: the `erasure` entry's own
  row, `erasure-names-a-kept-statement`, comes after the bytes go now, so the store holds it itself), and the rest
  below. It removes the bytes and the salt, and the index's place for them, and leaves a mark,
  `erased/<commitment>`, so a read answers `Erased`. Erasing one erased already is `Ok`, so orchestration retries
  until it can append the `erasure` entry.
- **The operator-label check is dropped.** Until the fold, the store erased only on an `erasure` entry by an actor
  in the operator's role for the operator's party (`ERASURE_BY_THE_OPERATOR`, the review of `746e398`, B1). On the
  decided path the authority is policy's permit, which the log admits only from policy's handle, and the `erasure`
  entry is Core's record of what was done, no longer the authority; a check of its actor would be a second policy in
  the store, so it is gone. The store's re-reading of the permit as a permitting `decide` of the same action is
  defence the log already gives (`effect-permits-a-decided-action`): switched off, it fails no test, since the log
  admits no erase effect without such a permit.
- **A crash anywhere in an erasure** (the review of `746e398`, S1). After the `done` result there are three steps:
  the shelf's mark, the removal of the bytes, the `erasure` entry. A failure answered `Unavailable` is carried out by
  calling `erase` again. After a crash, binding reconciles the store with the log: it carries out every erasure the
  log decided (a `done` result of a permitted erase effect) whose statement is still kept, where `erasable` allows
  it (a crash after the result, before the mark); the shelf's opening removes bytes left behind their mark (a crash
  between the mark and the removal); a mark no erasure the log decided accounts for refuses the binding
  (`MARK_WITHOUT_ITS_ERASURE`: bytes went with no decision); and every stated entry whose bytes are neither kept nor
  marked erased is reported, by receipt, in `BoundStore::missing`, a finding for the outside checker (`T-022-04`).
  Every decided erasure the store refuses on binding (of an advertisement, of what no stated entry names) is
  reported beside it, by its result's receipt and why, in `BoundStore::refused_erasures`. A crash after the store
  erased and before the `erasure` entry leaves the bytes erased and the decision recorded; orchestration appends the
  entry on its retry.
- **An erasure carried out but not settled is reported** (the review of `f236b41`). The store erases on the `done`
  result, and the `erasure` entry follows, so there is a gap. On binding, every erasure the store carried out is
  reported in `BoundStore::unsettled_erasures`, by its result's receipt, where the log holds no `erasure` entry for
  it (`ERASURE_NOT_RECORDED`: a crash in the gap) or an entry cites its statement (`ERASED_YET_CITED`: a citation
  that landed in the gap, which the log admits today, since it counts a statement erased from its `erasure` entry).
  Both are findings for `T-022-04`, beside `missing` and `refused_erasures`. Counting a statement erased from its
  done result, so no citation lands in the gap, is the contract author's half; the test admits either. An
  `erasure` entry counts as its record only when it is `done`: one that says `failed` after the bytes went is
  reported under its own reason (`ERASURE_RECORDED_AS_FAILED`; the review of `ce6abfb`: it hid the erasure). A
  contract follow-up for the contract author: the log should refuse an `erasure` entry with outcome `failed` after
  its effect's `done` result.
- **No erasure reaches what an entry rests on** (the review of `746e398`, S3, the coordinator's cautious default
  until the owner rules). `erasable` refuses a statement any verified entry of the log cites: by the receipt of a
  stated entry naming its commitment (a report pair, an arrival, graded evidence, a reliance, an acceptance, a
  measure, a raise, anything else), or by the commitment itself (a stuck order's decision names its evidence so)
  (`EVIDENCE_RESTED_ON`). The citation is found by searching each entry's `what` for either value, so a kind added
  later is held too. Stated entries, erasures and reads (a read check of a statement, and a permission over one)
  rest on nothing they name.
- **No erasure reaches an advertisement** (the coordinator's ruling in `T-022-01`'s notes): neither bytes whose shape
  is an advertisement's, nor bytes any stated entry of the log labels one, since the log reads back, on opening, the
  bytes of every entry labelled so (`ADVERTISEMENT_NEVER_ERASED`). A test holds that the log still opens after
  both were tried.
- **Who may erase** is policy's: an erasure is a decided action, and policy's role table answers no rule for
  `erase_statement` yet, so policy refuses every one until its rows add the operator's permission (core-internal's
  README hand-off to `F-021`).
- **Retention is the operator's** (`REQ-AUD-13`): the store erases nothing on its own, on no schedule.
- **A party's receipt** (`receipts::receipt`). In the style of RFC 9943's receipts: the log's signed word that an
  entry, by its record's digest, sits at its index in a tree of a given size whose RFC 9162 root it names, with the
  inclusion proof, the log's name and when it was issued (`PartyReceipt`, shape `urn:rutent:core-audit:party-receipt`).
  It is signed as provider-role signs a statement, Ed25519 over the canonical JSON without `signature.value`, and
  checked by the catalogue's strict verifier (`check_receipt`, which also holds it to the log it names).
  `holds_against` trusts the receipt it is given, so it is run only on one `check_receipt` answered; it checks it against whatever history
  the operator shows: the entry at its index, under the root at its size. The operator can't sign anew a receipt the
  party holds, so a rewrite or a removal before that size shows (`REQ-AUD-5`, ADR-0005).
- **The log's own key signs receipts**, never the deployment's `operator_key`, which signs the operator's statements:
  `Issuer::new` refuses it (`KEY_ACROSS_PURPOSES`). The issuer is bound once to its log and to policy's read check,
  as `Keeper` is, and only the assembly makes it. It takes the secret as an `ed25519_dalek::SigningKey` by value,
  which zeroises itself on drop (the crate's `zeroize` feature, on for `core-audit`). The deployment publishes its
  public key to parties and the checker; how is `F-030`'s and `F-033`'s. Whether `T-022-04`'s checkpoints sign with
  the same key is `T-022-04`'s to say.
- **Receipts only on an allowed read.** `Issuer::issue` asks policy first, with no borrow of the log held, then
  issues only for an entry the read covers: for `reads: evidence`, the stated entry holding that commitment (what the
  party stated, and what Core told it, its questions being the operator's statements); for an order or a segment, an
  entry naming it, a stated one by its `about` and a verified one by its `what`'s own `order` and `segment`; for the
  trail, any entry; for a resource, none (`NOT_COVERED`). A verified entry whose order sits elsewhere in its `what` is
  not covered by an order read: a reading, reversible by a later row naming each kind's order.
- **A read of an order's state covers no entry** (core-internal's fold `01e`, `Reads::OrderState`). It reads the
  order's recorded state and age alone, which is the state machine's reading of the order, not any one entry of
  the log; the inputs that make it say more (a route chosen, its decision record, a segment's evidence). So a
  receipt is issued under such a read for no entry (`NOT_COVERED`): the reading taken, reversible were an entry to
  record an order's state as such.
- **No contract shape for receipts** (a hand-off to `F-020`). The receipt is `core-audit`'s own JSON, a library
  type; a party reading it outside rutent's crates would want it under `contracts/`, which this row doesn't edit.
- **The router record's bytes** (core-internal's hand-off to `T-022-03`). The store keeps them through `keep` like
  any bytes, but the contract has no call to read them back by the `decision_record`'s unsalted `record` digest. An
  item for `F-020`.
- **Residuals for `T-025-05`.**
  - A receipt names the log's size when it was issued, so a party asking for receipts at two times learns how much
    the log grew between them: activity, which `REQ-AUD-14` keeps out of checkpoints. This is `THR-130`, on the
    form as accepted for v0.
  - A receipt's inclusion proof holds its neighbours' leaf hashes, SHA-256 of `0x00` and an entry's unsalted
    canonical JSON, so a party can confirm a guess of an adjacent entry, one about someone else's order among them.
    No entry holds a balance, so this is linkage, not `REQ-AUD-6`'s balance.
  - An erased file's blocks may stay on the disk; the store removes the file and syncs, and secure deletion is the
    deployment's storage (`F-030`).
  - A retried append of a stated entry whose bytes were erased since answers `Refused`, not its earlier receipt:
    the log attributes before its retry rule (`T-022-01`'s).
  - Cited after its erasure: the log still admits a later entry citing an erased statement's receipt, since
    `admit_entry` checks that the cited entry exists, not that its bytes do; so "refused while cited" holds at the
    erasure only. Refusing later citations needs a contract relation, an item for `F-020`.
  - Orphan bytes: a statement kept whose stated entry is never appended stays kept, named by no entry, and so no
    erasure can reach it (the review of `746e398`, S2). Orchestration keeps before it checks
    (`T-020-05`'s `record_refused`), which the coordinator routes apart.
- **A place per write** (the review of `746e398`, S2). A statement's place, the order restatements are found by,
  is taken before its write, whatever the write answers, so a write that reached the shelf before failing (its
  directory's sync after the rename, say) shares its place with no later statement.

### Tests

- `tests/receipts.rs` runs the log in its file and the evidence store in its directory, side by side under one temp
  directory, with a stand-in for policy's read check that allows or refuses every read, counts what it was asked,
  and can answer another request.
- Every rule was switched off in turn, and its test failed: the fresh salt, the commitment over the salt and the
  bytes, opening on statements that bear out their commitments and are filed under their own, the restatement index
  and its filing by signed content, an erasure dropping its place in the index, `fetch` handing nothing over on
  policy's refusal, asking policy first, reading evidence only and taking only the answer to its own read, an
  erasure only on the log's own entry at its receipt and only a `done` one, no erasure of an advertisement by its
  label or by its bytes, the shelf removing the bytes, opening removing bytes left behind their mark, an erased statement
  answering erased, a keep writing the shelf before answering, a failed write keeping nothing, a failed erasure
  erasing nothing, the directory's lock, the receipt's signature, its entry digest with its inclusion proof, a
  history holding a receipt only under its root, the issuer refusing the operator's key, taking policy's refusal
  and only the answer to its own read, issuing only for an entry the read covers, and a segment read naming its
  segment, and a kept statement checking against its entry by its commitment, its key and its signature.
- After the review of `746e398`, each new rule was switched off in turn too, and its test failed: an erasure only by
  the operator's role and only for its party, refusing a statement an entry rests on, by a stated entry's receipt and
  by its commitment alone, a read resting on nothing,
  binding carrying out a recorded erasure, refusing a mark with no erasure and reporting bytes gone with none, a
  failed write keeping its place, and a receipt checked against the log it names. After fold `01e`: a read of an
  order's state covering no entry, and a refused recorded erasure being reported, whoever recorded it. After the
  erasure fold: only a `done` result, only the log's own entry at its receipt, only what a stated entry names,
  binding carrying out a decided erasure, a mark needing its decided erasure, and a refused decided erasure being
  reported. The operator-label rules of the review of `746e398` are gone with the check. After the review of
  `f236b41`: an unrecorded, and a cited, carried-out erasure being reported; after that of `ce6abfb`, one recorded
  as failed.
- Some checks are implied by others, so switching one off alone fails no test: a receipt's key being the log's (the
  signature is verified under the log's key), its shape (only the issuer signs, and it signs that shape), its entry
  digest and its inclusion proof (each implies the other for a receipt the log signed; off together, they fail), and
  `holds_against` comparing the entry at its index (implied by the root).

## T-022-04

- **What is built.** `checkpoint::Checkpointer`, `Checker` and the `audit-check` binary, in
  `components/core-audit/src/checkpoint/` and `src/bin/audit-check.rs`, and `tests/checkpoints.rs`. Outside
  `checkpoint/`: one word, `pub(crate)`, on `receipts::receipt::timestamp`, which the schedule's times reuse; and
  `Cargo.toml`'s `[[bin]]`. Nothing in `log/`, `contracts/` or the workflows.
- **A schedule fixed in advance.** `Schedule` is a first time and a period in seconds, given by the deployment
  (`F-030`'s). Tick `k` falls at the first time plus `k` periods. A checkpoint's own time, `at`, is the
  schedule's, never the clock's at the time of writing. The checkpointer reads the log's clock only to find which
  ticks are due.
- **One checkpoint per due tick, whatever the log did** (`G-11`). `run_due` never reads whether the log moved: an
  idle log, an empty one included, ticks exactly as a busy one does. A tick that fell while nothing ran (a crash,
  a salt that couldn't be drawn, a store that failed) is issued on the next run, in order, over the log as it then
  is. Named default: no tick is skipped by catching up, and a checkpoint says nothing of when it was written (the
  deployment's publication time is the evidence of that, and is the checker's own record of when it received it).
  The alternative, skipping missed ticks so they show against the schedule, would let any downtime read as an
  attack; the checker still reports a tick the operator never published.
- **What is published, and what is not.** A checkpoint (`urn:rutent:core-audit:checkpoint`) is
  `{shape, log, tick, at, commitment, signature}`, all of fixed length: the tick is 16 hexadecimal digits, the
  commitment SHA-256, the signature Ed25519. It holds no size, no root, no salt and no count. The commitment is
  SHA-256 over a domain, the log, the tick, a fresh 32-byte salt from the operating system, the size and the root.
  **The salt is not published.** A published salt would let anyone holding a root (the empty log's, which everyone
  has, or one named by a party's receipt) test a checkpoint against it and learn the log hadn't moved, which is
  activity (`THR-130`). The salt and the size are the checkpoint's **opening**
  (`urn:rutent:core-audit:checkpoint-opening`), kept beside it in core-audit's own checkpoint store, a `Store` of
  its own apart from the log's and the evidence store's (ADR-0005; core-internal's README puts checkpoints outside
  the log), and handed to the checker with the history. So "anyone holding that history" (`REQ-AUD-14`) is read as
  the history plus its openings: a reading, reversible.
- **The signing key.** The log's own key signs checkpoints, as it signs parties' receipts (`T-022-03` left the
  choice to this row): the default, named. `Checkpointer::new` refuses the deployment's `operator_key`
  (`KEY_OF_THE_OPERATOR`). The shapes keep a checkpoint and a receipt from reading as one another, and the signing
  is the provider-role catalogue's (Ed25519 over the RFC 8785 JSON without `signature.value`, the strict verifier).
  A deployment that wants a separate checkpoint key hands the checkpointer another; the checker takes whichever
  public key it is given. Signing is on `DEC-20`'s path: for `T-025-05`.
- **Fail-closed in the checkpointer.** A salt that can't be drawn, a time that can't be built, or a store write that
  fails issues nothing and answers `Unavailable`; the tick is tried again. Since a failed write may have reached
  the store, the checkpointer reads the store back before its next issue, as the log does, and adopts what it
  holds, so no tick is ever issued twice with two salts. A store read back that holds anything but this log's
  checkpoints, signed by its key, with their openings, one tick after the last, refuses the checkpointer
  (`NOT_ITS_CHECKPOINTS`); and a log that fails the checkpoints the store already holds refuses it too
  (`HISTORY_FAILS_ITS_CHECKPOINTS`), since a new checkpoint over it would vouch for what the old ones show is
  changed. The checkpointer is only to run over a store whose review is clean.
- **The checker** (`Checker::check`) is a function of bytes: the history's records (the log's file, a record to a
  line, each leaf the hash of its line), the checkpoints held as they were received, their openings, and the log's
  public key and the schedule. It needs no `AuditLog` (whose `open` needs the evidence store's advertisement
  bytes, which a checker doesn't hold), no evidence store and no secret. It finds: a record that isn't an entry in
  its canonical form; a held checkpoint that isn't one, or is another log's, or isn't on the schedule, or isn't
  signed by the log's key; two different ones for a tick (the operator said two things); a tick with no checkpoint
  from where its watch began (`from`) through the last due, as a range; a checkpoint with no opening; one whose
  opening makes no commitment to any root of this history (`DoesNotHold`: a record altered or removed, or an
  opening that isn't its own); one whose opening names more entries than the history holds (`HistoryShorter`); and
  a size that falls from one tick to the next (`RolledBack`). The openings are the operator's, so are trusted for
  nothing: only the log it was issued over makes the commitment. A checker's watch begins at tick 0 unless the
  appointment says otherwise (`from`), and the tick still falling isn't yet missing (`closed_through`).
- **The two attackers.** `REQ-AUD-9`, `AC-22`: the test's attacker has everything: it rewrites the history's file,
  replays the whole schedule over the rewrite on a fresh checkpoint store with the log's own key, and hands the
  checker the new checkpoints and the openings of those. Shown alone they check out, which the test asserts as the
  limit: a checker that keeps no copy of what it received at publication can't see a rewrite. Held against the
  copies it received as each was published, the rewrite fails every checkpoint that committed to a history holding
  the changed record, the new ones sit beside them as a second word for every tick, and an operator withholding
  openings is itself a finding. `REQ-AUD-3`: the intruder short of control edits the history's file or the
  operator's checkpoint store and has neither the log's key nor the checker's copies; the operator's review
  (`Checker::review`, over its own two stores read as they are on the disk, run with the log closed since
  `FileStore` is locked while it is open) notices a record removed or altered, and a checkpoint removed from its
  store.
- **The checker's tool** is `audit-check` (`src/bin/audit-check.rs`), which reads three files (the history, the
  checkpoints as received, the openings) and takes the log's public key, the log's name and the schedule as
  arguments: no environment, no network, no other key (the test runs it with the environment cleared in an empty
  directory). Exit 0 clean, 1 findings (each on a line), 2 where it couldn't run. It is `REQ-AUD-10`'s means: the
  appointment (who the checker is, recorded, named to customers and providers) is `F-033`'s.
- **Not claimed.** A record appended and removed between two ticks is in no checkpoint, so none sees it; a party's
  receipt shows it (`REQ-AUD-5`), and a test pins that checkpoints alone don't. When a checkpoint is published,
  and so the checker's evidence of when it received it, is the deployment's choice (`F-030`). Equivocation and a
  rewrite are caught only if some checker kept the copies, and the same checker catches a rewrite of the checkpoint
  store only through them. `THR-131` stays accepted: a checkpoint shows consistency, not that a recorded segment
  happened. The checker re-runs none of the log's admission rules over the history (`admit_entry` needs the
  deployment's operator, and a rewrite that stays admissible is caught only by a checkpoint anyway); that is a
  possible extension, not claimed here.
- **Gaps, with owners.**
  - `BoundStore::missing`, `refused_erasures` and `unsettled_erasures` (`T-022-03`'s findings for this row) can't go
    to the outside checker, whose inputs exclude the evidence store. They belong to the operator's review, which
    needs the assembly to read them beside `Checker::review`: `T-024-02` (assembly), with `F-033` for who reads them.
  - A contract shape for a checkpoint and its opening, as for a receipt: they are `core-audit`'s own JSON, a
    library type, and a party reading one outside rutent's crates would want it under `contracts/`, which this row
    doesn't edit: a hand-off to `F-020`.
  - Where the deployment publishes checkpoints, and the schedule's first time and period, are `F-030`'s settings;
    who the checker is, and the `from` of its watch, `F-033`'s.
  - Residuals for `T-025-05`: signing (`DEC-20`) with the log's key shared with receipts; the checkpoint store's file
    holds each opening in the clear, so whoever reads it learns each tick's size, as whoever holds the history does;
    and a checker holding a checkpoint only from the operator, with no copy of its own, sees nothing (above).

### Tests and the rule-off table (`T-022-04`)

`tests/checkpoints.rs` (12 tests) and a unit test in `checker.rs`. The five tests the row names are present; the rest
hold the checkpointer's and the checker's own rules. Each rule was switched off in turn and the tests that went red:

| Rule switched off | Tests that fail |
|---|---|
| fresh salt | `a_commitment_binds_every_part_and_the_salt_is_not_guessable` |
| tick, or size, in the commitment | the same |
| checkpoint's time is the schedule's | most of the file (the checker finds none on the schedule) |
| a tick issued when the log is idle | busy/idle, due ticks, failed write |
| every missed tick caught up | busy/idle, due ticks, rewrite |
| operator's key refused | `the_checkpointers_key_is_never_the_operators` |
| open only over a history that holds / store in tick order | `removed_record_is_noticed_by_the_operators_own_review` |
| failed write read back | `a_failed_write_issues_nothing_and_never_a_tick_twice` |
| store written before answering | failed write, tool, review |
| checker: signature verified; log; records are entries | `altered_history_fails_against_a_checkpoint` |
| checker: on the schedule; opening of this log; salt of 32 bytes | `a_checkpoint_off_its_tick_and_an_opening_off_its_log_or_salt_open_nothing` |
| checker: equivocation; rollback; from | `a_skipped_tick_a_second_word_and_a_rollback_show_against_the_schedule` |
| checker: no opening; opening must make the commitment; size past the history; missing ticks | rewrite, altered, review, tool |
| review reads the checkpoint store | review |

Implied, so switching it off alone fails no test: the checker's check that the signing key is the log's (the
signature is verified under that same key).

### After the review of `9c446f1` (`T-022-04`)

- **A stored checkpoint is published at least once.** `run_due` hands out every stored checkpoint the caller hasn't
  acknowledged, not only the ones it issued that run; `Checkpointer::acknowledge(&Ran)` writes a durable cursor
  record (`Cursor`) into the checkpoint store, after the caller has published. So a write that reached the store and
  was reported failed, and a restart between the store and the answer, lose no tick: the checkpoint is handed out
  again until acknowledged. The cursor only names a stored tick and never goes back (otherwise the store is
  refused on opening, `NOT_ITS_CHECKPOINTS`); the checker's `split_kept` skips cursor records. Publication is
  therefore at least once, so a checker may be handed a checkpoint twice: identical copies are not a finding.
- **A backlog is bounded.** A run stores at most `MOST_PER_RUN` (64) ticks and hands out at most as many; the next
  run continues in order. After a long outage the catch-up takes several runs, with nothing skipped.
- **What the checker can't see, and what the appointed checker must therefore do** (requirements for `F-030`, who
  runs the deployment's publication, and `F-033`, who appoints and records the checker):
  - The checker holds the **openings** and **dated copies of the checkpoints as it received them**. A checkpoint
    carries the schedule's time, not the time it was written, so only a copy dated on receipt shows a late one.
  - It **runs each period**, so a tick caught up late is seen to arrive late against its own dated copies.
  - **Every missing opening is a finding** (`NoOpening`): a checker given a checkpoint without its opening can't
    open it, and an operator withholding openings is withholding the check.
- **Reviewer's rulings** (the review of `9c446f1`):
  - The private-salt reading (the opening, not the checkpoint, carries the salt and the size) is **accepted on
    those conditions**.
  - Backdating by catch-up is **bounded to after the last checkpoint the checker holds**: a checkpoint issued late
    for a tick can only commit to the log as it stood then, so it cannot alter what the checker already holds
    against earlier ticks.
- **Rule-off, new rules.** Each switched off, a test failed: handing out unacknowledged checkpoints (the failed
  write, the restart, the due-ticks tests); the acknowledgement being durable (restart, failed write, review); an
  acknowledgement naming a stored tick (restart); the review skipping cursor records (review, tool); the cap on
  ticks issued and the cap on checkpoints handed out (the long-outage test).
