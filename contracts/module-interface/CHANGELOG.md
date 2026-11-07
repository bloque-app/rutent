# module-interface changelog

## 0.1.0
- The module interface split: the sandboxed part's and the outside part's declarations, what crosses between
  them, destination declarations, secret lookups by name from the outside part only, the outbound call record and
  declared cross-module calls. (DEC-4, DEC-9, T-003-01)
- Signing a result, as ruled: the module never holds a key; the outside part signs the sandboxed part's result
  with the statement-signing key it declares in the required `outside.statement_key`, resolving the private half
  through the credential proxy by the deployment's name (the deployment `{name, operator_key}` of the enveloped
  advertisement), over the RFC 8785 (JCS) canonical envelope with only the top-level `signature.value` removed.
  `validate_statement_key` checks that a signed result's `signature.key` is the declared key, that the result
  belongs to the advertisement's deployment, that the key is one of the advertisement's `keys`, and that it isn't
  the `operator_key`; `signed-result-` fixtures bundle the three values. (DEC-4, T-003-01)
- After review: a secret lookup names the outside operation it serves, and its answer echoes it;
  `validate_lookup` refuses a lookup whose operation is undeclared or doesn't declare the secret, with
  `declared-secret-lookup-` fixtures bundling a declaration and a lookup. A resolved secret's value never prints
  through `Debug`. (DEC-4, DEC-9, T-003-01)
- After review: `validate_statement_key` also refuses a result made by another party than the declaring provider,
  and `statement_key` is checked in Rust as a canonical Ed25519 public key. (DEC-4, T-003-01)
- After review: a pinned address block may not overlap a special-purpose range; an address is in its one text
  form (RFC 5952 for IPv6, never IPv4-mapped), the IPv6 patterns admit at most one `::`, and a hop's optional
  fields refuse `null`. The fixtures' pinned blocks moved off the documentation ranges. A call declared twice has
  its own fixture. (DEC-4, T-003-01)
- After re-review: the special-purpose ranges add the NAT64 well-known prefix `64:ff9b::/96`, Teredo, IPv6
  benchmarking and site-local; `validate_record_secrets` refuses an outbound call record naming a secret its
  operation doesn't declare, with `declared-outbound-call-record-` fixtures. (DEC-4, T-003-01)
