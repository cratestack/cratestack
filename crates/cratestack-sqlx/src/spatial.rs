//! Decode adapter for PostGIS `geography`/`geometry` columns
//! (cratestack#842).
//!
//! Binding is symmetric-free: PostGIS registers an implicit cast from
//! `bytea`, so `push_bind_value` sends a plain `Vec<u8>` and Postgres
//! coerces it (see `query::support::values`). **Decoding is not** —
//! coming out of the database the column's type OID is `geography`'s
//! (or `geometry`'s), and sqlx type-checks the OID against the Rust
//! type before handing over the bytes. A bare `Vec<u8>` therefore
//! fails with "mismatched types; Rust type `Vec<u8>` is not compatible
//! with SQL type `geography`" even though the payload is exactly the
//! bytes we want.
//!
//! [`Ewkb`] exists to satisfy that check: it declares itself compatible
//! with both spatial type names and then hands back the raw bytes
//! unchanged. Same pattern, and same reason, as `pgvector::Vector` —
//! the public generated struct field stays a plain `Vec<u8>` and this
//! type only appears at the row-decode boundary.
//!
//! The payload is EWKB, PostGIS's binary wire format — verified
//! against postgis/postgis:16-3.4, where
//! `ST_AsEWKB(col::geometry) = col::bytea` holds.

use sqlx_core::decode::Decode;
use sqlx_core::error::BoxDynError;
use sqlx_core::type_info::TypeInfo as _;
use sqlx_core::types::Type;
use sqlx_postgres::{PgTypeInfo, PgValueRef, Postgres};

/// Raw EWKB bytes read from a `geography`/`geometry` column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ewkb(pub Vec<u8>);

impl Ewkb {
    /// Consume the wrapper, yielding the EWKB payload — what the
    /// generated model field actually stores.
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl Type<Postgres> for Ewkb {
    fn type_info() -> PgTypeInfo {
        // Resolved by name at runtime: `geography` is an extension type,
        // so it has no stable built-in OID to hardcode.
        PgTypeInfo::with_name("geography")
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        // One adapter for both spatial types. They're distinct Postgres
        // types but share the EWKB representation, and the schema
        // already decided which one the column is — the decoder just
        // needs to accept whichever it meets.
        matches!(ty.name(), "geography" | "geometry")
    }
}

impl<'r> Decode<'r, Postgres> for Ewkb {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        Ok(Ewkb(value.as_bytes()?.to_vec()))
    }
}
