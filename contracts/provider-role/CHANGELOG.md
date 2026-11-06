# provider-role changelog

## 0.2.0
- Tightened after review: a tuple listed twice is invalid; the evidence grade is the provider's weakest; private
  nodes are unique identifiers and an outer node names only its network, both implied by the advertisement; ramp
  and reserve tuples declare their unit at each end; a verifier-only provider needn't declare evidence or safe
  retry; the node schema is removed. (DEC-4, DEC-13, DEC-16, T-002-01)
- Integers must be plain JSON integers, which the Rust types enforce and the README records; a reserve tuple
  listed twice gets its own invalid fixture. (DEC-4, T-002-01)

## 0.1.0
- Advertisement and declaration schemas: the four categories, tuples per role, private nodes and their
  publisher, per-tuple terms, evidence, safe retry, tracking, required inputs and a reserve's declaration.
  (DEC-4, DEC-13, DEC-16, T-002-01)
