//! The statement envelope (`urn:rutent:provider-role:statement-envelope`, `REQ-PRT-1`), shared by every party
//! statement in rutent, the router contract's included.
//!
//! **Signing rule** (ruled by the orchestrator, 2026-11-06). The signed bytes are the RFC 8785 (JCS) canonical
//! JSON of the whole envelope with only `signature.value` removed, so `scheme`, `key`, `shape`, `deployment` and
//! `protocol_version` are all signed. The scheme is Ed25519 (RFC 8032), verified strictly. Every integer that can
//! be signed is at most 2^53 - 1, or is carried as a string. [`signing_input`](super::signing_input) computes the
//! signed bytes from a statement's text as received ([`canonical`](super::canonical)); verifying signatures is not
//! implemented here.
//!
//! Every key here is a [`PublicKey`], built only from its canonical encoding and compared by its decoded bytes.
//! The verifier (`M2`) must still refuse non-canonical base64 and non-canonical points ([`PublicKey`] says which).

use alloc::{borrow::ToOwned, string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use super::types::PublicKey;
use super::validate::made_after;

/// The protocol versions a reader takes statements under (`REQ-PRT-1`, `REQ-PRT-4`, finding 11 of `M3`'s milestone
/// review): the current one, and each older one still in force with the cutover date from which a statement under it
/// is void. Before 4.0.0 a reader named one exact version, so a protocol bump voided every statement made under the
/// version before it at once; with an older version in force, a provider re-signs under the current one before the
/// cutover, and a holdout's statements under the old one are void from the date on (`THR-64`).
///
/// A reader with no older version in force names its one version ([`ProtocolVersions::only`]), and takes statements
/// under it alone, as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolVersions {
    current: String,
    older: Vec<Cutover>,
}

/// An older protocol version still in force, and its cutover date: from that instant on, a statement under it is
/// void.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cutover {
    /// The older version.
    pub version: String,
    /// The cutover date, an RFC 3339 UTC time in the schema's form: a statement under `version` is void at it and
    /// after it.
    pub cutover: String,
}

impl ProtocolVersions {
    /// A reader's one version, with no older one in force.
    pub fn only(current: impl Into<String>) -> Self {
        Self {
            current: current.into(),
            older: Vec::new(),
        }
    }

    /// The same versions, with `version` in force until `cutover`. A version named again keeps its first cutover, and
    /// the current version is never an older one.
    #[must_use]
    pub fn with_older(mut self, version: impl Into<String>, cutover: impl Into<String>) -> Self {
        let version = version.into();
        if version != self.current && !self.older.iter().any(|c| c.version == version) {
            self.older.push(Cutover {
                version,
                cutover: cutover.into(),
            });
        }
        self
    }

    /// The current version: the one statements are made under from now on.
    pub fn current(&self) -> &str {
        &self.current
    }

    /// The older versions in force, each with its cutover date.
    pub fn older(&self) -> &[Cutover] {
        &self.older
    }

    /// Whether a statement under `version`, taken at `at` (an RFC 3339 UTC time in the schema's form), is in force:
    /// under the current version always; under an older one only before its cutover date; under any other never. A
    /// time `at` or a cutover that can't be ordered voids the older version's statement, which fails closed.
    pub fn in_force(&self, version: &str, at: &str) -> Result<(), ContextMismatch> {
        if version == self.current {
            return Ok(());
        }
        match self.older.iter().find(|c| c.version == version) {
            Some(older) if made_after(&older.cutover, at) => Ok(()),
            Some(older) => Err(ContextMismatch::PastCutover {
                version: older.version.clone(),
                cutover: older.cutover.clone(),
            }),
            None => Err(ContextMismatch::ProtocolVersion {
                expected: self.current.clone(),
                found: version.to_owned(),
            }),
        }
    }
}

/// A party's statement in its envelope. `T` is the statement's own shape. The schema of each kind of statement
/// fixes `shape` to its `$id`; [`Statement::check_shape`] does the same in Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Statement<T> {
    /// The deployment the statement belongs to; elsewhere it is void (`REQ-PRT-1`).
    pub deployment: Deployment,
    /// The protocol version the statement belongs to (`REQ-PRT-1`).
    pub protocol_version: String,
    /// The `$id` of the schema the statement follows, signed with the rest, so a statement can't be presented
    /// as another kind.
    pub shape: String,
    /// The party that made the statement (`REQ-AUD-2`).
    pub maker: String,
    /// When the maker made it, as an RFC 3339 UTC time.
    pub made_at: String,
    /// The maker's signature.
    pub signature: Signature,
    /// The statement itself.
    pub statement: T,
}

/// A deployment: globally unique, and bound to its operator by the operator's key (`REQ-PRT-1`, `THR-8`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    /// The deployment's name.
    pub name: String,
    /// The operator's Ed25519 public key.
    pub operator_key: PublicKey,
}

/// A maker's signature over the canonical envelope, as the module docs state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    /// The signature scheme.
    pub scheme: SignatureScheme,
    /// The public key that signed: one of the maker's published keys, or the deployment's operator key. Keys
    /// compare by their decoded bytes ([`PublicKey`]).
    pub key: PublicKey,
    /// The 64-byte signature, base64url without padding.
    pub value: String,
}

/// The signature schemes allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureScheme {
    /// Ed25519 (RFC 8032), verified strictly.
    Ed25519,
}

/// The `shape` of an advertisement statement.
pub const ADVERTISEMENT_SHAPE: &str = "urn:rutent:provider-role:advertisement";

/// The `shape` of an operation statement.
pub const OPERATION_SHAPE: &str = "urn:rutent:provider-role:operation";

/// The `shape` of a result statement.
pub const RESULT_SHAPE: &str = "urn:rutent:provider-role:result";

/// The `shape` of a judgement statement.
pub const JUDGEMENT_SHAPE: &str = "urn:rutent:provider-role:judgement";

/// The `shape` of a reserve's reversal confirmation statement.
pub const REVERSAL_CONFIRMATION_SHAPE: &str = "urn:rutent:provider-role:reversal-confirmation";

/// A statement type with a fixed `shape`: the `$id` of its schema.
pub trait StatementShape {
    /// The `$id` a statement of this type must name.
    const SHAPE: &'static str;

    /// Checks the shapes of any statements nested inside this one. None, unless a type says otherwise.
    fn check_nested_shapes(&self) -> Result<(), ShapeMismatch> {
        Ok(())
    }

    /// Checks any statements nested inside this one against the reader's context, at `at`, and that each was made
    /// by `maker`, the outer statement's maker. None, unless a type says otherwise.
    fn check_nested_context(
        &self,
        _maker: &str,
        _deployment: &Deployment,
        _versions: &ProtocolVersions,
        _at: &str,
    ) -> Result<(), ContextMismatch> {
        Ok(())
    }
}

/// A statement whose envelope names a shape other than its type's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeMismatch {
    /// The shape the type requires.
    pub expected: &'static str,
    /// The shape the envelope names.
    pub found: String,
}

/// A statement made for another deployment or protocol version (`REQ-PRT-1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMismatch {
    /// It names another deployment.
    Deployment {
        /// The deployment the reader belongs to.
        expected: Deployment,
        /// The deployment the statement names.
        found: Deployment,
    },
    /// It names a protocol version not in force: neither the reader's current one nor an older one it still takes.
    ProtocolVersion {
        /// The reader's current protocol version.
        expected: String,
        /// The version the statement names.
        found: String,
    },
    /// It names an older protocol version whose cutover date has passed (`REQ-PRT-4`, `THR-64`): void.
    PastCutover {
        /// The older version it names.
        version: String,
        /// That version's cutover date.
        cutover: String,
    },
    /// A statement nested in it was made by another party.
    NestedMaker {
        /// The outer statement's maker.
        outer: String,
        /// The nested statement's maker.
        inner: String,
    },
}

impl<T: StatementShape> Statement<T> {
    /// Checks that the envelope, and any statement nested in it, names its type's shape, as the schemas' `const`
    /// does. Deserialising doesn't check it, so a reader calls this before trusting a statement: otherwise a
    /// signed statement of one kind could be presented as another.
    pub fn check_shape(&self) -> Result<(), ShapeMismatch> {
        if self.shape != T::SHAPE {
            return Err(ShapeMismatch {
                expected: T::SHAPE,
                found: self.shape.clone(),
            });
        }
        self.statement.check_nested_shapes()
    }
}

impl<T: StatementShape> Statement<T> {
    /// Checks that the statement belongs to the reader's deployment and to a protocol version in force at `at`, so a
    /// statement replayed from another deployment or version is refused (`REQ-PRT-1`), and one under an older version
    /// past its cutover date is void (`REQ-PRT-4`, [`ProtocolVersions::in_force`]). `at` is the time the reader judges
    /// the statement at, an RFC 3339 UTC time: when it takes it, or, for a result of a segment begun before a
    /// cutover, when the segment began. The deployment's operator key is compared as well as its name. Statements
    /// nested in it, such as a judgement in a result, are checked the same way, and must have the same maker.
    pub fn check_context(
        &self,
        deployment: &Deployment,
        versions: &ProtocolVersions,
        at: &str,
    ) -> Result<(), ContextMismatch> {
        if &self.deployment != deployment {
            return Err(ContextMismatch::Deployment {
                expected: deployment.clone(),
                found: self.deployment.clone(),
            });
        }
        versions.in_force(&self.protocol_version, at)?;
        self.statement
            .check_nested_context(&self.maker, deployment, versions, at)
    }

    /// Checks that a nested statement was made by `maker` and belongs to the reader's context at `at`.
    pub fn check_as_nested(
        &self,
        maker: &str,
        deployment: &Deployment,
        versions: &ProtocolVersions,
        at: &str,
    ) -> Result<(), ContextMismatch> {
        if self.maker != maker {
            return Err(ContextMismatch::NestedMaker {
                outer: maker.to_owned(),
                inner: self.maker.clone(),
            });
        }
        self.check_context(deployment, versions, at)
    }
}

/// An operation signed with a key other than its deployment's operator key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotSignedByOperator {
    /// The deployment's operator key.
    pub operator_key: PublicKey,
    /// The key the operation names.
    pub key: PublicKey,
}

impl Statement<super::Operation> {
    /// Checks that the operation names the deployment's operator key as its signing key, since an operation is
    /// the operator's statement.
    pub fn check_signed_by_operator(&self) -> Result<(), NotSignedByOperator> {
        if self.signature.key == self.deployment.operator_key {
            Ok(())
        } else {
            Err(NotSignedByOperator {
                operator_key: self.deployment.operator_key.clone(),
                key: self.signature.key.clone(),
            })
        }
    }
}
