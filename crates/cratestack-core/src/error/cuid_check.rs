//! [`parse_cuid`], the format guard for a `Cuid` scalar value.

use super::CratestackError;

pub fn parse_cuid(value: &str) -> Result<String, CratestackError> {
    if is_valid_cuid(value) {
        Ok(value.to_owned())
    } else {
        Err(CratestackError::BadRequest(format!(
            "invalid cuid '{}': expected a lowercase alphanumeric id (2-32 chars)",
            value,
        )))
    }
}

/// Minimum accepted length for a `Cuid` scalar value.
///
/// cuid v1 ids are at least 2 characters (the `'c'` prefix plus at least one
/// more character); cuid2 ids can be as short as 2 characters too, so this
/// bound covers both formats.
const CUID_MIN_LEN: usize = 2;

/// Maximum accepted length for a `Cuid` scalar value.
///
/// cuid2 defaults to 24 characters but its length is configurable by the
/// generator; 32 gives generous headroom above the default while still
/// rejecting pathological/oversized input.
const CUID_MAX_LEN: usize = 32;

/// Validates that `value` is a plausible cuid, accepting both the legacy
/// cuid v1 shape (`'c'`-prefixed) and the current cuid2 shape (no fixed
/// prefix; the first character is a uniform random lowercase letter).
///
/// This is intentionally a format guard, not a full cuid2
/// checksum/fingerprint verification: lowercase alphanumeric only,
/// non-empty, bounded length.
fn is_valid_cuid(value: &str) -> bool {
    if !(CUID_MIN_LEN..=CUID_MAX_LEN).contains(&value.len()) {
        return false;
    }
    value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
}
