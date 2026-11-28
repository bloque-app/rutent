//! The host's wall time, written as the contracts write a time: for an answer's `made_at` and a record's start; and
//! read against a question's deadline only to drop a question already past it (`T-015-05`). Whether an answer is in
//! time is Core's to say, on Core's clock.

use std::time::{SystemTime, UNIX_EPOCH};

use rutent_contracts::router_api::Timestamp;

/// Whether `deadline` has passed on the host's wall clock. Only to drop a question that is already dead: the host
/// never holds an answer to the deadline, which Core does on its own clock. A host clock that runs ahead of Core's
/// drops questions Core would still have taken, and one that runs behind hands over questions Core will refuse: the
/// operator keeps the host's clock synchronised as Core's.
pub(crate) fn past(deadline: &Timestamp) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i128::try_from(d.as_nanos()).unwrap_or(i128::MAX));
    now > deadline.unix_nanos()
}

/// The time now, as RFC 3339 UTC to the second (`YYYY-MM-DDTHH:MM:SSZ`).
pub(crate) fn now_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(seconds)
}

/// The UTC time `seconds` after the epoch, as RFC 3339 to the second.
pub(crate) fn rfc3339(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let (year, month, day) = civil(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01 (H. Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::rfc3339;

    // verifies: REQ-PRT-1
    #[test]
    fn host_time_is_written_as_the_contracts_write_a_time() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_795_773_601), "2026-11-27T10:00:01Z");
    }
}
