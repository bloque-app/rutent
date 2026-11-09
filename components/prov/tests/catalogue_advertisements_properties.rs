//! Property tests of the verifier and the key history (`DEC-18`): seeded, so a failure names the seed that
//! replays it.

mod common;

use common::*;
use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::provider_role::{
    KeyPurpose, PublicKey, SignatureScheme, SigningKey as ListedKey,
};
use rutent_prov::catalogue::{Catalogue, Instant, decode_signature, tuples_of, verify_ed25519};

fn random_bytes<const N: usize>(rng: &mut Rng) -> [u8; N] {
    let mut bytes = [0u8; N];
    for b in &mut bytes {
        *b = rng.next() as u8;
    }
    bytes
}

// verifies: REQ-PRT-1
#[test]
fn signature_verifies_for_its_message_and_for_no_other_input() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let key = SigningKey::from_bytes(&random_bytes(&mut rng));
        let public = key.verifying_key().to_bytes();
        let mut message = vec![0u8; rng.below(80) as usize];
        message.iter_mut().for_each(|b| *b = rng.next() as u8);
        let signature = key.sign(&message).to_bytes();
        assert_eq!(
            verify_ed25519(&public, &signature, &message),
            Ok(()),
            "seed {seed}"
        );

        // Flip one bit of the message, the signature or the key: it never verifies.
        let bit = rng.below(8) as u8;
        let mut tampered = (message.clone(), signature, public);
        match rng.below(3) {
            0 if !message.is_empty() => {
                let at = rng.below(message.len() as u64) as usize;
                tampered.0[at] ^= 1 << bit;
            }
            1 => tampered.1[rng.below(64) as usize] ^= 1 << bit,
            _ => tampered.2[rng.below(32) as usize] ^= 1 << bit,
        }
        if tampered != (message, signature, public) {
            assert!(
                verify_ed25519(&tampered.2, &tampered.1, &tampered.0).is_err(),
                "seed {seed}"
            );
        }
    }
}

// verifies: REQ-PRT-1, THR-8
#[test]
fn signature_text_has_exactly_one_encoding() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let bytes: [u8; 64] = random_bytes(&mut rng);
        let text = base64url(&bytes);
        assert_eq!(decode_signature(&text), Some(bytes), "seed {seed}");

        // Another last character writes the same bytes when read leniently, and is refused.
        let alias = ['B', 'C', 'h', 'z', '-', '_', '7'][rng.below(7) as usize];
        assert_eq!(
            decode_signature(&format!("{}{alias}", &text[..85])),
            None,
            "seed {seed}"
        );
        // Padding, and a wrong length, are refused.
        assert_eq!(decode_signature(&format!("{text}=")), None, "seed {seed}");
        assert_eq!(
            decode_signature(&text[..rng.below(86) as usize]),
            None,
            "seed {seed}"
        );

        // Any text of 86 alphabet characters that is accepted is the encoding of what it decodes to.
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let random: String = (0..86)
            .map(|_| alphabet[rng.below(64) as usize] as char)
            .collect();
        if let Some(decoded) = decode_signature(&random) {
            assert_eq!(base64url(&decoded), random, "seed {seed}");
        }
    }
}

fn pool(n: u64) -> SigningKey {
    signing_key("pool", n as u8)
}

fn listed(key: &SigningKey, purpose: KeyPurpose) -> ListedKey {
    ListedKey {
        scheme: SignatureScheme::Ed25519,
        public_key: public_key(key),
        purpose,
    }
}

const NOW: Instant = Instant(2_000);

// verifies: REQ-CAT-2, THR-4
#[test]
fn key_history_holds_its_rules_under_any_sequence_of_advertisements() {
    let (mut accepted, mut refused) = (0, 0);
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let providers = [
            ("bridge-pay", ramp_advertisement()),
            ("dex-pool", exchange_advertisement()),
        ];
        let mut catalogue = Catalogue::new();
        catalogue.set_context(deployment(), rutent_contracts::provider_role::VERSION);
        for (i, (name, ad)) in providers.iter().enumerate() {
            let id = format!("g-{i}");
            catalogue
                .grant(grant(&id, name, tuples_of(ad), 100_000))
                .unwrap();
            catalogue
                .trust_advertising_key(&id, public_key(&pool(0)))
                .unwrap();
        }
        for step in 0..30 {
            let (name, ad) = &providers[rng.below(2) as usize];
            let last = catalogue.accepted_advertisement(name).cloned();
            let signer = match &last {
                Some(last) if !rng.chance(5) => {
                    let advertising: Vec<&ListedKey> = last
                        .statement
                        .keys
                        .iter()
                        .filter(|k| k.purpose == KeyPurpose::Advertising)
                        .collect();
                    let chosen = advertising[rng.below(advertising.len() as u64) as usize];
                    (0..6)
                        .map(pool)
                        .find(|k| public_key(k) == chosen.public_key)
                        .unwrap()
                }
                _ if rng.chance(4) => pool(rng.below(6)),
                _ => pool(0),
            };
            let mut keys = vec![listed(&signer, KeyPurpose::Advertising)];
            for n in 0..6 {
                match rng.below(4) {
                    0 => keys.push(listed(&pool(n), KeyPurpose::Advertising)),
                    1 => keys.push(listed(&pool(n), KeyPurpose::Statements)),
                    _ => {}
                }
            }
            let made_at = 1_900 + rng.below(500);
            let before: Vec<usize> = providers
                .iter()
                .map(|(n, _)| catalogue.advertisement_history(n).len())
                .collect();
            let bytes = advertisement_bytes(ad, Some(keys), Instant(made_at), &signer);
            let outcome = catalogue.join(&bytes, NOW);
            let after: Vec<usize> = providers
                .iter()
                .map(|(n, _)| catalogue.advertisement_history(n).len())
                .collect();
            if outcome.is_err() {
                refused += 1;
                assert_eq!(
                    before, after,
                    "seed {seed}, step {step}: a refusal changes nothing"
                );
            } else {
                accepted += 1;
            }

            // The rules, over everything accepted so far.
            let mut advertising: Vec<PublicKey> = Vec::new();
            let mut statements: Vec<PublicKey> = Vec::new();
            for (name, _) in &providers {
                let history = catalogue.advertisement_history(name);
                for (i, pair) in history.windows(2).enumerate() {
                    assert!(
                        pair[0].made_at < pair[1].made_at,
                        "seed {seed}, step {step}: accepted advertisements get newer"
                    );
                    for old in &pair[0].statement.keys {
                        let kept = pair[1]
                            .statement
                            .keys
                            .iter()
                            .any(|k| k.public_key == old.public_key);
                        if old.purpose == KeyPurpose::Statements && !kept {
                            // Dropped: never listed again by any later advertisement.
                            for later in &history[i + 2..] {
                                assert!(
                                    later
                                        .statement
                                        .keys
                                        .iter()
                                        .all(|k| k.public_key != old.public_key),
                                    "seed {seed}, step {step}: a dropped statements key stays dropped"
                                );
                            }
                        }
                    }
                }
                for held in &history {
                    for k in &held.statement.keys {
                        match k.purpose {
                            KeyPurpose::Advertising => advertising.push(k.public_key.clone()),
                            KeyPurpose::Statements => statements.push(k.public_key.clone()),
                        }
                    }
                }
            }
            assert!(
                advertising.iter().all(|k| !statements.contains(k)),
                "seed {seed}, step {step}: a key is for one purpose everywhere"
            );
        }
    }
    assert!(accepted > 100, "the property saw {accepted} acceptances");
    assert!(refused > 100, "the property saw {refused} refusals");
}

// verifies: REQ-CAT-2
#[test]
fn advertisement_is_accepted_when_newer_than_the_last_and_within_the_skew_and_not_otherwise() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let skew = [0u64, 60, 300][rng.below(3) as usize];
        let mut catalogue = Catalogue::new();
        catalogue.set_context(deployment(), rutent_contracts::provider_role::VERSION);
        catalogue.set_clock_skew(skew);
        catalogue
            .grant(grant(
                "g",
                "bridge-pay",
                tuples_of(&ramp_advertisement()),
                100_000,
            ))
            .unwrap();
        let key = advertising_key("bridge-pay");
        catalogue
            .trust_advertising_key("g", public_key(&key))
            .unwrap();
        let mut last: Option<u64> = None;
        for step in 0..20 {
            let made_at = 1_500 + rng.below(1_000);
            let bytes = advertisement_bytes(&ramp_advertisement(), None, Instant(made_at), &key);
            let expected = last.is_none_or(|l| made_at > l) && made_at <= NOW.0 + skew;
            let outcome = catalogue.join(&bytes, NOW);
            assert_eq!(
                outcome.is_ok(),
                expected,
                "seed {seed}, step {step}: made at {made_at} after {last:?} with skew {skew}: {outcome:?}"
            );
            if expected {
                last = Some(made_at);
            }
        }
    }
}
