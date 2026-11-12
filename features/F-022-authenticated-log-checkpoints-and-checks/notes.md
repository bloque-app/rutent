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
- **Two handles, one call each.** The store itself implements none of `core-internal`'s calls. It hands the log
  `LogsRead`, which implements `KeptStatements` alone (no read check: nothing it answers leaves Core's audit part),
  and orchestration and the parties `Keeper`, which implements `Evidence` alone and holds policy's read check
  (`calls::Reads`). So whoever holds `Keeper` reads no statement but through policy; a test holds that neither
  handle implements the other's call, and the store neither.
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
- **Erasure records first.** `EvidenceStore::erase(log, at)` carries an erasure out only on the log's own record: a
  verified `erasure` entry at the receipt `at`, the log's own, whose outcome is `done`. So the reference that stays
  is recorded before anything goes, and an erasure cut short is carried out by calling it again (`INV-7`).
  `EvidenceStore::erasable(log, commitment)` runs the same checks beforehand, so orchestration appends no erasure the
  store would refuse. An erasure names a kept statement, and the log refuses one no stated entry holds
  (`erasure-names-a-kept-statement`). It removes the bytes and the salt, and the index's place for them, and leaves a
  mark, `erased/<commitment>`, so a read answers `Erased`; the shelf writes the mark first and removes the file
  after, and opening finishes an erasure a crash cut short. Erasing one erased already is `Ok`.
- **No erasure reaches an advertisement** (the coordinator's ruling in `T-022-01`'s notes): neither bytes whose shape
  is an advertisement's, nor bytes any stated entry of the log labels one, since the log reads back, on opening, the
  bytes of every entry labelled so (`ADVERTISEMENT_NEVER_ERASED`). A test holds that the log still opens after
  both were tried.
- **Who may erase** (an item for `F-020`). core-internal's `erasure` is a verified entry with no decision behind it,
  and no row ties it to a permit, so the store erases on the operator's record alone. Whether an erasure needs a
  decision of policy's is a contract question.
- **Retention is the operator's** (`REQ-AUD-13`): the store erases nothing on its own, on no schedule.
- **A party's receipt** (`receipts::receipt`). In the style of RFC 9943's receipts: the log's signed word that an
  entry, by its record's digest, sits at its index in a tree of a given size whose RFC 9162 root it names, with the
  inclusion proof, the log's name and when it was issued (`PartyReceipt`, shape `urn:rutent:core-audit:party-receipt`).
  It is signed as provider-role signs a statement, Ed25519 over the canonical JSON without `signature.value`, and
  checked by the catalogue's strict verifier (`check_receipt`). `holds_against` checks it against whatever history
  the operator shows: the entry at its index, under the root at its size. The operator can't sign anew a receipt the
  party holds, so a rewrite or a removal before that size shows (`REQ-AUD-5`, ADR-0005).
- **The log's own key signs receipts**, never the deployment's `operator_key`, which signs the operator's statements:
  `Issuer::new` refuses it (`KEY_ACROSS_PURPOSES`). The deployment gives the issuer its secret and publishes its
  public key to parties and the checker; how is `F-030`'s and `F-033`'s. Whether `T-022-04`'s checkpoints sign with
  the same key is `T-022-04`'s to say.
- **Receipts only on an allowed read.** `Issuer::issue` asks policy first, with no borrow of the log held, then
  issues only for an entry the read covers: for `reads: evidence`, the stated entry holding that commitment (what the
  party stated, and what Core told it, its questions being the operator's statements); for an order or a segment, an
  entry naming it, a stated one by its `about` and a verified one by its `what`'s own `order` and `segment`; for the
  trail, any entry; for a resource, none (`NOT_COVERED`). A verified entry whose order sits elsewhere in its `what` is
  not covered by an order read: a reading, reversible by a later row naming each kind's order.
- **No contract shape for receipts** (a hand-off to `F-020`). The receipt is `core-audit`'s own JSON, a library
  type; a party reading it outside rutent's crates would want it under `contracts/`, which this row doesn't edit.
- **The router record's bytes** (core-internal's hand-off to `T-022-03`). The store keeps them through `keep` like
  any bytes, but the contract has no call to read them back by the `decision_record`'s unsalted `record` digest. An
  item for `F-020`.
- **Residuals for `T-025-05`.**
  - A receipt names the log's size when it was issued, so a party asking for receipts at two times learns how much
    the log grew between them: activity, which `REQ-AUD-14` keeps out of checkpoints.
  - A receipt's inclusion proof holds its neighbours' leaf hashes, SHA-256 of `0x00` and an entry's unsalted
    canonical JSON, so a party can confirm a guess of an adjacent entry, one about someone else's order among them.
    No entry holds a balance, so this is linkage, not `REQ-AUD-6`'s balance.
  - An erased file's blocks may stay on the disk; the store removes the file and syncs, and secure deletion is the
    deployment's storage (`F-030`).
  - A retried append of a stated entry whose bytes were erased since answers `Refused`, not its earlier receipt:
    the log attributes before its retry rule (`T-022-01`'s).

### Tests

- `tests/receipts.rs` runs the log in its file and the evidence store in its directory, side by side under one temp
  directory, with a stand-in for policy's read check that allows or refuses every read, counts what it was asked,
  and can answer another request.
- Every rule was switched off in turn, and its test failed: the fresh salt, the commitment over the salt and the
  bytes, opening on statements that bear out their commitments and are filed under their own, the restatement index
  and its filing by signed content, an erasure dropping its place in the index, `fetch` handing nothing over on
  policy's refusal, asking policy first, reading evidence only and taking only the answer to its own read, an
  erasure only on the log's own entry at its receipt and only a `done` one, no erasure of an advertisement by its
  label or by its bytes, the shelf removing the bytes, opening finishing an erasure cut short, an erased statement
  answering erased, a keep writing the shelf before answering, a failed write keeping nothing, a failed erasure
  erasing nothing, the directory's lock, the receipt's signature, its entry digest with its inclusion proof, a
  history holding a receipt only under its root, the issuer refusing the operator's key, taking policy's refusal
  and only the answer to its own read, issuing only for an entry the read covers, and a segment read naming its
  segment, and a kept statement checking against its entry by its commitment, its key and its signature.
- Some checks are implied by others, so switching one off alone fails no test: a receipt's key being the log's (the
  signature is verified under the log's key), its shape (only the issuer signs, and it signs that shape), its entry
  digest and its inclusion proof (each implies the other for a receipt the log signed; off together, they fail), and
  `holds_against` comparing the entry at its index (implied by the root).
