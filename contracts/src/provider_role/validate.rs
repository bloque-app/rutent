//! The rules of an advertisement that JSON Schema can't express: they compare two fields, or two entries of a
//! list. An advertisement is valid when it passes the schemas and [`validate`] returns `Ok`.

use std::cmp::Ordering;

use super::envelope::{Signature, Statement};
use super::types::{Advertisement, Band, Category, KeyPurpose, PublicKey, Terms};

/// A rule an advertisement breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleViolation {
    /// A ramp tuple whose origin and destination are one node (`REQ-CAT-13`).
    RampWithinOneNode {
        /// Index into `offers.ramp.tuples`.
        tuple: usize,
    },
    /// An amount band whose minimum is above its maximum (`REQ-CAT-11`).
    BandMinAboveMax {
        /// The offer the tuple is in.
        category: Category,
        /// Index into that offer's `tuples`.
        tuple: usize,
        /// Index into the tuple's `terms.bands`.
        band: usize,
    },
    /// An advertisement statement whose maker is not its `provider`.
    AdvertisementOfAnotherMaker,
    /// An advertisement statement signed with a key not in its own `keys`.
    SignedWithUnpublishedKey,
    /// An advertisement statement signed with a key it lists for `statements`: a module's outside part can
    /// resolve such a key, so it never signs an advertisement (`THR-4`).
    SignedWithStatementsKey,
    /// A key listed twice in `keys`, by its decoded bytes, whatever the purposes, so one key can't hold both
    /// (`THR-4`).
    KeyListedTwice {
        /// Index of the repeat in `keys`.
        key: usize,
        /// Index of the first entry with the same key.
        first: usize,
    },
    /// A later advertisement of another provider than the one it follows.
    SuccessorOfAnotherProvider,
    /// A later advertisement of another deployment than the one it follows (`REQ-PRT-1`).
    SuccessorOfAnotherDeployment,
    /// A later advertisement made at or before the one it follows. An older advertisement is public and still
    /// validly signed, so replayed after a newer one it would undo a key's removal, bringing back a `statements`
    /// key the provider dropped (`THR-4`).
    SuccessorNotNewer,
    /// A later advertisement not signed with a key the advertisement it follows lists for `advertising`. Signed
    /// with a key it adds itself, or with a `statements` key, it could add or promote an advertising key and
    /// rewrite the offers (`THR-4`, `REQ-CAT-2`).
    SuccessorNotSignedWithPreviousAdvertisingKey,
    /// A later advertisement listing for `advertising` a key the advertisement it follows lists for `statements`,
    /// which a module's outside part may have resolved (`THR-4`).
    StatementsKeyPromoted {
        /// Index into the later advertisement's `keys`.
        key: usize,
    },
    /// A reserve declaration whose `maker` is not the maker of the advertisement statement it sits in
    /// (`REQ-CAT-6`).
    ReserveDeclarationOfAnotherMaker,
    /// A tuple listed twice in one offer: two ramp or reserve tuples with one origin, destination and asset,
    /// or two exchange tuples with one node, from asset and to asset. Their terms could conflict
    /// (`REQ-CAT-11`).
    TupleDeclaredTwice {
        /// The offer the tuples are in.
        category: Category,
        /// Index of the repeat in that offer's `tuples`.
        tuple: usize,
        /// Index of the first tuple with the same key.
        first: usize,
    },
}

/// Checks an advertisement statement: every rule of [`validate`]; that the provider made it, signing with a
/// key it publishes in `keys` for `advertising`; that no key is listed twice, by its bytes; and that a reserve's
/// declaration is attributable to the statement's maker (`REQ-CAT-6`).
///
/// An advertisement that lists the key it is signed with vouches only for itself. Its keys are trusted by the
/// operator's grant at admission (`REQ-CAT-2`), and a later advertisement by [`validate_successor`]: the
/// catalogue (`F-012`) holds both, as `contracts/provider-role/README.md` says.
pub fn validate_statement(statement: &Statement<Advertisement>) -> Result<(), Vec<RuleViolation>> {
    let mut violations = validate(&statement.statement).err().unwrap_or_default();
    let advertisement = &statement.statement;
    if statement.maker != advertisement.provider {
        violations.push(RuleViolation::AdvertisementOfAnotherMaker);
    }
    match advertisement.purpose_of_signer(&statement.signature) {
        None => violations.push(RuleViolation::SignedWithUnpublishedKey),
        Some(KeyPurpose::Statements) => violations.push(RuleViolation::SignedWithStatementsKey),
        Some(KeyPurpose::Advertising) => {}
    }
    for (key, entry) in advertisement.keys.iter().enumerate() {
        if let Some(first) = advertisement.keys[..key]
            .iter()
            .position(|earlier| earlier.public_key == entry.public_key)
        {
            violations.push(RuleViolation::KeyListedTwice { key, first });
        }
    }
    if let Some(reserve) = &statement.statement.offers.reserve
        && reserve.declaration.maker != statement.maker
    {
        violations.push(RuleViolation::ReserveDeclarationOfAnotherMaker);
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Checks a later advertisement against the accepted one it follows, and returns every rule broken (`THR-4`,
/// `REQ-CAT-2`):
///
/// - both are one provider's, of one deployment, by name and operator key;
/// - `next` was made after `previous`, so a replayed older advertisement, public and validly signed, can't undo a
///   key's removal;
/// - `next` is signed with a key `previous` lists for `advertising`, so neither a key `next` adds nor a
///   `statements` key, which a module's outside part can resolve, can add or promote an advertising key or
///   rewrite the offers;
/// - no key `previous` lists for `statements` is listed for `advertising` in `next`.
///
/// It assumes both passed [`validate_statement`], and compares keys only; verifying the signature is `M2` and
/// `M3` work. It relates two advertisements: the catalogue (`F-012`) checks each against the one it accepted
/// last, and holds the rules across the whole history.
pub fn validate_successor(
    previous: &Statement<Advertisement>,
    next: &Statement<Advertisement>,
) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    if next.statement.provider != previous.statement.provider {
        violations.push(RuleViolation::SuccessorOfAnotherProvider);
    }
    if next.deployment != previous.deployment {
        violations.push(RuleViolation::SuccessorOfAnotherDeployment);
    }
    if !made_after(&next.made_at, &previous.made_at) {
        violations.push(RuleViolation::SuccessorNotNewer);
    }
    if previous.statement.purpose_of_signer(&next.signature) != Some(KeyPurpose::Advertising) {
        violations.push(RuleViolation::SuccessorNotSignedWithPreviousAdvertisingKey);
    }
    for (key, entry) in next.statement.keys.iter().enumerate() {
        if entry.purpose == KeyPurpose::Advertising
            && previous.statement.purpose_of(&entry.public_key) == Some(KeyPurpose::Statements)
        {
            violations.push(RuleViolation::StatementsKeyPromoted { key });
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

impl Advertisement {
    /// The purpose the advertisement lists `key` with, compared by bytes; `None` if it doesn't list it. A key
    /// listed twice is [`RuleViolation::KeyListedTwice`]; this gives the first entry's purpose.
    pub fn purpose_of(&self, key: &PublicKey) -> Option<KeyPurpose> {
        self.keys
            .iter()
            .find(|k| &k.public_key == key)
            .map(|k| k.purpose)
    }

    /// The purpose the advertisement lists a signature's key with, matching its scheme as well as its bytes;
    /// `None` if it lists no such key.
    pub fn purpose_of_signer(&self, signature: &Signature) -> Option<KeyPurpose> {
        self.keys
            .iter()
            .find(|k| k.scheme == signature.scheme && k.public_key == signature.key)
            .map(|k| k.purpose)
    }
}

/// Whether the RFC 3339 UTC time `later` is after `earlier`. Both must be in the schema's form: a fixed-width
/// date and time to the second, an optional fraction of 1 to 9 digits, then `Z`, with every field in range. The
/// fraction compares as digits padded with zeros, so `.5` is after `.25` and no text comparison is fooled by the
/// `.` sorting before `Z`. A time that isn't in that form (an offset such as `+05:00`, lowercase `t` or `z`, a
/// leap second, garbage) can't be ordered, so it is never newer and nothing is newer than it: this returns
/// `false`, which fails closed.
fn made_after(later: &str, earlier: &str) -> bool {
    /// The time as (seconds as fixed-width text, fraction padded to nine digits), or `None` if it isn't in the
    /// schema's form.
    fn parts(time: &str) -> Option<(&str, String)> {
        let time = time.strip_suffix('Z')?;
        let (seconds, fraction) = match time.split_once('.') {
            Some((seconds, fraction)) => {
                if fraction.is_empty()
                    || fraction.len() > 9
                    || !fraction.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                (seconds, fraction)
            }
            None => (time, ""),
        };
        let b = seconds.as_bytes();
        if b.len() != 19 {
            return None;
        }
        let digits = |range: std::ops::Range<usize>| -> Option<u32> {
            let field = &b[range];
            if !field.iter().all(u8::is_ascii_digit) {
                return None;
            }
            field
                .iter()
                .try_fold(0u32, |n, d| Some(n * 10 + u32::from(d - b'0')))
        };
        let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
        if separators.iter().any(|&(i, c)| b[i] != c) {
            return None;
        }
        let (month, day) = (digits(5..7)?, digits(8..10)?);
        let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
        digits(0..4)?;
        if !(1..=12).contains(&month)
            || !(1..=31).contains(&day)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return None;
        }
        Some((seconds, format!("{fraction:0<9}")))
    }
    match (parts(later), parts(earlier)) {
        (Some(later), Some(earlier)) => later > earlier,
        _ => false,
    }
}

/// Checks the rules the schemas can't express, and returns every one broken, offer by offer.
///
/// It assumes the advertisement already passed the schemas: amounts are then decimal digit strings with no
/// leading zeros, so they compare by length and then by text.
///
/// - A ramp's origin and destination are different nodes (`REQ-CAT-13`).
/// - Every amount band has its minimum at or below its maximum (`REQ-CAT-11`).
/// - No offer lists one tuple twice (`REQ-CAT-11`).
pub fn validate(advertisement: &Advertisement) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    let offers = &advertisement.offers;

    if let Some(reserve) = &offers.reserve {
        for (i, tuple) in reserve.tuples.iter().enumerate() {
            check_bands(Category::Reserve, i, &tuple.terms, &mut violations);
        }
        check_repeats(
            Category::Reserve,
            reserve
                .tuples
                .iter()
                .map(|t| (&t.origin, &t.destination, &t.asset)),
            &mut violations,
        );
    }
    if let Some(exchange) = &offers.exchange {
        for (i, tuple) in exchange.tuples.iter().enumerate() {
            check_bands(Category::Exchange, i, &tuple.terms, &mut violations);
        }
        check_repeats(
            Category::Exchange,
            exchange
                .tuples
                .iter()
                .map(|t| (&t.node, &t.from.id, &t.to.id)),
            &mut violations,
        );
    }
    if let Some(ramp) = &offers.ramp {
        for (i, tuple) in ramp.tuples.iter().enumerate() {
            if tuple.origin == tuple.destination {
                violations.push(RuleViolation::RampWithinOneNode { tuple: i });
            }
            check_bands(Category::Ramp, i, &tuple.terms, &mut violations);
        }
        check_repeats(
            Category::Ramp,
            ramp.tuples
                .iter()
                .map(|t| (&t.origin, &t.destination, &t.asset)),
            &mut violations,
        );
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

fn check_bands(
    category: Category,
    tuple: usize,
    terms: &Terms,
    violations: &mut Vec<RuleViolation>,
) {
    for (band, Band { min, max }) in terms.bands.iter().enumerate() {
        if compare_amounts(min, max) == Ordering::Greater {
            violations.push(RuleViolation::BandMinAboveMax {
                category,
                tuple,
                band,
            });
        }
    }
}

/// Reports each tuple whose key an earlier tuple of the same offer already has.
fn check_repeats<K: PartialEq>(
    category: Category,
    keys: impl Iterator<Item = K>,
    violations: &mut Vec<RuleViolation>,
) {
    let keys: Vec<K> = keys.collect();
    for (tuple, key) in keys.iter().enumerate() {
        if let Some(first) = keys[..tuple].iter().position(|earlier| earlier == key) {
            violations.push(RuleViolation::TupleDeclaredTwice {
                category,
                tuple,
                first,
            });
        }
    }
}

/// Compares two schema-valid amounts: decimal digits with no leading zeros.
fn compare_amounts(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::{compare_amounts, made_after};
    use std::cmp::Ordering;

    // verifies: THR-4
    #[test]
    fn a_later_time_is_later_whatever_its_fraction() {
        assert!(made_after(
            "2026-11-07T00:00:00Z",
            "2026-11-06T23:59:59.999Z"
        ));
        assert!(made_after(
            "2026-11-06T10:00:00.5Z",
            "2026-11-06T10:00:00.25Z"
        ));
        assert!(made_after("2026-11-06T10:00:00.1Z", "2026-11-06T10:00:00Z"));
        assert!(!made_after(
            "2026-11-06T10:00:00Z",
            "2026-11-06T10:00:00.000Z"
        ));
        assert!(!made_after("2026-11-06T10:00:00Z", "2026-11-06T10:00:00Z"));
        assert!(!made_after("2026-11-05T10:00:00Z", "2026-11-06T10:00:00Z"));
    }

    // verifies: THR-4
    #[test]
    fn a_time_off_the_schemas_form_is_never_newer() {
        // `+05:00` is later as text than `Z`, but it isn't a schema-valid time: not newer, either way round.
        assert!(!made_after(
            "2026-11-06T10:00:00+05:00",
            "2026-11-06T09:00:00Z"
        ));
        assert!(!made_after(
            "2026-11-07T10:00:00Z",
            "2026-11-06T09:00:00+05:00"
        ));
        for bad in [
            "",
            "garbage",
            "2026-11-06",
            "2026-11-06T10:00:00",
            "2026-11-06t10:00:00z",
            "2026-11-06T10:00:00.Z",
            "2026-11-06T10:00:00.1234567890Z",
            "2026-13-06T10:00:00Z",
            "2026-11-06T24:00:00Z",
            "2026-11-06T10:00:60Z",
            "+026-11-06T10:00:00Z",
        ] {
            assert!(!made_after(bad, "2026-11-06T10:00:00Z"), "{bad}");
            assert!(!made_after("2030-01-01T00:00:00Z", bad), "{bad}");
        }
    }

    // verifies: THR-4
    #[test]
    fn a_valid_newer_time_is_newer() {
        assert!(made_after(
            "2026-11-06T10:00:01Z",
            "2026-11-06T10:00:00.999999999Z"
        ));
        assert!(made_after(
            "2027-01-01T00:00:00.000000001Z",
            "2026-12-31T23:59:59Z"
        ));
    }

    // verifies: REQ-CAT-11
    #[test]
    fn amounts_compare_by_value_not_by_text() {
        assert_eq!(compare_amounts("9", "10"), Ordering::Less);
        assert_eq!(compare_amounts("100", "100"), Ordering::Equal);
        assert_eq!(compare_amounts("600", "500"), Ordering::Greater);
        assert_eq!(compare_amounts("0", "1"), Ordering::Less);
    }
}
