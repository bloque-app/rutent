//! Signing the platform's results with the provider's statement key (`T-014-05`; `DEC-4`, `THR-4`, `THR-8`).
//!
//! The module never holds a key (module-interface, "Signing a result"). The platform resolves the private half of
//! the statement key the provider's running release declares through the credential proxy, once the operation began
//! and before the module is handed it, so a store that can't give it, or gives another key ([`check_key`]),
//! refuses the operation before anything of it runs. It signs here, once the module answered: the result, and every
//! statement the module nested in it under the provider's own name, innermost first, under the provider-role
//! signing rule (Ed25519 over the RFC 8785 canonical form with only `signature.value` removed). A nested statement
//! of another maker keeps its own signature. The key is used for nothing else and dropped with the result signed.
//!
//! The store holds the private half as the base64url text, unpadded, of its 32-byte Ed25519 seed. It is refused
//! unless its public half is the declared statement key, so a store holding another key can't sign in the
//! provider's name. A statement nested in the provider's name is signed only if it is what the operation asked
//! for: a judgement of the subject and class asked, a reversal confirmation of the segment, asset and amount
//! asked, in the platform's deployment and protocol version (`REQ-PRT-1`, `THR-8`). So the platform never signs a
//! statement about something the operator never asked, which could be lifted out and used alone.
//!
//! Two more are signed since `T-020-56` (provider-role 4.0.0's carriers, `REQ-QTE-6`, `REQ-QTE-15`): the provider's
//! terms in its answer to a dry run, only for the tuple that dry run asks about (the provider asked, and the operation's
//! own two ends and assets), and a reserve's hold in its answer to a hold request, only of the quote asked (its
//! transaction, record, candidate and whom it is for) and valid no later than asked. Neither is signed in answer to
//! anything else, a status question included: terms ride on a dry run alone, a hold on its request alone.

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::module_interface::SecretValue;
use rutent_contracts::provider_role::{
    Deployment, HOLD_SHAPE, INNER_SEGMENTS_SHAPE, JUDGEMENT_SHAPE, Operation, PublicKey,
    RESULT_SHAPE, REVERSAL_CONFIRMATION_SHAPE, ResultBody, Statement, TERMS_SHAPE, signing_input,
};
use rutent_contracts::router_api::Timestamp;
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
    /// A statement the module nested under the provider's name is of a shape no result nests: only a judgement, a
    /// reversal confirmation, a status answer's result, a dry run's terms and a hold are, so the platform never signs,
    /// say, an advertisement that could be lifted out and passed on alone.
    NestedOfAnotherShape,
    /// A statement the module nested under the provider's name isn't about what the operation asked: a judgement
    /// of another subject or class, a confirmation of another segment, asset or amount, terms of another tuple than
    /// the dry run's, a hold of another quote or past the validity asked, or any of them in answer to an operation of
    /// another kind.
    NestedNotAsked,
    /// A status answer that names another question's transaction, or another transaction asked about, than the
    /// question it answers (`T-013-11`): the platform never signs a statement about something it wasn't asked.
    AnswerOfAnotherQuestion,
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
    /// For a status question: what the platform holds of the operation it asked about (`T-013-11`). `None` for any
    /// other question, and for a status question about an operation the platform holds nothing of, in which case no
    /// result is nested.
    pub about: Option<AboutAsked<'a>>,
}

/// The operation a status question asks about, and the final answer the platform already holds for it, if any.
#[derive(Debug, Clone, Copy)]
pub struct AboutAsked<'a> {
    /// The operation the platform carried out under the transaction asked about: a nested result must be of its
    /// kind, and a nested judgement or confirmation is judged against it.
    pub operation: &'a Operation,
    /// The platform's own record of the provider's answer to it, if it holds one: a nested result that differs
    /// from a final one (`succeeded` or `failed`) is not signed.
    pub held: Option<&'a ResultBody>,
}

/// `statement`, the answer to `answering`'s operation, signed with `key`, the private half of `declared`: the
/// statement and each statement nested in it in the provider's name, each only if it is what was asked.
pub fn sign_result(
    statement: &Statement<ResultBody>,
    answering: &Answering<'_>,
    key: &SecretValue,
    declared: &PublicKey,
) -> Result<Statement<ResultBody>, SigningError> {
    let signing = signing_key(key, declared)?;
    if let (Operation::StatusQuery(question), ResultBody::StatusQuery(answer)) =
        (answering.operation, &statement.statement)
        && (question.transaction != answer.transaction || question.about != answer.about)
    {
        return Err(SigningError::AnswerOfAnotherQuestion);
    }
    if let (Operation::InnerSegmentsQuery(question), ResultBody::InnerSegmentsQuery(answer)) =
        (answering.operation, &statement.statement)
        && (question.transaction != answer.transaction || question.node != answer.node)
    {
        return Err(SigningError::AnswerOfAnotherQuestion);
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

/// Whether `key` is the private half of `declared`, the statement key a release declares: what the platform checks
/// before it hands a module an operation, so a store holding another key refuses the operation before anything of it
/// runs (module-interface 4.0.0's `StatementKeyMisconfigured`).
pub fn check_key(key: &SecretValue, declared: &PublicKey) -> Result<(), SigningError> {
    signing_key(key, declared).map(|_| ())
}

/// `key` as a signing key, if it is the private half of `declared`.
fn signing_key(key: &SecretValue, declared: &PublicKey) -> Result<SigningKey, SigningError> {
    let seed = decode_seed(key.as_str()).ok_or(SigningError::MalformedKey)?;
    let signing = SigningKey::from_bytes(&seed);
    if signing.verifying_key().as_bytes() != &declared.bytes() {
        return Err(SigningError::NotTheDeclaredKey);
    }
    Ok(signing)
}

/// What a nested statement in the provider's name must match, as JSON.
struct Asked<'a> {
    provider: &'a str,
    deployment: Value,
    protocol_version: &'a str,
    operation: Value,
    /// The operation a status question asks about, and the final answer held for it.
    about: Option<(Value, Option<Value>)>,
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
            about: answering
                .about
                .map(|about| -> Result<_, SigningError> {
                    let held = about
                        .held
                        .map(|held| json(serde_json::to_value(held)))
                        .transpose()?
                        .filter(|held| {
                            // A final answer: done, failed, or refused for good. Not one that lifts, or says nothing.
                            matches!(
                                held["outcome"]["status"].as_str(),
                                Some("succeeded" | "failed")
                            ) || (held["outcome"]["status"] == "refused"
                                && held["outcome"]["refusal"]["class"] == "final")
                        });
                    Ok((json(serde_json::to_value(about.operation))?, held))
                })
                .transpose()?,
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
            // The provider's own result for the transaction a status question asks about, nested in its answer
            // (provider-role 4.0.0, `T-013-11`): signed only in answer to a status question, and only if it is the
            // result of the transaction asked about and of an operation, never of a status question or a hold.
            RESULT_SHAPE => return self.asks_a_result(body),
            // The provider's terms handed back in its answer to a dry run, and a reserve's hold in its answer to a hold
            // request (provider-role 4.0.0, `T-020-56`): each only in answer to its own question.
            TERMS_SHAPE => return self.asks_terms(body),
            HOLD_SHAPE => return self.asks_a_hold(body),
            // A provider's record of a private node, in answer to the question of that node's record alone
            // (provider-role 4.0.0, `T-020-58`).
            INNER_SEGMENTS_SHAPE => return self.asks_an_inner_segments_record(body),
            _ => return Err(SigningError::NestedOfAnotherShape),
        };
        // In answer to a status question, a statement the provider's own result nests is judged against the operation
        // asked about, which that result answers (`T-013-11`).
        let target = self
            .about
            .as_ref()
            .map_or(&self.operation, |(about, _)| about);
        let asked = target.get("kind").and_then(Value::as_str) == Some(kind)
            && fields.iter().all(|field| {
                let (asked, answered) = (target.get(*field), body.get(*field));
                asked.is_some() && asked == answered
            });
        if asked {
            Ok(())
        } else {
            Err(SigningError::NestedNotAsked)
        }
    }

    /// Whether `body`, terms the provider nested, are its terms for the tuple the operation, a dry run, asks about: a
    /// withdrawal's, deposit's, transfer's or conversion's dry run, never a real movement or another question, the
    /// tuple's provider the one asked, and its two ends the operation's own nodes and assets (`REQ-QTE-6`). Never in
    /// answer to a status question, whose result answers an operation already carried out, never a dry run.
    fn asks_terms(&self, body: &Value) -> Result<(), SigningError> {
        let operation = &self.operation;
        let kind = operation.get("kind").and_then(Value::as_str);
        let ends = match kind {
            Some("withdrawal" | "deposit" | "transfer") => (
                (operation.get("origin"), operation.get("asset")),
                (operation.get("destination"), operation.get("asset")),
            ),
            Some("conversion") => (
                (operation.get("node"), operation.get("from")),
                (operation.get("node"), operation.get("to")),
            ),
            _ => return Err(SigningError::NestedNotAsked),
        };
        let tuple = body.get("tuple");
        let end_is = |end: Option<&Value>, (node, asset): (Option<&Value>, Option<&Value>)| {
            let end = end.unwrap_or(&Value::Null);
            node.is_some()
                && asset.is_some_and(Value::is_string)
                && end.get("node") == node
                && end.get("asset").and_then(|a| a.get("id")) == asset
        };
        let asked = self.about.is_none()
            && operation.get("dry_run").and_then(Value::as_bool) == Some(true)
            && operation.get("provider").and_then(Value::as_str) == Some(self.provider)
            && tuple
                .and_then(|t| t.get("provider"))
                .and_then(Value::as_str)
                == Some(self.provider)
            && end_is(tuple.and_then(|t| t.get("from")), ends.0)
            && end_is(tuple.and_then(|t| t.get("to")), ends.1);
        if asked {
            Ok(())
        } else {
            Err(SigningError::NestedNotAsked)
        }
    }

    /// Whether `body`, a hold the reserve nested, is its hold of the quote the operation, a hold request, asks for: the
    /// request's own transaction, the record, the candidate and whom it is for, and a validity no later than the one
    /// asked, both read as times (`REQ-QTE-15`). Never in answer to anything but a hold request, a status question
    /// included.
    fn asks_a_hold(&self, body: &Value) -> Result<(), SigningError> {
        let operation = &self.operation;
        let same =
            |field: &str| operation.get(field).is_some() && operation.get(field) == body.get(field);
        let time = |value: &Value, field: &str| {
            value
                .get(field)
                .and_then(Value::as_str)
                .and_then(|text| Timestamp::new(text.to_owned()))
                .map(|t| t.unix_nanos())
        };
        let asked = self.about.is_none()
            && operation.get("kind").and_then(Value::as_str) == Some("hold")
            && operation.get("provider").and_then(Value::as_str) == Some(self.provider)
            && same("transaction")
            && same("record")
            && same("candidate")
            && same("for")
            && match (time(body, "valid_until"), time(operation, "valid_until")) {
                (Some(held), Some(asked)) => held <= asked,
                _ => false,
            };
        if asked {
            Ok(())
        } else {
            Err(SigningError::NestedNotAsked)
        }
    }

    /// Whether `body`, a record the provider nested, is its record of the private node the operation, a question of a
    /// node's record, asks about, the provider asked being the one that answers. Never in answer to anything else, a
    /// status question and a dry run included, so the platform signs no record a question didn't ask for, which could be
    /// lifted out and presented as the publisher's word about another node (`REQ-ORD-41`).
    fn asks_an_inner_segments_record(&self, body: &Value) -> Result<(), SigningError> {
        let operation = &self.operation;
        let asked = self.about.is_none()
            && operation.get("kind").and_then(Value::as_str) == Some("inner_segments_query")
            && operation.get("provider").and_then(Value::as_str) == Some(self.provider)
            && operation.get("node").is_some()
            && operation.get("node") == body.get("node");
        if asked {
            Ok(())
        } else {
            Err(SigningError::NestedNotAsked)
        }
    }

    /// Whether `body` is the result of the transaction the operation, a status question, asks about: of the kind of
    /// the operation the platform carried out under it, never a status answer or a hold, and, where the platform
    /// holds a final answer for it, the same.
    fn asks_a_result(&self, body: &Value) -> Result<(), SigningError> {
        let Some((about, held)) = &self.about else {
            return Err(SigningError::NestedNotAsked);
        };
        let kind = body.get("kind").and_then(Value::as_str);
        let asked = self.operation.get("kind").and_then(Value::as_str) == Some("status_query")
            && self.operation.get("about").is_some()
            && self.operation.get("about") == body.get("transaction")
            && kind.is_some()
            && kind != Some("status_query")
            && kind != Some("hold")
            && kind != Some("inner_segments_query")
            && about
                .get("kind")
                .and_then(Value::as_str)
                .map(|k| k.to_owned())
                == kind.map(str::to_owned)
            && about.get("transaction") == body.get("transaction")
            && held
                .as_ref()
                .is_none_or(|held| unsigned(held) == unsigned(body));
        if asked {
            Ok(())
        } else {
            Err(SigningError::NestedNotAsked)
        }
    }
}

/// `value` with every signature's value taken out, so two statements are compared by what they say.
fn unsigned(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, inner)| {
                    let inner = match (key.as_str(), inner) {
                        ("signature", Value::Object(signature)) => Value::Object(
                            signature
                                .iter()
                                .filter(|(name, _)| name.as_str() != "value")
                                .map(|(name, v)| (name.clone(), v.clone()))
                                .collect(),
                        ),
                        _ => unsigned(inner),
                    };
                    (key.clone(), inner)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(unsigned).collect()),
        other => other.clone(),
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
                about: None,
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
            about: None,
        })
        .expect("asked");
        let mut holder = serde_json::json!({ "inner": judged() });
        assert_eq!(
            super::sign_nested(&mut holder, &asked, &key, &declared),
            Err(SigningError::NestedNotAsked),
            "a result nested in answer to anything but a status question"
        );

        let short = SecretValue::new("AAAA").expect("text");
        assert_eq!(
            sign_as(&judged(), "id-check", &judgement_asked(), &short, &declared).err(),
            Some(SigningError::MalformedKey)
        );
    }

    /// [`sign_as`], for a status question about `about`, the operation the platform carried out, whose answer it
    /// holds as `held`.
    fn sign_about(
        result: &Value,
        question: &Operation,
        about: &Operation,
        held: Option<&ResultBody>,
        key: &SecretValue,
        declared: &PublicKey,
    ) -> Result<Value, SigningError> {
        let statement: Statement<ResultBody> =
            serde_json::from_value(result.clone()).expect("a result");
        let deployment = deployment(result);
        let version = result["protocol_version"].as_str().expect("a version");
        sign_result(
            &statement,
            &Answering {
                provider: "acme-co",
                deployment: &deployment,
                protocol_version: version,
                operation: question,
                about: Some(crate::proxy::AboutAsked {
                    operation: about,
                    held,
                }),
            },
            key,
            declared,
        )
        .map(|signed| serde_json::to_value(&signed).expect("JSON"))
    }

    /// The provider's own result for the transaction a status question asks about is signed with the provider's key,
    /// nested in its answer, only if it is the result of that transaction, of the kind of the operation the platform
    /// carried out under it, and the same as the final answer the platform holds; the answer itself must be to the
    /// question asked (`T-013-11`, `THR-8`, `REQ-ORD-1`).
    // verifies: REQ-ORD-1, THR-8
    #[test]
    fn result_nested_in_a_status_answer_is_signed_only_for_the_transaction_asked_about() {
        let (private, declared, key) = keys([11u8; 32]);
        let answered = fixture("result-status-query-answered-with-the-providers-result");
        let asked = asking("operation-status-query", &[]);
        let carried = asking("operation-withdrawal-from-a-bank-reserve", &[]);
        let sign = |result: &Value, question: &Operation, about: &Operation| {
            sign_about(result, question, about, None, &private, &declared)
        };
        let signed = sign(&answered, &asked, &carried).expect("signed");
        assert!(verifies(&signed, &key));
        assert!(verifies(&signed["statement"]["outcome"]["result"], &key));
        // No operation the platform holds of the transaction asked about, no nested result.
        assert_eq!(
            sign_as(&answered, "acme-co", &asked, &private, &declared).err(),
            Some(SigningError::NestedNotAsked)
        );

        // A question about another transaction than the result's.
        let other = asking("operation-status-query", &[("about", "tx-another")]);
        assert_eq!(
            sign(&answered, &other, &carried).err(),
            Some(SigningError::AnswerOfAnotherQuestion)
        );
        // An answer about another transaction than the question asks about, or to another question.
        for (field, value) in [("about", "tx-another"), ("transaction", "tx-status-2")] {
            let mut lying = answered.clone();
            lying["statement"][field] = value.into();
            assert_eq!(
                sign(&lying, &asked, &carried).err(),
                Some(SigningError::AnswerOfAnotherQuestion),
                "{field}"
            );
        }
        // A result that is itself a status answer or a hold, however it names the transaction.
        for kind in ["status_query", "hold"] {
            let mut nested = answered.clone();
            let refused =
                serde_json::json!({ "status": "refused", "refusal": { "class": "final" } });
            nested["statement"]["outcome"]["result"]["statement"] = if kind == "hold" {
                serde_json::json!({ "kind": kind, "transaction": "tx-001", "outcome": refused })
            } else {
                serde_json::json!({
                    "kind": kind, "transaction": "tx-001", "about": "tx-0", "outcome": refused
                })
            };
            assert_eq!(
                sign(&nested, &asked, &carried).err(),
                Some(SigningError::NestedNotAsked),
                "{kind}"
            );
        }
        // A result of another kind than the operation carried out: a second signed result for the transaction.
        let mut transfer = answered.clone();
        transfer["statement"]["outcome"]["result"]["statement"] = serde_json::json!({
            "kind": "transfer", "transaction": "tx-001",
            "outcome": { "status": "unknown" }
        });
        assert_eq!(
            sign(&transfer, &asked, &carried).err(),
            Some(SigningError::NestedNotAsked)
        );
        // In answer to another kind of question.
        let withdrawal = asking("operation-withdrawal-from-a-bank-reserve", &[]);
        assert_eq!(
            sign(&answered, &withdrawal, &carried).err(),
            Some(SigningError::NestedNotAsked)
        );
    }

    /// Where the platform holds a final answer for the transaction asked about, a nested result that says anything
    /// else is not signed; one it holds as not final, or the same, is (`T-013-11`).
    // verifies: REQ-ORD-1, INV-4
    #[test]
    fn nested_result_that_differs_from_a_final_answer_the_platform_holds_is_not_signed() {
        let (private, declared, _) = keys([12u8; 32]);
        let answered = fixture("result-status-query-answered-with-the-providers-result");
        let asked = asking("operation-status-query", &[]);
        let carried = asking("operation-withdrawal-from-a-bank-reserve", &[]);
        let nested: ResultBody =
            serde_json::from_value(answered["statement"]["outcome"]["result"]["statement"].clone())
                .expect("a result body");
        let sign = |held: Option<&ResultBody>| {
            sign_about(&answered, &asked, &carried, held, &private, &declared)
        };
        // The same final answer, signed or not signed in the held copy.
        assert!(sign(Some(&nested)).is_ok());
        // Another final answer: the same withdrawal failed instead of succeeded.
        let mut failed = serde_json::to_value(&nested).expect("JSON");
        let evidence = failed["outcome"]["evidence"].clone();
        failed["outcome"] = serde_json::json!({ "status": "failed", "evidence": evidence });
        let failed: ResultBody = serde_json::from_value(failed).expect("a failed withdrawal");
        assert_eq!(
            sign(Some(&failed)).err(),
            Some(SigningError::NestedNotAsked)
        );
        // A refusal for good binds the nested result too; one that lifts doesn't.
        let refused = |refusal: serde_json::Value| -> ResultBody {
            let mut body = serde_json::to_value(&nested).expect("JSON");
            body["outcome"] = serde_json::json!({ "status": "refused", "refusal": refusal });
            serde_json::from_value(body).expect("a refusal")
        };
        assert_eq!(
            sign(Some(&refused(serde_json::json!({ "class": "final" })))).err(),
            Some(SigningError::NestedNotAsked)
        );
        // A refusal that lifts says nothing final: it doesn't bind the nested result.
        let lifts = serde_json::json!({
            "class": "lifts_at_known_time", "lifts_at": "2026-11-07T00:00:00Z"
        });
        assert!(sign(Some(&refused(lifts))).is_ok());
        // One that isn't final doesn't hold the provider to it.
        let mut unknown = serde_json::to_value(&nested).expect("JSON");
        unknown["outcome"] = serde_json::json!({ "status": "unknown" });
        let unknown: ResultBody = serde_json::from_value(unknown).expect("an unknown outcome");
        assert!(sign(Some(&unknown)).is_ok());
    }

    /// A reversal confirmation nested in a status answer about a reversal confirmation is judged against that
    /// operation's segment, asset and amount, as it is in answer to the question itself (`T-013-11`).
    // verifies: REQ-ORD-1, THR-8
    #[test]
    fn confirmation_nested_in_a_status_answer_is_judged_against_the_operation_asked_about() {
        let (private, declared, key) = keys([13u8; 32]);
        let confirmed = fixture("result-reversal-confirmed-by-the-origin-reserve");
        let carried = asking("operation-reversal-confirmation-for-one-credit", &[]);
        let mut answered = fixture("result-status-query-answered-with-the-providers-result");
        answered["statement"]["about"] = "tx-015".into();
        answered["statement"]["outcome"]["result"] = confirmed;
        let asked = asking("operation-status-query", &[("about", "tx-015")]);
        let signed =
            sign_about(&answered, &asked, &carried, None, &private, &declared).expect("signed");
        assert!(verifies(&signed, &key));
        assert!(verifies(
            &signed["statement"]["outcome"]["result"]["statement"]["outcome"]["confirmation"],
            &key
        ));
        // Another credit than the operation asked about: not signed.
        let other = asking(
            "operation-reversal-confirmation-for-one-credit",
            &[("amount", "7")],
        );
        assert_eq!(
            sign_about(&answered, &asked, &other, None, &private, &declared).err(),
            Some(SigningError::NestedNotAsked)
        );
    }

    /// A dry run of the withdrawal the terms fixture's tuple is for: from `co-ach` to `bank-ledger`, in COP, at
    /// `acme-co`, with `changes` made to its body.
    fn dry_run_asked(changes: &[(&str, Value)]) -> Operation {
        let mut operation = serde_json::json!({
            "kind": "withdrawal", "transaction": "tx-012", "provider": "acme-co", "session": "sess-1",
            "origin": { "network": "co-ach" }, "destination": "bank-ledger", "asset": "COP",
            "amount": "25000000", "dry_run": true, "inputs": {},
        });
        for (field, value) in changes {
            operation[*field] = value.clone();
        }
        serde_json::from_value(operation).expect("a withdrawal")
    }

    /// The terms a provider nests in its answer to a dry run are signed with its key only where they are its terms
    /// for the tuple that dry run asks about: the provider asked, and the operation's own two ends and asset. Terms of
    /// another tuple, in answer to a real movement, or in answer to another question, are not signed (`T-020-56`,
    /// `REQ-QTE-6`, `THR-8`).
    // verifies: REQ-QTE-6, THR-8
    #[test]
    fn terms_nested_in_a_dry_run_answer_are_signed_only_for_the_tuple_asked() {
        let (private, declared, key) = keys([15u8; 32]);
        let answered = fixture("result-withdrawal-handing-back-its-terms");
        let signed = sign_as(
            &answered,
            "acme-co",
            &dry_run_asked(&[]),
            &private,
            &declared,
        )
        .expect("signed");
        assert!(verifies(&signed, &key));
        assert!(verifies(&signed["statement"]["outcome"]["terms"], &key));

        // A dry run of another tuple: another origin, destination or asset, or at another provider.
        for (field, value) in [
            ("origin", serde_json::json!({ "network": "co-other" })),
            ("destination", "other-ledger".into()),
            ("asset", "USD".into()),
            ("provider", "other-co".into()),
        ] {
            assert_eq!(
                sign_as(
                    &answered,
                    "acme-co",
                    &dry_run_asked(&[(field, value)]),
                    &private,
                    &declared
                )
                .err(),
                Some(SigningError::NestedNotAsked),
                "{field}"
            );
        }
        // Terms naming another provider's tuple, in the provider's own name.
        let mut theirs = answered.clone();
        theirs["statement"]["outcome"]["terms"]["statement"]["tuple"]["provider"] =
            "other-co".into();
        assert_eq!(
            sign_as(&theirs, "acme-co", &dry_run_asked(&[]), &private, &declared).err(),
            Some(SigningError::NestedNotAsked)
        );
        // In answer to the real movement, not a dry run.
        assert_eq!(
            sign_as(
                &answered,
                "acme-co",
                &dry_run_asked(&[("dry_run", false.into())]),
                &private,
                &declared
            )
            .err(),
            Some(SigningError::NestedNotAsked)
        );
        // In answer to another question: a hold request.
        let hold = asking("operation-hold-of-a-chosen-quote", &[]);
        assert_eq!(
            sign_as(&answered, "acme-co", &hold, &private, &declared).err(),
            Some(SigningError::NestedNotAsked)
        );
    }

    /// `result` with `terms`, the provider's terms envelope from the withdrawal fixture, nested in its outcome, made by
    /// `maker` for its tuple `from` → `to`, in the result's own deployment and protocol version.
    fn with_terms(result: &str, maker: &str, from: Value, to: Value) -> Value {
        let mut answered = fixture(result);
        let mut terms = fixture("result-withdrawal-handing-back-its-terms")["statement"]["outcome"]
            ["terms"]
            .clone();
        terms["maker"] = maker.into();
        terms["deployment"] = answered["deployment"].clone();
        terms["protocol_version"] = answered["protocol_version"].clone();
        terms["statement"]["tuple"] =
            serde_json::json!({ "provider": maker, "from": from, "to": to });
        answered["statement"]["outcome"]["terms"] = terms;
        answered
    }

    /// The terms of `answered`'s outcome with `change` made to their tuple.
    fn changed(answered: &Value, change: impl FnOnce(&mut Value)) -> Value {
        let mut other = answered.clone();
        change(&mut other["statement"]["outcome"]["terms"]["statement"]["tuple"]);
        other
    }

    /// The signer's rule for terms, for each kind of dry run besides a withdrawal's: a conversion's (at its one node,
    /// from the asset it takes in to the one it gives out, so a rate the other way round, or terms at another node, are
    /// refused), a transfer's (between its two nodes) and a deposit's (from its inner node to its outer one), each
    /// signed for its own tuple and refused for its ends swapped (`T-020-56`'s review, finding 1; `REQ-QTE-6`, `THR-8`).
    // verifies: REQ-QTE-6, THR-8
    #[test]
    fn terms_nested_in_a_conversions_transfers_or_deposits_dry_run_answer_are_signed_only_for_its_tuple()
     {
        let (private, declared, key) = keys([17u8; 32]);
        let end = |node: Value, asset: &str, decimals: u8| serde_json::json!({ "node": node, "asset": { "id": asset, "decimals": decimals } });
        let swap_ends = |t: &mut Value| {
            let from = t["from"].clone();
            t["from"] = t["to"].clone();
            t["to"] = from;
        };

        // A conversion at `acme-ledger`, from COP to USDC.
        let conversion = with_terms(
            "result-conversion-dry-run-quote",
            "acme-co",
            end("acme-ledger".into(), "COP", 2),
            end("acme-ledger".into(), "USDC", 6),
        );
        let asked = asking("operation-conversion-as-a-dry-run", &[]);
        let signed = sign_as(&conversion, "acme-co", &asked, &private, &declared).expect("signed");
        assert!(verifies(&signed["statement"]["outcome"]["terms"], &key));
        let reversed = changed(&conversion, |t| {
            let from = t["from"]["asset"].clone();
            t["from"]["asset"] = t["to"]["asset"].clone();
            t["to"]["asset"] = from;
        });
        let elsewhere = changed(&conversion, |t| t["to"]["node"] = "other-ledger".into());
        for (case, refused) in [
            ("the rate reversed", reversed),
            ("at another node", elsewhere),
        ] {
            assert_eq!(
                sign_as(&refused, "acme-co", &asked, &private, &declared).err(),
                Some(SigningError::NestedNotAsked),
                "conversion: {case}"
            );
        }

        // A transfer from `acme-ledger` to `polygon`, in USDC.
        let transfer = with_terms(
            "result-transfer-dry-run-quote",
            "bridge-pay",
            end("acme-ledger".into(), "USDC", 6),
            end("polygon".into(), "USDC", 6),
        );
        let mut asked: Value = fixture("operation-transfer-between-two-nodes")["statement"].clone();
        asked["dry_run"] = true.into();
        asked["transaction"] = "tx-010".into();
        let asked: Operation = serde_json::from_value(asked).expect("a transfer");
        let signed = sign_as(&transfer, "bridge-pay", &asked, &private, &declared).expect("signed");
        assert!(verifies(&signed["statement"]["outcome"]["terms"], &key));
        assert_eq!(
            sign_as(
                &changed(&transfer, swap_ends),
                "bridge-pay",
                &asked,
                &private,
                &declared
            )
            .err(),
            Some(SigningError::NestedNotAsked),
            "transfer: its ends swapped"
        );

        // A deposit from `bank-ledger` to `co-ach`, in COP: the withdrawal's answer, made a deposit's.
        let mut deposit = with_terms(
            "result-withdrawal-handing-back-its-terms",
            "acme-co",
            end("bank-ledger".into(), "COP", 2),
            end(serde_json::json!({ "network": "co-ach" }), "COP", 2),
        );
        deposit["statement"]["kind"] = "deposit".into();
        let asked: Operation = serde_json::from_value(serde_json::json!({
            "kind": "deposit", "transaction": "tx-012", "provider": "acme-co",
            "origin": "bank-ledger", "destination": { "network": "co-ach" }, "asset": "COP",
            "amount": "25000000", "dry_run": true, "inputs": {},
        }))
        .expect("a deposit");
        let signed = sign_as(&deposit, "acme-co", &asked, &private, &declared).expect("signed");
        assert!(verifies(&signed["statement"]["outcome"]["terms"], &key));
        assert_eq!(
            sign_as(
                &changed(&deposit, swap_ends),
                "acme-co",
                &asked,
                &private,
                &declared
            )
            .err(),
            Some(SigningError::NestedNotAsked),
            "deposit: its ends swapped"
        );
    }

    /// The hold a reserve nests in its answer to a hold request is signed with its key only where it is its hold of the
    /// quote asked: the request's transaction, record, candidate and whom it is for, valid no later than asked. A hold of
    /// another quote, past the validity asked, or in answer to another question, is not signed (`T-020-56`,
    /// `REQ-QTE-15`, `THR-8`).
    // verifies: REQ-QTE-15, THR-8
    #[test]
    fn hold_nested_in_a_hold_answer_is_signed_only_for_the_quote_asked() {
        let (private, declared, key) = keys([16u8; 32]);
        let answered = fixture("result-hold-held");
        let asked = asking("operation-hold-of-a-chosen-quote", &[]);
        let signed =
            sign_as(&answered, "reserve-bank-co", &asked, &private, &declared).expect("signed");
        assert!(verifies(&signed, &key));
        assert!(verifies(&signed["statement"]["outcome"]["hold"], &key));
        // Held for less long than asked: signed.
        let mut shorter = answered.clone();
        shorter["statement"]["outcome"]["hold"]["statement"]["valid_until"] =
            "2026-11-06T15:01:00Z".into();
        assert!(sign_as(&shorter, "reserve-bank-co", &asked, &private, &declared).is_ok());

        // A hold of another quote than asked, or for longer.
        let mut cases: Vec<(&str, Value)> = vec![
            ("transaction", "tx-hold-2".into()),
            ("candidate", "c2".into()),
            (
                "for",
                serde_json::json!({ "kind": "order", "order": "ord-2" }),
            ),
            ("valid_until", "2026-11-06T15:01:56Z".into()),
        ];
        let mut record = answered["statement"]["outcome"]["hold"]["statement"]["record"].clone();
        record["id"] = "dr-0005".into();
        cases.push(("record", record));
        // The answer stays under the request's transaction; only the hold it nests differs.
        for (field, value) in cases {
            let mut other = answered.clone();
            other["statement"]["outcome"]["hold"]["statement"][field] = value;
            assert_eq!(
                sign_as(&other, "reserve-bank-co", &asked, &private, &declared).err(),
                Some(SigningError::NestedNotAsked),
                "{field}"
            );
        }
        // A request made to another reserve.
        let elsewhere = asking(
            "operation-hold-of-a-chosen-quote",
            &[("provider", "another-reserve")],
        );
        assert_eq!(
            sign_as(
                &answered,
                "reserve-bank-co",
                &elsewhere,
                &private,
                &declared
            )
            .err(),
            Some(SigningError::NestedNotAsked)
        );
        // In answer to another question: a dry run.
        assert_eq!(
            sign_as(
                &answered,
                "reserve-bank-co",
                &dry_run_asked(&[("provider", "reserve-bank-co".into())]),
                &private,
                &declared
            )
            .err(),
            Some(SigningError::NestedNotAsked)
        );
        // Nested in a status answer, about whatever operation: never a result of the question asked about.
        let deployment = deployment(&answered);
        let carried = asking("operation-withdrawal-from-a-bank-reserve", &[]);
        let question = asking("operation-status-query", &[]);
        let asked = Asked::of(&Answering {
            provider: "reserve-bank-co",
            deployment: &deployment,
            protocol_version: "2.0.1",
            operation: &question,
            about: Some(crate::proxy::AboutAsked {
                operation: &carried,
                held: None,
            }),
        })
        .expect("asked");
        let mut holder = answered["statement"]["outcome"]["hold"].clone();
        assert_eq!(
            super::sign_nested(&mut holder, &asked, &key, &declared),
            Err(SigningError::NestedNotAsked)
        );
    }

    /// A provider's record of a private node, nested in its answer, is signed with its key only in answer to the question
    /// of that node's record, put to it: not for another node, nor for another provider's question, nor in answer to any
    /// other kind of question, a status question included (`REQ-ORD-41`, `T-020-58`).
    // verifies: REQ-ORD-41
    #[test]
    fn record_nested_in_an_inner_segments_answer_is_signed_only_for_the_node_asked() {
        let (private, declared, key) = keys([5u8; 32]);
        let answered = fixture("result-inner-segments-query-answered-with-the-providers-record");
        let asked = asking("operation-inner-segments-query", &[]);
        let maker = "acme-internal-rail";
        let signed = sign_as(&answered, maker, &asked, &private, &declared).expect("signed");
        assert!(verifies(&signed, &key));
        assert!(verifies(&signed["statement"]["outcome"]["record"], &key));
        // A question of another node, or under another transaction, than the answer names.
        for (field, value) in [("node", "acme-card-book"), ("transaction", "tx-inner-2")] {
            let other = asking("operation-inner-segments-query", &[(field, value)]);
            assert_eq!(
                sign_as(&answered, maker, &other, &private, &declared).err(),
                Some(SigningError::AnswerOfAnotherQuestion),
                "{field}"
            );
        }
        // A question put to another provider than the one that nests its record.
        let elsewhere = asking(
            "operation-inner-segments-query",
            &[("provider", "other-co")],
        );
        assert_eq!(
            sign_as(&answered, maker, &elsewhere, &private, &declared).err(),
            Some(SigningError::NestedNotAsked)
        );
        // The same record in answer to another kind of question, or to a status question about it.
        let withdrawal = asking("operation-withdrawal-from-a-bank-reserve", &[]);
        assert_eq!(
            sign_as(&answered, maker, &withdrawal, &private, &declared).err(),
            Some(SigningError::NestedNotAsked)
        );
        let status = asking("operation-status-query", &[]);
        let mut ours = answered.clone();
        ours["maker"] = "acme-co".into();
        ours["statement"]["outcome"]["record"]["maker"] = "acme-co".into();
        assert_eq!(
            sign_about(&ours, &status, &withdrawal, None, &private, &declared).err(),
            Some(SigningError::NestedNotAsked)
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
