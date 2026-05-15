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

/// Conflict target for an upsert. Defaults to the model's primary key
/// (matching the previous PK-only behavior). `Columns` lets callers
/// upsert on an arbitrary unique tuple — most commonly a natural key
/// that's distinct from the PK (e.g. `(owner_id, provider)` on a
/// per-owner-and-provider settings row, or `(pairing_id, slot)` on a
/// per-slot envelope).
///
/// The named columns MUST correspond to a `UNIQUE` constraint or
/// `UNIQUE` index on the target table — the database engine enforces
/// this and will surface a clear error if not. The upsert builder
/// additionally requires the input to carry a value for every column
/// in the target tuple, so the conflict probe (`SELECT … FOR UPDATE`)
/// has something to filter on.
///
/// Composite-constraint-by-name (`ON CONFLICT ON CONSTRAINT
/// my_unique_idx_v2`) is not yet exposed; pass the matching column
/// tuple via [`Self::Columns`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictTarget {
    /// The model's `@id` primary key. Default.
    PrimaryKey,
    /// A caller-supplied tuple of columns forming a unique key on the
    /// target table.
    Columns(&'static [&'static str]),
}

impl ConflictTarget {
    /// Sugar for `ConflictTarget::Columns(&[...])`.
    pub const fn columns(cols: &'static [&'static str]) -> Self {
        Self::Columns(cols)
    }
}

impl Default for ConflictTarget {
    fn default() -> Self {
        Self::PrimaryKey
    }
}

pub trait CreateModelInput<M> {
    fn sql_values(&self) -> Vec<SqlColumnValue>;
    /// Run schema-derived validators (`@length`, `@email`, `@regex`, ...) on
    /// the input. Default impl is a no-op for inputs without validators.
    fn validate(&self) -> Result<(), cratestack_core::CoolError> {
        Ok(())
    }
}

pub trait UpdateModelInput<M> {
    fn sql_values(&self) -> Vec<SqlColumnValue>;
    fn validate(&self) -> Result<(), cratestack_core::CoolError> {
        Ok(())
    }
}

/// Input shape for the upsert primitive — `INSERT … ON CONFLICT (<pk>) DO
/// UPDATE …`. `sql_values()` must include the primary-key column (so the
/// backend can target the conflict), and `primary_key_value()` exposes the
/// PK separately so the runtime can issue a `SELECT … FOR UPDATE` before
/// the upsert to drive `Created` vs. `Updated` event / audit semantics.
///
/// Only models with a client-supplied primary key (i.e. `@id` *without*
/// `@default(...)`) emit this trait impl; models with server-generated PKs
/// don't get an `.upsert()` builder at all. That's intentional — at v1 the
/// upsert primitive is PK-conflict only, and a server-generated PK can't be
/// upserted without the caller supplying one anyway.
pub trait UpsertModelInput<M>: Send {
    /// Full set of column→value bindings, *including* the primary key.
    fn sql_values(&self) -> Vec<SqlColumnValue>;

    /// The primary-key value, used to issue the `SELECT … FOR UPDATE` probe
    /// inside the upsert transaction. Must match the PK column carried in
    /// `sql_values()`.
    fn primary_key_value(&self) -> SqlValue;

    fn validate(&self) -> Result<(), cratestack_core::CoolError> {
        Ok(())
    }
}

pub trait IntoSqlValue {
    fn into_sql_value(self) -> SqlValue;
}

impl IntoSqlValue for bool {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Bool(self)
    }
}

impl IntoSqlValue for i64 {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Int(self)
    }
}

impl IntoSqlValue for f64 {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Float(self)
    }
}

impl IntoSqlValue for String {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::String(self)
    }
}

impl IntoSqlValue for &str {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::String(self.to_owned())
    }
}

impl IntoSqlValue for uuid::Uuid {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Uuid(self)
    }
}

impl IntoSqlValue for chrono::DateTime<chrono::Utc> {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::DateTime(self)
    }
}

impl IntoSqlValue for Value {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Json(self)
    }
}

impl IntoSqlValue for cratestack_core::Decimal {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Decimal(self)
    }
}

/// Accessor for a model's primary key. Implemented by the macro on every
/// generated model struct so the batch operations can pair returned rows
/// back to the position of their input PK in the request, producing a
/// `BatchItemResult` with the right `index` and a `NotFound` entry for any
/// requested PK that didn't come back.
pub trait ModelPrimaryKey<PK> {
    fn primary_key(&self) -> PK;
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
