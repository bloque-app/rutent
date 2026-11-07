//! Checks of the provider-role contract's string encodings, as its schemas state them.

const BASE64URL: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn canonical_base64url(value: &str, length: usize, last: &str) -> bool {
    value.len() == length
        && value.chars().all(|c| BASE64URL.contains(c))
        && value.chars().last().is_some_and(|c| last.contains(c))
}

/// Whether `value` is a 32-byte Ed25519 public key in canonical base64url without padding.
pub fn is_canonical_public_key(value: &str) -> bool {
    canonical_base64url(value, 43, "AEIMQUYcgkosw048")
}

/// Whether `value` is a 64-byte signature in canonical base64url without padding.
pub fn is_canonical_signature(value: &str) -> bool {
    canonical_base64url(value, 86, "AQgw")
}

/// Whether `value` is `major.minor.patch`, each a number with no leading zero.
pub fn is_protocol_version(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

/// Whether `value` is an RFC 3339 UTC time, to the second or finer, ending in `Z`.
pub fn is_utc_timestamp(value: &str) -> bool {
    let Some(body) = value.strip_suffix('Z') else {
        return false;
    };
    let (main, fraction) = match body.split_once('.') {
        Some((main, fraction)) => (main, Some(fraction)),
        None => (body, None),
    };
    if fraction
        .is_some_and(|f| !(1..=9).contains(&f.len()) || !f.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let bytes = main.as_bytes();
    if bytes.len() != 19 {
        return false;
    }
    let digits_at = |range: std::ops::Range<usize>| -> Option<u32> {
        main.get(range)
            .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
            .and_then(|s| s.parse().ok())
    };
    let separators = bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':';
    match (
        digits_at(0..4),
        digits_at(5..7),
        digits_at(8..10),
        digits_at(11..13),
        digits_at(14..16),
        digits_at(17..19),
    ) {
        (Some(_), Some(month), Some(day), Some(hour), Some(minute), Some(second)) => {
            separators
                && (1..=12).contains(&month)
                && (1..=31).contains(&day)
                && hour <= 23
                && minute <= 59
                && second <= 59
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: REQ-PRT-1
    #[test]
    fn encodings_are_checked_as_the_schemas_state_them() {
        assert!(is_canonical_public_key(&format!("{}A", "x".repeat(42))));
        assert!(!is_canonical_public_key(&format!("{}B", "x".repeat(42))));
        assert!(is_canonical_signature(&format!("{}Q", "x".repeat(85))));
        assert!(!is_canonical_signature(&format!("{}E", "x".repeat(85))));
        assert!(is_protocol_version("0.3.0"));
        assert!(!is_protocol_version("v3"));
        assert!(!is_protocol_version("01.2.3"));
        assert!(is_utc_timestamp("2026-11-06T15:04:05Z"));
        assert!(is_utc_timestamp("2026-11-06T15:04:05.123Z"));
        assert!(!is_utc_timestamp("2026-11-06T10:04:05-05:00"));
        assert!(!is_utc_timestamp("2026-13-06T15:04:05Z"));
    }
}
