//! The lock's dates: `YYYY-MM-DD`, a real calendar date. `prune --before`
//! compares them, and pruning the wrong generation cannot be undone for the
//! installed clients of that build, so a date that is not one is refused
//! where it enters (`lock --date`, a lock file's `locked_at`) and where it is
//! compared (`prune --before`), never compared as text.

use super::lock::LockError;

/// `(year, month, day)`, ordered as a date is.
pub(super) type Date = (u16, u8, u8);

/// A real calendar date in exactly `YYYY-MM-DD` form.
pub(super) fn parse_date(text: &str) -> Result<Date, LockError> {
    let bad = || LockError::Date(text.to_owned());
    let bytes = text.as_bytes();
    let shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit());
    if !shape {
        return Err(bad());
    }
    let (year, month, day) = (
        text[..4].parse::<u16>().map_err(|_| bad())?,
        text[5..7].parse::<u8>().map_err(|_| bad())?,
        text[8..].parse::<u8>().map_err(|_| bad())?,
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(bad()),
    };
    if day == 0 || day > last {
        return Err(bad());
    }
    Ok((year, month, day))
}
