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
  and for a module the log holds a done `accept_module` of and no advertisement. No advertisement is accepted for
  such a module, so none can turn it into a provider's.
- **Which module is the operator's** (a residual for `T-025-05`). The log knows a module the operator publishes only
  as one with an accepted release and no advertisement, since no entry says whose a module is. A provider's release
  accepted before the log recorded its advertisement would be read as the operator's until then; prov's admission
  takes a provider's release only against its accepted advertisement, so this needs policy to accept a release out
  of that order.
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
