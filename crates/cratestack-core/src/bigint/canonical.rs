//! The canonical decimal grammar shared by `FromStr` and `Deserialize`.
//!
//! `0`, or an optional `-`, a non-zero digit and at most 18 more digits,
//! inside `i64`. Everything `str::parse::<i64>` is lenient about is refused
//! here: a leading `+`, leading zeros, `-0`, whitespace, an empty string.

use core::fmt;
use core::num::IntErrorKind;

/// Why a string is not a canonical `BigInt`.
///
/// The text deliberately omits the input: a caller that wants it to appear in
/// a message adds a bounded excerpt itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseBigIntError {
    kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Not `0` or `-?[1-9][0-9]*`.
    NotCanonical,
    /// Canonical, but outside the `i64` range.
    OutOfRange,
}

impl fmt::Display for ParseBigIntError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            Kind::NotCanonical => formatter.write_str(
                "not a canonical decimal integer (expected `0`, or an optional `-`, \
                 a non-zero digit and up to 18 more digits)",
            ),
            Kind::OutOfRange => formatter.write_str(
                "outside the signed 64-bit range \
                 (-9223372036854775808 to 9223372036854775807)",
            ),
        }
    }
}

impl std::error::Error for ParseBigIntError {}

const NOT_CANONICAL: ParseBigIntError = ParseBigIntError {
    kind: Kind::NotCanonical,
};
const OUT_OF_RANGE: ParseBigIntError = ParseBigIntError {
    kind: Kind::OutOfRange,
};

pub(super) fn parse_canonical(text: &str) -> Result<i64, ParseBigIntError> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let negative = digits.len() != text.len();

    match digits.as_bytes() {
        [] => Err(NOT_CANONICAL),
        // `0` is the only form that starts with a zero, and it has no sign.
        [b'0'] if !negative => Ok(0),
        [b'0', ..] => Err(NOT_CANONICAL),
        bytes if bytes.iter().all(u8::is_ascii_digit) => {
            // Grammar already holds, so `parse` can only fail on range.
            text.parse::<i64>().map_err(|error| match error.kind() {
                IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => OUT_OF_RANGE,
                _ => NOT_CANONICAL,
            })
        }
        _ => Err(NOT_CANONICAL),
    }
}
