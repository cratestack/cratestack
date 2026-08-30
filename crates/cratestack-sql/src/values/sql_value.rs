use cratestack_core::Value;

use super::decimal_like::DecimalLike;

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Uuid(uuid::Uuid),
    DateTime(chrono::DateTime<chrono::Utc>),
    Json(Value),
    /// Holds whichever concrete decimal type the originating schema chose
    /// (cratestack#505 Direction 2 — `rust_decimal::Decimal`,
    /// `bigdecimal::BigDecimal`, or any other [`DecimalLike`]
    /// implementer), boxed rather than a fixed concrete type so two
    /// schemas that chose different backends can share this one compiled
    /// `SqlValue` without a Cargo-feature union collision. Unconditional —
    /// no `#[cfg]` gate, since this variant no longer names a concrete
    /// backend type at all.
    Decimal(Box<dyn DecimalLike>),
    /// A `Vector(n)` field's value (see `docs/design/extensions.md`
    /// §6). Defined unconditionally — no `pgvector` dependency is
    /// needed to hold a `Vec<f32>` — but only ever constructed by
    /// generated code gated on the `pgvector` Cargo feature (#161's
    /// compile-time check), and only ever bound to a real column by
    /// `cratestack-sqlx`'s own `pgvector`-gated encode path.
    Vector(Vec<f32>),
    /// A `Geography`/`Geometry` field's value as EWKB bytes (see
    /// `docs/design/extensions.md` §6b and cratestack#842). Defined
    /// unconditionally — no PostGIS dependency is needed to hold a
    /// `Vec<u8>` — but only ever constructed by generated code gated on
    /// the `postgis` Cargo feature, and bound to a real column by
    /// `cratestack-sqlx`'s own `postgis`-gated encode path.
    ///
    /// A distinct variant rather than reusing [`SqlValue::Bytes`] so
    /// the encode boundary can tell "these bytes are a geometry" from
    /// "these bytes are a bytea column", which matters for the
    /// `NULL` arm's type annotation.
    #[cfg(feature = "postgis")]
    Spatial(Vec<u8>),
    NullBool,
    NullInt,
    NullFloat,
    NullString,
    NullBytes,
    NullUuid,
    NullDateTime,
    NullJson,
    NullDecimal,
    NullVector,
    #[cfg(feature = "postgis")]
    NullSpatial,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    None,
    Single(SqlValue),
    Many(Vec<SqlValue>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlColumnValue {
    pub column: &'static str,
    pub value: SqlValue,
}

/// Detect the first duplicate value in a list of `SqlValue`s, used for
/// batch_upsert input deduplication. Linear-scan with `PartialEq` rather
/// than the hashed variant in `cratestack-core` because `SqlValue::Float`
/// and `SqlValue::Decimal` don't admit a sound `Hash` impl.
///
/// At the documented batch cap (≤ 1000 items) the O(N²) cost is on the
/// order of a million `PartialEq` comparisons, which dominates nothing
/// next to a single round-trip to Postgres. Returns `(first_index,
/// duplicate_index)` on collision, matching `cratestack_core::find_duplicate_position`.
pub fn find_duplicate_sql_value(values: &[SqlValue]) -> Option<(usize, usize)> {
    for (index, value) in values.iter().enumerate() {
        if let Some(earlier) = values[..index].iter().position(|prior| prior == value) {
            return Some((earlier, index));
        }
    }
    None
}
