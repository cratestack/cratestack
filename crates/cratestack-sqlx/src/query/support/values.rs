//! Value-shaped helpers: `SqlValue` → bind-slot push, `auth_field`
//! lookup with type narrowing, and slice-of-columns scan. The
//! comparisons the create-policy evaluator makes live in
//! [`super::comparison`].

use cratestack_core::{CratestackContext, Value};

use crate::{Json, SqlColumnValue, SqlValue, sqlx};

use super::decimal_bind::{bind_decimal, bind_null_decimal};

pub(crate) fn push_bind_value(query: &mut sqlx::QueryBuilder<sqlx::Postgres>, value: &SqlValue) {
    // Every arm is a statement (trailing `;`), not a `match`-expression
    // value: `bind_decimal`/`bind_null_decimal` can't return `&mut
    // QueryBuilder` without a lifetime parameter this free function has no
    // clean way to name (two independently-elided input lifetimes, no
    // `&self` to anchor on) — discarding each arm's value sidesteps that
    // entirely, at the cost of losing `push_bind`'s method-chaining value,
    // which nothing here used anyway.
    match value {
        SqlValue::Bool(value) => {
            query.push_bind(*value);
        }
        SqlValue::Int(value) => {
            query.push_bind(*value);
        }
        // `INT8`, the same wire type as `Int` until `Int` narrows to 32 bits.
        SqlValue::BigInt(value) => {
            query.push_bind(*value);
        }
        SqlValue::Float(value) => {
            query.push_bind(*value);
        }
        SqlValue::String(value) => {
            query.push_bind(value.clone());
        }
        SqlValue::Bytes(value) => {
            query.push_bind(value.clone());
        }
        SqlValue::Uuid(value) => {
            query.push_bind(*value);
        }
        SqlValue::DateTime(value) => {
            query.push_bind(*value);
        }
        SqlValue::Json(value) => {
            query.push_bind(Json(value.clone()));
        }
        // `SqlValue::Decimal` holds a `Box<dyn DecimalLike>` (cratestack#505
        // Direction 2), not a fixed concrete type, so this boundary has to
        // downcast to whichever concrete backend(s) this crate's own
        // `decimal-*` features enabled before it can call `push_bind` — sqlx
        // binds a concrete, `Encode`-implementing type, not a trait object.
        // See `bind_decimal` below.
        SqlValue::Decimal(value) => bind_decimal(query, value.as_ref()),
        SqlValue::NullBool => {
            query.push_bind(Option::<bool>::None);
        }
        SqlValue::NullInt => {
            query.push_bind(Option::<i64>::None);
        }
        SqlValue::NullBigInt => {
            query.push_bind(Option::<i64>::None);
        }
        SqlValue::NullFloat => {
            query.push_bind(Option::<f64>::None);
        }
        SqlValue::NullString => {
            query.push_bind(Option::<String>::None);
        }
        SqlValue::NullBytes => {
            query.push_bind(Option::<Vec<u8>>::None);
        }
        SqlValue::NullUuid => {
            query.push_bind(Option::<uuid::Uuid>::None);
        }
        SqlValue::NullDateTime => {
            query.push_bind(Option::<chrono::DateTime<chrono::Utc>>::None);
        }
        SqlValue::NullJson => {
            query.push_bind(Option::<Json<Value>>::None);
        }
        SqlValue::NullDecimal => bind_null_decimal(query),
        #[cfg(feature = "pgvector")]
        SqlValue::Vector(value) => {
            query.push_bind(pgvector::Vector::from(value.clone()));
        }
        #[cfg(feature = "pgvector")]
        SqlValue::NullVector => {
            query.push_bind(Option::<pgvector::Vector>::None);
        }
        // `Vector(n)`/`pgvector::Vector` requires the `pgvector` Cargo
        // feature on this crate. Reaching here without it means an
        // `SqlValue::Vector`/`NullVector` was constructed without
        // going through cratestack-macros' generated code, which
        // itself can't exist unless the matching feature is enabled
        // end-to-end (#161's compile-time gate) — an upstream
        // invariant violation, not a case to handle gracefully.
        #[cfg(not(feature = "pgvector"))]
        SqlValue::Vector(_) | SqlValue::NullVector => unreachable!(
            "SqlValue::Vector/NullVector requires the `pgvector` Cargo feature on \
             cratestack-sqlx"
        ),
        // EWKB bytes bound as `bytea`. PostGIS registers an *implicit*
        // cast from `bytea` to both `geography` and `geometry`, so a
        // bytea-typed parameter binds straight into a spatial column
        // with no `::geography` in the generated SQL — verified against
        // postgis/postgis:16-3.4, where
        // `PREPARE ins(bytea) AS INSERT INTO t(geog_col) VALUES ($1)`
        // prepares cleanly. Decoding is the asymmetric half and goes
        // through `crate::spatial::Ewkb`, because on the way *out* the
        // column's type OID is geography's, not bytea's.
        #[cfg(feature = "postgis")]
        SqlValue::Spatial(value) => {
            query.push_bind(value.clone());
        }
        #[cfg(feature = "postgis")]
        SqlValue::NullSpatial => {
            query.push_bind(Option::<Vec<u8>>::None);
        } // No `#[cfg(not(feature = "postgis"))]` counterpart to the
          // `Vector` arm above: `SqlValue::Spatial`/`NullSpatial` are
          // themselves gated on `postgis` in `cratestack-sql`
          // (cratestack#842), so without the feature the variants don't
          // exist and there is nothing left to match.
    }
}

/// The caller's claim as a bindable value, for the pushed-down
/// `col = $n` / `col != $n` forms. An integer claim binds `INT8`, which
/// compares numerically with a `BigInt` (or `Int`) column. A string claim
/// binds `TEXT` whatever the column is, because the predicate carries no
/// column type: against a `BIGINT` column Postgres refuses it
/// (`operator does not exist: bigint = text`, SQLSTATE 42883), which fails
/// the query rather than matching. So a `BigInt` auth claim has to be a JSON
/// integer in a policy comparison (see [`super::comparison`]).
pub(crate) fn auth_value_to_sql(ctx: &CratestackContext, auth_field: &str) -> Option<SqlValue> {
    match ctx.auth_field(auth_field)? {
        Value::Bool(value) => Some(SqlValue::Bool(*value)),
        Value::Int(value) => Some(SqlValue::Int(*value)),
        Value::String(value) => Some(SqlValue::String(value.clone())),
        _ => None,
    }
}

/// Suffix for the placeholder of a pushed-down `col = $n` / `col != $n`
/// comparison against a caller's claim.
///
/// The claim's runtime type is the one thing in such a statement that varies
/// from request to request while the SQL text stays the same, and sqlx caches
/// a persistent prepared statement by SQL text alone. Without this, an integer
/// claim prepares `amount != $1` with `$1` as `int8`; a later string claim
/// reuses that statement and sends its bytes as binary `int8`. Measured: a
/// 7-byte string fails with SQLSTATE 08P01, and an 8-byte one (`"12345678"`)
/// is read as the integer `0x3132333435363738`, so `!=` admitted every row.
/// Naming the type puts it in the text, so each claim type gets its own
/// statement, and Postgres types the parameter as `text` itself: against a
/// `BIGINT` column that is a deterministic `operator does not exist` (42883).
pub(crate) fn claim_type_suffix(value: &SqlValue) -> &'static str {
    match value {
        SqlValue::String(_) => "::text",
        _ => "",
    }
}

pub(crate) fn find_column_value<'a>(
    values: &'a [SqlColumnValue],
    column: &str,
) -> Option<&'a SqlValue> {
    values
        .iter()
        .find(|value| value.column == column)
        .map(|value| &value.value)
}
