//! Signing the platform's results with the provider's statement key (`T-014-05`; `DEC-4`, `THR-4`, `THR-8`).
//!
//! The module never holds a key (module-interface, "Signing a result"). The platform resolves the private half of
//! the statement key the provider's running release declares through the credential proxy, while the provider's
//! module still handles the operation, and signs here: the result, and every statement the module nested in it
//! under the provider's own name, innermost first, under the provider-role signing rule (Ed25519 over the RFC 8785
//! canonical form with only `signature.value` removed). A nested statement of another maker keeps its own
//! signature. The key is used for nothing else and dropped with the result signed.
//!
//! The store holds the private half as the base64url text, unpadded, of its 32-byte Ed25519 seed. It is refused
//! unless its public half is the declared statement key, so a store holding another key can't sign in the
//! provider's name. A statement nested in the provider's name is signed only if it is what the operation asked
//! for: a judgement of the subject and class asked, a reversal confirmation of the segment, asset and amount
//! asked, in the platform's deployment and protocol version (`REQ-PRT-1`, `THR-8`). So the platform never signs a
//! statement about something the operator never asked, which could be lifted out and used alone.

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::module_interface::SecretValue;
use rutent_contracts::provider_role::{
    Deployment, JUDGEMENT_SHAPE, Operation, PublicKey, REVERSAL_CONFIRMATION_SHAPE, ResultBody,
    Statement, signing_input,
};
use serde_json::Value;

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Why a result wasn't signed. No variant carries the key (`THR-50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningError {
    /// The key resolved isn't the base64url text of a 32-byte seed.
    MalformedKey,
    /// The key resolved isn't the private half of the statement key the release declares.
    NotTheDeclaredKey,
    /// A statement the module nested under the provider's name belongs to another deployment.
    NestedForAnotherDeployment,
    /// A statement the module nested under the provider's name names another protocol version than the
    /// platform's.
    NestedOfAnotherVersion,
    /// A statement the module nested under the provider's name is of a shape no result nests: only a judgement
    /// and a reversal confirmation are, so the platform never signs, say, a result for another transaction that
    /// could be lifted out and passed on alone.
    NestedOfAnotherShape,
    /// A statement the module nested under the provider's name isn't about what the operation asked: a judgement
    /// of another subject or class, a confirmation of another segment, asset or amount, or either one in answer
    /// to an operation of another kind.
    NestedNotAsked,
    /// The statement has no signing input under the contract's rule, or doesn't read back once signed.
    NoSigningInput,
}

/// What a result answers, as the platform knows it from its own state: the provider whose module answered, the
/// deployment and protocol version the platform serves, and the operation it handed out.
#[derive(Debug, Clone, Copy)]
pub struct Answering<'a> {
    /// The provider whose module answered, the result's maker.
    pub provider: &'a str,
    /// The deployment the platform serves.
    pub deployment: &'a Deployment,
    /// The protocol version the platform speaks.
    pub protocol_version: &'a str,
    /// The operation the result answers.
    pub operation: &'a Operation,
}

/// `statement`, the answer to `answering`'s operation, signed with `key`, the private half of `declared`: the
/// statement and each statement nested in it in the provider's name, each only if it is what was asked.
pub fn sign_result(
    statement: &Statement<ResultBody>,
    answering: &Answering<'_>,
    key: &SecretValue,
    declared: &PublicKey,
) -> Result<Statement<ResultBody>, SigningError> {
    let seed = decode_seed(key.as_str()).ok_or(SigningError::MalformedKey)?;
    let signing = SigningKey::from_bytes(&seed);
    if signing.verifying_key().as_bytes() != &declared.bytes() {
        return Err(SigningError::NotTheDeclaredKey);
    }
    let asked = Asked::of(answering)?;
    let mut value = serde_json::to_value(statement).map_err(|_| SigningError::NoSigningInput)?;
    if let Value::Object(map) = &mut value {
        for nested in map.values_mut() {
            sign_nested(nested, &asked, &signing, declared)?;
        }
    }
    sign(&mut value, &signing, declared)?;
    serde_json::from_value(value).map_err(|_| SigningError::NoSigningInput)
}

/// What a nested statement in the provider's name must match, as JSON.
struct Asked<'a> {
    provider: &'a str,
    deployment: Value,
    protocol_version: &'a str,
    operation: Value,
}

impl<'a> Asked<'a> {
    fn of(answering: &Answering<'a>) -> Result<Self, SigningError> {
        let json =
            |v: Result<Value, serde_json::Error>| v.map_err(|_| SigningError::NoSigningInput);
        Ok(Asked {
            provider: answering.provider,
            deployment: json(serde_json::to_value(answering.deployment))?,
            protocol_version: answering.protocol_version,
            operation: json(serde_json::to_value(answering.operation))?,
        })
    }

    /// Whether a statement of `shape` whose body is `body` is what the operation asked for: the same kind of
    /// question, and each field the question names equal.
    fn asks(&self, shape: &str, body: &Value) -> Result<(), SigningError> {
        let (kind, fields): (&str, &[&str]) = match shape {
            JUDGEMENT_SHAPE => ("judgement", &["subject", "class"]),
            REVERSAL_CONFIRMATION_SHAPE => {
                ("reversal_confirmation", &["segment", "asset", "amount"])
            }
            _ => return Err(SigningError::NestedOfAnotherShape),
        };
        let asked = self.operation.get("kind").and_then(Value::as_str) == Some(kind)
            && fields.iter().all(|field| {
                let (asked, answered) = (self.operation.get(*field), body.get(*field));
                asked.is_some() && asked == answered
            });
        if asked {
            Ok(())
        } else {
            Err(SigningError::NestedNotAsked)
        }
    }
}

/// Signs each statement inside `value` made by the provider, innermost first, each only if it was asked for.
fn sign_nested(
    value: &mut Value,
    asked: &Asked<'_>,
    key: &SigningKey,
    declared: &PublicKey,
) -> Result<(), SigningError> {
    match value {
        Value::Array(items) => {
            for item in items {
                sign_nested(item, asked, key, declared)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for nested in map.values_mut() {
                sign_nested(nested, asked, key, declared)?;
            }
            let is_statement = map.contains_key("shape")
                && map.contains_key("signature")
                && map.get("maker").and_then(Value::as_str) == Some(asked.provider);
            if !is_statement {
                return Ok(());
            }
            // The result schema pins each nested statement's shape, and the output was checked against it before
            // signing; the signer holds the same line itself, rather than sign any envelope it finds.
            let shape = map.get("shape").and_then(Value::as_str).unwrap_or_default();
            asked.asks(shape, map.get("statement").unwrap_or(&Value::Null))?;
            if map.get("deployment") != Some(&asked.deployment) {
                return Err(SigningError::NestedForAnotherDeployment);
            }
            if map.get("protocol_version").and_then(Value::as_str) != Some(asked.protocol_version) {
                return Err(SigningError::NestedOfAnotherVersion);
            }
            sign(value, key, declared)
        }
        _ => Ok(()),
    }
}

/// Puts `declared` in `statement`'s signature and signs its signing input with `key`.
fn sign(statement: &mut Value, key: &SigningKey, declared: &PublicKey) -> Result<(), SigningError> {
    statement["signature"]["key"] = declared.as_str().into();
    statement["signature"]["value"] = encode(&[0u8; 64]).into();
    let bytes = serde_json::to_vec(&*statement).map_err(|_| SigningError::NoSigningInput)?;
    let input = signing_input(&bytes).map_err(|_| SigningError::NoSigningInput)?;
    statement["signature"]["value"] = encode(&key.sign(&input).to_bytes()).into();
    Ok(())
}

/// The base64url text of `bytes`, unpadded.
fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(BASE64URL[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// The 32 bytes of a seed's canonical text: 43 characters of base64url, the last carrying no unused bits.
fn decode_seed(text: &str) -> Option<[u8; 32]> {
    if text.len() != 43 {
        return None;
    }
    decode(text)?.try_into().ok()
}

/// The bytes of canonical unpadded base64url text: only the one text of those bytes decodes, so unused low bits
/// set would be refused, not read as a second text for the same bytes.
fn decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 4 == 1 {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.as_bytes().chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            let v = BASE64URL.iter().position(|a| a == c)? as u32;
            n |= v << (18 - 6 * i);
        }
        let count = chunk.len() - 1;
        bytes.extend_from_slice(&n.to_be_bytes()[1..=count]);
    }
    (encode(&bytes) == text).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
    use rutent_contracts::module_interface::SecretValue;
    use rutent_contracts::provider_role::{
        Deployment, Operation, PublicKey, ResultBody, Statement, signing_input,
    };
    use serde_json::Value;

    use super::{Answering, Asked, SigningError, decode, decode_seed, encode, sign_result};

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/provider-role/fixtures/valid")
            .join(format!("{name}.json"));
        serde_json::from_slice(&std::fs::read(path).expect("the fixture")).expect("JSON")
    }

    /// A verifier's judgement result, the judgement nested in it by the verifier, `id-check`.
    fn judged() -> Value {
        fixture("result-judgement-yes")
    }

    /// The origin reserve's confirmation result, the confirmation nested in it by the reserve, `acme-co`.
    fn confirmed() -> Value {
        fixture("result-reversal-confirmed-by-the-origin-reserve")
    }

    /// The operation `result` answers, from its fixture, with `changes` made to its body.
    fn asking(name: &str, changes: &[(&str, &str)]) -> Operation {
        let mut operation = fixture(name)["statement"].clone();
        for (field, value) in changes {
            operation[*field] = (*value).into();
        }
        serde_json::from_value(operation).expect("an operation")
    }

    fn judgement_asked() -> Operation {
        asking("operation-judgement-question", &[])
    }

    fn deployment(value: &Value) -> Deployment {
        serde_json::from_value(value["deployment"].clone()).expect("a deployment")
    }

    /// Signs `result` as `maker`'s answer to `operation`, in the result's own deployment and protocol version.
    fn sign_as(
        result: &Value,
        maker: &str,
        operation: &Operation,
        key: &SecretValue,
        declared: &PublicKey,
    ) -> Result<Value, SigningError> {
        let statement: Statement<ResultBody> =
            serde_json::from_value(result.clone()).expect("a result");
        let deployment = deployment(result);
        let version = result["protocol_version"]
            .as_str()
            .expect("a version")
            .to_owned();
        sign_result(
            &statement,
            &Answering {
                provider: maker,
                deployment: &deployment,
                protocol_version: &version,
                operation,
            },
            key,
            declared,
        )
        .map(|signed| serde_json::to_value(&signed).expect("JSON"))
    }

    fn keys(seed: [u8; 32]) -> (SecretValue, PublicKey, SigningKey) {
        let key = SigningKey::from_bytes(&seed);
        let public = PublicKey::new(encode(key.verifying_key().as_bytes())).expect("a key");
        (SecretValue::new(encode(&seed)).expect("text"), public, key)
    }

    /// Whether `statement`'s signature is `key`'s over its signing input.
    fn verifies(statement: &Value, key: &SigningKey) -> bool {
        let bytes = serde_json::to_vec(statement).expect("JSON");
        let input = signing_input(&bytes).expect("a signing input");
        let value = statement["signature"]["value"].as_str().expect("a value");
        let Some(raw) = decode(value).and_then(|raw| <[u8; 64]>::try_from(raw).ok()) else {
            return false;
        };
        statement["signature"]["key"].as_str() == Some(&*encode(key.verifying_key().as_bytes()))
            && VerifyingKey::from(key)
                .verify_strict(&input, &Signature::from_bytes(&raw))
                .is_ok()
    }

    /// For any key, the result and the judgement the module nested in it in the provider's name are signed with
    /// that key, and verify; a key that isn't the declared one signs nothing (`DEC-18`).
    // verifies: THR-4, DEC-9
    #[test]
    fn a_result_and_what_the_provider_nested_in_it_are_signed_with_the_declared_key_only() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut seed = || -> [u8; 32] {
            core::array::from_fn(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
        };
        let asked = judgement_asked();
        for _ in 0..64 {
            let (private, declared, key) = keys(seed());
            let signed =
                sign_as(&judged(), "id-check", &asked, &private, &declared).expect("signed");
            assert!(verifies(&signed, &key));
            assert!(verifies(&signed["statement"]["outcome"]["judgement"], &key));
            let (_, other, _) = keys(seed());
            assert_eq!(
                sign_as(&judged(), "id-check", &asked, &private, &other).err(),
                Some(SigningError::NotTheDeclaredKey)
            );
        }
    }

    /// A module asking the platform to sign a judgement of another subject, or of another class, than the
    /// operation asked about gets nothing signed; the judgement asked about is signed.
    // verifies: THR-8, THR-4, REQ-PRT-1
    #[test]
    fn judgement_about_another_subject_than_asked_is_not_signed() {
        let (private, declared, _) = keys([3u8; 32]);
        assert!(
            sign_as(
                &judged(),
                "id-check",
                &judgement_asked(),
                &private,
                &declared
            )
            .is_ok()
        );
        for (field, value) in [("subject", "subj-someone-else"), ("class", "kyc.company")] {
            let other = asking("operation-judgement-question", &[(field, value)]);
            assert_eq!(
                sign_as(&judged(), "id-check", &other, &private, &declared).err(),
                Some(SigningError::NestedNotAsked),
                "{field}"
            );
        }
        // A judgement nested in answer to an operation that asked none.
        let confirmation = asking("operation-reversal-confirmation-for-one-credit", &[]);
        assert_eq!(
            sign_as(&judged(), "id-check", &confirmation, &private, &declared).err(),
            Some(SigningError::NestedNotAsked)
        );
    }

    /// A module asking the platform to sign a confirmation of another credit than the operation asked about, by
    /// segment, asset or amount, gets nothing signed; the credit asked about is confirmed and signed.
    // verifies: THR-8, THR-4, REQ-PRT-1
    #[test]
    fn confirmation_of_another_credit_than_asked_is_not_signed() {
        let (private, declared, key) = keys([5u8; 32]);
        let asked = asking("operation-reversal-confirmation-for-one-credit", &[]);
        let signed = sign_as(&confirmed(), "acme-co", &asked, &private, &declared).expect("signed");
        assert!(verifies(
            &signed["statement"]["outcome"]["confirmation"],
            &key
        ));
        for (field, value) in [("segment", "tx-999"), ("asset", "EURC"), ("amount", "999")] {
            let other = asking(
                "operation-reversal-confirmation-for-one-credit",
                &[(field, value)],
            );
            assert_eq!(
                sign_as(&confirmed(), "acme-co", &other, &private, &declared).err(),
                Some(SigningError::NestedNotAsked),
                "{field}"
            );
        }
    }

    /// A statement nested in the provider's name for another protocol version than the platform's is not signed
    /// (`REQ-PRT-1`); one for another deployment neither.
    // verifies: REQ-PRT-1, THR-8
    #[test]
    fn nested_statement_of_another_version_or_deployment_is_not_signed() {
        let (private, declared, _) = keys([7u8; 32]);
        let mut value = judged();
        value["statement"]["outcome"]["judgement"]["protocol_version"] = "9.9.9".into();
        assert_eq!(
            sign_as(&value, "id-check", &judgement_asked(), &private, &declared).err(),
            Some(SigningError::NestedOfAnotherVersion)
        );
        let mut value = judged();
        value["statement"]["outcome"]["judgement"]["deployment"]["name"] = "elsewhere".into();
        assert_eq!(
            sign_as(&value, "id-check", &judgement_asked(), &private, &declared).err(),
            Some(SigningError::NestedForAnotherDeployment)
        );
    }

    /// A statement nested by another maker keeps its signature; a result-shaped envelope in the provider's name
    /// is never signed; a key that isn't a seed's one text signs nothing.
    // verifies: THR-8, THR-4
    #[test]
    fn nested_statements_of_others_are_kept_and_of_another_shape_refused() {
        let (private, declared, key) = keys([9u8; 32]);
        let mut value = judged();
        value["statement"]["outcome"]["judgement"]["maker"] = "another-party".into();
        let theirs = value["statement"]["outcome"]["judgement"]["signature"].clone();
        let signed =
            sign_as(&value, "id-check", &judgement_asked(), &private, &declared).expect("signed");
        assert_eq!(
            signed["statement"]["outcome"]["judgement"]["signature"],
            theirs
        );
        assert!(verifies(&signed, &key));

        // A result-shaped envelope in the provider's name, wherever it sits, is never signed: it could be lifted
        // out and passed on as a result of its own.
        let deployment = deployment(&judged());
        let operation = judgement_asked();
        let asked = Asked::of(&Answering {
            provider: "id-check",
            deployment: &deployment,
            protocol_version: "2.0.1",
            operation: &operation,
        })
        .expect("asked");
        let mut holder = serde_json::json!({ "inner": judged() });
        assert_eq!(
            super::sign_nested(&mut holder, &asked, &key, &declared),
            Err(SigningError::NestedOfAnotherShape)
        );

        let short = SecretValue::new("AAAA").expect("text");
        assert_eq!(
            sign_as(&judged(), "id-check", &judgement_asked(), &short, &declared).err(),
            Some(SigningError::MalformedKey)
        );
    }

    /// Every seed has one text, and only that text decodes to it (`DEC-18`).
    // verifies: THR-4
    #[test]
    fn a_seed_reads_back_from_its_one_text_only() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..500 {
            let seed: [u8; 32] = core::array::from_fn(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            });
            let text = encode(&seed);
            assert_eq!(decode_seed(&text), Some(seed));
            let mut other = text.clone().into_bytes();
            // Another last character is another seed, or none: never a second text for this one.
            let last = other[42];
            other[42] = if last == b'A' { b'B' } else { b'A' };
            let other = String::from_utf8(other).expect("text");
            assert!(decode_seed(&other).is_none_or(|s| s != seed));
            assert_eq!(decode_seed(&text[..42]), None);
        }
    }
}
