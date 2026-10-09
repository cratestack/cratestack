//! `Serialize` and `Deserialize` for [`BigInt`]: a canonical decimal string on
//! every format, and only that form back.
//!
//! Two things refuse every other item (a JSON number, a CBOR integer of major
//! type 0 or 1, a CBOR bignum behind tag 2 or 3, a byte string, a bool, null).
//! `Deserialize` asks for a string with `deserialize_str`, and `serde_json` and
//! `minicbor-serde` both refuse a non-string item at that call without
//! consulting the visitor; and the visitor implements `visit_str` only, so a
//! buffered path that does consult it (`#[serde(flatten)]`, `untagged`) still
//! lands on serde's default `invalid_type`. Switching to `deserialize_any`
//! would be the regression, and the codec tests pin it. There is no
//! `is_human_readable()` branch: a value that crosses `serde_json::Value` on
//! its way to CBOR keeps the one form it started with.

use core::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::BigInt;
use super::canonical::parse_canonical;

/// How much of a refused string an error message may repeat. The message is
/// operator-only, but an unbounded echo of a hostile body is still a log line
/// the sender chooses the size of.
const EXCERPT_CHARS: usize = 32;

impl Serialize for BigInt {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for BigInt {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(BigIntVisitor)
    }
}

struct BigIntVisitor;

impl Visitor<'_> for BigIntVisitor {
    type Value = BigInt;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a BigInt as a canonical decimal string, e.g. \"9007199254740993\"")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<BigInt, E> {
        parse_canonical(text)
            .map(BigInt)
            .map_err(|error| E::custom(format_args!("`{}`: {error}", Excerpt(text))))
    }
}

/// `text`, cut to [`EXCERPT_CHARS`] characters with an ellipsis when longer.
struct Excerpt<'a>(&'a str);

impl fmt::Display for Excerpt<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.char_indices().nth(EXCERPT_CHARS) {
            Some((end, _)) => write!(formatter, "{}...", &self.0[..end]),
            None => formatter.write_str(self.0),
        }
    }
}
