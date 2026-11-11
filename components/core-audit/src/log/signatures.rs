//! The log's signature check ([`Signatures`]): strict Ed25519 over provider-role's signing input of a statement as
//! received, by the catalogue's verifier (`rutent_prov::catalogue::verify_envelope`), the one admission uses, so the
//! log carries no curve arithmetic of its own.

use rutent_contracts::core_internal::calls::Signatures;
use rutent_contracts::provider_role::PublicKey;
use rutent_prov::catalogue::verify_envelope;

/// Strict Ed25519 (RFC 8032 §5.1.7, with the catalogue's checks on the key and on `S`) over the signing input of
/// the bytes as received, against the key the envelope names. Whether that key signs for the maker is
/// [`StatementKeys`](rutent_contracts::core_internal::calls::StatementKeys)'s question, not this one's.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ed25519Strict;

impl Signatures for Ed25519Strict {
    fn verifies(&self, received: &[u8]) -> bool {
        let Ok(envelope) = serde_json::from_slice::<serde_json::Value>(received) else {
            return false;
        };
        let signature = &envelope["signature"];
        let (Some("ed25519"), Some(key), Some(value)) = (
            signature["scheme"].as_str(),
            signature["key"].as_str().and_then(PublicKey::new),
            signature["value"].as_str(),
        ) else {
            return false;
        };
        verify_envelope(&key, value, received).is_ok()
    }
}
