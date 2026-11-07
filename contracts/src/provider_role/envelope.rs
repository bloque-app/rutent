//! The statement envelope (`urn:rutent:provider-role:statement-envelope`, `REQ-PRT-1`), shared by every party
//! statement in rutent, the router contract's included.
//!
//! **Signing rule** (ruled by the orchestrator, 2026-11-06). The signed bytes are the RFC 8785 (JCS) canonical
//! JSON of the whole envelope with only `signature.value` removed, so `scheme`, `key`, `shape`, `deployment` and
//! `protocol_version` are all signed. The scheme is Ed25519 (RFC 8032), verified strictly. Every integer that can
//! be signed is at most 2^53 - 1, or is carried as a string. Verifying signatures is not implemented here.

use serde::{Deserialize, Serialize};

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
    /// The operator's Ed25519 public key, base64url without padding.
    pub operator_key: String,
}

/// A maker's signature over the canonical envelope, as the module docs state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    /// The signature scheme.
    pub scheme: SignatureScheme,
    /// The public key that signed: one of the maker's published keys, or the deployment's operator key.
    pub key: String,
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

/// A statement type with a fixed `shape`: the `$id` of its schema.
pub trait StatementShape {
    /// The `$id` a statement of this type must name.
    const SHAPE: &'static str;

    /// Checks the shapes of any statements nested inside this one. None, unless a type says otherwise.
    fn check_nested_shapes(&self) -> Result<(), ShapeMismatch> {
        Ok(())
    }

    /// Checks any statements nested inside this one against the reader's context, and that each was made by
    /// `maker`, the outer statement's maker. None, unless a type says otherwise.
    fn check_nested_context(
        &self,
        _maker: &str,
        _deployment: &Deployment,
        _protocol_version: &str,
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
    /// It names another protocol version.
    ProtocolVersion {
        /// The reader's protocol version.
        expected: String,
        /// The version the statement names.
        found: String,
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
    /// Checks that the statement belongs to the reader's deployment and protocol version, so a statement replayed
    /// from another is refused (`REQ-PRT-1`). The deployment's operator key is compared as well as its name.
    /// Statements nested in it, such as a judgement in a result, are checked the same way, and must have the
    /// same maker.
    pub fn check_context(
        &self,
        deployment: &Deployment,
        protocol_version: &str,
    ) -> Result<(), ContextMismatch> {
        if &self.deployment != deployment {
            return Err(ContextMismatch::Deployment {
                expected: deployment.clone(),
                found: self.deployment.clone(),
            });
        }
        if self.protocol_version != protocol_version {
            return Err(ContextMismatch::ProtocolVersion {
                expected: protocol_version.to_owned(),
                found: self.protocol_version.clone(),
            });
        }
        self.statement
            .check_nested_context(&self.maker, deployment, protocol_version)
    }

    /// Checks that a nested statement was made by `maker` and belongs to the reader's context.
    pub fn check_as_nested(
        &self,
        maker: &str,
        deployment: &Deployment,
        protocol_version: &str,
    ) -> Result<(), ContextMismatch> {
        if self.maker != maker {
            return Err(ContextMismatch::NestedMaker {
                outer: maker.to_owned(),
                inner: self.maker.clone(),
            });
        }
        self.check_context(deployment, protocol_version)
    }
}

/// An operation signed with a key other than its deployment's operator key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotSignedByOperator {
    /// The deployment's operator key.
    pub operator_key: String,
    /// The key the operation names.
    pub key: String,
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
