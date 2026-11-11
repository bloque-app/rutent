//! The module interface contract (`REQ-MDL-16`, `DEC-4`, ADR-0003, ADR-0004): the split of a module into a
//! sandboxed part and an outside part, what each may declare, and what crosses between them and out of them.
//!
//! - The **sandboxed part** computes provider-role results from provider-role operations
//!   ([`crate::provider_role`]). It declares only the operation kinds it handles ([`Sandboxed`]). It reaches
//!   nothing outside and never sees secret plaintext (`REQ-MDL-6`): it can only send the outside part an
//!   [`OutsideRequest`] for a declared outside operation, and read back an [`OutsideResponse`].
//! - The **outside part** is the provider's own code (`REQ-MDL-16`). It declares its outside operations, its
//!   destination allow-list, the names of the secrets it may resolve, and the other providers' modules it may
//!   call ([`Outside`]). It resolves a secret by name at call time with a [`SecretLookup`] (`REQ-MDL-17`) that
//!   names the outside operation it serves; [`validate_lookup`] checks that operation declares the secret
//!   (`REQ-MDL-3`, `REQ-MDL-6`). The proxy answers only for the calling process's own provider (`DEC-9`), and a
//!   resolved value never prints through `Debug` ([`SecretValue`], `THR-50`).
//! - **Signing** (`DEC-4`, `T-003-01`). The module never holds a key. The sandboxed part returns a result body;
//!   the outside part signs it with the statement-signing key it declares ([`Outside::statement_key`]). It
//!   resolves the private half through the credential proxy by the deployment's name (the deployment
//!   `{name, operator_key}` of the enveloped advertisement) and signs the RFC 8785 (JCS) canonical JSON of the
//!   whole envelope with only the top-level `signature.value` removed. [`validate_statement_key`] checks that the
//!   key is one of the advertisement's keys listed for `statements`, isn't the `operator_key`, and is the
//!   `signature.key` of a result the declaring provider made, in the advertisement's deployment. Keys are
//!   provider-role's [`PublicKey`], compared by their decoded bytes.
//! - The **platform** records every outbound call with its transaction, module version and destination, hop by
//!   hop ([`OutboundCallRecord`], `REQ-MDL-8`), and mediates [`CrossModuleCall`]s, made only where the route
//!   names them (`REQ-MDL-21`).
//!
//! The language-neutral artefacts are the JSON Schemas under `contracts/module-interface/schemas/`. They assume
//! no module format: bindings to one belong to `EXP-2`. The types here mirror them with `serde`, and
//! [`validate_declaration`], [`validate_record`], [`validate_call`], [`validate_lookup`],
//! [`validate_record_secrets`] and [`validate_statement_key`] enforce the rules a schema can't express. A pinned address block may not overlap a
//! [`SPECIAL_PURPOSE`] range, and an address is in its one text form.
//! - **Admission** (`T-003-02`, `REQ-MDL-1`, `REQ-MDL-2`, `REQ-MDL-3`). A release travels as the provider's
//!   signed [`ReleaseAssertion`]; the platform records it in an [`AdmissionArtefact`] with what it received, the
//!   build that made the machine code, the version history and the acceptance. [`widening`] computes how one
//!   release's capabilities widen the previous one's, a newly handled operation kind included; a widening
//!   release, and a first release, require a fresh acceptance. [`validate_artefact`], [`validate_assertion_key`]
//!   and [`validate_release`] check them; an assertion is signed with a key the accepted advertisement lists for
//!   `advertising`. A module the operator publishes, such as a public node's holding account, has no
//!   advertisement: its assertion is the operator's own, signed with the deployment's `operator_key`, and
//!   [`validate_operator_assertion`] checks it against the deployment the platform serves (4.0.0).
//! - **Refusals** (`REQ-MDL-1`, `REQ-MDL-11`, `REQ-MDL-12`). A platform refuses a release at admission with an
//!   [`AdmissionRefusal`], and an operation's answer with a [`PlatformRefusal`], each naming its own reason: a
//!   limit hit, a disallowed import, a re-admitted version, a release while another waits for acceptance and an
//!   assertion that doesn't verify among them. The reasons a platform gives, these two and those of the outbound
//!   call record, the secret answer and the cross-module call, are `#[non_exhaustive]`: a Rust caller's match
//!   has a wildcard arm. A new reason on the wire is still a major version (`DEC-4`).
//!
//! `contracts/module-interface/README.md` lists which rule lives where.

mod admission;
mod refusal;
mod types;
mod validate;

pub use admission::*;
pub use refusal::{AdmissionRefusal, PlatformRefusal};

pub use types::*;
pub use validate::{
    DeclaredList, RuleViolation, SPECIAL_PURPOSE, validate_call, validate_call_against,
    validate_declaration, validate_lookup, validate_record, validate_record_secrets,
    validate_statement_key,
};

/// The contract's version: the first heading of `contracts/module-interface/CHANGELOG.md`.
pub const VERSION: &str = "4.0.0";
