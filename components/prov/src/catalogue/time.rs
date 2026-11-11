//! RFC 3339 UTC times, as the contract writes them: `YYYY-MM-DDTHH:MM:SS` with an optional fraction of one to
//! nine digits, and `Z`. The catalogue reads them to order statements and to count a figure's age; it holds no
//! clock of its own.

use super::grant::Instant;

/// A moment to the nanosecond. It orders as time does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp {
    /// Whole seconds since the Unix epoch.
    pub secs: u64,
    /// The fraction of a second, in nanoseconds.
    pub nanos: u32,
}

impl Stamp {
    /// Reads a time in the contract's form, or `None` if it isn't in it (a leap second, an offset, an impossible
    /// date, a year before 1970).
    pub fn parse(text: &str) -> Option<Self> {
        let body = text.strip_suffix('Z')?;
        let (main, fraction) = match body.split_once('.') {
            Some((main, fraction)) => (main, fraction),
            None => (body, ""),
        };
        if body.contains('.') && (fraction.is_empty() || fraction.len() > 9) {
            return None;
        }
        if !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let b = main.as_bytes();
        if b.len() != 19 {
            return None;
        }
        for (i, c) in [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')] {
            if b[i] != c {
                return None;
            }
        }
        let digits = |range: core::ops::Range<usize>| -> Option<u64> {
            let field = &b[range];
            if !field.iter().all(u8::is_ascii_digit) {
                return None;
            }
            Some(field.iter().fold(0, |n, d| n * 10 + u64::from(d - b'0')))
        };
        let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
        let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
        if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let longest = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if !(1..=longest).contains(&day) || year < 1970 {
            return None;
        }
        let days = days_from_civil(year, month, day);
        let nanos = if fraction.is_empty() {
            0
        } else {
            let padded = format!("{fraction:0<9}");
            padded.parse().ok()?
        };
        Some(Self {
            secs: days * 86_400 + hour * 3_600 + minute * 60 + second,
            nanos,
        })
    }

    /// The whole second this moment is in, or the next one if it has a fraction: the figure that makes a check
    /// against a bound fail closed.
    pub fn ceil_secs(self) -> u64 {
        self.secs + u64::from(self.nanos > 0)
    }
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar (year 1970 or later).
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl Instant {
    /// The moment as an RFC 3339 UTC time, or `None` past the year 9999.
    pub fn rfc3339(self) -> Option<String> {
        let days = i64::try_from(self.0 / 86_400).ok()?;
        let secs = self.0 % 86_400;
        // Days since 1970-01-01 to a civil date (proleptic Gregorian).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        if !(0..=9_999).contains(&year) {
            return None;
        }
        Some(format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            secs / 3_600,
            secs % 3_600 / 60,
            secs % 60
        ))
    }
}
