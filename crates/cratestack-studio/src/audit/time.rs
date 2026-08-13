//! Minimal UTC RFC-3339 encoder.
//!
//! Split out of `audit.rs` so the ring buffer and its on-disk sink stay
//! the only things in that file. Kept hand-rolled rather than pulling
//! `chrono` for a single timestamp format.

use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    rfc3339_from_unix(secs)
}

/// Handles dates from 1970 onward, which covers anything
/// `SystemTime::now()` produces on a running machine.
pub(super) fn rfc3339_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let seconds_of_day = secs.rem_euclid(86_400) as u32;
    let (hour, rest) = (seconds_of_day / 3600, seconds_of_day % 3600);
    let (minute, second) = (rest / 60, rest % 60);

    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days` algorithm. Converts days from
/// 1970-01-01 to a (year, month, day) gregorian tuple.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_encodes_epoch() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn rfc3339_encodes_known_date() {
        // 2024-01-15T12:34:56Z = 1705322096
        assert_eq!(rfc3339_from_unix(1_705_322_096), "2024-01-15T12:34:56Z");
    }
}
