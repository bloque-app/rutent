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
- **Stores.** `MemoryStore`, and `FileStore`, one append-only file synced before each append answers. A deployment's
  store is `F-030`'s (`M4`); the trait lets it plug in.
- **The read surface** for the later rows, which can't edit `log/`: `entries()`, `len()`, `root()`, `root_at(size)`,
  `inclusion(index, size)` with `merkle::verify_inclusion`, and the free functions `record` and `digest`. No
  consistency proof is built: `T-022-04`'s checker holds the whole history and recomputes `root_at`.
- **The checks' handle.** `log::General<L>` wraps a shared log and implements `Log` alone. `T-022-05` and `T-022-06`
  take it (or any `Log`), never `AuditLog`; a test holds that it implements no `PolicyLog`.
- **The evidence store's read** is a stand-in until `T-022-03` builds the store: `log::KeptInMemory`, a map from
  commitment to `KeptStatement` that the caller fills with a salt of its own. It has no restatement index, draws no
  salt and answers no `Evidence` read. `T-022-03`'s store replaces it through `KeptStatements`, with no change to
  the log.
- **Signatures** are checked by the catalogue's strict verifier, `rutent_prov::catalogue::verify_envelope`, the one
  admission uses, over the bytes as received (`log::Ed25519Strict`). The log adds no curve code of its own.

### Whose keys: the register

- **What the log reads keys from.** Its own records and the deployment's settings (`log::Settings`), never
  orchestration. The operator signs as `Settings::operator` with the deployment's `operator_key`, and states for
  `Settings::operator_party`. A provider's grant is in force from an admitted `change_catalogue` effect of `grant`
  for one of its tuples until one of `revoke_grant` for it, each at its effect's own time. A provider's advertisement
  is a stated entry of the advertisement's shape whose bytes the evidence store keeps, held to provider-role's rules
  and the catalogue's (`F-012`): this deployment and protocol version, `validate_statement`, every listed key a key
  to accept, dated no more than the skew ahead of its `received_at`, a grant in force then, the signature strict, no
  dropped `statements` key listed again, and no key across purposes for any provider.
- **First trust (a default, and an item).** `core-internal`'s `change_catalogue` action names a provider and a tuple
  but no key, so a grant's effect can't say which advertising key the operator trusts. Taking the first
  advertisement recorded after a grant would be trust on first use: orchestration could append a self-signed
  advertisement for a granted provider, make its own key that provider's `statements` key, and then log a verified
  yes the provider never gave, the attack this row's amendment closes. The default: a provider's first
  advertisement is accepted only where it is signed with an advertising key the deployment's settings trust for it,
  and every advertising key it lists is one (`Settings::first_trust`, as the catalogue's grant names keys under
  `trust_advertising_key`); every later one only as `validate_successor` of the one accepted last. Reversible: a
  later `F-020` row may add the trusted key, or the advertisement's commitment, to the grant's action (a
  `core-internal` bump), and the log would read it from the effect instead. Recovery after a provider loses every
  advertising key is then a new trusted key in the settings, and isn't built here.
- **When a key signs.** A key signs a provider's statement where a grant of it is in force at the statement's
  `made_at`, and both the advertisement accepted last and the one in force at `made_at` list it for `statements`:
  the catalogue's own rule (`verify_statement`), stricter than `StatementKeys::signs_for`'s minimum, so a retired key
  can't sign a statement backdated to before its retirement. What the log recorded earlier stays as recorded.
- **An advertisement stays unverified as an entry.** It is signed with an advertising key, which `signs_for` never
  answers for, so its stated entry is recorded unverified; the register reads its bytes, not its attribution.
- **A grant's effect counts once admitted.** The register reads the `effect` entry, which `admit_entry` admits only
  on an unused permit of a permitting `decide` of the same action; it doesn't wait for the effect's `result`.
- **Whose clock a grant runs on** (the coordinator's ruling, after review). A grant's and a revocation's times are
  their effects' `when.at`, which orchestration writes, so the log holds them to its own clock: `Settings::clock`, a
  `Clock` the deployment gives it (`SystemClock` by default), which orchestration never writes. A `change_catalogue`
  effect dated more than the skew (`Settings::clock_skew_seconds`) from the log's clock when it is appended is
  refused (`CHANGE_OFF_THE_LOGS_CLOCK`), so a revocation can't be dated into 2100 and left with no force; and a
  tuple's grants and revocations must be dated in the log's order, so a re-grant backdated behind a revocation is
  refused (`CHANGE_BEFORE_ITS_TUPLES_LAST`) and the window it would reopen stays shut. Opening doesn't consult the
  clock: the records were held to it when they were appended. A revocation still holds only once orchestration
  appends its effect, which is orchestration's to do on policy's decision.
- **When an advertisement joins.** At its stated entry's `received_at`, the time Core
  stamps; a grant must be in force then.
- **Modules the operator publishes** (`REQ-CAT-23`) have no advertisement, and an `accept_module` effect names a
  version, not the release's `statement_key`, so the log knows no key that signs for them: their statements are
  recorded unverified, attributed to no one. A residual for `T-025-05`, and an item for an `F-020` row to name the
  key in the effect. `StatementKeys::publisher` answers the operator's party for the operator alone.

### Tests

- The coherent world of `fixtures/admission/world--*` is replayed through the real log: each placeholder receipt
  and commitment is rewritten to the one the log answered and the stand-in kept, every statement is signed for
  real (the conformance crate's `signing` keys, a dev-dependency the boundary check allows) and handed in
  unverified, and the world's five providers are granted and advertised first.
- `REQ-AUD-6`'s scan puts the run's secrets and a balance into kept statements' bodies, asserts the evidence
  store's stand-in holds each, and then that the log's file holds none of them, nor any kept statement's bytes or
  salt.
- Every rule of the append path and of the register was switched off in turn, and its test failed: the `check`,
  `attribute`, `admit_entry`, the retry rule, the read-back after a failed write, writing before taking an entry
  in, the canonical form on opening, first trust, the successor rule, the advertisement's signature, its context,
  `validate_statement`, its listed keys, the skew, the read-back failing as unavailable, the grant at the advertisement and at the statement, the
  revocation, the key in force at `made_at` and in the advertisement accepted last, the dropped and the crossed
  key, the operator's key, the publisher and the strict verifier.
