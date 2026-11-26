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

## Rows added after review (`plan/m3-owed-rows`)

- `T-022-04`'s hand-off, a contract shape for a checkpoint and its opening, is added by `T-020-21` (`core-internal`'s
  next round) and adopted here by `T-022-07`, which waits for it.

## T-022-02

- **What is built.** `components/core-audit/src/statements/mod.rs` and `tests/statements.rs`; nothing in `log/`,
  `receipts/` or `contracts/`. Most of the row was already true of the log (`attribute`), the evidence store (bytes
  as received, restatement index) and the contract's operator-command relation; those tests were green on first
  run and are witnessed by rule-offs below. New code is only where something was missing.
- **`Statements<E: Evidence, L: Log>`**, the way a party's statement enters: refuses non-envelopes before keeping
  anything (`NOT_A_STATEMENT`), keeps the bytes, appends the stated entry unverified naming the claimed maker, and
  sets `relayed_by` to whoever presented it where that is not the claimed maker. Answers `Received` with the
  store's `restates`. The log, never this, attributes. Named default: the claimed maker is the caller's input
  (the catalogue's actor for the envelope's maker); the log refuses to verify a mismatch.
- **`restatements(entries, kept)`**, a read over the log and the store's own read that finds every stated entry
  repeating the signed content (`signing_input`) of an earlier one, so a copy appended with no relay named is found
  after the fact; `Restatement::is_passed_off` marks another maker with no relay named. A garbled-signature copy has
  the same signed content and is found. The content key is recomputed from `signing_input`, duplicating the
  store's private `content_of`. An entry whose bytes the store answers none for (erased or lost) goes to `unread`.
- **`unproven(entries, kept)`**, holds each entry marked `verified` against the kept statement: commitment, maker,
  shape, time, key and strict signature. It cannot check that the key is the maker's (the log's register).
- **Reading, reversible:** "names its operator" is the actor reference (`acts`) of the entry's `who`, not a signing key.
- **Findings and the coordinator's rulings (not fixed here; outside this row's files).**
  1. `AuditLog::open` re-attributes nothing: a store holding entries marked `verified` that no kept statement
     bears out opens fine (tested), so on the reopen path `G-09` rests on `unproven` being run by a checker with
     evidence access. Ruling: re-attribution on opening goes to a new row, `T-022-08` (wave 25, before `T-022-07` edits
     `audit-check`; it also gives `audit-check` an optional `--evidence DIR` read); until it lands the residual goes
     to `T-025-05`, which reads that row's diff.
  2. `core-orch` writes `relayed_by: None` at five sites (`snapshot/statements.rs:320`, `reversal/journal.rs:142`,
     `segments/journal.rs:109`, `arrival/journal.rs:93`, `rounds/journal.rs:76`), whatever `Kept::restates` says.
     Ruling: `F-020`'s, with `T-024-02` (the assembly may route statements through `Statements`). Row: `T-020-28`
     (wave 26).
  3. A `decide` entry whose `who` is not the request's `caller` is admitted (probed), and a stated entry's `role` is
     the writer's label that no relation reads (probed: a verified provider statement claimed in the customer's role
     is recorded so). Ruling: both go to `T-020-21` as contract relations; its row now lists them, (12) `decision-is-for-its-caller`
     and (13) the role check in `attribute()`.
  4. A maker's resend of its own statement and a relay's copy passed off as the maker's with the same presenter
     identity read alike; only `Presented::presented_by` tells them apart. Ruling: a residual for `T-025-05`, whose
     row now lists it, with the checkpoint store's unsigned cursor records (`T-022-04`).
- **`unproven` holds what the append does** (after approval): the envelope's `deployment` is the one given (the
  deployment's own, a new argument) and the entry's `about.transaction` is the body's transaction (none where the
  body names none, and a statement about an order or segment names one), beside commitment, maker, shape, time, key
  and signature. The misdated case of the unverified-signature test appends genuine bytes under an entry dated
  otherwise; it passed at once (the contract's `attribute` already held it) and went red with that check off.
- **Rule-off table** (each switched off once, tests run, tree restored): intake's `relayed_by` -> `restated_quote_is_detected`;
  intake keeping stripped bytes -> all four others that attribute, incl. `signature_is_kept_with_the_bytes_it_signed`;
  scan filing by raw bytes -> `restated_quote_is_detected`; scan dropping the relay -> same; `unproven` without
  the signature check, and without the maker check -> `statement_whose_signature_doesnt_verify...`; log's strict
  verifier always true -> signature, restated, kept tests; log's `admit_entry` off ->
  `operator_commands_entry_names_its_operator` and `one_party_with_several_roles_is_recorded_per_role`. None hung.
  Added: `attribute`'s `made_at` check off -> `statement_whose_signature_doesnt_verify...`; `unproven`'s deployment
  check off, and its transaction check off -> `unproven_holds_the_deployment_and_the_transaction_the_append_does`.

## T-022-05

- **Layout.** `components/core-audit/src/compare/`: three pure comparisons, `report` (`compare_reports`), `holding`
  (`check_holding`) and `evidenced` (`value_evidenced_against_balances`), and the detectors, `detect::Compare`, which
  records and raises. `log::Unbuilt` keeps its `Comparisons` impl (the assembly's stub still builds on it).
- **No reach to anything that moves value, structurally.** (1) The pure modules import only data types of
  `rutent-contracts` and their siblings; they take `&` references and return a verdict, so they hold no handle, no
  channel, no clock and no mutable state. A test reads their source and holds that: only `rutent_contracts::` and
  `super::` imports, none of `Log`, `Operator`, `Clock`, `calls::`, `&mut`, cells, `dyn`, `Box`, `unsafe`, `static`,
  `std::fs/net/process/env/time`, `crate::`, and the three functions coerce to plain `fn` pointers on `&` arguments.
  (2) `Compare<L, O, C>` is the only thing with effects, and its type is its reach: a `log::General<L>` (implements
  `Log` alone: no decision entry, no statement attributed), an `Operator` (whose `raise` takes a `Raise`, a
  difference and receipts, and answers only a `Delivery`) and a `Clock`. (3) `core-audit` depends on neither
  `core-orch` nor `core-authz` (`DEC-2`), which a test reads from the manifest. ADR-0009's placement in
  `CMP-CORE-AUDIT` rests on these.
- **Order of steps.** A report pair: record the pair (`report_pair`, observed), compare, and where the amounts
  differ, deliver the raise and record it (`raise`, `done` or `failed` by the delivery, as the contract says). The
  answer is the pair's receipt in every case. A holding reading: record the check without its balance
  (`holding_check`), compare, raise. A raise cites the pair's (or check's) entry, and for a pair the two statements.
  A raise whose record can't be appended answers `Unavailable`, so orchestration appends the comparison again (the
  pair's and check's entries are idempotent); the operator may then see the raise twice. A comparison whose own
  record is refused or unavailable compares nothing.
- **Raise id and time.** The id is `raise-` and SHA-256 of the cites, time and difference; the time is the clock's,
  to the second (`compare::detect`'s civil-date conversion, since no formatter existed). The same difference at the
  same second is one raise id.
- **Defaults taken** (all fail closed or reversible):
  1. Reports are compared **exactly**. ADR-0009 ties a tolerance to the provider's declared increments
     (`REQ-CAT-11`) and its gross or net convention; `ReportPair` carries neither, so no tolerance is applied and a
     rounding gap is raised rather than hidden.
  2. Two reports of different assets (id or decimals) are `NotComparable(AssetsDiffer)`, even if the amounts match.
  3. A holding account's per-order sum past 78 digits is `NotComparable(SumPastTheBound)`.
  4. `INV-1`'s "value evidenced at nodes" is an input, `Evidenced { account, order, asset, amount }`, summed per
     account across orders and compared with each reading's balance by the same rule as `REQ-AUD-15`; a difference
     is a `Difference::Holding` carrying the balance's `word`. Evidence in another asset than the account's is
     `NotComparable(EvidencedInAnotherAsset)`; evidence at an account with no reading is `NotComparable(NoReading)`.
  5. A `NotComparable` verdict records the pair or check and raises nothing.
  6. The `core-audit` actor is a parameter (`Compare::core_audit(party)` builds the fixtures'
     `rutent`/`core-audit`/`core`/`component`).
- **Gaps, with owners.**
  1. `Difference` has no shape for a pair or reading that can't be compared (asset or unit differs, sum past the
     bound, evidence unread). Such a case is silent today. The contract author (`core-internal`, `T-020-21`'s
     round) needs a `Difference` variant, e.g. `NotComparable { subject, reason }`, to raise it.
  2. `Comparisons` has no method for the `INV-1` check, so it is `Compare::evidenced_against_balances`, an inherent
     method. Who reads the log into `Evidenced` is the assembly's (`T-024-02`, `T-024-08`); the log's records
     don't name a value resting at a node, so the feed is not built here.
  3. The ranking of raised mismatches by value and age (ADR-0009 consequences) is left to construction and not
     built: it is `F-023`'s (the operator's channel), `T-023-02`.
  4. Tolerance by increment and gross/net convention (default 1) needs `Report` to carry them, or the caller to
     pass the tuple's terms: the contract author (`core-internal`, `T-020-21`'s round), with `F-020` supplying them.
  5. A balance of a private node is its publisher's word and nothing more; `Word::Publisher` is read from the
     reading, set by orchestration. That a reading's `word` is true to its node (public node, `Checked`) is
     orchestration's (`T-020-26`).
- **Tests.** The report test runs on the real log replaying the contract's world up to its pair, so every entry
  written is admitted by the citation table (`sent-report-is-its-partys-statement`, `raise-cites-entries-of-the-log`);
  the holding tests run on the real log standing alone. The row's tests were written after the code in this session
  and then attacked by the rule-offs below, not seen red first; the property tests are seeded xorshift (400 cases
  each, a failing case names its seed, each outcome asserted to be drawn).
- **Rule-off table** (each switched off once, `compare` tests run, tree restored):

  | Switched off | Red |
  |---|---|
  | `compare_reports` always `Match` | pure-function, report-raises |
  | its asset check | pure-function |
  | sent and received swapped in the difference | pure-function, report-raises |
  | the operator not called | report-raises, both holding tests, `INV-1` |
  | an undelivered raise recorded `done` | report-raises |
  | the raise not recorded | report-raises, short/over, `INV-1` |
  | a pair raised though it matches | report-raises |
  | the pair not recorded | report-raises |
  | short and over swapped | short/over, private-node, `INV-1` |
  | the balance's word dropped from the difference | private-node, `INV-1` |
  | over ignored | short/over, `INV-1` |
  | a holding difference never raised | short/over, private-node |
  | the check entry's word wrong | private-node |
  | only the first evidence item summed | `INV-1` |
  | evidence's asset not checked | `INV-1` |
  | unread accounts dropped | `INV-1` |
  | `INV-1` differences not raised | `INV-1` |
  | a pure module importing `crate::log` | structure test |
  | a `&mut` in a pure module | structure test |

  None hung.

### The review's fix round (`T-022-05`, after `3db31459`)

The review found CHANGES: a comparison that couldn't be made was silent, so `REQ-ORD-9`, `REQ-AUD-15` and `INV-1`
failed open. This round needed a contract change, made under that rule's lift for exactly this case.

- **The contract (`core-internal` 1.0.0, folded in before its release, no bump: it is unreleased against
  `develop`).** Two kinds of `Difference`, each with its schema `$def` in `raise.schema.json` and an arm of
  `Raise::check`:
  - `reports_not_comparable { order, segment, sent: {amount, asset}, received: {amount, asset} }` (`Reported`): two
    reports in two assets, or one asset in two units (`THR-107`), each figure as its party stated it. `Raise::check`
    refuses one whose two assets are one (`raise-of-reports-not-comparable-in-one-asset`), and accepts equal amounts
    where the assets differ (`raise-of-reports-of-one-amount-in-two-units`). Chosen over an optional
    `received_asset` on `reports`, which would have let `reports` carry equal amounts and weakened its rule.
  - `holding_not_comparable { account, word?, asset?, reason }`, `reason` one of `evidenced_in_another_asset`,
    `no_reading`, `sum_past_the_bound` (`HoldingIncomparable`). `word` and `asset` (the account's, as its balance
    was read) are present exactly when a balance was read, so never for `no_reading`, where there is no balance
    whose word or asset they could be; the schema (`if`/`then`/`else`) and `Raise::check` both hold it. The asset is
    in the raise because `holding_check` names none (added on re-review).
  - Five valid fixtures (reports in two units and in two assets among them), five invalid (one Rust rule, four
    schema rules); README's rule, shape and record tables and the changelog. `Raise::check`'s refusal text gained "two reports in two assets or units"; it still contains the
    old text, so existing tests matching it hold.
  - The other users of `Difference` in the workspace: the only exhaustive match is `Raise::check`; the rest
    (`relations.rs`, `core-orch`'s charge and stuck raises, the scenarios' tests) use `let ... else`, `matches!` or a
    `_` arm, so nothing else changed and no behaviour was added there.
- **The checks raise each.** `ReportsVerdict::NotComparable` and `HoldingVerdict::NotComparable` now carry the
  contract's difference, and `Compare` raises on `Differs` and `NotComparable` alike, records the raise, and changes
  nothing else. The local `Incomparable` enum is gone. `INV-1`: `Evidenced` carries `evidence`, the receipt of the
  log's entry that evidences the value, so a raise about an account cites what it rests on; an account read cites its
  check and those entries, an unread one those entries alone (no check is recorded for it: there is no reading).
- **Defaults taken this round** (fail closed, reversible):
  7. A balance is compared with the held or evidenced amounts by walking them against it, never forming the sum,
     so a sum past 78 digits whose short is one an amount states is that `short` (held `[10^78-1, 1]` against
     `10^78-1` is short by 1). Only a short past what an amount states is raised as `holding_not_comparable` with
     `reason: sum_past_the_bound`, the balance's word and the account's asset, never as a `short` with a `by` that
     would understate it. An over is never past the bound, being at most the balance. (The first cut of this round
     called every such sum past every balance by more than an amount states; the re-review showed it wrong.)
  8. A raise for `INV-1` cites the check (where read) and every distinct evidence receipt at the account, in the
     evidence's order.
  9. A deterministic refusal of a raise's record is answered `AppendFailure::Refused`, which no caller retries;
     only an unavailable log is `Unavailable`. The contract allows it (`Refused` is the variant for a broken rule),
     but see gap 6. In `INV-1`'s check over many accounts a refusal (of a check's record or a raise's) doesn't stop
     the accounts after it: each is still compared, checked and raised, and the first refusal is answered at the
     end; an unavailable log stops the run, and the caller runs it again (added on re-review, which found the first
     refusal stopping every account after it, the fail-open class this row closes).
  10. `Receipt` is allowed in a pure module (`Evidenced` holds one): it is a plain value with no method reaching the
      log. The structure test no longer forbids it.
- Defaults 2 and 5 above are replaced: different assets or units are raised, and a `NotComparable` verdict is raised.
  Gap 1 above is closed.
- **Gaps added, with owners.**
  6. `AppendFailure`'s doc says nothing was appended; a comparison answered `Refused` because its raise's record was
     refused has appended its own record (the pair or check) first, and the operator's channel was already handed the
     raise. In `INV-1`'s check over many accounts it is more: the answer `Refused` follows a run in which every other
     account was checked, raised and recorded, so the log and the operator hold all of that, and only the refused
     record is missing; the answer doesn't say which account it was (the operator saw its raise, and the log holds its
     check). A comparison-specific answer, or a note on `Comparisons` that the comparison's records stand and naming
     the refused one, is the contract author's (`core-internal`), with the caller's handling `F-020`'s, which has no
     caller of `Comparisons` yet. A check's record refused with no evidence to cite leaves its raise with nothing to
     rest on, which `Raise::check` refuses before delivery: that account is not raised, and its refusal is the one
     answered if it came first.
  7. Closed on re-review: `holding_check` names no asset, so `holding_not_comparable` now carries the account's
     asset where a balance was read. For `no_reading` no account asset is known; the evidence's asset is the feed's
     (`T-024-02`), in the entries the raise cites. The other asset of `evidenced_in_another_asset` isn't in the raise
     either; if the operator's channel (`F-023`, `T-023-02`) needs it, that is a contract change.
- **The structure test, hardened.** Every `use` is read whole up to its `;`, `pub`, `pub(crate)` or private and
  wherever it stands; `super::` reaches only the pure siblings (`report`, `holding`, `evidenced`), so neither
  `super::super`, `super::detect` nor `mod.rs`'s re-exports pass; a path written in place is read too; no crate named
  `rutent_*` but `rutent_contracts` may appear (prov, stm, router, evidence, core); `extern crate`, `include!` and
  `#[path` are refused. The test holds its own reader to the review's three dodges and eight more.
- **The purity generator** draws, for reports apart, another asset (`EURC`) or the same code in other decimals
  (`THR-107`'s unit case), and asserts the unit case was drawn more than ten times in 400.
- **Tests first.** Contract: the seven fixtures and their table rows were added first and seen red on the fixture
  test's assertions (unknown kinds); then the shapes with `Raise::check`'s two arms switched off, seen red on the two
  rules; then the arms. `core-audit`: the behaviour tests (each not-comparable case raises, the refusal answer) were
  written first; the pure layer's verdict payload had to change for them to compile, so it was put in first with the
  detector untouched, and they were seen red on their assertions (nothing raised, `Unavailable` answered); then the
  detector. The hardened structure test was seen red on the current tree (`use super::Incomparable`, an item of
  `mod.rs`, not a pure sibling) before the pure layer dropped it. The generator change adds no code: its red is the
  rule-off of comparing the asset's id alone.
- **Rule-off table for this round** (each switched off once from the committed tree, the named tests run under a
  300 s alarm, the tree restored from git; none hung):

  | Switched off | Red |
  |---|---|
  | `Raise::check`'s `reports_not_comparable` arm (`=> false`) | contract fixture test (`raise-of-reports-not-comparable-in-one-asset` accepted by Rust) |
  | `Raise::check`'s word rule | contract fixture tests (`raise-of-an-unread-account-naming-a-word`, `raise-of-a-holding-not-comparable-without-its-word` accepted by Rust) |
  | the schema's word rule (`if`/`then`/`else`) | contract fixture test (both word fixtures' schema errors) |
  | a report pair that can't be compared not raised | report-raises |
  | a holding that can't be compared not raised | short/over (sum past the bound), `INV-1` |
  | unread accounts not raised in `INV-1` | `INV-1` |
  | the evidence's receipts not cited | `INV-1` |
  | a refused raise record answered `Unavailable` (the old mapping) | `INV-1` (the refusal case) |
  | reports compared by the asset's code alone | pure-function (the unit case; the old generator drew only `EURC`, which differs by code, so it would have stayed green), report-raises |
  | evidence compared by the asset's code alone | `INV-1` |
  | a sum past the bound read as a match | short/over |
  | the balance's word dropped from a holding that can't be compared | short/over, `INV-1` (`Raise::check` refuses the raise) |
  | `pub use super::super::log::General` in `holding.rs` | structure test |
  | a multi-line `use ...::{ calls::Operator as X }` in `holding.rs` | structure test |
  | an inline `rutent_prov::...` path in a body in `holding.rs` | structure test |
  | `use super::detect::Compare` in `report.rs` | structure test |

  The first two rows were one run (both arms off), each fixture's failure naming its arm. The structure test also
  holds its reader to these dodges and eight more in its own body, and was red on the tree before this round (`use
  super::Incomparable`). A first cut of the reader split `super::holding::{a, b}` on every comma and refused it: a
  bug of the test, fixed to read a braced group by its top-level items, with a case holding it.

### The re-review of `032378d1` (`T-022-05`)

CHANGES: one should-fix and five lows, each done.

- **A refused raise stopped the accounts after it (should-fix).** `evidenced_against_balances` returned at the first
  refused record, so a later account that differed was never checked or raised. It now keeps going, and answers the
  first refusal at the end (default 9, gap 6); the reviewer's probe is a case of the `INV-1` test.
- **A short past the bound.** `judge` walks the amounts against the balance instead of summing them (default 7), so
  held `[10^78-1, 1]` against `10^78-1` is short by 1, as is evidence summed the same way.
- **The account's asset** is in `holding_not_comparable` where a balance was read (gap 7 closed): schema, check,
  fixtures (`raise-of-an-unread-account-naming-an-asset`, `raise-of-a-holding-not-comparable-without-its-asset`).
- **A valid fixture of reports in two assets**, `raise-of-reports-in-two-assets`, beside the units one.
- **The reader:** no space is left round `::`, `.` or before `(`; capabilities (`calls::`, `Log`, `Operator`,
  `Clock`, `Comparisons`) are refused anywhere in the code, not only in a `use`; a `mod` declaration, in a file of its
  own or inline, is refused, and so is a directory of child modules beside a pure module. Its self-check gains the
  re-review's three dodges and three more.
- **The stale comment** in the holding test now says the past-the-bound case is raised.
- **Tests first.** Contract: the new and changed fixtures and their rows were red on the fixture test's assertions
  (`asset` unexpected); then the shapes with `Raise::check`'s asset clause absent, red on the two asset fixtures
  alone; then the clause. `core-audit`: the asset had to be plumbed through for the tests to compile, and with only
  that, the tests were red on their assertions: the reader's self-check (spaces round separators), the short past the
  bound in both the holding and `INV-1` tests. The `INV-1` test stopped at its earlier past-the-bound case, so the
  refusal case's own red is its rule-off (N1).
- **Rule-off table for the re-review** (from `ecb979d4`, each once, 300 s alarm, tree restored; none hung):

  | Switched off | Red |
  |---|---|
  | N1: the first refused raise stops the accounts after it | `INV-1` ("hold-y's over is raised after hold-x's refusal") |
  | N2: a sum past the bound always read as past it (the old sum) | short/over, `INV-1` |
  | N3: the account's asset dropped from a holding that can't be compared | short/over, `INV-1` (`Raise::check` refuses the raise) |
  | N4: the schema's asset rule (back to the word alone) | contract fixture test |
  | `Raise::check`'s asset clause | contract fixture test (the two asset fixtures accepted by Rust; the stage before the clause) |
  | N5: the reader keeps spaces round separators | structure test (its self-check) |
  | N6: capabilities checked inside a `use` only | structure test |
  | N7: a module declaration allowed | structure test |
  | N8: `super :: detect :: …` in `holding.rs` | structure test |
  | N9: an inline `calls::Operator` bound and `o . raise (r)` in `holding.rs` | structure test |
  | N10: `mod sneak;` in `holding.rs`, with `holding/sneak.rs` reaching `crate::compare::detect` | structure test |

## T-022-08

The open path's residual from `T-022-02`'s finding 1 is closed: a store holding a stated entry marked `verified` that
the statement kept behind it contradicts no longer opens as sound, and `audit-check` can hold the history to the
evidence.

- **What is built.** `components/core-audit/src/log/mod.rs` (the opening), `src/statements/disk.rs` (`KeptOnDisk`),
  `src/bin/audit-check.rs`, `tests/reopen.rs`; two tests of `tests/statements.rs` changed (below). Nothing in
  `contracts/`, `measures/` or `receipts/`.
- **One attribution, not two.** `attribute_stated` is the log's only attribution of a stated entry: the bytes kept
  under its commitment held to it (SHA-256 of the salt followed by the bytes), then `core-internal`'s `attribute` over
  the history and the register before the entry, with `Ed25519Strict`. The append runs it on every stated entry, as
  before (its answers are unchanged: nothing kept is `STATED_WITHOUT_ITS_KEPT_BYTES`, bytes that don't bear out the
  commitment `KEPT_BYTES_DIFFER`). The opening runs it again on every stated entry the store marks `verified`, with
  the entry's attribution turned back into the claim (`claimed_by` = the `who` it names), over the history and the
  register read back to that entry's place: the same inputs its append had, since neither `Register::record` nor
  `Keys` reads the clock (only `Register::admits` does, which the opening doesn't run). So the opening holds an entry
  to every check `unproven` makes (commitment, deployment, maker, shape, time, key, transaction, strict signature)
  and, from the register, that the key is the maker's at `made_at` and the party the one it states for.
- **What the opening answers.** The re-attributed entry must equal the entry the store holds, field for field. Where
  it doesn't (unverified, or verified otherwise), or the kept bytes don't bear out the commitment, the opening is
  refused: `AuditLog::open` answers `Refused(KEPT_STATEMENT_CONTRADICTS)`, and `AuditLog::open_naming` answers
  `NotOpened::Contradicted { entry }`, the receipt the entry has at its place, because `AppendFailure` holds a
  `&'static str` and can't name it. `open_naming` is `core-audit`'s own constructor, no contract shape. Nothing is
  repaired or dropped: the store isn't written. Where the evidence store keeps nothing under the commitment, the
  statement was erased since, and the log opens (`REQ-AUD-13`, `AC-36`); the bound store already reports bytes gone
  with no erasure (`BoundStore::missing`). An entry marked unverified is held to nothing.
- **`audit-check --evidence DIR --deployment NAME --operator-key KEY`.** `KeptOnDisk` reads the evidence store's
  directory as `DirectoryShelf` writes it (`kept/<commitment>.json`, `erased/<commitment>`), taking no lock and
  writing, removing or repairing nothing; each statement is answered as filed, so bytes that don't bear out their
  commitment reach `unproven` and are found. `read_history` (in `log/`) gives each canonical record its receipt
  (SHA-256 of the record, at its place, in the log `--log` names), passing over a record that isn't an entry, which
  the checker already reports. The tool runs `unproven` over it: each finding prints `NotBorneOut { index: N }` and
  exits 1; each erased (or lost) statement prints apart as `unread: entry N's ...` and changes no exit code; with no
  finding it prints that every verified entry whose statement is kept is borne out. Without `--evidence` it prints
  `the evidence was not read: ...` after what it printed before, and its exit codes are as before. The evidence is
  read whole before anything is printed. It reads no environment and no network.
- **Tests first.** The eight tests of `tests/reopen.rs` were written first, against stubs (`NotOpened`,
  `open_naming` that only called the old `open`, `read_history`; the tool ignoring the new flags), and six were red
  on their assertions: the two refusals (`left: None`), the unreadable store (`left: None`), and the three tool tests
  (exit 0 where 1 was expected; no "borne out", no "not read" line). Two were green on the old code, as they should
  be: `store whose verified entries are all borne out opens as before` and `store whose verified entry's statement
  was erased still opens` hold what the opening already did, and their red is their rule-off (R6, R4).
- **`T-022-02`'s tests that changed, and why (coordinator's ruling).** Two tests built their history by reopening
  the log on a store holding forged `verified` entries, which this row now refuses on purpose:
  `statement_whose_signature_doesnt_verify_is_not_logged_as_attributed` asserted the old behaviour ("the log opens:
  it re-attributes nothing on opening"), and `unproven_holds_the_deployment_and_the_transaction_the_append_does`
  opened it only to get receipts. Both now assert the opening is refused (`KEPT_STATEMENT_CONTRADICTS`) and read the
  history with `read_history` instead; their `unproven` checks are unchanged, places and all. Nothing is weakened:
  each gained an assertion. Every other test of `T-022-01`, `T-022-02`, `T-022-03` and `T-022-04` passes unchanged.
- **Defaults taken** (each reversible, each failing closed):
  1. **Erased is "nothing kept"; unreadable is a failure.** `KeptStatements::kept` answering `Ok(None)` is an erasure
     (or a loss, which `BoundStore::missing` reports) and the log opens; answering `Err` is a store that can't be
     read, and the opening fails `Unavailable`: the log doesn't open as sound, and the caller opens it again. A test
     holds it (`evidence store that can't be read fails the opening`).
  2. **Bytes that don't bear out the commitment contradict the entry**, and refuse the opening; on append they are
     refused as before.
  3. **A statement answered under another commitment than the one asked is nothing kept under it** (as `unproven` and
     the append already read it): unread, not a contradiction.
  4. **`--operator-key` is part of the deployment's name.** `unproven` compares a statement's `deployment`, which
     holds the operator's public key; the tool takes it only to compare, never to verify or sign with. "No key but
     the log's public one" is read as no key the tool verifies or signs with (coordinator's ruling). Named in the
     tool's help.
  5. **An evidence directory the tool can't read is no evidence store** (missing, or missing `kept/` or `erased/`, or
     a file the store doesn't write, or one filed under another commitment): exit 2, never an erasure.
  6. **A half-written statement (`.part`) is passed over, and one marked erased whose bytes are still filed is taken as
     erased**, as the store takes each on its next opening.
  7. **`--deployment` or `--operator-key` without `--evidence` is an argument error** (exit 2), so no one believes the
     evidence was read.
- **What opening costs, and no bound built.** Per stated entry marked `verified`: two reads of the evidence store
  (the shared step and `attribute` each fetch, as on append; `LogsRead` is a hash lookup in memory) and one strict
  Ed25519 check, beside the read back the log already made. For a statement by a module the operator publishes,
  `attribute` also calls `release_statement_keys`, which walks the history's catalogue changes and, for each, the
  history again for its `done` result: about c x n per such statement (c catalogue changes, n entries), so the opening
  is O(v + m x c x n) for v verified entries of which m are a published module's. The append already paid the same
  per statement; the opening pays it all at once. The stale path after a failed write reads back, and so
  re-attributes, the whole store. The row asks for no bound or cache, so none is built. Gap 2.
- **Gaps, with owners.**
  1. `AppendFailure` can't name the entry a refusal is about, so `open` names it only through `open_naming`. If the
     assembly needs it on the plain path, that is a contract shape: the contract author's (`core-internal`,
     `T-020-21`'s round), e.g. a `Refused` that carries a receipt.
  2. Opening cost on a big store (above): `catalogue_changes` and `release_statement_keys` rescan the history per
     call. An incremental release table, kept as the register is, would make it O(v); that is the contract author's
     (`core-internal`), and measuring it at production size is the assembly's (`T-024-02`, `F-030`).
  3. `audit-check` holds no register, so with `--evidence` it can't tell a statement signed by another maker's key in
     this maker's name (the `unproven` limit, `T-022-02`): `tests/reopen.rs` shows `unproven` clean on it while the
     opening refuses it. Rebuilding the register from the history and the kept advertisements would close it in the
     tool; it is `T-022-07`'s or a later row's call (`F-022`), and the checker appointment is `F-033`'s.
  4. Default 6 (an erased mark with its bytes still filed) has no test of its own: a real erasure leaves no such file
     but by a crash between mark and removal. `T-025-05` reads this row's diff.
- **Rule-off table** (each switched off once from `80352c46`, the four test binaries run under the lock with a 400 s
  alarm, the tree restored from git):

  | Switched off | Red |
  |---|---|
  | R1: no re-attribution on opening | both refusal tests, the unreadable store |
  | R2: the re-attributed entry not compared with the stored one | both refusal tests |
  | R3: bytes that don't bear out the commitment taken as no loss | refusal test (the swapped-bytes case) |
  | R4: nothing kept refuses the opening | erased-still-opens |
  | R5: an evidence store that can't be read taken as erased | the unreadable store |
  | R6: every re-attributed entry refused | borne-out-opens-as-before, erased-still-opens, both refusal tests (each names the wrong entry), `T-022-01`'s `reopening_refuses_an_advertisement_...` |
  | R7: the register's key check always passes (`Keys::signs_for`) | the other-maker's-key case of the maker refusal test; ten of `T-022-01`'s tests and `T-022-02`'s `restated_quote_is_detected` (the same check on append) |
  | R8: the tool not running `unproven` | both tool tests with the evidence |
  | R9: the tool counting an unread statement as a finding | the tool's erased-statement test |
  | R10: the "evidence was not read" line dropped | the tool's test without the evidence |
  | R11: an evidence directory that can't be read taken as empty | the tool's test with the evidence (its exit-2 case) |

  The first batch ran without `--no-fail-fast`, so R6 and R7 stopped at `tests/log.rs`; both were run again with it,
  as the table shows. None hung.

### The review of `bfe5144d` (`T-022-08`)

CHANGES: one should-fix and one low, each done. The log's refusal path, the register as it stood before each entry,
the two changed `T-022-02` tests and `--operator-key` held.

- **Tampering passed as a lawful erasure (should-fix).** Both the opening and the tool took any statement the store
  didn't keep for an erasure. The reviewer's probes: (1) a forged `verified` entry whose kept statement was then
  deleted opened, and the tool exited 0 with only an `unread` line; (2) a forged entry beside a hand-made empty
  `erased/<commitment>`, its contradicting statement still filed, did the same (default 6 failed open). An erasure is
  two things the history and the store both hold: the history's decided erasure of the commitment (the `done` result
  of a permitted `erase_statement` effect, which the bound store erases on) and the store's mark. Anything short of
  both is now refused or found.
  - **The opening** (`log/mod.rs`). A `verified` entry whose statement isn't kept is held, once the whole store is
    read back (an erasure is decided after the entry it erases), to the decided erasures the history holds
    (`log::decided_erasures`). With none for its commitment it is gone, not erased, and the opening is refused:
    `Refused(STATEMENT_GONE_WITHOUT_ITS_ERASURE)`, named by `NotOpened::Gone { entry }`. The log's own read
    (`KeptStatements`) can't see the store's marks; the bound store holds those to the history when it is bound
    (`MARK_WITHOUT_ITS_ERASURE`). Probe 2 is refused the same way on the opening: the evidence store, opening, removes
    the bytes behind a mark (`DirectoryShelf::load`), so the log finds nothing kept and no decided erasure.
  - **The tool** (`audit-check --evidence`, `KeptOnDisk::erasures`). Each stated entry whose statement isn't filed is
    erased only where the history holds a decided erasure of it and the store marks it (printed `erased: entry N's
    ...`, no finding). Otherwise it is a finding, exit 1: `StatementLost { index }` (neither filed nor marked; a
    decided erasure whose mark is gone included), `MarkWithoutItsErasure { commitment }` (a mark with no decided
    erasure), `MarkedYetFiled { commitment }` (a mark whose bytes are still filed). Erased and lost are now apart in
    the output. Lost is read over every stated entry, as the bound store's `missing` is, not only the verified ones.
  - **One reading of an erasure, not two.** `receipts::store::decided_erasure` was private; it is now `pub`, a
    one-word change in `T-022-03`'s file, so the log and the tool read a decided erasure exactly as the bound store
    does before it erases.
- **Defaults this round** (reversible, failing closed; they replace default 1's erasure half and default 6):
  8. **The opening refuses a gone statement** rather than opening and telling the operator. `REQ-AUD-13` and `AC-36`
     ask that history still check after an erasure; a statement gone with no erasure is no erasure, so neither asks
     the log to open on it, and `REQ-AUD-2` holds a verified attribution to bytes that bear it out. Only `verified`
     entries are held: an unverified entry's bytes gone are the bound store's report (`BoundStore::missing`), as
     before, which no `T-022-03` test changes.
  9. **A mark whose bytes are still filed is a finding** (default 6 replaced). The reader can't tell an erasure cut
     short between its mark and the removal from a mark made by hand; either way the bytes are filed, so
     `KeptOnDisk` answers them and `unproven` holds the entry to them.
  10. **A decided erasure whose statement is still filed and unmarked** (an erasure decided and not yet carried out,
     which binding carries out) is no finding and no erasure: the bytes are there and the entry is held to them.
- **The low.** `store still opens after a trusted provider's grant is revoked later`: a done `revoke_grant` after the
  provider's verified quote; the store reopens with the same entries and root, the quote still verified, since it is
  held to the register as it stood before it.
- **Tests this round.** Open: `store whose verified entry's statement is gone with no erasure is refused on opening`
  (an honest entry's statement removed by hand; probe 1; probe 2). Tool: `audit check with the evidence reports a
  statement gone with no erasure as a finding` (probe 1, probe 2, a mark made by hand over removed bytes), and the
  erased test now removes a real erasure's mark too and finds it lost. They were written with the fix, not before it:
  against `bfe5144d` they wouldn't compile (`NotOpened::Gone`, `STATEMENT_GONE_WITHOUT_ITS_ERASURE`), so their red is
  their rule-offs below.
- **Opening cost, added.** Only where a `verified` entry's statement isn't kept: one walk of the history per `done`
  result to find the decided erasures (`decided_erasure` looks each step up by receipt), so O(r x n) for r done
  results, paid once per opening.
- **Gaps added.**
  5. The opening doesn't run `admit_entry` over what it reads back, so someone with full access to the log's file who
     forges a whole erasure path (a permitting `decide`, its effect and its `done` result) beside a forged entry, and
     a mark, is taken for an erasure by the opening and the tool alike. A rewrite before the last checkpoint is the
     outside checker's (`DoesNotHold`); after it, nothing here sees it until the next tick. Re-admitting the history
     on opening is a cost and design call for `F-022` (with `T-025-05`, which reads this diff).
  6. Gap 4 above (default 6 untested) is closed: default 6 is replaced by default 9, which probe 2 tests.
- **Rule-off table for this round** (each switched off once from `41f207b8`, the four test binaries run with
  `--no-fail-fast` under the lock and a 400 s alarm, the tree restored from git):

  | Switched off | Red |
  |---|---|
  | N1: a gone statement taken as erased on opening (no check) | gone-is-refused-on-opening |
  | N2: decided erasures ignored on opening (every gone statement refused) | erased-still-opens |
  | N3: a lost statement no finding in the tool | the tool's gone test (probe 1 exits 0), the tool's erased test (its mark-removed case exits 0) |
  | N4: a mark with no decided erasure not reported | the tool's gone test (probe 2, and the hand-made mark over removed bytes) |
  | N5: a mark whose bytes are still filed not reported | the tool's gone test (probe 2) |
  | N6: erased by the mark alone | the tool's gone test (the hand-made mark prints `erased:`) |
  | N7: erased by the decision alone | the tool's erased test (its mark-removed case prints `erased:`), the tool's gone test (the hand-made mark over removed bytes reads as lost) |
  | N8: a statement filed behind a mark not answered (default 6 back) | the tool's gone test (probe 2: no `NotBorneOut`, no `MarkedYetFiled`) |

  None hung. This round's patches of `R2`, `R3`, `R4`, `R6` and `R9` no longer match the tree (`Box`, the gone push);
  those rule-offs ran against `80352c46` and the rules they switch off are unchanged in substance.

## T-022-06

- **Layout.** `components/core-audit/src/measures/`: `compute` (pure: `measure(history, window) -> Report`), `record`
  (`Record<L, C>`, the `Measures` impl: appends each `MeasureRecord` through a `General` handle on the log and a clock,
  nothing else) and `trail` (`read_trail`, a regulator's read). `log::Unbuilt` keeps its `Measures` impl.
- **Measures come from the log.** `measure` takes the log's entries as `AuditLog::entries()` hands them out and a
  `Window`; it counts the log's `measure` entries and nothing a caller supplies. Each measure is per provider or
  verifier (the record's `provider`, keyed by its text) and `COUNTS` says in words what each counts. The window is on
  the record's own `at`, half open, read by the instant (fraction to the nanosecond).
  Quotes: per asset (code and decimals), executed below, at and above the quote, with the total shortfall and excess
  (`None` once a total passes 78 digits, never wrapped). Dry runs: yes/no against done/failed. Failures: by market
  direction, with no figure counted apart. Refusals, reversals. Answers: yes/no, rate of no (`Rate`, counts, per
  million on request, no floats).
- **Not a measure of a measure.** The measures are structural about reaching nothing that moves value as `T-022-05`'s:
  `compute` imports only contract types, `std::collections` and `std::cmp`, takes `&` references, and a structure
  test reads its source (the reader is `compare`'s, copied, with those two std imports allowed). `Record`'s reach is
  its type (`General<L>`, `Clock`); `read_trail` is generic over `ReadChecks`, a one-method trait with a blanket impl
  over policy's `Reads`, so it cannot name grant or revoke. The test reads their sources for it. `DEC-2` unchanged.
- **The regulator's read.** Policy (`T-021-02`) decides and records the read, allowed or refused, as `check_read`
  (reader in its role, what, permission, time; no balance or secret). `read_trail` asks policy first, then holds the
  answer to the log: the trail is served only if the entry at the answer's receipt is a `check_read` equal to the
  check, with the answer's outcome; otherwise `Unrecorded`, nothing served. A refusal is returned with no entry in it
  and is checked recorded the same way. The trail is the history up to and including the read's own record. The log
  itself admits a permitted read only on a grant in the log, not revoked (`permitted-read-rests-on-its-grant`).
- **Defaults taken** (fail closed, reversible):
  1. Each distinct measure entry counts once; an identical record is one entry (log idempotence), two records are two.
  2. The window is on the record's `at`, not the entry's `when`.
  3. A read that is not of the trail is refused by `read_trail` before policy is asked.
  4. Anything unrecorded or answered for another request releases nothing.
  5. Quotes are compared in the asset's own unit, per asset; nothing is summed across assets or providers.
  6. Provider and verifier are one key (the record carries one `provider` id), so a party in both roles merges; a test
     shows it. A role field on `MeasureRecord` is for the next core-internal round, `T-020-30`.
- **Gaps, with owners.**
  1. Orchestration has no caller of `Measures::measure`, so no run records any measure by itself: `T-020-31` (new
     `F-020` row, `PLAN.md`) owns calling it, with the end-to-end scenario. Until then `each_measure_is_computed_from_simulated_runs`
     is a **counting test over records written as orchestration will write them**, not an end-to-end one; its header
     says so, and it asserts the failure, refusal and reversal counts explicitly. The scenario test plays that part, writing records from each segment's own
     accounting and receipts, through `Core::measure_writer`.
  2. That door is a new file `conformance/scenarios/src/assembly/measures.rs` (and its `pub mod` line in
     `assembly/mod.rs`), since the assembly keeps the log's handle private. Both are `F-024`'s to accept or move; the
     lead accepts them for now. It reaches only a `General` handle on the log and Core's clock.
  3. Closed in the review round. `Act::Fail` had no effect on a reserve because the stand-in `Reserve`
     (`conformance/src/role/stand_in.rs`) had no `force_failure` hook: the harness default answered `false`, the sim
     armed nothing, and the order settled (exchange and ramp had the hook, which is why `sim_providers.rs` worked).
     `Reserve` now has `fail_next`, taken in `movement` after the dry-run branch and before the book is touched, so a
     scripted failure fails with evidence having moved nothing. The bulk run's third order fails at its final
     deposit and is accounted `Failed`; the test asserts it.
  4. No run reverses anything in M3, so the reversals count over a run is 0, asserted explicitly; positive counts are
     `core-audit`'s unit test. `T-020-31` (new, `PLAN.md`) records completed reversals.
  5. Failures carry no market figure in M3 (the contract says so), so all count as `no_figure`. No row feeds a market
     figure: the first row that gives a part one owns it (none in this plan); `T-020-31` records `market` absent.
  6. Quote and executed amounts and the dry-run said/outcome reach the log only inside a `MeasureRecord`: the log's
     other entries don't hold them (they are in kept statements), so measures can't be derived from the rest of the log
     without evidence access: `F-020` for the feed.
- **Tests.** `components/core-audit/tests/measures.rs` (real `AuditLog`; policy stood in for by `Stand`, which records
  on policy's handle) and `conformance/scenarios/tests/bulk_measures.rs` (assembled Core over the simulated reserve,
  real engine, the operator's grant, revocation). Written after the code, then attacked by the rule-offs below, not seen
  red first; honest note, as `T-022-05`.
- **Rule-off table** (each switched off once from the committed tree, `cargo test -p rutent-core-audit --test measures`
  under a 300 s alarm, tree restored from git; none hung):

  | Switched off | Red |
  |---|---|
  | window end inclusive | window test, bulk measures |
  | window ignored | window test, bulk measures |
  | quote below/above swapped | past-the-bound, bulk measures |
  | yes-then-done counts every yes | bulk measures |
  | failure with no figure not counted | bulk measures |
  | a refusal counted as a reversal | bulk measures |
  | no counted as yes | bulk measures |
  | total past the bound kept at the last total | past-the-bound |
  | one asset code in two units merged | past-the-bound, bulk measures |
  | trail serves a read of anything | only-a-read-of-the-trail |
  | answer of another request taken | no-record-behind-it |
  | answer taken without its record | no-record-behind-it |
  | record of another outcome taken | no-record-behind-it |
  | trail not cut at its own record | trail-ends-at-its-record |
  | record of another reader taken | no-record-behind-it |
  | `compute` imports `crate::log` | structure test |
  | `compute` takes `&mut` | structure test |
  | the trail reader names `grant_permission` | structure test |
  | writer skips `record.check()` | **not red**: the log runs the same `check` on append, so the writer's own call is a second line of defence; kept |

  Not run against the scenario test (each run is about 80 s): its figures are the runs' own and its checks overlap
  the above. A first cut of three structure mutations did not compile (missing doc, moved closure) and was redone.

## T-022-07

- **What is built.** The checkpointer issues, the checkpoint store keeps and the checker reads the contract's own
  types: `rutent_contracts::core_internal::Checkpoint` (`urn:rutent:core-internal:checkpoint`) and
  `CheckpointOpening` (`urn:rutent:core-internal:checkpoint-opening`), re-exported by `core-audit`'s `checkpoint`
  module as `Checkpoint` and `Opening`, with `CheckpointShape`, `CheckpointOpeningShape`, `CheckpointSignature`,
  `Tick`, `Salt` and `SignatureValue`. The crate's own `Checkpoint`, `Opening`, `CHECKPOINT_SHAPE` and
  `OPENING_SHAPE` are gone: no local copy of a shape. Edits: `src/checkpoint/mod.rs`, `src/checkpoint/checker.rs`,
  doc of `src/bin/audit-check.rs` (no code: it reads lines through `Checker`, which now reads the contract's shapes),
  `tests/checkpoints.rs`, and `Cargo.toml` (`jsonschema` as a dev-dependency, as `core-orch` has it) with `Cargo.lock`.
  Nothing under `contracts/`.
- **Signing.** The signature is still the log's key's over the RFC 8785 canonical JSON of the checkpoint without
  `signature.value` (`signing_input`), which is what the contract's description says. The contract's
  `SignatureValue` can't hold the empty string the old code signed with, so the unsigned value carries a placeholder
  of the right shape; `signing_input` leaves it out, so what is signed is the same.
- **Default: a clean switch, old files are not read.** No deployment holds a checkpoint store or an `audit-check`
  input in `urn:rutent:core-audit:*` yet (pre-production), so there is no reader for them. A store holding such a record
  is refused on open (`NOT_ITS_CHECKPOINTS`, as for any record that isn't one), a checkpoint in the old shape is
  `Finding::NotACheckpoint`, and an old opening is no opening (`NoOpening`). Fails closed; reversible by adding a
  reader, which no row asks for.
- **Default: the commitment's domain is unchanged** (`rutent:core-audit:checkpoint:v1`). The contract leaves
  computing the commitment to `core-audit` ("SHA-256 over a domain, ...") and names no domain; the domain is not
  a shape.
- **Default: the store record `Kept` and the cursor stay `core-audit`'s own.** They are the store's records, not
  published and not read outside the crate; only the two members of `Kept` are the contract's shapes.
- **A stricter reading than before, by the contract's types.** A checkpoint or opening is now refused for a member
  more or fewer, a tick that isn't 16 lower-case hexadecimal digits, a salt that isn't 32 bytes in the canonical
  encoding, a signature value not in its canonical encoding, and another `shape`. The one visible change to a
  `T-022-04` test: `a_checkpoint_off_its_tick_and_an_opening_off_its_log_or_salt_open_nothing` built an opening with a
  16-byte salt and expected `DoesNotHold`; the contract's opening can't hold one, so the checker reads it as no
  opening and the test now expects `NoOpening` (still red, as before: nothing opens the checkpoint). The tests'
  builders (`forged`, the re-signing of an edited checkpoint) build the contract's types; no assertion else changed.
- **The rewrite is still detected.** `DoesNotHold` for a history rewritten before the last checkpoint is unchanged
  and is exercised by the `T-022-04` tests (`rewrite_with_full_operator_access_is_detected_by_the_checker`, the tool
  test) and again on the fixture-derived bytes below.
- **Tests** (`tests/checkpoints.rs`, written first and seen red on assertions): the four of the row, plus one for
  the default.
  - `checkpoint_issued_is_valid_against_the_contracts_schema`: three periods, busy and idle; each published checkpoint
    validates against `checkpoint.schema.json` (with the schemas it refers to registered, offline), is bytes of the
    contract's type that serialise back to the same bytes, and is the one the store holds.
  - `opening_issued_is_valid_against_the_contracts_schema`: the same for the openings, as handed over and as stored.
  - `checker_reads_the_contracts_checkpoint_and_opening_fixtures`: the contract's opening fixture byte for byte, and
    its checkpoint fixture with only the commitment, the log's key and the signature a static file can't hold set
    (everything else, tick 42, the time and the log's name, is the fixture's), over a 110-entry history on the
    schedule the fixture implies, is clean; the three invalid checkpoint fixtures, signed by the log's key so that only
    their shape refuses them, are `NotACheckpoint`; the fixture opening without its salt is `NoOpening`; `audit-check`
    exits 0 on the same bytes and 1 with `DoesNotHold` on a history rewritten after.
  - `checkpoints_of_busy_and_idle_periods_still_cant_be_told_apart`: on the contract's shape, the same length, the same
    six members, schema-valid, for a busy run and an idle one. (`T-022-04`'s own test of the same property still runs
    unchanged.)
  - `a_checkpoint_in_the_old_shape_is_no_checkpoint_and_its_store_is_refused`: the default above.
- **Rule-off table** (each switched off once, `cargo test -p rutent-core-audit --test checkpoints`, tree restored;
  none hung):

  | Switched off | Red |
  |---|---|
  | the checkpoint published with a `size` member | `checkpoint_issued_is_valid_against_the_contracts_schema`, `checkpoints_of_busy_and_idle_periods_still_cant_be_told_apart`, and five `T-022-04` tests that read what is published |
  | the opening handed to the checker with an extra member | `opening_issued_is_valid_against_the_contracts_schema`, and two `T-022-04` tests |
  | the checker reads a checkpoint leniently (drops `size` and `salt`, forces the `shape`) | `checker_reads_the_contracts_checkpoint_and_opening_fixtures`, `a_checkpoint_in_the_old_shape_is_no_checkpoint_and_its_store_is_refused` |
  | the store's read-back skips a record that isn't the contract's | `a_checkpoint_in_the_old_shape_is_no_checkpoint_and_its_store_is_refused` |

  A first cut of the fixtures test used the invalid fixtures as they are, whose signature is a placeholder, so the
  lenient reader still refused them on the signature and the test stayed green; they are re-signed now, which is what
  made the rule-off red.
- **Gaps.**
  - The contract's schema and fixtures can't say that the commitment opens, or that the signature is the log's: a
    static fixture holds a placeholder signature and commitment, so the test derives them (above). Owner: none; it is
    the nature of a fixture, and the derived test is the check.
  - A party reading a checkpoint outside rutent's crates with the schema alone cannot recompute the commitment, which
    needs the opening (by design, `REQ-AUD-14`); the domain the commitment is hashed under is `core-audit`'s and is
    not in the contract. If an outside checker is to recompute commitments independently of `core-audit`, the
    domain and the byte layout need a place in the contract's README. Owner: `T-025-05`'s review to decide whether it
    is a finding; not needed by any row.
  - `contracts/core-internal/README.md` still says `T-022-07` "moves `core-audit` off" the old URNs; it is now true,
    and is the contracts' own file, so unedited here (`DEC-20`). Owner: the next contract round (`T-020-30`).

## The owner's answers of 2026-11-18, in the plan (2026-11-23)

- **`holding-balance-evidence`: "Evidence store only".** The default this feature built (a `holding_balance` result's
  bytes kept in the evidence store, and only there) is now `SPEC.md` 0.4.2's `REQ-AUD-6`: a holding account's balance
  read to verify it may be kept only as evidence held outside the log, never in the log or a record. `T-024-20` scans
  for it.
- **New rows:** `T-022-09` (a statement received after its grant's revocation is refused, `THR-3`, item
  `thr3-backdated-after-revocation`; `T-025-05`'s residual) and `T-022-10` (the store erases a cited statement on a
  decided override, item `erasure-of-cited-evidence`, beside `T-021-13` and `T-020-42`).
- **After review of the plan change (PR #243):** the revocation rule is the contract's attribution rule
  (`StatementKeys::signs_for`, `attribute`'s step 8), so it is `T-020-39`'s item (7); `T-022-09` waits on it in wave 34
  and edits `log/keys.rs` only.

## T-022-09: a statement received after its grant's revocation is refused

Branch `f-022/t-022-09-10-revocation-override`, from `m3` at `a3bc16b4`. Edits `components/core-audit/src/log/keys.rs`
(a `DEC-20` path, waiting for the owner's click at the release into `main`) and its tests in `tests/log.rs`.

- **The rule.** `Keys::signs_for` now reads the statement's received time (`T-020-39`'s item 7 passed it, and the
  register ignored it): where `Register::revoked_before_receipt(provider, made_at, received)` holds, the answer is no
  whatever the `made_at`, so the statement is recorded unattributed, as any other the register can't attribute. It is
  the contract's `revoked_before_receipt` read over the register's done changes (`grants`) rather than a call into the
  contract, since `signs_for` holds no history: each grant (a tuple's, or a verifier's class) in force at `made_at`
  (its latest change at or before it, by the effect's time, the later on a tie) must have a revocation after
  `made_at` and no later than `received`. The operator's key is read before it and is untouched.
- **Owner's rulings applied.** `thr3-backdated-after-revocation` ("F-022 row"): refuse. `revoked-key-late-genuine`
  (applied default): a genuine result made before the revocation and received after it is refused too; its provider
  re-signs under its new grant. A revocation at the instant of receipt counts as before it (`<= received`).
- **Defaults.** (1) A grant made again is a new grant but the revocation of the same tuple still stands in the window:
  a statement made before the revocation and received after it stays refused though a grant stands again at its
  receipt, as the contract's function reads it (the revocation is looked for by grant, any time in
  `(made_at, received]`). (2) No grant in force at `made_at` leaves the rule false, and `in_force` refuses it apart.
  (3) The statement's own `received_at` is the only clock, never the log's: a statement received before the
  revocation stays attributed though appended after it.
- **Tests (`tests/log.rs`).** New: `a_statement_received_after_its_grants_revocation_is_refused_whatever_its_making_time`
  [`THR-3`], `a_statement_received_before_the_revocation_stays_attributed` [`REQ-AUD-2`],
  `a_statement_after_a_grant_made_again_is_attributed_only_from_the_new_grant` [`THR-3`], seen red on their
  assertions before the code. Flipped, as the row asks: `a_verifiers_class_grant_is_read_at_its_statements_own_time`
  now expects "its class, made inside the window", received five seconds after the revocation, unattributed. Its
  last case of the first block ("claimed as a provider, backdated") expected a refusal for the role; no key attributes
  it now, so it is recorded unattributed like the others of that block (a test artefact, not a Core bug: the class
  grant's limit on a role is still tested, with the grant in force, by
  `a_verifiers_class_grant_attributes_its_judgements_of_that_class_alone`). `T-022-02`'s key tests pass unchanged.
- **Gaps.** See the fix round below for what is closed and what stays a residual for `T-025-05`.
- **Rule-off table.**

  | Rule switched off | Red |
  |---|---|
  | A: `signs_for` skips `revoked_before_receipt` | the refusal test, the grant-made-again test, the class test |
  | B: `<= received` becomes `< received` (revocation at the instant of receipt) | the refusal test |
  | C: the revocation need not be no later than `received` | `a_provider_signs_only_under_a_grant_in_force`, the stays-attributed test |
  | D: the revocation need not be after `made_at` | the grant-made-again test |

### Fix round 1 (`T-022-09`, after the review of `babc2cb3`)

- **Several grants (finding 1).** `a_statement_stays_attributed_while_another_grant_of_its_maker_stands`: a provider
  with two tuples, one revoked, keeps a statement made before the revocation and received after it attributed; once
  the other is revoked it is refused. Rule-off F1 (`all` to `any`) turns it red.
- **Ties (finding 2).** The register now keeps each change's effect index beside its time and breaks a tie in time by
  it (`latest_at`), as the contract reads the effects in the log's order, in `in_force` and in
  `revoked_before_receipt` alike, not by the order the results arrived. `changes_at_one_instant_are_ordered_by_their_effects`:
  a revocation and a grant at one instant, results in the opposite order. Rule-off F2 (tie by time alone) turns it red.
- **The received time (finding 3).** Held to the log's clock, in `Register::admits`, where it matters: a stated entry
  whose maker has a revocation of a grant dated after the entry's `received_at` must have that time within the skew
  of the log's clock, else `Unavailable`, as an advertisement's behind-the-clock time is
  (`a_late_statement_dated_before_a_revocation_is_not_taken`). Rule-off F3 turns it red. **Narrower than every
  statement, as a default:** holding every stated entry to the clock failed six `checkpoints.rs` tests, whose worlds
  append entries dated days before their clock, so the hold applies only where a later revocation of the maker's
  grant exists, the one case where the received time decides attribution. **Residual for `T-025-05`:** within the skew
  (300 s by default) of the log's clock, orchestration can still date a statement received a few minutes before a
  revocation that is that recent; and the received time stays the writer's word for a maker with no revocation, where
  it decides nothing. A statement the log couldn't take (`Unavailable`) can't be taken on a retry with the same
  received time, which is still behind the clock and fails again; it has to be sent again with the time of the new
  append, and is then recorded unattributed, being received after the revocation.
- **The flipped test (finding 4).** `a_verifiers_class_grant_is_read_at_its_statements_own_time`'s first block is
  restored: its statements are received before the revocation (`2026-11-06T23:59:59Z`) and appended after it, with
  its original assertions (the class read at `made_at`, a backdated other class, result and provider claim refused),
  and one case received after the revocation, unattributed. The test helpers `answered` and `answered_received` set
  the log's clock to the statement's received time, since the world appends it as it is received.

## T-022-10: the evidence store erases a cited statement on a decided override

Same branch, after `T-022-09`. Edits `components/core-audit/src/receipts/store.rs` and `tests/receipts.rs` (a `DEC-20`
path). Core's half is `T-020-42`; policy's is `T-021-13`.

- **Design.** `BoundStore::erase(at)` reads the erasure back from the log (`decided`, which `decided_erasure` now wraps):
  the `done` result, its `erase_statement` effect, and the permitting `decide`, and takes `overrides_citations` from
  that decision's own request action, never from the effect. Set, the store skips the citation check (`erasable_from`
  with `citations_refuse` false) and erases: bytes and salt go, the commitment and the stated entry stay, a fetch
  answers `Erased`, and the entries citing it stay as they are. Every other rule holds under an override: the statement
  is kept and named by a stated entry, and an advertisement is never erased.
- **API.** `erasable` is unchanged (the default, `EVIDENCE_RESTED_ON`). New `erasable_overriding`, for orchestration
  (`T-020-42`) to run before asking policy for an override: it answers what the operator may ask, never what the store
  does, which is the permitting decision's word read from the log.
- **Findings.** In `reconcile`, `ERASED_YET_CITED` counts only entries after the override's decision: a citation made
  before it no longer holds the erasure back, one made after it (the log admits those between the decision and the
  effect) is reported in `unsettled_erasures`. Without an override every citation counts, as before.
- **Defaults.** (1) "After the decision" is the decision's log index, not the effect's or the result's. (2) The store
  erases an override even where a citation landed after the decision: the operator decided it, and the finding
  reports it. (3) An override's decision that isn't `permitted`, or whose action differs from the effect's, is no
  decided erasure (`NO_DECIDED_ERASURE`), as before.
- **Tests (`tests/receipts.rs`).** `store refuses a cited statement's erasure without an override` [`REQ-AUD-13`],
  `store erases a cited statement on a decided override and history still checks` [`REQ-AUD-13`, `AC-36`], `citation
  made after an override's decision is still reported` [`INV-7`] (as `store_refuses_...`, `store_erases_...`,
  `citation_made_after_...`). The first pins the default and passed before the code; the other two failed first. The
  test helper `decide_erasure` gained `decide_erasure_as` (override flag, a hook between decision and effect); the
  existing tests pass unchanged.
- **Gaps.** The store's refusal of an erasure whose decision lacks the override can't be built through an admitting
  log (the log refuses the effect while a statement is cited, and a citation after the effect begins), so no
  integration test reaches "override read as always true"; a unit test of `decided()` over a hand-built slice does
  (fix round 1, below). The log's own row holds the rest (`T-020-39`).
  `T-020-42` calls `erasable_overriding` and passes `overrides_citations` only where the operator asked.
- **Rule-off table.**

  | Rule switched off | Red |
  |---|---|
  | R1: `erase` ignores the decision's override | the override test, the late-citation test |
  | R2: every citation counts for `ERASED_YET_CITED`, whatever the decision's time | the override test |
  | R3: no citation counts (nothing reported) | the late-citation test |
  | R4: `erasable_overriding` still refuses a cited statement | the override test, the late-citation test |

### Fix round 1 (`T-022-10`, after the review of `babc2cb3`)

- **`decided()` unit-tested (finding 5).** `receipts::store::tests::the_override_is_read_from_the_permitting_decision`
  builds a decision, effect and `done` result by hand: `overrides` is false without the flag and true with it (and
  names the decision's receipt), and an effect that sets the flag on a decision that doesn't is `NO_DECIDED_ERASURE`.
  Rule-off F5 (`overrides: true || ...`) turns it red.

## T-022-11: the register counts a revocation from its effect, and the store's override erase reads its decision

Branch `f-022/t-022-11-revocation-window`, from `f01bc669`. Edits `components/core-audit/src/log/keys.rs`,
`src/receipts/store.rs` and their tests (a `DEC-20` path). Starts from `T-020-45`'s rule: a revocation counts from its
admitted effect until a `failed` result undoes it.

- **(1) L5, the register (`keys.rs`).** `Register::record` takes a `revoke_grant` effect into `grants` when the effect is
  recorded (its time, `false`, its index in the log, which is the history's length), and a `failed` result of a
  revocation's effect takes that entry out again. The `done` result no longer adds a revocation (it would count twice);
  a grant still counts from its `done` result. `in_force`, `revoked_before_receipt` and `revoked_after` (the clock hold
  on a late statement) all read `grants`, so all three follow. A statement received between a revocation's effect and
  its result is unattributed; once the result fails, it attributes again.
- **(2) The override erase (`store.rs`).** `BoundStore::erase` calls `erasable_overriding_since(commitment, decision)`
  with the permitting decision's receipt where the decision overrides, so a citation the log took after that decision
  refuses the erasure (`EVIDENCE_RESTED_ON`) and nothing is erased, instead of erasing and reporting `ERASED_YET_CITED`.
  `T-022-10`'s `citation_made_after_an_overrides_decision_is_still_reported` is now
  `store_refuses_an_override_erasure_of_a_statement_cited_after_its_decision`: `erase` is refused, the statement stays
  kept, and on reopening `refused_erasures` reports the decided erasure the store refused (`EVIDENCE_RESTED_ON`) while
  `unsettled_erasures` is empty. `reconcile`'s `ERASED_YET_CITED` check stays, a defence for a log whose store erased
  before this rule.
- **(3) The drift test.** `log::keys::tests::contracts_revoked_before_receipt_and_the_register_agree_over_the_attribution_fixtures`
  (a unit test, since the register is private to the log) reads every history-bearing fixture of core-internal's
  `fixtures/attribution`, renumbers its receipts to their places (the log numbers them so; results still name their
  effects), feeds the entries to a `Register` through `record`, and compares the register's `revoked_before_receipt`
  with the contract's at five received times per fixture (the claim's own, the made time, and +1 s, +1 day, +30 days).
- **Tests (`tests/log.rs`).** `statement_received_between_a_revocations_effect_and_its_result_is_unattributed`
  [`THR-3`] and `revocation_whose_result_failed_attributes_again` [`THR-3`], seen red on their assertions before the
  code; the store test above, red on its first assertion before the code. The drift test passes on first run (written
  after the code); its red was watched by the rule-offs below.
- **Defaults.** (1) A revocation whose effect has no result yet counts, as the contract reads it. (2) The drift test is
  a unit test over a register fed by `record`, not through an admitting log: fixture histories carry hand-numbered
  receipts that `admit_entry` would not take. (3) After a refused override erase, the log keeps its `done` result: the
  store reports it, orchestration decides the retry.
- **Gaps.** None open for this row. The contract's `in_force`-at-`made_at` reading is not compared (the contract
  exposes only `revoked_before_receipt`).
- **Rule-off table.**

  | Rule switched off | Red |
  |---|---|
  | R1: a revocation counts from its `done` result only | both log tests, the drift test |
  | R2: a `failed` result does not undo a revocation | `revocation_whose_result_failed_attributes_again`, `a_grant_trusts_only_the_keys_it_names_once_done`, the drift test |
  | R3: `erase` calls `erasable_overriding`, not `..._since` | the store refusal test |
  | R4: the receipt bound `<= received` becomes `< received` | the drift test, `a_statement_received_after_its_grants_revocation_is_refused_whatever_its_making_time` |

### Fix round 1 (`T-022-11`, after the review of `13b6415e`)

- **Names.** The three tests carry the plan's names (`statement_received_between_a_revocations_effect_and_its_result_is_unattributed`,
  `revocation_whose_result_failed_attributes_again`, `contracts_revoked_before_receipt_and_the_register_agree_over_the_attribution_fixtures`).
  `T-022-10`'s row named `citation made after an override's decision is still reported`, the test this row turned into
  `store refuses an override erasure of a statement cited after its decision`: the row now names the new one (a
  PLAN-only edit).
- **An erased statement answers `Ok`.** `erasable_overriding_since` answers `Ok` for a statement erased already, after
  the other rules, so a retried override erase after an erasure carried out under the older rule isn't refused (INV-8):
  `a_retried_override_erase_of_an_erased_statement_cited_since_answers_ok` (the reviewer's probe, as far as an admitting log lets it be built: it refuses a citation once an erasure has begun, so
  the state of an erasure carried out under the older rule can't be reached, and the check is asked with an earlier
  decision instead, which cites the same way). Rule-off F6 (drop the early `Ok`) turns it red.
- **Drift test.** It asserts each fixture's receipt indices are unique and every result's effect is found, and compares
  at the claimed `made_at` and at the time of every change in the history (ties with a grant's or a class's time).
- **Residual for `T-025-05`.** Orchestration writes the results, so a `failed` it writes can undo a revocation policy
  permitted. Before this row it could withhold `done` and so keep the revocation from counting: the power is the same,
  moved from "withhold" to "undo". A revocation's effect still needs policy's permit; its result doesn't.

### Fix round 2 (`T-022-11`, three lows of the approval at `1bf65bfa`)

- **L1.** `a_store_erased_under_the_older_rule_is_not_reported_refused_when_bound_again`: after a refused override erase
  (a citation between decision and effect), the kept file is deleted and the erased mark written straight into the
  evidence store, then the store is bound again: nothing is reported refused, the retried erase answers `Ok`, and after
  the `erasure` entry the late citation is the finding. Rule-off F6 (the early `Ok` dropped) turns it and
  `a_retried_override_erase_of_an_erased_statement_cited_since_answers_ok` red.
- **L2.** The drift test also compares a copy of each fixture with every change effect dated at one instant and its
  results last, in the opposite order. **Gap, owner `core-internal`'s contract (the round that exposes grants in force):**
  the tie-break can't show through `revoked_before_receipt`: a tie matters to whether a grant is in force at `made_at`,
  and the rule then needs a revocation strictly after `made_at`, which a tied change isn't, so the tie-break off (F2)
  leaves the drift test green. It stays held by `changes_at_one_instant_are_ordered_by_their_effects`. No fixture has a
  verifier's class grant, so the test's doc no longer claims classes are compared.
- **L3.** `PLAN.md`'s `T-022-10` Work text says a citation after the decision refuses the erasure (`T-022-11`); the
  register's doc is wrapped.

## T-020-53 (finding 4 of `M3`'s milestone review): every read Core makes of the evidence store, with its caller in Core

The store has two read doors, and a statement's bytes leave it by no other (`keep` is the write). A read with no caller
in Core is named out of `v0` below, with its reason. Line numbers are of `T-020-53`'s base (`1bc91b21`).

**Door 1: `KeptStatements::kept`, Core's own read, with no policy read** (nothing leaves Core's audit part).

| Read | Caller in Core | What it reads the bytes for |
|---|---|---|
| `AuditLog::attribute_stated` (`core-audit/src/log/mod.rs:287`; the shared `attribute`, `contracts/.../attribution.rs:570`) | every stated entry's append, through `Log::append` | the signature and maker the entry's attribution rests on (`REQ-AUD-2`) |
| `AuditLog` opening (`log/mod.rs:364`) | `AuditLog::open`, from the log's records | each recorded advertisement's bytes, to rebuild the catalogue's keys |
| `is_what_is_kept` (`core-orch/src/quote/held.rs:489`) | `Holds::hold` | the kept bytes of a signed hold bear out their commitment |
| `Terms::take` (`quote/terms.rs:198`) | taking a provider's stated terms | the kept bytes are the terms the entry names |
| `kept_as_read`, `kept_notice_agrees` (`arrival/mod.rs:389`, `:402`) | the arrival step on a credit and on its deposit (`T-020-50`) | the notice the log attributes is the one Core's figures are read from |
| `HandOn::started_in_the_log` (`hand_on/mod.rs:729`), under `recover_starts` | a Core opening again, per linked order | the start record's bytes, so a hand-on's start is proven from the log |
| `Intake::reopened` (`intake/mod.rs:477`) | a Core opening again, per order opened | an order's tuples' inputs, from `order_opened`'s commitment |
| `Reopening::kept` (`segments/reopen.rs:165` in `reopened`, `:236` in `read_flight`, `:711` in `resumed_answer`, `:763` and `:978` in `stated_value` and `settled_result`; implemented by the assembly's `reopen.rs`) | `Segments::reopened`, a Core opening again (`T-020-46`) | the terms providers stated, each handed-over segment's signed operation, a result's bytes, the stuck move's question and a settled segment's result |

**Door 2: `Evidence::fetch`, a read by a party or a checker, asked of policy first.**

| Read | Caller in Core | |
|---|---|---|
| `BoundStore::fetch` (`core-audit/src/receipts/store.rs:905`) | none | named out of `v0`, below |

**Door 3: the erasure reads, `Statements` (`core-orch/src/erasure.rs`), over `BoundStore::erasable*` and `is_erased`**
(`core-audit/src/receipts/store.rs:709-836`). They read the store's index and the log's history, not a statement's
bytes, and every one is asked before an erasure is decided.

| Read | Caller in Core |
|---|---|
| `Statements::erasable` (`erasure.rs:68`; the assembly's `Bound`, `assembly/erasure.rs:21`, to `BoundStore::erasable`) | `erase_statement` (`erasure.rs:293`) through `check(.., false)` (`:314`), before policy is asked; and again after the decision, `:347`, where no override was asked |
| `Statements::erasable_overriding` (`erasure.rs:104`; `assembly/erasure.rs:25`, to `BoundStore::erasable_overriding`) | `erase_statement` through `check(.., true)` (`:314`) before policy is asked, reached from `erase_statement_overriding` (`:395`), the entry for an override |
| `Statements::erasable_overriding_since` (`erasure.rs:79`, called at `:347`; `assembly/erasure.rs:29`, to `BoundStore::erasable_overriding_since`) | `erase_statement`, after the permitting decision, with the decision's receipt: citations made before it are lifted, none after |
| `BoundStore::is_erased` (`store.rs:836`) | none in Core: the store's own `reconcile` (`:643`) and `erasable_overriding_since` (`:736`) call it |
| `BoundStore::missing` (`store.rs:682`), `refused_erasures` (`:689`), `unsettled_erasures` (`:701`) | none in Core, out of `v0`: the store's reports of its own reconciliation, read only by tests (core-audit's `receipts.rs`; the scenarios' `assembly_wiring.rs:555` and `audit_scenarios.rs:481` assert `missing()` is empty). An erasure the store refused or left unsettled is Core's to finish through `Unsettled::settle` (`T-020-46` rebuilds it), not through these reads; a deployment that shows them to its operator owns that call (`F-030`) |

- **Left uncalled in `v0`, by choice: `Evidence::fetch` has no caller in Core.** It is the party and checker door the
  contract names (`contracts/core-internal/README.md:35` and `:261`: a kept statement leaves the store only through
  it), built and tested. No Core flow calls it: a party gets its evidence as the receipt `Issuer::issue` signs on
  request (`T-022-03`, `REQ-AUD-5`), which reads the log and no store, and the outside checker works from its own copy
  of the evidence directory (`audit-check --evidence`, `KeptOnDisk`). The path that meets each requirement the
  contract's README names for `fetch`, in `v0`:
  - `REQ-AUD-5`, a party's evidence: the receipt `Issuer::issue` signs on request, which the party keeps and checks
    against a changed record (`core-audit/tests/receipts.rs`).
  - `REQ-AUD-10`, the checker's check: the outside checker's run over the log and its copy of the evidence directory
    (`audit-check --evidence`); appointing and naming the checker is `F-033`'s (`M4`), as the scope file says.
  - `REQ-AUD-12`, a regulator's read of the trail: a read grant held to the operator (`T-021-14`), decided by policy's
    read check and recorded (`assembly_policy.rs`, `bulk_measures.rs`). Those tests hold the check, not the read:
    `read_trail` (`core-audit/src/measures/trail.rs:73`) has no caller in Core yet, so the trail itself, its entries and
    their receipts and never a statement's contents, reaches the regulator through `T-020-55` (3)'s entry point. Naming
    the regulator is `F-033`'s.
 Default: fails closed (no caller, no read). Gap,
  owner the lead: if `v0` wants a party to read its kept bytes, a row builds the interface that calls `fetch` under
  policy's read check (`REQ-AUZ-7`).
- **Named out of `v0`: `statements::restatements` (`core-audit/src/statements/mod.rs:226`) has no caller in Core.**
  It is the batch form of the check the store already makes at each `keep` (`Kept::restates`, which the append path
  uses), over a whole history. Only tests call it; `audit-check` runs `unproven` (`statements/mod.rs:275`,
  `bin/audit-check.rs:147`), the one batch read an outside caller has. Default: not built into Core; owner `F-025`'s
  re-check if a batch restatement check is wanted for the operator's review.
- **Not reads of the store.** Policy's `Source::Kept` / `Source::KeptAbout` (`core-authz/src/stuck/mod.rs:223-236`) ask
  whether a commitment is kept, and the assembly answers from the log's stated entries (`assembly/policy.rs:433`),
  never from the store; the assembly's own reads for pricing and holding accounts (`assembly/price.rs:116`,
  `assembly/holding.rs:608`) use door 1 as the simulation's stand-in for `F-030`'s production assembly, which is to
  read the same way. A stuck decision's evidence no longer rests on that answer: orchestration holds each digest to a
  logged statement itself (`T-020-53`, `F-020`'s notes).

