//! `SqlValue` ↔ `rusqlite` binding.
//!
//! SQLite has dynamic typing with five storage classes (NULL, INTEGER, REAL,
//! TEXT, BLOB). The cratestack `SqlValue` is richer than that, so the on-device
//! representation makes deliberate choices:
//!
//! - `Uuid`         → TEXT (canonical hyphenated lowercase form)
//! - `DateTime<Utc>`→ TEXT (RFC 3339, microsecond precision, always UTC)
//! - `Json`         → TEXT (compact serde_json serialization)
//! - `Decimal`      → TEXT (canonical string form — preserves precision)
//! - `Bytes`        → BLOB
//! - `Bool`         → INTEGER 0/1
//! - everything else maps to the obvious SQLite storage class.
//!
//! Decoding mirrors these choices. Round-trip is bit-exact for all variants
//! exercised by tests.

mod bind;
mod columns;
mod decode;
#[cfg(test)]
mod tests;

pub use bind::SqlValueParam;
#[cfg(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
pub use columns::DecimalColumn;
pub use columns::{DateTimeColumn, JsonColumn, UuidColumn};
#[cfg(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
pub use decode::decode_decimal;
pub use decode::{decode_datetime, decode_json, decode_uuid};
