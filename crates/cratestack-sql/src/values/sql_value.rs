use cratestack_core::Value;

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
    Decimal(cratestack_core::Decimal),
    NullBool,
    NullInt,
    NullFloat,
    NullString,
    NullBytes,
    NullUuid,
    NullDateTime,
    NullJson,
    NullDecimal,
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
