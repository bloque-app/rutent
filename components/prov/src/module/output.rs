//! What the platform makes of a module's output (`REQ-MDL-12`, `REQ-MDL-9`, `B-19`): the output is untrusted, so it
//! is judged against the provider-role contract's own result schema, patterns and lengths included, before the
//! platform types it, signs it or passes it on. Shape alone isn't enough: a string that parses as a result's field
//! can still be one its schema forbids (an amount with a sign, a reference of a megabyte).
//!
//! A refusal names a reason from the contract's enum and carries nothing the guest wrote. The same holds for what
//! the platform keeps of a guest's trap: [`trap_reason`] gives wasmtime's own words for the trap's kind, never the
//! guest's error text or the names in its binary, which a log or a report would echo to whoever reads it.

use std::sync::OnceLock;

use jsonschema::Validator;
use rutent_contracts::module_interface::PlatformRefusal;
use rutent_contracts::provider_role::ResultBody;
use serde_json::{Value, json};

/// The provider-role contract's schemas, which the result schema refers to by their `$id`. The contract crate
/// carries no schema text (it builds for `no_std`), so the files are read from `contracts/provider-role/schemas/`
/// at build time.
const SCHEMAS: [&str; 9] = [
    include_str!("../../../../contracts/provider-role/schemas/advertisement.schema.json"),
    include_str!("../../../../contracts/provider-role/schemas/common.schema.json"),
    include_str!("../../../../contracts/provider-role/schemas/declarations.schema.json"),
    include_str!("../../../../contracts/provider-role/schemas/judgement.schema.json"),
    include_str!("../../../../contracts/provider-role/schemas/operation.schema.json"),
    include_str!("../../../../contracts/provider-role/schemas/reserve-declaration.schema.json"),
    include_str!("../../../../contracts/provider-role/schemas/result.schema.json"),
    include_str!("../../../../contracts/provider-role/schemas/reversal-confirmation.schema.json"),
    include_str!("../../../../contracts/provider-role/schemas/statement-envelope.schema.json"),
];

/// The result body's definition in the result schema: the platform's own envelope is not the module's to judge.
const RESULT_BODY: &str = "urn:rutent:provider-role:result#/$defs/result";

/// The validator of a result body, built once. `None` if the embedded schemas don't build one, which fails closed:
/// no output is passed on then.
fn validator() -> Option<&'static Validator> {
    static VALIDATOR: OnceLock<Option<Validator>> = OnceLock::new();
    VALIDATOR
        .get_or_init(|| {
            let mut registry = jsonschema::Registry::new();
            for text in SCHEMAS {
                let schema: Value = serde_json::from_str(text).ok()?;
                let id = schema["$id"].as_str()?.to_owned();
                registry = registry.add(id, schema).ok()?;
            }
            let registry = registry.prepare().ok()?;
            jsonschema::options()
                .offline()
                .with_registry(&registry)
                .build(&json!({ "$ref": RESULT_BODY }))
                .ok()
        })
        .as_ref()
}

/// Whether the embedded schemas build the validator [`check`] uses.
pub fn schemas_load() -> bool {
    validator().is_some()
}

/// Judges one output, as the module emitted it, against `limit` bytes and the result schema, and types it.
///
/// - Longer than `limit`: [`PlatformRefusal::OversizedOutput`], before anything reads it.
/// - Not JSON, or JSON the result schema refuses (a shape, an unknown field, a pattern, a length):
///   [`PlatformRefusal::MalformedOutput`].
///
/// What is typed is the very value that was judged, never a second reading of the bytes.
pub fn check(output: &[u8], limit: usize) -> Result<ResultBody, PlatformRefusal> {
    if output.len() > limit {
        return Err(PlatformRefusal::OversizedOutput);
    }
    let value: Value =
        serde_json::from_slice(output).map_err(|_| PlatformRefusal::MalformedOutput)?;
    let validator = validator().ok_or(PlatformRefusal::MalformedOutput)?;
    if !validator.is_valid(&value) {
        return Err(PlatformRefusal::MalformedOutput);
    }
    serde_json::from_value(value).map_err(|_| PlatformRefusal::MalformedOutput)
}

/// Why a part's call stopped, as a platform may keep it: wasmtime's words for the kind of trap, or a fixed
/// sentence for each other kind of error it knows. A trap's backtrace, a guest's error string and the names in a binary are the guest's to choose, so none
/// of them is kept.
pub fn trap_reason(error: &wasmtime::Error) -> String {
    if let Some(trap) = error.downcast_ref::<wasmtime::Trap>() {
        return trap.to_string();
    }
    if error.downcast_ref::<wasmtime::OutOfMemory>().is_some() {
        return "resource limit: out of memory".to_owned();
    }
    "the part failed".to_owned()
}
