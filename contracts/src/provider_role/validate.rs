//! The rules of an advertisement that JSON Schema can't express: they compare two fields, or two entries of a
//! list. An advertisement is valid when it passes the schemas and [`validate`] returns `Ok`.

use std::cmp::Ordering;

use super::types::{Advertisement, Band, Category, Terms};

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
    use super::compare_amounts;
    use std::cmp::Ordering;

    // verifies: REQ-CAT-11
    #[test]
    fn amounts_compare_by_value_not_by_text() {
        assert_eq!(compare_amounts("9", "10"), Ordering::Less);
        assert_eq!(compare_amounts("100", "100"), Ordering::Equal);
        assert_eq!(compare_amounts("600", "500"), Ordering::Greater);
        assert_eq!(compare_amounts("0", "1"), Ordering::Less);
    }
}
