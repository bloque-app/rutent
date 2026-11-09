//! The Ed25519 verifier over the contract's canonical bytes (`T-012-02`; `REQ-PRT-1`, `THR-4`, `THR-8`).

mod common;

use common::*;
use ed25519_dalek::SigningKey;
use rutent_contracts::provider_role::PublicKey;
use rutent_prov::catalogue::{
    SignatureError, check_public_key, decode_signature, verify_ed25519, verify_envelope,
    verify_text,
};

/// The field prime 2^255 - 19, little-endian.
fn prime() -> [u8; 32] {
    let mut p = [0xff; 32];
    p[0] = 0xed;
    p[31] = 0x7f;
    p
}

/// `p + n` with the sign bit as given: another encoding of the point whose `y` is `n`, when `n` is below 19.
fn above_prime(n: u8, sign: bool) -> [u8; 32] {
    let mut bytes = prime();
    bytes[0] = bytes[0].wrapping_add(n);
    if sign {
        bytes[31] |= 0x80;
    }
    bytes
}

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn array<const N: usize>(text: &str) -> [u8; N] {
    hex(text).try_into().expect("the right length")
}

/// RFC 8032 section 7.1, tests 1 to 3: secret key, public key, message, signature.
const RFC_8032: [(&str, &str, &str, &str); 3] = [
    (
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "",
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
         5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    ),
    (
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        "72",
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da\
         085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    ),
    (
        "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        "af82",
        "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac\
         18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    ),
];

// verifies: REQ-PRT-1
#[test]
fn rfc_8032_test_vectors_verify() {
    for (secret, public, message, signature) in RFC_8032 {
        let (public, message, signature): ([u8; 32], _, [u8; 64]) =
            (array(public), hex(message), array(signature));
        assert_eq!(verify_ed25519(&public, &signature, &message), Ok(()));
        // The public key is the one the secret key gives, and the secret key signs the same bytes.
        let signing = SigningKey::from_bytes(&array(secret));
        assert_eq!(signing.verifying_key().to_bytes(), public);
        use ed25519_dalek::Signer;
        assert_eq!(signing.sign(&message).to_bytes(), signature);
    }
}

// verifies: REQ-PRT-1
#[test]
fn rfc_8032_vector_with_another_message_does_not_verify() {
    let (_, public, _, signature) = RFC_8032[1];
    assert_eq!(
        verify_ed25519(&array(public), &array(signature), &[0x73]),
        Err(SignatureError::DoesNotVerify)
    );
}

// verifies: REQ-PRT-1
#[test]
fn signature_in_another_key_does_not_verify() {
    let (_, _, message, signature) = RFC_8032[1];
    let (_, other, _, _) = RFC_8032[2];
    assert_eq!(
        verify_ed25519(&array(other), &array(signature), &hex(message)),
        Err(SignatureError::DoesNotVerify)
    );
}

// verifies: REQ-PRT-1, THR-8
#[test]
fn signature_text_in_a_non_canonical_base64_is_refused() {
    let (_, public, message, signature) = RFC_8032[0];
    let canonical = base64url(&hex(signature));
    assert_eq!(canonical.len(), 86);
    assert_eq!(
        decode_signature(&canonical).map(|s| s.to_vec()),
        Some(hex(signature))
    );
    let key = PublicKey::new(base64url(&hex(public))).unwrap();
    assert_eq!(verify_text(&key, &canonical, &hex(message)), Ok(()));

    let last = canonical.chars().last().unwrap();
    assert!("AQgw".contains(last), "a canonical signature ends in AQgw");
    let stem = &canonical[..85];
    let mut refused = vec![
        // Nonzero unused bits: another text for the same 64 bytes.
        format!("{stem}B"),
        format!("{stem}R"),
        format!("{stem}h"),
        // Padding, and the standard alphabet.
        format!("{canonical}=="),
        format!("{}=", &canonical[..85]),
        canonical.replacen(|c| c != '-' && c != '_', "+", 1),
        canonical.replacen(|c| c != '-' && c != '_', "/", 1),
        // Not 86 characters.
        canonical[..85].to_owned(),
        format!("{canonical}A"),
        String::new(),
    ];
    refused.push(canonical.replace('-', "+").replace('_', "/"));
    for text in refused {
        assert_eq!(decode_signature(&text), None, "{text}");
        assert_eq!(
            verify_text(&key, &text, &hex(message)),
            Err(SignatureError::SignatureEncoding),
            "{text}"
        );
    }
}

// verifies: REQ-PRT-1, THR-8
#[test]
fn public_key_text_in_a_non_canonical_base64_is_refused() {
    let (_, public, _, _) = RFC_8032[0];
    let canonical = base64url(&hex(public));
    assert!(PublicKey::new(canonical.clone()).is_some());
    let stem = &canonical[..42];
    for text in [
        format!("{stem}B"),
        format!("{canonical}="),
        canonical.replacen(|c| c != '-' && c != '_', "+", 1),
        canonical[..42].to_owned(),
    ] {
        assert!(PublicKey::new(text.clone()).is_none(), "{text}");
    }
}

// verifies: REQ-PRT-1
#[test]
fn public_key_with_y_at_or_above_the_prime_is_refused_as_non_canonical() {
    for n in 0..19u8 {
        for sign in [false, true] {
            let bytes = above_prime(n, sign);
            assert_eq!(
                check_public_key(&bytes).err(),
                Some(SignatureError::KeyNotCanonical),
                "p + {n}, sign {sign}"
            );
        }
    }
    // One below the prime is canonical, and a point or not for its own reasons.
    let mut below = prime();
    below[0] -= 1;
    assert_ne!(
        check_public_key(&below).err(),
        Some(SignatureError::KeyNotCanonical)
    );
}

/// Six of the eight points of small order, as RFC 8032 section 5.1.3 writes them.
const SMALL_ORDER: [&str; 6] = [
    // The identity, and the point of order two.
    "0100000000000000000000000000000000000000000000000000000000000000",
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    // The two points of order four.
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000080",
    // Two of the four of order eight.
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
];

// verifies: REQ-PRT-1
#[test]
fn small_order_public_key_is_refused() {
    for point in SMALL_ORDER {
        let key: [u8; 32] = array(point);
        assert_eq!(
            check_public_key(&key).err(),
            Some(SignatureError::KeySmallOrder),
            "{point}"
        );
        // The signature that verifies for every message under such a key when only the equation is checked: R
        // the identity and S zero. It is refused, for the key.
        let mut forged = [0u8; 64];
        forged[0] = 1;
        assert_eq!(
            verify_ed25519(&key, &forged, b"any message"),
            Err(SignatureError::KeySmallOrder),
            "{point}"
        );
    }
}

// verifies: REQ-PRT-1
#[test]
fn signature_with_a_scalar_at_or_above_the_group_order_is_refused() {
    let (_, public, message, signature) = RFC_8032[0];
    let signature: [u8; 64] = array(signature);
    // S + L is the same point equation with another encoding: the malleable twin.
    let order: [u8; 32] =
        array("edd3f55c1a631258d69cf7a2def9de140000000000000000000000000000001 0");
    let mut twin = signature;
    let mut carry = 0u16;
    for i in 0..32 {
        let sum = u16::from(signature[32 + i]) + u16::from(order[i]) + carry;
        twin[32 + i] = sum as u8;
        carry = sum >> 8;
    }
    assert_eq!(carry, 0);
    assert_eq!(
        verify_ed25519(&array(public), &twin, &hex(message)),
        Err(SignatureError::SignatureNotCanonical)
    );
    assert_eq!(
        verify_ed25519(&array(public), &signature, &hex(message)),
        Ok(())
    );
}

// verifies: REQ-PRT-1
#[test]
fn signature_with_a_tampered_commitment_is_refused() {
    let (_, public, message, signature) = RFC_8032[2];
    let mut signature: [u8; 64] = array(signature);
    signature[0] ^= 1;
    assert_eq!(
        verify_ed25519(&array(public), &signature, &hex(message)),
        Err(SignatureError::DoesNotVerify)
    );
}

fn statement() -> serde_json::Value {
    serde_json::json!({
        "deployment": {"name": "rutent-test", "operator_key": deployment().operator_key.as_str()},
        "protocol_version": "2.0.1",
        "shape": "urn:rutent:provider-role:result",
        "maker": "acme-co",
        "made_at": "2026-11-06T15:04:05Z",
        "signature": {"scheme": "ed25519", "key": "", "value": ""},
        "statement": {"b": [1, 2.50, "\u{e9}"], "a": 10}
    })
}

// verifies: REQ-PRT-1, THR-8
#[test]
fn statement_verifies_over_its_canonical_bytes_whatever_its_layout() {
    let key = signing_key("acme-co", 2);
    let sent = sign_json(statement(), &key);
    let verifies = |bytes: &[u8]| {
        let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        verify_envelope(
            &public_key(&key),
            value["signature"]["value"].as_str().unwrap(),
            bytes,
        )
    };
    assert_eq!(verifies(&sent), Ok(()));

    // The same statement with another layout of the same JSON: members reordered, spaces added, a number
    // written another way. Its canonical form is the same, so the signature still verifies.
    let value: serde_json::Value = serde_json::from_slice(&sent).unwrap();
    let relaid = format!(
        "{{ \"statement\" : {{ \"a\" : 1e1, \"b\" : [1, 2.5, \"\\u00e9\"] }},\n \"signature\" : {},\n \"maker\" : \"acme-co\", \"made_at\" : \"2026-11-06T15:04:05Z\", \"shape\" : \"urn:rutent:provider-role:result\", \"protocol_version\" : \"2.0.1\", \"deployment\" : {} }}",
        value["signature"], value["deployment"]
    );
    assert_eq!(verifies(relaid.as_bytes()), Ok(()));

    // A different statement does not.
    let mut other = value.clone();
    other["statement"]["a"] = 11.into();
    assert_eq!(
        verifies(&serde_json::to_vec(&other).unwrap()),
        Err(SignatureError::DoesNotVerify)
    );
    // Neither does a field the verifier's types never model, added after signing.
    let mut extra = value;
    extra["unmodelled"] = true.into();
    assert_eq!(
        verifies(&serde_json::to_vec(&extra).unwrap()),
        Err(SignatureError::DoesNotVerify)
    );
}

// verifies: REQ-PRT-1, THR-8
#[test]
fn statement_with_a_member_named_twice_has_no_signing_input() {
    let key = signing_key("acme-co", 2);
    let sent = String::from_utf8(sign_json(statement(), &key)).unwrap();
    let doubled = sent.replacen(
        "\"maker\":\"acme-co\"",
        "\"maker\":\"acme-co\",\"maker\":\"evil\"",
        1,
    );
    assert!(matches!(
        verify_envelope(&public_key(&key), &base64url(&[0; 64]), doubled.as_bytes()),
        Err(SignatureError::NoSigningInput(_))
    ));
}

// verifies: REQ-PRT-1
#[test]
fn public_key_that_is_no_point_of_the_curve_is_refused() {
    // About half the values of y below the prime have no x on the curve.
    let refused = (2u8..=64)
        .filter(|y| {
            let mut bytes = [0u8; 32];
            bytes[0] = *y;
            check_public_key(&bytes).err() == Some(SignatureError::KeyNotAPoint)
        })
        .count();
    assert!(refused > 10, "{refused} of 63");
}
