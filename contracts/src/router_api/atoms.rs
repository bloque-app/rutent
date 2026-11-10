//! Checked atoms: the router API's identifiers, versions, amounts and lists, each as strict as the schema atom
//! it mirrors.

use alloc::{borrow::ToOwned, string::String, vec::Vec};
use core::cmp::Ordering;
use core::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use super::Violation;

macro_rules! checked_text {
    ($(#[$doc:meta])* $name:ident, $check:path, $refusal:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The value, or `None` if the text breaks the schema's rule for it.
            pub fn new(text: impl Into<String>) -> Option<Self> {
                let text = text.into();
                $check(&text).then_some($name(text))
            }

            /// The value as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = Violation;

            fn try_from(text: String) -> Result<Self, Self::Error> {
                $name::new(text).ok_or(Violation($refusal))
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

checked_text!(
    /// An identifier: of a request, snapshot, figure, candidate, provider, deployment or void condition.
    /// Schema: `urn:rutent:provider-role:common#/$defs/identifier`.
    Id,
    is_identifier,
    "an identifier is 1 to 128 of A-Z a-z 0-9 . _ : -, starting with a letter or digit"
);

checked_text!(
    /// A version, as its maker names it: of a provider, a policy, a protocol or the router.
    /// Schema: `urn:rutent:provider-role:common#/$defs/version`.
    Version,
    is_version,
    "a version is 1 to 64 of A-Z a-z 0-9 . + _ -, starting with a letter or digit"
);

checked_text!(
    /// A non-negative whole number of a unit, in decimal digits with no leading zeros.
    /// Schema: `urn:rutent:provider-role:common#/$defs/amount`.
    Amount,
    is_amount,
    "an amount is up to 78 decimal digits with no leading zeros"
);

checked_text!(
    /// A positive decimal number, in digits with an optional fraction and no exponent, such as a rate in whole
    /// to-assets per whole from-asset. Schema: `urn:rutent:provider-role:common#/$defs/decimal`, by way of
    /// `urn:rutent:provider-role:result#/$defs/rate`.
    Decimal,
    is_decimal,
    "a decimal is up to 96 characters of digits with an optional fraction, no exponent, above zero"
);

checked_text!(
    /// A UTC time in RFC 3339 form, `YYYY-MM-DDTHH:MM:SS[.fraction]Z`, on a real calendar date and with no leap
    /// second. Schema: `urn:rutent:provider-role:common#/$defs/timestamp`, whose pattern states the same shape.
    Timestamp,
    is_timestamp,
    "a timestamp is an RFC 3339 UTC date-time ending in Z, on a real date, seconds 00 to 59"
);

checked_text!(
    /// A protocol version, as `major.minor.patch` (`REQ-PRT-1`). Schema:
    /// `urn:rutent:provider-role:common#/$defs/protocol_version`.
    ProtocolVersion,
    is_protocol_version,
    "a protocol version is major.minor.patch, each without leading zeros, up to 32 characters"
);

impl Timestamp {
    /// Nanoseconds since 1970-01-01T00:00:00Z, exactly.
    pub fn unix_nanos(&self) -> i128 {
        unix_nanos(&self.0).expect("checked when built")
    }
}

/// The time from `earlier` to `later`, rounded up to whole seconds: an age, never understated. `None` if `later`
/// is earlier or either isn't a timestamp.
pub(super) fn seconds_up(earlier: &str, later: &str) -> Option<u64> {
    let nanos = unix_nanos(later)?.checked_sub(unix_nanos(earlier)?)?;
    u64::try_from((nanos + 999_999_999).div_euclid(1_000_000_000)).ok()
}

/// The time from `earlier` to `later`, rounded down to whole seconds: a validity, never overstated. `None` if
/// `later` is earlier or either isn't a timestamp.
pub(super) fn seconds_down(earlier: &str, later: &str) -> Option<u64> {
    let nanos = unix_nanos(later)?.checked_sub(unix_nanos(earlier)?)?;
    u64::try_from(nanos.div_euclid(1_000_000_000)).ok()
}

fn is_timestamp(text: &str) -> bool {
    unix_nanos(text).is_some()
}

fn is_protocol_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    text.len() <= 32
        && parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

/// Parses the provider-role timestamp shape and checks the calendar date, returning nanoseconds since the Unix
/// epoch.
fn unix_nanos(text: &str) -> Option<i128> {
    let b = text.as_bytes();
    let digits = |range: core::ops::Range<usize>| -> Option<i64> {
        let part = b.get(range)?;
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| part.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
    };
    let shape = b.len() >= 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':';
    if !shape {
        return None;
    }
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    let nanos: i128 = match &b[19..] {
        b"Z" => 0,
        [b'.', fraction @ .., b'Z']
            if (1..=9).contains(&fraction.len()) && fraction.iter().all(u8::is_ascii_digit) =>
        {
            let value = fraction
                .iter()
                .fold(0i128, |n, d| n * 10 + i128::from(d - b'0'));
            value * 10i128.pow(9 - fraction.len() as u32)
        }
        _ => return None,
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > days_in_month || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm), March-based years.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    Some(i128::from(seconds) * 1_000_000_000 + nanos)
}

fn is_decimal(text: &str) -> bool {
    let (whole, fraction) = match text.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (text, None),
    };
    let w = whole.as_bytes();
    let whole_ok =
        !w.is_empty() && w.iter().all(u8::is_ascii_digit) && (w.len() == 1 || w[0] != b'0');
    let fraction_ok = fraction.is_none_or(|f| {
        !f.is_empty() && f.bytes().all(|c| c.is_ascii_digit()) && !f.ends_with('0')
    });
    text.len() <= 96 && whole_ok && fraction_ok && text != "0"
}

fn is_identifier(text: &str) -> bool {
    let b = text.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'))
}

fn is_version(text: &str) -> bool {
    let b = text.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'_' | b'-'))
}

fn is_amount(text: &str) -> bool {
    let b = text.as_bytes();
    !b.is_empty()
        && b.len() <= 78
        && b.iter().all(u8::is_ascii_digit)
        && (b.len() == 1 || b[0] != b'0')
}

impl Amount {
    /// Zero.
    pub fn zero() -> Self {
        Amount("0".to_owned())
    }

    /// Whether the amount is zero.
    pub fn is_zero(&self) -> bool {
        self.0 == "0"
    }

    /// The sum, or `None` if it has more than 78 digits.
    pub fn checked_add(&self, other: &Amount) -> Option<Amount> {
        let (a, b) = (self.0.as_bytes(), other.0.as_bytes());
        let mut digits = Vec::with_capacity(a.len().max(b.len()) + 1);
        let mut carry = 0;
        for i in 0..a.len().max(b.len()) {
            let digit = |s: &[u8]| s.len().checked_sub(i + 1).map_or(0, |j| s[j] - b'0');
            let sum = digit(a) + digit(b) + carry;
            digits.push(b'0' + sum % 10);
            carry = sum / 10;
        }
        if carry > 0 {
            digits.push(b'0' + carry);
        }
        digits.reverse();
        Amount::new(String::from_utf8(digits).expect("ASCII digits"))
    }
}

impl PartialOrd for Amount {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Amount {
    /// By value: with no leading zeros, a longer amount is larger, and amounts of one length compare as text.
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.cmp(&other.0))
    }
}

/// An amount greater than zero. Schema: `urn:rutent:provider-role:common#/$defs/positive_amount`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PositiveAmount(Amount);

impl PositiveAmount {
    /// The value, or `None` if the text isn't an amount above zero.
    pub fn new(text: impl Into<String>) -> Option<Self> {
        Amount::new(text)
            .filter(|a| !a.is_zero())
            .map(PositiveAmount)
    }

    /// The value as an amount.
    pub fn amount(&self) -> &Amount {
        &self.0
    }
}

impl TryFrom<String> for PositiveAmount {
    type Error = Violation;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        PositiveAmount::new(text).ok_or(Violation(
            "a positive amount is up to 78 decimal digits with no leading zeros, above zero",
        ))
    }
}

impl From<PositiveAmount> for String {
    fn from(value: PositiveAmount) -> Self {
        value.0.0
    }
}

/// A list with at least one item, as the schemas' `minItems: 1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<T>", into = "Vec<T>")]
#[serde(bound(
    serialize = "T: Serialize + Clone",
    deserialize = "T: Deserialize<'de>"
))]
pub struct NonEmpty<T>(Vec<T>);

impl<T> NonEmpty<T> {
    /// The list, or `None` if it is empty.
    pub fn new(items: Vec<T>) -> Option<Self> {
        (!items.is_empty()).then_some(NonEmpty(items))
    }

    /// The items, in order.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// The first item.
    pub fn first(&self) -> &T {
        &self.0[0]
    }

    /// The last item.
    pub fn last(&self) -> &T {
        &self.0[self.0.len() - 1]
    }
}

impl<T> core::ops::Deref for NonEmpty<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<T> TryFrom<Vec<T>> for NonEmpty<T> {
    type Error = Violation;

    fn try_from(items: Vec<T>) -> Result<Self, Self::Error> {
        NonEmpty::new(items).ok_or(Violation("the list must not be empty"))
    }
}

impl<T> From<NonEmpty<T>> for Vec<T> {
    fn from(value: NonEmpty<T>) -> Self {
        value.0
    }
}

/// A list whose items are all different, as the schemas' `uniqueItems: true`. It may be empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<T>", into = "Vec<T>")]
#[serde(bound(
    serialize = "T: Serialize + Clone",
    deserialize = "T: Deserialize<'de> + PartialEq"
))]
pub struct Distinct<T>(Vec<T>);

impl<T: PartialEq> Distinct<T> {
    /// The list, or `None` if an item repeats.
    pub fn new(items: Vec<T>) -> Option<Self> {
        let repeats = items
            .iter()
            .enumerate()
            .any(|(i, item)| items[..i].contains(item));
        (!repeats).then_some(Distinct(items))
    }
}

impl<T> core::ops::Deref for Distinct<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<T: PartialEq> TryFrom<Vec<T>> for Distinct<T> {
    type Error = Violation;

    fn try_from(items: Vec<T>) -> Result<Self, Self::Error> {
        Distinct::new(items).ok_or(Violation("the list must not repeat an item"))
    }
}

impl<T> From<Distinct<T>> for Vec<T> {
    fn from(value: Distinct<T>) -> Self {
        value.0
    }
}

/// Deserialises an optional field that, when present, holds a value: `null` is refused, as the schemas refuse it.
/// An absent field stays `None` through `#[serde(default)]`.
pub(super) fn some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Deserialises a present list that the schemas require non-empty, refusing `null` and `[]`: the schemas leave an
/// empty list out, so an explicit empty one is not one of its forms. Used with `#[serde(default)]`, so an absent
/// field is the empty list.
pub(super) fn non_empty_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let items = Vec::<T>::deserialize(deserializer)?;
    if items.is_empty() {
        return Err(serde::de::Error::custom(
            "a list present on the wire is not empty",
        ));
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::{
        Amount, Decimal, Distinct, Id, NonEmpty, PositiveAmount, ProtocolVersion, Timestamp,
        Version,
    };
    use alloc::vec;

    // verifies: DEC-4
    #[test]
    fn atoms_refuse_what_the_schema_atoms_refuse() {
        assert!(Id::new("ledger-in").is_some());
        assert!(Id::new("").is_none());
        assert!(Id::new("-lead").is_none());
        assert!(Id::new("a b").is_none());
        assert!(Id::new("a".repeat(129)).is_none());
        assert!(Version::new("1.0.0+build").is_some());
        assert!(Version::new("1 0").is_none());
        assert!(Amount::new("0").is_some());
        assert!(Amount::new("007").is_none());
        assert!(Amount::new("1.5").is_none());
        assert!(Amount::new("1".repeat(79)).is_none());
        assert!(PositiveAmount::new("0").is_none());
        assert!(NonEmpty::<u8>::new(vec![]).is_none());
        assert!(Distinct::new(vec![1, 2, 1]).is_none());
        assert!(Decimal::new("0.00025").is_some());
        assert!(Decimal::new("0").is_none());
        assert!(Decimal::new("1.50").is_none());
        assert!(Decimal::new("1e3").is_none());
        assert!(Timestamp::new("2026-11-06T15:00:00.25Z").is_some());
        assert!(Timestamp::new("2026-11-06T15:00:60Z").is_none());
        assert!(Timestamp::new("2026-02-30T15:00:00Z").is_none());
        assert!(Timestamp::new("2026-11-06T15:00:00+01:00").is_none());
        assert!(ProtocolVersion::new("0.1.0").is_some());
        assert!(ProtocolVersion::new("0.01.0").is_none());
    }

    // verifies: REQ-QTE-4
    #[test]
    fn ages_round_up_and_validities_round_down() {
        let t = |s: &str| Timestamp::new(s).unwrap().unix_nanos();
        assert_eq!(t("1970-01-01T00:00:00Z"), 0);
        assert_eq!(t("2000-03-01T00:00:00.5Z"), 951_868_800_500_000_000);
        let (made, taken) = ("2026-11-06T14:59:55.25Z", "2026-11-06T15:00:00Z");
        assert_eq!(super::seconds_up(made, taken), Some(5));
        assert_eq!(super::seconds_down(made, taken), Some(4));
        assert_eq!(super::seconds_up(made, made), Some(0));
        assert_eq!(super::seconds_up(taken, made), None);
    }

    // verifies: REQ-ORD-42
    #[test]
    fn amounts_add_and_compare_by_value() {
        let a = |s: &str| Amount::new(s).unwrap();
        assert_eq!(a("999").checked_add(&a("1")), Some(a("1000")));
        assert_eq!(a("0").checked_add(&a("0")), Some(a("0")));
        assert_eq!(a(&"9".repeat(78)).checked_add(&a("1")), None);
        assert!(a("9") < a("10"));
        assert!(a("600") > a("500"));
    }
}
