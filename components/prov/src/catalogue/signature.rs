//! The Ed25519 verifier (`REQ-PRT-1`, `THR-4`, `THR-8`): RFC 8032 strict verification over the contract's
//! canonical bytes.
//!
//! Admission checks a provider's signed statements with it, which makes it a `DEC-20` path ("module admission and
//! signature checking"); `F-013` reuses it for release assertions. It has two layers:
//!
//! - [`verify_ed25519`], over raw bytes: a key, a signature and a message. It refuses what `ed25519-dalek`'s
//!   strict verification leaves to the caller, each with its own [`SignatureError`]: a key whose `y` coordinate is
//!   at or above the field prime 2^255 - 19 ([`SignatureError::KeyNotCanonical`]), a key that is no curve point,
//!   a small-order key, a signature whose scalar `S` is at or above the group order `L` (malleable), and then
//!   `verify_strict`, which refuses a small-order `R` and compares `R` as the bytes it was sent as.
//! - [`verify_envelope`], over a statement as received: the bytes signed are
//!   [`signing_input`](rutent_contracts::provider_role::signing_input) of the received text (RFC 8785, only
//!   `signature.value` removed), never a re-serialisation of a parsed type.
//!
//! A signature's text is decoded by [`decode_signature`], which takes only the one canonical form: 86 characters
//! of the base64url alphabet, no padding, and a last character carrying no unused bits.

use ed25519_dalek::{Signature, VerifyingKey};
use rutent_contracts::provider_role::{CanonicalError, PublicKey, signing_input};

/// The field prime 2^255 - 19, little-endian.
const FIELD_PRIME: [u8; 32] = {
    let mut p = [0xff; 32];
    p[0] = 0xed;
    p[31] = 0x7f;
    p
};

/// The order `L` of the group, 2^252 + 27742317777372353535851937790883648493, little-endian.
const GROUP_ORDER: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

/// Why a signature isn't accepted. Each check has its own variant, so a test can say which one fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// The key's text isn't in its one canonical encoding.
    KeyEncoding,
    /// The key's `y` coordinate is at or above 2^255 - 19: a second encoding of a point (RFC 8032 section 5.1.3).
    KeyNotCanonical,
    /// The key decodes to no point of the curve.
    KeyNotAPoint,
    /// The key is a point of small order, which verifies signatures for many messages.
    KeySmallOrder,
    /// The signature's text isn't 86 characters of canonical unpadded base64url.
    SignatureEncoding,
    /// The signature's scalar `S` is at or above the group order, so another encoding of it would verify too.
    SignatureNotCanonical,
    /// The signature isn't the key's over the message: a wrong key or message, a small-order or non-canonical `R`.
    DoesNotVerify,
    /// The statement has no signing input under the contract's rule.
    NoSigningInput(CanonicalError),
}

/// Checks that 32 bytes are a key to accept signatures from, and returns it: the canonical encoding of a point of
/// the curve that isn't of small order. `ed25519-dalek` decompresses a `y` at or above the prime without a word,
/// so the canonical check is made here, first.
pub fn check_public_key(bytes: &[u8; 32]) -> Result<VerifyingKey, SignatureError> {
    let mut y = *bytes;
    y[31] &= 0x7f;
    if !less_than(&y, &FIELD_PRIME) {
        return Err(SignatureError::KeyNotCanonical);
    }
    let key = VerifyingKey::from_bytes(bytes).map_err(|_| SignatureError::KeyNotAPoint)?;
    if key.is_weak() {
        return Err(SignatureError::KeySmallOrder);
    }
    Ok(key)
}

/// Whether the little-endian number `a` is below `b`.
fn less_than(a: &[u8; 32], b: &[u8; 32]) -> bool {
    for i in (0..32).rev() {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
    }
    false
}

/// Verifies `signature` by `key` over `message`, strictly (RFC 8032 section 5.1.7 with the checks above).
pub fn verify_ed25519(
    key: &[u8; 32],
    signature: &[u8; 64],
    message: &[u8],
) -> Result<(), SignatureError> {
    let key = check_public_key(key)?;
    let mut s = [0u8; 32];
    s.copy_from_slice(&signature[32..]);
    if !less_than(&s, &GROUP_ORDER) {
        return Err(SignatureError::SignatureNotCanonical);
    }
    key.verify_strict(message, &Signature::from_bytes(signature))
        .map_err(|_| SignatureError::DoesNotVerify)
}

/// The 64 bytes a signature's text writes, if it is 86 characters of base64url without padding whose four unused
/// bits are zero (its last character is one of `AQgw`), the one canonical encoding.
pub fn decode_signature(text: &str) -> Option<[u8; 64]> {
    let digit = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    };
    let chars = text.as_bytes();
    if chars.len() != 86 {
        return None;
    }
    let mut bytes = [0u8; 64];
    let (mut acc, mut held, mut out) = (0u32, 0u32, 0usize);
    for &c in chars {
        acc = (acc << 6) | digit(c)?;
        held += 6;
        if held >= 8 {
            held -= 8;
            bytes[out] = (acc >> held) as u8;
            out += 1;
            acc &= (1 << held) - 1;
        }
    }
    // 86 characters carry 516 bits: 64 bytes, and four left over that must be zero.
    (out == 64 && held == 4 && acc == 0).then_some(bytes)
}

/// Verifies a signature written as text by a key, over a message.
pub fn verify_text(key: &PublicKey, signature: &str, message: &[u8]) -> Result<(), SignatureError> {
    let signature = decode_signature(signature).ok_or(SignatureError::SignatureEncoding)?;
    verify_ed25519(&key.bytes(), &signature, message)
}

/// Verifies the signature `value` of a statement received as `received`, over its signing input.
pub fn verify_envelope(
    key: &PublicKey,
    value: &str,
    received: &[u8],
) -> Result<(), SignatureError> {
    let input = signing_input(received).map_err(SignatureError::NoSigningInput)?;
    verify_text(key, value, &input)
}
